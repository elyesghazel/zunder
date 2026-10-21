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

/// Startup: make the bundled Nerd Font available (the glyphs need it).
pub fn init() {
    install_fonts();
}

/// Windows lets GTK put the window anywhere: center it on the monitor under the mouse, keep it
/// above other windows and give it the keyboard. Runs again a moment later, after GTK has
/// settled the final size (the preview column changes the width).
pub fn after_show(window: &gtk::ApplicationWindow) {
    let w = window.clone();
    gtk4::glib::idle_add_local_once(move || place(&w));
    let w = window.clone();
    gtk4::glib::timeout_add_local_once(std::time::Duration::from_millis(60), move || place(&w));
}

fn place(window: &gtk::ApplicationWindow) {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetWindowRect, HWND_TOPMOST, SWP_NOSIZE, SWP_SHOWWINDOW, SetForegroundWindow, SetWindowPos,
    };

    let Some(surface) = window.surface() else { return };
    let Ok(surface) = surface.downcast::<gdk4_win32::Win32Surface>() else { return };
    let hwnd = surface.handle().0;
    // SAFETY: plain Win32 calls on our own window handle, with zeroed out-structs that the
    // functions fill in; MONITORINFO's size is set as the API requires.
    unsafe {
        let mut rect: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return;
        }
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        let mut cursor = POINT { x: 0, y: 0 };
        GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return;
        }
        let area = info.rcWork;
        let x = area.left + (area.right - area.left - w) / 2;
        let y = area.top + (area.bottom - area.top - h) / 2;
        SetWindowPos(hwnd, HWND_TOPMOST, x, y, 0, 0, SWP_NOSIZE | SWP_SHOWWINDOW);
        SetForegroundWindow(hwnd);
    }
}

/// Fonts that ship in `share\fonts` next to `bin\zunder.exe`, installed for the current user
/// only (no admin), like "Install for me" in Explorer: copied to
/// %LOCALAPPDATA%\Microsoft\Windows\Fonts and registered under HKCU, then loaded for this session.
fn install_fonts() {
    use windows_sys::Win32::Graphics::Gdi::AddFontResourceW;
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegSetKeyValueW};

    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let Some(bundled) = std::env::current_exe().ok().and_then(|e| Some(e.parent()?.parent()?.join("share").join("fonts"))) else {
        return;
    };
    let Ok(entries) = fs::read_dir(&bundled) else { return };
    let target = dirs::data_local_dir().unwrap_or_default().join("Microsoft\\Windows\\Fonts");
    let _ = fs::create_dir_all(&target);
    for entry in entries.flatten() {
        let source = entry.path();
        if source.extension().is_none_or(|e| !e.eq_ignore_ascii_case("ttf")) {
            continue;
        }
        let Some(name) = source.file_name() else { continue };
        let installed = target.join(name);
        if installed.exists() {
            continue;
        }
        if fs::copy(&source, &installed).is_err() {
            continue;
        }
        let path = installed.to_string_lossy().into_owned();
        let label = format!("{} (TrueType)", source.file_stem().unwrap_or_default().to_string_lossy());
        let (key, value, data) = (wide("Software\\Microsoft\\Windows NT\\CurrentVersion\\Fonts"), wide(&label), wide(&path));
        // SAFETY: null-terminated UTF-16 strings that live for the duration of the calls.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            );
            AddFontResourceW(data.as_ptr());
        }
    }
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
