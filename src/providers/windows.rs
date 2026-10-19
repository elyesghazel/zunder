//! Open Hyprland windows, most recently focused first.

use std::collections::HashMap;
use std::process::Command;

use serde::Deserialize;

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item};
use crate::providers::apps;

#[derive(Clone, Debug, Deserialize)]
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

#[derive(Clone, Debug, Deserialize)]
pub struct Workspace {
    pub name: String,
}

fn yes() -> bool {
    true
}

pub fn list() -> Vec<Client> {
    let Ok(out) = Command::new("hyprctl").args(["clients", "-j"]).output() else {
        return Vec::new();
    };
    let mut clients: Vec<Client> = serde_json::from_slice(&out.stdout).unwrap_or_default();
    clients.retain(|c| c.mapped && !(c.class.is_empty() && c.title.is_empty()));
    clients.sort_by_key(|c| c.focus);
    clients
}

pub struct Windows {
    /// lowercase window class → icon, from the desktop files
    icons: HashMap<String, String>,
}

impl Windows {
    pub fn load() -> Self {
        let mut w = Windows { icons: HashMap::new() };
        w.reload();
        w
    }

    fn icon_for(&self, class: &str) -> String {
        let class = class.to_lowercase();
        // org.gnome.Nautilus → nautilus as a last resort
        let short = class.rsplit('.').next().unwrap_or(&class).to_owned();
        self.icons.get(&class).or_else(|| self.icons.get(&short)).cloned().unwrap_or(class)
    }
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
                    self.icons.entry(key).or_insert_with(|| app.icon.clone());
                }
            }
        }
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
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
                Some(Item {
                    title: if c.title.is_empty() { c.class.clone() } else { c.title.clone() },
                    subtitle: c.class.clone(),
                    hint: if c.focus == 0 { "focused".into() } else { format!("ws {ws}") },
                    glyph: "\u{f2d0}".into(),
                    icon: self.icon_for(&c.class),
                    preview: (!c.stable_id.is_empty()).then(|| c.stable_id.clone()),
                    action: Action::Focus(c.address.clone()),
                    score,
                    ..Default::default()
                })
            })
            .collect()
    }
}
