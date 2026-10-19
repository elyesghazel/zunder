//! `clip`: clipboard history from cliphist. Enter puts the entry back on the clipboard.
//! Needs `wl-paste --watch cliphist store` running (most Hyprland setups already have it).
//! cliphist keeps no dates, so the server notes when each entry shows up and the history is
//! split into Today, Yesterday, 3 days ago… from that.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone};
use regex::Regex;

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item, shell_quote};

pub struct Clipboard;

const PREFIX: &str = "clip";
const SHOWN: usize = 300;
const GLYPH: &str = "\u{f0ea}";

impl Provider for Clipboard {
    fn title(&self) -> &str {
        "Clipboard"
    }

    fn exclusive(&self, query: &str) -> bool {
        query == PREFIX || query.starts_with("clip ")
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        if !self.exclusive(cx.query) {
            if cx.query.is_empty() {
                return Vec::new();
            }
            return cx
                .score("Clipboard History")
                .map(|score| Item {
                    id: "cmd:clipboard".into(),
                    title: "Clipboard History".into(),
                    subtitle: "everything you copied".into(),
                    hint: "clip ␣".into(),
                    glyph: GLYPH.into(),
                    action: Action::Fill("clip ".into()),
                    score,
                    ..Default::default()
                })
                .into_iter()
                .collect();
        }

        let out = match Command::new("cliphist").arg("list").output() {
            Ok(out) => out,
            Err(_) => {
                return vec![Item {
                    title: "cliphist is not installed".into(),
                    subtitle: "pacman -S cliphist, then run `wl-paste --watch cliphist store`".into(),
                    glyph: "\u{f071}".into(),
                    ..Default::default()
                }];
            }
        };
        let rest = cx.query[PREFIX.len()..].trim();
        let seen = SEEN.lock().unwrap().clone();
        let newest = seen.keys().max().copied().unwrap_or(0);
        let now = Local::now();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| {
                let (id, preview) = line.split_once('\t')?;
                let score = if rest.is_empty() { 0 } else { cx.matcher_score(rest, preview)? };
                Some((score, id, preview, line))
            })
            .take(SHOWN)
            .enumerate()
            .map(|(i, (score, id, preview, line))| {
                let image = preview.starts_with("[[ binary data");
                let at = copied_at(id, &seen, newest, now.timestamp());
                Item {
                    title: if image { image_title(preview) } else { preview.trim().replace('\t', " ") },
                    image: image.then(|| image_path(id, preview)).flatten(),
                    image_cmd: image.then(|| decode_cmd(id, preview, line)).flatten(),
                    text: (!image).then(|| format!("printf '%s' {} | cliphist decode", shell_quote(line))),
                    hint: at.map(|t| time_hint(t, now)).unwrap_or_default(),
                    group: day_group(at, now.date_naive()),
                    glyph: if image { "\u{f03e}".into() } else { GLYPH.into() },
                    subtitle: if image { "image".into() } else { String::new() },
                    // cliphist decode takes the whole list line on stdin
                    action: Action::Run(format!("printf '%s' {} | cliphist decode | wl-copy", shell_quote(line))),
                    score: score * 1000 - i as i64,
                    ..Default::default()
                }
            })
            .collect()
    }
}

/// cliphist entry id → unix time it was first seen; 0 for entries from before zunder ran.
static SEEN: LazyLock<Mutex<HashMap<u64, i64>>> = LazyLock::new(|| {
    let seen = fs::read_to_string(seen_path()).ok().and_then(|s| serde_json::from_str(&s).ok());
    Mutex::new(seen.unwrap_or_default())
});

fn seen_path() -> PathBuf {
    dirs::data_dir().unwrap_or_default().join("zunder/clipboard-times.json")
}

/// Dates new clipboard entries as they're copied, for as long as the server runs.
pub fn watch() {
    // whatever is in the history now predates us (or the server was down): no date
    stamp(0);
    thread::spawn(|| {
        let child = Command::new("wl-paste")
            .args(["--watch", "echo"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let Some(out) = child.ok().and_then(|mut c| c.stdout.take()) else { return };
        for _ in BufReader::new(out).lines() {
            // give the `cliphist store` watcher time to write the entry
            thread::sleep(Duration::from_millis(500));
            stamp(Local::now().timestamp());
        }
    });
}

/// Records `at` for every entry not seen yet and forgets entries cliphist dropped.
fn stamp(at: i64) {
    let Ok(out) = Command::new("cliphist").arg("list").output() else { return };
    let ids: HashSet<u64> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_once('\t')?.0.parse().ok())
        .collect();
    let mut seen = SEEN.lock().unwrap();
    let before = seen.clone();
    seen.retain(|id, _| ids.contains(id));
    for id in ids {
        seen.entry(id).or_insert(at);
    }
    if *seen != before {
        let path = seen_path();
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string(&*seen) {
            let _ = fs::write(path, json);
        }
    }
}

/// When entry `id` was copied. One newer than anything recorded was copied just now and the
/// watcher hasn't caught up yet.
fn copied_at(id: &str, seen: &HashMap<u64, i64>, newest: u64, now: i64) -> Option<i64> {
    let id: u64 = id.parse().ok()?;
    match seen.get(&id) {
        Some(&t) => (t > 0).then_some(t),
        None => (id > newest).then_some(now),
    }
}

fn date_of(t: i64) -> Option<DateTime<Local>> {
    Local.timestamp_opt(t, 0).single()
}

/// Section of an entry copied at `at`: Today, Yesterday, 3 days ago, Last week…
fn day_group(at: Option<i64>, today: NaiveDate) -> String {
    let Some(date) = at.and_then(date_of).map(|d| d.date_naive()) else { return "Older".into() };
    match (today - date).num_days() {
        ..=0 => "Today".into(),
        1 => "Yesterday".into(),
        d @ 2..=6 => format!("{d} days ago"),
        7..=13 => "Last week".into(),
        d @ 14..=29 => format!("{} weeks ago", d / 7),
        30..=59 => "Last month".into(),
        _ => "Older".into(),
    }
}

/// `14:32` today, `Mon 14:32` this week, `12 Oct` before, `12 Oct 2025` in another year.
fn time_hint(at: i64, now: DateTime<Local>) -> String {
    let Some(t) = date_of(at) else { return String::new() };
    let days = (now.date_naive() - t.date_naive()).num_days();
    let fmt = match days {
        ..=0 => "%H:%M",
        1..=6 => "%a %H:%M",
        _ if t.year() == now.year() => "%-d %b",
        _ => "%-d %b %Y",
    };
    t.format(fmt).to_string()
}

static BINARY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"binary data ([\d.]+ \w+) (\w+) (\d+x\d+)").unwrap());

/// `[[ binary data 120 KiB png 800x600 ]]` → `png 800×600 · 120 KiB`
fn image_title(preview: &str) -> String {
    match BINARY.captures(preview) {
        Some(c) => format!("{} {} · {}", &c[2], c[3].replace('x', "×"), &c[1]),
        None => preview.trim().to_owned(),
    }
}

/// Where a clipboard image is decoded to: $XDG_RUNTIME_DIR/zunder-clip/<id>.<ext>
fn image_path(id: &str, preview: &str) -> Option<String> {
    let ext = BINARY.captures(preview).map(|c| c[2].to_owned()).unwrap_or_else(|| "png".into());
    let path: PathBuf = dirs::runtime_dir()?.join("zunder-clip").join(format!("{id}.{ext}"));
    Some(path.to_string_lossy().into_owned())
}

/// Decodes it there; the preview pane runs this the first time the entry is selected.
fn decode_cmd(id: &str, preview: &str, line: &str) -> Option<String> {
    let path = image_path(id, preview)?;
    let tmp = format!("{path}.tmp");
    Some(format!(
        "mkdir -p \"$(dirname {p})\" && printf '%s' {l} | cliphist decode > {t} && mv {t} {p}",
        p = shell_quote(&path),
        l = shell_quote(line),
        t = shell_quote(&tmp)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(y: i32, m: u32, d: u32, h: u32) -> i64 {
        Local.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap().timestamp()
    }

    #[test]
    fn groups_by_day() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 19).unwrap();
        let group = |d| day_group(Some(ts(2026, 10, d, 12)), today);
        assert_eq!(group(19), "Today");
        assert_eq!(group(18), "Yesterday");
        assert_eq!(group(16), "3 days ago");
        assert_eq!(group(10), "Last week");
        assert_eq!(group(1), "2 weeks ago");
        assert_eq!(day_group(Some(ts(2026, 9, 10, 12)), today), "Last month");
        assert_eq!(day_group(Some(ts(2026, 6, 1, 12)), today), "Older");
        assert_eq!(day_group(None, today), "Older");
    }

    #[test]
    fn dates_entries() {
        let seen = HashMap::from([(5, 100), (6, 0)]);
        assert_eq!(copied_at("5", &seen, 6, 999), Some(100));
        assert_eq!(copied_at("6", &seen, 6, 999), None);
        assert_eq!(copied_at("7", &seen, 6, 999), Some(999));
        assert_eq!(copied_at("4", &seen, 6, 999), None);
    }

    #[test]
    fn hints() {
        let now = Local.with_ymd_and_hms(2026, 10, 19, 15, 0, 0).unwrap();
        assert_eq!(time_hint(ts(2026, 10, 19, 9), now), "09:00");
        assert_eq!(time_hint(ts(2026, 10, 18, 9), now), "Sun 09:00");
        assert_eq!(time_hint(ts(2026, 10, 1, 9), now), "1 Oct");
        assert_eq!(time_hint(ts(2025, 10, 1, 9), now), "1 Oct 2025");
    }
}
