//! The Windows clipboard, through the clipboard API (piping into `clip.exe` turned non-ASCII text
//! into the OEM code page).

use std::time::Duration;

use windows_sys::Win32::Foundation::{GlobalFree, HWND};
use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

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

/// Replace the clipboard with `formats`, each a format and its bytes.
fn set(formats: &[(u32, &[u8])]) {
    let Some(_open) = Open::new(std::ptr::null_mut()) else {
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
