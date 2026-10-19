//! Things zunder remembers: favorites, keybinds and the launch log.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::item::Item;

fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("zunder")
}

fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_default().join("zunder")
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &PathBuf) -> T {
    fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn write_json<T: Serialize>(path: &PathBuf, value: &T) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    match serde_json::to_string_pretty(value) {
        Ok(json) => {
            let _ = fs::write(path, json);
        }
        Err(e) => eprintln!("zunder: can't write {}: {e}", path.display()),
    }
}

/// A saved copy of an item, stripped of per-search state.
fn snapshot(item: &Item) -> Item {
    Item { score: 0, preview: None, group: String::new(), ..item.clone() }
}

// ── favorites ───────────────────────────────────────────

/// Pinned to the start screen with Ctrl+F. ~/.config/zunder/favorites.json
pub struct Favorites {
    pub items: Vec<Item>,
}

impl Favorites {
    fn path() -> PathBuf {
        config_dir().join("favorites.json")
    }

    pub fn load() -> Self {
        Favorites { items: read_json(&Self::path()) }
    }

    pub fn contains(&self, id: &str) -> bool {
        !id.is_empty() && self.items.iter().any(|i| i.id == id)
    }

    /// Add or remove; returns whether it is a favorite now.
    pub fn toggle(&mut self, item: &Item) -> bool {
        let added = if self.contains(&item.id) {
            self.items.retain(|i| i.id != item.id);
            false
        } else {
            self.items.push(snapshot(item));
            true
        };
        write_json(&Self::path(), &self.items);
        added
    }
}

// ── launch log ──────────────────────────────────────────

#[derive(Serialize, Deserialize)]
pub struct Entry {
    /// Unix seconds.
    pub at: i64,
    pub item: Item,
}

/// Every launch, appended to ~/.local/share/zunder/launches.jsonl. Never trimmed.
pub fn log_launch(item: &Item) {
    let path = data_dir().join("launches.jsonl");
    let _ = fs::create_dir_all(data_dir());
    let entry = Entry { at: chrono::Local::now().timestamp(), item: snapshot(item) };
    let Ok(line) = serde_json::to_string(&entry) else { return };
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

/// The whole launch log, newest first.
pub fn launches() -> Vec<Entry> {
    let text = fs::read_to_string(data_dir().join("launches.jsonl")).unwrap_or_default();
    let mut entries: Vec<Entry> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    entries.reverse();
    entries
}

