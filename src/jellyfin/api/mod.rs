//! Shared Jellyfin REST client.
//!
//! Everything that talks to a Jellyfin server goes through here: native login,
//! the library sync thread, the app-scheme JSON API, and the play path.

pub mod auth;
pub mod client;
pub mod device_profile;
pub mod items;
pub mod model;

pub use client::{ApiError, JellyfinClient};
