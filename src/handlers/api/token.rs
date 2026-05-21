//! Apple Music developer token scraping + caching.

use std::sync::OnceLock;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::error::AppError;

const APPLE_MUSIC_URL: &str = "https://music.apple.com/us/browse";
pub const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const TOKEN_TTL_SECS: u64 = 3600;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Clone)]
struct CachedToken {
    token: String,
    fetched_at: std::time::Instant,
}

static TOKEN_CACHE: OnceLock<RwLock<Option<CachedToken>>> = OnceLock::new();

fn cache() -> &'static RwLock<Option<CachedToken>> {
    TOKEN_CACHE.get_or_init(|| RwLock::new(None))
}

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

fn token_cache_path() -> std::path::PathBuf {
    std::env::temp_dir().join("tokimo-apple-music-dev-token")
}

/// Clear both in-memory and file token caches, forcing a fresh scrape on next call.
pub async fn invalidate_developer_token() {
    {
        let mut guard = cache().write().await;
        *guard = None;
    }
    let _ = std::fs::remove_file(token_cache_path());
    info!("[AppleMusic] Developer token cache invalidated");
}

pub async fn get_developer_token() -> Result<String, AppError> {
    // 1. In-memory cache (fast path for server mode)
    {
        let guard = cache().read().await;
        if let Some(cached) = guard.as_ref()
            && cached.fetched_at.elapsed().as_secs() < TOKEN_TTL_SECS
        {
            return Ok(cached.token.clone());
        }
    }

    // 2. File cache (survives across CLI invocations)
    let cache_path = token_cache_path();
    if let Ok(content) = std::fs::read_to_string(&cache_path)
        && let Some((ts_str, token)) = content.split_once('\n')
        && let Ok(ts) = ts_str.parse::<u64>()
        && ts + TOKEN_TTL_SECS > now_secs()
    {
        // Refresh in-memory cache too
        let mut guard = cache().write().await;
        *guard = Some(CachedToken {
            token: token.to_string(),
            fetched_at: std::time::Instant::now() - std::time::Duration::from_secs(now_secs().saturating_sub(ts)),
        });
        return Ok(token.to_string());
    }

    // 3. Scrape from Apple
    debug!("[AppleMusic] Scraping fresh developer token...");
    let token = scrape_developer_token().await?;
    debug!("[AppleMusic] Token obtained ({} chars)", token.len());

    // Write to file cache
    let _ = std::fs::write(&cache_path, format!("{}\n{}", now_secs(), token));

    {
        let mut guard = cache().write().await;
        *guard = Some(CachedToken {
            token: token.clone(),
            fetched_at: std::time::Instant::now(),
        });
    }

    Ok(token)
}

/// Retry an Apple Music API call once on 401 (expired dev token).
pub async fn call_with_dev_token_retry<T, E, F, Fut>(description: &str, mut call: F) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    match call().await {
        Ok(v) => Ok(v),
        Err(e) if e.to_string().contains("401") => {
            info!("[AppleMusic] {description} got 401 — refreshing dev token and retrying");
            invalidate_developer_token().await;
            call().await
        }
        Err(e) => Err(e),
    }
}
