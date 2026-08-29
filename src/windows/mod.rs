pub mod command_shell;
pub mod focus;
pub mod icon;
pub mod job;

#[cfg(target_os = "windows")]
pub use command_shell::install_hidden_command_processor_shim;
pub use command_shell::run_command_processor_shim;
#[cfg(target_os = "windows")]
pub use focus::{ProcessWindowActivation, activate_process_window};
#[cfg(target_os = "windows")]
pub use icon::set_native_window_icon;
pub use icon::set_window_icon;
pub use job::confine_to_app_lifetime;
