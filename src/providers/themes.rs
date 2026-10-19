//! `theme`: pick a color scheme. Enter applies it and keeps the list open to try the next one.

use crate::config;
use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item};
use crate::theme;

pub struct Themes;

const PREFIX: &str = "theme";
const GLYPH: &str = "\u{f03d8}";

impl Provider for Themes {
    fn title(&self) -> &str {
        "Themes"
    }

    fn exclusive(&self, query: &str) -> bool {
        query == PREFIX || query.starts_with("theme ")
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        if !self.exclusive(cx.query) {
            if cx.query.is_empty() {
                return Vec::new();
            }
            let score = cx.score("Change Theme").or_else(|| cx.score("colors"));
            return score
                .map(|score| Item {
                    id: "cmd:theme".into(),
                    title: "Change Theme".into(),
                    subtitle: "color scheme".into(),
                    hint: "theme ␣".into(),
                    glyph: GLYPH.into(),
                    action: Action::Fill("theme ".into()),
                    score,
                    ..Default::default()
                })
                .into_iter()
                .collect();
        }

        let rest = cx.query[PREFIX.len()..].trim();
        let current = config::get().theme.clone();
        let names = config::choices("theme").unwrap_or_default();
        names
            .iter()
            .enumerate()
            .filter(|(_, name)| rest.is_empty() || cx.matcher_score(rest, name).is_some())
            .map(|(i, name)| {
                let mut subtitle = if theme::is_light(name) { "light" } else { "dark" }.to_owned();
                if theme::is_user(name) {
                    subtitle += ", from themes/";
                }
                let accent = theme::color(name, "accent").unwrap_or_default();
                let on = *name == current;
                Item {
                    title: name.clone(),
                    subtitle,
                    hint: if on { format!("● {accent}") } else { accent },
                    glyph: GLYPH.into(),
                    action: Action::Set { key: "theme".into(), value: Some(name.clone()) },
                    score: 10_000 - i as i64,
                    ..Default::default()
                }
            })
            .collect()
    }
}
