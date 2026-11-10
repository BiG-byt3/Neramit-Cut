//! Data model: pieces, stock bars, materials, cutting settings and solutions (the XML files
//! hold them as parts/row, stock/row, materials/linear/material, technical_parameters/linear
//! and results/linear/bar).

#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub length: f64,
    /// "width" in the XML; the 1D program calls it Height
    pub height: f64,
    pub quantity: i32,
    pub label: String,
    pub material: String,
    pub customer: String,
    pub color: i32,
    pub id: i32,
    pub use_it: bool,
    pub a1: f64,
    pub a2: f64,
}

impl Default for Part {
    fn default() -> Self {
        Part { length: 0.0, height: 0.0, quantity: 1, label: String::new(), material: String::new(), customer: String::new(), color: 16777215, id: 0, use_it: true, a1: 90.0, a2: 90.0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stock {
    pub length: f64,
    pub height: f64,
    pub quantity: i32,
    pub label: String,
    pub material: String,
    pub price: f64,
    pub priority: i32,
    pub id: i32,
    pub use_it: bool,
    pub a1: f64,
    pub a2: f64,
    /// weight of one metre (kg/m); 0 = from the material, or the standard rebar table
    pub weight: f64,
}

impl Default for Stock {
    fn default() -> Self {
        Stock { length: 0.0, height: 0.0, quantity: 1, label: String::new(), material: String::new(), price: 0.0, priority: 0, id: 0, use_it: true, a1: 90.0, a2: 90.0, weight: 0.0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    pub label: String,
    pub color: i32,
    pub ox: bool,
    pub oy: bool,
    pub oz: bool,
    pub mass: f64,
    pub area: f64,
}

impl Material {
    pub fn named(name: &str) -> Material {
        Material { name: name.into(), label: String::new(), color: 16777215, ox: true, oy: true, oz: true, mass: 0.0, area: 0.0 }
    }
}

/// technical_parameters/linear
#[derive(Clone, Debug, PartialEq)]
pub struct Linear {
    pub random_seed: i32,
    pub precision: f64,
    pub optimization_level: i32,
    /// 0 minimize waste, 1 minimize price
    pub optimization_type: i32,
    pub cutter_thickness: f64,
    pub same_cutter_for_all: bool,
    pub minimal_useful_size: f64,
    pub waste_le: f64,
    pub waste_ge: f64,
    pub max_pieces_per_bar: i32,
    pub add_grinding: bool,
    pub use_global_rotation: bool,
    pub ox: bool,
    pub oy: bool,
    pub oz: bool,
    pub space_between_cuts: f64,
    pub num_threads: i32,
}

impl Default for Linear {
    fn default() -> Self {
        Linear {
            random_seed: 0,
            precision: 0.001,
            optimization_level: 100,
            optimization_type: 0,
            cutter_thickness: 0.0,
            same_cutter_for_all: true,
            minimal_useful_size: 0.0,
            waste_le: 0.0,
            waste_ge: 0.0,
            max_pieces_per_bar: 0,
            add_grinding: true,
            use_global_rotation: false,
            ox: true,
            oy: true,
            oz: true,
            space_between_cuts: 0.0,
            num_threads: 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Technical {
    /// 0 price per bar, 1 price per unit of length
    pub price_per_what: i32,
    /// fractional rounding 1/2^(index+1); default 4 (1/32)
    pub round_fraction_index: i32,
    pub linear: Linear,
}

impl Default for Technical {
    fn default() -> Self {
        Technical { price_per_what: 0, round_fraction_index: 4, linear: Linear::default() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FontPrefs {
    pub name: String,
    pub size: i32,
    pub bold: bool,
    pub italic: bool,
}

impl Default for FontPrefs {
    fn default() -> Self {
        FontPrefs { name: "Noto Sans Thai".into(), size: 10, bold: false, italic: false }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Company {
    pub name: String,
    pub address: String,
    pub address2: String,
    pub city: String,
    pub zip: String,
    pub phone: String,
    pub email: String,
    /// logo image file
    pub logo: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColumnLayout {
    pub key: String,
    pub index: i32,
    pub visible: bool,
    pub width: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Interface {
    pub font: FontPrefs,
    pub font_print: FontPrefs,
    pub font_grid: FontPrefs,
    pub num_digits: i32,
    pub num_digits_percent: i32,
    pub num_digits_price: i32,
    pub language: String,
    pub header: String,
    pub company: Company,
    pub two_panels: bool,
    /// panel sizes of older settings in pixels (0 = none); converted to the shares below
    pub parts_panel_height: i32,
    pub basic_data_width: i32,
    /// share of the window for the pieces/stock column and of that column for the pieces
    /// (0 = the balanced default); kept as shares so the layout follows the window size
    pub basic_data_ratio: f64,
    pub parts_ratio: f64,
    /// main window of the last session: outer size in 96-DPI units (0 = the default size),
    /// top-left corner in screen pixels, maximized
    pub window_w: i32,
    pub window_h: i32,
    pub window_x: i32,
    pub window_y: i32,
    pub window_max: bool,
    pub group_parts_after_loading: bool,
    pub show_piece_size: bool,
    pub show_piece_label: bool,
    pub show_waste_size: bool,
    pub as_text: bool,
    pub show_customer: bool,
    pub show_angles: bool,
    pub group_identical: bool,
    pub fixed_height: bool,
    pub parts_cols: Vec<ColumnLayout>,
    pub stock_cols: Vec<ColumnLayout>,
    pub results_cols: Vec<ColumnLayout>,
    /// 2 = 2.x layout (column widths in 96-DPI units), 3 = cutting formula column,
    /// 4 = balanced panels (sizes 0 = automatic)
    pub layout_version: i32,
}

pub const LAYOUT_VERSION: i32 = 5;

fn cols(items: &[(&str, i32, bool, i32)]) -> Vec<ColumnLayout> {
    items.iter().map(|(k, i, v, w)| ColumnLayout { key: (*k).into(), index: *i, visible: *v, width: *w }).collect()
}

impl Default for Interface {
    fn default() -> Self {
        Interface {
            font: FontPrefs::default(),
            font_print: FontPrefs::default(),
            font_grid: FontPrefs::default(),
            num_digits: 3,
            num_digits_percent: 2,
            num_digits_price: 3,
            language: "Thai".into(),
            header: String::new(),
            company: Company::default(),
            two_panels: true,
            parts_panel_height: 0,
            basic_data_width: 0,
            basic_data_ratio: 0.0,
            parts_ratio: 0.0,
            window_w: 0,
            window_h: 0,
            window_x: 0,
            window_y: 0,
            window_max: false,
            group_parts_after_loading: false,
            show_piece_size: true,
            show_piece_label: true,
            show_waste_size: false,
            as_text: false,
            show_customer: false,
            show_angles: false,
            group_identical: false,
            fixed_height: false,
            parts_cols: cols(&[("index", 0, true, 56), ("length", 1, true, 90), ("quantity", 2, true, 80), ("width", 3, false, 70), ("material", 4, true, 110), ("label", 5, false, 80), ("a1", 6, false, 62), ("a2", 7, false, 62), ("customer", 8, true, 104)]),
            stock_cols: cols(&[("index", 0, true, 56), ("length", 1, true, 90), ("quantity", 2, true, 80), ("width", 3, false, 72), ("material", 4, true, 110), ("price", 5, true, 80), ("weight", 6, false, 90), ("label", 7, true, 92), ("priority", 8, false, 72), ("a1", 9, false, 60), ("a2", 10, false, 60)]),
            results_cols: cols(&[("index", 0, true, 44), ("image", 1, false, 70), ("length", 2, true, 74), ("quantity", 3, true, 68), ("width", 4, false, 72), ("material", 5, true, 82), ("label", 6, false, 84), ("waste", 7, true, 64), ("waste_percent", 8, false, 76), ("price", 9, false, 76), ("mass", 10, false, 72), ("area", 11, false, 72), ("pattern", 12, true, 130), ("pieces", 13, true, 600)]),
            layout_version: LAYOUT_VERSION,
        }
    }
}

/// One piece placed on a bar of the solution.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub part: usize,
    pub length: f64,
    pub a1: f64,
    pub a2: f64,
    pub original_a1: f64,
    pub original_a2: f64,
    pub rot: (bool, bool, bool),
    pub label: String,
    pub customer: String,
    pub color: i32,
    pub id: i32,
    pub start_bottom_x: f64,
    pub end_bottom_x: f64,
    pub start_top_x: f64,
    pub end_top_x: f64,
    pub price: f64,
}

/// A distinct cutting pattern ("bar") with its multiplicity.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub stock: usize,
    pub length: f64,
    pub height: f64,
    pub quantity: i32,
    pub material: String,
    pub label: String,
    pub priority: i32,
    pub id: i32,
    pub pieces: Vec<Placed>,
    /// reusable offcut (>= minimal useful size), 0 if none
    pub waste: f64,
    /// scrap (kerfs, spacing and unusable offcut)
    pub garbage_waste: f64,
    pub bar_price: f64,
    pub trim_left: f64,
    pub trim_right: f64,
    /// angle at the start of the offcut (180 - last cut)
    pub waste_angle: f64,
}

impl Bar {
    pub fn size_of_parts(&self) -> f64 {
        self.pieces.iter().map(|p| p.length).sum()
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Solution {
    pub bars: Vec<Bar>,
    /// (part index, count) of pieces that could not be placed
    pub unplaced: Vec<(usize, i32)>,
    pub running_time: f64,
    pub seed: i32,
    pub code: i32,
}

impl Solution {
    pub fn total_cost(&self) -> f64 {
        self.bars.iter().map(|b| b.bar_price * b.quantity as f64).sum()
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Project {
    pub parts: Vec<Part>,
    pub stock: Vec<Stock>,
    pub materials: Vec<Material>,
    pub technical: Technical,
    pub comments: String,
    pub name: String,
    pub demand_file_name: String,
    pub creation_time: String,
}

impl Project {
    /// The example job shown on the first start: a steel frame cut from 6 m and 4 m bars.
    pub fn sample() -> Project {
        let parts = [(1250.0, 6, "เสา"), (980.0, 8, "คานบน"), (760.0, 10, "คานล่าง"), (540.0, 12, "ค้ำยัน"), (420.0, 9, "ขาตั้ง"), (315.0, 14, "แผ่นรอง")]
            .iter()
            .enumerate()
            .map(|(i, (l, q, lab))| Part { length: *l, quantity: *q, label: (*lab).into(), material: DEFAULT_MATERIAL.into(), id: i as i32 + 1, ..Part::default() })
            .collect();
        let stock = vec![
            Stock { length: 6000.0, quantity: 12, material: DEFAULT_MATERIAL.into(), id: 1, ..Stock::default() },
            Stock { length: 4000.0, quantity: 8, material: DEFAULT_MATERIAL.into(), id: 2, ..Stock::default() },
        ];
        Project { parts, stock, materials: vec![Material::named(DEFAULT_MATERIAL)], ..Default::default() }
    }

    pub fn next_part_id(&self) -> i32 {
        self.parts.iter().map(|p| p.id).max().unwrap_or(0) + 1
    }
    pub fn next_stock_id(&self) -> i32 {
        self.stock.iter().map(|s| s.id).max().unwrap_or(0) + 1
    }
}

/// Columns that are no longer shown (height, label, angles): their values stay in the files.
pub const HIDDEN_PART_COLS: [&str; 4] = ["width", "label", "a1", "a2"];
pub const HIDDEN_STOCK_COLS: [&str; 3] = ["width", "a1", "a2"];

/// Standard weight (kg/m) of deformed (DB) and round (RB) bars by diameter in mm.
pub fn rebar_weight(material: &str) -> Option<f64> {
    let m = material.trim().to_uppercase();
    let digits: String = m.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
    if !(m.starts_with("DB") || m.starts_with("RB") || m.starts_with("SD") || m.starts_with("SR")) {
        return None;
    }
    let d: f64 = digits.parse().ok()?;
    Some(match d as i32 {
        6 => 0.222,
        9 => 0.499,
        10 => 0.616,
        12 => 0.888,
        16 => 1.578,
        20 => 2.466,
        25 => 3.853,
        28 => 4.834,
        32 => 6.313,
        36 => 7.990,
        40 => 9.865,
        _ => d * d * 0.006165,
    })
}

/// Material of the first start and of new rows.
pub const DEFAULT_MATERIAL: &str = "DB12";

/// Rebar sizes every material list starts with, and the material library of a new install.
pub const REBAR_MATERIALS: [&str; 7] = ["DB10", "DB12", "DB16", "DB20", "DB25", "RB6", "RB9"];

/// Name of the material of earlier versions' sample data; dropped from the library.
pub const OLD_SAMPLE_MATERIAL: &str = "เหล็กกล่อง";

/// Number display: up to `digits` decimals, trailing zeros removed.
pub fn fmt_num(v: f64, digits: i32) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let d = digits.clamp(0, 10) as usize;
    let mut s = format!("{:.*}", d, v);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        s = "0".into();
    }
    s
}

/// Parse a user number: decimal (dot or comma) or a fraction "a b/c", "b/c".
pub fn parse_num(s: &str) -> Option<f64> {
    let t = s.trim().replace(',', ".");
    if t.is_empty() {
        return Some(0.0);
    }
    if let Ok(v) = t.parse::<f64>() {
        return v.is_finite().then_some(v);
    }
    let parts: Vec<&str> = t.split_whitespace().collect();
    let frac = |x: &str| -> Option<f64> {
        let (a, b) = x.split_once('/')?;
        let (a, b) = (a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?);
        (b != 0.0).then(|| a / b)
    };
    match parts.as_slice() {
        [x] => frac(x),
        [w, x] => Some(w.parse::<f64>().ok()? + frac(x)?),
        _ => None,
    }
}
