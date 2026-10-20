//! Session and power commands.

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item};

pub struct System;

/// zunder's own commands, next to the platform's lock/sleep/restart…
const OWN: &[(&str, &str, &str, &str)] = &[
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
        let platform = crate::platform::system_commands().iter().map(|c| (c.title, c.keywords, c.glyph, c.command));
        platform
            .chain(OWN.iter().copied())
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
                    ..Default::default()
                })
            })
            .collect()
    }
}
