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

/// Startup: make the bundled Nerd Font available. On Windows the font is called
/// "JetBrainsMono NF", which the CSS lists next to its Linux name.
pub fn init() {
    install_fonts();
}

/// The unpacked folder: `<prefix>\bin\zunder.exe`.
fn prefix() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.parent()?.to_path_buf())
}

/// Windows lets GTK put the window anywhere: center it on the monitor under the mouse, keep it
/// above other windows and give it the keyboard. Runs again a moment later, after GTK has
/// settled the final size (the preview column changes the width).
pub fn after_show(window: &gtk::ApplicationWindow) {
    let w = window.clone();
    gtk4::glib::idle_add_local_once(move || place(&w));
    // the window grows when the preview column opens: center again on every size change
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
    let Some(bundled) = prefix().map(|p| p.join("share").join("fonts")) else { return };
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

/// Shortcuts (`.lnk`) in the user's and the shared Start menu, with their icons.
pub fn scan_apps() -> Vec<App> {
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    // SHGetFileInfo resolves shortcuts through the shell, which wants COM on this thread
    // SAFETY: initializing COM is allowed repeatedly; extra calls just return S_FALSE.
    unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
    let mut apps = Vec::new();
    for root in app_dirs() {
        walk(&root, &mut apps);
    }
    for app in &mut apps {
        app.icon = icon_png(Path::new(&app.id)).unwrap_or_default();
    }
    apps
}

/// The shortcut's icon as a PNG in %LOCALAPPDATA%\zunder\icons, extracted once and reused until
/// the shortcut changes.
fn icon_png(lnk: &Path) -> Option<String> {
    use windows_sys::Win32::UI::Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW};
    use windows_sys::Win32::UI::WindowsAndMessaging::DestroyIcon;

    let modified = fs::metadata(lnk).and_then(|m| m.modified()).ok()?;
    let stamp = modified.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let key = crate::store::bind_key(&format!("{}{stamp}", lnk.display()));
    let dir = dirs::data_local_dir()?.join("zunder").join("icons");
    let png = dir.join(format!("{key}.png"));
    if png.exists() {
        return Some(png.to_string_lossy().into_owned());
    }
    let _ = fs::create_dir_all(&dir);
    let wide: Vec<u16> = lnk.as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
    // SAFETY: SHFILEINFOW is plain data the call fills in; the icon it returns is ours to destroy.
    unsafe {
        let mut info: SHFILEINFOW = std::mem::zeroed();
        let ok = SHGetFileInfoW(wide.as_ptr(), 0, &mut info, std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_ICON | SHGFI_LARGEICON);
        if ok == 0 || info.hIcon.is_null() {
            return None;
        }
        let saved = save_icon(info.hIcon, &png);
        DestroyIcon(info.hIcon);
        saved.then(|| png.to_string_lossy().into_owned())
    }
}

/// HICON → RGBA → PNG. Icons without an alpha channel get it from their mask.
///
/// # Safety
/// `icon` must be a valid icon handle.
unsafe fn save_icon(icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON, png: &Path) -> bool {
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDIBits,
        GetObjectW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetIconInfo, ICONINFO};

    // SAFETY: the caller passes a valid icon; every GDI object created here is released below.
    unsafe {
        let mut info: ICONINFO = std::mem::zeroed();
        if GetIconInfo(icon, &mut info) == 0 {
            return false;
        }
        let (color, mask) = (info.hbmColor, info.hbmMask);
        let release = || {
            if !color.is_null() {
                DeleteObject(color);
            }
            if !mask.is_null() {
                DeleteObject(mask);
            }
        };
        if color.is_null() {
            release();
            return false;
        }
        let mut bm: BITMAP = std::mem::zeroed();
        GetObjectW(color, std::mem::size_of::<BITMAP>() as i32, (&mut bm as *mut BITMAP).cast());
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        if w <= 0 || h <= 0 {
            release();
            return false;
        }
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = w;
        bi.bmiHeader.biHeight = -h; // top-down rows
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB;
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        GetDIBits(dc, color, 0, h as u32, pixels.as_mut_ptr().cast(), &mut bi, DIB_RGB_COLORS);
        if pixels.chunks(4).all(|p| p[3] == 0) && !mask.is_null() {
            // old-style icon: opaque wherever the mask is black
            let mut bits = vec![0u8; (w * h * 4) as usize];
            GetDIBits(dc, mask, 0, h as u32, bits.as_mut_ptr().cast(), &mut bi, DIB_RGB_COLORS);
            for (p, m) in pixels.chunks_mut(4).zip(bits.chunks(4)) {
                p[3] = if m[0] == 0 { 255 } else { 0 };
            }
        }
        DeleteDC(dc);
        release();
        for p in pixels.chunks_mut(4) {
            p.swap(0, 2); // BGRA → RGBA
        }
        let pixbuf = gtk4::gdk_pixbuf::Pixbuf::from_mut_slice(pixels, gtk4::gdk_pixbuf::Colorspace::Rgb, true, 8, w, h, w * 4);
        pixbuf.savev(png, "png", &[]).is_ok()
    }
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
