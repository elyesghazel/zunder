//! macOS. Apps, actions and system commands work; the window switcher, previews and global
//! shortcuts are still to come (Accessibility API, ScreenCaptureKit, hotkey registration).

use std::fs;
use std::path::Path;

use gtk4 as gtk;
use gtk4::prelude::*;

use super::{SystemCommand, pipe_to, spawn};
use crate::providers::apps::App;
use crate::providers::windows::Client;
use crate::store::Bind;

/// No layer shell on macOS: a borderless window that the window manager centers.
pub fn init_overlay(window: &gtk::ApplicationWindow) {
    window.set_decorated(false);
    window.set_resizable(false);
}

/// No self-update on macOS yet: Enter opens the release page with the new dmg.
pub fn update_action() -> crate::item::Action {
    crate::item::Action::Open("https://github.com/elyesghazel/zunder/releases/latest".into())
}

pub fn update_hint() -> &'static str {
    "Enter opens the download"
}

/// Called once at startup, before GTK.
pub fn init() {}

/// Called right after the launcher is shown.
pub fn after_show(_window: &gtk::ApplicationWindow) {}

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
    spawn(&["open", target]);
}

pub fn copy(text: &str) {
    pipe_to("pbcopy", &[], text);
}

pub fn run_in_terminal(cmd: &str) {
    let script = format!(
        "tell application \"Terminal\" to do script \"{}\"",
        cmd.replace('\\', "\\\\").replace('"', "\\\"")
    );
    spawn(&["osascript", "-e", &script, "-e", "tell application \"Terminal\" to activate"]);
}

pub fn terminal_command(exec: &str) -> String {
    exec.to_owned()
}

pub fn system_commands() -> &'static [SystemCommand] {
    const C: &[SystemCommand] = &[
        SystemCommand { title: "Lock Screen", keywords: "lock", glyph: "\u{f023}", command: "pmset displaysleepnow" },
        SystemCommand { title: "Sleep", keywords: "suspend", glyph: "\u{f0904}", command: "pmset sleepnow" },
        SystemCommand {
            title: "Restart",
            keywords: "reboot",
            glyph: "\u{f021}",
            command: "osascript -e 'tell application \"System Events\" to restart'",
        },
        SystemCommand {
            title: "Shut Down",
            keywords: "power off",
            glyph: "⏻",
            command: "osascript -e 'tell application \"System Events\" to shut down'",
        },
        SystemCommand {
            title: "Log Out",
            keywords: "exit logout",
            glyph: "\u{f0343}",
            command: "osascript -e 'tell application \"System Events\" to log out'",
        },
    ];
    C
}

pub fn is_executable(meta: &fs::Metadata, _path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

pub fn install_binds(_binds: &[Bind]) -> Result<String, String> {
    Err("global shortcuts aren't supported on macOS yet".into())
}

/// Folders whose change means the app list needs a rescan.
pub fn app_dirs() -> Vec<std::path::PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    vec![
        Path::new("/Applications").to_path_buf(),
        Path::new("/Applications/Utilities").to_path_buf(),
        Path::new("/System/Applications").to_path_buf(),
        Path::new("/System/Applications/Utilities").to_path_buf(),
        home.join("Applications"),
    ]
}

/// `.app` bundles in /Applications, /System/Applications and ~/Applications.
pub fn scan_apps() -> Vec<App> {
    let mut apps = Vec::new();
    for root in app_dirs() {
        let Ok(entries) = fs::read_dir(&root) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "app") {
                continue;
            }
            let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            apps.push(App {
                id: path.to_string_lossy().into_owned(),
                exec: format!("open -a {}", crate::item::shell_quote(&path.to_string_lossy())),
                wm_class: name.clone(),
                name,
                ..Default::default()
            });
        }
    }
    apps
}
