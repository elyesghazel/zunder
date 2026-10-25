//! The Ctrl+K menu: everything you can do with a result besides Enter.

use std::path::Path;

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use crate::item::{Action, Item, WindowOp, shell_quote};
use crate::providers::windows::Client;

/// Menu entries the launcher handles itself (they need its state, not a shell).
pub const FAVORITE: &str = "zunder:favorite";
pub const KEYBIND: &str = "zunder:keybind";

/// The entries for `item`, filtered by `query`. `windows` tells a window's workspace.
pub fn menu(item: &Item, windows: &[Client], favorite: bool, query: &str) -> Vec<Item> {
    let mut out = Vec::new();
    let mut add = |title: &str, glyph: &str, hint: &str, action: Action| {
        out.push(Item { title: title.into(), glyph: glyph.into(), hint: hint.into(), action, ..Default::default() });
    };

    match &item.action {
        Action::Focus(address) => {
            let ws = windows.iter().find(|c| &c.address == address).map(|c| c.workspace.name.clone()).unwrap_or_default();
            let op = |op: WindowOp| Action::Window { address: address.clone(), op };
            add("Focus", "\u{f2d0}", "↵", Action::Focus(address.clone()));
            add("Bring to this workspace", "\u{f063}", "^↵", op(WindowOp::Pull));
            add("Fullscreen", "\u{f0293}", "", op(WindowOp::Fullscreen));
            add("Minimize", "\u{f05b0}", "^M", op(WindowOp::Minimize));
            add("Close", "\u{f00d}", "^W", op(WindowOp::Close));
            for n in 1..=9 {
                if ws != n.to_string() {
                    add(&format!("Move to workspace {n}"), "\u{f061}", &format!("^{n}"), op(WindowOp::Move(n.to_string())));
                }
            }
            add("Copy title", "\u{f0c5}", "", Action::Copy(item.title.clone()));
            add("Copy class", "\u{f0c5}", "", Action::Copy(item.subtitle.clone()));
        }
        Action::Open(target) if is_local(target) => {
            let path = Path::new(target);
            let folder = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
            let folder_str = folder.to_string_lossy().into_owned();
            add("Open", "\u{f15b}", "↵", Action::Open(target.clone()));
            if !path.is_dir() {
                add("Open containing folder", "\u{f07c}", "", Action::Open(folder_str.clone()));
            }
            add("Open a terminal here", "\u{f120}", "", Action::Run(terminal_in(&folder_str)));
            add("Copy path", "\u{f0c5}", "", Action::Copy(target.clone()));
            if let Some(name) = path.file_name() {
                add("Copy name", "\u{f0c5}", "", Action::Copy(name.to_string_lossy().into_owned()));
            }
        }
        Action::Open(target) => {
            add("Open", "\u{f0c1}", "↵", Action::Open(target.clone()));
            add("Copy link", "\u{f0c5}", "", Action::Copy(target.clone()));
        }
        Action::Run(cmd) if item.id.starts_with("app:") => {
            add("Launch", "\u{f135}", "↵", Action::Run(cmd.clone()));
            add("Launch in a terminal", "\u{f120}", "", Action::Term(cmd.clone()));
            if let Some(file) = desktop_file(&item.id["app:".len()..]) {
                add("Open desktop file", "\u{f15c}", "", Action::Open(file.clone()));
                add("Copy desktop file path", "\u{f0c5}", "", Action::Copy(file));
            }
            add("Copy command", "\u{f0c5}", "", Action::Copy(cmd.clone()));
        }
        Action::Run(cmd) => {
            add("Run", "\u{f04b}", "↵", Action::Run(cmd.clone()));
            add("Run in a terminal", "\u{f120}", "", Action::Term(cmd.clone()));
            add("Copy command", "\u{f0c5}", "", Action::Copy(cmd.clone()));
        }
        Action::Term(cmd) => {
            add("Run in a terminal", "\u{f120}", "↵", Action::Term(cmd.clone()));
            add("Copy command", "\u{f0c5}", "", Action::Copy(cmd.clone()));
        }
        Action::Copy(text) => {
            add("Copy", "\u{f0c5}", "↵", Action::Copy(text.clone()));
            if text.starts_with("http://") || text.starts_with("https://") {
                add("Open link", "\u{f0c1}", "", Action::Open(text.trim().to_owned()));
            }
        }
        Action::Fill(text) => add("Open", "\u{f061}", "↵", Action::Fill(text.clone())),
        Action::Window { .. } | Action::Set { .. } | Action::None => {}
    }

    for extra in &item.actions {
        let glyph = if extra.glyph.is_empty() { "\u{f0e7}" } else { extra.glyph.as_str() };
        add(&extra.title, glyph, "", extra.action.clone());
    }

    if !item.id.is_empty() {
        let set = |key: &str| Action::Set { key: key.into(), value: None };
        add(if favorite { "Remove from favorites" } else { "Add to favorites" }, "\u{f005}", "^F", set(FAVORITE));
        add("Set a global shortcut", "\u{f11c}", "^B", set(KEYBIND));
    }
    if !matches!(item.action, Action::Focus(_)) && !item.title.is_empty() {
        add("Copy title", "\u{f0c5}", "", Action::Copy(item.title.clone()));
    }

    let query = query.trim();
    if query.is_empty() {
        return out;
    }
    let matcher = SkimMatcherV2::default().ignore_case();
    let mut scored: Vec<(i64, Item)> = out.into_iter().filter_map(|i| Some((matcher.fuzzy_match(&i.title, query)?, i))).collect();
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(_, i)| i).collect()
}

fn is_local(target: &str) -> bool {
    // `/x` isn't absolute on Windows (no drive), but it's still no link
    !target.contains("://") && (Path::new(target).is_absolute() || target.starts_with('/'))
}

/// A shell line that opens the configured terminal in `dir`.
fn terminal_in(dir: &str) -> String {
    format!("cd {} && {}", shell_quote(dir), crate::config::terminal())
}

/// Where the desktop file with this id lives (the first one found, like the app list).
fn desktop_file(id: &str) -> Option<String> {
    // `kde-org.foo.desktop` may live in applications/kde/org.foo.desktop
    let dirs = crate::platform::app_dirs();
    let direct = dirs.iter().map(|d| d.join(id));
    let nested = dirs.iter().map(|d| d.join(id.replacen('-', "/", 1)));
    direct.chain(nested).find(|p| p.is_file()).map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    #[test]
    fn window_menu_skips_its_own_workspace() {
        let item = Item { id: "x".into(), title: "btop".into(), action: Action::Focus("0x1".into()), ..Default::default() };
        let client = Client {
            address: "0x1".into(),
            workspace: crate::providers::windows::Workspace { name: "3".into(), ..Default::default() },
            ..Default::default()
        };
        let menu = menu(&item, &[client], false, "");
        let t = titles(&menu);
        assert!(t.contains(&"Close"));
        assert!(t.contains(&"Move to workspace 2"));
        assert!(!t.contains(&"Move to workspace 3"));
        assert!(t.contains(&"Add to favorites"));
    }

    #[test]
    fn filters_by_query() {
        let item = Item { title: "notes.md".into(), action: Action::Open("/tmp/notes.md".into()), ..Default::default() };
        let menu = menu(&item, &[], false, "copy path");
        assert_eq!(menu.first().map(|i| i.title.as_str()), Some("Copy path"));
    }

    #[test]
    fn plugins_add_their_own() {
        let json = r#"{"title": "Copy id", "copy": "42"}"#;
        let extra: crate::item::MenuAction = serde_json::from_str(json).unwrap();
        assert_eq!(extra.action, Action::Copy("42".into()));
        let item = Item { title: "x".into(), actions: vec![extra], ..Default::default() };
        assert!(titles(&menu(&item, &[], false, "")).contains(&"Copy id"));
    }
}
