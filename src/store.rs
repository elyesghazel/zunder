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

// ── keybinds ────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub struct Bind {
    /// Hyprland syntax, e.g. `SUPER + SHIFT + F`.
    pub combo: String,
    pub item: Item,
}

/// Global shortcuts set with Ctrl+B. Kept in ~/.config/zunder/binds.json; `platform::install_binds`
/// turns them into real shortcuts that run `zunder run <key>`.
pub struct Binds {
    pub list: Vec<Bind>,
}

/// Stable short key for an item id (FNV-1a), safe to put on a command line.
pub fn bind_key(id: &str) -> String {
    let hash = id.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3));
    format!("{hash:016x}")
}

impl Binds {
    fn path() -> PathBuf {
        config_dir().join("binds.json")
    }

    pub fn load() -> Self {
        Binds { list: read_json(&Self::path()) }
    }

    pub fn combo_for(&self, id: &str) -> Option<&str> {
        self.list.iter().find(|b| !id.is_empty() && b.item.id == id).map(|b| b.combo.as_str())
    }

    pub fn find(&self, key: &str) -> Option<&Item> {
        self.list.iter().find(|b| bind_key(&b.item.id) == key).map(|b| &b.item)
    }

    /// Bind `combo` to `item`, replacing whatever either of them had before.
    pub fn set(&mut self, combo: &str, item: &Item) -> Result<String, String> {
        self.list.retain(|b| b.combo != combo && b.item.id != item.id);
        self.list.push(Bind { combo: combo.to_owned(), item: snapshot(item) });
        self.save()
    }

    pub fn remove(&mut self, id: &str) -> Result<String, String> {
        self.list.retain(|b| b.item.id != id);
        self.save()
    }

    fn save(&self) -> Result<String, String> {
        write_json(&Self::path(), &self.list);
        crate::platform::install_binds(&self.list)
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

#[cfg(test)]
mod tests {
    use super::bind_key;

    #[test]
    fn bind_keys_are_stable() {
        assert_eq!(bind_key("app:firefox.desktop"), bind_key("app:firefox.desktop"));
        assert_ne!(bind_key("app:a"), bind_key("app:b"));
        assert_eq!(bind_key("").len(), 16);
    }
}
