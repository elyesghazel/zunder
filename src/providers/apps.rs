//! Desktop applications from the XDG data dirs.

use std::collections::HashSet;

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item};

#[derive(Clone, Debug, Default)]
pub struct App {
    pub id: String,
    pub name: String,
    pub generic: String,
    pub comment: String,
    pub keywords: String,
    pub exec: String,
    pub icon: String,
    pub wm_class: String,
    pub categories: String,
    pub terminal: bool,
}

pub struct Apps {
    apps: Vec<App>,
}

/// How likely an app is to be wanted when there's no history yet.
fn suggestion_weight(app: &App) -> i64 {
    const WEIGHTS: &[(&str, i64)] = &[
        ("WebBrowser", 60),
        ("TerminalEmulator", 55),
        ("FileManager", 50),
        ("TextEditor", 45),
        ("IDE", 45),
        ("InstantMessaging", 40),
        ("Chat", 40),
        ("Music", 35),
        ("Player", 30),
        ("Development", 20),
        ("Office", 15),
        ("Graphics", 10),
        ("Game", 10),
        ("Utility", 5),
        ("Settings", -20),
        ("System", -15),
        ("Qt", -5),
    ];
    let cats: Vec<&str> = app.categories.split(';').collect();
    WEIGHTS.iter().filter(|(c, _)| cats.contains(c)).map(|(_, w)| w).sum()
}

impl Apps {
    pub fn load() -> Self {
        Apps { apps: scan_all() }
    }

    fn item(&self, app: &App, score: i64) -> Item {
        Item { score, ..item(app) }
    }
}

/// The result row for an app.
pub fn item(app: &App) -> Item {
    let cmd = if app.terminal { crate::platform::terminal_command(&app.exec) } else { app.exec.clone() };
    let subtitle = [&app.comment, &app.generic].into_iter().find(|s| !s.is_empty()).cloned().unwrap_or_default();
    Item {
        id: format!("app:{}", app.id),
        title: app.name.clone(),
        subtitle,
        hint: "app".into(),
        glyph: "\u{f135}".into(),
        icon: app.icon.clone(),
        action: Action::Run(cmd),
        ..Default::default()
    }
}

impl Provider for Apps {
    fn title(&self) -> &str {
        "Apps"
    }

    fn reload(&mut self) {
        self.apps = scan_all();
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        if cx.query.is_empty() {
            if cx.switch {
                return Vec::new();
            }
            // launched apps first, then the most "default-looking" apps so it's never empty
            let mut start: Vec<(i64, &App)> = self
                .apps
                .iter()
                .map(|a| {
                    let launches = i64::from(cx.history.count(&format!("app:{}", a.id)));
                    (launches * 1000 + suggestion_weight(a), a)
                })
                .filter(|(score, _)| *score > 0)
                .collect();
            start.sort_by_key(|x| std::cmp::Reverse(x.0));
            return start
                .into_iter()
                .take(crate::config::get().start_apps)
                .map(|(score, a)| {
                    let mut item = self.item(a, score);
                    item.hint = if score >= 1000 { "recent".into() } else { "suggested".into() };
                    item
                })
                .collect();
        }
        self.apps
            .iter()
            .filter_map(|a| {
                let best = [
                    cx.score(&a.name).map(|s| s * 2),
                    cx.score(&a.generic),
                    cx.score(&a.keywords),
                    cx.score(a.exec.split_whitespace().next().unwrap_or_default()),
                ]
                .into_iter()
                .flatten()
                .max()?;
                Some(self.item(a, best + cx.history.boost(&format!("app:{}", a.id))))
            })
            .collect()
    }
}

/// Installed apps from the platform, one entry per name, sorted.
pub fn scan_all() -> Vec<App> {
    let mut apps = crate::platform::scan_apps();
    // the same app often ships several entries (flatpak + native, aliases): keep the first
    let mut names = HashSet::new();
    apps.retain(|a| names.insert(a.name.to_lowercase()));
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}
