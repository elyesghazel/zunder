//! Runs the query through every provider and groups the results into sections.

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use crate::history::History;
use crate::store::{Binds, Favorites};
use crate::item::Item;
use crate::providers::{self, windows::Client};

#[derive(Clone, PartialEq)]
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

    /// Fuzzy score of an arbitrary `pattern` (e.g. the part after a prefix).
    pub fn matcher_score(&self, pattern: &str, text: &str) -> Option<i64> {
        self.matcher.fuzzy_match(text, pattern)
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
    /// Slow work for this query (disk, network), run on a thread. Its items arrive later as a
    /// section with the returned title, placed before the web search fallback.
    fn deferred(&self, _cx: &Ctx) -> Option<(String, Job)> {
        None
    }
    /// Rescan on-disk state (desktop files, plugins…). Called each time the launcher opens.
    fn reload(&mut self) {}
}

/// Work a provider hands to a background thread.
pub type Job = Box<dyn FnOnce() -> Vec<Item> + Send>;

pub struct Engine {
    providers: Vec<Box<dyn Provider>>,
    pub plugins: providers::plugins::Plugins,
    pub history: History,
    pub favorites: Favorites,
    pub binds: Binds,
    matcher: SkimMatcherV2,
}

const PER_SECTION: usize = 12;

impl Engine {
    pub fn new() -> Self {
        let mut providers = providers::all();
        let titles = providers.iter().map(|p| p.title().to_owned()).chain(["Plugins".to_owned()]).collect();
        providers.push(Box::new(providers::settings::Settings::new(titles)));
        Engine {
            providers,
            plugins: providers::plugins::Plugins::load(),
            history: History::load(),
            favorites: Favorites::load(),
            binds: Binds::load(),
            matcher: SkimMatcherV2::default().ignore_case(),
        }
    }

    /// Section titles that can be turned on and off.
    pub fn provider_titles(&self) -> Vec<String> {
        self.providers.iter().map(|p| p.title().to_owned()).filter(|t| t != "Settings").chain(["Plugins".to_owned()]).collect()
    }

    pub fn reload(&mut self) {
        for p in &mut self.providers {
            p.reload();
        }
        self.plugins.reload();
        // files other processes (settings window, `zunder import`) may have changed
        self.history = History::load();
        self.favorites = Favorites::load();
        self.binds = Binds::load();
    }

    pub fn search(&self, query: &str, windows: &[Client], switch: bool) -> (Vec<Section>, Vec<(String, Job)>) {
        let query = query.trim_start();
        let cx = Ctx { query, windows, history: &self.history, switch, matcher: &self.matcher };
        let providers = self.providers.iter().map(|p| p.as_ref()).chain([&self.plugins as &dyn Provider]);
        let all: Vec<&dyn Provider> = {
            let cfg = crate::config::get();
            providers.filter(|p| p.title() == "Settings" || cfg.enabled(p.title())).collect()
        };
        let exclusive = all.iter().find(|p| p.exclusive(query)).copied();
        let active: Vec<&dyn Provider> = match exclusive {
            Some(p) => vec![p],
            None => all,
        };

        let jobs: Vec<(String, Job)> = active.iter().filter_map(|p| p.deferred(&cx)).collect();
        let mut sections: Vec<(i64, Section)> = active
            .into_iter()
            .flat_map(|p| {
                let mut items = p.search(&cx);
                items.sort_by_key(|x| std::cmp::Reverse(x.score));
                let Some(best) = items.first().map(|i| i.score) else { return Vec::new() };
                if !query.is_empty() && exclusive.is_none() {
                    // skim matches very loosely; drop the long tail of scattered-letter hits
                    if best > 0 {
                        items.retain(|i| i.score >= best / 3);
                    }
                    items.truncate(PER_SECTION);
                }
                split_groups(p.title(), items)
                    .into_iter()
                    .map(|s| (s.items.first().map_or(best, |i| i.score), s))
                    .collect::<Vec<_>>()
            })
            .collect();

        // empty query keeps provider order; otherwise the best match leads
        if !query.is_empty() {
            sections.sort_by_key(|x| std::cmp::Reverse(x.0));
        }
        let mut sections: Vec<Section> = sections.into_iter().map(|(_, s)| s).collect();
        // pinned clips lead the clipboard history
        if exclusive.is_some_and(|p| p.title() == "Clipboard") {
            let rest = query.strip_prefix("clip").unwrap_or(query).trim();
            let pinned: Vec<Item> = self
                .favorites
                .items
                .iter()
                .filter(|i| i.id.starts_with(crate::providers::clipboard::PINNED))
                .filter(|i| rest.is_empty() || self.matcher.fuzzy_match(&i.title, rest).is_some())
                .map(|i| {
                    // the full clip in the preview pane, like the history entries below
                    let mut i = i.clone();
                    match &i.action {
                        crate::item::Action::Copy(text) => i.text = Some(crate::item::Text::Literal(text.clone())),
                        _ => i.image = Some(i.icon.clone()).filter(|p| !p.is_empty()),
                    }
                    i
                })
                .collect();
            if !pinned.is_empty() {
                sections.insert(0, Section { title: "Pinned".into(), items: pinned });
            }
        }
        if query.is_empty() && !switch && !self.favorites.items.is_empty() {
            sections.insert(0, Section { title: "Favorites".into(), items: self.favorites.items.clone() });
        }
        for item in sections.iter_mut().flat_map(|s| s.items.iter_mut()) {
            if let Some(combo) = self.binds.combo_for(&item.id) {
                item.hint = combo.to_owned();
            }
            if self.favorites.contains(&item.id) {
                item.hint = format!("★ {}", item.hint);
            }
        }
        (sections, jobs)
    }
}

/// One section per distinct `item.group`, in order of first appearance.
fn split_groups(title: &str, items: Vec<Item>) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    for item in items {
        let name = if item.group.is_empty() { title } else { item.group.as_str() };
        match sections.iter_mut().find(|s| s.title == name) {
            Some(s) => s.items.push(item),
            None => sections.push(Section { title: name.to_owned(), items: vec![item] }),
        }
    }
    sections
}
