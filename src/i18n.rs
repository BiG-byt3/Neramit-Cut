//! Thai / English UI text. Form captions are keyed by form and component name; messages by id.

use serde_json::Value;
use std::cell::Cell;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Th,
}

thread_local! {
    static LANG: Cell<Lang> = const { Cell::new(Lang::Th) };
    static MISSING: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn lang() -> Lang {
    LANG.with(|l| l.get())
}

pub fn set_lang(l: Lang) {
    LANG.with(|c| c.set(l));
}

fn table() -> &'static Value {
    static T: OnceLock<Value> = OnceLock::new();
    T.get_or_init(|| serde_json::from_str(include_str!("../assets/i18n.json")).unwrap_or(Value::Null))
}

fn pick(v: &Value) -> Option<String> {
    let a = v.as_array()?;
    let i = if lang() == Lang::Th { 1 } else { 0 };
    a.get(i).and_then(|s| s.as_str()).map(String::from)
}

/// Caption/Hint of a form component.
pub fn form_text(form: &str, comp: &str, prop: &str) -> Option<String> {
    pick(table().get("forms")?.get(form)?.get(comp)?.get(prop)?)
}

/// A message or short text by id.
pub fn t(id: &str) -> String {
    table().get("msg").and_then(|m| m.get(id)).and_then(pick).unwrap_or_else(|| id.to_string())
}

/// Text with {0}, {1} ... placeholders.
pub fn tf(id: &str, args: &[&str]) -> String {
    let mut s = t(id);
    for (i, a) in args.iter().enumerate() {
        s = s.replace(&format!("{{{i}}}"), a);
    }
    s
}

/// English UI text in the current language (composite captions such as
/// "Technical settings - Linear" are translated piece by piece).
pub fn tr(en: &str) -> String {
    if lang() == Lang::En || en.is_empty() {
        return en.to_string();
    }
    let Some(d) = table().get("dict") else { return en.to_string() };
    let look = |s: &str| -> Option<String> {
        if let Some(v) = d.get(s).and_then(|v| v.as_str()) {
            return Some(v.to_string());
        }
        // "Text:" / "Text..." / "&Text"
        for suf in [":", "...", " ...", " :"] {
            if let Some(base) = s.strip_suffix(suf) {
                if let Some(v) = d.get(base).and_then(|v| v.as_str()) {
                    return Some(format!("{v}{suf}"));
                }
            }
        }
        let plain = s.replace('&', "");
        if plain != s {
            return d.get(plain.as_str()).and_then(|v| v.as_str()).map(String::from);
        }
        None
    };
    if let Some(v) = look(en) {
        return v;
    }
    // split on separators and translate the pieces
    let seps = [" / ", " - ", " -", "-", " (", ")", " [", "]", "\n"];
    let mut out = String::new();
    let mut rest = en;
    let mut changed = false;
    while !rest.is_empty() {
        let next = seps.iter().filter_map(|s| rest.find(s).map(|i| (i, *s))).min_by_key(|(i, s)| (*i, usize::MAX - s.len()));
        let (piece, sep, tail) = match next {
            Some((i, s)) => (&rest[..i], s, &rest[i + s.len()..]),
            None => (rest, "", ""),
        };
        match look(piece.trim()) {
            Some(v) if !piece.trim().is_empty() => {
                out.push_str(&piece.replace(piece.trim(), &v));
                changed = true;
            }
            _ => out.push_str(piece),
        }
        out.push_str(sep);
        rest = tail;
    }
    if changed {
        out
    } else {
        if en.chars().any(|c| c.is_ascii_alphabetic()) {
            MISSING.with(|m| {
                let mut m = m.borrow_mut();
                if !m.iter().any(|x| x == en) {
                    m.push(en.to_string());
                }
            });
        }
        en.to_string()
    }
}

/// English texts that had no Thai translation (QA).
pub fn missing() -> Vec<String> {
    MISSING.with(|m| m.borrow().clone())
}

/// Runtime Caption/Hint of a form component: explicit entry, else the translated design text.
pub fn caption(form: &str, comp: &str, prop: &str, design: &str) -> String {
    if let Some(k) = prop.strip_prefix("Item").and_then(|k| k.parse::<usize>().ok()) {
        let it = table().get("forms").and_then(|f| f.get(form)).and_then(|f| f.get(comp)).and_then(|c| c.get("Items")).and_then(|i| i.get(k)).and_then(pick);
        return it.unwrap_or_else(|| tr(design));
    }
    form_text(form, comp, prop).unwrap_or_else(|| tr(design))
}
