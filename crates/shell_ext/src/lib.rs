#![allow(non_snake_case)]

use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use extension_change_core::{config::Config, paths, plugin};
use windows::core::{implement, IUnknown, Interface, Ref, BOOL, GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, E_NOTIMPL, E_POINTER, HINSTANCE, HMODULE, S_FALSE, S_OK,
};
use windows::Win32::System::Com::{
    CoTaskMemAlloc, CoTaskMemFree, IBindCtx, IClassFactory, IClassFactory_Impl,
};
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows::Win32::UI::Shell::{
    IEnumExplorerCommand, IEnumExplorerCommand_Impl, IExplorerCommand, IExplorerCommand_Impl,
    IShellItemArray, SIGDN_FILESYSPATH,
};

const CLSID_ROOT: GUID = GUID::from_u128(0x1f3d8c2a_9e4b_4b7a_8c2d_6e5f4a3b2c10);
const ECS_ENABLED: u32 = 0;
const ECS_HIDDEN: u32 = 2;
const ECF_HASSUBCOMMANDS: u32 = 1;

const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff", "ico", "heic", "heif",
];
const VIDEO_EXTS: &[&str] = &[
    "mp4", "mkv", "mov", "avi", "webm", "m4v", "wmv", "flv", "ts", "mts", "m2ts", "hevc", "265",
    "h265", "av1", "ivf",
];
const AUDIO_EXTS: &[&str] = &[
    "mp3", "wav", "flac", "ogg", "oga", "m4a", "aac", "opus", "wma", "aiff", "aif",
];
const FONT_EXTS: &[&str] = &["ttf", "otf", "woff", "woff2"];
const SCHEMATIC_EXTS: &[&str] = &["litematic", "schem", "schematic", "nbt", "bp", "axiom"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum SelectionKind {
    Audio,
    Font,
    Image,
    Video,
    Schematic,
    Other,
}

#[derive(Clone)]
struct ActionSpec {
    key: String,
    title: String,
    extensions: Vec<String>,
}

#[implement(IExplorerCommand)]
struct RootCommand;

impl IExplorerCommand_Impl for RootCommand_Impl {
    fn GetTitle(&self, items: Ref<IShellItemArray>) -> windows::core::Result<PWSTR> {
        let title = match selected_kind(items) {
            SelectionKind::Audio => "音声を変換",
            SelectionKind::Font => "フォントを変換",
            SelectionKind::Image => "画像を変換",
            SelectionKind::Video => "動画を変換",
            SelectionKind::Schematic => "構造物を変換",
            SelectionKind::Other => "変換",
        };
        Ok(to_pwstr(title))
    }

    fn GetIcon(&self, _items: Ref<IShellItemArray>) -> windows::core::Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    fn GetToolTip(&self, _items: Ref<IShellItemArray>) -> windows::core::Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    fn GetCanonicalName(&self) -> windows::core::Result<GUID> {
        Ok(CLSID_ROOT)
    }

    fn GetState(&self, items: Ref<IShellItemArray>, _slow: BOOL) -> windows::core::Result<u32> {
        let files = paths_from_items(items);
        if files.is_empty() {
            return Ok(ECS_ENABLED);
        }
        let actions = enabled_actions();
        if actions.iter().any(|action| {
            files
                .iter()
                .any(|file| matches_extension(file, &action.extensions))
        }) {
            Ok(ECS_ENABLED)
        } else {
            Ok(ECS_HIDDEN)
        }
    }

    fn Invoke(
        &self,
        items: Ref<IShellItemArray>,
        _bind: Ref<IBindCtx>,
    ) -> windows::core::Result<()> {
        let files = paths_from_items(items);
        launch_host(&files, None);
        Ok(())
    }

    fn GetFlags(&self) -> windows::core::Result<u32> {
        Ok(ECF_HASSUBCOMMANDS)
    }

    fn EnumSubCommands(&self) -> windows::core::Result<IEnumExplorerCommand> {
        let items = enabled_actions();
        let enumerator = EnumCommands {
            items: Arc::new(items),
            index: AtomicUsize::new(0),
        };
        Ok(enumerator.into())
    }
}

#[implement(IExplorerCommand)]
struct ActionCommand {
    spec: ActionSpec,
}

impl IExplorerCommand_Impl for ActionCommand_Impl {
    fn GetTitle(&self, _items: Ref<IShellItemArray>) -> windows::core::Result<PWSTR> {
        Ok(to_pwstr(&self.spec.title))
    }

    fn GetIcon(&self, _items: Ref<IShellItemArray>) -> windows::core::Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    fn GetToolTip(&self, _items: Ref<IShellItemArray>) -> windows::core::Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    fn GetCanonicalName(&self) -> windows::core::Result<GUID> {
        Ok(guid_for(&self.spec.key))
    }

    fn GetState(&self, items: Ref<IShellItemArray>, _slow: BOOL) -> windows::core::Result<u32> {
        let files = paths_from_items(items);
        if files.is_empty() {
            return Ok(ECS_ENABLED);
        }
        if files
            .iter()
            .any(|file| matches_extension(file, &self.spec.extensions))
        {
            Ok(ECS_ENABLED)
        } else {
            Ok(ECS_HIDDEN)
        }
    }

    fn Invoke(
        &self,
        items: Ref<IShellItemArray>,
        _bind: Ref<IBindCtx>,
    ) -> windows::core::Result<()> {
        let files: Vec<String> = paths_from_items(items)
            .into_iter()
            .filter(|file| matches_extension(file, &self.spec.extensions))
            .collect();
        if !files.is_empty() {
            launch_host(&files, Some(&self.spec.key));
        }
        Ok(())
    }

    fn GetFlags(&self) -> windows::core::Result<u32> {
        Ok(0)
    }

    fn EnumSubCommands(&self) -> windows::core::Result<IEnumExplorerCommand> {
        Err(windows::Win32::Foundation::E_NOTIMPL.into())
    }
}

#[implement(IEnumExplorerCommand)]
struct EnumCommands {
    items: Arc<Vec<ActionSpec>>,
    index: AtomicUsize,
}

impl IEnumExplorerCommand_Impl for EnumCommands_Impl {
    fn Next(
        &self,
        celt: u32,
        commands: *mut Option<IExplorerCommand>,
        fetched: *mut u32,
    ) -> HRESULT {
        if commands.is_null() {
            return E_POINTER;
        }

        let start = self.index.load(Ordering::SeqCst);
        let mut count = 0usize;
        for offset in 0..celt as usize {
            let Some(spec) = self.items.get(start + offset).cloned() else {
                break;
            };
            let command: IExplorerCommand = ActionCommand { spec }.into();
            unsafe {
                commands.add(offset).write(Some(command));
            }
            count += 1;
        }

        if !fetched.is_null() {
            unsafe {
                *fetched = count as u32;
            }
        }
        self.index.fetch_add(count, Ordering::SeqCst);

        if count == celt as usize {
            S_OK
        } else {
            S_FALSE
        }
    }

    fn Skip(&self, celt: u32) -> windows::core::Result<()> {
        let start = self.index.load(Ordering::SeqCst);
        let next = (start + celt as usize).min(self.items.len());
        self.index.store(next, Ordering::SeqCst);
        Ok(())
    }

    fn Reset(&self) -> windows::core::Result<()> {
        self.index.store(0, Ordering::SeqCst);
        Ok(())
    }

    fn Clone(&self) -> windows::core::Result<IEnumExplorerCommand> {
        let clone = EnumCommands {
            items: self.items.clone(),
            index: AtomicUsize::new(self.index.load(Ordering::SeqCst)),
        };
        Ok(clone.into())
    }
}

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(
        &self,
        _outer: Ref<IUnknown>,
        riid: *const GUID,
        object: *mut *mut c_void,
    ) -> windows::core::Result<()> {
        if object.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            *object = std::ptr::null_mut();
        }
        let command: IExplorerCommand = RootCommand.into();
        let hr = unsafe { command.query(riid, object) };
        hr.ok()
    }

    fn LockServer(&self, _lock: BOOL) -> windows::core::Result<()> {
        Ok(())
    }
}

#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(
    clsid: *const GUID,
    riid: *const GUID,
    object: *mut *mut c_void,
) -> HRESULT {
    if object.is_null() {
        return E_POINTER;
    }
    unsafe {
        *object = std::ptr::null_mut();
    }
    if clsid.is_null() || unsafe { *clsid } != CLSID_ROOT {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = Factory.into();
    unsafe { factory.query(riid, object) }
}

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}

#[no_mangle]
pub extern "system" fn DllMain(_instance: HINSTANCE, _reason: u32, _reserved: *mut c_void) -> BOOL {
    BOOL(1)
}

fn enabled_actions() -> Vec<ActionSpec> {
    let Some(dir) = module_dir() else {
        return Vec::new();
    };
    let plugin_dirs = paths::plugin_dirs(&dir);
    let config = Config::load(&paths::config_path());
    plugin::actions_many(&plugin_dirs)
        .into_iter()
        .filter(|action| config.is_enabled(&action.key()))
        .map(|action| ActionSpec {
            key: action.key(),
            title: action.action.name.clone(),
            extensions: action.plugin.manifest.extensions.clone(),
        })
        .collect()
}

fn module_dir() -> Option<PathBuf> {
    unsafe {
        let mut module = HMODULE::default();
        let address = module_dir as *const u16;
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(address),
            &mut module,
        )
        .ok()?;

        let mut buffer = [0u16; 32768];
        let length = GetModuleFileNameW(Some(module), &mut buffer) as usize;
        if length == 0 {
            return None;
        }
        let path = PathBuf::from(String::from_utf16_lossy(&buffer[..length]));
        path.parent().map(|parent| parent.to_path_buf())
    }
}

fn launch_host(files: &[String], action: Option<&str>) {
    let Some(dir) = module_dir() else {
        return;
    };
    let host = dir.join("ExtensionChange.exe");
    if !host.is_file() {
        return;
    }

    let mut command = std::process::Command::new(&host);
    command.current_dir(&dir);
    for file in files {
        command.arg("--file").arg(file);
    }
    if let Some(action) = action {
        command.arg("--action").arg(action).arg("--run");
    }
    command.creation_flags(0x0800_0000);
    let _ = command.spawn();
}

fn paths_from_items(items: Ref<IShellItemArray>) -> Vec<String> {
    let Some(items) = items.as_ref() else {
        return Vec::new();
    };
    let count = unsafe { items.GetCount() }.unwrap_or(0);
    let mut files = Vec::new();
    for index in 0..count {
        let Ok(item) = (unsafe { items.GetItemAt(index) }) else {
            continue;
        };
        let Ok(name) = (unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }) else {
            continue;
        };
        if let Ok(path) = unsafe { name.to_string() } {
            files.push(path);
        }
        unsafe {
            CoTaskMemFree(Some(name.0 as *const c_void));
        }
    }
    files
}

fn selected_kind(items: Ref<IShellItemArray>) -> SelectionKind {
    let files = paths_from_items(items);
    if files.is_empty() {
        return SelectionKind::Other;
    }
    let mut all_audio = true;
    let mut all_font = true;
    let mut all_image = true;
    let mut all_schematic = true;
    let mut all_video = true;

    for file in &files {
        let ext = Path::new(file)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        all_audio &= AUDIO_EXTS.contains(&ext.as_str());
        all_font &= FONT_EXTS.contains(&ext.as_str());
        all_image &= IMAGE_EXTS.contains(&ext.as_str());
        all_schematic &= SCHEMATIC_EXTS.contains(&ext.as_str());
        all_video &= VIDEO_EXTS.contains(&ext.as_str());
    }
    if all_audio {
        SelectionKind::Audio
    } else if all_font {
        SelectionKind::Font
    } else if all_image {
        SelectionKind::Image
    } else if all_video {
        SelectionKind::Video
    } else if all_schematic {
        SelectionKind::Schematic
    } else {
        SelectionKind::Other
    }
}

fn matches_extension(path: &str, extensions: &[String]) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            extensions
                .iter()
                .any(|known| known.eq_ignore_ascii_case(ext))
        })
        .unwrap_or(false)
}

fn guid_for(key: &str) -> GUID {
    let mut a = 0xcbf29ce484222325u64;
    let mut b = 0x9e3779b97f4a7c15u64;
    for byte in key.bytes() {
        a ^= byte as u64;
        a = a.wrapping_mul(0x100000001b3);
        b = b.rotate_left(5) ^ (byte as u64).wrapping_mul(0x9e3779b97f4a7c15);
    }
    GUID::from_u128(((a as u128) << 64) | b as u128)
}

fn to_pwstr(value: &str) -> PWSTR {
    let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let memory = CoTaskMemAlloc(wide.len() * std::mem::size_of::<u16>()) as *mut u16;
        if memory.is_null() {
            return PWSTR(std::ptr::null_mut());
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), memory, wide.len());
        PWSTR(memory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn com_root_and_subcommands() {
        std::env::set_var(
            "EXTENSION_CHANGE_PLUGINS",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../plugins"),
        );

        unsafe {
            let mut factory_raw = std::ptr::null_mut();
            let hr = DllGetClassObject(&CLSID_ROOT, &IClassFactory::IID, &mut factory_raw);
            assert!(hr.is_ok(), "DllGetClassObject failed: {hr:?}");
            let factory = IClassFactory::from_raw(factory_raw);

            let command: IExplorerCommand = factory
                .CreateInstance(None::<&IUnknown>)
                .expect("CreateInstance failed");

            let title = command
                .GetTitle(None::<&IShellItemArray>)
                .expect("GetTitle failed")
                .to_string()
                .expect("title is not valid UTF-16");
            assert_eq!(title, "変換");
            assert_ne!(
                command.GetFlags().expect("GetFlags failed") & ECF_HASSUBCOMMANDS,
                0
            );

            let enumerator = command.EnumSubCommands().expect("EnumSubCommands failed");
            let mut items: [Option<IExplorerCommand>; 1] = [None];
            let hr = enumerator.Next(&mut items, None);
            assert!(hr.is_ok());

            let sub = items[0].take().expect("no subcommand");
            let sub_title = sub
                .GetTitle(None::<&IShellItemArray>)
                .expect("GetTitle failed")
                .to_string()
                .expect("title is not valid UTF-16");
            assert!(!sub_title.is_empty());
        }
    }

    #[test]
    fn root_title_and_state_by_type() {
        use windows::core::HSTRING;
        use windows::Win32::System::Com::IBindCtx;
        use windows::Win32::UI::Shell::{
            IShellItem, SHCreateItemFromParsingName, SHCreateShellItemArrayFromShellItem,
        };

        unsafe fn array(path: &Path) -> IShellItemArray {
            let path = HSTRING::from(path.to_string_lossy().as_ref());
            let item: IShellItem =
                SHCreateItemFromParsingName(&path, None::<&IBindCtx>).expect("shell item");
            SHCreateShellItemArrayFromShellItem(&item).expect("shell item array")
        }

        let dir = std::env::temp_dir().join("extension_change_kind");
        let _ = std::fs::create_dir_all(&dir);
        let png = dir.join("a.png");
        let mp4 = dir.join("a.mp4");
        let ico = dir.join("a.ico");
        let txt = dir.join("a.txt");
        for path in [&png, &mp4, &ico, &txt] {
            let _ = std::fs::write(path, b"");
        }

        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            );
            let command: IExplorerCommand = RootCommand.into();
            let title = |path: &Path| {
                command
                    .GetTitle(Some(&array(path)))
                    .expect("GetTitle")
                    .to_string()
                    .expect("utf16")
            };
            assert_eq!(title(&png), "画像を変換");
            assert_eq!(title(&mp4), "動画を変換");
            assert_eq!(title(&ico), "画像を変換");
            assert_eq!(title(&txt), "変換");

            let state = |path: &Path| {
                command
                    .GetState(Some(&array(path)), false)
                    .expect("GetState")
            };
            assert_eq!(state(&png), ECS_ENABLED);
            assert_eq!(state(&txt), ECS_HIDDEN);
            windows::Win32::System::Com::CoUninitialize();
        }
    }
}
