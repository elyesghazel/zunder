//! `clip`: clipboard history. Enter puts the entry back on the clipboard.
//! On Linux it comes from cliphist, which needs `wl-paste --watch cliphist store` running (most
//! Hyprland setups already have it). On Windows and macOS zunder keeps its own (`crate::clips`).
//! The history is split into Today, Yesterday, 3 days ago… by when each entry was copied.

use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone};

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item};

#[cfg(target_os = "linux")]
use cliphist as source;
#[cfg(not(target_os = "linux"))]
use own as source;

pub struct Clipboard;

const PREFIX: &str = "clip";
const SHOWN: usize = 300;
const GLYPH: &str = "\u{f0ea}";
const IMAGE_GLYPH: &str = "\u{f03e}";

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
        source::history(cx, cx.query[PREFIX.len()..].trim())
    }
}

/// Dates new clipboard entries as they're copied, for as long as the server runs. cliphist keeps
/// no dates; zunder's own store does.
pub fn watch() {
    #[cfg(target_os = "linux")]
    cliphist::watch();
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

/// Id prefix of pinned clips in favorites.json.
pub const PINNED: &str = "clip:";

/// A favorite holding the content of a history entry (its `pin`), so it outlives the history.
/// Text is copied straight from favorites.json, images from ~/.local/share/zunder/clips.
pub fn pinned(pin: &str, entry: &Item) -> Option<Item> {
    let content = source::content(pin).filter(|c| !c.is_empty())?;
    let id = format!("{PINNED}{:016x}", crate::store::fnv1a(&content));
    let item = Item { id, glyph: entry.glyph.clone(), hint: "pinned".into(), ..Default::default() };
    if entry.image.is_some() {
        let ext = entry.image.as_deref().and_then(|p| p.rsplit_once('.')).map_or("png", |(_, e)| e);
        let path = crate::store::data_dir().join("clips").join(format!("{}.{ext}", &item.id[PINNED.len()..]));
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::write(&path, &content).ok()?;
        let path = path.to_string_lossy().into_owned();
        return Some(Item {
            title: entry.title.clone(),
            subtitle: "image".into(),
            icon: path.clone(),
            action: Action::CopyImage(path),
            ..item
        });
    }
    let text = String::from_utf8_lossy(&content).into_owned();
    let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    let mut title: String = first.chars().take(80).collect();
    if title.len() < first.len() || text.trim() != first {
        title.push('…');
    }
    Some(Item { title, action: Action::Copy(text), ..item })
}

#[cfg(target_os = "linux")]
mod cliphist {
    use std::collections::{HashMap, HashSet};
    use std::io::{BufRead, BufReader};
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::{LazyLock, Mutex};
    use std::thread;
    use std::time::Duration;

    use chrono::Local;
    use regex::Regex;

    use super::{GLYPH, IMAGE_GLYPH, SHOWN, day_group, time_hint};
    use crate::engine::Ctx;
    use crate::item::{Action, Item, Text, shell_quote};

    pub fn history(cx: &Ctx, rest: &str) -> Vec<Item> {
        let out = match Command::new("cliphist").arg("list").output() {
            Ok(out) => out,
            Err(_) => {
                return vec![crate::providers::missing_tool(
                    "cliphist",
                    "keeps the clipboard history",
                    "; then `zunder setup` starts it with Hyprland",
                )];
            }
        };
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
                    text: (!image).then(|| Text::Command(format!("printf '%s' {} | cliphist decode", shell_quote(line)))),
                    hint: at.map(|t| time_hint(t, now)).unwrap_or_default(),
                    group: day_group(at, now.date_naive()),
                    glyph: if image { IMAGE_GLYPH.into() } else { GLYPH.into() },
                    subtitle: if image { "image".into() } else { String::new() },
                    pin: Some(line.to_owned()),
                    // cliphist decode takes the whole list line on stdin
                    action: Action::Run(format!("printf '%s' {} | cliphist decode | wl-copy", shell_quote(line))),
                    score: score * 1000 - i as i64,
                    ..Default::default()
                }
            })
            .collect()
    }

    /// cliphist entry id → unix time it was first seen; 0 for entries from before zunder ran.
    static SEEN: LazyLock<Mutex<HashMap<u64, i64>>> =
        LazyLock::new(|| Mutex::new(crate::store::read_json(&seen_path())));

    fn seen_path() -> PathBuf {
        crate::store::data_dir().join("clipboard-times.json")
    }

    pub fn watch() {
        thread::spawn(|| {
            // whatever is in the history now predates us (or the server was down): no date
            stamp(0);
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
            crate::store::write_json(&seen_path(), &*seen);
        }
    }

    /// When entry `id` was copied. One newer than anything recorded was copied just now and the
    /// watcher hasn't caught up yet; before the first look at the history nothing is dated.
    fn copied_at(id: &str, seen: &HashMap<u64, i64>, newest: u64, now: i64) -> Option<i64> {
        let id: u64 = id.parse().ok()?;
        match seen.get(&id) {
            Some(&t) => (t > 0).then_some(t),
            None => (!seen.is_empty() && id > newest).then_some(now),
        }
    }

    /// The decoded content of a cliphist list line.
    pub fn content(line: &str) -> Option<Vec<u8>> {
        Command::new("sh")
            .args(["-c", &format!("printf '%s' {} | cliphist decode", shell_quote(line))])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| o.stdout)
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

        #[test]
        fn dates_entries() {
            let seen = HashMap::from([(5, 100), (6, 0)]);
            assert_eq!(copied_at("5", &seen, 6, 999), Some(100));
            assert_eq!(copied_at("6", &seen, 6, 999), None);
            assert_eq!(copied_at("7", &seen, 6, 999), Some(999));
            assert_eq!(copied_at("4", &seen, 6, 999), None);
            assert_eq!(copied_at("7", &HashMap::new(), 0, 999), None);
        }
    }
}

/// Windows and macOS: the history the platform watcher keeps in `crate::clips`.
#[cfg(not(target_os = "linux"))]
mod own {
    use chrono::Local;

    use super::{GLYPH, IMAGE_GLYPH, SHOWN, day_group, time_hint};
    use crate::clips::{self, Content};
    use crate::engine::Ctx;
    use crate::item::{Action, Item, Text};

    /// Rows on screen. GTK lays out every row of the list when it opens, which for all 300 took
    /// most of a second on a Windows laptop; older entries are still found by typing.
    const ROWS: usize = 50;

    pub fn history(cx: &Ctx, rest: &str) -> Vec<Item> {
        if !crate::config::get().clipboard_history {
            return vec![Item {
                title: "Clipboard history is off".into(),
                subtitle: "Enter turns it on".into(),
                glyph: GLYPH.into(),
                action: Action::Set { key: "clipboard_history".into(), value: Some("on".into()) },
                ..Default::default()
            }];
        }
        let now = Local::now();
        let found = clips::list().into_iter().filter_map(|clip| {
            let path = clips::image_path(&clip.id);
            let title = match &clip.content {
                Content::Text(text) => clips::summary(text, 100),
                Content::Image { width, height } => {
                    let bytes = std::fs::metadata(&path).ok()?.len();
                    format!("png {width}×{height} · {}", size(bytes))
                }
            };
            let score = if rest.is_empty() { 0 } else { cx.matcher_score(rest, &title)? };
            Some((score, clip, path, title))
        });
        let mut found: Vec<_> = found.take(if rest.is_empty() { ROWS } else { SHOWN }).collect();
        // a search looks through the whole history and shows the best matches, newest first on ties
        found.sort_by_key(|f| std::cmp::Reverse(f.0));
        found
            .into_iter()
            .take(ROWS)
            .enumerate()
            .map(|(i, (score, clip, path, title))| {
                let common = Item {
                    title,
                    hint: time_hint(clip.at, now),
                    group: day_group(Some(clip.at), now.date_naive()),
                    pin: Some(clip.id),
                    score: score * 1000 - i as i64,
                    ..Default::default()
                };
                match clip.content {
                    Content::Text(text) => Item {
                        glyph: GLYPH.into(),
                        text: Some(Text::Literal(text.chars().take(50_000).collect())),
                        action: Action::Copy(text),
                        ..common
                    },
                    Content::Image { .. } => {
                        let path = path.to_string_lossy().into_owned();
                        Item {
                            glyph: IMAGE_GLYPH.into(),
                            subtitle: "image".into(),
                            image: Some(path.clone()),
                            action: Action::CopyImage(path),
                            ..common
                        }
                    }
                }
            })
            .collect()
    }

    /// The text, or the PNG file's bytes, of a history entry.
    pub fn content(id: &str) -> Option<Vec<u8>> {
        match clips::find(id)?.content {
            Content::Text(text) => Some(text.into_bytes()),
            Content::Image { .. } => std::fs::read(clips::image_path(id)).ok(),
        }
    }

    /// `120 KiB`, like cliphist.
    fn size(bytes: u64) -> String {
        match bytes {
            0..1024 => format!("{bytes} B"),
            1024..1_048_576 => format!("{} KiB", bytes / 1024),
            _ => format!("{:.1} MiB", bytes as f64 / 1_048_576.0),
        }
    }
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
    fn hints() {
        let now = Local.with_ymd_and_hms(2026, 10, 19, 15, 0, 0).unwrap();
        assert_eq!(time_hint(ts(2026, 10, 19, 9), now), "09:00");
        assert_eq!(time_hint(ts(2026, 10, 18, 9), now), "Sun 09:00");
        assert_eq!(time_hint(ts(2026, 10, 1, 9), now), "1 Oct");
        assert_eq!(time_hint(ts(2025, 10, 1, 9), now), "1 Oct 2025");
    }
}
