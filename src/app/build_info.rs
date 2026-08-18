//! Build-time application metadata shared by adapters.

pub const APP_NAME: &str = "MediaFlick Desktop";
pub const APP_DESKTOP_ID: &str = "io.github.phob.MediaFlickDesktop";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_VERSION: &str = env!("MEDIAFLICK_DESKTOP_GIT_VERSION");
pub const CREATED_BY: &str = env!("MEDIAFLICK_DESKTOP_CREATED_BY");

#[cfg(test)]
mod tests {
    use super::*;

    const LINUX_DESKTOP_ENTRY: &str =
        include_str!("../../resources/linux/io.github.phob.MediaFlickDesktop.desktop");

    #[test]
    fn linux_desktop_entry_matches_app_identity() {
        assert_eq!(desktop_entry_value("Icon"), Some(APP_DESKTOP_ID));
        assert_eq!(desktop_entry_value("StartupWMClass"), Some(APP_DESKTOP_ID));
    }

    fn desktop_entry_value(key: &str) -> Option<&'static str> {
        LINUX_DESKTOP_ENTRY.lines().find_map(|line| {
            line.strip_prefix(key)
                .and_then(|value| value.strip_prefix('='))
        })
    }
}
