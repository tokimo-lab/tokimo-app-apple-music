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
    about = "Apple Music — Tokimo app CLI",
    long_about = "Apple Music CLI — call the apple-music app via the Tokimo main server.",
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
    /// Check Apple Music connection status (auth + placeholder output).
    Status,
    /// Search the Apple Music catalog (songs, albums, artists, playlists).
    Search {
        /// Search keyword
        query: String,
        /// Search types, comma-separated (songs,albums,artists,playlists)
        #[arg(short, long, default_value = "songs,albums")]
        types: String,
        /// Max results per type
        #[arg(short, long, default_value_t = 25)]
        limit: u32,
        /// Page number (starting from 1)
        #[arg(short, long, default_value_t = 1)]
        page: u32,
        /// Browsing region (e.g. us, jp, cn). Defaults to account region
        #[arg(short, long)]
        region: Option<String>,
    },
    /// Show album details (track list, release date, label, etc).
    Album {
        /// Apple Music album ID
        album_id: String,
        /// Output raw JSON response
        #[arg(long)]
        raw: bool,
        /// Browsing region (e.g. us, jp, cn). Defaults to account region
        #[arg(short, long)]
        region: Option<String>,
    },
    /// Show song details (album, lyrics status, ISRC, etc).
    Song {
        /// Apple Music song ID
        song_id: String,
        /// Output raw JSON response
        #[arg(long)]
        raw: bool,
        /// Fetch and display full lyrics
        #[arg(long)]
        lyrics: bool,
        /// Browsing region (e.g. us, jp, cn). Defaults to account region
        #[arg(short, long)]
        region: Option<String>,
    },
    /// Show artist details (bio, album list).
    Artist {
        /// Apple Music artist ID
        artist_id: String,
        /// Output raw JSON response
        #[arg(long)]
        raw: bool,
        /// Browsing region (e.g. us, jp, cn). Defaults to account region
        #[arg(short, long)]
        region: Option<String>,
    },
    /// Download and decrypt a song to a local file.
    Download {
        /// Apple Music track ID
        track_id: String,
        /// Output file path or directory
        #[arg(short, long, default_value = ".")]
        output: PathBuf,
        /// Audio quality: lossless, high, standard
        #[arg(short, long, default_value = "high")]
        quality: String,
        /// Browsing region (e.g. us, jp, cn). Defaults to account region
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
