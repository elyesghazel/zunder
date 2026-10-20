//! `settings`: the settings page. Enter toggles, `settings <key> <value>` sets a value.

use crate::config::{self, SETTINGS};
use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item};
use crate::providers::plugins;

pub struct Settings {
    /// Section titles of every other provider, for the on/off switches.
    providers: Vec<String>,
}

const PREFIX: &str = "settings";
const GLYPH: &str = "\u{f013}";

impl Settings {
    pub fn new(providers: Vec<String>) -> Self {
        Settings { providers }
    }
}

fn on_off(on: bool) -> String {
    if on { "● on".into() } else { "○ off".into() }
}

/// Open a file in the default app, creating it (and its folder) first.
fn open_file(path: &std::path::Path, folder: bool) -> Action {
    // make sure it exists so the file manager / editor has something to open
    if folder {
        let _ = std::fs::create_dir_all(path);
    } else if !path.exists() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, "");
    }
    Action::Open(path.to_string_lossy().into_owned())
}

impl Provider for Settings {
    fn title(&self) -> &str {
        "Settings"
    }

    fn exclusive(&self, query: &str) -> bool {
        query == PREFIX || query.starts_with("settings ")
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        if !self.exclusive(cx.query) {
            if cx.query.is_empty() {
                return Vec::new();
            }
            let score = cx.score("Zunder Settings").or_else(|| cx.score("preferences config"));
            return score
                .map(|score| Item {
                    id: "cmd:settings".into(),
                    title: "Zunder Settings".into(),
                    subtitle: "opens the settings window".into(),
                    hint: "window".into(),
                    glyph: GLYPH.into(),
                    action: Action::Run("zunder settings".into()),
                    score,
                    ..Default::default()
                })
                .into_iter()
                .collect();
        }

        let rest = cx.query[PREFIX.len()..].trim_start();
        let cfg = config::get();

        // `settings font_size 13` → one result that sets it
        if let Some((key, value)) = rest.split_once(' ')
            && let Some(s) = SETTINGS.iter().find(|s| s.key == key)
        {
            return vec![Item {
                title: format!("Set {} to {}", s.title.to_lowercase(), value.trim()),
                subtitle: s.help.into(),
                hint: format!("now {}", cfg.value(key).unwrap_or_default()),
                glyph: GLYPH.into(),
                action: Action::Set { key: key.into(), value: Some(value.into()) },
                group: s.group.into(),
                score: 1,
                ..Default::default()
            }];
        }

        let matches = |text: &str| rest.is_empty() || cx.matcher_score(rest, text).is_some();
        let mut items = Vec::new();
        let mut push = |item: Item| {
            let score = 10_000 - items.len() as i64;
            items.push(Item { score, glyph: GLYPH.into(), ..item });
        };

        for s in SETTINGS.iter().filter(|s| matches(s.title) || matches(s.key)) {
            let (hint, action) = if cfg.is_bool(s.key) {
                (on_off(cfg.value(s.key).as_deref() == Some("on")), Action::Set { key: s.key.into(), value: None })
            } else {
                let value = cfg.value(s.key).filter(|v| !v.is_empty()).unwrap_or_else(|| "default".into());
                (value, Action::Fill(format!("settings {} ", s.key)))
            };
            push(Item { title: s.title.into(), subtitle: s.help.into(), hint, action, group: s.group.into(), ..Default::default() });
        }

        for p in self.providers.iter().filter(|p| matches(p)) {
            push(Item {
                title: p.clone(),
                subtitle: "show this section in results".into(),
                hint: on_off(cfg.enabled(p)),
                action: Action::Set { key: format!("provider:{p}"), value: None },
                group: "Providers".into(),
                ..Default::default()
            });
        }

        let dir = config::path().parent().map(|d| d.to_path_buf()).unwrap_or_default();
        for (title, sub, action) in [
            ("Open config.toml", "every setting above, as a file", open_file(&config::path(), false)),
            ("Edit style.css", "CSS on top of the built-in theme", open_file(&dir.join("style.css"), false)),
            ("Open plugins folder", "drop executable scripts in here", open_file(&plugins::dir(), true)),
        ] {
            if matches(title) {
                push(Item { title: title.into(), subtitle: sub.into(), hint: "open".into(), action, group: "Files".into(), ..Default::default() });
            }
        }
        items
    }
}
