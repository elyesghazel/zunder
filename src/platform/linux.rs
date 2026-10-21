//! Linux with Hyprland: layer-shell overlay, hyprctl, grim, XDG desktop files, Hyprland binds.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use gtk4 as gtk;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use super::{SystemCommand, pipe_to, spawn};
use crate::providers::apps::App;
use crate::providers::windows::Client;
use crate::store::{Bind, bind_key};

/// A full-screen layer surface above everything, with the keyboard to itself.
pub fn init_overlay(window: &gtk::ApplicationWindow) {
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_namespace(Some("zunder"));
    window.set_keyboard_mode(KeyboardMode::Exclusive);
    window.set_exclusive_zone(-1);
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }
}

/// Called once at startup, before GTK.
pub fn init() {}

/// Called right after the launcher is shown.
pub fn after_show(_window: &gtk::ApplicationWindow) {}

/// The overlay covers the screen, so clicks outside the panel close it.
pub const FULL_SCREEN_OVERLAY: bool = true;

// ── windows ─────────────────────────────────────────────

/// The window list, kept up to date by a thread listening to Hyprland's event socket, so opening
/// zunder doesn't have to wait for `hyprctl` (10–25 ms). Falls back to asking directly.
pub fn list_windows() -> Vec<Client> {
    use std::sync::atomic::Ordering;
    if !WATCHING.swap(true, Ordering::SeqCst) {
        std::thread::spawn(watch_windows);
    }
    if let Some(cached) = WINDOWS.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return cached;
    }
    let fresh = fetch_windows();
    *WINDOWS.lock().unwrap_or_else(|e| e.into_inner()) = Some(fresh.clone());
    fresh
}

static WINDOWS: std::sync::Mutex<Option<Vec<Client>>> = std::sync::Mutex::new(None);
static WATCHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn fetch_windows() -> Vec<Client> {
    let Ok(out) = Command::new("hyprctl").args(["clients", "-j"]).output() else {
        return Vec::new();
    };
    serde_json::from_slice(&out.stdout).unwrap_or_default()
}

/// Refresh the cached list on every event that can change it. When the socket goes away, the
/// cache is dropped and the next `list_windows` asks hyprctl and starts listening again.
fn watch_windows() {
    use std::io::{BufRead, BufReader};
    use std::sync::atomic::Ordering;
    const EVENTS: &[&str] = &[
        "openwindow", "closewindow", "activewindow", "movewindow", "windowtitle", "fullscreen",
        "changefloatingmode", "minimized", "workspace", "focusedmon", "movetoworkspace",
    ];
    let socket = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").and_then(|sig| {
        let dir = dirs::runtime_dir()?.join("hypr").join(sig);
        std::os::unix::net::UnixStream::connect(dir.join(".socket2.sock")).ok()
    });
    if let Some(socket) = socket {
        for line in BufReader::new(socket).lines().map_while(Result::ok) {
            let event = line.split(">>").next().unwrap_or("");
            if EVENTS.iter().any(|e| event.starts_with(e)) {
                let fresh = fetch_windows();
                *WINDOWS.lock().unwrap_or_else(|e| e.into_inner()) = Some(fresh);
            }
        }
    }
    *WINDOWS.lock().unwrap_or_else(|e| e.into_inner()) = None;
    WATCHING.store(false, Ordering::SeqCst);
}

pub fn focus_window(address: &str) {
    // Hyprland's Lua config: `hyprctl dispatch` takes an hl.dsp.* expression
    let lua = format!("hl.dsp.focus({{ window = \"address:{address}\" }})");
    spawn(&["hyprctl", "dispatch", &lua]);
}

/// `grim -T` captures any window, even on another workspace.
pub fn capture_window(stable_id: &str, path: &Path) -> bool {
    Command::new("grim")
        .args(["-T", stable_id, "-s", "0.5", "-t", "jpeg", "-q", "92"])
        .arg(path)
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

// ── actions ─────────────────────────────────────────────

pub fn open(target: &str) {
    spawn(&["xdg-open", target]);
}

pub fn copy(text: &str) {
    pipe_to("wl-copy", &[], text);
}

pub fn run_in_terminal(cmd: &str) {
    let held = format!("{cmd}; printf '\\n\\033[2m[exit %s] enter to close\\033[0m' $?; read _");
    spawn(&[&crate::config::terminal(), "-e", "sh", "-c", &held]);
}

/// Command line for an app that wants a terminal (`Terminal=true`).
pub fn terminal_command(exec: &str) -> String {
    format!("{} -e {exec}", crate::config::terminal())
}

pub fn system_commands() -> &'static [SystemCommand] {
    const C: &[SystemCommand] = &[
        SystemCommand { title: "Lock Screen", keywords: "lock", glyph: "\u{f023}", command: "loginctl lock-session" },
        SystemCommand { title: "Suspend", keywords: "sleep", glyph: "\u{f0904}", command: "systemctl suspend" },
        SystemCommand { title: "Reboot", keywords: "restart", glyph: "\u{f021}", command: "systemctl reboot" },
        SystemCommand { title: "Shut Down", keywords: "power off poweroff", glyph: "⏻", command: "systemctl poweroff" },
        SystemCommand {
            title: "Log Out",
            keywords: "exit logout session",
            glyph: "\u{f0343}",
            command: "loginctl terminate-session \"$XDG_SESSION_ID\"",
        },
    ];
    C
}

pub fn is_executable(meta: &fs::Metadata, _path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

// ── global shortcuts ────────────────────────────────────

/// Lua configs get ~/.config/hypr/zunder.lua (`pcall(require, "zunder")`),
/// classic ones ~/.config/hypr/zunder.conf (`source = ~/.config/hypr/zunder.conf`).
/// Then Hyprland reloads.
pub fn install_binds(binds: &[Bind]) -> Result<String, String> {
    let hypr = dirs::config_dir().unwrap_or_default().join("hypr");
    let lua = hypr.join("hyprland.lua").exists();
    let mut out = String::from(if lua {
        "-- generated by zunder from ~/.config/zunder/binds.json, set binds with Ctrl+B in zunder\n"
    } else {
        "# generated by zunder from ~/.config/zunder/binds.json, set binds with Ctrl+B in zunder\n"
    });
    for b in binds {
        let cmd = format!("zunder run {}", bind_key(&b.item.id));
        if lua {
            out += &format!("hl.bind(\"{}\", hl.dsp.exec_cmd(\"{cmd}\")) -- {}\n", b.combo, b.item.title.replace('\n', " "));
        } else {
            let (mods, key) = b.combo.rsplit_once(" + ").unwrap_or(("", &b.combo));
            out += &format!("bind = {}, {key}, exec, {cmd} # {}\n", mods.replace(" + ", " "), b.item.title.replace('\n', " "));
        }
    }
    let path = hypr.join(if lua { "zunder.lua" } else { "zunder.conf" });
    fs::write(&path, out).map_err(|e| format!("can't write {}: {e}", path.display()))?;
    let reload = Command::new("hyprctl").arg("reload").output().map_err(|e| e.to_string())?;
    if !reload.status.success() {
        return Err(String::from_utf8_lossy(&reload.stdout).trim().to_owned());
    }
    Ok(path.display().to_string())
}

// ── apps: XDG desktop files ─────────────────────────────

fn data_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = dirs::data_dir().into_iter().collect();
    let sys = std::env::var("XDG_DATA_DIRS").ok().filter(|s| !s.is_empty());
    dirs.extend(sys.as_deref().unwrap_or("/usr/local/share:/usr/share").split(':').map(PathBuf::from));
    dirs
}

/// Folders whose change means the app list needs a rescan.
pub fn app_dirs() -> Vec<PathBuf> {
    data_dirs().into_iter().map(|d| d.join("applications")).collect()
}

pub fn scan_apps() -> Vec<App> {
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in data_dirs() {
        let root = dir.join("applications");
        scan(&root, &root, &mut seen, &mut apps);
    }
    apps
}

fn scan(root: &Path, dir: &Path, seen: &mut HashSet<String>, apps: &mut Vec<App>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan(root, &path, seen, apps);
            continue;
        }
        if path.extension().is_none_or(|e| e != "desktop") {
            continue;
        }
        // desktop file id: path relative to applications/, '/' → '-'; first one found wins
        let id = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('/', "-");
        if seen.insert(id.clone())
            && let Some(app) = parse(&path, id)
        {
            apps.push(app);
        }
    }
}

fn parse(path: &Path, id: String) -> Option<App> {
    let text = fs::read_to_string(path).ok()?;
    let mut fields: HashMap<&str, &str> = HashMap::new();
    let mut in_entry = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry
            && !line.starts_with('#')
            && let Some((k, v)) = line.split_once('=')
        {
            let k = k.trim();
            if !k.contains('[') {
                fields.entry(k).or_insert(v.trim());
            }
        }
    }
    let flag = |k: &str| fields.get(k).is_some_and(|v| *v == "true");
    if fields.get("Type") != Some(&"Application") || flag("NoDisplay") || flag("Hidden") {
        return None;
    }
    let get = |k: &str| fields.get(k).copied().unwrap_or_default().to_owned();
    Some(App {
        id,
        name: fields.get("Name")?.to_string(),
        generic: get("GenericName"),
        comment: get("Comment"),
        keywords: get("Keywords").replace(';', " "),
        exec: clean_exec(fields.get("Exec")?),
        icon: get("Icon"),
        wm_class: get("StartupWMClass"),
        categories: get("Categories"),
        terminal: flag("Terminal"),
    })
}

/// Drop the `%f %U …` field codes from an Exec line.
fn clean_exec(exec: &str) -> String {
    let mut out = String::with_capacity(exec.len());
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            if let Some('%') = chars.next() {
                out.push('%');
            }
        } else {
            out.push(c);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::clean_exec;

    #[test]
    fn strips_field_codes() {
        assert_eq!(clean_exec("firefox %u"), "firefox");
        assert_eq!(clean_exec("app --flag %F --x"), "app --flag --x");
        assert_eq!(clean_exec("printf 100%%"), "printf 100%");
    }
}
