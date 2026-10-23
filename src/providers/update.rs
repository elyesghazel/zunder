//! Start screen notice when a newer zunder is out. Checks GitHub at most once a day, in the
//! background; turn it off with `update_check = false`. Enter updates in a terminal.
//! `update` checks right now, whatever the setting says.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::engine::{Ctx, Job, Provider};
use crate::item::{Action, Item};

pub struct Update;

const PREFIX: &str = "update";
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

/// Ask GitHub for the latest tag and cache it. `None` when GitHub can't be reached.
fn refresh() -> Option<String> {
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
        return Some(latest.tag_name);
    }
    None
}

/// "zunder 0.3.0 is out", Enter installs it.
fn notice(tag: &str) -> Item {
    Item {
        title: format!("zunder {} is out", tag.trim_start_matches('v')),
        subtitle: format!("you have {} · {}", env!("CARGO_PKG_VERSION"), crate::platform::update_hint()),
        hint: "update".into(),
        glyph: "\u{f01b}".into(),
        id: "cmd:update".into(),
        action: crate::platform::update_action(),
        score: 1_000_000,
        ..Default::default()
    }
}

/// The answer to `update`: the new version, or that this one is the newest.
fn check_now() -> Vec<Item> {
    let current = env!("CARGO_PKG_VERSION");
    let item = match refresh() {
        Some(tag) if newer(&tag, current) => notice(&tag),
        Some(_) => Item {
            title: format!("zunder {current} is the latest version"),
            subtitle: "checked GitHub just now".into(),
            glyph: "\u{f00c}".into(),
            ..Default::default()
        },
        None => Item {
            title: "Couldn't reach GitHub".into(),
            subtitle: "no connection, or GitHub is rate-limiting; try again in a minute".into(),
            glyph: "\u{f071}".into(),
            ..Default::default()
        },
    };
    vec![item]
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

    fn exclusive(&self, query: &str) -> bool {
        query.trim_end() == PREFIX
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        if cx.switch {
            return Vec::new();
        }
        if self.exclusive(cx.query) {
            // replaced by `check_now`'s answer as soon as GitHub replies
            return vec![Item {
                id: "pending:update".into(),
                title: "Checking GitHub for a newer zunder…".into(),
                subtitle: format!("you have {}", env!("CARGO_PKG_VERSION")),
                glyph: "\u{f021}".into(),
                ..Default::default()
            }];
        }
        if !cx.query.is_empty() {
            return cx
                .score("Check for Updates")
                .map(|score| Item {
                    id: "cmd:check-update".into(),
                    title: "Check for Updates".into(),
                    subtitle: format!("you have zunder {}", env!("CARGO_PKG_VERSION")),
                    hint: "update".into(),
                    glyph: "\u{f021}".into(),
                    action: Action::Fill(PREFIX.into()),
                    score,
                    ..Default::default()
                })
                .into_iter()
                .collect();
        }
        if !crate::config::get().update_check {
            return Vec::new();
        }
        let (tag, stale) = cached();
        if stale {
            std::thread::spawn(refresh);
        }
        tag.filter(|t| newer(t, env!("CARGO_PKG_VERSION"))).map(|t| notice(&t)).into_iter().collect()
    }

    fn deferred(&self, cx: &Ctx) -> Option<(String, Job)> {
        self.exclusive(cx.query).then(|| ("Update".to_owned(), Box::new(check_now) as Job))
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
