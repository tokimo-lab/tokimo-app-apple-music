//! 静态资源 — embed 优先，dev 模式通过 cwd 相对路径覆盖（host 设 cwd = install_dir）。

use axum::{
    body::Bytes,
    extract::Path,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "ui/dist/"]
#[prefix = ""]
struct EmbeddedUi;

pub async fn serve(Path(path): Path<String>) -> Response {
    let normalised = normalise(&path);

    let ui_dist_opt = tokimo_bus_cli::manifest::parse_app_ui_dist(crate::MANIFEST)
        .ok()
        .flatten();
    let bytes: Bytes = if let Some(ui_dist) = ui_dist_opt.as_deref() {
        let candidate = std::path::Path::new(ui_dist).join(&normalised);
        if candidate.exists() {
            match tokio::fs::read(&candidate).await {
                Ok(b) => Bytes::from(b),
                Err(e) => {
                    return (StatusCode::NOT_FOUND, format!("asset {normalised}: {e}")).into_response();
                }
            }
        } else {
            match EmbeddedUi::get(&normalised) {
                Some(f) => Bytes::from(f.data.into_owned()),
                None => {
                    return (StatusCode::NOT_FOUND, format!("embedded asset not found: {normalised}")).into_response();
                }
            }
        }
    } else {
        match EmbeddedUi::get(&normalised) {
            Some(f) => Bytes::from(f.data.into_owned()),
            None => {
                return (StatusCode::NOT_FOUND, format!("embedded asset not found: {normalised}")).into_response();
            }
        }
    };

    let mime = mime_for_path(&normalised);
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        bytes,
    )
        .into_response()
}

fn normalise(path: &str) -> String {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() || trimmed.ends_with('/') {
        format!("{trimmed}index.html")
    } else {
        trimmed.to_string()
    }
}

fn mime_for_path(path: &str) -> String {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "js" | "mjs" => "application/javascript",
        "css" => "text/css",
        "html" | "htm" => "text/html; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "wasm" => "application/wasm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
    .to_string()
}
