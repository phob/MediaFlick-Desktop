//! Desktop-side input validation and artwork handling for Companion-backed
//! Seerr features. Companion owns every authenticated Seerr request.

mod discovery;
mod images;

pub use discovery::{DiscoverKind, DiscoverOptions};
pub use images::tmdb_image_path;

/// An advanced request pins a title to one Seerr download destination and one
/// quality profile owned by that destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestProfileSelection {
    pub server_id: i64,
    pub profile_id: i64,
}
