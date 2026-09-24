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

/// `/api/settings`, and the answer to every settings write. Mirrors
/// `ClientSettings` in `ui/src/lib/api/types.ts`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsView {
    client: ClientView,
    appearance: AppearanceView,
    capabilities: Capabilities,
    recoveries: Vec<Recovery>,
    server_url: Option<String>,
}

#[derive(Serialize)]
struct ClientView {
    player: PlayerView,
    playback: PlaybackView,
    application: ApplicationView,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlayerView {
    player_backend: &'static str,
    mpv_path: Option<String>,
    default_fullscreen: &'static str,
    mark_watched_next: Option<String>,
    comfort: crate::preferences::PlayerComfort,
    /// Computed from the selected backend and path; never written.
    player_configured: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaybackView {
    streaming_quality: &'static str,
    skip_intro: &'static str,
    skip_credits: &'static str,
    skip_recap: &'static str,
    skip_commercial: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplicationView {
    close_behavior: &'static str,
    show_scrollbars: bool,
    log_level: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppearanceView {
    accent: &'static str,
    density: &'static str,
    artwork_intensity: u8,
    backdrop_intensity: u8,
    reduced_motion: bool,
    card_previews: bool,
    show_media_info: bool,
    rating_sources: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Capabilities {
    platform: &'static str,
    libmpv: bool,
    integrated_libmpv_overlay: bool,
    mpv_installer: bool,
}

/// A durable settings file that was restored from its backup at startup.
#[derive(Serialize)]
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
                player_backend: settings.effective_backend().as_str(),
                mpv_path: settings.mpv_path.clone(),
                default_fullscreen: settings.default_fullscreen.as_str(),
                mark_watched_next: settings.mark_watched_next.clone(),
                comfort: settings.comfort.clone(),
                player_configured: crate::players::configured_player_path(settings).is_some(),
            },
            playback: PlaybackView {
                streaming_quality: settings.streaming_quality.as_str(),
                skip_intro: settings.skip_intro.as_str(),
                skip_credits: settings.skip_credits.as_str(),
                skip_recap: settings.skip_recap.as_str(),
                skip_commercial: settings.skip_commercial.as_str(),
            },
            application: ApplicationView {
                close_behavior: settings.close_behavior.as_str(),
                show_scrollbars: settings.show_scrollbars,
                log_level: settings.log_level.clone(),
            },
        },
        appearance: AppearanceView {
            accent: appearance.accent.as_str(),
            density: appearance.density.as_str(),
            artwork_intensity: appearance.artwork_intensity,
            backdrop_intensity: appearance.backdrop_intensity,
            reduced_motion: appearance.reduced_motion,
            card_previews: appearance.card_previews,
            show_media_info: appearance.show_media_info,
            rating_sources: appearance.rating_sources.clone(),
        },
        capabilities: Capabilities {
            platform: player_setup::platform_id(),
            libmpv: crate::players::bundled_libmpv_path().is_some(),
            integrated_libmpv_overlay: crate::shell::cef::libmpv_overlay::is_active(),
            mpv_installer: player_setup::supported(),
        },
        recoveries,
        server_url: settings.jellyfin_url.clone(),
    })
}

fn patch_player_settings(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let patch = request.body::<PlayerSettingsPatch>()?;
    match services.preferences.patch_player(patch) {
        Ok(change) => Ok(settings_response(&change.settings, Vec::new())),
        Err(error) => Err(ApiResponse::error(400, error.to_string())),
    }
}

fn patch_playback_settings(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let patch = request.body::<PlaybackSettingsPatch>()?;
    match services.preferences.patch_playback(patch) {
        Ok(change) => Ok(settings_response(&change.settings, Vec::new())),
        Err(error) => Err(ApiResponse::error(400, error.to_string())),
    }
}

fn patch_application_settings(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let patch = request.body::<ApplicationSettingsPatch>()?;
    match services.preferences.patch_application(patch) {
        Ok(change) => Ok(settings_response(&change.settings, Vec::new())),
        Err(error) => Err(ApiResponse::error(400, error.to_string())),
    }
}

fn patch_appearance_settings(services: &Arc<Services>, request: &ApiRequest) -> Handled {
    let patch = request.body::<AppearanceSettingsPatch>()?;
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
    if request.is("GET") {
        return Ok(ApiResponse::ok(json!(services.accounts.viewing(key))));
    }
    let value = request.body::<crate::preferences::ViewingSettings>()?;
    services
        .session
        .commit_if_current(&scope, stale_account_response, || {
            Ok(match services.accounts.save_viewing(key, &value) {
                Ok(()) => ApiResponse::ok(json!(value)),
                Err(error) => ApiResponse::error(400, error.to_string()),
            })
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
        return Ok(ApiResponse::ok(json!(services.accounts.browsing(key))));
    }
    let body = request.body::<BrowsingBody>()?;
    let (page, route) = (body.page.as_str(), body.route.as_str());
    services
        .session
        .commit_if_current(&scope, stale_account_response, || {
            Ok(match services.accounts.save_browsing(key, page, route) {
                Ok(()) => ApiResponse::ok(json!({"saved": true})),
                Err(error) => ApiResponse::error(400, error.to_string()),
            })
        })
}

#[cfg(test)]
mod tests {
    fn keys(value: &serde_json::Value) -> Vec<&str> {
        let mut keys = value
            .as_object()
            .map(|object| object.keys().map(String::as_str).collect::<Vec<_>>())
            .unwrap_or_default();
        keys.sort_unstable();
        keys
    }

    /// `ClientSettings` in `ui/src/lib/api/types.ts` requires every field.
    #[test]
    fn settings_send_every_field_the_ui_type_declares() -> Result<(), serde_json::Error> {
        let settings = crate::preferences::AppSettings::default();
        let recoveries = vec![super::Recovery {
            area: "Account settings",
            restored_backup: true,
        }];
        let response = super::settings_response(&settings, recoveries);
        let body: serde_json::Value = serde_json::from_slice(&response.body)?;
        assert_eq!(
            keys(&body),
            [
                "appearance",
                "capabilities",
                "client",
                "recoveries",
                "serverUrl"
            ]
        );
        assert_eq!(keys(&body["client"]), ["application", "playback", "player"]);
        assert_eq!(
            keys(&body["client"]["player"]),
            [
                "comfort",
                "defaultFullscreen",
                "markWatchedNext",
                "mpvPath",
                "playerBackend",
                "playerConfigured"
            ]
        );
        assert_eq!(
            keys(&body["capabilities"]),
            [
                "integratedLibmpvOverlay",
                "libmpv",
                "mpvInstaller",
                "platform"
            ]
        );
        assert_eq!(
            body["recoveries"],
            serde_json::json!([{ "area": "Account settings", "restoredBackup": true }])
        );
        Ok(())
    }
}
