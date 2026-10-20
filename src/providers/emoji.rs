//! Emoji: a few matches in every search from 3 letters on (`fire` → 🔥),
//! `:name` or `emoji name` for the whole picker. Enter copies.

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item};

pub struct Emoji;

const INLINE: usize = 3;
const FULL: usize = 200;

fn prefixed(query: &str) -> Option<&str> {
    if query == "emoji" {
        return Some("");
    }
    query.strip_prefix("emoji ").or_else(|| query.strip_prefix(':')).map(str::trim)
}

/// Every query word starts a word of the name or a shortcode: `red hea` → red heart.
fn word_score(query: &str, e: &emojis::Emoji) -> Option<i64> {
    let name = e.name().to_lowercase();
    let codes: Vec<&str> = e.shortcodes().collect();
    let words: Vec<&str> = name.split([' ', '-', ':']).chain(codes.iter().flat_map(|c| c.split('_'))).collect();
    let all = query.split_whitespace().all(|q| words.iter().any(|w| w.starts_with(q)));
    if !all {
        return None;
    }
    let exact = codes.contains(&query) || name == query;
    Some(if exact { 500 } else { 300 } - name.len() as i64)
}

fn item(e: &emojis::Emoji, score: i64) -> Item {
    Item {
        id: format!("emoji:{}", e.name()),
        title: e.name().to_owned(),
        subtitle: String::new(),
        hint: e.shortcode().map(|c| format!(":{c}:")).unwrap_or_default(),
        glyph: e.as_str().to_owned(),
        action: Action::Copy(e.as_str().to_owned()),
        score,
        ..Default::default()
    }
}

impl Provider for Emoji {
    fn title(&self) -> &str {
        "Emoji"
    }

    fn exclusive(&self, query: &str) -> bool {
        prefixed(query).is_some()
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        if let Some(rest) = prefixed(cx.query) {
            let rest = rest.to_lowercase();
            return emojis::iter()
                .filter(|e| e.skin_tone().is_none_or(|t| t == emojis::SkinTone::Default))
                .enumerate()
                .filter_map(|(i, e)| {
                    let score = if rest.is_empty() {
                        -(i as i64)
                    } else {
                        word_score(&rest, e).or_else(|| cx.matcher_score(&rest, e.name()))?
                    };
                    Some(item(e, score))
                })
                .take(if rest.is_empty() { FULL } else { usize::MAX })
                .collect();
        }
        let q = cx.query.trim().to_lowercase();
        if q.chars().count() < 3 || cx.switch {
            return Vec::new();
        }
        let mut hits: Vec<Item> = emojis::iter()
            .filter(|e| e.skin_tone().is_none_or(|t| t == emojis::SkinTone::Default))
            .filter_map(|e| Some(item(e, word_score(&q, e)? / 20)))
            .collect();
        hits.sort_by_key(|x| std::cmp::Reverse(x.score));
        hits.truncate(INLINE);
        hits
    }
}
