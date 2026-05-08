//! Database helpers — only used by CLI (`verify_token`). Server mode never calls this.

#![allow(dead_code)]

const SCHEMA: &str = "apple_music";

/// Connect to the Tokimo main database using `DATABASE_URL` from the environment.
pub async fn init_pool() -> anyhow::Result<sea_orm::DatabaseConnection> {
    let url = std::env::var("DATABASE_URL").map_err(|_| anyhow::anyhow!("DATABASE_URL is not set"))?;
    let db = sea_orm::Database::connect(&url)
        .await
        .map_err(|e| anyhow::anyhow!("database connect failed: {e}"))?;
    Ok(db)
}

/// Placeholder — apple-music has no own schema/tables yet.
///
/// TODO: create apple-music specific schema/tables when needed.
pub async fn init_schema(_db: &sea_orm::DatabaseConnection) -> anyhow::Result<()> {
    Ok(())
}
