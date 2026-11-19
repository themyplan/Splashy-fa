use crate::fs::{dirs, files, paths};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetterTarget {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String, // "collection" | "user" | "topic" | "related"
    pub value: String,
    pub enabled: bool,
    pub weight: u32,
}

/// One of the 4 predefined time-of-day groups.
/// `start_hour` is inclusive, `end_hour` is exclusive (both 0-23).
/// When `start_hour > end_hour` the range wraps around midnight (e.g. night: 22-06).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeGroup {
    pub id: String,
    pub label: String,
    pub start_hour: u32,
    pub end_hour: u32,
    /// IDs of GetterTargets that should fire during this group.
    pub target_ids: Vec<String>,
}

fn default_time_groups() -> Vec<TimeGroup> {
    vec![
        TimeGroup { id: "morning".into(),   label: "Morning".into(),   start_hour: 6,  end_hour: 12, target_ids: vec![] },
        TimeGroup { id: "day".into(),       label: "Day".into(),       start_hour: 12, end_hour: 17, target_ids: vec![] },
        TimeGroup { id: "afternoon".into(), label: "Afternoon".into(), start_hour: 17, end_hour: 22, target_ids: vec![] },
        TimeGroup { id: "night".into(),     label: "Night".into(),     start_hour: 22, end_hour: 6,  target_ids: vec![] },
    ]
}

/// One saved Unsplash API key. `id` is an opaque stable identifier
/// (never the key itself); `label` is the user-visible name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyEntry {
    pub id: String,
    pub label: String,
    pub key: String,
}

/// Generate a unique id for a new API key entry.
/// No new dependencies: wall-clock nanos + a process-local counter.
pub fn new_api_key_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("key-{}-{}", nanos, n)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// 5-field cron expression: "min hour dom month dow"
    /// e.g. "0 * * * *" = every hour, "*/30 * * * *" = every 30 minutes
    #[serde(default = "default_wallpaper_cron")]
    pub wallpaper_cron: String,
    #[serde(default = "default_storage_limit")]
    pub storage_limit_mb: u64,
    #[serde(default = "default_orientation")]
    pub orientation: String,
    #[serde(default = "default_quality")]
    pub quality: String,
    #[serde(default)]
    pub getter_targets: Vec<GetterTarget>,
    #[serde(default = "default_time_groups")]
    pub time_groups: Vec<TimeGroup>,
    /// Legacy single-key storage. Kept for migration only: load_settings()
    /// moves it into `api_keys` (label "کلید ۱") and clears this field in
    /// memory, so the next save drops it from settings.json. Never write a
    /// new key here; use `api_keys` + `active_api_key_id`.
    #[serde(default)]
    pub api_key: Option<String>,
    /// All saved Unsplash API keys (manual switching only — no auto-rotation).
    #[serde(default)]
    pub api_keys: Vec<ApiKeyEntry>,
    /// Id of the entry in `api_keys` used for all Unsplash calls.
    #[serde(default)]
    pub active_api_key_id: Option<String>,
    /// Base URL of the Unsplash server the app talks to. Default is the
    /// Cloudflare relay (Unsplash is unreachable from Iran without a VPN).
    /// Editable in Settings, Sonora-style; an empty value falls back to
    /// the relay default — never silently to direct api.unsplash.com.
    #[serde(default = "default_api_base_url")]
    pub api_base_url: String,
    /// Page direction: "ltr" (the original upstream layout) or "rtl".
    /// Default "ltr"; the user picks RTL in Settings if wanted.
    #[serde(default = "default_layout_direction")]
    pub layout_direction: String,
}

fn default_wallpaper_cron() -> String { "0 * * * *".to_string() }
fn default_storage_limit() -> u64 { 500 }
fn default_orientation() -> String { "landscape".to_string() }
fn default_quality() -> String { "regular".to_string() }
fn default_api_base_url() -> String { crate::unsplash::RELAY_BASE.to_string() }
fn default_layout_direction() -> String { "ltr".to_string() }

/// Effective server base URL (never empty — falls back to the relay).
pub fn effective_api_base_url(settings: &Settings) -> String {
    let base = settings.api_base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        default_api_base_url()
    } else {
        base.to_string()
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            wallpaper_cron: default_wallpaper_cron(),
            storage_limit_mb: 500,
            orientation: "landscape".to_string(),
            quality: "regular".to_string(),
            getter_targets: vec![],
            time_groups: default_time_groups(),
            api_key: option_env!("UNSPLASH_ACCESS_KEY").map(|s| s.to_string()),
            api_keys: vec![],
            active_api_key_id: None,
            api_base_url: default_api_base_url(),
            layout_direction: "ltr".to_string(),
        }
    }
}

/// The API key entry currently in use: the entry matching
/// `active_api_key_id`, falling back to the first entry when no active id
/// is set (or it no longer exists). Returns None when no key is saved.
pub fn active_api_key(settings: &Settings) -> Option<&ApiKeyEntry> {
    if let Some(id) = &settings.active_api_key_id {
        if let Some(entry) = settings.api_keys.iter().find(|e| &e.id == id) {
            return Some(entry);
        }
    }
    settings.api_keys.first()
}

/// Id of the active key entry, resolved the same way as `active_api_key`.
pub fn active_api_key_id(settings: &Settings) -> Option<String> {
    active_api_key(settings).map(|e| e.id.clone())
}

pub fn load_settings() -> Result<Settings, String> {
    let path = paths::settings_file()?;
    let mut settings: Settings = if !path.exists() {
        Settings::default()
    } else {
        let bytes = std::fs::read(&path)
            .map_err(|e| format!("Failed to read settings.json: {}", e))?;
        serde_json::from_slice(&bytes)
            .map_err(|e| format!("Failed to parse settings.json: {}", e))?
    };

    // One-time migration: a legacy single key moves into `api_keys` as
    // "کلید ۱" and becomes active. The old `api_key` field is cleared in
    // memory here, so it disappears from settings.json on the next save.
    // Nothing is saved by load itself; an existing key is never lost.
    if settings.api_keys.is_empty() {
        if let Some(old_key) = settings.api_key.take() {
            let id = new_api_key_id();
            settings.api_keys.push(ApiKeyEntry {
                id: id.clone(),
                label: "کلید ۱".to_string(),
                key: old_key,
            });
            settings.active_api_key_id = Some(id);
        }
    }

    // Sanitize: the active id must point at a real entry; otherwise fall
    // back to the first entry (or None when the list is empty).
    if !settings.api_keys.is_empty() {
        let active_ok = settings
            .active_api_key_id
            .as_ref()
            .map(|id| settings.api_keys.iter().any(|e| &e.id == id))
            .unwrap_or(false);
        if !active_ok {
            settings.active_api_key_id = settings.api_keys.first().map(|e| e.id.clone());
        }
    } else {
        settings.active_api_key_id = None;
    }

    // Normalize the server address and direction: an empty address falls
    // back to the relay default (never to direct api.unsplash.com), and
    // anything but "rtl" means "ltr" (the original upstream layout).
    settings.api_base_url = effective_api_base_url(&settings);
    if settings.layout_direction != "rtl" {
        settings.layout_direction = "ltr".to_string();
    }

    // One-time migration: if all groups are empty but targets exist, populate all groups.
    if settings.time_groups.iter().all(|g| g.target_ids.is_empty())
        && !settings.getter_targets.is_empty()
    {
        let all_ids: Vec<String> = settings.getter_targets.iter().map(|t| t.id.clone()).collect();
        for group in &mut settings.time_groups {
            group.target_ids = all_ids.clone();
        }
        let _ = save_settings(&settings);
    }

    Ok(settings)
}

pub fn save_settings(settings: &Settings) -> Result<(), String> {
    let path = paths::settings_file()?;
    if let Some(parent) = path.parent() {
        dirs::ensure_dir(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    files::write_atomic(&path, &bytes)
}

/// Add a target ID to every time group (called when a new source is added).
pub fn add_target_to_all_groups(settings: &mut Settings, target_id: &str) {
    for group in &mut settings.time_groups {
        if !group.target_ids.iter().any(|id| id == target_id) {
            group.target_ids.push(target_id.to_string());
        }
    }
}

/// Remove a target ID from every time group (called when a source is deleted).
pub fn remove_target_from_all_groups(settings: &mut Settings, target_id: &str) {
    for group in &mut settings.time_groups {
        group.target_ids.retain(|id| id != target_id);
    }
}

/// Find which time group the given hour belongs to.
pub fn group_for_hour(time_groups: &[TimeGroup], hour: u32) -> Option<&TimeGroup> {
    time_groups.iter().find(|g| {
        if g.start_hour < g.end_hour {
            hour >= g.start_hour && hour < g.end_hour
        } else {
            // Wraps around midnight (e.g. night 22-06)
            hour >= g.start_hour || hour < g.end_hour
        }
    })
}
