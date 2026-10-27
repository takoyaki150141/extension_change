use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result};
use extension_change_core::{config::Config, paths, plugin};
use serde_json::{json, Value};

use crate::engine::{self, JobEvent};

const PROTOCOL_VERSION: &str = "2024-11-05";

pub fn run(plugin_dirs: &[PathBuf], config: &Config) -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = line.context("failed to read MCP request")?;
        if line.trim().is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                write_response(
                    &mut stdout,
                    &error_response(Value::Null, -32700, &format!("parse error: {error}")),
                )?;
                continue;
            }
        };

        if request.get("method").and_then(Value::as_str) == Some("exit") {
            break;
        }

        if let Some(response) = handle_request(&request, plugin_dirs, config) {
            write_response(&mut stdout, &response)?;
        }
    }

    Ok(())
}

fn handle_request(request: &Value, plugin_dirs: &[PathBuf], config: &Config) -> Option<Value> {
    let method = request.get("method")?.as_str()?;
    let id = request.get("id").cloned()?;

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "extension-change",
                "version": env!("CARGO_PKG_VERSION")
            }
        })),
        "ping" => Ok(json!({})),
        "shutdown" => Ok(Value::Null),
        "tools/list" => Ok(tools_list()),
        "tools/call" => call_tool(request, plugin_dirs, config),
        _ => Err((-32601, format!("method not found: {method}"))),
    };

    Some(match result {
        Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
        Err((code, message)) => error_response(id, code, &message),
    })
}

fn tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "list_actions",
                "description": "List the conversion actions available in Extension Change, including their plugin, category, extensions, runtime and enabled state.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }
            },
            {
                "name": "convert",
                "description": "Run a conversion action on local files. Use list_actions first to get the action key. Outputs are placed next to each input file unless output_dir is set.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "description": "Action key in plugin/action form, for example image-convert/jpg"
                        },
                        "files": {
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 1,
                            "description": "Absolute paths of the input files"
                        },
                        "output_dir": {
                            "type": "string",
                            "description": "Optional output directory. If omitted or empty, output goes next to each input file."
                        }
                    },
                    "required": ["action", "files"],
                    "additionalProperties": false
                }
            },
            {
                "name": "create_plugin",
                "description": "Create a new plugin scaffold in the user plugin folder. The plugin is written immediately; reload plugins in Extension Change to use it.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "Plugin id using letters, digits, hyphen or underscore, for example pdf-tools"
                        },
                        "name": {
                            "type": "string",
                            "description": "Display name of the plugin"
                        },
                        "category": {
                            "type": "string",
                            "description": "Right-click menu category, for example PDFに変換"
                        },
                        "extensions": {
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 1,
                            "description": "File extensions without the dot"
                        },
                        "actions": {
                            "type": "array",
                            "minItems": 1,
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" },
                                    "name": { "type": "string" },
                                    "category": { "type": "string" }
                                },
                                "required": ["id", "name"],
                                "additionalProperties": false
                            }
                        },
                        "runtime": {
                            "type": "string",
                            "enum": ["js", "csharp", "rust"],
                            "description": "Plugin runtime. Defaults to js."
                        },
                        "handler": {
                            "type": "string",
                            "enum": ["image", "video", "audio", "font", "schematic", "custom"],
                            "description": "JS host API to call. Defaults to custom."
                        },
                        "overwrite": {
                            "type": "boolean",
                            "description": "Replace an existing plugin with the same id. Defaults to false."
                        }
                    },
                    "required": ["id", "name", "extensions", "actions"],
                    "additionalProperties": false
                }
            }
        ]
    })
}

fn call_tool(request: &Value, plugin_dirs: &[PathBuf], config: &Config) -> Result<Value, (i32, String)> {
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or((-32602, "missing tool name".to_string()))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    match name {
        "list_actions" => Ok(list_actions(plugin_dirs, config)),
        "convert" => Ok(convert(&arguments, plugin_dirs, config)),
        "create_plugin" => Ok(create_plugin(&arguments)),
        _ => Err((-32602, format!("unknown tool: {name}"))),
    }
}

fn create_plugin(arguments: &Value) -> Value {
    let Some(id) = arguments.get("id").and_then(Value::as_str).map(str::trim) else {
        return tool_error("id is required");
    };
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return tool_error("id must use only letters, digits, hyphen or underscore");
    }
    let Some(name) = arguments.get("name").and_then(Value::as_str).map(str::trim) else {
        return tool_error("name is required");
    };
    if name.is_empty() {
        return tool_error("name must not be empty");
    }

    let Some(extension_values) = arguments.get("extensions").and_then(Value::as_array) else {
        return tool_error("extensions must be an array");
    };
    let mut extensions = Vec::new();
    for value in extension_values {
        let Some(extension) = value.as_str() else {
            return tool_error("extensions must contain strings");
        };
        let extension = extension.trim().trim_start_matches('.').to_ascii_lowercase();
        if extension.is_empty() || extension.contains(['/', '\\']) {
            return tool_error("extensions must be simple file extensions");
        }
        if !extensions.contains(&extension) {
            extensions.push(extension);
        }
    }
    if extensions.is_empty() {
        return tool_error("extensions must contain at least one extension");
    }

    let Some(action_values) = arguments.get("actions").and_then(Value::as_array) else {
        return tool_error("actions must be an array");
    };
    let mut actions = Vec::new();
    for value in action_values {
        let Some(action_id) = value.get("id").and_then(Value::as_str).map(str::trim) else {
            return tool_error("each action needs an id");
        };
        if action_id.is_empty()
            || !action_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return tool_error("action ids must use only letters, digits, hyphen or underscore");
        }
        let Some(action_name) = value.get("name").and_then(Value::as_str).map(str::trim) else {
            return tool_error("each action needs a name");
        };
        if action_name.is_empty() {
            return tool_error("action names must not be empty");
        }
        let mut action = json!({ "id": action_id, "name": action_name });
        if let Some(category) = value
            .get("category")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            action["category"] = json!(category);
        }
        actions.push(action);
    }
    if actions.is_empty() {
        return tool_error("actions must contain at least one action");
    }

    let runtime = arguments
        .get("runtime")
        .and_then(Value::as_str)
        .unwrap_or("js");
    if !matches!(runtime, "js" | "csharp" | "rust") {
        return tool_error("runtime must be js, csharp or rust");
    }
    let handler = arguments
        .get("handler")
        .and_then(Value::as_str)
        .unwrap_or("custom");
    let category = arguments
        .get("category")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let overwrite = arguments
        .get("overwrite")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let root = paths::user_plugin_dir().join(id);
    if root.exists() {
        if !overwrite {
            return tool_error(&format!(
                "plugin already exists: {} (pass overwrite=true to replace it)",
                root.display()
            ));
        }
        if let Err(error) = std::fs::remove_dir_all(&root) {
            return tool_error(&format!("failed to remove existing plugin: {error}"));
        }
    }
    if let Err(error) = std::fs::create_dir_all(&root) {
        return tool_error(&format!("failed to create plugin folder: {error}"));
    }

    let entry = match runtime {
        "csharp" => "Plugin.exe",
        "rust" => "plugin.exe",
        _ => "index.js",
    };
    let mut manifest = json!({
        "id": id,
        "name": name,
        "extensions": extensions,
        "actions": actions,
        "runtime": runtime,
        "entry": entry
    });
    if let Some(category) = category {
        manifest["category"] = json!(category);
    }

    let manifest_text = serde_json::to_string_pretty(&manifest).unwrap_or_default();
    if let Err(error) = write_file(&root.join("plugin.json"), &manifest_text) {
        return tool_error(&error);
    }

    let mut files = vec!["plugin.json".to_string()];
    match runtime {
        "js" => {
            if let Err(error) = write_file(&root.join("index.js"), &js_scaffold(handler)) {
                return tool_error(&error);
            }
            files.push("index.js".to_string());
        }
        "csharp" => {
            if let Err(error) = write_file(
                &root.join("Plugin.csproj"),
                include_str!("../../../templates/csharp-plugin/Plugin.csproj"),
            ) {
                return tool_error(&error);
            }
            if let Err(error) = write_file(
                &root.join("Program.cs"),
                include_str!("../../../templates/csharp-plugin/Program.cs"),
            ) {
                return tool_error(&error);
            }
            files.push("Plugin.csproj".to_string());
            files.push("Program.cs".to_string());
        }
        "rust" => {
            if let Err(error) = write_file(
                &root.join("Cargo.toml"),
                include_str!("../../../templates/rust-plugin/Cargo.toml"),
            ) {
                return tool_error(&error);
            }
            if let Err(error) = write_file(
                &root.join("src").join("main.rs"),
                include_str!("../../../templates/rust-plugin/src/main.rs"),
            ) {
                return tool_error(&error);
            }
            files.push("Cargo.toml".to_string());
            files.push("src/main.rs".to_string());
        }
        _ => {}
    }

    let next = match runtime {
        "csharp" => format!(
            "Run dotnet publish -c Release, then copy Plugin.exe into {}.",
            root.display()
        ),
        "rust" => format!(
            "Run cargo build --release, then copy target\\release\\plugin.exe into {}.",
            root.display()
        ),
        _ => "Reload plugins in Extension Change.".to_string(),
    };
    let structured = json!({
        "ok": true,
        "path": root,
        "runtime": runtime,
        "files": files,
        "next": next
    });
    let text = serde_json::to_string_pretty(&structured)
        .unwrap_or_else(|_| "{\"ok\":true}".to_string());
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false
    })
}

fn js_scaffold(handler: &str) -> String {
    let call = match handler {
        "image" => "ctx.convert(file, String(ctx.action), { quality: 92 })",
        "video" => "ctx.video(file, String(ctx.action), {})",
        "audio" => "ctx.audio(file, String(ctx.action), {})",
        "font" => "ctx.font(file, String(ctx.action), {})",
        "schematic" => "ctx.schematic(file, String(ctx.action), {})",
        _ => "null",
    };
    format!(
        "module.exports = function (ctx) {{\n  for (let i = 0; i < ctx.files.length; i++) {{\n    const file = ctx.files[i];\n    const out = {call};\n    if (out) {{\n      ctx.log(file.name + \" -> \" + out);\n    }} else {{\n      ctx.log(file.name + \": TODO\");\n    }}\n    ctx.progress((i + 1) / ctx.files.length);\n  }}\n}};\n"
    )
}

fn write_file(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    std::fs::write(path, contents).map_err(|error| format!("failed to write {}: {error}", path.display()))
}
fn list_actions(plugin_dirs: &[PathBuf], config: &Config) -> Value {
    let actions = plugin::actions_many(plugin_dirs);
    let items: Vec<Value> = actions
        .iter()
        .map(|action| {
            json!({
                "key": action.key(),
                "plugin": action.plugin.manifest.id,
                "plugin_name": action.plugin.manifest.name,
                "action": action.action.id,
                "name": action.action.name,
                "category": action.category(),
                "extensions": action.plugin.manifest.extensions,
                "runtime": action.plugin.manifest.runtime,
                "enabled": config.is_enabled(&action.key()),
            })
        })
        .collect();

    let text = serde_json::to_string_pretty(&json!({ "actions": items }))
        .unwrap_or_else(|_| "{\"actions\":[]}".to_string());
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false
    })
}

fn convert(arguments: &Value, plugin_dirs: &[PathBuf], config: &Config) -> Value {
    let Some(action_key) = arguments.get("action").and_then(Value::as_str) else {
        return tool_error("action is required");
    };
    let Some(file_values) = arguments.get("files").and_then(Value::as_array) else {
        return tool_error("files must be an array");
    };
    let files: Vec<PathBuf> = file_values
        .iter()
        .filter_map(Value::as_str)
        .map(PathBuf::from)
        .collect();
    if files.is_empty() {
        return tool_error("files must contain at least one path");
    }

    let output_dir = arguments
        .get("output_dir")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from);

    let Some(action) = plugin::find_action_many(plugin_dirs, action_key) else {
        return tool_error(&format!("unknown action: {action_key}"));
    };
    if !config.is_enabled(action_key) {
        return tool_error(&format!("action is disabled: {action_key}"));
    }

    let events = Arc::new(Mutex::new(Events::default()));
    let emit: engine::Emit = {
        let events = events.clone();
        Arc::new(move |event| {
            let mut events = events.lock().unwrap();
            match event {
                JobEvent::Log(message) => events.logs.push(message),
                JobEvent::Progress(value) => events.progress = value,
                JobEvent::FileDone { input, output } => events.done.push((
                    input.to_string_lossy().into_owned(),
                    output.to_string_lossy().into_owned(),
                )),
            }
        })
    };

    if let Err(error) = engine::run_job(&action, &files, output_dir, emit) {
        return tool_error(&format!("{error:#}"));
    }

    let events = events.lock().unwrap();
    let outputs: Vec<Value> = events
        .done
        .iter()
        .map(|(input, output)| json!({ "input": input, "output": output }))
        .collect();
    let structured = json!({
        "ok": true,
        "action": action_key,
        "outputs": outputs,
        "logs": events.logs,
        "progress": events.progress,
    });
    let text = serde_json::to_string_pretty(&structured)
        .unwrap_or_else(|_| "{\"ok\":true}".to_string());

    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false
    })
}

#[derive(Default)]
struct Events {
    logs: Vec<String>,
    progress: f32,
    done: Vec<(String, String)>,
}

fn tool_error(message: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true
    })
}

fn error_response(id: Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

fn write_response(stdout: &mut impl Write, response: &Value) -> Result<()> {
    serde_json::to_writer(&mut *stdout, response)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}
