//! CLI entrypoints for apple-music.
//!
//! Subcommands: `status`, `search`, `download`.

use std::path::PathBuf;

use anyhow::Context;
use rust_apple_music::AudioQuality;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use tokimo_bus_auth::db::{connect_db, verify_token};
use tokimo_bus_cli::{Credentials, TokimoAuthArgs};
use uuid::Uuid;

use crate::handlers::api;

// ── Inline entity for `user_preferences` table (lives in main server's DB) ───

mod user_pref {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "user_preferences")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub user_id: Uuid,
        #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
        pub scope: String,
        #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
        pub scope_id: String,
        #[sea_orm(column_type = "JsonBinary")]
        pub value: Json,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

use user_pref::{Column, Entity};

// ── Shared init ──────────────────────────────────────────────────────────────

/// Resolve credentials → connect DB → verify token → return (db, user_id).
async fn init(auth: &TokimoAuthArgs) -> anyhow::Result<(DatabaseConnection, Uuid)> {
    let credentials = Credentials::resolve(auth).context("resolve Tokimo credentials failed")?;
    let db = connect_db().await.context("connect database failed")?;
    let verified = verify_token(&db, &credentials.token)
        .await
        .map_err(|e| anyhow::anyhow!("verify Tokimo token failed: {e}"))?;
    Ok((db, verified.user_id))
}

/// Read the Apple Music user token from `user_preferences` table.
async fn read_music_user_token(db: &DatabaseConnection, user_id: &Uuid) -> anyhow::Result<String> {
    let pref = Entity::find()
        .filter(Column::UserId.eq(*user_id))
        .filter(Column::Scope.eq("component"))
        .filter(Column::ScopeId.eq("apple-music-auth"))
        .one(db)
        .await
        .map_err(|e| anyhow::anyhow!("query user_preferences: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("no apple-music-auth preference found for user {user_id}"))?;

    pref.value
        .get("appleMusicToken")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("appleMusicToken is empty or missing"))
}

/// Read storefront from `user_preferences` table (set during login).
async fn read_storefront(db: &DatabaseConnection, user_id: &Uuid) -> anyhow::Result<String> {
    let pref = Entity::find()
        .filter(Column::UserId.eq(*user_id))
        .filter(Column::Scope.eq("component"))
        .filter(Column::ScopeId.eq("apple-music-settings"))
        .one(db)
        .await
        .map_err(|e| anyhow::anyhow!("query user_preferences: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("apple-music-settings not found. Please log in via the app first."))?;

    pref.value
        .get("storefront")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("storefront not set. Please log in via the app first."))
}

// ── Subcommands ──────────────────────────────────────────────────────────────

/// `status` — authenticate and print placeholder.
pub async fn run_status(auth: TokimoAuthArgs) -> anyhow::Result<()> {
    let (_db, user_id) = init(&auth).await?;
    println!("Authenticated as user_id={user_id}");
    println!("apple-music CLI: OK");
    Ok(())
}

/// `album` — show album detail with track listing.
pub async fn run_album(auth: TokimoAuthArgs, album_id: String, raw: bool) -> anyhow::Result<()> {
    let (db, user_id) = init(&auth).await?;

    let music_user_token = read_music_user_token(&db, &user_id).await.ok();
    let storefront = read_storefront(&db, &user_id).await?;

    if raw {
        let json = api::get_album_detail_json(music_user_token.as_deref(), &storefront, &album_id)
            .await
            .map_err(|e| anyhow::anyhow!("album detail: {e}"))?;
        println!("{}", serde_json::to_string_pretty(&json)?);
        return Ok(());
    }

    let detail = api::get_album_detail(music_user_token.as_deref(), &storefront, &album_id)
        .await
        .map_err(|e| anyhow::anyhow!("album detail: {e}"))?;

    // Header
    println!();
    println!("  Album:     {}", detail.name);
    println!("  Artist:    {}", detail.artist);
    if let Some(ref date) = detail.release_date {
        println!("  Released:  {date}");
    }
    if !detail.genre_names.is_empty() {
        println!("  Genre:     {}", detail.genre_names.join(", "));
    }
    if let Some(count) = detail.track_count {
        println!("  Tracks:    {count}");
    }
    if let Some(ref label) = detail.record_label {
        println!("  Label:     {label}");
    }
    if let Some(ref rating) = detail.content_rating {
        println!("  Rating:    {rating}");
    }
    if detail.is_single == Some(true) {
        println!("  Type:      Single");
    }
    if detail.is_prerelease == Some(true) {
        println!("  Pre-release: Yes");
    }
    if let Some(ref copyright) = detail.copyright {
        println!("  Copyright: {copyright}");
    }
    if let Some(ref notes) = detail.editorial_notes_short {
        println!("  Summary:   {notes}");
    }
    println!("  URL:       https://music.apple.com/{storefront}/album/{album_id}");
    println!("  ID:        {}", detail.id);

    // Track listing
    if !detail.tracks.is_empty() {
        println!();
        println!(
            "  {:<12} {:<4} {:<4} {:<40} {:<20} {:<8} Composer",
            "ID", "Disc", "#", "Name", "Artist", "Duration"
        );
        println!("  {}", "-".repeat(110));
        for t in &detail.tracks {
            println!(
                "  {:<12} {:<4} {:<4} {:<40} {:<20} {:<8} {}",
                truncate(&t.id, 12),
                t.disc_number.map_or("-".to_string(), |d| d.to_string()),
                t.track_number.map_or("-".to_string(), |n| n.to_string()),
                truncate(&t.name, 40),
                truncate(&t.artist, 20),
                format_duration(t.duration_ms),
                t.composer.as_deref().unwrap_or("-"),
            );
        }
    }

    Ok(())
}

/// `search` — search Apple Music catalog for songs and albums.
pub async fn run_search(
    auth: TokimoAuthArgs,
    query: String,
    types: String,
    limit: u32,
    page: u32,
) -> anyhow::Result<()> {
    let (db, user_id) = init(&auth).await?;
    let offset = (page.saturating_sub(1)) * limit;

    let music_user_token = read_music_user_token(&db, &user_id).await.ok();
    let storefront = read_storefront(&db, &user_id).await?;

    let results = api::search_catalog(music_user_token.as_deref(), &storefront, &query, &types, limit, offset)
        .await
        .map_err(|e| anyhow::anyhow!("search: {e}"))?;

    println!("Page {page} (offset {offset}, limit {limit}):\n");

    // ── Songs ──
    if !results.songs.is_empty() {
        println!("\n♫ Songs ({}):\n", results.songs.len());
        println!(
            "  {:<10} {:<35} {:<20} {:<25} {:<8} {:<15} Genre",
            "ID", "Name", "Artist", "Album", "Duration", "Composer"
        );
        println!("  {}", "-".repeat(140));
        for s in &results.songs {
            println!(
                "  {:<10} {:<35} {:<20} {:<25} {:<8} {:<15} {}",
                truncate(&s.id, 10),
                truncate(&s.name, 35),
                truncate(&s.artist, 20),
                truncate(&s.album, 25),
                format_duration(s.duration_ms),
                truncate(s.composer.as_deref().unwrap_or("-"), 15),
                s.genre_names.first().map(|s| s.as_str()).unwrap_or("-"),
            );
        }
    }

    // ── Albums ──
    if !results.albums.is_empty() {
        println!("\n📁 Albums ({}):\n", results.albums.len());
        println!(
            "  {:<10} {:<35} {:<20} {:<8} {:<12} Genre",
            "ID", "Name", "Artist", "Tracks", "Released"
        );
        println!("  {}", "-".repeat(110));
        for a in &results.albums {
            println!(
                "  {:<10} {:<35} {:<20} {:<8} {:<12} {}",
                truncate(&a.id, 10),
                truncate(&a.name, 35),
                truncate(&a.artist, 20),
                a.track_count.map_or("-".to_string(), |c| c.to_string()),
                a.release_date.as_deref().unwrap_or("-"),
                a.genre_names.first().map(|s| s.as_str()).unwrap_or("-"),
            );
            if let Some(ref tagline) = a.editorial_tagline {
                println!("  {:<10} {}", "", tagline);
            }
        }
    }

    // ── Artists ──
    if !results.artists.is_empty() {
        println!("\n🎤 Artists ({}):\n", results.artists.len());
        println!("  {:<10} {:<40} Genre", "ID", "Name");
        println!("  {}", "-".repeat(70));
        for a in &results.artists {
            println!(
                "  {:<10} {:<40} {}",
                truncate(&a.id, 10),
                truncate(&a.name, 40),
                if a.genre_names.is_empty() {
                    "-".to_string()
                } else {
                    a.genre_names.join(", ")
                },
            );
        }
    }

    // ── Playlists ──
    if !results.playlists.is_empty() {
        println!("\n🎧 Playlists ({}):\n", results.playlists.len());
        println!("  {:<10} {:<40} Curator", "ID", "Name");
        println!("  {}", "-".repeat(60));
        for p in &results.playlists {
            println!(
                "  {:<10} {:<40} {}",
                truncate(&p.id, 10),
                truncate(&p.name, 40),
                p.curator.as_deref().unwrap_or("-"),
            );
        }
    }

    if results.songs.is_empty()
        && results.albums.is_empty()
        && results.artists.is_empty()
        && results.playlists.is_empty()
    {
        println!("No results found for '{query}'.");
    }

    Ok(())
}

/// `download` — download + decrypt a track to a local file.
pub async fn run_download(
    auth: TokimoAuthArgs,
    track_id: String,
    output: PathBuf,
    quality: String,
) -> anyhow::Result<()> {
    let (db, user_id) = init(&auth).await?;

    let music_user_token = read_music_user_token(&db, &user_id)
        .await
        .context("music-user-token is required for download. Save it first via the app UI.")?;

    let quality = AudioQuality::from_str_loose(&quality);

    println!("Downloading track {track_id} (quality={})...", quality.as_str());

    let path = api::download_track(&music_user_token, &track_id, &output, quality)
        .await
        .map_err(|e| anyhow::anyhow!("download: {e}"))?;

    println!("Saved to: {}", path.display());
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let end = s
            .char_indices()
            .nth(max - 1)
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(s.len());
        format!("{}…", &s[..end])
    }
}

fn format_duration(ms: u64) -> String {
    let total_secs = ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{mins}:{secs:02}")
}
