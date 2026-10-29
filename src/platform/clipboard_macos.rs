//! The macOS clipboard: the history watcher, and writes through NSPasteboard (`pbcopy` guesses
//! the text encoding from the locale, which apps started from Finder don't have).

use std::path::Path;
use std::time::Duration;

use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeString, NSPasteboardTypeTIFF,
};
use objc2_foundation::{NSData, NSDictionary, NSString};

/// Types password managers put next to what they copy (nspasteboard.org), so history tools skip it.
const PRIVATE: &[&str] = &["org.nspasteboard.ConcealedType", "org.nspasteboard.TransientType"];

/// Runs on its own thread for as long as zunder does. macOS doesn't announce clipboard changes,
/// so this checks the pasteboard's change count twice a second, which costs next to nothing.
pub fn watch() {
    let mut seen = None;
    loop {
        objc2::rc::autoreleasepool(|_| {
            let board = NSPasteboard::generalPasteboard();
            let count = board.changeCount();
            if seen != Some(count) {
                seen = Some(count);
                record(&board);
            }
        });
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Adds what is on the pasteboard to the history: its text, else its image.
fn record(board: &NSPasteboard) {
    if !crate::config::get().clipboard_history {
        return;
    }
    let Some(types) = board.types() else { return };
    if PRIVATE.iter().any(|t| types.containsObject(&NSString::from_str(t))) {
        return;
    }
    // SAFETY: AppKit's pasteboard type names are immutable statics.
    let (string, png, tiff) = unsafe { (NSPasteboardTypeString, NSPasteboardTypePNG, NSPasteboardTypeTIFF) };
    if let Some(text) = board.stringForType(string).map(|s| s.to_string()).filter(|t| !t.trim().is_empty()) {
        crate::clips::add_text(&text);
        return;
    }
    let (data, is_png) = match board.dataForType(png) {
        Some(data) => (data, true),
        None => match board.dataForType(tiff) {
            Some(data) => (data, false),
            None => return,
        },
    };
    crate::clips::add_image(data.to_vec(), |bytes, path| save_png(&bytes, is_png, path));
}

/// PNG or TIFF pasteboard data → a PNG file. Returns the size in pixels.
fn save_png(bytes: &[u8], is_png: bool, path: &Path) -> Option<(u32, u32)> {
    let data = NSData::with_bytes(bytes);
    let rep = NSBitmapImageRep::imageRepWithData(&data)?;
    let size = (u32::try_from(rep.pixelsWide()).ok()?, u32::try_from(rep.pixelsHigh()).ok()?);
    let png = if is_png {
        bytes.to_vec()
    } else {
        // SAFETY: an empty dictionary is a valid set of properties.
        unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }?.to_vec()
    };
    std::fs::write(path, png).ok()?;
    Some(size)
}

pub fn copy(text: &str) {
    let board = NSPasteboard::generalPasteboard();
    board.clearContents();
    // SAFETY: as in `record`.
    board.setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString });
}

pub fn copy_image(path: &str) {
    let Ok(png) = std::fs::read(path) else { return };
    let board = NSPasteboard::generalPasteboard();
    board.clearContents();
    // SAFETY: as in `record`.
    board.setData_forType(Some(&NSData::with_bytes(&png)), unsafe { NSPasteboardTypePNG });
}
