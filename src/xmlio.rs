//! Project / parts / stock / settings XML files
//! (<?xml version="1.0"?>, tab indentation, values as element text).

use crate::model::*;
// ------------------------------------------------------------------ tiny DOM

#[derive(Clone, Debug, Default)]
pub struct Node {
    pub name: String,
    pub text: String,
    pub kids: Vec<Node>,
}

impl Node {
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.kids.iter().find(|k| k.name == name)
    }
    pub fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.kids.iter().filter(move |k| k.name == name)
    }
    /// text of the first child among `names` (aliases)
    pub fn get(&self, names: &[&str]) -> Option<&str> {
        names.iter().find_map(|n| self.child(n)).map(|n| n.text.as_str())
    }
    pub fn f64(&self, names: &[&str], d: f64) -> f64 {
        self.get(names).and_then(parse_num).unwrap_or(d)
    }
    pub fn i32(&self, names: &[&str], d: i32) -> i32 {
        self.get(names).and_then(|s| s.trim().parse::<f64>().ok()).map(|v| v as i32).unwrap_or(d)
    }
    pub fn bool(&self, names: &[&str], d: bool) -> bool {
        self.get(names).map(|s| s.trim() != "0" && !s.trim().is_empty()).unwrap_or(d)
    }
    pub fn str(&self, names: &[&str]) -> String {
        self.get(names).unwrap_or("").to_string()
    }
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(j) = rest.find(';') else { break };
        let ent = &rest[1..j];
        let ch = match ent {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            n if n.starts_with("#x") || n.starts_with("#X") => u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32),
            n if n.starts_with('#') => n[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[j + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Element/text parser (attributes are ignored: the format has none).
pub fn parse(text: &str) -> Result<Node, String> {
    let s = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut stack: Vec<Node> = vec![Node { name: "#root".into(), ..Default::default() }];
    let mut i = 0;
    let b = s.as_bytes();
    while i < b.len() {
        if b[i] != b'<' {
            let j = s[i..].find('<').map(|k| i + k).unwrap_or(b.len());
            stack.last_mut().unwrap().text.push_str(&unescape(&s[i..j]));
            i = j;
            continue;
        }
        let r = &s[i..];
        let skip = |end: &str| r.find(end).map(|k| i + k + end.len()).ok_or_else(|| format!("unterminated markup at byte {i}"));
        if r.starts_with("<?") {
            i = skip("?>")?;
        } else if r.starts_with("<!--") {
            i = skip("-->")?;
        } else if r.starts_with("<![CDATA[") {
            let j = skip("]]>")?;
            stack.last_mut().unwrap().text.push_str(&s[i + 9..j - 3]);
            i = j;
        } else if r.starts_with("<!") {
            i = skip(">")?;
        } else if r.starts_with("</") {
            let j = skip(">")?;
            let name = s[i + 2..j - 1].trim();
            let mut n = stack.pop().unwrap();
            if n.name != name || stack.is_empty() {
                return Err(format!("mismatched </{name}> at byte {i}"));
            }
            if !n.kids.is_empty() {
                n.text = n.text.trim().to_string();
            }
            stack.last_mut().unwrap().kids.push(n);
            i = j;
        } else {
            let j = skip(">")?;
            let inner = &s[i + 1..j - 1];
            let empty = inner.ends_with('/');
            let inner = inner.trim_end_matches('/');
            let name = inner.split(|c: char| c.is_whitespace()).next().unwrap_or("").to_string();
            if name.is_empty() {
                return Err(format!("bad tag at byte {i}"));
            }
            let n = Node { name, ..Default::default() };
            if empty {
                stack.last_mut().unwrap().kids.push(n);
            } else {
                stack.push(n);
            }
            i = j;
        }
    }
    if stack.len() != 1 {
        return Err(format!("unclosed <{}>", stack.last().unwrap().name));
    }
    let mut root = stack.pop().unwrap();
    root.kids.pop().ok_or_else(|| "empty XML".to_string())
}

// ------------------------------------------------------------------ writer (pugixml style)

pub struct W {
    out: String,
    depth: usize,
}

impl W {
    pub fn new(root: &str) -> W {
        let mut w = W { out: String::from("<?xml version=\"1.0\"?>\n"), depth: 0 };
        w.open(root);
        w
    }
    fn ind(&mut self) {
        for _ in 0..self.depth {
            self.out.push('\t');
        }
    }
    pub fn open(&mut self, name: &str) {
        self.ind();
        self.out.push_str(&format!("<{name}>\n"));
        self.depth += 1;
    }
    pub fn close(&mut self, name: &str) {
        self.depth -= 1;
        self.ind();
        self.out.push_str(&format!("</{name}>\n"));
    }
    pub fn val(&mut self, name: &str, v: &str) {
        self.ind();
        self.out.push_str(&format!("<{name}>{}</{name}>\n", esc(v)));
    }
    pub fn int(&mut self, name: &str, v: i64) {
        self.val(name, &v.to_string());
    }
    pub fn b(&mut self, name: &str, v: bool) {
        self.int(name, v as i64);
    }
    /// Numbers as text: integer, or decimal with minimal digits
    pub fn num(&mut self, name: &str, v: f64) {
        self.val(name, &num_text(v));
    }
    /// "%lg"
    pub fn lg(&mut self, name: &str, v: f64) {
        self.val(name, &lg(v));
    }
    pub fn finish(mut self) -> String {
        while self.depth > 0 {
            self.depth -= 1;
        }
        self.out
    }
    pub fn end_root(mut self, root: &str) -> String {
        self.close(root);
        self.out
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn num_text(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 9.0e15 {
        return format!("{}", v as i64);
    }
    for d in 1..=10 {
        let s = format!("{:.*}", d, v);
        if (s.parse::<f64>().unwrap_or(v) - v).abs() < 1e-12 * v.abs().max(1.0) {
            return s;
        }
    }
    format!("{v}")
}

/// C printf "%lg" (6 significant digits, trailing zeros removed)
pub fn lg(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let exp = v.abs().log10().floor() as i32;
    if !(-5..6).contains(&exp) {
        let s = format!("{:.5e}", v);
        let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
        let m = m.trim_end_matches('0').trim_end_matches('.');
        let e: i32 = e.parse().unwrap_or(0);
        return format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    }
    let decimals = (5 - exp).max(0) as usize;
    let s = format!("{:.*}", decimals, v);
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

// ------------------------------------------------------------------ records

pub fn write_part(w: &mut W, p: &Part) {
    w.open("row");
    w.num("length", p.length);
    w.num("width", p.height);
    w.int("quantity", p.quantity as i64);
    w.val("label", &p.label);
    w.val("material", &p.material);
    w.val("customer", &p.customer);
    w.int("color", p.color as i64);
    w.int("id", p.id as i64);
    w.b("use_it", p.use_it);
    w.lg("angle_1", p.a1);
    w.lg("angle_2", p.a2);
    w.close("row");
}

pub fn read_part(n: &Node) -> Part {
    Part {
        length: n.f64(&["length"], 0.0),
        height: n.f64(&["width", "height"], 0.0),
        quantity: n.i32(&["quantity"], 1),
        label: n.str(&["label"]),
        material: n.str(&["material"]),
        customer: n.str(&["customer"]),
        color: n.i32(&["color"], 16777215),
        id: n.i32(&["id"], 0),
        use_it: n.bool(&["use_it", "useit"], true),
        a1: n.f64(&["angle_1"], 90.0),
        a2: n.f64(&["angle_2"], 90.0),
    }
}

pub fn write_stock(w: &mut W, s: &Stock) {
    w.open("row");
    w.num("length", s.length);
    w.num("width", s.height);
    w.int("quantity", s.quantity as i64);
    w.val("label", &s.label);
    w.val("material", &s.material);
    w.lg("price", s.price);
    w.int("priority", s.priority as i64);
    w.int("id", s.id as i64);
    w.b("use_it", s.use_it);
    w.lg("angle_1", s.a1);
    w.lg("angle_2", s.a2);
    w.lg("weight_per_meter", s.weight);
    w.close("row");
}

pub fn read_stock(n: &Node) -> Stock {
    Stock {
        length: n.f64(&["length"], 0.0),
        height: n.f64(&["width", "height"], 0.0),
        quantity: n.i32(&["quantity"], 1),
        label: n.str(&["label"]),
        material: n.str(&["material"]),
        price: n.f64(&["price"], 0.0),
        priority: n.i32(&["priority"], 0),
        id: n.i32(&["id"], 0),
        use_it: n.bool(&["use_it", "useit"], true),
        a1: n.f64(&["angle_1"], 90.0),
        a2: n.f64(&["angle_2"], 90.0),
        weight: n.f64(&["weight_per_meter"], 0.0),
    }
}

pub fn write_material(w: &mut W, m: &Material) {
    w.open("material");
    w.val("name", &m.name);
    w.val("label", &m.label);
    w.int("color", m.color as i64);
    w.b("ox_rotation_allowed", m.ox);
    w.b("oy_rotation_allowed", m.oy);
    w.b("oz_rotation_allowed", m.oz);
    w.num("mass", m.mass);
    w.num("area", m.area);
    w.close("material");
}

pub fn read_material(n: &Node) -> Material {
    Material {
        name: n.str(&["name"]),
        label: n.str(&["label"]),
        color: n.i32(&["color"], 16777215),
        ox: n.bool(&["ox_rotation_allowed"], true),
        oy: n.bool(&["oy_rotation_allowed"], true),
        oz: n.bool(&["oz_rotation_allowed"], true),
        mass: n.f64(&["mass"], 0.0),
        area: n.f64(&["area"], 0.0),
    }
}

pub fn write_technical(w: &mut W, t: &Technical) {
    w.open("technical_parameters");
    w.int("price_per_what", t.price_per_what as i64);
    w.int("round_to_fractional_power_of_2_index", t.round_fraction_index as i64);
    let l = &t.linear;
    w.open("linear");
    w.int("random_seed", l.random_seed as i64);
    w.lg("precision", l.precision);
    w.int("optimization_level", l.optimization_level as i64);
    w.int("optimization_type", l.optimization_type as i64);
    w.num("cutter_thickness", l.cutter_thickness);
    w.b("use_the_same_cutter_thickness_for_all_materials", l.same_cutter_for_all);
    w.num("minimal_useful_size", l.minimal_useful_size);
    w.num("waste_less_equal_to", l.waste_le);
    w.num("waste_greater_equal_to", l.waste_ge);
    w.int("max_pieces_per_bar", l.max_pieces_per_bar as i64);
    w.b("add_grinding_thickness_to_part_size", l.add_grinding);
    w.b("use_global_rotation_for_all_materials", l.use_global_rotation);
    w.b("ox_rotation_allowed", l.ox);
    w.b("oy_rotation_allowed", l.oy);
    w.b("oz_rotation_allowed", l.oz);
    w.num("space_between_cuts", l.space_between_cuts);
    w.int("num_threads", l.num_threads as i64);
    w.close("linear");
    w.close("technical_parameters");
}

pub fn read_technical(n: &Node) -> Technical {
    let d = Technical::default();
    let ld = Linear::default();
    let l = n.child("linear");
    let lin = match l {
        Some(l) => Linear {
            random_seed: l.i32(&["random_seed"], ld.random_seed),
            precision: l.f64(&["precision"], ld.precision),
            optimization_level: l.i32(&["optimization_level"], ld.optimization_level),
            optimization_type: l.i32(&["optimization_type"], ld.optimization_type),
            cutter_thickness: l.f64(&["cutter_thickness", "cutter_thickness_1d"], ld.cutter_thickness),
            same_cutter_for_all: l.bool(&["use_the_same_cutter_thickness_for_all_materials"], ld.same_cutter_for_all),
            minimal_useful_size: l.f64(&["minimal_useful_size", "minimal_useful_size_1d"], ld.minimal_useful_size),
            waste_le: l.f64(&["waste_less_equal_to", "waste_less_equal_to_1d"], ld.waste_le),
            waste_ge: l.f64(&["waste_greater_equal_to", "waste_greater_equal_to_1d"], ld.waste_ge),
            max_pieces_per_bar: l.i32(&["max_pieces_per_bar"], ld.max_pieces_per_bar),
            add_grinding: l.bool(&["add_grinding_thickness_to_part_size"], ld.add_grinding),
            use_global_rotation: l.bool(&["use_global_rotation_for_all_materials"], ld.use_global_rotation),
            ox: l.bool(&["ox_rotation_allowed"], ld.ox),
            oy: l.bool(&["oy_rotation_allowed"], ld.oy),
            oz: l.bool(&["oz_rotation_allowed"], ld.oz),
            space_between_cuts: l.f64(&["space_between_cuts"], ld.space_between_cuts),
            num_threads: l.i32(&["num_threads"], ld.num_threads),
        },
        None => ld,
    };
    Technical {
        price_per_what: n.i32(&["price_per_what"], d.price_per_what),
        round_fraction_index: n.i32(&["round_to_fractional_power_of_2_index", "round_to_fractional_index"], d.round_fraction_index),
        linear: lin,
    }
}

/// results/linear (one <bar> per distinct pattern)
pub fn write_results(w: &mut W, sol: &Solution) {
    w.open("results");
    w.open("linear");
    for b in &sol.bars {
        w.open("bar");
        w.num("length", b.length);
        w.num("width", b.height);
        w.int("num_pieces", b.pieces.len() as i64);
        w.open("pieces");
        for p in &b.pieces {
            w.open("piece");
            w.num("length", p.length);
            w.lg("a1", p.a1);
            w.lg("original_a1", p.original_a1);
            w.lg("a2", p.a2);
            w.lg("original_a2", p.original_a2);
            w.b("rotated_ox", p.rot.0);
            w.b("rotated_oy", p.rot.1);
            w.b("rotated_oz", p.rot.2);
            w.int("priority", 0);
            w.val("label", &p.label);
            w.val("customer", &p.customer);
            w.int("color", p.color as i64);
            w.int("id", p.id as i64);
            w.num("start_bottom_x", p.start_bottom_x);
            w.num("end_bottom_x", p.end_bottom_x);
            w.num("start_top_x", p.start_top_x);
            w.num("end_top_x", p.end_top_x);
            w.lg("price", p.price);
            w.num("mass", 0.0);
            w.num("area", 0.0);
            w.close("piece");
        }
        w.close("pieces");
        w.num("size_of_parts", b.size_of_parts());
        w.int("quantity", b.quantity as i64);
        w.val("material", &b.material);
        w.val("label", &b.label);
        w.int("priority", b.priority as i64);
        w.open("waste");
        w.num("length", b.waste);
        w.num("width", b.height);
        w.val("label", "");
        w.close("waste");
        w.num("garbage_waste", b.garbage_waste);
        w.lg("bar_price", b.bar_price);
        w.lg("solution_cost", sol.total_cost());
        w.int("id", b.id as i64);
        w.open("trim");
        w.num("top_thick", 0.0);
        w.num("left_thick", b.trim_left);
        w.num("bottom_thick", 0.0);
        w.num("right_thick", b.trim_right);
        w.close("trim");
        w.lg("a1", 90.0);
        w.lg("a2", 90.0);
        w.num("mass", 0.0);
        w.num("area", 0.0);
        w.close("bar");
    }
    w.close("linear");
    w.close("results");
}

pub fn read_results(n: &Node, prj: &Project) -> Solution {
    let mut sol = Solution::default();
    let Some(lin) = n.child("linear") else { return sol };
    for b in lin.children("bar") {
        let length = b.f64(&["length"], 0.0);
        let material = b.str(&["material"]);
        let stock = prj.stock.iter().position(|s| (s.length - length).abs() < 1e-9 && s.material == material).unwrap_or(0);
        let mut pieces = vec![];
        if let Some(ps) = b.child("pieces") {
            for p in ps.children("piece") {
                let id = p.i32(&["id"], 0);
                let part = prj.parts.iter().position(|x| x.id == id).unwrap_or(0);
                pieces.push(Placed {
                    part,
                    length: p.f64(&["length"], 0.0),
                    a1: p.f64(&["a1"], 90.0),
                    a2: p.f64(&["a2"], 90.0),
                    original_a1: p.f64(&["original_a1"], 90.0),
                    original_a2: p.f64(&["original_a2"], 90.0),
                    rot: (p.bool(&["rotated_ox"], false), p.bool(&["rotated_oy"], false), p.bool(&["rotated_oz"], false)),
                    label: p.str(&["label"]),
                    customer: p.str(&["customer"]),
                    color: p.i32(&["color"], 16777215),
                    id,
                    start_bottom_x: p.f64(&["start_bottom_x"], 0.0),
                    end_bottom_x: p.f64(&["end_bottom_x"], 0.0),
                    start_top_x: p.f64(&["start_top_x"], 0.0),
                    end_top_x: p.f64(&["end_top_x"], 0.0),
                    price: p.f64(&["price"], 0.0),
                });
            }
        }
        let trim = b.child("trim");
        sol.bars.push(Bar {
            stock,
            length,
            height: b.f64(&["width", "height"], 0.0),
            quantity: b.i32(&["quantity"], 1),
            material,
            label: b.str(&["label"]),
            priority: b.i32(&["priority"], 0),
            id: b.i32(&["id"], 0),
            pieces,
            waste: b.child("waste").map(|w| w.f64(&["length"], 0.0)).unwrap_or(0.0),
            garbage_waste: b.f64(&["garbage_waste"], 0.0),
            bar_price: b.f64(&["bar_price"], 0.0),
            trim_left: trim.map(|t| t.f64(&["left_thick", "trim_left"], 0.0)).unwrap_or(0.0),
            trim_right: trim.map(|t| t.f64(&["right_thick", "trim_right"], 0.0)).unwrap_or(0.0),
            waste_angle: 90.0,
        });
    }
    sol
}

// ------------------------------------------------------------------ documents

pub fn parts_doc(parts: &[Part]) -> String {
    let mut w = W::new("data");
    w.open("parts");
    for p in parts {
        write_part(&mut w, p);
    }
    w.close("parts");
    w.end_root("data")
}

pub fn read_parts_doc(text: &str) -> Result<Vec<Part>, String> {
    let root = parse(text)?;
    let parts = root.child("parts").ok_or("no <parts> in file")?;
    Ok(parts.children("row").map(read_part).collect())
}

pub fn stock_doc(stock: &[Stock]) -> String {
    let mut w = W::new("data");
    w.open("stock");
    for s in stock {
        write_stock(&mut w, s);
    }
    w.close("stock");
    w.end_root("data")
}

pub fn read_stock_doc(text: &str) -> Result<Vec<Stock>, String> {
    let root = parse(text)?;
    let st = root.child("stock").ok_or("no <stock> in file")?;
    Ok(st.children("row").map(read_stock).collect())
}

pub fn materials_doc(mats: &[Material]) -> String {
    let mut w = W::new("data");
    w.open("materials");
    w.open("linear");
    for m in mats {
        write_material(&mut w, m);
    }
    w.close("linear");
    w.close("materials");
    w.end_root("data")
}

pub fn read_materials_doc(text: &str) -> Result<Vec<Material>, String> {
    let root = parse(text)?;
    let lin = root.child("materials").and_then(|m| m.child("linear")).ok_or("no <materials><linear>")?;
    Ok(lin.children("material").map(read_material).collect())
}

pub fn technical_doc(t: &Technical) -> String {
    let mut w = W::new("data");
    write_technical(&mut w, t);
    w.end_root("data")
}

pub fn read_technical_doc(text: &str) -> Result<Technical, String> {
    let root = parse(text)?;
    Ok(root.child("technical_parameters").map(read_technical).unwrap_or_default())
}

/// What a project export includes (Project import/export options window).
#[derive(Clone, Copy, Debug)]
pub struct Sections {
    pub parts: bool,
    pub stock: bool,
    pub settings: bool,
    pub results: bool,
    pub materials: bool,
    pub comments: bool,
}

impl Sections {
    pub fn all() -> Sections {
        Sections { parts: true, stock: true, settings: true, results: true, materials: true, comments: true }
    }
}

pub fn project_doc(prj: &Project, sol: Option<&Solution>, sec: Sections, version: &str) -> String {
    let mut w = W::new("data");
    if sec.parts {
        w.open("parts");
        for p in &prj.parts {
            write_part(&mut w, p);
        }
        w.close("parts");
    }
    if sec.stock {
        w.open("stock");
        for s in &prj.stock {
            write_stock(&mut w, s);
        }
        w.close("stock");
    }
    if sec.materials {
        w.open("materials");
        w.open("linear");
        for m in &prj.materials {
            write_material(&mut w, m);
        }
        w.close("linear");
        w.close("materials");
    }
    if sec.settings {
        write_technical(&mut w, &prj.technical);
    }
    if sec.results {
        if let Some(s) = sol {
            write_results(&mut w, s);
        }
    }
    w.val("project_name", &prj.name);
    w.val("demand_file_name", &prj.demand_file_name);
    w.val("program_version", version);
    w.val("working_dir", "");
    w.val("settings_dir", "");
    w.val("creation_time", &prj.creation_time);
    if sec.comments {
        w.val("comments", &prj.comments);
    }
    w.end_root("data")
}

pub struct Loaded {
    pub parts: Option<Vec<Part>>,
    pub stock: Option<Vec<Stock>>,
    pub materials: Option<Vec<Material>>,
    pub technical: Option<Technical>,
    pub results: Option<Node>,
    pub comments: Option<String>,
    pub creation_time: String,
    /// job name (section 0)
    pub name: String,
}

pub fn read_project(text: &str) -> Result<Loaded, String> {
    let root = parse(text)?;
    Ok(Loaded {
        parts: root.child("parts").map(|p| p.children("row").map(read_part).collect()),
        stock: root.child("stock").map(|p| p.children("row").map(read_stock).collect()),
        materials: root.child("materials").and_then(|m| m.child("linear")).map(|l| l.children("material").map(read_material).collect()),
        technical: root.child("technical_parameters").map(read_technical),
        results: root.child("results").cloned(),
        comments: root.child("comments").map(|c| c.text.clone()),
        creation_time: root.str(&["creation_time"]),
        name: root.str(&["project_name"]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn technical_xml_layout() {
        // technical.xml with the defaults (seed 2)
        let expected = "<?xml version=\"1.0\"?>\n<data>\n\t<technical_parameters>\n\t\t<price_per_what>0</price_per_what>\n\t\t<round_to_fractional_power_of_2_index>4</round_to_fractional_power_of_2_index>\n\t\t<linear>\n\t\t\t<random_seed>2</random_seed>\n\t\t\t<precision>0.001</precision>\n\t\t\t<optimization_level>100</optimization_level>\n\t\t\t<optimization_type>0</optimization_type>\n\t\t\t<cutter_thickness>0</cutter_thickness>\n\t\t\t<use_the_same_cutter_thickness_for_all_materials>1</use_the_same_cutter_thickness_for_all_materials>\n\t\t\t<minimal_useful_size>0</minimal_useful_size>\n\t\t\t<waste_less_equal_to>0</waste_less_equal_to>\n\t\t\t<waste_greater_equal_to>0</waste_greater_equal_to>\n\t\t\t<max_pieces_per_bar>0</max_pieces_per_bar>\n\t\t\t<add_grinding_thickness_to_part_size>1</add_grinding_thickness_to_part_size>\n\t\t\t<use_global_rotation_for_all_materials>0</use_global_rotation_for_all_materials>\n\t\t\t<ox_rotation_allowed>1</ox_rotation_allowed>\n\t\t\t<oy_rotation_allowed>1</oy_rotation_allowed>\n\t\t\t<oz_rotation_allowed>1</oz_rotation_allowed>\n\t\t\t<space_between_cuts>0</space_between_cuts>\n\t\t\t<num_threads>2</num_threads>\n\t\t</linear>\n\t</technical_parameters>\n</data>\n";
        let mut t = Technical::default();
        t.linear.random_seed = 2;
        assert_eq!(technical_doc(&t), expected);
        assert_eq!(read_technical_doc(expected).unwrap(), t);
    }

    #[test]
    fn materials_xml_layout() {
        let expected = "<?xml version=\"1.0\"?>\n<data>\n\t<materials>\n\t\t<linear>\n\t\t\t<material>\n\t\t\t\t<name>steel</name>\n\t\t\t\t<label></label>\n\t\t\t\t<color>16777215</color>\n\t\t\t\t<ox_rotation_allowed>1</ox_rotation_allowed>\n\t\t\t\t<oy_rotation_allowed>1</oy_rotation_allowed>\n\t\t\t\t<oz_rotation_allowed>1</oz_rotation_allowed>\n\t\t\t\t<mass>0</mass>\n\t\t\t\t<area>0</area>\n\t\t\t</material>\n\t\t</linear>\n\t</materials>\n</data>\n";
        assert_eq!(materials_doc(&[Material::named("steel")]), expected);
        assert_eq!(read_materials_doc(expected).unwrap(), vec![Material::named("steel")]);
    }

    #[test]
    fn lg_formats_like_printf() {
        assert_eq!(lg(0.001), "0.001");
        assert_eq!(lg(90.0), "90");
        assert_eq!(lg(12.5), "12.5");
        assert_eq!(lg(1234567.0), "1.23457e+06");
        assert_eq!(num_text(620.0), "620");
        assert_eq!(num_text(12.375), "12.375");
    }

    #[test]
    fn parts_roundtrip() {
        let prj = Project::sample();
        let back = read_parts_doc(&parts_doc(&prj.parts)).unwrap();
        assert_eq!(back, prj.parts);
    }
}
