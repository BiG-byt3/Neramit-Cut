//! Application controller: the main window, its commands and the shared state.

use crate::engine;
use crate::forms;
use crate::i18n::{self, Lang};
use crate::mainwin as mw;
use crate::model::*;
use crate::results::{self, View};
use crate::settings;
use crate::ui::grid::{self, GridEv};
use crate::ui::theme::{self, px};
use crate::ui::vcl::{self, form, Ev, Form, Kind};
use crate::ui::win::*;
use crate::{dlg, export, xmlio};
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, SetFocus, VK_CONTROL, VK_F9};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Outer size of the main window on the first start (96-DPI units).
pub const DEFAULT_WINDOW: (i32, i32) = (1320, 743);
/// Default panel shares: pieces and stock column of the width, pieces of that column.
pub const DEFAULT_SHARES: (f64, f64) = (0.45, 0.613);

pub struct App {
    pub main: usize,
    pub iface: Interface,
    pub technical: Technical,
    pub materials: Vec<Material>,
    /// solution on screen and the data it was computed from
    pub sol: Option<(Project, Solution)>,
    pub undo: Option<results::Undo>,
    pub parts_file: Option<PathBuf>,
    pub parts_dirty: bool,
    pub comments: String,
    pub creation_time: String,
    pub run: Option<Run>,
    /// message on the left of the status bar
    pub status: String,
}

/// An optimization running on a worker thread.
pub struct Run {
    pub form: usize,
    pub cancel: Arc<AtomicBool>,
    pub done: Arc<AtomicUsize>,
    pub total: usize,
    pub handle: Option<std::thread::JoinHandle<Solution>>,
    pub prj: Project,
}

thread_local! {
    static APP: std::cell::Cell<*mut App> = const { std::cell::Cell::new(null_mut()) };
}

pub fn app() -> &'static mut App {
    APP.with(|a| unsafe { &mut *a.get() })
}

pub fn main_form() -> &'static mut Form {
    form(app().main)
}

pub fn main_hwnd() -> HWND {
    main_form().hwnd
}

fn text_hook(form: &str, ctl: &str, prop: &str, design: &str) -> Option<String> {
    if ctl == "f_main_window" && prop == "Caption" {
        return Some("Neramit Cut".into());
    }
    if prop == "Caption" || prop == "Hint" || prop.starts_with("Item") {
        return Some(i18n::caption(form, ctl, prop, design));
    }
    i18n::form_text(form, ctl, prop)
}

pub fn lang_name(l: Lang) -> &'static str {
    match l {
        Lang::Th => "Thai",
        Lang::En => "English",
    }
}

/// Every start of the program is a fresh start: the default window layout and new tables.
/// The settings (cutting, display, language, company), the material library and saved
/// projects stay. QA screenshots (--shot) still read their own working folder.
static FRESH_START: AtomicBool = AtomicBool::new(true);

pub unsafe fn start(args: &[String]) -> i32 {
    theme::init_scale();
    theme::register_fonts();
    theme::gdiplus_init();
    settings::migrate_legacy();
    let fresh = !args.iter().any(|a| a == "--shot");
    FRESH_START.store(fresh, Ordering::Relaxed);
    let mut iface = settings::load_interface();
    if fresh {
        // the settings stay; only the window and panel layout start from the default
        let d = Interface::default();
        (iface.window_w, iface.window_h, iface.window_x, iface.window_y, iface.window_max) = (d.window_w, d.window_h, d.window_x, d.window_y, d.window_max);
        (iface.basic_data_ratio, iface.parts_ratio, iface.basic_data_width, iface.parts_panel_height) = (d.basic_data_ratio, d.parts_ratio, d.basic_data_width, d.parts_panel_height);
    }
    i18n::set_lang(if iface.language == "English" { Lang::En } else { Lang::Th });
    vcl::register_classes();
    vcl::ui().text_hook = Some(text_hook);
    vcl::ui().handler = Some(on_event);
    vcl::ui().paint_hook = Some(paint_hook);
    vcl::ui().key_hook = Some(key_hook);
    grid::set_handler(on_grid);
    let main = Form::load_comp("MAIN", forms::main_window());
    let a = Box::new(App {
        main,
        iface,
        technical: settings::load_technical(),
        materials: settings::load_materials(),
        sol: None,
        undo: None,
        parts_file: None,
        parts_dirty: false,
        comments: String::new(),
        creation_time: settings::now_stamp(),
        run: None,
        status: String::new(),
    });
    APP.with(|p| p.set(Box::into_raw(a)));
    let f = form(main);
    f.create(null_mut());
    place_window(f);
    setup_main(f);
    if let Some(i) = args.iter().position(|a| a == "--shot") {
        let dir = args.get(i + 1).cloned().unwrap_or_else(|| ".".into());
        let code = crate::qa::shots(f, &dir, &args[(i + 2).min(args.len())..]);
        // "stay": keep the window open after the captures (to drive it by hand or by script)
        if !args.iter().any(|a| a == "stay") {
            return code;
        }
        return vcl::run_loop(f.hwnd);
    }
    ShowWindow(f.hwnd, if app().iface.window_max && std::env::var_os("NERAMIT_SIZE").is_none() { SW_SHOWMAXIMIZED } else { SW_SHOWNORMAL });
    UpdateWindow(f.hwnd);
    // `--check-updates`: Help > Check for updates right at start (QA, and for a shortcut)
    if args.iter().any(|a| a == "--check-updates") {
        crate::update::check_ui(f.hwnd);
    }
    let code = vcl::run_loop(f.hwnd);
    save_session();
    // a downloaded update starts once the window has closed and the session is saved
    if let Err(e) = crate::update::on_exit() {
        crate::update::error_box(&e);
    }
    code
}

fn layout_flag(cols: &[ColumnLayout], key: &str) -> bool {
    cols.iter().find(|c| c.key == key).map(|c| c.visible).unwrap_or(true)
}

unsafe fn setup_main(f: &mut Form) {
    let i = app().iface.clone();
    for (n, v) in [
        ("results_piece_length_show_1D_mi", i.show_piece_size),
        ("results_piece_label_show_1D_mi", i.show_piece_label),
        ("results_waste_length_show_1D_mi", i.show_waste_size),
        ("results_as_text_1D_mi", i.as_text),
        ("results_pieces_GroupIdentical_1D_mi", i.group_identical),
        ("results_piece_customer_show_1D_mi", i.show_customer),
        ("results_piece_angles_show_1D_mi", i.show_angles),
    ] {
        f.menu_check(n, v);
    }
    setup_grids(f);
    sync_column_menus(f);
    load_session(f);
    apply_panel_sizes(f);
    accept_caption(f);
    set_results_enabled(f, false);
    lang_buttons(f);
    f.realign(0);
    SetFocus(f.hwnd_of("sgParts"));
    status(f);
}

/// Result commands are available only while a solution is shown.
pub unsafe fn set_results_enabled(f: &mut Form, on: bool) {
    for b in ["results_accept_undo_tb", "results_report_tb"] {
        f.toolbar_set("tb_results_menu", b, Some(on), None);
    }
}

unsafe fn lang_buttons(f: &mut Form) {
    let th = i18n::lang() == Lang::Th;
    f.toolbar_set("tbLang", "lang_th", None, Some(th));
    f.toolbar_set("tbLang", "lang_en", None, Some(!th));
}

pub const STAT_GRIDS: [&str; 6] = ["pgUtilizedParts1D", "pgUtilizedStock1D", "pgReusedWaste1D", "pgGarbageWaste1D", "pgMaterialStatistics1D", "pgMiscel1D"];

unsafe fn setup_grids(f: &mut Form) {
    let i = app().iface.clone();
    grid::with(f.hwnd_of("sgParts"), |g| {
        mw::style_grid(g, &i, mw::ROW_H);
        g.fixed_cols = 1;
        g.header_check = true;
        g.cols = mw::make_columns(&i.parts_cols, true);
        g.sort_col = None;
        for k in HIDDEN_PART_COLS {
            mw::set_col_visible(g, k, false);
        }
        g.choices = Some(material_choices);
        g.empty_text = i18n::t("empty_parts");
        g.empty_icon = Some(theme::icon::ADD);
        g.fit_all = true;
    });
    grid::with(f.hwnd_of("sgStock"), |g| {
        mw::style_grid(g, &i, mw::ROW_H);
        g.fixed_cols = 1;
        g.header_check = true;
        g.cols = mw::make_columns(&i.stock_cols, true);
        g.sort_col = None;
        for k in HIDDEN_STOCK_COLS {
            mw::set_col_visible(g, k, false);
        }
        g.choices = Some(material_choices);
        g.empty_text = i18n::t("empty_stock");
        g.empty_icon = Some(theme::icon::LAYERS);
        g.fit_all = true;
    });
    for n in ["sgParts", "sgStock"] {
        grid::with(f.hwnd_of(n), |g| {
            // two-digit row numbers next to the check box
            if let Some(c) = g.cols.iter_mut().find(|c| c.key == "index") {
                c.width = c.width.max(px(56));
            }
        });
    }
    // typing in the job name box: the edit's own face (Thai marks stack in plain GDI)
    SendMessageW(f.hwnd_of("edJobName"), WM_SETFONT, font(&theme::native_text_font(&theme::font(theme::BODY))) as usize, 1);
    results::setup_results_grid(f.hwnd_of("g_results_1d"), &i);
    for (k, n) in STAT_GRIDS.iter().enumerate() {
        results::setup_stat_grid(f.hwnd_of(n), k, &i);
    }
}

pub fn digits() -> (i32, i32) {
    (app().iface.num_digits, app().iface.num_digits_price)
}

unsafe fn load_session(f: &mut Form) {
    let path = settings::dir("").join("session.xml");
    let (d, dp) = digits();
    let loaded = if FRESH_START.load(Ordering::Relaxed) { None } else { settings::read_file(&path).ok().and_then(|t| xmlio::read_project(&t).ok()) };
    match loaded {
        Some(l) => {
            mw::fill_parts(f.hwnd_of("sgParts"), &l.parts.unwrap_or_default(), d);
            mw::fill_stock(f.hwnd_of("sgStock"), &l.stock.unwrap_or_default(), d, dp);
            app().comments = l.comments.unwrap_or_default();
            set_text(f.hwnd_of("edJobName"), &l.name);
            if !l.creation_time.is_empty() {
                app().creation_time = l.creation_time;
            }
        }
        None if FRESH_START.load(Ordering::Relaxed) => {
            // a fresh start: one new row in each table (length 8888, material "Select")
            mw::fill_parts(f.hwnd_of("sgParts"), &[], d);
            mw::fill_stock(f.hwnd_of("sgStock"), &[], d, dp);
            for n in ["sgStock", "sgParts"] {
                grid::with(f.hwnd_of(n), mw::append_row);
            }
        }
        None => {
            let s = Project::sample();
            mw::fill_parts(f.hwnd_of("sgParts"), &s.parts, d);
            mw::fill_stock(f.hwnd_of("sgStock"), &s.stock, d, dp);
        }
    }
}

pub unsafe fn current_project() -> Project {
    let f = main_form();
    let a = app();
    Project {
        parts: grid::with(f.hwnd_of("sgParts"), |g| mw::read_parts_lenient(g)).unwrap_or_default(),
        stock: grid::with(f.hwnd_of("sgStock"), |g| mw::read_stock_lenient(g)).unwrap_or_default(),
        materials: a.materials.clone(),
        technical: a.technical.clone(),
        comments: a.comments.clone(),
        name: get_text(f.hwnd_of("edJobName")).trim().to_string(),
        demand_file_name: a.parts_file.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        creation_time: a.creation_time.clone(),
    }
}

/// The main window where the last session left it (when that is still on a screen), else
/// the default size centred on the work area.
unsafe fn place_window(f: &mut Form) {
    let i = &app().iface;
    let mut wa = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut wa as *mut RECT as _, 0);
    let saved = i.window_w > 0 && i.window_h > 0;
    let (w, h) = if saved { (px(i.window_w), px(i.window_h)) } else { (px(DEFAULT_WINDOW.0), px(DEFAULT_WINDOW.1)) };
    let (w, h) = (w.min(wa.right - wa.left).max(px(980)), h.min(wa.bottom - wa.top).max(px(620)));
    let (mut x, mut y) = (wa.left + (wa.right - wa.left - w).max(0) / 2, wa.top + (wa.bottom - wa.top - h).max(0) / 2);
    if saved {
        let r = RECT { left: i.window_x, top: i.window_y, right: i.window_x + w, bottom: i.window_y + h };
        if !MonitorFromRect(&r, MONITOR_DEFAULTTONULL).is_null() {
            (x, y) = (i.window_x, i.window_y);
        }
    }
    SetWindowPos(f.hwnd, null_mut(), x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
}

/// Size, place and maximized state of the main window for the next start.
unsafe fn save_window_place(f: &Form) {
    let mut wp: WINDOWPLACEMENT = std::mem::zeroed();
    wp.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
    if GetWindowPlacement(f.hwnd, &mut wp) == 0 {
        return;
    }
    let r = wp.rcNormalPosition;
    let s = theme::scale();
    let i = &mut app().iface;
    i.window_w = ((r.right - r.left) as f64 / s).round() as i32;
    i.window_h = ((r.bottom - r.top) as f64 / s).round() as i32;
    (i.window_x, i.window_y) = (r.left, r.top);
    i.window_max = if wp.showCmd == SW_SHOWMINIMIZED as u32 { wp.flags & WPF_RESTORETOMAXIMIZED != 0 } else { IsZoomed(f.hwnd) != 0 };
}

unsafe fn save_session() {
    let f = main_form();
    save_layout(f);
    save_window_place(f);
    let prj = current_project();
    let doc = xmlio::project_doc(&prj, None, xmlio::Sections::all(), VERSION);
    let _ = settings::write_file(&settings::dir("").join("session.xml"), &doc);
    settings::save_interface(&app().iface);
    settings::save_technical(&app().technical);
}

/// Column widths and panel sizes back into the settings (in 96-DPI units).
pub unsafe fn save_layout(f: &mut Form) {
    let a = app();
    grid::with(f.hwnd_of("sgParts"), |g| mw::save_layout(g, &mut a.iface.parts_cols));
    grid::with(f.hwnd_of("sgStock"), |g| mw::save_layout(g, &mut a.iface.stock_cols));
    grid::with(f.hwnd_of("g_results_1d"), |g| mw::save_layout(g, &mut a.iface.results_cols));
}

/// Panel sizes as shares of the window, so they follow every resize: the share the user
/// dragged a splitter to, else the default (pieces and stock 45 % of the width, pieces
/// 61.3 % of that column).
unsafe fn apply_panel_sizes(f: &mut Form) {
    let (bw, bh) = f.client_size(f.id("pBody"));
    let avail_w = (bw - px(24) - px(12)) as f64;
    // the job bar takes the top of the body
    let job_h = f.c("pJob").h + px(12);
    let avail_h = (bh - px(24) - px(12) - job_h) as f64;
    let i = &mut app().iface;
    // pixel sizes of older settings become shares of the current window, once
    if i.basic_data_width > 0 && avail_w > 0.0 {
        i.basic_data_ratio = (px(i.basic_data_width) as f64 / avail_w).clamp(0.2, 0.8);
        i.basic_data_width = 0;
    }
    if i.parts_panel_height > 0 && avail_h > 0.0 {
        i.parts_ratio = (px(i.parts_panel_height) as f64 / avail_h).clamp(0.15, 0.85);
        i.parts_panel_height = 0;
    }
    let rw = if i.basic_data_ratio > 0.0 { i.basic_data_ratio } else { DEFAULT_SHARES.0 };
    let rh = if i.parts_ratio > 0.0 { i.parts_ratio } else { DEFAULT_SHARES.1 };
    if avail_w > 0.0 {
        f.c("pBasicData").w = (avail_w * rw).round().max(px(360) as f64) as i32;
    }
    if avail_h > 0.0 {
        f.c("pParts").h = (avail_h * rh).round().max(px(160) as f64) as i32;
    }
}

/// The last status message (there is no status bar; kept for the log of what happened).
pub unsafe fn set_status(text: &str) {
    app().status = text.to_string();
}

/// Window title: the program name and the open pieces file.
pub unsafe fn status(f: &mut Form) {
    let a = app();
    let file = a.parts_file.as_ref().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().into_owned());
    a.status = file.clone().unwrap_or_else(|| i18n::t("ready"));
    let title = match file {
        Some(n) => format!("Neramit Cut — {n}"),
        None => "Neramit Cut".to_string(),
    };
    SetWindowTextW(f.hwnd, wide(&title).as_ptr());
}

pub unsafe fn msg(text: &str, kind: u32) -> i32 {
    let caption = match kind {
        MB_ICONERROR => i18n::t("error"),
        MB_ICONWARNING => i18n::t("warning"),
        MB_ICONQUESTION => i18n::t("confirm"),
        _ => i18n::t("information"),
    };
    let flags = if kind == MB_ICONQUESTION { MB_YESNO | MB_ICONQUESTION } else { MB_OK | kind };
    let owner = vcl::ui().forms.iter().flatten().rev().find(|f| f.modal && !f.hwnd.is_null()).map(|f| f.hwnd).unwrap_or(main_hwnd());
    message_box(owner, text, &caption, flags)
}

pub unsafe fn ask(text: &str) -> bool {
    msg(text, MB_ICONQUESTION) == IDYES
}

// ------------------------------------------------------------------ painting

/// Custom surfaces: logo, KPI strip, status bar (main window) and dialog previews.
fn paint_hook(fi: usize, ctl: usize, dc: HDC, r: &RECT) -> bool {
    unsafe {
        let f = form(fi);
        if fi != app().main {
            return dlg::paint(fi, ctl, dc, r);
        }
        match f.ctls[ctl].name.as_str() {
            "pParts" | "pStock" => {
                vcl::paint_card(f, ctl, dc, r);
                paint_card_footer(ctl == f.id("pParts"), dc, r);
                true
            }
            "pJobDate" => {
                paint_behind_in_card(f, ctl, dc);
                let tr = RECT { left: 0, top: 0, right: r.right - px(16), bottom: r.bottom };
                let text = format!("{}  {}", i18n::t("report_date"), results::today());
                theme::text(dc, &text, &tr, &theme::font(theme::SMALL + 1), theme::TEXT_MUTED, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
                true
            }
            "pJobTitle" => {
                // section 0: badge and title, like the other cards
                paint_behind_in_card(f, ctl, dc);
                let d = px(24);
                let y = (r.bottom - d) / 2;
                theme::badge(dc, &RECT { left: px(14), top: y, right: px(14) + d, bottom: y + d }, "0", theme::ACCENT, theme::WHITE);
                let tr = RECT { left: px(14) + d + px(10), top: 0, right: r.right, bottom: r.bottom };
                theme::text(dc, &i18n::t("job_name"), &tr, &theme::font_semibold(15), theme::TEXT, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                true
            }
            _ => false,
        }
    }
}

/// A panel inside a card paints the part of the card behind it (rounded corners and border
/// included), so its own rectangle never hides the card's corners.
unsafe fn paint_behind_in_card(f: &Form, ctl: usize, dc: HDC) {
    let c = &f.ctls[ctl];
    let Some(card) = c.parent else { return };
    let r = RECT { left: -c.x, top: -c.y, right: f.ctls[card].w - c.x, bottom: f.ctls[card].h - c.y };
    vcl::paint_card(f, card, dc, &r);
}

/// The materials a material cell offers: the rebar sizes, then the material library and what
/// the tables use.
fn material_choices(g: &grid::Grid, col: usize) -> Option<Vec<String>> {
    if g.cols.get(col)?.key != "material" {
        return None;
    }
    let mut names: Vec<String> = app().materials.iter().map(|m| m.name.clone()).filter(|n| !n.is_empty()).collect();
    for r in 1..=g.rows.len() {
        let v = g.cell(r, col).trim().to_string();
        if !v.is_empty() && !names.contains(&v) {
            names.push(v);
        }
    }
    names.retain(|n| !REBAR_MATERIALS.contains(&n.as_str()) && n != OLD_SAMPLE_MATERIAL);
    results::material_order(&mut names);
    let mut list: Vec<String> = REBAR_MATERIALS.iter().map(|s| s.to_string()).collect();
    list.extend(names);
    Some(list)
}

/// Thousands separators for the summaries (31200 -> 31,200).
fn group_thousands(s: &str) -> String {
    let (sign, rest) = if let Some(r) = s.strip_prefix('-') { ("-", r) } else { ("", s) };
    let (int, frac) = match rest.find('.') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let mut out = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    format!("{sign}{out}{frac}")
}

/// Footer text of a data card: rows, pieces (or bars), total length; and how many rows are
/// ticked for the calculation.
unsafe fn card_summary(parts: bool) -> (String, String) {
    let f = main_form();
    let d = digits().0;
    // (rows, ticked rows, quantity, total length)
    let (rows, used, qty, len) = if parts {
        let v = grid::with(f.hwnd_of("sgParts"), |g| mw::read_parts_lenient(g)).unwrap_or_default();
        let u: Vec<&Part> = v.iter().filter(|p| p.use_it).collect();
        (v.len(), u.len(), u.iter().map(|p| p.quantity.max(0)).sum::<i32>(), u.iter().map(|p| p.length * p.quantity.max(0) as f64).sum::<f64>())
    } else {
        let v = grid::with(f.hwnd_of("sgStock"), |g| mw::read_stock_lenient(g)).unwrap_or_default();
        let u: Vec<&Stock> = v.iter().filter(|s| s.use_it).collect();
        (v.len(), u.len(), u.iter().map(|s| s.quantity.max(0)).sum::<i32>(), u.iter().map(|s| s.length * s.quantity.max(0) as f64).sum::<f64>())
    };
    let key = if parts { "card_parts" } else { "card_stock" };
    let left = i18n::tf(key, &[&rows.to_string(), &group_thousands(&qty.to_string()), &group_thousands(&fmt_num(len, d))]);
    let right = i18n::tf("card_used", &[&used.to_string(), &rows.to_string()]);
    (left, right)
}

unsafe fn paint_card_footer(parts: bool, dc: HDC, r: &RECT) {
    let (left, right) = card_summary(parts);
    let fh = px(forms::CARD_FOOT);
    let band = RECT { left: r.left + px(16), top: r.bottom - fh, right: r.right - px(16), bottom: r.bottom };
    let tone = theme::BLUE;
    let d = px(8);
    let cy = (band.top + band.bottom) / 2;
    theme::ellipse(dc, &RECT { left: band.left, top: cy - d / 2, right: band.left + d, bottom: cy - d / 2 + d }, tone.main);
    let fnt = theme::font(theme::SMALL);
    let rw = measure(&fnt, &right).cx;
    let narrow = band.right - band.left < rw + px(260);
    let lr = RECT { left: band.left + d + px(8), top: band.top, right: if narrow { band.right } else { band.right - rw - px(16) }, bottom: band.bottom };
    theme::text(dc, &left, &lr, &fnt, theme::TEXT_MUTED, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    if !narrow {
        theme::text(dc, &right, &band, &fnt, theme::TEXT_SUBTLE, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
    }
}

// ------------------------------------------------------------------ events

fn on_event(fi: usize, ctl: usize, ev: &Ev) -> bool {
    unsafe {
        if fi != app().main {
            if let Some(r) = app().run.as_ref() {
                if r.form == fi {
                    return on_progress(fi, ctl, ev);
                }
            }
            return dlg::on_event(fi, ctl, ev);
        }
        let f = form(fi);
        match ev {
            Ev::Close => {
                PostQuitMessage(0);
                false
            }
            Ev::Menu(m) => {
                on_menu(f, m);
                true
            }
            Ev::Button(i) => {
                let b = f.toolbar_button(ctl, *i);
                let tb = f.ctls[ctl].name.clone();
                on_tool(f, &tb, &b);
                true
            }
            Ev::Custom(s) if s == "SplitterMoved" => {
                // remember the share of the splitter the user moved; the other one stays as it was
                let (bw, bh) = f.client_size(f.id("pBody"));
                let (aw, ah) = ((bw - px(24) - px(12)).max(1) as f64, (bh - px(24) - px(12) - f.c("pJob").h - px(12)).max(1) as f64);
                match f.ctls[ctl].name.as_str() {
                    "pBody" => app().iface.basic_data_ratio = (f.c("pBasicData").w as f64 / aw).clamp(0.2, 0.8),
                    "pBasicData" => app().iface.parts_ratio = (f.c("pParts").h as f64 / ah).clamp(0.15, 0.85),
                    _ => {}
                }
                save_layout(f);
                true
            }
            Ev::DblClick if matches!(f.ctls[ctl].name.as_str(), "pBasicSplitter" | "spt_parts_stock") => {
                // double-click a splitter: back to the balanced layout
                if f.ctls[ctl].name == "pBasicSplitter" {
                    app().iface.basic_data_ratio = 0.0;
                } else {
                    app().iface.parts_ratio = 0.0;
                }
                apply_panel_sizes(f);
                f.realign(f.id("pBody"));
                true
            }
            Ev::Resize => {
                keep_panels_visible(f);
                false
            }
            _ => false,
        }
    }
}

unsafe fn popup_below(f: &mut Form, tb: &str, button: &str, menu: &str) {
    let r = f.toolbar_button_rect(tb, button);
    f.track_popup(menu, r.left, r.bottom + px(2));
}

const RESULT_COLUMN_MENU: [(&str, &str); 8] = [
    ("mi_show_image_column_results_1d", "image"),
    ("mi_show_material_column_results_1d", "material"),
    ("mi_show_waste_column_results_1d", "waste"),
    ("mi_show_waste_percent_column_results_1d", "waste_percent"),
    ("mi_show_price_column_results_1d", "price"),
    ("mi_show_mass_column_results_1d", "mass"),
    ("mi_show_area_column_results_1d", "area"),
    ("mi_show_pattern_column_results_1d", "pattern"),
];

/// (menu item, grid, column keys)
const DATA_COLUMN_MENU: [(&str, &str, &[&str]); 6] = [
    ("parts_cols_index_mi", "sgParts", &["index"]),
    ("parts_cols_customer_mi", "sgParts", &["customer"]),
    ("stock_cols_index_mi", "sgStock", &["index"]),
    ("stock_cols_price_mi", "sgStock", &["price"]),
    ("stock_cols_weight_mi", "sgStock", &["weight"]),
    ("stock_cols_priority_mi", "sgStock", &["priority"]),
];

unsafe fn sync_column_menus(f: &mut Form) {
    let h = f.hwnd_of("g_results_1d");
    for (m, key) in RESULT_COLUMN_MENU {
        let v = grid::with(h, |g| g.cols.iter().find(|c| c.key == key).map(|c| c.visible)).flatten().unwrap_or_else(|| layout_flag(&app().iface.results_cols, key));
        f.menu_check(m, v);
    }
    for (m, gname, keys) in DATA_COLUMN_MENU {
        let v = grid::with(f.hwnd_of(gname), |g| g.cols.iter().find(|c| c.key == keys[0]).map(|c| c.visible)).flatten().unwrap_or(true);
        f.menu_check(m, v);
    }
}

unsafe fn on_tool(f: &mut Form, tb: &str, b: &str) {
    match b {
        // ---- app bar
        "ab_file" => popup_below(f, tb, b, "pm_file"),
        "ab_settings" => popup_below(f, tb, b, "pm_settings"),
        "ab_help" => popup_below(f, tb, b, "pm_help"),
        "materials_mi" => dlg::materials(),
        "mi_runs" => latest_solutions(f, tb, b),
        "ab_open_project" => export::project_import(f, false),
        "ab_save_project" => export::project_export(f, true),
        "lang_th" | "lang_en" => {
            let l = if b == "lang_th" { Lang::Th } else { Lang::En };
            if l != i18n::lang() {
                i18n::set_lang(l);
                apply_language(f);
            } else {
                lang_buttons(f);
            }
        }
        // ---- parts
        "parts_append_new_row_tb" => {
            grid::with(f.hwnd_of("sgParts"), mw::append_row);
            app().parts_dirty = true;
            data_changed(f);
        }
        "parts_delete_row_tb" => {
            let has = grid::with(f.hwnd_of("sgParts"), |g| g.row > 0 && g.row <= g.rows.len()).unwrap_or(false);
            if has && ask(&i18n::t("delete_piece")) {
                grid::with(f.hwnd_of("sgParts"), mw::delete_row);
                app().parts_dirty = true;
                data_changed(f);
            }
        }
        "parts_load_tb" => export::parts_open(f, false),
        "parts_import_tb" => dlg::import_data(false, false),
        "parts_save_tb" => export::parts_save(f, false),
        "parts_print_tb" => export::print_table(f, "sgParts", &i18n::t("pieces")),
        "parts_print_labels_tb" => dlg::labels(false),
        "parts_columns_tb" => {
            sync_column_menus(f);
            popup_below(f, tb, b, "parts_columns_pm");
        }
        "parts_additional_commands_b" => popup_below(f, tb, b, "parts_additional_commands_pm"),
        // ---- stock
        "stock_append_new_row_tb" => {
            grid::with(f.hwnd_of("sgStock"), mw::append_row);
            data_changed(f);
        }
        "stock_delete_row_tb" => {
            let has = grid::with(f.hwnd_of("sgStock"), |g| g.row > 0 && g.row <= g.rows.len()).unwrap_or(false);
            if has && ask(&i18n::t("delete_piece")) {
                grid::with(f.hwnd_of("sgStock"), mw::delete_row);
                data_changed(f);
            }
        }
        "stock_import_tb" => dlg::import_data(true, false),
        "stock_print_tb" => export::print_table(f, "sgStock", &i18n::t("stock")),
        "stock_labels_print_tb" => dlg::labels(true),
        "stock_history_tb" => dlg::history(),
        "stock_columns_tb" => {
            sync_column_menus(f);
            popup_below(f, tb, b, "stock_columns_pm");
        }
        "stock_additional_commands_b" => popup_below(f, tb, b, "stock_additional_commands_pm"),
        // ---- results
        "btnStart" => start_optimization(f),
        "results_accept_undo_tb" => accept_or_undo(f),
        "results_print_tb" => dlg::print_results(),
        "results_as_tabular_tb" => dlg::tabular(),
        "results_adhesive_labels_print_tb" => dlg::labels_results(),
        "results_report_tb" => popup_below(f, tb, b, "pm_report"),
        "results_report_pdf_mi" => export::save_report_pdf(f),
        "results_view_tb" => {
            sync_column_menus(f);
            popup_below(f, tb, b, "pm_additional_options_1d");
        }
        _ => {}
    }
}

unsafe fn on_menu(f: &mut Form, m: &str) {
    let a = app();
    match m {
        "miExit" => PostQuitMessage(0),
        "settings_technical_1d_mi" => dlg::technical_1d(),
        "settings_technical_general_mi" => dlg::technical_general(),
        "settings_technical_default_mi" => {
            if ask(&i18n::t("restore_defaults")) {
                a.technical = Technical::default();
                settings::save_technical(&a.technical);
            }
        }
        "settings_interface_set_mi" => dlg::interface_settings(),
        "settings_interface_default_mi" => {
            if ask(&i18n::t("restore_defaults")) {
                let lang = a.iface.language.clone();
                a.iface = Interface { language: lang, ..Interface::default() };
                apply_interface(f);
            }
        }
        "mi_Language" => dlg::language(),
        "settings_working_dir_mi" => {
            if let Some(p) = crate::ui::common::choose_folder(f.hwnd, &i18n::tr("Working folder…")) {
                let _ = settings::set_working_dir(&p);
                a.technical = settings::load_technical();
                a.materials = settings::load_materials();
                set_status(&p.to_string_lossy());
            }
        }
        "project_import_mi" => export::project_import(f, false),
        "project_export_selected_mi" => export::project_export(f, false),
        "project_export_everything_mi" => export::project_export(f, true),
        "project_about_mi" => dlg::project_about(),
        "miOnlineHelp" => {
            msg(&i18n::t("shortcuts"), MB_ICONINFORMATION);
        }
        "project_import_service_mi" => export::project_import(f, true),
        "project_export_service_mi" => export::service_export(f),
        "About1" => dlg::about(),
        "mi_updates" => crate::update::check_ui(f.hwnd),
        // parts "More"
        "parts_save_as_mi" => export::parts_save(f, true),
        "parts_export_mi" => export::table_export(f, "sgParts"),
        "parts_load_append_mi" => export::parts_open(f, true),
        "parts_import_append_mi" => dlg::import_data(false, true),
        "parts_group_identical_mi" => {
            let (d, _) = digits();
            let p = grid::with(f.hwnd_of("sgParts"), |g| mw::read_parts_lenient(g)).unwrap_or_default();
            mw::fill_parts(f.hwnd_of("sgParts"), &mw::group_parts(&p), d);
            a.parts_dirty = true;
            data_changed(f);
        }
        "parts_clear_mi" => {
            if ask(&i18n::t("delete_all")) {
                if a.parts_dirty && ask(&i18n::t("parts_changed")) {
                    export::parts_save(f, false);
                }
                grid::with(f.hwnd_of("sgParts"), mw::clear);
                app().parts_file = None;
                app().parts_dirty = false;
                status(f);
                data_changed(f);
            }
        }
        // stock "More" / "Tools"
        "stock_export_mi" => export::table_export(f, "sgStock"),
        "stock_import_append_mi" => dlg::import_data(true, true),
        "stock_clear_grid_mi" => {
            if ask(&i18n::t("delete_all")) {
                grid::with(f.hwnd_of("sgStock"), mw::clear);
                data_changed(f);
            }
        }
        "stock_group_identical_mi" => {
            let (d, dp) = digits();
            let s = grid::with(f.hwnd_of("sgStock"), |g| mw::read_stock_lenient(g)).unwrap_or_default();
            mw::fill_stock(f.hwnd_of("sgStock"), &mw::group_stock(&s), d, dp);
            data_changed(f);
        }
        "stock_find_size_mi" => dlg::find_size(),
        // results export
        "results_save_as_image_tb" => export::save_image(f),
        "results_save_as_csv_tb" => export::save_results_table(f),
        "results_to_dxf_tb" => dlg::dxf_export(),
        "results_1d_copy_mi" | "results_export_copy_mi" => export::copy_results(f),
        // results display options
        "results_piece_length_show_1D_mi"
        | "results_piece_label_show_1D_mi"
        | "results_waste_length_show_1D_mi"
        | "results_as_text_1D_mi"
        | "results_pieces_GroupIdentical_1D_mi"
        | "results_piece_customer_show_1D_mi"
        | "results_piece_angles_show_1D_mi" => {
            let idx = f.index;
            let on = |n: &str| crate::ui::menu::is_checked(idx, n);
            a.iface.show_piece_size = on("results_piece_length_show_1D_mi");
            a.iface.show_piece_label = on("results_piece_label_show_1D_mi");
            a.iface.show_waste_size = on("results_waste_length_show_1D_mi");
            a.iface.as_text = on("results_as_text_1D_mi");
            a.iface.group_identical = on("results_pieces_GroupIdentical_1D_mi");
            a.iface.show_customer = on("results_piece_customer_show_1D_mi");
            a.iface.show_angles = on("results_piece_angles_show_1D_mi");
            results::update_iface(&a.iface);
            grid::with(f.hwnd_of("g_results_1d"), |g| g.invalidate());
        }
        m if m.starts_with("mi_show_") => {
            if let Some((_, key)) = RESULT_COLUMN_MENU.iter().find(|(n, _)| *n == m) {
                let v = crate::ui::menu::is_checked(f.index, m);
                grid::with(f.hwnd_of("g_results_1d"), |g| {
                    mw::set_col_visible(g, key, v);
                    g.update_scroll();
                    g.invalidate();
                });
                save_layout(f);
            }
        }
        m if DATA_COLUMN_MENU.iter().any(|(n, _, _)| *n == m) => {
            let (_, gname, keys) = DATA_COLUMN_MENU.iter().find(|(n, _, _)| *n == m).copied().unwrap();
            let v = crate::ui::menu::is_checked(f.index, m);
            grid::with(f.hwnd_of(gname), |g| {
                for k in keys.iter() {
                    mw::set_col_visible(g, k, v);
                }
                g.update_scroll();
                g.invalidate();
            });
            save_layout(f);
        }
        m if m.starts_with("run:") => {
            let i: usize = m[4..].parse().unwrap_or(0);
            if let Some((_, p)) = settings::list_runs().get(i) {
                export::open_run(f, p);
            }
        }
        // commands that moved from the toolbars into the "More" and "Print" menus
        other => on_tool(f, "", other),
    }
}

/// Keep both columns and both data cards usable when the window gets smaller.
unsafe fn keep_panels_visible(f: &mut Form) {
    let (bw, bh) = f.client_size(f.id("pBody"));
    if bw <= 0 || bh <= 0 {
        return;
    }
    // the panels keep their shares of the window
    let before = (f.c("pBasicData").w, f.c("pParts").h);
    apply_panel_sizes(f);
    let mut changed = before != (f.c("pBasicData").w, f.c("pParts").h);
    let max_w = (bw - px(24) - px(12) - px(520)).max(px(380));
    if f.c("pBasicData").w > max_w {
        f.c("pBasicData").w = max_w;
        changed = true;
    }
    let max_h = (bh - px(24) - px(12) - f.c("pJob").h - px(12) - px(170)).max(px(150));
    if f.c("pParts").h > max_h {
        f.c("pParts").h = max_h;
        changed = true;
    }
    if changed {
        f.realign(f.id("pBody"));
    }
}

/// Recompute the summaries under the pieces and stock tables.
pub unsafe fn data_changed(f: &mut Form) {
    for n in ["pParts", "pStock"] {
        let h = f.hwnd_of(n);
        let cr = client_rect(h);
        let band = RECT { left: 0, top: cr.bottom - px(forms::CARD_FOOT), right: cr.right, bottom: cr.bottom };
        InvalidateRect(h, &band, 0);
    }
}

/// "Latest solutions": the saved results of recent runs, newest first.
unsafe fn latest_solutions(f: &mut Form, tb: &str, b: &str) {
    let runs = settings::list_runs();
    if runs.is_empty() {
        msg(&i18n::t("no_solutions"), MB_ICONINFORMATION);
        return;
    }
    let m = CreatePopupMenu();
    for (i, (label, _)) in runs.iter().enumerate().take(20) {
        let cmd = 40000 + i as u16;
        f.menus.insert(cmd, format!("run:{i}"));
        AppendMenuW(m, MF_STRING, cmd as usize, wide(label).as_ptr());
    }
    crate::ui::menu::style_popup(m);
    let r = f.toolbar_button_rect(tb, b);
    TrackPopupMenu(m, TPM_LEFTALIGN | TPM_TOPALIGN | TPM_LEFTBUTTON, r.left, r.bottom + px(2), 0, f.hwnd, std::ptr::null());
    DestroyMenu(m);
}

/// Interface settings changed: fonts, digits, columns.
pub unsafe fn apply_interface(f: &mut Form) {
    let a = app();
    let (d, dp) = digits();
    let parts = grid::with(f.hwnd_of("sgParts"), |g| mw::read_parts_lenient(g)).unwrap_or_default();
    let stock = grid::with(f.hwnd_of("sgStock"), |g| mw::read_stock_lenient(g)).unwrap_or_default();
    let i = a.iface.clone();
    for n in ["sgParts", "sgStock"] {
        grid::with(f.hwnd_of(n), |g| mw::style_grid(g, &i, mw::ROW_H));
    }
    mw::fill_parts(f.hwnd_of("sgParts"), &parts, d);
    mw::fill_stock(f.hwnd_of("sgStock"), &stock, d, dp);
    grid::with(f.hwnd_of("g_results_1d"), |g| mw::style_grid(g, &i, results::ROW_H));
    for n in STAT_GRIDS {
        grid::with(f.hwnd_of(n), |g| mw::style_grid(g, &i, mw::ROW_H));
    }
    results::update_iface(&i);
    refresh_results(f);
    settings::save_interface(&a.iface);
}

/// Language changed: captions of every open form, grid titles, toolbars.
pub unsafe fn apply_language(f: &mut Form) {
    let a = app();
    a.iface.language = lang_name(i18n::lang()).into();
    for fo in vcl::ui().forms.iter_mut().flatten() {
        if !fo.hwnd.is_null() {
            fo.translate_all();
        }
    }
    let i = a.iface.clone();
    for n in ["sgParts", "sgStock", "g_results_1d"] {
        grid::with(f.hwnd_of(n), |g| {
            mw::retitle(g);
            g.update_scroll();
        });
    }
    grid::with(f.hwnd_of("sgParts"), |g| g.empty_text = i18n::t("empty_parts"));
    grid::with(f.hwnd_of("sgStock"), |g| g.empty_text = i18n::t("empty_stock"));
    for (k, n) in STAT_GRIDS.iter().enumerate() {
        results::setup_stat_grid(f.hwnd_of(n), k, &i);
    }
    accept_caption(f);
    set_results_enabled(f, app().sol.is_some());
    lang_buttons(f);
    status(f);
    f.realign(0);
    refresh_results(f);
    InvalidateRect(f.hwnd, std::ptr::null(), 1);
    settings::save_interface(&a.iface);
}

pub unsafe fn refresh_results(f: &mut Form) {
    results::fill_results(f.hwnd_of("g_results_1d"));
    let hs = STAT_GRIDS.map(|n| f.hwnd_of(n));
    results::fill_stats(&hs);
}

// ------------------------------------------------------------------ keyboard

fn key_hook(m: &MSG) -> bool {
    unsafe {
        let f = main_form();
        // only for the main window and its children
        if GetAncestor(m.hwnd, GA_ROOT) != f.hwnd {
            return false;
        }
        let ctrl = GetKeyState(VK_CONTROL as i32) < 0;
        match (m.wParam as u16, ctrl) {
            (VK_F9, _) => {
                start_optimization(f);
                true
            }
            (0x53, true) => {
                export::project_export(f, true);
                true
            }
            (0x4F, true) => {
                export::project_import(f, false);
                true
            }
            (0x50, true) => {
                // Ctrl+P: the report (PDF)
                export::save_report_pdf(f);
                true
            }
            _ => false,
        }
    }
}

// ------------------------------------------------------------------ optimization

unsafe fn show_cell_error(f: &mut Form, grid_name: &str, e: &mw::CellError) {
    grid::with(f.hwnd_of(grid_name), |g| g.move_to(e.row, e.col));
    SetFocus(f.hwnd_of(grid_name));
    msg(&e.msg, MB_ICONERROR);
}

pub unsafe fn start_optimization(f: &mut Form) {
    let a = app();
    if a.run.is_some() {
        return;
    }
    for n in ["sgParts", "sgStock"] {
        grid::with(f.hwnd_of(n), |g| g.commit_edit(true));
    }
    let parts = match grid::with(f.hwnd_of("sgParts"), |g| mw::read_parts(g)).unwrap() {
        Ok(p) => p,
        Err(e) => return show_cell_error(f, "sgParts", &e),
    };
    let stock = match grid::with(f.hwnd_of("sgStock"), |g| mw::read_stock(g)).unwrap() {
        Ok(s) => s,
        Err(e) => return show_cell_error(f, "sgStock", &e),
    };
    if !parts.iter().any(|p| p.use_it && p.quantity > 0) {
        msg(&i18n::t("need_piece"), MB_ICONERROR);
        return;
    }
    if !stock.iter().any(|s| s.use_it && s.quantity > 0) {
        msg(&i18n::t("need_stock"), MB_ICONERROR);
        return;
    }
    if let Some(r) = parts.iter().position(|p| p.use_it && (p.a1 != 90.0 || p.a2 != 90.0) && p.height <= 0.0) {
        let c = grid::with(f.hwnd_of("sgParts"), |g| mw::col_index(g, "width").unwrap_or(1)).unwrap_or(1);
        return show_cell_error(f, "sgParts", &mw::CellError { row: r + 1, col: c, msg: i18n::t("angles_need_height") });
    }
    let mats_parts: Vec<&str> = parts.iter().filter(|p| p.use_it).map(|p| p.material.as_str()).collect();
    if !mats_parts.iter().any(|m| stock.iter().any(|s| s.use_it && s.material == *m)) {
        msg(&i18n::t("materials_mismatch"), MB_ICONERROR);
        return;
    }
    // a new random seed for every run
    a.technical.linear.random_seed = a.technical.linear.random_seed.wrapping_add(1);
    settings::save_technical(&a.technical);
    let prj = Project { parts, stock, materials: a.materials.clone(), technical: a.technical.clone(), comments: a.comments.clone(), name: get_text(f.hwnd_of("edJobName")).trim().to_string(), demand_file_name: String::new(), creation_time: a.creation_time.clone() };
    let total: usize = prj.parts.iter().filter(|p| p.use_it).map(|p| p.quantity.max(0) as usize).sum();
    let cancel = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicUsize::new(0));
    let (c2, d2, p2) = (cancel.clone(), done.clone(), prj.clone());
    let handle = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || engine::optimize(&p2, &engine::Progress { cancel: &|| c2.load(Ordering::Relaxed), report: &|n, _| d2.store(n, Ordering::Relaxed) }))
        .ok();
    let pf = Form::load_comp("DLG_PROGRESS", forms::progress_window());
    a.run = Some(Run { form: pf, cancel, done, total, handle, prj });
    let p = form(pf);
    p.create(f.hwnd);
    let bar = p.hwnd_of("pb_optimization_progress");
    SendMessageW(bar, 0x0406 /* PBM_SETRANGE32 */, 0, total.max(1) as isize);
    SetTimer(p.hwnd, 1, 50, None);
    p.show_modal(f.hwnd);
    KillTimer(p.hwnd, 1);
    vcl::destroy_form(pf);
    let run = app().run.take();
    if let Some(mut run) = run {
        let sol = run.handle.take().and_then(|h| h.join().ok());
        if let Some(sol) = sol {
            if !run.cancel.load(Ordering::Relaxed) {
                show_solution(f, run.prj, sol, true);
            }
        }
    }
}

unsafe fn on_progress(fi: usize, ctl: usize, ev: &Ev) -> bool {
    let f = form(fi);
    let Some(run) = app().run.as_ref() else { return false };
    match ev {
        Ev::Custom(t) if t == "timer1" => {
            let bar = f.hwnd_of("pb_optimization_progress");
            let done = run.done.load(Ordering::Relaxed);
            SendMessageW(bar, 0x0402 /* PBM_SETPOS */, done, 0);
            f.set_text("lProgressInfo", &i18n::tf("progress_pieces", &[&done.to_string(), &run.total.to_string()]));
            if run.handle.as_ref().map(|h| h.is_finished()).unwrap_or(true) {
                f.close(1);
            }
            true
        }
        Ev::Click if f.ctls[ctl].name == "bbStop" => {
            run.cancel.store(true, Ordering::Relaxed);
            true
        }
        Ev::Close => {
            run.cancel.store(true, Ordering::Relaxed);
            true
        }
        _ => false,
    }
}

pub unsafe fn show_solution(f: &mut Form, prj: Project, sol: Solution, fresh: bool) {
    let a = app();
    let max_len = sol.bars.iter().map(|b| b.length).fold(0.0, f64::max);
    results::set_view(Some(View { sol: sol.clone(), prj: prj.clone(), iface: a.iface.clone(), max_len }));
    refresh_results(f);
    if fresh {
        settings::save_run(&prj, &sol);
    }
    a.sol = Some((prj, sol.clone()));
    a.undo = None;
    accept_caption(f);
    set_results_enabled(f, true);
    let pr = f.id("pcResults");
    f.show_page(pr, 0);
    InvalidateRect(f.hwnd_of("pcResults"), std::ptr::null(), 0);
    let bars: i32 = sol.bars.iter().map(|b| b.quantity).sum();
    set_status(&i18n::tf("status_solved", &[&bars.to_string(), &format!("{:.2}", sol.running_time)]));
    if sol.code == 12 {
        msg(&i18n::t("materials_missing"), MB_ICONWARNING);
    } else if !sol.unplaced.is_empty() {
        msg(&i18n::t("settings_unsatisfied"), MB_ICONWARNING);
    }
}

/// Toolbar caption and icon of Accept / Undo Accept.
pub unsafe fn accept_caption(f: &mut Form) {
    let undo = app().undo.is_some();
    let text = if undo { i18n::t("undo_accept") } else { i18n::t("accept") };
    let tb = f.id("tb_results_menu");
    if let Kind::ToolBar { buttons, .. } = &mut f.ctls[tb].kind {
        if let Some(b) = buttons.iter_mut().find(|b| b.name == "results_accept_undo_tb") {
            b.caption = text;
            b.icon = Some(if undo { theme::icon::UNDO } else { theme::icon::CHECK });
        }
    }
    f.rebuild_toolbar(tb);
    let on = app().sol.is_some();
    set_results_enabled(f, on);
}

unsafe fn accept_or_undo(f: &mut Form) {
    let a = app();
    let (d, dp) = digits();
    if let Some(u) = a.undo.take() {
        mw::fill_parts(f.hwnd_of("sgParts"), &u.parts, d);
        mw::fill_stock(f.hwnd_of("sgStock"), &u.stock, d, dp);
        accept_caption(f);
        set_status(&i18n::t("status_undone"));
        data_changed(f);
        return;
    }
    let Some((prj, sol)) = a.sol.clone() else { return };
    let parts_now = grid::with(f.hwnd_of("sgParts"), |g| mw::read_parts_lenient(g)).unwrap_or_default();
    let stock_now = grid::with(f.hwnd_of("sgStock"), |g| mw::read_stock_lenient(g)).unwrap_or_default();
    settings::save_history(&stock_now);
    let (parts, stock) = results::accept(&prj.parts, &prj.stock, &sol, &prj.technical.linear);
    a.undo = Some(results::Undo { parts: parts_now, stock: stock_now });
    mw::fill_parts(f.hwnd_of("sgParts"), &parts, d);
    mw::fill_stock(f.hwnd_of("sgStock"), &stock, d, dp);
    a.parts_dirty = true;
    accept_caption(f);
    set_status(&i18n::t("status_accepted"));
    data_changed(f);
}

// ------------------------------------------------------------------ grids

fn on_grid(fi: usize, ctl: usize, ev: &GridEv) -> bool {
    unsafe {
        if fi != app().main {
            return dlg::on_grid(fi, ctl, ev);
        }
        let f = form(fi);
        let name = f.ctls[ctl].name.clone();
        let h = f.ctls[ctl].hwnd;
        match (name.as_str(), ev) {
            ("sgParts" | "sgStock", GridEv::CellChanged(r, c, old)) => {
                let key = grid::with(h, |g| g.cols[*c].key.clone()).unwrap_or_default();
                let text = grid::with(h, |g| g.cell(*r, *c).to_string()).unwrap_or_default();
                if let Some(m) = mw::check_cell(&key, &text) {
                    grid::with(h, |g| {
                        g.set_cell(*r, *c, old);
                        g.invalidate();
                    });
                    msg(&m, MB_ICONERROR);
                } else {
                    if name == "sgParts" {
                        app().parts_dirty = true;
                    }
                    data_changed(f);
                }
                true
            }
            ("sgParts" | "sgStock", GridEv::CheckChanged(_)) => {
                data_changed(f);
                false
            }
            ("sgParts" | "sgStock", GridEv::HeaderClick(c)) => {
                grid::with(h, |g| {
                    if g.cols[*c].check {
                        return;
                    }
                    let desc = g.sort_col == Some(*c) && !g.sort_desc;
                    mw::sort_by(g, *c, desc);
                });
                true
            }
            (n, GridEv::HeaderClick(c)) if STAT_GRIDS.contains(&n) => {
                grid::with(h, |g| {
                    let desc = g.sort_col == Some(*c) && !g.sort_desc;
                    mw::sort_by(g, *c, desc);
                });
                true
            }
            ("g_results_1d", GridEv::HeaderClick(c)) => {
                grid::with(h, |g| {
                    if g.cols[*c].key == "pieces" {
                        return;
                    }
                    let desc = g.sort_col == Some(*c) && !g.sort_desc;
                    results::sort_results(g, *c, desc);
                });
                true
            }
            ("sgParts" | "sgStock", GridEv::Paste(t)) => {
                grid::with(h, |g| mw::paste(g, t));
                data_changed(f);
                true
            }
            ("g_results_1d", GridEv::RightClick(_, _, x, y)) => {
                f.track_popup("results_1d_pm", *x, *y);
                true
            }
            ("g_results_1d", GridEv::DblClick(..)) => {
                dlg::tabular();
                true
            }
            ("sgParts" | "sgStock", GridEv::KeyDown(k)) => {
                if *k == 0x2D {
                    grid::with(h, mw::append_row);
                    data_changed(f);
                    return true;
                }
                false
            }
            _ => false,
        }
    }
}
