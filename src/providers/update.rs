//! Start screen notice when a newer zunder is out. Checks GitHub at most once a day, in the
//! background; turn it off with `update_check = false`. Enter updates (in a terminal on Linux,
//! through `update install` on Windows). `update` checks right now, whatever the setting says.
//! The release notes come along with the check and show in the preview pane, and after an
//! update the start screen shows what's new for a day (`changelog` shows it any time).

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::engine::{Ctx, Job, Provider};
use crate::item::{Action, Item, Text};

pub struct Update;

const PREFIX: &str = "update";
/// Windows only: download the installer and start it (the notice fills this in).
pub const INSTALL: &str = "update install";
const CHANGELOG: &str = "changelog";
const API: &str = "https://api.github.com/repos/elyesghazel/zunder/releases";
#[cfg(windows)]
const RELEASES: &str = "https://github.com/elyesghazel/zunder/releases/latest";
/// How long the start screen shows what's new after an update.
const WHATS_NEW_FOR: Duration = Duration::from_secs(24 * 3600);

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Latest {
    tag_name: String,
    /// The release notes, markdown.
    body: String,
}

fn cache() -> PathBuf {
    dirs::cache_dir().unwrap_or_default().join("zunder/latest.json")
}

/// The notes of the version that's running, kept after an update: latest.json moves on.
fn notes_cache() -> PathBuf {
    dirs::cache_dir().unwrap_or_default().join("zunder/notes.json")
}

/// The last version the start screen announced, and since when (unix seconds).
fn seen_file() -> PathBuf {
    dirs::cache_dir().unwrap_or_default().join("zunder/seen-version")
}

fn read(path: &PathBuf) -> Option<Latest> {
    fs::read_to_string(path).ok().and_then(|s| serde_json::from_str::<Latest>(&s).ok()).filter(|l| !l.tag_name.is_empty())
}

/// One release from GitHub: `latest`, or `tags/v0.3.4`.
fn fetch(which: &str) -> Option<Latest> {
    let url = format!("{API}/{which}");
    let out = crate::platform::no_window(std::process::Command::new("curl").args(["-fsSL", "-m", "8", &url])).output().ok()?;
    serde_json::from_slice::<Latest>(&out.stdout).ok().filter(|l| !l.tag_name.is_empty())
}

/// The cached latest release, and whether it's time to ask GitHub again.
fn cached() -> (Option<Latest>, bool) {
    let path = cache();
    let latest = read(&path);
    let stale = fs::metadata(&path)
        .and_then(|m| m.modified())
        .map(|t| t.elapsed().unwrap_or(Duration::MAX) > Duration::from_secs(24 * 3600))
        .unwrap_or(true);
    (latest, stale)
}

/// Ask GitHub for the latest release and cache it. `None` when GitHub can't be reached.
fn refresh() -> Option<Latest> {
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
    let latest = fetch("latest")?;
    let _ = fs::write(&path, serde_json::to_string(&latest).unwrap_or_default());
    Some(latest)
}

/// The release notes of the running version: from the update check that found it, or asked for.
/// `None` while that's still unknown (a background fetch is then on its way).
fn current_notes() -> Option<Latest> {
    let current = env!("CARGO_PKG_VERSION");
    let is_current = |l: &Latest| !newer(&l.tag_name, current) && !newer(current, &l.tag_name);
    if let Some(notes) = read(&notes_cache()).filter(is_current) {
        return Some(notes);
    }
    if let Some(latest) = read(&cache()).filter(is_current) {
        let _ = fs::write(notes_cache(), serde_json::to_string(&latest).unwrap_or_default());
        return Some(latest);
    }
    use std::sync::atomic::{AtomicBool, Ordering};
    static FETCHING: AtomicBool = AtomicBool::new(false);
    if !FETCHING.swap(true, Ordering::SeqCst) {
        std::thread::spawn(move || {
            if let Some(notes) = fetch(&format!("tags/v{current}")) {
                let _ = fs::write(notes_cache(), serde_json::to_string(&notes).unwrap_or_default());
            }
        });
    }
    None
}

/// Release notes for the preview pane: markdown down to plain lines, without the install
/// instructions and the GitHub links every release ends with.
fn changelog(body: &str) -> String {
    let mut out = Vec::new();
    for line in body.lines() {
        let line = line.trim_end();
        if line.starts_with("Arch: ") || line.starts_with("**Full Changelog**") || line.starts_with("## What's Changed") {
            continue;
        }
        let line = line.replace("**", "").replace('`', "");
        let line = if let Some(heading) = line.trim_start_matches('#').strip_prefix(' ').filter(|_| line.starts_with('#')) {
            heading.to_owned()
        } else if let Some(entry) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            format!("  · {entry}")
        } else {
            line
        };
        if line.is_empty() && out.last().is_none_or(|l: &String| l.is_empty()) {
            continue;
        }
        out.push(line);
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    if out.is_empty() { "No notes for this release.\nEnter shows it on GitHub.".into() } else { out.join("\n") }
}

fn release_page(tag: &str) -> String {
    format!("https://github.com/elyesghazel/zunder/releases/tag/{tag}")
}

/// "zunder 0.3.0 is out", Enter installs it; what changed in the preview pane.
fn notice(latest: &Latest) -> Item {
    Item {
        title: format!("zunder {} is out", latest.tag_name.trim_start_matches('v')),
        subtitle: format!("you have {} · {}", env!("CARGO_PKG_VERSION"), crate::platform::update_hint()),
        hint: "update".into(),
        glyph: "\u{f01b}".into(),
        id: "cmd:update".into(),
        text: Some(Text::Literal(changelog(&latest.body))),
        action: crate::platform::update_action(),
        score: 1_000_000,
        ..Default::default()
    }
}

/// "What's new in zunder 0.3.5", the running version's notes in the preview pane.
fn whats_new(score: i64) -> Item {
    let current = env!("CARGO_PKG_VERSION");
    let notes = current_notes();
    Item {
        id: "cmd:whats-new".into(),
        title: format!("What's new in zunder {current}"),
        subtitle: "Enter opens the release on GitHub".into(),
        hint: "changelog".into(),
        glyph: "\u{f15c}".into(),
        text: Some(Text::Literal(notes.as_ref().map_or_else(|| "Getting the notes from GitHub…".into(), |n| changelog(&n.body)))),
        action: Action::Open(release_page(&format!("v{current}"))),
        score,
        ..Default::default()
    }
}

/// Just updated? Then the start screen shows what's new, for a day. A fresh install doesn't
/// count: that's the welcome screen's turn.
fn just_updated() -> bool {
    let current = env!("CARGO_PKG_VERSION");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let seen = fs::read_to_string(seen_file()).unwrap_or_default();
    if let Some((version, since)) = seen.split_once(' ')
        && version == current
    {
        return now.saturating_sub(since.trim().parse().unwrap_or(0)) < WHATS_NEW_FOR.as_secs();
    }
    // another version, or no file: the first start, or a zunder from before this file existed
    let fresh = !crate::config::path().exists();
    if let Some(dir) = seen_file().parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(seen_file(), format!("{current} {}", if fresh { 0 } else { now }));
    !fresh
}

/// The answer to `update`: the new version, or that this one is the newest.
fn check_now() -> Vec<Item> {
    let current = env!("CARGO_PKG_VERSION");
    let item = match refresh() {
        Some(latest) if newer(&latest.tag_name, current) => notice(&latest),
        Some(_) => Item {
            title: format!("zunder {current} is the latest version"),
            subtitle: "checked GitHub just now · what's new in it on the right".into(),
            glyph: "\u{f00c}".into(),
            text: whats_new(0).text,
            action: Action::Open(release_page(&format!("v{current}"))),
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

/// The answer to `update install`: the installer is running, or why it isn't. A failed download
/// opens the release page, so the update is one click away anyway.
#[cfg(windows)]
fn install() -> Vec<Item> {
    use std::sync::atomic::{AtomicBool, Ordering};
    // the query re-runs this job on every refresh; one download is enough
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Vec::new();
    }
    let result = crate::platform::install_update();
    RUNNING.store(false, Ordering::SeqCst);
    let item = match result {
        Ok(()) => Item {
            title: "Installing the new zunder…".into(),
            subtitle: "the installer closes zunder and starts it again".into(),
            glyph: "\u{f01b}".into(),
            ..Default::default()
        },
        Err(e) => {
            crate::platform::open(RELEASES);
            Item {
                title: "Couldn't download the update".into(),
                subtitle: format!("{e} · opened the release page instead"),
                glyph: "\u{f071}".into(),
                action: Action::Open(RELEASES.into()),
                ..Default::default()
            }
        }
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
        let query = query.trim_end();
        query == PREFIX || query == CHANGELOG || (cfg!(windows) && query == INSTALL)
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        if cx.switch {
            return Vec::new();
        }
        if cfg!(windows) && cx.query.trim_end() == INSTALL {
            // replaced by `install`'s answer once the download is done
            return vec![Item {
                id: "pending:update".into(),
                title: "Downloading the new zunder…".into(),
                subtitle: "the installer starts when it's done".into(),
                glyph: "\u{f019}".into(),
                ..Default::default()
            }];
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
        if cx.query.trim_end() == CHANGELOG {
            return vec![whats_new(1)];
        }
        if !cx.query.is_empty() {
            let check = cx.score("Check for Updates").map(|score| Item {
                id: "cmd:check-update".into(),
                title: "Check for Updates".into(),
                subtitle: format!("you have zunder {}", env!("CARGO_PKG_VERSION")),
                hint: "update".into(),
                glyph: "\u{f021}".into(),
                action: Action::Fill(PREFIX.into()),
                score,
                ..Default::default()
            });
            let new = cx.score("What's New").or_else(|| cx.score("Changelog")).map(whats_new);
            return check.into_iter().chain(new).collect();
        }
        let mut items = Vec::new();
        if crate::config::get().update_check {
            let (latest, stale) = cached();
            if stale {
                std::thread::spawn(refresh);
            }
            items.extend(latest.filter(|l| newer(&l.tag_name, env!("CARGO_PKG_VERSION"))).map(|l| notice(&l)));
        }
        if just_updated() {
            items.push(whats_new(999_999));
        }
        items
    }

    fn deferred(&self, cx: &Ctx) -> Option<(String, Job)> {
        #[cfg(windows)]
        if cx.query.trim_end() == INSTALL {
            return Some(("Update".to_owned(), Box::new(install)));
        }
        (cx.query.trim_end() == PREFIX).then(|| ("Update".to_owned(), Box::new(check_now) as Job))
    }
}

#[cfg(test)]
mod tests {
    use super::{changelog, newer};

    #[test]
    fn notes_to_plain_text() {
        let body = "## New\n\n- open clipboard history with `win+v` [windows]\n\n## Fixed\n\n- drop the dim [windows]\n\nArch: `sudo pacman -U` the .pkg.tar.zst\n\n**Full Changelog**: https://github.com/x/y/compare/a...b";
        assert_eq!(changelog(body), "New\n\n  · open clipboard history with win+v [windows]\n\nFixed\n\n  · drop the dim [windows]");
        assert!(changelog("Arch: install it\n\n**Full Changelog**: x").starts_with("No notes"));
    }

    #[test]
    fn compares_versions() {
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(newer("v0.10.0", "0.9.3"));
        assert!(!newer("v0.1.0", "0.1.0"));
        assert!(!newer("v0.1.0", "0.2.0"));
    }
}
