//! CLI entrypoints for apple-music.
//!
//! TODO: add Apple Music-specific subcommands (e.g. list playlists, play, status).

use anyhow::Context;
use tokimo_bus_auth::{
    cli::{Credentials, TokimoAuthArgs},
    db::{connect_db, verify_token},
};

/// Run the `status` subcommand: authenticate then print a placeholder.
pub async fn run_status(auth: TokimoAuthArgs) -> anyhow::Result<()> {
    let credentials = Credentials::resolve(&auth).context("resolve Tokimo credentials failed")?;
    let db = connect_db().await.context("connect database failed")?;
    let verified = match verify_token(&db, &credentials.token).await {
        Ok(v) => v,
        Err(e) => anyhow::bail!("verify Tokimo token failed: {e}"),
    };
    println!("Authenticated as user_id={}", verified.user_id);
    println!("apple-music CLI: TODO");
    Ok(())
}
