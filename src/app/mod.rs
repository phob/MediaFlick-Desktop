//! Process-level application support.
//!
//! Feature behavior belongs to playback, preferences, maintenance, or shell.

pub mod build_info;
pub mod cli;
#[cfg(target_os = "linux")]
pub mod desktop;
pub mod ids;
pub mod instance;
pub mod logger;
pub mod paths;
pub mod services;
pub mod urls;
