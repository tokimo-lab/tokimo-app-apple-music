//! Apple Music artist detail with album listing (paginated).

use super::token::{USER_AGENT, call_with_dev_token_retry, get_developer_token};

const AMP_API_URL: &str = "https://amp-api.music.apple.com";

#[derive(Debug, serde::Serialize)]
pub struct ArtistAlbum {
    pub id: String,
    pub name: String,
    pub release_date: Option<String>,
    pub track_count: Option<u64>,
    pub artwork_url: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct ArtistDetail {
    pub id: String,
    pub name: String,
    pub genre_names: Vec<String>,
    pub editorial_notes_short: Option<String>,
    pub editorial_notes_standard: Option<String>,
    pub artwork_url: Option<String>,
    pub url: Option<String>,
    pub albums: Vec<ArtistAlbum>,
}

pub async fn get_artist_detail(
    music_user_token: Option<&str>,
    storefront: &str,
    artist_id: &str,
) -> Result<ArtistDetail, String> {
    let user_token = music_user_token.map(str::to_string);
    let storefront = storefront.to_string();
    let artist_id = artist_id.to_string();
    call_with_dev_token_retry("get_artist_detail", || {
        let user_token = user_token.clone();
        let storefront = storefront.clone();
        let artist_id = artist_id.clone();
        async move {
            let dev_token = get_developer_token().await.map_err(|e| e.to_string())?;
            let client = reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .map_err(|e| format!("HTTP client error: {e}"))?;

            let url = format!("{AMP_API_URL}/v1/catalog/{storefront}/artists/{artist_id}?include=albums");
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
                .map_err(|e| format!("Failed to parse artist response: {e}"))?;

            let artist = json
                .pointer("/data/0")
                .ok_or_else(|| "Artist not found in response".to_string())?;

            let attrs = artist
                .get("attributes")
                .ok_or_else(|| "No attributes in artist response".to_string())?;

            // Parse albums from relationships (with pagination)
            let mut albums = Vec::new();
            let mut next_url = json
                .pointer("/data/0/relationships/albums/next")
                .and_then(|v| v.as_str())
                .map(str::to_owned);

            if let Some(album_data) = json
                .pointer("/data/0/relationships/albums/data")
                .and_then(|v| v.as_array())
            {
                for item in album_data {
                    let Some(item_attrs) = item.get("attributes") else {
                        continue;
                    };
                    albums.push(ArtistAlbum {
                        id: item["id"].as_str().unwrap_or("").to_string(),
                        name: item_attrs["name"].as_str().unwrap_or("").to_string(),
                        release_date: item_attrs["releaseDate"].as_str().map(str::to_owned),
                        track_count: item_attrs["trackCount"].as_u64(),
                        artwork_url: item_attrs
                            .pointer("/artwork/url")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned),
                        url: item_attrs["url"].as_str().map(str::to_owned),
                    });
                }
            }

            // Follow pagination to get all albums
            while let Some(ref path) = next_url {
                let page_url = format!("{AMP_API_URL}{path}");
                let page_resp = client
                    .get(&page_url)
                    .header("Authorization", format!("Bearer {dev_token}"))
                    .header("Origin", "https://music.apple.com")
                    .header("Referer", "https://music.apple.com/")
                    .send()
                    .await
                    .map_err(|e| format!("Apple Music API error: {e}"))?;

                if !page_resp.status().is_success() {
                    break;
                }

                let page_json: serde_json::Value = page_resp
                    .json()
                    .await
                    .map_err(|e| format!("Failed to parse albums page: {e}"))?;

                next_url = page_json.pointer("/next").and_then(|v| v.as_str()).map(str::to_owned);

                if let Some(items) = page_json.get("data").and_then(|v| v.as_array()) {
                    for item in items {
                        let Some(item_attrs) = item.get("attributes") else {
                            continue;
                        };
                        albums.push(ArtistAlbum {
                            id: item["id"].as_str().unwrap_or("").to_string(),
                            name: item_attrs["name"].as_str().unwrap_or("").to_string(),
                            release_date: item_attrs["releaseDate"].as_str().map(str::to_owned),
                            track_count: item_attrs["trackCount"].as_u64(),
                            artwork_url: item_attrs
                                .pointer("/artwork/url")
                                .and_then(|v| v.as_str())
                                .map(str::to_owned),
                            url: item_attrs["url"].as_str().map(str::to_owned),
                        });
                    }
                }
            }

            // Sort by release date, newest first
            albums.sort_by(|a, b| {
                b.release_date
                    .as_deref()
                    .unwrap_or("")
                    .cmp(a.release_date.as_deref().unwrap_or(""))
            });

            Ok(ArtistDetail {
                id: artist["id"].as_str().unwrap_or("").to_string(),
                name: attrs["name"].as_str().unwrap_or("").to_string(),
                genre_names: attrs
                    .get("genreNames")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                    .unwrap_or_default(),
                editorial_notes_short: attrs
                    .pointer("/editorialNotes/short")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                editorial_notes_standard: attrs
                    .pointer("/editorialNotes/standard")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                artwork_url: attrs
                    .pointer("/artwork/url")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                url: attrs["url"].as_str().map(str::to_owned),
                albums,
            })
        }
    })
    .await
}

/// Same as `get_artist_detail` but returns the raw JSON response.
pub async fn get_artist_detail_json(
    music_user_token: Option<&str>,
    storefront: &str,
    artist_id: &str,
) -> Result<serde_json::Value, String> {
    let user_token = music_user_token.map(str::to_string);
    let storefront = storefront.to_string();
    let artist_id = artist_id.to_string();
    call_with_dev_token_retry("get_artist_detail_json", || {
        let user_token = user_token.clone();
        let storefront = storefront.clone();
        let artist_id = artist_id.clone();
        async move {
            let dev_token = get_developer_token().await.map_err(|e| e.to_string())?;
            let client = reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .map_err(|e| format!("HTTP client error: {e}"))?;

            let url = format!("{AMP_API_URL}/v1/catalog/{storefront}/artists/{artist_id}?include=albums");
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
                .map_err(|e| format!("Failed to parse artist response: {e}"))?;

            Ok(json)
        }
    })
    .await
}
