//! MP4 sample-level decryption for CENC/CBCS encrypted audio.
//!
//! Supports both:
//! - CENC (AES-128-CTR) — used by legacy AAC streams
//! - CBCS (AES-128-CBC) — used by modern ALAC/AAC streams
//!
//! Two output modes:
//! - `decrypt_fmp4_inplace()` — in-place patching of the fragmented MP4 (legacy)
//! - `decrypt_to_clean_m4a()` — extract samples, decrypt, reassemble as clean
//!   non-fragmented M4A (ftyp+moov+mdat). This is the preferred path for
//!   browser playback, following the gamdl approach.

use aes::cipher::{BlockDecryptMut, KeyIvInit, StreamCipher};
use tracing::debug;

/// Per-sample IV (16 bytes) paired with subsample ranges `(clear_bytes, encrypted_bytes)`.
type SencEntry = (Vec<u8>, Vec<(u32, u32)>);
type SencEntries = Vec<SencEntry>;

// Default key for desc_index 0 (prefetch samples)
const DEFAULT_SONG_KEY: [u8; 16] = [
    0x32, 0xb8, 0xad, 0xe1, 0x76, 0x9e, 0x26, 0xb1, 0xff, 0xb8, 0x98, 0x63, 0x52, 0x79, 0x3f, 0xc6,
];

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

/// Information about one MP4 sample.
#[derive(Debug)]
struct Sample {
    offset: usize,
    size: usize,
    _duration: u32,
    desc_index: u32,
    iv: Vec<u8>,
    subsamples: Vec<(u32, u32)>, // (clear_bytes, encrypted_bytes)
}

/// Encryption scheme info from sinf/schm + sinf/schi/tenc.
#[derive(Debug, Clone)]
struct EncryptionInfo {
    scheme_type: String, // "cenc" or "cbcs"
    per_sample_iv_size: u8,
    constant_iv: Vec<u8>,
    kid: [u8; 16], // Key ID from tenc
}

impl Default for EncryptionInfo {
    fn default() -> Self {
        Self {
            scheme_type: "cbcs".to_string(),
            per_sample_iv_size: 0,
            constant_iv: vec![0u8; 16],
            kid: [0u8; 16],
        }
    }
}

/// Decrypt an encrypted fragmented MP4 **in-place**.
///
/// After this call `data` is a valid clear fMP4: the mdat sample bytes
/// have been decrypted and the `enca` sample-entry type is patched to `mp4a`.
/// The container structure (box sizes, offsets) is unchanged because
/// AES-CTR / CBCS produce same-length output.
///
/// `content_keys` — list of (`kid_hex`, `key_hex`) pairs from the CDM.
/// For legacy (CENC) streams with a single key, pass one pair.
/// For modern (CBCS) streams with key rotation, pass all pairs.
pub fn decrypt_fmp4_inplace(data: &mut [u8], content_keys: &[(String, String)], legacy: bool) -> Result<(), String> {
    // Parse all encryption infos (one per stsd entry) to get KIDs
    let enc_infos = extract_all_encryption_info(data);

    // Build desc_index → key mapping
    let mut keys = std::collections::HashMap::<u32, Vec<u8>>::new();

    if legacy {
        // Legacy CENC: single key for all samples
        if let Some((_, key_hex)) = content_keys.first() {
            let key = hex::decode(key_hex).map_err(|e| format!("Invalid key hex: {e}"))?;
            keys.insert(0, key);
        }
    } else {
        // Modern CBCS: fixed mapping (gamdl approach)
        // desc_index 0 → DEFAULT_SONG_KEY (prefetch samples)
        // desc_index 1 → CDM content key (actual audio)
        keys.insert(0, DEFAULT_SONG_KEY.to_vec());

        if let Some((kid_hex, key_hex)) = content_keys.last() {
            let key = hex::decode(key_hex).map_err(|e| format!("Invalid key hex: {e}"))?;
            debug!("[AppleMusic] Mapped desc_index 1 → kid={}, key={}", kid_hex, key_hex);
            keys.insert(1, key);
        }
    }

    debug!(
        "[AppleMusic] Key map: {} entries ({} content keys provided, {} stsd entries)",
        keys.len(),
        content_keys.len(),
        enc_infos.len()
    );

    // Use first enc_info for scheme detection (all entries share the same scheme)
    let enc_info = enc_infos.first().cloned().unwrap_or_default();

    // Parse samples BEFORE modifying the buffer
    let (samples, _) = extract_samples(data)?;
    let is_cenc = enc_info.scheme_type == "cenc" || legacy;

    debug!(
        "[AppleMusic] In-place decrypt: {} samples, scheme={}",
        samples.len(),
        enc_info.scheme_type
    );

    for sample in &samples {
        let Some(key) = keys.get(&sample.desc_index) else {
            continue;
        };

        let sample_data = data[sample.offset..sample.offset + sample.size].to_vec();

        let decrypted = if is_cenc {
            decrypt_cenc(key, &sample.iv, &sample_data, &sample.subsamples)?
        } else {
            let iv = if sample.iv.is_empty() {
                &enc_info.constant_iv
            } else {
                &sample.iv
            };
            decrypt_cbcs(key, iv, &sample_data, &sample.subsamples)?
        };

        data[sample.offset..sample.offset + decrypted.len()].copy_from_slice(&decrypted);
    }

    // Patch sample-entry type: enca → mp4a
    patch_enca_to_mp4a(data);

    debug!("[AppleMusic] In-place decrypt complete");
    Ok(())
}

/// Decrypt an encrypted MP4 file using provided content keys.
///
/// - `encrypted_data`: the raw encrypted MP4 bytes
/// - `content_keys`: list of (`kid_hex`, `key_hex`) pairs from CDM
/// - `legacy`: if true, use CENC mode; otherwise CBCS
///
/// Returns the decrypted audio data (raw samples, not re-muxed).
pub fn decrypt_mp4_samples(
    encrypted_data: &[u8],
    content_keys: &[(String, String)],
    legacy: bool,
) -> Result<Vec<u8>, String> {
    // Build key mapping using KID-based matching (same logic as decrypt_fmp4_inplace)
    let enc_infos = extract_all_encryption_info(encrypted_data);
    let mut keys = std::collections::HashMap::<u32, Vec<u8>>::new();

    if legacy {
        if let Some((_, key_hex)) = content_keys.first() {
            let key = hex::decode(key_hex).map_err(|e| format!("Invalid key hex: {e}"))?;
            keys.insert(0, key);
        }
    } else {
        keys.insert(0, DEFAULT_SONG_KEY.to_vec());
        for (idx, info) in enc_infos.iter().enumerate() {
            let kid_hex = hex::encode(info.kid);
            if let Some((_, key_hex)) = content_keys.iter().find(|(k, _)| *k == kid_hex) {
                let key = hex::decode(key_hex).map_err(|e| format!("Invalid key hex: {e}"))?;
                keys.insert(idx as u32, key);
            } else {
                keys.entry(idx as u32).or_insert_with(|| DEFAULT_SONG_KEY.to_vec());
            }
        }
    }

    let enc_info = enc_infos.first().cloned().unwrap_or_default();

    // Parse fragmented MP4
    let (samples, _) = extract_samples(encrypted_data)?;

    let is_cenc = enc_info.scheme_type == "cenc" || legacy;

    // Decrypt each sample
    let mut decrypted = Vec::with_capacity(encrypted_data.len());

    for sample in &samples {
        let Some(key) = keys.get(&sample.desc_index) else {
            // No key for this desc_index, keep as-is
            decrypted.extend_from_slice(&encrypted_data[sample.offset..sample.offset + sample.size]);
            continue;
        };

        let sample_data = &encrypted_data[sample.offset..sample.offset + sample.size];

        if is_cenc {
            let decrypted_sample = decrypt_cenc(key, &sample.iv, sample_data, &sample.subsamples)?;
            decrypted.extend_from_slice(&decrypted_sample);
        } else {
            let iv = if sample.iv.is_empty() {
                &enc_info.constant_iv
            } else {
                &sample.iv
            };
            let decrypted_sample = decrypt_cbcs(key, iv, sample_data, &sample.subsamples)?;
            decrypted.extend_from_slice(&decrypted_sample);
        }
    }

    debug!("[AppleMusic] Decrypted {} bytes total", decrypted.len());
    Ok(decrypted)
}

// ── Clean M4A reassembly (gamdl approach) ───────────────────────────────────

/// A sample with its data copied out of the fMP4, ready for decryption.
#[derive(Debug)]
struct SampleData {
    data: Vec<u8>,
    duration: u32,
    desc_index: u32,
    iv: Vec<u8>,
    subsamples: Vec<(u32, u32)>,
}

/// Extracted song info from an encrypted fragmented MP4.
struct SongInfo {
    #[allow(dead_code)]
    ftyp: Vec<u8>,
    moov: Vec<u8>,
    samples: Vec<SampleData>,
    enc_infos: Vec<EncryptionInfo>,
}

/// Decrypt an encrypted fragmented MP4 and produce a clean non-fragmented M4A.
///
/// This follows the gamdl approach:
/// 1. Parse fMP4 → extract ftyp, moov, and individual samples
/// 2. Decrypt each sample using the appropriate key
/// 3. Reassemble into a clean M4A: ftyp + moov (with stts/stsc/stsz/stco) + mdat
///
/// The output is universally playable by browsers (unlike patched fMP4).
pub fn decrypt_to_clean_m4a(
    encrypted_data: &[u8],
    content_keys: &[(String, String)],
    legacy: bool,
) -> Result<Vec<u8>, String> {
    // 1. Extract song info
    let song_info = extract_song(encrypted_data);

    // 2. Build key mapping — follow gamdl's fixed mapping approach.
    //    Apple Music stsd entries have all-zero KIDs (FairPlay, not Widevine),
    //    so KID-based matching doesn't work. Instead:
    //      desc_index 0 → DEFAULT_SONG_KEY (prefetch samples)
    //      desc_index 1 → CDM content key (actual audio)
    let mut keys = std::collections::HashMap::<u32, Vec<u8>>::new();

    if legacy {
        // Legacy CENC: single key for all samples
        if let Some((_, key_hex)) = content_keys.first() {
            let key = hex::decode(key_hex).map_err(|e| format!("Invalid key hex: {e}"))?;
            keys.insert(0, key);
        }
    } else {
        keys.insert(0, DEFAULT_SONG_KEY.to_vec());
        // Use the last CDM content key for desc_index 1
        // (first PSSH may be for prefetch; the content PSSH is typically last)
        if let Some((kid_hex, key_hex)) = content_keys.last() {
            let key = hex::decode(key_hex).map_err(|e| format!("Invalid key hex: {e}"))?;
            debug!("[AppleMusic] Key map: desc_index 1 → kid={}, key={}", kid_hex, key_hex);
            keys.insert(1, key);
        }
    }

    let enc_info = song_info.enc_infos.first().cloned().unwrap_or_default();
    let is_cenc = enc_info.scheme_type == "cenc" || legacy;

    // Debug: dump raw content_keys from CDM
    for (i, (kid, key)) in content_keys.iter().enumerate() {
        debug!("[AppleMusic] CDM content_key[{}]: kid={}, key={}", i, kid, key);
    }

    // Debug: dump enc_infos details
    for (i, ei) in song_info.enc_infos.iter().enumerate() {
        debug!(
            "[AppleMusic] enc_info[{}]: scheme={}, iv_size={}, constant_iv={}, kid={}",
            i,
            ei.scheme_type,
            ei.per_sample_iv_size,
            hex::encode(&ei.constant_iv),
            hex::encode(ei.kid)
        );
    }

    // Debug: dump key mapping
    for (di, key) in &keys {
        debug!("[AppleMusic] keys[{}] = {}", di, hex::encode(key));
    }

    // Debug: dump desc_index distribution
    let mut desc_counts = std::collections::HashMap::<u32, usize>::new();
    let mut sub_counts = std::collections::HashMap::<u32, usize>::new();
    for s in &song_info.samples {
        *desc_counts.entry(s.desc_index).or_default() += 1;
        if !s.subsamples.is_empty() {
            *sub_counts.entry(s.desc_index).or_default() += 1;
        }
    }
    for (di, count) in &desc_counts {
        let sub = sub_counts.get(di).copied().unwrap_or(0);
        debug!(
            "[AppleMusic] desc_index {}: {} samples ({} with subsamples)",
            di, count, sub
        );
    }

    debug!(
        "[AppleMusic] Clean M4A: {} samples, scheme={}, {} keys",
        song_info.samples.len(),
        enc_info.scheme_type,
        keys.len()
    );

    // 3. Decrypt each sample
    let mut decrypted_samples: Vec<Vec<u8>> = Vec::with_capacity(song_info.samples.len());
    let mut decrypted_data = Vec::new();

    for sample in &song_info.samples {
        let Some(key) = keys.get(&sample.desc_index) else {
            // No key, keep as-is
            decrypted_data.extend_from_slice(&sample.data);
            decrypted_samples.push(sample.data.clone());
            continue;
        };

        let decrypted = if is_cenc {
            decrypt_cenc(key, &sample.iv, &sample.data, &sample.subsamples)?
        } else {
            let iv = if sample.iv.is_empty() {
                // Per-desc encryption info lookup
                let desc_enc = song_info.enc_infos.get(sample.desc_index as usize).unwrap_or(&enc_info);
                if desc_enc.constant_iv.is_empty() {
                    &enc_info.constant_iv
                } else {
                    &desc_enc.constant_iv
                }
            } else {
                &sample.iv
            };
            decrypt_cbcs(key, iv, &sample.data, &sample.subsamples)?
        };

        decrypted_data.extend_from_slice(&decrypted);
        decrypted_samples.push(decrypted);
    }

    debug!(
        "[AppleMusic] Decrypted {} bytes from {} samples",
        decrypted_data.len(),
        song_info.samples.len()
    );

    // 4. Write clean non-fragmented M4A
    let output = write_clean_m4a(&song_info, &decrypted_samples, &decrypted_data);

    debug!("[AppleMusic] Clean M4A output: {} bytes", output.len());

    Ok(output)
}

/// Decrypt an encrypted fragmented MP4 via the `FairPlay` wrapper service.
///
/// This is the ALAC lossless path. Instead of local AES decryption, samples
/// are sent to a sidecar wrapper process over TCP for `FairPlay` decryption.
/// The clean M4A reassembly is identical to the Widevine path.
pub async fn decrypt_to_clean_m4a_via_wrapper(
    encrypted_data: &[u8],
    wrapper_config: &super::wrapper_client::WrapperConfig,
    track_id: &str,
    fairplay_key: &str,
) -> Result<Vec<u8>, String> {
    use super::wrapper_client::{WrapperSample, decrypt_samples_via_wrapper};

    // 1. Extract song structure (same as Widevine path)
    let song_info = extract_song(encrypted_data);

    debug!(
        "[AppleMusic] FairPlay decrypt: {} samples from {} fragments",
        song_info.samples.len(),
        song_info.samples.last().map_or(0, |s| s.desc_index)
    );

    // 2. Convert samples for the wrapper client
    let wrapper_samples: Vec<WrapperSample> = song_info
        .samples
        .iter()
        .map(|s| WrapperSample {
            data: s.data.clone(),
            duration: s.duration,
            desc_index: s.desc_index,
        })
        .collect();

    // 3. Decrypt via wrapper TCP
    let (decrypted_data, sample_sizes) =
        decrypt_samples_via_wrapper(wrapper_config, track_id, fairplay_key, &wrapper_samples).await?;

    // 4. Split decrypted data back into individual samples
    let mut decrypted_samples: Vec<Vec<u8>> = Vec::with_capacity(sample_sizes.len());
    let mut offset = 0;
    for &size in &sample_sizes {
        decrypted_samples.push(decrypted_data[offset..offset + size].to_vec());
        offset += size;
    }

    // 5. Write clean M4A (same as Widevine path)
    let output = write_clean_m4a(&song_info, &decrypted_samples, &decrypted_data);

    debug!(
        "[AppleMusic] FairPlay clean M4A: {} bytes ({} samples)",
        output.len(),
        sample_sizes.len()
    );

    Ok(output)
}

/// Extract song info (ftyp, moov, samples) from an encrypted fragmented MP4.
fn extract_song(data: &[u8]) -> SongInfo {
    let mut ftyp = Vec::new();
    let mut moov = Vec::new();
    let mut enc_infos = Vec::new();

    // First pass: collect top-level boxes
    let mut offset = 0;
    while offset + 8 <= data.len() {
        let Some((size, btype, _header_size)) = read_box_header(data, offset) else {
            break;
        };
        if size == 0 || offset + size > data.len() {
            break;
        }
        match btype.as_str() {
            "ftyp" => ftyp = data[offset..offset + size].to_vec(),
            "moov" => moov = data[offset..offset + size].to_vec(),
            _ => {}
        }
        offset += size;
    }

    // Extract encryption infos from moov
    if !moov.is_empty() {
        enc_infos = extract_all_encryption_info(data);
    }

    let enc_info_first = enc_infos.first().cloned().unwrap_or_default();

    // Find audio track ID from moov
    let audio_track_id = if moov.is_empty() {
        1
    } else {
        extract_audio_track_id(&moov).unwrap_or(1)
    };

    // Get trex defaults
    let (default_duration, default_size) = if moov.is_empty() {
        (1024, 0)
    } else {
        extract_trex_defaults(&moov, audio_track_id)
    };

    // Second pass: extract samples from moof+mdat pairs
    let mut samples = Vec::new();
    let mut moof_range: Option<(usize, usize)> = None;
    offset = 0;

    while offset + 8 <= data.len() {
        let Some((size, btype, header_size)) = read_box_header(data, offset) else {
            break;
        };
        if size == 0 || offset + size > data.len() {
            break;
        }

        match btype.as_str() {
            "moof" => {
                moof_range = Some((offset, size));
            }
            "mdat" => {
                if let Some((moof_off, moof_sz)) = moof_range.take() {
                    let moof_data = &data[moof_off..moof_off + moof_sz];
                    let mdat_content = &data[offset + header_size..offset + size];
                    extract_samples_from_moof_mdat(
                        moof_data,
                        mdat_content,
                        audio_track_id,
                        moof_off,
                        offset + header_size,
                        &enc_info_first,
                        default_duration,
                        default_size,
                        &mut samples,
                    );
                }
            }
            _ => {}
        }
        offset += size;
    }

    debug!(
        "[AppleMusic] Extracted {} samples from fMP4 ({} bytes ftyp, {} bytes moov)",
        samples.len(),
        ftyp.len(),
        moov.len()
    );

    SongInfo {
        ftyp,
        moov,
        samples,
        enc_infos,
    }
}

/// Parse a moof+mdat pair and extract samples with their actual data.
#[allow(clippy::too_many_arguments)]
fn extract_samples_from_moof_mdat(
    moof_box: &[u8],
    mdat_content: &[u8],
    audio_track_id: u32,
    moof_offset: usize,
    mdat_data_offset: usize,
    enc_info: &EncryptionInfo,
    default_duration: u32,
    default_size: u32,
    samples: &mut Vec<SampleData>,
) {
    // Skip moof box header (8 bytes)
    let moof = &moof_box[8..];

    // Find all traf boxes
    let mut traf_offset = 0;
    while traf_offset + 8 <= moof.len() {
        let Some((traf_size, traf_type, _)) = read_box_header(moof, traf_offset) else {
            break;
        };
        if traf_size == 0 {
            break;
        }

        if traf_type == "traf" {
            let traf = &moof[traf_offset + 8..(traf_offset + traf_size).min(moof.len())];

            // Parse tfhd
            let mut track_id = 0u32;
            let mut desc_index = 0u32;
            let mut tf_default_duration = default_duration;
            let mut tf_default_size = default_size;
            let mut base_data_offset: Option<u64> = None;

            if let Some((tfhd_start, tfhd_size)) = find_child_box(traf, "tfhd") {
                let tfhd = &traf[tfhd_start..tfhd_start + tfhd_size];
                if tfhd.len() >= 8 {
                    let flags = u32::from_be_bytes([0, tfhd[1], tfhd[2], tfhd[3]]);
                    track_id = read_u32(tfhd, 4);
                    let mut pos = 8;
                    if flags & 0x01 != 0 && pos + 8 <= tfhd.len() {
                        base_data_offset = Some(u64::from_be_bytes([
                            tfhd[pos],
                            tfhd[pos + 1],
                            tfhd[pos + 2],
                            tfhd[pos + 3],
                            tfhd[pos + 4],
                            tfhd[pos + 5],
                            tfhd[pos + 6],
                            tfhd[pos + 7],
                        ]));
                        pos += 8;
                    }
                    if flags & 0x02 != 0 && pos + 4 <= tfhd.len() {
                        desc_index = read_u32(tfhd, pos);
                        pos += 4;
                    }
                    if flags & 0x08 != 0 && pos + 4 <= tfhd.len() {
                        tf_default_duration = read_u32(tfhd, pos);
                        pos += 4;
                    }
                    if flags & 0x10 != 0 && pos + 4 <= tfhd.len() {
                        tf_default_size = read_u32(tfhd, pos);
                    }
                }
            }

            // Skip non-audio tracks
            if track_id != audio_track_id {
                traf_offset += traf_size;
                continue;
            }

            // Parse trun
            let mut trun_entries: Vec<(u32, u32)> = Vec::new(); // (duration, size)
            let mut trun_data_offset: Option<i32> = None;

            if let Some((trun_start, trun_size)) = find_child_box(traf, "trun") {
                let trun = &traf[trun_start..trun_start + trun_size];
                if trun.len() >= 8 {
                    let flags = u32::from_be_bytes([0, trun[1], trun[2], trun[3]]);
                    let sample_count = read_u32(trun, 4) as usize;
                    let mut pos = 8;

                    if flags & 0x01 != 0 && pos + 4 <= trun.len() {
                        trun_data_offset = Some(i32::from_be_bytes([
                            trun[pos],
                            trun[pos + 1],
                            trun[pos + 2],
                            trun[pos + 3],
                        ]));
                        pos += 4;
                    }
                    if flags & 0x04 != 0 {
                        pos += 4;
                    }

                    for _ in 0..sample_count {
                        let dur = if flags & 0x100 != 0 && pos + 4 <= trun.len() {
                            let d = read_u32(trun, pos);
                            pos += 4;
                            d
                        } else {
                            tf_default_duration
                        };
                        let sz = if flags & 0x200 != 0 && pos + 4 <= trun.len() {
                            let s = read_u32(trun, pos);
                            pos += 4;
                            s
                        } else {
                            tf_default_size
                        };
                        if flags & 0x400 != 0 {
                            pos += 4;
                        }
                        if flags & 0x800 != 0 {
                            pos += 4;
                        }
                        trun_entries.push((dur, sz));
                    }
                }
            }

            // Parse senc
            let mut senc_entries: SencEntries = Vec::new();
            if let Some((senc_start, senc_size)) = find_child_box(traf, "senc") {
                let senc = &traf[senc_start..senc_start + senc_size];
                if senc.len() >= 8 {
                    let flags = u32::from_be_bytes([0, senc[1], senc[2], senc[3]]);
                    let count = read_u32(senc, 4) as usize;
                    let iv_size = enc_info.per_sample_iv_size as usize;
                    let has_subsample_info = flags & 0x02 != 0;
                    let mut pos = 8;

                    for _ in 0..count {
                        let iv = if iv_size > 0 && pos + iv_size <= senc.len() {
                            let v = senc[pos..pos + iv_size].to_vec();
                            pos += iv_size;
                            v
                        } else {
                            Vec::new()
                        };
                        let mut subs = Vec::new();
                        if has_subsample_info && pos + 2 <= senc.len() {
                            let sub_count = read_u16(senc, pos) as usize;
                            pos += 2;
                            for _ in 0..sub_count {
                                if pos + 6 <= senc.len() {
                                    let clear = u32::from(read_u16(senc, pos));
                                    pos += 2;
                                    let enc = read_u32(senc, pos);
                                    pos += 4;
                                    subs.push((clear, enc));
                                }
                            }
                        }
                        senc_entries.push((iv, subs));
                    }
                }
            }

            // Compute mdat read offset
            let base = base_data_offset.unwrap_or(moof_offset as u64);
            let data_start = if let Some(off) = trun_data_offset {
                (base as i64 + i64::from(off)) as usize
            } else {
                mdat_data_offset
            };
            let mut read_off = data_start.saturating_sub(mdat_data_offset);

            let di = if desc_index > 0 { desc_index - 1 } else { 0 };

            for (i, (duration, size)) in trun_entries.iter().enumerate() {
                let sz = *size as usize;
                if sz > 0 && read_off + sz <= mdat_content.len() {
                    let (iv, subsamples) = if i < senc_entries.len() {
                        senc_entries[i].clone()
                    } else {
                        (Vec::new(), Vec::new())
                    };

                    samples.push(SampleData {
                        data: mdat_content[read_off..read_off + sz].to_vec(),
                        duration: *duration,
                        desc_index: di,
                        iv,
                        subsamples,
                    });
                }
                read_off += sz;
            }
        }

        traf_offset += traf_size;
    }
}

/// Extract the audio track ID from moov.
fn extract_audio_track_id(moov: &[u8]) -> Option<u32> {
    // Skip moov header
    let mut offset = 8;
    while offset + 8 <= moov.len() {
        let (size, btype, _) = read_box_header(moov, offset)?;
        if size == 0 || offset + size > moov.len() {
            break;
        }
        if btype == "trak" {
            let trak = &moov[offset..offset + size];
            // Check handler type
            if let Some(hdlr_pos) = find_bytes(trak, b"hdlr") {
                let handler_off = hdlr_pos + 4 + 4 + 4; // after 'hdlr' + version+flags + pre_defined
                if handler_off + 4 <= trak.len() && &trak[handler_off..handler_off + 4] == b"soun" {
                    // Extract track_id from tkhd
                    if let Some(tkhd_pos) = find_bytes(trak, b"tkhd") {
                        let version = trak[tkhd_pos + 4];
                        let tid_off = if version == 0 {
                            tkhd_pos + 4 + 4 + 4 + 4
                        } else {
                            tkhd_pos + 4 + 4 + 8 + 8
                        };
                        if tid_off + 4 <= trak.len() {
                            return Some(read_u32(trak, tid_off));
                        }
                    }
                }
            }
        }
        offset += size;
    }
    None
}

/// Find a byte pattern in data, returning the offset right after the pattern.
fn find_bytes(data: &[u8], pattern: &[u8]) -> Option<usize> {
    data.windows(pattern.len())
        .position(|w| w == pattern)
        .map(|p| p + pattern.len())
}

/// Extract trex defaults from moov.
fn extract_trex_defaults(moov: &[u8], target_track_id: u32) -> (u32, u32) {
    // Find mvex → trex
    if let Some((mvex_start, mvex_size)) = find_child_box_in(&moov[8..], "mvex") {
        let mvex = &moov[8 + mvex_start..8 + mvex_start + mvex_size];
        let mut offset = 0;
        while offset + 8 <= mvex.len() {
            let Some((size, btype, _)) = read_box_header(mvex, offset) else {
                break;
            };
            if size == 0 {
                break;
            }
            if btype == "trex" && size >= 32 {
                let trex = &mvex[offset..offset + size];
                let tid = read_u32(trex, 12);
                if target_track_id == 0 || tid == target_track_id {
                    let default_duration = read_u32(trex, 20);
                    let default_size = read_u32(trex, 24);
                    return (default_duration, default_size);
                }
            }
            offset += size;
        }
    }
    (1024, 0)
}

/// Find a child box in a container (content only, no header skip).
fn find_child_box_in(data: &[u8], box_type: &str) -> Option<(usize, usize)> {
    find_child_box(data, box_type)
}

/// Extract timescale from moov → trak(audio) → mdia → mdhd.
fn extract_timescale(moov: &[u8]) -> u32 {
    if let Some(mdhd_pos) = find_bytes(moov, b"mdhd") {
        let version = moov.get(mdhd_pos).copied().unwrap_or(0);
        let ts_off = if version == 0 {
            mdhd_pos + 4 + 4 + 4 // version+flags(4) + creation(4) + modification(4)
        } else {
            mdhd_pos + 4 + 8 + 8 // version+flags(4) + creation(8) + modification(8)
        };
        if ts_off + 4 <= moov.len() {
            return read_u32(moov, ts_off);
        }
    }
    44100
}

/// Write a clean non-fragmented M4A from decrypted samples.
///
/// Structure: ftyp + moov (with sample tables) + mdat
fn write_clean_m4a(song_info: &SongInfo, decrypted_samples: &[Vec<u8>], all_decrypted: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(all_decrypted.len() + 4096);

    // 1. Write ftyp
    write_ftyp(&mut out);

    // 2. Extract original boxes from moov for faithful reproduction
    let moov = &song_info.moov;
    let timescale = if moov.is_empty() {
        44100
    } else {
        extract_timescale(moov)
    };
    let total_duration: u64 = song_info.samples.iter().map(|s| u64::from(s.duration)).sum();

    // Extract cleaned stsd from original moov
    let clean_stsd = if moov.is_empty() {
        None
    } else {
        extract_and_clean_stsd(moov)
    };

    // Extract original boxes
    let orig_mvhd = if moov.is_empty() {
        None
    } else {
        find_child_box_raw(moov, *b"mvhd", 8)
    };
    let orig_trak = find_audio_trak_raw(moov);
    let (orig_tkhd, orig_mdhd, orig_hdlr, orig_smhd, orig_dinf) = if let Some(ref trak) = orig_trak {
        (
            find_child_box_raw(trak, *b"tkhd", 8),
            find_mdhd_in_trak(trak),
            find_hdlr_in_trak(trak),
            find_smhd_in_trak(trak),
            find_dinf_in_trak(trak),
        )
    } else {
        (None, None, None, None, None)
    };

    // 3. Build moov
    let moov_start = out.len();
    out.extend_from_slice(&[0u8; 8]); // placeholder

    // mvhd
    if let Some(ref mvhd) = orig_mvhd {
        let patched = patch_duration_in_box(mvhd, *b"mvhd", total_duration as u32);
        out.extend_from_slice(&patched);
    } else {
        write_default_mvhd(&mut out, timescale, total_duration as u32);
    }

    // trak
    let trak_start = out.len();
    out.extend_from_slice(&[0u8; 8]);

    // tkhd
    if let Some(ref tkhd) = orig_tkhd {
        let patched = patch_duration_in_box(tkhd, *b"tkhd", total_duration as u32);
        out.extend_from_slice(&patched);
    } else {
        write_default_tkhd(&mut out, total_duration as u32);
    }

    // mdia
    let mdia_start = out.len();
    out.extend_from_slice(&[0u8; 8]);

    // mdhd
    if let Some(ref mdhd) = orig_mdhd {
        let patched = patch_duration_in_box(mdhd, *b"mdhd", total_duration as u32);
        out.extend_from_slice(&patched);
    } else {
        write_default_mdhd(&mut out, timescale, total_duration as u32);
    }

    // hdlr
    if let Some(ref hdlr) = orig_hdlr {
        out.extend_from_slice(hdlr);
    } else {
        write_default_hdlr(&mut out);
    }

    // minf
    let minf_start = out.len();
    out.extend_from_slice(&[0u8; 8]);

    // smhd
    if let Some(ref smhd) = orig_smhd {
        out.extend_from_slice(smhd);
    } else {
        write_default_smhd(&mut out);
    }

    // dinf
    if let Some(ref dinf) = orig_dinf {
        out.extend_from_slice(dinf);
    } else {
        write_default_dinf(&mut out);
    }

    // stbl
    let stbl_start = out.len();
    out.extend_from_slice(&[0u8; 8]);

    // stsd
    if let Some(ref stsd) = clean_stsd {
        write_box(&mut out, *b"stsd", stsd);
    } else {
        write_default_stsd_aac(&mut out);
    }

    // stts (time-to-sample, run-length encoded)
    write_stts(&mut out, &song_info.samples);

    // stsc (sample-to-chunk — all in one chunk)
    let stsc_content = {
        let mut c = Vec::new();
        c.extend_from_slice(&0u32.to_be_bytes()); // version + flags
        c.extend_from_slice(&1u32.to_be_bytes()); // entry_count
        c.extend_from_slice(&1u32.to_be_bytes()); // first_chunk
        c.extend_from_slice(&(song_info.samples.len() as u32).to_be_bytes());
        c.extend_from_slice(&1u32.to_be_bytes()); // sample_description_index
        c
    };
    write_fullbox(&mut out, *b"stsc", &stsc_content);

    // stsz (sample sizes)
    let stsz_content = {
        let mut c = Vec::new();
        c.extend_from_slice(&0u32.to_be_bytes()); // version + flags
        c.extend_from_slice(&0u32.to_be_bytes()); // sample_size = 0 (variable)
        c.extend_from_slice(&(decrypted_samples.len() as u32).to_be_bytes());
        for s in decrypted_samples {
            c.extend_from_slice(&(s.len() as u32).to_be_bytes());
        }
        c
    };
    write_fullbox(&mut out, *b"stsz", &stsz_content);

    // stco (chunk offset — placeholder, fix up after writing mdat header)
    let stco_offset = out.len();
    let stco_content = {
        let mut c = Vec::new();
        c.extend_from_slice(&0u32.to_be_bytes()); // version + flags
        c.extend_from_slice(&1u32.to_be_bytes()); // entry_count
        c.extend_from_slice(&0u32.to_be_bytes()); // chunk_offset (placeholder)
        c
    };
    write_fullbox(&mut out, *b"stco", &stco_content);

    // Fix up container box sizes
    fixup_box_size(&mut out, stbl_start, *b"stbl");
    fixup_box_size(&mut out, minf_start, *b"minf");
    fixup_box_size(&mut out, mdia_start, *b"mdia");
    fixup_box_size(&mut out, trak_start, *b"trak");
    fixup_box_size(&mut out, moov_start, *b"moov");

    // 4. Write mdat
    let mdat_data_offset = out.len() + 8; // after mdat header
    let mdat_size = all_decrypted.len() + 8;
    out.extend_from_slice(&(mdat_size as u32).to_be_bytes());
    out.extend_from_slice(b"mdat");
    out.extend_from_slice(all_decrypted);

    // 5. Fix up stco to point to mdat data
    // stco_offset is the start of the stco box
    // stco box: size(4) + type(4) + version+flags(4) + entry_count(4) + offset(4)
    let stco_chunk_offset_pos = stco_offset + 4 + 4 + 4 + 4;
    out[stco_chunk_offset_pos..stco_chunk_offset_pos + 4].copy_from_slice(&(mdat_data_offset as u32).to_be_bytes());

    out
}

// ── M4A writer helpers ──────────────────────────────────────────────────────

fn write_ftyp(out: &mut Vec<u8>) {
    let mut content = Vec::new();
    content.extend_from_slice(b"M4A "); // major brand
    content.extend_from_slice(&0u32.to_be_bytes()); // minor version
    content.extend_from_slice(b"M4A mp42isom"); // compatible brands
    content.extend_from_slice(&[0u8; 4]); // padding
    write_box(out, *b"ftyp", &content);
}

fn write_box(out: &mut Vec<u8>, box_type: [u8; 4], content: &[u8]) {
    let size = (content.len() + 8) as u32;
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(&box_type);
    out.extend_from_slice(content);
}

fn write_fullbox(out: &mut Vec<u8>, box_type: [u8; 4], content: &[u8]) {
    // content already includes version(1) + flags(3) at the start
    let size = (content.len() + 8) as u32;
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(&box_type);
    out.extend_from_slice(content);
}

fn fixup_box_size(out: &mut [u8], start: usize, box_type: [u8; 4]) {
    let end = out.len();
    let size = (end - start) as u32;
    out[start..start + 4].copy_from_slice(&size.to_be_bytes());
    out[start + 4..start + 8].copy_from_slice(&box_type);
}

fn write_stts(out: &mut Vec<u8>, samples: &[SampleData]) {
    // Run-length encode durations
    let mut entries: Vec<(u32, u32)> = Vec::new();
    for sample in samples {
        if let Some(last) = entries.last_mut()
            && last.1 == sample.duration
        {
            last.0 += 1;
            continue;
        }
        entries.push((1, sample.duration));
    }

    let mut content = Vec::new();
    content.extend_from_slice(&0u32.to_be_bytes()); // version + flags
    content.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for (count, delta) in &entries {
        content.extend_from_slice(&count.to_be_bytes());
        content.extend_from_slice(&delta.to_be_bytes());
    }
    write_fullbox(out, *b"stts", &content);
}

fn write_default_mvhd(out: &mut Vec<u8>, timescale: u32, duration: u32) {
    let mut c = Vec::new();
    c.extend_from_slice(&0u32.to_be_bytes()); // version + flags
    c.extend_from_slice(&0u32.to_be_bytes()); // creation_time
    c.extend_from_slice(&0u32.to_be_bytes()); // modification_time
    c.extend_from_slice(&timescale.to_be_bytes());
    c.extend_from_slice(&duration.to_be_bytes());
    c.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // rate = 1.0
    c.extend_from_slice(&0x0100u16.to_be_bytes()); // volume = 1.0
    c.extend_from_slice(&[0u8; 10]); // reserved
    // matrix (9 × u32)
    for &v in &[0x0001_0000u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000] {
        c.extend_from_slice(&v.to_be_bytes());
    }
    c.extend_from_slice(&[0u8; 24]); // pre_defined
    c.extend_from_slice(&2u32.to_be_bytes()); // next_track_id
    write_fullbox(out, *b"mvhd", &c);
}

fn write_default_tkhd(out: &mut Vec<u8>, duration: u32) {
    let mut c = Vec::new();
    c.extend_from_slice(&7u32.to_be_bytes()); // version(0) + flags(7: enabled|in_movie|in_preview)
    c.extend_from_slice(&0u32.to_be_bytes()); // creation_time
    c.extend_from_slice(&0u32.to_be_bytes()); // modification_time
    c.extend_from_slice(&1u32.to_be_bytes()); // track_id
    c.extend_from_slice(&0u32.to_be_bytes()); // reserved
    c.extend_from_slice(&duration.to_be_bytes());
    c.extend_from_slice(&[0u8; 8]); // reserved
    c.extend_from_slice(&0u16.to_be_bytes()); // layer
    c.extend_from_slice(&0u16.to_be_bytes()); // alternate_group
    c.extend_from_slice(&0x0100u16.to_be_bytes()); // volume
    c.extend_from_slice(&0u16.to_be_bytes()); // reserved
    for &v in &[0x0001_0000u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000] {
        c.extend_from_slice(&v.to_be_bytes());
    }
    c.extend_from_slice(&0u32.to_be_bytes()); // width
    c.extend_from_slice(&0u32.to_be_bytes()); // height
    // Note: write_fullbox will write version+flags from the content
    // But tkhd is special — we already included version+flags in content
    let size = (c.len() + 8) as u32;
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(b"tkhd");
    out.extend_from_slice(&c);
}

fn write_default_mdhd(out: &mut Vec<u8>, timescale: u32, duration: u32) {
    let mut c = Vec::new();
    c.extend_from_slice(&0u32.to_be_bytes()); // version + flags
    c.extend_from_slice(&0u32.to_be_bytes()); // creation_time
    c.extend_from_slice(&0u32.to_be_bytes()); // modification_time
    c.extend_from_slice(&timescale.to_be_bytes());
    c.extend_from_slice(&duration.to_be_bytes());
    c.extend_from_slice(&0x55C4u16.to_be_bytes()); // language (und)
    c.extend_from_slice(&0u16.to_be_bytes()); // quality
    write_fullbox(out, *b"mdhd", &c);
}

fn write_default_hdlr(out: &mut Vec<u8>) {
    let mut c = Vec::new();
    c.extend_from_slice(&0u32.to_be_bytes()); // version + flags
    c.extend_from_slice(&0u32.to_be_bytes()); // pre_defined
    c.extend_from_slice(b"soun"); // handler_type
    c.extend_from_slice(&[0u8; 12]); // reserved
    c.extend_from_slice(b"SoundHandler\0");
    write_fullbox(out, *b"hdlr", &c);
}

fn write_default_smhd(out: &mut Vec<u8>) {
    let mut c = Vec::new();
    c.extend_from_slice(&0u32.to_be_bytes()); // version + flags
    c.extend_from_slice(&0u16.to_be_bytes()); // balance
    c.extend_from_slice(&0u16.to_be_bytes()); // reserved
    write_fullbox(out, *b"smhd", &c);
}

fn write_default_dinf(out: &mut Vec<u8>) {
    let dinf_start = out.len();
    out.extend_from_slice(&[0u8; 8]); // dinf placeholder

    let mut dref_c = Vec::new();
    dref_c.extend_from_slice(&0u32.to_be_bytes()); // version + flags
    dref_c.extend_from_slice(&1u32.to_be_bytes()); // entry_count
    // url entry (self-contained)
    dref_c.extend_from_slice(&12u32.to_be_bytes());
    dref_c.extend_from_slice(b"url ");
    dref_c.extend_from_slice(&1u32.to_be_bytes()); // flags = 1 (self-contained)
    write_fullbox(out, *b"dref", &dref_c);

    fixup_box_size(out, dinf_start, *b"dinf");
}

fn write_default_stsd_aac(out: &mut Vec<u8>) {
    // Minimal AAC stsd
    let mut c = Vec::new();
    c.extend_from_slice(&0u32.to_be_bytes()); // version + flags
    c.extend_from_slice(&1u32.to_be_bytes()); // entry_count
    // mp4a sample entry (minimal)
    let mp4a_start = c.len();
    c.extend_from_slice(&[0u8; 8]); // placeholder
    c.extend_from_slice(&[0u8; 6]); // reserved
    c.extend_from_slice(&1u16.to_be_bytes()); // data_reference_index
    c.extend_from_slice(&[0u8; 8]); // reserved
    c.extend_from_slice(&2u16.to_be_bytes()); // channel_count
    c.extend_from_slice(&16u16.to_be_bytes()); // sample_size (bits)
    c.extend_from_slice(&0u16.to_be_bytes()); // pre_defined
    c.extend_from_slice(&0u16.to_be_bytes()); // reserved
    c.extend_from_slice(&(44100u32 << 16).to_be_bytes()); // sample_rate (16.16)
    let mp4a_size = (c.len() - mp4a_start) as u32;
    c[mp4a_start..mp4a_start + 4].copy_from_slice(&mp4a_size.to_be_bytes());
    c[mp4a_start + 4..mp4a_start + 8].copy_from_slice(b"mp4a");
    write_fullbox(out, *b"stsd", &c);
}

/// Patch duration in a copied moov sub-box.
fn patch_duration_in_box(box_data: &[u8], box_type: [u8; 4], new_duration: u32) -> Vec<u8> {
    let mut data = box_data.to_vec();
    if data.len() < 12 {
        return data;
    }

    let version = data[8]; // After size(4) + type(4)

    match &box_type {
        b"mvhd" | b"mdhd" => {
            if version == 0 {
                // v0: ver+flags(4) + creation(4) + modification(4) + timescale(4) + duration(4)
                let off = 8 + 4 + 4 + 4 + 4; // = 24
                if off + 4 <= data.len() {
                    data[off..off + 4].copy_from_slice(&new_duration.to_be_bytes());
                }
            } else {
                // v1: ver+flags(4) + creation(8) + modification(8) + timescale(4) + duration(8)
                let off = 8 + 4 + 8 + 8 + 4; // = 32
                if off + 8 <= data.len() {
                    data[off..off + 8].copy_from_slice(&u64::from(new_duration).to_be_bytes());
                }
            }
        }
        b"tkhd" => {
            if version == 0 {
                // v0: ver+flags(4) + creation(4) + modification(4) + track_id(4) + reserved(4) + duration(4)
                let off = 8 + 4 + 4 + 4 + 4 + 4; // = 28
                if off + 4 <= data.len() {
                    data[off..off + 4].copy_from_slice(&new_duration.to_be_bytes());
                }
            } else {
                let off = 8 + 4 + 8 + 8 + 4 + 4; // = 36
                if off + 8 <= data.len() {
                    data[off..off + 8].copy_from_slice(&u64::from(new_duration).to_be_bytes());
                }
            }
        }
        _ => {}
    }

    data
}

/// Extract and clean stsd content from moov (remove sinf, rename enca → original format).
fn extract_and_clean_stsd(moov: &[u8]) -> Option<Vec<u8>> {
    // Navigate moov → trak (audio) → mdia → minf → stbl → stsd
    let moov_content = &moov[8..]; // skip moov header

    // Find audio trak (with 'soun' handler)
    let audio_trak = find_audio_trak_content(moov_content)?;

    let (mdia_start, mdia_size) = find_child_box(audio_trak, "mdia")?;
    let mdia = &audio_trak[mdia_start..mdia_start + mdia_size];

    let (minf_start, minf_size) = find_child_box(mdia, "minf")?;
    let minf = &mdia[minf_start..minf_start + minf_size];

    let (stbl_start, stbl_size) = find_child_box(minf, "stbl")?;
    let stbl = &minf[stbl_start..stbl_start + stbl_size];

    let (stsd_start, stsd_size) = find_child_box(stbl, "stsd")?;
    let stsd_content = &stbl[stsd_start..stsd_start + stsd_size];

    // stsd content: version(1)+flags(3) + entry_count(4) + entries...
    if stsd_content.len() < 8 {
        return None;
    }

    let version_flags = &stsd_content[..4];
    let entry_count = read_u32(stsd_content, 4) as usize;

    let mut cleaned_entries = Vec::new();
    let mut offset = 8;

    for _ in 0..entry_count {
        if offset + 8 > stsd_content.len() {
            break;
        }
        let entry_size = read_u32(stsd_content, offset) as usize;
        let entry_type = &stsd_content[offset + 4..offset + 8];

        if entry_size < 8 || offset + entry_size > stsd_content.len() {
            break;
        }

        let entry_data = &stsd_content[offset..offset + entry_size];

        if entry_type == b"enca" || entry_type == b"encv" || entry_type == b"encs" {
            cleaned_entries.push(clean_encrypted_sample_entry(entry_data));
        } else {
            cleaned_entries.push(remove_sinf_from_entry(entry_data));
        }

        offset += entry_size;
    }

    // Rebuild stsd content
    let mut result = version_flags.to_vec();
    result.extend_from_slice(&(cleaned_entries.len() as u32).to_be_bytes());
    for entry in &cleaned_entries {
        result.extend_from_slice(entry);
    }

    Some(result)
}

/// Clean an encrypted sample entry: rename enca → original format (from frma), remove sinf.
fn clean_encrypted_sample_entry(entry_data: &[u8]) -> Vec<u8> {
    if entry_data.len() < 36 {
        return entry_data.to_vec();
    }

    // Find original format from sinf/frma
    let original_format = find_original_format(entry_data).unwrap_or(*b"mp4a");

    // Audio sample entry: size(4) + type(4) + reserved(6) + data_ref(2) + audio_data(20) = 36 bytes
    let mut new_entry = Vec::with_capacity(entry_data.len());
    new_entry.extend_from_slice(&entry_data[..4]); // size (will be fixed)
    new_entry.extend_from_slice(&original_format); // replace type
    new_entry.extend_from_slice(&entry_data[8..36]); // audio header

    // Copy child boxes, excluding sinf
    let mut child_off = 36;
    while child_off + 8 <= entry_data.len() {
        let child_size = read_u32(entry_data, child_off) as usize;
        if child_size < 8 || child_off + child_size > entry_data.len() {
            break;
        }
        let child_type = &entry_data[child_off + 4..child_off + 8];
        if child_type != b"sinf" {
            new_entry.extend_from_slice(&entry_data[child_off..child_off + child_size]);
        }
        child_off += child_size;
    }

    // Fix size
    let new_size = new_entry.len() as u32;
    new_entry[..4].copy_from_slice(&new_size.to_be_bytes());

    new_entry
}

/// Find original format from sinf/frma in an encrypted sample entry.
fn find_original_format(entry_data: &[u8]) -> Option<[u8; 4]> {
    // Search for sinf box in child boxes (start at offset 36)
    let mut off = 36;
    while off + 8 <= entry_data.len() {
        let size = read_u32(entry_data, off) as usize;
        if size < 8 || off + size > entry_data.len() {
            break;
        }
        if &entry_data[off + 4..off + 8] == b"sinf" {
            // Search for frma inside sinf
            let sinf = &entry_data[off..off + size];
            let mut sinf_off = 8;
            while sinf_off + 8 <= sinf.len() {
                let frma_size = read_u32(sinf, sinf_off) as usize;
                if frma_size < 8 || sinf_off + frma_size > sinf.len() {
                    break;
                }
                if &sinf[sinf_off + 4..sinf_off + 8] == b"frma" && frma_size >= 12 {
                    let mut fmt = [0u8; 4];
                    fmt.copy_from_slice(&sinf[sinf_off + 8..sinf_off + 12]);
                    return Some(fmt);
                }
                sinf_off += frma_size;
            }
        }
        off += size;
    }
    None
}

/// Remove sinf from a non-encrypted sample entry.
fn remove_sinf_from_entry(entry_data: &[u8]) -> Vec<u8> {
    if entry_data.len() < 36 || !entry_data.windows(4).any(|w| w == b"sinf") {
        return entry_data.to_vec();
    }

    let mut new_entry = entry_data[..36].to_vec();
    let mut off = 36;
    while off + 8 <= entry_data.len() {
        let size = read_u32(entry_data, off) as usize;
        if size < 8 || off + size > entry_data.len() {
            break;
        }
        if &entry_data[off + 4..off + 8] != b"sinf" {
            new_entry.extend_from_slice(&entry_data[off..off + size]);
        }
        off += size;
    }

    let new_size = new_entry.len() as u32;
    new_entry[..4].copy_from_slice(&new_size.to_be_bytes());
    new_entry
}

/// Find a raw child box (returns full box bytes including header) by scanning container.
fn find_child_box_raw(container: &[u8], box_type: [u8; 4], skip: usize) -> Option<Vec<u8>> {
    let mut off = skip;
    while off + 8 <= container.len() {
        let size = read_u32(container, off) as usize;
        if size < 8 || off + size > container.len() {
            break;
        }
        if container[off + 4..off + 8] == box_type {
            return Some(container[off..off + size].to_vec());
        }
        off += size;
    }
    None
}

/// Find audio trak box content (the first trak with hdlr `handler_type` 'soun').
fn find_audio_trak_content(moov_content: &[u8]) -> Option<&[u8]> {
    let mut off = 0;
    while off + 8 <= moov_content.len() {
        let Some((size, btype, header_size)) = read_box_header(moov_content, off) else {
            break;
        };
        if size == 0 || off + size > moov_content.len() {
            break;
        }
        if btype == "trak" {
            let trak = &moov_content[off + header_size..off + size];
            // Check if this trak has handler_type 'soun'
            if let Some(hdlr_pos) = trak.windows(4).position(|w| w == b"hdlr") {
                let handler_off = hdlr_pos + 4 + 4 + 4;
                if handler_off + 4 <= trak.len() && &trak[handler_off..handler_off + 4] == b"soun" {
                    return Some(trak);
                }
            }
        }
        off += size;
    }
    None
}

/// Find audio trak box raw bytes.
fn find_audio_trak_raw(moov: &[u8]) -> Option<Vec<u8>> {
    if moov.len() < 8 {
        return None;
    }
    let mut off = 8; // skip moov header
    while off + 8 <= moov.len() {
        let size = read_u32(moov, off) as usize;
        if size < 8 || off + size > moov.len() {
            break;
        }
        if &moov[off + 4..off + 8] == b"trak" {
            let trak = &moov[off..off + size];
            if let Some(hdlr_pos) = trak.windows(4).position(|w| w == b"hdlr") {
                let handler_off = hdlr_pos + 4 + 4 + 4;
                if handler_off + 4 <= trak.len() && &trak[handler_off..handler_off + 4] == b"soun" {
                    return Some(trak.to_vec());
                }
            }
        }
        off += size;
    }
    None
}

// Helper fns to find sub-boxes within an audio trak
fn find_mdhd_in_trak(trak: &[u8]) -> Option<Vec<u8>> {
    find_deep_box(trak, &[b"mdia", b"mdhd"])
}

fn find_hdlr_in_trak(trak: &[u8]) -> Option<Vec<u8>> {
    find_deep_box(trak, &[b"mdia", b"hdlr"])
}

fn find_smhd_in_trak(trak: &[u8]) -> Option<Vec<u8>> {
    find_deep_box(trak, &[b"mdia", b"minf", b"smhd"])
}

fn find_dinf_in_trak(trak: &[u8]) -> Option<Vec<u8>> {
    find_deep_box(trak, &[b"mdia", b"minf", b"dinf"])
}

/// Find a box nested inside a trak (navigating through container boxes).
fn find_deep_box(data: &[u8], path: &[&[u8; 4]]) -> Option<Vec<u8>> {
    if path.is_empty() || data.len() < 8 {
        return None;
    }

    // Start searching from offset 8 (skip trak/mdia/minf header)
    let mut current = &data[8..];

    for (i, target) in path.iter().enumerate() {
        let is_last = i == path.len() - 1;
        let mut found = false;
        let mut off = 0;

        while off + 8 <= current.len() {
            let size = read_u32(current, off) as usize;
            if size < 8 || off + size > current.len() {
                break;
            }
            if &current[off + 4..off + 8] == *target {
                if is_last {
                    return Some(current[off..off + size].to_vec());
                }
                // Navigate into this container
                let header_size = 8;
                current = &current[off + header_size..off + size];
                found = true;
                break;
            }
            off += size;
        }

        if !found {
            return None;
        }
    }

    None
}

/// CENC decryption (AES-128-CTR).
fn decrypt_cenc(key: &[u8], iv: &[u8], data: &[u8], subsamples: &[(u32, u32)]) -> Result<Vec<u8>, String> {
    let mut padded_iv = [0u8; 16];
    let copy_len = iv.len().min(16);
    padded_iv[..copy_len].copy_from_slice(&iv[..copy_len]);

    type Aes128Ctr = ctr::Ctr128BE<aes::Aes128>;
    let mut cipher = Aes128Ctr::new_from_slices(key, &padded_iv).map_err(|e| format!("AES-CTR init: {e}"))?;

    if subsamples.is_empty() {
        let mut buf = data.to_vec();
        cipher.apply_keystream(&mut buf);
        return Ok(buf);
    }

    let mut result = Vec::with_capacity(data.len());
    let mut offset = 0;

    for &(clear_bytes, encrypted_bytes) in subsamples {
        let clear = clear_bytes as usize;
        let encrypted = encrypted_bytes as usize;

        // Clear bytes pass through
        result.extend_from_slice(&data[offset..offset + clear]);
        offset += clear;

        // Encrypted bytes
        let mut enc_buf = data[offset..offset + encrypted].to_vec();
        cipher.apply_keystream(&mut enc_buf);
        result.extend_from_slice(&enc_buf);
        offset += encrypted;
    }

    // Remaining data
    if offset < data.len() {
        result.extend_from_slice(&data[offset..]);
    }

    Ok(result)
}

/// CBCS decryption (AES-128-CBC with subsample patterns).
fn decrypt_cbcs(key: &[u8], iv: &[u8], data: &[u8], subsamples: &[(u32, u32)]) -> Result<Vec<u8>, String> {
    let mut padded_iv = [0u8; 16];
    let copy_len = iv.len().min(16);
    padded_iv[..copy_len].copy_from_slice(&iv[..copy_len]);

    if subsamples.is_empty() {
        // Full sample decryption
        let sample_len = data.len();
        let truncated_len = sample_len & !0xF;

        if truncated_len == 0 {
            return Ok(data.to_vec());
        }

        let mut buf = data[..truncated_len].to_vec();
        let dec = Aes128CbcDec::new_from_slices(key, &padded_iv).map_err(|e| format!("AES-CBC init: {e}"))?;

        let _pt = dec
            .decrypt_padded_mut::<aes::cipher::block_padding::NoPadding>(&mut buf)
            .map_err(|e| format!("AES-CBC decrypt: {e}"))?;

        let mut result = buf;
        if truncated_len < sample_len {
            result.extend_from_slice(&data[truncated_len..]);
        }
        return Ok(result);
    }

    // Subsample-based decryption
    let mut encrypted_concat = Vec::new();
    let mut offset = 0;

    for &(clear_bytes, encrypted_bytes) in subsamples {
        offset += clear_bytes as usize;
        if encrypted_bytes > 0 {
            encrypted_concat.extend_from_slice(&data[offset..offset + encrypted_bytes as usize]);
        }
        offset += encrypted_bytes as usize;
    }

    // Decrypt concatenated encrypted regions
    let total_enc = encrypted_concat.len();
    let cbc_len = total_enc & !0xF;
    let mut decrypted_concat = Vec::with_capacity(total_enc);

    if cbc_len > 0 {
        let mut buf = encrypted_concat[..cbc_len].to_vec();
        let dec = Aes128CbcDec::new_from_slices(key, &padded_iv).map_err(|e| format!("AES-CBC init: {e}"))?;
        let _pt = dec
            .decrypt_padded_mut::<aes::cipher::block_padding::NoPadding>(&mut buf)
            .map_err(|e| format!("AES-CBC decrypt: {e}"))?;
        decrypted_concat.extend_from_slice(&buf);
    }
    if cbc_len < total_enc {
        decrypted_concat.extend_from_slice(&encrypted_concat[cbc_len..]);
    }

    // Reassemble with clear regions
    let mut result = Vec::with_capacity(data.len());
    let mut dec_offset = 0;
    let mut data_offset = 0;

    for &(clear_bytes, encrypted_bytes) in subsamples {
        let clear = clear_bytes as usize;
        let encrypted = encrypted_bytes as usize;

        result.extend_from_slice(&data[data_offset..data_offset + clear]);
        data_offset += clear;

        if encrypted > 0 {
            result.extend_from_slice(&decrypted_concat[dec_offset..dec_offset + encrypted]);
            dec_offset += encrypted;
        }
        data_offset += encrypted;
    }

    if data_offset < data.len() {
        result.extend_from_slice(&data[data_offset..]);
    }

    Ok(result)
}

// ── MP4 box parsing ─────────────────────────────────────────────────────────

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
}

fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([data[offset], data[offset + 1]])
}

/// Read a box header: (`total_size`, `box_type_str`, `header_size`)
fn read_box_header(data: &[u8], offset: usize) -> Option<(usize, String, usize)> {
    if offset + 8 > data.len() {
        return None;
    }

    let size = read_u32(data, offset) as usize;
    let box_type = String::from_utf8_lossy(&data[offset + 4..offset + 8]).to_string();

    if size == 1 {
        // Extended size
        if offset + 16 > data.len() {
            return None;
        }
        let ext_size = u64::from_be_bytes([
            data[offset + 8],
            data[offset + 9],
            data[offset + 10],
            data[offset + 11],
            data[offset + 12],
            data[offset + 13],
            data[offset + 14],
            data[offset + 15],
        ]) as usize;
        Some((ext_size, box_type, 16))
    } else if size == 0 {
        // Box extends to end
        Some((data.len() - offset, box_type, 8))
    } else {
        Some((size, box_type, 8))
    }
}

/// Find a child box within data. Returns the offset and size of the box content.
fn find_child_box(data: &[u8], box_type: &str) -> Option<(usize, usize)> {
    let mut offset = 0;
    while offset + 8 <= data.len() {
        let (size, btype, header_size) = read_box_header(data, offset)?;
        if size == 0 {
            break;
        }
        if btype == box_type {
            let content_start = offset + header_size;
            let content_size = size - header_size;
            return Some((content_start, content_size));
        }
        offset += size;
    }
    None
}

/// Extract samples and encryption info from a fragmented MP4.
fn extract_samples(data: &[u8]) -> Result<(Vec<Sample>, EncryptionInfo), String> {
    let mut samples = Vec::new();
    let mut enc_info = EncryptionInfo::default();

    // Find moov box and extract encryption info FIRST (moov comes before moof in fMP4)
    let mut offset = 0;

    while offset + 8 <= data.len() {
        let (size, btype, header_size) = read_box_header(data, offset).ok_or("Unexpected end of MP4 data")?;
        if size == 0 {
            break;
        }

        if btype == "moov" {
            let moov_content = &data[offset + header_size..offset + size];
            if let Some(info) = extract_encryption_info(moov_content) {
                debug!(
                    "[AppleMusic] Encryption info: scheme={}, per_sample_iv_size={}, constant_iv_len={}",
                    info.scheme_type,
                    info.per_sample_iv_size,
                    info.constant_iv.len()
                );
                enc_info = info;
            }
            break;
        }
        offset += size;
    }

    // Now parse moof+mdat boxes using the correct enc_info
    offset = 0;
    while offset + 8 <= data.len() {
        let (size, btype, header_size) = read_box_header(data, offset).ok_or("Unexpected end of MP4 data")?;
        if size == 0 {
            break;
        }

        if btype == "moof" {
            let moof_content = &data[offset + header_size..offset + size];
            // mdat follows moof
            let mdat_start = offset + size;
            if mdat_start + 8 <= data.len() {
                let (mdat_size, mdat_type, mdat_header) =
                    read_box_header(data, mdat_start).ok_or("No mdat after moof")?;
                if mdat_type == "mdat" {
                    let mdat_content_start = mdat_start + mdat_header;
                    let mdat_content = &data[mdat_content_start..mdat_start + mdat_size];
                    parse_moof_mdat(moof_content, mdat_content, mdat_content_start, &enc_info, &mut samples)?;
                }
            }
        }

        offset += size;
    }

    Ok((samples, enc_info))
}

/// Parse moof+mdat to extract sample info.
fn parse_moof_mdat(
    moof: &[u8],
    _mdat: &[u8],
    mdat_abs_offset: usize,
    enc_info: &EncryptionInfo,
    samples: &mut Vec<Sample>,
) -> Result<(), String> {
    // Find traf in moof
    let (traf_start, traf_size) = find_child_box(moof, "traf").ok_or("No traf in moof")?;
    let traf = &moof[traf_start..traf_start + traf_size];

    // Parse tfhd for default values
    let mut default_sample_duration = 0u32;
    let mut default_sample_size = 0u32;
    let mut default_sample_desc_index = 0u32;

    if let Some((tfhd_start, tfhd_size)) = find_child_box(traf, "tfhd") {
        let tfhd = &traf[tfhd_start..tfhd_start + tfhd_size];
        if tfhd.len() >= 4 {
            let flags = u32::from_be_bytes([0, tfhd[1], tfhd[2], tfhd[3]]);
            let mut pos = 8; // version(1) + flags(3) + track_id(4)

            if flags & 0x01 != 0 {
                pos += 8;
            } // base_data_offset
            if flags & 0x02 != 0 && pos + 4 <= tfhd.len() {
                default_sample_desc_index = read_u32(tfhd, pos);
                pos += 4;
            }
            if flags & 0x08 != 0 && pos + 4 <= tfhd.len() {
                default_sample_duration = read_u32(tfhd, pos);
                pos += 4;
            }
            if flags & 0x10 != 0 && pos + 4 <= tfhd.len() {
                default_sample_size = read_u32(tfhd, pos);
            }
        }
    }

    // Parse trun for sample table
    let mut sample_infos = Vec::new();
    if let Some((trun_start, trun_size)) = find_child_box(traf, "trun") {
        let trun = &traf[trun_start..trun_start + trun_size];
        if trun.len() >= 8 {
            let flags = u32::from_be_bytes([0, trun[1], trun[2], trun[3]]);
            let sample_count = read_u32(trun, 4) as usize;
            let mut pos = 8;

            let has_data_offset = flags & 0x01 != 0;
            let has_first_sample_flags = flags & 0x04 != 0;
            let has_duration = flags & 0x100 != 0;
            let has_size = flags & 0x200 != 0;
            let has_flags = flags & 0x400 != 0;
            let has_composition = flags & 0x800 != 0;

            if has_data_offset {
                pos += 4;
            }
            if has_first_sample_flags {
                pos += 4;
            }

            for _ in 0..sample_count {
                let duration = if has_duration && pos + 4 <= trun.len() {
                    let d = read_u32(trun, pos);
                    pos += 4;
                    d
                } else {
                    default_sample_duration
                };

                let size = if has_size && pos + 4 <= trun.len() {
                    let s = read_u32(trun, pos);
                    pos += 4;
                    s
                } else {
                    default_sample_size
                };

                if has_flags {
                    pos += 4;
                }
                if has_composition {
                    pos += 4;
                }

                sample_infos.push((duration, size));
            }
        }
    }

    // Parse senc for per-sample IVs and subsamples
    let mut senc_entries: SencEntries = Vec::new();
    if let Some((senc_start, senc_size)) = find_child_box(traf, "senc") {
        let senc = &traf[senc_start..senc_start + senc_size];
        if senc.len() >= 8 {
            let flags = u32::from_be_bytes([0, senc[1], senc[2], senc[3]]);
            let count = read_u32(senc, 4) as usize;
            let iv_size = enc_info.per_sample_iv_size as usize;
            let has_subsample_info = flags & 0x02 != 0;
            let mut pos = 8;

            for _ in 0..count {
                let iv = if iv_size > 0 && pos + iv_size <= senc.len() {
                    let v = senc[pos..pos + iv_size].to_vec();
                    pos += iv_size;
                    v
                } else {
                    Vec::new()
                };

                let mut subs = Vec::new();
                if has_subsample_info && pos + 2 <= senc.len() {
                    let sub_count = read_u16(senc, pos) as usize;
                    pos += 2;
                    for _ in 0..sub_count {
                        if pos + 6 <= senc.len() {
                            let clear = u32::from(read_u16(senc, pos));
                            pos += 2;
                            let enc = read_u32(senc, pos);
                            pos += 4;
                            subs.push((clear, enc));
                        }
                    }
                }

                senc_entries.push((iv, subs));
            }
        }
    }

    // Combine into samples
    let mut data_offset = 0usize;
    for (i, (duration, size)) in sample_infos.iter().enumerate() {
        let (iv, subsamples) = if i < senc_entries.len() {
            senc_entries[i].clone()
        } else {
            (Vec::new(), Vec::new())
        };

        samples.push(Sample {
            offset: mdat_abs_offset + data_offset,
            size: *size as usize,
            _duration: *duration,
            desc_index: if default_sample_desc_index > 0 {
                default_sample_desc_index - 1 // 0-indexed
            } else {
                0
            },
            iv,
            subsamples,
        });

        data_offset += *size as usize;
    }

    Ok(())
}

/// Extract encryption info from moov box.
fn extract_encryption_info(moov: &[u8]) -> Option<EncryptionInfo> {
    // Navigate: moov → trak → mdia → minf → stbl → stsd → enca/mp4a → sinf → schm/schi/tenc
    let (trak_start, trak_size) = find_child_box(moov, "trak")?;
    let trak = &moov[trak_start..trak_start + trak_size];

    let (mdia_start, mdia_size) = find_child_box(trak, "mdia")?;
    let mdia = &trak[mdia_start..mdia_start + mdia_size];

    let (minf_start, minf_size) = find_child_box(mdia, "minf")?;
    let minf = &mdia[minf_start..minf_start + minf_size];

    let (stbl_start, stbl_size) = find_child_box(minf, "stbl")?;
    let stbl = &minf[stbl_start..stbl_start + stbl_size];

    let (stsd_start, stsd_size) = find_child_box(stbl, "stsd")?;
    let stsd = &stbl[stsd_start..stsd_start + stsd_size];

    // stsd is a full box: version(1) + flags(3) + entry_count(4)
    if stsd.len() < 8 {
        return None;
    }
    let stsd_entries = &stsd[8..];

    // Find sinf in any sample entry
    let mut offset = 0;
    while offset + 8 < stsd_entries.len() {
        let (entry_size, _entry_type, _entry_header) = read_box_header(stsd_entries, offset)?;
        if entry_size == 0 {
            break;
        }

        // Audio sample entry structure:
        // box_header(8) + reserved(6) + data_ref_index(2) + reserved(8)
        // + channelcount(2) + samplesize(2) + reserved(4) + samplerate(4) = 36 bytes
        // Child boxes (esds, sinf, etc.) start at offset 36
        let child_start = offset + 36;
        let child_end = offset + entry_size.min(stsd_entries.len() - offset);
        if child_start >= child_end {
            offset += entry_size;
            continue;
        }
        let entry_content = &stsd_entries[child_start..child_end];

        if let Some((sinf_start, sinf_size)) = find_child_box(entry_content, "sinf") {
            let sinf = &entry_content[sinf_start..sinf_start + sinf_size];

            let mut info = EncryptionInfo::default();

            // schm box → scheme_type
            if let Some((schm_start, schm_size)) = find_child_box(sinf, "schm") {
                let schm = &sinf[schm_start..schm_start + schm_size];
                if schm.len() >= 8 {
                    info.scheme_type = String::from_utf8_lossy(&schm[4..8]).to_string();
                }
            }

            // schi → tenc box
            if let Some((schi_start, schi_size)) = find_child_box(sinf, "schi") {
                let schi = &sinf[schi_start..schi_start + schi_size];
                if let Some((tenc_start, tenc_size)) = find_child_box(schi, "tenc") {
                    let tenc = &schi[tenc_start..tenc_start + tenc_size];
                    // tenc: version(1) + flags(3) + reserved(2) + isProtected(1) + perSampleIVSize(1) + KID(16)
                    if tenc.len() >= 24 {
                        info.per_sample_iv_size = tenc[7]; // byte 7
                        // KID is bytes 8..24
                        info.kid.copy_from_slice(&tenc[8..24]);
                        // If perSampleIVSize == 0, constant IV follows after KID
                        if info.per_sample_iv_size == 0 && tenc.len() >= 25 {
                            let const_iv_size = tenc[24] as usize;
                            if tenc.len() >= 25 + const_iv_size {
                                info.constant_iv = tenc[25..25 + const_iv_size].to_vec();
                            }
                        }
                    }
                }
            }

            return Some(info);
        }

        offset += entry_size;
    }

    None
}

/// Extract encryption info from ALL stsd entries in the moov box.
/// Returns one `EncryptionInfo` per stsd entry that has a sinf box.
fn extract_all_encryption_info(data: &[u8]) -> Vec<EncryptionInfo> {
    let mut results = Vec::new();

    // Find moov box
    let mut offset = 0;
    while offset + 8 <= data.len() {
        let Some((size, btype, header_size)) = read_box_header(data, offset) else {
            break;
        };
        if size == 0 {
            break;
        }
        if btype == "moov" {
            let moov = &data[offset + header_size..offset + size];
            // Navigate: moov → trak → mdia → minf → stbl → stsd
            if let Some(stsd_data) = navigate_to_stsd(moov) {
                if stsd_data.len() < 8 {
                    return results;
                }
                let stsd_entries = &stsd_data[8..]; // skip version(1) + flags(3) + entry_count(4)
                let mut entry_offset = 0;
                while entry_offset + 8 < stsd_entries.len() {
                    let Some((entry_size, _entry_type, _)) = read_box_header(stsd_entries, entry_offset) else {
                        break;
                    };
                    if entry_size == 0 {
                        break;
                    }

                    let child_start = entry_offset + 36;
                    let child_end = entry_offset + entry_size.min(stsd_entries.len() - entry_offset);
                    if child_start < child_end {
                        let entry_content = &stsd_entries[child_start..child_end];
                        if let Some((sinf_start, sinf_size)) = find_child_box(entry_content, "sinf") {
                            let sinf = &entry_content[sinf_start..sinf_start + sinf_size];
                            let mut info = EncryptionInfo::default();

                            if let Some((schm_start, schm_size)) = find_child_box(sinf, "schm") {
                                let schm = &sinf[schm_start..schm_start + schm_size];
                                if schm.len() >= 8 {
                                    info.scheme_type = String::from_utf8_lossy(&schm[4..8]).to_string();
                                }
                            }

                            if let Some((schi_start, schi_size)) = find_child_box(sinf, "schi") {
                                let schi = &sinf[schi_start..schi_start + schi_size];
                                if let Some((tenc_start, tenc_size)) = find_child_box(schi, "tenc") {
                                    let tenc = &schi[tenc_start..tenc_start + tenc_size];
                                    if tenc.len() >= 24 {
                                        info.per_sample_iv_size = tenc[7];
                                        info.kid.copy_from_slice(&tenc[8..24]);
                                        if info.per_sample_iv_size == 0 && tenc.len() >= 25 {
                                            let const_iv_size = tenc[24] as usize;
                                            if tenc.len() >= 25 + const_iv_size {
                                                info.constant_iv = tenc[25..25 + const_iv_size].to_vec();
                                            }
                                        }
                                    }
                                }
                            }

                            debug!(
                                "[AppleMusic] stsd entry {}: scheme={}, kid={}",
                                results.len(),
                                info.scheme_type,
                                hex::encode(info.kid)
                            );
                            results.push(info);
                        }
                    }

                    entry_offset += entry_size;
                }
            }
            break;
        }
        offset += size;
    }

    results
}

/// Navigate moov → trak → mdia → minf → stbl → stsd, return stsd content.
fn navigate_to_stsd(moov: &[u8]) -> Option<&[u8]> {
    let (trak_start, trak_size) = find_child_box(moov, "trak")?;
    let trak = &moov[trak_start..trak_start + trak_size];

    let (mdia_start, mdia_size) = find_child_box(trak, "mdia")?;
    let mdia = &trak[mdia_start..mdia_start + mdia_size];

    let (minf_start, minf_size) = find_child_box(mdia, "minf")?;
    let minf = &mdia[minf_start..minf_start + minf_size];

    let (stbl_start, stbl_size) = find_child_box(minf, "stbl")?;
    let stbl = &minf[stbl_start..stbl_start + stbl_size];

    let (stsd_start, stsd_size) = find_child_box(stbl, "stsd")?;
    Some(&stbl[stsd_start..stsd_start + stsd_size])
}

/// Replace the `enca` (encrypted audio) sample-entry type with `mp4a`
/// and neutralize the `sinf` box (rename to `free`) so that decoders
/// treat the data as clear AAC without encryption markers.
fn patch_enca_to_mp4a(data: &mut [u8]) {
    let mut offset = 0;
    while offset + 8 <= data.len() {
        let Some((size, btype, _header_size)) = read_box_header(data, offset) else {
            break;
        };
        if size == 0 {
            break;
        }
        if btype == "moov" {
            let moov_end = (offset + size).min(data.len());
            patch_sample_entry_in_moov(data, offset + 8, moov_end);
            break;
        }
        offset += size;
    }
}

/// Walk moov → trak → mdia → minf → stbl → stsd → sample entry,
/// rename `enca` → `mp4a` and `sinf` → `free`.
fn patch_sample_entry_in_moov(data: &mut [u8], start: usize, end: usize) {
    let mut offset = start;
    while offset + 8 <= end {
        let Some((size, btype, header_size)) = read_box_header(data, offset) else {
            break;
        };
        if size == 0 {
            break;
        }
        let box_end = (offset + size).min(end);
        match btype.as_str() {
            "trak" | "mdia" | "minf" | "stbl" => {
                patch_sample_entry_in_moov(data, offset + header_size, box_end);
            }
            "stsd" => {
                // stsd: header + 4 (version/flags) + 4 (entry_count), then entries
                let count_off = offset + header_size + 4;
                if count_off + 4 > box_end {
                    break;
                }
                let entry_count = u32::from_be_bytes([
                    data[count_off],
                    data[count_off + 1],
                    data[count_off + 2],
                    data[count_off + 3],
                ]) as usize;

                let mut entries_start = count_off + 4;
                for _ in 0..entry_count {
                    if entries_start + 8 > box_end {
                        break;
                    }
                    let entry_size = u32::from_be_bytes([
                        data[entries_start],
                        data[entries_start + 1],
                        data[entries_start + 2],
                        data[entries_start + 3],
                    ]) as usize;
                    if entry_size < 8 || entries_start + entry_size > box_end {
                        break;
                    }

                    if &data[entries_start + 4..entries_start + 8] == b"enca" {
                        data[entries_start + 4..entries_start + 8].copy_from_slice(b"mp4a");
                        debug!("[AppleMusic] Patched enca → mp4a at offset {entries_start}");
                    }

                    // Audio sample entry children start at offset 36
                    let children_start = entries_start + 36;
                    let entry_end = entries_start + entry_size;
                    let mut child_off = children_start;
                    while child_off + 8 <= entry_end {
                        let child_size = u32::from_be_bytes([
                            data[child_off],
                            data[child_off + 1],
                            data[child_off + 2],
                            data[child_off + 3],
                        ]) as usize;
                        if child_size < 8 || child_off + child_size > entry_end {
                            break;
                        }
                        if &data[child_off + 4..child_off + 8] == b"sinf" {
                            data[child_off + 4..child_off + 8].copy_from_slice(b"free");
                            debug!("[AppleMusic] Patched sinf → free at offset {child_off} ({child_size} bytes)");
                        }
                        child_off += child_size;
                    }

                    entries_start += entry_size;
                }
            }
            _ => {}
        }
        offset += size;
    }
}
