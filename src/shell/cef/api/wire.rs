use serde::Serialize;
use ts_rs::TS;

use crate::jellyfin::api::ApiError;

use crate::books::ReaderAppearance;
use crate::collections::{CollectionProfile, ProviderResult};
use crate::companion::{CompanionStatus, SeerrCompanyPage, SeerrNetworkList};
use crate::integrations::letterboxd::ReviewLookup;
use crate::integrations::mdblist::{RatingsBatchResponse, RatingsIntegrationStatus};
use crate::jellyfin::session::{QuickConnectPoll, QuickConnectStart, ServerInfo};
use crate::library::ItemPage;
use crate::library::browse::FilterOptions;
use crate::library::model::{DetailView, ItemSummary};
use crate::maintenance::updater::UpdateRelease;
use crate::playback::PlayerSnapshot;
use crate::preferences::{
    AppearanceSettingsPatch, ApplicationSettingsPatch, HomeSettings, PlaybackSettingsPatch,
    PlayerSettingsPatch,
};

use super::super::ui_events::{
    AboutInfo, ErrorToast, PlaybackCacheRefreshed, ShellEvent, UpdateProgress,
};
use super::auth::{LoginBody, LogoutBody, QuickConnectPollBody, ServerBody};
use super::books::{BookManifest, BookPositionSaved, BookPositionWrite};
use super::catalog::{
    Billboard, Genres, HomeResponse, HomeResumeResponse, HomeSettingsResponse, ItemAbout,
    ItemLetters, ItemSynopsis, OfficialRatings, PersonResolution, Studios, Tags,
};
use super::collections::{
    CollectionProfileDetail, CollectionProfilesIndex, CollectionSettings, CollectionTemplates,
    CollectionTitle, CommittedProfile, ConfirmationBody, FranchiseCollection,
    FranchiseCollectionsIndex, JellyfinCollectionDetail, JellyfinCollections, MovieCollection,
    ProfileDeleted, ProfileDraft, ProfileEdit, PublicListSearchBody, PublicListValidateBody,
    ReorderBody, SettingsPatch, UploadedArtwork,
};
use super::letterboxd::{
    AddProfileBody, EnabledBody, LetterboxdProfileRemoved, LetterboxdProfileSaved,
    LetterboxdProfiles,
};
use super::libraries::{LibrariesResponse, LibraryLatest};
use super::media::{
    ExternalBody, ExternalLinkOpened, FavoriteBody, FavoriteState, IdsBody, MediaInfoResponse,
    NextUpResponse, PlaybackPreferenceBody, PlaybackPreferenceSaved, PlayedBody, PlayedState,
    SimilarItems, TechnicalBatch, TrailerResponse,
};
use super::playback::{
    ItemBody, PlayBody, PlayQueueBody, PlayStarted, PlaybackNeighbors, PlayerCommandAccepted,
    PlayerCommandBody, SyncRequested,
};
use super::ratings::RatingsBatchBody;
use super::recovery::{DataFolderOpened, LibraryRecreated, StartupRecovery};
use super::seerr::{SeerrRequestBody, SeerrRequestCancelled};
use super::settings::{
    BrowsingBody, BrowsingRoutes, BrowsingSaved, SettingsView, ViewingSettingsWrite, ViewingView,
    WhatsNew,
};
use super::shell::{MpvHelpOpened, ShellQueued, ShellRequestBody, ShellRequestQueued};
use super::{ApiResponse, AppStatus, NO_STORE, StartupResponse};

#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a wire type, so the UI has no generated TypeScript for it",
    label = "API bodies must be named types, not `json!` values",
    note = "derive `Serialize` (or `Deserialize`) and `ts_rs::TS` on a named type, add it to `wire_types!` in src/shell/cef/api/wire.rs, then run `just wire-types`"
)]
pub(in crate::shell::cef) trait Wire: TS + sealed::Sealed + 'static {}

mod sealed {
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
    AboutInfo,
    AddProfileBody,
    AppStatus,
    AppearanceSettingsPatch,
    ApplicationSettingsPatch,
    Billboard,
    BookManifest,
    BookPositionSaved,
    BookPositionWrite,
    BrowsingBody,
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
    ConfirmationBody,
    DataFolderOpened,
    DetailView,
    EnabledBody,
    ErrorToast,
    ExternalBody,
    ExternalLinkOpened,
    FavoriteBody,
    FavoriteState,
    FilterOptions,
    FranchiseCollection,
    FranchiseCollectionsIndex,
    Genres,
    HomeResponse,
    HomeResumeResponse,
    HomeSettings,
    HomeSettingsResponse,
    IdsBody,
    ItemAbout,
    ItemBody,
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
    LibraryRecreated,
    LoginBody,
    LogoutBody,
    MediaInfoResponse,
    MovieCollection,
    MpvHelpOpened,
    NextUpResponse,
    OfficialRatings,
    PersonResolution,
    PlayBody,
    PlayQueueBody,
    PlayStarted,
    PlaybackCacheRefreshed,
    PlaybackNeighbors,
    PlaybackPreferenceBody,
    PlaybackPreferenceSaved,
    PlaybackSettingsPatch,
    PlayedBody,
    PlayedState,
    PlayerCommandAccepted,
    PlayerCommandBody,
    PlayerSettingsPatch,
    PlayerSnapshot,
    ProfileDeleted,
    ProfileDraft,
    ProfileEdit,
    ProviderResult,
    PublicListSearchBody,
    PublicListValidateBody,
    QuickConnectPoll,
    QuickConnectPollBody,
    QuickConnectStart,
    RatingsBatchBody,
    RatingsBatchResponse,
    RatingsIntegrationStatus,
    ReaderAppearance,
    ReorderBody,
    ReviewLookup,
    SeerrCompanyPage,
    SeerrNetworkList,
    SeerrRequestBody,
    SeerrRequestCancelled,
    ServerBody,
    ServerInfo,
    SettingsPatch,
    SettingsView,
    ShellEvent,
    ShellQueued,
    ShellRequestBody,
    ShellRequestQueued,
    SimilarItems,
    StartupRecovery,
    StartupResponse,
    Studios,
    SyncRequested,
    Tags,
    TechnicalBatch,
    TrailerResponse,
    UpdateProgress,
    UpdateRelease,
    UploadedArtwork,
    ViewingSettingsWrite,
    ViewingView,
    WhatsNew,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
struct ErrorBody {
    error: String,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    expired: Option<bool>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    account_changed: Option<bool>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    startup_failed: Option<bool>,
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

    pub(super) fn untyped(value: impl Serialize) -> Self {
        Self::json(200, value)
    }

    pub(super) fn error(status: u16, message: impl Into<String>) -> Self {
        Self::json(
            status,
            ErrorBody {
                error: message.into(),
                expired: None,
                account_changed: None,
                startup_failed: None,
            },
        )
    }

    pub(super) fn startup_failed(reason: &str) -> Self {
        Self::json(
            503,
            ErrorBody {
                error: reason.to_string(),
                expired: None,
                account_changed: None,
                startup_failed: Some(true),
            },
        )
    }

    pub(super) fn from_api_error(error: &ApiError) -> Self {
        Self::json(
            error.client_status(),
            ErrorBody {
                error: error.to_string(),
                expired: Some(*error == ApiError::Unauthorized),
                account_changed: (*error == ApiError::AccountChanged).then_some(true),
                startup_failed: None,
            },
        )
    }
}

#[cfg(test)]
mod guards;
#[cfg(test)]
mod typescript;
#[cfg(test)]
mod untyped_count;
