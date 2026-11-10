//! Windows common dialogs (open/save, font, folder, print) used like the VCL dialog components.

use super::win::*;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Controls::Dialogs::*;
use windows_sys::Win32::UI::Shell::*;

/// `filter` uses the VCL syntax: "Description|*.ext;*.ext2|Other|*.*".
fn filter_buf(filter: &str) -> Vec<u16> {
    let mut v: Vec<u16> = filter.replace('|', "\0").encode_utf16().collect();
    v.push(0);
    v.push(0);
    v
}

unsafe fn file_dialog(owner: HWND, filter: &str, default_ext: &str, initial_dir: Option<&std::path::Path>, file_name: &str, save: bool) -> Option<PathBuf> {
    let mut buf = vec![0u16; 4096];
    for (i, c) in file_name.encode_utf16().take(4000).enumerate() {
        buf[i] = c;
    }
    let filt = filter_buf(filter);
    let ext = wide(default_ext);
    let dir = initial_dir.map(|d| wide(&d.to_string_lossy()));
    let mut ofn: OPENFILENAMEW = std::mem::zeroed();
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = owner;
    ofn.lpstrFilter = filt.as_ptr();
    ofn.nFilterIndex = 1;
    ofn.lpstrFile = buf.as_mut_ptr();
    ofn.nMaxFile = buf.len() as u32;
    ofn.lpstrInitialDir = dir.as_ref().map(|d| d.as_ptr()).unwrap_or(null());
    if !default_ext.is_empty() {
        ofn.lpstrDefExt = ext.as_ptr();
    }
    ofn.Flags = OFN_EXPLORER | OFN_ENABLESIZING | OFN_HIDEREADONLY | OFN_NOCHANGEDIR;
    let ok = if save {
        ofn.Flags |= OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST;
        GetSaveFileNameW(&mut ofn)
    } else {
        ofn.Flags |= OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST;
        GetOpenFileNameW(&mut ofn)
    };
    if ok == 0 {
        return None;
    }
    let s = from_wide(&buf);
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

pub unsafe fn open_file(owner: HWND, filter: &str, initial_dir: Option<&std::path::Path>) -> Option<PathBuf> {
    file_dialog(owner, filter, "", initial_dir, "", false)
}

pub unsafe fn save_file(owner: HWND, filter: &str, default_ext: &str, initial_dir: Option<&std::path::Path>, file_name: &str) -> Option<PathBuf> {
    file_dialog(owner, filter, default_ext, initial_dir, file_name, true)
}

/// TFontDialog: edits `spec` in place.
pub unsafe fn choose_font(owner: HWND, spec: &mut FontSpec) -> bool {
    let mut lf: LOGFONTW = std::mem::zeroed();
    lf.lfHeight = spec.height;
    lf.lfWeight = if spec.bold { 700 } else { 400 };
    lf.lfItalic = spec.italic as u8;
    lf.lfUnderline = spec.underline as u8;
    lf.lfCharSet = DEFAULT_CHARSET as u8;
    for (i, c) in spec.name.encode_utf16().take(31).enumerate() {
        lf.lfFaceName[i] = c;
    }
    let mut cf: CHOOSEFONTW = std::mem::zeroed();
    cf.lStructSize = std::mem::size_of::<CHOOSEFONTW>() as u32;
    cf.hwndOwner = owner;
    cf.lpLogFont = &mut lf;
    cf.Flags = CF_SCREENFONTS | CF_INITTOLOGFONTSTRUCT | CF_EFFECTS;
    cf.rgbColors = spec.color;
    if ChooseFontW(&mut cf) == 0 {
        return false;
    }
    spec.name = from_wide(&lf.lfFaceName);
    spec.height = lf.lfHeight;
    spec.bold = lf.lfWeight >= 600;
    spec.italic = lf.lfItalic != 0;
    spec.underline = lf.lfUnderline != 0;
    spec.color = cf.rgbColors;
    true
}

/// SelectDirectory (folder browser).
pub unsafe fn choose_folder(owner: HWND, title: &str) -> Option<PathBuf> {
    let t = wide(title);
    let mut name = vec![0u16; 260];
    let mut bi: BROWSEINFOW = std::mem::zeroed();
    bi.hwndOwner = owner;
    bi.pszDisplayName = name.as_mut_ptr();
    bi.lpszTitle = t.as_ptr();
    bi.ulFlags = BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE;
    let pidl = SHBrowseForFolderW(&bi);
    if pidl.is_null() {
        return None;
    }
    let mut path = vec![0u16; 1024];
    let ok = SHGetPathFromIDListW(pidl, path.as_mut_ptr());
    windows_sys::Win32::System::Com::CoTaskMemFree(pidl as _);
    if ok == 0 {
        None
    } else {
        Some(PathBuf::from(from_wide(&path)))
    }
}

/// TPrintDialog: returns a printer DC (caller ends with DeleteDC).
pub unsafe fn print_dialog(owner: HWND) -> Option<HDC> {
    let mut pd: PRINTDLGW = std::mem::zeroed();
    pd.lStructSize = std::mem::size_of::<PRINTDLGW>() as u32;
    pd.hwndOwner = owner;
    pd.Flags = PD_RETURNDC | PD_NOSELECTION | PD_NOPAGENUMS | PD_USEDEVMODECOPIESANDCOLLATE;
    pd.nCopies = 1;
    if PrintDlgW(&mut pd) == 0 || pd.hDC.is_null() {
        return None;
    }
    if !pd.hDevMode.is_null() {
        windows_sys::Win32::Foundation::GlobalFree(pd.hDevMode as _);
    }
    if !pd.hDevNames.is_null() {
        windows_sys::Win32::Foundation::GlobalFree(pd.hDevNames as _);
    }
    Some(pd.hDC)
}

pub unsafe fn open_url(url: &str) {
    ShellExecuteW(null_mut(), wide("open").as_ptr(), wide(url).as_ptr(), null(), null(), 1);
}
