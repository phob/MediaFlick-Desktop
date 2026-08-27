//! Companion-mediated MDBList ratings for desktop cards.
//!
//! Catalog queries never call this module. Mounted cards request IDs after they
//! render; those IDs are deduplicated, resolved to stable TMDB/IMDb identities,
//! served stale-while-revalidate from SQLite, and refreshed in bounded batches.
//! The versioned Companion boundary owns all provider access and never returns
//! the plugin administrator's credential. Every response and cache value is
//! rebuilt through a fixed public rating schema before persistence or desktop
//! serialization.

mod cache;
mod schema;

use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::companion::CompanionSession;
use crate::library::Library;

use self::schema::known_source_definitions;

const CACHE_ORIGIN: &str = "plugin";

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
    in_flight: Mutex<HashSet<String>>,
}

impl RatingsService {
    pub fn new(library: Arc<Library>, companion: Arc<CompanionSession>) -> Self {
        Self {
            library,
            companion,
            in_flight: Mutex::new(HashSet::new()),
        }
    }

    pub fn status(&self, selected_sources: &[String]) -> Value {
        let _ = self.companion.probe(false);
        let available = self.companion.supports("ratings-v1");
        json!({
            "boundaryVersion": 1,
            "effectiveOrigin": if available { CACHE_ORIGIN } else { "none" },
            "available": available,
            "selectionEnabled": available,
            "plugin": {
                "available": available,
                "capability": "ratings-v1",
                "boundaryVersion": 1,
                "detail": if available {
                    "Server ratings are available."
                } else {
                    "Server ratings are unavailable."
                },
            },
            "sources": known_source_definitions(),
            "selectedSources": selected_sources,
        })
    }
}
