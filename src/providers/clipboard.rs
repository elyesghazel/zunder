//! `clip`: clipboard history from cliphist. Enter puts the entry back on the clipboard.
//! Needs `wl-paste --watch cliphist store` running (most Hyprland setups already have it).

use std::path::PathBuf;
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item, Text, shell_quote};

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
                return vec![super::missing_tool(
                    "cliphist",
                    "keeps the clipboard history",
                    "; then `zunder setup` starts it with Hyprland",
                )];
            }
        };
        let rest = cx.query[PREFIX.len()..].trim();
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
                    glyph: if image { "\u{f03e}".into() } else { GLYPH.into() },
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
}

/// Id prefix of pinned clips in favorites.json.
pub const PINNED: &str = "clip:";

/// A favorite holding the content of a cliphist entry, so it outlives the history.
/// Text is copied straight from favorites.json, images from ~/.local/share/zunder/clips.
pub fn pinned(line: &str, entry: &Item) -> Option<Item> {
    let out = Command::new("sh")
        .args(["-c", &format!("printf '%s' {} | cliphist decode", shell_quote(line))])
        .output()
        .ok()
        .filter(|o| o.status.success() && !o.stdout.is_empty())?;
    let id = format!("{PINNED}{:016x}", crate::store::fnv1a(&out.stdout));
    let item = Item { id, glyph: entry.glyph.clone(), hint: "pinned".into(), ..Default::default() };
    if entry.image.is_some() {
        let ext = entry.image.as_deref().and_then(|p| p.rsplit_once('.')).map_or("png", |(_, e)| e);
        let path = crate::store::data_dir().join("clips").join(format!("{}.{ext}", &item.id[PINNED.len()..]));
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::write(&path, &out.stdout).ok()?;
        let path = path.to_string_lossy().into_owned();
        return Some(Item {
            title: entry.title.clone(),
            subtitle: "image".into(),
            icon: path.clone(),
            action: Action::Run(format!("wl-copy < {}", shell_quote(&path))),
            ..item
        });
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    let mut title: String = first.chars().take(80).collect();
    if title.len() < first.len() || text.trim() != first {
        title.push('…');
    }
    Some(Item { title, action: Action::Copy(text), ..item })
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
