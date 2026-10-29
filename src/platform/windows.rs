//! Windows. Apps (Start menu shortcuts), actions, system commands, global shortcuts and the tray
//! work. No ALT+TAB switcher: Windows has its own.

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

/// Enter on the "new version" notice: fill in `update install`, whose pending row stands for
/// the download until `install_update` is done.
pub fn update_action() -> crate::item::Action {
    crate::item::Action::Fill(crate::providers::update::INSTALL.into())
}

/// Download the newest installer and start it. No PowerShell: a hidden download-and-run script
/// is what Defender's attack surface reduction rules block on company laptops. curl.exe ships
/// with Windows 10 and later; the installer starts like a double-clicked program, shows its
/// progress, stops this zunder, updates it in place and starts the daemon again.
pub fn install_update() -> Result<(), String> {
    const URL: &str = "https://github.com/elyesghazel/zunder/releases/latest/download/zunder-windows-setup.exe";
    let root = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    let setup = std::env::temp_dir().join("zunder-windows-setup.exe");
    let out = super::no_window(&mut std::process::Command::new(root.join(r"System32\curl.exe")))
        .args(["-fsSL", "--connect-timeout", "15", "-m", "600", "-A", concat!("zunder/", env!("CARGO_PKG_VERSION")), "-o"])
        .arg(&setup)
        .arg(URL)
        .output()
        .map_err(|e| format!("couldn't run curl: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.trim().trim_start_matches("curl: ").to_owned());
    }
    if !shell_execute(&setup.to_string_lossy(), Some("/SILENT /SUPPRESSMSGBOXES /NORESTART")) {
        return Err("couldn't start the installer".into());
    }
    Ok(())
}

pub fn update_hint() -> &'static str {
    "Enter updates now"
}

/// Startup: make the bundled Nerd Font available. On Windows the font is called
/// "JetBrainsMono NF", which the CSS lists next to its Linux name.
pub fn init() {
    install_fonts();
    std::thread::spawn(hotkey_loop);
}

/// The hotkey thread's id, so `install_binds` can wake it up.
static HOTKEY_THREAD: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
/// New binds for the hotkey thread, with where to send back what happened.
type Request = (Vec<Bind>, std::sync::mpsc::Sender<Result<String, String>>);
static REQUEST: std::sync::Mutex<Option<Request>> = std::sync::Mutex::new(None);
/// Sent to the hotkey thread: register the binds in `REQUEST`.
const WM_RELOAD_BINDS: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WM_APP + 1;
/// Hotkey id of ALT+SPACE; the binds from Ctrl+B come after it.
const TOGGLE: i32 = 1;

/// The global shortcuts: ALT+SPACE opens zunder from anywhere, like Raycast or PowerToys Run,
/// and every Ctrl+B bind runs its item. Hotkeys belong to the thread that registers them, so
/// they all live on this one with its own message loop; a press goes to zunder through its socket.
fn hotkey_loop() {
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_NOREPEAT, RegisterHotKey, VK_SPACE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};
    // SAFETY: registering for this thread (null window) and pumping its own message queue.
    unsafe {
        if RegisterHotKey(std::ptr::null_mut(), TOGGLE, MOD_ALT | MOD_NOREPEAT, u32::from(VK_SPACE)) == 0 {
            eprintln!("zunder: ALT+SPACE is taken by another program; use Ctrl+Alt+Space");
        }
        // the queue exists now (RegisterHotKey made it), so posted messages can't get lost
        let _ = HOTKEY_THREAD.set(GetCurrentThreadId());
        let mut keys = Vec::new();
        if let Err(e) = register_binds(&crate::store::Binds::load().list, &mut keys) {
            eprintln!("zunder: {e}");
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            match msg.message {
                WM_HOTKEY if msg.wParam == TOGGLE as usize => {
                    let _ = crate::ipc::send("toggle");
                }
                WM_HOTKEY => {
                    if let Some(key) = (msg.wParam).checked_sub(TOGGLE as usize + 1).and_then(|i| keys.get(i)) {
                        let _ = crate::ipc::send(&format!("run {key}"));
                    }
                }
                WM_RELOAD_BINDS => {
                    if let Some((binds, reply)) = REQUEST.lock().ok().and_then(|mut r| r.take()) {
                        let _ = reply.send(register_binds(&binds, &mut keys));
                    }
                }
                _ => {}
            }
        }
    }
}

/// Swap the registered binds for `binds`. `keys[i]` is the bind key behind hotkey id `TOGGLE + 1 + i`.
/// Runs on the hotkey thread only.
fn register_binds(binds: &[Bind], keys: &mut Vec<String>) -> Result<String, String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey};
    // SAFETY: thread hotkeys (null window) registered and released on this same thread.
    unsafe {
        for i in 0..keys.len() {
            UnregisterHotKey(std::ptr::null_mut(), TOGGLE + 1 + i as i32);
        }
    }
    keys.clear();
    let mut failed = Vec::new();
    for b in binds {
        let id = TOGGLE + 1 + keys.len() as i32;
        let ok = hotkey(&b.combo)
            // SAFETY: as above
            .is_some_and(|(mods, vk)| unsafe { RegisterHotKey(std::ptr::null_mut(), id, mods | MOD_NOREPEAT, vk) } != 0);
        if ok {
            keys.push(crate::store::bind_key(&b.item.id));
        } else {
            failed.push(b.combo.as_str());
        }
    }
    match failed.as_slice() {
        [] => Ok(String::new()),
        [one] => Err(format!("{one} is taken by another program")),
        many => Err(format!("{} are taken by other programs", many.join(", "))),
    }
}

/// `CTRL + SHIFT + F` (the combo zunder records) → `RegisterHotKey`'s modifiers and virtual key.
fn hotkey(combo: &str) -> Option<(u32, u32)> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    let mut parts: Vec<&str> = combo.split(" + ").collect();
    let key = parts.pop()?;
    let mut mods = 0;
    for m in parts {
        mods |= match m {
            "SUPER" => MOD_WIN,
            "CTRL" => MOD_CONTROL,
            "ALT" => MOD_ALT,
            "SHIFT" => MOD_SHIFT,
            _ => return None,
        };
    }
    let mut chars = key.chars();
    let vk = match (chars.next(), chars.next()) {
        // letters are recorded upper case, which is also their virtual key
        (Some(c @ ('A'..='Z' | '0'..='9')), None) => c as u16,
        _ => match key {
            "Return" => VK_RETURN,
            "space" => VK_SPACE,
            "Tab" => VK_TAB,
            "Escape" => VK_ESCAPE,
            "BackSpace" => VK_BACK,
            "Delete" => VK_DELETE,
            "Insert" => VK_INSERT,
            "Home" => VK_HOME,
            "End" => VK_END,
            "Page_Up" | "Prior" => VK_PRIOR,
            "Page_Down" | "Next" => VK_NEXT,
            "Left" => VK_LEFT,
            "Right" => VK_RIGHT,
            "Up" => VK_UP,
            "Down" => VK_DOWN,
            "comma" => VK_OEM_COMMA,
            "period" => VK_OEM_PERIOD,
            "minus" => VK_OEM_MINUS,
            "equal" | "plus" => VK_OEM_PLUS,
            "semicolon" => VK_OEM_1,
            "slash" => VK_OEM_2,
            "grave" => VK_OEM_3,
            "bracketleft" => VK_OEM_4,
            "backslash" => VK_OEM_5,
            "bracketright" => VK_OEM_6,
            "apostrophe" => VK_OEM_7,
            f => VK_F1 + f.strip_prefix('F')?.parse::<u16>().ok().filter(|n| (1..=24).contains(n))? - 1,
        },
    };
    Some((mods, u32::from(vk)))
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

pub fn window_op(_id: &str, _op: &crate::item::WindowOp) {}

pub fn capture_window(_id: &str, _path: &Path) -> bool {
    false
}

/// Files, folders, URLs and Start-menu shortcuts, the way Explorer opens them.
pub fn open(target: &str) {
    shell_execute(target, None);
}

/// `ShellExecuteW` with the "open" verb and optional command line parameters. False when
/// Windows couldn't start it.
fn shell_execute(target: &str, params: Option<&str>) -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (verb, file, params) = (wide("open"), wide(target), params.map(wide));
    let params = params.as_ref().map_or(std::ptr::null(), |p| p.as_ptr());
    // SAFETY: null-terminated UTF-16 strings that outlive the call; no parent window.
    let code = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), params, std::ptr::null(), SW_SHOWNORMAL) };
    // anything up to 32 is an error code
    code as isize > 32
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

/// Hands the binds to the hotkey thread (only it can register them) and waits for the result.
pub fn install_binds(binds: &[Bind]) -> Result<String, String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW;
    let thread = *HOTKEY_THREAD.get().ok_or("the shortcut thread isn't running")?;
    let (reply, result) = std::sync::mpsc::channel();
    *REQUEST.lock().map_err(|e| e.to_string())? = Some((binds.to_vec(), reply));
    // SAFETY: posting a plain message without pointers to a thread of this process.
    if unsafe { PostThreadMessageW(thread, WM_RELOAD_BINDS, 0, 0) } == 0 {
        return Err("the shortcut thread isn't running".into());
    }
    result.recv_timeout(std::time::Duration::from_secs(2)).map_err(|_| "the shortcut thread didn't answer".to_owned())?
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
    use windows_sys::Win32::UI::Shell::{
        ExtractIconExW, SHFILEINFOW, SHGFI_ICON, SHGFI_ICONLOCATION, SHGFI_LARGEICON, SHGetFileInfoW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};

    let modified = fs::metadata(lnk).and_then(|m| m.modified()).ok()?;
    let stamp = modified.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // "v3": icons from before the arrow-free lookup get extracted again
    let key = crate::store::bind_key(&format!("v3{}{stamp}", lnk.display()));
    let dir = dirs::data_local_dir()?.join("zunder").join("icons");
    let png = dir.join(format!("{key}.png"));
    if png.exists() {
        return Some(png.to_string_lossy().into_owned());
    }
    let _ = fs::create_dir_all(&dir);
    let wide: Vec<u16> = lnk.as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
    let size = std::mem::size_of::<SHFILEINFOW>() as u32;
    // SAFETY: SHFILEINFOW is plain data the calls fill in; every icon handle we get is destroyed.
    unsafe {
        // the shortcut's own icon location (usually the program's .exe): the plain icon, without
        // the arrow Windows draws on shortcuts
        let mut info: SHFILEINFOW = std::mem::zeroed();
        let mut icon: HICON = std::ptr::null_mut();
        // best: the program the shortcut points to (firefox.exe…); a plain file has no arrow
        if let Some(target) = lnk_target(lnk).filter(|t| t.exists()) {
            let target: Vec<u16> = target.as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
            if SHGetFileInfoW(target.as_ptr(), 0, &mut info, size, SHGFI_ICON | SHGFI_LARGEICON) != 0 {
                icon = info.hIcon;
            }
        }
        // next: an icon the shortcut names explicitly
        if icon.is_null()
            && SHGetFileInfoW(wide.as_ptr(), 0, &mut info, size, SHGFI_ICONLOCATION) != 0
            && info.szDisplayName[0] != 0
        {
            ExtractIconExW(info.szDisplayName.as_ptr(), info.iIcon, &mut icon, std::ptr::null_mut(), 1);
        }
        // no icon file (some Store apps): the shortcut's icon, arrow and all
        if icon.is_null() {
            let mut info: SHFILEINFOW = std::mem::zeroed();
            if SHGetFileInfoW(wide.as_ptr(), 0, &mut info, size, SHGFI_ICON | SHGFI_LARGEICON) == 0 {
                return None;
            }
            icon = info.hIcon;
        }
        if icon.is_null() {
            return None;
        }
        let saved = save_icon(icon, &png);
        DestroyIcon(icon);
        saved.then(|| png.to_string_lossy().into_owned())
    }
}

/// The path a shortcut points to, read from the .lnk's LinkInfo (no COM needed). `None` for
/// shortcuts without one, like Store apps.
fn lnk_target(lnk: &Path) -> Option<PathBuf> {
    let link = lnk::ShellLink::open(lnk, lnk::encoding::WINDOWS_1252).ok()?;
    let info = link.link_info().as_ref()?;
    let base = info.local_base_path_unicode().as_deref().or(info.local_base_path())?;
    let suffix = info.common_path_suffix_unicode().as_deref().unwrap_or(info.common_path_suffix());
    let mut path = base.to_string();
    if !suffix.is_empty() {
        if !path.ends_with('\\') {
            path.push('\\');
        }
        path.push_str(suffix);
    }
    Some(PathBuf::from(path))
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
                // opened with ShellExecute (see `open`), not through cmd
                exec: path.to_string_lossy().into_owned(),
                name,
                ..Default::default()
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::hotkey;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

    #[test]
    fn turns_combos_into_hotkeys() {
        assert_eq!(hotkey("CTRL + SHIFT + F"), Some((MOD_CONTROL | MOD_SHIFT, u32::from(b'F'))));
        assert_eq!(hotkey("SUPER + 1"), Some((MOD_WIN, u32::from(b'1'))));
        assert_eq!(hotkey("ALT + Return"), Some((MOD_ALT, u32::from(VK_RETURN))));
        assert_eq!(hotkey("F5"), Some((0, u32::from(VK_F5))));
        assert_eq!(hotkey("CTRL + F24"), Some((MOD_CONTROL, u32::from(VK_F24))));
        assert_eq!(hotkey("CTRL + F25"), None);
        assert_eq!(hotkey("CTRL + dead_acute"), None);
    }
}
