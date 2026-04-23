//! Apple Music app — 方案 3 形态：内嵌 axum + UDS。
//!
//! ## 与 helloworld 模板的差别
//!
//! - **无自己的 DB schema**：所有 user 配置（music-user-token、audio quality）通过
//!   server `/openapi/user/preferences/*` 读写，本 binary 不接 PostgreSQL
//! - **token 缓存仅在内存**：MusicKit 开发者 token + webplayback stream URL 走
//!   `OnceLock<RwLock<...>>`，进程重启即重建（合理，token 本来就是 1h TTL）
//! - **依赖 `rust-apple-music`**：复用 tokimo 主仓库已有的 HLS / ALAC 解密栈

mod app_server;
mod assets;
mod error;
mod handlers;
mod openapi_client;

use std::sync::{Arc, OnceLock};

use tokimo_bus_client::{BusClient, ClientConfig};
use tracing::{error, info};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tokimo_bus_client=info,tokimo_app_apple_music=debug".into()),
        )
        .init();

    if let Err(e) = run().await {
        error!(error = %e, "apple-music: fatal");
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let cfg = ClientConfig::from_env().map_err(|e| anyhow::anyhow!("ClientConfig: {e}"))?;
    info!(endpoint = ?cfg.endpoint, "apple-music: connecting to broker");

    let openapi = Arc::new(openapi_client::OpenApiClient::from_env()?);
    let client_slot: Arc<OnceLock<Arc<BusClient>>> = Arc::new(OnceLock::new());
    let ctx = Arc::new(handlers::AppCtx {
        openapi,
        client: Arc::clone(&client_slot),
    });

    let app_socket = app_server::spawn("apple-music", Arc::clone(&ctx))
        .await
        .map_err(|e| anyhow::anyhow!("app_server spawn: {e}"))?;

    let client = BusClient::builder(cfg)
        .service("apple-music", env!("CARGO_PKG_VERSION"))
        .data_plane(app_socket)
        .build()
        .await
        .map_err(|e| anyhow::anyhow!("bus build: {e}"))?;
    client_slot
        .set(Arc::clone(&client))
        .map_err(|_| anyhow::anyhow!("client_slot already set"))?;

    info!("apple-music: registered with broker");

    let shutdown = {
        let client = Arc::clone(&client);
        tokio::spawn(async move { client.run_until_shutdown().await })
    };

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("apple-music: SIGINT received");
            client.shutdown();
        }
        _ = shutdown => info!("apple-music: broker sent Shutdown"),
    }

    Ok(())
}
