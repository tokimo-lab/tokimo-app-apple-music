//! Widevine L3 CDM — license challenge generation and key extraction.
//!
//! Ported from pywidevine (Python) to pure Rust.

use aes::cipher::{BlockDecryptMut, KeyIvInit};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use cmac::{Cmac, Mac};
use prost::Message;
use rand_core::{OsRng, RngCore};
use rsa::signature::RandomizedSigner;
use rsa::{Oaep, RsaPrivateKey};
use sha1::Sha1;
use std::collections::HashMap;
use uuid::Uuid;

use super::widevine;

// ── WVD device parsing ──────────────────────────────────────────────────────

/// A parsed Widevine device (.wvd format v2).
pub struct WvDevice {
    pub private_key: RsaPrivateKey,
    pub client_id: widevine::ClientIdentification,
    pub system_id: u32,
}

impl WvDevice {
    /// Parse a base64-encoded WVD v2 blob.
    pub fn from_base64(b64: &str) -> Result<Self, String> {
        let data = B64.decode(b64).map_err(|e| format!("base64 decode: {e}"))?;
        Self::from_bytes(&data)
    }

    /// Parse raw WVD v2 bytes.
    ///
    /// Format: WVD(3) | version(1) | type(1) | `security_level(1)` | flags(1)
    ///       | `private_key_len(2` BE) | `private_key` | `client_id_len(2` BE) | `client_id`
    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        if data.len() < 7 {
            return Err("WVD too short".into());
        }
        if &data[0..3] != b"WVD" {
            return Err("Invalid WVD magic".into());
        }
        if data[3] != 2 {
            return Err(format!("Unsupported WVD version: {}", data[3]));
        }
        // data[4] = type (2=ANDROID), data[5] = security_level, data[6] = flags
        let mut offset = 7;

        let pk_len = u16::from_be_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2;
        let pk_der = &data[offset..offset + pk_len];
        offset += pk_len;

        let cid_len = u16::from_be_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2;
        let cid_bytes = &data[offset..offset + cid_len];

        let private_key = <RsaPrivateKey as rsa::pkcs8::DecodePrivateKey>::from_pkcs8_der(pk_der)
            .or_else(|_| <RsaPrivateKey as rsa::pkcs1::DecodeRsaPrivateKey>::from_pkcs1_der(pk_der))
            .map_err(|e| format!("Failed to parse RSA private key: {e}"))?;

        let client_id = widevine::ClientIdentification::decode(cid_bytes)
            .map_err(|e| format!("Failed to parse ClientIdentification: {e}"))?;

        // Extract system_id from the DRM certificate in client_id.token
        let system_id = {
            let signed_cert = widevine::SignedDrmCertificate::decode(client_id.token.as_deref().unwrap_or_default())
                .map_err(|e| format!("Failed to parse SignedDrmCertificate: {e}"))?;
            let drm_cert = widevine::DrmCertificate::decode(signed_cert.drm_certificate.as_deref().unwrap_or_default())
                .map_err(|e| format!("Failed to parse DrmCertificate: {e}"))?;
            drm_cert.system_id()
        };

        Ok(Self {
            private_key,
            client_id,
            system_id,
        })
    }
}

// ── CDM Session ─────────────────────────────────────────────────────────────

struct CdmSession {
    number: u32,
    context: HashMap<Vec<u8>, (Vec<u8>, Vec<u8>)>, // request_id → (enc_ctx, mac_ctx)
    keys: Vec<ContentKey>,
}

/// A decrypted content key.
#[derive(Debug, Clone)]
pub struct ContentKey {
    pub kid: Uuid,
    pub key: Vec<u8>,
    pub key_type: String,
}

// ── CDM ─────────────────────────────────────────────────────────────────────

/// Widevine Content Decryption Module (L3).
pub struct Cdm {
    device: WvDevice,
    sessions: HashMap<Vec<u8>, CdmSession>,
    session_counter: u32,
}

impl Cdm {
    pub fn new(device: WvDevice) -> Self {
        Self {
            device,
            sessions: HashMap::new(),
            session_counter: 0,
        }
    }

    /// Open a new CDM session. Returns a session ID.
    pub fn open(&mut self) -> Vec<u8> {
        self.session_counter += 1;
        let sid = format!("session-{}", self.session_counter).into_bytes();
        self.sessions.insert(
            sid.clone(),
            CdmSession {
                number: self.session_counter,
                context: HashMap::new(),
                keys: Vec::new(),
            },
        );
        sid
    }

    /// Close a CDM session.
    pub fn close(&mut self, session_id: &[u8]) {
        self.sessions.remove(session_id);
    }

    /// Generate a license challenge (`SignedMessage` protobuf bytes).
    pub fn get_license_challenge(&mut self, session_id: &[u8], pssh_data: &[u8]) -> Result<Vec<u8>, String> {
        let session = self.sessions.get_mut(session_id).ok_or("Invalid session")?;

        // Generate request_id (Android style: random(4) + 0000 + counter(8) as uppercase hex)
        let mut rng = OsRng;
        let mut rand_bytes = [0u8; 4];
        rng.fill_bytes(&mut rand_bytes);
        let mut request_id_raw = Vec::with_capacity(16);
        request_id_raw.extend_from_slice(&rand_bytes);
        request_id_raw.extend_from_slice(&[0u8; 4]);
        request_id_raw.extend_from_slice(&session.number.to_le_bytes());
        request_id_raw.extend_from_slice(&[0u8; 4]); // pad to 16 bytes
        let request_id = hex::encode_upper(&request_id_raw).into_bytes();

        // Build LicenseRequest
        let license_request = widevine::LicenseRequest {
            client_id: Some(self.device.client_id.clone()),
            content_id: Some(widevine::license_request::ContentIdentification {
                content_id_variant: Some(
                    widevine::license_request::content_identification::ContentIdVariant::WidevinePsshData(
                        widevine::license_request::content_identification::WidevinePsshData {
                            pssh_data: vec![pssh_data.to_vec()],
                            license_type: Some(widevine::LicenseType::Streaming.into()),
                            request_id: Some(request_id.clone()),
                        },
                    ),
                ),
            }),
            r#type: Some(widevine::license_request::RequestType::New.into()),
            request_time: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| format!("System time error: {e}"))?
                    .as_secs() as i64,
            ),
            protocol_version: Some(widevine::ProtocolVersion::Version21.into()),
            key_control_nonce_deprecated: None,
            key_control_nonce: Some(rng.next_u32()),
            encrypted_client_id: None,
        };

        let license_request_bytes = license_request.encode_to_vec();

        // Sign with RSA-PSS SHA1
        let signing_key = rsa::pss::SigningKey::<Sha1>::new(self.device.private_key.clone());
        let sig = signing_key.sign_with_rng(&mut rng, &license_request_bytes);
        let signature: Vec<u8> = Box::<[u8]>::from(sig).into_vec();

        // Wrap in SignedMessage
        let signed_message = widevine::SignedMessage {
            r#type: Some(widevine::signed_message::MessageType::LicenseRequest.into()),
            msg: Some(license_request_bytes.clone()),
            signature: Some(signature),
            session_key: None,
            remote_attestation: None,
            metric_data: vec![],
            service_version_info: None,
            session_key_type: None,
            oemcrypto_core_message: None,
        };

        // Store context for later key derivation
        let (enc_ctx, mac_ctx) = derive_context(&license_request_bytes);
        session.context.insert(request_id, (enc_ctx, mac_ctx));

        Ok(signed_message.encode_to_vec())
    }

    /// Parse a license response and extract content keys.
    pub fn parse_license(&mut self, session_id: &[u8], license_b64: &str) -> Result<(), String> {
        let license_bytes = B64
            .decode(license_b64)
            .map_err(|e| format!("base64 decode license: {e}"))?;

        let signed_msg = widevine::SignedMessage::decode(license_bytes.as_slice())
            .map_err(|e| format!("decode SignedMessage: {e}"))?;

        if signed_msg.r#type() != widevine::signed_message::MessageType::License {
            return Err(format!("Expected LICENSE message, got {:?}", signed_msg.r#type()));
        }

        let license = widevine::License::decode(signed_msg.msg()).map_err(|e| format!("decode License: {e}"))?;

        let request_id = license
            .id
            .as_ref()
            .and_then(|id| id.request_id.clone())
            .ok_or("No request_id in license")?;

        let session = self.sessions.get_mut(session_id).ok_or("Invalid session")?;

        let (enc_ctx, mac_ctx) = session.context.remove(&request_id).ok_or("No context for request_id")?;

        // Decrypt session key with RSA OAEP
        let session_key_encrypted = signed_msg
            .session_key
            .as_ref()
            .ok_or("No session_key in license response")?;

        let session_key = self
            .device
            .private_key
            .decrypt(Oaep::new::<Sha1>(), session_key_encrypted)
            .map_err(|e| format!("RSA decrypt session key: {e}"))?;

        // Derive enc_key, mac_key_server, mac_key_client
        let (enc_key, _mac_key_server, _mac_key_client) = derive_keys(&enc_ctx, &mac_ctx, &session_key);

        // Extract and decrypt content keys
        for key_container in &license.key {
            let key_id = key_container.id.as_deref().unwrap_or_default();
            let key_data = key_container.key.as_deref().unwrap_or_default();
            let key_iv = key_container.iv.as_deref().unwrap_or_default();

            if key_data.is_empty() || key_iv.is_empty() {
                continue;
            }

            // Decrypt key with AES-CBC using enc_key
            let decrypted_key = aes_cbc_decrypt(&enc_key, key_iv, key_data)?;
            // PKCS7 unpad
            let unpadded = pkcs7_unpad(&decrypted_key)?;

            let kid = kid_to_uuid(key_id);
            let key_type = key_container.r#type().as_str_name().to_string();

            session.keys.push(ContentKey {
                kid,
                key: unpadded.to_vec(),
                key_type,
            });
        }

        Ok(())
    }

    /// Get content keys from a session. Optionally filter by type.
    pub fn get_keys(&self, session_id: &[u8], type_filter: Option<&str>) -> Vec<ContentKey> {
        self.sessions
            .get(session_id)
            .map(|s| {
                s.keys
                    .iter()
                    .filter(|k| type_filter.is_none() || k.key_type == type_filter.unwrap())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}

// ── Context derivation ──────────────────────────────────────────────────────

fn derive_context(license_request: &[u8]) -> (Vec<u8>, Vec<u8>) {
    // enc_context = "ENCRYPTION" + \x00 + msg + key_size(128 bits as u32 BE)
    let mut enc_ctx = Vec::new();
    enc_ctx.extend_from_slice(b"ENCRYPTION");
    enc_ctx.push(0);
    enc_ctx.extend_from_slice(license_request);
    enc_ctx.extend_from_slice(&(128u32 * 8 / 8).to_be_bytes()); // 16 bytes = 128 bits
    // Wait, pywidevine uses: key_size = 16 * 8 = 128, then .to_bytes(4, "big")
    // So it's 128 as u32 big-endian = [0, 0, 0, 128]
    // Let me re-check: `key_size = 16 * 8  # 128-bit` → `key_size.to_bytes(4, "big")`
    // 128 in big-endian 4 bytes = 0x00000080
    let mut enc_ctx = Vec::new();
    enc_ctx.extend_from_slice(b"ENCRYPTION");
    enc_ctx.push(0);
    enc_ctx.extend_from_slice(license_request);
    enc_ctx.extend_from_slice(&128u32.to_be_bytes());

    // mac_context = "AUTHENTICATION" + \x00 + msg + key_size(512 bits as u32 BE)
    // key_size = 32 * 8 * 2 = 512
    let mut mac_ctx = Vec::new();
    mac_ctx.extend_from_slice(b"AUTHENTICATION");
    mac_ctx.push(0);
    mac_ctx.extend_from_slice(license_request);
    mac_ctx.extend_from_slice(&512u32.to_be_bytes());

    (enc_ctx, mac_ctx)
}

fn derive_keys(enc_context: &[u8], mac_context: &[u8], session_key: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let enc_key = cmac_derive(session_key, enc_context, 1);

    let mut mac_key_server = cmac_derive(session_key, mac_context, 1);
    mac_key_server.extend(cmac_derive(session_key, mac_context, 2));

    let mut mac_key_client = cmac_derive(session_key, mac_context, 3);
    mac_key_client.extend(cmac_derive(session_key, mac_context, 4));

    (enc_key, mac_key_server, mac_key_client)
}

fn cmac_derive(session_key: &[u8], context: &[u8], counter: u8) -> Vec<u8> {
    let mut mac = Cmac::<aes::Aes128>::new_from_slice(session_key).expect("CMAC key size mismatch");
    mac.update(&[counter]);
    mac.update(context);
    mac.finalize().into_bytes().to_vec()
}

// ── Crypto helpers ──────────────────────────────────────────────────────────

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

fn aes_cbc_decrypt(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    if key.len() != 16 {
        return Err(format!("AES key must be 16 bytes, got {}", key.len()));
    }
    let mut buf = data.to_vec();
    let decryptor = Aes128CbcDec::new_from_slices(key, iv).map_err(|e| format!("AES-CBC init: {e}"))?;
    let pt = decryptor
        .decrypt_padded_mut::<aes::cipher::block_padding::NoPadding>(&mut buf)
        .map_err(|e| format!("AES-CBC decrypt: {e}"))?;
    Ok(pt.to_vec())
}

fn pkcs7_unpad(data: &[u8]) -> Result<&[u8], String> {
    if data.is_empty() {
        return Err("Empty data for PKCS7 unpad".into());
    }
    let pad_len = *data.last().unwrap() as usize;
    if pad_len == 0 || pad_len > 16 || pad_len > data.len() {
        return Err(format!("Invalid PKCS7 padding: {pad_len}"));
    }
    // Verify padding bytes
    for &b in &data[data.len() - pad_len..] {
        if b as usize != pad_len {
            return Err("Invalid PKCS7 padding bytes".into());
        }
    }
    Ok(&data[..data.len() - pad_len])
}

fn kid_to_uuid(kid: &[u8]) -> Uuid {
    if kid.is_empty() {
        return Uuid::nil();
    }
    if kid.len() < 16 {
        let mut padded = [0u8; 16];
        padded[..kid.len()].copy_from_slice(kid);
        Uuid::from_bytes(padded)
    } else {
        Uuid::from_slice(&kid[..16]).unwrap_or(Uuid::nil())
    }
}

// ── PSSH builder ────────────────────────────────────────────────────────────

/// Build `WidevinePsshData` protobuf from a key ID.
#[allow(deprecated)]
pub fn build_pssh_data(key_id: &[u8]) -> Vec<u8> {
    let pssh = widevine::WidevinePsshData {
        key_ids: vec![key_id.to_vec()],
        algorithm: Some(widevine::widevine_pssh_data::Algorithm::Aesctr.into()),
        ..Default::default()
    };
    pssh.encode_to_vec()
}

// ── Hardcoded device ────────────────────────────────────────────────────────

/// The hardcoded WVD device from gamdl (Android 9 emulator, L3).
pub const HARDCODED_WVD_B64: &str = "V1ZEAgIDAASoMIIEpAIBAAKCAQEAwnCFAPXy4U1J7p1NohAS+xl040f5FBaE/59bPp301bGz0UGFT9VoEtY3vaeakKh/d319xTNvCSWsEDRaMmp/wSnMiEZUkkl04872jx2uHuR4k6KYuuJoqhsIo1TwUBueFZynHBUJzXQeW8Eb1tYAROGwp8W7r+b0RIjHC89RFnfVXpYlF5I6McktyzJNSOwlQbMqlVihfSUkv3WRd3HFmA0Oxay51CEIkoTlNTHVlzVyhov5eHCDSp7QENRgaaQ03jC/CcgFOoQymhsBtRCM0CQmfuAHjA9e77R6m/GJPy75G9fqoZM1RMzVDHKbKZPd3sFd0c0+77gLzW8cWEaaHwIDAQABAoIBAQCB2pN46MikHvHZIcTPDt0eRQoDH/YArGl2Lf7J+sOgU2U7wv49KtCug9IGHwDiyyUVsAFmycrF2RroV45FTUq0vi2SdSXV7Kjb20Ren/vBNeQw9M37QWmU8Sj7q6YyWb9hv5T69DHvvDTqIjVtbM4RMojAAxYti5hmjNIh2PrWfVYWhXxCQ/WqAjWLtZBM6Oww1byfr5I/wFogAKkgHi8wYXZ4LnIC8V7jLAhujlToOvMMC9qwcBiPKDP2FO+CPSXaqVhH+LPSEgLggnU3EirihgxovbLNAuDEeEbRTyR70B0lW19tLHixso4ZQa7KxlVUwOmrHSZf7nVuWqPpxd+BAoGBAPQLyJ1IeRavmaU8XXxfMdYDoc8+xB7v2WaxkGXb6ToX1IWPkbMz4yyVGdB5PciIP3rLZ6s1+ruuRRV0IZ98i1OuN5TSR56ShCGg3zkd5C4L/xSMAz+NDfYSDBdO8BVvBsw21KqSRUi1ctL7QiIvfedrtGb5XrE4zhH0gjXlU5qZAoGBAMv2segn0Jx6az4rqRa2Y7zRx4iZ77JUqYDBI8WMnFeR54uiioTQ+rOs3zK2fGIWlrn4ohco/STHQSUTB8oCOFLMx1BkOqiR+UyebO28DJY7+V9ZmxB2Guyi7W8VScJcIdpSOPyJFOWZQKXdQFW3YICD2/toUx/pDAJh1sEVQsV3AoGBANyyp1rthmvoo5cVbymhYQ08vaERDwU3PLCtFXu4E0Ow90VNn6Ki4ueXcv/gFOp7pISk2/yuVTBTGjCblCiJ1en4HFWekJwrvgg3Vodtq8Okn6pyMCHRqvWEPqD5hw6rGEensk0K+FMXnF6GULlfn4mgEkYpb+PvDhSYvQSGfkPJAoGAF/bAKFqlM/1eJEvU7go35bNwEiij9Pvlfm8y2L8Qj2lhHxLV240CJ6IkBz1Rl+S3iNohkT8LnwqaKNT3kVB5daEBufxMuAmOlOX4PmZdxDj/r6hDg8ecmjj6VJbXt7JDd/c5ItKoVeGPqu035dpJyE+1xPAY9CLZel4scTsiQTkCgYBt3buRcZMwnc4qqpOOQcXK+DWD6QvpkcJ55ygHYw97iP/lF4euwdHd+I5b+11pJBAao7G0fHX3eSjqOmzReSKboSe5L8ZLB2cAI8AsKTBfKHWmCa8kDtgQuI86fUfirCGdhdA9AVP2QXN2eNCuPnFWi0WHm4fYuUB5be2c18ucxAb9CAESmgsK3QMIAhIQ071yBlsbLoO2CSB9Ds0cmRif6uevBiKOAjCCAQoCggEBAMJwhQD18uFNSe6dTaIQEvsZdONH+RQWhP+fWz6d9NWxs9FBhU/VaBLWN72nmpCof3d9fcUzbwklrBA0WjJqf8EpzIhGVJJJdOPO9o8drh7keJOimLriaKobCKNU8FAbnhWcpxwVCc10HlvBG9bWAEThsKfFu6/m9ESIxwvPURZ31V6WJReSOjHJLcsyTUjsJUGzKpVYoX0lJL91kXdxxZgNDsWsudQhCJKE5TUx1Zc1coaL+Xhwg0qe0BDUYGmkNN4wvwnIBTqEMpobAbUQjNAkJn7gB4wPXu+0epvxiT8u+RvX6qGTNUTM1QxymymT3d7BXdHNPu+4C81vHFhGmh8CAwEAASjwIkgBUqoBCAEQABqBAQQlRbfiBNDb6eU6aKrsH5WJaYszTioXjPLrWN9dqyW0vwfT11kgF0BbCGkAXew2tLJJqIuD95cjJvyGUSN6VyhL6dp44fWEGDSBIPR0mvRq7bMP+m7Y/RLKf83+OyVJu/BpxivQGC5YDL9f1/A8eLhTDNKXs4Ia5DrmTWdPTPBL8SIgyfUtg3ofI+/I9Tf7it7xXpT0AbQBJfNkcNXGpO3JcBMSgAIL5xsXK5of1mMwAl6ygN1Gsj4aZ052otnwN7kXk12SMsXheWTZ/PYh2KRzmt9RPS1T8hyFx/Kp5VkBV2vTAqqWrGw/dh4URqiHATZJUlhO7PN5m2Kq1LVFdXjWSzP5XBF2S83UMe+YruNHpE5GQrSyZcBqHO0QrdPcU35GBT7S7+IJr2AAXvnjqnb8yrtpPWN2ZW/IWUJN2z4vZ7/HV4aj3OZhkxC1DIMNyvsusUKoQQuf8gwKiEe8cFwbwFSicywlFk9la2IPe8oFShcxAzHLCCn/TIYUAvEL3/4LgaZvqWm80qCPYbgIP5HT8hPYkKWJ4WYknEWK+3InbnkzteFfGrQFCq4CCAESEGnj6Ji7LD+4o7MoHYT4jBQYjtW+kQUijgIwggEKAoIBAQDY9um1ifBRIOmkPtDZTqH+CZUBbb0eK0Cn3NHFf8MFUDzPEz+emK/OTub/hNxCJCao//pP5L8tRNUPFDrrvCBMo7Rn+iUb+mA/2yXiJ6ivqcN9Cu9i5qOU1ygon9SWZRsujFFB8nxVreY5Lzeq0283zn1Cg1stcX4tOHT7utPzFG/ReDFQt0O/GLlzVwB0d1sn3SKMO4XLjhZdncrtF9jljpg7xjMIlnWJUqxDo7TQkTytJmUl0kcM7bndBLerAdJFGaXc6oSY4eNy/IGDluLCQR3KZEQsy/mLeV1ggQ44MFr7XOM+rd+4/314q/deQbjHqjWFuVr8iIaKbq+R63ShAgMBAAEo8CISgAMii2Mw6z+Qs1bvvxGStie9tpcgoO2uAt5Zvv0CDXvrFlwnSbo+qR71Ru2IlZWVSbN5XYSIDwcwBzHjY8rNr3fgsXtSJty425djNQtF5+J2jrAhf3Q2m7EI5aohZGpD2E0cr+dVj9o8x0uJR2NWR8FVoVQSXZpad3M/4QzBLNto/tz+UKyZwa7Sc/eTQc2+ZcDS3ZEO3lGRsH864Kf/cEGvJRBBqcpJXKfG+ItqEW1AAPptjuggzmZEzRq5xTGf6or+bXrKjCpBS9G1SOyvCNF1k5z6lG8KsXhgQxL6ADHMoulxvUIihyPY5MpimdXfUdEQ5HA2EqNiNVNIO4qP007jW51yAeThOry4J22xs8RdkIClOGAauLIl0lLA4flMzW+VfQl5xYxP0E5tuhn0h+844DslU8ZF7U1dU2QprIApffXD9wgAACk26Rggy8e96z8i86/+YYyZQkc9hIdCAERrgEYCEbByzONrdRDs1MrS/ch1moV5pJv63BIKvQHGvLkaFwoMY29tcGFueV9uYW1lEgd1bmtub3duGioKCm1vZGVsX25hbWUSHEFuZHJvaWQgU0RLIGJ1aWx0IGZvciB4ODZfNjQaGwoRYXJjaGl0ZWN0dXJlX25hbWUSBng4Nl82NBodCgtkZXZpY2VfbmFtZRIOZ2VuZXJpY194ODZfNjQaIAoMcHJvZHVjdF9uYW1lEhBzZGtfcGhvbmVfeDg2XzY0GmMKCmJ1aWxkX2luZm8SVUFuZHJvaWQvc2RrX3Bob25lX3g4Nl82NC9nZW5lcmljX3g4Nl82NDo5L1BTUjEuMTgwNzIwLjAxMi80OTIzMjE0OnVzZXJkZWJ1Zy90ZXN0LWtleXMaHgoUd2lkZXZpbmVfY2RtX3ZlcnNpb24SBjE0LjAuMBokCh9vZW1fY3J5cHRvX3NlY3VyaXR5X3BhdGNoX2xldmVsEgEwMg4QASAIKA0wAEAASABQAA==";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_wvd_device() {
        let device = WvDevice::from_base64(HARDCODED_WVD_B64).expect("WVD parsing failed");
        assert!(device.system_id > 0, "system_id should be non-zero");
        assert!(device.client_id.token.is_some(), "client_id should have token");
    }

    #[test]
    fn test_cdm_challenge_generation() {
        let device = WvDevice::from_base64(HARDCODED_WVD_B64).expect("WVD parsing failed");
        let mut cdm = Cdm::new(device);
        let session_id = cdm.open();

        // Fake PSSH data (just a key ID)
        let fake_pssh = build_pssh_data(&[0x01; 16]);
        let challenge = cdm
            .get_license_challenge(&session_id, &fake_pssh)
            .expect("Challenge generation failed");

        assert!(!challenge.is_empty(), "challenge should not be empty");

        // Verify it's valid protobuf (SignedMessage)
        let signed = widevine::SignedMessage::decode(challenge.as_slice())
            .expect("Challenge should be valid SignedMessage protobuf");
        assert!(signed.msg.is_some(), "should have msg");
        assert!(signed.signature.is_some(), "should have signature");

        cdm.close(&session_id);
    }

    #[test]
    fn test_build_pssh_data() {
        let key_id = [0xAA; 16];
        let pssh_data = build_pssh_data(&key_id);
        assert!(!pssh_data.is_empty());

        // Should be valid WidevinePsshData protobuf
        let parsed: widevine::WidevinePsshData =
            prost::Message::decode(pssh_data.as_slice()).expect("Should parse as WidevinePsshData");
        assert_eq!(parsed.key_ids.len(), 1);
        assert_eq!(parsed.key_ids[0], key_id.to_vec());
    }
}
