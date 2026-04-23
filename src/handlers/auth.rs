//! `/token` `/auth` `/quality` handlers
//!
//! 路径与原 server `/api/apps/apple-music/{token,auth,quality}` 一致。

use axum::{Json, extract::State, response::Json as RespJson};
use rust_apple_music::AudioQuality;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};

use crate::error::{ApiResponse, AppError, ok};

use super::{
    APPLE_MUSIC_PREF_SCOPE, APPLE_MUSIC_PREF_SCOPE_ID, APPLE_MUSIC_PREF_TOKEN_KEY, APPLE_MUSIC_QUALITY_KEY,
    APPLE_MUSIC_SETTINGS_SCOPE_ID, AppCaller, AppCtx, get_developer_token, parse_user_id, read_user_audio_quality,
    read_user_music_token,
};

// ── /token ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppleMusicTokenResponse {
    pub developer_token: String,
}

pub async fn get_apple_music_token(
    State(_ctx): State<Arc<AppCtx>>,
) -> Result<RespJson<ApiResponse<AppleMusicTokenResponse>>, AppError> {
    let token = get_developer_token().await.map_err(|e| {
        warn!("[AppleMusic] Failed to get developer token: {e}");
        e
    })?;
    Ok(ok(AppleMusicTokenResponse { developer_token: token }))
}

// ── /auth ─────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppleMusicAuthStatus {
    pub has_token: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAuthInput {
    pub music_user_token: String,
}

pub async fn get_apple_music_auth(
    State(ctx): State<Arc<AppCtx>>,
    caller: AppCaller,
) -> Result<RespJson<ApiResponse<AppleMusicAuthStatus>>, AppError> {
    let has_token = read_user_music_token(&ctx.openapi, &caller.cookie_header)
        .await?
        .is_some();
    Ok(ok(AppleMusicAuthStatus { has_token }))
}

pub async fn save_apple_music_auth(
    State(ctx): State<Arc<AppCtx>>,
    caller: AppCaller,
    Json(body): Json<SaveAuthInput>,
) -> Result<RespJson<ApiResponse<AppleMusicAuthStatus>>, AppError> {
    let _uid = parse_user_id(&caller.user_id)?;
    let value = serde_json::json!({
        APPLE_MUSIC_PREF_TOKEN_KEY: body.music_user_token,
    });
    ctx.openapi
        .pref_put(
            &caller.cookie_header,
            APPLE_MUSIC_PREF_SCOPE,
            APPLE_MUSIC_PREF_SCOPE_ID,
            value,
        )
        .await?;
    Ok(ok(AppleMusicAuthStatus { has_token: true }))
}

pub async fn delete_apple_music_auth(
    State(ctx): State<Arc<AppCtx>>,
    caller: AppCaller,
) -> Result<RespJson<ApiResponse<AppleMusicAuthStatus>>, AppError> {
    let _uid = parse_user_id(&caller.user_id)?;
    ctx.openapi
        .pref_delete(&caller.cookie_header, APPLE_MUSIC_PREF_SCOPE, APPLE_MUSIC_PREF_SCOPE_ID)
        .await?;
    Ok(ok(AppleMusicAuthStatus { has_token: false }))
}

// ── /quality ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioQualityResponse {
    pub audio_quality: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAudioQualityInput {
    pub audio_quality: String,
}

pub async fn get_audio_quality(
    State(ctx): State<Arc<AppCtx>>,
    caller: AppCaller,
) -> Result<RespJson<ApiResponse<AudioQualityResponse>>, AppError> {
    let quality = read_user_audio_quality(&ctx.openapi, &caller.cookie_header).await?;
    Ok(ok(AudioQualityResponse {
        audio_quality: quality.as_str().to_string(),
    }))
}

pub async fn set_audio_quality(
    State(ctx): State<Arc<AppCtx>>,
    caller: AppCaller,
    Json(body): Json<SetAudioQualityInput>,
) -> Result<RespJson<ApiResponse<AudioQualityResponse>>, AppError> {
    let quality = AudioQuality::from_str_loose(&body.audio_quality);
    let _uid = parse_user_id(&caller.user_id)?;
    let value = serde_json::json!({ APPLE_MUSIC_QUALITY_KEY: quality.as_str() });
    ctx.openapi
        .pref_put(
            &caller.cookie_header,
            APPLE_MUSIC_PREF_SCOPE,
            APPLE_MUSIC_SETTINGS_SCOPE_ID,
            value,
        )
        .await?;

    info!("[AppleMusic] User {} set audio quality to {:?}", caller.user_id, quality);
    Ok(ok(AudioQualityResponse {
        audio_quality: quality.as_str().to_string(),
    }))
}
