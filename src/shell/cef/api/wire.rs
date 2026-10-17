//! The JSON contract with the UI. `ApiResponse::ok` and `ApiRequest::body`
//! take only types listed in [`wire_types!`], and `generated_typescript_is_current`
//! renders that list into one file per Rust module in `ui/src/lib/api/wire/`,
//! re-exported by `ui/src/lib/api/wire.gen.ts`. The JSON
//! encoder and decoder they share are private to this module; the handlers
//! that predate it use `ApiResponse::untyped` and `ApiRequest::untyped_body`,
//! which `untyped_responses_only_shrink` counts.

use serde::Serialize;
use serde::de::DeserializeOwned;
use ts_rs::TS;

use crate::jellyfin::api::ApiError;

use crate::collections::{CollectionProfile, ProviderResult};
use crate::companion::CompanionStatus;
use crate::integrations::letterboxd::ReviewLookup;
use crate::integrations::mdblist::{RatingsBatchResponse, RatingsIntegrationStatus};
use crate::jellyfin::session::{QuickConnectPoll, QuickConnectStart, ServerInfo};
use crate::library::ItemPage;
use crate::library::browse::FilterOptions;
use crate::library::model::{DetailView, ItemSummary};
use crate::playback::PlayerSnapshot;

use super::catalog::{
    Billboard, Genres, HomeResponse, HomeResumeResponse, HomeSettingsResponse, ItemAbout,
    ItemLetters, ItemSynopsis, OfficialRatings, PersonResolution,
};
use super::collections::{
    CollectionProfileDetail, CollectionProfilesIndex, CollectionSettings, CollectionTemplates,
    CollectionTitle, CommittedProfile, FranchiseCollection, FranchiseCollectionsIndex,
    JellyfinCollectionDetail, JellyfinCollections, MovieCollection, ProfileDeleted,
    UploadedArtwork,
};
use super::letterboxd::{LetterboxdProfileRemoved, LetterboxdProfileSaved, LetterboxdProfiles};
use super::libraries::{LibrariesResponse, LibraryLatest};
use super::media::{
    ExternalLinkOpened, FavoriteState, MediaInfoResponse, NextUpResponse, PlaybackPreferenceSaved,
    PlayedState, TechnicalBatch, TrailerResponse,
};
use super::playback::{
    PlayStarted, PlaybackNeighbors, PlayerCommandAccepted, PlayerCommandBody, SyncRequested,
};
use super::seerr::SeerrRequestCancelled;
use super::settings::{BrowsingRoutes, BrowsingSaved, SettingsView, ViewingView};
use super::shell::{MpvHelpOpened, ShellQueued, ShellRequestQueued};
use super::{ApiRequest, ApiResponse, AppStatus, NO_STORE, StartupResponse};

/// A type the UI reads or sends by name. Only [`wire_types!`] implements it,
/// so a handler cannot answer with `json!` or with a type the generated
/// TypeScript does not know.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a wire type, so the UI has no generated TypeScript for it",
    label = "API bodies must be named types, not `json!` values",
    note = "derive `Serialize` (or `Deserialize`) and `ts_rs::TS` on a named type, add it to `wire_types!` in src/shell/cef/api/wire.rs, then run `just wire-types`"
)]
pub(super) trait Wire: TS + sealed::Sealed + 'static {}

mod sealed {
    /// Only [`super::wire_types!`] implements this, so no other module can
    /// make a type [`super::Wire`] without listing it.
    pub trait Sealed {}
}

macro_rules! wire_types {
    ($($ty:ty),* $(,)?) => {
        $(impl sealed::Sealed for $ty {})*
        $(impl Wire for $ty {})*

        #[cfg(test)]
        fn visit_roots(visitor: &mut impl ts_rs::TypeVisitor) {
            $(visitor.visit::<$ty>();)*
        }
    };
}

wire_types! {
    AppStatus,
    Billboard,
    BrowsingRoutes,
    BrowsingSaved,
    CollectionProfile,
    CollectionProfileDetail,
    CollectionProfilesIndex,
    CollectionSettings,
    CollectionTemplates,
    CollectionTitle,
    CommittedProfile,
    CompanionStatus,
    DetailView,
    ExternalLinkOpened,
    FavoriteState,
    FilterOptions,
    FranchiseCollection,
    FranchiseCollectionsIndex,
    Genres,
    HomeResponse,
    HomeResumeResponse,
    HomeSettingsResponse,
    ItemAbout,
    ItemLetters,
    ItemPage,
    ItemSummary,
    ItemSynopsis,
    JellyfinCollectionDetail,
    JellyfinCollections,
    LetterboxdProfileRemoved,
    LetterboxdProfileSaved,
    LetterboxdProfiles,
    LibrariesResponse,
    LibraryLatest,
    MediaInfoResponse,
    MovieCollection,
    MpvHelpOpened,
    NextUpResponse,
    OfficialRatings,
    PersonResolution,
    PlayStarted,
    PlaybackNeighbors,
    PlaybackPreferenceSaved,
    PlayedState,
    PlayerCommandAccepted,
    PlayerCommandBody,
    PlayerSnapshot,
    ProfileDeleted,
    ProviderResult,
    QuickConnectPoll,
    QuickConnectStart,
    RatingsBatchResponse,
    RatingsIntegrationStatus,
    ReviewLookup,
    SeerrRequestCancelled,
    ServerInfo,
    SettingsView,
    ShellQueued,
    ShellRequestQueued,
    StartupResponse,
    SyncRequested,
    TechnicalBatch,
    TrailerResponse,
    UploadedArtwork,
    ViewingView,
}

/// The body of every non-2xx JSON response. It is generated for the UI but
/// is not a [`Wire`] type, so `ApiResponse::ok` cannot send it with a 200.
#[derive(Serialize, TS)]
struct ErrorBody {
    error: String,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    expired: Option<bool>,
}

impl ApiRequest {
    /// Decodes the JSON body into the endpoint's request type. A request sent
    /// without a body decodes like `{}`, so it is accepted exactly when every
    /// field of `T` is optional. A body that is not JSON, or does not match
    /// `T`, is rejected rather than read as defaults: a garbled "mark as
    /// unwatched" must not mark the item watched.
    pub(super) fn body<T: DeserializeOwned + Wire>(&self) -> Result<T, ApiResponse> {
        self.decode()
    }

    /// [`Self::body`] for a request type the generated TypeScript does not
    /// know. `untyped_responses_only_shrink` keeps it from spreading.
    pub(super) fn untyped_body<T: DeserializeOwned>(&self) -> Result<T, ApiResponse> {
        self.decode()
    }

    fn decode<T: DeserializeOwned>(&self) -> Result<T, ApiResponse> {
        let body: &[u8] = if self.body.iter().all(u8::is_ascii_whitespace) {
            b"{}"
        } else {
            &self.body
        };
        serde_json::from_slice(body)
            .map_err(|error| ApiResponse::error(400, format!("invalid request body: {error}")))
    }
}

impl ApiResponse {
    fn json(status: u16, value: impl Serialize) -> Self {
        let body = serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec());
        drop(value);
        Self {
            status,
            content_type: "application/json; charset=utf-8".to_string(),
            body,
            cache_control: NO_STORE,
            headers: Vec::new(),
        }
    }

    pub(super) fn ok<T: Serialize + Wire>(value: T) -> Self {
        Self::json(200, value)
    }

    /// A response the UI reads without a generated type. Answer with a
    /// [`Wire`] type through [`Self::ok`] instead;
    /// `untyped_responses_only_shrink` keeps this from spreading.
    pub(super) fn untyped(value: impl Serialize) -> Self {
        Self::json(200, value)
    }

    pub(super) fn error(status: u16, message: impl Into<String>) -> Self {
        Self::json(
            status,
            ErrorBody {
                error: message.into(),
                expired: None,
            },
        )
    }

    pub(super) fn from_api_error(error: &ApiError) -> Self {
        Self::json(
            error.client_status(),
            ErrorBody {
                error: error.to_string(),
                expired: Some(*error == ApiError::Unauthorized),
            },
        )
    }
}

/// Handlers that predate this module answer through `ApiResponse::untyped`
/// and decode through `ApiRequest::untyped_body`. Moving one to a wire
/// type lowers these; nothing raises them.
#[cfg(test)]
const UNTYPED_RESPONSES: usize = 6; // guard-ratchet
#[cfg(test)]
const UNTYPED_BODIES: usize = 32; // guard-ratchet

#[cfg(test)]
mod guards;
#[cfg(test)]
mod typescript;
