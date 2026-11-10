//! QA helpers: capture a window to PNG (PrintWindow) and dump control geometry.

use super::vcl::{form, Form};
use super::win::*;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Storage::Xps::PrintWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub unsafe fn capture(hwnd: HWND, path: &str) -> Result<(), String> {
    let mut wr = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetWindowRect(hwnd, &mut wr);
    let (w, h) = (wr.right - wr.left, wr.bottom - wr.top);
    let sdc = GetDC(null_mut());
    let mdc = CreateCompatibleDC(sdc);
    let mut bi: BITMAPINFO = std::mem::zeroed();
    bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = w;
    bi.bmiHeader.biHeight = -h;
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    let mut bits: *mut core::ffi::c_void = null_mut();
    let bmp = CreateDIBSection(sdc, &bi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    let old = SelectObject(mdc, bmp as _);
    if std::env::var_os("NERAMIT_SCREENSHOT").is_some() {
        BitBlt(mdc, 0, 0, w, h, sdc, wr.left, wr.top, SRCCOPY);
    } else {
        PrintWindow(hwnd, mdc, 2);
    }
    let mut px = vec![0u8; (w * h * 4) as usize];
    std::ptr::copy_nonoverlapping(bits as *const u8, px.as_mut_ptr(), px.len());
    SelectObject(mdc, old);
    DeleteObject(bmp as _);
    DeleteDC(mdc);
    ReleaseDC(null_mut(), sdc);
    // BGRA -> RGBA, opaque
    for p in px.chunks_mut(4) {
        p.swap(0, 2);
        p[3] = 255;
    }
    image::save_buffer(path, &px, w as u32, h as u32, image::ColorType::Rgba8).map_err(|e| e.to_string())
}

/// Text dump of every control: name class visible rect(relative to form client) caption.
pub unsafe fn dump(f: &Form) -> String {
    let mut out = String::new();
    let mut origin = POINT { x: 0, y: 0 };
    ClientToScreen(f.hwnd, &mut origin);
    for (i, c) in f.ctls.iter().enumerate() {
        let (x, y) = abs_pos(f, i);
        let _ = origin;
        let tw = if c.class == "TCheckBox" || c.class == "TButton" || c.class == "TLabel" { super::win::measure(&c.font, &c.caption).cx } else { 0 };
        out.push_str(&format!("{}\t{}\t{}\t{},{},{}x{}\t{}\ttw={}\n", c.name, c.class, c.visible as u8, x, y, c.w, c.h, c.caption.replace('\n', "\\n"), tw));
    }
    out
}

pub fn abs_pos(f: &Form, id: usize) -> (i32, i32) {
    let mut x = 0;
    let mut y = 0;
    let mut i = Some(id);
    while let Some(k) = i {
        if k == 0 {
            break;
        }
        x += f.ctls[k].x;
        y += f.ctls[k].y;
        i = f.ctls[k].parent;
    }
    (x, y)
}

/// Run the normal message loop for `ms` milliseconds (a thread timer ends it).
pub unsafe fn pump(ms: u32) {
    let timer = SetTimer(null_mut(), 0, ms, None);
    let mut msg: MSG = std::mem::zeroed();
    while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
        if msg.message == WM_TIMER && msg.hwnd.is_null() && msg.wParam == timer {
            break;
        }
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    KillTimer(null_mut(), timer);
}

pub fn form_hwnd(i: usize) -> HWND {
    form(i).hwnd
}

