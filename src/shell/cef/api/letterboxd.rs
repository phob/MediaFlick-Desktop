use crate::integrations::letterboxd::ReviewLookup;

use super::media::ExternalLinkOpened;
use super::*;

#[derive(Serialize, ts_rs::TS)]
pub(super) struct LetterboxdProfiles {
    profiles: Vec<ExternalProfile>,
}

#[derive(Serialize, ts_rs::TS)]
pub(super) struct LetterboxdProfileSaved {
    profile: ExternalProfile,
}

#[derive(Serialize, ts_rs::TS)]
pub(super) struct LetterboxdProfileRemoved {
    removed: bool,
}

routes! {
    |services, request|
    GET ["integrations", "letterboxd"] -> LetterboxdProfiles = letterboxd_profiles(services),
    POST ["integrations", "letterboxd"] (body: AddProfileBody) -> LetterboxdProfileSaved =
        letterboxd_add_profile(services, body),
    PATCH ["integrations", "letterboxd", id] (body: EnabledBody) -> LetterboxdProfileSaved =
        letterboxd_set_enabled(services, &percent_decode(id), body),
    DELETE ["integrations", "letterboxd", id] -> LetterboxdProfileRemoved =
        letterboxd_remove_profile(services, &percent_decode(id)),
    POST ["integrations", "letterboxd", id, "refresh"] -> LetterboxdProfileSaved =
        letterboxd_refresh_profile(services, &percent_decode(id)),
    POST ["integrations", "letterboxd", id, "open"] -> ExternalLinkOpened =
        letterboxd_open_profile(services, &percent_decode(id)),
    GET ["letterboxd", "movie", tmdb_id] -> ReviewLookup =
        movie_letterboxd(services, &percent_decode(tmdb_id)),
    GET ["item", id, "letterboxd"] -> ReviewLookup = item_letterboxd(services, id),
}

fn letterboxd_scope(services: &Arc<Services>) -> Result<SessionScope, ApiResponse> {
    services
        .session
        .scope()
        .map_err(|_| ApiResponse::error(401, "sign in to manage connected profiles"))
}

fn valid_profile_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn unix_now() -> i64 {
    crate::app::clock::since_epoch().as_secs() as i64
}

fn letterboxd_profiles(services: &Arc<Services>) -> Result<LetterboxdProfiles, ApiResponse> {
    let scope = letterboxd_scope(services)?;
    let profiles = services.accounts.letterboxd_profiles(scope.account());
    Ok(LetterboxdProfiles { profiles })
}

#[derive(Deserialize, ts_rs::TS)]
#[ts(rename = "LetterboxdProfileBody")]
pub(super) struct AddProfileBody {
    profile: String,
}

#[derive(Deserialize, ts_rs::TS)]
#[ts(rename = "LetterboxdEnabledBody")]
pub(super) struct EnabledBody {
    enabled: bool,
}

fn letterboxd_add_profile(
    services: &Arc<Services>,
    body: Body<'_, AddProfileBody>,
) -> Result<LetterboxdProfileSaved, ApiResponse> {
    let body = body.decode()?;
    let scope = letterboxd_scope(services)?;
    let account = scope.account().clone();
    let profile = match letterboxd_integration::normalize_profile(&body.profile) {
        Ok(profile) => profile,
        Err(error) => return Err(ApiResponse::error(400, error)),
    };
    let existing = services.accounts.letterboxd_profiles(&account);
    if existing.len() >= letterboxd_integration::MAX_CONNECTED_PROFILES
        && !existing
            .iter()
            .any(|saved| saved.profile_key == profile.username)
    {
        return Err(ApiResponse::error(
            409,
            format!(
                "up to {} Letterboxd profiles can be connected",
                letterboxd_integration::MAX_CONNECTED_PROFILES
            ),
        ));
    }
    let verification = letterboxd_integration::verify(&profile);
    let display_name = verification
        .display_name()
        .unwrap_or(&profile.username)
        .to_string();
    let now = unix_now();
    let record = ExternalProfile {
        id: crate::app::ids::random_hex(16),
        provider: "letterboxd".to_string(),
        profile_key: profile.username.clone(),
        display_name,
        canonical_url: profile.canonical_url,
        enabled: true,
        verification_status: verification.as_str().to_string(),
        created_at: now,
        last_checked_at: Some(now),
        jellyfin_server_id: account.server_id().to_string(),
        jellyfin_user_id: account.user_id().to_string(),
    };
    services
        .session
        .commit_if_current(&scope, stale_account_response, |_live| {
            match services.accounts.save_letterboxd_profile(&account, &record) {
                Ok(profile) => Ok(LetterboxdProfileSaved { profile }),
                Err(error) => Err(account_config_failure(&error)),
            }
        })
}

fn letterboxd_set_enabled(
    services: &Arc<Services>,
    id: &str,
    body: Body<'_, EnabledBody>,
) -> Result<LetterboxdProfileSaved, ApiResponse> {
    if !valid_profile_id(id) {
        return Err(ApiResponse::error(404, "profile not found"));
    }
    let enabled = body.decode()?.enabled;
    let scope = letterboxd_scope(services)?;
    let account = scope.account().clone();
    services
        .session
        .commit_if_current(&scope, stale_account_response, |_live| {
            match services
                .accounts
                .set_letterboxd_profile_enabled(&account, id, enabled)
            {
                Ok(Some(profile)) => Ok(LetterboxdProfileSaved { profile }),
                Ok(None) => Err(ApiResponse::error(404, "profile not found")),
                Err(error) => Err(account_config_failure(&error)),
            }
        })
}

fn letterboxd_remove_profile(
    services: &Arc<Services>,
    id: &str,
) -> Result<LetterboxdProfileRemoved, ApiResponse> {
    if !valid_profile_id(id) {
        return Err(ApiResponse::error(404, "profile not found"));
    }
    let scope = letterboxd_scope(services)?;
    let account = scope.account().clone();
    services
        .session
        .commit_if_current(&scope, stale_account_response, |_live| {
            match services.accounts.remove_letterboxd_profile(&account, id) {
                Ok(true) => Ok(LetterboxdProfileRemoved { removed: true }),
                Ok(false) => Err(ApiResponse::error(404, "profile not found")),
                Err(error) => Err(account_config_failure(&error)),
            }
        })
}

fn letterboxd_refresh_profile(
    services: &Arc<Services>,
    id: &str,
) -> Result<LetterboxdProfileSaved, ApiResponse> {
    letterboxd_refresh_profile_with(services, id, letterboxd_integration::verify)
}

fn letterboxd_refresh_profile_with(
    services: &Arc<Services>,
    id: &str,
    verify: impl FnOnce(&letterboxd_integration::Profile) -> letterboxd_integration::Verification,
) -> Result<LetterboxdProfileSaved, ApiResponse> {
    if !valid_profile_id(id) {
        return Err(ApiResponse::error(404, "profile not found"));
    }
    let scope = letterboxd_scope(services)?;
    let account = scope.account().clone();
    let existing = match services.accounts.letterboxd_profile(&account, id) {
        Some(profile) if profile.provider == "letterboxd" => profile,
        _ => return Err(ApiResponse::error(404, "profile not found")),
    };
    let source = match letterboxd_integration::normalize_profile(&existing.profile_key) {
        Ok(profile) => profile,
        Err(_) => {
            return Err(ApiResponse::error(
                409,
                "stored Letterboxd profile is invalid",
            ));
        }
    };
    let verification = verify(&source);
    let update = crate::preferences::LetterboxdProfileVerification {
        canonical_url: &source.canonical_url,
        display_name: verification.display_name(),
        verification_status: verification.as_str(),
        checked_at: unix_now(),
    };
    services
        .session
        .commit_if_current(&scope, stale_account_response, |_live| {
            match services
                .accounts
                .update_letterboxd_profile_verification(&account, id, &update)
            {
                Ok(Some(profile)) => Ok(LetterboxdProfileSaved { profile }),
                Ok(None) => Err(ApiResponse::error(404, "profile not found")),
                Err(error) => Err(account_config_failure(&error)),
            }
        })
}

fn letterboxd_open_profile(
    services: &Arc<Services>,
    id: &str,
) -> Result<ExternalLinkOpened, ApiResponse> {
    if !valid_profile_id(id) {
        return Err(ApiResponse::error(404, "profile not found"));
    }
    let scope = letterboxd_scope(services)?;
    match services.accounts.letterboxd_profile(scope.account(), id) {
        Some(profile) if profile.provider == "letterboxd" => {
            super::super::bridge::open_external_link(&profile.canonical_url);
            Ok(ExternalLinkOpened {
                opened: true,
                url: profile.canonical_url,
            })
        }
        _ => Err(ApiResponse::error(404, "profile not found")),
    }
}

fn item_letterboxd(services: &Arc<Services>, item_id: &str) -> Result<ReviewLookup, ApiResponse> {
    let scope = letterboxd_scope(services)?;
    let item = match services.library.item(item_id) {
        Ok(Some(item)) => item,
        Ok(None) => return Err(ApiResponse::error(404, "item not found")),
        Err(error) => return Err(storage_failure(&error)),
    };
    if item.summary.kind != "Movie" {
        return Ok(ReviewLookup::default());
    }
    let Some(tmdb_id) = item
        .provider_ids
        .tmdb
        .as_deref()
        .filter(|value| !value.is_empty())
    else {
        return Ok(ReviewLookup::default());
    };
    Ok(letterboxd_reviews_for_movie(
        services,
        scope.account(),
        tmdb_id,
    ))
}

fn movie_letterboxd(services: &Arc<Services>, tmdb_id: &str) -> Result<ReviewLookup, ApiResponse> {
    let Some(tmdb_id) = canonical_tmdb_movie_id(tmdb_id) else {
        return Err(ApiResponse::error(400, "that is not a TMDB movie id"));
    };
    let scope = letterboxd_scope(services)?;
    Ok(letterboxd_reviews_for_movie(
        services,
        scope.account(),
        &tmdb_id,
    ))
}

fn canonical_tmdb_movie_id(value: &str) -> Option<String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let id = value.parse::<i64>().ok()?;
    (id > 0).then(|| id.to_string())
}

fn letterboxd_reviews_for_movie(
    services: &Arc<Services>,
    account: &AccountKey,
    tmdb_id: &str,
) -> ReviewLookup {
    let profiles = services.accounts.letterboxd_profiles(account);
    services.letterboxd.reviews_for_item(&profiles, tmdb_id)
}

fn account_config_failure(error: &std::io::Error) -> ApiResponse {
    tracing::error!("could not save account configuration: {error}");
    ApiResponse::error(500, "could not save account configuration")
}

#[cfg(test)]
mod tests {
    use crate::app::services::test_support::TestServices;
    use crate::integrations::letterboxd::{Verification, VerificationStatus};

    use super::*;

    fn stored_profile(id: &str, enabled: bool) -> ExternalProfile {
        ExternalProfile {
            id: id.to_string(),
            provider: "letterboxd".to_string(),
            profile_key: "alice".to_string(),
            display_name: "Alice".to_string(),
            canonical_url: "https://letterboxd.com/alice/".to_string(),
            enabled,
            verification_status: "unverified".to_string(),
            created_at: 100,
            last_checked_at: Some(100),
            jellyfin_server_id: String::new(),
            jellyfin_user_id: String::new(),
        }
    }

    fn signed_in_with_profile(id: &str, enabled: bool) -> (TestServices, AccountKey) {
        let fixture = TestServices::signed_in("http://127.0.0.1:8096");
        let account = fixture.services.session.account_key().expect("account");
        fixture
            .services
            .accounts
            .save_letterboxd_profile(&account, &stored_profile(id, enabled))
            .expect("seed profile");
        (fixture, account)
    }

    #[test]
    fn discovered_letterboxd_lookups_require_a_positive_tmdb_movie_id() {
        assert_eq!(canonical_tmdb_movie_id("603").as_deref(), Some("603"));
        assert_eq!(canonical_tmdb_movie_id("000603").as_deref(), Some("603"));
        for invalid in ["", "0", "+603", "-1", "603.0", "movie-603"] {
            assert_eq!(canonical_tmdb_movie_id(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn in_flight_refresh_does_not_resurrect_a_deleted_profile() {
        let id = "0123456789abcdef";
        let (fixture, account) = signed_in_with_profile(id, true);
        let services = fixture.services.clone();
        let accounts_path = fixture.accounts_path();
        let mut after_delete_bytes = Vec::new();

        let result = letterboxd_refresh_profile_with(&services, id, |_profile| {
            let removed = letterboxd_remove_profile(&services, id).expect("remove while in flight");
            assert!(removed.removed);
            after_delete_bytes = std::fs::read(&accounts_path).expect("accounts after delete");
            after_delete_bytes.extend_from_slice(b"\n\n");
            crate::preferences::store::atomic_write(&accounts_path, &after_delete_bytes)
                .expect("mark accounts file");
            Verification::new(
                VerificationStatus::Verified,
                Some("Alice Verified".to_string()),
            )
        });

        let response = result.err().expect("deleted profile must fail refresh");
        assert_eq!(response.status, 404);
        assert!(services.accounts.letterboxd_profiles(&account).is_empty());
        assert_eq!(
            std::fs::read(&accounts_path).expect("accounts after refresh"),
            after_delete_bytes,
            "a 404 refresh must not rewrite accounts.json"
        );
    }

    #[test]
    fn in_flight_refresh_does_not_overwrite_a_readded_profile_with_a_new_id() {
        let old_id = "0123456789abcdef";
        let new_id = "fedcba9876543210";
        let (fixture, account) = signed_in_with_profile(old_id, true);
        let services = fixture.services.clone();

        let result = letterboxd_refresh_profile_with(&services, old_id, |_profile| {
            letterboxd_remove_profile(&services, old_id).expect("remove old profile");
            let mut replacement = stored_profile(new_id, false);
            replacement.display_name = "Alice Replacement".to_string();
            replacement.created_at = 200;
            services
                .accounts
                .save_letterboxd_profile(&account, &replacement)
                .expect("add replacement profile");
            Verification::new(
                VerificationStatus::Verified,
                Some("Stale Alice".to_string()),
            )
        });

        let response = result.err().expect("old id must fail refresh");
        assert_eq!(response.status, 404);
        let profiles = services.accounts.letterboxd_profiles(&account);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, new_id);
        assert!(!profiles[0].enabled);
        assert_eq!(profiles[0].display_name, "Alice Replacement");
        assert_eq!(profiles[0].verification_status, "unverified");
    }

    #[test]
    fn in_flight_refresh_preserves_disabled_state_and_patches_only_verification_fields() {
        let id = "0123456789abcdef";
        let (fixture, account) = signed_in_with_profile(id, true);
        let services = fixture.services.clone();

        let saved = letterboxd_refresh_profile_with(&services, id, |_profile| {
            let disabled = services
                .accounts
                .set_letterboxd_profile_enabled(&account, id, false)
                .expect("disable in flight")
                .expect("existing profile");
            assert!(!disabled.enabled);
            Verification::new(
                VerificationStatus::Verified,
                Some("Alice Verified".to_string()),
            )
        })
        .expect("refresh succeeds on existing profile");

        assert!(!saved.profile.enabled);
        assert_eq!(saved.profile.display_name, "Alice Verified");
        assert_eq!(saved.profile.verification_status, "verified");
        assert_eq!(saved.profile.canonical_url, "https://letterboxd.com/alice/");
        assert!(saved.profile.last_checked_at.is_some_and(|ts| ts >= 100));
        assert_eq!(saved.profile.created_at, 100);

        let stored = services
            .accounts
            .letterboxd_profile(&account, id)
            .expect("profile remains stored");
        assert_eq!(stored, saved.profile);
    }

    #[test]
    fn in_flight_unverified_refresh_keeps_latest_display_name_and_disabled_state() {
        let id = "0123456789abcdef";
        let (fixture, account) = signed_in_with_profile(id, true);
        let services = fixture.services.clone();

        let saved = letterboxd_refresh_profile_with(&services, id, |_profile| {
            let mut concurrent = services
                .accounts
                .letterboxd_profile(&account, id)
                .expect("profile");
            concurrent.enabled = false;
            concurrent.display_name = "Alice Concurrent".to_string();
            services
                .accounts
                .save_letterboxd_profile(&account, &concurrent)
                .expect("concurrent edit");
            Verification::new(VerificationStatus::Unverified, None)
        })
        .expect("unverified refresh succeeds");

        assert!(!saved.profile.enabled);
        assert_eq!(saved.profile.display_name, "Alice Concurrent");
        assert_eq!(saved.profile.verification_status, "unverified");
    }

    #[test]
    fn in_flight_refresh_after_sign_out_returns_409_without_mutating_profile() {
        let id = "0123456789abcdef";
        let (fixture, account) = signed_in_with_profile(id, true);
        let services = fixture.services.clone();

        let result = letterboxd_refresh_profile_with(&services, id, |_profile| {
            services
                .session
                .clear_local(false)
                .expect("sign out in flight");
            Verification::new(
                VerificationStatus::Verified,
                Some("Alice Verified".to_string()),
            )
        });

        let response = result.err().expect("signed-out refresh must fail");
        assert_eq!(response.status, 409);
        let body: serde_json::Value = serde_json::from_slice(&response.body).expect("json");
        assert_eq!(body["accountChanged"], true);
        let stored = services
            .accounts
            .letterboxd_profile(&account, id)
            .expect("stored profile unchanged");
        assert_eq!(stored.verification_status, "unverified");
        assert_eq!(stored.display_name, "Alice");
    }
}
