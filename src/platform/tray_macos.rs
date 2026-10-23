//! macOS menu bar icon. zunder has no Dock icon (`LSUIElement`), so this is how you see the server
//! runs. Clicking it opens the menu; the entries send the same commands as the CLI.

use std::cell::RefCell;
use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{NSData, NSSize, NSString};

static TX: OnceLock<async_channel::Sender<String>> = OnceLock::new();

define_class!(
    /// Receives the menu clicks; the item's tag is its index in `TRAY_MENU`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZunderTrayTarget"]
    struct Target;

    impl Target {
        #[unsafe(method(run:))]
        fn run(&self, item: &NSMenuItem) {
            let cmd = usize::try_from(item.tag()).ok().and_then(|i| super::TRAY_MENU.get(i));
            if let (Some((_, cmd)), Some(tx)) = (cmd, TX.get()) {
                let _ = tx.send_blocking((*cmd).to_owned());
            }
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSObject's init on a freshly allocated object.
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    /// The status item and the menu's target, kept alive while the icon is up.
    static ITEM: RefCell<Option<(Retained<NSStatusItem>, Retained<Target>)>> = const { RefCell::new(None) };
}

/// Called once, on the main thread after GTK started, with the channel the menu sends to.
pub fn init_tray(tx: async_channel::Sender<String>) {
    let _ = TX.set(tx);
    sync_tray();
}

/// Show or remove the icon to match the `tray` setting.
pub fn sync_tray() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let on = crate::config::get().tray;
    ITEM.with(|slot| {
        let mut slot = slot.borrow_mut();
        match (on, slot.is_some()) {
            (true, false) => *slot = Some(add(mtm)),
            (false, true) => {
                if let Some((item, _)) = slot.take() {
                    NSStatusBar::systemStatusBar().removeStatusItem(&item);
                }
            }
            _ => {}
        }
    });
}

/// The menu bar item goes away with the process.
pub fn stop_tray() {}

fn add(mtm: MainThreadMarker) -> (Retained<NSStatusItem>, Retained<Target>) {
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    if let Some(button) = item.button(mtm) {
        // the 64 px logo, drawn at menu bar size (sharp on Retina)
        let png = super::TRAY_ICONS.last().map(|(_, png)| *png).unwrap_or_default();
        let data = NSData::with_bytes(png);
        if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
            image.setSize(NSSize::new(18.0, 18.0));
            button.setImage(Some(&image));
        }
        button.setToolTip(Some(&NSString::from_str("zunder")));
    }
    let target = Target::new(mtm);
    let menu = NSMenu::new(mtm);
    for (i, (label, _)) in super::TRAY_MENU.iter().enumerate() {
        if label.is_empty() {
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            continue;
        }
        // SAFETY: `run:` is implemented by Target with the signature AppKit calls it with.
        let entry = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(label),
                Some(sel!(run:)),
                &NSString::from_str(""),
            )
        };
        // SAFETY: the target is kept alive next to the item in ITEM.
        unsafe { entry.setTarget(Some(&target)) };
        entry.setTag(i as isize);
        menu.addItem(&entry);
    }
    item.setMenu(Some(&menu));
    (item, target)
}
