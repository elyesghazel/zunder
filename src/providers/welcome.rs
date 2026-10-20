//! First run: until there's a config.toml, the start screen shows where to begin.
//! `zunder setup` (or saving any setting) writes the config, and this goes away.

use crate::config;
use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item};

pub struct Welcome;

impl Provider for Welcome {
    fn title(&self) -> &str {
        "Welcome"
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        if !cx.query.is_empty() || cx.switch || config::path().exists() {
            return Vec::new();
        }
        let item = |title: &str, subtitle: &str, glyph: &str, action: Action, score: i64| Item {
            title: title.into(),
            subtitle: subtitle.into(),
            glyph: glyph.into(),
            action,
            score,
            ..Default::default()
        };
        vec![
            item("Run setup", "keybinds, autostart and theme, a few questions in a terminal", "\u{f0ad}", Action::Term("zunder setup".into()), 4),
            item("Pick a theme", "12 built in, Enter applies live", "\u{f1fc}", Action::Fill("theme ".into()), 3),
            item("Settings", "Ctrl+, from anywhere in zunder", "\u{f013}", Action::Run("zunder settings".into()), 2),
            item(
                "Keys and tricks",
                "Ctrl+F favorite · Ctrl+B global shortcut · > shell · = calc · clip · time in tokyo",
                "\u{f11c}",
                Action::Open("https://github.com/elyesghazel/zunder/blob/main/docs/usage.md".into()),
                1,
            ),
        ]
    }
}
