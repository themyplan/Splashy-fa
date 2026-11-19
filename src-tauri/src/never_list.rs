//! Photos the user marked "never" (هرگز): they must never become the
//! wallpaper again — not even after the shown history is cleared for a
//! new cycle. Persisted in `never_blacklist.json` next to settings.json
//! (NOT inside settings.json, so the settings file stays small).
//! A plain JSON array of photo-ID strings. Entries are never removed
//! by the app; there is no UI to un-blacklist yet. A blacklisted photo
//! that was saved to the save folder is NOT deleted — it simply never
//! becomes the wallpaper again.

use crate::fs::{files, paths};

fn blacklist_path() -> Result<std::path::PathBuf, String> {
    paths::never_blacklist_file()
}

/// All blacklisted photo IDs (empty when the file is missing/corrupt —
/// a corrupt file is treated as empty, never as a reason to fail a
/// wallpaper change).
pub fn load_ids() -> Vec<String> {
    let path = match blacklist_path() {
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
    let path = blacklist_path()?;
    let bytes = serde_json::to_vec_pretty(ids)
        .map_err(|e| format!("Failed to serialize never list: {}", e))?;
    files::write_atomic(&path, &bytes)
}

/// True when this photo ID is on the never list.
pub fn is_never(photo_id: &str) -> bool {
    load_ids().iter().any(|id| id == photo_id)
}

/// Add a photo ID to the never list (deduplicated). Best-effort: a
/// failure is only logged, never propagated — a blacklist write failure
/// must not break a wallpaper change.
pub fn add(photo_id: &str) {
    let mut ids = load_ids();
    if ids.iter().any(|id| id == photo_id) {
        return;
    }
    ids.push(photo_id.to_string());
    if let Err(e) = save_ids(&ids) {
        log::warn!("[never] Failed to save never list: {}", e);
    }
}
