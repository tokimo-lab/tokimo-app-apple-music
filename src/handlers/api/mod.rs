//! Apple Music API — split by domain.

mod album;
mod artist;
mod download;
mod search;
mod song;
mod token;

pub use album::*;
pub use artist::*;
pub use download::*;
pub use search::*;
pub use song::*;
pub use token::*;
