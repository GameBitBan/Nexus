//! Settings, stored as `config.json` in the data folder — the same file and keys the Python version uses,
//! so both versions can read each other's settings. Unknown keys are kept as they are.

use crate::util::{load_json, save_json};
use serde_json::{Map, Value, json};
use std::path::PathBuf;

/// Defaults exported from the Python version, plus settings that are new in the Rust version.
const DEFAULTS_JSON: &str = include_str!("../../../assets/defaults.json");

pub fn defaults() -> Map<String, Value> {
    let mut d: Map<String, Value> = serde_json::from_str(DEFAULTS_JSON).expect("bundled defaults.json is valid");
    for (k, v) in [
        ("mpris", json!(true)),
        ("renderer", json!("auto")),
        ("lossy_color", json!("")),
        ("gapless", json!(true)),
        ("layout_pref", json!(false)),
    ] {
        d.entry(k.to_string()).or_insert(v);
    }
    d
}

#[derive(Clone, Debug)]
pub struct Config {
    pub path: PathBuf,
    values: Map<String, Value>,
}

impl Config {
    pub fn load(path: PathBuf) -> Self {
        let mut values = defaults();
        let saved: Map<String, Value> = load_json(&path, Map::new());
        for (k, v) in saved {
            values.insert(k, v);
        }
        Config { path, values }
    }

    pub fn save(&mut self) {
        if let Some(Value::String(pw)) = self.values.get("slsk_password") {
            if !pw.is_empty() && !pw.starts_with("dpapi:") {
                let p = crate::secret::protect(pw);
                self.values.insert("slsk_password".into(), Value::String(p));
            }
        }
        if let Err(e) = save_json(&self.path, &self.values) {
            log::error!("saving config: {e}");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    pub fn set(&mut self, key: &str, v: impl Into<Value>) {
        self.values.insert(key.to_string(), v.into());
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.values.shift_remove(key)
    }

    pub fn b(&self, key: &str) -> bool {
        match self.values.get(key) {
            Some(Value::Bool(b)) => *b,
            Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
            Some(Value::String(s)) => !s.is_empty(),
            _ => false,
        }
    }

    pub fn f(&self, key: &str) -> f64 {
        match self.values.get(key) {
            Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
            Some(Value::Bool(b)) => f64::from(u8::from(*b)),
            Some(Value::String(s)) => s.parse().unwrap_or(0.0),
            _ => 0.0,
        }
    }

    pub fn i(&self, key: &str) -> i64 {
        self.f(key).round() as i64
    }

    pub fn s(&self, key: &str) -> String {
        match self.values.get(key) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) | None => String::new(),
            Some(v) => v.to_string(),
        }
    }

    pub fn strings(&self, key: &str) -> Vec<String> {
        match self.values.get(key) {
            Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            _ => Vec::new(),
        }
    }

    pub fn floats(&self, key: &str) -> Vec<f64> {
        match self.values.get(key) {
            Some(Value::Array(a)) => a.iter().filter_map(Value::as_f64).collect(),
            _ => Vec::new(),
        }
    }

    pub fn map(&self, key: &str) -> Map<String, Value> {
        match self.values.get(key) {
            Some(Value::Object(m)) => m.clone(),
            _ => Map::new(),
        }
    }

    pub fn all(&self) -> &Map<String, Value> {
        &self.values
    }

    /// The Soulseek password in plain text (it's DPAPI-encrypted at rest on Windows).
    pub fn secret(&self, key: &str) -> String {
        crate::secret::reveal(&self.s(key))
    }
}
