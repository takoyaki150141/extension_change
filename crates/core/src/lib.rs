pub mod config;
pub mod paths;
pub mod plugin;

pub use config::Config;
pub use paths::{app_data_dir, config_path, ensure_app_dirs, host_exe, plugin_dir};
pub use plugin::{Action, ActionEntry, Manifest, Plugin};
