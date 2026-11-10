//! Files and output: parts XML, projects, Excel/CSV tables, results image/table, DXF, printing
//! and adhesive labels.

use crate::app::{self, app, digits, main_form, msg};
use crate::dlg::{self, DxfOptions, LabelItem, LabelOptions};
use crate::i18n;
use crate::mainwin as mw;
use crate::model::*;
use crate::results;
use crate::settings;
use crate::ui::common;
use crate::ui::grid;
use crate::ui::vcl::Form;
use crate::ui::win::*;
use crate::xmlio;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Storage::Xps::{EndDoc, EndPage, StartDocW, StartPage, DOCINFOW};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const XML_FILTER: &str = "XML (*.xml)|*.xml";
const TABLE_FILTER: &str = "Excel (*.xlsx)|*.xlsx|CSV (*.csv)|*.csv";

unsafe fn error(e: &str) {
    msg(&format!("{}\n{e}", i18n::t("cannot_save")), MB_ICONERROR);
}

// ------------------------------------------------------------------ parts files

pub unsafe fn parts_open(f: &mut Form, append: bool) {
    let Some(p) = common::open_file(f.hwnd, XML_FILTER, Some(&settings::dir("parts"))) else { return };
    let parts = match settings::read_file(&p).and_then(|t| xmlio::read_parts_doc(&t).or_else(|_| xmlio::read_project(&t).ok().and_then(|l| l.parts).ok_or_else(|| "no parts".to_string()))) {
        Ok(v) => v,
        Err(e) => {
            msg(&format!("{}\n{e}", i18n::t("cannot_open")), MB_ICONERROR);
            return;
        }
    };
    let d = digits().0;
    let mut list = if append { grid::with(f.hwnd_of("sgParts"), |g| mw::read_parts_lenient(g)).unwrap_or_default() } else { vec![] };
    list.extend(parts);
    if app().iface.group_parts_after_loading {
        list = mw::group_parts(&list);
    }
    mw::fill_parts(f.hwnd_of("sgParts"), &list, d);
    if !append {
        app().parts_file = Some(p);
        app().parts_dirty = false;
    } else {
        app().parts_dirty = true;
    }
    app::status(f);
}

pub unsafe fn parts_save(f: &mut Form, save_as: bool) {
    grid::with(f.hwnd_of("sgParts"), |g| g.commit_edit(true));
    let path = match (&app().parts_file, save_as) {
        (Some(p), false) => p.clone(),
        _ => {
            let name = app().parts_file.as_ref().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            match common::save_file(f.hwnd, XML_FILTER, "xml", Some(&settings::dir("parts")), &name) {
                Some(p) => p,
                None => return,
            }
        }
    };
    let parts = grid::with(f.hwnd_of("sgParts"), |g| mw::read_parts_lenient(g)).unwrap_or_default();
    match settings::write_file(&path, &xmlio::parts_doc(&parts)) {
        Ok(()) => {
            app().parts_file = Some(path);
            app().parts_dirty = false;
            app::status(f);
        }
        Err(e) => error(&e),
    }
}

// ------------------------------------------------------------------ projects

pub unsafe fn project_export(f: &mut Form, everything: bool) {
    let has = app().sol.is_some();
    let Some((sec, comment)) = dlg::project_options(everything, has, &app().comments) else { return };
    app().comments = comment;
    let Some(path) = common::save_file(f.hwnd, "xml|*.xml", "xml", Some(&settings::dir("projects")), "") else { return };
    let prj = app::current_project();
    let sol = app().sol.as_ref().map(|s| s.1.clone());
    let doc = xmlio::project_doc(&prj, sol.as_ref(), sec, app::VERSION);
    if let Err(e) = settings::write_file(&path, &doc) {
        error(&e);
    }
}

/// Help > Service > Save everything: the whole state in one project file.
pub unsafe fn service_export(f: &mut Form) {
    let Some(path) = common::save_file(f.hwnd, "xml|*.xml", "xml", Some(&settings::dir("projects")), "") else { return };
    let prj = app::current_project();
    let sol = app().sol.as_ref().map(|s| s.1.clone());
    if let Err(e) = settings::write_file(&path, &xmlio::project_doc(&prj, sol.as_ref(), xmlio::Sections::all(), app::VERSION)) {
        error(&e);
    }
}

pub unsafe fn project_import(f: &mut Form, everything: bool) {
    let Some(path) = common::open_file(f.hwnd, "xml|*.xml", Some(&settings::dir("projects"))) else { return };
    let text = match settings::read_file(&path) {
        Ok(t) => t,
        Err(e) => {
            msg(&format!("{}\n{e}", i18n::t("cannot_open")), MB_ICONERROR);
            return;
        }
    };
    let l = match xmlio::read_project(&text) {
        Ok(l) => l,
        Err(e) => {
            msg(&format!("{}\n{e}", i18n::t("cannot_open")), MB_ICONERROR);
            return;
        }
    };
    let sec = if everything {
        xmlio::Sections::all()
    } else {
        match dlg::project_options(false, l.results.is_some(), l.comments.as_deref().unwrap_or("")) {
            Some((s, c)) => {
                app().comments = c;
                s
            }
            None => return,
        }
    };
    apply_loaded(f, l, sec);
}

unsafe fn apply_loaded(f: &mut Form, l: xmlio::Loaded, sec: xmlio::Sections) {
    let a = app();
    let (d, dp) = digits();
    if sec.parts {
        if let Some(p) = &l.parts {
            mw::fill_parts(f.hwnd_of("sgParts"), p, d);
        }
    }
    if sec.stock {
        if let Some(s) = &l.stock {
            mw::fill_stock(f.hwnd_of("sgStock"), s, d, dp);
        }
    }
    if sec.materials {
        if let Some(m) = &l.materials {
            a.materials = m.clone();
            settings::save_materials(&a.materials);
        }
    }
    if sec.settings {
        if let Some(t) = &l.technical {
            a.technical = t.clone();
            settings::save_technical(&a.technical);
        }
    }
    if sec.comments {
        if let Some(c) = &l.comments {
            a.comments = c.clone();
        }
    }
    if !l.creation_time.is_empty() {
        a.creation_time = l.creation_time.clone();
    }
    if sec.parts {
        set_text(f.hwnd_of("edJobName"), &l.name);
    }
    if sec.results {
        if let Some(r) = &l.results {
            let prj = Project { parts: l.parts.clone().unwrap_or_default(), stock: l.stock.clone().unwrap_or_default(), materials: a.materials.clone(), technical: l.technical.clone().unwrap_or_else(|| a.technical.clone()), ..Default::default() };
            let sol = xmlio::read_results(r, &prj);
            if !sol.bars.is_empty() {
                app::show_solution(f, prj, sol, false);
            }
        }
    }
}

/// A run from "Latest solutions".
pub unsafe fn open_run(f: &mut Form, p: &Path) {
    let Ok(text) = settings::read_file(p) else { return };
    let Ok(l) = xmlio::read_project(&text) else { return };
    let prj = Project { parts: l.parts.clone().unwrap_or_default(), stock: l.stock.clone().unwrap_or_default(), materials: l.materials.clone().unwrap_or_else(|| app().materials.clone()), technical: l.technical.clone().unwrap_or_default(), ..Default::default() };
    if let Some(r) = &l.results {
        let sol = xmlio::read_results(r, &prj);
        app::show_solution(f, prj, sol, false);
    }
}

// ------------------------------------------------------------------ tables (Excel / CSV)

/// Rows of a spreadsheet or delimited text file.
pub fn read_table(path: &Path) -> Result<Vec<Vec<String>>, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if matches!(ext.as_str(), "xlsx" | "xlsm" | "xls" | "xlsb" | "ods") {
        use calamine::{open_workbook_auto, Data, Reader};
        let mut wb = open_workbook_auto(path).map_err(|e| e.to_string())?;
        let name = wb.sheet_names().first().cloned().ok_or("no sheet")?;
        let range = wb.worksheet_range(&name).map_err(|e| e.to_string())?;
        let rows = range
            .rows()
            .map(|r| {
                r.iter()
                    .map(|c| match c {
                        Data::Empty => String::new(),
                        Data::String(s) => s.clone(),
                        Data::Float(v) => fmt_num(*v, 10),
                        Data::Int(v) => v.to_string(),
                        Data::Bool(b) => (*b as i32).to_string(),
                        other => other.to_string(),
                    })
                    .collect()
            })
            .collect();
        return Ok(rows);
    }
    let raw = std::fs::read(path).map_err(|e| e.to_string())?;
    let raw = raw.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&raw);
    let text = match std::str::from_utf8(raw) {
        Ok(s) => s.to_string(),
        Err(_) => raw.iter().map(|&b| if b < 0x80 { b as char } else if (0xA1..=0xFB).contains(&b) { char::from_u32(0x0E01 + (b - 0xA1) as u32).unwrap_or('?') } else { '?' }).collect(),
    };
    let first = text.lines().next().unwrap_or("");
    let delim = [b'\t', b';', b','].into_iter().max_by_key(|d| first.bytes().filter(|b| b == d).count()).unwrap_or(b',');
    let mut rd = csv::ReaderBuilder::new().delimiter(delim).has_headers(false).flexible(true).from_reader(text.as_bytes());
    let mut rows = vec![];
    for r in rd.records() {
        let r = r.map_err(|e| e.to_string())?;
        rows.push(r.iter().map(|s| s.to_string()).collect());
    }
    Ok(rows)
}

fn synonyms(field: &str) -> &'static [&'static str] {
    match field {
        "length" => &["length", "len", "l", "ความยาว", "ยาว", "cut length"],
        "width" => &["height", "width", "h", "w", "ความสูง", "สูง", "ความกว้าง"],
        "quantity" => &["quantity", "qty", "q", "count", "pcs", "ปริมาณ", "จำนวน", "no.", "nos"],
        "a1" => &["angle 1", "angle1", "angle_1", "a1", "มุม 1", "มุม1"],
        "a2" => &["angle 2", "angle2", "angle_2", "a2", "มุม 2", "มุม2"],
        "material" => &["material", "mat", "วัสดุ", "type", "size", "ชนิด", "ขนาด", "dia"],
        "label" => &["label", "name", "ฉลาก", "ป้าย", "ชื่อ"],
        "customer" => &["customer", "client", "ลูกค้า"],
        "price" => &["price", "cost", "ราคา"],
        "weight" => &["weight", "kg/m", "น้ำหนัก"],
        "priority" => &["priority", "ลำดับความสำคัญ"],
        _ => &[],
    }
}

/// Column of each field: by header names when the first row is a header, else by position.
pub fn guess_columns(table: &[Vec<String>], fields: &[&str]) -> (bool, HashMap<&'static str, Option<usize>>) {
    let first = table.first().cloned().unwrap_or_default();
    // a header has names only: a row with a number in it is data (1.1, DB10, 7.5, 127)
    let header = first.iter().any(|c| !c.trim().is_empty()) && first.iter().all(|c| c.trim().is_empty() || parse_num(c).is_none());
    let mut map: HashMap<&'static str, Option<usize>> = HashMap::new();
    let all = ["length", "width", "quantity", "a1", "a2", "material", "label", "customer", "price", "priority", "weight"];
    let mut taken: Vec<usize> = vec![];
    for f in fields {
        let key = all.iter().find(|k| *k == f).copied().unwrap_or("length");
        let col = if header {
            // an exact name first, then a name that starts with or contains a known word
            let hs: Vec<String> = first.iter().map(|h| h.trim().to_lowercase()).collect();
            let syn = synonyms(key);
            hs.iter().position(|h| syn.iter().any(|s| h == s))
                .or_else(|| hs.iter().enumerate().position(|(i, h)| !taken.contains(&i) && syn.iter().any(|s| s.chars().count() >= 3 && h.contains(s))))
        } else {
            None
        };
        if let Some(c) = col {
            taken.push(c);
        }
        map.insert(key, col);
    }
    if !header || map.values().all(|v| v.is_none()) {
        for (i, f) in fields.iter().enumerate() {
            let key = all.iter().find(|k| *k == f).copied().unwrap_or("length");
            map.insert(key, if i < first.len() { Some(i) } else { None });
        }
    }
    (header, map)
}

/// Visible columns of a grid (without check boxes and drawings) as a table with titles.
fn grid_table(h: HWND) -> Vec<Vec<String>> {
    grid::with(h, |g| {
        let cols: Vec<usize> = g.vis_cols().into_iter().filter(|&c| g.cols[c].key != "pieces" && g.cols[c].key != "image").collect();
        let mut out = vec![cols.iter().map(|&c| g.cols[c].title.replace('\n', " ")).collect::<Vec<_>>()];
        for r in 1..=g.rows.len() {
            out.push(cols.iter().map(|&c| if g.cols[c].toggle { if g.cell(r, c) == "1" { "1".into() } else { "0".into() } } else { g.cell(r, c).to_string() }).collect());
        }
        out
    })
    .unwrap_or_default()
}

pub fn write_table(path: &Path, rows: &[Vec<String>]) -> Result<(), String> {
    write_table_typed(path, rows, &[])
}

/// `text_cols` are written as text in Excel even when they look like numbers.
pub fn write_table_typed(path: &Path, rows: &[Vec<String>], text_cols: &[usize]) -> Result<(), String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ext == "xlsx" || ext == "xls" {
        let path = path.with_extension("xlsx");
        let mut wb = rust_xlsxwriter::Workbook::new();
        let ws = wb.add_worksheet();
        let bold = rust_xlsxwriter::Format::new().set_bold();
        for (r, row) in rows.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                let (r16, c16) = (r as u32, c as u16);
                if r > 0 && !text_cols.contains(&c) {
                    if let Some(n) = parse_num(v).filter(|_| !v.trim().is_empty() && v.trim().parse::<f64>().is_ok()) {
                        ws.write_number(r16, c16, n).map_err(|e| e.to_string())?;
                        continue;
                    }
                }
                if r == 0 {
                    ws.write_string_with_format(r16, c16, v, &bold).map_err(|e| e.to_string())?;
                } else {
                    ws.write_string(r16, c16, v).map_err(|e| e.to_string())?;
                }
            }
        }
        wb.save(&path).map_err(|e| e.to_string())
    } else {
        let sep = if ext == "txt" { b'\t' } else { b',' };
        let mut w = csv::WriterBuilder::new().delimiter(sep).from_writer(vec![0xEF, 0xBB, 0xBF]);
        for r in rows {
            w.write_record(r).map_err(|e| e.to_string())?;
        }
        let bytes = w.into_inner().map_err(|e| e.to_string())?;
        std::fs::write(path, bytes).map_err(|e| e.to_string())
    }
}

pub unsafe fn table_export(f: &mut Form, grid_name: &str) {
    let Some(p) = common::save_file(f.hwnd, TABLE_FILTER, "xlsx", None, "") else { return };
    if let Err(e) = write_table(&p, &grid_table(f.hwnd_of(grid_name))) {
        error(&e);
    }
}

pub unsafe fn grid_export(owner: HWND, h: HWND, _name: &str) {
    let Some(p) = common::save_file(owner, TABLE_FILTER, "xlsx", None, "") else { return };
    if let Err(e) = write_table(&p, &grid_table(h)) {
        error(&e);
    }
}

/// Results as a table: one row per pattern, pieces as text.
/// The cutting plan as a table: the visible columns, then the cutting formula
/// (457+457+…) when that column is hidden. Returns the table and the formula column.
fn results_table() -> (Vec<Vec<String>>, usize) {
    let h = main_form().hwnd_of("g_results_1d");
    let mut t = grid_table(h);
    let d = digits().0;
    let title = mw::col_title("pattern");
    if let Some(c) = t.first().and_then(|head| head.iter().position(|x| *x == title)) {
        return (t, c);
    }
    let c = t.first().map(|h| h.len()).unwrap_or(0);
    if let Some(head) = t.first_mut() {
        head.push(title);
    }
    let n = t.len();
    for r in 1..n {
        let text = results::bar_of_row(r).and_then(|bi| results::view(|v| v.sol.bars.get(bi).map(|b| results::pattern_text(b, d)))).flatten().unwrap_or_default();
        t[r].push(text);
    }
    (t, c)
}

pub unsafe fn save_results_table(f: &mut Form) {
    let Some(p) = common::save_file(f.hwnd, "Excel (*.xlsx)|*.xlsx|CSV (*.csv)|*.csv|Text Files (*.txt)|*.txt", "xlsx", Some(&settings::dir("results")), "") else { return };
    let (t, c) = results_table();
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let res = match results::view(|v| results::report(v, &job_name())) {
        Some(rep) if ext == "xlsx" || ext == "xls" => write_results_xlsx(&p, &t, c, &rep),
        _ => write_table_typed(&p, &t, &[c]),
    };
    if let Err(e) = res {
        error(&e);
    }
}

/// Sheet 1: the cutting plan table (cutting formula as text, length and material cells in
/// yellow like the print); sheet 2: title, bars used, length and weight tables, date.
pub fn write_results_xlsx(path: &Path, rows: &[Vec<String>], text_col: usize, rep: &results::Report) -> Result<(), String> {
    use rust_xlsxwriter::{Color, Format, Workbook};
    let path = path.with_extension("xlsx");
    let mut wb = Workbook::new();
    let bold = Format::new().set_bold();
    let yellow = Format::new().set_background_color(Color::RGB(0xFFFF00));
    let mat_col = rows.first().and_then(|h| h.iter().position(|x| *x == mw::col_title("material")));
    let len_col = rows.first().and_then(|h| h.iter().position(|x| *x == mw::col_title("length")));
    let yellow_mats: Vec<String> = rep.rows.iter().filter(|r| r.highlight_material).map(|r| r.material.clone()).collect();
    let yellow_lens: Vec<String> = rep.rows.iter().filter(|r| r.highlight_length).map(|r| r.length.clone()).collect();
    let ws = wb.add_worksheet();
    ws.set_name(&i18n::tr("Cutting plan")).map_err(|e| e.to_string())?;
    for (r, row) in rows.iter().enumerate() {
        // the two columns are highlighted each by its own groups
        let mat_hl = r > 0 && mat_col.and_then(|c| row.get(c)).map(|m| yellow_mats.contains(m)).unwrap_or(false);
        let len_hl = r > 0 && len_col.and_then(|c| row.get(c)).map(|l| yellow_lens.contains(l)).unwrap_or(false);
        for (c, v) in row.iter().enumerate() {
            let (r32, c16) = (r as u32, c as u16);
            let paint = (mat_hl && Some(c) == mat_col) || (len_hl && Some(c) == len_col);
            if r == 0 {
                ws.write_string_with_format(r32, c16, v, &bold).map_err(|e| e.to_string())?;
            } else if c != text_col && v.trim().parse::<f64>().is_ok() {
                let n = v.trim().parse::<f64>().unwrap();
                if paint {
                    ws.write_number_with_format(r32, c16, n, &yellow).map_err(|e| e.to_string())?;
                } else {
                    ws.write_number(r32, c16, n).map_err(|e| e.to_string())?;
                }
            } else if paint {
                ws.write_string_with_format(r32, c16, v, &yellow).map_err(|e| e.to_string())?;
            } else {
                ws.write_string(r32, c16, v).map_err(|e| e.to_string())?;
            }
        }
    }
    let s2 = wb.add_worksheet();
    s2.set_name(&i18n::t("summary_sheet")).map_err(|e| e.to_string())?;
    let mut y = 0u32;
    s2.write_string_with_format(y, 0, &rep.title, &bold).map_err(|e| e.to_string())?;
    y += 2;
    for line in &rep.bars_used {
        s2.write_string(y, 0, line).map_err(|e| e.to_string())?;
        y += 1;
    }
    let mut table = |y: &mut u32, heads: [String; 4], data: &[[String; 4]]| -> Result<(), String> {
        *y += 1;
        for (c, h) in heads.iter().enumerate() {
            s2.write_string_with_format(*y, c as u16, h, &bold).map_err(|e| e.to_string())?;
        }
        *y += 1;
        for r in data {
            for (c, v) in r.iter().enumerate() {
                match v.parse::<f64>() {
                    Ok(n) if c > 0 => s2.write_number(*y, c as u16, n).map(|_| ()),
                    _ => s2.write_string(*y, c as u16, v).map(|_| ()),
                }
                .map_err(|e| e.to_string())?;
            }
            *y += 1;
        }
        Ok(())
    };
    table(&mut y, [i18n::t("col_type"), i18n::t("col_total_used_length"), i18n::t("col_size_used_bars"), i18n::t("col_waste")], &rep.lengths)?;
    table(&mut y, [i18n::t("col_type"), i18n::t("col_total_used_length"), i18n::t("col_weight_m"), i18n::t("col_total_weight")], &rep.weights)?;
    s2.write_string(y + 1, 0, &format!("{} {}", i18n::t("report_date"), rep.date)).map_err(|e| e.to_string())?;
    s2.set_column_width(0, 18).map_err(|e| e.to_string())?;
    for c in 1..4 {
        s2.set_column_width(c, 20).map_err(|e| e.to_string())?;
    }
    wb.save(&path).map_err(|e| e.to_string())
}

pub unsafe fn copy_results(_f: &mut Form) {
    let (t, _) = results_table();
    let text: String = t.iter().map(|r| r.join("\t")).collect::<Vec<_>>().join("\r\n");
    set_clipboard(&text);
}

unsafe fn set_clipboard(text: &str) {
    use windows_sys::Win32::System::DataExchange::*;
    use windows_sys::Win32::System::Memory::*;
    let w = wide(text);
    if OpenClipboard(app::main_hwnd()) == 0 {
        return;
    }
    EmptyClipboard();
    let h = GlobalAlloc(GMEM_MOVEABLE, w.len() * 2);
    if !h.is_null() {
        let p = GlobalLock(h) as *mut u16;
        std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
        GlobalUnlock(h);
        SetClipboardData(13, h as _);
    }
    CloseClipboard();
}

// ------------------------------------------------------------------ results image

/// The results table with its drawings, rendered at screen resolution: each row has its data
/// on a first line and the cutting drawing across a second line.
unsafe fn render_results(dc: HDC, x0: i32, y0: i32, width: i32, rows: std::ops::Range<usize>, header: bool) -> i32 {
    let h = main_form().hwnd_of("g_results_1d");
    let Some(v) = results::view(|v| v.clone()) else { return y0 };
    let g = grid::grid(h);
    let cols: Vec<usize> = g.vis_cols().into_iter().filter(|&c| g.cols[c].key != "pieces").collect();
    let data_w: i32 = cols.iter().map(|&c| g.cols[c].width).sum();
    let total_w = width.max(data_w);
    // the last column takes the remaining width, like the cutting formula on screen
    let widths: Vec<i32> = cols.iter().enumerate().map(|(i, &c)| g.cols[c].width + if i + 1 == cols.len() { total_w - data_w } else { 0 }).collect();
    let fixed_w: i32 = cols.iter().zip(&widths).filter(|(c, _)| **c < g.fixed_cols).map(|(_, w)| *w).sum();
    let fnt = font(&g.font);
    let hfnt = font(&g.header_font);
    let mut y = y0;
    let line = |x1: i32, y1: i32, x2: i32, y2: i32| {
        MoveToEx(dc, x1, y1, std::ptr::null_mut());
        LineTo(dc, x2, y2);
    };
    SetBkMode(dc, TRANSPARENT as i32);
    if header {
        let mut x = x0;
        SelectObject(dc, hfnt as _);
        fill(dc, &rect(x0, y, total_w, g.header_h), g.fixed_color);
        for (&c, &w) in cols.iter().zip(&widths) {
            let mut r = RECT { left: x + 2, top: y, right: x + w - 3, bottom: y + g.header_h };
            draw_text(dc, &g.cols[c].title, &mut r, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
            line(x + w - 1, y, x + w - 1, y + g.header_h);
            x += w;
        }
        line(x0, y + g.header_h - 1, x0 + total_w, y + g.header_h - 1);
        line(x0, y, x0 + total_w, y);
        line(x0, y, x0, y + g.header_h);
        y += g.header_h;
    }
    let mut max_len = v.max_len;
    if max_len <= 0.0 {
        max_len = 1.0;
    }
    let view = results::View { max_len, ..v.clone() };
    for r in rows {
        let row = r + 1;
        let row_h = g.row_heights.get(r).copied().unwrap_or(g.row_h);
        let top_h = (row_h - g.detail_h).max(1);
        let mut x = x0;
        SelectObject(dc, fnt as _);
        for (&c, &w) in cols.iter().zip(&widths) {
            let fixed = c < g.fixed_cols;
            let hh = if fixed { row_h } else { top_h };
            let cell = RECT { left: x, top: y, right: x + w - 1, bottom: y + hh - 1 };
            if fixed {
                fill(dc, &cell, g.fixed_color);
            }
            let mut tr = RECT { left: x + 4, top: y, right: x + w - 6, bottom: y + hh };
            let al = [DT_LEFT, DT_CENTER, DT_RIGHT][g.cols[c].align.clamp(0, 2) as usize];
            draw_text(dc, g.cell(row, c), &mut tr, al | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
            line(x + w - 1, y, x + w - 1, y + hh);
            x += w;
        }
        if let Some(b) = results::bar_of_row(row).and_then(|bi| view.sol.bars.get(bi)) {
            let pr = RECT { left: x0 + fixed_w, top: y + top_h - 4, right: x0 + total_w - 1, bottom: y + row_h - 1 };
            results::draw_bar(dc, &pr, pr.right - pr.left, b, &view);
            SelectObject(dc, fnt as _);
        }
        line(x0 + total_w - 1, y + top_h, x0 + total_w - 1, y + row_h);
        line(x0, y + row_h - 1, x0 + total_w, y + row_h - 1);
        line(x0, y, x0, y + row_h);
        y += row_h;
    }
    y
}

pub unsafe fn save_image(f: &mut Form) {
    let Some(p) = common::save_file(f.hwnd, "PNG (*.png)|*.png|Bitmap (*.bmp)|*.bmp", "png", Some(&settings::dir("results")), "") else { return };
    if let Err(e) = write_image(f, &p) {
        error(&e);
    }
}

/// The cutting plan as a PNG or BMP picture.
pub unsafe fn write_image(f: &mut Form, p: &Path) -> Result<(), String> {
    let h = f.hwnd_of("g_results_1d");
    let (n, header_h, rows_h, width) = grid::with(h, |g| (g.rows.len(), g.header_h, (0..g.rows.len()).map(|r| g.row_heights.get(r).copied().unwrap_or(g.row_h)).sum::<i32>(), g.vis_cols().iter().map(|&c| g.cols[c].width).sum::<i32>())).unwrap_or((0, 39, 0, 800));
    let (w, hgt) = (width.max(crate::ui::theme::px(1000)) + 1, header_h + rows_h + 1);
    let screen = GetDC(std::ptr::null_mut());
    let mem = CreateCompatibleDC(screen);
    let bmp = CreateCompatibleBitmap(screen, w, hgt);
    let old = SelectObject(mem, bmp as _);
    fill(mem, &rect(0, 0, w, hgt), rgb(255, 255, 255));
    let pen = CreatePen(PS_SOLID, 1, 0);
    let op = SelectObject(mem, pen as _);
    render_results(mem, 0, 0, w - 1, 0..n, true);
    SelectObject(mem, op);
    DeleteObject(pen as _);
    // read the pixels back (32-bit top-down)
    let mut bi: BITMAPINFO = std::mem::zeroed();
    bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = w;
    bi.bmiHeader.biHeight = -hgt;
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    let mut buf = vec![0u8; (w * hgt * 4) as usize];
    SelectObject(mem, old);
    GetDIBits(mem, bmp, 0, hgt as u32, buf.as_mut_ptr() as _, &mut bi, DIB_RGB_COLORS);
    DeleteObject(bmp as _);
    DeleteDC(mem);
    ReleaseDC(std::ptr::null_mut(), screen);
    let mut rgba = Vec::with_capacity(buf.len());
    for px in buf.chunks(4) {
        rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
    }
    let img = image::RgbaImage::from_raw(w as u32, hgt as u32, rgba);
    match img {
        Some(i) => {
            let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ext == "bmp" {
                image::DynamicImage::ImageRgba8(i).to_rgb8().save(p).map_err(|e| e.to_string())
            } else {
                i.save(p).map_err(|e| e.to_string())
            }
        }
        None => Err("image".into()),
    }
}

// ------------------------------------------------------------------ DXF

pub unsafe fn save_dxf(o: &DxfOptions) {
    let Some(p) = common::save_file(app::main_hwnd(), "DXF|*.dxf", "dxf", Some(&settings::dir("results")), "") else { return };
    let Some(v) = results::view(|v| v.clone()) else { return };
    let text = dxf_text(&v, o, digits().0);
    if let Err(e) = std::fs::write(&p, text) {
        error(&e.to_string());
    }
}

/// R12 DXF: every pattern as a strip (bar contour, pieces, offcut, texts).
pub fn dxf_text(v: &results::View, o: &DxfOptions, d: i32) -> String {
    let mut s = String::new();
    let units = [0, 1, 2, 3, 4, 5, 6, 7][o.unit.clamp(0, 7) as usize];
    s.push_str(&format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n9\n$INSUNITS\n70\n{units}\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n"));
    let layer = |l: &str| if l.trim().is_empty() { "0".to_string() } else { l.trim().to_string() };
    let line = |s: &mut String, l: &str, x1: f64, y1: f64, x2: f64, y2: f64| {
        s.push_str(&format!("0\nLINE\n8\n{}\n10\n{x1}\n20\n{y1}\n30\n0\n11\n{x2}\n21\n{y2}\n31\n0\n", layer(l)));
    };
    let text = |s: &mut String, l: &str, x: f64, y: f64, h: f64, t: &str| {
        s.push_str(&format!("0\nTEXT\n8\n{}\n10\n{x}\n20\n{y}\n30\n0\n40\n{h}\n1\n{}\n72\n1\n11\n{x}\n21\n{y}\n31\n0\n", layer(l), t.replace('\n', " ")));
    };
    let fh = o.font_height;
    let order = results::display_order(&v.sol);
    let mut y = 0.0;
    for &bi in &order {
        let b = &v.sol.bars[bi];
        let h = if b.height > 0.0 { b.height } else { fh * 2.0 };
        if o.bar_contour {
            for (x1, y1, x2, y2) in [(0.0, y, b.length, y), (b.length, y, b.length, y + h), (b.length, y + h, 0.0, y + h), (0.0, y + h, 0.0, y)] {
                line(&mut s, &o.layer_bars, x1, y1, x2, y2);
            }
        }
        if o.bar_info {
            text(&mut s, &o.layer_text, b.length / 2.0, y + h + fh * 0.5, fh, &format!("{} x {} ({})", fmt_num(b.length, d), b.quantity, b.material));
        }
        for p in &b.pieces {
            if o.parts_contour {
                let (sb, eb, st, et) = (p.start_bottom_x, p.end_bottom_x, p.start_top_x, p.end_top_x);
                for (x1, y1, x2, y2) in [(sb, y, eb, y), (eb, y, et, y + h), (et, y + h, st, y + h), (st, y + h, sb, y)] {
                    line(&mut s, &o.layer_parts, x1, y1, x2, y2);
                }
            }
            let cx = (p.start_bottom_x + p.end_top_x) / 2.0;
            let mut t = vec![];
            if o.parts_size {
                t.push(fmt_num(p.length, d));
            }
            if o.parts_label && !p.label.is_empty() {
                t.push(p.label.clone());
            }
            if o.customer && !p.customer.is_empty() {
                t.push(p.customer.clone());
            }
            if o.angles && (p.a1 != 90.0 || p.a2 != 90.0) {
                t.push(format!("{}/{}", fmt_num(p.a1, 2), fmt_num(p.a2, 2)));
            }
            if !t.is_empty() {
                text(&mut s, &o.layer_text, cx, y + h / 2.0 - fh / 2.0, fh, &t.join(" "));
            }
        }
        if b.waste > 0.0 {
            let x = b.length - b.waste;
            if o.waste_contour {
                for (x1, y1, x2, y2) in [(x, y, b.length, y), (b.length, y, b.length, y + h), (b.length, y + h, x, y + h), (x, y + h, x, y)] {
                    line(&mut s, &o.layer_waste, x1, y1, x2, y2);
                }
            }
            if o.waste_size {
                text(&mut s, &o.layer_text, x + b.waste / 2.0, y + h / 2.0 - fh / 2.0, fh, &fmt_num(b.waste, d));
            }
        }
        y -= h + fh * 3.0;
    }
    s.push_str("0\nENDSEC\n0\nEOF\n");
    s
}

// ------------------------------------------------------------------ printing

struct Printer {
    dc: HDC,
    /// printable size in 1/96 inch
    w: i32,
    h: i32,
}

unsafe fn begin_print(title: &str) -> Option<Printer> {
    let dc = common::print_dialog(app::main_hwnd())?;
    let name = wide(title);
    let di = DOCINFOW { cbSize: std::mem::size_of::<DOCINFOW>() as i32, lpszDocName: name.as_ptr(), lpszOutput: std::ptr::null(), lpszDatatype: std::ptr::null(), fwType: 0 };
    if StartDocW(dc, &di) <= 0 {
        DeleteDC(dc);
        return None;
    }
    let dpi = GetDeviceCaps(dc, LOGPIXELSX as i32);
    let (pw, ph) = (GetDeviceCaps(dc, HORZRES as i32), GetDeviceCaps(dc, VERTRES as i32));
    Some(Printer { dc, w: pw * 96 / dpi.max(1), h: ph * 96 / dpi.max(1) })
}

unsafe fn page(p: &Printer) {
    StartPage(p.dc);
    let dpi = GetDeviceCaps(p.dc, LOGPIXELSX as i32);
    SetMapMode(p.dc, MM_ANISOTROPIC as i32);
    SetWindowExtEx(p.dc, 96, 96, std::ptr::null_mut());
    SetViewportExtEx(p.dc, dpi, dpi, std::ptr::null_mut());
    SetBkMode(p.dc, TRANSPARENT as i32);
    let pen = CreatePen(PS_SOLID, 1, 0);
    SelectObject(p.dc, pen as _);
}

unsafe fn end_page(p: &Printer) {
    EndPage(p.dc);
}

unsafe fn end_print(p: Printer) {
    EndDoc(p.dc);
    DeleteDC(p.dc);
}

unsafe fn print_header(p: &Printer, title: &str) -> i32 {
    let i = &app().iface;
    let big = FontSpec::new(crate::ui::theme::FACE_SEMIBOLD, -18);
    let small = FontSpec::new(crate::ui::theme::FACE, -12);
    SelectObject(p.dc, font(&big) as _);
    let mut r = RECT { left: 20, top: 20, right: p.w - 20, bottom: 50 };
    draw_text(p.dc, title, &mut r, DT_LEFT | DT_SINGLELINE | DT_NOPREFIX);
    SelectObject(p.dc, font(&small) as _);
    let mut r2 = RECT { left: 20, top: 24, right: p.w - 20, bottom: 50 };
    let right = format!("{}  {}", i.company.name, settings::now_stamp());
    draw_text(p.dc, right.trim(), &mut r2, DT_RIGHT | DT_SINGLELINE | DT_NOPREFIX);
    56
}

/// Print a table (rows of text with titles) across pages.
unsafe fn print_rows(p: &Printer, title: &str, table: &[Vec<String>], widths: &[i32]) {
    let fnt = FontSpec::points(&app().iface.font_grid.name, app().iface.font_grid.size.min(12));
    let mut y = 0;
    let mut first_page = true;
    let mut k = 1;
    let total: i32 = widths.iter().sum();
    let scale = if total > p.w - 40 { (p.w - 40) as f64 / total as f64 } else { 1.0 };
    let ws: Vec<i32> = widths.iter().map(|w| (*w as f64 * scale) as i32).collect();
    while k < table.len() || first_page {
        page(p);
        y = print_header(p, title);
        first_page = false;
        let row_h = (report_line_h(p.dc, &fnt) + 6).max(20);
        SelectObject(p.dc, font(&fnt) as _);
        // header row
        let mut x = 20;
        for (c, t) in table.first().map(|h| h.as_slice()).unwrap_or(&[]).iter().enumerate() {
            let w = *ws.get(c).unwrap_or(&60);
            fill(p.dc, &rect(x, y, w, row_h), rgb(0xE0, 0xE0, 0xE0));
            let mut r = RECT { left: x + 2, top: y, right: x + w - 2, bottom: y + row_h };
            draw_text(p.dc, t, &mut r, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
            frame(p.dc, &rect(x, y, w + 1, row_h + 1), 0);
            x += w;
        }
        y += row_h;
        while k < table.len() && y + row_h < p.h - 20 {
            let mut x = 20;
            for (c, t) in table[k].iter().enumerate() {
                let w = *ws.get(c).unwrap_or(&60);
                let mut r = RECT { left: x + 2, top: y, right: x + w - 2, bottom: y + row_h };
                draw_text(p.dc, t, &mut r, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
                frame(p.dc, &rect(x, y, w + 1, row_h + 1), 0);
                x += w;
            }
            y += row_h;
            k += 1;
        }
        end_page(p);
        if table.len() <= 1 {
            break;
        }
    }
    let _ = y;
}

fn grid_widths(h: HWND) -> Vec<i32> {
    grid::with(h, |g| g.vis_cols().into_iter().filter(|&c| g.cols[c].key != "pieces" && g.cols[c].key != "image").map(|c| g.cols[c].width.max(30)).collect()).unwrap_or_default()
}

pub unsafe fn print_grid(h: HWND, title: &str) {
    let table = grid_table(h);
    let widths = grid_widths(h);
    if let Some(p) = begin_print(title) {
        print_rows(&p, title, &table, &widths);
        end_print(p);
    }
}

pub unsafe fn print_table(f: &mut Form, grid_name: &str, title: &str) {
    print_grid(f.hwnd_of(grid_name), title);
}

/// The job name of section 0.
pub unsafe fn job_name() -> String {
    get_text(main_form().hwnd_of("edJobName")).trim().to_string()
}

/// The printed cutting plan: job name and bars used at the top, the table of patterns
/// (length and material in yellow for every other material group), then the length and
/// weight tables and the date.
pub unsafe fn print_results() {
    let Some(rep) = results::view(|v| results::report(v, &job_name())) else { return };
    let Some(p) = begin_print(&format!("Neramit Cut - {}", rep.title)) else { return };
    draw_report(&p, &rep);
    end_print(p);
}

/// The report on the pages of `p` (page breaks at its height).
unsafe fn draw_report(p: &Printer, rep: &results::Report) {
    let p = p;
    let normal = FontSpec::new(crate::ui::theme::FACE, -12);
    let bold = FontSpec::new(crate::ui::theme::FACE_SEMIBOLD, -12);
    let title_font = FontSpec::new(crate::ui::theme::FACE_SEMIBOLD, -16);
    page(&p);
    // rows a little taller than the font's line, so a tone mark over an upper vowel (ชิ้น, น้ำ)
    // stays inside the cell instead of being clipped by its top edge
    let (x0, row_h) = (40, report_line_h(p.dc, &normal) + 6);
    let head_fill = rgb(0xF2, 0xDC, 0xDB);
    let yellow = rgb(0xFF, 0xFF, 0x00);
    let width = p.w - 2 * x0;
    let cell = |x: i32, y: i32, w: i32, text: &str, f: &FontSpec, fill_c: Option<u32>, right: bool| {
        if let Some(c) = fill_c {
            fill(p.dc, &rect(x, y, w, row_h), c);
        }
        SelectObject(p.dc, font(f) as _);
        let mut r = RECT { left: x + 3, top: y, right: x + w - 3, bottom: y + row_h };
        draw_text(p.dc, text, &mut r, if right { DT_RIGHT } else { DT_LEFT } | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
        frame(p.dc, &rect(x, y, w + 1, row_h + 1), 0);
    };
    // columns: length, quantity, material, offcut, cutting formula (the rest)
    let ws = [56, 56, 70, 56];
    let fw = width - ws.iter().sum::<i32>();
    let heads = [i18n::t("col_length"), i18n::t("col_quantity"), i18n::t("col_material"), i18n::t("col_waste"), i18n::t("col_pattern")];
    let table_head = |y: i32| {
        let mut x = x0;
        for (i, h) in heads.iter().enumerate() {
            let w = if i < 4 { ws[i] } else { fw };
            cell(x, y, w, h, &bold, Some(head_fill), false);
            x += w;
        }
    };
    let bottom = p.h - 30;
    // header: title left, bars used right
    SelectObject(p.dc, font(&title_font) as _);
    let mut tr = RECT { left: x0, top: 24, right: x0 + width / 2, bottom: 24 + 2 * report_line_h(p.dc, &title_font) };
    draw_text(p.dc, &rep.title, &mut tr, DT_LEFT | DT_WORDBREAK | DT_NOPREFIX);
    SelectObject(p.dc, font(&normal) as _);
    let mut y = 24;
    for line in &rep.bars_used {
        let mut r = RECT { left: x0 + width / 2, top: y, right: x0 + width, bottom: y + row_h };
        draw_text(p.dc, line, &mut r, DT_LEFT | DT_SINGLELINE | DT_NOPREFIX);
        y += row_h;
    }
    y = y.max(tr.bottom).max(64) + 14;
    table_head(y);
    y += row_h;
    for row in &rep.rows {
        if y + row_h > bottom {
            end_page(&p);
            page(&p);
            y = 30;
            table_head(y);
            y += row_h;
        }
        let mut x = x0;
        cell(x, y, ws[0], &row.length, &normal, if row.highlight_length { Some(yellow) } else { None }, true);
        x += ws[0];
        cell(x, y, ws[1], &row.quantity, &normal, None, true);
        x += ws[1];
        cell(x, y, ws[2], &row.material, &normal, if row.highlight_material { Some(yellow) } else { None }, false);
        x += ws[2];
        cell(x, y, ws[3], &row.waste, &normal, None, true);
        x += ws[3];
        cell(x, y, fw, &row.formula, &normal, None, false);
        y += row_h;
    }
    // the two summary tables
    let small_table = |y: &mut i32, heads: [String; 4], rows: &[[String; 4]], widths: [i32; 4]| {
        let need = row_h * (rows.len() as i32 + 1) + 20;
        if *y + need > bottom {
            end_page(&p);
            page(&p);
            *y = 30;
        }
        *y += 20;
        let mut x = x0;
        for (i, h) in heads.iter().enumerate() {
            cell(x, *y, widths[i], h, &bold, Some(head_fill), false);
            x += widths[i];
        }
        *y += row_h;
        for r in rows {
            let mut x = x0;
            for (i, v) in r.iter().enumerate() {
                cell(x, *y, widths[i], v, &normal, None, i > 0);
                x += widths[i];
            }
            *y += row_h;
        }
    };
    small_table(&mut y, [i18n::t("col_type"), i18n::t("col_total_used_length"), i18n::t("col_size_used_bars"), i18n::t("col_waste")], &rep.lengths, [90, 130, 130, 110]);
    small_table(&mut y, [i18n::t("col_type"), i18n::t("col_total_used_length"), i18n::t("col_weight_m"), i18n::t("col_total_weight")], &rep.weights, [90, 130, 110, 130]);
    // the date at the bottom
    SelectObject(p.dc, font(&normal) as _);
    if y + 40 > bottom {
        end_page(&p);
        page(&p);
        y = 30;
    }
    let mut r = RECT { left: x0, top: y + 24, right: x0 + width, bottom: y + 24 + row_h };
    draw_text(p.dc, &format!("{} {}", i18n::t("report_date"), rep.date), &mut r, DT_RIGHT | DT_SINGLELINE | DT_NOPREFIX);
    end_page(&p);
}

/// Line height of `f` in the page's logical units (the page mapping must already be set).
unsafe fn report_line_h(dc: HDC, f: &FontSpec) -> i32 {
    let old = SelectObject(dc, font(f) as _);
    let mut tm: TEXTMETRICW = std::mem::zeroed();
    GetTextMetricsW(dc, &mut tm);
    SelectObject(dc, old);
    tm.tmHeight.max(12)
}

/// Report as a PDF file: the printed report sent to Windows' "Microsoft Print to PDF" with the
/// file as output (no print dialog), then opened.
pub unsafe fn save_report_pdf(f: &mut Form) {
    let Some(rep) = results::view(|v| results::report(v, &job_name())) else {
        msg(&i18n::t("no_plan_yet"), MB_ICONINFORMATION);
        return;
    };
    let name: String = rep.title.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let Some(path) = common::save_file(f.hwnd, "PDF (*.pdf)|*.pdf", "pdf", Some(&settings::dir("results")), &name) else { return };
    match write_report_pdf(&path, &rep) {
        Ok(()) => common::open_url(&path.to_string_lossy()),
        Err(e) => error(&e),
    }
}

pub unsafe fn write_report_pdf(path: &Path, rep: &results::Report) -> Result<(), String> {
    let dc = CreateDCW(wide("WINSPOOL").as_ptr(), wide("Microsoft Print to PDF").as_ptr(), std::ptr::null(), std::ptr::null());
    if dc.is_null() {
        return Err(i18n::t("pdf_printer_missing"));
    }
    let _ = std::fs::remove_file(path);
    let name = wide(&format!("Neramit Cut - {}", rep.title));
    let out = wide(&path.to_string_lossy());
    let di = DOCINFOW { cbSize: std::mem::size_of::<DOCINFOW>() as i32, lpszDocName: name.as_ptr(), lpszOutput: out.as_ptr(), lpszDatatype: std::ptr::null(), fwType: 0 };
    if StartDocW(dc, &di) <= 0 {
        DeleteDC(dc);
        return Err(i18n::t("pdf_printer_missing"));
    }
    let dpi = GetDeviceCaps(dc, LOGPIXELSX as i32);
    let (pw, ph) = (GetDeviceCaps(dc, HORZRES as i32), GetDeviceCaps(dc, VERTRES as i32));
    let p = Printer { dc, w: pw * 96 / dpi.max(1), h: ph * 96 / dpi.max(1) };
    draw_report(&p, rep);
    end_print(p);
    // the spooler writes the file a moment later
    for _ in 0..50 {
        if path.exists() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if path.exists() { Ok(()) } else { Err(i18n::t("pdf_printer_missing")) }
}

/// QA: the printed report drawn on one long A4-wide picture.
pub unsafe fn report_png(path: &Path) -> Result<(), String> {
    let rep = results::view(|v| results::report(v, &job_name())).ok_or("no plan")?;
    let (w, h) = (794, 140 + 30 * (rep.rows.len() as i32 + rep.lengths.len() as i32 + rep.weights.len() as i32 + 8) + 30 * rep.bars_used.len() as i32);
    let screen = GetDC(std::ptr::null_mut());
    let mem = CreateCompatibleDC(screen);
    let bmp = CreateCompatibleBitmap(screen, w, h);
    let old = SelectObject(mem, bmp as _);
    fill(mem, &rect(0, 0, w, h), rgb(255, 255, 255));
    let pen = CreatePen(PS_SOLID, 1, 0);
    let op = SelectObject(mem, pen as _);
    let p = Printer { dc: mem, w, h: h + 100 };
    draw_report(&p, &rep);
    SelectObject(mem, op);
    DeleteObject(pen as _);
    let mut bi: BITMAPINFO = std::mem::zeroed();
    bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = w;
    bi.bmiHeader.biHeight = -h;
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    let mut buf = vec![0u8; (w * h * 4) as usize];
    SelectObject(mem, old);
    GetDIBits(mem, bmp, 0, h as u32, buf.as_mut_ptr() as _, &mut bi, DIB_RGB_COLORS);
    DeleteObject(bmp as _);
    DeleteDC(mem);
    ReleaseDC(std::ptr::null_mut(), screen);
    let rgba: Vec<u8> = buf.chunks(4).flat_map(|px| [px[2], px[1], px[0], 255]).collect();
    image::RgbaImage::from_raw(w as u32, h as u32, rgba).ok_or("image")?.save(path).map_err(|e| e.to_string())
}

/// QA: the Excel export without the file dialog.
pub unsafe fn results_xlsx(path: &Path) -> Result<(), String> {
    let (t, c) = results_table();
    let rep = results::view(|v| results::report(v, &job_name())).ok_or("no plan")?;
    write_results_xlsx(path, &t, c, &rep)
}

pub unsafe fn print_statistics(sel: &[bool; 6]) {
    let titles = [i18n::tr("Pieces used"), i18n::tr("Bars used"), i18n::tr("Offcuts"), i18n::tr("Scrap"), i18n::tr("By material"), i18n::tr("Summary")];
    let Some(p) = begin_print("Neramit Cut") else { return };
    let mf = main_form();
    for (i, name) in app::STAT_GRIDS.iter().enumerate() {
        if !sel[i] {
            continue;
        }
        let h = mf.hwnd_of(name);
        print_rows(&p, &titles[i], &grid_table(h), &grid_widths(h));
    }
    end_print(p);
}

/// Labels on sheets: `rows` x `cols` per page inside the paper margins (percent of the page).
pub unsafe fn print_labels(o: &LabelOptions, items: &[LabelItem]) {
    if items.is_empty() {
        return;
    }
    let Some(p) = begin_print(&i18n::t("adhesive_labels")) else { return };
    let (ml, mt, mr, mb) = (p.w * o.margins[0] / 100, p.h * o.margins[1] / 100, p.w * o.margins[2] / 100, p.h * o.margins[3] / 100);
    let (aw, ah) = (p.w - ml - mr, p.h - mt - mb);
    let (cw, ch) = (aw / o.cols.max(1), ah / o.rows.max(1));
    let per = (o.rows * o.cols).max(1) as usize;
    let normal = FontSpec::points(&o.font.name, o.font.size.max(6));
    let mut big = FontSpec::points(&o.font.name, (o.font.size + 4).max(10));
    big.bold = true;
    for (k, chunk) in items.chunks(per).enumerate() {
        let _ = k;
        page(&p);
        for (i, it) in chunk.iter().enumerate() {
            let (r, c) = ((i as i32) / o.cols, (i as i32) % o.cols);
            let cell = rect(ml + c * cw, mt + r * ch, cw, ch);
            let pad_x = cw * o.inner / 100;
            let pad_y = ch * o.inner / 100;
            let inner = RECT { left: cell.left + pad_x, top: cell.top + pad_y, right: cell.right - pad_x, bottom: cell.bottom - pad_y };
            let lh_big = text_height(&big);
            let lh = text_height(&normal);
            let total: i32 = it.lines.iter().enumerate().map(|(n, _)| if n == 0 { lh_big } else { lh }).sum();
            let mut y = inner.top + ((inner.bottom - inner.top) - total).max(0) / 2;
            for (n, l) in it.lines.iter().enumerate() {
                let fs = if n == 0 { &big } else { &normal };
                SelectObject(p.dc, font(fs) as _);
                let hgt = if n == 0 { lh_big } else { lh };
                let mut r = RECT { left: inner.left, top: y, right: inner.right, bottom: y + hgt };
                draw_text(p.dc, l, &mut r, DT_CENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
                y += hgt;
            }
        }
        end_page(&p);
    }
    end_print(p);
}

#[allow(dead_code)]
fn unused(_: PathBuf) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_by_header() {
        let t = vec![vec!["Qty".to_string(), "Length".into(), "Material".into()], vec!["2".into(), "100".into(), "steel".into()]];
        let (h, m) = guess_columns(&t, &["length", "width", "quantity", "material"]);
        assert!(h);
        assert_eq!(m["length"], Some(1));
        assert_eq!(m["quantity"], Some(0));
        assert_eq!(m["width"], None);
    }

    #[test]
    fn formula_column_stays_text_in_excel() {
        use calamine::{open_workbook_auto, Data, Reader};
        let dir = std::env::temp_dir().join("neramitcut_xlsx_test");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("plan.xlsx");
        let rows = vec![vec!["Length".to_string(), "Cutting formula".into()], vec!["2800".into(), "457+457".into()], vec!["3000".into(), "3000".into()]];
        write_table_typed(&p, &rows, &[1]).unwrap();
        let mut wb = open_workbook_auto(&p).unwrap();
        let name = wb.sheet_names()[0].clone();
        let r = wb.worksheet_range(&name).unwrap();
        assert_eq!(r.get((1, 1)), Some(&Data::String("457+457".into())));
        assert_eq!(r.get((2, 1)), Some(&Data::String("3000".into())));
        assert!(matches!(r.get((1, 0)), Some(Data::Float(_))));
    }

    #[test]
    fn columns_by_position() {
        let t = vec![vec!["100".to_string(), "0".into(), "2".into()]];
        let (h, m) = guess_columns(&t, &["length", "width", "quantity"]);
        assert!(!h);
        assert_eq!(m["quantity"], Some(2));
    }

    #[test]
    fn dxf_has_entities() {
        let prj = Project::sample();
        let sol = crate::engine::optimize(&prj, &crate::engine::Progress { cancel: &|| false, report: &|_, _| {} });
        let v = results::View { max_len: 3000.0, sol, prj, iface: Interface::default() };
        let o = DxfOptions { unit: 4, font_height: 50.0, layer_text: "0".into(), layer_bars: "0".into(), layer_parts: "0".into(), layer_waste: "0".into(), bar_contour: true, bar_info: true, parts_contour: true, parts_size: true, parts_label: true, customer: false, angles: false, waste_contour: true, waste_size: true };
        let s = dxf_text(&v, &o, 3);
        assert!(s.starts_with("0\nSECTION"));
        assert!(s.contains("LINE"));
        assert!(s.ends_with("EOF\n"));
    }
}
