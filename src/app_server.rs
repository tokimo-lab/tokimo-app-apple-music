//! 内嵌 axum HTTP server，监听本地 socket。
//!
//! server 端 `/api/apps/apple-music/<rest>` 透明反代到本 sock 的 `/<rest>`。

use std::sync::Arc;

use axum::{
    Router,
    routing::{get, post},
};
use tokimo_bus_protocol::{BusListener, DataPlaneSocket};
use tracing::{error, info};

use crate::{
    assets,
    handlers::{self, AppCtx},
};

pub async fn spawn(service: &str, ctx: Arc<AppCtx>) -> anyhow::Result<DataPlaneSocket> {
    let (listener, socket) = BusListener::bind_for_app(service)?;
    info!(?socket, "apple-music: app server listening");

    let router = build_router(ctx);

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, router).await {
            error!(error = %e, "apple-music: app server stopped");
        }
    });

    Ok(socket)
}

fn build_router(ctx: Arc<AppCtx>) -> Router {
    Router::new()
        // 路径与原 server `/api/apps/apple-music/*` 保持完全一致（去掉前缀），
        // 这样前端代码无需改动。
        .route("/token", get(handlers::auth::get_apple_music_token))
        .route(
            "/auth",
            get(handlers::auth::get_apple_music_auth)
                .post(handlers::auth::save_apple_music_auth)
                .delete(handlers::auth::delete_apple_music_auth),
        )
        .route(
            "/quality",
            get(handlers::auth::get_audio_quality).put(handlers::auth::set_audio_quality),
        )
        .route("/proxy", post(handlers::proxy::proxy_apple_music_api))
        .route("/get-key", post(handlers::audio::get_decryption_key_handler))
        .route("/audio/{track_id}", get(handlers::audio::get_audio_handler))
        .route("/audio-debug/{track_id}", get(handlers::audio::get_audio_debug_handler))
        // assets：与 helloworld 一致
        .route("/assets/{*path}", get(assets::serve))
        .with_state(ctx)
}
