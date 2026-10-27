#![allow(non_snake_case)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use extension_change_core::{config::Config, paths, plugin};
use windows::core::{implement, IUnknown, Interface, PSTR, Ref, BOOL, GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, E_INVALIDARG, E_NOTIMPL, E_POINTER, HINSTANCE, HMODULE, S_FALSE, S_OK,
};
use windows::Win32::System::Com::{
    CoTaskMemAlloc, CoTaskMemFree, IDataObject, IBindCtx, IClassFactory, IClassFactory_Impl,
    DVASPECT_CONTENT, FORMATETC, STGMEDIUM, TYMED_HGLOBAL,
};
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows::Win32::System::Ole::ReleaseStgMedium;
use windows::Win32::System::Registry::HKEY;
use windows::Win32::UI::Shell::{
    DragQueryFileW, HDROP, IContextMenu, IContextMenu_Impl, IEnumExplorerCommand,
    IEnumExplorerCommand_Impl, IExplorerCommand, IExplorerCommand_Impl, IShellExtInit,
    IShellExtInit_Impl, IShellItemArray, CMINVOKECOMMANDINFO, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, InsertMenuW, HMENU, MF_BYPOSITION, MF_POPUP, MF_STRING,
};

const CLSID_ROOT: GUID = GUID::from_u128(0x1f3d8c2a_9e4b_4b7a_8c2d_6e5f4a3b2c10);
const ECS_ENABLED: u32 = 0;
const ECS_HIDDEN: u32 = 2;
const ECF_HASSUBCOMMANDS: u32 = 1;

#[derive(Clone)]
struct ActionSpec {
    key: String,
    title: String,
    extensions: Vec<String>,
    category: String,
}

#[derive(Clone)]
struct CategorySpec {
    title: String,
    actions: Vec<ActionSpec>,
}

#[implement(IExplorerCommand, IContextMenu, IShellExtInit)]
struct RootCommand {
    files: RefCell<Vec<String>>,
    commands: RefCell<HashMap<u32, String>>,
}

impl RootCommand {
    fn new() -> Self {
        Self {
            files: RefCell::new(Vec::new()),
            commands: RefCell::new(HashMap::new()),
        }
    }
}

impl IExplorerCommand_Impl for RootCommand_Impl {
    fn GetTitle(&self, _items: Ref<IShellItemArray>) -> windows::core::Result<PWSTR> {
        Ok(to_pwstr("変換"))
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
        let items = enabled_categories();
        let enumerator = EnumCategories {
            items: Arc::new(items),
            index: AtomicUsize::new(0),
        };
        Ok(enumerator.into())
    }
}

impl IShellExtInit_Impl for RootCommand_Impl {
    fn Initialize(
        &self,
        _pidl_folder: *const windows::Win32::UI::Shell::Common::ITEMIDLIST,
        pdtobj: Ref<IDataObject>,
        _hkey: HKEY,
    ) -> windows::core::Result<()> {
        let Some(data) = pdtobj.as_ref() else {
            return Ok(());
        };
        let format = FORMATETC {
            cfFormat: 15,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        let mut medium = unsafe { data.GetData(&format)? };
        let hdrop: HDROP = unsafe { std::mem::transmute(medium.u.hGlobal) };
        let count = unsafe { DragQueryFileW(hdrop, u32::MAX, None) };
        for index in 0..count {
            let length = unsafe { DragQueryFileW(hdrop, index, None) };
            let mut buffer = vec![0u16; length as usize + 1];
            let copied = unsafe { DragQueryFileW(hdrop, index, Some(&mut buffer)) };
            if copied > 0 {
                buffer.truncate(copied as usize);
                self.files
                    .borrow_mut()
                    .push(String::from_utf16_lossy(&buffer));
            }
        }
        unsafe {
            ReleaseStgMedium(&mut medium);
        }
        Ok(())
    }
}

impl IContextMenu_Impl for RootCommand_Impl {
    fn QueryContextMenu(
        &self,
        hmenu: HMENU,
        indexmenu: u32,
        idcmdfirst: u32,
        idcmdlast: u32,
        uflags: u32,
    ) -> HRESULT {
        if uflags & 0x0000_0001 != 0 {
            return S_FALSE;
        }

        let files = self.files.borrow();
        if files.is_empty() {
            return S_FALSE;
        }

        let Ok(root) = (unsafe { CreatePopupMenu() }) else {
            return S_FALSE;
        };
        let mut commands = self.commands.borrow_mut();
        commands.clear();

        let mut next_id = idcmdfirst;
        let mut category_count = 0usize;
        for category in enabled_categories() {
            let actions: Vec<ActionSpec> = category
                .actions
                .into_iter()
                .filter(|action| {
                    files
                        .iter()
                        .any(|file| matches_extension(file, &action.extensions))
                })
                .collect();
            if actions.is_empty() {
                continue;
            }

            let Ok(submenu) = (unsafe { CreatePopupMenu() }) else {
                continue;
            };
            let mut action_count = 0usize;
            for action in actions {
                if next_id > idcmdlast {
                    break;
                }
                let title = wide(&action.title);
                unsafe {
                    AppendMenuW(
                        submenu,
                        MF_STRING,
                        next_id as usize,
                        PCWSTR(title.as_ptr()),
                    );
                }
                commands.insert(next_id, action.key);
                next_id += 1;
                action_count += 1;
            }
            if action_count == 0 {
                unsafe {
                    DestroyMenu(submenu);
                }
                continue;
            }

            let title = wide(&category.title);
            unsafe {
                AppendMenuW(root, MF_POPUP, submenu.0 as usize, PCWSTR(title.as_ptr()));
            }
            category_count += 1;
        }

        if category_count == 0 {
            unsafe {
                DestroyMenu(root);
            }
            return S_FALSE;
        }

        let title = wide("変換");
        unsafe {
            InsertMenuW(
                hmenu,
                indexmenu,
                MF_BYPOSITION | MF_POPUP,
                root.0 as usize,
                PCWSTR(title.as_ptr()),
            );
        }
        S_OK
    }

    fn InvokeCommand(&self, pici: *const CMINVOKECOMMANDINFO) -> windows::core::Result<()> {
        if pici.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let info = unsafe { &*pici };
        let verb = info.lpVerb.0 as *const u16;
        if verb.is_null() {
            return Err(E_INVALIDARG.into());
        }
        let id = unsafe { std::ptr::read_unaligned(verb) } as u32;
        let Some(key) = self.commands.borrow().get(&id).cloned() else {
            return Err(E_INVALIDARG.into());
        };
        launch_host(&self.files.borrow(), Some(&key));
        Ok(())
    }

    fn GetCommandString(
        &self,
        _idcmd: usize,
        _utype: u32,
        _preserved: *const u32,
        _pszname: PSTR,
        _cchmax: u32,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
}
#[implement(IExplorerCommand)]
struct CategoryCommand {
    spec: CategorySpec,
}

impl IExplorerCommand_Impl for CategoryCommand_Impl {
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
        Ok(guid_for(&format!("category/{}", self.spec.title)))
    }

    fn GetState(&self, items: Ref<IShellItemArray>, _slow: BOOL) -> windows::core::Result<u32> {
        let files = paths_from_items(items);
        if files.is_empty() {
            return Ok(ECS_ENABLED);
        }
        if self.spec.actions.iter().any(|action| {
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
        _items: Ref<IShellItemArray>,
        _bind: Ref<IBindCtx>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn GetFlags(&self) -> windows::core::Result<u32> {
        Ok(ECF_HASSUBCOMMANDS)
    }

    fn EnumSubCommands(&self) -> windows::core::Result<IEnumExplorerCommand> {
        let enumerator = EnumCommands {
            items: Arc::new(self.spec.actions.clone()),
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

#[implement(IEnumExplorerCommand)]
struct EnumCategories {
    items: Arc<Vec<CategorySpec>>,
    index: AtomicUsize,
}

impl IEnumExplorerCommand_Impl for EnumCategories_Impl {
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
            let command: IExplorerCommand = CategoryCommand { spec }.into();
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
        let clone = EnumCategories {
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
        let command: IExplorerCommand = RootCommand::new().into();
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
            category: action.category(),
        })
        .collect()
}

fn enabled_categories() -> Vec<CategorySpec> {
    let mut categories: Vec<CategorySpec> = Vec::new();
    for action in enabled_actions() {
        let category = action.category.clone();
        if let Some(existing) = categories
            .iter_mut()
            .find(|existing| existing.title == category)
        {
            existing.actions.push(action);
        } else {
            categories.push(CategorySpec {
                title: category,
                actions: vec![action],
            });
        }
    }
    categories.sort_by(|a, b| {
        category_rank(&a.title)
            .cmp(&category_rank(&b.title))
            .then_with(|| a.title.cmp(&b.title))
    });
    categories
}

fn category_rank(title: &str) -> i32 {
    match title {
        "動画に変換" => 10,
        "音声に変換" => 20,
        "画像に変換" => 30,
        "フォントに変換" => 40,
        "構造物に変換" => 50,
        "3Dに変換" => 60,
        "文書に変換" => 70,
        "その他" => 100,
        _ => 100,
    }
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
        command.arg("--action").arg(action);
        if action != "ffmpeg-other/convert" {
            command.arg("--run");
        }
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

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn matches_extension(path: &str, extensions: &[String]) -> bool {
    if extensions.iter().any(|value| value == "*") {
        return true;
    }
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
            assert_ne!(
                sub.GetFlags().expect("GetFlags failed") & ECF_HASSUBCOMMANDS,
                0
            );

            let actions = sub.EnumSubCommands().expect("nested subcommands failed");
            let mut action_items: [Option<IExplorerCommand>; 1] = [None];
            let hr = actions.Next(&mut action_items, None);
            assert!(hr.is_ok());
            assert!(action_items[0].is_some());
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
            let command: IExplorerCommand = RootCommand::new().into();
            let title = |path: &Path| {
                command
                    .GetTitle(Some(&array(path)))
                    .expect("GetTitle")
                    .to_string()
                    .expect("utf16")
            };
            assert_eq!(title(&png), "変換");
            assert_eq!(title(&mp4), "変換");
            assert_eq!(title(&ico), "変換");
            assert_eq!(title(&txt), "変換");

            let state = |path: &Path| {
                command
                    .GetState(Some(&array(path)), false)
                    .expect("GetState")
            };
            assert_eq!(state(&png), ECS_ENABLED);
            assert_eq!(state(&txt), ECS_ENABLED);
            windows::Win32::System::Com::CoUninitialize();
        }
    }
}
