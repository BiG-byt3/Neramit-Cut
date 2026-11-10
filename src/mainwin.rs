//! Parts and stock tables of the main window: columns, grid <-> model, validation, editing
//! commands (append, delete, clear, group, sort).

use crate::i18n;
use crate::model::*;
use crate::ui::grid::{self, Column, Grid};
use crate::ui::theme;
use crate::ui::win::*;

/// Body row height of the data tables (96 DPI).
pub const ROW_H: i32 = 36;
use windows_sys::Win32::Foundation::HWND;

pub const PARTS_KEYS: [&str; 9] = ["index", "length", "quantity", "width", "material", "label", "a1", "a2", "customer"];
pub const STOCK_KEYS: [&str; 10] = ["index", "length", "quantity", "width", "material", "price", "label", "priority", "a1", "a2"];

pub fn col_title(key: &str) -> String {
    match key {
        "index" => "#".into(),
        "length" => i18n::t("col_length"),
        "quantity" => i18n::t("col_quantity"),
        "width" => i18n::t("col_height"),
        "material" => i18n::t("col_material"),
        "label" => i18n::t("col_label"),
        "a1" => format!("{} 1", i18n::t("col_angle")),
        "a2" => format!("{} 2", i18n::t("col_angle")),
        "customer" => i18n::t("col_customer"),
        "price" => i18n::t("col_price"),
        "weight" => i18n::t("col_weight_m"),
        "priority" => i18n::t("col_priority"),
        "image" => i18n::t("col_image"),
        "waste" => i18n::t("col_waste"),
        "waste_percent" => format!("{} %", i18n::t("col_waste")),
        "mass" => i18n::t("col_mass"),
        "area" => i18n::t("col_area"),
        "pieces" => i18n::t("col_pieces"),
        "pattern" => i18n::t("col_pattern"),
        _ => key.to_string(),
    }
}

/// Numbers are right-aligned.
fn align_of(key: &str) -> i32 {
    match key {
        "index" | "length" | "quantity" | "width" | "a1" | "a2" | "price" | "weight" | "priority" | "waste" | "waste_percent" | "mass" | "area" => 2,
        _ => 0,
    }
}

/// Grid fonts and sizes: the table font of the settings for the cells, a single-line
/// semibold header.
pub fn style_grid(g: &mut Grid, iface: &Interface, body_h: i32) {
    let s = theme::scale();
    let mut body = FontSpec::new(&iface.font_grid.name, -((iface.font_grid.size as f64 * 96.0 / 72.0 * s).round() as i32));
    body.bold = iface.font_grid.bold;
    body.italic = iface.font_grid.italic;
    body.color = theme::TEXT;
    g.font = body;
    g.header_font = theme::font_semibold(theme::SMALL);
    g.header_wrap = false;
    g.header_h = theme::px(38);
    g.row_h = theme::px(body_h);
    let n = g.rows.len();
    g.row_heights = vec![g.row_h; n];
    g.fixed_color = theme::SURFACE_ALT;
}

pub fn make_columns(layout: &[ColumnLayout], check_index: bool) -> Vec<Column> {
    let mut v: Vec<&ColumnLayout> = layout.iter().collect();
    v.sort_by_key(|c| c.index);
    v.iter()
        .map(|c| {
            let mut col = Column::new(&c.key, &col_title(&c.key), theme::px(c.width));
            col.align = align_of(&c.key);
            col.visible = c.visible;
            col.check = check_index && c.key == "index";
            col.readonly = c.key == "index" || c.key == "image" || c.key == "pieces";
            col
        })
        .collect()
}

pub fn retitle(g: &mut Grid) {
    for c in g.cols.iter_mut() {
        c.title = col_title(&c.key);
    }
    g.invalidate();
}

/// Column widths/visibility back into the saved layout.
pub fn save_layout(g: &Grid, layout: &mut [ColumnLayout]) {
    for (i, c) in g.cols.iter().enumerate() {
        if let Some(l) = layout.iter_mut().find(|l| l.key == c.key) {
            l.width = (c.width as f64 / theme::scale()).round() as i32;
            l.visible = c.visible;
            l.index = i as i32;
        }
    }
}

pub fn col_index(g: &Grid, key: &str) -> Option<usize> {
    g.cols.iter().position(|c| c.key == key)
}

pub fn set_col_visible(g: &mut Grid, key: &str, v: bool) {
    if let Some(i) = col_index(g, key) {
        g.cols[i].visible = v;
    }
}

// ------------------------------------------------------------------ rows

fn num(v: f64, d: i32) -> String {
    fmt_num(v, d)
}

pub fn part_cells(g: &Grid, p: &Part, index: usize, d: i32) -> Vec<String> {
    g.cols
        .iter()
        .map(|c| match c.key.as_str() {
            "index" => index.to_string(),
            "length" => num(p.length, d),
            "quantity" => p.quantity.to_string(),
            "width" => num(p.height, d),
            "material" => p.material.clone(),
            "label" => p.label.clone(),
            "a1" => num(p.a1, d),
            "a2" => num(p.a2, d),
            "customer" => p.customer.clone(),
            _ => String::new(),
        })
        .collect()
}

pub fn stock_cells(g: &Grid, s: &Stock, index: usize, d: i32, dp: i32) -> Vec<String> {
    g.cols
        .iter()
        .map(|c| match c.key.as_str() {
            "index" => index.to_string(),
            "length" => num(s.length, d),
            "quantity" => s.quantity.to_string(),
            "width" => num(s.height, d),
            "material" => s.material.clone(),
            "price" => num(s.price, dp),
            "weight" => num(s.weight, 3),
            "label" => s.label.clone(),
            "priority" => s.priority.to_string(),
            "a1" => num(s.a1, d),
            "a2" => num(s.a2, d),
            _ => String::new(),
        })
        .collect()
}

pub fn fill_parts(h: HWND, parts: &[Part], d: i32) {
    grid::with(h, |g| {
        let rows: Vec<Vec<String>> = parts.iter().enumerate().map(|(i, p)| part_cells(g, p, i + 1, d)).collect();
        let checked = parts.iter().map(|p| p.use_it).collect();
        g.set_rows(rows, checked);
    });
}

pub fn fill_stock(h: HWND, stock: &[Stock], d: i32, dp: i32) {
    grid::with(h, |g| {
        let rows: Vec<Vec<String>> = stock.iter().enumerate().map(|(i, s)| stock_cells(g, s, i + 1, d, dp)).collect();
        let checked = stock.iter().map(|s| s.use_it).collect();
        g.set_rows(rows, checked);
    });
}

#[derive(Debug, Clone)]
pub struct CellError {
    pub row: usize,
    pub col: usize,
    pub msg: String,
}

fn too_big(v: f64) -> bool {
    v.abs() > 1.0e9
}

/// Validate one cell value. Returns the message for an invalid value.
pub fn check_cell(key: &str, text: &str) -> Option<String> {
    let t = text.trim();
    let number = |positive: bool, allow_empty: bool| -> Option<String> {
        if t.is_empty() {
            return if allow_empty { None } else { Some(i18n::t("must_be_positive")) };
        }
        match parse_num(t) {
            None => Some(i18n::t("not_a_number")),
            Some(v) if too_big(v) => Some(i18n::t("number_too_big")),
            Some(v) if positive && v <= 0.0 => Some(i18n::t("must_be_positive")),
            Some(v) if v < 0.0 => Some(i18n::t("must_not_be_negative")),
            _ => None,
        }
    };
    let integer = |min: i64| -> Option<String> {
        if t.is_empty() {
            return None;
        }
        match t.parse::<i64>() {
            Err(_) => Some(i18n::t("not_an_integer")),
            Ok(v) if v as f64 > 1.0e9 => Some(i18n::t("number_too_big")),
            Ok(v) if v < min => Some(if min > 0 { i18n::t("must_be_positive") } else { i18n::t("must_not_be_negative") }),
            _ => None,
        }
    };
    match key {
        "length" => number(true, true),
        "width" | "price" | "weight" => number(false, true),
        "quantity" => integer(1),
        "priority" => integer(0),
        "a1" | "a2" => {
            if t.is_empty() {
                return None;
            }
            match parse_num(t) {
                Some(v) if v > 0.0 && v < 180.0 => None,
                Some(_) => Some(i18n::t("invalid_piece")),
                None => Some(i18n::t("not_an_integer")),
            }
        }
        _ => None,
    }
}

fn row_empty(g: &Grid, r: usize) -> bool {
    g.cols.iter().enumerate().all(|(c, col)| col.key == "index" || g.cell(r, c).trim().is_empty())
}

fn cell_by_key<'a>(g: &'a Grid, r: usize, key: &str) -> &'a str {
    match col_index(g, key) {
        Some(c) => g.cell(r, c).trim(),
        None => "",
    }
}

fn read_row_common(g: &Grid, r: usize) -> Result<(f64, i32, f64, f64, f64), CellError> {
    for (c, col) in g.cols.iter().enumerate() {
        if let Some(msg) = check_cell(&col.key, g.cell(r, c)) {
            return Err(CellError { row: r, col: c, msg });
        }
    }
    let n = |k: &str, d: f64| parse_num(cell_by_key(g, r, k)).filter(|_| !cell_by_key(g, r, k).is_empty()).unwrap_or(d);
    let length = n("length", 0.0);
    if length <= 0.0 {
        return Err(CellError { row: r, col: col_index(g, "length").unwrap_or(1), msg: i18n::t("must_be_positive") });
    }
    let q = cell_by_key(g, r, "quantity").parse::<i32>().unwrap_or(1);
    Ok((length, q, n("width", 0.0), n("a1", 90.0), n("a2", 90.0)))
}

/// Parts from the grid (empty rows are skipped).
pub fn read_parts(g: &Grid) -> Result<Vec<Part>, CellError> {
    let mut out = vec![];
    for r in 1..=g.rows.len() {
        if row_empty(g, r) {
            continue;
        }
        let (length, quantity, height, a1, a2) = read_row_common(g, r)?;
        out.push(Part {
            length,
            height,
            quantity,
            label: cell_by_key(g, r, "label").to_string(),
            material: cell_by_key(g, r, "material").to_string(),
            customer: cell_by_key(g, r, "customer").to_string(),
            color: 16777215,
            id: cell_by_key(g, r, "index").parse().unwrap_or(r as i32),
            use_it: *g.checked.get(r - 1).unwrap_or(&true),
            a1,
            a2,
        });
    }
    Ok(out)
}

pub fn read_stock(g: &Grid) -> Result<Vec<Stock>, CellError> {
    let mut out = vec![];
    for r in 1..=g.rows.len() {
        if row_empty(g, r) {
            continue;
        }
        let (length, quantity, height, a1, a2) = read_row_common(g, r)?;
        out.push(Stock {
            length,
            height,
            quantity,
            label: cell_by_key(g, r, "label").to_string(),
            material: cell_by_key(g, r, "material").to_string(),
            price: parse_num(cell_by_key(g, r, "price")).unwrap_or(0.0),
            priority: cell_by_key(g, r, "priority").parse().unwrap_or(0),
            weight: parse_num(cell_by_key(g, r, "weight")).unwrap_or(0.0).max(0.0),
            id: cell_by_key(g, r, "index").parse().unwrap_or(r as i32),
            use_it: *g.checked.get(r - 1).unwrap_or(&true),
            a1,
            a2,
        });
    }
    Ok(out)
}

/// Parts ignoring invalid rows (used for saving while editing).
pub fn read_parts_lenient(g: &Grid) -> Vec<Part> {
    let mut out = vec![];
    for r in 1..=g.rows.len() {
        if row_empty(g, r) {
            continue;
        }
        if let Ok((length, quantity, height, a1, a2)) = read_row_common(g, r) {
            out.push(Part {
                length,
                height,
                quantity,
                label: cell_by_key(g, r, "label").to_string(),
                material: cell_by_key(g, r, "material").to_string(),
                customer: cell_by_key(g, r, "customer").to_string(),
                color: 16777215,
                id: cell_by_key(g, r, "index").parse().unwrap_or(r as i32),
                use_it: *g.checked.get(r - 1).unwrap_or(&true),
                a1,
                a2,
            });
        }
    }
    out
}

pub fn read_stock_lenient(g: &Grid) -> Vec<Stock> {
    (1..=g.rows.len())
        .filter(|&r| !row_empty(g, r))
        .filter_map(|r| {
            let (length, quantity, height, a1, a2) = read_row_common(g, r).ok()?;
            Some(Stock {
                length,
                height,
                quantity,
                label: cell_by_key(g, r, "label").to_string(),
                material: cell_by_key(g, r, "material").to_string(),
                price: parse_num(cell_by_key(g, r, "price")).unwrap_or(0.0),
                weight: parse_num(cell_by_key(g, r, "weight")).unwrap_or(0.0).max(0.0),
                priority: cell_by_key(g, r, "priority").parse().unwrap_or(0),
                id: cell_by_key(g, r, "index").parse().unwrap_or(r as i32),
                use_it: *g.checked.get(r - 1).unwrap_or(&true),
                a1,
                a2,
            })
        })
        .collect()
}

// ------------------------------------------------------------------ commands

pub fn renumber(g: &mut Grid) {
    if let Some(c) = col_index(g, "index") {
        for r in 1..=g.rows.len() {
            g.set_cell(r, c, &r.to_string());
        }
    }
}

/// Append an empty, checked row and focus its first data cell.
pub const NEW_ROW_LENGTH: &str = "8888";

pub fn append_row(g: &mut Grid) {
    g.commit_edit(true);
    let n = g.cols.len();
    let mut row = vec![String::new(); n];
    // a new row starts with length 8888, to be overwritten
    if let Some(c) = g.cols.iter().position(|c| c.key == "length") {
        row[c] = NEW_ROW_LENGTH.to_string();
    }
    g.rows.push(row);
    g.checked.push(true);
    g.row_heights.push(g.row_h);
    renumber(g);
    let first = g.cols.iter().position(|c| c.visible && !c.check && !c.readonly).unwrap_or(1);
    g.update_scroll();
    g.move_to(g.rows.len(), first);
}

pub fn delete_row(g: &mut Grid) {
    if g.row == 0 || g.row > g.rows.len() {
        return;
    }
    g.commit_edit(false);
    let r = g.row - 1;
    g.rows.remove(r);
    if r < g.checked.len() {
        g.checked.remove(r);
    }
    if r < g.row_heights.len() {
        g.row_heights.remove(r);
    }
    renumber(g);
    if g.row > g.rows.len() {
        g.row = g.rows.len();
    }
    if g.row == 0 && !g.rows.is_empty() {
        g.row = 1;
    }
    if g.top >= g.rows.len() {
        g.top = g.rows.len().saturating_sub(1);
    }
    g.update_scroll();
    g.invalidate();
}

pub fn clear(g: &mut Grid) {
    g.commit_edit(false);
    g.set_rows(vec![], vec![]);
    g.row = 0;
}

fn sort_key_num(s: &str) -> Option<f64> {
    if s.trim().is_empty() {
        None
    } else {
        parse_num(s)
    }
}

/// Sort rows by a column (numbers numerically, text case-insensitively).
pub fn sort_by(g: &mut Grid, c: usize, desc: bool) {
    g.commit_edit(true);
    let mut idx: Vec<usize> = (0..g.rows.len()).collect();
    let numeric = !matches!(g.cols[c].key.as_str(), "material" | "label" | "customer");
    let rows = &g.rows;
    idx.sort_by(|&a, &b| {
        let (x, y) = (rows[a].get(c).map(|s| s.as_str()).unwrap_or(""), rows[b].get(c).map(|s| s.as_str()).unwrap_or(""));
        let o = if numeric {
            match (sort_key_num(x), sort_key_num(y)) {
                (Some(p), Some(q)) => p.partial_cmp(&q).unwrap_or(std::cmp::Ordering::Equal),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            }
        } else {
            x.to_lowercase().cmp(&y.to_lowercase())
        };
        if desc {
            o.reverse()
        } else {
            o
        }
    });
    g.rows = idx.iter().map(|&i| g.rows[i].clone()).collect();
    g.checked = idx.iter().map(|&i| *g.checked.get(i).unwrap_or(&true)).collect();
    g.sort_col = Some(c);
    g.sort_desc = desc;
    g.invalidate();
}

/// Merge identical parts (same everything but quantity) into one row.
pub fn group_parts(parts: &[Part]) -> Vec<Part> {
    let mut out: Vec<Part> = vec![];
    for p in parts {
        match out.iter_mut().find(|o| o.length == p.length && o.height == p.height && o.material == p.material && o.label == p.label && o.customer == p.customer && o.a1 == p.a1 && o.a2 == p.a2 && o.use_it == p.use_it) {
            Some(o) => o.quantity += p.quantity,
            None => out.push(p.clone()),
        }
    }
    for (i, p) in out.iter_mut().enumerate() {
        p.id = i as i32 + 1;
    }
    out
}

pub fn group_stock(stock: &[Stock]) -> Vec<Stock> {
    let mut out: Vec<Stock> = vec![];
    for s in stock {
        match out.iter_mut().find(|o| o.length == s.length && o.height == s.height && o.material == s.material && o.label == s.label && o.price == s.price && o.priority == s.priority && o.a1 == s.a1 && o.a2 == s.a2 && o.use_it == s.use_it) {
            Some(o) => o.quantity += s.quantity,
            None => out.push(s.clone()),
        }
    }
    for (i, s) in out.iter_mut().enumerate() {
        s.id = i as i32 + 1;
    }
    out
}

/// Paste tab/newline separated text at the current cell (Excel copy).
pub fn paste(g: &mut Grid, text: &str) {
    let start_row = g.row.max(1);
    let vis: Vec<usize> = g.vis_cols().into_iter().filter(|&c| !g.cols[c].readonly && !g.cols[c].check).collect();
    let start = vis.iter().position(|&c| c == g.col).unwrap_or(0);
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let r = start_row + i;
        while g.rows.len() < r {
            let n = g.cols.len();
            g.rows.push(vec![String::new(); n]);
            g.checked.push(true);
            g.row_heights.push(g.row_h);
        }
        for (k, v) in line.split('\t').enumerate() {
            if let Some(&c) = vis.get(start + k) {
                g.set_cell(r, c, v.trim());
            }
        }
    }
    renumber(g);
    g.update_scroll();
    g.invalidate();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouping_merges_quantities() {
        let p = Part { length: 100.0, quantity: 2, ..Part::default() };
        let q = Part { length: 100.0, quantity: 3, ..Part::default() };
        let r = Part { length: 200.0, quantity: 1, ..Part::default() };
        let g = group_parts(&[p, q, r]);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].quantity, 5);
    }

    #[test]
    fn cell_checks() {
        assert!(check_cell("length", "12,5").is_none());
        assert!(check_cell("length", "1 1/2").is_none());
        assert!(check_cell("length", "abc").is_some());
        assert!(check_cell("quantity", "0").is_some());
        assert!(check_cell("a1", "180").is_some());
        assert!(check_cell("a1", "45").is_none());
    }
}
