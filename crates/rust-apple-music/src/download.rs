//! Apple Music download and decryption pipeline.
//!
//! Flow: webplayback → m3u8 → extract PSSH → license exchange → decrypt

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use super::cdm::{self, Cdm, WvDevice};

// ── Audio quality tiers ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioQuality {
    /// ALAC lossless (modern flow only; falls back to High on legacy)
    Lossless,
    /// AAC 256 kbps — current default
    #[default]
    High,
    /// Lowest available AAC bitrate
    Standard,
}

impl AudioQuality {
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "lossless" => Self::Lossless,
            "standard" | "low" => Self::Standard,
            _ => Self::High,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Lossless => "lossless",
            Self::High => "high",
            Self::Standard => "standard",
        }
    }

    /// Short suffix for cache file naming.
    pub fn cache_suffix(&self) -> &'static str {
        match self {
            Self::Lossless => "lossless",
            Self::High => "high",
            Self::Standard => "std",
        }
    }
}

// ── Constants ───────────────────────────────────────────────────────────────

const WEBPLAYBACK_URL: &str = "https://play.itunes.apple.com/WebObjects/MZPlay.woa/wa/webPlayback";
const LICENSE_URL: &str = "https://play.itunes.apple.com/WebObjects/MZPlay.woa/wa/acquireWebPlaybackLicense";
const AMP_API_URL: &str = "https://amp-api.music.apple.com";
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// Default Widevine PSSH URI for the prefetch key (s1/e1). Must be skipped
/// when collecting content keys — its key is the hardcoded `DEFAULT_SONG_KEY`.
const WIDEVINE_DEFAULT_PSSH_URI: &str =
    "data:text/plain;base64,AAAAOHBzc2gAAAAA7e+LqXnWSs6jyCfc1R0h7QAAABgSEAAAAAAAAAAczEvZTEgICBI88aJmwY=";

// ── DTOs ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebplaybackResponse {
    song_list: Vec<SongListItem>,
}

#[derive(Debug, Deserialize)]
struct SongListItem {
    assets: Vec<SongAsset>,
}

#[derive(Debug, Deserialize)]
struct SongAsset {
    flavor: String,
    #[serde(rename = "URL")]
    url: String,
}

#[derive(Debug, Deserialize)]
struct LicenseResponse {
    status: i32,
    license: Option<String>,
    #[serde(rename = "errorCode")]
    error_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecryptionResult {
    pub track_id: String,
    /// Content keys as (`kid_hex`, `key_hex`) pairs — supports key rotation
    pub content_keys: Vec<(String, String)>,
    pub stream_url: String,
    /// True for legacy (CENC/AES-CTR) streams from webplayback
    pub legacy: bool,
    /// `FairPlay` key URI for wrapper-based ALAC decryption (None = Widevine path)
    pub fairplay_key: Option<String>,
}

// ── Stream info extraction ──────────────────────────────────────────────────

#[derive(Debug)]
struct StreamInfo {
    stream_url: String,
    /// All unique Widevine PSSH URIs found in the stream (supports key rotation)
    widevine_psshs: Vec<String>,
    fairplay_key: Option<String>,
    legacy: bool,
}

// ── HTTP client builder ─────────────────────────────────────────────────────

fn build_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))
}

// ── Core pipeline ───────────────────────────────────────────────────────────

/// Full pipeline: get stream info → extract PSSH → license exchange → return decryption key.
///
/// For lossless quality with an available `FairPlay` wrapper, returns the `FairPlay`
/// key URI directly (no Widevine CDM needed). For other qualities, uses Widevine.
pub async fn get_decryption_key(
    developer_token: &str,
    media_user_token: &str,
    track_id: &str,
    quality: AudioQuality,
) -> Result<DecryptionResult, String> {
    let client = build_client()?;

    // Step 1: Get webplayback to find stream URL
    info!(
        "[AppleMusic] Getting webplayback for track {track_id} (quality={:?})",
        quality
    );
    let mut stream_info = get_stream_info(&client, developer_token, media_user_token, track_id, quality).await?;
    debug!("[AppleMusic] Stream URL: {}", stream_info.stream_url);

    // FairPlay path: lossless with wrapper available — skip Widevine entirely
    if quality == AudioQuality::Lossless && stream_info.fairplay_key.is_some() {
        let wrapper_config = super::wrapper_client::WrapperConfig::from_env();
        if wrapper_config.is_some() {
            info!("[AppleMusic] FairPlay lossless path for track {track_id}");
            return Ok(DecryptionResult {
                track_id: track_id.to_string(),
                content_keys: vec![],
                stream_url: stream_info.stream_url,
                legacy: false,
                fairplay_key: stream_info.fairplay_key,
            });
        }
        // The ALAC variant only has FairPlay keys — re-fetch using AAC quality to get Widevine PSSH
        info!("[AppleMusic] FairPlay wrapper not enabled, falling back to Widevine AAC");
        stream_info = get_stream_info(&client, developer_token, media_user_token, track_id, AudioQuality::High).await?;
        debug!("[AppleMusic] AAC fallback stream URL: {}", stream_info.stream_url);
    }

    if stream_info.widevine_psshs.is_empty() {
        return Err("No Widevine PSSH found in stream".into());
    }

    info!(
        "[AppleMusic] Found {} Widevine PSSH(s) for track {track_id}",
        stream_info.widevine_psshs.len()
    );

    let mut all_keys: Vec<(String, String)> = Vec::new();

    // Request license for each unique PSSH (supports key rotation)
    for (i, widevine_uri) in stream_info.widevine_psshs.iter().enumerate() {
        let pssh_data = extract_pssh_data(widevine_uri)?;
        debug!("[AppleMusic] PSSH {i}: {} bytes", pssh_data.len());

        let device = WvDevice::from_base64(cdm::HARDCODED_WVD_B64)?;
        let mut cdm = Cdm::new(device);
        let session_id = cdm.open();

        let challenge_bytes = cdm.get_license_challenge(&session_id, &pssh_data)?;
        let challenge_b64 = B64.encode(&challenge_bytes);

        let license_b64 = exchange_license(
            &client,
            developer_token,
            media_user_token,
            track_id,
            widevine_uri,
            &challenge_b64,
        )
        .await?;

        cdm.parse_license(&session_id, &license_b64)?;
        let keys = cdm.get_keys(&session_id, Some("CONTENT"));
        cdm.close(&session_id);

        for key in &keys {
            let kid_hex = hex::encode(key.kid.as_bytes());
            let key_hex = hex::encode(&key.key);
            if !all_keys.iter().any(|(k, _)| *k == kid_hex) {
                info!("[AppleMusic] Key {}: kid={kid_hex}, key={key_hex}", all_keys.len());
                all_keys.push((kid_hex, key_hex));
            }
        }
    }

    if all_keys.is_empty() {
        return Err("No CONTENT keys found in license responses".into());
    }

    info!(
        "[AppleMusic] Got {} content key(s) for track {track_id}",
        all_keys.len()
    );

    Ok(DecryptionResult {
        track_id: track_id.to_string(),
        content_keys: all_keys,
        stream_url: stream_info.stream_url,
        legacy: stream_info.legacy,
        fairplay_key: None,
    })
}

// ── Get stream info ─────────────────────────────────────────────────────────

async fn get_stream_info(
    client: &reqwest::Client,
    dev_token: &str,
    mut_token: &str,
    track_id: &str,
    quality: AudioQuality,
) -> Result<StreamInfo, String> {
    // Resolve library IDs (i.xxx) to catalog IDs
    let resolved_id = if track_id.starts_with("i.") {
        resolve_library_to_catalog(client, dev_token, mut_token, track_id).await?
    } else {
        track_id.to_string()
    };
    let track_id = &resolved_id;

    // Try modern flow first (extendedAssetUrls → enhancedHls)
    let modern_result = get_stream_info_modern(client, dev_token, mut_token, track_id, quality).await;

    match &modern_result {
        Ok(info) if !info.widevine_psshs.is_empty() => {
            return modern_result;
        }
        Ok(info) if info.fairplay_key.is_some() && quality == AudioQuality::Lossless => {
            // FairPlay ALAC — return as-is (caller will use wrapper if available)
            debug!("[AppleMusic] Modern flow: FairPlay lossless ALAC path");
            return modern_result;
        }
        Ok(info) => {
            debug!(
                "[AppleMusic] Modern flow returned no Widevine PSSH (variant={})",
                &info.stream_url[..info.stream_url.len().min(60)]
            );
        }
        Err(e) => {
            debug!("[AppleMusic] Modern flow failed: {e}");
        }
    }

    // Fall back to legacy webplayback (Widevine CENC)
    if quality == AudioQuality::Lossless {
        info!("[AppleMusic] Lossless requested but FairPlay wrapper not available — falling back to AAC 256k");
    }
    let legacy = get_stream_info_legacy(client, dev_token, mut_token, track_id).await?;
    Ok(legacy)
}

/// Modern flow: get HLS master m3u8 from extendedAssetUrls
async fn get_stream_info_modern(
    client: &reqwest::Client,
    dev_token: &str,
    mut_token: &str,
    track_id: &str,
    quality: AudioQuality,
) -> Result<StreamInfo, String> {
    // Fetch song metadata with extendedAssetUrls
    let url = format!("{AMP_API_URL}/v1/catalog/us/songs/{track_id}");
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {dev_token}"))
        .header("Media-User-Token", mut_token)
        .header("Origin", "https://music.apple.com")
        .query(&[("extend", "extendedAssetUrls"), ("include", "albums")])
        .send()
        .await
        .map_err(|e| format!("Failed to fetch song metadata: {e}"))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse song metadata: {e}"))?;

    let enhanced_hls = body
        .pointer("/data/0/attributes/extendedAssetUrls/enhancedHls")
        .and_then(|v| v.as_str())
        .ok_or("No enhancedHls URL in song metadata")?;

    debug!("[AppleMusic] Master m3u8 URL: {enhanced_hls}");

    // Fetch and parse master m3u8
    let m3u8_text = client
        .get(enhanced_hls)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch master m3u8: {e}"))?
        .text()
        .await
        .map_err(|e| format!("Failed to read master m3u8: {e}"))?;

    let master = m3u8_rs::parse_master_playlist_res(m3u8_text.as_bytes())
        .map_err(|e| format!("Failed to parse master m3u8: {e}"))?;

    // Debug: log session data and session keys present in master m3u8
    debug!(
        "[AppleMusic] Master m3u8: {} variants, {} session_data, {} session_keys",
        master.variants.len(),
        master.session_data.len(),
        master.session_key.len()
    );
    for sd in &master.session_data {
        debug!("[AppleMusic] Session data: id={}", sd.data_id);
    }
    for sk in &master.session_key {
        debug!(
            "[AppleMusic] Session key: method={:?}, keyformat={:?}, uri={}",
            sk.0.method,
            sk.0.keyformat,
            sk.0.uri
                .as_deref()
                .unwrap_or("none")
                .chars()
                .take(80)
                .collect::<String>()
        );
    }

    // Select variant based on requested quality
    let _base_url = enhanced_hls.rsplitn(2, '/').last().unwrap_or("");
    let base_url = &enhanced_hls[..=enhanced_hls.rfind('/').unwrap_or(0)];

    let variant = select_variant_by_quality(&master.variants, quality);
    let variant = variant.ok_or("No audio variant found in master m3u8")?;

    info!(
        "[AppleMusic] Selected variant: codec={:?}, bandwidth={}, uri={}",
        variant.codecs,
        variant.bandwidth,
        &variant.uri[..variant.uri.len().min(60)]
    );

    let variant_url = if variant.uri.starts_with("http") {
        variant.uri.clone()
    } else {
        format!("{base_url}{}", variant.uri)
    };

    // Check for session data (AudioSessionKeyInfo)
    let mut widevine_psshs: Vec<String> = Vec::new();
    let mut fairplay_key = None;

    // Try extracting from session data first
    for sd in &master.session_data {
        if sd.data_id == "com.apple.hls.AudioSessionKeyInfo"
            && let m3u8_rs::SessionDataField::Value(ref value) = sd.field
            && let Ok(decoded) = B64.decode(value)
            && let Ok(json) = serde_json::from_slice::<serde_json::Value>(&decoded)
        {
            debug!("[AppleMusic] Session data keys: {}", json);
            if let Some(uri) = extract_widevine_from_session_data(&json) {
                debug!(
                    "[AppleMusic] Got content Widevine PSSH from session data (len={})",
                    uri.len()
                );
                if !widevine_psshs.contains(&uri) {
                    widevine_psshs.push(uri);
                }
            }
            fairplay_key = extract_fairplay_from_session_data(&json);
        }
    }

    // If no session data, fetch variant m3u8 and extract from #EXT-X-KEY
    if widevine_psshs.is_empty() {
        debug!("[AppleMusic] No Widevine from session data, fetching variant m3u8");

        // Also check master #EXT-X-SESSION-KEY for Widevine
        for sk in &master.session_key {
            let keyformat = sk.0.keyformat.as_deref().unwrap_or("");
            let uri = sk.0.uri.as_deref().unwrap_or("");
            if keyformat.contains("edef8ba9-79d6-4ace-a3c8-27dcd51d21ed") {
                if uri == WIDEVINE_DEFAULT_PSSH_URI {
                    debug!("[AppleMusic] Skipping default Widevine session key");
                    continue;
                }
                let uri_str = uri.to_string();
                if !widevine_psshs.contains(&uri_str) {
                    debug!("[AppleMusic] Got Widevine PSSH from session key");
                    widevine_psshs.push(uri_str);
                }
            }
        }

        if widevine_psshs.is_empty() {
            let variant_text = client
                .get(&variant_url)
                .send()
                .await
                .map_err(|e| format!("Failed to fetch variant m3u8: {e}"))?
                .text()
                .await
                .map_err(|e| format!("Failed to read variant m3u8: {e}"))?;

            // Log raw key lines for debugging
            for line in variant_text.lines() {
                if line.starts_with("#EXT-X-KEY") || line.starts_with("#EXT-X-SESSION") {
                    debug!("[AppleMusic] Variant key line: {}", &line[..line.len().min(120)]);
                }
            }

            let media_playlist = m3u8_rs::parse_media_playlist_res(variant_text.as_bytes())
                .map_err(|e| format!("Failed to parse variant m3u8: {e}"))?;

            for segment in &media_playlist.segments {
                if let Some(ref key) = segment.key {
                    let keyformat = key.keyformat.as_deref().unwrap_or("");
                    let uri = key.uri.as_deref().unwrap_or("");
                    if keyformat.contains("edef8ba9-79d6-4ace-a3c8-27dcd51d21ed") {
                        // Skip the default/prefetch Widevine PSSH
                        if uri == WIDEVINE_DEFAULT_PSSH_URI {
                            continue;
                        }
                        let uri_str = uri.to_string();
                        if !widevine_psshs.contains(&uri_str) {
                            widevine_psshs.push(uri_str);
                        }
                    } else if keyformat.contains("streamingkeydelivery") {
                        fairplay_key = Some(uri.to_string());
                    }
                }
            }
        }
    }

    debug!(
        "[AppleMusic] Modern flow: {} Widevine PSSH(s) found",
        widevine_psshs.len()
    );

    Ok(StreamInfo {
        stream_url: variant_url,
        widevine_psshs,
        fairplay_key,
        legacy: false,
    })
}

/// Pick the best HLS variant based on the requested quality tier.
fn select_variant_by_quality(
    variants: &[m3u8_rs::VariantStream],
    quality: AudioQuality,
) -> Option<&m3u8_rs::VariantStream> {
    let is_alac = |v: &m3u8_rs::VariantStream| v.codecs.as_ref().is_some_and(|c| c.contains("alac"));
    let is_aac = |v: &m3u8_rs::VariantStream| {
        v.codecs
            .as_ref()
            .is_some_and(|c| c.contains("mp4a") && !c.contains("alac"))
    };

    match quality {
        AudioQuality::Lossless => {
            // Prefer ALAC (highest bandwidth), fall back to highest AAC
            variants
                .iter()
                .filter(|v| is_alac(v))
                .max_by_key(|v| v.bandwidth)
                .or_else(|| variants.iter().filter(|v| is_aac(v)).max_by_key(|v| v.bandwidth))
        }
        AudioQuality::High => {
            // Highest AAC (256 kbps)
            variants.iter().filter(|v| is_aac(v)).max_by_key(|v| v.bandwidth)
        }
        AudioQuality::Standard => {
            // Lowest AAC
            variants.iter().filter(|v| is_aac(v)).min_by_key(|v| v.bandwidth)
        }
    }
}

/// Legacy flow: webplayback → 28:ctrp256 m3u8 → extract key URI
async fn get_stream_info_legacy(
    client: &reqwest::Client,
    dev_token: &str,
    mut_token: &str,
    track_id: &str,
) -> Result<StreamInfo, String> {
    let webplayback = get_webplayback(client, dev_token, mut_token, track_id).await?;

    let song = webplayback.song_list.first().ok_or("Empty songList in webplayback")?;

    // Prefer flavor "28:ctrp256" (Widevine CENC AAC 256kbps)
    let asset = song
        .assets
        .iter()
        .find(|a| a.flavor == "28:ctrp256")
        .or_else(|| song.assets.iter().find(|a| a.flavor.contains("ctrp")))
        .or_else(|| song.assets.first())
        .ok_or("No assets in webplayback")?;

    debug!("[AppleMusic] Legacy: flavor={}", asset.flavor);

    let m3u8_url = &asset.url;

    let m3u8_text = client
        .get(m3u8_url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch legacy m3u8: {e}"))?
        .text()
        .await
        .map_err(|e| format!("Failed to read legacy m3u8: {e}"))?;

    let mut widevine_psshs: Vec<String> = Vec::new();

    // Legacy m3u8 uses METHOD=ISO-23001-7 with a data URI containing raw key ID.
    // No Widevine UUID — just take the first #EXT-X-KEY URI (following gamdl).
    for line in m3u8_text.lines() {
        if !line.starts_with("#EXT-X-KEY") {
            continue;
        }
        if let Some(uri_start) = line.find("URI=\"") {
            let rest = &line[uri_start + 5..];
            if let Some(uri_end) = rest.find('"') {
                let uri = rest[..uri_end].to_string();
                if !widevine_psshs.contains(&uri) {
                    widevine_psshs.push(uri);
                }
            }
        }
    }

    debug!("[AppleMusic] Legacy: {} key URI(s) found", widevine_psshs.len());

    Ok(StreamInfo {
        stream_url: m3u8_url.clone(),
        widevine_psshs,
        fairplay_key: None,
        legacy: true,
    })
}

// ── Webplayback ─────────────────────────────────────────────────────────────

async fn get_webplayback(
    client: &reqwest::Client,
    dev_token: &str,
    mut_token: &str,
    track_id: &str,
) -> Result<WebplaybackResponse, String> {
    let resp = client
        .post(WEBPLAYBACK_URL)
        .header("Authorization", format!("Bearer {dev_token}"))
        .header("Media-User-Token", mut_token)
        .header("Origin", "https://music.apple.com")
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "salableAdamId": track_id,
            "language": "en-US",
        }))
        .send()
        .await
        .map_err(|e| format!("Webplayback request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Webplayback returned {status}: {body}"));
    }

    let body_text = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read webplayback body: {e}"))?;

    debug!(
        "[AppleMusic] Webplayback response (first 500 chars): {}",
        &body_text[..body_text.len().min(500)]
    );

    serde_json::from_str::<WebplaybackResponse>(&body_text).map_err(|e| {
        format!(
            "Failed to parse webplayback JSON: {e}\nBody: {}",
            &body_text[..body_text.len().min(200)]
        )
    })
}

// ── License exchange ────────────────────────────────────────────────────────

async fn exchange_license(
    client: &reqwest::Client,
    dev_token: &str,
    mut_token: &str,
    track_id: &str,
    track_uri: &str,
    challenge_b64: &str,
) -> Result<String, String> {
    let resp = client
        .post(LICENSE_URL)
        .header("Authorization", format!("Bearer {dev_token}"))
        .header("Media-User-Token", mut_token)
        .header("Origin", "https://music.apple.com")
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "challenge": challenge_b64,
            "key-system": "com.widevine.alpha",
            "uri": track_uri,
            "adamId": track_id,
            "isLibrary": false,
            "user-initiated": true,
        }))
        .send()
        .await
        .map_err(|e| format!("License exchange request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("License server returned {status}: {body}"));
    }

    let license_resp: LicenseResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse license response: {e}"))?;

    if license_resp.status != 0 {
        return Err(format!(
            "License exchange failed: status={}, errorCode={:?}",
            license_resp.status, license_resp.error_code
        ));
    }

    license_resp
        .license
        .ok_or_else(|| "No license field in response".to_string())
}

// ── PSSH extraction ─────────────────────────────────────────────────────────

/// Extract PSSH init data from a Widevine URI.
///
/// URI formats:
/// - `skd://itunes.apple.com/P.../s1/e1,BASE64_PSSH_DATA`
/// - Raw base64 PSSH data
fn extract_pssh_data(uri: &str) -> Result<Vec<u8>, String> {
    // Split by comma — the PSSH data is after the last comma
    let pssh_part = uri.rsplit(',').next().unwrap_or(uri);

    // Try to decode as base64
    let decoded = B64
        .decode(pssh_part)
        .map_err(|e| format!("Failed to base64-decode PSSH: {e}"))?;

    // Check if it's a full PSSH box (starts with "pssh" magic at offset 4)
    if decoded.len() > 32 && &decoded[4..8] == b"pssh" {
        // Extract the actual WidevinePsshData from the PSSH box
        // PSSH box: size(4) + "pssh"(4) + version(4) + systemid(16) + datalen(4) + data
        let data_offset = 32;
        if decoded.len() > data_offset + 4 {
            let data_len = u32::from_be_bytes([
                decoded[data_offset],
                decoded[data_offset + 1],
                decoded[data_offset + 2],
                decoded[data_offset + 3],
            ]) as usize;
            let data_start = data_offset + 4;
            if decoded.len() >= data_start + data_len {
                return Ok(decoded[data_start..data_start + data_len].to_vec());
            }
        }
    }

    // If it's already WidevinePsshData, check if it's a valid protobuf
    if decoded.len() > 2 {
        // Try to parse as WidevinePsshData protobuf
        if prost::Message::decode(&mut decoded.as_slice())
            .map(|_: super::widevine::WidevinePsshData| ())
            .is_ok()
        {
            return Ok(decoded);
        }
    }

    // Treat as raw key ID and build PSSH data
    Ok(cdm::build_pssh_data(&decoded))
}

// ── Session data parsing helpers ────────────────────────────────────────────

/// Resolve a library track ID (e.g. "i.NJv0A6kflGPVoBv") to a catalog Adam ID.
async fn resolve_library_to_catalog(
    client: &reqwest::Client,
    dev_token: &str,
    mut_token: &str,
    library_id: &str,
) -> Result<String, String> {
    let url = format!("{AMP_API_URL}/v1/me/library/songs/{library_id}/catalog");
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {dev_token}"))
        .header("Media-User-Token", mut_token)
        .header("Origin", "https://music.apple.com")
        .send()
        .await
        .map_err(|e| format!("Failed to resolve library ID: {e}"))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse library resolution: {e}"))?;

    let catalog_id = body
        .pointer("/data/0/id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("Could not resolve library ID {library_id} to catalog ID"))?;

    info!("[AppleMusic] Resolved library {library_id} → catalog {catalog_id}");
    Ok(catalog_id.to_string())
}

fn extract_widevine_from_session_data(json: &serde_json::Value) -> Option<String> {
    // AudioSessionKeyInfo is a dict of drm_id → { drm_type → { URI: "..." } }
    // drm_id "1" is the prefetch key — skip it (following gamdl's approach).
    // Only return non-prefetch Widevine PSSHs.
    if let Some(obj) = json.as_object() {
        let drm_ids: Vec<&String> = obj.keys().collect();
        debug!("[AppleMusic] Session data drm_ids: {:?}", drm_ids);
        for (drm_id, drm_info) in obj {
            if drm_id == "1" {
                debug!("[AppleMusic] Skipping prefetch drm_id=1");
                continue;
            }
            if let Some(wv) = drm_info.get("urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed")
                && let Some(uri) = wv.get("URI").and_then(|v| v.as_str())
            {
                debug!("[AppleMusic] Found content Widevine URI from drm_id={drm_id}");
                return Some(uri.to_string());
            }
        }
    }
    None
}

fn extract_fairplay_from_session_data(json: &serde_json::Value) -> Option<String> {
    if let Some(obj) = json.as_object() {
        for (_drm_id, drm_info) in obj {
            if let Some(fp) = drm_info.get("com.apple.streamingkeydelivery")
                && let Some(uri) = fp.get("URI").and_then(|v| v.as_str())
            {
                return Some(uri.to_string());
            }
        }
    }
    None
}
