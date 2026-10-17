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

use serde::Serialize;

use crate::companion::CompanionSession;
use crate::library::Library;

use self::schema::{RatingSourceDefinition, known_source_definitions};

pub use self::cache::RatingsBatchResponse;

/// Where a card's ratings came from. The Companion is the only source.
#[derive(Clone, Copy, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
enum RatingOrigin {
    Plugin,
}

/// Where new ratings come from, `none` while the Companion offers none.
#[derive(Clone, Copy, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
enum EffectiveOrigin {
    Plugin,
    None,
}

/// `/api/integrations/ratings`: whether card ratings are available and which
/// sources the app knows.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RatingsIntegrationStatus {
    boundary_version: u8,
    effective_origin: EffectiveOrigin,
    available: bool,
    selection_enabled: bool,
    plugin: RatingsPluginStatus,
    sources: Vec<RatingSourceDefinition>,
    selected_sources: Vec<String>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct RatingsPluginStatus {
    available: bool,
    capability: &'static str,
    boundary_version: u8,
    detail: &'static str,
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

    pub fn status(&self, selected_sources: Vec<String>) -> RatingsIntegrationStatus {
        let _ = self.companion.probe(false);
        let available = self.companion.supports("ratings-v1");
        RatingsIntegrationStatus {
            boundary_version: 1,
            effective_origin: if available {
                EffectiveOrigin::Plugin
            } else {
                EffectiveOrigin::None
            },
            available,
            selection_enabled: available,
            plugin: RatingsPluginStatus {
                available,
                capability: "ratings-v1",
                boundary_version: 1,
                detail: if available {
                    "Server ratings are available."
                } else {
                    "Server ratings are unavailable."
                },
            },
            sources: known_source_definitions(),
            selected_sources,
        }
    }
}
