//! History of photo IDs already shown as wallpaper.
//!
//! Persisted in `shown_history.json` next to settings.json (NOT inside
//! settings.json, so the settings file stays small). Used to avoid
//! showing the same photo twice until the whole pool is exhausted.
//! A plain JSON array of photo-ID strings; it grows ~12 bytes per shown
//! wallpaper, so even years of hourly changes stay well under 1 MB.
//! IDs are recorded only when a photo actually becomes the wallpaper —
//! a prefetched photo that never gets shown does not burn its showing.

use crate::fs::{files, paths};

fn history_path() -> Result<std::path::PathBuf, String> {
    paths::shown_history_file()
}

/// All recorded photo IDs (empty when no history exists yet, or when the
/// file is missing/corrupt — a corrupt file is treated as empty, never
/// as a reason to fail a wallpaper change).
pub fn load_ids() -> Vec<String> {
    let path = match history_path() {
        Ok(p) => p,
        Err(_) => return vec![],
    };
    if !path.exists() {
        return vec![];
    }
    std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok())
        .unwrap_or_default()
}

fn save_ids(ids: &[String]) -> Result<(), String> {
    let path = history_path()?;
    let bytes = serde_json::to_vec_pretty(ids)
        .map_err(|e| format!("Failed to serialize shown history: {}", e))?;
    files::write_atomic(&path, &bytes)
}

/// True when this photo ID was already shown as wallpaper.
pub fn is_shown(photo_id: &str) -> bool {
    load_ids().iter().any(|id| id == photo_id)
}

/// Record a photo ID as shown. Best-effort: a failure is only logged,
/// never propagated — a history write failure must not break a
/// wallpaper change.
pub fn record(photo_id: &str) {
    let mut ids = load_ids();
    if ids.iter().any(|id| id == photo_id) {
        return;
    }
    ids.push(photo_id.to_string());
    if let Err(e) = save_ids(&ids) {
        log::warn!("[history] Failed to save shown history: {}", e);
    }
}

/// Clear the history: the pool is treated as exhausted and photos may
/// repeat from the start (a new cycle). A failure is only logged.
pub fn clear() {
    if let Err(e) = save_ids(&[]) {
        log::warn!("[history] Failed to clear shown history: {}", e);
    }
}
