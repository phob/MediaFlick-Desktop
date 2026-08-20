//! Seerr, the project formerly called Jellyseerr, as a request backend.
//!
//! MediaFlick is a client to Seerr, not a second arr orchestrator. Seerr owns
//! quality profiles, root folders, approval rules, and quotas. MediaFlick keeps
//! the user's session cookie, never an instance-wide API key.
//!
//! A session belongs to one Jellyfin account. Every client acquisition checks
//! that binding so one user's Seerr cookie cannot serve another user.

pub mod api;
pub mod headless;

mod discovery;
mod session;

pub use discovery::{DiscoverKind, DiscoverOptions};
pub use session::{RequestProfileSelection, SeerrSession};
