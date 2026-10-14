mod discovery;
mod images;

pub use discovery::{DiscoverKind, DiscoverOptions};
pub use images::tmdb_image_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestProfileSelection {
    pub server_id: i64,
    pub profile_id: i64,
}
