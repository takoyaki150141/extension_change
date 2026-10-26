use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub actions: Vec<Action>,
    pub entry: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Plugin {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

#[derive(Debug, Clone)]
pub struct ActionEntry {
    pub plugin: Plugin,
    pub action: Action,
}

impl ActionEntry {
    pub fn key(&self) -> String {
        format!("{}/{}", self.plugin.manifest.id, self.action.id)
    }

    pub fn label(&self) -> &str {
        &self.action.name
    }

    pub fn matches_extension(&self, ext: &str) -> bool {
        let ext = ext.trim_start_matches('.').to_ascii_lowercase();
        self.plugin
            .manifest
            .extensions
            .iter()
            .any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(&ext))
    }
}

impl Plugin {
    pub fn entry_path(&self) -> PathBuf {
        self.dir.join(&self.manifest.entry)
    }

    pub fn actions(&self) -> Vec<Action> {
        if self.manifest.actions.is_empty() {
            vec![Action {
                id: "default".to_string(),
                name: self.manifest.name.clone(),
                description: None,
            }]
        } else {
            self.manifest.actions.clone()
        }
    }
}

pub fn load_plugin(dir: &Path) -> Result<Plugin, String> {
    let manifest_path = dir.join("plugin.json");
    let raw = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let manifest: Manifest =
        serde_json::from_str(&raw).map_err(|e| format!("{}: {e}", manifest_path.display()))?;

    let id = manifest.id.trim();
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!(
            "{}: id must be non-empty and use only a-z, 0-9, '-' or '_'",
            manifest_path.display()
        ));
    }

    let entry = Path::new(&manifest.entry);
    if entry.is_absolute()
        || entry
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!(
            "{}: entry must be a relative path inside the plugin directory",
            manifest_path.display()
        ));
    }
    if !dir.join(entry).is_file() {
        return Err(format!(
            "{}: entry file does not exist",
            dir.join(entry).display()
        ));
    }

    let mut manifest = manifest;
    manifest.extensions = manifest
        .extensions
        .into_iter()
        .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .collect();
    manifest.actions = manifest
        .actions
        .into_iter()
        .filter(|a| !a.id.trim().is_empty())
        .collect();

    Ok(Plugin {
        dir: dir.to_path_buf(),
        manifest,
    })
}

pub fn discover(plugin_root: &Path) -> (Vec<Plugin>, Vec<String>) {
    let mut plugins = Vec::new();
    let mut errors = Vec::new();

    let entries = match std::fs::read_dir(plugin_root) {
        Ok(entries) => entries,
        Err(e) => {
            errors.push(format!("{}: {e}", plugin_root.display()));
            return (plugins, errors);
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if !path.join("plugin.json").is_file() {
            continue;
        }
        match load_plugin(&path) {
            Ok(plugin) => plugins.push(plugin),
            Err(e) => errors.push(e),
        }
    }

    plugins.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
    (plugins, errors)
}

pub fn discover_many(dirs: &[PathBuf]) -> (Vec<Plugin>, Vec<String>) {
    let mut plugins: BTreeMap<String, Plugin> = BTreeMap::new();
    let mut errors = Vec::new();
    for dir in dirs {
        let (found, errs) = discover(dir);
        for plugin in found {
            plugins.insert(plugin.manifest.id.clone(), plugin);
        }
        errors.extend(errs);
    }
    let mut plugins: Vec<Plugin> = plugins.into_values().collect();
    plugins.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
    (plugins, errors)
}

pub fn actions_many(dirs: &[PathBuf]) -> Vec<ActionEntry> {
    let (plugins, _) = discover_many(dirs);
    let mut out = Vec::new();
    for plugin in plugins {
        for action in plugin.actions() {
            out.push(ActionEntry {
                plugin: plugin.clone(),
                action,
            });
        }
    }
    out
}

pub fn find_action_many(dirs: &[PathBuf], key: &str) -> Option<ActionEntry> {
    actions_many(dirs).into_iter().find(|a| a.key() == key)
}

pub fn actions(plugin_root: &Path) -> Vec<ActionEntry> {
    let (plugins, _) = discover(plugin_root);
    let mut out = Vec::new();
    for plugin in plugins {
        for action in plugin.actions() {
            out.push(ActionEntry {
                plugin: plugin.clone(),
                action,
            });
        }
    }
    out
}

pub fn find_action(plugin_root: &Path, key: &str) -> Option<ActionEntry> {
    actions(plugin_root).into_iter().find(|a| a.key() == key)
}
