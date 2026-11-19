pub mod client;
pub mod models;

pub use client::UnsplashClient;
pub use models::*;

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, OnceLock};

/// Latest Unsplash rate-limit numbers for the active key, taken from the
/// `X-Ratelimit-Limit` / `X-Ratelimit-Remaining` response headers.
/// `key_id` is the id of the `ApiKeyEntry` the numbers belong to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitStatus {
    pub key_id: Option<String>,
    pub limit: u32,
    pub remaining: u32,
}

/// Tauri managed state holding the latest `RateLimitStatus` (None until the
/// first api.unsplash.com call with the current active key). It is the same
/// shared value the client writes to (see `rate_limit_state()`), registered
/// in lib.rs with `.manage(crate::unsplash::rate_limit_state())` and read
/// back by the `get_rate_limit_status` command via `State`.
pub type RateLimitState = Arc<Mutex<Option<RateLimitStatus>>>;

/// Process-wide shared rate-limit slot. The Unsplash client has no
/// AppHandle, so it writes here; lib.rs registers this same value as
/// Tauri managed state so commands can read it through `State`.
pub fn rate_limit_state() -> RateLimitState {
    static STATE: OnceLock<RateLimitState> = OnceLock::new();
    STATE
        .get_or_init(|| Arc::new(Mutex::new(None)))
        .clone()
}

/// Parse the rate-limit headers of one api.unsplash.com response and store
/// them. Silently does nothing when either header is missing or unparsable
/// (e.g. a non-Unsplash host). Called for every response, including
/// 403/429 errors, so the counter reflects the latest server state.
pub fn record_rate_limit_headers(
    headers: &reqwest::header::HeaderMap,
    key_id: Option<String>,
) {
    let limit = headers
        .get("x-ratelimit-limit")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u32>().ok());
    let remaining = headers
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u32>().ok());
    if let (Some(limit), Some(remaining)) = (limit, remaining) {
        if let Ok(mut slot) = rate_limit_state().lock() {
            *slot = Some(RateLimitStatus {
                key_id,
                limit,
                remaining,
            });
        }
    }
}

/// Clear the stored rate-limit status. Called whenever the active key
/// changes, so the old key's numbers are never shown for the new key.
pub fn reset_rate_limit() {
    if let Ok(mut slot) = rate_limit_state().lock() {
        *slot = None;
    }
}

/// Build a client using the ACTIVE API key from settings.
/// This is the single funnel every Unsplash call goes through — no other
/// code builds an UnsplashClient directly. Returns an error if no key has
/// been configured yet. Manual key switching only: the caller never picks
/// a different key on failure.
pub fn client_from_active_key() -> Result<UnsplashClient, String> {
    let settings = crate::settings::load_settings()?;
    let entry = crate::settings::active_api_key(&settings).ok_or_else(|| {
        "ERR_NO_API_KEY: No Unsplash API key configured. Please add your key in Settings.".to_string()
    })?;
    Ok(UnsplashClient::new_with_id(
        entry.key.clone(),
        Some(entry.id.clone()),
    ))
}

/// Backwards-compatible alias for `client_from_active_key`.
/// Kept so any caller missed in the reroute still resolves the active key.
pub fn get_client() -> Result<UnsplashClient, String> {
    client_from_active_key()
}
