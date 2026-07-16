//! Build-time application metadata shared by adapters.

pub const APP_NAME: &str = "MediaFlick Desktop";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_VERSION: &str = env!("MEDIAFLICK_DESKTOP_GIT_VERSION");
pub const CREATED_BY: &str = env!("MEDIAFLICK_DESKTOP_CREATED_BY");
