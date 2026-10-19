//! Session and power commands.

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item};

pub struct System;

const COMMANDS: &[(&str, &str, &str, &str)] = &[
    // title, keywords, glyph, command
    ("Lock Screen", "lock", "\u{f023}", "loginctl lock-session"),
    ("Suspend", "sleep", "󰤄", "systemctl suspend"),
    ("Reboot", "restart", "\u{f021}", "systemctl reboot"),
    ("Shut Down", "power off poweroff", "⏻", "systemctl poweroff"),
    ("Log Out", "exit logout session", "󰍃", "loginctl terminate-session \"$XDG_SESSION_ID\""),
    ("Reload Zunder", "rescan refresh", "\u{f2f1}", "zunder reload"),
    ("Quit Zunder", "exit", "\u{f00d}", "zunder quit"),
];

impl Provider for System {
    fn title(&self) -> &str {
        "System"
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        if cx.query.is_empty() {
            return Vec::new();
        }
        COMMANDS
            .iter()
            .filter_map(|(title, keywords, glyph, cmd)| {
                let score = cx.score(title).or_else(|| cx.score(keywords).map(|s| s / 2))?;
                Some(Item {
                    id: format!("sys:{title}"),
                    title: (*title).into(),
                    subtitle: String::new(),
                    hint: "system".into(),
                    glyph: (*glyph).into(),
                    action: Action::Run((*cmd).into()),
                    score: score + cx.history.boost(&format!("sys:{title}")),
                })
            })
            .collect()
    }
}
