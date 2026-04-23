//! Apple Music audio streaming: download HLS segments, decrypt in-place, cache.

use std::path::{Path, PathBuf};

use tokio::fs;
use tracing::{debug, info};

use super::decrypt::{decrypt_to_clean_m4a, decrypt_to_clean_m4a_via_wrapper};
use super::download::{AudioQuality, get_decryption_key};

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// Download, decrypt, and cache an Apple Music track as a clear fMP4 file.
///
/// `cached_stream_url` — if provided, skip the webplayback/catalog API calls
/// and use this URL directly (captured from `MusicKit`'s proxy traffic).
///
/// Returns the path to the cached file. Subsequent calls for the same
/// `track_id` return the cached path immediately.
pub async fn download_decrypted_audio(
    dev_token: &str,
    music_user_token: &str,
    track_id: &str,
    cache_dir: &Path,
    cached_stream_url: Option<&str>,
    quality: AudioQuality,
) -> Result<PathBuf, String> {
    // Check cache — quality suffix ensures separate files per quality tier
    let suffix = quality.cache_suffix();
    let mp4_path = cache_dir.join(format!("am_{track_id}_{suffix}.mp4"));

    if mp4_path.exists() {
        debug!(
            "[AppleMusic] Cache hit for track {track_id} (quality={})",
            quality.as_str()
        );
        return Ok(mp4_path);
    }

    info!(
        "[AppleMusic] Downloading + decrypting track {track_id} (quality={})",
        quality.as_str()
    );

    // 1. Get decryption key + stream URL
    let result = get_decryption_key(dev_token, music_user_token, track_id, quality).await?;
    let _ = cached_stream_url; // TODO: re-add stream URL cache optimization

    // Always use result.stream_url — it's resolved to the variant m3u8 URL
    let stream_url = result.stream_url.clone();

    info!(
        "[AppleMusic] Using stream URL: {}...",
        &stream_url[..stream_url.len().min(80)]
    );

    // 2. Fetch and parse the HLS media playlist
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;

    let m3u8_text = client
        .get(&stream_url)
        .send()
        .await
        .map_err(|e| format!("Fetch m3u8: {e}"))?
        .text()
        .await
        .map_err(|e| format!("Read m3u8: {e}"))?;

    debug!(
        "[AppleMusic] m3u8 content (first 500 chars): {}",
        &m3u8_text[..m3u8_text.len().min(500)]
    );

    // Check if this is actually a master playlist (not media)
    if m3u8_text.contains("#EXT-X-STREAM-INF") {
        return Err(format!(
            "Stream URL is a master playlist, not a media playlist. URL: {}",
            &stream_url[..stream_url.len().min(100)]
        ));
    }

    let playlist = m3u8_rs::parse_media_playlist_res(m3u8_text.as_bytes()).map_err(|e| format!("Parse m3u8: {e:?}"))?;

    let base_url = stream_url.rsplit_once('/').map_or("", |(b, _)| b);

    // 3. Detect byte-range playlist vs individual-segment playlist.
    let is_byte_range = playlist.segments.iter().any(|s| s.byte_range.is_some());

    let fmp4_data = if is_byte_range {
        let seg_uri = &playlist.segments[0].uri;
        let full_url = resolve_url(base_url, seg_uri);
        info!(
            "[AppleMusic] Byte-range playlist ({} segments), downloading full file",
            playlist.segments.len()
        );
        let data = client
            .get(&full_url)
            .send()
            .await
            .map_err(|e| format!("Download full file: {e}"))?
            .bytes()
            .await
            .map_err(|e| format!("Read full file: {e}"))?;
        info!("[AppleMusic] Downloaded {} bytes", data.len());
        data.to_vec()
    } else {
        let mut buf = Vec::new();

        let init_uri = playlist
            .segments
            .iter()
            .find_map(|seg| seg.map.as_ref())
            .map(|m| &m.uri)
            .ok_or("No init segment (EXT-X-MAP) in playlist")?;

        let init_url = resolve_url(base_url, init_uri);
        let init_data = client
            .get(&init_url)
            .send()
            .await
            .map_err(|e| format!("Download init segment: {e}"))?
            .bytes()
            .await
            .map_err(|e| format!("Read init segment: {e}"))?;
        buf.extend_from_slice(&init_data);

        info!(
            "[AppleMusic] Init segment: {} bytes, {} media segments",
            init_data.len(),
            playlist.segments.len()
        );

        for (i, seg) in playlist.segments.iter().enumerate() {
            let seg_url = resolve_url(base_url, &seg.uri);
            let seg_data = client
                .get(&seg_url)
                .send()
                .await
                .map_err(|e| format!("Download segment {i}: {e}"))?
                .bytes()
                .await
                .map_err(|e| format!("Read segment {i}: {e}"))?;
            append_moof_mdat_only(&seg_data, &mut buf);
        }
        buf
    };

    info!("[AppleMusic] Downloaded {} bytes total, decrypting...", fmp4_data.len());

    // 4. Decrypt and reassemble as clean non-fragmented M4A
    let clean_m4a = if let Some(ref fp_key) = result.fairplay_key {
        // FairPlay ALAC path — decrypt via wrapper TCP service
        let wrapper_config =
            super::wrapper_client::WrapperConfig::from_env().ok_or("FairPlay wrapper not configured")?;
        info!("[AppleMusic] Using FairPlay wrapper for ALAC decryption");
        decrypt_to_clean_m4a_via_wrapper(&fmp4_data, &wrapper_config, track_id, fp_key).await?
    } else {
        // Widevine path — local AES decrypt
        decrypt_to_clean_m4a(&fmp4_data, &result.content_keys, result.legacy)?
    };

    // 5. Write to cache
    fs::create_dir_all(cache_dir)
        .await
        .map_err(|e| format!("Create cache dir: {e}"))?;

    fs::write(&mp4_path, &clean_m4a)
        .await
        .map_err(|e| format!("Write cache: {e}"))?;

    info!("[AppleMusic] Cached decrypted audio: {}", mp4_path.display());

    Ok(mp4_path)
}

fn resolve_url(base: &str, uri: &str) -> String {
    if uri.starts_with("http") {
        uri.to_string()
    } else {
        format!("{base}/{uri}")
    }
}

/// Append only moof+mdat boxes from segment data, skipping ftyp/moov/styp/sidx/free.
fn append_moof_mdat_only(seg_data: &[u8], out: &mut Vec<u8>) {
    let mut offset = 0;
    while offset + 8 <= seg_data.len() {
        let size = u32::from_be_bytes([
            seg_data[offset],
            seg_data[offset + 1],
            seg_data[offset + 2],
            seg_data[offset + 3],
        ]) as usize;
        if size == 0 || offset + size > seg_data.len() {
            break;
        }
        let btype = &seg_data[offset + 4..offset + 8];
        // Keep moof and mdat boxes, skip everything else
        if btype == b"moof" || btype == b"mdat" {
            out.extend_from_slice(&seg_data[offset..offset + size]);
        }
        offset += size;
    }
}
