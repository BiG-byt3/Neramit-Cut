//! Data grid: header and fixed columns, check boxes, sort indicator, two-line rows,
//! in-place editing and native scroll bars.

use super::theme::{self, px};
use super::vcl::{self, Ev};
use super::win::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub const GRID_CLASS: &str = "BxGrid";

#[derive(Clone, Debug)]
pub struct Column {
    pub key: String,
    pub title: String,
    pub width: i32,
    pub visible: bool,
    /// 0 left, 1 centre, 2 right
    pub align: i32,
    pub check: bool,
    pub readonly: bool,
    /// cell text "1"/"0" shown as a centred check box, toggled by a click
    pub toggle: bool,
}

impl Column {
    pub fn new(key: &str, title: &str, width: i32) -> Column {
        Column { key: key.into(), title: title.into(), width, visible: true, align: 0, check: false, readonly: false, toggle: false }
    }
}

pub type CellColorFn = fn(grid: &Grid, row: usize, col: usize) -> Option<u32>;
pub type CellDrawFn = fn(grid: &Grid, row: usize, col: usize, dc: HDC, r: &RECT) -> bool;
/// Height of a body row (rows that need more room, e.g. a long wrapped text).
pub type RowHeightFn = fn(grid: &Grid, row: usize) -> i32;
/// Values a column offers in a drop-down list (None: free text only).
pub type ChoicesFn = fn(grid: &Grid, col: usize) -> Option<Vec<String>>;
/// Paints the second line of a row (`detail_h` tall, across the body columns).
pub type DetailDrawFn = fn(grid: &Grid, row: usize, dc: HDC, r: &RECT);

pub struct Grid {
    pub form: usize,
    pub ctl: usize,
    pub hwnd: HWND,
    pub cols: Vec<Column>,
    /// body rows; row 0 of the grid is the header
    pub rows: Vec<Vec<String>>,
    pub checked: Vec<bool>,
    pub row_heights: Vec<i32>,
    pub fixed_cols: usize,
    pub header_h: i32,
    pub row_h: i32,
    pub fixed_color: u32,
    pub sel_color: u32,
    pub font: FontSpec,
    pub header_font: FontSpec,
    pub header_wrap: bool,
    pub top: usize,
    pub left: usize,
    pub row: usize,
    pub col: usize,
    pub sort_col: Option<usize>,
    pub sort_desc: bool,
    pub sort_on_click: bool,
    pub editable: bool,
    pub edit: HWND,
    pub edit_cell: (usize, usize),
    pub cell_color: Option<CellColorFn>,
    pub cell_draw: Option<CellDrawFn>,
    /// rows of two lines: the cells on the first, `detail_draw` across the second
    pub detail_h: i32,
    pub detail_draw: Option<DetailDrawFn>,
    pub row_height_fn: Option<RowHeightFn>,
    pub choices: Option<ChoicesFn>,
    /// the other corner of a block of selected cells (row, col); equal to (row, col) when
    /// one cell is selected; a mouse drag in progress
    pub anchor: (usize, usize),
    pub sel_drag: bool,
    pub header_check: bool,
    pub tag: String,
    pub scroll_x: i32,
    pub show_focus: bool,
    /// row under the mouse (0 = none)
    pub hot_row: usize,
    /// overlay scroll bars: rows that fit on a page, bar under the mouse (1 vertical,
    /// 2 horizontal), drag in progress (bar, mouse start, value start)
    pub vis_rows: usize,
    pub sb_hot: u8,
    pub sb_drag: Option<(u8, i32, i32)>,
    /// text shown when there are no rows, with an optional icon above it
    pub empty_text: String,
    pub empty_icon: Option<char>,
    /// highlight the whole current row
    pub row_select: bool,
    /// column (key) that takes the remaining width of the grid
    pub fill_key: Option<String>,
    pub fill_min: i32,
    /// widest the fill column grows (0: no limit)
    pub fill_max: i32,
    /// all body columns share the width of the grid in proportion to their widths
    pub fit_all: bool,
}

thread_local! {
    static GRIDS: RefCell<HashMap<isize, Box<Grid>>> = RefCell::new(HashMap::new());
}

pub fn register() {
    unsafe {
        let cls = wide(GRID_CLASS);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS | CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(grid_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance(),
            hIcon: null_mut(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: cls.as_ptr(),
            hIconSm: null_mut(),
        };
        RegisterClassExW(&wc);
    }
}

pub fn attach(hwnd: HWND, form: usize, ctl: usize) {
    let c = &vcl::form(form).ctls[ctl];
    let fixed_color = c.comp.props.get("FixedColor").and_then(tcolor).unwrap_or(unsafe { GetSysColor(COLOR_BTNFACE) });
    let sel_color = c.comp.props.get("SelectionColor").and_then(tcolor).unwrap_or(rgb(0xB6, 0xCA, 0xEA));
    let font = c.font.clone();
    let row_h = c.comp.int("DefaultRowHeight", 24);
    let g = Grid {
        form,
        ctl,
        hwnd,
        cols: vec![],
        rows: vec![],
        checked: vec![],
        row_heights: vec![],
        fixed_cols: c.comp.int("FixedCols", 1).max(0) as usize,
        header_h: row_h,
        row_h,
        fixed_color,
        sel_color,
        font: font.clone(),
        header_font: font,
        header_wrap: true,
        top: 0,
        left: 0,
        row: 0,
        col: 0,
        sort_col: None,
        sort_desc: false,
        sort_on_click: c.comp.bool("SortOnClick", false),
        editable: c.comp.set_has("Options", "goEditing"),
        edit: null_mut(),
        edit_cell: (0, 0),
        cell_color: None,
        cell_draw: None,
        detail_h: 0,
        detail_draw: None,
        row_height_fn: None,
        choices: None,
        anchor: (0, 0),
        sel_drag: false,
        header_check: false,
        tag: String::new(),
        scroll_x: 0,
        vis_rows: 1,
        sb_hot: 0,
        sb_drag: None,
        show_focus: true,
        hot_row: 0,
        empty_text: String::new(),
        empty_icon: None,
        row_select: true,
        fill_key: None,
        fill_min: px(260),
        fill_max: 0,
        fit_all: false,
    };
    GRIDS.with(|m| m.borrow_mut().insert(hwnd as isize, Box::new(g)));
}

/// Access the grid behind a window.
pub fn with<R>(hwnd: HWND, f: impl FnOnce(&mut Grid) -> R) -> Option<R> {
    let p: *mut Grid = GRIDS.with(|m| m.borrow_mut().get_mut(&(hwnd as isize)).map(|b| b.as_mut() as *mut Grid))?;
    // The box is never moved or dropped while the window lives; re-entrant handlers are allowed.
    Some(f(unsafe { &mut *p }))
}

pub fn grid(hwnd: HWND) -> &'static mut Grid {
    let p: *mut Grid = GRIDS.with(|m| m.borrow_mut().get_mut(&(hwnd as isize)).map(|b| b.as_mut() as *mut Grid)).expect("grid");
    unsafe { &mut *p }
}

const LINE_FIXED: u32 = 0x000000;
const LINE_BODY: u32 = 0xC0C0C0;
const HEADER_RIGHT: i32 = 4;
const BODY_RIGHT: i32 = 4;
const SORT_SPACE: i32 = 12;
const CHECK_TEXT_GAP: i32 = 2;

impl Grid {
    pub fn vis_cols(&self) -> Vec<usize> {
        (0..self.cols.len()).filter(|&c| self.cols[c].visible).collect()
    }
    fn height_of(&self, row: usize) -> i32 {
        if row == 0 {
            self.header_h
        } else {
            *self.row_heights.get(row - 1).unwrap_or(&self.row_h)
        }
    }
    pub fn row_count(&self) -> usize {
        self.rows.len() + 1
    }
    pub fn cell(&self, row: usize, col: usize) -> &str {
        if row == 0 {
            return &self.cols[col].title;
        }
        self.rows.get(row - 1).and_then(|r| r.get(col)).map(|s| s.as_str()).unwrap_or("")
    }
    pub fn set_cell(&mut self, row: usize, col: usize, v: &str) {
        if row == 0 {
            self.cols[col].title = v.into();
            return;
        }
        let ncol = self.cols.len();
        if let Some(r) = self.rows.get_mut(row - 1) {
            if r.len() < ncol {
                r.resize(ncol, String::new());
            }
            r[col] = v.into();
        }
    }
    pub fn set_rows(&mut self, rows: Vec<Vec<String>>, checked: Vec<bool>) {
        self.rows = rows;
        self.checked = checked;
        self.checked.resize(self.rows.len(), true);
        self.row_heights = vec![self.row_h; self.rows.len()];
        if self.row > self.rows.len() {
            self.row = self.rows.len();
        }
        if self.row == 0 && !self.rows.is_empty() {
            self.row = 1;
        }
        if self.top >= self.rows.len().max(1) {
            self.top = 0;
        }
        self.fix_col();
        self.update_scroll();
        self.invalidate();
    }
    fn fix_col(&mut self) {
        let vis = self.vis_cols();
        if !vis.contains(&self.col) || self.col < self.fixed_cols {
            self.col = vis.iter().copied().find(|&c| c >= self.fixed_cols && !self.cols[c].check).or_else(|| vis.first().copied()).unwrap_or(0);
        }
    }
    pub fn invalidate(&self) {
        unsafe { InvalidateRect(self.hwnd, null(), 0) };
    }

    fn fixed_width(&self) -> i32 {
        self.vis_cols().iter().filter(|&&c| c < self.fixed_cols).map(|&c| self.cols[c].width).sum()
    }

    /// Column x-positions (left edge) of visible columns for the current horizontal scroll.
    fn col_x(&self) -> Vec<(usize, i32)> {
        let mut out = vec![];
        let mut x = 0;
        for c in self.vis_cols() {
            if c < self.fixed_cols {
                out.push((c, x));
                x += self.cols[c].width;
            }
        }
        let mut sx = x - self.scroll_x;
        for c in self.vis_cols() {
            if c >= self.fixed_cols {
                out.push((c, sx));
                sx += self.cols[c].width;
            }
        }
        out
    }

    fn content_width(&self) -> i32 {
        self.vis_cols().iter().map(|&c| self.cols[c].width).sum()
    }

    fn row_y(&self) -> Vec<(usize, i32)> {
        let mut out = vec![(0usize, 0i32)];
        let mut y = self.header_h;
        for r in (self.top + 1)..self.row_count() {
            out.push((r, y));
            y += self.height_of(r);
        }
        out
    }

    pub fn hit(&self, x: i32, y: i32) -> Option<(usize, usize)> {
        let fw = self.fixed_width();
        let mut col = None;
        for (c, cx) in self.col_x() {
            if c >= self.fixed_cols && cx + self.cols[c].width <= fw {
                continue;
            }
            if x >= cx && x < cx + self.cols[c].width {
                col = Some(c);
                break;
            }
        }
        let mut row = None;
        for (r, ry) in self.row_y() {
            if y >= ry && y < ry + self.height_of(r) {
                row = Some(r);
                break;
            }
        }
        Some((row?, col?))
    }

    pub fn cell_rect(&self, row: usize, col: usize) -> Option<RECT> {
        let x = self.col_x().iter().find(|(c, _)| *c == col).map(|(_, x)| *x)?;
        let y = self.row_y().iter().find(|(r, _)| *r == row).map(|(_, y)| *y)?;
        // two-line rows: a cell is on the data line, not over the drawing under it
        let h = self.height_of(row) - if row > 0 && self.detail_h > 0 && col >= self.fixed_cols { self.detail_h } else { 0 };
        Some(rect(x, y, self.cols[col].width, h.max(1)))
    }

    /// Scale the visible body columns so together they fill the grid, each keeping its share
    /// (balanced columns instead of one wide last column).
    fn fit_all_columns(&mut self, client_w: i32) {
        let vis: Vec<usize> = self.vis_cols().into_iter().filter(|&c| c >= self.fixed_cols).collect();
        let fixed: i32 = self.vis_cols().iter().filter(|&&c| c < self.fixed_cols).map(|&c| self.cols[c].width).sum();
        let total: i32 = vis.iter().map(|&c| self.cols[c].width).sum();
        let avail = client_w - fixed;
        if vis.is_empty() || total <= 0 || avail < vis.len() as i32 * px(40) {
            return;
        }
        let mut used = 0;
        for (k, &c) in vis.iter().enumerate() {
            let w = if k + 1 == vis.len() { avail - used } else { (self.cols[c].width as i64 * avail as i64 / total as i64) as i32 };
            self.cols[c].width = w.max(px(40));
            used += self.cols[c].width;
        }
    }

    /// Give the fill column the width left over by the other visible columns.
    fn fit_fill_column(&mut self, client_w: i32) {
        let Some(key) = self.fill_key.clone() else { return };
        let Some(i) = self.cols.iter().position(|c| c.key == key) else { return };
        let others: i32 = self.vis_cols().iter().filter(|&&c| c != i).map(|&c| self.cols[c].width).sum();
        let mut w = (client_w - others).max(self.fill_min);
        if self.fill_max > 0 {
            w = w.min(self.fill_max);
        }
        self.cols[i].width = w;
    }

    pub fn update_scroll(&mut self) {
        unsafe {
            let cr = client_rect(self.hwnd);
            if self.fit_all {
                self.fit_all_columns(cr.right);
            } else {
                self.fit_fill_column(cr.right);
            }
            if let Some(f) = self.row_height_fn {
                let n = self.rows.len();
                self.row_heights = (1..=n).map(|r| f(self, r)).collect();
            }
            let total_h: i32 = (1..self.row_count()).map(|r| self.height_of(r)).sum();
            let page_h = (cr.bottom - self.header_h).max(1);
            // vertical scroll in rows
            let rows = self.rows.len() as i32;
            let mut vis = 0;
            let mut acc = 0;
            for r in (self.top + 1)..self.row_count() {
                acc += self.height_of(r);
                if acc > page_h {
                    break;
                }
                vis += 1;
            }
            // overlay scroll bars are painted by the grid (see paint_scrollbars)
            ShowScrollBar(self.hwnd, SB_BOTH, 0);
            self.vis_rows = vis.max(1) as usize;
            if total_h <= page_h {
                self.top = 0;
            } else {
                self.top = self.top.min((rows as usize).saturating_sub(self.vis_rows));
            }
            if self.content_width() <= cr.right {
                self.scroll_x = 0;
            }
        }
    }

    pub fn ensure_visible(&mut self) {
        if self.row == 0 {
            return;
        }
        unsafe {
            let cr = client_rect(self.hwnd);
            if self.row <= self.top {
                self.top = self.row - 1;
            } else {
                loop {
                    let mut y = self.header_h;
                    for r in (self.top + 1)..=self.row {
                        y += self.height_of(r);
                    }
                    if y <= cr.bottom || self.top + 1 >= self.row {
                        break;
                    }
                    self.top += 1;
                }
            }
            // horizontal
            if self.col >= self.fixed_cols {
                if let Some((_, x)) = self.col_x().into_iter().find(|(c, _)| *c == self.col) {
                    let fw = self.fixed_width();
                    let w = self.cols[self.col].width;
                    if x < fw {
                        self.scroll_x -= fw - x;
                    } else if x + w > cr.right {
                        self.scroll_x += (x + w - cr.right).min(x - fw);
                    }
                    self.scroll_x = self.scroll_x.max(0);
                }
            }
        }
        self.update_scroll();
    }

    unsafe fn paint(&self, dc: HDC, cr: &RECT) {
        fill(dc, cr, theme::SURFACE);
        let fw = self.fixed_width();
        let cols = self.col_x();
        let content_right = cols.iter().map(|(c, x)| x + self.cols[*c].width).max().unwrap_or(0).min(cr.right);
        for (r, y) in self.row_y() {
            if y > cr.bottom {
                break;
            }
            let h = self.height_of(r);
            // row background
            if r == 0 {
                fill(dc, &RECT { left: 0, top: y, right: cr.right, bottom: y + h }, theme::SURFACE_ALT);
                hline(dc, 0, cr.right, y + h - 1, theme::STROKE);
            } else {
                let bg = if self.row_select && r == self.row && self.show_focus {
                    theme::ACCENT_SOFT
                } else if r == self.hot_row {
                    theme::ROW_HOT
                } else if r % 2 == 0 {
                    theme::ROW_ALT
                } else {
                    theme::SURFACE
                };
                // across the whole width, also past a fill column that stopped at its widest
                fill(dc, &RECT { left: 0, top: y, right: cr.right, bottom: y + h }, bg);
                hline(dc, 0, cr.right, y + h - 1, theme::DIVIDER);
            }
            // two-line rows: cells on the first line (fixed columns span both)
            let top_h = if r > 0 && self.detail_h > 0 { (h - self.detail_h).max(1) } else { h };
            // body columns first so fixed columns paint over scrolled cells
            for pass in [false, true] {
                for &(c, x) in &cols {
                    let fixed_col = c < self.fixed_cols;
                    if fixed_col != pass || x > cr.right {
                        continue;
                    }
                    if !fixed_col && x + self.cols[c].width <= fw {
                        continue;
                    }
                    let cell = rect(x, y, self.cols[c].width, if fixed_col { h } else { top_h });
                    let saved = SaveDC(dc);
                    if !fixed_col {
                        IntersectClipRect(dc, fw, 0, cr.right, cr.bottom);
                    }
                    self.paint_cell(dc, r, c, &cell);
                    RestoreDC(dc, saved);
                }
            }
            if top_h < h {
                if let Some(f) = self.detail_draw {
                    let dr = RECT { left: fw, top: y + top_h, right: content_right.min(cr.right), bottom: y + h - 1 };
                    let saved = SaveDC(dc);
                    IntersectClipRect(dc, dr.left, dr.top, dr.right, dr.bottom);
                    f(self, r, dc, &dr);
                    RestoreDC(dc, saved);
                }
            }
        }
        if self.rows.is_empty() && !self.empty_text.is_empty() {
            let mut top = self.header_h + px(24);
            if let Some(ch) = self.empty_icon {
                let d = px(56);
                let x = cr.right / 2 - d / 2;
                let ir = RECT { left: x, top: top + px(8), right: x + d, bottom: top + px(8) + d };
                theme::ellipse(dc, &ir, theme::ACCENT_SOFT);
                theme::glyph(dc, ch, &ir, 24, theme::ACCENT);
                top = ir.bottom + px(14);
            }
            let er = RECT { left: px(16), top, right: cr.right - px(16), bottom: (top + px(96)).min(cr.bottom) };
            let mut f = theme::font(theme::BODY);
            f.color = theme::TEXT_SUBTLE;
            theme::text(dc, &self.empty_text, &er, &f, theme::TEXT_MUTED, DT_CENTER | DT_WORDBREAK);
        }
        if let Some((r0, r1, cols)) = self.block() {
            if let (Some(&c0), Some(&c1)) = (cols.first(), cols.last()) {
                if let (Some(a), Some(b)) = (self.cell_rect(r0, c0), self.cell_rect(r1, c1)) {
                    let saved = SaveDC(dc);
                    IntersectClipRect(dc, fw, self.header_h, cr.right, cr.bottom);
                    let br = RECT { left: a.left, top: a.top - 1, right: b.right, bottom: b.bottom };
                    theme::round_rect_w(dc, &br, px(2) as f32, None, Some(theme::ACCENT), px(1) as f32);
                    RestoreDC(dc, saved);
                }
            }
        }
        // the current cell is framed in every grid with the focus (read-only ones too: it is
        // what Ctrl+C copies)
        if self.show_focus && self.row > 0 && self.col >= self.fixed_cols && GetFocus() == self.hwnd {
            if let Some(cell) = self.cell_rect(self.row, self.col) {
                let saved = SaveDC(dc);
                IntersectClipRect(dc, fw, self.header_h, cr.right, cr.bottom);
                self.paint_focus(dc, &cell);
                if let Some(ar) = self.arrow_rect() {
                    theme::glyph(dc, theme::icon::CHEVRON_DOWN, &ar, 10, theme::ACCENT);
                }
                RestoreDC(dc, saved);
            }
        }
    }

    /// The selected block: rows r0..=r1 and the visible columns between the two corners.
    pub fn block(&self) -> Option<(usize, usize, Vec<usize>)> {
        let (ar, ac) = self.anchor;
        if ar == 0 || self.row == 0 || ar > self.rows.len() || (ar, ac) == (self.row, self.col) {
            return None;
        }
        let vis = self.vis_cols();
        let (pa, pc) = (vis.iter().position(|&c| c == ac)?, vis.iter().position(|&c| c == self.col)?);
        let cols = vis[pa.min(pc)..=pa.max(pc)].iter().copied().filter(|&c| c >= self.fixed_cols).collect();
        Some((ar.min(self.row), ar.max(self.row), cols))
    }

    fn in_block(&self, r: usize, c: usize) -> bool {
        self.block().map(|(r0, r1, cols)| r >= r0 && r <= r1 && cols.contains(&c)).unwrap_or(false)
    }

    /// The cells an edit applies to: the block, or the current cell.
    pub fn target_cells(&self) -> Vec<(usize, usize)> {
        let ok = |c: usize| !self.cols[c].readonly && !self.cols[c].check && !self.cols[c].toggle;
        match self.block() {
            Some((r0, r1, cols)) => (r0..=r1).flat_map(|r| cols.iter().filter(|&&c| ok(c)).map(move |&c| (r, c))).collect(),
            None if self.row > 0 && ok(self.col) => vec![(self.row, self.col)],
            None => vec![],
        }
    }

    /// Write one value into several cells (one CellChanged each).
    fn fill_cells(&mut self, cells: &[(usize, usize)], v: &str) {
        for &(r, c) in cells {
            if self.cell(r, c) != v {
                let old = self.cell(r, c).to_string();
                self.set_cell(r, c, v);
                fire_grid(self, GridEv::CellChanged(r, c, old));
            }
        }
        self.invalidate();
    }

    /// The block (or the current cell) as tab-separated text.
    fn block_text(&self) -> String {
        let (r0, r1, cols) = self.block().unwrap_or((self.row, self.row, vec![self.col]));
        (r0..=r1).map(|r| cols.iter().map(|&c| self.cell(r, c).to_string()).collect::<Vec<_>>().join("\t")).collect::<Vec<_>>().join("\r\n")
    }

    /// The drop-down list of the current cell; true when it was shown.
    unsafe fn drop_down(&mut self) -> bool {
        let Some(f) = self.choices else { return false };
        let Some(list) = f(self, self.col) else { return false };
        let Some(r) = self.cell_rect(self.row, self.col) else { return false };
        let m = CreatePopupMenu();
        for (i, v) in list.iter().enumerate() {
            AppendMenuW(m, MF_STRING, i + 1, wide(v).as_ptr());
        }
        if !list.is_empty() {
            AppendMenuW(m, MF_SEPARATOR, 0, null());
        }
        AppendMenuW(m, MF_STRING, 9999, wide(&crate::i18n::tr("Type another…")).as_ptr());
        crate::ui::menu::style_popup(m);
        let mut pt = POINT { x: r.left, y: r.bottom };
        ClientToScreen(self.hwnd, &mut pt);
        let cmd = TrackPopupMenu(m, TPM_LEFTALIGN | TPM_TOPALIGN | TPM_RETURNCMD | TPM_LEFTBUTTON, pt.x, pt.y, 0, self.hwnd, null());
        DestroyMenu(m);
        match cmd {
            0 => {}
            9999 => {
                self.start_text_edit(None);
            }
            k => {
                if let Some(v) = list.get(k as usize - 1) {
                    let cells = self.target_cells();
                    self.fill_cells(&cells, v);
                }
            }
        }
        true
    }

    /// The column offers a list of values (material).
    pub fn has_choices(&self, c: usize) -> bool {
        self.choices.map(|f| f(self, c).is_some()).unwrap_or(false)
    }

    /// Area of the drop-down arrow in the current cell.
    unsafe fn arrow_rect(&self) -> Option<RECT> {
        let f = self.choices?;
        f(self, self.col)?;
        let r = self.cell_rect(self.row, self.col)?;
        Some(RECT { left: r.right - px(24), top: r.top, right: r.right, bottom: r.bottom })
    }

    /// Vertical bar: (track, thumb), when the rows do not all fit.
    unsafe fn vbar(&self) -> Option<(RECT, RECT)> {
        let cr = client_rect(self.hwnd);
        let n = self.rows.len();
        let total_h: i32 = (1..self.row_count()).map(|r| self.height_of(r)).sum();
        if total_h <= cr.bottom - self.header_h || n == 0 {
            return None;
        }
        let w = if self.sb_hot == 1 || matches!(self.sb_drag, Some((1, _, _))) { px(10) } else { px(6) };
        let bottom = cr.bottom - px(3) - if self.hbar().is_some() { px(10) } else { 0 };
        let track = RECT { left: cr.right - w - px(3), top: self.header_h + px(3), right: cr.right - px(3), bottom };
        let th = track.bottom - track.top;
        let max_top = n.saturating_sub(self.vis_rows).max(1);
        let len = ((th as f64 * self.vis_rows as f64 / n as f64) as i32).clamp(px(28).min(th), th);
        let y = track.top + ((th - len) as f64 * self.top.min(max_top) as f64 / max_top as f64) as i32;
        Some((track, RECT { left: track.left, top: y, right: track.right, bottom: y + len }))
    }

    /// Horizontal bar: (track, thumb), when the columns are wider than the grid.
    unsafe fn hbar(&self) -> Option<(RECT, RECT)> {
        let cr = client_rect(self.hwnd);
        let cw = self.content_width();
        if cw <= cr.right {
            return None;
        }
        let h = if self.sb_hot == 2 || matches!(self.sb_drag, Some((2, _, _))) { px(10) } else { px(6) };
        let fw = self.fixed_width();
        let track = RECT { left: fw + px(3), top: cr.bottom - h - px(3), right: cr.right - px(3), bottom: cr.bottom - px(3) };
        let tw = track.right - track.left;
        let view = (cr.right - fw).max(1);
        let range = (cw - cr.right).max(1);
        let len = ((tw as f64 * view as f64 / (cw - fw) as f64) as i32).clamp(px(28).min(tw), tw);
        let x = track.left + ((tw - len) as f64 * self.scroll_x.min(range) as f64 / range as f64) as i32;
        Some((track, RECT { left: x, top: track.top, right: x + len, bottom: track.bottom }))
    }

    /// Modern overlay scroll bars: a slim rounded thumb, no arrows; wider under the mouse.
    unsafe fn paint_scrollbars(&self, dc: HDC) {
        for (bar, which) in [(self.vbar(), 1u8), (self.hbar(), 2u8)] {
            let Some((track, thumb)) = bar else { continue };
            let active = self.sb_hot == which || matches!(self.sb_drag, Some((w, _, _)) if w == which);
            let r = (px(5)) as f32;
            if active {
                theme::round_rect_alpha(dc, &track, r, theme::TEXT, 18);
            }
            theme::round_rect_alpha(dc, &thumb, r, theme::TEXT, if active { 120 } else { 70 });
        }
    }

    /// Which bar a point is on (1 vertical, 2 horizontal), with a little extra room.
    unsafe fn bar_at(&self, x: i32, y: i32) -> u8 {
        let cr = client_rect(self.hwnd);
        if let Some((t, _)) = self.vbar() {
            if x >= cr.right - px(14) && y >= t.top && y <= t.bottom {
                return 1;
            }
        }
        if let Some((t, _)) = self.hbar() {
            if y >= cr.bottom - px(14) && x >= t.left && x <= t.right {
                return 2;
            }
        }
        0
    }

    /// Move a bar to follow the mouse (drag) or jump a page towards it (click on the track).
    unsafe fn bar_drag(&mut self, which: u8, x: i32, y: i32) {
        if let Some((_, x0, v0)) = self.sb_drag {
            if which == 1 {
                if let Some((track, thumb)) = self.vbar() {
                    let free = ((track.bottom - track.top) - (thumb.bottom - thumb.top)).max(1);
                    let max_top = self.rows.len().saturating_sub(self.vis_rows) as f64;
                    self.top = (v0 as f64 + (y - x0) as f64 * max_top / free as f64).round().clamp(0.0, max_top) as usize;
                }
            } else if let Some((track, thumb)) = self.hbar() {
                let free = ((track.right - track.left) - (thumb.right - thumb.left)).max(1);
                let range = (self.content_width() - client_rect(self.hwnd).right).max(0) as f64;
                self.scroll_x = (v0 as f64 + (x - x0) as f64 * range / free as f64).round().clamp(0.0, range) as i32;
            }
        }
    }

    unsafe fn paint_check(&self, dc: HDC, box_r: &RECT, on: bool) {
        if on {
            theme::round_rect(dc, box_r, px(3) as f32, Some(theme::ACCENT), None);
            theme::glyph(dc, theme::icon::CHECK, box_r, 11, theme::WHITE);
        } else {
            theme::round_rect(dc, box_r, px(3) as f32, Some(theme::SURFACE), Some(theme::STROKE_STRONG));
        }
    }

    /// `cell` includes its divider line (the last pixel row).
    unsafe fn paint_cell(&self, dc: HDC, r: usize, c: usize, cell: &RECT) {
        let fixed = r == 0 || c < self.fixed_cols;
        let inner = RECT { left: cell.left, top: cell.top, right: cell.right, bottom: cell.bottom - 1 };
        if r > 0 && c < self.fixed_cols {
            let sel = self.row_select && r == self.row && self.show_focus;
            fill(dc, &inner, if sel { theme::ACCENT_SOFT } else { theme::SURFACE_ALT });
            if sel && c == 0 {
                // the current row: a short accent bar at the left edge
                let br = RECT { left: inner.left, top: inner.top + px(4), right: inner.left + px(3), bottom: inner.bottom - px(4) };
                theme::round_rect(dc, &br, px(1) as f32, Some(theme::ACCENT), None);
            }
            vline(dc, cell.right - 1, cell.top, cell.bottom, theme::DIVIDER);
        } else if r > 0 {
            if let Some(f) = self.cell_color {
                if let Some(cc) = f(self, r, c) {
                    fill(dc, &inner, cc);
                }
            }
            if self.in_block(r, c) {
                fill(dc, &inner, theme::ACCENT_SOFT2);
            }
            if self.cols.get(c + 1).is_some() {
                vline(dc, cell.right - 1, cell.top + px(6), cell.bottom - px(6), rgb(0xF0, 0xF1, 0xF3));
            }
        } else if c + 1 < self.cols.len() {
            vline(dc, cell.right - 1, cell.top + px(8), cell.bottom - px(8), theme::STROKE);
        }
        if let Some(f) = self.cell_draw {
            if f(self, r, c, dc, &inner) {
                return;
            }
        }
        let col = &self.cols[c];
        let pad = px(8);
        let mut tr = RECT { left: inner.left + pad, top: inner.top, right: inner.right - pad, bottom: inner.bottom };
        let box_size = px(16);
        if col.check && (r > 0 || self.header_check) {
            let checked = if r == 0 { !self.checked.is_empty() && self.checked.iter().all(|&b| b) } else { *self.checked.get(r - 1).unwrap_or(&false) };
            let top = inner.top + (inner.bottom - inner.top - box_size) / 2;
            let box_r = RECT { left: inner.left + px(8), top, right: inner.left + px(8) + box_size, bottom: top + box_size };
            self.paint_check(dc, &box_r, checked);
            tr.left = box_r.right + px(6);
        }
        if col.toggle && r > 0 {
            let on = self.cell(r, c) == "1";
            let left = inner.left + (inner.right - inner.left - box_size) / 2;
            let top = inner.top + (inner.bottom - inner.top - box_size) / 2;
            self.paint_check(dc, &RECT { left, top, right: left + box_size, bottom: top + box_size }, on);
            return;
        }
        let text = self.cell(r, c);
        if r == 0 {
            if self.sort_col == Some(c) {
                tr.right -= px(14);
            }
            let hf = self.header_font.clone();
            let mut flags = DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS;
            if self.header_wrap {
                flags = (flags & !(DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS)) | DT_WORDBREAK;
                let mut calc = tr;
                let saved = SaveDC(dc);
                let old = SelectObject(dc, font(&hf) as _);
                draw_text(dc, text, &mut calc, flags | DT_CALCRECT);
                SelectObject(dc, old);
                RestoreDC(dc, saved);
                let th = calc.bottom - calc.top;
                tr.top += ((inner.bottom - inner.top) - th).max(0) / 2;
            }
            theme::text(dc, text, &tr, &hf, theme::HEADER_TEXT, flags);
            if self.sort_col == Some(c) {
                let ar = RECT { left: inner.right - pad - px(12), top: inner.top, right: inner.right - pad + px(2), bottom: inner.bottom };
                theme::glyph(dc, if self.sort_desc { theme::icon::CHEVRON_DOWN } else { '\u{E70E}' }, &ar, 9, theme::TEXT_SUBTLE);
            }
            return;
        }
        let list = self.editable && !fixed && self.has_choices(c);
        if list {
            let ar = RECT { left: inner.right - px(24), top: inner.top, right: inner.right, bottom: inner.bottom };
            theme::glyph(dc, theme::icon::CHEVRON_DOWN, &ar, 10, theme::TEXT_SUBTLE);
            tr.right = ar.left;
            if text.is_empty() {
                theme::text(dc, &crate::i18n::tr("Select"), &tr, &self.font, theme::TEXT_SUBTLE, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
                return;
            }
        }
        if text.is_empty() {
            return;
        }
        let align = col.align;
        let mut flags = [DT_LEFT, DT_CENTER, DT_RIGHT][align.clamp(0, 2) as usize] | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS;
        if col.check {
            flags = DT_RIGHT | DT_VCENTER | DT_SINGLELINE;
        }
        let color = if fixed && col.key == "index" { theme::TEXT_SUBTLE } else { self.font.color };
        theme::text(dc, text, &tr, &self.font, color, flags);
    }

    /// Current cell: 2 px accent frame.
    unsafe fn paint_focus(&self, dc: HDC, cell: &RECT) {
        let fr = RECT { left: cell.left, top: cell.top - 1, right: cell.right, bottom: cell.bottom };
        theme::round_rect_w(dc, &fr, px(3) as f32, None, Some(theme::ACCENT), px(2) as f32);
    }

    pub fn move_to(&mut self, row: usize, col: usize) {
        self.commit_edit(true);
        self.row = row.clamp(1.min(self.rows.len()), self.rows.len());
        self.col = col;
        self.fix_col();
        // a jump to a cell is a new selection, not a block from the old cell
        self.anchor = (self.row, self.col);
        self.ensure_visible();
        self.invalidate();
    }

    pub fn start_edit(&mut self, initial: Option<char>) {
        if !self.editable || self.row == 0 || self.cols.get(self.col).map(|c| c.readonly || c.check || c.toggle).unwrap_or(true) || self.col < self.fixed_cols {
            return;
        }
        // a column with a list: F2 / Enter / double click open it, typing edits the text
        if initial.is_none() && unsafe { self.drop_down() } {
            return;
        }
        self.start_text_edit(initial);
    }

    pub fn start_text_edit(&mut self, initial: Option<char>) {
        unsafe {
            self.ensure_visible();
            let Some(r) = self.cell_rect(self.row, self.col) else { return };
            // the text is typed where the cell shows it: numbers on the right
            let right = self.cols.get(self.col).map(|c| c.align == 2).unwrap_or(false);
            if !self.edit.is_null() && ((GetWindowLongW(self.edit, GWL_STYLE) as u32 & ES_RIGHT as u32) != 0) != right {
                DestroyWindow(self.edit);
                self.edit = null_mut();
            }
            if self.edit.is_null() {
                let align = if right { ES_RIGHT as u32 } else { 0 };
                self.edit = CreateWindowExW(0, wide("EDIT").as_ptr(), null(), WS_CHILD | ES_AUTOHSCROLL as u32 | align, 0, 0, 10, 10, self.hwnd, 77 as _, hinstance(), null());
                windows_sys::Win32::UI::Shell::SetWindowSubclass(self.edit, Some(inplace_proc), 1, 0);
            }
            SendMessageW(self.edit, WM_SETFONT, font(&theme::native_text_font(&self.font)) as usize, 0);
            let text = match initial {
                Some(ch) => ch.to_string(),
                None => self.cell(self.row, self.col).to_string(),
            };
            set_text(self.edit, &text);
            self.edit_cell = (self.row, self.col);
            let th = text_height(&self.font);
            let ey = r.top + ((r.bottom - r.top) - th) / 2;
            SetWindowPos(self.edit, HWND_TOP, r.left + px(8), ey, r.right - r.left - px(14), th, SWP_SHOWWINDOW);
            SetFocus(self.edit);
            let n = text.encode_utf16().count();
            SendMessageW(self.edit, EM_SETSEL, if initial.is_some() { n } else { 0 }, n as isize);
        }
    }

    pub fn commit_edit(&mut self, save: bool) {
        if self.edit.is_null() || unsafe { IsWindowVisible(self.edit) } == 0 {
            return;
        }
        unsafe {
            let text = get_text(self.edit);
            ShowWindow(self.edit, SW_HIDE);
            let (r, c) = self.edit_cell;
            if save {
                let mut cells = self.target_cells();
                if !cells.contains(&(r, c)) {
                    cells = vec![(r, c)];
                }
                self.fill_cells(&cells, &text);
            }
            SetFocus(self.hwnd);
        }
    }
}

#[derive(Clone, Debug)]
pub enum GridEv {
    /// row, column, previous text
    CellChanged(usize, usize, String),
    CheckChanged(usize),
    HeaderClick(usize),
    Sorted,
    KeyDown(u32),
    DblClick(usize, usize),
    Click(usize, usize),
    RightClick(usize, usize, i32, i32),
    Paste(String),
}

pub type GridHandler = fn(form: usize, ctl: usize, ev: &GridEv) -> bool;

thread_local! {
    static HANDLER: std::cell::Cell<Option<GridHandler>> = std::cell::Cell::new(None);
}

pub fn set_handler(h: GridHandler) {
    HANDLER.with(|c| c.set(Some(h)));
}

fn fire_grid(g: &Grid, ev: GridEv) -> bool {
    let (f, c) = (g.form, g.ctl);
    match HANDLER.with(|h| h.get()) {
        Some(h) => h(f, c, &ev),
        None => false,
    }
}

pub fn pre_translate(_msg: &MSG) -> bool {
    false
}

unsafe extern "system" fn inplace_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    let parent = GetParent(hwnd);
    match msg {
        WM_GETDLGCODE => return (DLGC_WANTALLKEYS | DLGC_WANTARROWS | DLGC_WANTCHARS) as LRESULT,
        WM_KEYDOWN => match wp as u16 {
            VK_RETURN => {
                with(parent, |g| g.commit_edit(true));
                return 0;
            }
            VK_ESCAPE => {
                with(parent, |g| g.commit_edit(false));
                return 0;
            }
            VK_UP | VK_DOWN | VK_TAB => {
                with(parent, |g| g.commit_edit(true));
                SendMessageW(parent, WM_KEYDOWN, wp, lp);
                return 0;
            }
            _ => {}
        },
        WM_CHAR if wp == 13 || wp == 27 || wp == 9 => return 0,
        WM_KILLFOCUS => {
            let to = wp as HWND;
            if to != parent {
                with(parent, |g| g.commit_edit(true));
            }
        }
        _ => {}
    }
    windows_sys::Win32::UI::Shell::DefSubclassProc(hwnd, msg, wp, lp)
}

unsafe extern "system" fn grid_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let gp: Option<*mut Grid> = GRIDS.with(|m| m.borrow_mut().get_mut(&(hwnd as isize)).map(|b| b.as_mut() as *mut Grid));
    let Some(gp) = gp else {
        return DefWindowProcW(hwnd, msg, wp, lp);
    };
    let g = &mut *gp;
    match msg {
        WM_ERASEBKGND => return 1,
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let dc = BeginPaint(hwnd, &mut ps);
            let cr = client_rect(hwnd);
            let buf = Buffer::new(dc, cr.right, cr.bottom);
            g.paint(buf.dc, &cr);
            g.paint_scrollbars(buf.dc);
            buf.blit();
            drop(buf);
            EndPaint(hwnd, &ps);
            return 0;
        }
        WM_SIZE => {
            g.update_scroll();
            g.invalidate();
        }
        WM_NCPAINT => {
            let r = DefWindowProcW(hwnd, msg, wp, lp);
            paint_border(hwnd);
            return r;
        }
        WM_GETDLGCODE => return (DLGC_WANTARROWS | DLGC_WANTCHARS | DLGC_WANTTAB) as LRESULT,
        WM_SETFOCUS | WM_KILLFOCUS => g.invalidate(),
        WM_MOUSEMOVE => {
            let (x, y) = (get_x(lp), get_y(lp));
            if let Some((which, _, _)) = g.sb_drag {
                g.bar_drag(which, x, y);
                g.update_scroll();
                g.invalidate();
                return 0;
            }
            if g.sel_drag {
                if let Some((r, c)) = g.hit(x.max(0), y.max(g.header_h)) {
                    if r > 0 && (r, c) != (g.row, g.col) && c >= g.fixed_cols {
                        g.row = r;
                        g.col = c;
                        g.ensure_visible();
                        g.invalidate();
                    }
                }
                return 0;
            }
            let bar = g.bar_at(x, y);
            if bar != g.sb_hot {
                g.sb_hot = bar;
                g.invalidate();
                let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                TrackMouseEvent(&mut tme);
            }
            let hot = g.hit(x, y).map(|(r, _)| r).unwrap_or(0);
            if hot != g.hot_row {
                g.hot_row = hot;
                g.invalidate();
                let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                TrackMouseEvent(&mut tme);
            }
        }
        WM_MOUSELEAVE => {
            if g.hot_row != 0 || g.sb_hot != 0 {
                g.hot_row = 0;
                g.sb_hot = 0;
                g.invalidate();
            }
        }
        WM_LBUTTONUP if g.sel_drag => {
            g.sel_drag = false;
            ReleaseCapture();
            return 0;
        }
        WM_LBUTTONUP if g.sb_drag.is_some() => {
            g.sb_drag = None;
            ReleaseCapture();
            g.invalidate();
            return 0;
        }
        WM_LBUTTONDOWN if g.bar_at(get_x(lp), get_y(lp)) != 0 => {
            let (x, y) = (get_x(lp), get_y(lp));
            let which = g.bar_at(x, y);
            let thumb = if which == 1 { g.vbar() } else { g.hbar() }.map(|b| b.1);
            if let Some(t) = thumb {
                let on_thumb = if which == 1 { y >= t.top && y < t.bottom } else { x >= t.left && x < t.right };
                if on_thumb {
                    g.sb_drag = Some((which, if which == 1 { y } else { x }, if which == 1 { g.top as i32 } else { g.scroll_x }));
                    SetCapture(hwnd);
                } else if which == 1 {
                    let page = g.vis_rows.max(1);
                    g.top = if y < t.top { g.top.saturating_sub(page) } else { g.top + page };
                } else {
                    let page = (client_rect(hwnd).right - g.fixed_width()).max(1);
                    g.scroll_x += if x < t.left { -page } else { page };
                    g.scroll_x = g.scroll_x.clamp(0, (g.content_width() - client_rect(hwnd).right).max(0));
                }
            }
            g.update_scroll();
            g.invalidate();
            return 0;
        }
        WM_LBUTTONDOWN | WM_RBUTTONDOWN => {
            SetFocus(hwnd);
            let (x, y) = (get_x(lp), get_y(lp));
            if let Some((r, c)) = g.hit(x, y) {
                if r == 0 {
                    if msg == WM_LBUTTONDOWN {
                        if g.cols[c].check && g.header_check {
                            let all = !g.checked.iter().all(|&b| b);
                            for i in 0..g.checked.len() {
                                g.checked[i] = all;
                            }
                            g.invalidate();
                            fire_grid(g, GridEv::CheckChanged(0));
                        } else {
                            fire_grid(g, GridEv::HeaderClick(c));
                        }
                    }
                } else {
                    g.commit_edit(true);
                    if msg == WM_LBUTTONDOWN && g.cols[c].toggle && g.editable && !g.cols[c].readonly {
                        let old = g.cell(r, c).to_string();
                        let now = if old == "1" { "0" } else { "1" };
                        g.set_cell(r, c, now);
                        g.row = r;
                        g.col = c;
                        g.invalidate();
                        fire_grid(g, GridEv::CellChanged(r, c, old));
                    } else if msg == WM_LBUTTONDOWN && g.cols[c].check {
                        if let Some(v) = g.checked.get_mut(r - 1) {
                            *v = !*v;
                        }
                        g.row = r;
                        g.invalidate();
                        fire_grid(g, GridEv::CheckChanged(r));
                    } else {
                        let list = g.editable && c >= g.fixed_cols && !g.cols[c].readonly && g.has_choices(c);
                        let on_arrow = msg == WM_LBUTTONDOWN && list && (g.cell(r, c).trim().is_empty() || g.cell_rect(r, c).map(|cr| x >= cr.right - px(24)).unwrap_or(false));
                        g.row = r;
                        if c >= g.fixed_cols {
                            g.col = c;
                        }
                        let shift = windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x10) < 0;
                        if msg == WM_LBUTTONDOWN && !shift {
                            g.anchor = (g.row, g.col);
                        }
                        // drag to select a block, also in read-only tables (to copy it)
                        if msg == WM_LBUTTONDOWN {
                            g.sel_drag = true;
                            SetCapture(hwnd);
                        }
                        g.ensure_visible();
                        g.invalidate();
                        if on_arrow {
                            g.sel_drag = false;
                            ReleaseCapture();
                            g.drop_down();
                            return 0;
                        }
                        if msg == WM_LBUTTONDOWN {
                            fire_grid(g, GridEv::Click(r, c));
                        } else {
                            let mut pt = POINT { x, y };
                            ClientToScreen(hwnd, &mut pt);
                            fire_grid(g, GridEv::RightClick(r, c, pt.x, pt.y));
                        }
                    }
                }
            }
            return 0;
        }
        WM_LBUTTONDBLCLK => {
            let (x, y) = (get_x(lp), get_y(lp));
            if let Some((r, c)) = g.hit(x, y) {
                if r > 0 && !g.cols[c].check {
                    if !fire_grid(g, GridEv::DblClick(r, c)) {
                        g.row = r;
                        g.col = c;
                        let editable = g.editable && c >= g.fixed_cols && !g.cols[c].readonly && !g.cols[c].toggle;
                        if editable {
                            g.start_edit(None);
                        }
                    }
                } else if r == 0 {
                    fire_grid(g, GridEv::HeaderClick(c));
                }
            }
            return 0;
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp >> 16) & 0xFFFF) as i16 as i32;
            let steps = (delta / 120 * 3).unsigned_abs() as usize;
            if loword(wp) as u32 & 0x0004 != 0 {
                // MK_SHIFT: the wheel scrolls sideways
                g.scroll_x = (g.scroll_x - delta / 120 * px(60)).clamp(0, (g.content_width() - client_rect(hwnd).right).max(0));
            } else if delta < 0 {
                g.top = (g.top + steps).min(g.rows.len().saturating_sub(1));
            } else {
                g.top = g.top.saturating_sub(steps);
            }
            g.update_scroll();
            g.invalidate();
            return 0;
        }
        WM_VSCROLL => {
            let code = loword(wp) as i32;
            let n = g.rows.len();
            match code {
                c if c == SB_LINEDOWN as i32 => g.top = (g.top + 1).min(n.saturating_sub(1)),
                c if c == SB_LINEUP as i32 => g.top = g.top.saturating_sub(1),
                c if c == SB_PAGEDOWN as i32 => g.top = (g.top + 10).min(n.saturating_sub(1)),
                c if c == SB_PAGEUP as i32 => g.top = g.top.saturating_sub(10),
                c if c == SB_THUMBTRACK as i32 || c == SB_THUMBPOSITION as i32 => g.top = hiword(wp) as usize,
                _ => {}
            }
            g.update_scroll();
            g.invalidate();
            return 0;
        }
        WM_HSCROLL => {
            let code = loword(wp) as i32;
            match code {
                c if c == SB_LINERIGHT as i32 => g.scroll_x += 20,
                c if c == SB_LINELEFT as i32 => g.scroll_x -= 20,
                c if c == SB_PAGERIGHT as i32 => g.scroll_x += 200,
                c if c == SB_PAGELEFT as i32 => g.scroll_x -= 200,
                c if c == SB_THUMBTRACK as i32 || c == SB_THUMBPOSITION as i32 => {
                    let mut si: SCROLLINFO = std::mem::zeroed();
                    si.cbSize = std::mem::size_of::<SCROLLINFO>() as u32;
                    si.fMask = SIF_TRACKPOS;
                    GetScrollInfo(hwnd, SB_HORZ, &mut si);
                    g.scroll_x = si.nTrackPos;
                }
                _ => {}
            }
            g.scroll_x = g.scroll_x.clamp(0, (g.content_width() - client_rect(hwnd).right).max(0));
            g.update_scroll();
            g.invalidate();
            return 0;
        }
        WM_KEYDOWN => {
            if fire_grid(g, GridEv::KeyDown(wp as u32)) {
                return 0;
            }
            let vis = g.vis_cols();
            let pos = vis.iter().position(|&c| c == g.col).unwrap_or(0);
            let shift = windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x10) < 0;
            match wp as u16 {
                VK_DOWN => {
                    if g.row < g.rows.len() {
                        g.row += 1;
                    }
                }
                VK_UP => {
                    if g.row > 1 {
                        g.row -= 1;
                    }
                }
                VK_RIGHT | VK_TAB => {
                    if pos + 1 < vis.len() {
                        g.col = vis[pos + 1];
                    } else if wp as u16 == VK_TAB && g.row < g.rows.len() {
                        g.row += 1;
                        g.col = 0;
                        g.fix_col();
                    }
                }
                VK_LEFT => {
                    if pos > 0 && vis[pos - 1] >= g.fixed_cols {
                        g.col = vis[pos - 1];
                    }
                }
                VK_HOME => g.row = 1.min(g.rows.len()),
                VK_END => g.row = g.rows.len(),
                VK_NEXT => g.row = (g.row + 10).min(g.rows.len()),
                VK_PRIOR => g.row = g.row.saturating_sub(10).max(1.min(g.rows.len())),
                VK_F2 | VK_RETURN => {
                    g.start_edit(None);
                    return 0;
                }
                VK_DELETE => {
                    if g.editable {
                        let cells = g.target_cells();
                        g.fill_cells(&cells, "");
                    }
                    return 0;
                }
                _ => return 0,
            }
            if !shift {
                g.anchor = (g.row, g.col);
            }
            g.ensure_visible();
            g.invalidate();
            return 0;
        }
        WM_CHAR => {
            let ch = char::from_u32(wp as u32).unwrap_or('\0');
            if ch == '\u{1}' {
                // Ctrl+A: the whole table
                let vis: Vec<usize> = g.vis_cols().into_iter().filter(|&c| c >= g.fixed_cols).collect();
                if let (Some(&first), Some(&last)) = (vis.first(), vis.last()) {
                    if !g.rows.is_empty() {
                        g.anchor = (1, first);
                        g.row = g.rows.len();
                        g.col = last;
                        g.invalidate();
                    }
                }
                return 0;
            }
            if ch == '\u{3}' {
                // Ctrl+C: the block as tab-separated text (pastes into Excel)
                set_clipboard_text(&g.block_text());
                return 0;
            }
            if ch == '\u{16}' {
                // Ctrl+V
                if let Some(t) = clipboard_text() {
                    fire_grid(g, GridEv::Paste(t));
                }
                return 0;
            }
            if ch >= ' ' && g.row > 0 {
                if g.has_choices(g.col) {
                    g.start_edit(None);
                } else {
                    g.start_edit(Some(ch));
                }
            }
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

/// TStyleServices.PaintBorder: themed EDIT text background over the 2 px client edge.
pub unsafe fn paint_border(hwnd: HWND) {
    let dc = GetWindowDC(hwnd);
    let mut wr = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetWindowRect(hwnd, &mut wr);
    let r = rect(0, 0, wr.right - wr.left, wr.bottom - wr.top);
    let b = GetSystemMetrics(SM_CXEDGE);
    ExcludeClipRect(dc, r.left + b, r.top + b, r.right - b, r.bottom - b);
    let theme = OpenThemeData(hwnd, wide("EDIT").as_ptr());
    if theme != 0 {
        DrawThemeBackground(theme, dc, 1 /* EP_EDITTEXT */, 1 /* ETS_NORMAL */, &r, null());
        CloseThemeData(theme);
    }
    ReleaseDC(hwnd, dc);
}

pub fn set_clipboard_text(text: &str) {
    use windows_sys::Win32::System::DataExchange::*;
    use windows_sys::Win32::System::Memory::*;
    unsafe {
        let w = wide(text);
        if OpenClipboard(null_mut()) == 0 {
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
}

pub fn clipboard_text() -> Option<String> {
    use windows_sys::Win32::System::DataExchange::*;
    use windows_sys::Win32::System::Memory::*;
    unsafe {
        if OpenClipboard(null_mut()) == 0 {
            return None;
        }
        let h = GetClipboardData(13 /* CF_UNICODETEXT */);
        let mut out = None;
        if !h.is_null() {
            let p = GlobalLock(h as _) as *const u16;
            if !p.is_null() {
                let mut n = 0;
                while *p.add(n) != 0 {
                    n += 1;
                }
                out = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
                GlobalUnlock(h as _);
            }
        }
        CloseClipboard();
        out
    }
}

pub fn ev_name(ev: &Ev) -> &'static str {
    match ev {
        Ev::Click => "click",
        _ => "",
    }
}
