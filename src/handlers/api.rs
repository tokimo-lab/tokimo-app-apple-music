//! Shared Apple Music API helpers — developer token scraping + search/download.
//!
//! Used by both the HTTP handlers (via `get_developer_token`) and the CLI.

use std::sync::OnceLock;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::error::AppError;

const APPLE_MUSIC_URL: &str = "https://music.apple.com/us/browse";
pub const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const TOKEN_TTL_SECS: u64 = 3600;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Cached developer token ────────────────────────────────────────────────────

#[derive(Clone)]
struct CachedToken {
    token: String,
    fetched_at: std::time::Instant,
}

static TOKEN_CACHE: OnceLock<RwLock<Option<CachedToken>>> = OnceLock::new();

fn cache() -> &'static RwLock<Option<CachedToken>> {
    TOKEN_CACHE.get_or_init(|| RwLock::new(None))
}

async fn scrape_developer_token() -> Result<String, AppError> {
    use regex::Regex;

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| AppError::Internal(format!("HTTP client error: {e}")))?;

    let html = client
        .get(APPLE_MUSIC_URL)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to fetch Apple Music page: {e}")))?
        .text()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to read page body: {e}")))?;

    let js_path_re = Regex::new(r"/assets/index[~-][a-zA-Z0-9._-]+\.js")
        .map_err(|e| AppError::Internal(format!("Regex error: {e}")))?;

    let js_path = js_path_re
        .find(&html)
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| AppError::Internal("Could not find JS bundle path in Apple Music page".into()))?;

    let js_url = format!("https://music.apple.com{js_path}");
    debug!("[AppleMusic] Fetching JS bundle: {js_url}");

    let js_content = client
        .get(&js_url)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to fetch JS bundle: {e}")))?
        .text()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to read JS bundle: {e}")))?;

    let token_re = Regex::new(r#""(eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6[^"]+)""#)
        .map_err(|e| AppError::Internal(format!("Regex error: {e}")))?;

    if let Some(cap) = token_re.captures(&js_content) {
        return Ok(cap[1].to_string());
    }

    let jwt_re = Regex::new(r#""(eyJ[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{20,})""#)
        .map_err(|e| AppError::Internal(format!("Regex error: {e}")))?;

    if let Some(cap) = jwt_re.captures(&js_content) {
        return Ok(cap[1].to_string());
    }

    Err(AppError::Internal(
        "Could not extract developer token from Apple Music JS bundle".into(),
    ))
}

fn token_cache_path() -> std::path::PathBuf {
    std::env::temp_dir().join("tokimo-apple-music-dev-token")
}

/// Clear both in-memory and file token caches, forcing a fresh scrape on next call.
pub async fn invalidate_developer_token() {
    {
        let mut guard = cache().write().await;
        *guard = None;
    }
    let _ = std::fs::remove_file(token_cache_path());
    info!("[AppleMusic] Developer token cache invalidated");
}

pub async fn get_developer_token() -> Result<String, AppError> {
    // 1. In-memory cache (fast path for server mode)
    {
        let guard = cache().read().await;
        if let Some(cached) = guard.as_ref()
            && cached.fetched_at.elapsed().as_secs() < TOKEN_TTL_SECS
        {
            return Ok(cached.token.clone());
        }
    }

    // 2. File cache (survives across CLI invocations)
    let cache_path = token_cache_path();
    if let Ok(content) = std::fs::read_to_string(&cache_path)
        && let Some((ts_str, token)) = content.split_once('\n')
        && let Ok(ts) = ts_str.parse::<u64>()
        && ts + TOKEN_TTL_SECS > now_secs()
    {
        // Refresh in-memory cache too
        let mut guard = cache().write().await;
        *guard = Some(CachedToken {
            token: token.to_string(),
            fetched_at: std::time::Instant::now() - std::time::Duration::from_secs(now_secs().saturating_sub(ts)),
        });
        return Ok(token.to_string());
    }

    // 3. Scrape from Apple
    debug!("[AppleMusic] Scraping fresh developer token...");
    let token = scrape_developer_token().await?;
    debug!("[AppleMusic] Token obtained ({} chars)", token.len());

    // Write to file cache
    let _ = std::fs::write(&cache_path, format!("{}\n{}", now_secs(), token));

    {
        let mut guard = cache().write().await;
        *guard = Some(CachedToken {
            token: token.clone(),
            fetched_at: std::time::Instant::now(),
        });
    }

    Ok(token)
}

/// Retry an Apple Music API call once on 401 (expired dev token).
///
/// On the first 401, invalidates the cached dev token and retries with a fresh one.
/// The closure receives the current dev token and should call `get_developer_token()`
/// (or accept a pre-fetched token) for each attempt.
pub async fn call_with_dev_token_retry<T, E, F, Fut>(description: &str, mut call: F) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    match call().await {
        Ok(v) => Ok(v),
        Err(e) if e.to_string().contains("401") => {
            info!("[AppleMusic] {description} got 401 — refreshing dev token and retrying");
            invalidate_developer_token().await;
            call().await
        }
        Err(e) => Err(e),
    }
}

// ── Search types ─────────────────────────────────────────────────────────────

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

// ── Album detail ─────────────────────────────────────────────────────────────

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

// ── Download ─────────────────────────────────────────────────────────────────

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
            download_decrypted_audio(&token, &user_token, &track, &cache, None, quality).await
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
