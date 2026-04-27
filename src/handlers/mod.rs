//! 业务 handlers — 完全镜像主 server `apps/apple_music/handlers/*` 的对外 API，
//! 但是：
//! - 鉴权从 cookie 提取改为读 server 注入的 `x-tokimo-user-id` header
//! - DB 访问全部经过 `OpenApiClient` → server `/openapi/user/preferences/*`
//! - 不依赖 `crate::AppState`，自己持有 `AppCtx`（无 db pool）

pub mod audio;
pub mod auth;
pub mod proxy;

use std::sync::{Arc, OnceLock};

use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, request::Parts},
};
use std::collections::HashMap;
use tokio::sync::RwLock;
use tracing::{debug, info};
use uuid::Uuid;

use rust_apple_music::AudioQuality;
use tokimo_bus_client::BusClient;

use crate::error::AppError;
use crate::openapi_client::OpenApiClient;

pub struct AppCtx {
    pub openapi: Arc<OpenApiClient>,
    #[allow(dead_code)]
    pub client: Arc<OnceLock<Arc<BusClient>>>,
}

// ── Per-request extractor：用 server 注入的 header 拿 user_id + cookie ────────

/// 从入站请求头提取：
/// - `x-tokimo-user-id`：server 反代时强制注入的可信 user id
/// - `Cookie`：透传给 `/openapi/*` 用于 server 端鉴权
pub struct AppCaller {
    pub user_id: String,
    pub cookie_header: String,
}

impl<S> FromRequestParts<S> for AppCaller
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let user_id = parts
            .headers
            .get("x-tokimo-user-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
            .ok_or_else(|| AppError::Unauthorized("missing x-tokimo-user-id".into()))?;
        let cookie_header = collect_cookie_header(&parts.headers);
        Ok(Self { user_id, cookie_header })
    }
}

fn collect_cookie_header(headers: &HeaderMap) -> String {
    headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect::<Vec<_>>()
        .join("; ")
}

pub fn parse_user_id(raw: &str) -> Result<Uuid, AppError> {
    raw.parse::<Uuid>()
        .map_err(|_| AppError::bad_request("Invalid user ID"))
}

// ── Cached developer token ────────────────────────────────────────────────────

#[derive(Clone)]
struct CachedToken {
    token: String,
    fetched_at: std::time::Instant,
}

const TOKEN_TTL_SECS: u64 = 3600;

static TOKEN_CACHE: std::sync::OnceLock<RwLock<Option<CachedToken>>> = std::sync::OnceLock::new();

fn cache() -> &'static RwLock<Option<CachedToken>> {
    TOKEN_CACHE.get_or_init(|| RwLock::new(None))
}

// ── Webplayback stream cache ──────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct CachedStreamInfo {
    pub stream_url: String,
    pub fetched_at: std::time::Instant,
}

static STREAM_CACHE: std::sync::OnceLock<RwLock<HashMap<String, CachedStreamInfo>>> = std::sync::OnceLock::new();

pub fn stream_cache() -> &'static RwLock<HashMap<String, CachedStreamInfo>> {
    STREAM_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

pub fn cache_webplayback_response(request_body: &Option<serde_json::Value>, response_bytes: &[u8]) {
    let track_id = request_body
        .as_ref()
        .and_then(|b| b.get("salableAdamId"))
        .and_then(|v| match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        });

    let Some(track_id) = track_id else { return };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(response_bytes) else {
        return;
    };
    let Some(assets) = json.pointer("/songList/0/assets").and_then(|v| v.as_array()) else {
        return;
    };

    let url = assets
        .iter()
        .find(|a| a.get("flavor").and_then(|f| f.as_str()) == Some("28:ctrp256"))
        .or_else(|| assets.first())
        .and_then(|a| a.get("URL").and_then(|u| u.as_str()));

    if let Some(url) = url {
        info!(
            "[AppleMusic] Cached webplayback stream for track {track_id}: {}...",
            &url[..url.len().min(60)]
        );
        let entry = CachedStreamInfo {
            stream_url: url.to_string(),
            fetched_at: std::time::Instant::now(),
        };
        let track_id_owned = track_id;
        tokio::spawn(async move {
            stream_cache().write().await.insert(track_id_owned, entry);
        });
    }
}

pub fn cache_catalog_response(url: &str, response_bytes: &[u8]) {
    let track_id = url
        .rsplit('/')
        .next()
        .and_then(|s| s.split('?').next())
        .map(std::string::ToString::to_string);

    let Some(track_id) = track_id else { return };
    if !track_id.chars().all(|c| c.is_ascii_digit()) {
        return;
    }
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(response_bytes) else {
        return;
    };
    let hls_url = json
        .pointer("/data/0/attributes/extendedAssetUrls/enhancedHls")
        .and_then(|v| v.as_str());
    if let Some(hls_url) = hls_url {
        info!("[AppleMusic] Cached catalog enhancedHls for track {track_id}");
        let entry = CachedStreamInfo {
            stream_url: hls_url.to_string(),
            fetched_at: std::time::Instant::now(),
        };
        let track_id_owned = track_id;
        tokio::spawn(async move {
            stream_cache().write().await.insert(track_id_owned, entry);
        });
    }
}

// ── Constants ─────────────────────────────────────────────────────────────────

pub const APPLE_MUSIC_URL: &str = "https://music.apple.com/us/browse";
pub const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

pub const APPLE_MUSIC_PREF_SCOPE: &str = "component";
pub const APPLE_MUSIC_PREF_SCOPE_ID: &str = "apple-music-auth";
pub const APPLE_MUSIC_PREF_TOKEN_KEY: &str = "appleMusicToken";
pub const APPLE_MUSIC_SETTINGS_SCOPE_ID: &str = "apple-music-settings";
pub const APPLE_MUSIC_QUALITY_KEY: &str = "audioQuality";

pub const ALLOWED_APPLE_HOSTS: &[&str] = &[
    "api.music.apple.com",
    "amp-api.music.apple.com",
    "amp-api-edge.music.apple.com",
    "universal-activity-service.itunes.apple.com",
    "play.itunes.apple.com",
    "buy.itunes.apple.com",
];

pub fn is_allowed_apple_host(host: &str) -> bool {
    ALLOWED_APPLE_HOSTS.contains(&host) || host.ends_with(".mzstatic.com")
}

// ── Token scraping ────────────────────────────────────────────────────────────

async fn scrape_developer_token() -> Result<String, AppError> {
    use regex::Regex;

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| AppError::Internal(format!("HTTP client error: {e}")))?;

    let html = client
        .get(APPLE_MUSIC_URL)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to fetch Apple Music page: {e}")))?
        .text()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to read page body: {e}")))?;

    let js_path_re = Regex::new(r"/assets/index[~-][a-zA-Z0-9._-]+\.js")
        .map_err(|e| AppError::Internal(format!("Regex error: {e}")))?;

    let js_path = js_path_re
        .find(&html)
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| AppError::Internal("Could not find JS bundle path in Apple Music page".into()))?;

    let js_url = format!("https://music.apple.com{js_path}");
    debug!("[AppleMusic] Fetching JS bundle: {js_url}");

    let js_content = client
        .get(&js_url)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to fetch JS bundle: {e}")))?
        .text()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to read JS bundle: {e}")))?;

    let token_re = Regex::new(r#""(eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6[^"]+)""#)
        .map_err(|e| AppError::Internal(format!("Regex error: {e}")))?;

    if let Some(cap) = token_re.captures(&js_content) {
        return Ok(cap[1].to_string());
    }

    let jwt_re = Regex::new(r#""(eyJ[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{20,})""#)
        .map_err(|e| AppError::Internal(format!("Regex error: {e}")))?;

    if let Some(cap) = jwt_re.captures(&js_content) {
        return Ok(cap[1].to_string());
    }

    Err(AppError::Internal(
        "Could not extract developer token from Apple Music JS bundle".into(),
    ))
}

pub async fn get_developer_token() -> Result<String, AppError> {
    {
        let guard = cache().read().await;
        if let Some(cached) = guard.as_ref()
            && cached.fetched_at.elapsed().as_secs() < TOKEN_TTL_SECS
        {
            return Ok(cached.token.clone());
        }
    }

    debug!("[AppleMusic] Scraping fresh developer token...");
    let token = scrape_developer_token().await?;
    debug!("[AppleMusic] Token obtained ({} chars)", token.len());

    {
        let mut guard = cache().write().await;
        *guard = Some(CachedToken {
            token: token.clone(),
            fetched_at: std::time::Instant::now(),
        });
    }

    Ok(token)
}

// ── User preferences helpers（走 OpenApi）──────────────────────────────────────

pub async fn read_user_music_token(openapi: &OpenApiClient, cookie_header: &str) -> Result<Option<String>, AppError> {
    let value = openapi
        .pref_get(cookie_header, APPLE_MUSIC_PREF_SCOPE, APPLE_MUSIC_PREF_SCOPE_ID)
        .await?;
    Ok(value
        .and_then(|v| {
            v.get(APPLE_MUSIC_PREF_TOKEN_KEY)
                .and_then(|t| t.as_str().map(str::to_owned))
        })
        .filter(|s| !s.is_empty()))
}

pub async fn save_user_music_token(openapi: &OpenApiClient, cookie_header: &str, token: &str) -> Result<(), AppError> {
    let value = serde_json::json!({ APPLE_MUSIC_PREF_TOKEN_KEY: token });
    openapi
        .pref_put(cookie_header, APPLE_MUSIC_PREF_SCOPE, APPLE_MUSIC_PREF_SCOPE_ID, value)
        .await
}

pub fn extract_refreshed_token(headers: &reqwest::header::HeaderMap) -> Option<String> {
    for value in &headers.get_all("set-cookie") {
        let Ok(s) = value.to_str() else { continue };
        if let Some(rest) = s.strip_prefix("media-user-token=") {
            let token = rest.split(';').next().unwrap_or("").trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

pub async fn read_user_audio_quality(openapi: &OpenApiClient, cookie_header: &str) -> Result<AudioQuality, AppError> {
    let value = openapi
        .pref_get(cookie_header, APPLE_MUSIC_PREF_SCOPE, APPLE_MUSIC_SETTINGS_SCOPE_ID)
        .await?;
    Ok(value
        .and_then(|v| {
            v.get(APPLE_MUSIC_QUALITY_KEY)
                .and_then(|q| q.as_str().map(AudioQuality::from_str_loose))
        })
        .unwrap_or_default())
}
