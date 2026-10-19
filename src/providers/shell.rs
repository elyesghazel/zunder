//! `> cmd` runs a shell command in a terminal.

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item};

pub struct Shell;

impl Provider for Shell {
    fn title(&self) -> &str {
        "Shell"
    }

    fn exclusive(&self, query: &str) -> bool {
        query.starts_with('>')
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        let Some(cmd) = cx.query.strip_prefix('>').map(str::trim) else { return Vec::new() };
        if cmd.is_empty() {
            return vec![Item {
                title: "type a command".into(),
                glyph: "\u{f120}".into(),
                ..Default::default()
            }];
        }
        vec![
            Item {
                title: cmd.to_owned(),
                subtitle: "run in terminal".into(),
                hint: "↵".into(),
                glyph: "\u{f120}".into(),
                action: Action::Term(cmd.to_owned()),
                score: 2,
                ..Default::default()
            },
            Item {
                title: cmd.to_owned(),
                subtitle: "run in background".into(),
                glyph: "\u{f120}".into(),
                action: Action::Run(cmd.to_owned()),
                score: 1,
                ..Default::default()
            },
        ]
    }
}
