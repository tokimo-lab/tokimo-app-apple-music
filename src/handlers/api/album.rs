//! Apple Music album detail.

use super::token::{USER_AGENT, call_with_dev_token_retry, get_developer_token};

const AMP_API_URL: &str = "https://amp-api.music.apple.com";

#[derive(Debug, serde::Serialize)]
pub struct AlbumTrack {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub duration_ms: u64,
    pub track_number: Option<u64>,
    pub disc_number: Option<u64>,
    pub composer: Option<String>,
    pub has_lyrics: bool,
    pub has_time_synced_lyrics: bool,
    pub isrc: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct AlbumDetail {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub artist_id: Option<String>,
    pub release_date: Option<String>,
    pub genre: Option<String>,
    pub genre_names: Vec<String>,
    pub track_count: Option<u64>,
    pub copyright: Option<String>,
    pub content_rating: Option<String>,
    pub record_label: Option<String>,
    pub is_single: Option<bool>,
    pub is_prerelease: Option<bool>,
    pub artwork_url: Option<String>,
    pub editorial_notes_short: Option<String>,
    pub url: Option<String>,
    pub tracks: Vec<AlbumTrack>,
}

pub async fn get_album_detail(
    music_user_token: Option<&str>,
    storefront: &str,
    album_id: &str,
) -> Result<AlbumDetail, String> {
    let user_token = music_user_token.map(str::to_string);
    let storefront = storefront.to_string();
    let album_id = album_id.to_string();
    call_with_dev_token_retry("get_album_detail", || {
        let user_token = user_token.clone();
        let storefront = storefront.clone();
        let album_id = album_id.clone();
        async move {
            let dev_token = get_developer_token().await.map_err(|e| e.to_string())?;
            let client = reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .map_err(|e| format!("HTTP client error: {e}"))?;

            let url = format!("{AMP_API_URL}/v1/catalog/{storefront}/albums/{album_id}");
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
                .map_err(|e| format!("Failed to parse album response: {e}"))?;

            let album = json
                .pointer("/data/0")
                .ok_or_else(|| "Album not found in response".to_string())?;

            let attrs = album
                .get("attributes")
                .ok_or_else(|| "No attributes in album response".to_string())?;

            let mut tracks = Vec::new();
            if let Some(track_data) = json
                .pointer("/data/0/relationships/tracks/data")
                .and_then(|v| v.as_array())
            {
                for t in track_data {
                    let ta = match t.get("attributes") {
                        Some(a) => a,
                        None => continue,
                    };
                    tracks.push(AlbumTrack {
                        id: t["id"].as_str().unwrap_or("").to_string(),
                        name: ta["name"].as_str().unwrap_or("").to_string(),
                        artist: ta["artistName"].as_str().unwrap_or("").to_string(),
                        duration_ms: ta
                            .get("durationInMillis")
                            .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok())))
                            .unwrap_or(0),
                        track_number: ta["trackNumber"].as_u64(),
                        disc_number: ta["discNumber"].as_u64(),
                        composer: ta["composerName"].as_str().map(str::to_owned),
                        has_lyrics: ta["hasLyrics"].as_bool().unwrap_or(false),
                        has_time_synced_lyrics: ta["hasTimeSyncedLyrics"].as_bool().unwrap_or(false),
                        isrc: ta["isrc"].as_str().map(str::to_owned),
                        url: ta["url"].as_str().map(str::to_owned),
                    });
                }
            }

            Ok(AlbumDetail {
                id: album["id"].as_str().unwrap_or("").to_string(),
                name: attrs["name"].as_str().unwrap_or("").to_string(),
                artist: attrs["artistName"].as_str().unwrap_or("").to_string(),
                artist_id: json
                    .pointer("/data/0/relationships/artists/data/0/id")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                release_date: attrs["releaseDate"].as_str().map(str::to_owned),
                genre: attrs
                    .pointer("/genreNames/0")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                genre_names: attrs
                    .get("genreNames")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                    .unwrap_or_default(),
                track_count: attrs["trackCount"].as_u64(),
                copyright: attrs["copyright"].as_str().map(str::to_owned),
                content_rating: attrs["contentRating"].as_str().map(str::to_owned),
                record_label: attrs["recordLabel"].as_str().map(str::to_owned),
                is_single: attrs["isSingle"].as_bool(),
                is_prerelease: attrs["isPrerelease"].as_bool(),
                artwork_url: attrs
                    .pointer("/artwork/url")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                editorial_notes_short: attrs
                    .pointer("/editorialNotes/short")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                url: attrs["url"].as_str().map(str::to_owned),
                tracks,
            })
        }
    })
    .await
}

/// Same as `get_album_detail` but returns the raw JSON response.
pub async fn get_album_detail_json(
    music_user_token: Option<&str>,
    storefront: &str,
    album_id: &str,
) -> Result<serde_json::Value, String> {
    let user_token = music_user_token.map(str::to_string);
    let storefront = storefront.to_string();
    let album_id = album_id.to_string();
    call_with_dev_token_retry("get_album_detail_json", || {
        let user_token = user_token.clone();
        let storefront = storefront.clone();
        let album_id = album_id.clone();
        async move {
            let dev_token = get_developer_token().await.map_err(|e| e.to_string())?;
            let client = reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .map_err(|e| format!("HTTP client error: {e}"))?;

            let url = format!("{AMP_API_URL}/v1/catalog/{storefront}/albums/{album_id}");

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
                .map_err(|e| format!("Failed to parse album response: {e}"))?;

            Ok(json)
        }
    })
    .await
}
