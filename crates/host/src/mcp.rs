use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result};
use extension_change_core::{config::Config, plugin};
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
        _ => Err((-32602, format!("unknown tool: {name}"))),
    }
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
