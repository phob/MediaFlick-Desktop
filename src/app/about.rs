use serde_json::json;

pub const APP_NAME: &str = "MediaFlick Desktop";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_VERSION: &str = env!("MEDIAFLICK_DESKTOP_GIT_VERSION");
pub const CREATED_BY: &str = env!("MEDIAFLICK_DESKTOP_CREATED_BY");

const ABOUT_DIALOG_SCRIPT: &str = include_str!("about_dialog.js");
const ABOUT_INFO_PLACEHOLDER: &str = "__MEDIAFLICK_ABOUT_INFO_JSON__";

pub fn info_json() -> serde_json::Value {
    json!({
        "appName": APP_NAME,
        "version": APP_VERSION,
        "gitVersion": GIT_VERSION,
        "createdBy": CREATED_BY,
    })
}

pub fn dialog_script() -> String {
    ABOUT_DIALOG_SCRIPT.replace(ABOUT_INFO_PLACEHOLDER, &info_json().to_string())
}
