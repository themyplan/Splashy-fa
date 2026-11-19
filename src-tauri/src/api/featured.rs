use crate::settings::load_settings;
use serde::Serialize;

/// The built-in "featured" source (Unsplash's hand-picked photos).
/// A single on/off switch, not a list: the target itself lives in
/// `getter_targets` (kind "featured", disabled by default) and the
/// Active Sources modal renders it as a card like the other sources.
#[derive(Debug, Serialize)]
pub struct FeaturedSource {
    pub enabled: bool,
}

#[tauri::command]
pub async fn get_featured_source() -> Result<FeaturedSource, String> {
    let settings = load_settings()?;
    let enabled = settings
        .getter_targets
        .iter()
        .any(|t| t.kind == "featured" && t.enabled);
    Ok(FeaturedSource { enabled })
}
