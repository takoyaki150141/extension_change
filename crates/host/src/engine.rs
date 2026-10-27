use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context as _};
use extension_change_core::plugin::ActionEntry;
use image::ImageFormat;
use rquickjs::{Context, Exception, Function, Runtime};
use wait_timeout::ChildExt;

pub const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
pub const TIME_LIMIT: Duration = Duration::from_secs(1800);

#[derive(Debug, Clone)]
pub enum JobEvent {
    Log(String),
    Progress(f32),
    FileDone { input: PathBuf, output: PathBuf },
}

pub type Emit = Arc<dyn Fn(JobEvent) + Send + Sync>;

pub fn run_job(
    action: &ActionEntry,
    files: &[PathBuf],
    output_dir: Option<PathBuf>,
    emit: Emit,
) -> anyhow::Result<()> {
    if action.plugin.is_native() {
        return run_native(action, files, output_dir, emit);
    }

    let entry = action.plugin.entry_path();
    let source = std::fs::read_to_string(&entry)
        .with_context(|| format!("plugin entry not found: {}", entry.display()))?;

    let allowed: HashSet<PathBuf> = files
        .iter()
        .filter_map(|p| std::fs::canonicalize(p).ok())
        .collect();
    if allowed.is_empty() {
        bail!("no readable input files");
    }

    let convert = Arc::new(ConvertContext {
        allowed,
        output_dir: output_dir.clone(),
        reserved: std::sync::Mutex::new(HashSet::new()),
        emit: emit.clone(),
    });

    let files_json = serde_json::to_string(&files_payload(files))?;

    let (tx, rx) = std::sync::mpsc::channel::<anyhow::Result<()>>();
    let action_id = action.action.id.clone();

    std::thread::Builder::new()
        .name("plugin-js".into())
        .spawn(move || {
            let result = run_js(&source, &files_json, &action_id, convert, emit);
            let _ = tx.send(result);
        })
        .context("failed to start plugin thread")?;

    match rx.recv_timeout(TIME_LIMIT + Duration::from_secs(5)) {
        Ok(result) => result,
        Err(_) => Err(anyhow!("plugin timed out")),
    }
}

fn run_native(
    action: &ActionEntry,
    files: &[PathBuf],
    output_dir: Option<PathBuf>,
    emit: Emit,
) -> anyhow::Result<()> {
    let entry = action.plugin.entry_path();
    if !entry.is_file() {
        bail!("native plugin entry not found: {}", entry.display());
    }
    if files.is_empty() {
        bail!("no input files");
    }

    let host_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    let request = serde_json::json!({
        "protocol": 1,
        "pluginId": action.plugin.manifest.id,
        "pluginName": action.plugin.manifest.name,
        "actionId": action.action.id,
        "actionName": action.action.name,
        "pluginDir": action.plugin.dir,
        "hostDir": host_dir,
        "ffmpeg": ffmpeg_path(),
        "outputDir": output_dir,
        "files": files_payload(files),
    });

    let mut child = Command::new(&entry)
        .current_dir(&action.plugin.dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x0800_0000)
        .spawn()
        .with_context(|| format!("native plugin failed to start: {}", entry.display()))?;

    {
        let mut stdin = child.stdin.take().context("native plugin stdin unavailable")?;
        stdin
            .write_all(request.to_string().as_bytes())
            .with_context(|| format!("failed to send request to {}", entry.display()))?;
    }

    let stdout = child.stdout.take().context("native plugin stdout unavailable")?;
    let (tx, rx) = mpsc::channel::<String>();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let stderr = child.stderr.take().context("native plugin stderr unavailable")?;
    let stderr_reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = BufReader::new(stderr).read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    });

    let deadline = Instant::now() + TIME_LIMIT;
    let mut plugin_error: Option<String> = None;

    loop {
        let now = Instant::now();
        if now >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            let _ = stderr_reader.join();
            bail!("native plugin timed out: {}", entry.display());
        }
        match rx.recv_timeout((deadline - now).min(Duration::from_millis(250))) {
            Ok(line) => handle_native_line(&line, &emit, &mut plugin_error),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    let remaining = deadline.saturating_duration_since(Instant::now());
    let status = match child.wait_timeout(remaining)? {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            let _ = stderr_reader.join();
            bail!("native plugin timed out: {}", entry.display());
        }
    };
    let _ = reader.join();
    let stderr_text = stderr_reader.join().unwrap_or_default();
    let stderr_text = stderr_text.trim();

    if let Some(message) = plugin_error {
        if stderr_text.is_empty() {
            bail!("{message}");
        }
        bail!("{message}\n{stderr_text}");
    }
    if !status.success() {
        if stderr_text.is_empty() {
            bail!("native plugin exited with {status}");
        }
        bail!("native plugin exited with {status}: {stderr_text}");
    }

    Ok(())
}

#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum NativeEvent {
    Log {
        message: String,
    },
    Progress {
        value: f64,
    },
    Done {
        input: String,
        output: String,
    },
    Error {
        message: String,
    },
}

fn handle_native_line(line: &str, emit: &Emit, plugin_error: &mut Option<String>) {
    let line = line.trim().trim_start_matches('\u{feff}').trim();
    if line.is_empty() {
        return;
    }
    match serde_json::from_str::<NativeEvent>(line) {
        Ok(NativeEvent::Log { message }) => emit(JobEvent::Log(message)),
        Ok(NativeEvent::Progress { value }) => {
            emit(JobEvent::Progress(value.clamp(0.0, 1.0) as f32))
        }
        Ok(NativeEvent::Done { input, output }) => emit(JobEvent::FileDone {
            input: PathBuf::from(input),
            output: PathBuf::from(output),
        }),
        Ok(NativeEvent::Error { message }) => {
            emit(JobEvent::Log(message.clone()));
            if plugin_error.is_none() {
                *plugin_error = Some(message);
            }
        }
        Err(_) => emit(JobEvent::Log(line.to_string())),
    }
}

fn files_payload(files: &[PathBuf]) -> Vec<serde_json::Value> {
    files
        .iter()
        .map(|p| {
            let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            serde_json::json!({
                "name": p.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                "path": p.to_string_lossy(),
                "ext": p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase(),
                "size": size
            })
        })
        .collect()
}

fn run_js(
    source: &str,
    files_json: &str,
    action_id: &str,
    convert: Arc<ConvertContext>,
    emit: Emit,
) -> anyhow::Result<()> {
    let rt = Runtime::new().map_err(|e| anyhow!("quickjs runtime: {e}"))?;
    rt.set_memory_limit(MEMORY_LIMIT);
    rt.set_max_stack_size(1024 * 1024);

    let deadline = Instant::now() + TIME_LIMIT;
    rt.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));

    let ctx = Context::full(&rt).map_err(|e| anyhow!("quickjs context: {e}"))?;

    ctx.with(|ctx| -> anyhow::Result<()> {
        let globals = ctx.globals();

        let emit_log = emit.clone();
        let log_fn = Function::new(ctx.clone(), move |msg: String| {
            emit_log(JobEvent::Log(msg));
        })
        .map_err(|e| anyhow!("{e}"))?;

        let emit_progress = emit.clone();
        let progress_fn = Function::new(ctx.clone(), move |value: f64| {
            let value = value.clamp(0.0, 1.0) as f32;
            emit_progress(JobEvent::Progress(value));
        })
        .map_err(|e| anyhow!("{e}"))?;

        let convert_fn = convert.clone();
        let convert_js = Function::new(
            ctx.clone(),
            move |path: String, format: String, options: String| -> String {
                match convert_fn.convert(&path, &format, &options) {
                    Ok(path) => serde_json::json!({ "ok": true, "path": path }).to_string(),
                    Err(e) => {
                        serde_json::json!({ "ok": false, "error": e.to_string() }).to_string()
                    }
                }
            },
        )
        .map_err(|e| anyhow!("{e}"))?;

        let name_fn = convert.clone();
        let name_js = Function::new(ctx.clone(), move |path: String, ext: String| -> String {
            match name_fn.target_name(&path, &ext) {
                Ok(path) => serde_json::json!({ "ok": true, "path": path }).to_string(),
                Err(e) => serde_json::json!({ "ok": false, "error": e.to_string() }).to_string(),
            }
        })
        .map_err(|e| anyhow!("{e}"))?;

        let video_fn = convert.clone();
        let video_js = Function::new(
            ctx.clone(),
            move |path: String, format: String, options: String| -> String {
                match video_fn.video(&path, &format, &options) {
                    Ok(path) => serde_json::json!({ "ok": true, "path": path }).to_string(),
                    Err(e) => {
                        serde_json::json!({ "ok": false, "error": e.to_string() }).to_string()
                    }
                }
            },
        )
        .map_err(|e| anyhow!("{e}"))?;

        let schematic_fn = convert.clone();
        let schematic_js = Function::new(
            ctx.clone(),
            move |path: String, format: String, options: String| -> String {
                match schematic_fn.schematic(&path, &format, &options) {
                    Ok(path) => serde_json::json!({ "ok": true, "path": path }).to_string(),
                    Err(e) => {
                        serde_json::json!({ "ok": false, "error": e.to_string() }).to_string()
                    }
                }
            },
        )
        .map_err(|e| anyhow!("{e}"))?;

        let mesh_fn = convert.clone();
        let mesh_js = Function::new(
            ctx.clone(),
            move |path: String, format: String, options: String| -> String {
                match mesh_fn.mesh(&path, &format, &options) {
                    Ok(path) => serde_json::json!({ "ok": true, "path": path }).to_string(),
                    Err(e) => {
                        serde_json::json!({ "ok": false, "error": e.to_string() }).to_string()
                    }
                }
            },
        )
        .map_err(|e| anyhow!("{e}"))?;

        let excel_csv_fn = convert.clone();
        let excel_to_csv_js = Function::new(ctx.clone(), move |path: String| -> String {
            match excel_csv_fn.excel_to_csv(&path) {
                Ok(path) => serde_json::json!({ "ok": true, "path": path }).to_string(),
                Err(e) => serde_json::json!({ "ok": false, "error": e.to_string() }).to_string(),
            }
        })
        .map_err(|e| anyhow!("{e}"))?;

        let csv_excel_fn = convert.clone();
        let csv_to_excel_js = Function::new(ctx.clone(), move |path: String| -> String {
            match csv_excel_fn.csv_to_excel(&path) {
                Ok(path) => serde_json::json!({ "ok": true, "path": path }).to_string(),
                Err(e) => serde_json::json!({ "ok": false, "error": e.to_string() }).to_string(),
            }
        })
        .map_err(|e| anyhow!("{e}"))?;

        let md_html_fn = convert.clone();
        let markdown_to_html_js = Function::new(ctx.clone(), move |path: String| -> String {
            match md_html_fn.markdown_to_html(&path) {
                Ok(path) => serde_json::json!({ "ok": true, "path": path }).to_string(),
                Err(e) => serde_json::json!({ "ok": false, "error": e.to_string() }).to_string(),
            }
        })
        .map_err(|e| anyhow!("{e}"))?;

        let md_epub_fn = convert.clone();
        let markdown_to_epub_js = Function::new(ctx.clone(), move |path: String| -> String {
            match md_epub_fn.markdown_to_epub(&path) {
                Ok(path) => serde_json::json!({ "ok": true, "path": path }).to_string(),
                Err(e) => serde_json::json!({ "ok": false, "error": e.to_string() }).to_string(),
            }
        })
        .map_err(|e| anyhow!("{e}"))?;
        let font_fn = convert.clone();
        let font_js = Function::new(
            ctx.clone(),
            move |path: String, format: String, options: String| -> String {
                match font_fn.font(&path, &format, &options) {
                    Ok(path) => serde_json::json!({ "ok": true, "path": path }).to_string(),
                    Err(e) => {
                        serde_json::json!({ "ok": false, "error": e.to_string() }).to_string()
                    }
                }
            },
        )
        .map_err(|e| anyhow!("{e}"))?;

        globals
            .set("__action", action_id)
            .map_err(|e| anyhow!("{e}"))?;
        globals
            .set("__filesJson", files_json)
            .map_err(|e| anyhow!("{e}"))?;
        globals
            .set(
                "__outputDir",
                convert
                    .output_dir
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
            )
            .map_err(|e| anyhow!("{e}"))?;
        globals.set("__log", log_fn).map_err(|e| anyhow!("{e}"))?;
        globals
            .set("__progress", progress_fn)
            .map_err(|e| anyhow!("{e}"))?;
        globals
            .set("__convertRaw", convert_js)
            .map_err(|e| anyhow!("{e}"))?;
        globals
            .set("__nameRaw", name_js)
            .map_err(|e| anyhow!("{e}"))?;
        globals
            .set("__videoRaw", video_js)
            .map_err(|e| anyhow!("{e}"))?;
        globals
            .set("__fontRaw", font_js)
            .map_err(|e| anyhow!("{e}"))?;
        globals
            .set("__excelToCsvRaw", excel_to_csv_js)
            .map_err(|e| anyhow!("{e}"))?;
        globals
            .set("__csvToExcelRaw", csv_to_excel_js)
            .map_err(|e| anyhow!("{e}"))?;
        globals
            .set("__markdownToHtmlRaw", markdown_to_html_js)
            .map_err(|e| anyhow!("{e}"))?;
        globals
            .set("__markdownToEpubRaw", markdown_to_epub_js)
            .map_err(|e| anyhow!("{e}"))?;
        globals
            .set("__meshRaw", mesh_js)
            .map_err(|e| anyhow!("{e}"))?;
        globals
            .set("__schematicRaw", schematic_js)
            .map_err(|e| anyhow!("{e}"))?;

        let wrapped = format!(
            r#"(function () {{
  "use strict";
  const module = {{ exports: {{}} }};
  const exports = module.exports;
{source}
  const files = JSON.parse(__filesJson);
  const ctx = {{
    action: __action,
    outputDir: __outputDir,
    files: files,
    log: function (m) {{ __log(String(m)); }},
    progress: function (v) {{ __progress(Number(v)); }},
    convert: function (f, fmt, opts) {{
      const p = (f && typeof f === "object") ? f.path : f;
      const r = JSON.parse(__convertRaw(String(p), String(fmt), JSON.stringify(opts || {{}})));
      if (!r.ok) throw new Error(r.error || "convert failed");
      return r.path;
    }},
    excelToCsv: function (f) {{
      const p = (f && typeof f === "object") ? f.path : f;
      const r = JSON.parse(__excelToCsvRaw(String(p)));
      if (!r.ok) throw new Error(r.error || "Excel to CSV failed");
      return r.path;
    }},
    csvToExcel: function (f) {{
      const p = (f && typeof f === "object") ? f.path : f;
      const r = JSON.parse(__csvToExcelRaw(String(p)));
      if (!r.ok) throw new Error(r.error || "CSV to Excel failed");
      return r.path;
    }},
    markdownToHtml: function (f) {{
      const p = (f && typeof f === "object") ? f.path : f;
      const r = JSON.parse(__markdownToHtmlRaw(String(p)));
      if (!r.ok) throw new Error(r.error || "Markdown to HTML failed");
      return r.path;
    }},
    markdownToEpub: function (f) {{
      const p = (f && typeof f === "object") ? f.path : f;
      const r = JSON.parse(__markdownToEpubRaw(String(p)));
      if (!r.ok) throw new Error(r.error || "Markdown to EPUB failed");
      return r.path;
    }},
    mesh: function (f, fmt, opts) {{
      const p = (f && typeof f === "object") ? f.path : f;
      const r = JSON.parse(__meshRaw(String(p), String(fmt), JSON.stringify(opts || {{}})));
      if (!r.ok) throw new Error(r.error || "mesh conversion failed");
      return r.path;
    }},
    name: function (f, ext) {{
      const p = (f && typeof f === "object") ? f.path : f;
      const r = JSON.parse(__nameRaw(String(p), String(ext)));
      if (!r.ok) throw new Error(r.error || "name failed");
      return r.path;
    }},
    video: function (f, fmt, opts) {{
      const p = (f && typeof f === "object") ? f.path : f;
      const r = JSON.parse(__videoRaw(String(p), String(fmt), JSON.stringify(opts || {{}})));
      if (!r.ok) throw new Error(r.error || "video conversion failed");
      return r.path;
    }},
    audio: function (f, fmt, opts) {{
      const p = (f && typeof f === "object") ? f.path : f;
      const r = JSON.parse(__videoRaw(String(p), String(fmt), JSON.stringify(opts || {{}})));
      if (!r.ok) throw new Error(r.error || "audio conversion failed");
      return r.path;
    }},
    font: function (f, fmt, opts) {{
      const p = (f && typeof f === "object") ? f.path : f;
      const r = JSON.parse(__fontRaw(String(p), String(fmt), JSON.stringify(opts || {{}})));
      if (!r.ok) throw new Error(r.error || "font conversion failed");
      return r.path;
    }},
    schematic: function (f, fmt, opts) {{
      const p = (f && typeof f === "object") ? f.path : f;
      const r = JSON.parse(__schematicRaw(String(p), String(fmt), JSON.stringify(opts || {{}})));
      if (!r.ok) throw new Error(r.error || "schematic conversion failed");
      return r.path;
    }}
  }};
  const entry = module.exports;
  if (typeof entry !== "function") throw new Error("module.exports must be a function");
  entry(ctx);
}})();"#
        );

        match ctx.eval::<(), _>(wrapped.as_bytes()) {
            Ok(()) => Ok(()),
            Err(error) => {
                let message = ctx
                    .catch()
                    .into_object()
                    .and_then(Exception::from_object)
                    .and_then(|exception| exception.message())
                    .unwrap_or_else(|| format!("{error}"));
                Err(anyhow!("{message}"))
            }
        }
    })
}

pub struct ConvertContext {
    allowed: HashSet<PathBuf>,
    output_dir: Option<PathBuf>,
    reserved: std::sync::Mutex<HashSet<PathBuf>>,
    emit: Emit,
}

impl ConvertContext {
    fn checked_input(&self, path: &str) -> anyhow::Result<PathBuf> {
        let canonical =
            std::fs::canonicalize(path).with_context(|| format!("input not found: {path}"))?;
        if !self.allowed.contains(&canonical) {
            bail!("file was not selected: {path}");
        }
        Ok(canonical)
    }

    pub fn target_name(&self, path: &str, ext: &str) -> anyhow::Result<String> {
        let ext = normalize_ext(ext)?;
        self.target_name_for(path, &ext)
    }

    fn target_name_for(&self, path: &str, ext: &str) -> anyhow::Result<String> {
        let input = self.checked_input(path)?;
        let ext = ext.trim_start_matches('.').to_ascii_lowercase();
        let dir = match &self.output_dir {
            Some(dir) => {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("cannot create output dir: {}", dir.display()))?;
                dir.clone()
            }
            None => input
                .parent()
                .ok_or_else(|| anyhow!("input has no parent directory"))?
                .to_path_buf(),
        };
        let stem = input
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("input has no file name"))?;

        let mut reserved = self.reserved.lock().unwrap();
        for i in 0..10_000 {
            let name = if i == 0 {
                format!("{stem}.{ext}")
            } else {
                format!("{stem}-{i}.{ext}")
            };
            let candidate = dir.join(name);
            if candidate != input && !candidate.exists() && !reserved.contains(&candidate) {
                reserved.insert(candidate.clone());
                return Ok(candidate.to_string_lossy().to_string());
            }
        }
        bail!("could not find a free output name")
    }

    pub fn convert(&self, path: &str, format: &str, options: &str) -> anyhow::Result<String> {
        let input = self.checked_input(path)?;
        let ext = normalize_ext(format)?;
        let target = PathBuf::from(self.target_name(&input.to_string_lossy(), &ext)?);

        if matches!(ext.as_str(), "avif" | "jxl") {
            return self.ffmpeg_image(&input, &target, &ext);
        }

        let img = match image::open(&input) {
            Ok(img) => img,
            Err(_) => return self.ffmpeg_image(&input, &target, &ext),
        };
        write_image(&img, &target, &ext, options)?;

        (self.emit)(JobEvent::FileDone {
            input: input.clone(),
            output: target.clone(),
        });
        Ok(target.to_string_lossy().to_string())
    }

    fn ffmpeg_image(&self, input: &Path, target: &Path, ext: &str) -> anyhow::Result<String> {
        let ffmpeg = ffmpeg_path().ok_or_else(|| anyhow!("ffmpeg.exe が見つかりません"))?;
        let mut command = Command::new(&ffmpeg);
        command.creation_flags(0x0800_0000);
        command
            .arg("-nostdin")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-y")
            .arg("-i")
            .arg(input)
            .arg("-frames:v")
            .arg("1");
        match ext {
            "jpg" => {
                command.arg("-q:v").arg("2");
            }
            "avif" => {
                command
                    .arg("-c:v")
                    .arg("libaom-av1")
                    .arg("-still-picture")
                    .arg("1")
                    .arg("-crf")
                    .arg("30");
            }
            "jxl" => {
                command.arg("-c:v").arg("libjxl").arg("-quality").arg("90");
            }
            _ => {}
        }
        command
            .arg(target)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = command
            .output()
            .with_context(|| format!("cannot start ffmpeg: {}", ffmpeg.display()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("ffmpeg image conversion failed: {}", stderr.trim());
        }
        (self.emit)(JobEvent::FileDone {
            input: input.to_path_buf(),
            output: target.to_path_buf(),
        });
        Ok(target.to_string_lossy().to_string())
    }

    pub fn video(&self, path: &str, format: &str, options: &str) -> anyhow::Result<String> {
        let input = self.checked_input(path)?;
        let format = format.trim().trim_start_matches('.').to_ascii_lowercase();
        if !matches!(
            format.as_str(),
            "mp4"
                | "mp4copy"
                | "mkv"
                | "webm"
                | "gif"
                | "jpeg"
                | "png"
                | "mp3"
                | "wav"
                | "flac"
                | "ogg"
                | "m4a"
                | "aac"
                | "opus"
                | "avi"
                | "mov"
                | "m4v"
                | "wmv"
                | "flv"
                | "ts"
                | "mpg"
                | "prores"
                | "hevc"
                | "av1"
                | "wma"
                | "aiff"
                | "alac"
                | "ac3"
                | "eac3"
                | "movaudio"
        ) {
            bail!("unsupported video format: {format}");
        }

        let target_format = match format.as_str() {
            "mp4copy" => "mp4",
            "jpeg" => "jpg",
            "prores" => "mov",
            "hevc" => "mp4",
            "av1" => "mkv",
            "alac" => "m4a",
            "movaudio" => "mov",
            other => other,
        };
        let target = PathBuf::from(self.target_name_for(&input.to_string_lossy(), target_format)?);
        let ffmpeg = ffmpeg_path().ok_or_else(|| {
            anyhow!(
                "ffmpeg.exe が見つかりません。FFmpeg を PATH に置くか、ExtensionChange.exe と同じフォルダに置いてください。"
            )
        })?;

        let quality = serde_json::from_str::<serde_json::Value>(options)
            .ok()
            .and_then(|value| value.get("quality").and_then(|q| q.as_u64()))
            .unwrap_or(23)
            .min(51)
            .to_string();

        if format == "jpeg" || format == "png" {
            let mut command = Command::new(&ffmpeg);
            command.creation_flags(0x0800_0000);
            command
                .arg("-nostdin")
                .arg("-hide_banner")
                .arg("-loglevel")
                .arg("error")
                .arg("-y")
                .arg("-i")
                .arg(&input)
                .arg("-frames:v")
                .arg("1");
            if format == "jpeg" {
                command.arg("-q:v").arg("2");
            }
            command
                .arg(&target)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let output = command
                .output()
                .with_context(|| format!("cannot start ffmpeg: {}", ffmpeg.display()))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("ffmpeg image conversion failed: {}", stderr.trim());
            }
            (self.emit)(JobEvent::FileDone {
                input: input.clone(),
                output: target.clone(),
            });
            return Ok(target.to_string_lossy().to_string());
        }

        if format == "mkv" || format == "mp4copy" {
            let mut copy = Command::new(&ffmpeg);
            copy.creation_flags(0x0800_0000);
            copy.arg("-nostdin")
                .arg("-hide_banner")
                .arg("-loglevel")
                .arg("error")
                .arg("-y")
                .arg("-i")
                .arg(&input)
                .arg("-c")
                .arg("copy")
                .arg(&target)
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let copied = copy
                .status()
                .with_context(|| format!("cannot start ffmpeg: {}", ffmpeg.display()))?;
            if copied.success() {
                (self.emit)(JobEvent::FileDone {
                    input: input.clone(),
                    output: target.clone(),
                });
                return Ok(target.to_string_lossy().to_string());
            }
            if format == "mkv" {
                bail!("ffmpeg copy failed: {copied}");
            }
        }

        let encode_format = if format == "mp4copy" {
            "mp4"
        } else {
            format.as_str()
        };

        let mut command = Command::new(&ffmpeg);
        command.creation_flags(0x0800_0000);
        command
            .arg("-nostdin")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-y")
            .arg("-i")
            .arg(&input);

        match encode_format {
            "mp4" | "mov" | "m4v" => {
                command
                    .arg("-c:v")
                    .arg("libx264")
                    .arg("-crf")
                    .arg(&quality)
                    .arg("-preset")
                    .arg("medium")
                    .arg("-c:a")
                    .arg("aac")
                    .arg("-b:a")
                    .arg("128k");
            }
            "webm" => {
                command
                    .arg("-c:v")
                    .arg("libvpx-vp9")
                    .arg("-crf")
                    .arg(&quality)
                    .arg("-b:v")
                    .arg("0")
                    .arg("-c:a")
                    .arg("libopus");
            }
            "gif" => {
                command
                    .arg("-vf")
                    .arg("fps=12,scale=480:-1:flags=lanczos")
                    .arg("-loop")
                    .arg("0");
            }
            "mp3" => {
                command
                    .arg("-vn")
                    .arg("-c:a")
                    .arg("libmp3lame")
                    .arg("-q:a")
                    .arg("2");
            }
            "wav" => {
                command.arg("-vn").arg("-c:a").arg("pcm_s16le");
            }
            "flac" => {
                command.arg("-vn").arg("-c:a").arg("flac");
            }
            "ogg" => {
                command
                    .arg("-vn")
                    .arg("-c:a")
                    .arg("libvorbis")
                    .arg("-q:a")
                    .arg("5");
            }
            "m4a" | "aac" => {
                command
                    .arg("-vn")
                    .arg("-c:a")
                    .arg("aac")
                    .arg("-b:a")
                    .arg("192k");
            }
            "opus" => {
                command
                    .arg("-vn")
                    .arg("-c:a")
                    .arg("libopus")
                    .arg("-b:a")
                    .arg("128k");
            }
            "mkv" => {
                command
                    .arg("-c:v")
                    .arg("libx264")
                    .arg("-crf")
                    .arg(&quality)
                    .arg("-preset")
                    .arg("medium")
                    .arg("-c:a")
                    .arg("aac")
                    .arg("-b:a")
                    .arg("128k");
            }
            "avi" => {
                command
                    .arg("-c:v")
                    .arg("mpeg4")
                    .arg("-q:v")
                    .arg("5")
                    .arg("-c:a")
                    .arg("libmp3lame")
                    .arg("-b:a")
                    .arg("192k");
            }
            "wmv" => {
                command
                    .arg("-c:v")
                    .arg("wmv2")
                    .arg("-b:v")
                    .arg("2M")
                    .arg("-c:a")
                    .arg("wmav2")
                    .arg("-b:a")
                    .arg("192k");
            }
            "flv" => {
                command
                    .arg("-c:v")
                    .arg("flv")
                    .arg("-b:v")
                    .arg("2M")
                    .arg("-c:a")
                    .arg("libmp3lame")
                    .arg("-b:a")
                    .arg("128k");
            }
            "ts" => {
                command
                    .arg("-c:v")
                    .arg("libx264")
                    .arg("-crf")
                    .arg(&quality)
                    .arg("-preset")
                    .arg("medium")
                    .arg("-c:a")
                    .arg("aac")
                    .arg("-b:a")
                    .arg("128k");
            }
            "mpg" => {
                command
                    .arg("-c:v")
                    .arg("mpeg2video")
                    .arg("-q:v")
                    .arg("3")
                    .arg("-c:a")
                    .arg("mp2")
                    .arg("-b:a")
                    .arg("192k");
            }
            "prores" => {
                command
                    .arg("-c:v")
                    .arg("prores_ks")
                    .arg("-profile:v")
                    .arg("3")
                    .arg("-c:a")
                    .arg("pcm_s16le");
            }
            "hevc" => {
                command
                    .arg("-c:v")
                    .arg("libx265")
                    .arg("-crf")
                    .arg(&quality)
                    .arg("-preset")
                    .arg("medium")
                    .arg("-c:a")
                    .arg("aac")
                    .arg("-b:a")
                    .arg("128k")
                    .arg("-tag:v")
                    .arg("hvc1");
            }
            "av1" => {
                command
                    .arg("-c:v")
                    .arg("libsvtav1")
                    .arg("-crf")
                    .arg(&quality)
                    .arg("-c:a")
                    .arg("libopus")
                    .arg("-b:a")
                    .arg("128k");
            }
            "wma" => {
                command
                    .arg("-vn")
                    .arg("-c:a")
                    .arg("wmav2")
                    .arg("-b:a")
                    .arg("192k");
            }
            "aiff" => {
                command.arg("-vn").arg("-c:a").arg("pcm_s16be");
            }
            "alac" => {
                command.arg("-vn").arg("-c:a").arg("alac");
            }
            "ac3" => {
                command
                    .arg("-vn")
                    .arg("-c:a")
                    .arg("ac3")
                    .arg("-b:a")
                    .arg("192k");
            }
            "eac3" => {
                command
                    .arg("-vn")
                    .arg("-c:a")
                    .arg("eac3")
                    .arg("-b:a")
                    .arg("192k");
            }
            "movaudio" => {
                command
                    .arg("-vn")
                    .arg("-c:a")
                    .arg("aac")
                    .arg("-b:a")
                    .arg("192k");
            }
            _ => unreachable!(),
        }

        command.arg(&target);
        command.stdout(Stdio::null()).stderr(Stdio::null());
        let mut child = command
            .spawn()
            .with_context(|| format!("cannot start ffmpeg: {}", ffmpeg.display()))?;
        match child.wait_timeout(Duration::from_secs(1800))? {
            Some(status) if status.success() => {}
            Some(status) => bail!("ffmpeg failed: {status}"),
            None => {
                let _ = child.kill();
                bail!("ffmpeg timed out");
            }
        }

        (self.emit)(JobEvent::FileDone {
            input: input.clone(),
            output: target.clone(),
        });
        Ok(target.to_string_lossy().to_string())
    }
    fn document_convert(
        &self,
        path: &str,
        target_ext: &str,
        convert: fn(&Path, &Path) -> anyhow::Result<()>,
    ) -> anyhow::Result<String> {
        let input = self.checked_input(path)?;
        let target = PathBuf::from(
            self.target_name_for(&input.to_string_lossy(), target_ext)?,
        );
        convert(&input, &target)?;
        (self.emit)(JobEvent::FileDone {
            input: input.clone(),
            output: target.clone(),
        });
        Ok(target.to_string_lossy().to_string())
    }

    pub fn excel_to_csv(&self, path: &str) -> anyhow::Result<String> {
        self.document_convert(path, "csv", crate::document::excel_to_csv)
    }

    pub fn csv_to_excel(&self, path: &str) -> anyhow::Result<String> {
        self.document_convert(path, "xlsx", crate::document::csv_to_excel)
    }

    pub fn markdown_to_html(&self, path: &str) -> anyhow::Result<String> {
        self.document_convert(path, "html", crate::document::markdown_to_html)
    }

    pub fn markdown_to_epub(&self, path: &str) -> anyhow::Result<String> {
        self.document_convert(path, "epub", crate::document::markdown_to_epub)
    }
    pub fn mesh(&self, path: &str, format: &str, _options: &str) -> anyhow::Result<String> {
        let input = self.checked_input(path)?;
        let ext = normalize_mesh_ext(format)?;
        let target = PathBuf::from(self.target_name_for(&input.to_string_lossy(), &ext)?);
        crate::mesh::convert(&input, &target, &ext)?;
        (self.emit)(JobEvent::FileDone {
            input: input.clone(),
            output: target.clone(),
        });
        Ok(target.to_string_lossy().to_string())
    }
    pub fn font(&self, path: &str, format: &str, _options: &str) -> anyhow::Result<String> {
        let input = self.checked_input(path)?;
        let format = format.trim().trim_start_matches('.').to_ascii_lowercase();
        if !matches!(format.as_str(), "ttf" | "otf" | "woff" | "woff2") {
            bail!("unsupported font format: {format}");
        }
        let target = PathBuf::from(self.target_name_for(&input.to_string_lossy(), &format)?);
        let source = std::fs::read(&input)
            .with_context(|| format!("cannot read font: {}", input.display()))?;

        let sfnt = if source.starts_with(b"wOFF") {
            woff::version1::decompress(&source).ok_or_else(|| anyhow!("WOFF decode failed"))?
        } else if source.starts_with(b"wOF2") {
            woff::version2::decompress(&source).ok_or_else(|| anyhow!("WOFF2 decode failed"))?
        } else {
            source
        };

        let outline = if sfnt.starts_with(b"OTTO") {
            "otf"
        } else if sfnt.starts_with(&[0, 1, 0, 0]) {
            "ttf"
        } else {
            bail!("unsupported sfnt font");
        };

        match format.as_str() {
            "ttf" | "otf" => {
                if format != outline {
                    bail!(
                        "outline mismatch: input={outline}, target={format}; TTF and OTF outline conversion is not supported"
                    );
                }
                std::fs::write(&target, &sfnt)
                    .with_context(|| format!("cannot write: {}", target.display()))?;
            }
            "woff" => {
                let data = woff::version1::compress(&sfnt, 1, 0)
                    .ok_or_else(|| anyhow!("WOFF encode failed"))?;
                std::fs::write(&target, data)
                    .with_context(|| format!("cannot write: {}", target.display()))?;
            }
            "woff2" => {
                let data = woff::version2::compress(&sfnt, Vec::new(), 11, outline == "ttf")
                    .ok_or_else(|| anyhow!("WOFF2 encode failed"))?;
                std::fs::write(&target, data)
                    .with_context(|| format!("cannot write: {}", target.display()))?;
            }
            _ => unreachable!(),
        }

        (self.emit)(JobEvent::FileDone {
            input: input.clone(),
            output: target.clone(),
        });
        Ok(target.to_string_lossy().to_string())
    }

    pub fn schematic(&self, path: &str, format: &str, _options: &str) -> anyhow::Result<String> {
        let input = self.checked_input(path)?;
        let format = format.trim().trim_start_matches('.').to_ascii_lowercase();
        let (id, ext) = match format.as_str() {
            "litematic" | "litematica" => ("litematica", "litematic"),
            "schem" => ("schem", "schem"),
            "bp" | "axiom" => ("bp", "bp"),
            "schematic" => ("schematic", "schematic"),
            "nbt" => ("nbt", "nbt"),
            _ => bail!("unsupported schematic format: {format}"),
        };
        let target = PathBuf::from(self.target_name_for(&input.to_string_lossy(), ext)?);
        let helper =
            schem_convert_path().ok_or_else(|| anyhow!("schem-convert.exe が見つかりません"))?;

        let mut command = Command::new(&helper);
        command.creation_flags(0x0800_0000);
        command
            .arg("-in")
            .arg(&input)
            .arg("-out")
            .arg(&target)
            .arg("-format")
            .arg(id)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = command
            .output()
            .with_context(|| format!("cannot start schem-convert: {}", helper.display()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("schematic conversion failed: {}", stderr.trim());
        }

        (self.emit)(JobEvent::FileDone {
            input: input.clone(),
            output: target.clone(),
        });
        Ok(target.to_string_lossy().to_string())
    }
}

fn schem_convert_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("EXTENSION_CHANGE_SCHEM_CONVERT") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("schem-convert.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let candidate = PathBuf::from(local)
            .join("ExtensionChange")
            .join("tools")
            .join("schem-convert.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("schem-convert.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    #[cfg(debug_assertions)]
    {
        let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/schem-convert/schem-convert.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn ffmpeg_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("EXTENSION_CHANGE_FFMPEG") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("ffmpeg.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let candidate = PathBuf::from(local)
            .join("ExtensionChange")
            .join("ffmpeg")
            .join("ffmpeg.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("ffmpeg.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
fn normalize_mesh_ext(ext: &str) -> anyhow::Result<String> {
    let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    match ext.as_str() {
        "obj" | "ply" | "off" => Ok(ext),
        _ => bail!("unsupported 3D output format: {ext}"),
    }
}

fn normalize_ext(ext: &str) -> anyhow::Result<String> {
    let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" | "tif" | "tiff" | "ico" | "avif" | "jxl" | "tga" | "pnm" | "qoi" => {
            Ok(if ext == "jpeg" {
                "jpg".into()
            } else if ext == "tif" {
                "tiff".into()
            } else {
                ext
            })
        }
        _ => bail!("unsupported output format: {ext}"),
    }
}

fn write_image(
    img: &image::DynamicImage,
    target: &Path,
    ext: &str,
    options: &str,
) -> anyhow::Result<()> {
    let quality = serde_json::from_str::<serde_json::Value>(options)
        .ok()
        .and_then(|v| v.get("quality").and_then(|q| q.as_u64()))
        .unwrap_or(92)
        .clamp(1, 100) as u8;

    let file = std::fs::File::create(target)
        .with_context(|| format!("cannot write: {}", target.display()))?;
    let mut writer = std::io::BufWriter::new(file);

    match ext {
        "jpg" => {
            let rgb = img.to_rgb8();
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut writer, quality);
            enc.encode_image(&rgb)
                .with_context(|| format!("jpeg encode failed: {}", target.display()))?;
        }
        "webp" => {
            img.write_to(&mut writer, ImageFormat::WebP)
                .with_context(|| format!("webp encode failed: {}", target.display()))?;
        }
        "ico" => {
            let icon = make_icon(img);
            icon.write_to(&mut writer, ImageFormat::Ico)
                .with_context(|| format!("ico encode failed: {}", target.display()))?;
        }
        _ => {
            let format = match ext {
                "png" => ImageFormat::Png,
                "bmp" => ImageFormat::Bmp,
                "gif" => ImageFormat::Gif,
                "tiff" => ImageFormat::Tiff,
                "ico" => ImageFormat::Ico,
                "tga" => ImageFormat::Tga,
                "pnm" => ImageFormat::Pnm,
                "qoi" => ImageFormat::Qoi,
                _ => bail!("unsupported output format: {ext}"),
            };
            img.write_to(&mut writer, format)
                .with_context(|| format!("encode failed: {}", target.display()))?;
        }
    }
    Ok(())
}

fn make_icon(img: &image::DynamicImage) -> image::DynamicImage {
    let thumb = img.thumbnail(256, 256);
    let mut canvas = image::RgbaImage::new(256, 256);
    let x = i64::from((256 - thumb.width()) / 2);
    let y = i64::from((256 - thumb.height()) / 2);
    image::imageops::overlay(&mut canvas, &thumb.to_rgba8(), x, y);
    image::DynamicImage::ImageRgba8(canvas)
}
