//! Apple Music catalog search + storefront lookup.

use tracing::info;

use super::token::{USER_AGENT, call_with_dev_token_retry, get_developer_token};

const AMP_API_URL: &str = "https://amp-api.music.apple.com";

#[derive(Debug, serde::Serialize)]
pub struct SongResult {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub release_date: Option<String>,
    pub artwork_url: Option<String>,
    pub genre_names: Vec<String>,
    pub composer: Option<String>,
    pub has_lyrics: bool,
    pub has_time_synced_lyrics: bool,
    pub url: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct AlbumResult {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub track_count: Option<u64>,
    pub release_date: Option<String>,
    pub artwork_url: Option<String>,
    pub genre_names: Vec<String>,
    pub editorial_tagline: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct ArtistResult {
    pub id: String,
    pub name: String,
    pub genre_names: Vec<String>,
    pub url: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct PlaylistResult {
    pub id: String,
    pub name: String,
    pub curator: Option<String>,
    pub artwork_url: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct SearchResults {
    pub songs: Vec<SongResult>,
    pub albums: Vec<AlbumResult>,
    pub artists: Vec<ArtistResult>,
    pub playlists: Vec<PlaylistResult>,
}

/// Get the user's storefront region from their Apple Music account via API.
pub async fn get_user_storefront_from_api(music_user_token: &str) -> Result<String, String> {
    let user_token = music_user_token.to_string();
    call_with_dev_token_retry("get_user_storefront", || {
        let user_token = user_token.clone();
        async move {
            let dev_token = get_developer_token().await.map_err(|e| e.to_string())?;
            let client = reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .map_err(|e| format!("HTTP client error: {e}"))?;

            let resp = client
                .get(format!("{AMP_API_URL}/v1/me/storefront"))
                .header("Authorization", format!("Bearer {dev_token}"))
                .header("Media-User-Token", user_token.as_str())
                .header("Origin", "https://music.apple.com")
                .header(reqwest::header::COOKIE, format!("media-user-token={user_token}"))
                .send()
                .await
                .map_err(|e| format!("Get storefront: {e}"))?;

            if !resp.status().is_success() {
                return Err(format!("Storefront returned {}", resp.status()));
            }

            let json: serde_json::Value = resp.json().await.map_err(|e| format!("Parse storefront: {e}"))?;

            json.pointer("/data/0/id")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .ok_or_else(|| "No storefront id in response".to_string())
        }
    })
    .await
}

pub async fn search_catalog(
    music_user_token: Option<&str>,
    storefront: &str,
    query: &str,
    types: &str,
    limit: u32,
    offset: u32,
) -> Result<SearchResults, String> {
    let user_token = music_user_token.map(str::to_string);
    let storefront = storefront.to_string();
    let query = query.to_string();
    let types = types.to_string();
    call_with_dev_token_retry("search_catalog", || {
        let user_token = user_token.clone();
        let storefront = storefront.clone();
        let query = query.clone();
        let types = types.clone();
        async move {
            let dev_token = get_developer_token().await.map_err(|e| e.to_string())?;
            let client = reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .map_err(|e| format!("HTTP client error: {e}"))?;

            let url = format!("{AMP_API_URL}/v1/catalog/{storefront}/search");

            let mut req = client
                .get(&url)
                .header("Authorization", format!("Bearer {dev_token}"))
                .header("Origin", "https://music.apple.com")
                .header("Referer", "https://music.apple.com/")
                .query(&[
                    ("term", query.to_string()),
                    ("types", types.to_string()),
                    ("limit", limit.to_string()),
                    ("offset", offset.to_string()),
                ]);

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
                .map_err(|e| format!("Failed to parse search response: {e}"))?;

            let mut results = SearchResults::default();

            if let Some(songs) = json.pointer("/results/songs/data").and_then(|v| v.as_array()) {
                for song in songs {
                    let attrs = match song.get("attributes") {
                        Some(a) => a,
                        None => continue,
                    };
                    results.songs.push(SongResult {
                        id: song["id"].as_str().unwrap_or("").to_string(),
                        name: attrs["name"].as_str().unwrap_or("").to_string(),
                        artist: attrs["artistName"].as_str().unwrap_or("").to_string(),
                        album: attrs["albumName"].as_str().unwrap_or("").to_string(),
                        duration_ms: attrs
                            .get("durationInMillis")
                            .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok())))
                            .unwrap_or(0),
                        release_date: attrs["releaseDate"].as_str().map(str::to_owned),
                        artwork_url: attrs
                            .pointer("/artwork/url")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned),
                        genre_names: attrs
                            .get("genreNames")
                            .and_then(|v| v.as_array())
                            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                            .unwrap_or_default(),
                        composer: attrs["composerName"].as_str().map(str::to_owned),
                        has_lyrics: attrs["hasLyrics"].as_bool().unwrap_or(false),
                        has_time_synced_lyrics: attrs["hasTimeSyncedLyrics"].as_bool().unwrap_or(false),
                        url: attrs["url"].as_str().map(str::to_owned),
                    });
                }
            }

            if let Some(albums) = json.pointer("/results/albums/data").and_then(|v| v.as_array()) {
                for album in albums {
                    let attrs = match album.get("attributes") {
                        Some(a) => a,
                        None => continue,
                    };
                    results.albums.push(AlbumResult {
                        id: album["id"].as_str().unwrap_or("").to_string(),
                        name: attrs["name"].as_str().unwrap_or("").to_string(),
                        artist: attrs["artistName"].as_str().unwrap_or("").to_string(),
                        track_count: attrs["trackCount"].as_u64(),
                        release_date: attrs["releaseDate"].as_str().map(str::to_owned),
                        artwork_url: attrs
                            .pointer("/artwork/url")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned),
                        genre_names: attrs
                            .get("genreNames")
                            .and_then(|v| v.as_array())
                            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                            .unwrap_or_default(),
                        editorial_tagline: attrs
                            .pointer("/editorialNotes/tagline")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned),
                        url: attrs["url"].as_str().map(str::to_owned),
                    });
                }
            }

            if let Some(artists) = json.pointer("/results/artists/data").and_then(|v| v.as_array()) {
                for artist in artists {
                    let attrs = match artist.get("attributes") {
                        Some(a) => a,
                        None => continue,
                    };
                    results.artists.push(ArtistResult {
                        id: artist["id"].as_str().unwrap_or("").to_string(),
                        name: attrs["name"].as_str().unwrap_or("").to_string(),
                        genre_names: attrs
                            .get("genreNames")
                            .and_then(|v| v.as_array())
                            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                            .unwrap_or_default(),
                        url: attrs["url"].as_str().map(str::to_owned),
                    });
                }
            }

            if let Some(playlists) = json.pointer("/results/playlists/data").and_then(|v| v.as_array()) {
                for playlist in playlists {
                    let attrs = match playlist.get("attributes") {
                        Some(a) => a,
                        None => continue,
                    };
                    results.playlists.push(PlaylistResult {
                        id: playlist["id"].as_str().unwrap_or("").to_string(),
                        name: attrs["name"].as_str().unwrap_or("").to_string(),
                        curator: attrs["curatorName"].as_str().map(str::to_owned),
                        artwork_url: attrs
                            .pointer("/artwork/url")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned),
                        url: attrs["url"].as_str().map(str::to_owned),
                    });
                }
            }

            info!(
                "[AppleMusic] Search '{query}': {} songs, {} albums, {} artists, {} playlists",
                results.songs.len(),
                results.albums.len(),
                results.artists.len(),
                results.playlists.len(),
            );

            Ok(results)
        }
    })
    .await
}
