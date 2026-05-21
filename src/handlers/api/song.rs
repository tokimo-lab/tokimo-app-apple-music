//! Apple Music song detail + lyrics.

use super::token::{USER_AGENT, call_with_dev_token_retry, get_developer_token};

const AMP_API_URL: &str = "https://amp-api.music.apple.com";

#[derive(Debug, serde::Serialize)]
pub struct SongDetail {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub artist_id: Option<String>,
    pub album: String,
    pub album_id: Option<String>,
    pub duration_ms: u64,
    pub track_number: Option<u64>,
    pub disc_number: Option<u64>,
    pub release_date: Option<String>,
    pub genre_names: Vec<String>,
    pub composer: Option<String>,
    pub has_lyrics: bool,
    pub has_time_synced_lyrics: bool,
    pub content_rating: Option<String>,
    pub copyright: Option<String>,
    pub isrc: Option<String>,
    pub artwork_url: Option<String>,
    pub editorial_notes_short: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct SongLyrics {
    pub ttml: Option<String>,
    pub lines: Vec<(Option<String>, String)>,
}

pub async fn get_song_detail(
    music_user_token: Option<&str>,
    storefront: &str,
    song_id: &str,
) -> Result<SongDetail, String> {
    let user_token = music_user_token.map(str::to_string);
    let storefront = storefront.to_string();
    let song_id = song_id.to_string();
    call_with_dev_token_retry("get_song_detail", || {
        let user_token = user_token.clone();
        let storefront = storefront.clone();
        let song_id = song_id.clone();
        async move {
            let dev_token = get_developer_token().await.map_err(|e| e.to_string())?;
            let client = reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .map_err(|e| format!("HTTP client error: {e}"))?;

            let url = format!("{AMP_API_URL}/v1/catalog/{storefront}/songs/{song_id}");
            let mut req = client
                .get(&url)
                .header("Authorization", format!("Bearer {dev_token}"))
                .header("Origin", "https://music.apple.com")
                .header("Referer", "https://music.apple.com/");

            if let Some(ref token) = user_token {
                req = req
                    .header("Media-User-Token", token.as_str())
                    .header(reqwest::header::COOKIE, format!("media-user-token={token}"));
            }

            let resp = req.send().await.map_err(|e| format!("Apple Music API error: {e}"))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("Apple Music API returned {status}: {body}"));
            }

            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| format!("Failed to parse song response: {e}"))?;

            let song = json
                .pointer("/data/0")
                .ok_or_else(|| "Song not found in response".to_string())?;

            let attrs = song
                .get("attributes")
                .ok_or_else(|| "No attributes in song response".to_string())?;

            Ok(SongDetail {
                id: song["id"].as_str().unwrap_or("").to_string(),
                name: attrs["name"].as_str().unwrap_or("").to_string(),
                artist: attrs["artistName"].as_str().unwrap_or("").to_string(),
                artist_id: json
                    .pointer("/data/0/relationships/artists/data/0/id")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                album: attrs["albumName"].as_str().unwrap_or("").to_string(),
                album_id: json
                    .pointer("/data/0/relationships/albums/data/0/id")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                duration_ms: attrs
                    .get("durationInMillis")
                    .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok())))
                    .unwrap_or(0),
                track_number: attrs["trackNumber"].as_u64(),
                disc_number: attrs["discNumber"].as_u64(),
                release_date: attrs["releaseDate"].as_str().map(str::to_owned),
                genre_names: attrs
                    .get("genreNames")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                    .unwrap_or_default(),
                composer: attrs["composerName"].as_str().map(str::to_owned),
                has_lyrics: attrs["hasLyrics"].as_bool().unwrap_or(false),
                has_time_synced_lyrics: attrs["hasTimeSyncedLyrics"].as_bool().unwrap_or(false),
                content_rating: attrs["contentRating"].as_str().map(str::to_owned),
                copyright: attrs["copyright"].as_str().map(str::to_owned),
                isrc: attrs["isrc"].as_str().map(str::to_owned),
                artwork_url: attrs
                    .pointer("/artwork/url")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                editorial_notes_short: attrs
                    .pointer("/editorialNotes/short")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                url: attrs["url"].as_str().map(str::to_owned),
            })
        }
    })
    .await
}

/// Same as `get_song_detail` but returns the raw JSON response.
pub async fn get_song_detail_json(
    music_user_token: Option<&str>,
    storefront: &str,
    song_id: &str,
) -> Result<serde_json::Value, String> {
    let user_token = music_user_token.map(str::to_string);
    let storefront = storefront.to_string();
    let song_id = song_id.to_string();
    call_with_dev_token_retry("get_song_detail_json", || {
        let user_token = user_token.clone();
        let storefront = storefront.clone();
        let song_id = song_id.clone();
        async move {
            let dev_token = get_developer_token().await.map_err(|e| e.to_string())?;
            let client = reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .map_err(|e| format!("HTTP client error: {e}"))?;

            let url = format!("{AMP_API_URL}/v1/catalog/{storefront}/songs/{song_id}");
            let mut req = client
                .get(&url)
                .header("Authorization", format!("Bearer {dev_token}"))
                .header("Origin", "https://music.apple.com")
                .header("Referer", "https://music.apple.com/");

            if let Some(ref token) = user_token {
                req = req
                    .header("Media-User-Token", token.as_str())
                    .header(reqwest::header::COOKIE, format!("media-user-token={token}"));
            }

            let resp = req.send().await.map_err(|e| format!("Apple Music API error: {e}"))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("Apple Music API returned {status}: {body}"));
            }

            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| format!("Failed to parse song response: {e}"))?;

            Ok(json)
        }
    })
    .await
}

pub async fn get_song_lyrics(music_user_token: &str, storefront: &str, song_id: &str) -> Result<SongLyrics, String> {
    let user_token = music_user_token.to_string();
    let storefront = storefront.to_string();
    let song_id = song_id.to_string();
    call_with_dev_token_retry("get_song_lyrics", || {
        let user_token = user_token.clone();
        let storefront = storefront.clone();
        let song_id = song_id.clone();
        async move {
            let dev_token = get_developer_token().await.map_err(|e| e.to_string())?;
            let client = reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .map_err(|e| format!("HTTP client error: {e}"))?;

            let url = format!("{AMP_API_URL}/v1/catalog/{storefront}/songs/{song_id}/lyrics");
            let req = client
                .get(&url)
                .header("Authorization", format!("Bearer {dev_token}"))
                .header("Media-User-Token", user_token.as_str())
                .header(reqwest::header::COOKIE, format!("media-user-token={user_token}"))
                .header("Origin", "https://music.apple.com")
                .header("Referer", "https://music.apple.com/");

            let resp = req.send().await.map_err(|e| format!("Apple Music API error: {e}"))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("Apple Music API returned {status}: {body}"));
            }

            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| format!("Failed to parse lyrics response: {e}"))?;

            let attrs = json
                .pointer("/data/0/attributes")
                .ok_or_else(|| "No lyrics available for this song".to_string())?;

            let ttml = attrs["ttml"].as_str().map(str::to_owned);

            let lines = if let Some(ref ttml_str) = ttml {
                extract_lyrics_from_ttml(ttml_str)
            } else {
                Vec::new()
            };

            Ok(SongLyrics { ttml, lines })
        }
    })
    .await
}

/// Extract timestamped text lines from Apple Music TTML format.
fn extract_lyrics_from_ttml(ttml: &str) -> Vec<(Option<String>, String)> {
    use regex::Regex;

    let Ok(p_re) = Regex::new(r#"<p\s[^>]*?begin="([^"]+)"[^>]*>(.*?)</p>"#) else {
        return Vec::new();
    };
    let Ok(p_no_ts_re) = Regex::new(r"<p[^>]*>(.*?)</p>") else {
        return Vec::new();
    };
    let tag_re = Regex::new(r"<[^>]+>").unwrap();

    let has_timestamps = ttml.contains("begin=");

    if has_timestamps {
        p_re.captures_iter(ttml)
            .filter_map(|cap| {
                let raw_ts = cap[1].to_string();
                let ts = format_ttml_timestamp(&raw_ts);
                let text = tag_re.replace_all(&cap[2], "");
                let text = text.trim();
                if text.is_empty() {
                    None
                } else {
                    Some((Some(ts), text.to_string()))
                }
            })
            .collect()
    } else {
        p_no_ts_re
            .captures_iter(ttml)
            .filter_map(|cap| {
                let text = tag_re.replace_all(&cap[1], "");
                let text = text.trim();
                if text.is_empty() {
                    None
                } else {
                    Some((None, text.to_string()))
                }
            })
            .collect()
    }
}

/// Convert TTML timestamp to "M:SS" format.
fn format_ttml_timestamp(raw: &str) -> String {
    let parts: Vec<&str> = raw.split(':').collect();
    let (h, m, s_with_ms) = match parts.len() {
        3 => (
            parts[0].parse::<u64>().unwrap_or(0),
            parts[1].parse::<u64>().unwrap_or(0),
            parts[2],
        ),
        2 => (0, parts[0].parse::<u64>().unwrap_or(0), parts[1]),
        1 => {
            let total_secs = raw.split('.').next().unwrap_or("0").parse::<u64>().unwrap_or(0);
            let mins = total_secs / 60;
            let secs = total_secs % 60;
            return format!("{mins}:{secs:02}");
        }
        _ => return raw.to_string(),
    };
    let secs = s_with_ms.split('.').next().unwrap_or("0").parse::<u64>().unwrap_or(0);
    let total_mins = h * 60 + m;
    format!("{total_mins}:{secs:02}")
}

/// Same as `get_song_lyrics` but returns the raw JSON response.
pub async fn get_song_lyrics_json(
    music_user_token: &str,
    storefront: &str,
    song_id: &str,
) -> Result<serde_json::Value, String> {
    let user_token = music_user_token.to_string();
    let storefront = storefront.to_string();
    let song_id = song_id.to_string();
    call_with_dev_token_retry("get_song_lyrics_json", || {
        let user_token = user_token.clone();
        let storefront = storefront.clone();
        let song_id = song_id.clone();
        async move {
            let dev_token = get_developer_token().await.map_err(|e| e.to_string())?;
            let client = reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .map_err(|e| format!("HTTP client error: {e}"))?;

            let url = format!("{AMP_API_URL}/v1/catalog/{storefront}/songs/{song_id}/lyrics");
            let req = client
                .get(&url)
                .header("Authorization", format!("Bearer {dev_token}"))
                .header("Media-User-Token", user_token.as_str())
                .header(reqwest::header::COOKIE, format!("media-user-token={user_token}"))
                .header("Origin", "https://music.apple.com")
                .header("Referer", "https://music.apple.com/");

            let resp = req.send().await.map_err(|e| format!("Apple Music API error: {e}"))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("Apple Music API returned {status}: {body}"));
            }

            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| format!("Failed to parse lyrics response: {e}"))?;

            Ok(json)
        }
    })
    .await
}
