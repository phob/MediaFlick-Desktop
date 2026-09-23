use super::*;

pub(super) fn route(
    services: &Arc<Services>,
    segments: &[&str],
    request: &ApiRequest,
) -> Option<ApiResponse> {
    let response = match segments {
        ["settings"] if request.is("GET") => settings_snapshot(services),
        ["settings", "viewing"] => viewing_settings(services, request),
        ["settings", "browsing"] => browsing_settings(services, request),
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
                "defaultFullscreen": settings.default_fullscreen.as_str(),
                "markWatchedNext": bindings.mark_watched_next,
                "comfort": settings.comfort,
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
            "mpvInstaller": player_setup::supported(),
        },
        "recoveries": recoveries,
        "serverUrl": settings.jellyfin_url,
    }))
}

fn patch_player_settings(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let patch = match request.body::<PlayerSettingsPatch>() {
        Ok(patch) => patch,
        Err(response) => return response,
    };
    match services.preferences.patch_player(patch) {
        Ok(change) => settings_response(&change.settings, &[]),
        Err(error) => ApiResponse::error(400, error.to_string()),
    }
}

fn patch_playback_settings(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let patch = match request.body::<PlaybackSettingsPatch>() {
        Ok(patch) => patch,
        Err(response) => return response,
    };
    match services.preferences.patch_playback(patch) {
        Ok(change) => settings_response(&change.settings, &[]),
        Err(error) => ApiResponse::error(400, error.to_string()),
    }
}

fn patch_application_settings(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let patch = match request.body::<ApplicationSettingsPatch>() {
        Ok(patch) => patch,
        Err(response) => return response,
    };
    match services.preferences.patch_application(patch) {
        Ok(change) => settings_response(&change.settings, &[]),
        Err(error) => ApiResponse::error(400, error.to_string()),
    }
}

fn patch_appearance_settings(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let patch = match request.body::<AppearanceSettingsPatch>() {
        Ok(patch) => patch,
        Err(response) => return response,
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

fn viewing_settings(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let scope = match services.session.scope() {
        Ok(scope) => scope,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    let key = scope.account();
    if request.is("GET") {
        return ApiResponse::ok(json!(services.accounts.viewing(key)));
    }
    if !request.is("PATCH") {
        return ApiResponse::error(405, "method not allowed");
    }
    let value = match request.body::<crate::preferences::ViewingSettings>() {
        Ok(value) => value,
        Err(response) => return response,
    };
    services
        .session
        .commit_if_current(&scope, stale_account_response, || {
            Ok(match services.accounts.save_viewing(key, &value) {
                Ok(()) => ApiResponse::ok(json!(value)),
                Err(error) => ApiResponse::error(400, error.to_string()),
            })
        })
        .unwrap_or_else(|response| response)
}

#[derive(Deserialize)]
struct BrowsingBody {
    page: String,
    route: String,
}

fn browsing_settings(services: &Arc<Services>, request: &ApiRequest) -> ApiResponse {
    let scope = match services.session.scope() {
        Ok(scope) => scope,
        Err(error) => return ApiResponse::from_api_error(&error),
    };
    let key = scope.account();
    if request.is("GET") {
        return ApiResponse::ok(json!(services.accounts.browsing(key)));
    }
    if !request.is("PATCH") {
        return ApiResponse::error(405, "method not allowed");
    }
    let body = match request.body::<BrowsingBody>() {
        Ok(body) => body,
        Err(response) => return response,
    };
    let (page, route) = (body.page.as_str(), body.route.as_str());
    services
        .session
        .commit_if_current(&scope, stale_account_response, || {
            Ok(match services.accounts.save_browsing(key, page, route) {
                Ok(()) => ApiResponse::ok(json!({"saved": true})),
                Err(error) => ApiResponse::error(400, error.to_string()),
            })
        })
        .unwrap_or_else(|response| response)
}

#[cfg(test)]
mod tests {
    #[test]
    fn player_snapshot_owns_comfort_settings() -> Result<(), serde_json::Error> {
        let settings = crate::preferences::AppSettings::default();
        let response = super::settings_response(&settings, &[]);
        let body: serde_json::Value = serde_json::from_slice(&response.body)?;
        assert_eq!(
            body["client"]["player"]["comfort"],
            serde_json::to_value(settings.comfort)?
        );
        assert!(body["client"]["playback"].get("comfort").is_none());
        Ok(())
    }
}
