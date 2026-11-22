mod cache;
mod in_flight;
mod schema;

use std::fmt;
use std::sync::Arc;

use serde::Serialize;

use crate::companion::CompanionSession;
use crate::jellyfin::session::Session;
use crate::library::Library;

use self::in_flight::InFlightCoordinator;
use self::schema::{RatingSourceDefinition, known_source_definitions};

pub use self::cache::RatingsBatchResponse;

#[derive(Clone, Copy, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
enum RatingOrigin {
    Plugin,
}

#[derive(Clone, Copy, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
enum EffectiveOrigin {
    Plugin,
    None,
}

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
pub enum RatingsError {
    Storage,
    AccountChanged,
    Cancelled,
}

impl fmt::Display for RatingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage => formatter.write_str("ratings storage is unavailable"),
            Self::AccountChanged => crate::jellyfin::api::ApiError::AccountChanged.fmt(formatter),
            Self::Cancelled => crate::jellyfin::api::ApiError::Cancelled.fmt(formatter),
        }
    }
}

impl std::error::Error for RatingsError {}

pub struct RatingsService {
    library: Arc<Library>,
    session: Arc<Session>,
    companion: Arc<CompanionSession>,
    in_flight: InFlightCoordinator,
}

impl RatingsService {
    pub fn new(
        library: &Arc<Library>,
        session: &Arc<Session>,
        companion: &Arc<CompanionSession>,
    ) -> Self {
        Self {
            library: library.clone(),
            session: session.clone(),
            companion: companion.clone(),
            in_flight: InFlightCoordinator::default(),
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
