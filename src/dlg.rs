//! Dialog windows, filled from and written back to the application state.

use crate::app::{self, app, ask, digits, main_form, main_hwnd, msg};
use crate::export;
use crate::forms;
use crate::i18n::{self, Lang};
use crate::mainwin as mw;
use crate::model::*;
use crate::results;
use crate::settings;
use crate::ui::common;
use crate::ui::comp::Comp;
use crate::ui::grid::{self, Column, Grid, GridEv};
use crate::ui::theme::{self, px};
use crate::ui::vcl::{destroy_form, form, Ev, Form};
use crate::ui::win::*;
use crate::xmlio::num_text;
use std::cell::RefCell;
use std::path::PathBuf;
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub const MR_OK: i32 = 1;

/// Transient data of the open dialogs.
#[derive(Default)]
struct State {
    import: Option<ImportState>,
    history: Vec<(String, PathBuf)>,
    find: Vec<(f64, i32, f64, f64)>,
    labels: LabelOptions,
    labels_count: usize,
    stat_print: [bool; 6],
    font_draw: Option<FontPrefs>,
    font_grid: Option<FontPrefs>,
    logo: Option<String>,
}

thread_local! {
    static ST: RefCell<State> = RefCell::new(State { stat_print: [true; 6], ..Default::default() });
}

fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    ST.with(|s| f(&mut s.borrow_mut()))
}

unsafe fn open(key: &str, comp: Comp) -> &'static mut Form {
    let fi = Form::load_comp(key, comp);
    let f = form(fi);
    f.create(main_hwnd());
    f
}

unsafe fn finish(f: &mut Form) {
    let i = f.index;
    destroy_form(i);
    SetFocus(main_hwnd());
}

unsafe fn focus(f: &mut Form, name: &str) {
    SetFocus(f.hwnd_of(name));
}

/// Real number >= 0 (or > 0 when `positive`) from an edit; shows a message otherwise.
unsafe fn real(f: &mut Form, name: &str, positive: bool) -> Option<f64> {
    let t = f.text(name);
    let v = parse_num(&t);
    let bad = match v {
        None => Some(i18n::t("not_a_number")),
        Some(x) if positive && x <= 0.0 => Some(i18n::t("must_be_positive")),
        Some(x) if x < 0.0 => Some(i18n::t("must_not_be_negative")),
        Some(x) if x > 1.0e9 => Some(i18n::t("number_too_big")),
        _ => None,
    };
    if let Some(m) = bad {
        focus(f, name);
        msg(&m, MB_ICONERROR);
        return None;
    }
    v
}

unsafe fn integer(f: &mut Form, name: &str) -> Option<i32> {
    match f.text(name).trim().parse::<i32>() {
        Ok(v) if v >= 0 => Some(v),
        _ => {
            focus(f, name);
            msg(&i18n::t("must_not_be_negative"), MB_ICONERROR);
            None
        }
    }
}

/// Grids of dialogs share the look of the main tables.
fn dialog_grid(g: &mut Grid, editable: bool) {
    let iface = app().iface.clone();
    mw::style_grid(g, &iface, mw::ROW_H);
    g.fixed_cols = 0;
    g.editable = editable;
}

fn col(key: &str, title: &str, w: i32, align: i32) -> Column {
    let mut c = Column::new(key, title, px(w));
    c.readonly = true;
    c.align = align;
    c
}

// ------------------------------------------------------------------ events

pub fn on_event(fi: usize, ctl: usize, ev: &Ev) -> bool {
    unsafe {
        let f = form(fi);
        let key = f.key.clone();
        let name = f.ctls[ctl].name.clone();
        match (key.as_str(), name.as_str(), ev) {
            ("DLG_TECH", "bbOk", Ev::Click) => !technical_valid(f),
            ("DLG_IFACE", "bb_graphic_font" | "bb_grid_font", Ev::Click) => {
                let draw = name == "bb_graphic_font";
                let cur = st(|s| if draw { s.font_draw.clone() } else { s.font_grid.clone() }).unwrap_or_else(|| if draw { app().iface.font_print.clone() } else { app().iface.font_grid.clone() });
                let mut spec = FontSpec::points(&cur.name, cur.size);
                spec.bold = cur.bold;
                spec.italic = cur.italic;
                if common::choose_font(f.hwnd, &mut spec) {
                    let p = FontPrefs { name: spec.name.clone(), size: spec.size_pt(), bold: spec.bold, italic: spec.italic };
                    f.set_text(&name, &describe_font(&p));
                    st(|s| if draw { s.font_draw = Some(p) } else { s.font_grid = Some(p) });
                }
                true
            }
            ("DLG_IFACE", "bBrowse", Ev::Click) => {
                if let Some(p) = common::open_file(f.hwnd, "Images|*.bmp;*.png;*.jpg;*.jpeg|*.*|*.*", None) {
                    let s = p.to_string_lossy().into_owned();
                    f.set_text("lLogoFile", p.file_name().map(|n| n.to_string_lossy().into_owned()).as_deref().unwrap_or(""));
                    st(|x| x.logo = Some(s));
                }
                true
            }
            ("DLG_MATERIALS", _, Ev::Button(i)) => {
                let b = f.toolbar_button(ctl, *i);
                materials_tool(f, &b);
                true
            }
            ("DLG_MATERIALS", "bbOk", Ev::Click) => !materials_valid(f),
            ("DLG_FIND", "bbStartFindOptimalLength", Ev::Click) => {
                find_size_run(f);
                true
            }
            ("DLG_FIND", "bbStopFindOptimalLength", Ev::Click) => {
                FIND_STOP.with(|s| s.set(true));
                true
            }
            ("DLG_TABULAR", _, Ev::Button(i)) => {
                let b = f.toolbar_button(ctl, *i);
                tabular_tool(f, &b);
                true
            }
            ("DLG_LABELS_PARTS" | "DLG_LABELS_STOCK", "bbFont", Ev::Click) => {
                let cur = st(|s| s.labels.font.clone());
                let mut spec = FontSpec::points(&cur.name, cur.size);
                spec.bold = cur.bold;
                if common::choose_font(f.hwnd, &mut spec) {
                    let p = FontPrefs { name: spec.name.clone(), size: spec.size_pt(), bold: spec.bold, italic: spec.italic };
                    f.set_text("bbFont", &describe_font(&p));
                    st(|s| s.labels.font = p);
                    InvalidateRect(f.hwnd_of("pLabelPreview"), std::ptr::null(), 0);
                }
                true
            }
            ("DLG_LABELS_PARTS" | "DLG_LABELS_STOCK", _, Ev::Click | Ev::Change) => {
                InvalidateRect(f.hwnd_of("pLabelPreview"), std::ptr::null(), 0);
                false
            }
            ("DLG_ABOUT", "b_license_agreement", Ev::Click) => {
                msg(&i18n::t("license_text"), MB_ICONINFORMATION);
                true
            }
            ("DLG_ABOUT", "b_updates", Ev::Click) => {
                crate::update::check_ui(f.hwnd);
                true
            }
            ("DLG_ABOUT", "b_libraries", Ev::Click) => {
                msg(&i18n::t("libraries_text"), MB_ICONINFORMATION);
                true
            }
            ("DLG_IMPORT", _, Ev::Menu(m)) => {
                import_pick(f, m);
                true
            }
            ("DLG_IMPORT", "cbHeader", Ev::Click) => {
                let on = f.checked("cbHeader");
                st(|s| {
                    if let Some(i) = s.import.as_mut() {
                        i.header = on;
                    }
                });
                import_refresh(f);
                true
            }
            _ => false,
        }
    }
}

pub fn on_grid(fi: usize, ctl: usize, ev: &GridEv) -> bool {
    unsafe {
        let f = form(fi);
        let key = f.key.clone();
        let name = f.ctls[ctl].name.clone();
        match (key.as_str(), name.as_str(), ev) {
            ("DLG_HISTORY", "sgInventoryHistory", GridEv::Click(r, _)) => {
                history_preview(f, *r);
                true
            }
            ("DLG_FIND", "pgOptimalLength", GridEv::Click(..)) => true,
            ("DLG_IMPORT", "pgData", GridEv::HeaderClick(c)) => {
                // column 0 is the row tick; the others pick a column of the file
                if *c > 0 {
                    import_header_menu(f, *c - 1);
                }
                true
            }
            ("DLG_IMPORT", "pgData", GridEv::CheckChanged(_)) => {
                let h = f.ctls[ctl].hwnd;
                let ticks = grid::with(h, |g| g.checked.clone()).unwrap_or_default();
                st(|s| {
                    if let Some(i) = s.import.as_mut() {
                        for (k, row) in i.body().into_iter().enumerate() {
                            if let (Some(u), Some(&t)) = (i.use_row.get_mut(row), ticks.get(k)) {
                                *u = t;
                            }
                        }
                    }
                });
                true
            }
            ("DLG_MATERIALS", "pg_data", GridEv::CellChanged(r, c, old)) => {
                let h = f.ctls[ctl].hwnd;
                let (key, text) = grid::with(h, |g| (g.cols[*c].key.clone(), g.cell(*r, *c).to_string())).unwrap_or_default();
                if (key == "mass" || key == "area") && parse_num(&text).map(|v| v < 0.0).unwrap_or(true) {
                    grid::with(h, |g| g.set_cell(*r, *c, old));
                    msg(&i18n::t("must_not_be_negative"), MB_ICONERROR);
                }
                true
            }
            ("DLG_MATERIALS", "pg_data", GridEv::DblClick(r, c)) => {
                let h = f.ctls[ctl].hwnd;
                let is_img = grid::with(h, |g| g.cols[*c].key == "image").unwrap_or(false);
                if is_img {
                    if let Some(p) = common::open_file(f.hwnd, "Images|*.bmp;*.png;*.jpg;*.jpeg|*.*|*.*", None) {
                        grid::with(h, |g| {
                            g.set_cell(*r, *c, &p.to_string_lossy());
                            g.invalidate();
                        });
                    }
                    return true;
                }
                false
            }
            _ => false,
        }
    }
}

/// Custom surfaces of dialogs (About header, sticker preview).
pub fn paint(fi: usize, ctl: usize, dc: HDC, r: &RECT) -> bool {
    unsafe {
        let f = form(fi);
        match (f.key.as_str(), f.ctls[ctl].name.as_str()) {
            ("DLG_ABOUT", "pAboutHead") => {
                fill(dc, r, theme::SURFACE);
                let s = px(64);
                theme::logo(dc, 0, (r.bottom - s) / 2 - px(6), s);
                let x = s + px(18);
                theme::wordmark(dc, &RECT { left: x, top: px(8), right: r.right, bottom: px(44) }, 22, false);
                let line = format!("{} {}", i18n::t("version"), app::VERSION);
                theme::text(dc, &line, &RECT { left: x, top: px(44), right: r.right, bottom: px(66) }, &theme::font(theme::BODY), theme::TEXT_MUTED, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                theme::text(dc, &i18n::t("tagline"), &RECT { left: x, top: px(66), right: r.right, bottom: px(88) }, &theme::font(theme::SMALL), theme::TEXT_SUBTLE, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                true
            }
            ("DLG_LABELS_PARTS" | "DLG_LABELS_STOCK", "pLabelPreview") => {
                fill(dc, r, theme::SURFACE);
                paint_label_preview(f, dc, r);
                true
            }
            _ => false,
        }
    }
}

// ------------------------------------------------------------------ settings

pub unsafe fn technical_1d() {
    let f = open("DLG_TECH", forms::technical());
    let l = app().technical.linear.clone();
    f.set_text("eCutterThickness1D", &num_text(l.cutter_thickness));
    f.set_text("eLatimeMinimaUtilizabila1D", &num_text(l.minimal_useful_size));
    f.set_text("eMaxPiecesPerBar", &l.max_pieces_per_bar.to_string());
    f.set_text("eWasteGreaterThan", &num_text(l.waste_ge));
    f.set_text("eWasteSmallerThan", &num_text(l.waste_le));
    f.set_text("e_space_between_pieces", &num_text(l.space_between_cuts));
    f.set_text("ePrecision", &num_text(l.precision));
    f.set_radio_index("rgMinimize", l.optimization_type.clamp(0, 1));
    f.set_track_pos("tbOptimizare", l.optimization_level.clamp(1, 150));
    if f.show_modal(main_hwnd()) == MR_OK {
        let a = app();
        let l = &mut a.technical.linear;
        l.cutter_thickness = parse_num(&f.text("eCutterThickness1D")).unwrap_or(0.0);
        l.minimal_useful_size = parse_num(&f.text("eLatimeMinimaUtilizabila1D")).unwrap_or(0.0);
        l.max_pieces_per_bar = f.text("eMaxPiecesPerBar").trim().parse().unwrap_or(0);
        l.waste_ge = parse_num(&f.text("eWasteGreaterThan")).unwrap_or(0.0);
        l.waste_le = parse_num(&f.text("eWasteSmallerThan")).unwrap_or(0.0);
        l.space_between_cuts = parse_num(&f.text("e_space_between_pieces")).unwrap_or(0.0);
        l.precision = parse_num(&f.text("ePrecision")).unwrap_or(0.001);
        l.optimization_type = f.radio_index("rgMinimize").max(0);
        l.optimization_level = f.track_pos("tbOptimizare");
        settings::save_technical(&a.technical);
        app::set_status(&i18n::t("status_saved_settings"));
    }
    finish(f);
}

unsafe fn technical_valid(f: &mut Form) -> bool {
    real(f, "eCutterThickness1D", false).is_some()
        && real(f, "e_space_between_pieces", false).is_some()
        && real(f, "eLatimeMinimaUtilizabila1D", false).is_some()
        && real(f, "eWasteGreaterThan", false).is_some()
        && real(f, "eWasteSmallerThan", false).is_some()
        && real(f, "ePrecision", true).is_some()
        && integer(f, "eMaxPiecesPerBar").is_some()
}

pub unsafe fn technical_general() {
    let f = open("DLG_GENERAL", forms::general());
    let t = app().technical.clone();
    let items: Vec<String> = ["1/2", "1/4", "1/8", "1/16", "1/32"].iter().map(|s| s.to_string()).collect();
    f.set_combo_items("cbRoundFractionsTo", &items, t.round_fraction_index.clamp(0, 4));
    f.set_radio_index("rgInventoryPrice", t.price_per_what.clamp(0, 1));
    if f.show_modal(main_hwnd()) == MR_OK {
        let a = app();
        a.technical.round_fraction_index = f.combo_index("cbRoundFractionsTo").max(0);
        a.technical.price_per_what = f.radio_index("rgInventoryPrice").max(0);
        settings::save_technical(&a.technical);
    }
    finish(f);
}

fn describe_font(p: &FontPrefs) -> String {
    format!("{}, {} pt", p.name, p.size)
}

pub unsafe fn interface_settings() {
    let f = open("DLG_IFACE", forms::interface());
    let i = app().iface.clone();
    st(|s| {
        s.font_draw = None;
        s.font_grid = None;
        s.logo = None;
    });
    f.set_text("bb_graphic_font", &describe_font(&i.font_print));
    f.set_text("bb_grid_font", &describe_font(&i.font_grid));
    f.set_text("se_digits_sizes", &i.num_digits.to_string());
    f.set_text("se_digits_percent", &i.num_digits_percent.to_string());
    f.set_text("se_digits_price", &i.num_digits_price.to_string());
    f.set_checked("cbGroupPartsAfterLoadingFromFile", i.group_parts_after_loading);
    for (n, v) in [("eCompanyName", &i.company.name), ("eAddress1", &i.company.address), ("eAddress2", &i.company.address2), ("eCity", &i.company.city), ("eZip", &i.company.zip), ("eEmail", &i.company.email), ("ePhone", &i.company.phone)] {
        f.set_text(n, v);
    }
    let logo_name = std::path::Path::new(&i.company.logo).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    f.set_text("lLogoFile", &logo_name);
    let pc = f.id("pc_settings");
    f.show_page(pc, 0);
    if f.show_modal(main_hwnd()) == MR_OK {
        let a = app();
        if let Some(p) = st(|s| s.font_draw.take()) {
            a.iface.font_print = p;
        }
        if let Some(p) = st(|s| s.font_grid.take()) {
            a.iface.font_grid = p;
        }
        let dg = |n: &str, d: i32| f.text(n).trim().parse::<i32>().unwrap_or(d).clamp(0, 10);
        a.iface.num_digits = dg("se_digits_sizes", 3);
        a.iface.num_digits_percent = dg("se_digits_percent", 2);
        a.iface.num_digits_price = dg("se_digits_price", 3);
        a.iface.group_parts_after_loading = f.checked("cbGroupPartsAfterLoadingFromFile");
        a.iface.company.name = f.text("eCompanyName");
        a.iface.company.address = f.text("eAddress1");
        a.iface.company.address2 = f.text("eAddress2");
        a.iface.company.city = f.text("eCity");
        a.iface.company.zip = f.text("eZip");
        a.iface.company.email = f.text("eEmail");
        a.iface.company.phone = f.text("ePhone");
        if let Some(l) = st(|s| s.logo.take()) {
            a.iface.company.logo = l;
        }
        app::apply_interface(main_form());
    }
    finish(f);
}

pub unsafe fn language() {
    let f = open("DLG_LANG", forms::language());
    f.set_radio_index("rgLanguage", if i18n::lang() == Lang::Th { 0 } else { 1 });
    if f.show_modal(main_hwnd()) == MR_OK {
        let l = if f.radio_index("rgLanguage") == 1 { Lang::En } else { Lang::Th };
        finish(f);
        if l != i18n::lang() {
            i18n::set_lang(l);
            app::apply_language(main_form());
        }
        return;
    }
    finish(f);
}

// ------------------------------------------------------------------ materials

fn material_columns() -> Vec<Column> {
    let mk = |key: &str, title: String, w: i32| {
        let mut c = Column::new(key, &title, px(w));
        c.toggle = key.starts_with('o') && key.len() == 2;
        c.readonly = key == "index";
        c.align = if matches!(key, "index" | "mass" | "area") { 2 } else { 0 };
        c
    };
    vec![
        mk("index", "#".into(), 56),
        mk("name", i18n::t("col_name"), 200),
        mk("ox", i18n::t("col_rot_x"), 100),
        mk("oy", i18n::t("col_rot_y"), 100),
        mk("oz", i18n::t("col_rot_z"), 100),
        mk("mass", i18n::t("col_mass_len"), 110),
        mk("area", i18n::t("col_area_len"), 110),
        mk("image", i18n::t("col_image"), 140),
    ]
}

fn material_row(i: usize, m: &Material) -> Vec<String> {
    let b = |v: bool| if v { "1" } else { "0" }.to_string();
    vec![i.to_string(), m.name.clone(), b(m.ox), b(m.oy), b(m.oz), num_text(m.mass), num_text(m.area), if m.label.is_empty() { "…".into() } else { m.label.clone() }]
}

pub unsafe fn materials() {
    let f = open("DLG_MATERIALS", forms::materials());
    let h = f.hwnd_of("pg_data");
    let mats = app().materials.clone();
    grid::with(h, |g| {
        dialog_grid(g, true);
        g.fixed_cols = 1;
        g.cols = material_columns();
        let rows: Vec<Vec<String>> = mats.iter().enumerate().map(|(i, m)| material_row(i + 1, m)).collect();
        let n = rows.len();
        g.set_rows(rows, vec![true; n]);
        g.empty_text = i18n::t("empty_materials");
    });
    if f.show_modal(main_hwnd()) == MR_OK {
        let list = grid::with(h, |g| {
            (1..=g.rows.len())
                .filter(|&r| !g.cell(r, 1).trim().is_empty())
                .map(|r| Material {
                    name: g.cell(r, 1).trim().to_string(),
                    label: if g.cell(r, 7) == "…" { String::new() } else { g.cell(r, 7).to_string() },
                    color: 16777215,
                    ox: g.cell(r, 2) == "1",
                    oy: g.cell(r, 3) == "1",
                    oz: g.cell(r, 4) == "1",
                    mass: parse_num(g.cell(r, 5)).unwrap_or(0.0),
                    area: parse_num(g.cell(r, 6)).unwrap_or(0.0),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
        app().materials = list;
        settings::save_materials(&app().materials);
    }
    finish(f);
}

unsafe fn materials_tool(f: &mut Form, b: &str) {
    let h = f.hwnd_of("pg_data");
    match b {
        "tb_append_row" => {
            grid::with(h, |g| {
                g.rows.push(material_row(g.rows.len() + 1, &Material::named("")));
                g.checked.push(true);
                g.row_heights.push(g.row_h);
                g.update_scroll();
                let n = g.rows.len();
                g.move_to(n, 1);
            });
            SetFocus(h);
        }
        "tb_delete" => {
            let has = grid::with(h, |g| g.row > 0 && g.row <= g.rows.len()).unwrap_or(false);
            if has && ask(&i18n::t("delete_material")) {
                grid::with(h, |g| {
                    mw::delete_row(g);
                    mw::renumber(g);
                });
            }
        }
        "tb_print" => export::print_grid(h, &i18n::t("materials")),
        "tb_export" => export::grid_export(f.hwnd, h, "materials"),
        _ => {}
    }
}

unsafe fn materials_valid(f: &mut Form) -> bool {
    let h = f.hwnd_of("pg_data");
    grid::with(h, |g| g.commit_edit(true));
    let names: Vec<String> = grid::with(h, |g| (1..=g.rows.len()).map(|r| g.cell(r, 1).trim().to_string()).collect()).unwrap_or_default();
    for (i, n) in names.iter().enumerate() {
        if n.is_empty() {
            grid::with(h, |g| g.move_to(i + 1, 1));
            msg(&i18n::t("material_name_empty"), MB_ICONERROR);
            return false;
        }
        if names[..i].contains(n) {
            grid::with(h, |g| g.move_to(i + 1, 1));
            msg(&i18n::t("material_duplicate"), MB_ICONERROR);
            return false;
        }
    }
    true
}

// ------------------------------------------------------------------ project

pub unsafe fn project_about() {
    let f = open("DLG_PROJECT", forms::project_about());
    let name = app().parts_file.as_ref().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let created = app().creation_time.clone();
    f.set_text("l_project_name", &if name.is_empty() { i18n::tf("project_created", &[&created]) } else { format!("{name} · {}", i18n::tf("project_created", &[&created])) });
    f.set_text("m_comment", &app().comments.replace('\n', "\r\n"));
    if f.show_modal(main_hwnd()) == MR_OK {
        app().comments = f.text("m_comment").replace("\r\n", "\n");
    }
    finish(f);
}

/// Options of a project import/export: which sections, and the comment.
pub unsafe fn project_options(export_all: bool, has_results: bool, comment: &str) -> Option<(crate::xmlio::Sections, String)> {
    let f = open("DLG_OPTIONS", forms::project_options());
    for n in ["cb_parts", "cb_stock", "cb_settings", "cb_materials"] {
        f.set_checked(n, true);
    }
    f.set_checked("cb_results", has_results);
    f.set_enabled("cb_results", has_results);
    f.set_text("m_comment", &comment.replace('\n', "\r\n"));
    if export_all {
        for n in ["cb_parts", "cb_stock", "cb_settings", "cb_materials", "cb_results"] {
            f.set_enabled(n, false);
        }
    }
    let r = f.show_modal(main_hwnd());
    let out = if r == MR_OK {
        Some((
            crate::xmlio::Sections { parts: f.checked("cb_parts"), stock: f.checked("cb_stock"), settings: f.checked("cb_settings"), results: f.checked("cb_results") && has_results, materials: f.checked("cb_materials"), comments: true },
            f.text("m_comment").replace("\r\n", "\n"),
        ))
    } else {
        None
    };
    finish(f);
    out
}

// ------------------------------------------------------------------ about

pub unsafe fn about() {
    let f = open("DLG_ABOUT", forms::about());
    f.show_modal(main_hwnd());
    finish(f);
}

// ------------------------------------------------------------------ stock history

pub unsafe fn history() {
    let f = open("DLG_HISTORY", forms::history());
    let list = settings::list_history();
    st(|s| s.history = list.clone());
    let hl = f.hwnd_of("sgInventoryHistory");
    grid::with(hl, |g| {
        dialog_grid(g, false);
        g.cols = vec![col("date", &i18n::t("col_saved_at"), 290, 0)];
        let rows: Vec<Vec<String>> = list.iter().map(|(l, _)| vec![l.clone()]).collect();
        let n = rows.len();
        g.set_rows(rows, vec![true; n]);
        g.empty_text = i18n::t("empty_history");
    });
    let iface = app().iface.clone();
    let hp = f.hwnd_of("pgPreviewHistory");
    grid::with(hp, |g| {
        dialog_grid(g, false);
        g.cols = mw::make_columns(&iface.stock_cols, false);
        for c in g.cols.iter_mut() {
            c.visible = c.key != "a1" && c.key != "a2";
        }
    });
    if !list.is_empty() {
        history_preview(f, 1);
    }
    f.set_enabled("bbRestore", !list.is_empty());
    let r = f.show_modal(main_hwnd());
    if r == 100 {
        let row = grid::with(hl, |g| g.row).unwrap_or(0);
        if let Some((_, p)) = list.get(row.wrapping_sub(1)) {
            if let Ok(stock) = settings::read_file(p).and_then(|t| crate::xmlio::read_stock_doc(&t)) {
                let (d, dp) = digits();
                mw::fill_stock(main_form().hwnd_of("sgStock"), &stock, d, dp);
                app::data_changed(main_form());
            }
        }
    }
    finish(f);
}

unsafe fn history_preview(f: &mut Form, r: usize) {
    let p = st(|s| s.history.get(r.wrapping_sub(1)).map(|x| x.1.clone()));
    let stock = p.and_then(|p| settings::read_file(&p).ok()).and_then(|t| crate::xmlio::read_stock_doc(&t).ok()).unwrap_or_default();
    let (d, dp) = digits();
    mw::fill_stock(f.hwnd_of("pgPreviewHistory"), &stock, d, dp);
}

// ------------------------------------------------------------------ import data

#[derive(Default, Clone)]
struct ImportState {
    /// every row of the file (the first may be the header)
    table: Vec<Vec<String>>,
    /// target field -> source column
    map: Vec<(String, Option<usize>)>,
    header: bool,
    /// rows of the file to import (by index in `table`)
    use_row: Vec<bool>,
}

impl ImportState {
    /// Rows of the file that hold data (all of them, or all but the header).
    fn body(&self) -> Vec<usize> {
        (if self.header { 1 } else { 0 }..self.table.len()).collect()
    }
}

fn import_fields(stock: bool) -> Vec<&'static str> {
    if stock {
        vec!["length", "quantity", "material", "price", "weight"]
    } else {
        vec!["length", "quantity", "material", "customer"]
    }
}

pub unsafe fn import_data(stock: bool, append: bool) {
    let owner = main_hwnd();
    let filter = "Excel / CSV|*.xlsx;*.xlsm;*.xls;*.ods;*.csv;*.txt|*.*|*.*";
    let Some(path) = common::open_file(owner, filter, Some(&settings::dir(if stock { "" } else { "parts" }))) else { return };
    let table = match export::read_table(&path) {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => {
            msg(&i18n::t("data_empty"), MB_ICONERROR);
            return;
        }
        Err(e) => {
            msg(&format!("{}\n{e}", i18n::t("cannot_open")), MB_ICONERROR);
            return;
        }
    };
    let fields = import_fields(stock);
    let (header, map) = export::guess_columns(&table, &fields);
    let n = table.len();
    st(|s| s.import = Some(ImportState { table, map: fields.iter().map(|k| (k.to_string(), map.get(*k).copied().flatten())).collect(), header, use_row: vec![true; n] }));
    let f = open("DLG_IMPORT", forms::import_data());
    f.set_checked("cbHeader", header);
    grid::with(f.hwnd_of("pgData"), |g| {
        dialog_grid(g, false);
        g.header_check = true;
        g.fixed_cols = 1;
    });
    import_refresh(f);
    if f.show_modal(main_hwnd()) == MR_OK {
        let rows = st(|s| s.import.clone()).map(|s| import_rows(&s)).unwrap_or_default();
        let mf = main_form();
        let (d, dp) = digits();
        if stock {
            let mut list: Vec<Stock> = if append { grid::with(mf.hwnd_of("sgStock"), |g| mw::read_stock_lenient(g)).unwrap_or_default() } else { vec![] };
            list.extend(rows.into_iter().filter_map(|r| row_to_stock(&r)));
            mw::fill_stock(mf.hwnd_of("sgStock"), &list, d, dp);
        } else {
            let mut list: Vec<Part> = if append { grid::with(mf.hwnd_of("sgParts"), |g| mw::read_parts_lenient(g)).unwrap_or_default() } else { vec![] };
            list.extend(rows.into_iter().filter_map(|r| row_to_part(&r)));
            if app().iface.group_parts_after_loading {
                list = mw::group_parts(&list);
            }
            mw::fill_parts(mf.hwnd_of("sgParts"), &list, d);
            app().parts_dirty = true;
        }
        app::data_changed(mf);
    }
    st(|s| s.import = None);
    finish(f);
}

/// QA: import a parts file without the dialog (header and columns guessed).
pub unsafe fn qa_import(path: &str) {
    let Ok(table) = export::read_table(std::path::Path::new(path)) else { return };
    let fields = import_fields(false);
    let (header, map) = export::guess_columns(&table, &fields);
    let n = table.len();
    let s = ImportState { table, map: fields.iter().map(|k| (k.to_string(), map.get(*k).copied().flatten())).collect(), header, use_row: vec![true; n] };
    let list: Vec<Part> = import_rows(&s).into_iter().filter_map(|r| row_to_part(&r)).collect();
    let mf = main_form();
    mw::fill_parts(mf.hwnd_of("sgParts"), &list, digits().0);
    app::data_changed(mf);
}

/// Field -> value maps of the ticked rows.
fn import_rows(s: &ImportState) -> Vec<Vec<(String, String)>> {
    s.body()
        .into_iter()
        .filter(|&i| s.use_row.get(i).copied().unwrap_or(true))
        .map(|i| &s.table[i])
        .filter(|r| r.iter().any(|c| !c.trim().is_empty()))
        .map(|r| s.map.iter().map(|(k, c)| (k.clone(), c.and_then(|c| r.get(c)).cloned().unwrap_or_default())).collect())
        .collect()
}

fn field<'a>(r: &'a [(String, String)], k: &str) -> &'a str {
    r.iter().find(|x| x.0 == k).map(|x| x.1.trim()).unwrap_or("")
}

fn row_to_part(r: &[(String, String)]) -> Option<Part> {
    let length = parse_num(field(r, "length"))?;
    if length <= 0.0 {
        return None;
    }
    let num = |k: &str, d: f64| parse_num(field(r, k)).filter(|_| !field(r, k).is_empty()).unwrap_or(d);
    Some(Part {
        length,
        height: num("width", 0.0),
        quantity: field(r, "quantity").parse::<f64>().map(|v| v as i32).unwrap_or(1).max(1),
        label: field(r, "label").into(),
        material: field(r, "material").into(),
        customer: field(r, "customer").into(),
        a1: num("a1", 90.0),
        a2: num("a2", 90.0),
        ..Part::default()
    })
}

fn row_to_stock(r: &[(String, String)]) -> Option<Stock> {
    let length = parse_num(field(r, "length"))?;
    if length <= 0.0 {
        return None;
    }
    let num = |k: &str, d: f64| parse_num(field(r, k)).filter(|_| !field(r, k).is_empty()).unwrap_or(d);
    Some(Stock {
        length,
        height: num("width", 0.0),
        quantity: field(r, "quantity").parse::<f64>().map(|v| v as i32).unwrap_or(1).max(1),
        label: field(r, "label").into(),
        material: field(r, "material").into(),
        price: num("price", 0.0),
        weight: num("weight", 0.0),
        priority: field(r, "priority").parse::<f64>().map(|v| v as i32).unwrap_or(0),
        ..Stock::default()
    })
}

unsafe fn import_refresh(f: &mut Form) {
    let Some(s) = st(|s| s.import.clone()) else { return };
    let h = f.hwnd_of("pgData");
    grid::with(h, |g| {
        // a tick per row of the file, then the fields
        let mut first = col("index", "#", 64, 2);
        first.check = true;
        g.cols = std::iter::once(first)
            .chain(s.map.iter().map(|(k, c)| {
                let src = match c {
                    Some(c) => format!("← {}", c + 1),
                    None => "← –".into(),
                };
                col(k, &format!("{}  {src}", mw::col_title(k)), 130, 0)
            }))
            .collect();
        let body = s.body();
        let rows: Vec<Vec<String>> = body
            .iter()
            .map(|&i| std::iter::once((i + 1).to_string()).chain(s.map.iter().map(|(_, c)| c.and_then(|c| s.table[i].get(c)).cloned().unwrap_or_default())).collect())
            .collect();
        let ticks = body.iter().map(|&i| s.use_row.get(i).copied().unwrap_or(true)).collect();
        g.set_rows(rows, ticks);
    });
}

unsafe fn import_header_menu(f: &mut Form, c: usize) {
    let Some(s) = st(|s| s.import.clone()) else { return };
    let ncols = s.table.iter().map(|r| r.len()).max().unwrap_or(0);
    let m = CreatePopupMenu();
    let none = wide(&i18n::t("import_skip"));
    AppendMenuW(m, MF_STRING, 41000, none.as_ptr());
    f.menus.insert(41000, format!("imp:{c}:-"));
    for k in 0..ncols.min(60) {
        let sample = s.table.iter().find_map(|r| r.get(k).filter(|v| !v.trim().is_empty()).cloned()).unwrap_or_default();
        let label = wide(&format!("{} {}  ({})", i18n::t("import_column"), k + 1, sample.chars().take(30).collect::<String>()));
        let cmd = 41001 + k as u16;
        f.menus.insert(cmd, format!("imp:{c}:{k}"));
        AppendMenuW(m, MF_STRING | if s.map.get(c).and_then(|x| x.1) == Some(k) { MF_CHECKED } else { 0 }, cmd as usize, label.as_ptr());
    }
    crate::ui::menu::style_popup(m);
    let mut pt = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    TrackPopupMenu(m, TPM_LEFTALIGN | TPM_TOPALIGN, pt.x, pt.y, 0, f.hwnd, std::ptr::null());
    DestroyMenu(m);
}

unsafe fn import_pick(f: &mut Form, m: &str) {
    let parts: Vec<&str> = m.split(':').collect();
    if parts.len() != 3 || parts[0] != "imp" {
        return;
    }
    let c: usize = parts[1].parse().unwrap_or(0);
    let src: Option<usize> = parts[2].parse().ok();
    st(|s| {
        if let Some(i) = s.import.as_mut() {
            if let Some(x) = i.map.get_mut(c) {
                x.1 = src;
            }
        }
    });
    import_refresh(f);
}

// ------------------------------------------------------------------ find the best stock length

thread_local! {
    static FIND_STOP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub unsafe fn find_size() {
    let f = open("DLG_FIND", forms::find_size());
    let parts = grid::with(main_form().hwnd_of("sgParts"), |g| mw::read_parts_lenient(g)).unwrap_or_default();
    let mut mats: Vec<String> = vec![];
    let mut heights: Vec<String> = vec![];
    for p in parts.iter().filter(|p| p.use_it) {
        if !mats.contains(&p.material) {
            mats.push(p.material.clone());
        }
        let h = fmt_num(p.height, digits().0);
        if !heights.contains(&h) {
            heights.push(h);
        }
    }
    f.set_combo_items("cbMaterialName", &mats, if mats.is_empty() { -1 } else { 0 });
    f.set_combo_items("cbWidth", &heights, if heights.is_empty() { -1 } else { 0 });
    let longest = parts.iter().map(|p| p.length).fold(0.0, f64::max);
    f.set_text("eMinLengthRange", &num_text(longest));
    f.set_text("eMaxLengthRange", &num_text((longest * 4.0).max(longest)));
    f.set_text("eStepLengthRange", &num_text((longest / 10.0).round().max(1.0)));
    let h = f.hwnd_of("pgOptimalLength");
    grid::with(h, |g| {
        dialog_grid(g, false);
        g.cols = vec![
            col("length", &mw::col_title("length"), 130, 2),
            col("quantity", &i18n::t("col_bars_needed"), 130, 2),
            col("util", &i18n::t("col_utilization"), 150, 2),
            col("waste", &i18n::t("col_waste"), 130, 2),
            col("wastep", &format!("{} %", i18n::t("col_waste")), 110, 2),
        ];
        g.empty_text = i18n::t("empty_find");
    });
    st(|s| s.find.clear());
    if f.show_modal(main_hwnd()) == MR_OK {
        let row = grid::with(h, |g| g.row).unwrap_or(0);
        if let Some(&(len, n, _, _)) = st(|s| s.find.get(row.wrapping_sub(1)).copied()).as_ref() {
            let mf = main_form();
            let mut stock = grid::with(mf.hwnd_of("sgStock"), |g| mw::read_stock_lenient(g)).unwrap_or_default();
            let material = f.text("cbMaterialName");
            let height = parse_num(&f.text("cbWidth")).unwrap_or(0.0);
            stock.push(Stock { length: len, height, quantity: n, material, ..Stock::default() });
            let (d, dp) = digits();
            mw::fill_stock(mf.hwnd_of("sgStock"), &stock, d, dp);
            app::data_changed(mf);
        }
    }
    finish(f);
}

unsafe fn find_size_run(f: &mut Form) {
    let (Some(lo), Some(hi), Some(step)) = (real(f, "eMinLengthRange", true), real(f, "eMaxLengthRange", true), real(f, "eStepLengthRange", true)) else { return };
    if hi < lo {
        focus(f, "eMaxLengthRange");
        msg(&i18n::t("range_invalid"), MB_ICONERROR);
        return;
    }
    let material = f.text("cbMaterialName");
    let height = parse_num(&f.text("cbWidth")).unwrap_or(0.0);
    let parts: Vec<Part> = grid::with(main_form().hwnd_of("sgParts"), |g| mw::read_parts_lenient(g)).unwrap_or_default().into_iter().filter(|p| p.use_it && p.material == material && (p.height - height).abs() < 1e-9).collect();
    if parts.is_empty() {
        msg(&i18n::t("need_piece"), MB_ICONERROR);
        return;
    }
    let need: f64 = parts.iter().map(|p| p.length * p.quantity as f64).sum();
    let steps = (((hi - lo) / step).floor() as usize + 1).min(2000);
    let bar = f.hwnd_of("pbOptimalLength");
    SendMessageW(bar, 0x0406, 0, steps as isize);
    FIND_STOP.with(|s| s.set(false));
    let mut out = vec![];
    let mut technical = app().technical.clone();
    technical.linear.optimization_level = technical.linear.optimization_level.min(30);
    let longest = parts.iter().map(|p| p.length).fold(0.0, f64::max);
    for k in 0..steps {
        let len = lo + step * k as f64;
        if len + 1e-9 < longest {
            continue;
        }
        let prj = Project { parts: parts.clone(), stock: vec![Stock { length: len, height, quantity: 1_000_000, material: material.clone(), ..Stock::default() }], materials: app().materials.clone(), technical: technical.clone(), ..Default::default() };
        let sol = crate::engine::optimize(&prj, &crate::engine::Progress { cancel: &|| false, report: &|_, _| {} });
        let n: i32 = sol.bars.iter().map(|b| b.quantity).sum();
        let total = len * n as f64;
        out.push((len, n, if total > 0.0 { need / total * 100.0 } else { 0.0 }, total - need));
        SendMessageW(bar, 0x0402, k + 1, 0);
        crate::ui::shot::pump(0);
        if FIND_STOP.with(|s| s.get()) {
            break;
        }
    }
    out.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    let (d, dp) = (digits().0, app().iface.num_digits_percent);
    let rows: Vec<Vec<String>> = out.iter().map(|(l, n, u, w)| vec![fmt_num(*l, d), n.to_string(), format!("{} %", fmt_num(*u, dp)), fmt_num(*w, d), fmt_num(100.0 - u, dp)]).collect();
    st(|s| s.find = out);
    grid::with(f.hwnd_of("pgOptimalLength"), |g| {
        let n = rows.len();
        g.set_rows(rows, vec![true; n]);
        g.row = if n > 0 { 1 } else { 0 };
    });
}

// ------------------------------------------------------------------ tabular view

const TABULAR_MENU: [(&str, &[&str]); 7] = [
    ("mi_show_index_column_results_1d", &["index"]),
    ("mi_show_width_column_results_1d", &["width"]),
    ("mi_show_angles_column_results_1d", &["a1", "a2"]),
    ("mi_show_position_top_column_results_1d", &["lt", "rt"]),
    ("mi_show_position_bottom_column_results_1d", &["lb", "rb"]),
    ("mi_show_label_column_results_1d", &["label"]),
    ("mi_show_customer_column_results_1d", &["customer"]),
];

pub unsafe fn tabular() {
    let mf = main_form();
    let row = grid::with(mf.hwnd_of("g_results_1d"), |g| g.row).unwrap_or(1);
    let Some(bi) = results::bar_of_row(row.max(1)) else { return };
    let f = open("DLG_TABULAR", forms::tabular());
    let d = app().iface.num_digits;
    let rows: Vec<Vec<String>> = results::view(|v| {
        v.sol.bars.get(bi).map(|b| {
            b.pieces
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    vec![
                        (i + 1).to_string(),
                        fmt_num(p.length, d),
                        fmt_num(b.height, d),
                        fmt_num(p.start_bottom_x, d),
                        fmt_num(p.start_top_x, d),
                        fmt_num(p.end_bottom_x, d),
                        fmt_num(p.end_top_x, d),
                        fmt_num(p.a1, d),
                        fmt_num(p.a2, d),
                        p.label.clone(),
                        p.customer.clone(),
                    ]
                })
                .collect()
        })
    })
    .flatten()
    .unwrap_or_default();
    let title = results::view(|v| v.sol.bars.get(bi).map(|b| i18n::tf("tabular_title", &[&fmt_num(b.length, d), &b.material, &b.quantity.to_string()]))).flatten().unwrap_or_default();
    if !title.is_empty() {
        crate::ui::win::set_text(f.hwnd, &title);
    }
    grid::with(f.hwnd_of("g_pieces_1d"), |g| {
        dialog_grid(g, false);
        g.fixed_cols = 1;
        g.cols = vec![
            col("index", "#", 52, 2),
            col("length", &mw::col_title("length"), 92, 2),
            col("width", &mw::col_title("width"), 80, 2),
            col("lb", &i18n::t("col_start_bottom"), 110, 2),
            col("lt", &i18n::t("col_start_top"), 110, 2),
            col("rb", &i18n::t("col_end_bottom"), 110, 2),
            col("rt", &i18n::t("col_end_top"), 110, 2),
            col("a1", &mw::col_title("a1"), 72, 2),
            col("a2", &mw::col_title("a2"), 72, 2),
            col("label", &mw::col_title("label"), 100, 0),
            col("customer", &mw::col_title("customer"), 120, 0),
        ];
        let n = rows.len();
        g.set_rows(rows, vec![true; n]);
    });
    for (m, _) in TABULAR_MENU {
        f.menu_check(m, true);
    }
    f.show_modal(main_hwnd());
    finish(f);
}

unsafe fn tabular_columns(f: &mut Form) {
    let h = f.hwnd_of("g_pieces_1d");
    for (m, keys) in TABULAR_MENU {
        let v = crate::ui::menu::is_checked(f.index, m);
        grid::with(h, |g| {
            for k in keys.iter() {
                mw::set_col_visible(g, k, v);
            }
        });
    }
    grid::with(h, |g| {
        g.update_scroll();
        g.invalidate();
    });
}

unsafe fn tabular_tool(f: &mut Form, b: &str) {
    let h = f.hwnd_of("g_pieces_1d");
    match b {
        "tb_export" => export::grid_export(f.hwnd, h, "pieces"),
        "tb_print" => export::print_grid(h, &i18n::t("pieces")),
        "columns_tb" => {
            let r = f.toolbar_button_rect("ToolBar1", "columns_tb");
            let m = f.popup_menu("tabular_1d_columns_pm");
            let cmd = TrackPopupMenu(m, TPM_LEFTALIGN | TPM_TOPALIGN | TPM_RETURNCMD, r.left, r.bottom + px(2), 0, f.hwnd, std::ptr::null());
            DestroyMenu(m);
            if cmd != 0 {
                if let Some(name) = f.menus.get(&(cmd as u16)).cloned() {
                    let now = !crate::ui::menu::is_checked(f.index, &name);
                    f.menu_check(&name, now);
                    tabular_columns(f);
                }
            }
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ stickers

#[derive(Clone)]
pub struct LabelOptions {
    pub rows: i32,
    pub cols: i32,
    pub margins: [i32; 4],
    pub inner: i32,
    pub piece_size: bool,
    pub company: bool,
    pub label: bool,
    pub custom: bool,
    pub custom_text: String,
    pub customer: bool,
    pub material: bool,
    pub date: bool,
    pub font: FontPrefs,
}

impl Default for LabelOptions {
    fn default() -> Self {
        LabelOptions {
            rows: 8,
            cols: 3,
            margins: [2; 4],
            inner: 4,
            piece_size: true,
            company: true,
            label: true,
            custom: false,
            custom_text: String::new(),
            customer: true,
            material: true,
            date: true,
            font: FontPrefs { name: theme::FACE.into(), size: 9, bold: false, italic: false },
        }
    }
}

/// One label: lines of text (piece size first, larger).
pub struct LabelItem {
    pub lines: Vec<String>,
}

fn stock_key(stock: bool) -> &'static str {
    if stock {
        "DLG_LABELS_STOCK"
    } else {
        "DLG_LABELS_PARTS"
    }
}

/// Options as currently set in the dialog.
unsafe fn read_label_options(f: &mut Form) -> LabelOptions {
    let stock = f.key == "DLG_LABELS_STOCK";
    let o = st(|s| s.labels.clone());
    let n = |f: &Form, name: &str, d: i32| f.text(name).trim().parse::<i32>().unwrap_or(d);
    LabelOptions {
        rows: n(f, "eRows", 8).clamp(1, 20),
        cols: n(f, "eColumns", 3).clamp(1, 5),
        margins: [n(f, "eLeftStickerMargin", 0).clamp(0, 20), n(f, "eTopStickerMargin", 0).clamp(0, 20), n(f, "eRightStickerMargin", 0).clamp(0, 20), n(f, "eBottomStickerMargin", 0).clamp(0, 20)],
        inner: n(f, "eMarginsOfTheLabel", 4).clamp(0, 10),
        piece_size: f.checked("cbPieceSize"),
        company: f.checked("cbCompanyName"),
        label: f.checked(if stock { "cbInventoryLabel" } else { "cbDemandLabel" }),
        custom: f.checked("cbCustomMessage"),
        custom_text: f.text("eCustomMessage"),
        customer: !stock && f.checked("cbClientName"),
        material: f.checked(if stock { "cbMaterial" } else { "cbMaterialType" }),
        date: !stock && f.checked("cbDate"),
        font: o.font,
    }
}

unsafe fn paint_label_preview(f: &mut Form, dc: HDC, r: &RECT) {
    let o = read_label_options(f);
    let mut lines: Vec<String> = vec![];
    if o.piece_size {
        lines.push("1250".into());
    }
    let company = app().iface.company.name.clone();
    if o.company {
        lines.push(if company.is_empty() { i18n::t("sample_company") } else { company });
    }
    if o.label {
        lines.push(i18n::t("sample_label"));
    }
    if o.customer {
        lines.push(i18n::t("sample_customer"));
    }
    if o.material {
        lines.push(i18n::t("sample_material"));
    }
    if o.custom && !o.custom_text.is_empty() {
        lines.push(o.custom_text.clone());
    }
    if o.date {
        lines.push(settings::today());
    }
    let w = (r.right - r.left).min(px(300));
    let card = RECT { left: r.left, top: r.top, right: r.left + w, bottom: r.bottom };
    theme::round_rect(dc, &card, px(8) as f32, Some(theme::SURFACE), Some(theme::STROKE_STRONG));
    let s = theme::scale();
    let normal = FontSpec::new(&o.font.name, -((o.font.size as f64 * 96.0 / 72.0 * s).round() as i32));
    let mut big = FontSpec::new(&o.font.name, -(((o.font.size + 5) as f64 * 96.0 / 72.0 * s).round() as i32));
    big.bold = true;
    let lh_big = text_height(&big);
    let lh = text_height(&normal);
    let total: i32 = lines.iter().enumerate().map(|(i, _)| if i == 0 && o.piece_size { lh_big } else { lh }).sum();
    let mut y = card.top + ((card.bottom - card.top) - total).max(0) / 2;
    for (i, l) in lines.iter().enumerate() {
        let (fs, h) = if i == 0 && o.piece_size { (&big, lh_big) } else { (&normal, lh) };
        let tr = RECT { left: card.left + px(8), top: y, right: card.right - px(8), bottom: y + h };
        theme::text(dc, l, &tr, fs, theme::TEXT, DT_CENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        y += h;
    }
    let info = RECT { left: card.right + px(16), top: r.top, right: r.right, bottom: r.bottom };
    let per = o.rows * o.cols;
    let count = st(|s| s.labels_count);
    let pages = if per > 0 { (count as i32 + per - 1) / per } else { 0 };
    let text = i18n::tf("labels_summary", &[&count.to_string(), &per.to_string(), &pages.to_string()]);
    theme::text(dc, &text, &info, &theme::font(theme::SMALL), theme::TEXT_MUTED, DT_LEFT | DT_WORDBREAK);
}

unsafe fn labels_dialog(stock: bool, count: usize) -> Option<LabelOptions> {
    st(|s| s.labels_count = count);
    let f = open(stock_key(stock), forms::labels(stock));
    let o = st(|s| s.labels.clone());
    f.set_text("eRows", &o.rows.to_string());
    f.set_text("eColumns", &o.cols.to_string());
    for (n, v) in [("eLeftStickerMargin", o.margins[0]), ("eTopStickerMargin", o.margins[1]), ("eRightStickerMargin", o.margins[2]), ("eBottomStickerMargin", o.margins[3])] {
        f.set_text(n, &v.to_string());
    }
    f.set_text("eMarginsOfTheLabel", &o.inner.to_string());
    f.set_text("bbFont", &describe_font(&o.font));
    f.set_checked("cbPieceSize", o.piece_size);
    f.set_checked("cbCompanyName", o.company);
    f.set_checked(if stock { "cbInventoryLabel" } else { "cbDemandLabel" }, o.label);
    f.set_checked("cbCustomMessage", o.custom);
    f.set_text("eCustomMessage", &o.custom_text);
    f.set_checked(if stock { "cbMaterial" } else { "cbMaterialType" }, o.material);
    if !stock {
        f.set_checked("cbClientName", o.customer);
        f.set_checked("cbDate", o.date);
    }
    f.set_text("lNr1D", "");
    let r = f.show_modal(main_hwnd());
    let out = if r == MR_OK {
        let o = read_label_options(f);
        st(|s| s.labels = o.clone());
        Some(o)
    } else {
        None
    };
    finish(f);
    out
}

fn part_label(o: &LabelOptions, p: &Part, d: i32) -> LabelItem {
    let mut lines = vec![];
    if o.piece_size {
        lines.push(fmt_num(p.length, d));
    }
    if o.company && !app().iface.company.name.is_empty() {
        lines.push(app().iface.company.name.clone());
    }
    if o.label && !p.label.is_empty() {
        lines.push(p.label.clone());
    }
    if o.customer && !p.customer.is_empty() {
        lines.push(p.customer.clone());
    }
    if o.material && !p.material.is_empty() {
        lines.push(p.material.clone());
    }
    if o.custom && !o.custom_text.is_empty() {
        lines.push(o.custom_text.clone());
    }
    if o.date {
        lines.push(settings::today());
    }
    LabelItem { lines }
}

pub unsafe fn labels(stock: bool) {
    let mf = main_form();
    let d = digits().0;
    if stock {
        let list = grid::with(mf.hwnd_of("sgStock"), |g| mw::read_stock_lenient(g)).unwrap_or_default();
        let count: usize = list.iter().filter(|s| s.use_it).map(|s| s.quantity.max(0) as usize).sum();
        let Some(o) = labels_dialog(true, count) else { return };
        let mut items = vec![];
        for s in list.iter().filter(|s| s.use_it) {
            for _ in 0..s.quantity.max(0) {
                let mut lines = vec![];
                if o.piece_size {
                    lines.push(fmt_num(s.length, d));
                }
                if o.company && !app().iface.company.name.is_empty() {
                    lines.push(app().iface.company.name.clone());
                }
                if o.label && !s.label.is_empty() {
                    lines.push(s.label.clone());
                }
                if o.material && !s.material.is_empty() {
                    lines.push(s.material.clone());
                }
                if o.custom && !o.custom_text.is_empty() {
                    lines.push(o.custom_text.clone());
                }
                items.push(LabelItem { lines });
            }
        }
        export::print_labels(&o, &items);
    } else {
        let list = grid::with(mf.hwnd_of("sgParts"), |g| mw::read_parts_lenient(g)).unwrap_or_default();
        let count: usize = list.iter().filter(|p| p.use_it).map(|p| p.quantity.max(0) as usize).sum();
        let Some(o) = labels_dialog(false, count) else { return };
        let mut items = vec![];
        for p in list.iter().filter(|p| p.use_it) {
            for _ in 0..p.quantity.max(0) {
                items.push(part_label(&o, p, d));
            }
        }
        export::print_labels(&o, &items);
    }
}

/// Stickers for every piece of the solution.
pub unsafe fn labels_results() {
    let Some((prj, sol)) = app().sol.clone() else { return };
    let count: usize = sol.bars.iter().map(|b| b.pieces.len() * b.quantity.max(0) as usize).sum();
    let Some(o) = labels_dialog(false, count) else { return };
    let d = digits().0;
    let mut items = vec![];
    for b in &sol.bars {
        for _ in 0..b.quantity.max(0) {
            for p in &b.pieces {
                if let Some(part) = prj.parts.get(p.part) {
                    items.push(part_label(&o, part, d));
                }
            }
        }
    }
    export::print_labels(&o, &items);
}

// ------------------------------------------------------------------ DXF

#[derive(Clone)]
pub struct DxfOptions {
    pub unit: i32,
    pub font_height: f64,
    pub layer_text: String,
    pub layer_bars: String,
    pub layer_parts: String,
    pub layer_waste: String,
    pub bar_contour: bool,
    pub bar_info: bool,
    pub parts_contour: bool,
    pub parts_size: bool,
    pub parts_label: bool,
    pub customer: bool,
    pub angles: bool,
    pub waste_contour: bool,
    pub waste_size: bool,
}

pub unsafe fn dxf_export() {
    if app().sol.is_none() {
        return;
    }
    let f = open("DLG_DXF", forms::dxf());
    let units: Vec<String> = ["Unitless", "Inches", "Feet", "Miles", "Millimeters", "Centimeters", "Meters", "Kilometers"].iter().map(|s| i18n::tr(s)).collect();
    f.set_combo_items("cb_measurement_unit", &units, 4);
    f.set_text("e_font_height", "50");
    for n in ["e_Text", "e_sheet", "e_parts", "e_waste"] {
        f.set_text(n, "0");
    }
    let r = f.show_modal(main_hwnd());
    let o = (r == MR_OK).then(|| DxfOptions {
        unit: f.combo_index("cb_measurement_unit").max(0),
        font_height: parse_num(&f.text("e_font_height")).filter(|v| *v > 0.0).unwrap_or(50.0),
        layer_text: f.text("e_Text"),
        layer_bars: f.text("e_sheet"),
        layer_parts: f.text("e_parts"),
        layer_waste: f.text("e_waste"),
        bar_contour: f.checked("cb_export_bar_contour"),
        bar_info: f.checked("cb_export_info_bar"),
        parts_contour: f.checked("cb_export_parts_contour"),
        parts_size: f.checked("cb_export_parts_size"),
        parts_label: f.checked("cb_export_parts_label"),
        customer: f.checked("cb_export_customer_name"),
        angles: f.checked("cb_export_angles"),
        waste_contour: f.checked("cb_export_waste_contour"),
        waste_size: f.checked("cb_export_waste_size"),
    });
    finish(f);
    if let Some(o) = o {
        export::save_dxf(&o);
    }
}

// ------------------------------------------------------------------ printing

pub unsafe fn print_results() {
    let mf = main_form();
    let on_stats = mf.page_index("pcResults") == 1;
    if on_stats {
        let f = open("DLG_PRINTSTATS", forms::print_stats());
        let cur = st(|s| s.stat_print);
        let names = ["cbUtilizedParts1D", "cbUtilizedStock1D", "cbReusedWaste1D", "cbGarbageWaste1D", "cbMaterialStatistics1D", "cbMiscel1D"];
        for (i, n) in names.iter().enumerate() {
            f.set_checked(n, cur[i]);
        }
        let r = f.show_modal(main_hwnd());
        let sel = names.map(|n| f.checked(n));
        finish(f);
        if r != MR_OK {
            return;
        }
        st(|s| s.stat_print = sel);
        export::print_statistics(&sel);
    } else {
        export::print_results();
    }
}
