//! Builder for component trees, so windows are designed in code. Lengths are given at
//! 96 DPI and scaled with `theme::px`.

use super::comp::Comp;
use super::theme::{self, px};
use serde_json::{json, Map, Value};

#[derive(Clone, Debug)]
pub struct N(pub Comp);

impl N {
    pub fn new(class: &str, name: &str) -> N {
        N(Comp { class: class.into(), name: name.into(), props: Map::new(), children: vec![] })
    }
    pub fn p(mut self, key: &str, v: impl Into<Value>) -> N {
        self.0.props.insert(key.into(), v.into());
        self
    }
    /// Add a child. Aligned siblings are ordered by position (VCL AlignControls), so children
    /// without an explicit Left/Top get increasing ones in declaration order.
    pub fn kid(mut self, c: N) -> N {
        let mut c = c.0;
        let idx = self.0.children.len() as i64 * 1000 + 1000;
        if !c.props.contains_key("Top") {
            c.props.insert("Top".into(), Value::from(idx));
        }
        if !c.props.contains_key("Left") {
            c.props.insert("Left".into(), Value::from(idx));
        }
        self.0.children.push(c);
        self
    }
    pub fn kids(mut self, cs: Vec<N>) -> N {
        for c in cs {
            self = self.kid(c);
        }
        self
    }
    pub fn caption(self, s: &str) -> N {
        self.p("Caption", s)
    }
    pub fn hint(self, s: &str) -> N {
        self.p("Hint", s).p("ShowHint", true)
    }
    pub fn align(self, a: &str) -> N {
        self.p("Align", a)
    }
    pub fn top(self) -> N {
        self.align("alTop")
    }
    pub fn bottom(self) -> N {
        self.align("alBottom")
    }
    pub fn left(self) -> N {
        self.align("alLeft")
    }
    pub fn right(self) -> N {
        self.align("alRight")
    }
    pub fn client(self) -> N {
        self.align("alClient")
    }
    pub fn x(self, v: i32) -> N {
        self.p("Left", px(v))
    }
    pub fn y(self, v: i32) -> N {
        self.p("Top", px(v))
    }
    pub fn w(self, v: i32) -> N {
        self.p("Width", px(v))
    }
    pub fn h(self, v: i32) -> N {
        self.p("Height", px(v))
    }
    pub fn at(self, x: i32, y: i32, w: i32, h: i32) -> N {
        self.x(x).y(y).w(w).h(h)
    }
    /// AlignWithMargins with margins left, top, right, bottom.
    pub fn m(self, l: i32, t: i32, r: i32, b: i32) -> N {
        self.p("AlignWithMargins", true).p("Margins.Left", px(l)).p("Margins.Top", px(t)).p("Margins.Right", px(r)).p("Margins.Bottom", px(b))
    }
    pub fn pad(self, l: i32, t: i32, r: i32, b: i32) -> N {
        self.p("Padding.Left", px(l)).p("Padding.Top", px(t)).p("Padding.Right", px(r)).p("Padding.Bottom", px(b))
    }
    pub fn color(self, c: u32) -> N {
        self.p("Color", c as i64).p("ParentColor", false)
    }
    pub fn style(self, s: &str) -> N {
        self.p("Style", s)
    }
    pub fn icon(self, s: &str) -> N {
        self.p("Icon", s)
    }
    pub fn font(self, f: &super::win::FontSpec) -> N {
        let mut st: Vec<Value> = vec![];
        if f.bold {
            st.push("fsBold".into());
        }
        self.p("ParentFont", false).p("Font.Name", f.name.clone()).p("Font.Height", f.height).p("Font.Color", f.color as i64).p("Font.Style", Value::Array(st))
    }
    pub fn visible(self, v: bool) -> N {
        self.p("Visible", v)
    }
    pub fn tab(self, order: i32) -> N {
        self.p("TabOrder", order)
    }
    pub fn anchors(self, list: &[&str]) -> N {
        self.p("Anchors", Value::Array(list.iter().map(|s| Value::String((*s).into())).collect()))
    }
    pub fn height_px(&self) -> i32 {
        self.0.int("Height", 0)
    }
    /// Height including AlignWithMargins top/bottom.
    pub fn outer_h(&self) -> i32 {
        let c = &self.0;
        let m = if c.bool("AlignWithMargins", false) { c.int("Margins.Top", 0) + c.int("Margins.Bottom", 0) } else { 0 };
        c.int("Height", 0) + m
    }
}

// ------------------------------------------------------------------ components

pub fn form(class: &str, name: &str, caption: &str, cw: i32, ch: i32) -> N {
    N::new(class, name)
        .caption(caption)
        .p("ClientWidth", px(cw))
        .p("ClientHeight", px(ch))
        .p("Position", "poMainFormCenter")
        .p("BorderStyle", "bsDialog")
        .color(theme::SURFACE)
        .font(&theme::font(theme::BODY))
}

pub fn panel(name: &str) -> N {
    N::new("TPanel", name).p("BevelOuter", "bvNone").p("ShowCaption", false).p("ParentBackground", false)
}

pub fn card(name: &str) -> N {
    panel(name).style("card").color(theme::SURFACE).pad(1, 1, 1, 8)
}

pub fn label(name: &str, caption: &str) -> N {
    N::new("TLabel", name).caption(caption).p("Transparent", true)
}

pub fn muted(name: &str, caption: &str) -> N {
    let mut f = theme::font(theme::SMALL);
    f.color = theme::TEXT_MUTED;
    label(name, caption).font(&f)
}

pub fn title(name: &str, caption: &str, size: i32) -> N {
    label(name, caption).font(&theme::font_semibold(size))
}

pub fn edit(name: &str) -> N {
    N::new("TEdit", name).h(32).p("Text", "")
}

pub fn memo(name: &str) -> N {
    N::new("TMemo", name).p("ScrollBars", "ssVertical")
}

pub fn combo(name: &str, items: &[&str], index: i32) -> N {
    N::new("TComboBox", name).h(32).p("Style", "csDropDownList").p("Items.Strings", Value::Array(items.iter().map(|s| Value::String((*s).into())).collect())).p("ItemIndex", index)
}

pub fn check(name: &str, caption: &str, checked: bool) -> N {
    N::new("TCheckBox", name).caption(caption).p("Checked", checked).h(28)
}

pub fn radio_group(name: &str, caption: &str, items: &[&str], index: i32, columns: i32) -> N {
    N::new("TRadioGroup", name).caption(caption).p("Items.Strings", Value::Array(items.iter().map(|s| Value::String((*s).into())).collect())).p("ItemIndex", index).p("Columns", columns)
}

pub fn button(name: &str, caption: &str) -> N {
    N::new("TButton", name).caption(caption).h(32).w(96)
}

pub fn primary(name: &str, caption: &str) -> N {
    button(name, caption).p("Default", true).style("primary")
}

/// Modal buttons (OK = 1, Cancel = 2, other values are form-defined).
pub fn modal_button(name: &str, caption: &str, result: i32, primary_look: bool) -> N {
    let b = N::new("TBitBtn", name).caption(caption).h(32).w(104).p("ModalResult", result);
    if primary_look {
        b.p("Default", true).style("primary")
    } else {
        b.p("Cancel", result == 2)
    }
}

pub fn grid(name: &str) -> N {
    N::new("TGrid", name).p("Options", Value::Array(vec![json!("goEditing")])).p("BorderStyle", "bsNone")
}

pub fn toolbar(name: &str, buttons: Vec<N>) -> N {
    N::new("TToolBar", name).p("ShowCaptions", true).p("ButtonHeight", px(32)).kids(buttons)
}

/// Tool button: caption shown when `text` is true, otherwise only the icon (caption = tooltip).
pub fn tool(name: &str, caption: &str, icon: &str, text: bool) -> N {
    let n = N::new("TToolButton", name).caption(caption).hint(caption).icon(icon);
    if text {
        n.p("Look", "text")
    } else {
        n.p("Look", "icon")
    }
}

pub fn tool_title(name: &str, caption: &str) -> N {
    N::new("TToolButton", name).caption(caption).p("Look", "title")
}

pub fn tool_primary(name: &str, caption: &str, icon: &str) -> N {
    N::new("TToolButton", name).caption(caption).hint(caption).icon(icon).p("Look", "primary")
}

pub fn tool_drop(name: &str, caption: &str, icon: &str, text: bool) -> N {
    tool(name, caption, icon, text).p("Drop", true)
}

pub fn tool_sep(name: &str) -> N {
    N::new("TToolButton", name).p("Style", "tbsSeparator").p("Width", px(9))
}

pub fn pages(name: &str) -> N {
    N::new("TPageControl", name)
}

pub fn sheet(name: &str, caption: &str) -> N {
    N::new("TTabSheet", name).caption(caption)
}

pub fn splitter(name: &str, align: &str, size: i32) -> N {
    let n = N::new("TSplitter", name).align(align).p("MinSize", px(120));
    if align == "alLeft" || align == "alRight" {
        n.w(size)
    } else {
        n.h(size)
    }
}

pub fn popup(name: &str, items: Vec<N>) -> N {
    N::new("TPopupMenu", name).kids(items)
}

pub fn item(name: &str, caption: &str) -> N {
    N::new("TMenuItem", name).caption(caption)
}

pub fn item_icon(name: &str, caption: &str, icon: &str) -> N {
    item(name, caption).icon(icon)
}

pub fn item_check(name: &str, caption: &str) -> N {
    item(name, caption).p("AutoCheck", true)
}

pub fn sep(name: &str) -> N {
    item(name, "-")
}

pub fn trackbar(name: &str, min: i32, max: i32) -> N {
    N::new("TTrackBar", name).p("Min", min).p("Max", max).h(32)
}

pub fn spin(name: &str, min: i32, max: i32) -> N {
    N::new("TCSpinEdit", name).p("MinValue", min).p("MaxValue", max).h(32).p("Text", "0")
}

pub fn progress(name: &str) -> N {
    N::new("TProgressBar", name).h(6)
}

// ------------------------------------------------------------------ form layout

pub const LABEL_W: i32 = 230;
pub const ROW_H: i32 = 40;

/// Label on the left, control on the right, one row.
pub fn field(row: &str, label_name: &str, caption: &str, ctrl: N) -> N {
    let lab = label(label_name, caption).left().w(LABEL_W).p("Layout", "tlCenter").p("AutoSize", false).p("WordWrap", true);
    let ctrl = ctrl.client().m(0, 4, 0, 4);
    panel(row).top().h(ROW_H).kids(vec![lab, ctrl])
}

/// Field with a short control (fixed width) and an optional unit/hint after it.
pub fn field_short(row: &str, label_name: &str, caption: &str, ctrl: N, width: i32, unit: Option<(&str, &str)>) -> N {
    let lab = label(label_name, caption).left().w(LABEL_W).p("Layout", "tlCenter").p("AutoSize", false).p("WordWrap", true);
    let ctrl = ctrl.left().w(width).m(0, 4, 0, 4);
    let mut row = panel(row).top().h(ROW_H).kids(vec![lab, ctrl]);
    if let Some((n, t)) = unit {
        let mut f = theme::font(theme::SMALL);
        f.color = theme::TEXT_MUTED;
        row = row.kid(label(n, t).left().w(160).p("Layout", "tlCenter").p("AutoSize", false).m(10, 0, 0, 0).font(&f));
    }
    row
}

/// Section heading (accent colour) with an optional description line.
pub fn section(name: &str, caption: &str, first: bool) -> N {
    let mut f = theme::font_semibold(15);
    f.color = theme::ACCENT;
    label(name, caption).font(&f).top().p("AutoSize", false).h(26).m(0, if first { 0 } else { 18 }, 0, 4)
}

pub fn note(name: &str, caption: &str, lines: i32) -> N {
    muted(name, caption).top().p("AutoSize", false).p("WordWrap", true).h(18 * lines.max(1)).m(0, 0, 0, 6)
}

/// A dialog: content stacked top-down inside padding, footer with right-aligned buttons.
/// The client height follows the content.
pub fn dialog(class: &str, name: &str, caption: &str, width: i32, content: Vec<N>, buttons: Vec<N>) -> N {
    let body_h: i32 = content.iter().map(|c| c.outer_h()).sum();
    let footer_h = px(64);
    let pad = px(24);
    let mut footer = panel("p_bottom").bottom().p("Height", footer_h).style("footer").color(theme::SURFACE_ALT).pad(24, 16, 16, 16);
    // right-aligned buttons, the last one at the right edge
    for b in buttons {
        footer = footer.kid(b.right().m(8, 0, 0, 0));
    }
    let body = panel("p_body").client().color(theme::SURFACE).pad(24, 20, 24, 12).kids(content);
    let ch = body_h + px(20) + px(12) + footer_h;
    let _ = pad;
    form(class, name, caption, width, 0).p("ClientHeight", ch).kids(vec![footer, body])
}
