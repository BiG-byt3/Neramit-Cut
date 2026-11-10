//! Component trees (the model of the window layouts built by ui::build).

use serde_json::{Map, Value};

#[derive(Clone, Debug)]
pub struct Comp {
    pub class: String,
    pub name: String,
    pub props: Map<String, Value>,
    pub children: Vec<Comp>,
}

impl Comp {
    pub fn from_json(v: &Value) -> Comp {
        Comp {
            class: v["class"].as_str().unwrap_or("").to_string(),
            name: v["name"].as_str().unwrap_or("").to_string(),
            props: v["properties"].as_object().cloned().unwrap_or_default(),
            children: v["children"].as_array().map(|a| a.iter().map(Comp::from_json).collect()).unwrap_or_default(),
        }
    }
    pub fn int(&self, key: &str, default: i32) -> i32 {
        self.props.get(key).and_then(|v| v.as_i64()).map(|v| v as i32).unwrap_or(default)
    }
    pub fn float(&self, key: &str, default: f64) -> f64 {
        self.props.get(key).and_then(|v| v.as_f64()).unwrap_or(default)
    }
    pub fn str(&self, key: &str) -> Option<&str> {
        self.props.get(key).and_then(|v| v.as_str())
    }
    pub fn bool(&self, key: &str, default: bool) -> bool {
        self.props.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
    }
    pub fn has(&self, key: &str) -> bool {
        self.props.contains_key(key)
    }
    pub fn list(&self, key: &str) -> Vec<String> {
        self.props
            .get(key)
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default()
    }
    pub fn set_has(&self, key: &str, item: &str) -> bool {
        self.list(key).iter().any(|s| s == item)
    }
    pub fn find(&self, name: &str) -> Option<&Comp> {
        if self.name == name {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(name))
    }
}
