//! Small Win32/GDI helpers shared by the VCL-style runtime.
#![allow(clippy::missing_safety_doc)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

pub unsafe fn hinstance() -> HINSTANCE {
    GetModuleHandleW(null())
}

pub fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

/// VCL TColor (BGR integer, named constant, or clXXX system colour) to COLORREF.
pub fn tcolor(v: &serde_json::Value) -> Option<u32> {
    match v {
        serde_json::Value::Number(n) => n.as_i64().map(color_from_int),
        serde_json::Value::String(s) => color_name(s),
        _ => None,
    }
}

pub fn color_from_int(c: i64) -> u32 {
    if c < 0 {
        // system colour: 0xFF0000xx
        unsafe { GetSysColor((c as u32 & 0xFF) as _) }
    } else {
        c as u32 & 0x00FF_FFFF
    }
}

pub fn color_name(s: &str) -> Option<u32> {
    let sys = |i: SYS_COLOR_INDEX| unsafe { GetSysColor(i) };
    Some(match s {
        "clBlack" => rgb(0, 0, 0),
        "clMaroon" => rgb(128, 0, 0),
        "clGreen" => rgb(0, 128, 0),
        "clOlive" => rgb(128, 128, 0),
        "clNavy" => rgb(0, 0, 128),
        "clPurple" => rgb(128, 0, 128),
        "clTeal" => rgb(0, 128, 128),
        "clGray" => rgb(128, 128, 128),
        "clSilver" => rgb(192, 192, 192),
        "clRed" => rgb(255, 0, 0),
        "clLime" => rgb(0, 255, 0),
        "clYellow" => rgb(255, 255, 0),
        "clBlue" => rgb(0, 0, 255),
        "clFuchsia" => rgb(255, 0, 255),
        "clAqua" => rgb(0, 255, 255),
        "clWhite" => rgb(255, 255, 255),
        "clMoneyGreen" => rgb(192, 220, 192),
        "clSkyBlue" => rgb(166, 202, 240),
        "clCream" => rgb(255, 251, 240),
        "clMedGray" => rgb(160, 160, 164),
        "clBtnFace" => sys(COLOR_BTNFACE),
        "clWindow" => sys(COLOR_WINDOW),
        "clWindowText" => sys(COLOR_WINDOWTEXT),
        "clBtnText" => sys(COLOR_BTNTEXT),
        "clBtnShadow" => sys(COLOR_BTNSHADOW),
        "clBtnHighlight" => sys(COLOR_BTNHIGHLIGHT),
        "clHighlight" => sys(COLOR_HIGHLIGHT),
        "clHighlightText" => sys(COLOR_HIGHLIGHTTEXT),
        "clGrayText" => sys(COLOR_GRAYTEXT),
        "cl3DDkShadow" => sys(COLOR_3DDKSHADOW),
        "cl3DLight" => sys(COLOR_3DLIGHT),
        "clInfoBk" => sys(COLOR_INFOBK),
        "clInfoText" => sys(COLOR_INFOTEXT),
        "clMenu" => sys(COLOR_MENU),
        "clMenuText" => sys(COLOR_MENUTEXT),
        "clActiveCaption" => sys(COLOR_ACTIVECAPTION),
        "clAppWorkSpace" => sys(COLOR_APPWORKSPACE),
        "clWindowFrame" => sys(COLOR_WINDOWFRAME),
        "clNone" => 0x1FFF_FFFF,
        _ => return None,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FontSpec {
    pub name: String,
    pub height: i32, // VCL Font.Height (negative = character height in px)
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: u32,
}

impl Default for FontSpec {
    fn default() -> Self {
        FontSpec { name: "Noto Sans Thai".into(), height: -14, bold: false, italic: false, underline: false, color: 0 }
    }
}

impl FontSpec {
    pub fn new(name: &str, height: i32) -> Self {
        FontSpec { name: name.into(), height, ..Default::default() }
    }
    /// Font from a point size like VCL Font.Size at 96 DPI.
    pub fn points(name: &str, size: i32) -> Self {
        FontSpec::new(name, -((size * 96 + 36) / 72))
    }
    pub fn size_pt(&self) -> i32 {
        (-self.height * 72 + 48) / 96
    }
}

thread_local! {
    static FONTS: RefCell<HashMap<FontSpec, HFONT>> = RefCell::new(HashMap::new());
}

/// Cached GDI font (ClearType).
pub fn font(spec: &FontSpec) -> HFONT {
    let mut key = spec.clone();
    key.color = 0;
    FONTS.with(|f| {
        if let Some(h) = f.borrow().get(&key) {
            return *h;
        }
        let mut lf: LOGFONTW = unsafe { std::mem::zeroed() };
        lf.lfHeight = spec.height;
        lf.lfWeight = if spec.bold { 700 } else { 400 };
        // the bundled Noto Sans Thai has real Medium/SemiBold faces: use them instead of synthetic bold
        let mut face = spec.name.clone();
        if spec.bold && face == "Noto Sans Thai" {
            face = "Noto Sans Thai SemiBold".into();
            lf.lfWeight = 400;
        }
        lf.lfItalic = spec.italic as u8;
        lf.lfUnderline = spec.underline as u8;
        lf.lfCharSet = DEFAULT_CHARSET as u8;
        lf.lfOutPrecision = OUT_DEFAULT_PRECIS as u8;
        lf.lfClipPrecision = CLIP_DEFAULT_PRECIS as u8;
        lf.lfQuality = CLEARTYPE_QUALITY as u8;
        lf.lfPitchAndFamily = DEFAULT_PITCH as u8;
        for (i, c) in face.encode_utf16().take(31).enumerate() {
            lf.lfFaceName[i] = c;
        }
        let h = unsafe { CreateFontIndirectW(&lf) };
        f.borrow_mut().insert(key, h);
        h
    })
}

pub unsafe fn fill(dc: HDC, r: &RECT, color: u32) {
    let b = CreateSolidBrush(color);
    FillRect(dc, r, b);
    DeleteObject(b as _);
}

pub unsafe fn hline(dc: HDC, x1: i32, x2: i32, y: i32, color: u32) {
    fill(dc, &RECT { left: x1, top: y, right: x2, bottom: y + 1 }, color);
}

pub unsafe fn vline(dc: HDC, x: i32, y1: i32, y2: i32, color: u32) {
    fill(dc, &RECT { left: x, top: y1, right: x + 1, bottom: y2 }, color);
}

pub unsafe fn frame(dc: HDC, r: &RECT, color: u32) {
    hline(dc, r.left, r.right, r.top, color);
    hline(dc, r.left, r.right, r.bottom - 1, color);
    vline(dc, r.left, r.top, r.bottom, color);
    vline(dc, r.right - 1, r.top, r.bottom, color);
}

/// VCL Frame3D: top/left in `tl`, bottom/right in `br`, then shrinks the rect.
pub unsafe fn frame3d(dc: HDC, r: &mut RECT, tl: u32, br: u32, width: i32) {
    for _ in 0..width {
        hline(dc, r.left, r.right - 1, r.top, tl);
        vline(dc, r.left, r.top, r.bottom - 1, tl);
        hline(dc, r.left, r.right, r.bottom - 1, br);
        vline(dc, r.right - 1, r.top, r.bottom, br);
        r.left += 1;
        r.top += 1;
        r.right -= 1;
        r.bottom -= 1;
    }
}

pub unsafe fn draw_text(dc: HDC, text: &str, r: &mut RECT, flags: u32) -> i32 {
    // Thai with stacked marks: shaped (see ui::shape)
    if let Some(h) = super::shape::draw_text(dc, text, r, flags) {
        return h;
    }
    // DrawTextW reads the buffer even for a zero length: always pass a terminated string.
    let w = wide(text);
    DrawTextW(dc, w.as_ptr(), (w.len() - 1) as i32, r, flags)
}

pub unsafe fn text_size(dc: HDC, text: &str) -> SIZE {
    let w = wide(text);
    let mut s = SIZE { cx: 0, cy: 0 };
    GetTextExtentPoint32W(dc, w.as_ptr(), (w.len() - 1) as i32, &mut s);
    s
}

/// Text extent with a given font using a screen DC (like TCanvas.TextWidth).
pub fn measure(spec: &FontSpec, text: &str) -> SIZE {
    unsafe {
        let dc = GetDC(null_mut());
        let old = SelectObject(dc, font(spec) as _);
        let s = text_size(dc, text);
        SelectObject(dc, old);
        ReleaseDC(null_mut(), dc);
        s
    }
}

pub fn text_height(spec: &FontSpec) -> i32 {
    unsafe {
        let dc = GetDC(null_mut());
        let old = SelectObject(dc, font(spec) as _);
        let mut tm: TEXTMETRICW = std::mem::zeroed();
        GetTextMetricsW(dc, &mut tm);
        SelectObject(dc, old);
        ReleaseDC(null_mut(), dc);
        tm.tmHeight
    }
}

pub unsafe fn set_text(hwnd: HWND, s: &str) {
    SetWindowTextW(hwnd, wide(s).as_ptr());
}

pub unsafe fn get_text(hwnd: HWND) -> String {
    let n = GetWindowTextLengthW(hwnd);
    let mut buf = vec![0u16; n as usize + 1];
    GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    from_wide(&buf)
}

pub unsafe fn client_rect(hwnd: HWND) -> RECT {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetClientRect(hwnd, &mut r);
    r
}

pub fn rect(x: i32, y: i32, w: i32, h: i32) -> RECT {
    RECT { left: x, top: y, right: x + w, bottom: y + h }
}

pub unsafe fn message_box(owner: HWND, text: &str, caption: &str, flags: u32) -> i32 {
    MessageBoxW(owner, wide(text).as_ptr(), wide(caption).as_ptr(), flags)
}

pub fn loword(v: usize) -> u16 {
    (v & 0xFFFF) as u16
}
pub fn hiword(v: usize) -> u16 {
    ((v >> 16) & 0xFFFF) as u16
}
pub fn get_x(l: LPARAM) -> i32 {
    (l & 0xFFFF) as i16 as i32
}
pub fn get_y(l: LPARAM) -> i32 {
    ((l >> 16) & 0xFFFF) as i16 as i32
}
pub fn makelparam(x: i32, y: i32) -> LPARAM {
    ((y as u32 & 0xFFFF) << 16 | (x as u32 & 0xFFFF)) as LPARAM
}

/// Off-screen buffer for flicker-free painting.
pub struct Buffer {
    pub dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    target: HDC,
    pub w: i32,
    pub h: i32,
}

impl Buffer {
    pub unsafe fn new(target: HDC, w: i32, h: i32) -> Buffer {
        let dc = CreateCompatibleDC(target);
        let bmp = CreateCompatibleBitmap(target, w.max(1), h.max(1));
        let old = SelectObject(dc, bmp as _);
        Buffer { dc, bmp, old, target, w, h }
    }
    pub unsafe fn blit(&self) {
        BitBlt(self.target, 0, 0, self.w, self.h, self.dc, 0, 0, SRCCOPY);
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bmp as _);
            DeleteDC(self.dc);
        }
    }
}

/// Debug trace to %TEMP%\NeramitCut-trace.txt (enabled when NERAMIT_TRACE is set).
pub fn trace(s: &str) {
    use std::io::Write;
    if std::env::var_os("NERAMIT_TRACE").is_none() {
        return;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(std::env::temp_dir().join("NeramitCut-trace.txt")) {
        let _ = writeln!(f, "{s}");
    }
}
