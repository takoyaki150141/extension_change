use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::Duration;

use extension_change_core::config::Config;
use extension_change_core::paths;
use extension_change_core::plugin::{self, ActionEntry};
use native_windows_gui as nwg;

use crate::engine::{self, Emit, JobEvent};
use crate::ui_style;

enum Msg {
    Event(JobEvent),
    Done(Result<(), String>),
}

pub fn run(
    files: Vec<PathBuf>,
    initial_action: Option<String>,
    output: Option<PathBuf>,
    plugin_dirs: Vec<PathBuf>,
    config_path: PathBuf,
    config: Config,
) -> Result<(), String> {
    nwg::init().map_err(|e| format!("failed to initialize GUI: {e}"))?;
    let _ = nwg::Font::set_global_family("Yu Gothic UI");

    let app = App::new(
        files,
        initial_action,
        output,
        plugin_dirs,
        config_path,
        config,
    )?;

    nwg::dispatch_thread_events();
    drop(app);
    Ok(())
}

pub fn run_progress(
    action: ActionEntry,
    files: Vec<PathBuf>,
    output: Option<PathBuf>,
) -> Result<(), String> {
    nwg::init().map_err(|e| format!("failed to initialize GUI: {e}"))?;
    let _ = nwg::Font::set_global_family("Yu Gothic UI");

    let app = ProgressApp::new(action, files, output)?;
    nwg::dispatch_thread_events();
    drop(app);
    Ok(())
}

#[allow(dead_code)]
struct App {
    window: nwg::Window,
    title_font: nwg::Font,
    body_font: nwg::Font,
    small_font: nwg::Font,
    section_font: nwg::Font,
    header_bg: nwg::Label,
    accent_bar: nwg::Label,
    header_line: nwg::Label,
    title_label: nwg::Label,
    files_label: nwg::Label,
    files_list: nwg::ListBox<String>,
    action_label: nwg::Label,
    action_combo: nwg::ComboBox<String>,
    format_label: nwg::Label,
    format_combo: nwg::ComboBox<String>,
    format_actions: RefCell<Vec<Option<ActionEntry>>>,
    output_label: nwg::Label,
    output_input: nwg::TextInput,
    browse_btn: nwg::Button,
    settings_btn: nwg::Button,
    run_btn: nwg::Button,
    progress: nwg::ProgressBar,
    log_label: nwg::Label,
    log_list: nwg::ListBox<String>,
    body_divider: nwg::Label,
    footer_bg: nwg::Label,
    footer_line: nwg::Label,
    hint_label: nwg::Label,
    version_label: nwg::Label,
    notice: nwg::Notice,

    settings_window: nwg::Window,
    checks: RefCell<Vec<nwg::CheckBox>>,
    check_keys: RefCell<Vec<String>>,
    save_btn: nwg::Button,
    open_btn: nwg::Button,
    reload_btn: nwg::Button,

    plugin_dirs: Vec<PathBuf>,
    config_path: PathBuf,
    config: RefCell<Config>,
    actions: RefCell<Vec<ActionEntry>>,
    files: RefCell<Vec<PathBuf>>,
    initial_action: RefCell<Option<String>>,
    job_rx: RefCell<Option<Receiver<Msg>>>,
    job_active: RefCell<bool>,
    handlers: RefCell<Vec<nwg::EventHandler>>,
}

impl App {
    fn new(
        files: Vec<PathBuf>,
        initial_action: Option<String>,
        output: Option<PathBuf>,
        plugin_dirs: Vec<PathBuf>,
        config_path: PathBuf,
        config: Config,
    ) -> Result<Rc<App>, String> {
        let err = |e: nwg::NwgError| format!("failed to build GUI: {e}");

        let mut title_font = nwg::Font::default();
        nwg::Font::builder()
            .family("Yu Gothic UI")
            .size(24)
            .weight(700)
            .build(&mut title_font)
            .map_err(err)?;

        let mut body_font = nwg::Font::default();
        nwg::Font::builder()
            .family("Yu Gothic UI")
            .size(15)
            .build(&mut body_font)
            .map_err(err)?;

        let mut small_font = nwg::Font::default();
        nwg::Font::builder()
            .family("Yu Gothic UI")
            .size(12)
            .build(&mut small_font)
            .map_err(err)?;

        let mut section_font = nwg::Font::default();
        nwg::Font::builder()
            .family("Yu Gothic UI")
            .size(15)
            .weight(600)
            .build(&mut section_font)
            .map_err(err)?;

        let mut window = nwg::Window::default();
        nwg::Window::builder()
            .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::VISIBLE)
            .size((900, 520))
            .position(ui_style::center(900, 520))
            .title("Extension Change")
            .build(&mut window)
            .map_err(err)?;

        let mut header_bg = nwg::Label::default();
        nwg::Label::builder()
            .text("")
            .position((0, 0))
            .size((900, 58))
            .parent(&window)
            .background_color(Some([255, 255, 255]))
            .build(&mut header_bg)
            .map_err(err)?;

        let mut accent_bar = nwg::Label::default();
        nwg::Label::builder()
            .text("")
            .position((16, 16))
            .size((4, 26))
            .parent(&window)
            .background_color(Some(ui_style::accent_rgb()))
            .build(&mut accent_bar)
            .map_err(err)?;

        let mut header_line = nwg::Label::default();
        nwg::Label::builder()
            .text("")
            .position((0, 57))
            .size((900, 1))
            .parent(&window)
            .background_color(Some([215, 215, 215]))
            .build(&mut header_line)
            .map_err(err)?;

        let mut title_label = nwg::Label::default();
        nwg::Label::builder()
            .text("Extension Change")
            .position((28, 14))
            .size((600, 32))
            .parent(&window)
            .font(Some(&title_font))
            .background_color(Some([255, 255, 255]))
            .build(&mut title_label)
            .map_err(err)?;

        let mut settings_btn = nwg::Button::default();
        nwg::Button::builder()
            .text("プラグイン設定")
            .position((734, 14))
            .size((150, 34))
            .parent(&window)
            .font(Some(&body_font))
            .build(&mut settings_btn)
            .map_err(err)?;

        let mut files_label = nwg::Label::default();
        nwg::Label::builder()
            .text("対象ファイル")
            .position((16, 70))
            .size((520, 22))
            .parent(&window)
            .font(Some(&section_font))
            .background_color(Some(ui_style::BG_RGB))
            .build(&mut files_label)
            .map_err(err)?;

        let mut files_list = nwg::ListBox::default();
        nwg::ListBox::builder()
            .collection(Vec::<String>::new())
            .position((16, 94))
            .size((520, 150))
            .parent(&window)
            .font(Some(&body_font))
            .build(&mut files_list)
            .map_err(err)?;

        let mut log_label = nwg::Label::default();
        nwg::Label::builder()
            .text("ログ")
            .position((552, 70))
            .size((332, 22))
            .parent(&window)
            .font(Some(&section_font))
            .background_color(Some(ui_style::BG_RGB))
            .build(&mut log_label)
            .map_err(err)?;

        let mut log_list = nwg::ListBox::default();
        nwg::ListBox::builder()
            .collection(Vec::<String>::new())
            .position((552, 94))
            .size((332, 328))
            .parent(&window)
            .font(Some(&body_font))
            .build(&mut log_list)
            .map_err(err)?;

        let mut body_divider = nwg::Label::default();
        nwg::Label::builder()
            .text("")
            .position((540, 68))
            .size((1, 356))
            .parent(&window)
            .background_color(Some([215, 215, 215]))
            .build(&mut body_divider)
            .map_err(err)?;

        let mut action_label = nwg::Label::default();
        nwg::Label::builder()
            .text("操作")
            .position((16, 258))
            .size((520, 22))
            .parent(&window)
            .font(Some(&section_font))
            .background_color(Some(ui_style::BG_RGB))
            .build(&mut action_label)
            .map_err(err)?;

        let mut action_combo = nwg::ComboBox::default();
        nwg::ComboBox::builder()
            .collection(Vec::<String>::new())
            .position((16, 282))
            .size((520, 160))
            .parent(&window)
            .font(Some(&body_font))
            .build(&mut action_combo)
            .map_err(err)?;

        let mut format_label = nwg::Label::default();
        nwg::Label::builder()
            .text("出力形式（その他）")
            .position((560, 354))
            .size((324, 22))
            .parent(&window)
            .font(Some(&section_font))
            .background_color(Some(ui_style::BG_RGB))
            .build(&mut format_label)
            .map_err(err)?;

        let mut format_combo = nwg::ComboBox::default();
        nwg::ComboBox::builder()
            .collection(ffmpeg_formats())
            .position((560, 378))
            .size((324, 160))
            .parent(&window)
            .font(Some(&body_font))
            .build(&mut format_combo)
            .map_err(err)?;
        let _ = format_combo.set_selection_string("mp4");
        format_label.set_visible(false);
        format_combo.set_visible(false);
        let mut output_label = nwg::Label::default();
        nwg::Label::builder()
            .text("出力先（空欄なら元のフォルダ）")
            .position((16, 354))
            .size((520, 22))
            .parent(&window)
            .font(Some(&section_font))
            .background_color(Some(ui_style::BG_RGB))
            .build(&mut output_label)
            .map_err(err)?;

        let mut output_input = nwg::TextInput::default();
        nwg::TextInput::builder()
            .text(
                output
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default()
                    .as_str(),
            )
            .position((16, 378))
            .size((410, 30))
            .parent(&window)
            .font(Some(&body_font))
            .build(&mut output_input)
            .map_err(err)?;

        let mut browse_btn = nwg::Button::default();
        nwg::Button::builder()
            .text("参照")
            .position((436, 378))
            .size((100, 30))
            .parent(&window)
            .font(Some(&body_font))
            .build(&mut browse_btn)
            .map_err(err)?;

        let mut run_btn = nwg::Button::default();
        nwg::Button::builder()
            .text("実行")
            .position((16, 420))
            .size((520, 42))
            .parent(&window)
            .font(Some(&section_font))
            .build(&mut run_btn)
            .map_err(err)?;

        let mut progress = nwg::ProgressBar::default();
        nwg::ProgressBar::builder()
            .range(0..100)
            .position((16, 470))
            .size((868, 14))
            .parent(&window)
            .build(&mut progress)
            .map_err(err)?;

        let mut footer_bg = nwg::Label::default();
        nwg::Label::builder()
            .text("")
            .position((0, 492))
            .size((900, 28))
            .parent(&window)
            .background_color(Some([255, 255, 255]))
            .build(&mut footer_bg)
            .map_err(err)?;

        let mut footer_line = nwg::Label::default();
        nwg::Label::builder()
            .text("")
            .position((0, 492))
            .size((900, 1))
            .parent(&window)
            .background_color(Some([215, 215, 215]))
            .build(&mut footer_line)
            .map_err(err)?;

        let mut hint_label = nwg::Label::default();
        nwg::Label::builder()
            .text("右クリックからも変換できます")
            .position((16, 496))
            .size((600, 20))
            .parent(&window)
            .font(Some(&small_font))
            .background_color(Some([255, 255, 255]))
            .build(&mut hint_label)
            .map_err(err)?;

        let mut version_label = nwg::Label::default();
        nwg::Label::builder()
            .text("v0.11.2")
            .position((734, 496))
            .size((150, 20))
            .parent(&window)
            .font(Some(&small_font))
            .background_color(Some([255, 255, 255]))
            .build(&mut version_label)
            .map_err(err)?;

        let mut notice = nwg::Notice::default();
        nwg::Notice::builder()
            .parent(&window)
            .build(&mut notice)
            .map_err(err)?;

        let (settings_window, checks, check_keys, save_btn, open_btn, reload_btn) =
            build_settings(&plugin_dirs, &config, &body_font)?;

        ui_style::apply_chrome(&window);
        ui_style::apply_chrome(&settings_window);
        ui_style::set_window_background(&window, [255, 255, 255]);
        ui_style::set_window_background(&settings_window, [255, 255, 255]);
        ui_style::send_to_back(&header_bg.handle);
        ui_style::send_to_back(&footer_bg.handle);
        ui_style::send_to_back(&header_line.handle);
        ui_style::send_to_back(&footer_line.handle);
        ui_style::send_to_back(&body_divider.handle);
        ui_style::set_clipsiblings(&header_bg.handle);
        ui_style::set_clipsiblings(&accent_bar.handle);
        ui_style::set_clipsiblings(&header_line.handle);
        ui_style::set_clipsiblings(&title_label.handle);
        ui_style::set_clipsiblings(&settings_btn.handle);
        ui_style::control_to_top(&settings_btn.handle);

        let app = Rc::new(App {
            window,
            title_font,
            body_font,
            small_font,
            section_font,
            header_bg,
            accent_bar,
            header_line,
            title_label,
            files_label,
            files_list,
            action_label,
            action_combo,
            format_label,
            format_combo,
            format_actions: RefCell::new(Vec::new()),
            output_label,
            output_input,
            browse_btn,
            settings_btn,
            run_btn,
            progress,
            log_label,
            log_list,
            body_divider,
            footer_bg,
            footer_line,
            hint_label,
            version_label,
            notice,
            settings_window,
            checks: RefCell::new(checks),
            check_keys: RefCell::new(check_keys),
            save_btn,
            open_btn,
            reload_btn,
            plugin_dirs,
            config_path,
            config: RefCell::new(config),
            actions: RefCell::new(Vec::new()),
            files: RefCell::new(Vec::new()),
            initial_action: RefCell::new(initial_action),
            job_rx: RefCell::new(None),
            job_active: RefCell::new(false),
            handlers: RefCell::new(Vec::new()),
        });

        bind_handlers(&app);
        app.set_files(files);
        app.refresh_actions();

        Ok(app)
    }

    fn on_event(&self, evt: nwg::Event, data: &nwg::EventData, handle: nwg::ControlHandle) {
        match evt {
            nwg::Event::OnWindowClose => {
                if &handle == &self.window {
                    nwg::stop_thread_dispatch();
                } else if &handle == &self.settings_window {
                    if let nwg::EventData::OnWindowClose(d) = data {
                        d.close(false);
                    }
                    self.settings_window.set_visible(false);
                    ui_style::control_to_top(&self.settings_btn.handle);
                }
            }
            nwg::Event::OnButtonClick => {
                if &handle == &self.run_btn {
                    self.start_job();
                } else if &handle == &self.browse_btn {
                    self.pick_output();
                } else if &handle == &self.settings_btn {
                    self.show_settings();
                } else if &handle == &self.save_btn {
                    self.save_settings();
                } else if &handle == &self.open_btn {
                    self.open_plugin_folder();
                } else if &handle == &self.reload_btn {
                    self.reload_plugins();
                }
            }
            nwg::Event::OnComboxBoxSelection => {
                if &handle == &self.action_combo.handle {
                    self.update_format_visibility();
                }
            }
            nwg::Event::OnNotice => self.drain_job(),
            _ => {}
        }
    }

    fn set_files(&self, files: Vec<PathBuf>) {
        let names: Vec<String> = files
            .iter()
            .map(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| clean_path(p))
            })
            .collect();
        self.files_list.set_collection(names);
        *self.files.borrow_mut() = files;
    }

    fn refresh_actions(&self) {
        let files = self.files.borrow().clone();
        let all = plugin::actions_many(&self.plugin_dirs);
        let config = self.config.borrow();

        let filtered: Vec<ActionEntry> = all
            .into_iter()
            .filter(|action| config.is_enabled(&action.key()))
            .filter(|action| {
                files.is_empty()
                    || files.iter().any(|file| {
                        file.extension()
                            .and_then(|e| e.to_str())
                            .map(|e| action.matches_extension(e))
                            .unwrap_or(false)
                    })
            })
            .collect();
        drop(config);

        let labels: Vec<String> = filtered
            .iter()
            .map(|a| format!("{} / {}", a.plugin.manifest.name, a.action.name))
            .collect();
        self.action_combo.set_collection(labels);

        let wanted = self.initial_action.borrow().clone();
        let selected = wanted
            .as_ref()
            .and_then(|key| filtered.iter().position(|a| &a.key() == key))
            .unwrap_or(0);
        if !filtered.is_empty() {
            self.action_combo.set_selection(Some(selected));
        } else {
            self.action_combo.set_selection(None);
        }

        let mut choices = Vec::new();
        let mut format_actions = Vec::new();
        for action in &filtered {
            if action.key() == "ffmpeg-other/convert" {
                continue;
            }
            choices.push(format!(
                "操作: {} / {}",
                action.plugin.manifest.name, action.action.name
            ));
            format_actions.push(Some(action.clone()));
        }
        for extension in ffmpeg_formats() {
            choices.push(extension);
            format_actions.push(None);
        }
        self.format_combo.set_collection(choices);
        let _ = self.format_combo.set_selection_string("mp4");
        *self.format_actions.borrow_mut() = format_actions;

        *self.actions.borrow_mut() = filtered;
        self.update_format_visibility();
    }

    fn update_format_visibility(&self) {
        let generic = self
            .selected_action()
            .map(|action| action.key() == "ffmpeg-other/convert")
            .unwrap_or(false);
        self.format_label.set_visible(generic);
        self.format_combo.set_visible(generic);
    }

    fn selected_action(&self) -> Option<ActionEntry> {
        let index = self.action_combo.selection()?;
        self.actions.borrow().get(index).cloned()
    }

    fn start_job(&self) {
        if *self.job_active.borrow() {
            return;
        }
        let Some(action) = self.selected_action() else {
            self.log_line("操作が選ばれていません。");
            return;
        };
        let files = self.files.borrow().clone();
        if files.is_empty() {
            self.log_line("ファイルが選ばれていません。");
            return;
        }

        let (action, target_format) = if action.key() == "ffmpeg-other/convert" {
            let selected_action = self
                .format_combo
                .selection()
                .and_then(|index| self.format_actions.borrow().get(index).cloned().flatten());
            if let Some(selected) = selected_action {
                (selected, None)
            } else {
                let format = match self.format_combo.selection_string() {
                    Some(value) if !value.trim().is_empty() => value,
                    _ => {
                        self.log_line("出力形式が選ばれていません。");
                        return;
                    }
                };
                (action, Some(format))
            }
        } else {
            (action, None)
        };

        let out_text = self.output_input.text();
        let out_text = out_text.trim().to_string();
        let output = if out_text.is_empty() {
            None
        } else {
            Some(PathBuf::from(out_text.clone()))
        };

        {
            let mut config = self.config.borrow_mut();
            config.output_dir = if out_text.is_empty() {
                None
            } else {
                Some(out_text)
            };
            let _ = config.save(&self.config_path);
        }

        *self.job_active.borrow_mut() = true;
        self.run_btn.set_enabled(false);
        self.progress.set_pos(0);
        self.log_list.clear();
        self.log_line(&format!("開始: {}", action.action.name));

        let rx = spawn_job(&action, files, output, target_format, self.notice.sender());
        *self.job_rx.borrow_mut() = Some(rx);
    }

    fn drain_job(&self) {
        let mut events = Vec::new();
        let mut done = None;
        {
            let rx = self.job_rx.borrow();
            if let Some(rx) = rx.as_ref() {
                for msg in rx.try_iter() {
                    match msg {
                        Msg::Event(event) => events.push(event),
                        Msg::Done(result) => done = Some(result),
                    }
                }
            }
        }

        for event in events {
            match event {
                JobEvent::Log(message) => self.log_line(&message),
                JobEvent::Progress(value) => self.progress.set_pos((value * 100.0) as u32),
                JobEvent::FileDone { input, output } => {
                    self.log_line(&format!(
                        "{} -> {}",
                        clean_path(&input),
                        clean_path(&output)
                    ));
                }
            }
        }

        if let Some(result) = done {
            self.job_rx.borrow_mut().take();
            *self.job_active.borrow_mut() = false;
            self.run_btn.set_enabled(true);
            match result {
                Ok(()) => {
                    self.progress.set_pos(100);
                    self.log_line("完了");
                }
                Err(message) => {
                    self.log_line(&format!("エラー: {message}"));
                }
            }
        }
    }

    fn pick_output(&self) {
        let mut dialog = nwg::FileDialog::default();
        let built = nwg::FileDialog::builder()
            .title("出力先フォルダ")
            .action(nwg::FileDialogAction::OpenDirectory)
            .build(&mut dialog);
        if built.is_err() {
            self.log_line("フォルダ選択を開けませんでした。");
            return;
        }
        if dialog.run(Some(&self.window)) {
            if let Ok(item) = dialog.get_selected_item() {
                self.output_input.set_text(&item.to_string_lossy());
            }
        }
    }

    fn show_settings(&self) {
        self.settings_window.set_visible(true);
        self.settings_window.set_focus();
        ui_style::bring_to_front(&self.settings_window);
    }

    fn open_plugin_folder(&self) {
        let dir = paths::user_plugin_dir();
        let _ = std::fs::create_dir_all(&dir);
        let path = clean_path(&dir);
        let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
    }

    fn reload_plugins(&self) {
        self.checks.borrow_mut().clear();
        let all = plugin::actions_many(&self.plugin_dirs);
        let mut checks = Vec::new();
        let mut keys = Vec::new();
        let config = self.config.borrow();
        for (index, action) in all.iter().take(80).enumerate() {
            let key = action.key();
            let label = format!("{} / {}", action.plugin.manifest.name, action.action.name);
            let mut check = nwg::CheckBox::default();
            match nwg::CheckBox::builder()
                .text(&label)
                .position((10 + (index as i32 % 4) * 195, 10 + (index as i32 / 4) * 22))
                .size((185, 20))
                .parent(&self.settings_window)
                .font(Some(&self.body_font))
                .background_color(Some([255, 255, 255]))
                .check_state(if config.is_enabled(&key) {
                    nwg::CheckBoxState::Checked
                } else {
                    nwg::CheckBoxState::Unchecked
                })
                .build(&mut check)
            {
                Ok(()) => {
                    checks.push(check);
                    keys.push(key);
                }
                Err(error) => {
                    self.log_line(&format!("プラグイン一覧を作れません: {error}"));
                    break;
                }
            }
        }
        drop(config);
        *self.checks.borrow_mut() = checks;
        *self.check_keys.borrow_mut() = keys;
        self.refresh_actions();
        self.log_line("プラグインを再読み込みしました。");
    }

    fn save_settings(&self) {
        {
            let checks = self.checks.borrow();
            let keys = self.check_keys.borrow();
            let mut config = self.config.borrow_mut();
            for (index, check) in checks.iter().enumerate() {
                if let Some(key) = keys.get(index) {
                    config.set_enabled(key, check.check_state() == nwg::CheckBoxState::Checked);
                }
            }
            if let Err(e) = config.save(&self.config_path) {
                self.log_line(&format!("設定を保存できません: {e}"));
            }
        }
        self.refresh_actions();
        self.settings_window.set_visible(false);
        ui_style::control_to_top(&self.settings_btn.handle);
        self.log_line("設定を保存しました。");
    }

    fn log_line(&self, message: &str) {
        self.log_list.push(message.to_string());
        let len = self.log_list.len();
        if len > 0 {
            self.log_list.set_selection(Some(len - 1));
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        for handler in self.handlers.borrow().iter() {
            nwg::unbind_event_handler(handler);
        }
    }
}

#[allow(dead_code)]
struct ProgressApp {
    window: nwg::Window,
    title_font: nwg::Font,
    body_font: nwg::Font,
    title_label: nwg::Label,
    status_label: nwg::Label,
    progress: nwg::ProgressBar,
    close_btn: nwg::Button,
    notice: nwg::Notice,
    rx: RefCell<Option<Receiver<Msg>>>,
    done: Cell<bool>,
    close_pending: Cell<bool>,
    handlers: RefCell<Vec<nwg::EventHandler>>,
}

impl ProgressApp {
    fn new(
        action: ActionEntry,
        files: Vec<PathBuf>,
        output: Option<PathBuf>,
    ) -> Result<Rc<ProgressApp>, String> {
        let err = |e: nwg::NwgError| format!("failed to build GUI: {e}");

        let mut title_font = nwg::Font::default();
        nwg::Font::builder()
            .family("Yu Gothic UI")
            .size(20)
            .weight(700)
            .build(&mut title_font)
            .map_err(err)?;

        let mut body_font = nwg::Font::default();
        nwg::Font::builder()
            .family("Yu Gothic UI")
            .size(15)
            .build(&mut body_font)
            .map_err(err)?;

        let mut window = nwg::Window::default();
        nwg::Window::builder()
            .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::VISIBLE)
            .size((440, 160))
            .position(ui_style::center(440, 160))
            .title("変換中")
            .build(&mut window)
            .map_err(err)?;

        let mut header_bg = nwg::Label::default();
        nwg::Label::builder()
            .text("")
            .position((0, 0))
            .size((900, 58))
            .parent(&window)
            .background_color(Some([255, 255, 255]))
            .build(&mut header_bg)
            .map_err(err)?;

        let mut accent_bar = nwg::Label::default();
        nwg::Label::builder()
            .text("")
            .position((16, 16))
            .size((4, 26))
            .parent(&window)
            .background_color(Some(ui_style::accent_rgb()))
            .build(&mut accent_bar)
            .map_err(err)?;

        let mut header_line = nwg::Label::default();
        nwg::Label::builder()
            .text("")
            .position((0, 57))
            .size((900, 1))
            .parent(&window)
            .background_color(Some([215, 215, 215]))
            .build(&mut header_line)
            .map_err(err)?;

        let mut title_label = nwg::Label::default();
        nwg::Label::builder()
            .text(&format!(
                "{} / {}",
                action.plugin.manifest.name, action.action.name
            ))
            .position((16, 14))
            .size((408, 28))
            .parent(&window)
            .font(Some(&title_font))
            .background_color(Some(ui_style::BG_RGB))
            .build(&mut title_label)
            .map_err(err)?;

        let first_name = files
            .first()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("ファイル");
        let status_text = format!("変換中: {first_name}");

        let mut status_label = nwg::Label::default();
        nwg::Label::builder()
            .text(&status_text)
            .position((16, 48))
            .size((408, 24))
            .parent(&window)
            .font(Some(&body_font))
            .background_color(Some(ui_style::BG_RGB))
            .build(&mut status_label)
            .map_err(err)?;

        let mut progress = nwg::ProgressBar::default();
        nwg::ProgressBar::builder()
            .range(0..100)
            .position((16, 78))
            .size((408, 14))
            .parent(&window)
            .build(&mut progress)
            .map_err(err)?;

        let mut close_btn = nwg::Button::default();
        nwg::Button::builder()
            .text("キャンセル")
            .position((16, 102))
            .size((408, 36))
            .parent(&window)
            .font(Some(&body_font))
            .build(&mut close_btn)
            .map_err(err)?;

        let mut notice = nwg::Notice::default();
        nwg::Notice::builder()
            .parent(&window)
            .build(&mut notice)
            .map_err(err)?;

        ui_style::apply_chrome(&window);
        ui_style::marquee(&progress, true);

        let app = Rc::new(ProgressApp {
            window,
            title_font,
            body_font,
            title_label,
            status_label,
            progress,
            close_btn,
            notice,
            rx: RefCell::new(None),
            done: Cell::new(false),
            close_pending: Cell::new(false),
            handlers: RefCell::new(Vec::new()),
        });

        bind_progress_handlers(&app);

        let rx = spawn_job(&action, files, output, None, app.notice.sender());
        *app.rx.borrow_mut() = Some(rx);

        Ok(app)
    }

    fn on_event(&self, evt: nwg::Event, _data: &nwg::EventData, handle: nwg::ControlHandle) {
        match evt {
            nwg::Event::OnWindowClose => {
                if &handle == &self.window {
                    nwg::stop_thread_dispatch();
                }
            }
            nwg::Event::OnButtonClick => {
                if &handle == &self.close_btn {
                    nwg::stop_thread_dispatch();
                }
            }
            nwg::Event::OnNotice => self.drain(),
            _ => {}
        }
    }

    fn drain(&self) {
        let mut events = Vec::new();
        let mut done = None;
        {
            let rx = self.rx.borrow();
            if let Some(rx) = rx.as_ref() {
                for msg in rx.try_iter() {
                    match msg {
                        Msg::Event(event) => events.push(event),
                        Msg::Done(result) => done = Some(result),
                    }
                }
            }
        }

        for event in events {
            match event {
                JobEvent::Log(message) => self.status_label.set_text(&short(&message, 52)),
                JobEvent::Progress(value) => self.progress.set_pos((value * 100.0) as u32),
                JobEvent::FileDone { output, .. } => {
                    let name = output
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("出力");
                    self.status_label.set_text(&format!("保存: {name}"));
                }
            }
        }

        if let Some(result) = done {
            self.rx.borrow_mut().take();
            self.done.set(true);
            ui_style::marquee(&self.progress, false);
            match result {
                Ok(()) => {
                    self.progress.set_pos(100);
                    self.status_label.set_text("完了");
                    self.close_btn.set_text("閉じる");
                    self.close_pending.set(true);
                    let sender = self.notice.sender();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(650));
                        sender.notice();
                    });
                }
                Err(message) => {
                    self.status_label
                        .set_text(&short(&format!("エラー: {message}"), 60));
                    self.close_btn.set_text("閉じる");
                    self.close_btn.set_focus();
                }
            }
        }

        if self.done.get() && self.close_pending.get() {
            self.window.close();
            nwg::stop_thread_dispatch();
        }
    }
}

impl Drop for ProgressApp {
    fn drop(&mut self) {
        for handler in self.handlers.borrow().iter() {
            nwg::unbind_event_handler(handler);
        }
    }
}

fn spawn_job(
    action: &ActionEntry,
    files: Vec<PathBuf>,
    output: Option<PathBuf>,
    target_format: Option<String>,
    notice: nwg::NoticeSender,
) -> Receiver<Msg> {
    let (tx, rx) = mpsc::channel();
    let emit_tx = tx.clone();
    let emit: Emit = Arc::new(move |event| {
        let _ = emit_tx.send(Msg::Event(event));
        notice.notice();
    });

    let action = action.clone();
    std::thread::spawn(move || {
        let result = if action.key() == "ffmpeg-other/convert" {
            engine::run_ffmpeg_other(
                &action,
                &files,
                output,
                target_format.as_deref().unwrap_or("mp4"),
                emit,
            )
        } else {
            engine::run_job(&action, &files, output, emit)
        };
        let _ = tx.send(Msg::Done(result.map_err(|e| format!("{e:#}"))));
        notice.notice();
    });

    rx
}

fn bind_handlers(app: &Rc<App>) {
    let weak = Rc::downgrade(app);
    let handler = nwg::full_bind_event_handler(&app.window.handle, move |evt, data, handle| {
        if let Some(app) = weak.upgrade() {
            app.on_event(evt, &data, handle);
        }
    });
    app.handlers.borrow_mut().push(handler);

    let weak = Rc::downgrade(app);
    let handler =
        nwg::full_bind_event_handler(&app.settings_window.handle, move |evt, data, handle| {
            if let Some(app) = weak.upgrade() {
                app.on_event(evt, &data, handle);
            }
        });
    app.handlers.borrow_mut().push(handler);
}

fn bind_progress_handlers(app: &Rc<ProgressApp>) {
    let weak = Rc::downgrade(app);
    let handler = nwg::full_bind_event_handler(&app.window.handle, move |evt, data, handle| {
        if let Some(app) = weak.upgrade() {
            app.on_event(evt, &data, handle);
        }
    });
    app.handlers.borrow_mut().push(handler);
}

fn build_settings(
    plugin_dirs: &[PathBuf],
    config: &Config,
    body_font: &nwg::Font,
) -> Result<
    (
        nwg::Window,
        Vec<nwg::CheckBox>,
        Vec<String>,
        nwg::Button,
        nwg::Button,
        nwg::Button,
    ),
    String,
> {
    let all = plugin::actions_many(plugin_dirs);

    let mut window = nwg::Window::default();
    nwg::Window::builder()
        .flags(nwg::WindowFlags::WINDOW)
        .size((800, 540))
        .position((280, 150))
        .title("プラグインの有効化")
        .build(&mut window)
        .map_err(|e| format!("failed to build settings window: {e}"))?;

    let mut checks = Vec::new();
    let mut keys = Vec::new();
    for (index, action) in all.iter().take(80).enumerate() {
        let key = action.key();
        let label = format!("{} / {}", action.plugin.manifest.name, action.action.name);
        let mut check = nwg::CheckBox::default();
        nwg::CheckBox::builder()
            .text(&label)
            .position((10 + (index as i32 % 4) * 195, 10 + (index as i32 / 4) * 22))
            .size((185, 20))
            .parent(&window)
            .font(Some(body_font))
            .background_color(Some([255, 255, 255]))
            .check_state(if config.is_enabled(&key) {
                nwg::CheckBoxState::Checked
            } else {
                nwg::CheckBoxState::Unchecked
            })
            .build(&mut check)
            .map_err(|e| format!("failed to build checkbox: {e}"))?;
        checks.push(check);
        keys.push(key);
    }

    let mut save_btn = nwg::Button::default();
    nwg::Button::builder()
        .text("保存")
        .position((12, 480))
        .size((240, 32))
        .parent(&window)
        .font(Some(body_font))
        .build(&mut save_btn)
        .map_err(|e| format!("failed to build button: {e}"))?;

    let mut open_btn = nwg::Button::default();
    nwg::Button::builder()
        .text("フォルダを開く")
        .position((270, 480))
        .size((240, 32))
        .parent(&window)
        .font(Some(body_font))
        .build(&mut open_btn)
        .map_err(|e| format!("failed to build button: {e}"))?;

    let mut reload_btn = nwg::Button::default();
    nwg::Button::builder()
        .text("再読み込み")
        .position((528, 480))
        .size((240, 32))
        .parent(&window)
        .font(Some(body_font))
        .build(&mut reload_btn)
        .map_err(|e| format!("failed to build button: {e}"))?;

    Ok((window, checks, keys, save_btn, open_btn, reload_btn))
}

fn ffmpeg_formats() -> Vec<String> {
    include_str!("ffmpeg_formats.txt")
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn short(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

fn clean_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}
