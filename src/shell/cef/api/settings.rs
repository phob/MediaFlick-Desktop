use std::collections::BTreeMap;

use crate::library::libraries::LibraryRegistry;
use crate::preferences::{
    NextEpisode, StartupView, SubtitleMode, ViewingSettings, ViewingWrite, startup_view,
};

use super::catalog::still_signed_in;

use super::*;

routes! {
    |services, request|
    GET ["settings"] -> SettingsView = Ok(settings_snapshot(services)),
    GET ["settings", "viewing"] -> ViewingView = viewing(services),
    PATCH ["settings", "viewing"] (body: ViewingSettingsWrite) -> ViewingView =
        save_viewing(services, body),
    GET ["settings", "browsing"] -> BrowsingRoutes = browsing(services),
    PATCH ["settings", "browsing"] (body: BrowsingBody) -> BrowsingSaved =
        save_browsing(services, body),
    PATCH ["settings", "client", "player"] (body: PlayerSettingsPatch) -> SettingsView =
        patch_player_settings(services, body),
    PATCH ["settings", "client", "playback"] (body: PlaybackSettingsPatch) -> SettingsView =
        patch_playback_settings(services, body),
    PATCH ["settings", "client", "application"] (body: ApplicationSettingsPatch) -> SettingsView =
        patch_application_settings(services, body),
    PATCH ["settings", "appearance"] (body: AppearanceSettingsPatch) -> SettingsView =
        patch_appearance_settings(services, body),
    GET ["settings", "whats-new"] -> WhatsNew = Ok(whats_new(services)),
    POST ["settings", "whats-new", "seen"] -> WhatsNew = close_whats_new(services),
}

#[derive(Serialize, ts_rs::TS)]
pub(super) struct WhatsNew {
    pending: bool,
}

fn whats_new(services: &Services) -> WhatsNew {
    WhatsNew {
        pending: services.preferences.whats_new_pending(),
    }
}

fn close_whats_new(services: &Services) -> Result<WhatsNew, ApiResponse> {
    services
        .preferences
        .close_whats_new()
        .map_err(|error| ApiResponse::error(500, error.to_string()))?;
    Ok(whats_new(services))
}

pub(super) fn settings_snapshot(services: &Services) -> SettingsView {
    let mut recoveries = Vec::new();
    push_recovery(
        &mut recoveries,
        "Application settings",
        crate::preferences::store::take_device_recovery_notice(),
    );
    push_recovery(
        &mut recoveries,
        "Account settings",
        services.accounts.take_recovery_notice(),
    );
    push_recovery(
        &mut recoveries,
        "Playback preferences",
        services.playback_preferences.take_recovery_notice(),
    );
    push_recovery(
        &mut recoveries,
        "Reading places and reader appearance",
        services.reader.take_recovery_notice(),
    );
    push_recovery(
        &mut recoveries,
        "Deletion journal",
        services.pending_deletions.take_recovery_notice(),
    );
    settings_view(&services.preferences.snapshot(), recoveries)
}

fn push_recovery(
    recoveries: &mut Vec<Recovery>,
    area: &'static str,
    notice: Option<crate::preferences::RecoveryNotice>,
) {
    if let Some(notice) = notice {
        recoveries.push(Recovery {
            area,
            restored_backup: notice.restored_backup,
        });
    }
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct SettingsView {
    client: ClientView,
    appearance: AppearanceView,
    defaults: SettingsDefaults,
    capabilities: Capabilities,
    recoveries: Vec<Recovery>,
    server_url: Option<String>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct SettingsDefaults {
    client: ClientDefaults,
    appearance: AppearanceView,
}

#[derive(Serialize, ts_rs::TS)]
struct ClientDefaults {
    player: PlayerDefaults,
    playback: PlaybackView,
    application: ApplicationView,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct PlayerDefaults {
    player_backend: crate::preferences::PlayerBackend,
    mpv_path: Option<String>,
    default_fullscreen: crate::preferences::FullscreenBehavior,
    mark_watched_next: Option<String>,
    comfort: crate::preferences::PlayerComfort,
}

#[derive(Serialize, ts_rs::TS)]
struct ClientView {
    player: PlayerView,
    playback: PlaybackView,
    application: ApplicationView,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct PlayerView {
    player_backend: crate::preferences::PlayerBackend,
    mpv_path: Option<String>,
    default_fullscreen: crate::preferences::FullscreenBehavior,
    mark_watched_next: Option<String>,
    comfort: crate::preferences::PlayerComfort,
    player_configured: bool,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct PlaybackView {
    streaming_quality: StreamingQuality,
    skip_intro: crate::preferences::SegmentSkipMode,
    skip_credits: crate::preferences::SegmentSkipMode,
    skip_recap: crate::preferences::SegmentSkipMode,
    skip_commercial: crate::preferences::SegmentSkipMode,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct ApplicationView {
    close_behavior: crate::preferences::CloseBehavior,
    show_scrollbars: bool,
    log_level: String,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct AppearanceView {
    accent: crate::preferences::AppearanceAccent,
    density: crate::preferences::AppearanceDensity,
    artwork_intensity: u8,
    backdrop_intensity: u8,
    reduced_motion: bool,
    card_previews: bool,
    show_media_info: bool,
    rating_sources: Vec<String>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct Capabilities {
    platform: player_setup::Platform,
    libmpv: bool,
    integrated_libmpv_overlay: bool,
    mpv_installer: bool,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct Recovery {
    area: &'static str,
    restored_backup: bool,
}

fn playback_view(settings: &AppSettings) -> PlaybackView {
    PlaybackView {
        streaming_quality: settings.streaming_quality,
        skip_intro: settings.skip_intro,
        skip_credits: settings.skip_credits,
        skip_recap: settings.skip_recap,
        skip_commercial: settings.skip_commercial,
    }
}

fn application_view(settings: &AppSettings) -> ApplicationView {
    ApplicationView {
        close_behavior: settings.close_behavior,
        show_scrollbars: settings.show_scrollbars,
        log_level: settings.log_level.clone(),
    }
}

fn appearance_view(appearance: &crate::preferences::AppearanceSettings) -> AppearanceView {
    AppearanceView {
        accent: appearance.accent,
        density: appearance.density,
        artwork_intensity: appearance.artwork_intensity,
        backdrop_intensity: appearance.backdrop_intensity,
        reduced_motion: appearance.reduced_motion,
        card_previews: appearance.card_previews,
        show_media_info: appearance.show_media_info,
        rating_sources: appearance.rating_sources.clone(),
    }
}

fn settings_defaults(libmpv: bool) -> SettingsDefaults {
    let defaults = AppSettings::default();
    SettingsDefaults {
        client: ClientDefaults {
            player: PlayerDefaults {
                player_backend: if libmpv {
                    crate::preferences::PlayerBackend::Libmpv
                } else {
                    crate::preferences::PlayerBackend::Mpv
                },
                mpv_path: defaults.mpv_path.clone(),
                default_fullscreen: defaults.default_fullscreen,
                mark_watched_next: defaults.mark_watched_next.clone(),
                comfort: defaults.comfort.clone(),
            },
            playback: playback_view(&defaults),
            application: application_view(&defaults),
        },
        appearance: appearance_view(&defaults.appearance),
    }
}

fn settings_view(settings: &AppSettings, recoveries: Vec<Recovery>) -> SettingsView {
    let libmpv = crate::players::bundled_libmpv_path().is_some();
    SettingsView {
        client: ClientView {
            player: PlayerView {
                player_backend: settings.effective_backend(),
                mpv_path: settings.mpv_path.clone(),
                default_fullscreen: settings.default_fullscreen,
                mark_watched_next: settings.mark_watched_next.clone(),
                comfort: settings.comfort.clone(),
                player_configured: crate::players::configured_player_path(settings).is_some(),
            },
            playback: playback_view(settings),
            application: application_view(settings),
        },
        appearance: appearance_view(&settings.appearance),
        defaults: settings_defaults(libmpv),
        capabilities: Capabilities {
            platform: player_setup::platform(),
            libmpv,
            integrated_libmpv_overlay: crate::shell::cef::libmpv_overlay::is_active(),
            mpv_installer: player_setup::supported(),
        },
        recoveries,
        server_url: settings.jellyfin_url.clone(),
    }
}

fn patch_player_settings(
    services: &Arc<Services>,
    body: Body<'_, PlayerSettingsPatch>,
) -> Result<SettingsView, ApiResponse> {
    let patch = body.decode()?;
    match services.preferences.patch_player(patch) {
        Ok(change) => Ok(settings_view(&change.settings, Vec::new())),
        Err(error) => Err(ApiResponse::error(400, error.to_string())),
    }
}

fn patch_playback_settings(
    services: &Arc<Services>,
    body: Body<'_, PlaybackSettingsPatch>,
) -> Result<SettingsView, ApiResponse> {
    let patch = body.decode()?;
    match services.preferences.patch_playback(patch) {
        Ok(change) => Ok(settings_view(&change.settings, Vec::new())),
        Err(error) => Err(ApiResponse::error(400, error.to_string())),
    }
}

fn patch_application_settings(
    services: &Arc<Services>,
    body: Body<'_, ApplicationSettingsPatch>,
) -> Result<SettingsView, ApiResponse> {
    let patch = body.decode()?;
    match services.preferences.patch_application(patch) {
        Ok(change) => Ok(settings_view(&change.settings, Vec::new())),
        Err(error) => Err(ApiResponse::error(400, error.to_string())),
    }
}

fn patch_appearance_settings(
    services: &Arc<Services>,
    body: Body<'_, AppearanceSettingsPatch>,
) -> Result<SettingsView, ApiResponse> {
    let patch = body.decode()?;
    let scope = session_scope(services)?;
    services
        .session
        .commit_if_current(&scope, stale_account_response, |live| {
            match services.preferences.patch_appearance(live.account(), patch) {
                Ok(change) => Ok(settings_view(&change.settings, Vec::new())),
                Err(error) => Err(ApiResponse::error(400, error.to_string())),
            }
        })
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct ViewingView {
    spoiler_protection: bool,
    next_episode: NextEpisode,
    countdown_seconds: u8,
    episode_limit: u8,
    audio_languages: Vec<String>,
    subtitle_languages: Vec<String>,
    prefer_original_audio: bool,
    subtitle_mode: SubtitleMode,
    resume_rewind_seconds: u8,
    text_scale: u16,
    poster_size: u16,
    preview_delay_ms: u16,
    startup_destination: StartupView,
    remember_filters: bool,
    hide_watched: bool,
}

fn viewing_view(settings: ViewingSettings, registry: &LibraryRegistry) -> ViewingView {
    let ViewingSettings {
        spoiler_protection,
        next_episode,
        countdown_seconds,
        episode_limit,
        audio_languages,
        subtitle_languages,
        prefer_original_audio,
        subtitle_mode,
        resume_rewind_seconds,
        text_scale,
        poster_size,
        preview_delay_ms,
        startup_destination,
        remember_filters,
        hide_watched,
    } = settings;
    ViewingView {
        spoiler_protection,
        next_episode,
        countdown_seconds,
        episode_limit,
        audio_languages,
        subtitle_languages,
        prefer_original_audio,
        subtitle_mode,
        resume_rewind_seconds,
        text_scale,
        poster_size,
        preview_delay_ms,
        startup_destination: startup_view(&startup_destination, registry),
        remember_filters,
        hide_watched,
    }
}

fn library_registry(services: &Services) -> LibraryRegistry {
    services.library.libraries().unwrap_or_else(|error| {
        tracing::warn!(target: "config", "could not read the library list: {error}");
        LibraryRegistry::Unknown
    })
}

pub(super) fn viewing(services: &Services) -> Result<ViewingView, ApiResponse> {
    let scope = session_scope(services)?;
    let registry = library_registry(services);
    let viewing = viewing_view(services.accounts.viewing(scope.account()), &registry);
    still_signed_in(services, &scope)?;
    Ok(viewing)
}

#[derive(Deserialize, ts_rs::TS)]
#[serde(transparent)]
#[ts(type = "Omit<ViewingView, \"startupDestination\"> & { startupDestination?: StartupView }")]
pub(super) struct ViewingSettingsWrite(ViewingWrite);

fn save_viewing(
    services: &Arc<Services>,
    body: Body<'_, ViewingSettingsWrite>,
) -> Result<ViewingView, ApiResponse> {
    let scope = session_scope(services)?;
    let key = scope.account();
    let registry = library_registry(services);
    let ViewingSettingsWrite(write) = body.decode()?;
    services
        .session
        .commit_if_current(&scope, stale_account_response, |_live| {
            match services.accounts.save_viewing_with(key, |stored| {
                write.apply(stored.startup_destination.clone())
            }) {
                Ok(saved) => Ok(viewing_view(saved, &registry)),
                Err(error) => Err(ApiResponse::error(400, error.to_string())),
            }
        })
}

#[derive(Serialize, ts_rs::TS)]
pub(super) struct BrowsingRoutes(BTreeMap<String, String>);

#[derive(Serialize, ts_rs::TS)]
pub(super) struct BrowsingSaved {
    saved: bool,
}

#[derive(Deserialize, ts_rs::TS)]
pub(super) struct BrowsingBody {
    page: String,
    route: String,
}

pub(super) fn browsing(services: &Services) -> Result<BrowsingRoutes, ApiResponse> {
    let scope = session_scope(services)?;
    Ok(BrowsingRoutes(services.accounts.browsing(scope.account())))
}

fn save_browsing(
    services: &Arc<Services>,
    body: Body<'_, BrowsingBody>,
) -> Result<BrowsingSaved, ApiResponse> {
    let scope = session_scope(services)?;
    let key = scope.account();
    let body = body.decode()?;
    let (page, route) = (body.page.as_str(), body.route.as_str());
    services
        .session
        .commit_if_current(&scope, stale_account_response, |_live| {
            match services.accounts.save_browsing(key, page, route) {
                Ok(()) => Ok(BrowsingSaved { saved: true }),
                Err(error) => Err(ApiResponse::error(400, error.to_string())),
            }
        })
}

#[cfg(test)]
mod tests {
    fn assert_has_keys(value: &serde_json::Value, keys: &[&str]) {
        for key in keys {
            assert!(value.get(*key).is_some(), "{key}");
        }
    }

    #[test]
    fn settings_send_every_field_the_ui_type_declares() -> Result<(), serde_json::Error> {
        let settings = crate::preferences::AppSettings::default();
        let recoveries = vec![super::Recovery {
            area: "Account settings",
            restored_backup: true,
        }];
        let body = serde_json::to_value(super::settings_view(&settings, recoveries))?;
        assert_has_keys(
            &body,
            &[
                "appearance",
                "capabilities",
                "client",
                "defaults",
                "recoveries",
                "serverUrl",
            ],
        );
        assert_has_keys(&body["client"], &["application", "playback", "player"]);
        assert_has_keys(
            &body["client"]["player"],
            &[
                "comfort",
                "defaultFullscreen",
                "markWatchedNext",
                "mpvPath",
                "playerBackend",
                "playerConfigured",
            ],
        );
        assert_has_keys(
            &body["capabilities"],
            &[
                "integratedLibmpvOverlay",
                "libmpv",
                "mpvInstaller",
                "platform",
            ],
        );
        assert_eq!(
            body["recoveries"],
            serde_json::json!([{ "area": "Account settings", "restoredBackup": true }])
        );
        Ok(())
    }

    #[test]
    fn settings_expose_canonical_app_settings_defaults_regardless_of_saved_overrides()
    -> Result<(), serde_json::Error> {
        let customized = crate::preferences::AppSettings {
            streaming_quality: crate::preferences::StreamingQuality::Mbps20,
            skip_intro: crate::preferences::SegmentSkipMode::Always,
            close_behavior: crate::preferences::CloseBehavior::MinimizeWindow,
            show_scrollbars: true,
            log_level: "warn".to_string(),
            default_fullscreen: crate::preferences::FullscreenBehavior::Windowed,
            mpv_path: Some("/usr/bin/mpv".to_string()),
            mark_watched_next: None,
            appearance: crate::preferences::AppearanceSettings {
                accent: crate::preferences::AppearanceAccent::Violet,
                density: crate::preferences::AppearanceDensity::Compact,
                artwork_intensity: 40,
                ..crate::preferences::AppearanceSettings::default()
            },
            comfort: crate::preferences::PlayerComfort {
                stop_key: "x".to_string(),
                ..crate::preferences::PlayerComfort::default()
            },
            ..crate::preferences::AppSettings::default()
        };

        let body = serde_json::to_value(super::settings_view(&customized, Vec::new()))?;
        let canonical = crate::preferences::AppSettings::default();
        assert_eq!(
            body["defaults"]["client"]["playback"],
            serde_json::json!({
                "streamingQuality": canonical.streaming_quality,
                "skipIntro": canonical.skip_intro,
                "skipCredits": canonical.skip_credits,
                "skipRecap": canonical.skip_recap,
                "skipCommercial": canonical.skip_commercial,
            })
        );
        assert_eq!(
            body["defaults"]["client"]["application"],
            serde_json::json!({
                "closeBehavior": canonical.close_behavior,
                "showScrollbars": canonical.show_scrollbars,
                "logLevel": canonical.log_level,
            })
        );
        assert_eq!(
            body["defaults"]["appearance"],
            serde_json::json!({
                "accent": canonical.appearance.accent,
                "density": canonical.appearance.density,
                "artworkIntensity": canonical.appearance.artwork_intensity,
                "backdropIntensity": canonical.appearance.backdrop_intensity,
                "reducedMotion": canonical.appearance.reduced_motion,
                "cardPreviews": canonical.appearance.card_previews,
                "showMediaInfo": canonical.appearance.show_media_info,
                "ratingSources": canonical.appearance.rating_sources,
            })
        );
        let expected_backend = if body["capabilities"]["libmpv"] == serde_json::json!(true) {
            "libmpv"
        } else {
            "mpv"
        };
        assert_eq!(
            body["defaults"]["client"]["player"],
            serde_json::json!({
                "playerBackend": expected_backend,
                "mpvPath": canonical.mpv_path,
                "defaultFullscreen": canonical.default_fullscreen,
                "markWatchedNext": canonical.mark_watched_next,
                "comfort": canonical.comfort,
            })
        );
        assert_eq!(
            super::settings_defaults(true).client.player.player_backend,
            crate::preferences::PlayerBackend::Libmpv
        );
        assert_eq!(
            super::settings_defaults(false).client.player.player_backend,
            crate::preferences::PlayerBackend::Mpv
        );
        Ok(())
    }

    #[test]
    fn patch_appearance_writes_to_scoped_account_even_if_preferences_active_account_lags() {
        use crate::app::services::test_support::TestServices;
        use crate::preferences::{AccountKey, AppearanceAccent};
        use crate::shell::cef::api::tests::{
            error_of, post, read_request_with_headers, respond, send, server_url,
        };
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let url = server_url(&listener);
        let server = thread::spawn(move || {
            let (mut info, _) = listener.accept().expect("info");
            let _ = read_request_with_headers(&mut info);
            respond(
                &mut info,
                "200 OK",
                r#"{"ServerName":"Server","Version":"12.4.0"}"#,
            );
            let (mut login, _) = listener.accept().expect("login");
            let _ = read_request_with_headers(&mut login);
            respond(
                &mut login,
                "200 OK",
                r#"{"AccessToken":"bob-token","ServerId":"server","User":{"Id":"bob","Name":"Bob","Policy":{"IsAdministrator":false}}}"#,
            );
        });

        let fixture = TestServices::signed_in_as(&url, "alice");
        let alice = AccountKey::new("server", "alice").expect("alice");
        let bob = AccountKey::new("server", "bob").expect("bob");

        let alice_patch = super::ApiRequest {
            method: "PATCH".to_string(),
            ..post("/api/settings/appearance", br#"{"accent":"amber"}"#)
        };
        let alice_response = send(&fixture, &alice_patch);
        assert_eq!(alice_response.status, 200, "{}", error_of(&alice_response));

        fixture
            .services
            .session
            .login(&url, "bob", "secret")
            .expect("sign in Bob without updating preferences active_account");
        server.join().expect("server");

        let bob_patch = super::ApiRequest {
            method: "PATCH".to_string(),
            ..post("/api/settings/appearance", br#"{"accent":"violet"}"#)
        };
        let bob_response = send(&fixture, &bob_patch);
        assert_eq!(bob_response.status, 200, "{}", error_of(&bob_response));

        assert_eq!(
            fixture.services.accounts.appearance(&alice).accent,
            AppearanceAccent::Amber
        );
        assert_eq!(
            fixture.services.accounts.appearance(&bob).accent,
            AppearanceAccent::Violet
        );
    }

    #[test]
    fn patch_appearance_rejects_signed_out_requests() {
        use crate::app::services::test_support::TestServices;
        use crate::preferences::AppearanceSettings;
        use crate::shell::cef::api::tests::{post, send};

        let fixture = TestServices::signed_out();
        let patch = super::ApiRequest {
            method: "PATCH".to_string(),
            ..post("/api/settings/appearance", br#"{"accent":"violet"}"#)
        };
        assert_eq!(send(&fixture, &patch).status, 409);
        assert_eq!(
            fixture.services.preferences.snapshot().appearance,
            AppearanceSettings::default()
        );
    }
}
