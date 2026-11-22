pub use crate::app::build_info::{APP_NAME, APP_VERSION, CREATED_BY, GIT_VERSION};

const ABOUT_DIALOG_SCRIPT: &str = include_str!("about_dialog.js");
const ABOUT_INFO_PLACEHOLDER: &str = "__MEDIAFLICK_ABOUT_INFO_JSON__";

pub fn dialog_script(info: &str) -> String {
    ABOUT_DIALOG_SCRIPT.replace(ABOUT_INFO_PLACEHOLDER, info)
}
