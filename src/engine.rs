//! Runs the query through every provider and groups the results into sections.

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use crate::history::History;
use crate::item::Item;
use crate::providers::{self, windows::Client};

pub struct Section {
    pub title: String,
    pub items: Vec<Item>,
}

/// Everything a provider may look at while searching.
pub struct Ctx<'a> {
    pub query: &'a str,
    pub windows: &'a [Client],
    pub history: &'a History,
    /// Opened with ALT+TAB: windows only, previous window preselected.
    pub switch: bool,
    matcher: &'a SkimMatcherV2,
}

impl Ctx<'_> {
    /// Fuzzy score of the query against `text`, `None` if it does not match.
    pub fn score(&self, text: &str) -> Option<i64> {
        self.matcher.fuzzy_match(text, self.query)
    }
}

/// A source of results. Add a feature = add a file in `providers/` + one line in `providers::all()`.
pub trait Provider {
    /// Section heading.
    fn title(&self) -> &str;
    /// Results for `cx.query` (may be empty). `score` orders items and sections.
    fn search(&self, cx: &Ctx) -> Vec<Item>;
    /// When true for this query, only this provider's section is shown (e.g. `>` for shell).
    fn exclusive(&self, _query: &str) -> bool {
        false
    }
    /// Rescan on-disk state (desktop files, plugins…). Called each time the launcher opens.
    fn reload(&mut self) {}
}

pub struct Engine {
    providers: Vec<Box<dyn Provider>>,
    pub history: History,
    matcher: SkimMatcherV2,
}

const PER_SECTION: usize = 12;

impl Engine {
    pub fn new() -> Self {
        Engine {
            providers: providers::all(),
            history: History::load(),
            matcher: SkimMatcherV2::default().ignore_case(),
        }
    }

    pub fn reload(&mut self) {
        for p in &mut self.providers {
            p.reload();
        }
    }

    pub fn search(&self, query: &str, windows: &[Client], switch: bool) -> Vec<Section> {
        let query = query.trim_start();
        let cx = Ctx { query, windows, history: &self.history, switch, matcher: &self.matcher };
        let all: Vec<&dyn Provider> = self.providers.iter().map(|p| p.as_ref()).collect();
        let active: Vec<&dyn Provider> = match all.iter().find(|p| p.exclusive(query)) {
            Some(p) => vec![*p],
            None => all,
        };

        let mut sections: Vec<(i64, Section)> = active
            .into_iter()
            .filter_map(|p| {
                let mut items = p.search(&cx);
                items.sort_by(|a, b| b.score.cmp(&a.score));
                let best = items.first()?.score;
                if !query.is_empty() {
                    // skim matches very loosely; drop the long tail of scattered-letter hits
                    if best > 0 {
                        items.retain(|i| i.score >= best / 3);
                    }
                    items.truncate(PER_SECTION);
                }
                Some((best, Section { title: p.title().to_owned(), items }))
            })
            .collect();

        // empty query keeps provider order; otherwise the best match leads
        if !query.is_empty() {
            sections.sort_by(|a, b| b.0.cmp(&a.0));
        }
        sections.into_iter().map(|(_, s)| s).collect()
    }
}
