#![windows_subsystem = "windows"]
//! Neramit Cut Setup: a wizard (Welcome, license agreement, install folder, additional tasks,
//! ready, installing, finish) that installs NeramitCut.exe for the current Windows user, by default in
//! %LOCALAPPDATA%\Programs\Neramit Cut, so no administrator rights are needed. The program then
//! registers itself (`NeramitCut.exe --register <tasks>`): shortcuts and an entry in
//! Settings > Apps to remove it again.
//!
//! `NeramitCut-Setup.exe /S` installs without questions, into the existing folder with the tasks
//! picked last time; `--relaunch` opens the program afterwards.
//! `--wait-pid <pid>` waits for the old program to close before replacing it.
//! The wizard is in Thai when Windows is in Thai, otherwise in English.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_IGNORE_ACL_ERRORS};
use windows_sys::Win32::System::Com::{CoInitializeEx, CoTaskMemFree};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, LoadLibraryW};
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
use windows_sys::Win32::UI::Controls::{InitCommonControlsEx, ICC_PROGRESS_CLASS, INITCOMMONCONTROLSEX, PBM_SETPOS, PBM_SETRANGE32};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SHBrowseForFolderW, SHGetPathFromIDListW, SetWindowSubclass, BROWSEINFOW};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const APP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/NeramitCut.exe.deflate"));
const VERSION: &str = env!("CARGO_PKG_VERSION");
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\NeramitCut";

const WM_PROGRESS: u32 = WM_APP + 1;
// edit control messages (shared by the rich edit control)
const EM_SETSEL: u32 = 0x00B1;
const EM_SETRECTNP: u32 = 0x00B4;
const WM_DONE: u32 = WM_APP + 2;

// pages
const WELCOME: u8 = 0;
const LICENSE: u8 = 1;
const FOLDER: u8 = 2;
const TASKS: u8 = 3;
const READY: u8 = 4;
const INSTALLING: u8 = 5;
const FINISH: u8 = 6;

/// Width of the picture down the left side of the Welcome and Finish pages (96-dpi units).
const BANNER_W: i32 = 164;

// control ids
const ID_BACK: usize = 10;
const ID_NEXT: usize = 11;
const ID_CANCEL: usize = 12;
const ID_BROWSE: usize = 20;
const ID_ACCEPT: usize = 22;
const ID_DECLINE: usize = 23;

const LICENSE_EN: &str = include_str!("../license-en.txt");
const LICENSE_TH: &str = include_str!("../license-th.txt");

/// The license text files use a small markup: "# " title, "## " heading, "- " bullet; any other
/// line is a paragraph. As plain text (no rich edit control) the markers are dropped.
fn license_plain(src: &str) -> String {
    src.lines()
        .map(|l| {
            if let Some(h) = l.strip_prefix("## ").or_else(|| l.strip_prefix("# ")) {
                h.to_string()
            } else if let Some(b) = l.strip_prefix("- ") {
                format!("  \u{2022} {b}")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The license as RTF for the rich edit control: green bold headings, bullets and paragraph
/// spacing. Thai uses Leelawadee UI, whose Thai marks have room above and below the line.
fn license_rtf(src: &str, thai: bool) -> String {
    fn esc(s: &str) -> String {
        let mut o = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '\\' | '{' | '}' => {
                    o.push('\\');
                    o.push(c);
                }
                c if (c as u32) < 0x80 => o.push(c),
                c => {
                    let mut b = [0u16; 2];
                    for u in c.encode_utf16(&mut b) {
                        o.push_str(&format!("\\u{}?", *u as i16));
                    }
                }
            }
        }
        o
    }
    let (font, size) = (thai as u8, 18);
    let mut o = format!(
        "{{\\rtf1\\ansi\\deff0{{\\fonttbl{{\\f0\\fnil Segoe UI;}}{{\\f1\\fnil Leelawadee UI;}}}}\
         {{\\colortbl;\\red32\\green32\\blue32;\\red26\\green107\\blue216;\\red110\\green110\\blue110;}}\\f{font}\\fs{size}\\cf1\n"
    );
    let mut lines = src.lines().filter(|l| !l.trim().is_empty()).peekable();
    let mut first = true;
    while let Some(l) = lines.next() {
        let next_is_bullet = lines.peek().is_some_and(|n| n.starts_with("- "));
        if let Some(h) = l.strip_prefix("# ") {
            o.push_str(&format!("\\pard\\sa20\\cf2\\b\\fs{} {}\\b0\\fs{size}\\cf1\\par\n", size + 4, esc(h)));
        } else if let Some(h) = l.strip_prefix("## ") {
            o.push_str(&format!("\\pard\\sb{}\\sa60\\keepn\\cf2\\b {}\\b0\\cf1\\par\n", if first { 0 } else { 200 }, esc(h)));
        } else if let Some(b) = l.strip_prefix("- ") {
            o.push_str(&format!("\\pard\\fi-200\\li400\\tx400\\sa{}\\bullet\\tab {}\\par\n", if next_is_bullet { 30 } else { 120 }, esc(b)));
        } else if first {
            // the copyright line under the title
            o.push_str(&format!("\\pard\\sa160\\cf3 {}\\cf1\\par\n", esc(l)));
        } else {
            o.push_str(&format!("\\pard\\sa{} {}\\par\n", if next_is_bullet { 60 } else { 120 }, esc(l)));
        }
        first = l.starts_with("# ");
    }
    o.push('}');
    o
}

/// The license text keeps the arrow and page keys for scrolling, but Tab moves on and Enter / Esc
/// reach the wizard buttons (a rich edit control would keep Tab to itself).
unsafe extern "system" fn license_keys(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    let r = DefSubclassProc(h, msg, wp, lp);
    if msg == WM_GETDLGCODE {
        return r & !((DLGC_WANTTAB | DLGC_WANTALLKEYS) as LRESULT);
    }
    r
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn thai() -> bool {
    static T: OnceLock<bool> = OnceLock::new();
    // LANG_THAI = 0x1E (primary language of the Windows display language); NERAMIT_SETUP_LANG=th|en for QA
    *T.get_or_init(|| match std::env::var("NERAMIT_SETUP_LANG").as_deref() {
        Ok("th") => true,
        Ok("en") => false,
        _ => (unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() } & 0x3FF) == 0x1E,
    })
}

/// English or Thai text.
fn t(en: &'static str, th: &'static str) -> &'static str {
    if thai() {
        th
    } else {
        en
    }
}

fn reg_value(name: &str) -> Option<String> {
    let (k, n) = (wide(UNINSTALL_KEY), wide(name));
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: the buffer and its byte size are passed together
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as *mut c_void, &mut size) };
    if r != 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// The tasks of the wizard, as `NeramitCut.exe --register` takes them.
#[derive(Clone, Copy)]
struct Tasks {
    desktop: bool,
    start_menu: bool,
}

impl Tasks {
    fn previous() -> Tasks {
        match reg_value("InstallOptions") {
            Some(s) => {
                let has = |k: &str| s.split(',').any(|p| p.trim() == k);
                Tasks { desktop: has("desktop"), start_menu: has("startmenu") }
            }
            None => Tasks { desktop: true, start_menu: true },
        }
    }
    fn arg(self) -> String {
        let parts: Vec<&str> = [(self.desktop, "desktop"), (self.start_menu, "startmenu")]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, k)| *k)
            .collect();
        if parts.is_empty() {
            "none".into()
        } else {
            parts.join(",")
        }
    }
}

fn default_dir() -> PathBuf {
    if let Some(d) = reg_value("InstallLocation").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    local.join("Programs").join("Neramit Cut")
}

/// Write the complete new program beside its destination, then exchange files atomically.
/// The old executable is kept until the new one has run successfully; project files are untouched.
fn replace_program(exe: &Path, bytes: &[u8]) -> std::io::Result<Option<PathBuf>> {
    use std::io::Write;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let suffix = format!("{}-{stamp}", std::process::id());
    let staged = exe.with_extension(format!("exe.{suffix}.tmp"));
    // A .rollback suffix prevents --register from clearing this backup as a stale .old file.
    let backup = exe.with_extension(format!("exe.{suffix}.rollback"));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&staged)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if exe.exists() {
            let (dest, src, old) = (wide(&exe.to_string_lossy()), wide(&staged.to_string_lossy()), wide(&backup.to_string_lossy()));
            // SAFETY: NUL-terminated paths outlive the call; all paths are in the same directory.
            // Staged files inherit the destination directory's ACL. Modify permission
            // must suffice even when the user cannot merge the old file's ACL.
            if unsafe { ReplaceFileW(dest.as_ptr(), src.as_ptr(), old.as_ptr(), REPLACEFILE_IGNORE_ACL_ERRORS, std::ptr::null(), std::ptr::null()) } == 0 {
                let error = std::io::Error::last_os_error();
                // Windows may have moved the old name before an exchange failed. Restore it if needed.
                if !exe.exists() && backup.is_file() {
                    let _ = std::fs::rename(&backup, exe);
                }
                return Err(error);
            }
            Ok(Some(backup.clone()))
        } else {
            std::fs::rename(&staged, exe)?;
            Ok(None)
        }
    })();
    let _ = std::fs::remove_file(staged);
    result
}

/// Restore the previous executable when the replacement cannot even start.
fn restore_program(exe: &Path, backup: &Path) -> std::io::Result<()> {
    if !exe.exists() {
        return std::fs::rename(backup, exe);
    }
    let (dest, src) = (wide(&exe.to_string_lossy()), wide(&backup.to_string_lossy()));
    // SAFETY: the NUL-terminated paths outlive the call, with no extra backup requested.
    if unsafe { ReplaceFileW(dest.as_ptr(), src.as_ptr(), std::ptr::null(), REPLACEFILE_IGNORE_ACL_ERRORS, std::ptr::null(), std::ptr::null()) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn wait_for_program(pid: u32, timeout_ms: u32) -> Result<(), String> {
    if pid == 0 || pid == std::process::id() {
        return Err("Invalid update process ID.".into());
    }
    // SAFETY: the process handle opened here is closed after waiting; no process is terminated.
    let (result, error) = unsafe {
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if process.is_null() {
            // A process that has already exited no longer has an openable PID.
            return if GetLastError() == ERROR_INVALID_PARAMETER { Ok(()) } else { Err(std::io::Error::last_os_error().to_string()) };
        }
        let result = WaitForSingleObject(process, timeout_ms);
        let error = std::io::Error::last_os_error();
        CloseHandle(process);
        (result, error)
    };
    match result {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(t("Neramit Cut is still running. Close it and run Setup again. Your installed program has not been changed.", "Neramit Cut ยังเปิดอยู่ กรุณาปิดโปรแกรมแล้วเรียก Setup อีกครั้ง โปรแกรมที่ติดตั้งอยู่ยังไม่ถูกเปลี่ยนแปลง").into()),
        _ => Err(error.to_string()),
    }
}

/// Copies the program and registers it; `progress(0..=100)`.
fn install(dir: &Path, tasks: Tasks, progress: &dyn Fn(u32)) -> Result<(PathBuf, i32), String> {
    progress(5);
    // the copy an earlier uninstall left in %TEMP% to remove the program
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join("Neramit Cut Uninstall"));
    std::fs::create_dir_all(dir).map_err(|e| format!("{} {}\n\n({e})", t("Could not create the folder", "สร้างโฟลเดอร์ไม่ได้:"), dir.display()))?;
    let exe = dir.join("NeramitCut.exe");
    let bytes = miniz_oxide::inflate::decompress_to_vec(APP).map_err(|_| t("The setup file is damaged. Download it again.", "ไฟล์ติดตั้งเสียหาย กรุณาดาวน์โหลดใหม่").to_string())?;
    progress(55);
    let backup = replace_program(&exe, &bytes).map_err(|e| format!("{} {}\n\n({e})", t("Could not replace the program. Close Neramit Cut and run Setup again:", "แทนที่โปรแกรมไม่ได้ กรุณาปิด Neramit Cut แล้วเรียก Setup อีกครั้ง:"), exe.display()))?;
    progress(80);
    let registration = std::process::Command::new(&exe).args(["--register", &tasks.arg()]).status();
    let code = match registration {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            let restored = backup.as_ref().map(|p| restore_program(&exe, p)).transpose();
            let detail = match restored {
                Ok(_) => t("The previous program was kept.", "เก็บโปรแกรมรุ่นเดิมไว้แล้ว").to_string(),
                Err(e) => format!("{} ({e})", t("The previous program is preserved in the .rollback file in the install folder.", "โปรแกรมรุ่นเดิมถูกเก็บไว้ในไฟล์ .rollback ในโฟลเดอร์ติดตั้ง")),
            };
            return Err(format!("{} ({error})\n\n{detail}", t("Could not start Neramit Cut.", "เปิด Neramit Cut ไม่ได้")));
        }
    };
    if code != 0 {
        return Err(t("Neramit Cut was copied, but some shortcuts could not be created.", "คัดลอกโปรแกรมแล้ว แต่สร้างทางลัดบางส่วนไม่สำเร็จ").into());
    }
    if let Some(path) = backup {
        let _ = std::fs::remove_file(path);
    }
    progress(100);
    Ok((exe, code))
}

/// The installed program is running (Windows does not let a running program be overwritten).
fn in_use(exe: &Path) -> bool {
    exe.exists() && std::fs::OpenOptions::new().write(true).open(exe).is_err()
}

/// A multi-line edit control wants Windows line ends.
fn crlf(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\n', "\r\n")
}

// ------------------------------------------------------------------------------------------
// the wizard window

struct Ui {
    hwnd: HWND,
    scale: f32,
    font: HFONT,
    title_font: HFONT,
    /// the title of the Welcome and Finish pages
    big_font: HFONT,
    white: HBRUSH,
    footer: HBRUSH,
    banner: HBRUSH,
    icon: HICON,
    big_icon: HICON,
    /// (control, page it belongs to)
    controls: Vec<(HWND, u8)>,
    back: HWND,
    next: HWND,
    cancel: HWND,
    folder_edit: HWND,
    accept: HWND,
    /// the license text; paint() frames it
    license: HWND,
    chk_desktop: HWND,
    chk_start: HWND,
    ready_text: HWND,
    progress: HWND,
    status_text: HWND,
    finish_text: HWND,
    chk_launch: HWND,
}

// SAFETY: the window handles are only used on the thread that made the window
unsafe impl Sync for Ui {}
unsafe impl Send for Ui {}

static UI: OnceLock<Ui> = OnceLock::new();
static PAGE: AtomicU32 = AtomicU32::new(0);
/// set when an older install exists: no folder page, it is updated in place
static UPGRADE: OnceLock<Option<String>> = OnceLock::new();
static RESULT: Mutex<Option<Result<(PathBuf, i32), String>>> = Mutex::new(None);

fn upgrade() -> Option<&'static str> {
    UPGRADE.get().and_then(|v| v.as_deref())
}

fn px(v: i32) -> i32 {
    let s = UI.get().map(|u| u.scale).unwrap_or(1.0);
    (v as f32 * s).round() as i32
}

unsafe fn set_text(h: HWND, s: &str) {
    let w = wide(s);
    SetWindowTextW(h, w.as_ptr());
}

unsafe fn get_text(h: HWND) -> String {
    let n = GetWindowTextLengthW(h);
    let mut buf = vec![0u16; n as usize + 1];
    let got = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
    String::from_utf16_lossy(&buf[..got.max(0) as usize])
}

unsafe fn checked(h: HWND) -> bool {
    SendMessageW(h, BM_GETCHECK, 0, 0) == 1
}

unsafe fn set_check(h: HWND, on: bool) {
    SendMessageW(h, BM_SETCHECK, on as usize, 0);
}

fn message(owner: HWND, text: &str, style: u32) -> i32 {
    let (tx, c) = (wide(text), wide(&format!("Neramit Cut {VERSION} Setup")));
    // SAFETY: NUL-terminated strings
    unsafe { MessageBoxW(owner, tx.as_ptr(), c.as_ptr(), style) }
}

/// Header title and subtitle of a page.
fn heading(page: u8) -> (&'static str, String) {
    match page {
        WELCOME => (t("Welcome to the Neramit Cut Setup Wizard", "ยินดีต้อนรับสู่ตัวช่วยติดตั้ง Neramit Cut"), String::new()),
        LICENSE => (t("License Agreement", "ข้อตกลงการใช้งาน"), t("Please read the following important information before continuing.", "กรุณาอ่านข้อมูลสำคัญต่อไปนี้ก่อนดำเนินการต่อ").into()),
        FOLDER => (t("Choose Install Location", "เลือกตำแหน่งที่ติดตั้ง"), t("Choose the folder in which to install Neramit Cut.", "เลือกโฟลเดอร์ที่จะติดตั้ง Neramit Cut").into()),
        TASKS => (t("Select Additional Tasks", "เลือกงานเพิ่มเติม"), t("Which additional tasks should be performed?", "ต้องการให้ตั้งค่าอะไรเพิ่มเติมบ้าง").into()),
        READY => (t("Ready to Install", "พร้อมติดตั้ง"), t("Setup is now ready to install Neramit Cut on your computer.", "พร้อมติดตั้ง Neramit Cut ลงในเครื่องแล้ว").into()),
        INSTALLING => (t("Installing", "กำลังติดตั้ง"), t("Please wait while Neramit Cut is installed.", "กรุณารอสักครู่ ระหว่างติดตั้ง Neramit Cut").into()),
        _ => (t("Completing the Neramit Cut Setup Wizard", "ติดตั้ง Neramit Cut เสร็จสมบูรณ์"), String::new()),
    }
}

fn tasks_from_ui(u: &Ui) -> Tasks {
    // SAFETY: checkbox handles of this window
    unsafe {
        Tasks {
            desktop: checked(u.chk_desktop),
            start_menu: checked(u.chk_start),
        }
    }
}

unsafe fn show_page(page: u8) {
    let Some(u) = UI.get() else { return };
    PAGE.store(page as u32, Ordering::Relaxed);
    for &(h, p) in &u.controls {
        ShowWindow(h, if p == page { SW_SHOW } else { SW_HIDE });
    }
    ShowWindow(u.back, if page == FINISH || page == INSTALLING { SW_HIDE } else { SW_SHOW });
    EnableWindow(u.back, (page != WELCOME) as i32);
    EnableWindow(u.next, (page != INSTALLING && (page != LICENSE || checked(u.accept))) as i32);
    EnableWindow(u.cancel, (page != INSTALLING && page != FINISH) as i32);
    set_text(
        u.next,
        match page {
            READY => t("&Install", "&ติดตั้ง"),
            FINISH => t("&Finish", "&เสร็จสิ้น"),
            _ => t("&Next >", "&ถัดไป >"),
        },
    );
    if page == READY {
        let tk = tasks_from_ui(u);
        let mut s = format!("{}\n      {}\n\n", t("Destination folder:", "โฟลเดอร์ปลายทาง:"), get_text(u.folder_edit));
        s += t("Additional tasks:", "งานเพิ่มเติม:");
        s += "\n";
        let mut any = false;
        for (on, label) in [
            (tk.desktop, t("Create a desktop shortcut", "สร้างทางลัดบนเดสก์ท็อป")),
            (tk.start_menu, t("Create a Start menu shortcut", "สร้างทางลัดในเมนูเริ่ม")),
        ] {
            if on {
                s += &format!("      {label}\n");
                any = true;
            }
        }
        if !any {
            s += "      -\n";
        }
        set_text(u.ready_text, &crlf(&s));
    }
    // the header and every button (enabled state, label) are drawn again
    RedrawWindow(u.hwnd, std::ptr::null(), std::ptr::null_mut(), RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW);
}

unsafe fn browse(u: &Ui) {
    let title = wide(t("Select the folder to install Neramit Cut in:", "เลือกโฟลเดอร์ที่จะติดตั้ง Neramit Cut:"));
    let mut name = vec![0u16; 260];
    let bi = BROWSEINFOW {
        hwndOwner: u.hwnd,
        pidlRoot: std::ptr::null_mut(),
        pszDisplayName: name.as_mut_ptr(),
        lpszTitle: title.as_ptr(),
        ulFlags: 0x1 | 0x40, // BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE
        lpfn: None,
        lParam: 0,
        iImage: 0,
    };
    let pidl = SHBrowseForFolderW(&bi);
    if pidl.is_null() {
        return;
    }
    let mut path = vec![0u16; 1024];
    if SHGetPathFromIDListW(pidl, path.as_mut_ptr()) != 0 {
        let n = path.iter().position(|&c| c == 0).unwrap_or(0);
        let mut p = PathBuf::from(String::from_utf16_lossy(&path[..n]));
        if p.file_name().map(|f| f != "Neramit Cut").unwrap_or(true) {
            p = p.join("Neramit Cut");
        }
        set_text(u.folder_edit, &p.to_string_lossy());
    }
    CoTaskMemFree(pidl as *const c_void);
}

/// The folder can be written by this user (no administrator rights are asked for).
fn writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".neramit-setup-test");
    let ok = std::fs::write(&probe, b"ok").is_ok();
    let _ = std::fs::remove_file(probe);
    ok
}

unsafe fn next(u: &Ui) {
    let page = PAGE.load(Ordering::Relaxed) as u8;
    match page {
        WELCOME => show_page(LICENSE),
        LICENSE => show_page(if upgrade().is_some() { TASKS } else { FOLDER }),
        FOLDER => {
            let dir = PathBuf::from(get_text(u.folder_edit).trim());
            if dir.as_os_str().is_empty() || !dir.is_absolute() {
                message(u.hwnd, t("Enter a full folder path, for example C:\\Users\\you\\Programs\\Neramit Cut.", "กรุณาระบุโฟลเดอร์แบบเต็ม เช่น C:\\Users\\you\\Programs\\Neramit Cut"), MB_ICONWARNING);
                return;
            }
            if !writable(&dir) {
                message(
                    u.hwnd,
                    t(
                        "You cannot write to this folder without administrator rights.\n\nChoose a folder in your own user profile (the suggested folder works).",
                        "โฟลเดอร์นี้ต้องใช้สิทธิ์ผู้ดูแลระบบในการเขียน\n\nกรุณาเลือกโฟลเดอร์ในโปรไฟล์ของคุณเอง (โฟลเดอร์ที่แนะนำใช้ได้)",
                    ),
                    MB_ICONWARNING,
                );
                return;
            }
            show_page(TASKS);
        }
        TASKS => show_page(READY),
        READY => {
            let dir = PathBuf::from(get_text(u.folder_edit).trim());
            while in_use(&dir.join("NeramitCut.exe")) {
                let ask = t(
                    "Neramit Cut is running. Close it, then click Retry to continue, or Cancel to exit Setup.",
                    "Neramit Cut กำลังเปิดอยู่ กรุณาปิดโปรแกรมก่อน แล้วกด ลองใหม่ เพื่อดำเนินการต่อ หรือ ยกเลิก เพื่อออก",
                );
                if message(u.hwnd, ask, MB_RETRYCANCEL | MB_ICONWARNING) != IDRETRY {
                    return;
                }
            }
            show_page(INSTALLING);
            let tasks = tasks_from_ui(u);
            let hwnd = u.hwnd as isize;
            std::thread::spawn(move || {
                let post = |pct: u32| {
                    // SAFETY: posting to our own window; it outlives the installation
                    unsafe { PostMessageW(hwnd as HWND, WM_PROGRESS, pct as usize, 0) };
                };
                let r = install(&dir, tasks, &post);
                if let Ok(mut g) = RESULT.lock() {
                    *g = Some(r);
                }
                unsafe { PostMessageW(hwnd as HWND, WM_DONE, 0, 0) };
            });
        }
        FINISH => {
            if checked(u.chk_launch) {
                if let Some(Ok((exe, _))) = RESULT.lock().ok().and_then(|g| g.clone()) {
                    let _ = std::process::Command::new(&exe).current_dir(exe.parent().unwrap_or(Path::new("."))).spawn();
                }
            }
            DestroyWindow(u.hwnd);
        }
        _ => {}
    }
}

unsafe fn back() {
    let page = PAGE.load(Ordering::Relaxed) as u8;
    match page {
        LICENSE => show_page(WELCOME),
        FOLDER => show_page(LICENSE),
        TASKS => show_page(if upgrade().is_some() { LICENSE } else { FOLDER }),
        READY => show_page(TASKS),
        _ => {}
    }
}

unsafe fn done(u: &Ui) {
    let r = RESULT.lock().ok().and_then(|g| g.clone());
    match r {
        Some(Ok(_)) => {
            let mut s = String::from(t("Neramit Cut has been installed on your computer.", "ติดตั้ง Neramit Cut ลงในเครื่องเรียบร้อยแล้ว"));
            s += "\n\n";
            s += t(
                "Start it from the Start menu or the desktop shortcut. Your materials, settings and saved jobs are kept. To remove Neramit Cut, use Settings > Apps > Installed apps.",
                "เปิดโปรแกรมได้จากเมนูเริ่มหรือทางลัดบนเดสก์ท็อป วัสดุ ค่าตั้ง และงานที่บันทึกไว้ยังอยู่ครบ หากต้องการถอนการติดตั้ง ไปที่ Settings > Apps > Installed apps",
            );
            set_text(u.finish_text, &s);
            show_page(FINISH);
            SetFocus(u.next);
        }
        Some(Err(e)) => {
            message(u.hwnd, &e, MB_ICONERROR);
            show_page(READY);
        }
        None => {}
    }
}

unsafe fn paint(u: &Ui) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let dc = BeginPaint(u.hwnd, &mut ps);
    let mut rc: RECT = std::mem::zeroed();
    GetClientRect(u.hwnd, &mut rc);
    let page = PAGE.load(Ordering::Relaxed) as u8;
    let body = RECT { left: 0, top: 0, right: rc.right, bottom: rc.bottom - px(52) };
    FillRect(dc, &body, u.white);
    let foot = RECT { left: 0, top: rc.bottom - px(52), right: rc.right, bottom: rc.bottom };
    FillRect(dc, &foot, u.footer);
    let line = CreateSolidBrush(0x00E0E0E0);
    FillRect(dc, &RECT { left: 0, top: rc.bottom - px(52), right: rc.right, bottom: rc.bottom - px(52) + 1 }, line);
    let (title, sub) = heading(page);
    SetBkMode(dc, 1); // TRANSPARENT
    let old = SelectObject(dc, u.title_font as HGDIOBJ);
    if page == WELCOME || page == FINISH {
        FillRect(dc, &RECT { left: 0, top: 0, right: px(BANNER_W), bottom: rc.bottom - px(52) }, u.banner);
        if !u.big_icon.is_null() {
            DrawIconEx(dc, px((BANNER_W - 96) / 2), px(56), u.big_icon, px(96), px(96), 0, std::ptr::null_mut(), DI_NORMAL);
        }
        SetTextColor(dc, 0x00FFFFFF);
        let mut r = RECT { left: 0, top: px(166), right: px(BANNER_W), bottom: px(192) };
        let name = wide("Neramit Cut");
        DrawTextW(dc, name.as_ptr(), -1, &mut r, DT_SINGLELINE | DT_CENTER);
        SelectObject(dc, u.font as HGDIOBJ);
        SetTextColor(dc, 0x00FBE6D5);
        let mut r = RECT { left: 0, top: px(194), right: px(BANNER_W), bottom: px(214) };
        let ver = wide(&format!("{} {VERSION}", t("Version", "เวอร์ชัน")));
        DrawTextW(dc, ver.as_ptr(), -1, &mut r, DT_SINGLELINE | DT_CENTER);
        SelectObject(dc, u.big_font as HGDIOBJ);
        SetTextColor(dc, 0x00202020);
        let mut r = RECT { left: px(BANNER_W + 20), top: px(20), right: rc.right - px(20), bottom: px(84) };
        let tw = wide(title);
        DrawTextW(dc, tw.as_ptr(), -1, &mut r, DT_WORDBREAK);
    } else {
        FillRect(dc, &RECT { left: 0, top: px(64), right: rc.right, bottom: px(64) + 1 }, line);
        SetTextColor(dc, 0x00202020);
        let mut r = RECT { left: px(20), top: px(12), right: rc.right - px(76), bottom: px(36) };
        let tw = wide(title);
        DrawTextW(dc, tw.as_ptr(), -1, &mut r, DT_SINGLELINE | DT_END_ELLIPSIS);
        SelectObject(dc, u.font as HGDIOBJ);
        SetTextColor(dc, 0x00555555);
        let mut r = RECT { left: px(32), top: px(38), right: rc.right - px(76), bottom: px(60) };
        let sw = wide(&sub);
        DrawTextW(dc, sw.as_ptr(), -1, &mut r, DT_SINGLELINE | DT_END_ELLIPSIS);
        if !u.icon.is_null() {
            DrawIconEx(dc, rc.right - px(60), px(8), u.icon, px(48), px(48), 0, std::ptr::null_mut(), DI_NORMAL);
        }
    }
    if page == LICENSE {
        // a thin frame one pixel outside the license text (it has no border of its own)
        let mut r: RECT = std::mem::zeroed();
        GetWindowRect(u.license, &mut r);
        MapWindowPoints(std::ptr::null_mut(), u.hwnd, &mut r as *mut RECT as *mut POINT, 2);
        let frame = CreateSolidBrush(0x00D5D5D5);
        FrameRect(dc, &RECT { left: r.left - 1, top: r.top - 1, right: r.right + 1, bottom: r.bottom + 1 }, frame);
        DeleteObject(frame as HGDIOBJ);
    }
    DeleteObject(line as HGDIOBJ);
    SelectObject(dc, u.font as HGDIOBJ);
    SetTextColor(dc, 0x00808080);
    let mut r = RECT { left: px(16), top: rc.bottom - px(36), right: px(300), bottom: rc.bottom - px(14) };
    let cw = wide("Neramit Cut Made by BiGByte Studio");
    DrawTextW(dc, cw.as_ptr(), -1, &mut r, DT_SINGLELINE);
    SelectObject(dc, old);
    EndPaint(u.hwnd, &ps);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let Some(u) = UI.get() else { return DefWindowProcW(hwnd, msg, wp, lp) };
    match msg {
        WM_PAINT => {
            paint(u);
            0
        }
        WM_ERASEBKGND => 1,
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            let dc = wp as HDC;
            SetBkColor(dc, 0x00FFFFFF);
            SetTextColor(dc, 0x00202020);
            u.white as LRESULT
        }
        WM_COMMAND => {
            match wp & 0xFFFF {
                // 1 = IDOK: Enter (IsDialogMessage) presses Next
                ID_NEXT | 1 => next(u),
                ID_BACK => back(),
                ID_CANCEL | 2 => {
                    // 2 = IDCANCEL (Esc)
                    let page = PAGE.load(Ordering::Relaxed) as u8;
                    if page != INSTALLING
                        && (page == FINISH || message(hwnd, t("Setup is not complete. Exit Setup?", "การติดตั้งยังไม่เสร็จ ต้องการออกจากโปรแกรมติดตั้งหรือไม่"), MB_YESNO | MB_ICONQUESTION) == IDYES)
                    {
                        DestroyWindow(hwnd);
                    }
                }
                ID_BROWSE => browse(u),
                ID_ACCEPT | ID_DECLINE => {
                    EnableWindow(u.next, checked(u.accept) as i32);
                }
                _ => {}
            }
            0
        }
        WM_PROGRESS => {
            SendMessageW(u.progress, PBM_SETPOS, wp, 0);
            set_text(
                u.status_text,
                if wp < 55 {
                    t("Extracting files...", "กำลังแตกไฟล์...")
                } else if wp < 80 {
                    t("Copying Neramit Cut...", "กำลังคัดลอก Neramit Cut...")
                } else {
                    t("Creating shortcuts...", "กำลังสร้างทางลัด...")
                },
            );
            0
        }
        WM_DONE => {
            done(u);
            0
        }
        WM_CLOSE => {
            SendMessageW(hwnd, WM_COMMAND, ID_CANCEL, 0);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// QA: `NERAMIT_SETUP_SHOT="0=a.bmp;2=b.bmp;5=c.bmp"` lays the wizard out off-screen, saves each page
/// as a BMP and quits (nothing is installed; page 5 shows a made-up "done" result).
unsafe fn qa_shots(u: &Ui, spec: &str) {
    for item in spec.split(';') {
        let Some((page, path)) = item.split_once('=') else { continue };
        let page: u8 = page.trim().parse().unwrap_or(0);
        if page == FINISH {
            if let Ok(mut g) = RESULT.lock() {
                *g = Some(Ok((std::env::temp_dir().join("NeramitCut.exe"), 0)));
            }
            done(u);
        } else {
            show_page(page);
            // NERAMIT_SETUP_LICENSE_END=1: the license page scrolled to the end of the text
            if page == LICENSE && std::env::var_os("NERAMIT_SETUP_LICENSE_END").is_some() {
                SendMessageW(u.license, WM_VSCROLL, SB_BOTTOM as usize, 0);
            }
        }
        let mut m: MSG = std::mem::zeroed();
        // themed buttons fade between enabled and disabled: let the animation finish
        for _ in 0..25 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            while PeekMessageW(&mut m, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&m);
                DispatchMessageW(&m);
            }
            UpdateWindow(u.hwnd);
        }
        let mut rc: RECT = std::mem::zeroed();
        GetWindowRect(u.hwnd, &mut rc);
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
        let screen = GetDC(std::ptr::null_mut());
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp as HGDIOBJ);
        // PW_RENDERFULLCONTENT: works for a window that is not on the screen
        windows_sys::Win32::Storage::Xps::PrintWindow(u.hwnd, mem, 2);
        SelectObject(mem, old);
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = w;
        bi.bmiHeader.biHeight = h; // bottom-up, as a BMP file stores it
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        let mut px = vec![0u8; (w * h * 4) as usize];
        GetDIBits(mem, bmp, 0, h as u32, px.as_mut_ptr() as *mut c_void, &mut bi, DIB_RGB_COLORS);
        DeleteObject(bmp as HGDIOBJ);
        DeleteDC(mem);
        ReleaseDC(std::ptr::null_mut(), screen);
        let mut f = Vec::with_capacity(54 + px.len());
        f.extend_from_slice(b"BM");
        f.extend_from_slice(&(54 + px.len() as u32).to_le_bytes());
        f.extend_from_slice(&0u32.to_le_bytes());
        f.extend_from_slice(&54u32.to_le_bytes());
        f.extend_from_slice(&40u32.to_le_bytes());
        f.extend_from_slice(&w.to_le_bytes());
        f.extend_from_slice(&h.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&32u16.to_le_bytes());
        f.extend_from_slice(&[0u8; 24]);
        f.extend_from_slice(&px);
        let _ = std::fs::write(path.trim(), f);
    }
    DestroyWindow(u.hwnd);
}

unsafe fn font(dc: HDC, pt: i32, weight: i32) -> HFONT {
    let face = wide("Segoe UI");
    let h = -(pt * GetDeviceCaps(dc, LOGPIXELSY as i32) / 72);
    CreateFontW(h, 0, 0, 0, weight, 0, 0, 0, 1, 0, 0, 5, 0, face.as_ptr())
}

unsafe fn run_wizard() {
    let _ = CoInitializeEx(std::ptr::null(), 0x2);
    let icc = INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_PROGRESS_CLASS };
    InitCommonControlsEx(&icc);
    let hinst = GetModuleHandleW(std::ptr::null());
    let screen = GetDC(std::ptr::null_mut());
    let scale = GetDeviceCaps(screen, LOGPIXELSY as i32) as f32 / 96.0;
    let (f, fb, ft, fbig) = (font(screen, 9, 400), font(screen, 9, 700), font(screen, 12, 600), font(screen, 13, 700));
    ReleaseDC(std::ptr::null_mut(), screen);
    // the application icon (resource 1)
    let icon = LoadImageW(hinst, 1 as _, IMAGE_ICON, (48.0 * scale) as i32, (48.0 * scale) as i32, 0) as HICON;
    let small = LoadImageW(hinst, 1 as _, IMAGE_ICON, (16.0 * scale) as i32, (16.0 * scale) as i32, 0) as HICON;
    let big_icon = LoadImageW(hinst, 1 as _, IMAGE_ICON, (96.0 * scale) as i32, (96.0 * scale) as i32, 0) as HICON;
    let class = wide("NeramitCutSetup");
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinst,
        hIcon: icon,
        hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: class.as_ptr(),
        hIconSm: small,
    };
    RegisterClassExW(&wc);
    let s = |v: i32| (v as f32 * scale).round() as i32;
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
    let mut r = RECT { left: 0, top: 0, right: s(520), bottom: s(380) };
    AdjustWindowRectEx(&mut r, style, 0, 0);
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    let x = (GetSystemMetrics(SM_CXSCREEN) - w) / 2;
    let y = (GetSystemMetrics(SM_CYSCREEN) - h) / 2;
    let title = wide(&format!("{} - Neramit Cut {VERSION}", t("Setup", "ติดตั้ง")));
    let hwnd = CreateWindowExW(0, class.as_ptr(), title.as_ptr(), style, x, y, w, h, std::ptr::null_mut(), std::ptr::null_mut(), hinst, std::ptr::null());

    // controls are laid out in 96-dpi units
    let mk = |class: &str, text: &str, style: u32, ex: u32, x: i32, y: i32, w: i32, hh: i32, id: usize, fnt: HFONT| {
        let (c, tx) = (wide(class), wide(text));
        let hc = CreateWindowExW(ex, c.as_ptr(), tx.as_ptr(), WS_CHILD | style, s(x), s(y), s(w), s(hh), hwnd, id as HMENU, hinst, std::ptr::null());
        SendMessageW(hc, WM_SETFONT, fnt as usize, 1);
        hc
    };
    let tab = WS_TABSTOP;
    let mut controls = Vec::new();
    // welcome
    let up = upgrade();
    let welcome = match up {
        Some(old) => format!(
            "{}\n\n{}",
            if thai() {
                format!("Neramit Cut {old} ติดตั้งอยู่แล้วในเครื่องนี้ โปรแกรมติดตั้งจะอัปเดตเป็นเวอร์ชัน {VERSION} ในโฟลเดอร์เดิม")
            } else {
                format!("Neramit Cut {old} is already installed. Setup will update it to version {VERSION} in the same folder.")
            },
            t("Your projects and settings are kept.\n\nClick Next to continue, or Cancel to exit Setup.", "ไฟล์งานและค่าตั้งของคุณจะยังอยู่ครบ\n\nกด ถัดไป เพื่อดำเนินการต่อ หรือ ยกเลิก เพื่อออก")
        ),
        None => format!(
            "{}\n\n{}\n\n{}",
            if thai() { format!("โปรแกรมนี้จะติดตั้ง Neramit Cut {VERSION} ลงในเครื่องของคุณ") } else { format!("This will install Neramit Cut {VERSION} on your computer.") },
            t(
                "Neramit Cut is installed for your Windows account, so no administrator rights are needed.",
                "Neramit Cut จะติดตั้งสำหรับบัญชี Windows ของคุณ จึงไม่ต้องใช้สิทธิ์ผู้ดูแลระบบ (Admin)",
            ),
            t("Click Next to continue, or Cancel to exit Setup.", "กด ถัดไป เพื่อดำเนินการต่อ หรือ ยกเลิก เพื่อออก")
        ),
    };
    // Welcome and Finish: the text sits right of the picture
    let (rx, rw) = (BANNER_W + 20, 520 - BANNER_W - 40);
    let welcome_text = mk("STATIC", &welcome, 0, 0, rx, 92, rw, 220, 0, f);
    controls.push((welcome_text, WELCOME));
    // license
    controls.push((
        mk(
            "STATIC",
            t(
                "Please read the agreement below, then select \"I accept\" to continue.",
                "กรุณาอ่านข้อตกลงด้านล่าง แล้วเลือก \"ฉันยอมรับข้อตกลง\" เพื่อติดตั้งต่อ",
            ),
            0,
            0,
            32,
            76,
            456,
            20,
            0,
            f,
        ),
        LICENSE,
    ));
    let license_src = if thai() { LICENSE_TH } else { LICENSE_EN };
    let es = (ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32 | WS_VSCROLL;
    // a rich edit control (bold headings, bullets); a plain edit control if it cannot be loaded.
    // It has no border of its own: paint() draws a thin frame round it.
    let rich = !LoadLibraryW(wide("Msftedit.dll").as_ptr()).is_null();
    let license_box = mk(if rich { "RICHEDIT50W" } else { "EDIT" }, "", tab | es, 0, 33, 101, 454, 182, 0, f);
    if rich {
        const EM_SETBKGNDCOLOR: u32 = 0x0443;
        const EM_SETTEXTEX: u32 = 0x0461;
        #[repr(C)]
        struct SetTextEx {
            flags: u32,
            codepage: u32,
        }
        SendMessageW(license_box, EM_SETBKGNDCOLOR, 0, 0x00FFFFFF);
        let mut rtf = license_rtf(license_src, thai()).into_bytes();
        rtf.push(0);
        // the RTF is plain ASCII (other characters are \u escapes), so the ANSI code page reads it
        let st = SetTextEx { flags: 0, codepage: 0 };
        SendMessageW(license_box, EM_SETTEXTEX, &st as *const SetTextEx as WPARAM, rtf.as_ptr() as LPARAM);
    } else {
        // set after the font, so the edit control measures the text (and shows its scroll bar)
        set_text(license_box, &crlf(&license_plain(license_src)));
    }
    // room between the text and the frame
    let mut inner: RECT = std::mem::zeroed();
    GetClientRect(license_box, &mut inner);
    let inner = RECT { left: inner.left + s(12), top: inner.top + s(8), right: inner.right - s(10), bottom: inner.bottom - s(6) };
    SendMessageW(license_box, EM_SETRECTNP, 0, &inner as *const RECT as LPARAM);
    SendMessageW(license_box, EM_SETSEL, 0, 0);
    SetWindowSubclass(license_box, Some(license_keys), 1, 0);
    controls.push((license_box, LICENSE));
    let accept = mk("BUTTON", t("I &accept the agreement", "ฉัน&ยอมรับข้อตกลง"), tab | WS_GROUP | BS_AUTORADIOBUTTON as u32, 0, 32, 294, 220, 22, ID_ACCEPT, f);
    let decline = mk("BUTTON", t("I &do not accept the agreement", "ฉัน&ไม่ยอมรับข้อตกลง"), BS_AUTORADIOBUTTON as u32, 0, 260, 294, 228, 22, ID_DECLINE, f);
    set_check(decline, true);
    controls.push((accept, LICENSE));
    controls.push((decline, LICENSE));
    // folder
    controls.push((mk("STATIC", t("Setup will install Neramit Cut into the following folder.\n\nTo continue, click Next. To choose a different folder, click Browse.", "โปรแกรมจะติดตั้ง Neramit Cut ลงในโฟลเดอร์ต่อไปนี้\n\nกด ถัดไป เพื่อดำเนินการต่อ หรือกด เลือก... เพื่อเปลี่ยนโฟลเดอร์"), 0, 0, 32, 84, 456, 64, 0, f), FOLDER));
    let folder_edit = mk("EDIT", &default_dir().to_string_lossy(), tab | ES_AUTOHSCROLL as u32, WS_EX_CLIENTEDGE, 32, 156, 360, 24, 0, f);
    controls.push((folder_edit, FOLDER));
    controls.push((mk("BUTTON", t("B&rowse...", "เ&ลือก..."), tab | BS_PUSHBUTTON as u32, 0, 400, 155, 88, 26, ID_BROWSE, f), FOLDER));
    let mb = (APP.len() as f64 * 2.3 / 1_048_576.0).ceil();
    let space_text = mk("STATIC", &format!("{} {mb:.0} MB", t("Space required:", "พื้นที่ที่ต้องใช้:")), 0, 0, 32, 196, 456, 20, 0, f);
    controls.push((space_text, FOLDER));
    // tasks
    controls.push((mk("STATIC", t("Select the additional tasks you would like Setup to perform, then click Next.", "เลือกสิ่งที่ต้องการให้โปรแกรมติดตั้งทำเพิ่มเติม แล้วกด ถัดไป"), 0, 0, 32, 80, 456, 34, 0, f), TASKS));
    controls.push((mk("STATIC", t("Shortcuts:", "ทางลัด:"), 0, 0, 32, 112, 456, 20, 0, fb), TASKS));
    let chk_desktop = mk("BUTTON", t("Create a &desktop shortcut", "สร้างทางลัดบน&เดสก์ท็อป"), tab | BS_AUTOCHECKBOX as u32, 0, 44, 132, 440, 22, 0, f);
    let chk_start = mk("BUTTON", t("Create a &Start menu shortcut", "สร้างทางลัดใน&เมนูเริ่ม"), tab | BS_AUTOCHECKBOX as u32, 0, 44, 154, 440, 22, 0, f);
    for c in [chk_desktop, chk_start] {
        controls.push((c, TASKS));
    }
    let prev = Tasks::previous();
    set_check(chk_desktop, prev.desktop);
    set_check(chk_start, prev.start_menu);
    // ready
    controls.push((
        mk(
            "STATIC",
            t(
                "Click Install to continue with the installation, or click Back if you want to review or change any settings.",
                "กด ติดตั้ง เพื่อเริ่มติดตั้ง หรือกด ย้อนกลับ หากต้องการตรวจสอบหรือเปลี่ยนการตั้งค่า",
            ),
            0,
            0,
            32,
            76,
            456,
            34,
            0,
            f,
        ),
        READY,
    ));
    let ready_text = mk("EDIT", "", tab | es, WS_EX_CLIENTEDGE, 32, 112, 456, 200, 0, f);
    controls.push((ready_text, READY));
    // installing
    let status_text = mk("STATIC", "", 0, 0, 32, 110, 456, 20, 0, f);
    let progress = mk("msctls_progress32", "", 0, 0, 32, 136, 456, 18, 0, f);
    SendMessageW(progress, PBM_SETRANGE32, 0, 100);
    controls.push((status_text, INSTALLING));
    controls.push((progress, INSTALLING));
    // finish
    let finish_text = mk("STATIC", "", 0, 0, rx, 92, rw, 144, 0, f);
    let chk_launch = mk("BUTTON", t("&Launch Neramit Cut", "เ&ปิด Neramit Cut"), tab | BS_AUTOCHECKBOX as u32, 0, rx, 250, rw, 22, 0, f);
    set_check(chk_launch, true);
    controls.push((finish_text, FINISH));
    controls.push((chk_launch, FINISH));
    // footer
    let back = mk("BUTTON", t("< &Back", "< &ย้อนกลับ"), WS_VISIBLE | tab | BS_PUSHBUTTON as u32, 0, 236, 341, 84, 28, ID_BACK, f);
    let nextb = mk("BUTTON", t("&Next >", "&ถัดไป >"), WS_VISIBLE | tab | BS_DEFPUSHBUTTON as u32, 0, 324, 341, 84, 28, ID_NEXT, f);
    let cancel = mk("BUTTON", t("Cancel", "ยกเลิก"), WS_VISIBLE | tab | BS_PUSHBUTTON as u32, 0, 420, 341, 84, 28, ID_CANCEL, f);

    let _ = UI.set(Ui {
        hwnd,
        scale,
        font: f,
        title_font: ft,
        big_font: fbig,
        white: CreateSolidBrush(0x00FFFFFF),
        footer: CreateSolidBrush(0x00F3F3F3),
        // the program's blue (#1A6BD8, as COLORREF 0x00BBGGRR)
        banner: CreateSolidBrush(0x00D86B1A),
        icon,
        big_icon,
        controls,
        back,
        next: nextb,
        cancel,
        folder_edit,
        accept,
        license: license_box,
        chk_desktop,
        chk_start,
        ready_text,
        progress,
        status_text,
        finish_text,
        chk_launch,
    });
    show_page(WELCOME);
    if let Ok(spec) = std::env::var("NERAMIT_SETUP_SHOT") {
        SetWindowPos(hwnd, std::ptr::null_mut(), -10000, -10000, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        qa_shots(UI.get().unwrap(), &spec);
        return;
    }
    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
    SetFocus(nextb);
    let mut m: MSG = std::mem::zeroed();
    while GetMessageW(&mut m, std::ptr::null_mut(), 0, 0) > 0 {
        // Tab / Enter / Esc between the controls, as in a dialog box
        if IsDialogMessageW(hwnd, &m) == 0 {
            TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let quiet = args.iter().any(|a| a.eq_ignore_ascii_case("/s") || a == "--quiet");
    let relaunch = args.iter().any(|a| a == "--relaunch");
    let _ = UPGRADE.set(reg_value("DisplayVersion").filter(|_| reg_value("InstallLocation").map(|d| Path::new(&d).join("NeramitCut.exe").exists()).unwrap_or(false)));
    if quiet {
        let waiting = args.iter().position(|a| a == "--wait-pid").map(|index| {
            let pid = args.get(index + 1).and_then(|s| s.parse::<u32>().ok()).ok_or_else(|| "Invalid --wait-pid argument.".to_string())?;
            wait_for_program(pid, 30_000)
        }).unwrap_or(Ok(()));
        match waiting.and_then(|_| install(&default_dir(), Tasks::previous(), &|_| {})) {
            Ok((exe, _)) => {
                if relaunch {
                    let _ = std::process::Command::new(&exe).current_dir(exe.parent().unwrap_or(Path::new("."))).spawn();
                }
            }
            Err(error) => {
                message(std::ptr::null_mut(), &error, MB_ICONERROR);
                std::process::exit(1);
            }
        }
        return;
    }
    // SAFETY: the wizard runs on this thread only
    unsafe { run_wizard() };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;
    use std::sync::atomic::AtomicUsize;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("neramit-setup-test-{}-{stamp}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            // Only this test's unique temporary directory is removed; no app registration occurs.
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn staged_install_preserves_unrelated_files() {
        let dir = TestDir::new();
        let project = dir.0.join("parts.xml");
        std::fs::write(&project, b"user project").unwrap();
        let exe = dir.0.join("NeramitCut.exe");
        assert!(replace_program(&exe, b"new program").unwrap().is_none());
        assert_eq!(std::fs::read(&exe).unwrap(), b"new program");
        assert_eq!(std::fs::read(project).unwrap(), b"user project");
    }

    #[test]
    fn replacement_can_restore_previous_program() {
        let dir = TestDir::new();
        let exe = dir.0.join("NeramitCut.exe");
        std::fs::write(&exe, b"previous program").unwrap();
        let backup = replace_program(&exe, b"replacement program").unwrap().unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"replacement program");
        assert_eq!(std::fs::read(&backup).unwrap(), b"previous program");
        restore_program(&exe, &backup).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"previous program");
        assert!(!backup.exists());
    }

    #[test]
    fn locked_program_is_not_removed_on_failure() {
        let dir = TestDir::new();
        let exe = dir.0.join("NeramitCut.exe");
        std::fs::write(&exe, b"previous program").unwrap();
        let locked = std::fs::OpenOptions::new().read(true).share_mode(0).open(&exe).unwrap();
        assert!(replace_program(&exe, b"replacement program").is_err());
        drop(locked);
        assert_eq!(std::fs::read(&exe).unwrap(), b"previous program");
        assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
    }

    #[test]
    fn wait_rejects_invalid_and_self_process() {
        assert!(wait_for_program(0, 1).is_err());
        assert!(wait_for_program(std::process::id(), 1).is_err());
    }

    #[test]
    fn wait_accepts_process_that_has_exited() {
        let mut child = std::process::Command::new("cmd.exe").args(["/d", "/c", "exit", "0"]).spawn().unwrap();
        let pid = child.id();
        assert!(wait_for_program(pid, 5_000).is_ok());
        assert!(child.wait().unwrap().success());
    }
}
