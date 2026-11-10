//! Popup menus drawn in the application style: Noto Sans Thai, an icon column, check marks,
//! rounded hover and thin separators.

use super::theme::{self, px};
use super::vcl::{form, ui};
use super::win::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

struct Item {
    form: usize,
    /// component of the item, None for separators
    ctl: Option<usize>,
}

thread_local! {
    static ITEMS: RefCell<HashMap<usize, Item>> = RefCell::new(HashMap::new());
    static CHECKED: RefCell<HashSet<(usize, String)>> = RefCell::new(HashSet::new());
    static NEXT_SEP: std::cell::Cell<usize> = const { std::cell::Cell::new(0x7000_0000) };
}

/// Owner-draw data of a menu item (`data` is the value given to AppendMenu).
pub fn register_item(form: usize, ctl: Option<usize>, data: usize) {
    ITEMS.with(|m| m.borrow_mut().insert(data, Item { form, ctl }));
}

pub fn separator_data(form: usize) -> usize {
    let d = NEXT_SEP.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    });
    register_item(form, None, d);
    d
}

pub fn owner_form_of_cmd(cmd: u16) -> Option<usize> {
    if let Some(f) = ITEMS.with(|m| m.borrow().get(&(cmd as usize)).map(|i| i.form)) {
        return Some(f);
    }
    for (i, f) in ui().forms.iter().enumerate() {
        if let Some(f) = f {
            if f.menus.contains_key(&cmd) {
                return Some(i);
            }
        }
    }
    None
}

pub fn set_checked(form: usize, name: &str, v: bool) {
    CHECKED.with(|c| {
        if v {
            c.borrow_mut().insert((form, name.to_string()));
        } else {
            c.borrow_mut().remove(&(form, name.to_string()));
        }
    });
}

pub fn is_checked(form: usize, name: &str) -> bool {
    CHECKED.with(|c| c.borrow().contains(&(form, name.to_string())))
}

fn item_font() -> FontSpec {
    theme::font(theme::BODY)
}

const ICON_COL: i32 = 40;

pub unsafe fn measure_item(_hwnd: HWND, mis: &mut MEASUREITEMSTRUCT) {
    let info = ITEMS.with(|m| m.borrow().get(&mis.itemData).map(|i| (i.form, i.ctl)));
    let Some((fi, ctl)) = info else {
        mis.itemHeight = px(32) as u32;
        mis.itemWidth = px(160) as u32;
        return;
    };
    let Some(ci) = ctl else {
        mis.itemHeight = px(9) as u32;
        mis.itemWidth = px(40) as u32;
        return;
    };
    let (text, shortcut) = item_text(fi, ci);
    let tw = measure(&item_font(), &text).cx;
    let sw = if shortcut.is_empty() { 0 } else { measure(&theme::font(theme::SMALL), &shortcut).cx + px(24) };
    mis.itemWidth = (px(ICON_COL) + tw + sw + px(28)) as u32;
    mis.itemHeight = px(32) as u32;
}

pub unsafe fn draw_item(dis: &DRAWITEMSTRUCT) {
    let info = ITEMS.with(|m| m.borrow().get(&dis.itemData).map(|i| (i.form, i.ctl)));
    let Some((fi, ctl)) = info else { return };
    let dc = dis.hDC;
    let r = dis.rcItem;
    fill(dc, &r, theme::SURFACE);
    let Some(ci) = ctl else {
        let y = (r.top + r.bottom) / 2;
        hline(dc, r.left + px(8), r.right - px(8), y, theme::DIVIDER);
        return;
    };
    let f = form(fi);
    let c = &f.ctls[ci];
    let selected = dis.itemState & ODS_SELECTED != 0;
    let disabled = dis.itemState & (ODS_DISABLED | ODS_GRAYED) != 0;
    let checked = dis.itemState & ODS_CHECKED != 0 || is_checked(fi, &c.name);
    if selected && !disabled {
        let hr = RECT { left: r.left + px(4), top: r.top + 1, right: r.right - px(4), bottom: r.bottom - 1 };
        theme::round_rect(dc, &hr, px(4) as f32, Some(theme::ACCENT_SOFT), None);
    }
    let ir = RECT { left: r.left + px(6), top: r.top, right: r.left + px(ICON_COL) - px(4), bottom: r.bottom };
    let icon_color = if disabled { theme::TEXT_DISABLED } else { theme::ACCENT };
    if checked {
        theme::glyph(dc, theme::icon::CHECK, &ir, 14, icon_color);
    } else if let Some(ch) = c.comp.str("Icon").and_then(theme::icon_from_name) {
        theme::glyph(dc, ch, &ir, 16, if disabled { theme::TEXT_DISABLED } else { theme::TEXT_MUTED });
    }
    let (text, shortcut) = item_text(fi, ci);
    let fg = if disabled { theme::TEXT_DISABLED } else { theme::TEXT };
    let tr = RECT { left: r.left + px(ICON_COL), top: r.top, right: r.right - px(12), bottom: r.bottom };
    theme::text(dc, &text, &tr, &item_font(), fg, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    if !shortcut.is_empty() {
        let sr = RECT { left: r.left + px(ICON_COL), top: r.top, right: r.right - px(20), bottom: r.bottom };
        theme::text(dc, &shortcut, &sr, &theme::font(theme::SMALL), theme::TEXT_SUBTLE, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
    }
}

fn item_text(f: usize, c: usize) -> (String, String) {
    let ctl = &form(f).ctls[c];
    (ctl.caption.replace('&', ""), ctl.comp.str("ShortCutText").unwrap_or("").to_string())
}

/// White background for a popup and its submenus.
pub unsafe fn style_popup(m: HMENU) {
    let mut mi: MENUINFO = std::mem::zeroed();
    mi.cbSize = std::mem::size_of::<MENUINFO>() as u32;
    mi.fMask = MIM_BACKGROUND | MIM_APPLYTOSUBMENUS;
    mi.hbrBack = super::vcl::bg_brush(theme::SURFACE);
    SetMenuInfo(m, &mi);
}
