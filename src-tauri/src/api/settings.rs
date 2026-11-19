use crate::scheduler::notify_cron_changed;
use crate::settings::{
    active_api_key, active_api_key_id, load_settings, new_api_key_id, save_settings, ApiKeyEntry,
};
use crate::unsplash::{record_rate_limit_headers, reset_rate_limit, RateLimitState, RateLimitStatus};
use chrono::Utc;
use cron::Schedule;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct AppSettings {
    pub quality: String,
    pub orientation: String,
    pub wallpaper_cron: String,
    pub api_key: Option<String>,
    pub api_base_url: String,
    pub layout_direction: String,
    pub wallpaper_fit: String,
}

/// One saved key as the frontend is allowed to see it: the full key is
/// NEVER sent. Only the last 4 characters are shown, the rest masked.
#[derive(Debug, Clone, Serialize)]
pub struct ApiKeyListItem {
    pub id: String,
    pub label: String,
    pub masked_key: String,
    pub is_active: bool,
}

/// One entry of a full key list sent by the frontend to `save_api_keys`.
/// `key` may be empty for an already-saved entry, which means "keep the
/// stored key" (the frontend only ever holds the masked form of it).
/// A non-empty `key` replaces the stored key for that entry.
#[derive(Debug, Clone, Deserialize)]
pub struct SaveApiKeyInput {
    pub id: String,
    pub label: String,
    pub key: String,
}

/// Mask an API key for display: every character except the last 4 is
/// replaced with "•". Keys of 4 characters or fewer are fully masked.
pub fn mask_api_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() <= 4 {
        return "•".repeat(chars.len());
    }
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{}{}", "•".repeat(chars.len() - 4), tail)
}

fn list_items() -> Result<Vec<ApiKeyListItem>, String> {
    let s = load_settings()?;
    let active = active_api_key_id(&s);
    Ok(s.api_keys
        .iter()
        .map(|e| ApiKeyListItem {
            id: e.id.clone(),
            label: e.label.clone(),
            masked_key: mask_api_key(&e.key),
            is_active: Some(&e.id) == active.as_ref(),
        })
        .collect())
}

fn next_label(settings: &crate::settings::Settings) -> String {
    format!("کلید {}", settings.api_keys.len() + 1)
}

/// Probe a key against Unsplash. Returns Ok(()) on 200, an error otherwise.
/// When `record` is true, the probe response's rate-limit headers are
/// recorded for the given entry id (the probe itself is an
/// api.unsplash.com call). Only pass true when the probed key is (or is
/// about to become) the active key — otherwise the probe would overwrite
/// the active key's stored numbers with another key's.
async fn probe_key(key: &str, entry_id: &str, record: bool) -> Result<(), String> {
    // Probe through the configured server base (Settings `api_base_url`,
    // relay by default) — same route the client uses.
    let base = load_settings()
        .map(|s| crate::settings::effective_api_base_url(&s))
        .unwrap_or_else(|_| crate::unsplash::RELAY_BASE.to_string());
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
    let resp = client
        .get(crate::unsplash::api_url_with_base("photos", &base))
        .query(&[("per_page", "1")])
        .header("Authorization", format!("Client-ID {}", key))
        .send()
        .await
        .map_err(|e| format!("ERR_NETWORK: {}", e))?;

    if record {
        record_rate_limit_headers(resp.headers(), Some(entry_id.to_string()));
    }

    match resp.status().as_u16() {
        200 => Ok(()),
        401 => Err("Invalid API key. Please check and try again.".to_string()),
        403 => Err("API key forbidden. You may have exceeded your rate limit.".to_string()),
        s => Err(format!("Unexpected response from Unsplash: HTTP {}", s)),
    }
}

#[tauri::command]
pub async fn get_settings() -> Result<AppSettings, String> {
    let s = load_settings()?;
    // Resolve the key BEFORE moving the String fields out of `s` below:
    // borrowing `s` after a partial move is E0382.
    let api_key = active_api_key(&s)
        .map(|e| e.key.clone())
        .or_else(|| s.api_key.clone());
    let api_base_url = crate::settings::effective_api_base_url(&s);
    let layout_direction = s.layout_direction.clone();
    let wallpaper_fit = crate::settings::effective_wallpaper_fit(&s);
    Ok(AppSettings {
        quality: s.quality,
        orientation: s.orientation,
        wallpaper_cron: s.wallpaper_cron,
        api_key,
        api_base_url,
        layout_direction,
        wallpaper_fit,
    })
}

/// The active key (full value). Kept for App.tsx, which only checks
/// whether a key exists to decide whether to show the first-run setup.
/// Prefer `get_api_keys` (masked) for any display purpose.
#[tauri::command]
pub async fn get_api_key() -> Result<Option<String>, String> {
    let s = load_settings()?;
    Ok(active_api_key(&s)
        .map(|e| e.key.clone())
        .or(s.api_key))
}

/// All saved keys with masked values, for the Settings key list.
#[tauri::command]
pub async fn get_api_keys() -> Result<Vec<ApiKeyListItem>, String> {
    list_items()
}

/// Save the whole key list (labels, active selection, deletions, and key
/// replacements) from the frontend. An entry with an empty `key` keeps its
/// stored key; an entry with an unknown id and a non-empty key is added.
/// Refuses an empty list — the app always needs at least one key.
/// When the active key changes, the stored rate-limit status is reset.
#[tauri::command]
pub async fn save_api_keys(
    keys: Vec<SaveApiKeyInput>,
    active_id: Option<String>,
) -> Result<(), String> {
    if keys.is_empty() {
        return Err("At least one API key is required.".to_string());
    }

    let mut settings = load_settings()?;
    let old_active = active_api_key_id(&settings);

    let mut new_keys: Vec<ApiKeyEntry> = Vec::with_capacity(keys.len());
    for (i, input) in keys.into_iter().enumerate() {
        let label = input.label.trim().to_string();
        let label = if label.is_empty() {
            format!("کلید {}", i + 1)
        } else {
            label
        };
        let new_key = input.key.trim().to_string();

        // Resolve the id: keep a known stored id, otherwise mint a fresh
        // one (covers frontend temp ids like "new-..." for unsaved rows).
        let known = settings.api_keys.iter().any(|e| e.id == input.id);
        let id = if input.id.trim().is_empty() || !known {
            new_api_key_id()
        } else {
            input.id.clone()
        };
        if new_keys.iter().any(|e| e.id == id) {
            return Err("Duplicate key id.".to_string());
        }

        let key = if new_key.is_empty() {
            // Keep the stored key for this entry.
            match settings.api_keys.iter().find(|e| e.id == id) {
                Some(existing) => existing.key.clone(),
                None => return Err("API key cannot be empty.".to_string()),
            }
        } else {
            new_key
        };

        new_keys.push(ApiKeyEntry { id, label, key });
    }

    // Resolve the active key: the requested one if it is in the list,
    // else the previously active one if it survived, else the first entry.
    let active = match &active_id {
        Some(id) if new_keys.iter().any(|e| &e.id == id) => Some(id.clone()),
        _ => match &old_active {
            Some(id) if new_keys.iter().any(|e| &e.id == id) => Some(id.clone()),
            _ => new_keys.first().map(|e| e.id.clone()),
        },
    };
    let active = active.ok_or_else(|| "At least one API key is required.".to_string())?;

    if old_active.as_ref() != Some(&active) {
        reset_rate_limit();
    }

    settings.api_keys = new_keys;
    settings.active_api_key_id = Some(active);
    settings.api_key = None;
    save_settings(&settings)
}

/// Validate the key against the Unsplash API, then save it as a NEW entry
/// (label "کلید N+1") and make it active if it is the first key.
/// The key is never sent back to the frontend.
#[tauri::command]
pub async fn verify_and_save_api_key(key: String) -> Result<(), String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("API key cannot be empty.".to_string());
    }

    let mut settings = load_settings()?;
    if settings.api_keys.iter().any(|e| e.key == key) {
        return Err("This API key is already saved.".to_string());
    }
    let is_first = settings.api_keys.is_empty();

    // A new active key invalidates the old key's rate-limit numbers.
    // Reset BEFORE the probe so the probe's own headers become the fresh
    // status for the new key.
    if is_first {
        reset_rate_limit();
    }
    let id = new_api_key_id();
    probe_key(&key, &id, is_first).await?;

    let label = next_label(&settings);
    settings.api_keys.push(ApiKeyEntry {
        id: id.clone(),
        label,
        key,
    });
    if is_first {
        settings.active_api_key_id = Some(id);
    }
    settings.api_key = None;
    save_settings(&settings)
}

/// Validate a replacement key for an EXISTING entry and save it in place
/// (same id, same label). Used by the per-row Verify & Save button when
/// the user retypes the key of a saved row. If the entry is the active
/// key, the stored rate-limit status is reset and replaced by the probe's
/// own headers.
#[tauri::command]
pub async fn verify_and_update_api_key(id: String, key: String) -> Result<(), String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("API key cannot be empty.".to_string());
    }

    let mut settings = load_settings()?;
    let pos = settings
        .api_keys
        .iter()
        .position(|e| e.id == id)
        .ok_or_else(|| "Key not found.".to_string())?;
    if settings
        .api_keys
        .iter()
        .any(|e| e.id != id && e.key == key)
    {
        return Err("This API key is already saved.".to_string());
    }

    let is_active = active_api_key_id(&settings).as_deref() == Some(id.as_str());
    if is_active {
        reset_rate_limit();
    }
    probe_key(&key, &id, is_active).await?;

    settings.api_keys[pos].key = key;
    settings.api_key = None;
    save_settings(&settings)
}

/// Latest rate-limit status for the ACTIVE key, from the most recent
/// api.unsplash.com response (any call). Returns None before the first
/// call with this key — the frontend shows `0/50` in that case.
/// A stored status belonging to a different (no longer active) key is
/// treated as absent.
#[tauri::command]
pub async fn get_rate_limit_status(
    state: State<'_, RateLimitState>,
) -> Result<Option<RateLimitStatus>, String> {
    let settings = load_settings()?;
    let active = active_api_key_id(&settings);
    let slot = state
        .lock()
        .map_err(|_| "Rate-limit state is unavailable.".to_string())?;
    Ok(match slot.clone() {
        Some(status) if status.key_id == active => Some(status),
        _ => None,
    })
}

#[tauri::command]
pub async fn update_settings(
    quality: String,
    orientation: String,
    wallpaper_cron: String,
    api_base_url: String,
    layout_direction: String,
    wallpaper_fit: String,
) -> Result<(), String> {
    let valid_qualities = ["raw", "full", "regular", "small", "thumb"];
    if !valid_qualities.contains(&quality.as_str()) {
        return Err(format!("Invalid quality: {}", quality));
    }
    let valid_orientations = ["landscape", "portrait", "squarish"];
    if !valid_orientations.contains(&orientation.as_str()) {
        return Err(format!("Invalid orientation: {}", orientation));
    }
    let valid_fits = ["fill", "fit", "stretch", "tile", "center", "span"];
    if !valid_fits.contains(&wallpaper_fit.as_str()) {
        return Err(format!("Invalid wallpaper fit: {}", wallpaper_fit));
    }
    validate_cron(&wallpaper_cron)?;

    // Server address: empty means the relay default; anything else must
    // be an http(s) URL. Direction: only "ltr" / "rtl".
    let base = api_base_url.trim().trim_end_matches('/').to_string();
    let api_base_url = if base.is_empty() {
        crate::unsplash::RELAY_BASE.to_string()
    } else if base.starts_with("http://") || base.starts_with("https://") {
        base
    } else {
        return Err("Invalid server address: must start with http:// or https://".to_string());
    };
    if layout_direction != "ltr" && layout_direction != "rtl" {
        return Err(format!("Invalid layout direction: {}", layout_direction));
    }

    let mut s = load_settings()?;
    s.quality = quality;
    s.orientation = orientation;
    s.wallpaper_cron = wallpaper_cron.clone();
    s.api_base_url = api_base_url;
    s.layout_direction = layout_direction;
    s.wallpaper_fit = wallpaper_fit;
    save_settings(&s)?;

    notify_cron_changed(wallpaper_cron);
    Ok(())
}

pub fn validate_cron(expr: &str) -> Result<(), String> {
    if expr == "@startup" || expr.is_empty() {
        return Ok(());
    }
    let six_field = format!("0 {}", expr.trim());
    let schedule = Schedule::from_str(&six_field)
        .map_err(|e| format!("Invalid cron expression \"{}\": {}", expr, e))?;

    // Enforce minimum 30-minute interval by checking the actual gap between
    // the first two upcoming occurrences.
    let mut upcoming = schedule.upcoming(Utc);
    if let (Some(first), Some(second)) = (upcoming.next(), upcoming.next()) {
        let gap_mins = (second - first).num_minutes();
        if gap_mins < 30 {
            return Err(format!(
                "Minimum allowed interval is 30 minutes. \
                 This expression fires every ~{} minute(s).",
                gap_mins
            ));
        }
    }

    Ok(())
}
