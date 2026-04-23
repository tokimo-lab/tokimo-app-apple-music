//! `FairPlay` wrapper TCP client for ALAC lossless decryption.
//!
//! The wrapper is a sidecar service that handles `FairPlay` DRM decryption
//! using a white-box AES implementation. It communicates via a simple TCP
//! protocol:
//!
//! - Key load: `[1B id_len][id_bytes][1B key_len][key_bytes]`
//! - Key switch: `[0x00, 0x00, 0x00, 0x00]` (4 zero bytes)
//! - Sample decrypt: `[4B LE size][data]` → receive `[decrypted_data]` (same size)
//! - Close: `[0x00, 0x00, 0x00, 0x00, 0x00]` (5 zero bytes)
//!
//! CBCS truncation: only `size & !0xF` bytes are encrypted (16-byte aligned).

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, info};

/// Prefetch key URI used for `desc_index` 0 (first sample description).
const PREFETCH_KEY: &str = "skd://itunes.apple.com/P000000000/s1/e1";

/// Configuration for the `FairPlay` wrapper service.
#[derive(Debug, Clone)]
pub struct WrapperConfig {
    pub host: String,
    pub port: u16,
}

impl Default for WrapperConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 10020,
        }
    }
}

impl WrapperConfig {
    /// Load config from environment variables.
    pub fn from_env() -> Option<Self> {
        let enabled = std::env::var("FAIRPLAY_WRAPPER_ENABLED")
            .unwrap_or_default()
            .eq_ignore_ascii_case("true");

        if !enabled {
            return None;
        }

        Some(Self {
            host: std::env::var("FAIRPLAY_WRAPPER_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
            port: std::env::var("FAIRPLAY_WRAPPER_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10020),
        })
    }

    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// A sample to be decrypted, with its `desc_index` for key routing.
pub struct WrapperSample {
    pub data: Vec<u8>,
    pub duration: u32,
    pub desc_index: u32,
}

/// Decrypt ALAC samples via the `FairPlay` wrapper TCP service.
///
/// Returns a flat `Vec<u8>` of all decrypted sample data concatenated,
/// plus a `Vec<usize>` of individual sample sizes (for stsz).
pub async fn decrypt_samples_via_wrapper(
    config: &WrapperConfig,
    track_id: &str,
    fairplay_key: &str,
    samples: &[WrapperSample],
) -> Result<(Vec<u8>, Vec<usize>), String> {
    let mut stream = TcpStream::connect(config.addr())
        .await
        .map_err(|e| format!("Wrapper connect to {}: {e}", config.addr()))?;

    info!(
        "[FairPlay] Connected to wrapper at {}, decrypting {} samples for track {}",
        config.addr(),
        samples.len(),
        track_id
    );

    let keys: [&str; 2] = [PREFETCH_KEY, fairplay_key];
    let mut last_desc_idx: i32 = -1;
    let mut decrypted_data = Vec::with_capacity(samples.len() * 20_000);
    let mut sample_sizes = Vec::with_capacity(samples.len());

    for (i, sample) in samples.iter().enumerate() {
        let desc_idx = sample.desc_index as i32;

        // Switch key when desc_index changes
        if desc_idx != last_desc_idx {
            if last_desc_idx >= 0 {
                // Send key switch signal (4 zero bytes)
                stream
                    .write_all(&0u32.to_le_bytes())
                    .await
                    .map_err(|e| format!("Wrapper key switch: {e}"))?;
                stream.flush().await.map_err(|e| format!("Wrapper flush: {e}"))?;
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }

            // Send key info
            let key_uri = keys[sample.desc_index.min(1) as usize];
            let id_bytes: &[u8] = if sample.desc_index == 0 {
                b"0"
            } else {
                track_id.as_bytes()
            };

            let mut key_msg = Vec::with_capacity(2 + id_bytes.len() + key_uri.len());
            key_msg.push(id_bytes.len() as u8);
            key_msg.extend_from_slice(id_bytes);
            key_msg.push(key_uri.len() as u8);
            key_msg.extend_from_slice(key_uri.as_bytes());

            stream
                .write_all(&key_msg)
                .await
                .map_err(|e| format!("Wrapper send key: {e}"))?;
            stream.flush().await.map_err(|e| format!("Wrapper flush: {e}"))?;

            // Wait for FairPlay handshake
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            last_desc_idx = desc_idx;
            debug!(
                "[FairPlay] Key loaded: desc_idx={} ({})",
                sample.desc_index,
                if sample.desc_index == 0 { "prefetch" } else { "content" }
            );
        }

        // CBCS: truncate to 16-byte boundary
        let sample_len = sample.data.len();
        let aligned = sample_len & !0xF;

        if aligned >= 16 {
            // Send: [4B LE size][data]
            stream
                .write_all(&(aligned as u32).to_le_bytes())
                .await
                .map_err(|e| format!("Wrapper send sample {i}: {e}"))?;
            stream
                .write_all(&sample.data[..aligned])
                .await
                .map_err(|e| format!("Wrapper send data {i}: {e}"))?;
            stream.flush().await.map_err(|e| format!("Wrapper flush {i}: {e}"))?;

            // Receive decrypted data (same size)
            let mut dec_buf = vec![0u8; aligned];
            stream
                .read_exact(&mut dec_buf)
                .await
                .map_err(|e| format!("Wrapper recv sample {i}: {e}"))?;

            decrypted_data.extend_from_slice(&dec_buf);
            // Append unaligned tail (cleartext)
            if aligned < sample_len {
                decrypted_data.extend_from_slice(&sample.data[aligned..]);
            }
        } else {
            // Too small to encrypt, keep as-is
            decrypted_data.extend_from_slice(&sample.data);
        }

        sample_sizes.push(sample_len);

        if (i + 1) % 500 == 0 {
            debug!(
                "[FairPlay] Decrypted {}/{} samples ({} bytes)",
                i + 1,
                samples.len(),
                decrypted_data.len()
            );
        }
    }

    // Send close signal (5 zero bytes)
    stream
        .write_all(&[0u8; 5])
        .await
        .map_err(|e| format!("Wrapper close: {e}"))?;

    info!(
        "[FairPlay] Decrypted {} samples ({} bytes) for track {}",
        samples.len(),
        decrypted_data.len(),
        track_id
    );

    Ok((decrypted_data, sample_sizes))
}
