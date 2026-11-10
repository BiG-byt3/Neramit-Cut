//! A small form runtime: builds Win32 windows from component trees with layout rules for
//! Align, Anchors, Margins and AutoSize.

use super::comp::Comp;
use super::theme::{self, px};
use super::win::*;
use std::cell::Cell;
use std::collections::HashMap;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub const FORM_CLASS: &str = "BxForm";
pub const BOX_CLASS: &str = "BxBox";
const TBM_GETPOS: u32 = 0x400;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    None,
    Top,
    Bottom,
    Left,
    Right,
    Client,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bevel {
    None,
    Lowered,
    Raised,
    Space,
}

#[derive(Clone, Debug)]
pub enum Ev {
    Click,
    DblClick,
    Change,
    Exit,
    KeyDown(u32),
    TabChange,
    Close,
    Resize,
    /// Toolbar button click: index of the button in the toolbar.
    Button(usize),
    /// Menu/popup command (component name of the menu item).
    Menu(String),
    Custom(String),
}

#[derive(Clone, Debug, Default)]
pub struct ToolBtn {
    pub name: String,
    pub caption: String,
    pub hint: String,
    pub image: i32,
    pub down: bool,
    pub enabled: bool,
    pub visible: bool,
    pub check: bool,
    pub autosize: bool,
    pub separator: bool,
    /// icon glyph (Segoe Fluent Icons)
    pub icon: Option<char>,
    /// "text" (icon + caption), "icon" (icon only, caption as tooltip), "title", "primary"
    pub look: String,
    /// shows a chevron and opens a menu
    pub drop: bool,
    pub width: i32,
    /// step number shown in a round badge before a title (0 = none)
    pub step: i32,
}

#[derive(Clone, Debug)]
pub enum Kind {
    Form,
    Panel { outer: Bevel, inner: Bevel, bevel_width: i32, border_width: i32, show_caption: bool },
    Label { layout: i32, alignment: i32, wordwrap: bool, transparent: bool, autosize: bool },
    GroupBox,
    RadioGroup { items: Vec<String>, columns: i32, index: i32 },
    CheckBox { checked: bool },
    Radio { checked: bool },
    Button { default: bool },
    BitBtn { glyph: String, num_glyphs: i32, default: bool, modal: i32, layout_left: bool },
    Edit { multiline: bool },
    Combo { items: Vec<String>, drop_list: bool, index: i32 },
    PageControl { images: String, index: usize },
    TabSheet { image: i32, tab_visible: bool },
    ToolBar { images: String, disabled: String, bw: i32, bh: i32, captions: bool, edge_top: i32, buttons: Vec<ToolBtn>, compact: usize },
    StatusBar { simple: bool, text: String },
    Splitter { vertical: bool, min: i32 },
    Image { asset: String },
    Shape,
    TrackBar { min: i32, max: i32, pos: i32 },
    UpDown { min: i32, max: i32, pos: i32, associate: String },
    SpinEdit { min: i32, max: i32 },
    Progress,
    Grid,
    NonVisual,
}

pub struct Ctl {
    pub name: String,
    pub class: String,
    pub comp: Comp,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub hwnd: HWND,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// anchor base: bounds and parent client size when the anchors were captured
    pub ax: i32,
    pub ay: i32,
    pub aw: i32,
    pub ah: i32,
    pub apw: i32,
    pub aph: i32,
    pub align: Align,
    pub anchors: [bool; 4], // left, top, right, bottom
    pub margins: [i32; 4],
    pub awm: bool,
    pub visible: bool,
    pub enabled: bool,
    pub caption: String,
    pub hint: String,
    pub font: FontSpec,
    pub color: u32,
    pub parent_bg: bool,
    pub kind: Kind,
    pub tab_order: i32,
    pub min_w: i32,
    pub min_h: i32,
    /// size last applied to the window (for OnCanResize)
    pub applied: (i32, i32),
}

impl Ctl {
    pub fn windowed(&self) -> bool {
        !matches!(self.kind, Kind::Label { .. } | Kind::Splitter { .. } | Kind::Image { .. } | Kind::Shape | Kind::NonVisual)
    }
    pub fn rect(&self) -> RECT {
        rect(self.x, self.y, self.w, self.h)
    }
}

pub struct Form {
    pub key: String,
    pub index: usize,
    pub ctls: Vec<Ctl>,
    pub by_name: HashMap<String, usize>,
    pub hwnd: HWND,
    pub modal_result: i32,
    pub modal: bool,
    pub closed: bool,
    pub sizeable: bool,
    pub menus: HashMap<u16, String>,
    pub menu_ctl: HashMap<String, usize>,
    pub drag: Option<(usize, i32, i32)>,
    pub tag: String,
}

pub type Handler = fn(form: usize, ctl: usize, ev: &Ev) -> bool;

pub struct Ui {
    pub forms: Vec<Option<Box<Form>>>,
    pub hwnd_map: HashMap<isize, (usize, usize)>,
    pub handler: Option<Handler>,
    pub next_cmd: u16,
    pub paint_hook: Option<fn(form: usize, ctl: usize, dc: HDC, r: &RECT) -> bool>,
    pub text_hook: Option<fn(form: &str, ctl: &str, prop: &str, design: &str) -> Option<String>>,
    /// OnCanResize: (form, ctl, new_w, new_h) -> allow
    pub can_resize: Option<fn(form: usize, ctl: usize, new_w: i32, new_h: i32) -> bool>,
    /// application keyboard shortcuts: return true when the key was handled
    pub key_hook: Option<fn(msg: &MSG) -> bool>,
}

thread_local! {
    static UI_PTR: Cell<*mut Ui> = Cell::new(null_mut());
}

pub fn ui() -> &'static mut Ui {
    UI_PTR.with(|p| {
        if p.get().is_null() {
            let b = Box::new(Ui {
                forms: vec![],
                hwnd_map: HashMap::new(),
                handler: None,
                next_cmd: 1000,
                paint_hook: None,
                key_hook: None,
                text_hook: None,
                can_resize: None,
            });
            p.set(Box::into_raw(b));
        }
        unsafe { &mut *p.get() }
    })
}

pub fn form(i: usize) -> &'static mut Form {
    ui().forms[i].as_mut().expect("form").as_mut()
}

/// Free a form and its windows.
pub unsafe fn destroy_form(i: usize) {
    let h = try_form(i).map(|f| f.hwnd).unwrap_or(null_mut());
    if !h.is_null() {
        DestroyWindow(h);
    }
    ui().hwnd_map.retain(|_, v| v.0 != i);
    if let Some(slot) = ui().forms.get_mut(i) {
        *slot = None;
    }
}

pub fn try_form(i: usize) -> Option<&'static mut Form> {
    ui().forms.get_mut(i).and_then(|f| f.as_mut()).map(|b| b.as_mut())
}

fn fire(form: usize, ctl: usize, ev: Ev) -> bool {
    if let Some(h) = ui().handler {
        h(form, ctl, &ev)
    } else {
        false
    }
}

// ---------------------------------------------------------------- loading

fn align_of(s: Option<&str>) -> Align {
    match s {
        Some("alTop") => Align::Top,
        Some("alBottom") => Align::Bottom,
        Some("alLeft") => Align::Left,
        Some("alRight") => Align::Right,
        Some("alClient") => Align::Client,
        _ => Align::None,
    }
}

fn bevel_of(s: Option<&str>, default: Bevel) -> Bevel {
    match s {
        Some("bvNone") => Bevel::None,
        Some("bvLowered") => Bevel::Lowered,
        Some("bvRaised") => Bevel::Raised,
        Some("bvSpace") => Bevel::Space,
        _ => default,
    }
}

fn font_of(c: &Comp, parent: &FontSpec) -> FontSpec {
    if c.bool("ParentFont", true) && !c.has("Font.Name") && !c.has("Font.Height") {
        return parent.clone();
    }
    let mut f = parent.clone();
    if let Some(n) = c.str("Font.Name") {
        f.name = n.to_string();
    }
    if c.has("Font.Height") {
        f.height = c.int("Font.Height", -11);
    }
    if c.has("Font.Style") {
        let st = c.list("Font.Style");
        f.bold = st.iter().any(|s| s == "fsBold");
        f.italic = st.iter().any(|s| s == "fsItalic");
        f.underline = st.iter().any(|s| s == "fsUnderline");
    }
    if let Some(v) = c.props.get("Font.Color") {
        f.color = tcolor(v).unwrap_or(0);
    }
    f
}

impl Form {
    /// A form from a component tree built in code (see ui::build).
    pub fn load_comp(key: &str, root: Comp) -> usize {
        let index = ui().forms.len();
        let mut f = Box::new(Form {
            key: key.to_string(),
            index,
            ctls: vec![],
            by_name: HashMap::new(),
            hwnd: null_mut(),
            modal_result: 0,
            modal: false,
            closed: false,
            sizeable: true,
            menus: HashMap::new(),
            menu_ctl: HashMap::new(),
            drag: None,
            tag: String::new(),
        });
        let base_font = theme::font(theme::BODY);
        f.add(&root, None, &base_font, theme::SURFACE);
        ui().forms.push(Some(f));
        index
    }

    fn add(&mut self, c: &Comp, parent: Option<usize>, pfont: &FontSpec, pcolor: u32) -> usize {
        let id = self.ctls.len();
        let font = font_of(c, pfont);
        let parent_color = c.bool("ParentColor", !c.has("Color"));
        let color = if parent_color && c.class != "TEdit" && c.class != "TMemo" {
            pcolor
        } else {
            c.props.get("Color").and_then(tcolor).unwrap_or(match c.class.as_str() {
                "TEdit" | "TMemo" | "TComboBox" | "TGrid" => unsafe { GetSysColor(COLOR_WINDOW) },
                _ => pcolor,
            })
        };
        let is_form = parent.is_none();
        let (w, h) = if is_form {
            (c.int("ClientWidth", c.int("Width", 300)), c.int("ClientHeight", c.int("Height", 200)))
        } else {
            (c.int("Width", default_w(&c.class)), c.int("Height", default_h(&c.class)))
        };
        let anchors_list = c.list("Anchors");
        let anchors = if c.has("Anchors") {
            [
                anchors_list.iter().any(|a| a == "akLeft"),
                anchors_list.iter().any(|a| a == "akTop"),
                anchors_list.iter().any(|a| a == "akRight"),
                anchors_list.iter().any(|a| a == "akBottom"),
            ]
        } else {
            [true, true, false, false]
        };
        let kind = kind_of(c);
        // A missing Caption is empty.
        let caption = c.str("Caption").map(String::from).unwrap_or_default();
        let caption = match &kind {
            Kind::Edit { .. } | Kind::SpinEdit { .. } => c.str("Text").unwrap_or("").to_string(),
            Kind::Combo { .. } => c.str("Text").unwrap_or("").to_string(),
            _ => caption,
        };
        let ctl = Ctl {
            name: c.name.clone(),
            class: c.class.clone(),
            comp: Comp { class: c.class.clone(), name: c.name.clone(), props: c.props.clone(), children: vec![] },
            parent,
            children: vec![],
            hwnd: null_mut(),
            x: if is_form { 0 } else { c.int("Left", 0) },
            y: if is_form { 0 } else { c.int("Top", 0) },
            w,
            h,
            ax: c.int("Left", 0),
            ay: c.int("Top", 0),
            aw: w,
            ah: h,
            apw: 0,
            aph: 0,
            align: match (c.str("Align"), c.class.as_str()) {
                (None, "TToolBar") => Align::Top,
                (None, "TStatusBar") => Align::Bottom,
                (a, _) => align_of(a),
            },
            anchors,
            margins: [c.int("Margins.Left", 3), c.int("Margins.Top", 3), c.int("Margins.Right", 3), c.int("Margins.Bottom", 3)],
            awm: c.bool("AlignWithMargins", false),
            visible: c.bool("Visible", true),
            enabled: c.bool("Enabled", true),
            caption,
            hint: c.str("Hint").unwrap_or("").to_string(),
            font: font.clone(),
            color,
            parent_bg: c.bool("ParentBackground", true),
            kind,
            tab_order: c.int("TabOrder", 0),
            min_w: c.int("Constraints.MinWidth", 0),
            min_h: c.int("Constraints.MinHeight", 0),
            applied: (w, h),
        };
        self.by_name.insert(c.name.clone(), id);
        self.ctls.push(ctl);
        if let Some(p) = parent {
            self.ctls[p].children.push(id);
        }
        // Tool buttons and menu items are folded into their owners.
        if c.class == "TToolBar" {
            let mut buttons: Vec<ToolBtn> = c
                .children
                .iter()
                .filter(|b| b.class == "TToolButton")
                .map(|b| ToolBtn {
                    name: b.name.clone(),
                    caption: b.str("Caption").unwrap_or("").to_string(),
                    hint: b.str("Hint").unwrap_or("").to_string(),
                    image: b.int("ImageIndex", -1),
                    down: b.bool("Down", false),
                    enabled: b.bool("Enabled", true),
                    visible: b.bool("Visible", true),
                    check: b.str("Style") == Some("tbsCheck"),
                    autosize: b.bool("AutoSize", false),
                    separator: matches!(b.str("Style"), Some("tbsSeparator") | Some("tbsDivider")),
                    icon: b.str("Icon").and_then(theme::icon_from_name),
                    look: b.str("Look").unwrap_or("text").to_string(),
                    drop: b.bool("Drop", false),
                    width: b.int("Width", 0),
                    step: b.int("Step", 0),
                })
                .collect();
            let order: Vec<(i32, usize)> = c.children.iter().enumerate().filter(|(_, b)| b.class == "TToolButton").map(|(i, b)| (b.int("Left", 0), i)).collect();
            let mut sorted = order.clone();
            sorted.sort();
            let by_index: Vec<ToolBtn> = sorted.iter().map(|(_, i)| buttons[order.iter().position(|o| o.1 == *i).unwrap()].clone()).collect();
            buttons = by_index;
            for b in &buttons {
                // keep tool buttons addressable by name (as non-visual entries)
                let bid = self.ctls.len();
                let mut bc = Comp::from_json(&serde_json::json!({"class":"TToolButton","name":b.name}));
                bc.props.insert("Caption".into(), serde_json::Value::String(b.caption.clone()));
                if !b.hint.is_empty() {
                    bc.props.insert("Hint".into(), serde_json::Value::String(b.hint.clone()));
                }
                self.ctls.push(Ctl { kind: Kind::NonVisual, comp: bc, name: b.name.clone(), class: "TToolButton".into(), parent: Some(id), children: vec![], hwnd: null_mut(), x: 0, y: 0, w: 0, h: 0, ax: 0, ay: 0, aw: 0, ah: 0, apw: 0, aph: 0, align: Align::None, anchors: [true, true, false, false], margins: [0; 4], awm: false, visible: b.visible, enabled: b.enabled, caption: b.caption.clone(), hint: b.hint.clone(), font: font.clone(), color, parent_bg: true, tab_order: 0, min_w: 0, min_h: 0, applied: (0, 0) });
                self.by_name.insert(b.name.clone(), bid);
            }
            if let Kind::ToolBar { buttons: bb, .. } = &mut self.ctls[id].kind {
                *bb = buttons;
            }
            return id;
        }
        if c.class == "TMainMenu" || c.class == "TPopupMenu" {
            self.add_menu_items(c, id);
            return id;
        }
        for ch in &c.children {
            self.add(ch, Some(id), &font, color);
        }
        id
    }

    fn add_menu_items(&mut self, c: &Comp, parent: usize) {
        for m in c.children.iter().filter(|m| m.class == "TMenuItem") {
            let id = self.ctls.len();
            self.ctls.push(Ctl {
                kind: Kind::NonVisual,
                comp: Comp { class: m.class.clone(), name: m.name.clone(), props: m.props.clone(), children: vec![] },
                name: m.name.clone(),
                class: m.class.clone(),
                parent: Some(parent),
                children: vec![],
                hwnd: null_mut(),
                x: 0, y: 0, w: 0, h: 0, ax: 0, ay: 0, aw: 0, ah: 0, apw: 0, aph: 0,
                align: Align::None,
                anchors: [true, true, false, false],
                margins: [0; 4],
                awm: false,
                visible: m.bool("Visible", true),
                enabled: m.bool("Enabled", true),
                caption: m.str("Caption").unwrap_or("").to_string(),
                hint: m.str("Hint").unwrap_or("").to_string(),
                font: FontSpec::default(),
                color: 0,
                parent_bg: true,
                tab_order: 0,
                min_w: 0,
                min_h: 0,
                applied: (0, 0),
            });
            self.by_name.insert(m.name.clone(), id);
            self.ctls[parent].children.push(id);
            self.add_menu_items(m, id);
        }
    }

    pub fn id(&self, name: &str) -> usize {
        *self.by_name.get(name).unwrap_or_else(|| panic!("{}: no control {name}", self.key))
    }
    pub fn try_id(&self, name: &str) -> Option<usize> {
        self.by_name.get(name).copied()
    }
    pub fn c(&mut self, name: &str) -> &mut Ctl {
        let i = self.id(name);
        &mut self.ctls[i]
    }
    pub fn hwnd_of(&self, name: &str) -> HWND {
        self.ctls[self.id(name)].hwnd
    }
}

fn default_w(class: &str) -> i32 {
    match class {
        "TButton" | "TBitBtn" => 75,
        "TEdit" | "TComboBox" => 121,
        "TCheckBox" | "TRadioButton" => 97,
        "TLabel" => 31,
        "TSplitter" => 3,
        _ => 185,
    }
}

fn default_h(class: &str) -> i32 {
    match class {
        "TButton" | "TBitBtn" => 25,
        "TEdit" | "TComboBox" => 21,
        "TCheckBox" | "TRadioButton" => 17,
        "TLabel" => 13,
        "TSplitter" => 3,
        _ => 41,
    }
}

fn kind_of(c: &Comp) -> Kind {
    match c.class.as_str() {
        s if s.starts_with("Tf") || s.starts_with("TF") || s.starts_with("Tdm") => Kind::Form,
        "TPanel" => Kind::Panel {
            outer: bevel_of(c.str("BevelOuter"), Bevel::Raised),
            inner: bevel_of(c.str("BevelInner"), Bevel::None),
            bevel_width: c.int("BevelWidth", 1),
            border_width: c.int("BorderWidth", 0),
            show_caption: c.bool("ShowCaption", true),
        },
        "TLabel" => Kind::Label {
            layout: match c.str("Layout") {
                Some("tlCenter") => 1,
                Some("tlBottom") => 2,
                _ => 0,
            },
            alignment: match c.str("Alignment") {
                Some("taRightJustify") => 2,
                Some("taCenter") => 1,
                _ => 0,
            },
            wordwrap: c.bool("WordWrap", false),
            transparent: c.bool("Transparent", false),
            autosize: c.bool("AutoSize", true),
        },
        "TGroupBox" => Kind::GroupBox,
        "TRadioGroup" => Kind::RadioGroup { items: c.list("Items.Strings"), columns: c.int("Columns", 1), index: c.int("ItemIndex", -1) },
        "TCheckBox" => Kind::CheckBox { checked: c.bool("Checked", false) || c.str("State") == Some("cbChecked") },
        "TRadioButton" => Kind::Radio { checked: c.bool("Checked", false) },
        "TButton" => Kind::Button { default: c.bool("Default", false) },
        "TBitBtn" => {
            let kind = c.str("Kind").unwrap_or("bkCustom");
            let glyph = match kind {
                "bkOK" => "vcl.BBOK.bmp".to_string(),
                "bkCancel" => "vcl.BBCANCEL.bmp".to_string(),
                "bkHelp" => "vcl.BBHELP.bmp".to_string(),
                "bkYes" => "vcl.BBYES.bmp".to_string(),
                "bkNo" => "vcl.BBNO.bmp".to_string(),
                "bkClose" => "vcl.BBCLOSE.bmp".to_string(),
                "bkAbort" => "vcl.BBABORT.bmp".to_string(),
                "bkRetry" => "vcl.BBRETRY.bmp".to_string(),
                "bkIgnore" => "vcl.BBIGNORE.bmp".to_string(),
                "bkAll" => "vcl.BBALL.bmp".to_string(),
                _ => String::new(),
            };
            let modal = match kind {
                "bkOK" => 1,
                "bkCancel" => 2,
                "bkAbort" => 3,
                "bkRetry" => 4,
                "bkIgnore" => 5,
                "bkYes" => 6,
                "bkNo" => 7,
                _ => match c.str("ModalResult") {
                    Some("mrOk") => 1,
                    Some("mrCancel") => 2,
                    _ => c.int("ModalResult", 0),
                },
            };
            Kind::BitBtn {
                glyph,
                num_glyphs: c.int("NumGlyphs", 1),
                default: c.bool("Default", kind == "bkOK"),
                modal,
                layout_left: true,
            }
        }
        "TEdit" => Kind::Edit { multiline: false },
        "TMemo" => Kind::Edit { multiline: true },
        "TComboBox" => Kind::Combo { items: c.list("Items.Strings"), drop_list: c.str("Style") == Some("csDropDownList"), index: c.int("ItemIndex", -1) },
        "TPageControl" => Kind::PageControl { images: c.str("Images").unwrap_or("").to_string(), index: 0 },
        "TTabSheet" => Kind::TabSheet { image: c.int("ImageIndex", -1), tab_visible: c.bool("TabVisible", true) },
        "TToolBar" => {
            let mut edge_top = 0;
            let borders = if c.has("EdgeBorders") { c.list("EdgeBorders") } else { vec!["ebTop".to_string()] };
            if borders.iter().any(|b| b == "ebTop") {
                let inner = c.str("EdgeInner").unwrap_or("esRaised");
                let outer = c.str("EdgeOuter").unwrap_or("esLowered");
                // With themes the VCL neither paints toolbar edges nor reserves room for them.
                let _ = (inner, outer);
                edge_top = 0;
            }
            Kind::ToolBar {
                images: c.str("Images").unwrap_or("").to_string(),
                disabled: c.str("DisabledImages").unwrap_or("").to_string(),
                bw: c.int("ButtonWidth", 23),
                bh: c.int("ButtonHeight", 22),
                captions: c.bool("ShowCaptions", false),
                edge_top,
                buttons: vec![],
                compact: 0,
            }
        }
        "TStatusBar" => Kind::StatusBar { simple: c.bool("SimplePanel", false), text: c.str("SimpleText").unwrap_or("").to_string() },
        "TSplitter" => Kind::Splitter {
            vertical: match c.str("Align") {
                Some("alLeft") | Some("alRight") => true,
                Some("alTop") | Some("alBottom") => false,
                _ => c.int("Width", 3) < c.int("Height", 100),
            },
            min: c.int("MinSize", 30),
        },
        "TImage" => Kind::Image { asset: String::new() },
        "TShape" => Kind::Shape,
        "TTrackBar" => Kind::TrackBar { min: c.int("Min", 0), max: c.int("Max", 10), pos: c.int("Position", 0) },
        "TUpDown" => Kind::UpDown { min: c.int("Min", 0), max: c.int("Max", 100), pos: c.int("Position", 0), associate: c.str("Associate").unwrap_or("").to_string() },
        "TCSpinEdit" => Kind::SpinEdit { min: c.int("MinValue", 0), max: c.int("MaxValue", 0) },
        "TProgressBar" => Kind::Progress,
        "TGrid" => Kind::Grid,
        _ => Kind::NonVisual,
    }
}

// ---------------------------------------------------------------- window creation

pub unsafe fn register_classes() {
    let mut icc = INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: 0xFFFF };
    InitCommonControlsEx(&mut icc);
    // H/VREDRAW: a panel that changes size repaints all of it (its layout depends on the size;
    // without it a panel that only shrinks keeps showing its old picture)
    for (name, style) in [(FORM_CLASS, CS_DBLCLKS | CS_HREDRAW | CS_VREDRAW), (BOX_CLASS, CS_DBLCLKS | CS_HREDRAW | CS_VREDRAW)] {
        let cls = wide(name);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style,
            lpfnWndProc: Some(box_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance(),
            hIcon: LoadIconW(hinstance(), 1 as _),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: cls.as_ptr(),
            hIconSm: null_mut(),
        };
        RegisterClassExW(&wc);
    }
    super::grid::register();
}

pub struct FormStyle {
    pub style: u32,
    pub ex: u32,
}

fn form_style(c: &Comp) -> (FormStyle, bool) {
    let bs = c.str("BorderStyle").unwrap_or("bsSizeable");
    let icons = if c.has("BorderIcons") { c.list("BorderIcons") } else { vec!["biSystemMenu".into(), "biMinimize".into(), "biMaximize".into()] };
    let mut style = WS_CLIPCHILDREN | WS_CAPTION;
    let mut ex = 0;
    let sizeable = matches!(bs, "bsSizeable" | "bsSizeToolWin");
    match bs {
        "bsDialog" => {
            style |= WS_POPUP | WS_SYSMENU | DS_MODALFRAME as u32;
            ex |= WS_EX_DLGMODALFRAME | WS_EX_WINDOWEDGE;
        }
        "bsSingle" | "bsToolWindow" => {
            style |= WS_POPUP | WS_BORDER;
            if bs == "bsToolWindow" {
                ex |= WS_EX_TOOLWINDOW;
            }
        }
        "bsNone" => style = WS_POPUP | WS_CLIPCHILDREN,
        _ => style |= WS_OVERLAPPED | WS_THICKFRAME,
    }
    if bs != "bsDialog" && bs != "bsNone" {
        if icons.iter().any(|i| i == "biSystemMenu") {
            style |= WS_SYSMENU;
            if bs != "bsToolWindow" && bs != "bsSizeToolWin" {
                if icons.iter().any(|i| i == "biMinimize") {
                    style |= WS_MINIMIZEBOX;
                }
                if icons.iter().any(|i| i == "biMaximize") {
                    style |= WS_MAXIMIZEBOX;
                }
            }
        }
    }
    (FormStyle { style, ex }, sizeable)
}

impl Form {
    /// Create all windows. `owner` is the owning top-level window (dialogs).
    pub unsafe fn create(&mut self, owner: HWND) {
        let root = &self.ctls[0];
        let (fs, sizeable) = form_style(&root.comp);
        self.sizeable = sizeable;
        let mut r = rect(0, 0, root.w, root.h);
        let has_menu = self.ctls.iter().any(|c| c.class == "TMainMenu");
        let menu = if has_menu { self.build_main_menu() } else { null_mut() };
        // VCL applies ClientWidth/ClientHeight before the main menu is attached, so the menu
        // bar takes its height out of the client area.
        AdjustWindowRectEx(&mut r, fs.style, 0, fs.ex);
        let title = self.text_for(0, "Caption");
        let hwnd = CreateWindowExW(
            fs.ex,
            wide(FORM_CLASS).as_ptr(),
            wide(&title).as_ptr(),
            fs.style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            r.right - r.left,
            r.bottom - r.top,
            owner,
            menu,
            hinstance(),
            null(),
        );
        self.hwnd = hwnd;
        self.ctls[0].hwnd = hwnd;
        self.ctls[0].caption = title;
        // Windows 11: title bar in the surface colour (the main window: the brand colour),
        // rounded corners
        let caption: u32 = self.ctls[0].comp.int("CaptionColor", theme::SURFACE as i32) as u32;
        windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(hwnd, 35 /* DWMWA_CAPTION_COLOR */, &caption as *const u32 as _, 4);
        if let Some(tc) = self.ctls[0].comp.props.get("CaptionTextColor").and_then(|v| v.as_i64()) {
            let tc = tc as u32;
            windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(hwnd, 36 /* DWMWA_TEXT_COLOR */, &tc as *const u32 as _, 4);
        }
        let corners: u32 = 2; // DWMWCP_ROUND
        windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(hwnd, 33 /* DWMWA_WINDOW_CORNER_PREFERENCE */, &corners as *const u32 as _, 4);
        ui().hwnd_map.insert(hwnd as isize, (self.index, 0));
        if !menu.is_null() {
            let cr = client_rect(hwnd);
            self.ctls[0].w = cr.right;
            self.ctls[0].h = cr.bottom;
        }
        let kids = self.ctls[0].children.clone();
        for k in kids {
            self.create_ctl(k);
        }
        self.translate_all();
        self.realign(0);
        for i in 0..self.ctls.len() {
            self.capture_anchor(i);
        }
    }

    /// Runtime text of a property, from the design text through the translation hook.
    pub fn text_for(&self, id: usize, prop: &str) -> String {
        let c = &self.ctls[id];
        let mut design = c.comp.str(prop).unwrap_or("").to_string();
        // TBitBtn.Kind supplies the caption when the design has none (OK, Cancel...)
        if prop == "Caption" && design.is_empty() && c.class == "TBitBtn" && !c.comp.has("Caption") {
            design = match c.comp.str("Kind").unwrap_or("") {
                "bkOK" => "OK",
                "bkCancel" => "Cancel",
                "bkClose" => "Close",
                "bkYes" => "Yes",
                "bkNo" => "No",
                "bkHelp" => "Help",
                "bkAbort" => "Abort",
                "bkRetry" => "Retry",
                "bkIgnore" => "Ignore",
                "bkAll" => "All",
                _ => "",
            }
            .to_string();
        }
        self.translate(&c.name, prop, &design)
    }

    pub fn translate(&self, comp: &str, prop: &str, design: &str) -> String {
        if let Some(h) = ui().text_hook {
            if let Some(t) = h(&self.key, comp, prop, design) {
                return t;
            }
        }
        design.to_string()
    }

    /// Re-apply translated captions to every control.
    pub unsafe fn translate_all(&mut self) {
        for i in 0..self.ctls.len() {
            let t = self.text_for(i, "Caption");
            let hint = self.text_for(i, "Hint");
            let c = &mut self.ctls[i];
            c.hint = hint;
            if c.class == "TToolButton" || c.class == "TMenuItem" {
                c.caption = t;
                continue;
            }
            if matches!(c.kind, Kind::Edit { .. } | Kind::Combo { .. } | Kind::SpinEdit { .. }) {
                continue;
            }
            c.caption = t.clone();
            if !c.hwnd.is_null() && !matches!(c.kind, Kind::PageControl { .. } | Kind::ToolBar { .. } | Kind::Grid) {
                set_text(c.hwnd, &t);
            }
        }
        // radio group items
        for i in 0..self.ctls.len() {
            if let Kind::RadioGroup { .. } = self.ctls[i].kind {
                let design = self.ctls[i].comp.list("Items.Strings");
                let name = self.ctls[i].name.clone();
                let items: Vec<String> = design.iter().enumerate().map(|(k, s)| self.translate(&name, &format!("Item{k}"), s)).collect();
                let h = self.ctls[i].hwnd;
                if let Kind::RadioGroup { items: cur, .. } = &mut self.ctls[i].kind {
                    if cur.len() == items.len() {
                        *cur = items.clone();
                        if !h.is_null() {
                            for (k, it) in items.iter().enumerate() {
                                set_text(GetDlgItem(h, 5000 + k as i32), it);
                            }
                        }
                    }
                }
            }
        }
        // tool buttons get their captions from the NonVisual entries
        for i in 0..self.ctls.len() {
            if let Kind::ToolBar { .. } = self.ctls[i].kind {
                let names: Vec<String> = match &self.ctls[i].kind {
                    Kind::ToolBar { buttons, .. } => buttons.iter().map(|b| b.name.clone()).collect(),
                    _ => vec![],
                };
                for (bi, n) in names.iter().enumerate() {
                    let (cap, hint) = {
                        let c = &self.ctls[self.id(n)];
                        (c.caption.clone(), c.hint.clone())
                    };
                    if let Kind::ToolBar { buttons, .. } = &mut self.ctls[i].kind {
                        buttons[bi].caption = cap;
                        buttons[bi].hint = hint;
                    }
                }
                if !self.ctls[i].hwnd.is_null() {
                    self.rebuild_toolbar(i);
                }
            }
            if let Kind::PageControl { .. } = self.ctls[i].kind {
                if !self.ctls[i].hwnd.is_null() {
                    self.rebuild_tabs(i);
                }
            }
            if self.ctls[i].class == "TLabel" && self.label_autosize(i) {
                self.autosize_label(i);
            }
        }
        if !self.hwnd.is_null() {
            set_text(self.hwnd, &self.ctls[0].caption.clone());
            if self.ctls.iter().any(|c| c.class == "TMainMenu") {
                let old = GetMenu(self.hwnd);
                let m = self.build_main_menu();
                SetMenu(self.hwnd, m);
                if !old.is_null() {
                    DestroyMenu(old);
                }
            }
        }
    }

    fn label_autosize(&self, i: usize) -> bool {
        matches!(self.ctls[i].kind, Kind::Label { autosize: true, .. })
    }

    /// TLabel.AdjustBounds: size to the caption (DT_CALCRECT), keeping the anchor side.
    pub unsafe fn autosize_label(&mut self, i: usize) {
        let c = &self.ctls[i];
        let (wordwrap, alignment) = match c.kind {
            Kind::Label { wordwrap, alignment, .. } => (wordwrap, alignment),
            _ => return,
        };
        let dc = GetDC(null_mut());
        let old = SelectObject(dc, font(&c.font) as _);
        let mut r = rect(0, 0, if wordwrap { c.w } else { 0 }, 0);
        let text = if c.caption.is_empty() { " ".to_string() } else { c.caption.clone() };
        let mut flags = DT_CALCRECT | DT_EXPANDTABS;
        if wordwrap {
            flags |= DT_WORDBREAK;
        }
        draw_text(dc, &text, &mut r, flags);
        SelectObject(dc, old);
        ReleaseDC(null_mut(), dc);
        let (nw, nh) = (r.right - r.left, r.bottom - r.top);
        let c = &mut self.ctls[i];
        match c.align {
            Align::Top | Align::Bottom | Align::Client => {
                c.h = nh;
            }
            Align::Left | Align::Right => {
                c.w = nw;
            }
            Align::None => {
                if !wordwrap {
                    if alignment == 2 {
                        c.x += c.w - nw;
                    } else if alignment == 1 {
                        c.x += (c.w - nw) / 2;
                    }
                    c.w = nw;
                }
                c.h = nh;
            }
        }
    }

    unsafe fn create_ctl(&mut self, id: usize) {
        let parent = self.ctls[id].parent.unwrap();
        let phwnd = self.host_of(parent);
        let c = &self.ctls[id];
        let cls_name = c.class.clone();
        let mut style = WS_CHILD | WS_CLIPSIBLINGS;
        if c.visible {
            style |= WS_VISIBLE;
        }
        if !c.enabled {
            style |= WS_DISABLED;
        }
        let mut ex = 0u32;
        let mut class = BOX_CLASS;
        let caption = self.text_for(id, "Caption");
        let mut text = caption.clone();
        match &c.kind {
            Kind::Panel { .. } | Kind::GroupBox | Kind::RadioGroup { .. } | Kind::TabSheet { .. } => {
                style |= WS_CLIPCHILDREN;
            }
            Kind::CheckBox { .. } => {
                class = "BUTTON";
                style |= if std::env::var_os("NERAMIT_AUTOCB").is_some() { BS_AUTOCHECKBOX } else { BS_3STATE } as u32 | WS_TABSTOP;
            }
            Kind::Radio { .. } => {
                class = "BUTTON";
                style |= BS_RADIOBUTTON as u32 | WS_TABSTOP;
            }
            Kind::Button { .. } => {
                class = "BUTTON";
                style |= WS_TABSTOP | BS_OWNERDRAW as u32;
            }
            Kind::BitBtn { .. } => {
                class = "BUTTON";
                style |= WS_TABSTOP | BS_OWNERDRAW as u32;
            }
            Kind::Edit { multiline } => {
                class = "EDIT";
                // single-line edits: the border is drawn by edit_proc (rounded, accent underline
                // when focused); memos keep the themed client edge
                style |= WS_TABSTOP | ES_AUTOHSCROLL as u32;
                if *multiline {
                    ex |= WS_EX_CLIENTEDGE;
                    style |= ES_MULTILINE as u32 | ES_WANTRETURN as u32 | ES_AUTOVSCROLL as u32;
                    style &= !(ES_AUTOHSCROLL as u32);
                    let lines = c.comp.list("Lines.Strings");
                    text = lines.join("\r\n");
                    match c.comp.str("ScrollBars") {
                        Some("ssVertical") => style |= WS_VSCROLL,
                        Some("ssBoth") => style |= WS_VSCROLL | WS_HSCROLL,
                        _ => {}
                    }
                } else {
                    text = c.comp.str("Text").unwrap_or("").to_string();
                }
                if c.comp.bool("NumbersOnly", false) {
                    style |= ES_NUMBER as u32;
                }
                if c.comp.bool("ReadOnly", false) {
                    style |= ES_READONLY as u32;
                }
            }
            Kind::SpinEdit { .. } => {
                class = "EDIT";
                style |= WS_TABSTOP | ES_AUTOHSCROLL as u32 | ES_MULTILINE as u32;
                text = c.comp.str("Text").unwrap_or("0").to_string();
            }
            Kind::Combo { drop_list, .. } => {
                class = "COMBOBOX";
                style |= WS_TABSTOP | WS_VSCROLL | if *drop_list { CBS_DROPDOWNLIST } else { CBS_DROPDOWN } as u32 | CBS_AUTOHSCROLL as u32;
                if c.comp.bool("Sorted", false) {
                    style |= CBS_SORT as u32;
                }
            }
            Kind::PageControl { .. } => {
                class = "SysTabControl32";
                // tabs are painted and hit-tested by tabs_proc (underline pivot)
                style |= WS_CLIPCHILDREN | WS_TABSTOP | TCS_FOCUSNEVER;
                text.clear();
            }
            Kind::ToolBar { .. } => {
                class = "ToolbarWindow32";
                style |= WS_CLIPCHILDREN | CCS_NORESIZE as u32 | CCS_NOPARENTALIGN as u32 | CCS_NODIVIDER as u32 | CCS_NOMOVEY as u32 | TBSTYLE_FLAT | TBSTYLE_TRANSPARENT | TBSTYLE_TOOLTIPS | TBSTYLE_LIST;
                text.clear();
            }
            Kind::StatusBar { .. } => {
                class = "msctls_statusbar32";
                style |= SBARS_SIZEGRIP as u32 | CCS_NOPARENTALIGN as u32 | CCS_NORESIZE as u32 | CCS_NOMOVEY as u32;
                text.clear();
            }
            Kind::TrackBar { .. } => {
                class = "msctls_trackbar32";
                style |= WS_TABSTOP | TBS_AUTOTICKS as u32 | TBS_BOTTOM as u32 | TBS_FIXEDLENGTH as u32 | TBS_ENABLESELRANGE as u32;
                text.clear();
            }
            Kind::UpDown { .. } => {
                class = "msctls_updown32";
                style |= UDS_SETBUDDYINT as u32 | UDS_ALIGNRIGHT as u32 | UDS_ARROWKEYS as u32 | UDS_NOTHOUSANDS as u32;
                text.clear();
            }
            Kind::Progress => {
                class = "msctls_progress32";
                text.clear();
            }
            Kind::Grid => {
                class = super::grid::GRID_CLASS;
                style |= WS_TABSTOP;
                if c.comp.str("BorderStyle") != Some("bsNone") {
                    ex |= WS_EX_CLIENTEDGE;
                }
                text.clear();
            }
            Kind::Label { .. } | Kind::Splitter { .. } | Kind::Image { .. } | Kind::Shape | Kind::NonVisual => {
                // graphic or non-visual: no window
                if cls_name != "TMainMenu" && cls_name != "TPopupMenu" {
                    self.ctls[id].caption = caption;
                }
                let kids = self.ctls[id].children.clone();
                let _ = kids;
                return;
            }
            Kind::Form => {}
        }
        let hwnd = CreateWindowExW(
            ex,
            wide(class).as_ptr(),
            wide(&text).as_ptr(),
            style,
            c.x,
            c.y,
            c.w.max(0),
            c.h.max(0),
            phwnd,
            (id + 100) as _,
            hinstance(),
            null(),
        );
        // edit boxes, radio buttons and check boxes draw text themselves: a face that stacks
        // Thai marks in plain GDI (owner-drawn buttons draw with their own font)
        let fnt = if matches!(class, "EDIT" | "COMBOBOX" | "LISTBOX" | "BUTTON") { font(&theme::native_text_font(&self.ctls[id].font)) } else { font(&self.ctls[id].font) };
        SendMessageW(hwnd, WM_SETFONT, fnt as usize, 0);
        self.ctls[id].hwnd = hwnd;
        self.ctls[id].caption = caption;
        ui().hwnd_map.insert(hwnd as isize, (self.index, id));
        self.init_native(id);
        let kids = self.ctls[id].children.clone();
        for k in kids {
            self.create_ctl(k);
        }
        if let Kind::PageControl { .. } = self.ctls[id].kind {
            self.rebuild_tabs(id);
            self.show_page(id, self.active_page_index(id));
        }
    }

    /// The window that hosts children of `id` (graphic containers delegate upwards).
    pub fn host_of(&self, id: usize) -> HWND {
        let mut i = id;
        loop {
            if !self.ctls[i].hwnd.is_null() {
                return self.ctls[i].hwnd;
            }
            match self.ctls[i].parent {
                Some(p) => i = p,
                None => return self.hwnd,
            }
        }
    }

    unsafe fn init_native(&mut self, id: usize) {
        let hwnd = self.ctls[id].hwnd;
        match self.ctls[id].kind.clone() {
            Kind::CheckBox { checked } | Kind::Radio { checked } => {
                SendMessageW(hwnd, BM_SETCHECK, checked as usize, 0);
            }
            Kind::BitBtn { .. } => {
                SetWindowSubclass(hwnd, Some(button_proc), 1, 0);
            }
            Kind::Button { .. } => {
                SetWindowSubclass(hwnd, Some(button_proc), 1, 0);
            }
            Kind::Combo { items, index, .. } => {
                // selection field as tall as the edits
                let ih = self.ctls[id].h - px(8);
                if ih > 0 {
                    SendMessageW(hwnd, CB_SETITEMHEIGHT, usize::MAX, ih as isize);
                    SendMessageW(hwnd, CB_SETITEMHEIGHT, 0, px(28) as isize);
                }
                for it in &items {
                    SendMessageW(hwnd, CB_ADDSTRING, 0, wide(it).as_ptr() as isize);
                }
                if index >= 0 {
                    SendMessageW(hwnd, CB_SETCURSEL, index as usize, 0);
                } else {
                    let t = self.ctls[id].comp.str("Text").unwrap_or("").to_string();
                    set_text(hwnd, &t);
                }
            }
            Kind::ToolBar { .. } => {
                SendMessageW(hwnd, TB_BUTTONSTRUCTSIZE, std::mem::size_of::<TBBUTTON>(), 0);
                SetWindowSubclass(hwnd, Some(toolbar_proc), 1, 0);
                self.rebuild_toolbar(id);
            }
            Kind::PageControl { .. } => {
                SetWindowSubclass(hwnd, Some(tabs_proc), 1, 0);
            }
            Kind::StatusBar { simple, text } => {
                if simple {
                    SendMessageW(hwnd, SB_SIMPLE, 1, 0);
                    SendMessageW(hwnd, SB_SETTEXTW, 255, wide(&text).as_ptr() as isize);
                }
            }
            Kind::TrackBar { min, max, pos } => {
                SendMessageW(hwnd, TBM_SETRANGEMIN, 0, min as isize);
                SendMessageW(hwnd, TBM_SETRANGEMAX, 0, max as isize);
                SendMessageW(hwnd, TBM_SETPOS, 1, pos as isize);
                SendMessageW(hwnd, TBM_SETTHUMBLENGTH, 20, 0);
                SendMessageW(hwnd, TBM_SETPAGESIZE, 0, (((max - min) / 5).max(1)) as isize);
            }
            Kind::UpDown { min, max, pos, .. } => {
                SendMessageW(hwnd, UDM_SETRANGE32, min as usize, max as isize);
                SendMessageW(hwnd, UDM_SETPOS32, 0, pos as isize);
            }
            Kind::Edit { .. } | Kind::SpinEdit { .. } => {
                SetWindowSubclass(hwnd, Some(edit_proc), 1, 0);
                SendMessageW(hwnd, EM_SETMARGINS, (EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize, 0);
                SetWindowPos(hwnd, null_mut(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED);
            }
            Kind::Grid => {
                super::grid::attach(hwnd, self.index, id);
                // Explorer theme: the newer, slimmer scroll bars
                windows_sys::Win32::UI::Controls::SetWindowTheme(hwnd, wide("Explorer").as_ptr(), null());
            }
            Kind::RadioGroup { items, index, .. } => {
                // TRadioGroup owns TGroupButton children (BUTTON BS_RADIOBUTTON)
                for (i, it) in items.iter().enumerate() {
                    let b = CreateWindowExW(
                        0,
                        wide("BUTTON").as_ptr(),
                        wide(it).as_ptr(),
                        WS_CHILD | WS_VISIBLE | WS_TABSTOP * (i == 0) as u32 | BS_RADIOBUTTON as u32,
                        0,
                        0,
                        10,
                        10,
                        hwnd,
                        (5000 + i) as _,
                        hinstance(),
                        null(),
                    );
                    SendMessageW(b, WM_SETFONT, font(&theme::native_text_font(&self.ctls[id].font)) as usize, 0);
                    SendMessageW(b, BM_SETCHECK, (i as i32 == index) as usize, 0);
                }
            }
            _ => {}
        }
    }

    /// Width of a modern tool button (icon, caption, chevron, padding).
    pub fn tool_width(b: &ToolBtn, font_spec: &FontSpec) -> i32 {
        Self::tool_width_mode(b, font_spec, false)
    }

    /// When a toolbar is too narrow its last `n` text buttons show only their icon (the
    /// caption becomes the tooltip); returns, per button, whether it is collapsed.
    pub fn collapsed_flags(buttons: &[ToolBtn], n: usize) -> Vec<bool> {
        let mut flags = vec![false; buttons.len()];
        let mut left = n;
        for (i, b) in buttons.iter().enumerate().rev() {
            if left == 0 {
                break;
            }
            if b.visible && b.look == "text" && b.icon.is_some() {
                flags[i] = true;
                left -= 1;
            }
        }
        flags
    }

    /// In compact mode text buttons show only their icon (the caption becomes the tooltip).
    pub fn tool_width_mode(b: &ToolBtn, font_spec: &FontSpec, compact: bool) -> i32 {
        if b.separator {
            return px(9);
        }
        if compact && b.look == "text" && b.icon.is_some() {
            return px(36) + if b.drop { px(14) } else { 0 };
        }
        if b.width > 0 && b.look != "title" && b.look != "icon" {
            return b.width;
        }
        match b.look.as_str() {
            "icon" => px(36) + if b.drop { px(14) } else { 0 },
            "title" => measure(&theme::font_semibold(15), &b.caption).cx + px(20) + if b.step > 0 { px(34) } else { 0 },
            _ => {
                let f = if b.look == "primary" { theme::font_medium(theme::BODY) } else { font_spec.clone() };
                let mut w = px(12) + measure(&f, &b.caption).cx + px(12);
                if b.icon.is_some() {
                    w += px(16) + px(8);
                }
                if b.drop {
                    w += px(16);
                }
                w
            }
        }
    }

    pub unsafe fn rebuild_toolbar(&mut self, id: usize) {
        let hwnd = self.ctls[id].hwnd;
        let (bh, buttons, compact) = match &self.ctls[id].kind {
            Kind::ToolBar { bh, buttons, compact, .. } => (*bh, buttons.clone(), *compact),
            _ => return,
        };
        let collapsed = Self::collapsed_flags(&buttons, compact);
        let font_spec = self.ctls[id].font.clone();
        let n = SendMessageW(hwnd, TB_BUTTONCOUNT, 0, 0) as i32;
        for _ in 0..n {
            SendMessageW(hwnd, TB_DELETEBUTTON, 0, 0);
        }
        SendMessageW(hwnd, TB_SETEXTENDEDSTYLE, 0, (TBSTYLE_EX_MIXEDBUTTONS | TBSTYLE_EX_DOUBLEBUFFER) as isize);
        // no image list: icons are glyphs drawn in the custom draw handler
        SendMessageW(hwnd, TB_SETBITMAPSIZE, 0, makelparam(0, 0));
        SendMessageW(hwnd, TB_SETPADDING, 0, makelparam(0, 0));
        let mut strings: Vec<Vec<u16>> = vec![];
        let mut tb: Vec<TBBUTTON> = vec![];
        for (i, b) in buttons.iter().enumerate() {
            let s = wide(&b.caption);
            let mut state = 0u8;
            if b.enabled && b.look != "title" {
                state |= TBSTATE_ENABLED as u8;
            }
            if b.down {
                state |= TBSTATE_CHECKED as u8;
            }
            if !b.visible {
                state |= TBSTATE_HIDDEN as u8;
            }
            let mut style = BTNS_BUTTON as u8;
            if b.look != "icon" && !collapsed[i] {
                style |= BTNS_SHOWTEXT as u8;
            }
            if b.check || b.down {
                style |= BTNS_CHECK as u8;
            }
            if b.separator {
                style = BTNS_SEP as u8;
            }
            let mut btn: TBBUTTON = std::mem::zeroed();
            btn.iBitmap = if b.separator { px(9) } else { -2 /* I_IMAGENONE */ };
            btn.idCommand = (i + 1) as i32;
            btn.fsState = state;
            btn.fsStyle = style;
            btn.iString = -1;
            strings.push(s);
            tb.push(btn);
        }
        for (i, b) in tb.iter_mut().enumerate() {
            if !buttons[i].separator {
                b.iString = strings[i].as_ptr() as isize;
            }
        }
        SendMessageW(hwnd, TB_ADDBUTTONSW, tb.len(), tb.as_ptr() as isize);
        let height = if bh > 0 { bh } else { px(32) };
        SendMessageW(hwnd, TB_SETBUTTONSIZE, 0, makelparam(px(32), height));
        for (i, b) in buttons.iter().enumerate() {
            if b.separator {
                continue;
            }
            let mut bi: TBBUTTONINFOW = std::mem::zeroed();
            bi.cbSize = std::mem::size_of::<TBBUTTONINFOW>() as u32;
            bi.dwMask = TBIF_SIZE;
            bi.cx = Self::tool_width_mode(b, &font_spec, collapsed[i]) as u16;
            SendMessageW(hwnd, TB_SETBUTTONINFOW, i + 1, &bi as *const _ as isize);
        }
        SendMessageW(hwnd, TB_AUTOSIZE, 0, 0);
        InvalidateRect(hwnd, null(), 1);
    }

    pub unsafe fn toolbar_set(&mut self, tb: &str, button: &str, enabled: Option<bool>, down: Option<bool>) {
        let id = self.id(tb);
        let hwnd = self.ctls[id].hwnd;
        if let Kind::ToolBar { buttons, .. } = &mut self.ctls[id].kind {
            if let Some(i) = buttons.iter().position(|b| b.name == button) {
                if let Some(e) = enabled {
                    buttons[i].enabled = e;
                    SendMessageW(hwnd, TB_ENABLEBUTTON, i + 1, e as isize);
                }
                if let Some(d) = down {
                    buttons[i].down = d;
                    SendMessageW(hwnd, TB_CHECKBUTTON, i + 1, d as isize);
                }
            }
        }
    }

    /// Name of the toolbar button with index `i`.
    pub fn toolbar_button(&self, tb: usize, i: usize) -> String {
        match &self.ctls[tb].kind {
            Kind::ToolBar { buttons, .. } => buttons.get(i).map(|b| b.name.clone()).unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// Screen rectangle of a toolbar button.
    pub unsafe fn toolbar_button_rect(&self, tb: &str, button: &str) -> RECT {
        let id = self.id(tb);
        let hwnd = self.ctls[id].hwnd;
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if let Kind::ToolBar { buttons, .. } = &self.ctls[id].kind {
            if let Some(i) = buttons.iter().position(|b| b.name == button) {
                SendMessageW(hwnd, TB_GETRECT, i + 1, &mut r as *mut _ as isize);
                let mut a = POINT { x: r.left, y: r.top };
                let mut b = POINT { x: r.right, y: r.bottom };
                ClientToScreen(hwnd, &mut a);
                ClientToScreen(hwnd, &mut b);
                r = RECT { left: a.x, top: a.y, right: b.x, bottom: b.y };
            }
        }
        r
    }

    /// Screen rectangle of a control.
    pub unsafe fn screen_rect(&self, name: &str) -> RECT {
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(self.hwnd_of(name), &mut r);
        r
    }

    pub unsafe fn rebuild_tabs(&mut self, id: usize) {
        let hwnd = self.ctls[id].hwnd;
        SendMessageW(hwnd, TCM_DELETEALLITEMS, 0, 0);
        let sheets: Vec<usize> = self.ctls[id].children.iter().copied().filter(|&k| matches!(self.ctls[k].kind, Kind::TabSheet { tab_visible: true, .. }) && self.ctls[k].visible).collect();
        for (pos, &k) in sheets.iter().enumerate() {
            let text = wide(&self.ctls[k].caption);
            let image = match self.ctls[k].kind {
                Kind::TabSheet { image, .. } => image,
                _ => -1,
            };
            let mut item: TCITEMW = std::mem::zeroed();
            item.mask = TCIF_TEXT | TCIF_IMAGE;
            item.pszText = text.as_ptr() as *mut u16;
            item.iImage = image;
            SendMessageW(hwnd, TCM_INSERTITEMW, pos, &item as *const _ as isize);
        }
    }

    pub fn sheets(&self, pc: usize) -> Vec<usize> {
        self.ctls[pc].children.iter().copied().filter(|&k| matches!(self.ctls[k].kind, Kind::TabSheet { tab_visible: true, .. }) && self.ctls[k].visible).collect()
    }

    fn active_page_index(&self, pc: usize) -> usize {
        let active = self.ctls[pc].comp.str("ActivePage").unwrap_or("").to_string();
        self.sheets(pc).iter().position(|&k| self.ctls[k].name == active).unwrap_or(0)
    }

    /// Select tab `index` of page control `pc` and show its sheet.
    pub unsafe fn show_page(&mut self, pc: usize, index: usize) {
        let sheets = self.sheets(pc);
        if let Kind::PageControl { index: cur, .. } = &mut self.ctls[pc].kind {
            *cur = index;
        }
        let hwnd = self.ctls[pc].hwnd;
        SendMessageW(hwnd, TCM_SETCURSEL, index, 0);
        let mut r = client_rect(hwnd);
        // pages sit below the tab strip
        r.top = self.tab_strip_h(pc);
        for (i, &k) in sheets.iter().enumerate() {
            let c = &mut self.ctls[k];
            c.x = r.left;
            c.y = r.top;
            c.w = r.right - r.left;
            c.h = r.bottom - r.top;
            if !c.hwnd.is_null() {
                SetWindowPos(c.hwnd, null_mut(), c.x, c.y, c.w, c.h, SWP_NOZORDER | SWP_NOACTIVATE);
                ShowWindow(c.hwnd, if i == index { SW_SHOW } else { SW_HIDE });
            }
        }
        if let Some(&k) = sheets.get(index) {
            self.realign(k);
        }
    }

    /// Height of the underline tab strip ("Small" strips are used for nested pages).
    pub fn tab_strip_h(&self, pc: usize) -> i32 {
        if self.ctls[pc].comp.bool("Small", false) {
            px(36)
        } else {
            px(42)
        }
    }

    fn tab_font(&self, pc: usize, selected: bool) -> FontSpec {
        let size = if self.ctls[pc].comp.bool("Small", false) { 13 } else { theme::BODY };
        if selected {
            theme::font_semibold(size)
        } else {
            theme::font(size)
        }
    }

    /// Tab rectangles (sheet index, rect) of the underline strip.
    pub fn tab_layout(&self, pc: usize) -> Vec<(usize, RECT)> {
        let h = self.tab_strip_h(pc);
        let mut x = self.ctls[pc].comp.int("TabInset", px(12));
        let mut out = vec![];
        for (i, &k) in self.sheets(pc).iter().enumerate() {
            let w = measure(&self.tab_font(pc, true), &self.ctls[k].caption).cx + px(24);
            out.push((i, RECT { left: x, top: 0, right: x + w, bottom: h }));
            x += w + px(4);
        }
        out
    }

    unsafe fn paint_tabs(&self, pc: usize, dc: HDC) {
        let c = &self.ctls[pc];
        let r = client_rect(c.hwnd);
        let bg = self.ctls[c.parent.unwrap_or(0)].color;
        let h = self.tab_strip_h(pc);
        fill(dc, &RECT { left: 0, top: 0, right: r.right, bottom: h }, bg);
        hline(dc, 0, r.right, h - 1, theme::DIVIDER);
        let cur = match c.kind {
            Kind::PageControl { index, .. } => index,
            _ => 0,
        };
        let hot = GetPropW(c.hwnd, wide("BxHotTab").as_ptr()) as isize - 1;
        let sheets = self.sheets(pc);
        for (i, tr) in self.tab_layout(pc) {
            let sel = i == cur;
            if !sel && hot == i as isize {
                let hr = RECT { left: tr.left, top: tr.top + px(4), right: tr.right, bottom: tr.bottom - px(5) };
                theme::round_rect(dc, &hr, px(4) as f32, Some(theme::HOVER), None);
            }
            let f = self.tab_font(pc, sel);
            let color = if sel { theme::ACCENT } else { theme::TEXT_MUTED };
            let caption = &self.ctls[sheets[i]].caption;
            theme::text(dc, caption, &RECT { left: tr.left, top: tr.top, right: tr.right, bottom: tr.bottom - px(2) }, &f, color, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            if sel {
                let tw = measure(&f, caption).cx;
                let ux = tr.left + (tr.right - tr.left - tw) / 2;
                let ur = RECT { left: ux, top: h - px(3), right: ux + tw, bottom: h };
                theme::round_rect(dc, &ur, px(2) as f32, Some(theme::ACCENT), None);
            }
        }
    }

    pub fn page_index(&self, pc: &str) -> usize {
        match self.ctls[self.id(pc)].kind {
            Kind::PageControl { index, .. } => index,
            _ => 0,
        }
    }

    // ------------------------------------------------------------ layout

    pub fn client_size(&self, id: usize) -> (i32, i32) {
        let c = &self.ctls[id];
        if id == 0 && !self.hwnd.is_null() {
            let r = unsafe { client_rect(self.hwnd) };
            return (r.right, r.bottom);
        }
        (c.w, c.h)
    }

    /// TWinControl.AdjustClientRect for the container.
    fn align_rect(&self, id: usize) -> RECT {
        let (w, h) = self.client_size(id);
        let mut r = rect(0, 0, w, h);
        let c = &self.ctls[id];
        match &c.kind {
            Kind::Panel { outer, inner, bevel_width, border_width, .. } => {
                let mut b = *border_width;
                if *outer != Bevel::None {
                    b += bevel_width;
                }
                if *inner != Bevel::None {
                    b += bevel_width;
                }
                r = RECT { left: b, top: b, right: w - b, bottom: h - b };
            }
            Kind::GroupBox | Kind::RadioGroup { .. } => {
                // modern group: caption line on top, content below without a frame inset
                let th = text_height(&theme::font_semibold(14));
                r = RECT { left: 0, top: th + px(6), right: w, bottom: h };
            }
            _ => {}
        }
        // TWinControl.Padding
        let cp = &c.comp;
        r.left += cp.int("Padding.Left", 0);
        r.top += cp.int("Padding.Top", 0);
        r.right -= cp.int("Padding.Right", 0);
        r.bottom -= cp.int("Padding.Bottom", 0);
        r
    }

    pub fn capture_anchor(&mut self, id: usize) {
        if let Some(p) = self.ctls[id].parent {
            let (pw, ph) = self.client_size(p);
            let c = &mut self.ctls[id];
            c.ax = c.x;
            c.ay = c.y;
            c.aw = c.w;
            c.ah = c.h;
            c.apw = pw;
            c.aph = ph;
        }
    }

    /// Set bounds from code (like setting Left/Top/Width/Height): also rebases anchors.
    pub unsafe fn set_bounds(&mut self, name: &str, x: i32, y: i32, w: i32, h: i32) {
        let id = self.id(name);
        let c = &mut self.ctls[id];
        c.x = x;
        c.y = y;
        c.w = w;
        c.h = h;
        self.capture_anchor(id);
        self.apply_bounds(id);
        if let Some(p) = self.ctls[id].parent {
            self.realign(p);
        }
    }

    pub unsafe fn realign(&mut self, id: usize) {
        let kids = self.ctls[id].children.clone();
        let (pw, ph) = self.client_size(id);
        // anchors for alNone controls
        for &k in &kids {
            let c = &mut self.ctls[k];
            if c.align != Align::None || c.class == "TToolButton" || c.class == "TMenuItem" || c.apw == 0 {
                continue;
            }
            let dw = pw - c.apw;
            let dh = ph - c.aph;
            let (l, t, r, b) = (c.anchors[0], c.anchors[1], c.anchors[2], c.anchors[3]);
            if r && l {
                c.x = c.ax;
                c.w = (c.aw + dw).max(0);
            } else if r {
                c.x = c.ax + dw;
                c.w = c.aw;
            } else if !l {
                c.x = c.ax + dw / 2;
            } else {
                c.x = c.ax;
            }
            if b && t {
                c.y = c.ay;
                c.h = (c.ah + dh).max(0);
            } else if b {
                c.y = c.ay + dh;
            } else if !t {
                c.y = c.ay + dh / 2;
            } else {
                c.y = c.ay;
            }
        }
        // VCL AlignControls
        let mut r = self.align_rect(id);
        for al in [Align::Top, Align::Bottom, Align::Left, Align::Right, Align::Client] {
            let mut list: Vec<usize> = kids.iter().copied().filter(|&k| self.ctls[k].align == al && self.ctls[k].visible && self.ctls[k].class != "TToolButton").collect();
            let key = |c: &Ctl| -> i32 {
                let m = if c.awm { c.margins } else { [0; 4] };
                match al {
                    Align::Top => c.y - m[1],
                    Align::Bottom => -(c.y + c.h + m[3]),
                    Align::Left => c.x - m[0],
                    Align::Right => -(c.x + c.w + m[2]),
                    _ => 0,
                }
            };
            list.sort_by_key(|&k| key(&self.ctls[k]));
            for k in list {
                let c = &mut self.ctls[k];
                let m = if c.awm { c.margins } else { [0; 4] };
                match al {
                    Align::Top => {
                        c.x = r.left + m[0];
                        c.y = r.top + m[1];
                        c.w = (r.right - r.left - m[0] - m[2]).max(0);
                        r.top += c.h + m[1] + m[3];
                    }
                    Align::Bottom => {
                        c.x = r.left + m[0];
                        c.w = (r.right - r.left - m[0] - m[2]).max(0);
                        c.y = r.bottom - c.h - m[3];
                        r.bottom -= c.h + m[1] + m[3];
                    }
                    Align::Left => {
                        c.x = r.left + m[0];
                        c.y = r.top + m[1];
                        c.h = (r.bottom - r.top - m[1] - m[3]).max(0);
                        r.left += c.w + m[0] + m[2];
                    }
                    Align::Right => {
                        c.x = r.right - c.w - m[2];
                        c.y = r.top + m[1];
                        c.h = (r.bottom - r.top - m[1] - m[3]).max(0);
                        r.right -= c.w + m[0] + m[2];
                    }
                    Align::Client => {
                        c.x = r.left + m[0];
                        c.y = r.top + m[1];
                        c.w = (r.right - r.left - m[0] - m[2]).max(0);
                        c.h = (r.bottom - r.top - m[1] - m[3]).max(0);
                    }
                    Align::None => {}
                }
            }
        }
        for &k in &kids {
            self.apply_bounds(k);
        }
        for &k in &kids {
            match self.ctls[k].kind {
                Kind::PageControl { index, .. } => {
                    if !self.ctls[k].hwnd.is_null() {
                        self.show_page(k, index);
                    }
                }
                Kind::RadioGroup { .. } => self.arrange_radio(k),
                Kind::ToolBar { compact, .. } => {
                    // collapse text buttons from the right, one at a time, until the row fits
                    let want = match &self.ctls[k].kind {
                        Kind::ToolBar { buttons, .. } if self.ctls[k].w > 0 => {
                            let total = buttons.iter().filter(|b| b.visible && b.look == "text" && b.icon.is_some()).count();
                            (0..=total)
                                .find(|&n| {
                                    let flags = Self::collapsed_flags(buttons, n);
                                    let need: i32 = buttons.iter().enumerate().filter(|(_, b)| b.visible).map(|(i, b)| Self::tool_width_mode(b, &self.ctls[k].font, flags[i])).sum();
                                    need <= self.ctls[k].w
                                })
                                .unwrap_or(total)
                        }
                        _ => 0,
                    };
                    if want != compact && !self.ctls[k].hwnd.is_null() {
                        if let Kind::ToolBar { compact, .. } = &mut self.ctls[k].kind {
                            *compact = want;
                        }
                        self.rebuild_toolbar(k);
                    }
                }
                Kind::NonVisual => {}
                _ => {
                    if !self.ctls[k].children.is_empty() {
                        self.realign(k);
                    }
                }
            }
        }
        if !self.ctls[id].hwnd.is_null() && self.ctls[id].windowed() {
            InvalidateRect(self.ctls[id].hwnd, null(), 1);
        }
    }

    pub unsafe fn apply_bounds(&mut self, k: usize) {
        {
            let c = &self.ctls[k];
            if (c.w, c.h) != c.applied && c.comp.has("OnCanResize") {
                if let Some(h) = ui().can_resize {
                    let (nw, nh) = (c.w, c.h);
                    if !h(self.index, k, nw, nh) {
                        let (ow, oh) = self.ctls[k].applied;
                        self.ctls[k].w = ow;
                        self.ctls[k].h = oh;
                    }
                }
            }
        }
        let c = &mut self.ctls[k];
        let resized = c.applied != (c.w, c.h);
        c.applied = (c.w, c.h);
        if let Kind::ToolBar { bh, edge_top, .. } = c.kind {
            if c.comp.bool("AutoSize", false) {
                c.h = bh + edge_top;
            }
        }
        if !c.hwnd.is_null() {
            SetWindowPos(c.hwnd, null_mut(), c.x, c.y, c.w, c.h, SWP_NOZORDER | SWP_NOACTIVATE);
            if resized {
                // native controls (toolbars, edits) too: never keep a picture of the old size
                InvalidateRect(c.hwnd, null(), 0);
            }
        } else if c.windowed() {
            // nothing
        }
    }

    unsafe fn arrange_radio(&mut self, id: usize) {
        let c = &self.ctls[id];
        let (items, columns) = match &c.kind {
            Kind::RadioGroup { items, columns, .. } => (items.len() as i32, (*columns).max(1)),
            _ => return,
        };
        if items == 0 {
            return;
        }
        let th = text_height(&c.font);
        let per_col = (items + columns - 1) / columns;
        let bw = (c.w - 10) / columns;
        let i0 = c.h - th - 5;
        let bh = i0 / per_col;
        let top = th + 1 + (i0 % per_col) / 2;
        for i in 0..items {
            let b = GetDlgItem(c.hwnd, 5000 + i);
            let x = (i / per_col) * bw + 8;
            let y = (i % per_col) * bh + top;
            SetWindowPos(b, null_mut(), x, y, bw, bh, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }

    pub unsafe fn set_visible(&mut self, name: &str, v: bool) {
        let id = self.id(name);
        self.ctls[id].visible = v;
        if !self.ctls[id].hwnd.is_null() {
            ShowWindow(self.ctls[id].hwnd, if v { SW_SHOWNA } else { SW_HIDE });
        }
        if let Some(p) = self.ctls[id].parent {
            self.realign(p);
            let h = self.host_of(p);
            InvalidateRect(h, null(), 1);
        }
    }

    pub unsafe fn set_enabled(&mut self, name: &str, v: bool) {
        let id = self.id(name);
        self.ctls[id].enabled = v;
        if !self.ctls[id].hwnd.is_null() {
            EnableWindow(self.ctls[id].hwnd, v as i32);
        }
    }

    /// Move control `name` under a new parent (TControl.Parent := ...).
    pub unsafe fn reparent(&mut self, name: &str, parent: &str) {
        let id = self.id(name);
        let np = self.id(parent);
        if let Some(op) = self.ctls[id].parent {
            self.ctls[op].children.retain(|&k| k != id);
        }
        self.ctls[id].parent = Some(np);
        self.ctls[np].children.push(id);
        let host = self.host_of(np);
        if !self.ctls[id].hwnd.is_null() {
            SetParent(self.ctls[id].hwnd, host);
        }
    }

    pub fn checked(&self, name: &str) -> bool {
        let c = &self.ctls[self.id(name)];
        if !c.hwnd.is_null() {
            unsafe { SendMessageW(c.hwnd, BM_GETCHECK, 0, 0) == 1 }
        } else {
            matches!(c.kind, Kind::CheckBox { checked: true } | Kind::Radio { checked: true })
        }
    }

    pub unsafe fn set_checked(&mut self, name: &str, v: bool) {
        let id = self.id(name);
        match &mut self.ctls[id].kind {
            Kind::CheckBox { checked } | Kind::Radio { checked } => *checked = v,
            _ => {}
        }
        if !self.ctls[id].hwnd.is_null() {
            SendMessageW(self.ctls[id].hwnd, BM_SETCHECK, v as usize, 0);
        }
    }

    pub fn text(&self, name: &str) -> String {
        let c = &self.ctls[self.id(name)];
        if !c.hwnd.is_null() {
            unsafe { get_text(c.hwnd) }
        } else {
            c.caption.clone()
        }
    }

    pub unsafe fn set_text(&mut self, name: &str, t: &str) {
        let id = self.id(name);
        self.ctls[id].caption = t.to_string();
        let h = self.ctls[id].hwnd;
        if !h.is_null() && !matches!(self.ctls[id].kind, Kind::PageControl { .. } | Kind::ToolBar { .. }) {
            set_text(h, t);
        } else if h.is_null() {
            if self.label_autosize(id) {
                self.autosize_label(id);
                if let Some(p) = self.ctls[id].parent {
                    self.realign(p);
                }
            }
            if let Some(p) = self.ctls[id].parent {
                let host = self.host_of(p);
                InvalidateRect(host, null(), 1);
            }
        }
    }

    pub fn radio_index(&self, name: &str) -> i32 {
        let c = &self.ctls[self.id(name)];
        if let Kind::RadioGroup { items, .. } = &c.kind {
            for i in 0..items.len() {
                let b = unsafe { GetDlgItem(c.hwnd, 5000 + i as i32) };
                if unsafe { SendMessageW(b, BM_GETCHECK, 0, 0) } == 1 {
                    return i as i32;
                }
            }
        }
        -1
    }

    pub unsafe fn set_radio_index(&mut self, name: &str, idx: i32) {
        let id = self.id(name);
        let c = &mut self.ctls[id];
        if let Kind::RadioGroup { items, index, .. } = &mut c.kind {
            *index = idx;
            for i in 0..items.len() {
                let b = GetDlgItem(c.hwnd, 5000 + i as i32);
                SendMessageW(b, BM_SETCHECK, (i as i32 == idx) as usize, 0);
            }
        }
    }

    pub unsafe fn set_radio_items(&mut self, name: &str, new_items: Vec<String>) {
        let id = self.id(name);
        let h = self.ctls[id].hwnd;
        if let Kind::RadioGroup { items, .. } = &mut self.ctls[id].kind {
            for i in 0..items.len() {
                DestroyWindow(GetDlgItem(h, 5000 + i as i32));
            }
            *items = new_items.clone();
        }
        for (i, it) in new_items.iter().enumerate() {
            let b = CreateWindowExW(0, wide("BUTTON").as_ptr(), wide(it).as_ptr(), WS_CHILD | WS_VISIBLE | BS_RADIOBUTTON as u32 | if i == 0 { WS_TABSTOP } else { 0 }, 0, 0, 10, 10, h, (5000 + i) as _, hinstance(), null());
            SendMessageW(b, WM_SETFONT, font(&theme::native_text_font(&self.ctls[id].font)) as usize, 0);
        }
        self.arrange_radio(id);
    }

    pub fn combo_index(&self, name: &str) -> i32 {
        unsafe { SendMessageW(self.hwnd_of(name), CB_GETCURSEL, 0, 0) as i32 }
    }

    pub unsafe fn set_combo_items(&mut self, name: &str, items: &[String], index: i32) {
        let h = self.hwnd_of(name);
        SendMessageW(h, CB_RESETCONTENT, 0, 0);
        for it in items {
            SendMessageW(h, CB_ADDSTRING, 0, wide(it).as_ptr() as isize);
        }
        SendMessageW(h, CB_SETCURSEL, index as usize, 0);
    }

    pub fn track_pos(&self, name: &str) -> i32 {
        unsafe { SendMessageW(self.hwnd_of(name), TBM_GETPOS, 0, 0) as i32 }
    }

    pub unsafe fn set_track_pos(&mut self, name: &str, v: i32) {
        SendMessageW(self.hwnd_of(name), TBM_SETPOS, 1, v as isize);
    }

    // ------------------------------------------------------------ menus

    unsafe fn build_main_menu(&mut self) -> HMENU {
        let id = match self.ctls.iter().position(|c| c.class == "TMainMenu") {
            Some(i) => i,
            None => return null_mut(),
        };
        let bar = CreateMenu();
        self.fill_menu(bar, id, true);
        bar
    }

    /// Check state of a menu item (popups pick it up when they are built; the main menu bar
    /// is updated in place).
    pub unsafe fn menu_check(&mut self, name: &str, v: bool) {
        super::menu::set_checked(self.index, name, v);
        if !self.hwnd.is_null() {
            let bar = GetMenu(self.hwnd);
            if !bar.is_null() {
                for (cmd, n) in self.menus.iter() {
                    if n == name {
                        CheckMenuItem(bar, *cmd as u32, MF_BYCOMMAND | if v { MF_CHECKED } else { MF_UNCHECKED });
                    }
                }
            }
        }
    }

    /// Show a popup menu at screen coordinates; the chosen item arrives as Ev::Menu.
    pub unsafe fn track_popup(&mut self, name: &str, x: i32, y: i32) {
        let m = self.popup_menu(name);
        TrackPopupMenu(m, TPM_LEFTALIGN | TPM_TOPALIGN | TPM_LEFTBUTTON, x, y, 0, self.hwnd, null());
        DestroyMenu(m);
    }

    pub unsafe fn popup_menu(&mut self, name: &str) -> HMENU {
        let id = self.id(name);
        let m = CreatePopupMenu();
        self.fill_menu(m, id, false);
        m
    }

    unsafe fn fill_menu(&mut self, menu: HMENU, id: usize, bar: bool) {
        let kids: Vec<usize> = self.ctls[id].children.iter().copied().filter(|&k| self.ctls[k].visible).collect();
        for k in kids {
            let text = self.ctls[k].caption.clone();
            if text == "-" {
                if bar {
                    AppendMenuW(menu, MF_SEPARATOR, 0, null());
                } else {
                    let data = super::menu::separator_data(self.index);
                    AppendMenuW(menu, MF_SEPARATOR | MF_OWNERDRAW, 0, data as *const u16);
                }
                continue;
            }
            let cmd = ui().next_cmd;
            ui().next_cmd = ui().next_cmd.wrapping_add(1).max(1000);
            let name = self.ctls[k].name.clone();
            self.menus.insert(cmd, name.clone());
            self.menu_ctl.insert(name.clone(), k);
            let has_kids = !self.ctls[k].children.is_empty();
            let mut flags = MF_STRING;
            if !self.ctls[k].enabled {
                flags |= MF_GRAYED;
            }
            if self.ctls[k].comp.bool("Checked", false) || super::menu::is_checked(self.index, &name) {
                flags |= MF_CHECKED;
            }
            let mut item = cmd as usize;
            if has_kids {
                let sub = CreatePopupMenu();
                self.fill_menu(sub, k, false);
                flags |= MF_POPUP;
                item = sub as usize;
            }
            if bar {
                AppendMenuW(menu, flags, item, wide(&text).as_ptr());
            } else {
                super::menu::register_item(self.index, Some(k), cmd as usize);
                AppendMenuW(menu, flags | MF_OWNERDRAW, item, cmd as usize as *const u16);
            }
        }
        if !bar {
            super::menu::style_popup(menu);
        }
    }

    // ------------------------------------------------------------ show / modal

    pub unsafe fn center(&mut self, owner: HWND) {
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(self.hwnd, &mut r);
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let pos = self.ctls[0].comp.str("Position").unwrap_or("poDefaultPosOnly").to_string();
        let mut work = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut _ as _, 0);
        let (x, y) = match pos.as_str() {
            "poMainFormCenter" | "poOwnerFormCenter" if !owner.is_null() => {
                let mut o = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                GetWindowRect(owner, &mut o);
                (o.left + (o.right - o.left - w) / 2, o.top + (o.bottom - o.top - h) / 2)
            }
            "poDesigned" => (self.ctls[0].comp.int("Left", 0), self.ctls[0].comp.int("Top", 0)),
            _ => (work.left + (work.right - work.left - w) / 2, work.top + (work.bottom - work.top - h) / 2),
        };
        SetWindowPos(self.hwnd, null_mut(), x.max(work.left), y.max(work.top), 0, 0, SWP_NOSIZE | SWP_NOZORDER);
    }

    /// TCustomForm.ShowModal
    pub unsafe fn show_modal(&mut self, owner: HWND) -> i32 {
        self.modal_result = 0;
        self.modal = true;
        self.closed = false;
        self.center(owner);
        if !owner.is_null() {
            EnableWindow(owner, 0);
        }
        ShowWindow(self.hwnd, SW_SHOW);
        let me = self.index;
        let mut msg: MSG = std::mem::zeroed();
        while form(me).modal_result == 0 {
            if GetMessageW(&mut msg, null_mut(), 0, 0) <= 0 {
                PostQuitMessage(msg.wParam as i32);
                break;
            }
            if IsDialogMessageW(form(me).hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        let f = form(me);
        if !owner.is_null() {
            EnableWindow(owner, 1);
            SetActiveWindow(owner);
        }
        ShowWindow(f.hwnd, SW_HIDE);
        f.modal = false;
        f.modal_result
    }

    /// ClientWidth/ClientHeight at run time (dialogs size themselves when shown).
    pub unsafe fn set_client_size(&mut self, w: i32, h: i32) {
        let style = GetWindowLongW(self.hwnd, GWL_STYLE) as u32;
        let ex = GetWindowLongW(self.hwnd, GWL_EXSTYLE) as u32;
        let mut r = rect(0, 0, w, h);
        AdjustWindowRectEx(&mut r, style, (!GetMenu(self.hwnd).is_null()) as i32, ex);
        SetWindowPos(self.hwnd, null_mut(), 0, 0, r.right - r.left, r.bottom - r.top, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        let cr = client_rect(self.hwnd);
        self.ctls[0].w = cr.right;
        self.ctls[0].h = cr.bottom;
        self.realign(0);
    }

    pub unsafe fn close(&mut self, result: i32) {
        self.modal_result = if result == 0 { 2 } else { result };
        if !self.modal {
            ShowWindow(self.hwnd, SW_HIDE);
        }
    }
}

// ---------------------------------------------------------------- painting

/// A "card" panel: rounded surface; props Tone (blue, teal, violet…), HeadH and FootH
/// (header and footer bands, device pixels).
pub unsafe fn paint_card(f: &Form, id: usize, dc: HDC, r: &RECT) {
    let c = &f.ctls[id];
    let parent_color = c.parent.map(|p| f.ctls[p].color).unwrap_or(theme::BG);
    let tone = c.comp.str("Tone").and_then(theme::tone);
    theme::card(dc, r, parent_color, tone, c.comp.int("HeadH", 0), c.comp.int("FootH", 0));
}

/// The tone of the card that holds `id` (toolbar titles take their badge colour from it).
pub fn card_tone(f: &Form, id: usize) -> Option<theme::Tone> {
    let mut i = Some(id);
    while let Some(k) = i {
        if let Some(t) = f.ctls[k].comp.str("Tone").and_then(theme::tone) {
            return Some(t);
        }
        i = f.ctls[k].parent;
    }
    None
}

unsafe fn paint_container(f: &mut Form, id: usize, dc: HDC) {
    let c = &f.ctls[id];
    let (w, h) = f.client_size(id);
    let r = rect(0, 0, w, h);
    if let Some(hook) = ui().paint_hook {
        if hook(f.index, id, dc, &r) {
            let kids = f.ctls[id].children.clone();
            for k in kids {
                paint_graphic(f, k, dc);
            }
            return;
        }
    }
    match &c.kind {
        Kind::Panel { .. } => match c.comp.str("Style").unwrap_or("") {
            "card" => paint_card(f, id, dc, &r),
            "appbar" => {
                fill(dc, &r, c.color);
                hline(dc, 0, w, h - 1, theme::STROKE);
            }
            "footer" => {
                fill(dc, &r, c.color);
                hline(dc, 0, w, 0, theme::DIVIDER);
            }
            "divider" => {
                fill(dc, &r, c.color);
                hline(dc, 0, w, h - 1, theme::DIVIDER);
            }
            _ => fill(dc, &r, c.color),
        },
        Kind::GroupBox | Kind::RadioGroup { .. } => {
            fill(dc, &r, c.color);
            if !c.caption.is_empty() {
                let tf = theme::font_semibold(14);
                let th = text_height(&tf);
                theme::text(dc, &c.caption, &RECT { left: 0, top: 0, right: w, bottom: th }, &tf, if c.enabled { theme::TEXT } else { theme::TEXT_DISABLED }, DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS);
            }
        }
        _ => fill(dc, &r, c.color),
    }
    // graphic children
    let kids = f.ctls[id].children.clone();
    for k in kids {
        paint_graphic(f, k, dc);
    }
}

impl Form {
    /// Modern look: sheets are flat surfaces, nothing draws the themed tab background.
    pub fn in_themed_sheet(&self, id: usize) -> bool {
        if std::env::var_os("NERAMIT_CLASSIC").is_none() {
            return false;
        }
        let mut i = self.ctls[id].parent;
        let mut first = true;
        if let Kind::TabSheet { .. } = self.ctls[id].kind {
            return true;
        }
        while let Some(p) = i {
            match &self.ctls[p].kind {
                Kind::TabSheet { .. } => return true,
                Kind::Panel { .. } if !self.ctls[p].parent_bg && !first => return false,
                Kind::Form => return false,
                _ => {}
            }
            first = false;
            i = self.ctls[p].parent;
        }
        false
    }
}

unsafe fn paint_graphic(f: &mut Form, k: usize, dc: HDC) {
    let c = &f.ctls[k];
    if !c.visible || c.windowed() {
        return;
    }
    if let Some(hook) = ui().paint_hook {
        let r = c.rect();
        if hook(f.index, k, dc, &r) {
            return;
        }
    }
    match &c.kind {
        Kind::Label { layout, alignment, wordwrap, transparent, .. } => {
            let mut r = c.rect();
            if !*transparent && !f.in_themed_sheet(k) {
                fill(dc, &r, c.color);
            }
            SetBkMode(dc, TRANSPARENT as i32);
            SetTextColor(dc, if c.enabled { c.font.color } else { theme::TEXT_DISABLED });
            let old = SelectObject(dc, font(&c.font) as _);
            let mut flags = DT_EXPANDTABS | [DT_LEFT, DT_CENTER, DT_RIGHT][*alignment as usize];
            if *wordwrap {
                flags |= DT_WORDBREAK;
            }
            if *layout != 0 {
                let mut cr = r;
                draw_text(dc, &c.caption, &mut cr, flags | DT_CALCRECT);
                let th = cr.bottom - cr.top;
                if *layout == 1 {
                    r.top += (r.bottom - r.top - th) / 2;
                } else {
                    r.top = r.bottom - th;
                }
            }
            draw_text(dc, &c.caption, &mut r, flags);
            SelectObject(dc, old);
        }
        Kind::Shape => {
            let r = c.rect();
            let pen = CreatePen(PS_SOLID, 1, 0);
            let brush = CreateSolidBrush(rgb(255, 255, 255));
            let op = SelectObject(dc, pen as _);
            let ob = SelectObject(dc, brush as _);
            Rectangle(dc, r.left, r.top, r.right, r.bottom);
            SelectObject(dc, op);
            SelectObject(dc, ob);
            DeleteObject(pen as _);
            DeleteObject(brush as _);
        }
        Kind::Image { .. } => {}
        Kind::Splitter { vertical, .. } => {
            // a short rounded grip so the handle can be found
            let r = c.rect();
            let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
            let (gw, gh) = if *vertical { (px(4), px(36)) } else { (px(36), px(4)) };
            let g = RECT { left: cx - gw / 2, top: cy - gh / 2, right: cx - gw / 2 + gw, bottom: cy - gh / 2 + gh };
            let dragging = f.drag.map(|d| d.0 == k).unwrap_or(false);
            theme::round_rect(dc, &g, px(2) as f32, Some(if dragging { theme::ACCENT } else { theme::STROKE_STRONG }), None);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- BitBtn

/// Flat buttons: primary (accent fill) for default buttons, standard (outlined) otherwise.
unsafe fn draw_button(f: &Form, id: usize, dis: &DRAWITEMSTRUCT) {
    let c = &f.ctls[id];
    let primary = match &c.kind {
        Kind::BitBtn { default, .. } => *default,
        Kind::Button { default } => *default,
        _ => return,
    } || c.comp.str("Style") == Some("primary");
    let subtle = c.comp.str("Style") == Some("subtle");
    let dc = dis.hDC;
    let r = dis.rcItem;
    let pressed = dis.itemState & ODS_SELECTED != 0;
    let focused = dis.itemState & ODS_FOCUS != 0 && dis.itemState & ODS_NOFOCUSRECT == 0;
    let disabled = dis.itemState & ODS_DISABLED != 0;
    let hot = GetPropW(dis.hwndItem, wide("BxHot").as_ptr()) as usize != 0;
    let parent_color = c.parent.map(|p| f.ctls[p].color).unwrap_or(theme::SURFACE);
    fill(dc, &r, parent_color);
    let (bg, border, fg) = if disabled {
        (if primary { theme::STROKE } else { theme::SURFACE_ALT }, if subtle { None } else { Some(theme::STROKE) }, theme::TEXT_DISABLED)
    } else if primary {
        (if pressed { theme::ACCENT_PRESSED } else if hot { theme::ACCENT_HOVER } else { theme::ACCENT }, None, theme::WHITE)
    } else if subtle {
        (if pressed { theme::PRESSED } else if hot { theme::HOVER } else { parent_color }, None, theme::TEXT)
    } else {
        (if pressed { theme::PRESSED } else if hot { theme::HOVER } else { theme::SURFACE }, Some(theme::STROKE_STRONG), theme::TEXT)
    };
    let radius = px(4) as f32;
    if primary && !disabled {
        let (a, b) = if pressed { (theme::ACCENT_PRESSED, theme::ACCENT_PRESSED) } else if hot { (theme::ACCENT_HOVER, theme::PRIMARY_TO) } else { (theme::PRIMARY_FROM, theme::PRIMARY_TO) };
        theme::gradient_rect(dc, &r, radius, &[a, b]);
    } else {
        theme::round_rect(dc, &r, radius, Some(bg), border);
    }
    if focused {
        let fr = RECT { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
        theme::round_rect_w(dc, &fr, radius, None, Some(if primary { theme::ACCENT_PRESSED } else { theme::ACCENT }), px(2) as f32);
    }
    let fnt = if primary { theme::font_medium(theme::BODY) } else { c.font.clone() };
    let icon = c.comp.str("Icon").and_then(theme::icon_from_name);
    let tw = measure(&fnt, &c.caption).cx;
    let iw = if icon.is_some() { px(16) + if c.caption.is_empty() { 0 } else { px(8) } } else { 0 };
    let total = iw + tw;
    let x0 = r.left + ((r.right - r.left) - total) / 2;
    if let Some(ch) = icon {
        let ir = RECT { left: x0, top: r.top, right: x0 + px(16), bottom: r.bottom };
        theme::glyph(dc, ch, &ir, 16, fg);
    }
    let tr = RECT { left: x0 + iw, top: r.top, right: x0 + iw + tw + 1, bottom: r.bottom };
    theme::text(dc, &c.caption, &tr, &fnt, fg, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
}

unsafe extern "system" fn button_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => {
            if GetPropW(hwnd, wide("BxHot").as_ptr()).is_null() {
                SetPropW(hwnd, wide("BxHot").as_ptr(), 1 as _);
                InvalidateRect(hwnd, null(), 0);
                let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                TrackMouseEvent(&mut tme);
            }
        }
        WM_MOUSELEAVE => {
            RemovePropW(hwnd, wide("BxHot").as_ptr());
            InvalidateRect(hwnd, null(), 0);
        }
        // owner-drawn buttons lose the default-button look: Enter is handled by the dialog
        WM_GETDLGCODE => {
            let r = DefSubclassProc(hwnd, msg, wp, lp);
            if let Some((fi, id)) = lookup(hwnd) {
                let def = matches!(form(fi).ctls[id].kind, Kind::BitBtn { default: true, .. } | Kind::Button { default: true });
                if def {
                    return r | DLGC_DEFPUSHBUTTON as isize;
                }
            }
            return r;
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}

/// Custom draw of a toolbar item: rounded hover, accent primary, glyph icon, caption, chevron.
unsafe fn toolbar_item_draw(f: &Form, tb: usize, cd: &NMTBCUSTOMDRAW) -> LRESULT {
    let nm = &cd.nmcd;
    match nm.dwDrawStage {
        CDDS_PREPAINT => return CDRF_NOTIFYITEMDRAW as LRESULT,
        CDDS_ITEMPREPAINT => {}
        _ => return CDRF_DODEFAULT as LRESULT,
    }
    let Kind::ToolBar { buttons, .. } = &f.ctls[tb].kind else { return CDRF_DODEFAULT as LRESULT };
    let Some(b) = buttons.get(nm.dwItemSpec.wrapping_sub(1)) else { return CDRF_DODEFAULT as LRESULT };
    let dc = nm.hdc;
    let r = nm.rc;
    let state = nm.uItemState;
    let hot = state & CDIS_HOT != 0;
    let pressed = state & CDIS_SELECTED != 0;
    let checked = state & CDIS_CHECKED != 0;
    let disabled = state & CDIS_DISABLED != 0 && b.look != "title";
    let bg_parent = f.ctls[f.ctls[tb].parent.unwrap_or(0)].color;
    // toolbars on the brand bar draw light on dark
    let dark = theme::is_dark(bg_parent);
    let inner = RECT { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
    let radius = px(4) as f32;
    let fg;
    if b.look == "title" {
        let tf = theme::font_semibold(15);
        let mut x = r.left + px(4);
        if b.step > 0 {
            let d = px(24);
            let y = r.top + (r.bottom - r.top - d) / 2;
            let tone = card_tone(f, tb).map(|t| t.main).unwrap_or(theme::ACCENT);
            theme::badge(dc, &RECT { left: x, top: y, right: x + d, bottom: y + d }, &b.step.to_string(), tone, theme::WHITE);
            x += d + px(10);
        }
        theme::text(dc, &b.caption, &RECT { left: x, top: r.top, right: r.right, bottom: r.bottom }, &tf, if dark { theme::WHITE } else { theme::TEXT }, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        return CDRF_SKIPDEFAULT as LRESULT;
    }
    let mut icon_color = if disabled { theme::TEXT_DISABLED } else if b.look == "primary" { theme::WHITE } else { theme::ACCENT };
    if b.look == "primary" {
        if disabled {
            theme::round_rect(dc, &inner, radius, Some(theme::STROKE), None);
        } else {
            let (a, c2) = if pressed { (theme::ACCENT_PRESSED, theme::ACCENT_PRESSED) } else if hot { (theme::ACCENT_HOVER, theme::PRIMARY_TO) } else { (theme::PRIMARY_FROM, theme::PRIMARY_TO) };
            theme::gradient_rect(dc, &inner, radius, &[a, c2]);
        }
        fg = if disabled { theme::TEXT_DISABLED } else { theme::WHITE };
    } else if dark {
        // checked = the selected segment (language): a white pill with accent text
        if checked && !pressed {
            theme::round_rect(dc, &inner, px(6) as f32, Some(theme::WHITE), None);
        } else if pressed {
            theme::round_rect_alpha(dc, &inner, px(6) as f32, theme::WHITE, 70);
        } else if hot && !disabled {
            theme::round_rect_alpha(dc, &inner, px(6) as f32, theme::WHITE, 40);
        }
        fg = if checked && !pressed { theme::ACCENT } else if disabled { theme::BRAND_TEXT_SOFT } else { theme::WHITE };
        icon_color = fg;
    } else {
        if pressed || checked {
            theme::round_rect(dc, &inner, radius, Some(if checked && !pressed { theme::ACCENT_SOFT } else { theme::PRESSED }), None);
        } else if hot && !disabled {
            theme::round_rect(dc, &inner, radius, Some(theme::HOVER), None);
        }
        fg = if disabled { theme::TEXT_DISABLED } else if checked { theme::ACCENT } else { theme::TEXT };
    }
    let fnt = if b.look == "primary" { theme::font_medium(theme::BODY) } else { f.ctls[tb].font.clone() };
    let compact = match &f.ctls[tb].kind {
        Kind::ToolBar { buttons, compact, .. } => Form::collapsed_flags(buttons, *compact).get(nm.dwItemSpec.wrapping_sub(1)).copied().unwrap_or(false),
        _ => false,
    };
    if b.look == "icon" || compact {
        let w = if b.drop { r.right - px(14) } else { r.right };
        theme::glyph(dc, b.icon.unwrap_or(theme::icon::MORE), &RECT { left: r.left, top: r.top, right: w, bottom: r.bottom }, 16, icon_color);
        if b.drop {
            theme::glyph(dc, theme::icon::CHEVRON_DOWN, &RECT { left: w - px(4), top: r.top, right: r.right - px(2), bottom: r.bottom }, 9, fg);
        }
        return CDRF_SKIPDEFAULT as LRESULT;
    }
    let mut x = r.left + px(12);
    if let Some(ch) = b.icon {
        theme::glyph(dc, ch, &RECT { left: x, top: r.top, right: x + px(16), bottom: r.bottom }, 16, icon_color);
        x += px(16) + px(8);
    }
    let right = if b.drop { r.right - px(26) } else { r.right - px(8) };
    theme::text(dc, &b.caption, &RECT { left: x, top: r.top, right: right + px(4), bottom: r.bottom }, &fnt, fg, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    if b.drop {
        theme::glyph(dc, theme::icon::CHEVRON_DOWN, &RECT { left: r.right - px(24), top: r.top, right: r.right - px(8), bottom: r.bottom }, 9, fg);
    }
    CDRF_SKIPDEFAULT as LRESULT
}

// ---------------------------------------------------------------- window procedures

pub fn lookup(hwnd: HWND) -> Option<(usize, usize)> {
    ui().hwnd_map.get(&(hwnd as isize)).copied()
}

unsafe extern "system" fn box_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let Some((fi, id)) = lookup(hwnd) else {
        return DefWindowProcW(hwnd, msg, wp, lp);
    };
    let Some(f) = try_form(fi) else {
        return DefWindowProcW(hwnd, msg, wp, lp);
    };
    match msg {
        WM_ERASEBKGND => {
            // Children (transparent toolbars, themed buttons) ask us to paint behind them.
            let dc = wp as HDC;
            if WindowFromDC(dc) != hwnd {
                paint_container(f, id, dc);
            }
            return 1;
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let dc = BeginPaint(hwnd, &mut ps);
            let r = client_rect(hwnd);
            let buf = Buffer::new(dc, r.right, r.bottom);
            paint_container(f, id, buf.dc);
            buf.blit();
            drop(buf);
            EndPaint(hwnd, &ps);
            return 0;
        }
        WM_PRINTCLIENT => {
            if std::env::var_os("NERAMIT_NOPRINT").is_some() {
                return 0;
            }
            paint_container(f, id, wp as HDC);
            return 0;
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            let child = lp as HWND;
            let dc = wp as HDC;
            SetBkMode(dc, TRANSPARENT as i32);
            let mut color = f.ctls[id].color;
            SetTextColor(dc, theme::TEXT);
            if let Some((_, cid)) = lookup(child) {
                let cc = &f.ctls[cid];
                if !cc.enabled && matches!(cc.kind, Kind::CheckBox { .. } | Kind::Radio { .. }) {
                    SetTextColor(dc, theme::TEXT_DISABLED);
                }
                if let Some(p) = cc.parent {
                    color = f.ctls[p].color;
                }
                // read-only edits draw through WM_CTLCOLORSTATIC
                if let Kind::Edit { .. } = cc.kind {
                    color = theme::SURFACE_ALT;
                    SetBkColor(dc, color);
                    SetBkMode(dc, OPAQUE as i32);
                }
            } else if let Some((_, gid)) = lookup(GetParent(child)) {
                color = f.ctls[gid].color;
            }
            return bg_brush(color) as LRESULT;
        }
        WM_CTLCOLOREDIT => {
            let dc = wp as HDC;
            if let Some((_, cid)) = lookup(lp as HWND) {
                let cc = &f.ctls[cid];
                SetBkColor(dc, cc.color);
                SetTextColor(dc, cc.font.color);
                return bg_brush(cc.color) as LRESULT;
            }
        }
        WM_DRAWITEM => {
            let dis = &*(lp as *const DRAWITEMSTRUCT);
            if dis.CtlType == ODT_MENU {
                super::menu::draw_item(dis);
                return 1;
            }
            if let Some((cf, cid)) = lookup(dis.hwndItem) {
                draw_button(form(cf), cid, dis);
                return 1;
            }
        }
        WM_MEASUREITEM => {
            let mis = &mut *(lp as *mut MEASUREITEMSTRUCT);
            if mis.CtlType == ODT_MENU {
                super::menu::measure_item(hwnd, mis);
                return 1;
            }
        }
        WM_COMMAND => {
            let code = hiword(wp) as u32;
            let child = lp as HWND;
            if child.is_null() {
                // menu
                let cmd = loword(wp);
                let fid = super::menu::owner_form_of_cmd(cmd).unwrap_or(fi);
                let name = form(fid).menus.get(&cmd).cloned();
                if let Some(name) = name {
                    let mid = form(fid).try_id(&name).unwrap_or(0);
                    if form(fid).ctls[mid].comp.bool("AutoCheck", false) {
                        let now = !super::menu::is_checked(fid, &name);
                        form(fid).menu_check(&name, now);
                    }
                    fire(fid, mid, Ev::Menu(name));
                }
                return 0;
            }
            if let Some((cf, cid)) = lookup(child) {
                let kind = form(cf).ctls[cid].kind.clone();
                match kind {
                    Kind::CheckBox { .. } if code == BN_CLICKED as u32 => {
                        let h = form(cf).ctls[cid].hwnd;
                        let now = SendMessageW(h, BM_GETCHECK, 0, 0) != 1;
                        SendMessageW(h, BM_SETCHECK, now as usize, 0);
                        if let Kind::CheckBox { checked } = &mut form(cf).ctls[cid].kind {
                            *checked = now;
                        }
                        fire(cf, cid, Ev::Click);
                    }
                    Kind::Radio { .. } if code == BN_CLICKED as u32 => {
                        let h = form(cf).ctls[cid].hwnd;
                        let parent = form(cf).ctls[cid].parent.unwrap();
                        let sibs = form(cf).ctls[parent].children.clone();
                        for s in sibs {
                            if let Kind::Radio { checked } = &mut form(cf).ctls[s].kind {
                                *checked = s == cid;
                                SendMessageW(form(cf).ctls[s].hwnd, BM_SETCHECK, (s == cid) as usize, 0);
                            }
                        }
                        let _ = h;
                        fire(cf, cid, Ev::Click);
                    }
                    Kind::Button { .. } | Kind::BitBtn { .. } if code == BN_CLICKED as u32 => {
                        let handled = fire(cf, cid, Ev::Click);
                        if !handled {
                            if let Kind::BitBtn { modal, .. } = form(cf).ctls[cid].kind {
                                if modal != 0 {
                                    form(cf).modal_result = modal;
                                }
                            }
                        }
                    }
                    Kind::Edit { .. } | Kind::SpinEdit { .. } if code == EN_CHANGE => {
                        fire(cf, cid, Ev::Change);
                    }
                    Kind::Edit { .. } | Kind::SpinEdit { .. } if code == EN_KILLFOCUS => {
                        fire(cf, cid, Ev::Exit);
                    }
                    Kind::Combo { .. } if code == CBN_SELCHANGE => {
                        fire(cf, cid, Ev::Change);
                    }
                    Kind::ToolBar { .. } => {
                        let cmd = loword(wp) as usize;
                        if cmd >= 1 {
                            fire(cf, cid, Ev::Button(cmd - 1));
                        }
                    }
                    _ => {}
                }
                return 0;
            }
            // radio group buttons
            if let Some((cf, gid)) = lookup(GetParent(child)) {
                if let Kind::RadioGroup { .. } = form(cf).ctls[gid].kind {
                    if code == BN_CLICKED as u32 {
                        let idx = GetDlgCtrlID(child) - 5000;
                        form(cf).set_radio_index(&form(cf).ctls[gid].name.clone(), idx);
                        fire(cf, gid, Ev::Click);
                    }
                    return 0;
                }
            }
        }
        WM_NOTIFY => {
            let nm = &*(lp as *const NMHDR);
            if nm.code == NM_CUSTOMDRAW {
                if let Some((cf, cid)) = lookup(nm.hwndFrom) {
                    if let Kind::ToolBar { .. } = form(cf).ctls[cid].kind {
                        return toolbar_item_draw(form(cf), cid, &*(lp as *const NMTBCUSTOMDRAW));
                    }
                }
            }
            if let Some((cf, cid)) = lookup(nm.hwndFrom) {
                match form(cf).ctls[cid].kind.clone() {
                    Kind::PageControl { .. } if nm.code == TCN_SELCHANGE => {
                        let sel = SendMessageW(nm.hwndFrom, TCM_GETCURSEL, 0, 0) as usize;
                        form(cf).show_page(cid, sel);
                        fire(cf, cid, Ev::TabChange);
                    }
                    Kind::ToolBar { .. } if nm.code == TBN_DROPDOWN => {}
                    Kind::UpDown { .. } if nm.code == UDN_DELTAPOS => {
                        fire(cf, cid, Ev::Change);
                    }
                    _ => {}
                }
            }
            if nm.code == TTN_GETDISPINFOW {
                // tooltips of toolbar buttons
                let di = &mut *(lp as *mut NMTTDISPINFOW);
                let tb = GetParent(GetParent(nm.hwndFrom));
                let _ = tb;
                let idc = nm.idFrom;
                if let Some(text) = toolbar_hint(nm.hwndFrom, idc) {
                    TIP_BUF.with(|b| {
                        *b.borrow_mut() = wide(&text);
                        di.lpszText = b.borrow().as_ptr() as *mut u16;
                    });
                }
            }
        }
        WM_HSCROLL => {
            if let Some((cf, cid)) = lookup(lp as HWND) {
                fire(cf, cid, Ev::Change);
            }
        }
        WM_TIMER => {
            fire(fi, id, Ev::Custom(format!("timer{wp}")));
            return 0;
        }
        WM_SIZE => {
            if id == 0 && wp != SIZE_MINIMIZED as usize {
                f.realign(0);
                fire(fi, 0, Ev::Resize);
            }
        }
        WM_GETMINMAXINFO => {
            if id == 0 {
                let c = &f.ctls[0];
                if c.min_w > 0 || c.min_h > 0 {
                    let mmi = &mut *(lp as *mut MINMAXINFO);
                    mmi.ptMinTrackSize.x = mmi.ptMinTrackSize.x.max(c.min_w);
                    mmi.ptMinTrackSize.y = mmi.ptMinTrackSize.y.max(c.min_h);
                }
            }
        }
        WM_CLOSE => {
            if id == 0 {
                if fire(fi, 0, Ev::Close) {
                    return 0;
                }
                if f.modal {
                    f.modal_result = 2;
                    return 0;
                }
                ShowWindow(hwnd, SW_HIDE);
                return 0;
            }
        }
        WM_SETCURSOR => {
            if hiword(lp as usize) as u32 == WM_MOUSEMOVE || loword(lp as usize) as u32 == HTCLIENT {
                let mut pt = POINT { x: 0, y: 0 };
                GetCursorPos(&mut pt);
                ScreenToClient(hwnd, &mut pt);
                if let Some(s) = splitter_at(f, id, pt.x, pt.y) {
                    let vertical = matches!(f.ctls[s].kind, Kind::Splitter { vertical: true, .. });
                    SetCursor(LoadCursorW(null_mut(), if vertical { IDC_SIZEWE } else { IDC_SIZENS }));
                    return 1;
                }
            }
        }
        WM_LBUTTONDOWN => {
            let (x, y) = (get_x(lp), get_y(lp));
            if let Some(s) = splitter_at(f, id, x, y) {
                f.drag = Some((s, x, y));
                SetCapture(hwnd);
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            if let Some(g) = graphic_at(f, id, x, y) {
                fire(fi, g, Ev::Click);
            }
        }
        WM_LBUTTONDBLCLK => {
            let (x, y) = (get_x(lp), get_y(lp));
            if let Some(s) = splitter_at(f, id, x, y) {
                fire(fi, s, Ev::DblClick);
                return 0;
            }
            if let Some(g) = graphic_at(f, id, x, y) {
                fire(fi, g, Ev::DblClick);
            }
        }
        WM_MOUSEMOVE => {
            if let Some((s, sx, sy)) = f.drag {
                let (x, y) = (get_x(lp), get_y(lp));
                drag_splitter(f, s, x - sx, y - sy);
                f.drag = Some((s, x, y));
                return 0;
            }
        }
        WM_LBUTTONUP => {
            if f.drag.take().is_some() {
                ReleaseCapture();
                InvalidateRect(hwnd, null(), 0);
                fire(fi, id, Ev::Custom("SplitterMoved".into()));
                return 0;
            }
        }
        WM_KEYDOWN => {
            fire(fi, id, Ev::KeyDown(wp as u32));
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

thread_local! {
    static TIP_BUF: std::cell::RefCell<Vec<u16>> = std::cell::RefCell::new(vec![0]);
    static BRUSHES: std::cell::RefCell<HashMap<u32, HBRUSH>> = std::cell::RefCell::new(HashMap::new());
}

pub fn bg_brush(color: u32) -> HBRUSH {
    BRUSHES.with(|b| *b.borrow_mut().entry(color).or_insert_with(|| unsafe { CreateSolidBrush(color) }))
}

unsafe fn toolbar_hint(tooltip: HWND, idc: usize) -> Option<String> {
    // find the toolbar that owns this tooltip
    for (h, (fi, id)) in ui().hwnd_map.clone() {
        let hw = h as HWND;
        if let Some(f) = try_form(fi) {
            if let Kind::ToolBar { buttons, .. } = &f.ctls[id].kind {
                if SendMessageW(hw, TB_GETTOOLTIPS, 0, 0) as HWND == tooltip {
                    return buttons.get(idc.wrapping_sub(1)).map(|b| b.hint.clone()).filter(|s| !s.is_empty());
                }
            }
        }
    }
    None
}

fn splitter_at(f: &Form, id: usize, x: i32, y: i32) -> Option<usize> {
    f.ctls[id].children.iter().copied().find(|&k| {
        let c = &f.ctls[k];
        matches!(c.kind, Kind::Splitter { .. }) && c.visible && x >= c.x && x < c.x + c.w && y >= c.y && y < c.y + c.h
    })
}

fn graphic_at(f: &Form, id: usize, x: i32, y: i32) -> Option<usize> {
    f.ctls[id].children.iter().copied().find(|&k| {
        let c = &f.ctls[k];
        !c.windowed() && c.visible && x >= c.x && x < c.x + c.w && y >= c.y && y < c.y + c.h
    })
}

/// TSplitter: resize the control it is attached to (the previous control with the same align).
unsafe fn drag_splitter(f: &mut Form, s: usize, dx: i32, dy: i32) {
    let p = f.ctls[s].parent.unwrap();
    let (sa, sx, sy, min) = {
        let c = &f.ctls[s];
        (c.align, c.x, c.y, match c.kind {
            Kind::Splitter { min, .. } => min,
            _ => 30,
        })
    };
    let kids = f.ctls[p].children.clone();
    let target = kids.iter().copied().filter(|&k| k != s && f.ctls[k].align == sa && f.ctls[k].visible).min_by_key(|&k| {
        let c = &f.ctls[k];
        match sa {
            Align::Top => (sy - (c.y + c.h)).abs(),
            Align::Left => (sx - (c.x + c.w)).abs(),
            Align::Bottom => (c.y - (sy + 3)).abs(),
            _ => (c.x - sx).abs(),
        }
    });
    let Some(t) = target else { return };
    let (pw, ph) = f.client_size(p);
    let c = &mut f.ctls[t];
    match sa {
        Align::Top => c.h = (c.h + dy).clamp(min.min(ph), (ph - min).max(min)),
        Align::Bottom => c.h = (c.h - dy).clamp(min.min(ph), (ph - min).max(min)),
        Align::Left => c.w = (c.w + dx).clamp(min.min(pw), (pw - min).max(min)),
        Align::Right => c.w = (c.w - dx).clamp(min.min(pw), (pw - min).max(min)),
        _ => {}
    }
    f.realign(p);
    let host = f.host_of(p);
    InvalidateRect(host, null(), 0);
}

unsafe extern "system" fn toolbar_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    if let Some((fi, id)) = lookup(hwnd) {
        if let Some(f) = try_form(fi) {
            let edge_top = match f.ctls[id].kind {
                Kind::ToolBar { edge_top, .. } => edge_top,
                _ => 0,
            };
            match msg {
                WM_NCCALCSIZE if edge_top > 0 => {
                    let r = DefSubclassProc(hwnd, msg, wp, lp);
                    let rc = &mut *(lp as *mut RECT);
                    rc.top += edge_top;
                    return r;
                }
                WM_NCPAINT if edge_top > 0 => {
                    let dc = GetWindowDC(hwnd);
                    let mut wr = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                    GetWindowRect(hwnd, &mut wr);
                    let w = wr.right - wr.left;
                    // TToolBar.WMNCPaint: DrawEdge(EdgeInner/EdgeOuter, BF_TOP)
                    let (inner, outer) = (f.ctls[id].comp.str("EdgeInner").unwrap_or("esRaised").to_string(), f.ctls[id].comp.str("EdgeOuter").unwrap_or("esLowered").to_string());
                    let mut flags = 0;
                    flags |= match inner.as_str() {
                        "esRaised" => BDR_RAISEDINNER,
                        "esLowered" => BDR_SUNKENINNER,
                        _ => 0,
                    };
                    flags |= match outer.as_str() {
                        "esRaised" => BDR_RAISEDOUTER,
                        "esLowered" => BDR_SUNKENOUTER,
                        _ => 0,
                    };
                    let mut r = rect(0, 0, w, edge_top);
                    fill(dc, &r, f.ctls[id].color);
                    let mut r2 = rect(0, 0, w, 10);
                    DrawEdge(dc, &mut r2, flags, BF_TOP);
                    let _ = &mut r;
                    ReleaseDC(hwnd, dc);
                    return 0;
                }
                WM_COMMAND => {
                    let cmd = loword(wp) as usize;
                    if cmd >= 1 && lp == 0 {
                        fire(fi, id, Ev::Button(cmd - 1));
                        return 0;
                    }
                }
                WM_ERASEBKGND => {
                    // transparent toolbar: let the parent paint behind it
                    let dc = wp as HDC;
                    let r = client_rect(hwnd);
                    let p = f.ctls[id].parent.unwrap();
                    if f.in_themed_sheet(id) {
                        DrawThemeParentBackground(hwnd, dc, null());
                    } else {
                        fill(dc, &r, f.ctls[p].color);
                    }
                    return 1;
                }
                _ => {}
            }
        }
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}

unsafe extern "system" fn tabs_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    if let Some((fi, id)) = lookup(hwnd) {
        match msg {
            WM_ERASEBKGND => return 1,
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let dc = BeginPaint(hwnd, &mut ps);
                let r = client_rect(hwnd);
                let f = form(fi);
                let h = f.tab_strip_h(id).min(r.bottom.max(1));
                let buf = Buffer::new(dc, r.right.max(1), h);
                f.paint_tabs(id, buf.dc);
                buf.blit();
                drop(buf);
                EndPaint(hwnd, &ps);
                return 0;
            }
            WM_LBUTTONDOWN => {
                let (x, y) = (get_x(lp), get_y(lp));
                let f = form(fi);
                if let Some((i, _)) = f.tab_layout(id).into_iter().find(|(_, r)| x >= r.left && x < r.right && y >= r.top && y < r.bottom) {
                    let cur = f.page_index(&f.ctls[id].name.clone());
                    if i != cur {
                        f.show_page(id, i);
                        InvalidateRect(hwnd, null(), 0);
                        fire(fi, id, Ev::TabChange);
                    }
                }
                return 0;
            }
            WM_MOUSEMOVE => {
                let (x, y) = (get_x(lp), get_y(lp));
                let f = form(fi);
                let hot = f.tab_layout(id).into_iter().find(|(_, r)| x >= r.left && x < r.right && y >= r.top && y < r.bottom).map(|(i, _)| i as isize).unwrap_or(-1);
                let old = GetPropW(hwnd, wide("BxHotTab").as_ptr()) as isize - 1;
                if hot != old {
                    SetPropW(hwnd, wide("BxHotTab").as_ptr(), (hot + 1) as _);
                    InvalidateRect(hwnd, null(), 0);
                    let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    TrackMouseEvent(&mut tme);
                }
                return 0;
            }
            WM_MOUSELEAVE => {
                SetPropW(hwnd, wide("BxHotTab").as_ptr(), null_mut());
                InvalidateRect(hwnd, null(), 0);
                return 0;
            }
            WM_SIZE => {
                let r = DefSubclassProc(hwnd, msg, wp, lp);
                let f = form(fi);
                let idx = match f.ctls[id].kind {
                    Kind::PageControl { index, .. } => index,
                    _ => 0,
                };
                f.show_page(id, idx);
                return r;
            }
            WM_NOTIFY | WM_COMMAND | WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_DRAWITEM => {
                // children of sheets talk to the sheet windows; forward anything else to the form proc
                return box_proc(hwnd, msg, wp, lp);
            }
            _ => {}
        }
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}

fn edit_framed(fi: usize, id: usize) -> bool {
    matches!(form(fi).ctls[id].kind, Kind::Edit { multiline: false } | Kind::SpinEdit { .. })
}

unsafe fn paint_edit_frame(hwnd: HWND, fi: usize, id: usize) {
    let f = form(fi);
    let c = &f.ctls[id];
    let mut wr = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetWindowRect(hwnd, &mut wr);
    let (w, h) = (wr.right - wr.left, wr.bottom - wr.top);
    let mut cr = client_rect(hwnd);
    let mut pt = POINT { x: 0, y: 0 };
    ClientToScreen(hwnd, &mut pt);
    OffsetRect(&mut cr, pt.x - wr.left, pt.y - wr.top);
    let dc = GetWindowDC(hwnd);
    let buf = Buffer::new(dc, w.max(1), h.max(1));
    let parent = c.parent.map(|p| f.ctls[p].color).unwrap_or(theme::SURFACE);
    let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
    let disabled = style & WS_DISABLED != 0;
    let readonly = style & ES_READONLY as u32 != 0;
    let focused = GetFocus() == hwnd;
    let bg = if disabled || readonly { theme::SURFACE_ALT } else { theme::SURFACE };
    let r = rect(0, 0, w, h);
    fill(buf.dc, &r, parent);
    theme::round_rect(buf.dc, &r, px(4) as f32, Some(bg), Some(if focused { theme::STROKE_STRONG } else { theme::STROKE_STRONG }));
    if !disabled && !readonly {
        let lh = if focused { px(2) } else { 1 };
        let line = RECT { left: px(2), top: h - lh, right: w - px(2), bottom: h };
        fill(buf.dc, &line, if focused { theme::ACCENT } else { theme::TEXT_SUBTLE });
    }
    // copy everything except the client area
    let saved = SaveDC(dc);
    ExcludeClipRect(dc, cr.left, cr.top, cr.right, cr.bottom);
    BitBlt(dc, 0, 0, w, h, buf.dc, 0, 0, SRCCOPY);
    RestoreDC(dc, saved);
    drop(buf);
    ReleaseDC(hwnd, dc);
}

unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    // boxes with "ShapedText": while they do not have the focus the text (or the cue hint) is
    // drawn by the program in the UI face, like every other label; typing uses the edit
    if msg == WM_PAINT && GetFocus() != hwnd {
        if let Some((fi, id)) = lookup(hwnd) {
            let c = &form(fi).ctls[id];
            if c.comp.bool("ShapedText", false) {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let dc = BeginPaint(hwnd, &mut ps);
                let r = client_rect(hwnd);
                fill(dc, &r, theme::SURFACE);
                let text = get_text(hwnd);
                let (s, color) = if text.is_empty() { (c.comp.str("Cue").unwrap_or("").to_string(), theme::TEXT_SUBTLE) } else { (text, theme::TEXT) };
                let s = if s.is_empty() { s } else { crate::i18n::tr(&s) };
                theme::text(dc, &s, &RECT { left: 1, top: 0, right: r.right, bottom: r.bottom }, &c.font, color, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
                EndPaint(hwnd, &ps);
                return 0;
            }
        }
    }
    if let Some((fi, id)) = lookup(hwnd) {
        if edit_framed(fi, id) {
            match msg {
                WM_NCCALCSIZE => {
                    let rc = &mut *(lp as *mut RECT);
                    let h = rc.bottom - rc.top;
                    let lh = text_height(&form(fi).ctls[id].font);
                    let pad_y = ((h - lh) / 2).max(1);
                    rc.left += px(10);
                    rc.right -= px(10);
                    rc.top += pad_y;
                    rc.bottom -= (h - lh - pad_y).max(1);
                    return 0;
                }
                WM_NCPAINT => {
                    paint_edit_frame(hwnd, fi, id);
                    return 0;
                }
                WM_SETFOCUS | WM_KILLFOCUS | WM_ENABLE | WM_SIZE => {
                    let r = DefSubclassProc(hwnd, msg, wp, lp);
                    RedrawWindow(hwnd, null(), null_mut(), RDW_FRAME | RDW_INVALIDATE | RDW_NOCHILDREN);
                    return r;
                }
                WM_NCHITTEST => {
                    // the padding belongs to the text box
                    let r = DefSubclassProc(hwnd, msg, wp, lp);
                    return if r == HTNOWHERE as isize || r == HTBORDER as isize { HTCLIENT as isize } else { r };
                }
                _ => {}
            }
        }
    }
    if msg == WM_KEYDOWN {
        if let Some((fi, id)) = lookup(hwnd) {
            if fire(fi, id, Ev::KeyDown(wp as u32)) {
                return 0;
            }
        }
    }
    if msg == WM_CHAR && (wp == 13 || wp == 10) {
        if let Some((fi, id)) = lookup(hwnd) {
            if matches!(form(fi).ctls[id].kind, Kind::SpinEdit { .. }) {
                return 0;
            }
        }
    }
    if msg == WM_GETDLGCODE {
        if let Some((fi, id)) = lookup(hwnd) {
            if let Kind::Edit { multiline: true } = form(fi).ctls[id].kind {
                return DefSubclassProc(hwnd, msg, wp, lp);
            }
            let r = DefSubclassProc(hwnd, msg, wp, lp);
            return r & !(DLGC_WANTALLKEYS as isize);
        }
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}

/// Message loop for the application main window.
pub unsafe fn run_loop(main: HWND) -> i32 {
    let mut msg: MSG = std::mem::zeroed();
    while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
        if super::grid::pre_translate(&msg) {
            continue;
        }
        if (msg.message == WM_KEYDOWN || msg.message == WM_SYSKEYDOWN) && ui().key_hook.map(|h| h(&msg)).unwrap_or(false) {
            continue;
        }
        let top = GetAncestor(msg.hwnd, GA_ROOT);
        if !top.is_null() && top != main && IsDialogMessageW(top, &msg) != 0 {
            continue;
        }
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    msg.wParam as i32
}

pub unsafe fn hot_tracking_bitbtn(hwnd: HWND, on: bool) {
    SetPropW(hwnd, wide("BxHot").as_ptr(), on as usize as _);
    InvalidateRect(hwnd, null(), 0);
}

pub fn vk_down(vk: u16) -> bool {
    unsafe { GetKeyState(vk as i32) < 0 }
}
