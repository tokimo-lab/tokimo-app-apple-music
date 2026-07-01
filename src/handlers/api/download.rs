//! Apple Music track download + decrypt.

use super::token::{call_with_dev_token_retry, get_developer_token};

pub async fn download_track(
    music_user_token: &str,
    track_id: &str,
    output: &std::path::Path,
    quality: rust_apple_music::AudioQuality,
) -> Result<std::path::PathBuf, String> {
    use rust_apple_music::stream::download_decrypted_audio;
    use std::path::PathBuf;

    let cache_dir = std::env::temp_dir().join("tokimo-apple-music-cli");
    let user_token = music_user_token.to_string();
    let track = track_id.to_string();

    let cached_path = call_with_dev_token_retry("download_track", || {
        let user_token = user_token.clone();
        let track = track.clone();
        let cache = cache_dir.clone();
        async move {
            let token = get_developer_token().await.map_err(|e| e.to_string())?;
            download_decrypted_audio(&token, &user_token, &track, &cache, None, None, quality).await
        }
    })
    .await?;

    // Treat as directory if it exists as one OR looks like one (ends with /)
    let is_dir = output.is_dir() || output.to_str().is_some_and(|s| s.ends_with('/'));
    if is_dir {
        tokio::fs::create_dir_all(output)
            .await
            .map_err(|e| format!("Create output dir: {e}"))?;
        let ext = cached_path.extension().and_then(|e| e.to_str()).unwrap_or("m4a");
        let filename = format!("{track_id}_{quality}.{ext}", quality = quality.as_str());
        let dest = output.join(&filename);
        tokio::fs::copy(&cached_path, &dest)
            .await
            .map_err(|e| format!("Copy to output: {e}"))?;
        Ok(dest)
    } else {
        if let Some(parent) = output.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Create output dir: {e}"))?;
        }
        tokio::fs::copy(&cached_path, output)
            .await
            .map_err(|e| format!("Copy to output: {e}"))?;
        Ok(PathBuf::from(output))
    }
}
