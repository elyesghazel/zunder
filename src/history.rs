//! Launch counts, used to float things you use often to the top.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
pub struct History {
    counts: HashMap<String, u32>,
    #[serde(skip)]
    path: PathBuf,
}

impl History {
    pub fn load() -> Self {
        let path = dirs::data_dir().unwrap_or_default().join("zunder/history.json");
        let mut h: History = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        h.path = path;
        h
    }

    pub fn count(&self, id: &str) -> u32 {
        self.counts.get(id).copied().unwrap_or(0)
    }

    pub fn boost(&self, id: &str) -> i64 {
        i64::from(self.count(id).min(25)) * 6
    }

    pub fn bump(&mut self, id: &str) {
        if id.is_empty() {
            return;
        }
        *self.counts.entry(id.to_owned()).or_default() += 1;
        self.save();
    }

    /// At least `n` launches, for imports (running it twice doesn't double anything).
    pub fn raise_to(&mut self, id: &str, n: u32) {
        let count = self.counts.entry(id.to_owned()).or_default();
        *count = (*count).max(n);
    }

    pub fn save(&self) {
        if let Some(dir) = self.path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string(self) {
            let _ = fs::write(&self.path, json);
        }
    }
}
