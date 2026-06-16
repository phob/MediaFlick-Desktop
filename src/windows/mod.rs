pub mod command_shell;
pub mod icon;

#[cfg(target_os = "windows")]
pub use command_shell::install_hidden_command_processor_shim;
pub use command_shell::run_command_processor_shim;
pub use icon::set_window_icon;
