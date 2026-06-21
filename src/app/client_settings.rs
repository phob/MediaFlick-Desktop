use serde_json::json;

use crate::app::mpv_setup;
use crate::app::settings::AppSettings;
use crate::mpv::input::MpvInputBindings;

const CLIENT_SETTINGS_DIALOG_SCRIPT: &str = include_str!("client_settings_dialog.js");
const CLIENT_SETTINGS_PLACEHOLDER: &str = "__MEDIAFLICK_CLIENT_SETTINGS_JSON__";

pub fn dialog_script(settings: &AppSettings, bindings: &MpvInputBindings) -> String {
    let data = json!({
        "mpvPath": settings.mpv_path.as_deref().unwrap_or_default(),
        "logLevel": settings.log_level,
        "defaultFullscreen": settings.default_fullscreen.as_str(),
        "closeBehavior": settings.close_behavior.as_str(),
        "showScrollbars": settings.show_scrollbars,
        "skipIntro": settings.skip_intro.as_str(),
        "skipCredits": settings.skip_credits.as_str(),
        "skipRecap": settings.skip_recap.as_str(),
        "skipCommercial": settings.skip_commercial.as_str(),
        "markWatchedNext": bindings.mark_watched_next.as_deref().unwrap_or_default(),
        "mpvPlatform": mpv_setup::platform_id(),
        "mpvCanDownload": mpv_setup::supported(),
        "mpvHelpUrl": mpv_setup::MPV_HELP_URL,
    });

    CLIENT_SETTINGS_DIALOG_SCRIPT
        .replace(CLIENT_SETTINGS_PLACEHOLDER, &data.to_string())
        .replace("{{bridge_token}}", crate::jellyfin::bridge::bridge_token())
}
