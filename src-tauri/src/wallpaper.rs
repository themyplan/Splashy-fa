use crate::fs::{dirs, files, images, paths};
use crate::getter::run_getter_cycle;
use crate::settings::{group_for_hour, load_settings, Settings};
use crate::unsplash::{ApiPhoto, UnsplashClient, WallpaperMeta};
use chrono::{Local, NaiveTime, Timelike, Utc};
use cron::Schedule;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tokio::sync::Mutex;

/// Serializes concurrent `change_wallpaper` calls so file rotation never races.
static CHANGE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
fn change_lock() -> &'static Mutex<()> {
    CHANGE_LOCK.get_or_init(|| Mutex::new(()))
}

/// Full wallpaper change cycle with rotation and prefetch.
///
/// Rotation:
///   current_wallpaper.jpg  →  previous_wallpaper.jpg
///   next_wallpaper.jpg     →  current_wallpaper.jpg  (if it exists)
///   (else run a fresh getter cycle for current)
///
/// After the new wallpaper is set a background task prefetches the next one.
pub async fn change_wallpaper() -> Result<String, String> {
    let _guard = change_lock().lock().await;
    log::info!("[wallpaper] Starting wallpaper change cycle");
    let settings = load_settings()?;

    let current_dir = paths::current_dir()?;
    dirs::ensure_dir(&current_dir)?;

    let current_img  = current_dir.join("current_wallpaper.jpg");
    let current_meta = current_dir.join("current_wallpaper_meta.json");
    let prev_img     = current_dir.join("previous_wallpaper.jpg");
    let prev_meta    = current_dir.join("previous_wallpaper_meta.json");
    let next_img     = current_dir.join("next_wallpaper.jpg");
    let next_meta    = current_dir.join("next_wallpaper_meta.json");

    // Rotate current → previous (ignore errors; first launch has no current yet)
    if current_img.exists()  { let _ = std::fs::rename(&current_img,  &prev_img); }
    if current_meta.exists() { let _ = std::fs::rename(&current_meta, &prev_meta); }

    let result_msg;

    if next_img.exists() {
        // Promote prefetched next → current
        std::fs::rename(&next_img, &current_img)
            .map_err(|e| format!("Failed to promote next wallpaper: {}", e))?;
        if next_meta.exists() {
            let _ = std::fs::rename(&next_meta, &current_meta);
        }

        // Shift the queue forward so the next queued photo becomes the
        // head (slot 2 → slot 1, slot 3 → slot 2, ...). Slots above the
        // desired count are deleted instead of shifted.
        shift_queue(&current_dir, crate::settings::effective_prefetch_count(&settings));

        set_wallpaper(&current_img, Some(&prev_img))?;

        // The prefetched photo only counts as shown now that it is the
        // wallpaper — prefetch alone must not burn its one showing.
        if let Some(meta) = read_meta(&current_meta) {
            crate::shown_history::record(&meta.photo_id);
        }

        result_msg = read_meta(&current_meta)
            .map(|m| format!("Wallpaper set!\nPhoto by {} — https://unsplash.com/photos/{}", m.author_name, m.photo_id))
            .unwrap_or_else(|| "Wallpaper set!".to_string());

        log::info!("[wallpaper] Used prefetched wallpaper");
    } else {
        // No prefetched next — fetch fresh
        let client = crate::unsplash::client_from_active_key()?;
        log::debug!(
            "[wallpaper] Fresh fetch: orientation={}, {} getter_target(s)",
            settings.orientation, settings.getter_targets.len()
        );

        let now_time = Local::now().naive_local().time();
        let time_group = group_for_hour(&settings.time_groups, now_time.hour())
            .map(|g| g.id.clone());
        let getter = run_getter_cycle(&client, &settings, Some(now_time)).await?;
        let photo  = getter.photo;
        log::info!("[wallpaper] Photo: id={} source={}:{} group={:?}", photo.id, getter.source_type, getter.source_value.as_deref().unwrap_or("random"), time_group);

        // Only count the download with Unsplash when the image bytes
        // actually came from the network — a reused saved copy was
        // already counted the first time it was downloaded.
        let downloaded = resolve_wallpaper_image(&client, &photo, &settings.quality, &current_img, &settings).await?;
        if downloaded {
            if let Some(ref dl) = photo.links.download_location {
                client.register_download(dl).await;
            }
        }

        let dominant_color = images::dominant_color_hex(&current_img).ok();
        write_meta(&photo, getter.source_type, getter.source_value, time_group, dominant_color, &current_meta)?;

        set_wallpaper(&current_img, Some(&prev_img))?;
        // Record AFTER the wallpaper is actually set, so a failed
        // change never burns the photo's one showing.
        crate::shown_history::record(&photo.id);
        log::info!("[wallpaper] Wallpaper set — photo by @{}", photo.user.username);
        result_msg = format!(
            "Wallpaper set!\nPhoto by {} — https://unsplash.com/photos/{}",
            photo.user.name, photo.id
        );
    }

    Ok(result_msg)
}

/// Restore the previous wallpaper: swap current <-> previous (image + meta)
/// and set the swapped-in image as the desktop wallpaper. Swapping (rather
/// than overwriting) means the user can toggle back and forth between the
/// two. A missing current (first wallpaper ever) is fine: previous simply
/// becomes current.
pub async fn restore_previous_wallpaper() -> Result<String, String> {
    let _guard = change_lock().lock().await;
    log::info!("[wallpaper] Restoring previous wallpaper");

    let current_dir = paths::current_dir()?;
    let current_img  = current_dir.join("current_wallpaper.jpg");
    let current_meta = current_dir.join("current_wallpaper_meta.json");
    let prev_img     = current_dir.join("previous_wallpaper.jpg");
    let prev_meta    = current_dir.join("previous_wallpaper_meta.json");

    if !prev_img.exists() {
        return Err("No previous wallpaper to restore".to_string());
    }

    // A photo the user marked "never" must not come back through
    // restore, either: it never becomes the wallpaper again.
    if let Some(meta) = read_meta(&prev_meta) {
        if crate::never_list::is_never(&meta.photo_id) {
            return Err("That photo is on your never list".to_string());
        }
    }

    let tmp_img  = current_dir.join("swap_tmp_wallpaper.jpg");
    let tmp_meta = current_dir.join("swap_tmp_wallpaper_meta.json");
    let _ = std::fs::remove_file(&tmp_img);
    let _ = std::fs::remove_file(&tmp_meta);

    if current_img.exists() {
        std::fs::rename(&current_img, &tmp_img)
            .map_err(|e| format!("Failed to swap wallpapers: {}", e))?;
    }
    std::fs::rename(&prev_img, &current_img)
        .map_err(|e| format!("Failed to swap wallpapers: {}", e))?;
    if tmp_img.exists() {
        std::fs::rename(&tmp_img, &prev_img)
            .map_err(|e| format!("Failed to swap wallpapers: {}", e))?;
    }
    if current_meta.exists() {
        let _ = std::fs::rename(&current_meta, &tmp_meta);
    }
    if prev_meta.exists() {
        let _ = std::fs::rename(&prev_meta, &current_meta);
    }
    if tmp_meta.exists() {
        let _ = std::fs::rename(&tmp_meta, &prev_meta);
    }

    set_wallpaper(&current_img, Some(&prev_img))?;

    Ok(read_meta(&current_meta)
        .map(|m| format!("Wallpaper restored!\nPhoto by {} — https://unsplash.com/photos/{}", m.author_name, m.photo_id))
        .unwrap_or_else(|| "Wallpaper restored!".to_string()))
}

// ─── prefetch queue ───────────────────────────────────────────────────

/// Hard cap for queue slots (also the cap for the prefetch-count setting).
const MAX_QUEUE_SLOTS: usize = 20;

/// Paths of queue slot `n` (1-based). Slot 1 is the historical
/// `next_wallpaper.jpg` — the head of the queue, which is also what the
/// Home "next" card and `should_reprefetch_on_enable` look at.
fn queue_slot_paths(current_dir: &Path, slot: usize) -> (PathBuf, PathBuf) {
    if slot <= 1 {
        (
            current_dir.join("next_wallpaper.jpg"),
            current_dir.join("next_wallpaper_meta.json"),
        )
    } else {
        (
            current_dir.join(format!("next_{}_wallpaper.jpg", slot)),
            current_dir.join(format!("next_{}_wallpaper_meta.json", slot)),
        )
    }
}

/// Photo IDs already sitting in the prefetch queue (read from the slot
/// metas), so a fill run never queues the same photo twice.
fn queued_photo_ids(current_dir: &Path) -> std::collections::HashSet<String> {
    (1..=MAX_QUEUE_SLOTS)
        .filter_map(|slot| {
            let (_, meta_path) = queue_slot_paths(current_dir, slot);
            read_meta(&meta_path).map(|m| m.photo_id)
        })
        .collect()
}

/// Delete queue slots above `desired` (used when the user lowers the
/// count or turns prefetching off — the queue collapses to one).
fn prune_queue(current_dir: &Path, desired: usize) {
    for slot in (desired + 1)..=MAX_QUEUE_SLOTS {
        let (img, meta) = queue_slot_paths(current_dir, slot);
        let _ = std::fs::remove_file(&img);
        let _ = std::fs::remove_file(&meta);
    }
}

/// Shift the queue forward after the head was promoted: slot 2 becomes
/// the head, slot 3 becomes slot 2, and so on. Slots above `desired`
/// are deleted instead of shifted.
fn shift_queue(current_dir: &Path, desired: usize) {
    for slot in 2..=MAX_QUEUE_SLOTS {
        let (img, meta) = queue_slot_paths(current_dir, slot);
        if slot <= desired {
            let (prev_img, prev_meta) = queue_slot_paths(current_dir, slot - 1);
            if img.exists() {
                let _ = std::fs::rename(&img, &prev_img);
            }
            if meta.exists() {
                let _ = std::fs::rename(&meta, &prev_meta);
            }
        } else {
            let _ = std::fs::remove_file(&img);
            let _ = std::fs::remove_file(&meta);
        }
    }
}

/// Fill the first empty queue slot. Returns Ok(true) when a photo was
/// queued, Ok(false) when the queue is full. A photo already sitting in
/// the queue is never queued twice: picks are retried a few times
/// against the queued IDs (the getter itself already skips photos in
/// the shown history).
async fn prefetch_one_slot(
    current_dir: &Path,
    settings: &Settings,
    queued: &mut std::collections::HashSet<String>,
) -> Result<bool, String> {
    let desired = crate::settings::effective_prefetch_count(settings);
    let empty_slot = (1..=desired).find(|&slot| {
        let (img, _) = queue_slot_paths(current_dir, slot);
        !img.exists()
    });
    let Some(slot) = empty_slot else {
        return Ok(false);
    };

    let client = crate::unsplash::client_from_active_key()?;
    let next_time = compute_next_fire_time(&settings.wallpaper_cron);
    let time_group = next_time
        .and_then(|t| group_for_hour(&settings.time_groups, t.hour()))
        .map(|g| g.id.clone());

    let mut last_err = String::new();
    for _ in 0..5usize {
        let getter = run_getter_cycle(&client, settings, next_time).await?;
        let photo = getter.photo;
        if queued.contains(&photo.id) {
            log::info!(
                "[wallpaper] Prefetch pick id={} already queued, retrying",
                photo.id
            );
            last_err = format!("photo {} already queued", photo.id);
            continue;
        }

        // Prefetch does NOT record the photo in the shown history — only
        // becoming the wallpaper counts. register_download is likewise
        // skipped when a saved copy was reused (counted the first time).
        let (dest, meta_path) = queue_slot_paths(current_dir, slot);
        let downloaded =
            resolve_wallpaper_image(&client, &photo, &settings.quality, &dest, settings).await?;
        if downloaded {
            if let Some(ref dl) = photo.links.download_location {
                client.register_download(dl).await;
            }
        }

        let dominant_color = images::dominant_color_hex(&dest).ok();
        write_meta(
            &photo,
            getter.source_type,
            getter.source_value,
            time_group,
            dominant_color,
            &meta_path,
        )?;
        log::info!(
            "[wallpaper] Prefetched slot {}/{} — photo id={}",
            slot,
            desired,
            photo.id
        );
        queued.insert(photo.id);
        return Ok(true);
    }
    Err(last_err)
}

/// Fill every empty queue slot up to the desired count, and prune slots
/// above it. Exposed publicly so the scheduler and the re-prefetch task
/// can call it and emit an event on completion.
pub async fn prefetch_upcoming_wallpapers() -> Result<(), String> {
    let settings = load_settings()?;
    let current_dir = paths::current_dir()?;
    dirs::ensure_dir(&current_dir)?;
    let desired = crate::settings::effective_prefetch_count(&settings);
    prune_queue(&current_dir, desired);
    let mut queued = queued_photo_ids(&current_dir);
    loop {
        match prefetch_one_slot(&current_dir, &settings, &mut queued).await {
            Ok(true) => continue,
            Ok(false) => break,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

// ─── save-to-folder ───────────────────────────────────────────────────────

/// Folder where wallpapers are saved when `save_wallpapers` is on.
/// None when the feature is off or no folder has been chosen yet.
fn save_folder(settings: &Settings) -> Option<PathBuf> {
    if !settings.save_wallpapers {
        return None;
    }
    let folder = settings.wallpaper_save_folder.trim();
    if folder.is_empty() {
        None
    } else {
        Some(PathBuf::from(folder))
    }
}

fn saved_image_path(folder: &Path, photo_id: &str) -> PathBuf {
    folder.join(format!("{}.jpg", photo_id))
}

fn saved_meta_path(folder: &Path, photo_id: &str) -> PathBuf {
    folder.join(format!("{}.json", photo_id))
}

/// Small sidecar written next to each saved wallpaper copy, so the app
/// knows at which quality the copy was saved. A copy saved at another
/// quality is NOT reused — the photo is downloaded again at the current
/// quality and the saved copy is overwritten.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedCopyMeta {
    photo_id: String,
    quality: String,
    author_name: String,
    unsplash_url: String,
    saved_iso: String,
}

fn saved_quality_matches(folder: &Path, photo_id: &str, quality: &str) -> bool {
    std::fs::read(saved_meta_path(folder, photo_id))
        .ok()
        .and_then(|b| serde_json::from_slice::<SavedCopyMeta>(&b).ok())
        .map(|m| m.quality == quality)
        .unwrap_or(false)
}

/// Save a copy of the wallpaper at `src` into the save folder
/// (`<photo_id>.jpg` + `<photo_id>.json`). Best-effort helper: returns
/// Err on failure, callers only log it — saving must never break
/// setting the wallpaper. Nothing is ever deleted from the save folder.
fn save_wallpaper_copy(folder: &Path, photo: &ApiPhoto, quality: &str, src: &Path) -> Result<(), String> {
    dirs::ensure_dir(folder)?;
    let dest = saved_image_path(folder, &photo.id);
    std::fs::copy(src, &dest)
        .map_err(|e| format!("Failed to copy wallpaper to {}: {}", dest.display(), e))?;
    let meta = SavedCopyMeta {
        photo_id: photo.id.clone(),
        quality: quality.to_string(),
        author_name: photo.user.name.clone(),
        unsplash_url: format!("https://unsplash.com/photos/{}", photo.id),
        saved_iso: Utc::now().to_rfc3339(),
    };
    let bytes = serde_json::to_vec_pretty(&meta)
        .map_err(|e| format!("Failed to serialize saved copy meta: {}", e))?;
    files::write_atomic(&saved_meta_path(folder, &photo.id), &bytes)
}

/// Resolve the image bytes for `photo` into `dest`.
/// When saving is on and a copy of this photo (same quality) already
/// exists in the save folder, the local copy is used instead of
/// downloading. Otherwise the image is downloaded and a copy is saved
/// (best-effort) for next time.
/// Returns Ok(true) when the bytes came from the network, Ok(false)
/// when a saved local copy was reused.
pub async fn resolve_wallpaper_image(
    client: &UnsplashClient,
    photo: &ApiPhoto,
    quality: &str,
    dest: &Path,
    settings: &Settings,
) -> Result<bool, String> {
    if let Some(folder) = save_folder(settings) {
        let saved = saved_image_path(&folder, &photo.id);
        if saved.exists() && saved_quality_matches(&folder, &photo.id, quality) {
            if let Some(parent) = dest.parent() {
                dirs::ensure_dir(parent)?;
            }
            std::fs::copy(&saved, dest)
                .map_err(|e| format!("Failed to reuse saved wallpaper: {}", e))?;
            log::info!("[wallpaper] Reused saved copy of photo {} (no download)", photo.id);
            return Ok(false);
        }
    }

    download_wallpaper_to_path(client, photo, quality, dest).await?;

    if let Some(folder) = save_folder(settings) {
        if let Err(e) = save_wallpaper_copy(&folder, photo, quality, dest) {
            log::warn!("[wallpaper] Could not save wallpaper copy: {}", e);
        }
    }
    Ok(true)
}

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Download a photo at the requested quality to `dest` atomically.
pub async fn download_wallpaper_to_path(
    client: &UnsplashClient,
    photo: &ApiPhoto,
    quality: &str,
    dest: &Path,
) -> Result<(), String> {
    let image_url = match quality {
        "raw"   => photo.urls.raw.as_deref(),
        "full"  => photo.urls.full.as_deref(),
        "small" => photo.urls.small.as_deref(),
        "thumb" => photo.urls.thumb.as_deref(),
        _       => photo.urls.regular.as_deref(),
    }
    .ok_or_else(|| format!("No image URL for quality \"{}\"", quality))?;

    let (bytes, content_type) = client.download_image(image_url).await?;

    if let Some(parent) = dest.parent() {
        dirs::ensure_dir(parent)?;
    }
    if content_type.contains("jpeg") || content_type.contains("jpg") {
        files::write_atomic(dest, &bytes)?;
    } else {
        images::save_as_jpeg_atomic(dest, &bytes)?;
    }
    Ok(())
}

/// Backwards-compatible wrapper: download to `current/current_wallpaper.jpg`.
pub async fn download_wallpaper(
    client: &UnsplashClient,
    photo: &ApiPhoto,
    quality: &str,
) -> Result<PathBuf, String> {
    let current_dir = paths::current_dir()?;
    dirs::ensure_dir(&current_dir)?;
    let dest = current_dir.join("current_wallpaper.jpg");
    download_wallpaper_to_path(client, photo, quality, &dest).await?;
    Ok(dest)
}

/// Serialize and atomically write a `WallpaperMeta` JSON file.
fn write_meta(
    photo: &ApiPhoto,
    source_type: String,
    source_value: Option<String>,
    time_group: Option<String>,
    dominant_color: Option<String>,
    path: &Path,
) -> Result<(), String> {
    let meta = WallpaperMeta {
        photo_id:        photo.id.clone(),
        source_type,
        source_value,
        author_username: photo.user.username.clone(),
        author_name:     photo.user.name.clone(),
        unsplash_url:    format!("https://unsplash.com/photos/{}", photo.id),
        downloaded_iso:  Utc::now().to_rfc3339(),
        time_group,
        dominant_color,
    };
    let bytes = serde_json::to_vec_pretty(&meta)
        .map_err(|e| format!("Failed to serialize meta: {}", e))?;
    files::write_atomic(path, &bytes)
}

/// Try to deserialize a `WallpaperMeta` from a file; returns `None` on any error.
pub fn read_meta(path: &Path) -> Option<WallpaperMeta> {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

/// Returns `true` if enabling a new source should trigger an immediate re-prefetch.
/// True when: no next wallpaper is queued, OR the queued one was from the "random" fallback.
pub fn should_reprefetch_on_enable() -> bool {
    let current_dir = match paths::current_dir() {
        Ok(d) => d,
        Err(_) => return false,
    };
    let img_path  = current_dir.join("next_wallpaper.jpg");
    let meta_path = current_dir.join("next_wallpaper_meta.json");

    if !img_path.exists() {
        return true;
    }
    match read_meta(&meta_path) {
        Some(m) => m.source_type == "random",
        None    => true,
    }
}

/// Check if any queued wallpaper came from `source_type` + `source_value`.
/// If one matches, delete it (all matching slots) so the scheduler fetches
/// a fresh one. Returns `true` if anything was invalidated.
pub fn invalidate_next_if_from_source(source_type: &str, source_value: &str) -> bool {
    let current_dir = match paths::current_dir() {
        Ok(d) => d,
        Err(_) => return false,
    };
    let mut invalidated = false;
    for slot in 1..=MAX_QUEUE_SLOTS {
        let (img_path, meta_path) = queue_slot_paths(&current_dir, slot);
        let Some(meta) = read_meta(&meta_path) else {
            continue;
        };
        if meta.source_type == source_type
            && meta.source_value.as_deref() == Some(source_value)
        {
            let _ = std::fs::remove_file(&img_path);
            let _ = std::fs::remove_file(&meta_path);
            log::info!(
                "[wallpaper] Invalidated queued wallpaper in slot {} — source {}:{} was disabled",
                slot, source_type, source_value
            );
            invalidated = true;
        }
    }
    invalidated
}

/// Compute the local wall-clock time of the next scheduled wallpaper change.
/// Returns `None` for startup-only or invalid expressions.
fn compute_next_fire_time(cron_expr: &str) -> Option<NaiveTime> {
    if cron_expr == "@startup" || cron_expr.is_empty() {
        return None;
    }
    let six_field = format!("0 {}", cron_expr.trim());
    let schedule = Schedule::from_str(&six_field).ok()?;
    let next_utc = schedule.upcoming(Utc).next()?;
    let next_local = next_utc.with_timezone(&Local);
    Some(next_local.naive_local().time())
}

/// Map the user's fit setting to the `wallpaper` crate mode.
/// Fill = Crop (WallpaperStyle 10), Fit = Fit (6), Stretch = Stretch (2),
/// Tile = Tile, Center = Center, Span = Span (22). Unknown values fall
/// back to Crop — the behavior of every build before 1.0.7-5.
pub fn wallpaper_mode(fit: &str) -> wallpaper::Mode {
    match fit {
        "fit" => wallpaper::Mode::Fit,
        "stretch" => wallpaper::Mode::Stretch,
        "tile" => wallpaper::Mode::Tile,
        "center" => wallpaper::Mode::Center,
        "span" => wallpaper::Mode::Span,
        _ => wallpaper::Mode::Crop,
    }
}

/// Re-apply the user's chosen fit mode to the wallpaper that is already
/// set, without changing the image. Used when the user picks a new fit in
/// Settings so the desktop updates at once instead of waiting for the
/// next wallpaper change. No-op on macOS (the macOS path always uses fill).
pub fn apply_wallpaper_fit() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mode = load_settings()
            .map(|s| wallpaper_mode(&crate::settings::effective_wallpaper_fit(&s)))
            .unwrap_or(wallpaper::Mode::Crop);
        wallpaper::set_mode(mode)
            .map_err(|e| format!("Failed to set wallpaper mode: {}", e))
    }
}

/// Set the desktop wallpaper to the file at `path`.
pub fn set_wallpaper(path: &PathBuf, prev_path: Option<&PathBuf>) -> Result<(), String> {
    let path_str = path
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize path: {}", e))?
        .to_string_lossy()
        .to_string();
    let path_str = path_str
        .strip_prefix(r"\\?\")
        .unwrap_or(&path_str)
        .to_string();

    #[cfg(target_os = "macos")]
    {
        let prev_str = prev_path
            .filter(|p| p.exists())
            .and_then(|p| p.canonicalize().ok())
            .map(|p| p.to_string_lossy().to_string());
        return set_wallpaper_macos(&path_str, prev_str.as_deref());
    }

    #[cfg(not(target_os = "macos"))]
    {
        wallpaper::set_from_path(&path_str)
            .map_err(|e| format!("Failed to set wallpaper: {}", e))?;
        // Apply the user's chosen fit on EVERY set. Before 1.0.7-5 this
        // was hardcoded to Crop (Fill, WallpaperStyle 10), which is why
        // a registry edit to Fit (6) was overwritten on every change.
        // Loading the setting here (rather than threading it through the
        // callers) guarantees change_wallpaper AND restore_previous
        // both honor it.
        let mode = load_settings()
            .map(|s| wallpaper_mode(&crate::settings::effective_wallpaper_fit(&s)))
            .unwrap_or(wallpaper::Mode::Crop);
        wallpaper::set_mode(mode)
            .map_err(|e| format!("Failed to set wallpaper mode: {}", e))?;
        Ok(())
    }
}

/// Sets the wallpaper on macOS using NSWorkspace with fill/crop scaling.
/// Each call writes the image to a unique timestamped filename so macOS
/// never hits its URL→bitmap cache (which causes stale content to be shown
/// when the same path is reused with different content across cycles).
#[cfg(target_os = "macos")]
fn set_wallpaper_macos(path_str: &str, _prev_str: Option<&str>) -> Result<(), String> {
    use objc2::runtime::AnyObject;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSScreen, NSWorkspace, NSWorkspaceDesktopImageAllowClippingKey,
        NSWorkspaceDesktopImageScalingKey,
    };
    use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

    // macOS caches desktop wallpapers keyed by file URL. Reusing the same path
    // (current_wallpaper.jpg) with new content across cycles causes macOS to
    // show the stale cached bitmap instead of the updated file. Using a fresh
    // unique URL every call forces macOS to re-read from disk.
    let parent = std::path::Path::new(path_str)
        .parent()
        .unwrap_or(std::path::Path::new("/tmp"));

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let active_name = format!("wallpaper_active_{}.jpg", ts);
    let active_path = parent.join(&active_name);

    std::fs::copy(path_str, &active_path)
        .map_err(|e| format!("Failed to prepare wallpaper: {}", e))?;

    let active_str = active_path.to_string_lossy().to_string();

    unsafe {
        // SAFETY: setDesktopImageURL:forScreen:options:error: is documented as
        // thread-safe. MainThreadMarker is used only to satisfy NSScreen::screens.
        let mtm = MainThreadMarker::new_unchecked();

        let ns_path = NSString::from_str(&active_str);
        let url = NSURL::fileURLWithPath(&ns_path);
        let workspace = NSWorkspace::sharedWorkspace();

        // NSImageScaleProportionallyUpOrDown (3) + allowClipping = true → fill/crop
        let scaling_val = NSNumber::new_usize(3);
        let clipping_val = NSNumber::new_bool(true);
        let scaling_any = &*((&*scaling_val as *const NSNumber).cast::<AnyObject>());
        let clipping_any = &*((&*clipping_val as *const NSNumber).cast::<AnyObject>());

        let options = NSDictionary::<NSString, AnyObject>::from_slices(
            &[NSWorkspaceDesktopImageScalingKey, NSWorkspaceDesktopImageAllowClippingKey],
            &[scaling_any, clipping_any],
        );

        for screen in NSScreen::screens(mtm).iter() {
            workspace
                .setDesktopImageURL_forScreen_options_error(&url, &screen, &options)
                .map_err(|e| format!("Failed to set wallpaper: {}", e.localizedDescription()))?;
        }
    }

    // Clean up any previous unique-named copies, keeping only the one we just set.
    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("wallpaper_active_")
                && name.ends_with(".jpg")
                && name != active_name
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    Ok(())
}
