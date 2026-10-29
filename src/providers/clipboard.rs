//! `clip`: clipboard history. Enter puts the entry back on the clipboard.
//! On Linux it comes from cliphist, which needs `wl-paste --watch cliphist store` running (most
//! Hyprland setups already have it). On Windows and macOS zunder keeps its own (`crate::clips`).

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
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::LazyLock;

    use regex::Regex;

    use super::{GLYPH, IMAGE_GLYPH, SHOWN};
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
                Item {
                    title: if image { image_title(preview) } else { preview.trim().replace('\t', " ") },
                    image: image.then(|| image_path(id, preview)).flatten(),
                    image_cmd: image.then(|| decode_cmd(id, preview, line)).flatten(),
                    text: (!image).then(|| Text::Command(format!("printf '%s' {} | cliphist decode", shell_quote(line)))),
                    hint: format!("#{id}"),
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
}

/// Windows and macOS: the history the platform watcher keeps in `crate::clips`.
#[cfg(not(target_os = "linux"))]
mod own {
    use super::{GLYPH, IMAGE_GLYPH, SHOWN};
    use crate::clips::{self, Content};
    use crate::engine::Ctx;
    use crate::item::{Action, Item, Text};

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
        let now = chrono::Utc::now().timestamp();
        clips::list()
            .into_iter()
            .filter_map(|clip| {
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
            })
            .take(SHOWN)
            .enumerate()
            .map(|(i, (score, clip, path, title))| {
                let common = Item {
                    title,
                    hint: age(now - clip.at),
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

    /// How long ago it was copied: `now`, `5m`, `3h`, `2d`.
    fn age(secs: i64) -> String {
        match secs {
            ..60 => "now".into(),
            60..3600 => format!("{}m", secs / 60),
            3600..86_400 => format!("{}h", secs / 3600),
            _ => format!("{}d", secs / 86_400),
        }
    }
}
