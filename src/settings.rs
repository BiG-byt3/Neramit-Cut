//! Settings files: a working folder (default
//! Documents\NeramitCut) holding interface.xml, technical.xml, materials.xml and the parts,
//! projects, results and history subfolders. The chosen working folder itself is remembered in
//! %APPDATA%\NeramitCut\working_dir.txt. Folders of DOi Cut1D (the program this one was copied from, and
//! its former names ViBEcut1D, BrixCut1D) are copied over on the first start.

use crate::model::*;
use crate::xmlio::{self, Node, W};
use std::path::{Path, PathBuf};

fn documents() -> PathBuf {
    unsafe {
        let mut buf = [0u16; 260];
        // CSIDL_PERSONAL
        if windows_sys::Win32::UI::Shell::SHGetFolderPathW(std::ptr::null_mut(), 5, std::ptr::null_mut(), 0, buf.as_mut_ptr()) == 0 {
            let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
            return PathBuf::from(String::from_utf16_lossy(&buf[..n]));
        }
    }
    std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Documents")).unwrap_or_else(|| PathBuf::from("."))
}

fn app_data_root() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn app_data() -> PathBuf {
    app_data_root().join("NeramitCut")
}

pub fn default_working_dir() -> PathBuf {
    documents().join("NeramitCut")
}

/// Copy a folder tree; files already in `to` are kept.
pub fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let dst = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_tree(&e.path(), &dst)?;
        } else if !dst.exists() {
            std::fs::copy(e.path(), &dst)?;
        }
    }
    Ok(())
}

/// First start under the name NeramitCut: copy the settings and the default working folder of
/// DOi Cut1D (else its former names ViBEcut1D, BrixCut1D); the old folders stay untouched.
const LEGACY_NAMES: [&str; 3] = ["DOiCut1D", "ViBEcut1D", "BrixCut1D"];

pub fn migrate_legacy() {
    if std::env::var_os("NERAMIT_WORKDIR").is_some() {
        return;
    }
    let new_app = app_data();
    if !new_app.exists() {
        if let Some(old) = LEGACY_NAMES.iter().map(|n| app_data_root().join(n)).find(|p| p.is_dir()) {
            let _ = copy_tree(&old, &new_app);
            // a working folder left at an old default moves with the rename
            let wd = new_app.join("working_dir.txt");
            if let Ok(s) = std::fs::read_to_string(&wd) {
                if LEGACY_NAMES.iter().any(|n| Path::new(s.trim()) == documents().join(n)) {
                    let _ = std::fs::write(&wd, default_working_dir().to_string_lossy().as_bytes());
                }
            }
        }
    }
    let new_wd = default_working_dir();
    if !new_wd.exists() {
        if let Some(old) = LEGACY_NAMES.iter().map(|n| documents().join(n)).find(|p| p.is_dir()) {
            let _ = copy_tree(&old, &new_wd);
        }
    }
}

pub fn working_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("NERAMIT_WORKDIR") {
        return PathBuf::from(p);
    }
    if let Ok(s) = std::fs::read_to_string(app_data().join("working_dir.txt")) {
        let p = PathBuf::from(s.trim());
        if !s.trim().is_empty() && p.is_dir() {
            return p;
        }
    }
    default_working_dir()
}

pub fn set_working_dir(p: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(app_data())?;
    std::fs::write(app_data().join("working_dir.txt"), p.to_string_lossy().as_bytes())
}

pub fn dir(sub: &str) -> PathBuf {
    let d = if sub.is_empty() { working_dir() } else { working_dir().join(sub) };
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn write_file(path: &Path, text: &str) -> Result<(), String> {
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let tmp = path.with_extension("tmp~");
    std::fs::write(&tmp, text.as_bytes()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn read_file(path: &Path) -> Result<String, String> {
    let b = std::fs::read(path).map_err(|e| e.to_string())?;
    let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&b);
    Ok(String::from_utf8_lossy(b).into_owned())
}

// ------------------------------------------------------------------ interface.xml

fn write_font(w: &mut W, name: &str, f: &FontPrefs) {
    w.open(name);
    w.val("name", &f.name);
    w.int("size", f.size as i64);
    w.b("bold", f.bold);
    w.b("italic", f.italic);
    w.close(name);
}

fn read_font(n: Option<&Node>, d: &FontPrefs) -> FontPrefs {
    match n {
        Some(n) => FontPrefs { name: n.get(&["name"]).filter(|s| !s.is_empty()).unwrap_or(&d.name).to_string(), size: n.i32(&["size"], d.size).clamp(4, 72), bold: n.bool(&["bold"], d.bold), italic: n.bool(&["italic"], d.italic) },
        None => d.clone(),
    }
}

fn write_cols(w: &mut W, name: &str, cols: &[ColumnLayout]) {
    w.open(name);
    for c in cols {
        w.open("column");
        w.val("key", &c.key);
        w.int("index", c.index as i64);
        w.b("visible", c.visible);
        w.int("width", c.width as i64);
        w.close("column");
    }
    w.close(name);
}

fn read_cols(n: Option<&Node>, d: &[ColumnLayout]) -> Vec<ColumnLayout> {
    let mut out = d.to_vec();
    if let Some(n) = n {
        for c in n.children("column") {
            let key = c.str(&["key"]);
            if let Some(o) = out.iter_mut().find(|o| o.key == key) {
                o.visible = c.bool(&["visible"], o.visible);
                o.width = c.i32(&["width"], o.width).clamp(8, 4000);
                o.index = c.i32(&["index"], o.index);
            }
        }
    }
    out
}

pub fn interface_doc(i: &Interface) -> String {
    let mut w = W::new("data");
    w.open("interface");
    w.val("language", &i.language);
    write_font(&mut w, "font", &i.font);
    write_font(&mut w, "font_drawing", &i.font_print);
    write_font(&mut w, "font_grid", &i.font_grid);
    w.int("num_digits", i.num_digits as i64);
    w.int("num_digits_percent", i.num_digits_percent as i64);
    w.int("num_digits_price", i.num_digits_price as i64);
    w.val("header", &i.header);
    w.open("company");
    w.val("name", &i.company.name);
    w.val("address", &i.company.address);
    w.val("address2", &i.company.address2);
    w.val("city", &i.company.city);
    w.val("zip", &i.company.zip);
    w.val("phone", &i.company.phone);
    w.val("email", &i.company.email);
    w.val("logo", &i.company.logo);
    w.close("company");
    w.b("two_panels", i.two_panels);
    w.int("parts_panel_height", i.parts_panel_height as i64);
    w.int("basic_data_width", i.basic_data_width as i64);
    w.num("basic_data_ratio", i.basic_data_ratio);
    w.num("parts_panel_ratio", i.parts_ratio);
    w.int("window_width", i.window_w as i64);
    w.int("window_height", i.window_h as i64);
    w.int("window_left", i.window_x as i64);
    w.int("window_top", i.window_y as i64);
    w.b("window_maximized", i.window_max);
    w.b("group_parts_after_loading", i.group_parts_after_loading);
    w.b("show_piece_size", i.show_piece_size);
    w.b("show_piece_label", i.show_piece_label);
    w.b("show_waste_size", i.show_waste_size);
    w.b("as_text", i.as_text);
    w.b("show_customer", i.show_customer);
    w.b("show_angles", i.show_angles);
    w.b("group_identical", i.group_identical);
    w.b("fixed_height", i.fixed_height);
    write_cols(&mut w, "parts_columns", &i.parts_cols);
    write_cols(&mut w, "stock_columns", &i.stock_cols);
    write_cols(&mut w, "results_columns", &i.results_cols);
    w.int("layout_version", i.layout_version as i64);
    w.close("interface");
    w.end_root("data")
}

fn read_company(n: &Node) -> Company {
    let co = n.child("company");
    let cs = |k: &str| co.map(|c| c.str(&[k])).unwrap_or_default();
    Company { name: cs("name"), address: cs("address"), address2: cs("address2"), city: cs("city"), zip: cs("zip"), phone: cs("phone"), email: cs("email"), logo: cs("logo") }
}

pub fn read_interface_doc(text: &str) -> Interface {
    let d = Interface::default();
    let Ok(root) = xmlio::parse(text) else { return d };
    let Some(n) = root.child("interface") else { return d };
    // settings of the 1.x classic look: keep the user's numbers, start the new layout
    // and fonts fresh
    let version = n.i32(&["layout_version"], 1);
    if version < 2 {
        return Interface {
            num_digits: n.i32(&["num_digits"], d.num_digits).clamp(0, 10),
            num_digits_percent: n.i32(&["num_digits_percent"], d.num_digits_percent).clamp(0, 10),
            num_digits_price: n.i32(&["num_digits_price"], d.num_digits_price).clamp(0, 10),
            group_parts_after_loading: n.bool(&["group_parts_after_loading"], d.group_parts_after_loading),
            company: read_company(n),
            ..d
        };
    }

    Interface {
        language: n.get(&["language"]).map(String::from).unwrap_or(d.language.clone()),
        font: read_font(n.child("font"), &d.font),
        font_print: read_font(n.child("font_drawing"), &d.font_print),
        font_grid: read_font(n.child("font_grid"), &d.font_grid),
        num_digits: n.i32(&["num_digits"], d.num_digits).clamp(0, 10),
        num_digits_percent: n.i32(&["num_digits_percent"], d.num_digits_percent).clamp(0, 10),
        num_digits_price: n.i32(&["num_digits_price"], d.num_digits_price).clamp(0, 10),
        header: n.str(&["header"]),
        company: read_company(n),
        two_panels: n.bool(&["two_panels"], d.two_panels),
        group_parts_after_loading: n.bool(&["group_parts_after_loading"], d.group_parts_after_loading),
        show_piece_size: n.bool(&["show_piece_size"], d.show_piece_size),
        show_piece_label: n.bool(&["show_piece_label"], d.show_piece_label),
        show_waste_size: n.bool(&["show_waste_size"], d.show_waste_size),
        as_text: n.bool(&["as_text"], d.as_text),
        show_customer: n.bool(&["show_customer"], d.show_customer),
        show_angles: n.bool(&["show_angles"], d.show_angles),
        group_identical: n.bool(&["group_identical"], d.group_identical),
        fixed_height: n.bool(&["fixed_height"], d.fixed_height),
        parts_cols: read_cols(n.child("parts_columns"), &d.parts_cols),
        stock_cols: read_cols(n.child("stock_columns"), &d.stock_cols),
        results_cols: if version < 3 { d.results_cols.clone() } else { read_cols(n.child("results_columns"), &d.results_cols) },
        // before 4 the panels had fixed sizes; the new design starts balanced
        parts_panel_height: if version < 4 { 0 } else { n.i32(&["parts_panel_height"], d.parts_panel_height) },
        basic_data_width: if version < 4 { 0 } else { n.i32(&["basic_data_width"], d.basic_data_width) },
        // 5: the cutting plan got a wider default share
        basic_data_ratio: if version < 5 { 0.0 } else { n.f64(&["basic_data_ratio"], 0.0).clamp(0.0, 0.9) },
        parts_ratio: n.f64(&["parts_panel_ratio"], 0.0).clamp(0.0, 0.9),
        window_w: n.i32(&["window_width"], 0).clamp(0, 20000),
        window_h: n.i32(&["window_height"], 0).clamp(0, 20000),
        window_x: n.i32(&["window_left"], 0),
        window_y: n.i32(&["window_top"], 0),
        window_max: n.bool(&["window_maximized"], false),
        layout_version: LAYOUT_VERSION,
    }
}

pub fn load_interface() -> Interface {
    read_file(&dir("").join("interface.xml")).map(|t| read_interface_doc(&t)).unwrap_or_default()
}

pub fn save_interface(i: &Interface) {
    let _ = write_file(&dir("").join("interface.xml"), &interface_doc(i));
}

pub fn load_technical() -> Technical {
    read_file(&dir("").join("technical.xml")).ok().and_then(|t| xmlio::read_technical_doc(&t).ok()).unwrap_or_default()
}

pub fn save_technical(t: &Technical) {
    let _ = write_file(&dir("").join("technical.xml"), &xmlio::technical_doc(t));
}

pub fn load_materials() -> Vec<Material> {
    let mut list = read_file(&dir("").join("materials.xml")).ok().and_then(|t| xmlio::read_materials_doc(&t).ok()).unwrap_or_else(|| REBAR_MATERIALS.iter().map(|m| Material::named(m)).collect());
    list.retain(|m| m.name != OLD_SAMPLE_MATERIAL);
    list
}

pub fn save_materials(m: &[Material]) {
    let _ = write_file(&dir("").join("materials.xml"), &xmlio::materials_doc(m));
}

// ------------------------------------------------------------------ stock history

/// Stock snapshots taken before a solution is accepted (Inventory history window).
pub fn save_history(stock: &[Stock]) {
    let name = format!("{}.xml", now_stamp().replace(['/', ':'], "").replace('-', "_"));
    let _ = write_file(&dir("history").join(name), &xmlio::stock_doc(stock));
}

pub fn list_history() -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(dir("history"))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "xml").unwrap_or(false))
                .map(|p| {
                    let s = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    // yyyymmdd_hhmmss -> yyyy/mm/dd-hh:mm:ss
                    let label = if s.len() == 15 { format!("{}/{}/{}-{}:{}:{}", &s[0..4], &s[4..6], &s[6..8], &s[9..11], &s[11..13], &s[13..15]) } else { s };
                    (label, p)
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v
}

// ------------------------------------------------------------------ latest solutions

pub fn save_run(prj: &Project, sol: &Solution) -> Option<PathBuf> {
    let name = format!("{}.xml", now_stamp().replace(['/', ':'], "").replace('-', "_"));
    let p = dir("results").join(name);
    let doc = xmlio::project_doc(prj, Some(sol), xmlio::Sections::all(), env!("CARGO_PKG_VERSION"));
    write_file(&p, &doc).ok()?;
    // keep the 20 most recent
    let mut all = list_runs();
    if all.len() > 20 {
        for (_, old) in all.split_off(20) {
            let _ = std::fs::remove_file(old);
        }
    }
    Some(p)
}

pub fn list_runs() -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(dir("results"))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "xml").unwrap_or(false))
                .map(|p| {
                    let s = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    let label = if s.len() == 15 { format!("{}/{}/{} {}:{}:{}", &s[0..4], &s[4..6], &s[6..8], &s[9..11], &s[11..13], &s[13..15]) } else { s };
                    (label, p)
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v
}

/// Local time as "YYYY/MM/DD-HH:MM:SS".
pub fn now_stamp() -> String {
    unsafe {
        let mut st: windows_sys::Win32::Foundation::SYSTEMTIME = std::mem::zeroed();
        windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut st);
        format!("{:04}/{:02}/{:02}-{:02}:{:02}:{:02}", st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond)
    }
}

pub fn today() -> String {
    unsafe {
        let mut st: windows_sys::Win32::Foundation::SYSTEMTIME = std::mem::zeroed();
        windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut st);
        format!("{:02}.{:02}.{:04}", st.wDay, st.wMonth, st.wYear)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_roundtrip() {
        let mut i = Interface::default();
        i.language = "Thai".into();
        i.company.name = "BiGByte & Co <test>".into();
        i.results_cols[1].visible = true;
        assert_eq!(read_interface_doc(&interface_doc(&i)), i);
    }

    #[test]
    fn layout_2_keeps_settings_but_resets_result_columns() {
        let mut i = Interface::default();
        i.font_grid.size = 12;
        i.results_cols[2].width = 300;
        i.layout_version = 2;
        let back = read_interface_doc(&interface_doc(&i));
        assert_eq!(back.font_grid.size, 12);
        assert_eq!(back.results_cols, Interface::default().results_cols);
    }

    #[test]
    fn layout_3_starts_with_balanced_panels() {
        let mut i = Interface::default();
        i.basic_data_width = 700;
        i.parts_panel_height = 300;
        i.layout_version = 3;
        let back = read_interface_doc(&interface_doc(&i));
        assert_eq!((back.basic_data_width, back.parts_panel_height), (0, 0));
        i.layout_version = LAYOUT_VERSION;
        let back = read_interface_doc(&interface_doc(&i));
        assert_eq!((back.basic_data_width, back.parts_panel_height), (700, 300));
        i.basic_data_ratio = 0.55;
        i.parts_ratio = 0.4;
        (i.window_w, i.window_h, i.window_x, i.window_y, i.window_max) = (1320, 743, -8, 120, true);
        let back = read_interface_doc(&interface_doc(&i));
        assert_eq!((back.basic_data_ratio, back.parts_ratio), (0.55, 0.4));
        assert_eq!((back.window_w, back.window_h, back.window_x, back.window_y, back.window_max), (1320, 743, -8, 120, true));
    }

    #[test]
    fn copy_tree_keeps_existing_files() {
        let base = std::env::temp_dir().join("neramitcut_copy_tree_test");
        let _ = std::fs::remove_dir_all(&base);
        let (a, b) = (base.join("old"), base.join("new"));
        std::fs::create_dir_all(a.join("parts")).unwrap();
        std::fs::write(a.join("interface.xml"), "old").unwrap();
        std::fs::write(a.join("parts").join("p.xml"), "part").unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("interface.xml"), "new").unwrap();
        copy_tree(&a, &b).unwrap();
        assert_eq!(std::fs::read_to_string(b.join("interface.xml")).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(b.join("parts").join("p.xml")).unwrap(), "part");
        let _ = std::fs::remove_dir_all(&base);
    }
}
