//! `clip`: clipboard history from cliphist. Enter puts the entry back on the clipboard.
//! Needs `wl-paste --watch cliphist store` running (most Hyprland setups already have it).

use std::process::Command;

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
                    title: preview.trim().replace('\t', " "),
                    hint: format!("#{id}"),
                    glyph: if image { "\u{f03e}".into() } else { GLYPH.into() },
                    // cliphist decode takes the whole list line on stdin
                    action: Action::Run(format!("printf '%s' {} | cliphist decode | wl-copy", shell_quote(line))),
                    score: score * 1000 - i as i64,
                    ..Default::default()
                }
            })
            .collect()
    }
}
