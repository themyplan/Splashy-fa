use super::RELAY_BASE;
use super::models::{
    ApiCollection, ApiPhoto, ApiPhotoStatistics, ApiRelatedPhotosResult, ApiSearchPhotosResult,
    ApiTopic, ApiUserProfile,
};

pub struct UnsplashClient {
    pub(crate) inner: reqwest::Client,
    access_key: String,
    /// Id of the `ApiKeyEntry` this client authenticates with, if known.
    /// Used only to tag the recorded rate-limit status; None when the
    /// client was built without an entry id.
    key_id: Option<String>,
    /// Base URL of the Unsplash server (default: the Cloudflare relay).
    /// Comes from Settings (`api_base_url`), so the user can change the
    /// server address without a rebuild, Sonora-style.
    base_url: String,
}

impl UnsplashClient {
    pub fn new(access_key: impl Into<String>) -> Self {
        Self::new_with_id(access_key, None)
    }

    /// Build a client bound to a saved key entry, so the rate-limit headers
    /// of its responses can be attributed to that entry.
    pub fn new_with_id(access_key: impl Into<String>, key_id: Option<String>) -> Self {
        Self::new_with_id_and_base(access_key, key_id, RELAY_BASE)
    }

    /// Build a client bound to a saved key entry and a configured server
    /// base URL (Settings `api_base_url`; falls back to the relay default
    /// when empty).
    pub fn new_with_id_and_base(
        access_key: impl Into<String>,
        key_id: Option<String>,
        base_url: impl Into<String>,
    ) -> Self {
        let inner = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("Failed to build HTTP client");
        let base = base_url.into();
        let base = base.trim().trim_end_matches('/').to_string();
        let base = if base.is_empty() {
            RELAY_BASE.to_string()
        } else {
            base
        };
        Self {
            inner,
            access_key: access_key.into(),
            key_id,
            base_url: base,
        }
    }

    /// API endpoint URL against this client's configured base, e.g.
    /// api_url("photos/random") -> "<base>/api/photos/random".
    fn api_url(&self, path: &str) -> String {
        super::api_url_with_base(path, &self.base_url)
    }

    /// Rewrite an Unsplash URL (api.unsplash.com / images.unsplash.com)
    /// to this client's configured base. Idempotent.
    fn relay_url(&self, url: &str) -> String {
        super::relay_url_with_base(url, &self.base_url)
    }

    fn auth(&self) -> String {
        format!("Client-ID {}", self.access_key)
    }

    /// Capture X-Ratelimit-Limit / X-Ratelimit-Remaining from one
    /// api.unsplash.com response into the shared rate-limit state.
    /// Call immediately after receiving the response, before the body is
    /// consumed and before any status check, so error responses (403/429)
    /// are recorded too.
    fn record_rate_limit(&self, headers: &reqwest::header::HeaderMap) {
        crate::unsplash::record_rate_limit_headers(headers, self.key_id.clone());
    }

    fn check_status(status: reqwest::StatusCode, body: &str) -> Result<(), String> {
        if status.as_u16() == 429 {
            return Err("ERR_RATE_LIMITED: 429 Too Many Requests".to_string());
        }
        if status.as_u16() == 403 {
            return Err("ERR_RATE_LIMITED: 403 Forbidden".to_string());
        }
        if !status.is_success() {
            return Err(format!("Unsplash API error {}: {}", status, body));
        }
        Ok(())
    }

    /// GET /photos/random with arbitrary query params.
    pub async fn get_random_photo(&self, params: &[(&str, &str)]) -> Result<ApiPhoto, String> {
        let query_str = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");
        log::debug!(
            "[unsplash] GET {}?{}",
            self.api_url("photos/random"),
            query_str
        );

        let resp = self
            .inner
            .get(self.api_url("photos/random"))
            .query(params)
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| format!("ERR_NETWORK: {}", e))?;

        self.record_rate_limit(resp.headers());
        let status = resp.status();
        log::debug!("[unsplash] /photos/random -> {}", status);

        if status.as_u16() == 429 {
            return Err("ERR_RATE_LIMITED: 429 Too Many Requests".to_string());
        }
        if status.as_u16() == 403 {
            return Err("ERR_RATE_LIMITED: 403 Forbidden".to_string());
        }
        if !status.is_success() {
            let txt = resp.text().await.unwrap_or_default();
            log::warn!("[unsplash] /photos/random error body: {}", txt);
            return Err(format!("Unsplash API error {}: {}", status, txt));
        }

        resp.json::<ApiPhoto>()
            .await
            .map_err(|e| format!("Failed to parse photo JSON: {}", e))
    }

    /// Fire-and-forget download registration per Unsplash guidelines.
    pub async fn register_download(&self, url: &str) {
        // The download-location URL comes back from the API as an
        // api.unsplash.com URL; route it through the relay too.
        let url = self.relay_url(url);
        log::debug!("[unsplash] register_download: GET {}", url);
        if let Ok(resp) = self
            .inner
            .get(&url)
            .header("Authorization", self.auth())
            .send()
            .await
        {
            // The download-location endpoint is api.unsplash.com, so its
            // response carries rate-limit headers like any other API call.
            self.record_rate_limit(resp.headers());
        }
    }

    /// GET /collections/:id with optional ETag for conditional fetch.
    /// Returns `None` on 304 Not Modified, `Some((collection, new_etag))` on 200.
    pub async fn poll_collection(
        &self,
        id: &str,
        etag: Option<&str>,
    ) -> Result<Option<(ApiCollection, Option<String>)>, String> {
        let url = self.api_url(&format!("collections/{}", id));
        log::debug!("[unsplash] GET {} (etag={:?})", url, etag);

        let mut req = self.inner.get(&url).header("Authorization", self.auth());
        if let Some(tag) = etag {
            req = req.header("If-None-Match", tag);
        }
        let resp = req.send().await.map_err(|e| format!("ERR_NETWORK: {}", e))?;

        self.record_rate_limit(resp.headers());
        let status = resp.status();
        if status.as_u16() == 304 {
            log::debug!("[unsplash] GET {} -> 304 Not Modified", url);
            return Ok(None);
        }
        if status.as_u16() == 404 {
            return Err(format!("ERR_COLLECTION_NOT_FOUND: {}", id));
        }

        let new_etag = resp
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let body = resp.text().await.unwrap_or_default();
        Self::check_status(status, &body)?;

        let collection = serde_json::from_str::<ApiCollection>(&body)
            .map_err(|e| format!("Failed to parse collection JSON: {}", e))?;

        Ok(Some((collection, new_etag)))
    }

    /// GET /collections/:id
    pub async fn get_collection(&self, id: &str) -> Result<ApiCollection, String> {
        let url = self.api_url(&format!("collections/{}", id));
        log::debug!("[unsplash] GET {}", url);
        let resp = self
            .inner
            .get(&url)
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| format!("ERR_NETWORK: {}", e))?;

        self.record_rate_limit(resp.headers());
        let status = resp.status();
        log::debug!("[unsplash] GET {} -> {}", url, status);
        if status.as_u16() == 404 {
            return Err(format!("ERR_COLLECTION_NOT_FOUND: {}", id));
        }
        let body = resp.text().await.unwrap_or_default();
        Self::check_status(status, &body)?;

        serde_json::from_str::<ApiCollection>(&body)
            .map_err(|e| format!("Failed to parse collection JSON: {}", e))
    }

    /// GET /users/:username
    pub async fn get_user_profile(&self, username: &str) -> Result<ApiUserProfile, String> {
        let url = self.api_url(&format!("users/{}", username));
        log::debug!("[unsplash] GET {}", url);
        let resp = self
            .inner
            .get(&url)
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| format!("ERR_NETWORK: {}", e))?;

        self.record_rate_limit(resp.headers());
        let status = resp.status();
        log::debug!("[unsplash] GET {} -> {}", url, status);
        if status.as_u16() == 404 {
            return Err(format!("ERR_USER_NOT_FOUND: {}", username));
        }
        let body = resp.text().await.unwrap_or_default();
        Self::check_status(status, &body)?;

        serde_json::from_str::<ApiUserProfile>(&body)
            .map_err(|e| format!("Failed to parse user profile JSON: {}", e))
    }

    /// GET /topics — returns up to 30 topics.
    pub async fn get_topics(&self) -> Result<Vec<ApiTopic>, String> {
        log::debug!("[unsplash] GET {}?per_page=30", self.api_url("topics"));
        let resp = self
            .inner
            .get(self.api_url("topics"))
            .query(&[("per_page", "30")])
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| format!("ERR_NETWORK: {}", e))?;

        self.record_rate_limit(resp.headers());
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Self::check_status(status, &body)?;

        serde_json::from_str::<Vec<ApiTopic>>(&body)
            .map_err(|e| format!("Failed to parse topics JSON: {}", e))
    }

    /// GET /photos — list a page of photos.
    pub async fn list_photos(
        &self,
        page: u32,
        per_page: u32,
        order_by: &str,
    ) -> Result<Vec<ApiPhoto>, String> {
        log::debug!("[unsplash] GET /photos?page={}&per_page={}&order_by={}", page, per_page, order_by);
        let resp = self
            .inner
            .get(self.api_url("photos"))
            .query(&[
                ("page", page.to_string()),
                ("per_page", per_page.to_string()),
                ("order_by", order_by.to_string()),
            ])
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| format!("ERR_NETWORK: {}", e))?;

        self.record_rate_limit(resp.headers());
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Self::check_status(status, &body)?;

        serde_json::from_str::<Vec<ApiPhoto>>(&body)
            .map_err(|e| format!("Failed to parse photos JSON: {}", e))
    }

    /// GET /photos/:photo_id — retrieve a single photo by ID.
    pub async fn get_photo(&self, photo_id: &str) -> Result<ApiPhoto, String> {
        let url = self.api_url(&format!("photos/{}", photo_id));
        log::debug!("[unsplash] GET {}", url);
        let resp = self
            .inner
            .get(&url)
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| format!("ERR_NETWORK: {}", e))?;

        self.record_rate_limit(resp.headers());
        let status = resp.status();
        if status.as_u16() == 404 {
            return Err(format!("Photo not found: {}", photo_id));
        }
        let body = resp.text().await.unwrap_or_default();
        Self::check_status(status, &body)?;

        serde_json::from_str::<ApiPhoto>(&body)
            .map_err(|e| format!("Failed to parse photo JSON: {}", e))
    }

    /// GET /photos/:photo_id/statistics
    pub async fn get_photo_statistics(
        &self,
        photo_id: &str,
        resolution: &str,
        quantity: u32,
    ) -> Result<ApiPhotoStatistics, String> {
        let url = self.api_url(&format!("photos/{}/statistics", photo_id));
        log::debug!("[unsplash] GET {}?resolution={}&quantity={}", url, resolution, quantity);
        let resp = self
            .inner
            .get(&url)
            .query(&[
                ("resolution", resolution.to_string()),
                ("quantity", quantity.to_string()),
            ])
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| format!("ERR_NETWORK: {}", e))?;

        self.record_rate_limit(resp.headers());
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Self::check_status(status, &body)?;

        serde_json::from_str::<ApiPhotoStatistics>(&body)
            .map_err(|e| format!("Failed to parse photo statistics JSON: {}", e))
    }

    /// GET /photos/:photo_id/download — track a download per Unsplash guidelines.
    pub async fn track_photo_download(&self, photo_id: &str) -> Result<(), String> {
        let url = self.api_url(&format!("photos/{}/download", photo_id));
        log::debug!("[unsplash] GET {} (track download)", url);
        let resp = self
            .inner
            .get(&url)
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| format!("ERR_NETWORK: {}", e))?;

        self.record_rate_limit(resp.headers());
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Track download failed {}: {}", status, body));
        }
        Ok(())
    }

    /// GET /search/photos
    pub async fn search_photos(
        &self,
        query: &str,
        page: u32,
        per_page: u32,
        orientation: Option<&str>,
        collections: Option<&str>,
        color: Option<&str>,
        content_filter: &str,
    ) -> Result<ApiSearchPhotosResult, String> {
        log::debug!("[unsplash] GET /search/photos?query={}&page={}&per_page={}", query, page, per_page);
        let mut params: Vec<(&str, String)> = vec![
            ("query", query.to_string()),
            ("page", page.to_string()),
            ("per_page", per_page.to_string()),
            ("content_filter", content_filter.to_string()),
        ];
        if let Some(o) = orientation {
            params.push(("orientation", o.to_string()));
        }
        if let Some(c) = collections {
            params.push(("collections", c.to_string()));
        }
        if let Some(c) = color {
            params.push(("color", c.to_string()));
        }

        let resp = self
            .inner
            .get(self.api_url("search/photos"))
            .query(&params)
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| format!("ERR_NETWORK: {}", e))?;

        self.record_rate_limit(resp.headers());
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Self::check_status(status, &body)?;

        serde_json::from_str::<ApiSearchPhotosResult>(&body)
            .map_err(|e| format!("Failed to parse search result JSON: {}", e))
    }

    /// GET /collections — list public collections (paginated).
    pub async fn list_collections_api(
        &self,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<ApiCollection>, String> {
        log::debug!("[unsplash] GET /collections?page={}&per_page={}", page, per_page);
        let resp = self
            .inner
            .get(self.api_url("collections"))
            .query(&[("page", page.to_string()), ("per_page", per_page.to_string())])
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| format!("ERR_NETWORK: {}", e))?;

        self.record_rate_limit(resp.headers());
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Self::check_status(status, &body)?;

        serde_json::from_str::<Vec<ApiCollection>>(&body)
            .map_err(|e| format!("Failed to parse collections JSON: {}", e))
    }

    /// GET /collections/:id/photos
    pub async fn get_collection_photos(
        &self,
        collection_id: &str,
        page: u32,
        per_page: u32,
        orientation: Option<&str>,
    ) -> Result<Vec<ApiPhoto>, String> {
        let url = self.api_url(&format!("collections/{}/photos", collection_id));
        log::debug!("[unsplash] GET {}?page={}&per_page={}", url, page, per_page);
        let mut params = vec![
            ("page", page.to_string()),
            ("per_page", per_page.to_string()),
        ];
        if let Some(o) = orientation {
            params.push(("orientation", o.to_string()));
        }

        let resp = self
            .inner
            .get(&url)
            .query(&params)
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| format!("ERR_NETWORK: {}", e))?;

        self.record_rate_limit(resp.headers());
        let status = resp.status();
        if status.as_u16() == 404 {
            return Err(format!("ERR_COLLECTION_NOT_FOUND: {}", collection_id));
        }
        let body = resp.text().await.unwrap_or_default();
        Self::check_status(status, &body)?;

        serde_json::from_str::<Vec<ApiPhoto>>(&body)
            .map_err(|e| format!("Failed to parse collection photos JSON: {}", e))
    }

    /// GET /users/:username/photos
    pub async fn get_user_photos(
        &self,
        username: &str,
        page: u32,
        per_page: u32,
        order_by: &str,
        orientation: Option<&str>,
    ) -> Result<Vec<ApiPhoto>, String> {
        let url = self.api_url(&format!("users/{}/photos", username));
        log::debug!("[unsplash] GET {}?page={}&per_page={}&order_by={}", url, page, per_page, order_by);
        let mut params = vec![
            ("page", page.to_string()),
            ("per_page", per_page.to_string()),
            ("order_by", order_by.to_string()),
        ];
        if let Some(o) = orientation {
            params.push(("orientation", o.to_string()));
        }

        let resp = self
            .inner
            .get(&url)
            .query(&params)
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| format!("ERR_NETWORK: {}", e))?;

        self.record_rate_limit(resp.headers());
        let status = resp.status();
        if status.as_u16() == 404 {
            return Err(format!("ERR_USER_NOT_FOUND: {}", username));
        }
        let body = resp.text().await.unwrap_or_default();
        Self::check_status(status, &body)?;

        serde_json::from_str::<Vec<ApiPhoto>>(&body)
            .map_err(|e| format!("Failed to parse user photos JSON: {}", e))
    }

    /// GET /users/:username/collections
    pub async fn get_user_collections(
        &self,
        username: &str,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<ApiCollection>, String> {
        let url = self.api_url(&format!("users/{}/collections", username));
        log::debug!("[unsplash] GET {}?page={}&per_page={}", url, page, per_page);
        let resp = self
            .inner
            .get(&url)
            .query(&[("page", page.to_string()), ("per_page", per_page.to_string())])
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| format!("ERR_NETWORK: {}", e))?;

        self.record_rate_limit(resp.headers());
        let status = resp.status();
        if status.as_u16() == 404 {
            return Err(format!("ERR_USER_NOT_FOUND: {}", username));
        }
        let body = resp.text().await.unwrap_or_default();
        Self::check_status(status, &body)?;

        serde_json::from_str::<Vec<ApiCollection>>(&body)
            .map_err(|e| format!("Failed to parse user collections JSON: {}", e))
    }

    /// GET /users/:username/likes
    pub async fn get_user_liked_photos(
        &self,
        username: &str,
        page: u32,
        per_page: u32,
        order_by: &str,
        orientation: Option<&str>,
    ) -> Result<Vec<ApiPhoto>, String> {
        let url = self.api_url(&format!("users/{}/likes", username));
        log::debug!("[unsplash] GET {}?page={}&per_page={}&order_by={}", url, page, per_page, order_by);
        let mut params = vec![
            ("page", page.to_string()),
            ("per_page", per_page.to_string()),
            ("order_by", order_by.to_string()),
        ];
        if let Some(o) = orientation {
            params.push(("orientation", o.to_string()));
        }

        let resp = self
            .inner
            .get(&url)
            .query(&params)
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| format!("ERR_NETWORK: {}", e))?;

        self.record_rate_limit(resp.headers());
        let status = resp.status();
        if status.as_u16() == 404 {
            return Err(format!("ERR_USER_NOT_FOUND: {}", username));
        }
        let body = resp.text().await.unwrap_or_default();
        Self::check_status(status, &body)?;

        serde_json::from_str::<Vec<ApiPhoto>>(&body)
            .map_err(|e| format!("Failed to parse user liked photos JSON: {}", e))
    }

    /// GET /topics — paginated with ordering.
    pub async fn list_topics_paged(
        &self,
        page: u32,
        per_page: u32,
        order_by: &str,
    ) -> Result<Vec<ApiTopic>, String> {
        log::debug!("[unsplash] GET /topics?page={}&per_page={}&order_by={}", page, per_page, order_by);
        let resp = self
            .inner
            .get(self.api_url("topics"))
            .query(&[
                ("page", page.to_string()),
                ("per_page", per_page.to_string()),
                ("order_by", order_by.to_string()),
            ])
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| format!("ERR_NETWORK: {}", e))?;

        self.record_rate_limit(resp.headers());
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Self::check_status(status, &body)?;

        serde_json::from_str::<Vec<ApiTopic>>(&body)
            .map_err(|e| format!("Failed to parse topics JSON: {}", e))
    }

    /// GET /topics/:id_or_slug/photos
    pub async fn get_topic_photos(
        &self,
        id_or_slug: &str,
        page: u32,
        per_page: u32,
        order_by: &str,
        orientation: Option<&str>,
    ) -> Result<Vec<ApiPhoto>, String> {
        let url = self.api_url(&format!("topics/{}/photos", id_or_slug));
        log::debug!("[unsplash] GET {}?page={}&per_page={}&order_by={}", url, page, per_page, order_by);
        let mut params = vec![
            ("page", page.to_string()),
            ("per_page", per_page.to_string()),
            ("order_by", order_by.to_string()),
        ];
        if let Some(o) = orientation {
            params.push(("orientation", o.to_string()));
        }

        let resp = self
            .inner
            .get(&url)
            .query(&params)
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| format!("ERR_NETWORK: {}", e))?;

        self.record_rate_limit(resp.headers());
        let status = resp.status();
        if status.as_u16() == 404 {
            return Err(format!("Topic not found: {}", id_or_slug));
        }
        let body = resp.text().await.unwrap_or_default();
        Self::check_status(status, &body)?;

        serde_json::from_str::<Vec<ApiPhoto>>(&body)
            .map_err(|e| format!("Failed to parse topic photos JSON: {}", e))
    }

    /// GET /photos/:photo_id/related?page=<N> — returns related photos.
    /// Returns Err containing "ERR_BAD_PAGE" on 400 (page out of range).
    pub async fn get_related_photos(
        &self,
        photo_id: &str,
        page: u32,
    ) -> Result<ApiRelatedPhotosResult, String> {
        let url = self.api_url(&format!("photos/{}/related", photo_id));
        log::debug!("[unsplash] GET {}?page={}", url, page);
        let resp = self
            .inner
            .get(&url)
            .query(&[("page", page.to_string())])
            .header("Authorization", self.auth())
            .send()
            .await
            .map_err(|e| format!("ERR_NETWORK: {}", e))?;

        self.record_rate_limit(resp.headers());
        let status = resp.status();
        if status.as_u16() == 400 {
            return Err(format!("ERR_BAD_PAGE: 400 for page {}", page));
        }
        let body = resp.text().await.unwrap_or_default();
        Self::check_status(status, &body)?;

        serde_json::from_str::<ApiRelatedPhotosResult>(&body)
            .map_err(|e| format!("Failed to parse related photos JSON: {}", e))
    }

    /// Download raw image bytes from any URL (no auth header needed).
    /// Returns (bytes, content_type).
    /// NOTE: image downloads come from images.unsplash.com and do NOT count
    /// against the API rate limit, so no rate-limit headers are recorded here.
    pub async fn download_image(&self, url: &str) -> Result<(Vec<u8>, String), String> {
        // Image URLs come back from the API as images.unsplash.com URLs;
        // fetch them through the relay (/img) so they load without a VPN.
        let url = self.relay_url(url);
        let resp = self
            .inner
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to download image: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Image download failed: status {}", resp.status()));
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("Failed to read image bytes: {}", e))?
            .to_vec();

        Ok((bytes, content_type))
    }
}
