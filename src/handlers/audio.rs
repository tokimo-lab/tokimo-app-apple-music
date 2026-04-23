//! `/get-key` `/audio/{track_id}` `/audio-debug/{track_id}` handlers

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::Json as RespJson,
};
use rust_apple_music::{AudioQuality, download, stream};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use crate::error::{ApiResponse, AppError, ok};

use super::{AppCaller, AppCtx, USER_AGENT, get_developer_token, read_user_music_token, stream_cache};

// ── /get-key ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecryptKeyRequest {
    pub track_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecryptKeyResponse {
    pub track_id: String,
    pub content_keys: Vec<ContentKeyPair>,
    pub stream_url: String,
}

#[derive(Serialize)]
pub struct ContentKeyPair {
    pub kid: String,
    pub key: String,
}

pub async fn get_decryption_key_handler(
    State(ctx): State<Arc<AppCtx>>,
    caller: AppCaller,
    Json(body): Json<DecryptKeyRequest>,
) -> Result<RespJson<ApiResponse<DecryptKeyResponse>>, AppError> {
    let dev_token = get_developer_token().await?;
    let music_user_token = read_user_music_token(&ctx.openapi, &caller.cookie_header)
        .await?
        .ok_or_else(|| AppError::bad_request("No music-user-token stored. Please login to Apple Music first."))?;

    info!("[AppleMusic] get-key request for track {}", body.track_id);

    let result = download::get_decryption_key(&dev_token, &music_user_token, &body.track_id, AudioQuality::default())
        .await
        .map_err(|e| AppError::Internal(format!("Decryption pipeline failed: {e}")))?;

    Ok(ok(DecryptKeyResponse {
        track_id: result.track_id,
        content_keys: result
            .content_keys
            .into_iter()
            .map(|(kid, key)| ContentKeyPair { kid, key })
            .collect(),
        stream_url: result.stream_url,
    }))
}

// ── /audio/{track_id} ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AudioQueryParams {
    #[serde(default)]
    #[allow(dead_code)]
    pub quality: Option<String>,
}

pub async fn get_audio_handler(
    State(ctx): State<Arc<AppCtx>>,
    caller: AppCaller,
    headers: HeaderMap,
    axum::extract::Path(track_id): axum::extract::Path<String>,
    axum::extract::Query(_query): axum::extract::Query<AudioQueryParams>,
) -> Result<axum::response::Response, AppError> {
    let dev_token = get_developer_token().await?;
    let music_user_token = read_user_music_token(&ctx.openapi, &caller.cookie_header)
        .await?
        .ok_or_else(|| AppError::bad_request("No music-user-token stored. Please login to Apple Music first."))?;

    let quality = AudioQuality::High;
    info!(
        "[AppleMusic] Audio request for track {track_id} (quality={})",
        quality.as_str()
    );

    let cached_url = {
        let cache = stream_cache().read().await;
        cache.get(&track_id).and_then(|entry| {
            if entry.fetched_at.elapsed().as_secs() < 1800 {
                Some(entry.stream_url.clone())
            } else {
                None
            }
        })
    };

    if cached_url.is_some() {
        info!("[AppleMusic] Using cached stream URL from proxy interception");
    }

    let cache_dir = std::env::temp_dir().join("tokimo_am_cache");

    let path = stream::download_decrypted_audio(
        &dev_token,
        &music_user_token,
        &track_id,
        &cache_dir,
        cached_url.as_deref(),
        quality,
    )
    .await
    .map_err(|e| AppError::Internal(format!("Audio stream failed: {e}")))?;

    let data = tokio::fs::read(&path)
        .await
        .map_err(|e| AppError::Internal(format!("Read cached audio: {e}")))?;
    let total = data.len() as u64;
    let range = parse_range(headers.get(header::RANGE), total);
    let end = range.offset.saturating_add(range.length);
    let slice = data
        .get(range.offset as usize..end as usize)
        .ok_or_else(|| AppError::bad_request("Invalid range"))?;

    let mut builder = axum::response::Response::builder()
        .status(range.status)
        .header(header::CONTENT_TYPE, "audio/mp4")
        .header(header::CACHE_CONTROL, "private, max-age=86400")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_LENGTH, slice.len().to_string());

    if range.status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!(
                "bytes {}-{}/{}",
                range.offset,
                range.offset + range.length.saturating_sub(1),
                total
            ),
        );
    }

    Ok(builder.body(axum::body::Body::from(slice.to_vec())).unwrap())
}

struct ParsedRange {
    offset: u64,
    length: u64,
    status: StatusCode,
}

fn parse_range(range_header: Option<&axum::http::HeaderValue>, total: u64) -> ParsedRange {
    if let Some(val) = range_header
        && let Ok(s) = val.to_str()
        && let Some(rest) = s.strip_prefix("bytes=")
    {
        let parts: Vec<&str> = rest.splitn(2, '-').collect();
        if parts.len() == 2 {
            let start = parts[0].parse::<u64>().unwrap_or(0);
            let end = if parts[1].is_empty() {
                total.saturating_sub(1)
            } else {
                parts[1].parse::<u64>().unwrap_or(total.saturating_sub(1))
            }
            .min(total.saturating_sub(1));
            if start <= end {
                return ParsedRange {
                    offset: start,
                    length: end - start + 1,
                    status: StatusCode::PARTIAL_CONTENT,
                };
            }
        }
    }

    ParsedRange {
        offset: 0,
        length: total,
        status: StatusCode::OK,
    }
}

// ── /audio-debug/{track_id} ───────────────────────────────────────────────────

pub async fn get_audio_debug_handler(
    State(ctx): State<Arc<AppCtx>>,
    caller: AppCaller,
    axum::extract::Path(track_id): axum::extract::Path<String>,
) -> Result<RespJson<serde_json::Value>, AppError> {
    let dev_token = get_developer_token().await?;
    let music_user_token = read_user_music_token(&ctx.openapi, &caller.cookie_header)
        .await?
        .ok_or_else(|| AppError::bad_request("No music-user-token stored. Please login to Apple Music first."))?;

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| AppError::Internal(format!("HTTP client error: {e}")))?;

    let mut result = serde_json::json!({});

    // Step 1: storefront
    let storefront_resp = client
        .get("https://amp-api.music.apple.com/v1/me/account")
        .header("Authorization", format!("Bearer {dev_token}"))
        .header("Media-User-Token", &music_user_token)
        .header("Origin", "https://music.apple.com")
        .query(&[("meta", "subscription")])
        .send()
        .await;
    match storefront_resp {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            result["storefront_status"] = serde_json::json!(status);
            result["storefront_body"] = serde_json::json!(&body[..body.len().min(500)]);
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body)
                && let Some(sf) = json.pointer("/meta/subscription/storefront").and_then(|v| v.as_str())
            {
                result["storefront"] = serde_json::json!(sf);
            }
        }
        Err(e) => {
            result["storefront_error"] = serde_json::json!(format!("{e}"));
        }
    }

    let storefront = result["storefront"].as_str().unwrap_or("us");

    // Step 2: catalog
    let catalog_url = format!("https://amp-api.music.apple.com/v1/catalog/{storefront}/songs/{track_id}");
    let catalog_resp = client
        .get(&catalog_url)
        .header("Authorization", format!("Bearer {dev_token}"))
        .header("Media-User-Token", &music_user_token)
        .header("Origin", "https://music.apple.com")
        .query(&[("extend", "extendedAssetUrls"), ("include", "albums")])
        .send()
        .await;
    match catalog_resp {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            result["catalog_status"] = serde_json::json!(status);
            result["catalog_body"] = serde_json::json!(&body[..body.len().min(800)]);
        }
        Err(e) => {
            result["catalog_error"] = serde_json::json!(format!("{e}"));
        }
    }

    // Step 3: webplayback
    let wp_resp = client
        .post("https://play.itunes.apple.com/WebObjects/MZPlay.woa/wa/webPlayback")
        .header("Authorization", format!("Bearer {dev_token}"))
        .header("Media-User-Token", &music_user_token)
        .header("Origin", "https://music.apple.com")
        .header("Referer", "https://music.apple.com/")
        .header("Content-Type", "application/json")
        .header(
            reqwest::header::COOKIE,
            format!("media-user-token={}", &music_user_token),
        )
        .json(&serde_json::json!({
            "salableAdamId": track_id,
            "language": "en-US",
        }))
        .send()
        .await;
    match wp_resp {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            result["webplayback_status"] = serde_json::json!(status);
            result["webplayback_body"] = serde_json::json!(&body[..body.len().min(800)]);
        }
        Err(e) => {
            result["webplayback_error"] = serde_json::json!(format!("{e}"));
        }
    }

    result["dev_token_len"] = serde_json::json!(dev_token.len());
    result["mut_len"] = serde_json::json!(music_user_token.len());

    if result["catalog_status"] == 404 {
        for alt_sf in &["cn", "tw", "hk", "jp", "sg"] {
            let alt_url = format!("https://amp-api.music.apple.com/v1/catalog/{alt_sf}/songs/{track_id}");
            let alt_resp = client
                .get(&alt_url)
                .header("Authorization", format!("Bearer {dev_token}"))
                .header("Media-User-Token", &music_user_token)
                .header("Origin", "https://music.apple.com")
                .query(&[("extend", "extendedAssetUrls")])
                .send()
                .await;
            if let Ok(resp) = alt_resp {
                let status = resp.status().as_u16();
                if status == 200 {
                    let body = resp.text().await.unwrap_or_default();
                    result["alt_catalog_storefront"] = serde_json::json!(alt_sf);
                    result["alt_catalog_status"] = serde_json::json!(status);
                    result["alt_catalog_body"] = serde_json::json!(&body[..body.len().min(800)]);

                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body)
                        && let Some(hls_url) = json
                            .pointer("/data/0/attributes/extendedAssetUrls/enhancedHls")
                            .and_then(|v| v.as_str())
                        && let Ok(m3u8_resp) = client.get(hls_url).send().await
                        && let Ok(m3u8_text) = m3u8_resp.text().await
                    {
                        result["m3u8_content"] = serde_json::json!(&m3u8_text[..m3u8_text.len().min(2000)]);
                        result["m3u8_has_widevine"] =
                            serde_json::json!(m3u8_text.contains("edef8ba9-79d6-4ace-a3c8-27dcd51d21ed"));
                        result["m3u8_has_fairplay"] = serde_json::json!(m3u8_text.contains("streamingkeydelivery"));
                        result["m3u8_has_session_data"] = serde_json::json!(m3u8_text.contains("AudioSessionKeyInfo"));
                    }

                    break;
                }
            }
        }
    }

    let lib_url = format!("https://amp-api.music.apple.com/v1/me/library/songs/{track_id}");
    let lib_resp = client
        .get(&lib_url)
        .header("Authorization", format!("Bearer {dev_token}"))
        .header("Media-User-Token", &music_user_token)
        .header("Origin", "https://music.apple.com")
        .send()
        .await;
    match lib_resp {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            result["library_status"] = serde_json::json!(status);
            result["library_body"] = serde_json::json!(&body[..body.len().min(800)]);
        }
        Err(e) => {
            result["library_error"] = serde_json::json!(format!("{e}"));
        }
    }

    let cached = stream_cache().read().await;
    if let Some(entry) = cached.get(&track_id) {
        result["stream_cache"] = serde_json::json!({
            "url": &entry.stream_url[..entry.stream_url.len().min(120)],
            "age_secs": entry.fetched_at.elapsed().as_secs(),
        });
    } else {
        result["stream_cache"] = serde_json::json!(null);
        result["stream_cache_keys"] = serde_json::json!(cached.keys().take(10).cloned().collect::<Vec<_>>());
    }

    Ok(RespJson(result))
}
