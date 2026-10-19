//! Fallback: search the web for whatever was typed.

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item, url_encode};

pub struct Web;

impl Provider for Web {
    fn title(&self) -> &str {
        "Web"
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        let q = cx.query.trim();
        if q.is_empty() {
            return Vec::new();
        }
        vec![Item {
            title: format!("Search “{q}”"),
            subtitle: "duckduckgo".into(),
            hint: "web".into(),
            glyph: "\u{f0ac}".into(),
            action: Action::Open(format!("https://duckduckgo.com/?q={}", url_encode(q))),
            score: -1,
            ..Default::default()
        }]
    }
}
