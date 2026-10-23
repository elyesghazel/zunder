//! Windows tray icon in the notification area, so you can see the server runs. A hidden
//! message-only window on its own thread owns the icon; left click toggles zunder, right click
//! opens the menu, and the commands go through the same channel as the CLI's.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, HWND_MESSAGE, LR_DEFAULTCOLOR,
    MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
    SM_CXSMICON, SendMessageW, SetForegroundWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    TranslateMessage, WM_APP, WM_CLOSE, WM_DESTROY, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
};

/// The icon's mouse events arrive as this message.
const WM_TRAY: u32 = WM_APP + 1;

static TX: OnceLock<async_channel::Sender<String>> = OnceLock::new();
/// The hidden window while the icon is up.
static HWND_NOW: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
static RUNNING: AtomicBool = AtomicBool::new(false);
/// Explorer broadcasts this after it restarts; the icon has to be added again.
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

/// Called once at startup with the channel the menu sends commands to.
pub fn init_tray(tx: async_channel::Sender<String>) {
    let _ = TX.set(tx);
    sync_tray();
}

/// Show or remove the icon to match the `tray` setting.
pub fn sync_tray() {
    if crate::config::get().tray {
        if !RUNNING.swap(true, Ordering::SeqCst) {
            std::thread::spawn(tray_loop);
        }
    } else {
        stop_tray();
    }
}

/// Take the icon down now; Windows would otherwise leave a dead one until the mouse passes over it.
pub fn stop_tray() {
    let hwnd = HWND_NOW.load(Ordering::SeqCst);
    if !hwnd.is_null() {
        // SAFETY: our own window; SendMessage waits until its thread has removed the icon.
        unsafe { SendMessageW(hwnd, WM_CLOSE, 0, 0) };
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn send(cmd: &str) {
    if let Some(tx) = TX.get() {
        let _ = tx.send_blocking(cmd.to_owned());
    }
}

fn tray_loop() {
    // SAFETY: a window class and a message-only window owned by this thread, and the standard
    // message loop for it. Every struct is zeroed and then filled in as the API documents.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide("zunder-tray");
        let _ = TASKBAR_CREATED.get_or_init(|| RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()));
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(proc_);
        wc.hInstance = instance;
        wc.lpszClassName = class.as_ptr();
        // fails harmlessly when the class is still registered from an earlier run
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            RUNNING.store(false, Ordering::SeqCst);
            return;
        }
        HWND_NOW.store(hwnd, Ordering::SeqCst);
        add_icon(hwnd);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        HWND_NOW.store(std::ptr::null_mut(), Ordering::SeqCst);
        RUNNING.store(false, Ordering::SeqCst);
    }
}

fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    // SAFETY: NOTIFYICONDATAW is plain data; zeroed is its documented empty state.
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = 1;
    data
}

/// The logo at the small-icon size of this display (16 px at 100 %, 24 at 150 %, 32 at 200 %).
fn add_icon(hwnd: HWND) {
    // SAFETY: GetSystemMetrics has no preconditions; CreateIconFromResourceEx reads the PNG bytes
    // we pass with their exact length (Windows reads PNG icon resources since Vista).
    unsafe {
        let want = GetSystemMetrics(SM_CXSMICON).max(16);
        let (size, png) = super::TRAY_ICONS
            .iter()
            .find(|(s, _)| *s >= want)
            .or(super::TRAY_ICONS.last())
            .copied()
            .unwrap_or_default();
        let icon = CreateIconFromResourceEx(png.as_ptr(), png.len() as u32, 1, 0x0003_0000, size, size, LR_DEFAULTCOLOR);
        let mut data = icon_data(hwnd);
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        data.uCallbackMessage = WM_TRAY;
        data.hIcon = icon;
        for (slot, c) in data.szTip.iter_mut().zip("zunder".encode_utf16()) {
            *slot = c;
        }
        Shell_NotifyIconW(NIM_ADD, &data);
    }
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: called by Windows on the tray thread with our own window.
    unsafe {
        match msg {
            WM_TRAY => {
                match lparam as u32 & 0xffff {
                    WM_LBUTTONUP => send("toggle"),
                    WM_RBUTTONUP => menu(hwnd),
                    _ => {}
                }
                0
            }
            WM_CLOSE => {
                Shell_NotifyIconW(NIM_DELETE, &icon_data(hwnd));
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            m if Some(&m) == TASKBAR_CREATED.get() => {
                add_icon(hwnd);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// The right-click menu at the mouse, the same entries as on Linux.
unsafe fn menu(hwnd: HWND) {
    // SAFETY: a popup menu we create and destroy here; the labels outlive the TrackPopupMenu call.
    unsafe {
        let menu = CreatePopupMenu();
        let labels: Vec<Vec<u16>> = super::TRAY_MENU.iter().map(|(label, _)| wide(label)).collect();
        for (i, label) in labels.iter().enumerate() {
            if label.len() == 1 {
                AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            } else {
                AppendMenuW(menu, MF_STRING, i + 1, label.as_ptr());
            }
        }
        let mut at = POINT { x: 0, y: 0 };
        GetCursorPos(&mut at);
        // without this the menu stays open when you click elsewhere
        SetForegroundWindow(hwnd);
        let picked =
            TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, at.x, at.y, 0, hwnd, std::ptr::null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        if let Some((_, cmd)) = (picked as usize).checked_sub(1).and_then(|i| super::TRAY_MENU.get(i)) {
            send(cmd);
        }
    }
}
