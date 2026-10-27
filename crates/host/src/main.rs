#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod engine;
mod gui;
mod mcp;
mod ui_style;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context as _};
use extension_change_core::{config::Config, paths, plugin};

struct Args {
    files: Vec<PathBuf>,
    action: Option<String>,
    output: Option<PathBuf>,
    headless: bool,
    run: bool,
    list: bool,
    mcp: bool,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let args = parse_args()?;
    let base = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let plugin_dirs = paths::plugin_dirs(&base);
    let config_path = paths::config_path();
    let config = Config::load(&config_path);

    if args.mcp {
        return mcp::run(&plugin_dirs, &config);
    }

    if args.list {
        println!("plugins: {:?}", plugin_dirs);
        let (plugins, errors) = plugin::discover_many(&plugin_dirs);
        for plugin in &plugins {
            for action in plugin.actions() {
                let key = format!("{}/{}", plugin.manifest.id, action.id);
                println!(
                    "{}  {}  enabled={}",
                    key,
                    action.name,
                    config.is_enabled(&key)
                );
            }
        }
        for error in errors {
            println!("error: {error}");
        }
        return Ok(());
    }

    if args.headless {
        let key = args
            .action
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("--action is required with --headless"))?;
        let action = plugin::find_action_many(&plugin_dirs, key)
            .ok_or_else(|| anyhow::anyhow!("unknown action: {key}"))?;
        if !config.is_enabled(key) {
            bail!("action is disabled: {key}");
        }
        let output = args.output.clone().or_else(|| {
            config
                .output_dir
                .as_ref()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        });
        let emit: engine::Emit = Arc::new(|event| match event {
            engine::JobEvent::Log(message) => println!("{message}"),
            engine::JobEvent::Progress(value) => println!("progress {:.0}%", value * 100.0),
            engine::JobEvent::FileDone { input, output } => {
                println!("{} -> {}", input.display(), output.display())
            }
        });
        engine::run_job(&action, &args.files, output, emit)?;
        return Ok(());
    }

    let default_output = args.output.clone().or_else(|| {
        config
            .output_dir
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
    });

    if args.run {
        if let Some(key) = args.action.clone() {
            let action = plugin::find_action_many(&plugin_dirs, &key)
                .ok_or_else(|| anyhow::anyhow!("unknown action: {key}"))?;
            if !config.is_enabled(&key) {
                bail!("action is disabled: {key}");
            }
            let output = args.output.clone();
            return gui::run_progress(action, args.files, output).map_err(|e| anyhow::anyhow!(e));
        }
    }

    gui::run(
        args.files,
        args.action,
        default_output,
        plugin_dirs,
        config_path,
        config,
    )
    .map_err(|e| anyhow::anyhow!(e))
}

fn parse_args() -> anyhow::Result<Args> {
    let mut args = Args {
        files: Vec::new(),
        action: None,
        output: None,
        headless: false,
        run: false,
        list: false,
        mcp: false,
    };

    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--file" => {
                let value = iter.next().context("--file requires a path")?;
                args.files.push(PathBuf::from(value));
            }
            "--action" => {
                args.action = Some(iter.next().context("--action requires a value")?);
            }
            "--output" => {
                args.output = Some(PathBuf::from(
                    iter.next().context("--output requires a path")?,
                ));
            }
            "--headless" => args.headless = true,
            "--run" => args.run = true,
            "--list" => args.list = true,
            "--mcp" => args.mcp = true,
            other => {
                if other.starts_with('-') {
                    bail!("unknown option: {other}");
                }
                args.files.push(PathBuf::from(other));
            }
        }
    }

    Ok(args)
}
