//! Visual design of Neramit Cut: Noto Sans Thai, a white and blue palette (blue window caption,
//! white surfaces, one blue accent), anti-aliased rounded surfaces (GDI+), Segoe Fluent icon
//! glyphs and DPI scaling.

use super::win::*;
use std::cell::Cell;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Graphics::GdiPlus::*;

const fn c(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

// ------------------------------------------------------------------ palette

pub const BG: u32 = c(0xF2, 0xF5, 0xFA);
pub const SURFACE: u32 = c(0xFF, 0xFF, 0xFF);
pub const SURFACE_ALT: u32 = c(0xF7, 0xF9, 0xFC);
pub const STROKE: u32 = c(0xDC, 0xE3, 0xED);
pub const STROKE_STRONG: u32 = c(0xC2, 0xCC, 0xDA);
pub const DIVIDER: u32 = c(0xE8, 0xED, 0xF4);
pub const TEXT: u32 = c(0x1A, 0x24, 0x33);
pub const TEXT_MUTED: u32 = c(0x52, 0x60, 0x73);
/// table headings
pub const HEADER_TEXT: u32 = c(0x2A, 0x37, 0x4A);
pub const TEXT_SUBTLE: u32 = c(0x84, 0x91, 0xA3);
pub const TEXT_DISABLED: u32 = c(0xB4, 0xBE, 0xCB);
pub const ACCENT: u32 = c(0x1A, 0x6B, 0xD8);
pub const ACCENT_HOVER: u32 = c(0x15, 0x5B, 0xBC);
pub const ACCENT_PRESSED: u32 = c(0x10, 0x4A, 0x9A);
pub const ACCENT_SOFT: u32 = c(0xE9, 0xF2, 0xFD);
pub const ACCENT_SOFT2: u32 = c(0xD5, 0xE6, 0xFB);
pub const HOVER: u32 = c(0xED, 0xF2, 0xF9);
pub const PRESSED: u32 = c(0xE0, 0xE8, 0xF3);
/// grid rows: under the mouse, every second row
pub const ROW_HOT: u32 = c(0xF4, 0xF8, 0xFF);
pub const ROW_ALT: u32 = c(0xFA, 0xFB, 0xFD);
pub const SUCCESS: u32 = c(0x15, 0x80, 0x3D);
pub const SUCCESS_SOFT: u32 = c(0xE7, 0xF6, 0xEC);
pub const WARNING: u32 = c(0xB4, 0x53, 0x09);
pub const DANGER: u32 = c(0xC8, 0x2A, 0x2A);
pub const DANGER_SOFT: u32 = c(0xFD, 0xEC, 0xEC);
pub const WHITE: u32 = c(0xFF, 0xFF, 0xFF);

/// The main window caption.
pub const BRAND: u32 = c(0x17, 0x5C, 0xC4);
/// Secondary text on the brand colour.
pub const BRAND_TEXT_SOFT: u32 = c(0xD3, 0xE4, 0xFB);
/// Primary buttons: a gentle blue gradient.
pub const PRIMARY_FROM: u32 = c(0x1A, 0x6B, 0xD8);
pub const PRIMARY_TO: u32 = c(0x2F, 0x83, 0xEC);

/// A colour family: the strong colour (badges, icons), a soft tint (backgrounds) and a dark
/// ink (numbers on the tint).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tone {
    pub main: u32,
    pub soft: u32,
    pub ink: u32,
}

/// The one tone of the work areas and summary tiles.
pub const BLUE: Tone = Tone { main: c(0x1A, 0x6B, 0xD8), soft: c(0xF3, 0xF8, 0xFF), ink: c(0x14, 0x3F, 0x7E) };
/// Meaningful colours only: a good result, something left over.
pub const GREEN: Tone = Tone { main: c(0x16, 0xA3, 0x4A), soft: c(0xEC, 0xF8, 0xF0), ink: c(0x15, 0x80, 0x3D) };
pub const RED: Tone = Tone { main: c(0xDC, 0x35, 0x35), soft: c(0xFD, 0xEE, 0xEE), ink: c(0xB0, 0x22, 0x22) };

pub fn tone(name: &str) -> Option<Tone> {
    Some(match name {
        "blue" => BLUE,
        "green" => GREEN,
        "red" => RED,
        _ => return None,
    })
}

/// True for colours that need light text on top (the brand bar).
pub fn is_dark(color: u32) -> bool {
    let (r, g, b) = ((color & 0xFF) as f64, ((color >> 8) & 0xFF) as f64, ((color >> 16) & 0xFF) as f64);
    0.299 * r + 0.587 * g + 0.114 * b < 128.0
}

/// Fills of the pieces in the cutting drawings (pastel, readable with dark text).
pub const PIECE_FILLS: [u32; 10] = [
    c(0xD6, 0xE8, 0xFB),
    c(0xDC, 0xF2, 0xE3),
    c(0xFC, 0xEB, 0xD2),
    c(0xEA, 0xE2, 0xFA),
    c(0xFB, 0xE0, 0xE6),
    c(0xD8, 0xF0, 0xF0),
    c(0xF3, 0xF0, 0xCF),
    c(0xE3, 0xE8, 0xEE),
    c(0xF7, 0xE1, 0xF4),
    c(0xE0, 0xEE, 0xD6),
];
pub const PIECE_STROKES: [u32; 10] = [
    c(0x6A, 0xA5, 0xE0),
    c(0x6F, 0xBF, 0x8A),
    c(0xE0, 0xA8, 0x55),
    c(0xA3, 0x8A, 0xDB),
    c(0xE0, 0x86, 0x9C),
    c(0x5F, 0xB5, 0xB5),
    c(0xC4, 0xB4, 0x44),
    c(0x97, 0xA3, 0xB0),
    c(0xCF, 0x82, 0xC4),
    c(0x8F, 0xBA, 0x6E),
];

// ------------------------------------------------------------------ scale

thread_local! {
    static SCALE: Cell<f64> = const { Cell::new(1.0) };
}

/// System DPI (the process is system-DPI aware) times NERAMIT_SCALE (testing aid).
pub fn init_scale() {
    let dpi = unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForSystem() }.max(96) as f64;
    let user: f64 = std::env::var("NERAMIT_SCALE").ok().and_then(|s| s.parse().ok()).filter(|v: &f64| *v >= 0.5 && *v <= 4.0).unwrap_or(1.0);
    SCALE.with(|s| s.set(dpi / 96.0 * user));
}

pub fn scale() -> f64 {
    SCALE.with(|s| s.get())
}

/// Device pixels for a length designed at 96 DPI.
pub fn px(v: i32) -> i32 {
    (v as f64 * scale()).round() as i32
}

pub fn pxf(v: f64) -> f32 {
    (v * scale()) as f32
}

// ------------------------------------------------------------------ fonts

pub const FACE: &str = "Noto Sans Thai";
pub const FACE_MEDIUM: &str = "Noto Sans Thai Medium";
pub const FACE_SEMIBOLD: &str = "Noto Sans Thai SemiBold";

/// Body text size (px at 96 DPI).
pub const BODY: i32 = 14;
pub const SMALL: i32 = 12;
pub const GRID: i32 = 13;

static FONT_DATA: [&[u8]; 3] = [
    include_bytes!("../../assets/fonts/NotoSansThai-Regular.ttf"),
    include_bytes!("../../assets/fonts/NotoSansThai-Medium.ttf"),
    include_bytes!("../../assets/fonts/NotoSansThai-SemiBold.ttf"),
];

/// Native edit boxes draw their text with plain GDI, without the mark-to-mark positioning the
/// Noto Sans Thai tone marks need (see ui::shape): they use Windows' Thai UI face instead.
pub fn native_text_font(f: &FontSpec) -> FontSpec {
    let mut g = f.clone();
    if f.name.starts_with(FACE) {
        g.bold = f.bold || f.name.ends_with("SemiBold");
        g.name = "Leelawadee UI".into();
    }
    g
}

/// The bundled font files with the GDI face name each one gets.
pub fn font_files() -> [(&'static str, &'static [u8]); 3] {
    [(FACE, FONT_DATA[0]), (FACE_MEDIUM, FONT_DATA[1]), (FACE_SEMIBOLD, FONT_DATA[2])]
}

/// Make the bundled Noto Sans Thai faces available to this process only.
pub fn register_fonts() {
    for d in FONT_DATA.iter() {
        let mut n = 0u32;
        unsafe {
            AddFontMemResourceEx(d.as_ptr() as _, d.len() as u32, null(), &mut n);
        }
    }
}

pub fn font(size: i32) -> FontSpec {
    let mut f = FontSpec::new(FACE, -px(size));
    f.color = TEXT;
    f
}

pub fn font_medium(size: i32) -> FontSpec {
    let mut f = FontSpec::new(FACE_MEDIUM, -px(size));
    f.color = TEXT;
    f
}

pub fn font_semibold(size: i32) -> FontSpec {
    let mut f = FontSpec::new(FACE_SEMIBOLD, -px(size));
    f.color = TEXT;
    f
}

/// Segoe Fluent Icons (Windows 11), else Segoe MDL2 Assets (Windows 10).
pub fn icon_face() -> &'static str {
    thread_local! {
        static FACE_NAME: Cell<Option<&'static str>> = const { Cell::new(None) };
    }
    FACE_NAME.with(|f| {
        if let Some(n) = f.get() {
            return n;
        }
        let win = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
        let n = if std::path::Path::new(&win).join("Fonts").join("SegoeIcons.ttf").exists() { "Segoe Fluent Icons" } else { "Segoe MDL2 Assets" };
        f.set(Some(n));
        n
    })
}

pub fn icon_font(size: i32) -> FontSpec {
    FontSpec::new(icon_face(), -px(size))
}

pub mod icon {
    pub const ADD: char = '\u{E710}';
    pub const DELETE: char = '\u{E74D}';
    pub const OPEN: char = '\u{E8DA}';
    pub const FOLDER: char = '\u{E8B7}';
    pub const SAVE: char = '\u{E74E}';
    pub const SAVE_AS: char = '\u{E792}';
    pub const IMPORT: char = '\u{E8B5}';
    pub const EXPORT: char = '\u{EDE1}';
    pub const CLEAR: char = '\u{E75C}';
    pub const PRINT: char = '\u{E749}';
    pub const TAG: char = '\u{E8EC}';
    pub const MORE: char = '\u{E712}';
    pub const HISTORY: char = '\u{E81C}';
    pub const TOOLS: char = '\u{E90F}';
    pub const PLAY: char = '\u{E768}';
    pub const CHECK: char = '\u{E73E}';
    pub const UNDO: char = '\u{E7A7}';
    pub const TABLE: char = '\u{E80A}';
    pub const COLUMNS: char = '\u{E8A9}';
    pub const LIST: char = '\u{E8FD}';
    pub const SETTINGS: char = '\u{E713}';
    pub const GLOBE: char = '\u{E774}';
    pub const HELP: char = '\u{E897}';
    pub const INFO: char = '\u{E946}';
    pub const DOCUMENT: char = '\u{E8A5}';
    pub const CHEVRON_DOWN: char = '\u{E70D}';
    pub const CHEVRON_RIGHT: char = '\u{E76C}';
    pub const COPY: char = '\u{E8C8}';
    pub const CHART: char = '\u{E9D2}';
    pub const REPORT: char = '\u{E9F9}';
    pub const LAYERS: char = '\u{E81E}';
    pub const SLIDERS: char = '\u{E9E9}';
    pub const WARNING: char = '\u{E7BA}';
    pub const ERROR: char = '\u{E783}';
    pub const SUCCESS: char = '\u{E930}';
    pub const CALCULATOR: char = '\u{E8EF}';
    pub const SEARCH: char = '\u{E721}';
    pub const CLOSE: char = '\u{E894}';
    pub const STOP: char = '\u{E71A}';
    pub const SWAP: char = '\u{E8AB}';
    pub const CUT: char = '\u{E8C6}';
    pub const STOPWATCH: char = '\u{E916}';
    pub const MONEY: char = '\u{E8C7}';
    pub const BARS: char = '\u{E8FD}';
}

pub fn icon_from_name(name: &str) -> Option<char> {
    use icon::*;
    Some(match name {
        "add" => ADD,
        "delete" => DELETE,
        "open" => OPEN,
        "folder" => FOLDER,
        "save" => SAVE,
        "save_as" => SAVE_AS,
        "import" => IMPORT,
        "export" => EXPORT,
        "clear" => CLEAR,
        "print" => PRINT,
        "tag" => TAG,
        "more" => MORE,
        "history" => HISTORY,
        "tools" => TOOLS,
        "play" => PLAY,
        "check" => CHECK,
        "undo" => UNDO,
        "table" => TABLE,
        "columns" => COLUMNS,
        "list" => LIST,
        "settings" => SETTINGS,
        "globe" => GLOBE,
        "help" => HELP,
        "info" => INFO,
        "document" => DOCUMENT,
        "copy" => COPY,
        "chart" => CHART,
        "report" => REPORT,
        "layers" => LAYERS,
        "sliders" => SLIDERS,
        "calculator" => CALCULATOR,
        "search" => SEARCH,
        "close" => CLOSE,
        "stop" => STOP,
        "swap" => SWAP,
        _ => return None,
    })
}

/// Draw an icon glyph centred in `r`.
pub unsafe fn glyph(dc: HDC, ch: char, r: &RECT, size: i32, color: u32) {
    let old = SelectObject(dc, super::win::font(&icon_font(size)) as _);
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, color);
    let mut rr = *r;
    let s: String = ch.to_string();
    draw_text(dc, &s, &mut rr, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
    SelectObject(dc, old);
}

/// Text in `r` with a font and colour.
pub unsafe fn text(dc: HDC, s: &str, r: &RECT, f: &FontSpec, color: u32, flags: u32) {
    let old = SelectObject(dc, super::win::font(f) as _);
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, color);
    let mut rr = *r;
    draw_text(dc, s, &mut rr, flags | DT_NOPREFIX);
    SelectObject(dc, old);
}

// ------------------------------------------------------------------ GDI+ shapes

thread_local! {
    static GDIP: Cell<usize> = const { Cell::new(0) };
}

pub fn gdiplus_init() {
    GDIP.with(|t| {
        if t.get() != 0 {
            return;
        }
        let input = GdiplusStartupInput { GdiplusVersion: 1, DebugEventCallback: 0, SuppressBackgroundThread: 0, SuppressExternalCodecs: 0 };
        let mut token = 0usize;
        unsafe {
            GdiplusStartup(&mut token, &input, null_mut());
        }
        t.set(token.max(1));
    });
}

fn argb(color: u32, alpha: u8) -> u32 {
    let (r, g, b) = (color & 0xFF, (color >> 8) & 0xFF, (color >> 16) & 0xFF);
    (alpha as u32) << 24 | r << 16 | g << 8 | b
}

struct G(*mut GpGraphics);

impl G {
    unsafe fn new(dc: HDC) -> G {
        gdiplus_init();
        let mut g: *mut GpGraphics = null_mut();
        GdipCreateFromHDC(dc, &mut g);
        GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
        GdipSetPixelOffsetMode(g, PixelOffsetModeHalf);
        G(g)
    }
}

impl Drop for G {
    fn drop(&mut self) {
        unsafe {
            GdipDeleteGraphics(self.0);
        }
    }
}

unsafe fn round_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> *mut GpPath {
    let mut p: *mut GpPath = null_mut();
    GdipCreatePath(FillModeWinding, &mut p);
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    if r <= 0.5 {
        GdipAddPathLine(p, x, y, x + w, y);
        GdipAddPathLine(p, x + w, y, x + w, y + h);
        GdipAddPathLine(p, x + w, y + h, x, y + h);
    } else {
        let d = r * 2.0;
        GdipAddPathArc(p, x, y, d, d, 180.0, 90.0);
        GdipAddPathArc(p, x + w - d, y, d, d, 270.0, 90.0);
        GdipAddPathArc(p, x + w - d, y + h - d, d, d, 0.0, 90.0);
        GdipAddPathArc(p, x, y + h - d, d, d, 90.0, 90.0);
    }
    GdipClosePathFigure(p);
    p
}

/// Rounded rectangle covering `r` (device pixels): optional fill and 1 px stroke inside.
pub unsafe fn round_rect(dc: HDC, r: &RECT, radius: f32, fill_color: Option<u32>, stroke: Option<u32>) {
    round_rect_w(dc, r, radius, fill_color, stroke, 1.0);
}

pub unsafe fn round_rect_w(dc: HDC, r: &RECT, radius: f32, fill_color: Option<u32>, stroke: Option<u32>, width: f32) {
    let g = G::new(dc);
    let (x, y, w, h) = (r.left as f32, r.top as f32, (r.right - r.left) as f32, (r.bottom - r.top) as f32);
    if let Some(fc) = fill_color {
        let p = round_path(x, y, w, h, radius);
        let mut b: *mut GpSolidFill = null_mut();
        GdipCreateSolidFill(argb(fc, 255), &mut b);
        GdipFillPath(g.0, b as *mut GpBrush, p);
        GdipDeleteBrush(b as *mut GpBrush);
        GdipDeletePath(p);
    }
    if let Some(sc) = stroke {
        let hw = width / 2.0;
        let p = round_path(x + hw, y + hw, w - width, h - width, (radius - hw).max(0.0));
        let mut pen: *mut GpPen = null_mut();
        GdipCreatePen1(argb(sc, 255), width, UnitPixel, &mut pen);
        GdipDrawPath(g.0, pen, p);
        GdipDeletePen(pen);
        GdipDeletePath(p);
    }
}

/// Diagonal hatch inside a rounded rectangle (offcuts).
pub unsafe fn hatch_rect(dc: HDC, r: &RECT, radius: f32, fore: u32, back: u32) {
    let g = G::new(dc);
    let (x, y, w, h) = (r.left as f32, r.top as f32, (r.right - r.left) as f32, (r.bottom - r.top) as f32);
    let p = round_path(x, y, w, h, radius);
    let mut b: *mut GpHatch = null_mut();
    GdipCreateHatchBrush(HatchStyleWideUpwardDiagonal, argb(fore, 255), argb(back, 255), &mut b);
    GdipFillPath(g.0, b as *mut GpBrush, p);
    GdipDeleteBrush(b as *mut GpBrush);
    GdipDeletePath(p);
}

pub unsafe fn ellipse(dc: HDC, r: &RECT, color: u32) {
    let g = G::new(dc);
    let mut b: *mut GpSolidFill = null_mut();
    GdipCreateSolidFill(argb(color, 255), &mut b);
    GdipFillEllipse(g.0, b as *mut GpBrush, r.left as f32, r.top as f32, (r.right - r.left) as f32, (r.bottom - r.top) as f32);
    GdipDeleteBrush(b as *mut GpBrush);
}

/// Rounded rectangle filled with a colour at some opacity (hover on the brand bar).
pub unsafe fn round_rect_alpha(dc: HDC, r: &RECT, radius: f32, color: u32, alpha: u8) {
    let g = G::new(dc);
    let (x, y, w, h) = (r.left as f32, r.top as f32, (r.right - r.left) as f32, (r.bottom - r.top) as f32);
    let p = round_path(x, y, w, h, radius);
    let mut b: *mut GpSolidFill = null_mut();
    GdipCreateSolidFill(argb(color, alpha), &mut b);
    GdipFillPath(g.0, b as *mut GpBrush, p);
    GdipDeleteBrush(b as *mut GpBrush);
    GdipDeletePath(p);
}

/// Rounded rectangle with a left-to-right gradient through `colors` (two or more).
pub unsafe fn gradient_rect(dc: HDC, r: &RECT, radius: f32, colors: &[u32]) {
    if colors.is_empty() || r.right <= r.left || r.bottom <= r.top {
        return;
    }
    let g = G::new(dc);
    let (x, y, w, h) = (r.left as f32, r.top as f32, (r.right - r.left) as f32, (r.bottom - r.top) as f32);
    let p = round_path(x, y, w, h, radius);
    let rf = RectF { X: x - 1.0, Y: y, Width: w + 2.0, Height: h.max(1.0) };
    let first = colors[0];
    let last = *colors.last().unwrap();
    let mut b: *mut GpLineGradient = null_mut();
    GdipCreateLineBrushFromRect(&rf, argb(first, 255), argb(last, 255), LinearGradientModeHorizontal, WrapModeTileFlipX, &mut b);
    if colors.len() > 2 {
        let cs: Vec<u32> = colors.iter().map(|&c| argb(c, 255)).collect();
        let n = cs.len();
        let pos: Vec<f32> = (0..n).map(|i| i as f32 / (n - 1) as f32).collect();
        GdipSetLinePresetBlend(b, cs.as_ptr(), pos.as_ptr(), n as i32);
    }
    GdipFillPath(g.0, b as *mut GpBrush, p);
    GdipDeleteBrush(b as *mut GpBrush);
    GdipDeletePath(p);
}

/// Paint `f` only inside the band `top..bottom` (device coordinates of `dc`).
unsafe fn in_band(dc: HDC, left: i32, top: i32, right: i32, bottom: i32, f: impl FnOnce()) {
    let saved = SaveDC(dc);
    IntersectClipRect(dc, left, top, right, bottom);
    f();
    RestoreDC(dc, saved);
}

/// A card: white rounded surface; a header band of `head_h` tinted with the tone; a light
/// footer band of `foot_h`; a hairline outline.
pub unsafe fn card(dc: HDC, r: &RECT, parent: u32, tone: Option<Tone>, head_h: i32, foot_h: i32) {
    fill(dc, r, parent);
    let rad = px(10) as f32;
    round_rect(dc, r, rad, Some(SURFACE), None);
    if head_h > 0 {
        let soft = tone.map(|t| t.soft).unwrap_or(SURFACE_ALT);
        in_band(dc, r.left, r.top, r.right, r.top + head_h, || round_rect(dc, r, rad, Some(soft), None));
        // a card that is all header (the job bar) keeps its rounded bottom: no divider line
        if head_h < r.bottom - r.top - 1 {
            hline(dc, r.left + 1, r.right - 1, r.top + head_h - 1, STROKE);
        }
    }
    if foot_h > 0 {
        in_band(dc, r.left, r.bottom - foot_h, r.right, r.bottom, || round_rect(dc, r, rad, Some(SURFACE_ALT), None));
        hline(dc, r.left + 1, r.right - 1, r.bottom - foot_h, DIVIDER);
    }
    round_rect(dc, r, rad, None, Some(STROKE));
}

/// A round badge with a short text (step numbers).
pub unsafe fn badge(dc: HDC, r: &RECT, s: &str, fill_color: u32, fg: u32) {
    ellipse(dc, r, fill_color);
    text(dc, s, r, &font_semibold(12), fg, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
}

/// "Neramit Cut" with "Neramit" emphasised; returns the width used.
pub unsafe fn wordmark(dc: HDC, r: &RECT, size: i32, on_dark: bool) -> i32 {
    let strong = font_semibold(size);
    let rest = font_medium(size);
    let a = measure(&strong, "Neramit").cx;
    let b = measure(&rest, " Cut").cx;
    let (c1, c2) = if on_dark { (WHITE, BRAND_TEXT_SOFT) } else { (ACCENT, TEXT) };
    text(dc, "Neramit", &RECT { left: r.left, top: r.top, right: r.left + a + 2, bottom: r.bottom }, &strong, c1, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    text(dc, " Cut", &RECT { left: r.left + a, top: r.top, right: r.left + a + b + 2, bottom: r.bottom }, &rest, c2, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    a + b
}

/// Rounded rectangle with a top-to-bottom gradient.
pub unsafe fn gradient_rect_v(dc: HDC, r: &RECT, radius: f32, top: u32, bottom: u32) {
    let g = G::new(dc);
    let (x, y, w, h) = (r.left as f32, r.top as f32, (r.right - r.left) as f32, (r.bottom - r.top) as f32);
    let p = round_path(x, y, w, h, radius);
    let rf = RectF { X: x, Y: y - 1.0, Width: w.max(1.0), Height: h + 2.0 };
    let mut b: *mut GpLineGradient = null_mut();
    GdipCreateLineBrushFromRect(&rf, argb(top, 255), argb(bottom, 255), LinearGradientModeVertical, WrapModeTileFlipX, &mut b);
    GdipFillPath(g.0, b as *mut GpBrush, p);
    GdipDeleteBrush(b as *mut GpBrush);
    GdipDeletePath(p);
}

/// The Neramit Cut logo in a square: the engraved leaf-cutter ant portrait in assets/neramitcut.ico,
/// exported from tools/logo/leafcutter-portrait-source.png by tools/make_icon.py at the size closest
/// to `size`, so the logo is the same everywhere.
pub unsafe fn logo(dc: HDC, x: i32, y: i32, size: i32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, LoadImageW, DI_NORMAL, IMAGE_ICON, LR_DEFAULTCOLOR};
    let module = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null());
    let icon = LoadImageW(module, 1 as _, IMAGE_ICON, size, size, LR_DEFAULTCOLOR);
    if !icon.is_null() {
        DrawIconEx(dc, x, y, icon as _, size, size, 0, std::ptr::null_mut(), DI_NORMAL);
        DestroyIcon(icon as _);
    }
}
