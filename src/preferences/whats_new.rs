//! The one-time "What's new" that a device updated from MediaFlick 0.1.6
//! shows after sign-in. Whether it was closed is `whats_new_seen` in
//! `settings.json`.

use std::path::Path;

/// Whether the config directory `dir` holds MediaFlick 0.1.6's settings.
/// 0.1.6 kept them in `config.json` and saved that file whenever its window
/// closed (`save_webui_window_settings` in v0.1.6:src/shell/cef/mod.rs), so
/// every device that ran it has one. Later builds keep their settings in
/// `settings.json` and never write `config.json`.
pub fn carries_0_1_6_data(dir: &Path) -> bool {
    dir.join("config.json").is_file()
}
