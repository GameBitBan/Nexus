//! `playlists.json` (name → list of paths, in the order they were made) and `stats.json` (play counts).

use crate::paths::data_dir;
use crate::util::{load_json, save_json};
use serde_json::{Map, Value, json};

#[derive(Clone, Debug, Default)]
pub struct Playlists {
    pub items: Vec<(String, Vec<String>)>,
}

impl Playlists {
    pub fn load() -> Self {
        let m: Map<String, Value> = load_json(&data_dir().join("playlists.json"), Map::new());
        let items = m
            .into_iter()
            .map(|(k, v)| {
                let paths = v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                (k, paths)
            })
            .collect();
        Playlists { items }
    }

    pub fn save(&self) {
        let m: Map<String, Value> = self.items.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
        if let Err(e) = save_json(&data_dir().join("playlists.json"), &m) {
            log::error!("saving playlists: {e}");
        }
    }

    pub fn names(&self) -> Vec<String> {
        self.items.iter().map(|(k, _)| k.clone()).collect()
    }

    pub fn get(&self, name: &str) -> Option<&Vec<String>> {
        self.items.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Vec<String>> {
        self.items.iter_mut().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.items.iter().any(|(k, _)| k == name)
    }

    pub fn position(&self, name: &str) -> Option<usize> {
        self.items.iter().position(|(k, _)| k == name)
    }

    /// Adds to the end (or extends an existing one).
    pub fn add(&mut self, name: &str, paths: impl IntoIterator<Item = String>) {
        match self.get_mut(name) {
            Some(v) => v.extend(paths),
            None => self.items.push((name.to_string(), paths.into_iter().collect())),
        }
    }

    pub fn insert_at(&mut self, idx: usize, name: String, paths: Vec<String>) {
        let idx = idx.min(self.items.len());
        self.items.insert(idx, (name, paths));
    }

    pub fn remove(&mut self, name: &str) -> Option<(usize, Vec<String>)> {
        let i = self.position(name)?;
        Some((i, self.items.remove(i).1))
    }

    pub fn rename(&mut self, old: &str, new: &str) -> bool {
        if new.is_empty() || old == new || self.contains(new) {
            return false;
        }
        match self.items.iter_mut().find(|(k, _)| k == old) {
            Some(e) => {
                e.0 = new.to_string();
                true
            }
            None => false,
        }
    }

    /// A free "new playlist", "new playlist 2", … name.
    pub fn free_name(&self, base: &str) -> String {
        let mut name = base.to_string();
        let mut i = 2;
        while self.contains(&name) {
            name = format!("{base} {i}");
            i += 1;
        }
        name
    }

    /// After files were renamed/moved.
    pub fn remap(&mut self, moved: &std::collections::HashMap<String, String>) {
        for (_, v) in &mut self.items {
            for p in v.iter_mut() {
                if let Some(n) = moved.get(p) {
                    *p = n.clone();
                }
            }
        }
    }
}

/// Play counts: `{path: {"plays": n, "last": unix_time}}`.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub map: Map<String, Value>,
}

impl Stats {
    pub fn load() -> Self {
        Stats { map: load_json(&data_dir().join("stats.json"), Map::new()) }
    }

    pub fn save(&self) {
        if let Err(e) = save_json(&data_dir().join("stats.json"), &self.map) {
            log::error!("saving stats: {e}");
        }
    }

    pub fn plays(&self, path: &str) -> u64 {
        self.map.get(path).and_then(|e| e.get("plays")).and_then(Value::as_u64).unwrap_or(0)
    }

    pub fn count_play(&mut self, path: &str) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let plays = self.plays(path) + 1;
        let e = self.map.entry(path.to_string()).or_insert_with(|| json!({}));
        if let Value::Object(o) = e {
            o.insert("plays".into(), json!(plays));
            o.insert("last".into(), json!(now));
        }
    }

    pub fn remap(&mut self, moved: &std::collections::HashMap<String, String>) {
        for (a, b) in moved {
            if let Some(v) = self.map.shift_remove(a) {
                self.map.insert(b.clone(), v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_names_and_rename() {
        let mut p = Playlists::default();
        p.add("new playlist", ["a".to_string()]);
        assert_eq!(p.free_name("new playlist"), "new playlist 2");
        assert!(p.rename("new playlist", "mine"));
        assert!(!p.rename("mine", "mine"));
        p.add("mine", ["b".to_string()]);
        assert_eq!(p.get("mine").unwrap(), &vec!["a".to_string(), "b".to_string()]);
        let (i, paths) = p.remove("mine").unwrap();
        assert_eq!((i, paths.len()), (0, 2));
    }
}
