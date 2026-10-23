//! Fallback for whatever was typed: search the web, or ask the `ai` plugin.

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item, url_encode};

pub struct Web;

impl Provider for Web {
    fn title(&self) -> &str {
        "Other"
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        let q = cx.query.trim();
        if q.is_empty() {
            return Vec::new();
        }
        let mut items = vec![Item {
            title: format!("Search “{q}”"),
            id: format!("web:{q}"),
            subtitle: "web search".into(),
            hint: "web".into(),
            glyph: "\u{f0ac}".into(),
            action: Action::Open(crate::config::get().search_url.replace("{q}", &url_encode(q))),
            score: -1,
            ..Default::default()
        }];
        // Only offered when an `ai` plugin is installed; a trailing `?` makes it answer.
        if super::plugins::dir().join("ai").exists() {
            let ask = q.trim_end_matches('?');
            items.push(Item {
                title: format!("Ask AI “{ask}”"),
                id: format!("ai:{q}"),
                subtitle: "answer here in zunder".into(),
                hint: "ai".into(),
                glyph: "\u{f0335}".into(),
                action: Action::Fill(format!("ai {ask}?")),
                score: -2,
                ..Default::default()
            });
        }
        items
    }
}
