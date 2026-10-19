//! Open Hyprland windows, most recently focused first.

use std::process::Command;

use serde::Deserialize;

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item};

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

pub struct Windows;

impl Provider for Windows {
    fn title(&self) -> &str {
        "Windows"
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
                    id: String::new(),
                    title: if c.title.is_empty() { c.class.clone() } else { c.title.clone() },
                    subtitle: c.class.clone(),
                    hint: if c.focus == 0 { "focused".into() } else { format!("ws {ws}") },
                    glyph: "\u{f2d0}".into(),
                    action: Action::Focus(c.address.clone()),
                    score,
                })
            })
            .collect()
    }
}
