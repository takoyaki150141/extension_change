use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub disabled: BTreeSet<String>,
    #[serde(default)]
    pub output_dir: Option<String>,
}

impl Config {
    pub fn load(path: &Path) -> Self {
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, raw)?;
        std::fs::rename(&tmp, path)
    }

    pub fn is_enabled(&self, key: &str) -> bool {
        !self.disabled.contains(key)
    }

    pub fn set_enabled(&mut self, key: &str, enabled: bool) {
        if enabled {
            self.disabled.remove(key);
        } else {
            self.disabled.insert(key.to_string());
        }
    }
}
