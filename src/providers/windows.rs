//! Open windows, most recently focused first. The list comes from `platform::list_windows`.

use std::collections::HashMap;

use serde::Deserialize;

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item};
use crate::providers::apps;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Client {
    pub address: String,
    pub class: String,
    pub title: String,
    pub workspace: Workspace,
    #[serde(rename = "focusHistoryID")]
    pub focus: i64,
    #[serde(default = "yes")]
    pub mapped: bool,
    /// What `grim -T` takes to capture this window.
    #[serde(rename = "stableId", default)]
    pub stable_id: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Workspace {
    #[serde(default)]
    pub id: i64,
    pub name: String,
}

fn yes() -> bool {
    true
}

pub fn list() -> Vec<Client> {
    let mut clients = crate::platform::list_windows();
    let hidden = crate::config::get().hide_windows.clone();
    clients.retain(|c| {
        c.mapped && !(c.class.is_empty() && c.title.is_empty()) && !hidden.iter().any(|h| h.eq_ignore_ascii_case(&c.class))
    });
    clients.sort_by_key(|c| c.focus);
    clients
}

pub struct Windows {
    /// lowercase window class → (icon, app name), from the desktop files
    icons: HashMap<String, (String, String)>,
}

impl Windows {
    pub fn load() -> Self {
        let mut w = Windows { icons: HashMap::new() };
        w.reload();
        w
    }

    /// The icon and app name for a window class; without a desktop file the class itself.
    fn app_for(&self, class: &str) -> (String, String) {
        let lower = class.to_lowercase();
        // org.gnome.Nautilus → nautilus as a last resort
        let short = lower.rsplit('.').next().unwrap_or(&lower).to_owned();
        match self.icons.get(&lower).or_else(|| self.icons.get(&short)) {
            Some((icon, name)) => (icon.clone(), name.clone()),
            None => (lower, pretty_class(class)),
        }
    }
}

/// `org.gnome.Nautilus` → `Nautilus`, `kitty` → `Kitty`.
fn pretty_class(class: &str) -> String {
    let short = class.rsplit('.').next().unwrap_or(class);
    let mut chars = short.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

impl Provider for Windows {
    fn title(&self) -> &str {
        "Windows"
    }

    fn reload(&mut self) {
        self.icons.clear();
        for app in apps::scan_all().into_iter().filter(|a| !a.icon.is_empty()) {
            let id = app.id.trim_end_matches(".desktop").to_lowercase();
            let short = id.rsplit('.').next().unwrap_or(&id).to_owned();
            for key in [app.wm_class.to_lowercase(), id, short, app.name.to_lowercase()] {
                if !key.is_empty() {
                    self.icons.entry(key).or_insert_with(|| (app.icon.clone(), app.name.clone()));
                }
            }
        }
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        if cx.query.is_empty() && !cx.switch && !crate::config::get().start_windows {
            return Vec::new();
        }
        cx.windows
            .iter()
            .enumerate()
            .filter_map(|(rank, c)| {
                let score = if cx.query.is_empty() {
                    1000 - rank as i64
                } else {
                    cx.score(&format!("{} {}", c.title, c.class))? + 20
                };
                let ws = c.workspace.name.trim_start_matches("special:");
                let (icon, app) = self.app_for(&c.class);
                Some(Item {
                    title: if c.title.is_empty() { c.class.clone() } else { c.title.clone() },
                    subtitle: c.class.clone(),
                    hint: if c.focus == 0 { "focused".into() } else { format!("ws {ws}") },
                    glyph: "\u{f2d0}".into(),
                    icon,
                    app,
                    preview: (!c.stable_id.is_empty()).then(|| c.stable_id.clone()),
                    action: Action::Focus(c.address.clone()),
                    score,
                    ..Default::default()
                })
            })
            .collect()
    }
}
