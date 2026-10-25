//! macOS. Apps, actions, system commands and the menu bar icon work; global shortcuts are still
//! to come. No ALT+TAB switcher: macOS has ⌘Tab.

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

/// Called right after the launcher is shown: center it on its screen, again on every size change
/// (the window grows with the results and the preview column). GTK can't place windows on macOS,
/// so this goes through the NSWindow behind it.
pub fn after_show(window: &gtk::ApplicationWindow) {
    let w = window.clone();
    gtk::glib::idle_add_local_once(move || place(&w));
    thread_local! {
        static HOOKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if let Some(surface) = window.surface()
        && !HOOKED.with(|h| h.replace(true))
    {
        let w = window.downgrade();
        surface.connect_layout(move |_, _, _| {
            if let Some(w) = w.upgrade() {
                place(&w);
            }
        });
    }
}

fn place(window: &gtk::ApplicationWindow) {
    use objc2_app_kit::{NSFloatingWindowLevel, NSWindow};
    use objc2_foundation::NSPoint;

    let Some(surface) = window.surface() else { return };
    let Ok(surface) = surface.downcast::<gdk4_macos::MacosSurface>() else { return };
    let native = surface.native();
    if native.is_null() {
        return;
    }
    // SAFETY: GDK hands out the NSWindow it owns for this surface; it lives as long as the
    // surface, and we're on the main thread as AppKit requires.
    let ns: &NSWindow = unsafe { &*native.cast::<NSWindow>() };
    let Some(screen) = ns.screen() else { return };
    let area = screen.visibleFrame();
    let frame = ns.frame();
    // AppKit's origin is the bottom left corner
    let x = area.origin.x + (area.size.width - frame.size.width) / 2.0;
    let y = area.origin.y + (area.size.height - frame.size.height) / 2.0;
    ns.setFrameOrigin(NSPoint::new(x.round(), y.round()));
    ns.setLevel(NSFloatingWindowLevel);
}

/// The window is only as big as the panel; it closes when it loses focus instead.
pub const FULL_SCREEN_OVERLAY: bool = false;

pub fn list_windows() -> Vec<Client> {
    Vec::new()
}

pub fn focus_window(_id: &str) {}

pub fn window_op(_id: &str, _op: &crate::item::WindowOp) {}

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

/// `.app` bundles in /Applications, /System/Applications and ~/Applications. Their icons are
/// `.icns` files GTK can't read: they're turned into PNGs in the background (see `convert_icons`)
/// and show up from the next open on.
pub fn scan_apps() -> Vec<App> {
    let cache = dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("zunder").join("icons");
    let mut apps = Vec::new();
    let mut missing = Vec::new();
    for root in app_dirs() {
        let Ok(entries) = fs::read_dir(&root) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "app") {
                continue;
            }
            let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let plist = path.join("Contents").join("Info.plist");
            let stamp = fs::metadata(&plist)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            let png = cache.join(format!("{}.png", crate::store::bind_key(&format!("{}{stamp}", path.display()))));
            if !png.exists() {
                missing.push((path.clone(), png.clone()));
            }
            apps.push(App {
                id: path.to_string_lossy().into_owned(),
                exec: format!("open -a {}", crate::item::shell_quote(&path.to_string_lossy())),
                wm_class: name.clone(),
                icon: png.to_string_lossy().into_owned(),
                name,
                ..Default::default()
            });
        }
    }
    if !missing.is_empty() {
        let _ = fs::create_dir_all(&cache);
        std::thread::spawn(move || convert_icons(missing));
    }
    apps
}

/// `.app` → its `.icns` (named in Info.plist, usually AppIcon) → a 64px PNG, with the `plutil`
/// and `sips` every Mac has.
fn convert_icons(apps: Vec<(std::path::PathBuf, std::path::PathBuf)>) {
    use std::process::{Command, Stdio};
    static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    for (app, png) in apps {
        let resources = app.join("Contents").join("Resources");
        let named = Command::new("plutil")
            .args(["-extract", "CFBundleIconFile", "raw", "-o", "-"])
            .arg(app.join("Contents").join("Info.plist"))
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .filter(|n| !n.is_empty())
            .map(|n| if n.ends_with(".icns") { n } else { format!("{n}.icns") });
        let Some(icns) = named.map(|n| resources.join(n)).into_iter().chain([resources.join("AppIcon.icns")]).find(|p| p.exists())
        else {
            continue;
        };
        let _ = Command::new("sips")
            .args(["-s", "format", "png", "-Z", "64"])
            .arg(&icns)
            .arg("--out")
            .arg(&png)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
}
