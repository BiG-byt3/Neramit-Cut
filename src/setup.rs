//! Per-user install support, run by NeramitCut-Setup: `--register` adds the shortcuts and an
//! entry in Settings > Apps for this NeramitCut.exe; `--uninstall` takes them away again and
//! removes the program. Neramit Cut has no file type of its own, so no file associations are made.
//! Everything lives under HKEY_CURRENT_USER and the user's own folders: no administrator rights.
//! Settings (%APPDATA%\NeramitCut) and the working folder (Documents\NeramitCut) are never touched.

use std::ffi::c_void;
use std::path::{Path, PathBuf};

use windows_sys::core::{GUID, HRESULT};
use windows_sys::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree};
use windows_sys::Win32::System::Registry::{RegDeleteKeyW, RegDeleteTreeW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD, REG_SZ, RRF_RT_REG_SZ};
use windows_sys::Win32::UI::Shell::{SHGetKnownFolderPath, FOLDERID_Desktop, FOLDERID_Programs};
use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONINFORMATION, MB_ICONQUESTION, MB_YESNO};

const PRODUCT: &str = "Neramit Cut";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\NeramitCut";
const SHORTCUT: &str = "Neramit Cut.lnk";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set_str(key: &str, name: &str, value: &str) -> bool {
    let (k, n, v) = (wide(key), wide(name), wide(value));
    // SAFETY: NUL-terminated UTF-16 buffers that outlive the call; the size includes the NUL
    unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), REG_SZ, v.as_ptr() as *const c_void, (v.len() * 2) as u32) == 0 }
}

fn set_dword(key: &str, name: &str, value: u32) -> bool {
    let (k, n) = (wide(key), wide(name));
    // SAFETY: as in set_str; the data is a 4-byte value
    unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), REG_DWORD, &value as *const u32 as *const c_void, 4) == 0 }
}

/// The folder NeramitCut-Setup installed the program in (from the Settings > Apps entry).
pub fn install_location() -> Option<PathBuf> {
    let (k, n) = (wide(UNINSTALL_KEY), wide("InstallLocation"));
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: the buffer and its byte size are passed together
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as *mut c_void, &mut size) };
    if r != 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(PathBuf::from(String::from_utf16_lossy(&buf[..len]))).filter(|p| !p.as_os_str().is_empty())
}

fn delete_tree(key: &str) {
    let k = wide(key);
    // SAFETY: NUL-terminated key name
    unsafe {
        RegDeleteTreeW(HKEY_CURRENT_USER, k.as_ptr());
        RegDeleteKeyW(HKEY_CURRENT_USER, k.as_ptr());
    }
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    let mut p: *mut u16 = std::ptr::null_mut();
    // SAFETY: the shell allocates the string; it is freed with CoTaskMemFree once copied
    unsafe {
        let hr = SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut p);
        if hr < 0 || p.is_null() {
            return None;
        }
        let mut n = 0;
        while *p.add(n) != 0 {
            n += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
        CoTaskMemFree(p as *const c_void);
        Some(PathBuf::from(s))
    }
}

const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);
const IID_SHELL_LINK_W: GUID = GUID::from_u128(0x000214f9_0000_0000_c000_000000000046);
const IID_PERSIST_FILE: GUID = GUID::from_u128(0x0000010b_0000_0000_c000_000000000046);

#[repr(C)]
struct Unknown {
    query: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct ShellLinkVtbl {
    base: Unknown,
    _get_path: usize,
    _get_id_list: usize,
    _set_id_list: usize,
    _get_description: usize,
    set_description: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    _get_working_directory: usize,
    set_working_directory: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    _get_arguments: usize,
    _set_arguments: usize,
    _get_hotkey: usize,
    _set_hotkey: usize,
    _get_show_cmd: usize,
    _set_show_cmd: usize,
    _get_icon_location: usize,
    set_icon_location: unsafe extern "system" fn(*mut c_void, *const u16, i32) -> HRESULT,
    _set_relative_path: usize,
    _resolve: usize,
    set_path: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
}

#[repr(C)]
struct PersistFileVtbl {
    base: Unknown,
    _get_class_id: usize,
    _is_dirty: usize,
    _load: usize,
    save: unsafe extern "system" fn(*mut c_void, *const u16, i32) -> HRESULT,
}

unsafe fn vtbl<T>(p: *mut c_void) -> &'static T {
    &**(p as *mut *const T)
}

/// Writes a .lnk at `link` that starts `exe`.
fn make_shortcut(link: &Path, exe: &Path) -> bool {
    let target = wide(&exe.to_string_lossy());
    let dir = wide(&exe.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default());
    let desc = wide(&format!("{PRODUCT} - Linear Cutting Optimizer"));
    let file = wide(&link.to_string_lossy());
    // SAFETY: plain COM calls through the documented IShellLinkW / IPersistFile vtables; every
    // interface obtained is released before returning
    unsafe {
        let _ = CoInitializeEx(std::ptr::null(), 0x2);
        let mut sl: *mut c_void = std::ptr::null_mut();
        if CoCreateInstance(&CLSID_SHELL_LINK, std::ptr::null_mut(), 0x1, &IID_SHELL_LINK_W, &mut sl) < 0 || sl.is_null() {
            return false;
        }
        let v = vtbl::<ShellLinkVtbl>(sl);
        (v.set_path)(sl, target.as_ptr());
        (v.set_working_directory)(sl, dir.as_ptr());
        (v.set_description)(sl, desc.as_ptr());
        (v.set_icon_location)(sl, target.as_ptr(), 0);
        let mut pf: *mut c_void = std::ptr::null_mut();
        let mut ok = false;
        if (v.base.query)(sl, &IID_PERSIST_FILE, &mut pf) >= 0 && !pf.is_null() {
            ok = (vtbl::<PersistFileVtbl>(pf).save)(pf, file.as_ptr(), 1) >= 0;
            (vtbl::<Unknown>(pf).release)(pf);
        }
        (v.base.release)(sl);
        ok
    }
}

fn message(text: &str, style: u32) -> i32 {
    let (t, c) = (wide(text), wide(PRODUCT));
    // SAFETY: NUL-terminated strings; no owner window
    unsafe { MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), style) }
}

fn thai() -> bool {
    // as Setup: Thai when the Windows display language is Thai (LANG_THAI)
    (unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() } & 0x3FF) == 0x1E
}

/// `--register [desktop,startmenu]`: shortcuts and the Settings > Apps entry for this exe.
/// Exit code 0, or 1 when something could not be written.
pub fn register(opts: &str) -> i32 {
    let has = |k: &str| opts.split(',').any(|p| p.trim() == k);
    let (desktop, start_menu) = (has("desktop"), has("startmenu"));
    let Ok(exe) = std::env::current_exe() else { return 1 };
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    let exe_s = exe.to_string_lossy().to_string();
    let size_kb = std::fs::metadata(&exe).map(|m| (m.len() / 1024) as u32).unwrap_or(0);
    let mut ok = true;
    ok &= set_str(UNINSTALL_KEY, "DisplayName", PRODUCT);
    ok &= set_str(UNINSTALL_KEY, "DisplayVersion", env!("CARGO_PKG_VERSION"));
    ok &= set_str(UNINSTALL_KEY, "Publisher", env!("CARGO_PKG_AUTHORS"));
    ok &= set_str(UNINSTALL_KEY, "DisplayIcon", &format!("\"{exe_s}\",0"));
    ok &= set_str(UNINSTALL_KEY, "InstallLocation", &dir.to_string_lossy());
    let picked: Vec<&str> = [(desktop, "desktop"), (start_menu, "startmenu")].iter().filter(|(on, _)| *on).map(|(_, k)| *k).collect();
    ok &= set_str(UNINSTALL_KEY, "InstallOptions", &picked.join(","));
    ok &= set_str(UNINSTALL_KEY, "UninstallString", &format!("\"{exe_s}\" --uninstall"));
    ok &= set_str(UNINSTALL_KEY, "QuietUninstallString", &format!("\"{exe_s}\" --uninstall --quiet"));
    ok &= set_dword(UNINSTALL_KEY, "NoModify", 1);
    ok &= set_dword(UNINSTALL_KEY, "NoRepair", 1);
    ok &= set_dword(UNINSTALL_KEY, "EstimatedSize", size_kb);
    // an update keeps the user's choice: a shortcut that was not wanted is not made again
    for (folder, want) in [(&FOLDERID_Programs, start_menu), (&FOLDERID_Desktop, desktop)] {
        if let Some(link) = known_folder(folder).map(|d| d.join(SHORTCUT)) {
            if want {
                ok &= make_shortcut(&link, &exe);
            } else {
                let _ = std::fs::remove_file(link);
            }
        }
    }
    if ok {
        0
    } else {
        1
    }
}

/// `--uninstall [--quiet]`: removes the shortcuts and the Settings > Apps entry, then the install
/// folder once this process has exited.
pub fn uninstall(quiet: bool) -> i32 {
    let thai = thai();
    let ask = if thai {
        "ต้องการถอนการติดตั้ง Neramit Cut ออกจากเครื่องนี้หรือไม่\n\nวัสดุ ค่าตั้ง และงานที่บันทึกไว้จะยังอยู่"
    } else {
        "Remove Neramit Cut from this computer?\n\nYour materials, settings and saved jobs are kept."
    };
    if !quiet && message(ask, MB_YESNO | MB_ICONQUESTION) != IDYES {
        return 1;
    }
    delete_tree(UNINSTALL_KEY);
    for link in [&FOLDERID_Programs, &FOLDERID_Desktop].into_iter().filter_map(known_folder).map(|d| d.join(SHORTCUT)) {
        let _ = std::fs::remove_file(link);
    }
    // a running program cannot delete its own file: a copy of it in a folder of its own under
    // %TEMP% removes the program once this process has exited
    if let Ok(exe) = std::env::current_exe() {
        let helper_dir = std::env::temp_dir().join("Neramit Cut Uninstall");
        let helper = helper_dir.join("NeramitCut-Uninstall.exe");
        let started = std::fs::create_dir_all(&helper_dir).is_ok()
            && std::fs::copy(&exe, &helper).is_ok()
            && std::process::Command::new(&helper)
                .arg("--uninstall-finish")
                .arg(exe.parent().unwrap_or(Path::new("")))
                .arg(std::process::id().to_string())
                .current_dir(std::env::temp_dir())
                .spawn()
                .is_ok();
        if !started && !quiet {
            let dir = exe.parent().map(|d| d.display().to_string()).unwrap_or_default();
            message(
                &if thai { format!("ลบไฟล์โปรแกรมเองไม่ได้ กรุณาลบโฟลเดอร์นี้ด้วยตนเอง:\n{dir}") } else { format!("The program files could not be removed. Delete this folder yourself:\n{dir}") },
                MB_ICONINFORMATION,
            );
        }
    }
    if !quiet {
        message(if thai { "ถอนการติดตั้ง Neramit Cut เรียบร้อยแล้ว" } else { "Neramit Cut has been removed." }, MB_ICONINFORMATION);
    }
    0
}

/// `--uninstall-finish <dir> <pid>` (the copy `uninstall` starts): waits for that process to exit,
/// then removes the program from `dir`.
pub fn uninstall_finish(dir: &Path, pid: u32) -> i32 {
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    // SAFETY: a handle opened here and closed here
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if !h.is_null() {
            WaitForSingleObject(h, 30_000);
            windows_sys::Win32::Foundation::CloseHandle(h);
        }
    }
    // only a folder that holds nothing but the program is removed (never a user's Downloads)
    let only_ours = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().all(|e| e.file_name().to_string_lossy().starts_with("NeramitCut")))
        .unwrap_or(false);
    // (the file can stay locked for a moment after its process has gone)
    for _ in 0..20 {
        let done = if only_ours {
            std::fs::remove_dir_all(dir).is_ok() || !dir.exists()
        } else {
            std::fs::read_dir(dir)
                .map(|rd| {
                    rd.flatten()
                        .filter(|e| e.file_name().to_string_lossy().starts_with("NeramitCut.exe"))
                        .all(|e| std::fs::remove_file(e.path()).is_ok())
                })
                .unwrap_or(true)
        };
        if done {
            return 0;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    1
}
