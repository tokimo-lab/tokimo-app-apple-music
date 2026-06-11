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
mod cli;
mod error;
mod handlers;
mod openapi_client;

const MANIFEST: &str = include_str!("../tokimo-app.toml");

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use clap::{Parser, Subcommand};
use tokimo_bus_cli::TokimoAuthArgs;
use tokimo_bus_client::{BusClient, ClientConfig};
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(
    name = "tokimo-app-apple-music",
    about = "Apple Music — Tokimo 子 app CLI",
    long_about = "Apple Music CLI — 通过 Tokimo 主 server 调用 apple-music app。",
    term_width = 100
)]
struct Cli {
    #[command(flatten)]
    auth: TokimoAuthArgs,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// 检查 Apple Music 连接状态（认证 + 占位输出）。
    Status,
    /// 搜索 Apple Music 目录（歌曲、专辑）。
    Search {
        /// 搜索关键词
        query: String,
        /// 搜索类型，逗号分隔 (songs,albums)
        #[arg(short, long, default_value = "songs,albums")]
        types: String,
        /// 每种类型最多返回的结果数
        #[arg(short, long, default_value_t = 25)]
        limit: u32,
        /// 页码（从 1 开始）
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// 浏览区域 (如 us, jp, cn)，不传则使用账号所在区域
        #[arg(short, long)]
        region: Option<String>,
    },
    /// 查看专辑详情（曲目列表、发行日期、厂牌等）。
    Album {
        /// Apple Music 专辑 ID
        album_id: String,
        /// 输出原始 JSON 响应
        #[arg(long)]
        raw: bool,
        /// 浏览区域 (如 us, jp, cn)，不传则使用账号所在区域
        #[arg(short, long)]
        region: Option<String>,
    },
    /// 查看歌曲详情（专辑、歌词状态、ISRC 等）。
    Song {
        /// Apple Music song ID
        song_id: String,
        /// 输出原始 JSON 响应
        #[arg(long)]
        raw: bool,
        /// 获取并显示完整歌词
        #[arg(long)]
        lyrics: bool,
        /// 浏览区域 (如 us, jp, cn)，不传则使用账号所在区域
        #[arg(short, long)]
        region: Option<String>,
    },
    /// 查看歌手详情（简介、专辑列表）。
    Artist {
        /// Apple Music artist ID
        artist_id: String,
        /// 输出原始 JSON 响应
        #[arg(long)]
        raw: bool,
        /// 浏览区域 (如 us, jp, cn)，不传则使用账号所在区域
        #[arg(short, long)]
        region: Option<String>,
    },
    /// 下载并解密一首歌曲到本地文件。
    Download {
        /// Apple Music track ID
        track_id: String,
        /// 输出文件路径或目录
        #[arg(short, long, default_value = ".")]
        output: PathBuf,
        /// 音质: lossless, high, standard
        #[arg(short, long, default_value = "high")]
        quality: String,
        /// 浏览区域 (如 us, jp, cn)，不传则使用账号所在区域
        #[arg(short, long)]
        region: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let Cli { auth, command } = Cli::parse();

    match command {
        None if std::env::var_os("TOKIMO_BUS_SOCKET").is_some() => {
            // server 模式：由 supervisor 无参拉起，初始化 tracing
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "info,tokimo_bus_client=info,tokimo_app_apple_music=debug".into()),
                )
                .init();
            if let Err(e) = run_server().await {
                error!(error = %e, "apple-music: fatal");
                std::process::exit(1);
            }
        }
        None => {
            use clap::CommandFactory;
            let mut cmd = Cli::command();
            tokimo_bus_cli::print_help_unified(&mut cmd);
            std::process::exit(0);
        }
        Some(cmd) => {
            // CLI 模式：纯文本错误，不输出 tracing 日志
            let result = match cmd {
                Command::Status => cli::run_status(auth).await,
                Command::Album { album_id, raw, region } => cli::run_album(auth, album_id, raw, region).await,
                Command::Song {
                    song_id,
                    raw,
                    lyrics,
                    region,
                } => cli::run_song(auth, song_id, raw, lyrics, region).await,
                Command::Artist { artist_id, raw, region } => cli::run_artist(auth, artist_id, raw, region).await,
                Command::Search {
                    query,
                    types,
                    limit,
                    page,
                    region,
                } => cli::run_search(auth, query, types, limit, page, region).await,
                Command::Download {
                    track_id,
                    output,
                    quality,
                    region,
                } => cli::run_download(auth, track_id, output, quality, region).await,
            };
            if let Err(error) = result {
                eprintln!("Error: {error:#}");
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

async fn run_server() -> anyhow::Result<()> {
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
