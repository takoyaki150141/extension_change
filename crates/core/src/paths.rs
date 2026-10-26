use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "ExtensionChange";
pub const HOST_EXE: &str = "ExtensionChange.exe";
pub const PLUGIN_DIR: &str = "plugins";

pub fn app_data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("APPDATA"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(APP_NAME)
}

pub fn config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("EXTENSION_CHANGE_CONFIG") {
        return PathBuf::from(path);
    }
    app_data_dir().join("config.json")
}

pub fn plugin_dir(base: &Path) -> PathBuf {
    plugin_dirs(base)
        .into_iter()
        .next()
        .unwrap_or_else(|| app_data_dir().join(PLUGIN_DIR))
}

pub fn plugin_dirs(base: &Path) -> Vec<PathBuf> {
    if let Some(path) = std::env::var_os("EXTENSION_CHANGE_PLUGINS") {
        return vec![PathBuf::from(path)];
    }

    let mut dirs = Vec::new();
    let user = app_data_dir().join(PLUGIN_DIR);
    if user.is_dir() {
        dirs.push(user);
    }

    #[cfg(debug_assertions)]
    if let Some(dir) = dev_plugin_dir() {
        dirs.push(dir);
    }

    let local = base.join(PLUGIN_DIR);
    if local.is_dir() {
        dirs.push(local);
    }
    dirs
}

pub fn user_plugin_dir() -> PathBuf {
    app_data_dir().join(PLUGIN_DIR)
}

#[cfg(debug_assertions)]
fn dev_plugin_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
    if dir.is_dir() {
        Some(dir.canonicalize().unwrap_or(dir))
    } else {
        None
    }
}

pub fn host_exe(base: &Path) -> PathBuf {
    if let Some(path) = std::env::var_os("EXTENSION_CHANGE_HOST") {
        return PathBuf::from(path);
    }
    let beside = base.join(HOST_EXE);
    if beside.is_file() {
        beside
    } else {
        std::env::current_exe().unwrap_or_else(|_| beside)
    }
}

pub fn ensure_app_dirs() -> std::io::Result<PathBuf> {
    let dir = app_data_dir();
    std::fs::create_dir_all(dir.join(PLUGIN_DIR))?;
    Ok(dir)
}
