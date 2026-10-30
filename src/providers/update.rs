//! Start screen notice when a newer zunder is out. Checks GitHub at most once a day, in the
//! background; turn it off with `update_check = false`. Enter updates (in a terminal on Linux,
//! through `update install` on Windows). `update` checks right now, whatever the setting says.
//! What changed lives in the changelog window (`zunder changelog`, or `changelog` here); after
//! an update the start screen points to it for a day.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::engine::{Ctx, Job, Provider};
use crate::item::{Action, Item, MenuAction};

pub struct Update;

const PREFIX: &str = "update";
/// Windows only: download the installer and start it (the notice fills this in).
pub const INSTALL: &str = "update install";
const CHANGELOG: &str = "changelog";
const API: &str = "https://api.github.com/repos/elyesghazel/zunder/releases";
#[cfg(windows)]
const RELEASES: &str = "https://github.com/elyesghazel/zunder/releases/latest";
/// How long the start screen points to what's new after an update.
const WHATS_NEW_FOR: Duration = Duration::from_secs(24 * 3600);

#[derive(Serialize, Deserialize, Default)]
struct Latest {
    tag_name: String,
}

/// One GitHub release, for the changelog window.
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Release {
    pub tag_name: String,
    /// The release notes, markdown.
    pub body: String,
    /// `2026-10-30T21:12:44Z`
    pub published_at: String,
}

/// One "## New" block of a release's notes: entries with their scope (`[windows]`), if any.
#[derive(Debug, PartialEq)]
pub struct Section {
    pub heading: String,
    pub entries: Vec<(String, Option<String>)>,
}

fn cache_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_default().join("zunder")
}

fn cache() -> PathBuf {
    cache_dir().join("latest.json")
}

fn releases_cache() -> PathBuf {
    cache_dir().join("releases.json")
}

/// The last version the start screen announced, and since when (unix seconds).
fn seen_file() -> PathBuf {
    cache_dir().join("seen-version")
}

fn curl(url: &str) -> Option<Vec<u8>> {
    let out = crate::platform::no_window(std::process::Command::new("curl").args(["-fsSL", "-m", "8", url])).output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// The cached latest tag, and whether it's time to ask GitHub again.
fn cached() -> (Option<String>, bool) {
    let path = cache();
    let tag = fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<Latest>(&s).ok()).map(|l| l.tag_name);
    let stale = fs::metadata(&path)
        .and_then(|m| m.modified())
        .map(|t| t.elapsed().unwrap_or(Duration::MAX) > Duration::from_secs(24 * 3600))
        .unwrap_or(true);
    (tag.filter(|t| !t.is_empty()), stale)
}

/// Ask GitHub for the latest tag and cache it. `None` when GitHub can't be reached.
fn refresh() -> Option<String> {
    let path = cache();
    // touch first, so a failing check (offline) isn't retried on every open
    let _ = fs::create_dir_all(cache_dir());
    if !path.exists() {
        let _ = fs::write(&path, "{}");
    } else if let Ok(f) = fs::File::options().append(true).open(&path) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
    let latest = serde_json::from_slice::<Latest>(&curl(&format!("{API}/latest"))?).ok().filter(|l| !l.tag_name.is_empty())?;
    let _ = fs::write(&path, serde_json::to_string(&latest).unwrap_or_default());
    Some(latest.tag_name)
}

/// The releases from the last time the changelog window asked, newest first.
pub fn cached_releases() -> Vec<Release> {
    fs::read_to_string(releases_cache()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Every release from GitHub, newest first, cached for next time. `None` when offline.
pub fn fetch_releases() -> Option<Vec<Release>> {
    let releases: Vec<Release> = serde_json::from_slice(&curl(&format!("{API}?per_page=100"))?).ok()?;
    let _ = fs::create_dir_all(cache_dir());
    let _ = fs::write(releases_cache(), serde_json::to_string(&releases).unwrap_or_default());
    Some(releases)
}

/// A release body as sections: the install instructions and GitHub's own links dropped, and
/// GitHub's generated "* fix x by @someone in https://…" lines cut to "fix x".
pub fn sections(body: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for line in body.lines().map(str::trim) {
        if let Some(heading) = line.strip_prefix("## ").or_else(|| line.strip_prefix("### ")) {
            if heading != "What's Changed" && heading != "New Contributors" {
                out.push(Section { heading: heading.to_owned(), entries: Vec::new() });
            }
            continue;
        }
        let Some(entry) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) else { continue };
        let entry = entry.split(" by @").next().unwrap_or(entry).replace("**", "").replace('`', "");
        let (text, scope) = match entry.strip_suffix(']').and_then(|e| e.rsplit_once(" [")) {
            Some((text, scope)) => (text.to_owned(), Some(scope.to_owned())),
            None => (entry, None),
        };
        if out.is_empty() {
            out.push(Section { heading: "Changes".into(), entries: Vec::new() });
        }
        if let Some(last) = out.last_mut() {
            last.entries.push((text, scope));
        }
    }
    out.retain(|s| !s.entries.is_empty());
    out
}

pub fn release_page(tag: &str) -> String {
    format!("https://github.com/elyesghazel/zunder/releases/tag/{tag}")
}

/// Ctrl+K on the update items: what changed.
fn changelog_action() -> MenuAction {
    MenuAction { title: "What's new".into(), glyph: "\u{f15c}".into(), action: Action::Run("zunder changelog".into()) }
}

/// "zunder 0.3.0 is out", Enter installs it.
fn notice(tag: &str) -> Item {
    Item {
        title: format!("zunder {} is out", tag.trim_start_matches('v')),
        subtitle: format!("you have {} · {} · Ctrl+K what's new", env!("CARGO_PKG_VERSION"), crate::platform::update_hint()),
        hint: "update".into(),
        glyph: "\u{f01b}".into(),
        id: "cmd:update".into(),
        action: crate::platform::update_action(),
        actions: vec![changelog_action()],
        score: 1_000_000,
        ..Default::default()
    }
}

/// Opens the changelog window.
fn whats_new(score: i64) -> Item {
    Item {
        id: "cmd:whats-new".into(),
        title: format!("What's new in zunder {}", env!("CARGO_PKG_VERSION")),
        subtitle: "Enter opens the changelog".into(),
        hint: "changelog".into(),
        glyph: "\u{f15c}".into(),
        action: Action::Run("zunder changelog".into()),
        score,
        ..Default::default()
    }
}

/// Just updated? Then the start screen points to what's new, for a day. A fresh install doesn't
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
    let _ = fs::create_dir_all(cache_dir());
    let _ = fs::write(seen_file(), format!("{current} {}", if fresh { 0 } else { now }));
    !fresh
}

/// The answer to `update`: the new version, or that this one is the newest.
fn check_now() -> Vec<Item> {
    let current = env!("CARGO_PKG_VERSION");
    let item = match refresh() {
        Some(tag) if newer(&tag, current) => notice(&tag),
        Some(_) => Item {
            title: format!("zunder {current} is the latest version"),
            subtitle: "checked GitHub just now · Enter shows what's new".into(),
            glyph: "\u{f00c}".into(),
            action: Action::Run("zunder changelog".into()),
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
            let (tag, stale) = cached();
            if stale {
                std::thread::spawn(refresh);
            }
            items.extend(tag.filter(|t| newer(t, env!("CARGO_PKG_VERSION"))).map(|t| notice(&t)));
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
    use super::{newer, sections};

    #[test]
    fn notes_to_sections() {
        let body = "## New\n\n- open clipboard history with `win+v` [windows]\n- a theme picker\n\n## Fixed\n\n- drop the dim [windows]\n\nArch: `sudo pacman -U` the .pkg.tar.zst\n\n**Full Changelog**: https://github.com/x/y/compare/a...b";
        let s = sections(body);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].heading, "New");
        assert_eq!(s[0].entries, vec![("open clipboard history with win+v".into(), Some("windows".into())), ("a theme picker".into(), None)]);
        assert_eq!(s[1].entries.len(), 1);
        assert!(sections("Arch: install it\n\n**Full Changelog**: x").is_empty());
        let generated = "## What's Changed\n* fix the thing by @someone in https://github.com/x/y/pull/1";
        assert_eq!(sections(generated)[0].entries[0].0, "fix the thing");
    }

    #[test]
    fn compares_versions() {
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(newer("v0.10.0", "0.9.3"));
        assert!(!newer("v0.1.0", "0.1.0"));
        assert!(!newer("v0.1.0", "0.2.0"));
    }
}
