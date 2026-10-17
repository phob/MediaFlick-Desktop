use crate::library::libraries::LibraryRegistry;
use crate::preferences::{ViewingSettings, ViewingWrite, startup_view};

use super::catalog::still_signed_in;

use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<Handled> {
    let response = match segments {
        ["settings"] if request.is("GET") => settings_snapshot(services),
        ["settings", "viewing"] if request.is("GET") || request.is("PATCH") => {
            viewing_settings(services, request)
        }
        ["settings", "browsing"] if request.is("GET") || request.is("PATCH") => {
            browsing_settings(services, request)
        }
        ["settings", "client", "player"] if request.is("PATCH") => {
            patch_player_settings(services, request)
        }
        ["settings", "client", "playback"] if request.is("PATCH") => {
            patch_playback_settings(services, request)
        }
        ["settings", "client", "application"] if request.is("PATCH") => {
            patch_application_settings(services, request)
        }
        ["settings", "appearance"] if request.is("PATCH") => {
            patch_appearance_settings(services, request)
        }
        _ => return None,
    };
    Some(response)
}

fn settings_snapshot(services: &Arc<Services>) -> Handled {
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
        "Deletion journal",
        services.pending_deletions.take_recovery_notice(),
    );
    Ok(settings_response(
        &services.preferences.snapshot(),
        recoveries,
    ))
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
    capabilities: Capabilities,
    recoveries: Vec<Recovery>,
    server_url: Option<String>,
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
    /// Canonical IDs from the fixed public MDBList source catalog.
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

fn settings_response(settings: &AppSettings, recoveries: Vec<Recovery>) -> ApiResponse {
    let appearance = &settings.appearance;
    ApiResponse::ok(SettingsView {
        client: ClientView {
            player: PlayerView {
                player_backend: settings.effective_backend(),
                mpv_path: settings.mpv_path.clone(),
                default_fullscreen: settings.default_fullscreen,
                mark_watched_next: settings.mark_watched_next.clone(),
                comfort: settings.comfort.clone(),
                player_configured: crate::players::configured_player_path(settings).is_some(),
            },
            playback: PlaybackView {
                streaming_quality: settings.streaming_quality,
                skip_intro: settings.skip_intro,
                skip_credits: settings.skip_credits,
                skip_recap: settings.skip_recap,
                skip_commercial: settings.skip_commercial,
            },
            application: ApplicationView {
                close_behavior: settings.close_behavior,
                show_scrollbars: settings.show_scrollbars,
                log_level: settings.log_level.clone(),
            },
        },
        appearance: AppearanceView {
            accent: appearance.accent,
            density: appearance.density,
            artwork_intensity: appearance.artwork_intensity,
            backdrop_intensity: appearance.backdrop_intensity,
            reduced_motion: appearance.reduced_motion,
            card_previews: appearance.card_previews,
            show_media_info: appearance.show_media_info,
            rating_sources: appearance.rating_sources.clone(),
        },
        capabilities: Capabilities {
            platform: player_setup::platform(),
            libmpv: crate::players::bundled_libmpv_path().is_some(),
            integrated_libmpv_overlay: crate::shell::cef::libmpv_overlay::is_active(),
            mpv_installer: player_setup::supported(),
        },
        recoveries,
        server_url: settings.jellyfin_url.clone(),
    })
}

fn patch_player_settings(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let patch = request.untyped_body::<PlayerSettingsPatch>()?;
    match services.preferences.patch_player(patch) {
        Ok(change) => Ok(settings_response(&change.settings, Vec::new())),
        Err(error) => Err(ApiResponse::error(400, error.to_string())),
    }
}

fn patch_playback_settings(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let patch = request.untyped_body::<PlaybackSettingsPatch>()?;
    match services.preferences.patch_playback(patch) {
        Ok(change) => Ok(settings_response(&change.settings, Vec::new())),
        Err(error) => Err(ApiResponse::error(400, error.to_string())),
    }
}

fn patch_application_settings(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let patch = request.untyped_body::<ApplicationSettingsPatch>()?;
    match services.preferences.patch_application(patch) {
        Ok(change) => Ok(settings_response(&change.settings, Vec::new())),
        Err(error) => Err(ApiResponse::error(400, error.to_string())),
    }
}

fn patch_appearance_settings(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let patch = request.untyped_body::<AppearanceSettingsPatch>()?;
    let scope = session_scope(services)?;
    services
        .session
        .commit_if_current(&scope, stale_account_response, || {
            Ok(match services.preferences.patch_appearance(patch) {
                Ok(change) => settings_response(&change.settings, Vec::new()),
                Err(error) => ApiResponse::error(400, error.to_string()),
            })
        })
}

fn viewing_settings(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let scope = session_scope(services)?;
    let key = scope.account();
    let registry = services.library.libraries().unwrap_or_else(|error| {
        tracing::warn!(target: "config", "could not read the library list: {error}");
        LibraryRegistry::Unknown
    });
    if request.is("GET") {
        let mut viewing = services.accounts.viewing(key);
        viewing.startup_destination = startup_view(&viewing.startup_destination, &registry);
        still_signed_in(services, &scope)?;
        return Ok(ApiResponse::untyped(json!(viewing)));
    }
    let write = request.untyped_body::<ViewingWrite>()?;
    services
        .session
        .commit_if_current(&scope, stale_account_response, || {
            let saved = match services.accounts.save_viewing_with(key, |stored| {
                write.apply(stored.startup_destination.clone())
            }) {
                Ok(saved) => saved,
                Err(error) => return Ok(ApiResponse::error(400, error.to_string())),
            };
            Ok(ApiResponse::untyped(json!(ViewingSettings {
                startup_destination: startup_view(&saved.startup_destination, &registry),
                ..saved
            })))
        })
}

#[derive(Deserialize)]
struct BrowsingBody {
    page: String,
    route: String,
}

fn browsing_settings(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let scope = session_scope(services)?;
    let key = scope.account();
    if request.is("GET") {
        return Ok(ApiResponse::untyped(json!(services.accounts.browsing(key))));
    }
    let body = request.untyped_body::<BrowsingBody>()?;
    let (page, route) = (body.page.as_str(), body.route.as_str());
    services
        .session
        .commit_if_current(&scope, stale_account_response, || {
            Ok(match services.accounts.save_browsing(key, page, route) {
                Ok(()) => ApiResponse::untyped(json!({"saved": true})),
                Err(error) => ApiResponse::error(400, error.to_string()),
            })
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
        let response = super::settings_response(&settings, recoveries);
        let body: serde_json::Value = serde_json::from_slice(&response.body)?;
        assert_has_keys(
            &body,
            &[
                "appearance",
                "capabilities",
                "client",
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
}
