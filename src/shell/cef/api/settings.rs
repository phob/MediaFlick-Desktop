use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["settings"] if request.is("GET") => settings_snapshot(services),
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

fn settings_snapshot(services: &Arc<Services>) -> ApiResponse {
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
    settings_response(&services.preferences.snapshot(), &recoveries)
}

fn push_recovery(
    recoveries: &mut Vec<Value>,
    area: &str,
    notice: Option<crate::preferences::RecoveryNotice>,
) {
    if let Some(notice) = notice {
        recoveries.push(json!({
            "area": area,
            "restoredBackup": notice.restored_backup,
        }));
    }
}

fn settings_response(settings: &AppSettings, recoveries: &[Value]) -> ApiResponse {
    let bindings = MpvInputBindings::load();
    ApiResponse::ok(json!({
        "client": {
            "player": {
                "playerBackend": settings.effective_backend().as_str(),
                "mpvPath": settings.mpv_path,
                "mpchcPath": settings.mpchc_path,
                "defaultFullscreen": settings.default_fullscreen.as_str(),
                "markWatchedNext": bindings.mark_watched_next,
                "playerConfigured": crate::players::configured_player_path(settings).is_some(),
            },
            "playback": {
                "streamingQuality": settings.streaming_quality.as_str(),
                "skipIntro": settings.skip_intro.as_str(),
                "skipCredits": settings.skip_credits.as_str(),
                "skipRecap": settings.skip_recap.as_str(),
                "skipCommercial": settings.skip_commercial.as_str(),
            },
            "application": {
                "closeBehavior": settings.close_behavior.as_str(),
                "showScrollbars": settings.show_scrollbars,
                "logLevel": settings.log_level,
            },
        },
        "appearance": {
            "theme": settings.appearance.theme.as_str(),
            "accent": settings.appearance.accent.as_str(),
            "density": settings.appearance.density.as_str(),
            "artworkIntensity": settings.appearance.artwork_intensity,
            "backdropIntensity": settings.appearance.backdrop_intensity,
            "reducedMotion": settings.appearance.reduced_motion,
            "cardPreviews": settings.appearance.card_previews,
            "showMediaInfo": settings.appearance.show_media_info,
            "ratingSources": settings.appearance.rating_sources,
        },
        "capabilities": {
            "platform": player_setup::platform_id(),
            "libmpv": crate::players::bundled_libmpv_path().is_some(),
            "integratedLibmpvOverlay": crate::shell::cef::prototype_osr::is_active(),
            "mpchc": cfg!(target_os = "windows"),
            "mpvInstaller": player_setup::supported(),
        },
        "recoveries": recoveries,
        // Retained for small existing consumers while they move to the
        // sectioned shape above.
        "streamingQuality": settings.streaming_quality.as_str(),
        "playerBackend": settings.effective_backend().as_str(),
        "playerConfigured": crate::players::configured_player_path(settings).is_some(),
        "serverUrl": settings.jellyfin_url,
    }))
}

fn patch_player_settings(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let patch = match serde_json::from_value::<PlayerSettingsPatch>(request.json()) {
        Ok(patch) => patch,
        Err(error) => return ApiResponse::error(400, format!("invalid player settings: {error}")),
    };
    match services.preferences.patch_player(patch) {
        Ok(change) => settings_response(&change.settings, &[]),
        Err(error) => ApiResponse::error(400, error.to_string()),
    }
}

fn patch_playback_settings(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let patch = match serde_json::from_value::<PlaybackSettingsPatch>(request.json()) {
        Ok(patch) => patch,
        Err(error) => {
            return ApiResponse::error(400, format!("invalid playback settings: {error}"));
        }
    };
    match services.preferences.patch_playback(patch) {
        Ok(change) => settings_response(&change.settings, &[]),
        Err(error) => ApiResponse::error(400, error.to_string()),
    }
}

fn patch_application_settings(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let patch = match serde_json::from_value::<ApplicationSettingsPatch>(request.json()) {
        Ok(patch) => patch,
        Err(error) => {
            return ApiResponse::error(400, format!("invalid application settings: {error}"));
        }
    };
    match services.preferences.patch_application(patch) {
        Ok(change) => settings_response(&change.settings, &[]),
        Err(error) => ApiResponse::error(400, error.to_string()),
    }
}

fn patch_appearance_settings(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let patch = match serde_json::from_value::<AppearanceSettingsPatch>(request.json()) {
        Ok(patch) => patch,
        Err(error) => {
            return ApiResponse::error(400, format!("invalid appearance settings: {error}"));
        }
    };
    let scope = match services.session.scope() {
        Ok(scope) => scope,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    services
        .session
        .commit_if_current(&scope, stale_account_response, || {
            Ok(match services.preferences.patch_appearance(patch) {
                Ok(change) => settings_response(&change.settings, &[]),
                Err(error) => ApiResponse::error(400, error.to_string()),
            })
        })
        .unwrap_or_else(|response| response)
}
