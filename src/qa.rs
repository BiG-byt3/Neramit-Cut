//! Screenshot mode for visual QA:
//! `NeramitCut.exe --shot <dir> [th] [start] [stats=N] [dlg=<name>]...`

use crate::app::{self, app};
use crate::engine;
use crate::i18n::{self, Lang};
use crate::model::*;
use crate::ui::grid;
use crate::ui::shot;
use crate::ui::vcl::{self, Form};
use crate::ui::win::*;
use std::cell::RefCell;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi::UpdateWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

thread_local! {
    static PENDING: RefCell<Option<String>> = const { RefCell::new(None) };
}

unsafe extern "system" fn capture_modal(_h: HWND, _m: u32, id: usize, _t: u32) {
    KillTimer(null_mut(), id);
    let Some(path) = PENDING.with(|p| p.borrow_mut().take()) else { return };
    let modal = vcl::ui().forms.iter_mut().flatten().rev().find(|f| f.modal).map(|f| f.index);
    if let Some(i) = modal {
        let f = vcl::form(i);
        UpdateWindow(f.hwnd);
        shot::pump(300);
        let _ = shot::capture(f.hwnd, &format!("{path}.png"));
        let _ = std::fs::write(format!("{path}.txt"), shot::dump(f));
        f.close(2);
    }
}

unsafe extern "system" fn capture_popup(_h: HWND, _m: u32, id: usize, _t: u32) {
    KillTimer(null_mut(), id);
    let Some(path) = PENDING.with(|p| p.borrow_mut().take()) else { return };
    let menu = FindWindowW(wide("#32768").as_ptr(), std::ptr::null());
    if !menu.is_null() {
        let _ = shot::capture(menu, &format!("{path}.png"));
    }
    EndMenu();
}

unsafe fn dialog(dir: &str, name: &str) {
    PENDING.with(|p| *p.borrow_mut() = Some(format!("{dir}/dlg_{name}")));
    SetTimer(null_mut(), 0, 600, Some(capture_modal));
    match name {
        "tech1d" => crate::dlg::technical_1d(),
        "general" => crate::dlg::technical_general(),
        "iface" => crate::dlg::interface_settings(),
        "lang" => crate::dlg::language(),
        "materials" => crate::dlg::materials(),
        "about" => crate::dlg::about(),
        "project_about" => crate::dlg::project_about(),
        "options" => {
            crate::dlg::project_options(false, app().sol.is_some(), "");
        }
        "history" => crate::dlg::history(),
        "find" => crate::dlg::find_size(),
        "tabular" => crate::dlg::tabular(),
        "dxf" => crate::dlg::dxf_export(),
        "labels_parts" => crate::dlg::labels(false),
        "labels_stock" => crate::dlg::labels(true),
        _ => {}
    }
}

pub unsafe fn shots(f: &mut Form, dir: &str, args: &[String]) -> i32 {
    let _ = std::fs::create_dir_all(dir);
    // the program starts maximized; captures are taken after restoring to a fixed size
    ShowWindow(f.hwnd, SW_SHOWMAXIMIZED);
    UpdateWindow(f.hwnd);
    shot::pump(200);
    ShowWindow(f.hwnd, SW_RESTORE);
    let (w, h) = std::env::var("NERAMIT_SHOT_SIZE").ok().and_then(|s| s.split_once('x').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))).unwrap_or((1366, 800));
    SetWindowPos(f.hwnd, null_mut(), 0, 0, w, h, SWP_NOZORDER);
    UpdateWindow(f.hwnd);
    shot::pump(300);
    for a in args {
        match a.as_str() {
            "th" | "en" => {
                i18n::set_lang(if a == "th" { Lang::Th } else { Lang::En });
                app::apply_language(f);
            }
            "sample" => {
                let s = Project::sample();
                crate::mainwin::fill_parts(f.hwnd_of("sgParts"), &s.parts, 3);
                crate::mainwin::fill_stock(f.hwnd_of("sgStock"), &s.stock, 3, 3);
            }
            "start" => {
                let prj = app::current_project();
                let sol = engine::optimize(&prj, &engine::Progress { cancel: &|| false, report: &|_, _| {} });
                app::show_solution(f, prj, sol, false);
            }
            s if s.starts_with("stats=") => {
                let k: usize = s[6..].parse().unwrap_or(0);
                let pr = f.id("pcResults");
                f.show_page(pr, 1);
                let ps = f.id("pcStatistics1D");
                f.show_page(ps, k);
            }
            s if s.starts_with("dlg=") => {
                dialog(dir, &s[4..]);
            }
            // max / restore / size=WxH: resize the window the way a user does
            "max" => {
                ShowWindow(f.hwnd, SW_MAXIMIZE);
                shot::pump(400);
            }
            "restore" => {
                ShowWindow(f.hwnd, SW_RESTORE);
                shot::pump(400);
            }
            s if s.starts_with("size=") => {
                if let Some((w, h)) = s[5..].split_once('x').and_then(|(a, b)| Some((a.parse::<i32>().ok()?, b.parse::<i32>().ok()?))) {
                    SetWindowPos(f.hwnd, null_mut(), 0, 0, w, h, SWP_NOZORDER | SWP_NOMOVE);
                    shot::pump(400);
                }
            }
            s if s.starts_with("fonttest=") => {
                font_test(&std::path::Path::new(dir).join(&s[9..]));
            }
            s if s.starts_with("report=") => {
                // report=<file.png>: the printed cutting plan
                let _ = crate::export::report_png(&std::path::Path::new(dir).join(&s[7..]));
            }
            s if s.starts_with("pdf=") => {
                if let Some(rep) = crate::results::view(|v| crate::results::report(v, &crate::export::job_name())) {
                    let _ = crate::export::write_report_pdf(&std::path::Path::new(dir).join(&s[4..]), &rep);
                }
            }
            s if s.starts_with("xlsx=") => {
                let _ = crate::export::results_xlsx(&std::path::Path::new(dir).join(&s[5..]));
            }
            s if s.starts_with("job=") => {
                crate::ui::win::set_text(f.hwnd_of("edJobName"), &s[4..].replace('_', " "));
            }
            s if s.starts_with("import=") => {
                // import=<csv>: the parts of a file, as the import dialog would read them (header guessed)
                crate::dlg::qa_import(&s[7..]);
            }
            s if s.starts_with("png=") => {
                // png=<file>: the cutting plan picture (File > Export > Image)
                let _ = crate::export::write_image(f, &std::path::Path::new(dir).join(&s[4..]));
            }
            s if s.starts_with("menu=") => {
                // menu=<item>: choose a menu item (check items toggle)
                let n = &s[5..];
                if let Some(id) = f.try_id(n) {
                    if f.ctls[id].comp.bool("AutoCheck", false) {
                        let now = !crate::ui::menu::is_checked(f.index, n);
                        f.menu_check(n, now);
                    }
                    (vcl::ui().handler.unwrap())(f.index, id, &vcl::Ev::Menu(n.to_string()));
                }
            }
            s if s.starts_with("popup=") => {
                // popup=<toolbar>/<button>: open the button's menu and capture it
                if let Some((tb, b)) = s[6..].split_once('/') {
                    PENDING.with(|p| *p.borrow_mut() = Some(format!("{dir}/popup_{b}")));
                    SetTimer(null_mut(), 0, 500, Some(capture_popup));
                    let id = f.id(tb);
                    let idx = match &f.ctls[id].kind {
                        vcl::Kind::ToolBar { buttons, .. } => buttons.iter().position(|x| x.name == b),
                        _ => None,
                    };
                    if let Some(i) = idx {
                        (vcl::ui().handler.unwrap())(f.index, id, &vcl::Ev::Button(i));
                    }
                }
            }
            s if s.starts_with("tool=") => {
                // tool=<toolbar>/<button>
                if let Some((tb, b)) = s[5..].split_once('/') {
                    let id = f.id(tb);
                    let idx = match &f.ctls[id].kind {
                        vcl::Kind::ToolBar { buttons, .. } => buttons.iter().position(|x| x.name == b),
                        _ => None,
                    };
                    if let Some(i) = idx {
                        (vcl::ui().handler.unwrap())(f.index, id, &vcl::Ev::Button(i));
                    }
                }
            }
            s if s.starts_with("check=") => {
                let n = &s[6..];
                let v = !f.checked(n);
                f.set_checked(n, v);
                if let Some(id) = f.try_id(n) {
                    (vcl::ui().handler.unwrap())(f.index, id, &vcl::Ev::Click);
                }
            }
            _ => {}
        }
    }
    UpdateWindow(f.hwnd);
    shot::pump(800);
    let _ = shot::capture(f.hwnd, &format!("{dir}/main.png"));
    let mut d = shot::dump(f);
    for g in ["sgParts", "sgStock", "g_results_1d"] {
        grid::with(f.hwnd_of(g), |gr| {
            for c in &gr.cols {
                d.push_str(&format!("{g}.col\t{}\tw={}\ttw={}\n", c.title, c.width, measure(&gr.header_font, &c.title).cx));
            }
        });
    }
    let _ = std::fs::write(format!("{dir}/main.txt"), d);
    let _ = std::fs::write(format!("{dir}/missing_th.txt"), i18n::missing().join("\n"));
    0
}

/// Thai sample words: plain DrawTextW (left) and the program's own drawing (right), in the
/// bundled faces and in Leelawadee UI, saved as a PNG.
unsafe fn font_test(path: &std::path::Path) {
    use crate::ui::theme;
    use crate::ui::win::{draw_text, fill, font, rgb, wide, FontSpec};
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::*;
    let words = "ตั้งค่า ค้ำยัน ขาตั้ง ที่ ผู้จัดทำ ปั้น ฝั่ง กั้น ชิ้น ป้าย ญาติ ฐาน ปุ่ม ปู่ ฎีกา";
    let (w, h) = (1500, 420);
    let screen = GetDC(null_mut());
    let mem = CreateCompatibleDC(screen);
    let bmp = CreateCompatibleBitmap(screen, w, h);
    let old = SelectObject(mem, bmp as _);
    fill(mem, &RECT { left: 0, top: 0, right: w, bottom: h }, rgb(255, 255, 255));
    SetBkMode(mem, TRANSPARENT as i32);
    let mut y = 10;
    for (face, size) in [(theme::FACE, 14), (theme::FACE, 24), (theme::FACE_SEMIBOLD, 24), ("Leelawadee UI", 24)] {
        let f = FontSpec::new(face, -size);
        let of = SelectObject(mem, font(&f) as _);
        let ws = wide(words);
        let mut r1 = RECT { left: 10, top: y, right: 740, bottom: y + 90 };
        DrawTextW(mem, ws.as_ptr(), (ws.len() - 1) as i32, &mut r1, DT_LEFT | DT_WORDBREAK | DT_NOPREFIX);
        let mut r2 = RECT { left: 760, top: y, right: w - 10, bottom: y + 90 };
        draw_text(mem, words, &mut r2, DT_LEFT | DT_WORDBREAK | DT_NOPREFIX);
        SelectObject(mem, of);
        y += if size > 20 { 100 } else { 60 };
    }
    // end ellipsis in narrow boxes
    let f = FontSpec::new(theme::FACE, -12);
    let of = SelectObject(mem, font(&f) as _);
    for (i, wbox) in [20, 26, 32, 40, 50, 60, 80].iter().enumerate() {
        let x = 10 + i as i32 * 100;
        let mut r = RECT { left: x, top: y, right: x + wbox, bottom: y + 20 };
        fill(mem, &r, rgb(235, 240, 250));
        draw_text(mem, "แผ่นรอง", &mut r, DT_CENTER | DT_TOP | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
        let mut r = RECT { left: x, top: y + 24, right: x + wbox, bottom: y + 44 };
        fill(mem, &r, rgb(235, 240, 250));
        draw_text(mem, "ค้ำยันตั้ง", &mut r, DT_LEFT | DT_TOP | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
    }
    // composite glyphs through the shaped path (the Thai mark forces it), then plain DrawTextW
    let probe = "ตั้ง… ตั้ง... ตั้ง×2 ตั้งé ตั้ง° ตั้ง·ก ตั้ง—ข";
    let mut r = RECT { left: 760, top: y, right: w - 10, bottom: y + 20 };
    draw_text(mem, probe, &mut r, DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX);
    let ws = wide(probe);
    let mut r = RECT { left: 760, top: y + 24, right: w - 10, bottom: y + 44 };
    DrawTextW(mem, ws.as_ptr(), (ws.len() - 1) as i32, &mut r, DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX);
    SelectObject(mem, of);
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
    ReleaseDC(null_mut(), screen);
    let rgba: Vec<u8> = buf.chunks(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect();
    if let Some(img) = image::RgbaImage::from_raw(w as u32, h as u32, rgba) {
        let _ = img.save(path);
    }
}
