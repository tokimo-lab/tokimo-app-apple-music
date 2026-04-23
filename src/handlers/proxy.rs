//! `/proxy` — Apple Music API 透传。

use axum::{Json, extract::State};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::error::AppError;

use super::{
    AppCaller, AppCtx, USER_AGENT, cache_catalog_response, cache_webplayback_response, extract_refreshed_token,
    get_developer_token, is_allowed_apple_host, read_user_music_token, save_user_music_token,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyRequest {
    #[serde(default)]
    pub target_url: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub params: HashMap<String, String>,
    #[serde(default)]
    pub body: Option<serde_json::Value>,
    #[serde(default)]
    pub music_user_token: Option<String>,
}

pub async fn proxy_apple_music_api(
    State(ctx): State<Arc<AppCtx>>,
    caller: AppCaller,
    Json(body): Json<ProxyRequest>,
) -> Result<axum::response::Response, AppError> {
    let url = if let Some(ref target) = body.target_url {
        let parsed = reqwest::Url::parse(target).map_err(|_| AppError::bad_request("Invalid target URL"))?;
        let host = parsed.host_str().unwrap_or("");
        if !is_allowed_apple_host(host) {
            return Err(AppError::bad_request(format!("Proxy not allowed for host: {host}")));
        }
        target.clone()
    } else if let Some(ref path) = body.path {
        format!("https://api.music.apple.com{path}")
    } else {
        return Err(AppError::bad_request("Either targetUrl or path is required"));
    };

    let dev_token = get_developer_token().await?;
    let stored_token = read_user_music_token(&ctx.openapi, &caller.cookie_header).await?;
    let music_user_token = stored_token.or_else(|| body.music_user_token.as_ref().filter(|t| !t.is_empty()).cloned());

    let method = body.method.as_deref().unwrap_or("GET").to_uppercase();
    debug!("[AppleMusic] Proxy → {method} {}", &url[..url.len().min(120)]);

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| AppError::Internal(format!("HTTP client error: {e}")))?;

    let mut req = match method.as_str() {
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        "PATCH" => client.patch(&url),
        _ => client.get(&url),
    };

    req = req
        .header("Authorization", format!("Bearer {dev_token}"))
        .header("Origin", "https://music.apple.com")
        .header("Referer", "https://music.apple.com/");

    if let Some(ref token) = music_user_token {
        req = req
            .header("Media-User-Token", token)
            .header(reqwest::header::COOKIE, format!("media-user-token={token}"));
    }

    for (key, value) in &body.params {
        req = req.query(&[(key.as_str(), value.as_str())]);
    }

    if let Some(ref req_body) = body.body {
        req = req.json(req_body);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Apple Music API error: {e}")))?;

    let status = resp.status().as_u16();

    if url.contains("webPlayback") {
        debug!(
            "[AppleMusic] Proxy webPlayback: status={status}, body={:?}",
            body.body.as_ref().map(std::string::ToString::to_string)
        );
    }
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();

    if status == 200
        && let Some(refreshed) = extract_refreshed_token(resp.headers())
    {
        let is_new = music_user_token.as_deref() != Some(refreshed.as_str());
        if is_new {
            info!("[AppleMusic] Token auto-refreshed by Apple — updating stored token");
            let openapi = ctx.openapi.clone();
            let cookie = caller.cookie_header.clone();
            tokio::spawn(async move {
                if let Err(e) = save_user_music_token(&openapi, &cookie, &refreshed).await {
                    warn!("[AppleMusic] Failed to save refreshed token: {e}");
                }
            });
        }
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to read response: {e}")))?;

    if status == 200 && url.contains("webPlayback") {
        cache_webplayback_response(&body.body, &bytes);
    }
    if status == 200 && url.contains("/songs/") && url.contains("catalog") {
        cache_catalog_response(&url, &bytes);
    }

    if status == 403 && music_user_token.is_some() {
        warn!("[AppleMusic] Apple returned 403 — music-user-token may be expired");
    }

    let mut builder = axum::response::Response::builder()
        .status(status)
        .header("content-type", content_type);

    if (status == 401 || status == 403) && music_user_token.is_some() {
        builder = builder.header("x-apple-music-token-expired", "true");
    }

    Ok(builder.body(axum::body::Body::from(bytes)).unwrap())
}
