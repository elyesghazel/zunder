//! Windows. Apps (Start menu shortcuts), actions and system commands work; the window switcher,
//! previews and global shortcuts are still to come (EnumWindows, DWM thumbnails, RegisterHotKey).

use std::fs;
use std::path::{Path, PathBuf};

use gtk4 as gtk;
use gtk4::prelude::*;

use super::{SystemCommand, pipe_to, spawn};
use crate::providers::apps::App;
use crate::providers::windows::Client;
use crate::store::Bind;

/// No layer shell on Windows: a borderless window that the window manager places.
pub fn init_overlay(window: &gtk::ApplicationWindow) {
    window.set_decorated(false);
    window.set_resizable(false);
}

/// The window is only as big as the panel; it closes when it loses focus instead.
pub const FULL_SCREEN_OVERLAY: bool = false;

pub fn list_windows() -> Vec<Client> {
    Vec::new()
}

pub fn focus_window(_id: &str) {}

pub fn capture_window(_id: &str, _path: &Path) -> bool {
    false
}

pub fn open(target: &str) {
    spawn(&["cmd", "/C", "start", "", target]);
}

pub fn copy(text: &str) {
    pipe_to("clip", &[], text);
}

pub fn run_in_terminal(cmd: &str) {
    spawn(&["cmd", "/C", "start", "cmd", "/K", cmd]);
}

pub fn terminal_command(exec: &str) -> String {
    format!("start cmd /K {exec}")
}

pub fn system_commands() -> &'static [SystemCommand] {
    const C: &[SystemCommand] = &[
        SystemCommand { title: "Lock Screen", keywords: "lock", glyph: "\u{f023}", command: "rundll32.exe user32.dll,LockWorkStation" },
        SystemCommand { title: "Sleep", keywords: "suspend", glyph: "\u{f0904}", command: "rundll32.exe powrprof.dll,SetSuspendState 0,1,0" },
        SystemCommand { title: "Restart", keywords: "reboot", glyph: "\u{f021}", command: "shutdown /r /t 0" },
        SystemCommand { title: "Shut Down", keywords: "power off", glyph: "⏻", command: "shutdown /s /t 0" },
        SystemCommand { title: "Log Out", keywords: "exit logout sign out", glyph: "\u{f0343}", command: "shutdown /l" },
    ];
    C
}

/// No exec bit on Windows: anything that Windows can run directly counts.
pub fn is_executable(meta: &fs::Metadata, path: &Path) -> bool {
    meta.is_file()
        && path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| ["exe", "bat", "cmd", "ps1", "py"].contains(&e.to_ascii_lowercase().as_str()))
}

pub fn install_binds(_binds: &[Bind]) -> Result<String, String> {
    Err("global shortcuts aren't supported on Windows yet".into())
}

/// Folders whose change means the app list needs a rescan.
pub fn app_dirs() -> Vec<PathBuf> {
    ["APPDATA", "PROGRAMDATA"]
        .iter()
        .filter_map(|v| std::env::var_os(v))
        .map(|base| PathBuf::from(base).join("Microsoft\\Windows\\Start Menu\\Programs"))
        .collect()
}

/// Shortcuts (`.lnk`) in the user's and the shared Start menu.
pub fn scan_apps() -> Vec<App> {
    let mut apps = Vec::new();
    for root in app_dirs() {
        walk(&root, &mut apps);
    }
    apps
}

fn walk(dir: &Path, apps: &mut Vec<App>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, apps);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
            let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if name.to_lowercase().contains("uninstall") {
                continue;
            }
            apps.push(App {
                id: path.to_string_lossy().into_owned(),
                exec: format!("start \"\" \"{}\"", path.display()),
                name,
                ..Default::default()
            });
        }
    }
}
