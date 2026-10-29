//! The macOS clipboard, through NSPasteboard (`pbcopy` guesses the text encoding from the
//! locale, which apps started from Finder don't have).

use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;

pub fn copy(text: &str) {
    let board = NSPasteboard::generalPasteboard();
    board.clearContents();
    // SAFETY: AppKit's pasteboard type names are immutable statics.
    board.setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString });
}
