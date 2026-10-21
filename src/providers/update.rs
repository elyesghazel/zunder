//! Start screen notice when a newer zunder is out. Checks GitHub at most once a day, in the
//! background; turn it off with `update_check = false`. Enter updates in a terminal.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item};

pub struct Update;

/// Enter on the notice runs the installer again in a terminal: it updates, then restarts zunder.
const UPDATE: &str = "curl -fsSL https://raw.githubusercontent.com/elyesghazel/zunder/main/get.sh | sh -s -- --no-setup";
const API: &str = "https://api.github.com/repos/elyesghazel/zunder/releases/latest";

#[derive(Serialize, Deserialize, Default)]
struct Latest {
    tag_name: String,
}

fn cache() -> PathBuf {
    dirs::cache_dir().unwrap_or_default().join("zunder/latest.json")
}

/// The cached latest tag, and whether it's time to ask GitHub again.
fn cached() -> (Option<String>, bool) {
    let path = cache();
    let tag = fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<Latest>(&s).ok()).map(|l| l.tag_name);
    let stale = fs::metadata(&path)
        .and_then(|m| m.modified())
        .map(|t| t.elapsed().unwrap_or(Duration::MAX) > Duration::from_secs(24 * 3600))
        .unwrap_or(true);
    (tag, stale)
}

fn refresh() {
    let path = cache();
    // touch first, so a failing check (offline) isn't retried on every open
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if !path.exists() {
        let _ = fs::write(&path, "{}");
    } else if let Ok(f) = fs::File::options().append(true).open(&path) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
    let out = std::process::Command::new("curl").args(["-fsSL", "-m", "8", API]).output();
    if let Ok(out) = out
        && let Ok(latest) = serde_json::from_slice::<Latest>(&out.stdout)
        && !latest.tag_name.is_empty()
    {
        let _ = fs::write(&path, serde_json::to_string(&latest).unwrap_or_default());
    }
}

/// "0.10.1" → [0, 10, 1], for comparing versions as numbers.
fn version(v: &str) -> Vec<u64> {
    v.trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

pub fn newer(latest: &str, current: &str) -> bool {
    version(latest) > version(current)
}

impl Provider for Update {
    fn title(&self) -> &str {
        "Update"
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        if !cx.query.is_empty() || cx.switch || !crate::config::get().update_check {
            return Vec::new();
        }
        let (tag, stale) = cached();
        if stale {
            std::thread::spawn(refresh);
        }
        let current = env!("CARGO_PKG_VERSION");
        tag.filter(|t| newer(t, current))
            .map(|t| Item {
                title: format!("zunder {} is out", t.trim_start_matches('v')),
                subtitle: format!("you have {current} · Enter updates now"),
                hint: "update".into(),
                glyph: "\u{f01b}".into(),
                id: "cmd:update".into(),
                action: Action::Term(UPDATE.into()),
                score: 1_000_000,
                ..Default::default()
            })
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn compares_versions() {
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(newer("v0.10.0", "0.9.3"));
        assert!(!newer("v0.1.0", "0.1.0"));
        assert!(!newer("v0.1.0", "0.2.0"));
    }
}
