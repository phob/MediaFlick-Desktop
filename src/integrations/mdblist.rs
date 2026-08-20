//! Quota-aware MDBList ratings for desktop cards.
//!
//! Catalog queries never call this module. Mounted cards request IDs after they
//! render; those IDs are deduplicated, resolved to stable TMDB/IMDb identities,
//! served stale-while-revalidate from SQLite, and refreshed in bounded batches.
//! A versioned Companion boundary is the fallback only when no valid local key
//! exists, and never returns the plugin administrator's credential. Every
//! upstream/cache value is rebuilt through a fixed public rating schema before
//! persistence or desktop serialization.

mod cache;
mod credentials;
mod schema;
mod transport;

use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, Mutex};

use crate::companion::CompanionSession;
use crate::integrations::credentials::{CredentialStore, OsCredentialStore};
use crate::library::Library;

use self::transport::{HttpTransport, MdbTransport};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    Local,
    Plugin,
}

impl Origin {
    fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local_mdblist",
            Self::Plugin => "plugin",
        }
    }
}

#[derive(Debug)]
pub struct RatingsError(String);

impl RatingsError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for RatingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RatingsError {}

pub struct RatingsService {
    library: Arc<Library>,
    companion: Arc<CompanionSession>,
    credentials: Arc<dyn CredentialStore>,
    transport: Arc<dyn MdbTransport>,
    in_flight: Mutex<HashSet<String>>,
}

impl RatingsService {
    pub fn new(library: Arc<Library>, companion: Arc<CompanionSession>) -> Self {
        Self {
            library,
            companion,
            credentials: Arc::new(OsCredentialStore),
            transport: Arc::new(HttpTransport::new()),
            in_flight: Mutex::new(HashSet::new()),
        }
    }
}
