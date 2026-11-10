//! Thai text with full OpenType shaping. GDI's DrawText draws the bundled Noto Sans Thai
//! without its mark-to-mark positioning, so a tone mark after an upper vowel (ตั้ง, ค้ำ, ที่)
//! sinks into the vowel. Text with such marks is shaped here with rustybuzz (HarfBuzz) and drawn
//! glyph by glyph with ExtTextOut; GDI still rasterises the glyphs, so they stay as sharp.

use std::cell::RefCell;
use std::ptr::null;
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Globalization::{ScriptBreak, ScriptItemize, SCRIPT_ITEM, SCRIPT_LOGATTR};
use windows_sys::Win32::Graphics::Gdi::*;

thread_local! {
    static FACES: RefCell<Vec<(&'static str, rustybuzz::Face<'static>)>> = RefCell::new(
        super::theme::font_files().iter().filter_map(|(name, data)| rustybuzz::Face::from_slice(data, 0).map(|f| (*name, f))).collect(),
    );
}

/// Thai above and below marks: the text needs shaping when it has one of these.
fn has_thai_marks(s: &str) -> bool {
    s.chars().any(|c| matches!(c as u32, 0x0E31 | 0x0E33..=0x0E3A | 0x0E47..=0x0E4E))
}

/// One shaped line: glyph ids, glyph origins relative to the line start (y down) and width.
struct Run {
    glyphs: Vec<u16>,
    xs: Vec<i32>,
    ys: Vec<i32>,
    width: i32,
    /// byte offset in the text where each glyph's cluster starts
    clusters: Vec<usize>,
}

unsafe fn face_name(dc: HDC) -> String {
    let mut buf = [0u16; 64];
    let n = GetTextFaceW(dc, buf.len() as i32, buf.as_mut_ptr());
    String::from_utf16_lossy(&buf[..(n.max(1) as usize - 1).min(buf.len())])
}

/// Shapes `s` with the font selected in `dc`. None when the font is not one of ours or the
/// text has a character the font does not have (DrawText then falls back as before).
unsafe fn shape(dc: HDC, s: &str) -> Option<Run> {
    let name = face_name(dc);
    let mut tm: TEXTMETRICW = std::mem::zeroed();
    GetTextMetricsW(dc, &mut tm);
    let ppem = (tm.tmHeight - tm.tmInternalLeading).max(1) as f64;
    FACES.with(|faces| {
        let faces = faces.borrow();
        let face = &faces.iter().find(|(n, _)| n.eq_ignore_ascii_case(&name))?.1;
        if s.chars().any(|c| !c.is_control() && face.glyph_index(c).is_none()) {
            return None;
        }
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(s);
        buf.guess_segment_properties();
        let out = rustybuzz::shape(face, &[], buf);
        let (infos, pos) = (out.glyph_infos(), out.glyph_positions());
        let glyphs: Vec<u16> = infos.iter().map(|g| g.glyph_id as u16).collect();
        let n = glyphs.len();
        // hinted GDI advances keep the spacing of the rest of the UI
        let mut gdi = vec![0i32; n];
        if n > 0 {
            GetCharWidthI(dc, 0, n as u32, glyphs.as_ptr(), gdi.as_mut_ptr());
        }
        let k = ppem / face.units_per_em() as f64;
        let (mut xs, mut ys) = (Vec::with_capacity(n), Vec::with_capacity(n));
        let mut pen = 0.0f64;
        for i in 0..n {
            xs.push((pen + pos[i].x_offset as f64 * k).round() as i32);
            ys.push((-pos[i].y_offset as f64 * k).round() as i32);
            if pos[i].x_advance != 0 {
                // the font's own advance is hinted by GDI; kerning comes from the shaping
                let own = face.glyph_hor_advance(rustybuzz::ttf_parser::GlyphId(glyphs[i])).unwrap_or(0) as i32;
                pen += gdi[i] as f64 + (pos[i].x_advance - own) as f64 * k;
            }
        }
        let clusters = infos.iter().map(|g| g.cluster as usize).collect();
        Some(Run { glyphs, xs, ys, width: pen.round() as i32, clusters })
    })
}

unsafe fn draw_run(dc: HDC, x: i32, y: i32, run: &Run, clip: Option<&RECT>) {
    let n = run.glyphs.len();
    if n == 0 {
        return;
    }
    // ETO_PDY: (dx, dy) to the next glyph origin
    let mut d = vec![0i32; 2 * n];
    for i in 0..n {
        let (nx, ny) = if i + 1 < n { (run.xs[i + 1], run.ys[i + 1]) } else { (run.width, 0) };
        d[2 * i] = nx - run.xs[i];
        d[2 * i + 1] = ny - run.ys[i];
    }
    let old = SetTextAlign(dc, TA_LEFT | TA_TOP | TA_NOUPDATECP);
    let opts = ETO_GLYPH_INDEX | ETO_PDY | if clip.is_some() { ETO_CLIPPED } else { 0 };
    ExtTextOutW(dc, x + run.xs[0], y + run.ys[0], opts, clip.map(|r| r as *const RECT).unwrap_or(null()), run.glyphs.as_ptr(), n as u32, d.as_ptr());
    SetTextAlign(dc, old);
}

/// Byte offsets where a line may begin (Uniscribe word breaks, Thai dictionary included).
unsafe fn break_points(s: &str) -> Vec<usize> {
    let w: Vec<u16> = s.encode_utf16().collect();
    let mut offs = Vec::with_capacity(w.len() + 1);
    let mut b = 0;
    for c in s.chars() {
        for _ in 0..c.len_utf16() {
            offs.push(b);
        }
        b += c.len_utf8();
    }
    offs.push(s.len());
    let mut items: Vec<SCRIPT_ITEM> = vec![std::mem::zeroed(); w.len() + 2];
    let mut n = 0;
    let mut attrs = vec![SCRIPT_LOGATTR { _bitfield: 0 }; w.len()];
    if !w.is_empty() && ScriptItemize(w.as_ptr(), w.len() as i32, items.len() as i32 - 1, null(), null(), items.as_mut_ptr(), &mut n) == 0 {
        for k in 0..n as usize {
            let (a, e) = (items[k].iCharPos as usize, (items[k + 1].iCharPos as usize).min(w.len()));
            if e > a {
                ScriptBreak(w[a..].as_ptr(), (e - a) as i32, &items[k].a, attrs[a..].as_mut_ptr());
            }
        }
    }
    let mut out = vec![];
    for i in 1..w.len() {
        let soft = attrs[i]._bitfield & 1 != 0;
        let after_space = w[i - 1] == b' ' as u16;
        if soft || after_space {
            out.push(offs[i]);
        }
    }
    out.push(s.len());
    out.dedup();
    out
}

/// Lines of one paragraph that fit `avail` pixels.
unsafe fn wrap(dc: HDC, s: &str, avail: i32) -> Option<Vec<Run>> {
    let breaks = break_points(s);
    let mut lines = vec![];
    let mut start = 0;
    while start < s.len() {
        let mut best: Option<(usize, Run)> = None;
        for &b in breaks.iter().filter(|&&b| b > start) {
            let run = shape(dc, s[start..b].trim_end())?;
            if run.width <= avail || best.is_none() {
                let fits = run.width <= avail;
                best = Some((b, run));
                if !fits {
                    break;
                }
            } else {
                break;
            }
        }
        let (b, run) = best?;
        lines.push(run);
        start = b;
        while start < s.len() && s.as_bytes()[start] == b' ' {
            start += 1;
        }
    }
    if lines.is_empty() {
        lines.push(shape(dc, "")?);
    }
    Some(lines)
}

/// The longest start of `s` that fits with "…" after it.
unsafe fn ellipsize(dc: HDC, s: &str, run: Run, avail: i32) -> Option<Run> {
    if run.width <= avail {
        return Some(run);
    }
    let mut cuts: Vec<usize> = run.clusters.clone();
    cuts.sort_unstable();
    cuts.dedup();
    for &c in cuts.iter().rev() {
        if c == 0 || !s.is_char_boundary(c) {
            continue;
        }
        let r = shape(dc, &format!("{}…", s[..c].trim_end()))?;
        if r.width <= avail {
            return Some(r);
        }
    }
    shape(dc, "…")
}

/// DrawText for text with Thai marks; None leaves the call to DrawText.
pub unsafe fn draw_text(dc: HDC, s: &str, r: &mut RECT, flags: u32) -> Option<i32> {
    if !has_thai_marks(s) || s.contains('\t') || (flags & DT_NOPREFIX == 0 && s.contains('&')) || flags & (DT_PATH_ELLIPSIS | DT_WORD_ELLIPSIS | DT_EDITCONTROL) != 0 {
        return None;
    }
    let mut tm: TEXTMETRICW = std::mem::zeroed();
    GetTextMetricsW(dc, &mut tm);
    let line_h = tm.tmHeight + if flags & DT_EXTERNALLEADING != 0 { tm.tmExternalLeading } else { 0 };
    let avail = r.right - r.left;
    let single = flags & DT_SINGLELINE != 0;
    let mut lines: Vec<Run> = vec![];
    if single {
        let t: String = s.chars().filter(|&c| c != '\r' && c != '\n').collect();
        let run = shape(dc, &t)?;
        lines.push(if flags & DT_END_ELLIPSIS != 0 && flags & DT_CALCRECT == 0 { ellipsize(dc, &t, run, avail)? } else { run });
    } else {
        for para in s.split('\n') {
            let para = para.trim_end_matches('\r');
            if flags & DT_WORDBREAK != 0 && avail > 0 {
                lines.extend(wrap(dc, para, avail)?);
            } else {
                lines.push(shape(dc, para)?);
            }
        }
    }
    let total_h = line_h * lines.len() as i32;
    let widest = lines.iter().map(|l| l.width).max().unwrap_or(0);
    if flags & DT_CALCRECT != 0 {
        if flags & DT_WORDBREAK == 0 || single {
            r.right = r.left + widest;
        }
        r.bottom = r.top + total_h;
        return Some(total_h);
    }
    let mut y = r.top;
    if single {
        if flags & DT_VCENTER != 0 {
            y = r.top + (r.bottom - r.top - line_h) / 2;
        } else if flags & DT_BOTTOM != 0 {
            y = r.bottom - line_h;
        }
    }
    let clip = if flags & DT_NOCLIP != 0 { None } else { Some(*r) };
    for l in &lines {
        let x = if flags & DT_CENTER != 0 {
            r.left + (avail - l.width) / 2
        } else if flags & DT_RIGHT != 0 {
            r.right - l.width
        } else {
            r.left
        };
        draw_run(dc, x, y, l, clip.as_ref());
        y += line_h;
    }
    Some(total_h)
}
