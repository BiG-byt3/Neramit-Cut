//! Results: the cutting-pattern table with its bar drawings, the statistics tables and
//! accepting a solution (stock and parts update with undo).

use crate::i18n;
use crate::mainwin::{col_index, col_title, make_columns, style_grid};
use crate::model::*;
use crate::ui::grid::{self, Column, Grid};
use crate::ui::theme::{self, px};
use crate::ui::win::*;

/// Row height of the cutting plan (96 DPI).
/// A result row: the data on a first line, the cutting plan across a second line.
pub const ROW_H: i32 = 86;
pub const PLAN_H: i32 = 50;
use std::cell::RefCell;
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::*;

#[derive(Clone, Default)]
pub struct View {
    pub sol: Solution,
    pub prj: Project,
    pub iface: Interface,
    pub max_len: f64,
}

thread_local! {
    static VIEW: RefCell<Option<View>> = const { RefCell::new(None) };
}

pub fn set_view(v: Option<View>) {
    VIEW.with(|s| *s.borrow_mut() = v);
}

pub fn view<R>(f: impl FnOnce(&View) -> R) -> Option<R> {
    VIEW.with(|s| s.borrow().as_ref().map(f))
}

pub fn update_iface(iface: &Interface) {
    VIEW.with(|s| {
        if let Some(v) = s.borrow_mut().as_mut() {
            v.iface = iface.clone();
        }
    });
}

// ------------------------------------------------------------------ results table

pub fn setup_results_grid(h: HWND, iface: &Interface) {
    grid::with(h, |g| {
        style_grid(g, iface, ROW_H);
        g.fixed_cols = 1;
        g.editable = false;
        g.cols = make_columns(&iface.results_cols, false);
        g.sort_col = None;
        g.sort_desc = false;
        g.cell_draw = Some(draw_cell);
        g.detail_h = px(PLAN_H);
        g.detail_draw = Some(draw_plan_line);
        g.row_height_fn = Some(result_row_height);
        // the drawing has its own line, so it is not a column; height and label are not used
        for c in g.cols.iter_mut().filter(|c| matches!(c.key.as_str(), "pieces" | "width" | "label")) {
            c.visible = false;
        }
        g.empty_text = i18n::t("empty_results");
        g.empty_icon = Some(theme::icon::CUT);
        g.fill_key = Some("pattern".into());
        g.fill_min = px(160);
    });
}

fn material_of<'a>(prj: &'a Project, name: &str) -> Option<&'a Material> {
    prj.materials.iter().find(|m| m.name == name)
}

/// Order of the result rows: most repeated patterns first, then longer bars, then more waste.
pub fn display_order(sol: &Solution) -> Vec<usize> {
    let mut v: Vec<usize> = (0..sol.bars.len()).collect();
    v.sort_by(|&a, &b| {
        let (x, y) = (&sol.bars[a], &sol.bars[b]);
        y.quantity.cmp(&x.quantity).then(y.length.partial_cmp(&x.length).unwrap_or(std::cmp::Ordering::Equal)).then(y.waste.partial_cmp(&x.waste).unwrap_or(std::cmp::Ordering::Equal))
    });
    v
}

pub fn fill_results(h: HWND) {
    let Some(v) = VIEW.with(|s| s.borrow().clone()) else {
        grid::with(h, |g| g.set_rows(vec![], vec![]));
        return;
    };
    let d = v.iface.num_digits;
    let order = display_order(&v.sol);
    grid::with(h, |g| {
        let mut rows = vec![];
        for (n, &bi) in order.iter().enumerate() {
            let b = &v.sol.bars[bi];
            let mat = material_of(&v.prj, &b.material);
            let row: Vec<String> = g
                .cols
                .iter()
                .map(|c| match c.key.as_str() {
                    "index" => (n + 1).to_string(),
                    "length" => fmt_num(b.length, d),
                    "quantity" => b.quantity.to_string(),
                    "width" => fmt_num(b.height, d),
                    "material" => b.material.clone(),
                    "label" => b.label.clone(),
                    "waste" => fmt_num(b.waste, d),
                    "waste_percent" => fmt_num(if b.length > 0.0 { (b.length - b.size_of_parts()) / b.length * 100.0 } else { 0.0 }, v.iface.num_digits_percent),
                    "price" => fmt_num(b.bar_price, v.iface.num_digits_price),
                    "mass" => fmt_num(mat.map(|m| m.mass * b.length).unwrap_or(0.0), d),
                    "area" => fmt_num(mat.map(|m| m.area * b.length).unwrap_or(0.0), d),
                    "bar" => bi.to_string(),
                    "pattern" => pattern_text(b, d),
                    _ => String::new(),
                })
                .collect();
            rows.push(row);
        }
        let n = rows.len();
        g.set_rows(rows, vec![true; n]);
        g.row = if n > 0 { 1 } else { 0 };
        g.top = 0;
        g.scroll_x = 0;
        g.update_scroll();
    });
    BAR_OF_ROW.with(|m| *m.borrow_mut() = order);
}

thread_local! {
    static BAR_OF_ROW: RefCell<Vec<usize>> = const { RefCell::new(vec![]) };
}

pub fn bar_of_row(r: usize) -> Option<usize> {
    BAR_OF_ROW.with(|m| m.borrow().get(r.wrapping_sub(1)).copied())
}

/// Re-sort the result rows by a column (header click), keeping the bar mapping.
pub fn sort_results(g: &mut Grid, c: usize, desc: bool) {
    let mut idx: Vec<usize> = (0..g.rows.len()).collect();
    let numeric = !matches!(g.cols[c].key.as_str(), "material" | "label");
    let rows = g.rows.clone();
    idx.sort_by(|&a, &b| {
        let (x, y) = (rows[a][c].as_str(), rows[b][c].as_str());
        let o = if numeric { parse_num(x).unwrap_or(0.0).partial_cmp(&parse_num(y).unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal) } else { x.to_lowercase().cmp(&y.to_lowercase()) };
        if desc {
            o.reverse()
        } else {
            o
        }
    });
    g.rows = idx.iter().map(|&i| rows[i].clone()).collect();
    BAR_OF_ROW.with(|m| {
        let old = m.borrow().clone();
        *m.borrow_mut() = idx.iter().map(|&i| old.get(i).copied().unwrap_or(0)).collect();
    });
    if let Some(ic) = col_index(g, "index") {
        for r in 1..=g.rows.len() {
            g.set_cell(r, ic, &r.to_string());
        }
    }
    g.sort_col = Some(c);
    g.sort_desc = desc;
    g.invalidate();
}

// ------------------------------------------------------------------ bar drawing

/// Grid callback: draws the "pieces" column.
fn draw_cell(g: &Grid, r: usize, c: usize, dc: HDC, inner: &RECT) -> bool {
    if r > 0 && g.cols[c].key == "pattern" {
        let text = g.cell(r, c);
        let pad = px(8);
        let tr = RECT { left: inner.left + pad, top: inner.top + px(4), right: inner.right - pad, bottom: inner.bottom - px(4) };
        unsafe { draw_formula(dc, &short_formula_tokens(text), &tr, &g.font, g.font.color) };
        return true;
    }
    let _ = (dc, inner);
    false
}

/// A result row grows by a line for each extra line the cutting formula needs (up to 4), so
/// the whole formula stays readable.
fn result_row_height(g: &Grid, r: usize) -> i32 {
    let Some(c) = g.cols.iter().position(|c| c.key == "pattern" && c.visible) else { return g.row_h };
    let avail = (g.cols[c].width - px(16)).max(1);
    let (mut lines, mut cur) = (1, 0);
    for t in short_formula_tokens(g.cell(r, c)) {
        let w = measure(&g.font, &t).cx;
        if cur > 0 && cur + w > avail {
            lines += 1;
            cur = w;
        } else {
            cur += w;
        }
    }
    g.row_h + (lines.min(4) - 1) * text_height(&g.font)
}

/// Second line of a result row: the cutting drawing across the table.
fn draw_plan_line(_g: &Grid, r: usize, dc: HDC, rr: &RECT) {
    let Some(bi) = bar_of_row(r) else { return };
    VIEW.with(|s| {
        if let Some(v) = s.borrow().as_ref() {
            if let Some(b) = v.sol.bars.get(bi) {
                // the drawing starts a little higher: the data line has room under its text
                let pr = RECT { left: rr.left, top: rr.top - px(4), right: rr.right, bottom: rr.bottom };
                unsafe { draw_bar(dc, &pr, pr.right - pr.left, b, v) };
            }
        }
    });
}

/// Cutting formula of a bar: the piece lengths in cutting order joined with "+"
/// (for example 457+457+457+457+457+457), ready to paste into Excel.
pub fn pattern_text(b: &Bar, d: i32) -> String {
    b.pieces.iter().map(|p| fmt_num(p.length, d)).collect::<Vec<_>>().join("+")
}

/// The formula split after each "+" (457+ | 457+ | 457), the places where it may wrap.
fn formula_tokens(formula: &str) -> Vec<String> {
    let parts: Vec<&str> = formula.split('+').collect();
    let n = parts.len();
    parts.iter().enumerate().filter(|(_, p)| !p.is_empty()).map(|(i, p)| if i + 1 < n { format!("{p}+") } else { p.to_string() }).collect()
}

/// The formula as it is shown on screen: runs of the same length counted
/// (650+650+457+457+457 -> 2 × 650 + 3 × 457). Copy and export keep the full formula.
fn short_formula_tokens(formula: &str) -> Vec<String> {
    let parts: Vec<&str> = formula.split('+').filter(|p| !p.is_empty()).collect();
    let mut groups: Vec<String> = vec![];
    let mut k = 0;
    while k < parts.len() {
        let mut n = 1;
        while k + n < parts.len() && parts[k + n] == parts[k] {
            n += 1;
        }
        groups.push(if n > 1 { format!("{} × {}", n, parts[k]) } else { parts[k].to_string() });
        k += n;
    }
    let m = groups.len();
    groups.into_iter().enumerate().map(|(i, g)| if i + 1 < m { format!("{g} + ") } else { g }).collect()
}

/// Draws tokens as lines that wrap between tokens, centred vertically in `r`.
/// When they need more lines than fit, the last line ends with "…".
unsafe fn draw_formula(dc: HDC, tokens: &[String], r: &RECT, f: &FontSpec, color: u32) {
    let old = SelectObject(dc, font(f) as _);
    let line_h = text_size(dc, "0+").cy.max(1);
    let max_w = (r.right - r.left).max(1);
    let max_lines = ((r.bottom - r.top) / line_h).max(1) as usize;
    let mut lines: Vec<String> = vec![];
    let mut cur = String::new();
    for t in tokens {
        let cand = format!("{cur}{t}");
        if !cur.is_empty() && text_size(dc, cand.trim_end()).cx > max_w {
            lines.push(std::mem::take(&mut cur));
            cur = t.trim_start().to_string();
        } else {
            cur = cand;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    SelectObject(dc, old);
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(l) = lines.last_mut() {
            l.push('…');
        }
    }
    let mut y = r.top + ((r.bottom - r.top) - line_h * lines.len() as i32).max(0) / 2;
    for l in &lines {
        let lr = RECT { left: r.left, top: y, right: r.right, bottom: y + line_h };
        theme::text(dc, l.trim_end(), &lr, f, color, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
        y += line_h;
    }
}

/// Cutting drawing of one pattern: a light track for the bar, rounded pastel pieces with the
/// size inside and the label under them, the offcut hatched at the end.
pub unsafe fn draw_bar(dc: HDC, inner: &RECT, col_w: i32, b: &Bar, v: &View) {
    let saved = SaveDC(dc);
    IntersectClipRect(dc, inner.left, inner.top, inner.right, inner.bottom);
    let iface = &v.iface;
    let d = iface.num_digits;
    let s = theme::scale();
    let size_font = {
        let mut f = FontSpec::new(&iface.font_print.name, -((iface.font_print.size as f64 * 96.0 / 72.0 * s).round() as i32));
        f.color = theme::TEXT;
        f
    };
    let label_font = theme::font(theme::SMALL);
    let pad_x = px(10);
    let x0 = inner.left + pad_x;
    let h_box = px(26);
    let top = inner.top + px(8);
    let bottom = top + h_box;
    if iface.as_text {
        // the formula is on the data line; here the pieces as counts: 2 × 1250 (label) · …
        let mut tokens: Vec<String> = vec![];
        let mut k = 0;
        while k < b.pieces.len() {
            let p = &b.pieces[k];
            let mut n = 1;
            while k + n < b.pieces.len() && b.pieces[k + n].length == p.length && b.pieces[k + n].label == p.label {
                n += 1;
            }
            let mut s = format!("{} × {}", n, fmt_num(p.length, d));
            if iface.show_piece_label && !p.label.is_empty() {
                s.push_str(&format!(" ({})", p.label));
            }
            tokens.push(if tokens.is_empty() { s } else { format!("   ·   {s}") });
            k += n;
        }
        if b.waste > 0.0 && iface.show_waste_size {
            tokens.push(format!("   ({} {})", i18n::t("col_waste"), fmt_num(b.waste, d)));
        }
        let tr = RECT { left: inner.left + pad_x, top: inner.top + px(4), right: inner.right - pad_x, bottom: inner.bottom - px(4) };
        draw_formula(dc, &tokens, &tr, &size_font, theme::TEXT);
        RestoreDC(dc, saved);
        return;
    }
    // each bar across the whole width, so its pieces are easy to read (its length is on the data line)
    let width = (col_w - 2 * pad_x).max(10);
    let scale = if b.length > 0.0 { width as f64 / b.length } else if v.max_len > 0.0 { width as f64 / v.max_len } else { 0.0 };
    let px_at = |x: f64| x0 + (x * scale).round() as i32;
    // bar track
    let track = RECT { left: x0, top, right: px_at(b.length), bottom };
    theme::round_rect(dc, &track, px(4) as f32, Some(theme::SURFACE_ALT), Some(theme::STROKE));
    // identical neighbours always share one box ("11 × 457"): fewer, larger boxes
    let group = true;
    let mut k = 0;
    while k < b.pieces.len() {
        let p = &b.pieces[k];
        let mut n = 1;
        if group {
            while k + n < b.pieces.len() && b.pieces[k + n].length == p.length && b.pieces[k + n].label == p.label && b.pieces[k + n].a1 == p.a1 && b.pieces[k + n].a2 == p.a2 {
                n += 1;
            }
        }
        let last = &b.pieces[k + n - 1];
        let ci = p.part % theme::PIECE_FILLS.len();
        let (fillc, strokec) = (theme::PIECE_FILLS[ci], theme::PIECE_STROKES[ci]);
        let (sb, st, eb, et) = (px_at(p.start_bottom_x), px_at(p.start_top_x), px_at(last.end_bottom_x), px_at(last.end_top_x));
        let (l, r) = (sb.min(st), eb.max(et));
        if sb == st && eb == et {
            let pr = RECT { left: l, top, right: r.max(l + 2), bottom };
            theme::round_rect(dc, &pr, px(3) as f32, Some(fillc), Some(strokec));
        } else {
            // mitred ends: trapezoid
            let brush = CreateSolidBrush(fillc);
            let pen = CreatePen(PS_SOLID, 1, strokec);
            let ob = SelectObject(dc, brush as _);
            let op = SelectObject(dc, pen as _);
            let pts = [POINT { x: st, y: top }, POINT { x: et, y: top }, POINT { x: eb, y: bottom - 1 }, POINT { x: sb, y: bottom - 1 }];
            Polygon(dc, pts.as_ptr(), 4);
            SelectObject(dc, ob);
            SelectObject(dc, op);
            DeleteObject(brush as _);
            DeleteObject(pen as _);
        }
        if iface.show_piece_size {
            let s = if n > 1 { format!("{} × {}", n, fmt_num(p.length, d)) } else { fmt_num(p.length, d) };
            let tr = RECT { left: l + px(2), top, right: r - px(2), bottom };
            if measure(&size_font, &s).cx <= r - l - px(4) {
                theme::text(dc, &s, &tr, &size_font, theme::TEXT, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            }
        }
        if iface.show_angles && (p.a1 != 90.0 || last.a2 != 90.0) {
            let small = theme::font(10);
            let ta = RECT { left: l + px(3), top: bottom - px(13), right: r - px(3), bottom };
            theme::text(dc, &format!("{}°", fmt_num(p.a1, 1)), &ta, &small, theme::TEXT_MUTED, DT_LEFT | DT_BOTTOM | DT_SINGLELINE);
            theme::text(dc, &format!("{}°", fmt_num(last.a2, 1)), &ta, &small, theme::TEXT_MUTED, DT_RIGHT | DT_BOTTOM | DT_SINGLELINE);
        }
        let mut under = String::new();
        if iface.show_piece_label {
            under.push_str(&p.label);
        }
        if iface.show_customer && !p.customer.is_empty() {
            if !under.is_empty() {
                under.push_str(" · ");
            }
            under.push_str(&p.customer);
        }
        if !under.is_empty() {
            let tr = RECT { left: l, top: bottom + px(2), right: r, bottom: inner.bottom };
            theme::text(dc, &under, &tr, &label_font, theme::TEXT_MUTED, DT_CENTER | DT_TOP | DT_SINGLELINE | DT_END_ELLIPSIS);
        }
        k += n;
    }
    // offcut
    if b.waste > 0.0 {
        let end = px_at(b.length);
        let start = end - (b.waste * scale).round() as i32;
        if end - start >= 3 {
            let wr = RECT { left: start, top, right: end, bottom };
            theme::hatch_rect(dc, &wr, px(3) as f32, rgb(0xF2, 0xB8, 0xB5), theme::DANGER_SOFT);
            theme::round_rect(dc, &wr, px(3) as f32, None, Some(rgb(0xE8, 0x9A, 0x96)));
            if iface.show_waste_size {
                let s = fmt_num(b.waste, d);
                let sw = measure(&label_font, &s).cx;
                if sw + px(6) < end - start {
                    theme::text(dc, &s, &wr, &label_font, theme::DANGER, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                } else {
                    let tr = RECT { left: start - px(40), top: bottom + px(2), right: end, bottom: inner.bottom };
                    theme::text(dc, &s, &tr, &label_font, theme::DANGER, DT_RIGHT | DT_TOP | DT_SINGLELINE);
                }
            }
        }
    }
    RestoreDC(dc, saved);
}

// ------------------------------------------------------------------ statistics

pub struct StatTables {
    pub parts: Vec<Vec<String>>,
    pub bars: Vec<Vec<String>>,
    pub reused: Vec<Vec<String>>,
    pub disposed: Vec<Vec<String>>,
    pub materials: Vec<Vec<String>>,
    pub misc: Vec<Vec<String>>,
}

/// Weight of one metre of a material: the stock row's value, else the material's, else the
/// standard rebar table (DB10 0.616 kg/m ...), else 0.
pub fn weight_per_m(prj: &Project, material: &str) -> f64 {
    prj.stock.iter().filter(|s| s.material == material && s.weight > 0.0).map(|s| s.weight).next()
        .or_else(|| prj.materials.iter().find(|m| m.name == material && m.mass > 0.0).map(|m| m.mass))
        .or_else(|| rebar_weight(material))
        .unwrap_or(0.0)
}

/// Materials in natural order (DB10, DB12, DB16, DB20, DB25: letters, then the number).
pub fn material_order(names: &mut [String]) {
    let key = |s: &String| {
        let letters: String = s.chars().take_while(|c| !c.is_ascii_digit()).collect();
        let num: f64 = s.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit() || *c == '.').collect::<String>().parse().unwrap_or(0.0);
        (letters, (num * 1000.0) as i64, s.clone())
    };
    names.sort_by_key(key);
}

/// Materials whose rows are highlighted in yellow in the printed / exported cutting plan.
/// The materials in natural order are split into two alternating groups ([1st, 3rd, 5th...]
/// and [2nd, 4th...]); the group with fewer rows in total is highlighted, so neighbouring
/// materials always differ and as little as possible is coloured.
pub fn highlighted_materials(rows_per_material: &[(String, usize)]) -> Vec<String> {
    let mut names: Vec<String> = rows_per_material.iter().map(|x| x.0.clone()).collect();
    names.dedup();
    material_order(&mut names);
    let count = |m: &String| rows_per_material.iter().filter(|x| &x.0 == m).map(|x| x.1).sum::<usize>();
    let (even, odd): (Vec<(usize, &String)>, Vec<(usize, &String)>) = names.iter().enumerate().partition(|(i, _)| i % 2 == 0);
    let se: usize = even.iter().map(|(_, m)| count(m)).sum();
    let so: usize = odd.iter().map(|(_, m)| count(m)).sum();
    let pick = if so < se && !odd.is_empty() { odd } else { even };
    pick.into_iter().map(|(_, m)| m.clone()).collect()
}

/// Bar lengths whose length cells are highlighted, chosen like the materials (alternating
/// groups of the lengths in order, the smaller group in yellow) but on their own, so the
/// length column marks a change of bar length; one length only: nothing to mark.
pub fn highlighted_lengths(lengths: &[String]) -> Vec<String> {
    let mut per: Vec<(String, usize)> = vec![];
    for l in lengths {
        match per.iter_mut().find(|x| &x.0 == l) {
            Some(x) => x.1 += 1,
            None => per.push((l.clone(), 1)),
        }
    }
    if per.len() < 2 {
        return vec![];
    }
    highlighted_materials(&per)
}

/// Statistics tables: pieces used, bars used, offcuts, by material, summary.
pub fn stat_columns(which: usize) -> Vec<Column> {
    let c = |key: &str, title: String, w: i32| {
        let mut col = Column::new(key, &title, px(w));
        col.align = if matches!(key, "material" | "label" | "customer" | "parameter") { 0 } else { 2 };
        col.readonly = true;
        col
    };
    let t = |k: &str| col_title(k);
    match which {
        0 => ["index", "length", "quantity", "material", "customer"].iter().map(|k| c(k, t(k), if *k == "index" { 56 } else { 110 })).collect(),
        1 => vec![c("index", t("index"), 56), c("length", t("length"), 110), c("quantity", t("quantity"), 110), c("material", t("material"), 110), c("price", t("price"), 110), c("weight", t("weight"), 110)],
        2 | 3 => ["index", "length", "quantity", "material"].iter().map(|k| c(k, t(k), if *k == "index" { 56 } else { 120 })).collect(),
        4 => vec![
            c("material", t("material"), 110),
            c("bars", i18n::t("bars"), 110),
            c("pieces", i18n::t("number_of_pieces"), 110),
            c("used", i18n::t("col_total_used_length"), 130),
            c("size", i18n::t("col_size_used_bars"), 130),
            c("waste", t("waste"), 110),
            c("waste_percent", format!("% {}", i18n::t("col_waste")), 100),
            c("price", t("price"), 100),
            c("weight", i18n::t("col_total_weight"), 120),
        ],
        _ => vec![c("parameter", i18n::t("col_parameter"), 280), c("value", i18n::t("col_value"), 180)],
    }
}

pub fn statistics(v: &View) -> StatTables {
    let d = v.iface.num_digits;
    let dp = v.iface.num_digits_percent;
    let dpr = v.iface.num_digits_price;
    let sol = &v.sol;
    // pieces used: by part row
    let mut parts: Vec<(usize, i32)> = vec![];
    for b in &sol.bars {
        for p in &b.pieces {
            match parts.iter_mut().find(|x| x.0 == p.part) {
                Some(x) => x.1 += b.quantity,
                None => parts.push((p.part, b.quantity)),
            }
        }
    }
    parts.sort_by_key(|x| x.0);
    let parts_rows = parts
        .iter()
        .enumerate()
        .filter_map(|(i, (pi, n))| {
            let p = v.prj.parts.get(*pi)?;
            Some(vec![(i + 1).to_string(), fmt_num(p.length, d), n.to_string(), p.material.clone(), p.customer.clone()])
        })
        .collect();
    // bars used: by stock row
    let mut bars: Vec<(usize, i32)> = vec![];
    for b in &sol.bars {
        match bars.iter_mut().find(|x| x.0 == b.stock) {
            Some(x) => x.1 += b.quantity,
            None => bars.push((b.stock, b.quantity)),
        }
    }
    bars.sort_by_key(|x| x.0);
    let bars_rows = bars
        .iter()
        .enumerate()
        .filter_map(|(i, (si, n))| {
            let s = v.prj.stock.get(*si)?;
            let price = if v.prj.technical.price_per_what == 1 { s.price * s.length } else { s.price };
            Some(vec![(i + 1).to_string(), fmt_num(s.length, d), n.to_string(), s.material.clone(), fmt_num(price, dpr), fmt_num(weight_per_m(&v.prj, &s.material), 3)])
        })
        .collect();
    // offcuts (one list: no split into reusable and scrap)
    let mut offcuts: Vec<(f64, String, i32)> = vec![];
    for b in &sol.bars {
        if b.waste <= 0.0 {
            continue;
        }
        match offcuts.iter_mut().find(|x| x.0 == b.waste && x.1 == b.material) {
            Some(x) => x.2 += b.quantity,
            None => offcuts.push((b.waste, b.material.clone(), b.quantity)),
        }
    }
    offcuts.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let off_rows: Vec<Vec<String>> = offcuts.iter().enumerate().map(|(i, x)| vec![(i + 1).to_string(), fmt_num(x.0, d), x.2.to_string(), x.1.clone()]).collect();
    // per material
    let mut mats: Vec<String> = vec![];
    for b in &sol.bars {
        if !mats.contains(&b.material) {
            mats.push(b.material.clone());
        }
    }
    material_order(&mut mats);
    let mut mat_rows = vec![];
    let mut total_weight = 0.0;
    for m in &mats {
        let (mut used, mut size, mut nb, mut np, mut price) = (0.0, 0.0, 0, 0, 0.0);
        for b in sol.bars.iter().filter(|b| &b.material == m) {
            let q = b.quantity as f64;
            nb += b.quantity;
            np += b.pieces.len() as i32 * b.quantity;
            used += b.size_of_parts() * q;
            size += b.length * q;
            price += b.bar_price * q;
        }
        let waste = (size - used).max(0.0);
        let weight = used * metres_per_unit(&v.prj, m) * weight_per_m(&v.prj, m);
        total_weight += weight;
        mat_rows.push(vec![
            m.clone(),
            nb.to_string(),
            np.to_string(),
            fmt_num(used, d),
            fmt_num(size, d),
            fmt_num(waste, d),
            fmt_num(if used > 0.0 { waste / used * 100.0 } else { 0.0 }, dp),
            fmt_num(price, dpr),
            fmt_num(weight, 2),
        ]);
    }
    // summary
    let total_pieces: i32 = sol.bars.iter().map(|b| b.pieces.len() as i32 * b.quantity).sum();
    let total_bars: i32 = sol.bars.iter().map(|b| b.quantity).sum();
    let used: f64 = sol.bars.iter().map(|b| b.size_of_parts() * b.quantity as f64).sum();
    let size: f64 = sol.bars.iter().map(|b| b.length * b.quantity as f64).sum();
    let unplaced: i32 = sol.unplaced.iter().map(|u| u.1).sum();
    let mut misc = vec![
        vec![i18n::t("number_of_pieces"), total_pieces.to_string()],
        vec![i18n::t("bars"), total_bars.to_string()],
        vec![i18n::t("col_total_used_length"), fmt_num(used, d)],
        vec![i18n::t("col_size_used_bars"), fmt_num(size, d)],
        vec![i18n::t("col_waste"), fmt_num((size - used).max(0.0), d)],
        vec![format!("% {}", i18n::t("col_waste")), fmt_num(if used > 0.0 { (size - used).max(0.0) / used * 100.0 } else { 0.0 }, dp)],
        vec![i18n::t("col_price"), fmt_num(sol.total_cost(), dpr)],
        vec![i18n::t("col_total_weight"), fmt_num(total_weight, 2)],
        vec![i18n::t("running_time"), format!("{:.2} s", sol.running_time)],
    ];
    if unplaced > 0 {
        misc.push(vec![i18n::t("settings_unsatisfied"), unplaced.to_string()]);
    }
    StatTables { parts: parts_rows, bars: bars_rows, reused: off_rows, disposed: vec![], materials: mat_rows, misc }
}

pub fn setup_stat_grid(h: HWND, which: usize, iface: &Interface) {
    grid::with(h, |g| {
        style_grid(g, iface, crate::mainwin::ROW_H);
        g.fixed_cols = match which {
            2..=4 => 1,
            _ => 0,
        };
        g.editable = false;
        g.show_focus = false;
        g.cols = stat_columns(which);
    });
}

pub fn fill_stats(hs: &[HWND; 6]) {
    let t = view(statistics);
    let tables: [Vec<Vec<String>>; 6] = match t {
        Some(t) => [t.parts, t.bars, t.reused, t.disposed, t.materials, t.misc],
        None => Default::default(),
    };
    for (i, rows) in tables.into_iter().enumerate() {
        grid::with(hs[i], |g| {
            let n = rows.len();
            g.set_rows(rows, vec![true; n]);
        });
    }
}

// ------------------------------------------------------------------ accept

#[derive(Clone)]
pub struct Undo {
    pub parts: Vec<Part>,
    pub stock: Vec<Stock>,
}

/// Remove the cut pieces from the parts, the used bars from the stock, and add reusable
/// offcuts to the stock.
pub fn accept(parts: &[Part], stock: &[Stock], sol: &Solution, lin: &Linear) -> (Vec<Part>, Vec<Stock>) {
    let mut parts = parts.to_vec();
    let mut stock = stock.to_vec();
    for b in &sol.bars {
        if let Some(s) = stock.get_mut(b.stock) {
            s.quantity -= b.quantity;
        }
        // a row of the plan may stand for pieces of several part rows of the same length:
        // take them from the piece's own row first, then from rows of the same length
        for p in &b.pieces {
            let mut need = b.quantity;
            let material = parts.get(p.part).map(|x| x.material.clone()).unwrap_or_default();
            let same = |x: &Part| (x.length - p.length).abs() < 1e-9 && x.material == material;
            let mut order: Vec<usize> = vec![p.part];
            order.extend((0..parts.len()).filter(|&i| i != p.part && same(&parts[i])));
            for i in order {
                if need == 0 {
                    break;
                }
                if let Some(x) = parts.get_mut(i) {
                    let take = need.min(x.quantity.max(0));
                    x.quantity -= take;
                    need -= take;
                }
            }
        }
    }
    let mut offcuts: Vec<Stock> = vec![];
    for b in &sol.bars {
        if b.waste > 0.0 && b.waste + 1e-9 >= lin.minimal_useful_size {
            let src = stock.get(b.stock).cloned().unwrap_or_default();
            match offcuts.iter_mut().find(|o| o.length == b.waste && o.material == b.material && o.height == b.height) {
                Some(o) => o.quantity += b.quantity,
                None => offcuts.push(Stock { length: b.waste, height: b.height, quantity: b.quantity, material: b.material.clone(), label: src.label.clone(), price: if src.length > 0.0 && src.price > 0.0 { src.price * b.waste / src.length } else { 0.0 }, priority: src.priority, id: 0, use_it: true, a1: 90.0, a2: 90.0, weight: src.weight }),
            }
        }
    }
    parts.retain(|p| p.quantity > 0);
    stock.retain(|s| s.quantity > 0);
    stock.extend(offcuts);
    for (i, p) in parts.iter_mut().enumerate() {
        p.id = i as i32 + 1;
    }
    for (i, s) in stock.iter_mut().enumerate() {
        s.id = i as i32 + 1;
    }
    (parts, stock)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine;

    #[test]
    fn accept_consumes_everything_in_the_sample() {
        let prj = Project::sample();
        let sol = engine::optimize(&prj, &engine::Progress { cancel: &|| false, report: &|_, _| {} });
        let (parts, stock) = accept(&prj.parts, &prj.stock, &sol, &prj.technical.linear);
        assert!(parts.is_empty());
        let used: i32 = sol.bars.iter().map(|b| b.quantity).sum();
        let before: i32 = prj.stock.iter().map(|s| s.quantity).sum();
        let after: i32 = stock.iter().filter(|s| s.length >= 4000.0).map(|s| s.quantity).sum();
        assert_eq!(before - used, after);
    }

    #[test]
    fn cutting_formula_joins_pieces_with_plus() {
        let prj = Project::sample();
        let sol = engine::optimize(&prj, &engine::Progress { cancel: &|| false, report: &|_, _| {} });
        for b in &sol.bars {
            let f = pattern_text(b, 3);
            let sum: f64 = f.split('+').map(|x| x.parse::<f64>().unwrap()).sum();
            assert_eq!(sum, b.size_of_parts(), "{f}");
            assert_eq!(f.split('+').count(), b.pieces.len());
        }
        let mut b = sol.bars[0].clone();
        b.pieces.truncate(1);
        b.pieces.push(b.pieces[0].clone());
        b.pieces[0].length = 457.0;
        b.pieces[1].length = 12.5;
        assert_eq!(pattern_text(&b, 3), "457+12.5");
    }

    #[test]
    fn statistics_add_up() {
        let prj = Project::sample();
        let sol = engine::optimize(&prj, &engine::Progress { cancel: &|| false, report: &|_, _| {} });
        let v = View { max_len: 3000.0, sol, prj, iface: Interface::default() };
        let t = statistics(&v);
        assert_eq!(t.misc[0][1], "59");
        // offcut % of the piece length
        let used: f64 = t.materials.iter().map(|r| r[3].parse::<f64>().unwrap()).sum();
        let waste: f64 = t.materials.iter().map(|r| r[5].parse::<f64>().unwrap()).sum();
        assert!(waste >= 0.0 && used > 0.0);
        assert_eq!(t.parts.len(), 6);
    }
}

#[cfg(test)]
mod highlight_tests {
    use super::*;

    #[test]
    fn lengths_are_highlighted_on_their_own() {
        let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(highlighted_lengths(&l(&["12", "12", "12"])).is_empty());
        assert_eq!(highlighted_lengths(&l(&["6000", "6000", "4000", "6000"])), vec!["4000"]);
    }

    #[test]
    fn the_smaller_alternate_group_is_highlighted() {
        // the example of the request: DB10 50, DB12 100, DB16 5, DB20 70, DB25 20 rows
        let rows: Vec<(String, usize)> = [("DB12", 100), ("DB10", 50), ("DB20", 70), ("DB16", 5), ("DB25", 20)].iter().map(|(m, n)| (m.to_string(), *n)).collect();
        // [DB10, DB16, DB25] = 75 against [DB12, DB20] = 170
        assert_eq!(highlighted_materials(&rows), vec!["DB10", "DB16", "DB25"]);
    }

    #[test]
    fn rebar_weights() {
        assert_eq!(rebar_weight("DB12"), Some(0.888));
        assert_eq!(rebar_weight("db16"), Some(1.578));
        assert_eq!(rebar_weight("pipe"), None);
    }
}

// ------------------------------------------------------------------ printed report

/// Length unit of a material's bars: lengths of 100 or more are taken as millimetres
/// (6000), smaller ones as metres (12). Returns metres per unit.
pub fn metres_per_unit(prj: &Project, material: &str) -> f64 {
    let longest = prj.stock.iter().filter(|s| s.material == material).map(|s| s.length).fold(0.0, f64::max);
    if longest >= 100.0 {
        0.001
    } else {
        1.0
    }
}

pub struct ReportRow {
    pub length: String,
    pub quantity: String,
    pub material: String,
    pub waste: String,
    pub formula: String,
    /// the length cell in yellow: its own groups of bar lengths (see highlighted_lengths)
    pub highlight_length: bool,
    /// the material cell in yellow (see highlighted_materials)
    pub highlight_material: bool,
}

/// The cutting plan as printed and exported: title, bars used, rows grouped by material,
/// a length / offcut table, a weight table and the date.
pub struct Report {
    pub title: String,
    pub bars_used: Vec<String>,
    pub rows: Vec<ReportRow>,
    /// type, total length of the pieces, length of the bars used, offcut
    pub lengths: Vec<[String; 4]>,
    /// type, length of the pieces, weight per metre, total weight
    pub weights: Vec<[String; 4]>,
    pub date: String,
}

/// Today as dd-mm-yyyy (Buddhist year in Thai).
pub fn today() -> String {
    let mut st: windows_sys::Win32::Foundation::SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut st) };
    let year = st.wYear as i32 + if i18n::lang() == i18n::Lang::Th { 543 } else { 0 };
    format!("{:02}-{:02}-{}", st.wDay, st.wMonth, year)
}

pub fn report(v: &View, job: &str) -> Report {
    let d = v.iface.num_digits;
    let sol = &v.sol;
    let mut mats: Vec<String> = vec![];
    for b in &sol.bars {
        if !mats.contains(&b.material) {
            mats.push(b.material.clone());
        }
    }
    material_order(&mut mats);
    // bars used per material and length
    let mut bars_used = vec![];
    for m in &mats {
        let mut lens: Vec<(f64, i32)> = vec![];
        for b in sol.bars.iter().filter(|b| &b.material == m) {
            match lens.iter_mut().find(|x| x.0 == b.length) {
                Some(x) => x.1 += b.quantity,
                None => lens.push((b.length, b.quantity)),
            }
        }
        lens.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let unit = if metres_per_unit(&v.prj, m) < 1.0 { i18n::t("unit_mm") } else { i18n::t("unit_m") };
        for (l, n) in lens {
            bars_used.push(i18n::tf("bars_used_line", &[m, &fmt_num(l, d), &unit, &n.to_string()]));
        }
    }
    // rows: by material, then the most repeated patterns first
    let order = display_order(sol);
    let mut rows_per: Vec<(String, usize)> = mats.iter().map(|m| (m.clone(), sol.bars.iter().filter(|b| &b.material == m).count())).collect();
    rows_per.retain(|x| x.1 > 0);
    let yellow = highlighted_materials(&rows_per);
    let yellow_lens = highlighted_lengths(&sol.bars.iter().map(|b| fmt_num(b.length, d)).collect::<Vec<_>>());
    let mut rows = vec![];
    for m in &mats {
        for &bi in order.iter().filter(|&&bi| &sol.bars[bi].material == m) {
            let b = &sol.bars[bi];
            rows.push(ReportRow {
                length: fmt_num(b.length, d),
                quantity: b.quantity.to_string(),
                material: b.material.clone(),
                waste: fmt_num(b.waste, d),
                formula: pattern_text(b, d),
                highlight_length: yellow_lens.contains(&fmt_num(b.length, d)),
                highlight_material: yellow.contains(m),
            });
        }
    }
    let mut lengths = vec![];
    let mut weights = vec![];
    for m in &mats {
        let (mut used, mut size) = (0.0, 0.0);
        for b in sol.bars.iter().filter(|b| &b.material == m) {
            used += b.size_of_parts() * b.quantity as f64;
            size += b.length * b.quantity as f64;
        }
        lengths.push([m.clone(), fmt_num(used, d), fmt_num(size, d), fmt_num((size - used).max(0.0), d)]);
        let w = weight_per_m(&v.prj, m);
        weights.push([m.clone(), fmt_num(used, d), fmt_num(w, 3), format!("{:.2}", used * metres_per_unit(&v.prj, m) * w)]);
    }
    let title = if job.trim().is_empty() { i18n::tr("Cutting plan") } else { job.trim().to_string() };
    Report { title, bars_used, rows, lengths, weights, date: today() }
}
