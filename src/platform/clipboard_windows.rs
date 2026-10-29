//! The Windows clipboard: the history watcher, and writes through the clipboard API (piping into
//! `clip.exe` turned non-ASCII text into the OEM code page).

use std::ffi::c_void;
use std::path::Path;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::time::Duration;

use windows_sys::Win32::Foundation::{GlobalFree, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
use windows_sys::Win32::System::Ole::{CF_DIB, CF_DIBV5, CF_UNICODETEXT};

use super::dib;

/// The watcher's message-only window. It also owns what zunder puts on the clipboard: with no
/// owner, `SetClipboardData` may fail.
static OWNER: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Runs on its own thread for as long as zunder does: Windows tells a window about every
/// clipboard change (`WM_CLIPBOARDUPDATE`), and this one records it in `crate::clips`.
pub fn watch() {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG, RegisterClassW, WNDCLASSW,
    };
    let class = wide("zunder-clipboard");
    // SAFETY: a window class and a message-only window that belong to this thread, which pumps
    // their messages until the process exits; `class` outlives every call that reads it.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW { lpfnWndProc: Some(on_message), hInstance: instance, lpszClassName: class.as_ptr(), ..std::mem::zeroed() };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            std::ptr::null(),
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
        if hwnd.is_null() || AddClipboardFormatListener(hwnd) == 0 {
            eprintln!("zunder: can't watch the clipboard, `clip` stays empty");
            return;
        }
        OWNER.store(hwnd, Ordering::Release);
        record(hwnd);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn on_message(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{DefWindowProcW, WM_CLIPBOARDUPDATE};
    if msg == WM_CLIPBOARDUPDATE {
        record(hwnd);
        return 0;
    }
    // SAFETY: the default handling, with the arguments Windows passed in.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

enum Copied {
    Text(String),
    /// A packed DIB.
    Image(Vec<u8>),
}

/// Adds what was just copied to the history.
fn record(hwnd: HWND) {
    if !crate::config::get().clipboard_history {
        return;
    }
    // read everything first: the clipboard stays locked for other programs while it's open
    let copied = {
        let Some(_open) = Open::new(hwnd) else { return };
        if private() {
            return;
        }
        match data(CF_UNICODETEXT.into()).map(|bytes| utf16(&bytes)).filter(|t| !t.trim().is_empty()) {
            Some(text) => Copied::Text(text),
            None => match data(CF_DIBV5.into()).or_else(|| data(CF_DIB.into())) {
                Some(dib) => Copied::Image(dib),
                None => return,
            },
        }
    };
    match copied {
        Copied::Text(text) => crate::clips::add_text(&text),
        Copied::Image(bytes) => {
            let Some((w, h, rgba)) = dib::to_rgba(&bytes) else { return };
            crate::clips::add_image(rgba, |rgba, path| {
                let pixbuf = gtk4::gdk_pixbuf::Pixbuf::from_mut_slice(rgba, gtk4::gdk_pixbuf::Colorspace::Rgb, true, 8, w, h, w * 4);
                pixbuf.savev(path, "png", &[]).ok().map(|()| (w as u32, h as u32))
            });
        }
    }
}

/// Password managers mark what they copy so clipboard history tools skip it.
fn private() -> bool {
    let format = |name: &str| {
        let name = wide(name);
        // SAFETY: a null-terminated UTF-16 string that outlives the call.
        unsafe { RegisterClipboardFormatW(name.as_ptr()) }
    };
    // SAFETY: only asks about formats; the clipboard is open.
    let has = |name: &str| unsafe { IsClipboardFormatAvailable(format(name)) } != 0;
    has("ExcludeClipboardContentFromMonitorProcessing")
        || has("Clipboard Viewer Ignore")
        || data(format("CanIncludeInClipboardHistory")).is_some_and(|v| v.get(..4) == Some(&[0; 4]))
}

/// The clipboard, opened for this thread and closed again on drop.
struct Open;

impl Open {
    /// Programs hold the clipboard for a moment while they write it: try for about 200 ms.
    fn new(owner: HWND) -> Option<Self> {
        for _ in 0..10 {
            // SAFETY: plain call; a null owner is allowed.
            if unsafe { OpenClipboard(owner) } != 0 {
                return Some(Open);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: this thread opened the clipboard in `Open::new`.
        unsafe { CloseClipboard() };
    }
}

/// A copy of the clipboard's data in `format`. Call with the clipboard open.
fn data(format: u32) -> Option<Vec<u8>> {
    // SAFETY: the handle belongs to the open clipboard and stays valid until it's closed; it's
    // locked while we copy out of it, and GlobalSize is the size of the locked block.
    unsafe {
        let handle = GetClipboardData(format);
        if handle.is_null() {
            return None;
        }
        let ptr = GlobalLock(handle).cast::<u8>();
        if ptr.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(ptr, GlobalSize(handle)).to_vec();
        GlobalUnlock(handle);
        Some(bytes)
    }
}

/// CF_UNICODETEXT: UTF-16, up to the first NUL.
fn utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).take_while(|&u| u != 0).collect();
    String::from_utf16_lossy(&units)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// Replace the clipboard with `formats`, each a format and its bytes.
fn set(formats: &[(u32, &[u8])]) {
    let Some(_open) = Open::new(OWNER.load(Ordering::Acquire)) else {
        eprintln!("zunder: the clipboard is busy");
        return;
    };
    // SAFETY: the clipboard is open. Each block is allocated, filled while locked and handed to
    // the clipboard, which owns it from then on; we only free it when that fails.
    unsafe {
        EmptyClipboard();
        for (format, bytes) in formats {
            let mem = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
            if mem.is_null() {
                continue;
            }
            let ptr = GlobalLock(mem);
            if ptr.is_null() {
                GlobalFree(mem);
                continue;
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
            GlobalUnlock(mem);
            if SetClipboardData(*format, mem).is_null() {
                GlobalFree(mem);
            }
        }
    }
}

pub fn copy(text: &str) {
    let bytes: Vec<u8> = text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
    set(&[(CF_UNICODETEXT.into(), &bytes)]);
}

/// The PNG as is, for programs that take it (browsers, Office), and as a bitmap for the rest.
pub fn copy_image(path: &str) {
    let Ok(png) = std::fs::read(path) else { return };
    let png_format = {
        let name = wide("PNG");
        // SAFETY: a null-terminated UTF-16 string that outlives the call.
        unsafe { RegisterClipboardFormatW(name.as_ptr()) }
    };
    match rgba(Path::new(path)) {
        Some((w, h, rgba)) => set(&[(png_format, &png), (CF_DIB.into(), &dib::from_rgba(w, h, &rgba))]),
        None => set(&[(png_format, &png)]),
    }
}

/// An image file as `(width, height, RGBA rows)`.
fn rgba(path: &Path) -> Option<(i32, i32, Vec<u8>)> {
    let pixbuf = gtk4::gdk_pixbuf::Pixbuf::from_file(path).ok()?;
    let pixbuf = if pixbuf.has_alpha() { pixbuf } else { pixbuf.add_alpha(false, 0, 0, 0).ok()? };
    let (w, h, stride) = (pixbuf.width(), pixbuf.height(), pixbuf.rowstride() as usize);
    let bytes = pixbuf.read_pixel_bytes();
    let row = w as usize * 4;
    let mut rgba = Vec::with_capacity(row * h as usize);
    for y in 0..h as usize {
        rgba.extend_from_slice(bytes.get(y * stride..y * stride + row)?);
    }
    Some((w, h, rgba))
}
