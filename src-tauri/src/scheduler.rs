use crate::api::sync::{sync_collections, sync_users};
use crate::api::topics::sync_topics;
use crate::settings::load_settings;
use crate::wallpaper::{change_wallpaper, prefetch_upcoming_wallpapers};
use chrono::Utc;
use cron::Schedule;
use rand::Rng;
use std::str::FromStr;
use std::sync::OnceLock;
use tauri::Emitter;
use tokio::sync::watch;
use tokio::time::{sleep, Duration, Instant};

static CRON_TX: OnceLock<watch::Sender<String>> = OnceLock::new();
static REPREFETCH_TX: OnceLock<watch::Sender<u64>> = OnceLock::new();

/// Call this whenever the cron expression changes in settings.
/// The wallpaper scheduler will pick it up immediately and reschedule.
pub fn notify_cron_changed(new_cron: String) {
    if let Some(tx) = CRON_TX.get() {
        let _ = tx.send(new_cron);
    }
}

/// Call this when a getter source is disabled and the prefetched next wallpaper came from it.
/// Triggers an immediate re-prefetch so the next wallpaper is always from an active source.
pub fn notify_reprefetch() {
    if let Some(tx) = REPREFETCH_TX.get() {
        let next = tx.borrow().wrapping_add(1);
        let _ = tx.send(next);
    }
}

/// Parse a standard 5-field cron expression ("min hour dom month dow").
/// The `cron` crate expects 6 fields, so we prepend a "0" seconds field.
fn parse_cron(expr: &str) -> Result<Schedule, String> {
    let six_field = format!("0 {}", expr.trim());
    Schedule::from_str(&six_field)
        .map_err(|e| format!("Invalid cron expression \"{}\": {}", expr, e))
}

/// Run users, collections, and topics sync once at startup — no periodic scheduling.
pub fn run_startup_sync() {
    tauri::async_runtime::spawn(async {
        log::info!("[scheduler] Running startup sync");
        match sync_users().await {
            Ok(()) => log::info!("[scheduler] Users synced"),
            Err(e) => log::error!("[scheduler] Users sync failed: {}", e),
        }
        match sync_collections().await {
            Ok(()) => log::info!("[scheduler] Collections synced"),
            Err(e) => log::error!("[scheduler] Collections sync failed: {}", e),
        }
        match sync_topics().await {
            Ok(topics) => log::info!("[scheduler] Topics synced: {} topics", topics.len()),
            Err(e) => log::error!("[scheduler] Topics sync failed: {}", e),
        }
    });
}

/// Spawn the wallpaper scheduler.
/// Uses a watch channel so changing the cron expression in settings
/// immediately cancels the current sleep and reschedules.
pub fn start_wallpaper_scheduler(app_handle: tauri::AppHandle) {
    let initial_cron = load_settings()
        .map(|s| s.wallpaper_cron)
        .unwrap_or_else(|_| "0 * * * *".to_string());

    let (tx, mut rx) = watch::channel(initial_cron);
    CRON_TX.set(tx).ok();

    // Spawn a separate task that listens for re-prefetch requests (source disabled mid-cycle)
    let reprefetch_app = app_handle.clone();
    let (reprefetch_tx, mut reprefetch_rx) = watch::channel(0u64);
    REPREFETCH_TX.set(reprefetch_tx).ok();
    tauri::async_runtime::spawn(async move {
        loop {
            reprefetch_rx.changed().await.ok();
            log::info!("[scheduler] Re-prefetching next wallpaper (source disabled)");
            let app = reprefetch_app.clone();
            tauri::async_runtime::spawn(async move {
                match prefetch_upcoming_wallpapers().await {
                    Ok(()) => {
                        log::info!("[scheduler] Re-prefetch complete");
                        let _ = app.emit("next-wallpaper-ready", ());
                    }
                    Err(e) => log::warn!("[scheduler] Re-prefetch failed: {}", e),
                }
            });
        }
    });

    tauri::async_runtime::spawn(async move {
        log::info!("[scheduler] Starting wallpaper scheduler");

        // Run once immediately on launch. On a network failure keep
        // retrying every 5 minutes until the next scheduled time (or
        // until success / a settings change in startup-only mode).
        if let Err(e) = run_wallpaper_cycle(&app_handle).await {
            if is_network_error(&e) {
                let deadline = delay_until_next_fire(&rx.borrow().clone())
                    .map(|d| Instant::now() + d);
                retry_after_network_failure(&app_handle, &mut rx, deadline).await;
            }
        }

        loop {
            let cron_expr = rx.borrow().clone();

            // Startup-only mode: wallpaper was already changed above; just wait for settings to change.
            if cron_expr == "@startup" || cron_expr.is_empty() {
                log::info!("[scheduler] Startup-only mode — no recurring schedule");
                rx.changed().await.ok();
                continue;
            }

            let schedule = match parse_cron(&cron_expr) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("[scheduler] {e} — waiting for a valid expression");
                    rx.changed().await.ok();
                    continue;
                }
            };

            let next = match schedule.upcoming(Utc).next() {
                Some(t) => t,
                None => {
                    log::error!("[scheduler] Cron expression has no upcoming occurrences");
                    rx.changed().await.ok();
                    continue;
                }
            };

            let base_delay = (next - Utc::now())
                .to_std()
                .unwrap_or(std::time::Duration::ZERO);

            // Add ±120s jitter to spread API load across the user base.
            let jitter_secs: i64 = rand::rng().random_range(-120..=120);
            let jittered_secs = (base_delay.as_secs() as i64 + jitter_secs).max(0) as u64;
            let delay = std::time::Duration::from_secs(jittered_secs);

            log::info!(
                "[scheduler] Next wallpaper change in {:.0?} (cron: {}, jitter: {:+}s)",
                delay,
                cron_expr,
                jitter_secs
            );

            tokio::select! {
                _ = sleep(delay) => {
                    if let Err(e) = run_wallpaper_cycle(&app_handle).await {
                        if is_network_error(&e) {
                            // Retry every 5 minutes until the NEXT scheduled
                            // time after this one (a failed network call
                            // consumes no API quota).
                            let deadline = delay_until_next_fire(&rx.borrow().clone())
                                .map(|d| Instant::now() + d);
                            retry_after_network_failure(&app_handle, &mut rx, deadline).await;
                        }
                    }
                }
                _ = rx.changed() => {
                    log::info!("[scheduler] Cron expression changed, rescheduling");
                }
            }
        }
    });
}

/// Network errors are the only failures worth retrying: a failed
/// network call consumes no API quota, and the network may come back at
/// any moment. Rate-limit (429/403) and other errors wait for the next
/// scheduled time, as before.
fn is_network_error(e: &str) -> bool {
    e.contains("ERR_NETWORK")
}

/// Delay until the next cron occurrence (with the usual ±120s jitter),
/// or None when there is no upcoming occurrence (startup-only mode or
/// an invalid expression).
fn delay_until_next_fire(cron_expr: &str) -> Option<Duration> {
    if cron_expr == "@startup" || cron_expr.is_empty() {
        return None;
    }
    let schedule = parse_cron(cron_expr).ok()?;
    let next = schedule.upcoming(Utc).next()?;
    let base_delay = (next - Utc::now()).to_std().unwrap_or(Duration::ZERO);
    let jitter_secs: i64 = rand::rng().random_range(-120..=120);
    let jittered_secs = (base_delay.as_secs() as i64 + jitter_secs).max(0) as u64;
    Some(Duration::from_secs(jittered_secs))
}

const RETRY_INTERVAL: Duration = Duration::from_secs(300);

/// After a network failure, retry the wallpaper change every 5 minutes
/// until it succeeds, the next scheduled cron time arrives (`deadline`),
/// or the cron expression changes (`rx`). `deadline = None` (startup-only
/// mode) retries until success or a settings change.
async fn retry_after_network_failure(
    app_handle: &tauri::AppHandle,
    rx: &mut watch::Receiver<String>,
    deadline: Option<Instant>,
) {
    loop {
        let remaining = deadline.map(|d| d.saturating_duration_since(Instant::now()));
        if remaining == Some(Duration::ZERO) {
            log::info!("[scheduler] Next scheduled time reached — stopping retries");
            return;
        }
        let wait = remaining
            .map(|r| r.min(RETRY_INTERVAL))
            .unwrap_or(RETRY_INTERVAL);
        tokio::select! {
            _ = sleep(wait) => {}
            _ = rx.changed() => {
                log::info!("[scheduler] Schedule changed — stopping retries");
                return;
            }
        }
        match run_wallpaper_cycle(app_handle).await {
            Ok(()) => {
                log::info!("[scheduler] Retry succeeded — back on schedule");
                return;
            }
            Err(e) if is_network_error(&e) => {
                log::warn!("[scheduler] Retry failed (network) — will retry again in 5 minutes");
            }
            Err(e) => {
                log::error!(
                    "[scheduler] Retry failed with a non-network error — waiting for the next scheduled time: {}",
                    e
                );
                return;
            }
        }
    }
}

async fn run_wallpaper_cycle(app_handle: &tauri::AppHandle) -> Result<(), String> {
    log::info!("[scheduler] Running wallpaper change");
    match change_wallpaper().await {
        Ok(msg) => {
            log::info!("[scheduler] {}", msg);
            let _ = app_handle.emit("wallpaper-changed", ());

            let app2 = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                log::info!("[scheduler] Prefetching upcoming wallpapers");
                match prefetch_upcoming_wallpapers().await {
                    Ok(()) => {
                        log::info!("[scheduler] Upcoming wallpapers ready");
                        let _ = app2.emit("next-wallpaper-ready", ());
                    }
                    Err(e) => log::warn!("[scheduler] Prefetch failed: {}", e),
                }
            });
            Ok(())
        }
        Err(e) => {
            log::error!("[scheduler] Wallpaper change failed: {}", e);
            Err(e)
        }
    }
}
