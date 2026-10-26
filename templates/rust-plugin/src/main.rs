use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Request {
    #[allow(dead_code)]
    plugin_id: String,
    #[allow(dead_code)]
    plugin_name: String,
    #[allow(dead_code)]
    action_id: String,
    #[allow(dead_code)]
    action_name: String,
    #[allow(dead_code)]
    plugin_dir: String,
    #[allow(dead_code)]
    host_dir: String,
    #[allow(dead_code)]
    ffmpeg: Option<String>,
    output_dir: Option<String>,
    files: Vec<FileItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileItem {
    name: String,
    path: String,
    #[allow(dead_code)]
    ext: String,
    #[allow(dead_code)]
    size: u64,
}

fn emit(value: serde_json::Value) {
    println!("{value}");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let request: Request = serde_json::from_str(&input)?;
    let total = request.files.len().max(1);

    for (index, file) in request.files.iter().enumerate() {
        let input_path = PathBuf::from(&file.path);
        let output_dir = request
            .output_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                input_path
                    .parent()
                    .unwrap_or(Path::new("."))
                    .to_path_buf()
            });
        std::fs::create_dir_all(&output_dir)?;

        let stem = input_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("out");
        let extension = input_path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| format!(".{value}"))
            .unwrap_or_default();
        let output = output_dir.join(format!("{stem}-copy{extension}"));
        std::fs::copy(&input_path, &output)?;

        emit(json!({ "type": "log", "message": format!("copied {}", file.name) }));
        emit(json!({ "type": "done", "input": file.path, "output": output.to_string_lossy() }));
        emit(json!({ "type": "progress", "value": (index + 1) as f64 / total as f64 }));
        std::io::stdout().flush()?;
    }

    Ok(())
}
