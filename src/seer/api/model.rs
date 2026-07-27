//! The subset of Seer's API shapes this app consumes.
//!
//! Everything is `#[serde(default)]` for the same reason the Jellyfin models
//! are: Seer renamed itself from Jellyseerr and keeps moving, and a field that
//! appears or disappears between releases must degrade to missing metadata
//! rather than fail the whole call.

use serde::{Deserialize, Serialize};

/// `mediaServerType` on `/settings/public`: 1 Plex, 2 Jellyfin, 3 Emby,
/// 4 not configured.
pub const MEDIA_SERVER_JELLYFIN: i64 = 2;

/// `GET /api/v1/settings/public` — the only endpoint that answers before login.
///
/// An uninitialized instance answers with just `initialized` and
/// `plexClientIdentifier`, so every other field has to tolerate being absent.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PublicSettings {
    pub initialized: bool,
    pub application_title: Option<String>,
    pub media_server_type: Option<i64>,
    pub local_login: bool,
    pub media_server_login: bool,
    /// Seer's "enable new Jellyfin sign-in": when off, a user who has never
    /// been imported gets a 403 from login instead of an account.
    /// The `Plex` in the name is a leftover from Overseerr.
    pub new_plex_login: bool,
    #[serde(rename = "movie4kEnabled")]
    pub movie_4k_enabled: bool,
    #[serde(rename = "series4kEnabled")]
    pub series_4k_enabled: bool,
    pub partial_requests_enabled: bool,
}

/// `GET /api/v1/status`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StatusInfo {
    pub version: String,
    pub commit_tag: String,
}

/// `GET /api/v1/auth/me`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SeerUser {
    pub id: i64,
    pub email: Option<String>,
    pub username: Option<String>,
    pub jellyfin_username: Option<String>,
    pub display_name: Option<String>,
    pub avatar: Option<String>,
    pub permissions: u64,
    /// The Jellyfin account this Seer user is backed by. The link is bound to
    /// it, which is what keeps user A's cookie from serving user B.
    pub jellyfin_user_id: Option<String>,
}

impl SeerUser {
    /// What to show in the UI, in Seer's own order of preference.
    pub fn preferred_name(&self) -> &str {
        [
            self.display_name.as_deref(),
            self.username.as_deref(),
            self.jellyfin_username.as_deref(),
            self.email.as_deref(),
        ]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or("Seer user")
    }
}

/// `GET /api/v1/user/{id}/quota`. `/auth/me` carries the permission mask but
/// not current usage, so this is a second call.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UserQuota {
    pub movie: QuotaStatus,
    pub tv: QuotaStatus,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QuotaStatus {
    /// Length of the rolling window, in days. Absent means unlimited.
    pub days: Option<i64>,
    pub limit: Option<i64>,
    pub used: i64,
    pub remaining: Option<i64>,
    /// Seer's own verdict: the quota is currently exhausted.
    pub restricted: bool,
}

/// Seer's permission mask (`server/lib/permissions.ts`). Only the bits this
/// app acts on are named.
pub mod permission {
    pub const ADMIN: u64 = 2;
    pub const REQUEST: u64 = 32;
    pub const AUTO_APPROVE: u64 = 128;
    pub const AUTO_APPROVE_MOVIE: u64 = 256;
    pub const AUTO_APPROVE_TV: u64 = 512;
    pub const REQUEST_4K: u64 = 1024;
    pub const REQUEST_4K_MOVIE: u64 = 2048;
    pub const REQUEST_4K_TV: u64 = 4096;
    pub const AUTO_APPROVE_4K: u64 = 32768;
    pub const AUTO_APPROVE_4K_MOVIE: u64 = 65536;
    pub const AUTO_APPROVE_4K_TV: u64 = 131072;
    pub const REQUEST_MOVIE: u64 = 262144;
    pub const REQUEST_TV: u64 = 524288;
}

/// What one media kind may do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    pub request: bool,
    pub auto_approve: bool,
}

/// Seer's permissions are per-type — movie, TV, each with a 4K variant and an
/// auto-approve bit — so this cannot collapse to a `canRequest` boolean.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub movie: Capability,
    pub tv: Capability,
    #[serde(rename = "movie4k")]
    pub movie_4k: Capability,
    #[serde(rename = "tv4k")]
    pub tv_4k: Capability,
}

impl Capabilities {
    /// Derives what the user may ask for from their permission mask and the
    /// instance's own 4K switches: a 4K permission on an instance with 4K
    /// turned off is not something to offer.
    pub fn derive(permissions: u64, movie_4k_enabled: bool, series_4k_enabled: bool) -> Self {
        let admin = permissions & permission::ADMIN != 0;
        let has = |bits: u64| admin || permissions & bits != 0;
        Self {
            movie: Capability {
                request: has(permission::REQUEST | permission::REQUEST_MOVIE),
                auto_approve: has(permission::AUTO_APPROVE | permission::AUTO_APPROVE_MOVIE),
            },
            tv: Capability {
                request: has(permission::REQUEST | permission::REQUEST_TV),
                auto_approve: has(permission::AUTO_APPROVE | permission::AUTO_APPROVE_TV),
            },
            movie_4k: Capability {
                request: movie_4k_enabled
                    && has(permission::REQUEST_4K | permission::REQUEST_4K_MOVIE),
                auto_approve: movie_4k_enabled
                    && has(permission::AUTO_APPROVE_4K | permission::AUTO_APPROVE_4K_MOVIE),
            },
            tv_4k: Capability {
                request: series_4k_enabled
                    && has(permission::REQUEST_4K | permission::REQUEST_4K_TV),
                auto_approve: series_4k_enabled
                    && has(permission::AUTO_APPROVE_4K | permission::AUTO_APPROVE_4K_TV),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Capabilities, PublicSettings, SeerUser, permission};

    #[test]
    fn an_uninitialized_instance_still_deserializes() {
        let settings: PublicSettings =
            serde_json::from_str(r#"{"initialized":false,"plexClientIdentifier":"abc"}"#)
                .expect("settings");
        assert!(!settings.initialized);
        assert_eq!(settings.media_server_type, None);
        assert!(!settings.partial_requests_enabled);
    }

    #[test]
    fn the_live_public_settings_shape_is_read_whole() {
        let settings: PublicSettings = serde_json::from_str(
            r#"{"initialized":true,"applicationTitle":"Seerr","mediaServerType":2,
                "localLogin":true,"mediaServerLogin":true,"newPlexLogin":true,
                "movie4kEnabled":false,"series4kEnabled":false,"partialRequestsEnabled":true}"#,
        )
        .expect("settings");
        assert!(settings.initialized);
        assert_eq!(settings.media_server_type, Some(2));
        assert_eq!(settings.application_title.as_deref(), Some("Seerr"));
        assert!(settings.new_plex_login);
        assert!(!settings.movie_4k_enabled);
        assert!(settings.partial_requests_enabled);
    }

    #[test]
    fn a_plain_user_may_request_but_not_approve() {
        let capabilities = Capabilities::derive(permission::REQUEST, false, false);
        assert!(capabilities.movie.request);
        assert!(capabilities.tv.request);
        assert!(!capabilities.movie.auto_approve);
        assert!(!capabilities.tv.auto_approve);
    }

    #[test]
    fn per_type_bits_do_not_leak_across_types() {
        let capabilities = Capabilities::derive(permission::REQUEST_MOVIE, false, false);
        assert!(capabilities.movie.request);
        assert!(!capabilities.tv.request);

        let capabilities = Capabilities::derive(permission::AUTO_APPROVE_TV, false, false);
        assert!(capabilities.tv.auto_approve);
        assert!(!capabilities.movie.auto_approve);
    }

    #[test]
    fn admins_may_do_everything_the_instance_allows() {
        let capabilities = Capabilities::derive(permission::ADMIN, true, true);
        assert!(capabilities.movie.request && capabilities.movie.auto_approve);
        assert!(capabilities.tv.request && capabilities.tv.auto_approve);
        assert!(capabilities.movie_4k.request && capabilities.tv_4k.request);
    }

    #[test]
    fn four_k_permissions_stay_hidden_while_the_instance_has_four_k_off() {
        let mask = permission::REQUEST_4K | permission::AUTO_APPROVE_4K;
        let capabilities = Capabilities::derive(mask, false, false);
        assert!(!capabilities.movie_4k.request);
        assert!(!capabilities.tv_4k.request);
        assert!(!capabilities.movie_4k.auto_approve);

        let capabilities = Capabilities::derive(mask, true, false);
        assert!(capabilities.movie_4k.request);
        assert!(!capabilities.tv_4k.request);
    }

    #[test]
    fn a_user_with_no_permissions_may_do_nothing() {
        let capabilities = Capabilities::derive(0, true, true);
        assert_eq!(capabilities, Capabilities::default());
    }

    #[test]
    fn the_display_name_falls_back_through_seers_own_order() {
        let user: SeerUser = serde_json::from_str(
            r#"{"id":3,"email":"a@b.c","jellyfinUsername":"pho","displayName":"  ","permissions":32}"#,
        )
        .expect("user");
        assert_eq!(user.id, 3);
        assert_eq!(user.preferred_name(), "pho");
        assert_eq!(SeerUser::default().preferred_name(), "Seer user");
    }
}
