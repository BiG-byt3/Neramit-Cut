//! Online updates from the GitHub releases of the project (tools/release.py makes them).
//!
//! `latest.json` of the newest release names the version, the size and SHA-256 of
//! NeramitCut-Setup-<version>.exe and of the standalone NeramitCut-<version>.exe, each with an
//! ECDSA P-256 signature over "NeramitCut|<version>|<sha256>" (Setup) or
//! "NeramitCutPortable|<version>|<sha256>" (standalone exe) made with the studio's private key.
//! Only a file that matches its signed digest is ever run. Nothing happens on its own: the user
//! starts a check with Help > Check for updates (or the button in About).
//!
//! An installed copy (the one NeramitCut-Setup put in its folder) downloads Setup and runs it as
//! `/S --wait-pid <pid> [--relaunch]` once the window has closed and the session is saved.
//! A standalone copy downloads the new exe, which replaces only the old exe file
//! (`--finish-portable-update <target> <pid> [--relaunch]`) after the old process has exited.
//! Materials, settings and saved jobs are never touched. HTTP goes through WinHTTP (system proxy)
//! and the checks through Windows CNG, so no extra libraries are needed.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Networking::WinHttp::*;
use windows_sys::Win32::Security::Cryptography::{
    BCryptCloseAlgorithmProvider, BCryptDestroyKey, BCryptHash, BCryptImportKeyPair, BCryptOpenAlgorithmProvider, BCryptVerifySignature,
    BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDSA_P256_ALGORITHM, BCRYPT_ECDSA_PUBLIC_P256_MAGIC, BCRYPT_SHA256_ALG_HANDLE,
};
use windows_sys::Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_IGNORE_ACL_ERRORS};
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    LoadImageW, MessageBoxW, PostQuitMessage, SendMessageW, HICON, IDCANCEL, IMAGE_ICON, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_TOPMOST,
};

/// Where releases are published; `NERAMIT_UPDATE_BASE` points elsewhere for tests (http allowed).
const REPO_URL: &str = "https://github.com/BiG-byt3/Neramit-Cut";
/// The public half of the studio's release signing key (X || Y), see tools/release.py.
const PUBLIC_KEY: &str = "079365895bc684e6f65070f57d60cfd988eb0a52935c89e6f0a3051c2bb05defeb880ad40d7ee125f0699e862fa0a16759b8d1fd8e28496b9741d57d7800658f";
const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;
const PRODUCT: &str = "Neramit Cut";

#[derive(Clone, Debug)]
struct Artifact {
    size: u64,
    sha256: String,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    installer: Artifact,
    portable: Option<Artifact>,
    manifest: Vec<u8>,
    pub notes_en: String,
    pub notes_th: String,
}

#[derive(Clone, Debug)]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    /// the release and the share downloaded so far (0..1)
    Downloading(Release, f32),
    Ready(Release, PathBuf),
    Failed(String),
}

static STATUS: Mutex<Status> = Mutex::new(Status::Idle);
/// Each check or download gets a number; Cancel moves it on, so a late answer is dropped.
static GEN: AtomicU32 = AtomicU32::new(0);
/// The downloaded file to run when the window closes, and whether it reopens the program.
static INSTALL_ON_EXIT: Mutex<Option<(Release, PathBuf, bool)>> = Mutex::new(None);
/// Texts in Thai (set from the program's language on the UI thread).
static THAI: AtomicBool = AtomicBool::new(true);

fn tt(en: &'static str, th: &'static str) -> &'static str {
    if THAI.load(Ordering::Relaxed) {
        th
    } else {
        en
    }
}

pub fn status() -> Status {
    STATUS.lock().map(|s| s.clone()).unwrap_or(Status::Idle)
}

fn set(s: Status) {
    if let Ok(mut g) = STATUS.lock() {
        *g = s;
    }
}

/// Sets the status only when no newer check or download has started since `gen`.
fn set_for(gen: u32, s: Status) {
    if let Ok(mut g) = STATUS.lock() {
        if GEN.load(Ordering::SeqCst) == gen {
            *g = s;
        }
    }
}

fn base() -> String {
    std::env::var("NERAMIT_UPDATE_BASE").unwrap_or_else(|_| REPO_URL.into()).trim_end_matches('/').to_string()
}

/// The release page in the browser.
pub fn page_url() -> String {
    format!("{}/releases/latest", base())
}

pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn parse_version(v: &str) -> Option<[u32; 3]> {
    let parts: Vec<_> = v.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0'))) {
        return None;
    }
    Some([parts[0].parse().ok()?, parts[1].parse().ok()?, parts[2].parse().ok()?])
}

pub fn is_newer(v: &str) -> bool {
    matches!((parse_version(v), parse_version(current())), (Some(new), Some(old)) if new > old)
}

/// This exe is the one NeramitCut-Setup installed, so Setup can replace it.
pub fn installed() -> bool {
    let (Ok(exe), Some(dir)) = (std::env::current_exe(), crate::setup::install_location()) else { return false };
    let norm = |p: &Path| p.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase();
    exe.parent().map(|d| norm(d) == norm(&dir)).unwrap_or(false)
}

fn fetch_latest() -> Result<Release, String> {
    let body = http_get(&format!("{}/releases/latest/download/latest.json", base()), 256 * 1024, &mut |_, _| true)?;
    parse_manifest(&body)
}

/// `--check-update [--download]`: read-only release diagnostics (JSON on stdout), used to verify a
/// published release end to end.
pub fn cli_check(download_assets: bool) -> i32 {
    THAI.store(false, Ordering::Relaxed);
    let result = fetch_latest().and_then(|rel| {
        let mut verified = Vec::new();
        if download_assets {
            for (name, artifact) in std::iter::once((asset_name(&rel.version, true), &rel.installer)).chain(rel.portable.as_ref().map(|a| (asset_name(&rel.version, false), a))) {
                let url = format!("{}/releases/download/v{}/{name}", base(), rel.version);
                validate_data(&http_get(&url, artifact.size, &mut |_, _| true)?, artifact)?;
                verified.push(name);
            }
        }
        Ok(serde_json::json!({"current": current(), "latest": rel.version, "newer_available": is_newer(&rel.version), "signature_verified": true, "portable_available": rel.portable.is_some(), "installed": installed(), "verified_assets": verified}))
    });
    match result {
        Ok(value) => {
            println!("{value}");
            0
        }
        Err(error) => {
            eprintln!("{}", serde_json::json!({ "error": error }));
            1
        }
    }
}

fn asset_name(version: &str, setup: bool) -> String {
    format!("NeramitCut-{}{version}.exe", if setup { "Setup-" } else { "" })
}

fn parse_manifest(body: &[u8]) -> Result<Release, String> {
    let j: serde_json::Value = serde_json::from_slice(body).map_err(|e| format!("latest.json: {e}"))?;
    let s = |k: &str| j.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let version = s("version");
    if parse_version(&version).is_none() {
        return Err(tt("The update version is invalid.", "หมายเลขเวอร์ชันของอัปเดตไม่ถูกต้อง").into());
    }
    let artifact = |prefix: &str, product: &str| -> Result<Artifact, String> {
        let size = j.get(format!("{prefix}size")).and_then(|v| v.as_u64()).unwrap_or(0);
        let sha256 = s(&format!("{prefix}sha256")).to_lowercase();
        let sig = hex(&s(&format!("{prefix}signature"))).filter(|b| b.len() == 64);
        if size == 0
            || size > MAX_DOWNLOAD
            || hex(&sha256).filter(|b| b.len() == 32).is_none()
            || !sig.map(|sig| verify(format!("{product}|{version}|{sha256}").as_bytes(), &sig)).unwrap_or(false)
        {
            return Err(tt("The update information is not signed by BiGByte Studio.", "ข้อมูลอัปเดตไม่ได้ลงลายมือชื่อโดย BiGByte Studio").into());
        }
        Ok(Artifact { size, sha256 })
    };
    let installer = artifact("", "NeramitCut")?;
    let portable = if j.get("portable_sha256").is_some() { Some(artifact("portable_", "NeramitCutPortable")?) } else { None };
    Ok(Release { version, installer, portable, manifest: body.to_vec(), notes_en: s("notes_en"), notes_th: s("notes_th") })
}

pub fn can_download(rel: &Release) -> bool {
    installed() || rel.portable.is_some()
}

fn selected_artifact(rel: &Release, is_installed: bool) -> Result<&Artifact, String> {
    if is_installed {
        Ok(&rel.installer)
    } else {
        rel.portable.as_ref().ok_or_else(|| tt("This release has no update for the standalone program. Open the download page instead.", "รุ่นนี้ไม่มีไฟล์อัปเดตสำหรับโปรแกรมแบบไม่ต้องติดตั้ง กรุณาเปิดหน้าดาวน์โหลดแทน").into())
    }
}

fn validate_data(data: &[u8], artifact: &Artifact) -> Result<(), String> {
    if data.len() as u64 != artifact.size || to_hex(&sha256(data)) != artifact.sha256 {
        return Err(tt("The downloaded file does not match the signed release.", "ไฟล์ที่ดาวน์โหลดไม่ตรงกับรุ่นที่ลงลายมือชื่อไว้").into());
    }
    Ok(())
}

/// Starts a check in the background.
fn start_check() {
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    set(Status::Checking);
    std::thread::spawn(move || {
        let s = match fetch_latest() {
            Ok(rel) if is_newer(&rel.version) => Status::Available(rel),
            Ok(_) => Status::UpToDate,
            Err(e) => Status::Failed(e),
        };
        set_for(gen, s);
    });
}

/// Downloads Setup or the standalone exe in the background and checks it against the signed digest.
fn start_download(rel: Release) {
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    set(Status::Downloading(rel.clone(), 0.0));
    let is_installed = installed();
    std::thread::spawn(move || {
        let name = asset_name(&rel.version, is_installed);
        let url = format!("{}/releases/download/v{}/{name}", base(), rel.version);
        let mut last = 0.0f32;
        let r = selected_artifact(&rel, is_installed)
            .and_then(|artifact| {
                http_get(&url, artifact.size, &mut |got, total| {
                    let total = if total > 0 { total } else { artifact.size };
                    let f = (got as f32 / total as f32).min(1.0);
                    if f - last >= 0.01 {
                        last = f;
                        set_for(gen, Status::Downloading(rel.clone(), f));
                    }
                    GEN.load(Ordering::SeqCst) == gen
                })
            })
            .and_then(|data| {
                validate_data(&data, selected_artifact(&rel, is_installed)?)?;
                // kept with the program's own data (not %TEMP%); an earlier download is replaced
                let dir = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join(PRODUCT).join("Updates").join(&rel.version);
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let path = dir.join(&name);
                std::fs::write(&path, &data).map_err(|e| e.to_string())?;
                std::fs::write(dir.join("latest.json"), &rel.manifest).map_err(|e| e.to_string())?;
                Ok(path)
            });
        set_for(
            gen,
            match r {
                Ok(p) => Status::Ready(rel, p),
                Err(e) => Status::Failed(e),
            },
        );
    });
}

/// Stops the running check or download (its answer is dropped).
fn cancel() {
    GEN.fetch_add(1, Ordering::SeqCst);
    set(Status::Idle);
}

pub fn install_on_exit(rel: Release, path: PathBuf, relaunch: bool) {
    if let Ok(mut g) = INSTALL_ON_EXIT.lock() {
        *g = Some((rel, path, relaunch));
    }
}

/// Called after the window has closed and the session is saved: starts a downloaded update.
pub fn on_exit() -> Result<(), String> {
    let Some((rel, p, relaunch)) = INSTALL_ON_EXIT.lock().ok().and_then(|mut g| g.take()) else { return Ok(()) };
    let result: Result<(), String> = (|| {
        // checked again now: the download may have waited for days
        let is_installed = installed();
        let trusted = parse_manifest(&rel.manifest)?;
        let data = std::fs::read(&p).map_err(|e| e.to_string())?;
        validate_data(&data, selected_artifact(&trusted, is_installed)?)?;
        let mut c = std::process::Command::new(&p);
        if is_installed {
            c.args(["/S", "--wait-pid", &std::process::id().to_string()]);
        } else {
            c.arg("--finish-portable-update").arg(std::env::current_exe().map_err(|e| e.to_string())?).arg(std::process::id().to_string());
        }
        if relaunch {
            c.arg("--relaunch");
        }
        c.spawn().map_err(|e| format!("{} ({e})", tt("Could not start the update.", "เริ่มการอัปเดตไม่ได้")))?;
        Ok(())
    })();
    if let Err(ref error) = result {
        set(Status::Failed(error.clone()));
    }
    result
}

pub fn error_box(text: &str) {
    let (t, c) = (wide(text), wide(PRODUCT));
    // SAFETY: NUL-terminated strings; no owner window (the main window may be gone)
    unsafe { MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), MB_OK | MB_ICONERROR | MB_TOPMOST) };
}

/// The downloaded standalone exe runs this before it opens any window: it waits for the old
/// program to exit, then puts itself in place of the old exe file.
pub fn finish_portable_update(args: &[String]) -> i32 {
    THAI.store(crate::settings::load_interface().language != "English", Ordering::Relaxed);
    let finish = || -> Result<(), String> {
        let bad = || tt("Invalid update target.", "ตำแหน่งที่จะอัปเดตไม่ถูกต้อง").to_string();
        let target = args.first().map(PathBuf::from).filter(|p| p.is_absolute() && p.extension().map(|e| e.eq_ignore_ascii_case("exe")).unwrap_or(false)).ok_or_else(bad)?;
        let pid = args.get(1).and_then(|p| p.parse::<u32>().ok()).filter(|pid| *pid > 0 && *pid != std::process::id()).ok_or_else(bad)?;
        let source = std::env::current_exe().map_err(|e| e.to_string())?;
        if source == target {
            return Err(bad());
        }
        let manifest = std::fs::read(source.parent().ok_or_else(bad)?.join("latest.json")).map_err(|e| e.to_string())?;
        let rel = parse_manifest(&manifest)?;
        if rel.version != current() {
            return Err(tt("The update version is invalid.", "หมายเลขเวอร์ชันของอัปเดตไม่ถูกต้อง").into());
        }
        let artifact = selected_artifact(&rel, false)?;
        validate_data(&std::fs::read(&source).map_err(|e| e.to_string())?, artifact)?;
        wait_for_exit(pid)?;
        let backup = replace_portable(&source, &target, artifact)?;
        if args.iter().any(|a| a == "--relaunch") {
            if let Err(error) = relaunch(&target) {
                let recovery = restore_portable(&target, &backup)
                    .map(|_| tt("The previous program was restored.", "คืนโปรแกรมรุ่นเดิมแล้ว").to_string())
                    .unwrap_or_else(|e| format!("{} {} ({e})", tt("The previous program is kept at", "โปรแกรมรุ่นเดิมเก็บไว้ที่"), backup.display()));
                return Err(format!("{} ({error})\n\n{recovery}", tt("Could not open Neramit Cut.", "เปิด Neramit Cut ไม่ได้")));
            }
        }
        let _ = std::fs::remove_file(backup);
        Ok(())
    };
    match finish() {
        Ok(()) => 0,
        Err(e) => {
            error_box(&format!("{}\n\n{e}", tt("Could not complete the update. Your materials, settings and saved jobs have not been changed.", "อัปเดตไม่สำเร็จ วัสดุ ค่าตั้ง และงานที่บันทึกไว้ไม่ได้ถูกเปลี่ยนแปลง")));
            1
        }
    }
}

/// Opens the updated program. A freshly written exe is sometimes stopped right after it starts
/// (an antivirus scan of the new file), so a start that ends within two seconds is tried again.
fn relaunch(target: &Path) -> std::io::Result<()> {
    let dir = target.parent().unwrap_or(Path::new("."));
    for attempt in 0..3 {
        let mut child = std::process::Command::new(target).current_dir(dir).spawn()?;
        std::thread::sleep(std::time::Duration::from_millis(2000));
        if child.try_wait()?.is_none() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(500 * (attempt + 1)));
    }
    // Keep the rollback path active when every restart exits immediately.
    Err(std::io::Error::other(tt("Neramit Cut closed immediately after restarting.", "Neramit Cut ปิดทันทีหลังจากเริ่มใหม่")))
}

fn wait_for_exit(pid: u32) -> Result<(), String> {
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    // SAFETY: the process handle is opened here, waited on and closed here.
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if h.is_null() {
            return if windows_sys::Win32::Foundation::GetLastError() == windows_sys::Win32::Foundation::ERROR_INVALID_PARAMETER {
                Ok(())
            } else {
                Err(tt("Could not wait for Neramit Cut to close.", "รอให้ Neramit Cut ปิดไม่ได้").into())
            };
        }
        let result = WaitForSingleObject(h, 60_000);
        windows_sys::Win32::Foundation::CloseHandle(h);
        if result != windows_sys::Win32::Foundation::WAIT_OBJECT_0 {
            return Err(tt("Close Neramit Cut before updating.", "กรุณาปิด Neramit Cut ก่อนอัปเดต").into());
        }
    }
    Ok(())
}

fn replace_portable(source: &Path, target: &Path, artifact: &Artifact) -> Result<PathBuf, String> {
    use std::io::Write;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let suffix = format!("{}-{stamp}", std::process::id());
    let staged = target.with_extension(format!("exe.{suffix}.new"));
    let backup = target.with_extension(format!("exe.{suffix}.rollback"));
    let result = (|| {
        let data = std::fs::read(source).map_err(|e| e.to_string())?;
        validate_data(&data, artifact)?;
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&staged).map_err(|e| e.to_string())?;
        file.write_all(&data).and_then(|_| file.sync_all()).map_err(|e| e.to_string())?;
        drop(file);
        validate_data(&std::fs::read(&staged).map_err(|e| e.to_string())?, artifact)?;
        let (dest, src, old) = (wide(&target.to_string_lossy()), wide(&staged.to_string_lossy()), wide(&backup.to_string_lossy()));
        // A user may have Modify permission without WRITE_DAC. The staged sibling already
        // inherits this directory's ACL, so inability to merge a file ACL must not block updates.
        // SAFETY: NUL-terminated paths outlive the call and are all on the same filesystem.
        if unsafe { ReplaceFileW(dest.as_ptr(), src.as_ptr(), old.as_ptr(), REPLACEFILE_IGNORE_ACL_ERRORS, std::ptr::null(), std::ptr::null()) } == 0 {
            let error = std::io::Error::last_os_error();
            if !target.exists() && backup.is_file() {
                if let Err(recovery) = std::fs::rename(&backup, target) {
                    return Err(format!("Could not replace {}: {error}. Could not restore it ({recovery}). The previous program is kept at {}.", target.display(), backup.display()));
                }
            }
            let recovery = if backup.is_file() { format!(" The previous program is kept at {}.", backup.display()) } else { String::new() };
            return Err(format!("Could not replace {}: {error}.{recovery}", target.display()));
        }
        Ok(backup.clone())
    })();
    let _ = std::fs::remove_file(staged);
    result
}

fn restore_portable(target: &Path, backup: &Path) -> Result<(), String> {
    if !target.exists() {
        return std::fs::rename(backup, target).map_err(|e| e.to_string());
    }
    let (dest, src) = (wide(&target.to_string_lossy()), wide(&backup.to_string_lossy()));
    // SAFETY: NUL-terminated paths outlive the call; no additional backup is requested.
    if unsafe { ReplaceFileW(dest.as_ptr(), src.as_ptr(), std::ptr::null(), REPLACEFILE_IGNORE_ACL_ERRORS, std::ptr::null(), std::ptr::null()) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

pub fn open_page() {
    let (url, verb) = (wide(&page_url()), wide("open"));
    // SAFETY: NUL-terminated strings that outlive the call
    unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), url.as_ptr(), std::ptr::null(), std::ptr::null(), windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL);
    }
}

// ------------------------------------------------------------------ dialogs

const ID_DOWNLOAD: i32 = 100;
const ID_PAGE: i32 = 101;
const ID_LATER: i32 = 102;
const ID_NOW: i32 = 103;
const ID_ON_CLOSE: i32 = 104;
const ID_RETRY: i32 = 105;

/// The work behind a progress dialog finished on its own (not through Cancel).
static PROGRESS_DONE: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn progress_proc(hwnd: HWND, msg: u32, _wp: usize, _lp: isize, marquee: isize) -> i32 {
    if msg == TDN_CREATED as u32 && marquee != 0 {
        SendMessageW(hwnd, TDM_SET_PROGRESS_BAR_MARQUEE as u32, 1, 30);
    }
    if msg == TDN_TIMER as u32 {
        match status() {
            Status::Checking => {}
            Status::Downloading(_, f) => {
                SendMessageW(hwnd, TDM_SET_PROGRESS_BAR_POS as u32, (f * 100.0).round() as usize, 0);
            }
            _ => {
                PROGRESS_DONE.store(true, Ordering::SeqCst);
                SendMessageW(hwnd, TDM_CLICK_BUTTON as u32, IDCANCEL as usize, 0);
            }
        }
    }
    0
}

fn app_icon() -> HICON {
    // SAFETY: icon resource 1 of this exe (winresource), loaded once per dialog at 32 px scaled
    unsafe {
        let hinst = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null());
        let px = (32.0 * crate::ui::theme::scale()).round() as i32;
        LoadImageW(hinst, 1 as _, IMAGE_ICON, px, px, 0) as HICON
    }
}

/// A task dialog; returns the id of the button pressed (IDCANCEL for Esc / Cancel / the close box).
unsafe fn task_dialog(owner: HWND, instruction: &str, content: &str, expanded: Option<(&str, &str)>, buttons: &[(i32, &str)], flags: i32, progress: Option<bool>) -> i32 {
    let title = wide(PRODUCT);
    let (instr, body) = (wide(instruction), wide(content));
    let texts: Vec<Vec<u16>> = buttons.iter().map(|(_, t)| wide(t)).collect();
    let btns: Vec<TASKDIALOG_BUTTON> = buttons.iter().zip(&texts).map(|((id, _), t)| TASKDIALOG_BUTTON { nButtonID: *id, pszButtonText: t.as_ptr() }).collect();
    let (ex_text, ex_ctl) = expanded.map(|(a, b)| (wide(a), wide(b))).unwrap_or_default();
    let mut cfg: TASKDIALOGCONFIG = std::mem::zeroed();
    cfg.cbSize = std::mem::size_of::<TASKDIALOGCONFIG>() as u32;
    cfg.hwndParent = owner;
    cfg.dwFlags = flags | TDF_ALLOW_DIALOG_CANCELLATION | TDF_POSITION_RELATIVE_TO_WINDOW;
    let icon = app_icon();
    if !icon.is_null() {
        cfg.dwFlags |= TDF_USE_HICON_MAIN;
        cfg.Anonymous1 = TASKDIALOGCONFIG_0 { hMainIcon: icon };
    }
    cfg.pszWindowTitle = title.as_ptr();
    cfg.pszMainInstruction = instr.as_ptr();
    cfg.pszContent = body.as_ptr();
    if !btns.is_empty() {
        cfg.cButtons = btns.len() as u32;
        cfg.pButtons = btns.as_ptr();
        cfg.nDefaultButton = btns[0].nButtonID;
    }
    if expanded.is_some() {
        cfg.pszExpandedInformation = ex_text.as_ptr();
        cfg.pszCollapsedControlText = ex_ctl.as_ptr();
        cfg.pszExpandedControlText = ex_ctl.as_ptr();
    }
    if let Some(marquee) = progress {
        cfg.dwFlags |= TDF_CALLBACK_TIMER | if marquee { TDF_SHOW_MARQUEE_PROGRESS_BAR } else { TDF_SHOW_PROGRESS_BAR };
        cfg.dwCommonButtons = TDCBF_CANCEL_BUTTON;
        cfg.pfCallback = Some(progress_proc);
        cfg.lpCallbackData = marquee as isize;
    } else if btns.is_empty() {
        cfg.dwCommonButtons = TDCBF_OK_BUTTON;
    }
    let mut pressed = IDCANCEL;
    TaskDialogIndirect(&cfg, &mut pressed, std::ptr::null_mut(), std::ptr::null_mut());
    if !icon.is_null() {
        windows_sys::Win32::UI::WindowsAndMessaging::DestroyIcon(icon);
    }
    pressed
}

/// Runs a progress dialog until the background work ends; false when the user cancelled it.
unsafe fn wait_with_progress(owner: HWND, instruction: &str, content: &str, marquee: bool) -> bool {
    PROGRESS_DONE.store(false, Ordering::SeqCst);
    // the work may already be over (a fast or failed connection)
    if !matches!(status(), Status::Checking | Status::Downloading(..)) {
        return true;
    }
    task_dialog(owner, instruction, content, None, &[], 0, Some(marquee));
    if PROGRESS_DONE.load(Ordering::SeqCst) || !matches!(status(), Status::Checking | Status::Downloading(..)) {
        return true;
    }
    cancel();
    false
}

/// Help > Check for updates: check, show what is new, download, then install now or on close.
pub unsafe fn check_ui(owner: HWND) {
    THAI.store(crate::i18n::lang() == crate::i18n::Lang::Th, Ordering::Relaxed);
    if !matches!(status(), Status::Ready(..)) {
        start_check();
        if !wait_with_progress(owner, tt("Checking for updates…", "กำลังตรวจหาอัปเดต…"), tt("Connecting to the release page of Neramit Cut.", "กำลังเชื่อมต่อกับหน้าเผยแพร่ของ Neramit Cut"), true) {
            return;
        }
    }
    loop {
        match status() {
            Status::UpToDate => {
                let text = if THAI.load(Ordering::Relaxed) { format!("คุณใช้ Neramit Cut เวอร์ชันล่าสุดอยู่แล้ว ({})", current()) } else { format!("You have the latest version of Neramit Cut ({}).", current()) };
                task_dialog(owner, tt("Neramit Cut is up to date", "Neramit Cut เป็นเวอร์ชันล่าสุดแล้ว"), &text, None, &[], 0, None);
                set(Status::Idle);
                return;
            }
            Status::Failed(e) => {
                set(Status::Idle);
                let r = task_dialog(
                    owner,
                    tt("Could not update Neramit Cut", "อัปเดต Neramit Cut ไม่สำเร็จ"),
                    &e,
                    None,
                    &[(ID_RETRY, tt("Try again", "ลองอีกครั้ง")), (ID_PAGE, tt("Open the download page", "เปิดหน้าดาวน์โหลด")), (IDCANCEL, tt("Close", "ปิด"))],
                    0,
                    None,
                );
                match r {
                    ID_RETRY => {
                        start_check();
                        if !wait_with_progress(owner, tt("Checking for updates…", "กำลังตรวจหาอัปเดต…"), "", true) {
                            return;
                        }
                    }
                    ID_PAGE => {
                        open_page();
                        return;
                    }
                    _ => return,
                }
            }
            Status::Available(rel) => {
                let th = THAI.load(Ordering::Relaxed);
                let instruction = if th { format!("มี Neramit Cut เวอร์ชัน {} ให้อัปเดต", rel.version) } else { format!("Neramit Cut {} is available", rel.version) };
                let content = if th {
                    format!("เวอร์ชันที่ใช้อยู่: {}\nวัสดุ ค่าตั้ง และงานที่บันทึกไว้จะยังอยู่ครบหลังอัปเดต", current())
                } else {
                    format!("You have version {}.\nYour materials, settings and saved jobs are kept.", current())
                };
                let notes = if th && !rel.notes_th.is_empty() { &rel.notes_th } else { &rel.notes_en };
                let expanded = (!notes.is_empty()).then_some((notes.as_str(), tt("What's new", "มีอะไรใหม่")));
                let mut buttons = Vec::new();
                if can_download(&rel) {
                    buttons.push((ID_DOWNLOAD, tt("Download and install\nThe update is checked against the studio's signature before it runs.", "ดาวน์โหลดและติดตั้ง\nไฟล์อัปเดตจะถูกตรวจลายมือชื่อของสตูดิโอก่อนติดตั้ง")));
                }
                buttons.push((ID_PAGE, tt("Open the download page", "เปิดหน้าดาวน์โหลด")));
                buttons.push((ID_LATER, tt("Not now", "ไว้ภายหลัง")));
                match task_dialog(owner, &instruction, &content, expanded, &buttons, TDF_USE_COMMAND_LINKS | TDF_EXPANDED_BY_DEFAULT, None) {
                    ID_DOWNLOAD => {
                        start_download(rel);
                        if !wait_with_progress(owner, tt("Downloading the update…", "กำลังดาวน์โหลดอัปเดต…"), tt("You can keep working; this takes a moment.", "รอสักครู่"), false) {
                            return;
                        }
                    }
                    ID_PAGE => {
                        set(Status::Idle);
                        open_page();
                        return;
                    }
                    _ => {
                        set(Status::Idle);
                        return;
                    }
                }
            }
            Status::Ready(rel, path) => {
                let th = THAI.load(Ordering::Relaxed);
                let instruction = if th { format!("ดาวน์โหลด Neramit Cut {} เรียบร้อยแล้ว", rel.version) } else { format!("Neramit Cut {} has been downloaded", rel.version) };
                let r = task_dialog(
                    owner,
                    &instruction,
                    tt("Your session is saved before Neramit Cut closes.", "งานที่เปิดอยู่จะถูกบันทึกก่อนปิดโปรแกรม"),
                    None,
                    &[
                        (ID_NOW, tt("Update now\nNeramit Cut closes, updates and opens again.", "อัปเดตเดี๋ยวนี้\nปิด Neramit Cut อัปเดต แล้วเปิดใหม่ให้อัตโนมัติ")),
                        (ID_ON_CLOSE, tt("Update when I close Neramit Cut", "อัปเดตเมื่อฉันปิด Neramit Cut")),
                    ],
                    TDF_USE_COMMAND_LINKS,
                    None,
                );
                match r {
                    ID_NOW => {
                        install_on_exit(rel, path, true);
                        PostQuitMessage(0);
                    }
                    ID_ON_CLOSE => install_on_exit(rel, path, false),
                    // Esc: the download stays ready for the next check
                    _ => {}
                }
                return;
            }
            Status::Idle | Status::Checking | Status::Downloading(..) => return,
        }
    }
}

#[allow(dead_code)]
fn info_box(text: &str) {
    let (t, c) = (wide(text), wide(PRODUCT));
    // SAFETY: NUL-terminated strings
    unsafe { MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), MB_OK | MB_ICONINFORMATION) };
}

// ------------------------------------------------------------------ crypto and HTTP

fn hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn sha256(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    // SAFETY: the SHA-256 pseudo-handle (Windows 10+); input and output sizes are passed with the buffers
    unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, std::ptr::null(), 0, data.as_ptr(), data.len() as u32, out.as_mut_ptr(), 32) };
    out
}

/// ECDSA P-256 / SHA-256 signature (r || s) of `msg` against PUBLIC_KEY.
fn verify(msg: &[u8], sig: &[u8]) -> bool {
    let Some(key) = hex(PUBLIC_KEY) else { return false };
    let mut blob = Vec::with_capacity(8 + 64);
    blob.extend_from_slice(&BCRYPT_ECDSA_PUBLIC_P256_MAGIC.to_le_bytes());
    blob.extend_from_slice(&32u32.to_le_bytes());
    blob.extend_from_slice(&key);
    let hash = sha256(msg);
    // SAFETY: CNG handles are closed on every path; buffers are passed with their sizes
    unsafe {
        let mut alg = std::ptr::null_mut();
        if BCryptOpenAlgorithmProvider(&mut alg, BCRYPT_ECDSA_P256_ALGORITHM, std::ptr::null(), 0) != 0 {
            return false;
        }
        let mut k = std::ptr::null_mut();
        let mut ok = false;
        if BCryptImportKeyPair(alg, std::ptr::null_mut(), BCRYPT_ECCPUBLIC_BLOB, &mut k, blob.as_ptr(), blob.len() as u32, 0) == 0 {
            ok = BCryptVerifySignature(k, std::ptr::null(), hash.as_ptr(), 32, sig.as_ptr(), sig.len() as u32, 0) == 0;
            BCryptDestroyKey(k);
        }
        BCryptCloseAlgorithmProvider(alg, 0);
        ok
    }
}

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a WinHTTP handle this module opened
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// GET `url` (redirects followed, e.g. GitHub to its download host). `progress(received, total)`
/// returns false to stop the download.
fn http_get(url: &str, max_bytes: u64, progress: &mut dyn FnMut(u64, u64) -> bool) -> Result<Vec<u8>, String> {
    let bad = || format!("Bad address: {url}");
    let (secure, rest) = match (url.strip_prefix("https://"), url.strip_prefix("http://")) {
        (Some(r), _) => (true, r),
        (_, Some(r)) => (false, r),
        _ => return Err(bad()),
    };
    let (host_port, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().map_err(|_| bad())?),
        None => (host_port, if secure { 443 } else { 80 }),
    };
    let net_err = || tt("Could not connect to the update server. Check the internet connection.", "เชื่อมต่อเซิร์ฟเวอร์อัปเดตไม่ได้ กรุณาตรวจสอบการเชื่อมต่ออินเทอร์เน็ต").to_string();
    let too_large = || tt("The update download is too large.", "ไฟล์อัปเดตใหญ่เกินไป").to_string();
    let agent = wide(&format!("NeramitCut/{}", current()));
    let (h, p, get) = (wide(host), wide(if path.is_empty() { "/" } else { path }), wide("GET"));
    // SAFETY: WinHTTP calls with NUL-terminated strings; every handle is closed by its guard
    unsafe {
        let mut session = Handle(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, std::ptr::null(), std::ptr::null(), 0));
        if session.0.is_null() {
            // before Windows 8.1: the proxy set with netsh
            session = Handle(WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, std::ptr::null(), std::ptr::null(), 0));
        }
        if session.0.is_null() {
            return Err(net_err());
        }
        WinHttpSetTimeouts(session.0, 15_000, 15_000, 20_000, 30_000);
        let conn = Handle(WinHttpConnect(session.0, h.as_ptr(), port, 0));
        if conn.0.is_null() {
            return Err(net_err());
        }
        // WINHTTP_FLAG_REFRESH: never an older latest.json from a cache
        let flags = 0x100 | if secure { WINHTTP_FLAG_SECURE } else { 0 };
        let req = Handle(WinHttpOpenRequest(conn.0, get.as_ptr(), p.as_ptr(), std::ptr::null(), std::ptr::null(), std::ptr::null(), flags));
        if req.0.is_null() || WinHttpSendRequest(req.0, std::ptr::null(), 0, std::ptr::null(), 0, 0, 0) == 0 || WinHttpReceiveResponse(req.0, std::ptr::null_mut()) == 0 {
            return Err(net_err());
        }
        let query = |level: u32| {
            let mut v = 0u32;
            let mut len = 4u32;
            let ok = WinHttpQueryHeaders(req.0, level | WINHTTP_QUERY_FLAG_NUMBER, std::ptr::null(), &mut v as *mut u32 as *mut c_void, &mut len, std::ptr::null_mut()) != 0;
            ok.then_some(v)
        };
        let code = query(WINHTTP_QUERY_STATUS_CODE).unwrap_or(0);
        if code == 404 {
            return Err(tt("No release has been published yet.", "ยังไม่มีการเผยแพร่เวอร์ชันใด ๆ").into());
        }
        if code != 200 {
            return Err(format!("{} (HTTP {code})", tt("The update server did not answer as expected.", "เซิร์ฟเวอร์อัปเดตตอบกลับไม่ถูกต้อง")));
        }
        let total = query(WINHTTP_QUERY_CONTENT_LENGTH).unwrap_or(0) as u64;
        if total > max_bytes {
            return Err(too_large());
        }
        let mut out = Vec::with_capacity(total as usize);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let mut n = 0u32;
            if WinHttpReadData(req.0, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut n) == 0 {
                return Err(net_err());
            }
            if n == 0 {
                break;
            }
            if out.len() as u64 + n as u64 > max_bytes {
                return Err(too_large());
            }
            out.extend_from_slice(&buf[..n as usize]);
            if !progress(out.len() as u64, total) {
                return Err("cancelled".into());
            }
        }
        if total > 0 && out.len() as u64 != total {
            return Err(tt("The update download was interrupted. Try again.", "การดาวน์โหลดถูกขัดจังหวะ กรุณาลองอีกครั้ง").into());
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_numeric_stable_and_cannot_be_paths() {
        assert!(parse_version("1.10.0") > parse_version("1.9.9"));
        for invalid in ["", "1", "1.0", "1.0.0.0", "1.0.0-beta", "v1.0.0", "1.0../evil", "01.0.0", "1.+0.0"] {
            assert!(parse_version(invalid).is_none(), "{invalid}");
            assert!(!is_newer(invalid));
        }
        assert!(!is_newer(current()));
        assert!(is_newer("999.0.0"));
    }

    #[test]
    fn windows_sha256_matches_known_vectors() {
        assert_eq!(to_hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(to_hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn unsigned_or_other_product_manifests_are_rejected() {
        // a real signature of Neramit Planner 1.0.0's Setup: valid for the studio key, but signed
        // for another product, so Neramit Cut must not accept it
        let planner = serde_json::json!({
            "version": "1.0.0", "size": 7578624,
            "sha256": "f31c9d67baabf3562a2368c70f27633685b66fe30e9da2dd9a50c95b211ec2f8",
            "signature": "94f5a55a8a6adf33c9435cd0fe018781c9415317712c9de288838040955c32291a9f9c71f63a906c306e1e4184be6e06b6c86d563da4667d8b89fbc15eb1a2e6"
        });
        assert!(verify(b"NeramitPlanner|1.0.0|f31c9d67baabf3562a2368c70f27633685b66fe30e9da2dd9a50c95b211ec2f8", &hex(planner["signature"].as_str().unwrap()).unwrap()));
        assert!(parse_manifest(&serde_json::to_vec(&planner).unwrap()).is_err());
        let mut zero = planner.clone();
        zero["signature"] = serde_json::json!("00".repeat(64));
        assert!(parse_manifest(&serde_json::to_vec(&zero).unwrap()).is_err());
        assert!(parse_manifest(b"not json").is_err());
    }

    #[test]
    fn download_validation_rejects_changed_and_truncated_bytes() {
        let artifact = Artifact { size: 3, sha256: to_hex(&sha256(b"abc")) };
        assert!(validate_data(b"abc", &artifact).is_ok());
        assert!(validate_data(b"ab", &artifact).is_err());
        assert!(validate_data(b"abd", &artifact).is_err());
    }

    #[test]
    fn portable_replacement_preserves_other_files_and_rejects_bad_payload() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("neramitcut-update-test-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("download.exe");
        let target = dir.join("NeramitCut.exe");
        let job = dir.join("parts.xml");
        std::fs::write(&source, b"new").unwrap();
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&job, b"job data").unwrap();
        let bad = Artifact { size: 3, sha256: "00".repeat(32) };
        assert!(replace_portable(&source, &target, &bad).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        let good = Artifact { size: 3, sha256: to_hex(&sha256(b"new")) };
        let backup = replace_portable(&source, &target, &good).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(std::fs::read(&backup).unwrap(), b"old");
        restore_portable(&target, &backup).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert_eq!(std::fs::read(&job).unwrap(), b"job data");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_ready_download_returns_to_retryable_failed_state() {
        let rel = Release { version: "9.9.9".into(), installer: Artifact { size: 1, sha256: "00".repeat(32) }, portable: None, manifest: b"{}".to_vec(), notes_en: String::new(), notes_th: String::new() };
        let missing = std::env::temp_dir().join(format!("neramitcut-missing-update-{}.exe", std::process::id()));
        install_on_exit(rel, missing, true);
        assert!(on_exit().is_err());
        assert!(matches!(status(), Status::Failed(_)));
        assert!(INSTALL_ON_EXIT.lock().unwrap().is_none());
        set(Status::Idle);
    }
}
