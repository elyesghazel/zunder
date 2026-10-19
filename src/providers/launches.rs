//! `history`: everything ever launched from zunder, newest first. Enter runs it again.

use chrono::{Local, TimeZone};

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item};
use crate::store;

pub struct Launches;

const PREFIX: &str = "history";
const SHOWN: usize = 300;

fn when(at: i64) -> String {
    let Some(t) = Local.timestamp_opt(at, 0).single() else { return String::new() };
    let now = Local::now();
    let days = (now.date_naive() - t.date_naive()).num_days();
    match days {
        0 => t.format("today %H:%M").to_string(),
        1 => t.format("yesterday %H:%M").to_string(),
        2..=6 => t.format("%a %H:%M").to_string(),
        _ if t.format("%Y").to_string() == now.format("%Y").to_string() => t.format("%d %b %H:%M").to_string(),
        _ => t.format("%d %b %Y").to_string(),
    }
}

impl Provider for Launches {
    fn title(&self) -> &str {
        "History"
    }

    fn exclusive(&self, query: &str) -> bool {
        query == PREFIX || query.starts_with("history ")
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        if !self.exclusive(cx.query) {
            if cx.query.is_empty() {
                return Vec::new();
            }
            return cx
                .score("Launch History")
                .map(|score| Item {
                    id: "cmd:history".into(),
                    title: "Launch History".into(),
                    subtitle: "everything you ran from zunder".into(),
                    hint: "history ␣".into(),
                    glyph: "\u{f1da}".into(),
                    action: Action::Fill("history ".into()),
                    score,
                    ..Default::default()
                })
                .into_iter()
                .collect();
        }

        let rest = cx.query[PREFIX.len()..].trim();
        store::launches()
            .into_iter()
            .filter(|e| rest.is_empty() || cx.matcher_score(rest, &format!("{} {}", e.item.title, e.item.subtitle)).is_some())
            .take(SHOWN)
            .enumerate()
            .map(|(i, e)| Item { hint: when(e.at), score: -(i as i64), ..e.item })
            .collect()
    }
}
