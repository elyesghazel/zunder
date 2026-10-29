//! macOS global shortcuts: Option+Space toggles zunder and every Ctrl+B bind runs its item.
//!
//! Carbon's `RegisterEventHotKey`, which needs no accessibility permission. Presses arrive as
//! Carbon events on the main thread: GDK pulls them out of the NSApplication queue and hands what
//! it doesn't know to `[NSApp sendEvent:]`, which dispatches them to the handler below. So
//! registering, unregistering and handling all happen on the main thread, no thread of our own.

use std::cell::RefCell;
use std::ffi::c_void;

use crate::store::Bind;

type OSStatus = i32;
type Ref = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyID {
    signature: u32,
    id: u32,
}

#[repr(C)]
struct EventTypeSpec {
    class: u32,
    kind: u32,
}

type Handler = extern "C" fn(call: Ref, event: Ref, user_data: *mut c_void) -> OSStatus;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> Ref;
    fn InstallEventHandler(
        target: Ref,
        handler: Handler,
        count: usize,
        types: *const EventTypeSpec,
        user_data: *mut c_void,
        out: *mut Ref,
    ) -> OSStatus;
    fn RegisterEventHotKey(code: u32, modifiers: u32, id: EventHotKeyID, target: Ref, options: u32, out: *mut Ref) -> OSStatus;
    fn UnregisterEventHotKey(hotkey: Ref) -> OSStatus;
    fn GetEventParameter(
        event: Ref,
        name: u32,
        desired_type: u32,
        actual_type: *mut u32,
        size: usize,
        actual_size: *mut usize,
        data: *mut c_void,
    ) -> OSStatus;
    fn TISCopyCurrentKeyboardLayoutInputSource() -> Ref;
    fn TISGetInputSourceProperty(source: Ref, key: *const c_void) -> Ref;
    static kTISPropertyUnicodeKeyLayoutData: *const c_void;
    fn LMGetKbdType() -> u8;
    fn UCKeyTranslate(
        layout: *const c_void,
        code: u16,
        action: u16,
        modifiers: u32,
        keyboard_type: u32,
        options: u32,
        dead_key_state: *mut u32,
        max_len: usize,
        len: *mut usize,
        chars: *mut u16,
    ) -> OSStatus;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataGetBytePtr(data: Ref) -> *const u8;
    fn CFRelease(cf: Ref);
}

const fn four_cc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

const SIGNATURE: u32 = four_cc(b"zndr");
const K_EVENT_CLASS_KEYBOARD: u32 = four_cc(b"keyb");
const K_EVENT_HOT_KEY_PRESSED: u32 = 5;
const K_EVENT_PARAM_DIRECT_OBJECT: u32 = four_cc(b"----");
const TYPE_EVENT_HOT_KEY_ID: u32 = four_cc(b"hkid");
const EVENT_NOT_HANDLED: OSStatus = -9874;
const K_UC_KEY_ACTION_DOWN: u16 = 0;
const K_UC_KEY_TRANSLATE_NO_DEAD_KEYS: u32 = 1;

const CMD: u32 = 1 << 8;
const SHIFT: u32 = 1 << 9;
const OPTION: u32 = 1 << 11;
const CONTROL: u32 = 1 << 12;
const SPACE: u32 = 0x31;

/// Hotkey id of Option+Space; the binds from Ctrl+B come after it.
const TOGGLE: u32 = 1;

thread_local! {
    /// The registered binds: `(hotkey, bind key)`, the bind with id `TOGGLE + 1 + i` at `i`.
    static BINDS: RefCell<Vec<(Ref, String)>> = const { RefCell::new(Vec::new()) };
    static STARTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// First loop iteration: the press handler, Option+Space and the saved binds.
pub fn start() {
    if objc2::MainThreadMarker::new().is_none() || STARTED.with(|s| s.replace(true)) {
        return;
    }
    let types = [EventTypeSpec { class: K_EVENT_CLASS_KEYBOARD, kind: K_EVENT_HOT_KEY_PRESSED }];
    let mut toggle = std::ptr::null_mut();
    // SAFETY: on the main thread; the handler is a plain function that lives forever, and the
    // type list is only read during the call.
    unsafe {
        let target = GetApplicationEventTarget();
        if InstallEventHandler(target, on_press, types.len(), types.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut()) != 0 {
            eprintln!("zunder: can't listen for global shortcuts");
            return;
        }
        let id = EventHotKeyID { signature: SIGNATURE, id: TOGGLE };
        if RegisterEventHotKey(SPACE, OPTION, id, target, 0, &mut toggle) != 0 {
            eprintln!("zunder: Option+Space is taken by another program; bind `zunder toggle` to a key instead");
        }
    }
    if let Err(e) = register_binds(&crate::store::Binds::load().list) {
        eprintln!("zunder: {e}");
    }
}

extern "C" fn on_press(_call: Ref, event: Ref, _user_data: *mut c_void) -> OSStatus {
    let mut id = EventHotKeyID { signature: 0, id: 0 };
    // SAFETY: `event` is the hotkey event Carbon is dispatching; we read its fixed-size id.
    let status = unsafe {
        GetEventParameter(
            event,
            K_EVENT_PARAM_DIRECT_OBJECT,
            TYPE_EVENT_HOT_KEY_ID,
            std::ptr::null_mut(),
            std::mem::size_of::<EventHotKeyID>(),
            std::ptr::null_mut(),
            (&raw mut id).cast(),
        )
    };
    if status != 0 || id.signature != SIGNATURE {
        return EVENT_NOT_HANDLED;
    }
    // through the socket like the CLI, so a press while the UI is busy just queues up
    let key = |i: u32| BINDS.with(|b| b.borrow().get(i as usize).map(|(_, k)| k.clone()));
    if id.id == TOGGLE {
        let _ = crate::ipc::send("toggle");
    } else if let Some(key) = id.id.checked_sub(TOGGLE + 1).and_then(key) {
        let _ = crate::ipc::send(&format!("run {key}"));
    }
    0
}

/// Swap the registered binds for `binds`. Main thread only, like everything Carbon.
pub fn register_binds(binds: &[Bind]) -> Result<String, String> {
    if objc2::MainThreadMarker::new().is_none() {
        return Err("global shortcuts can only be set from the main thread".into());
    }
    let layout = Layout::current();
    BINDS.with(|registered| {
        let mut registered = registered.borrow_mut();
        for (hotkey, _) in registered.drain(..) {
            // SAFETY: a hotkey we registered and haven't released yet.
            unsafe { UnregisterEventHotKey(hotkey) };
        }
        let mut failed = Vec::new();
        for b in binds {
            let id = EventHotKeyID { signature: SIGNATURE, id: TOGGLE + 1 + registered.len() as u32 };
            let mut hotkey = std::ptr::null_mut();
            let ok = hotkey_for(&b.combo).is_some_and(|(mods, key)| {
                let code = match key {
                    Key::Char(c) => layout.as_ref().and_then(|l| l.code(c)).or_else(|| ansi_code(c)),
                    Key::Code(code) => Some(code),
                };
                // SAFETY: on the main thread, registering for this application's event target.
                code.is_some_and(|code| unsafe { RegisterEventHotKey(code, mods, id, GetApplicationEventTarget(), 0, &mut hotkey) } == 0)
            });
            if ok {
                registered.push((hotkey, crate::store::bind_key(&b.item.id)));
            } else {
                failed.push(b.combo.as_str());
            }
        }
        match failed.as_slice() {
            [] => Ok(String::new()),
            [one] => Err(format!("{one} is taken by another program")),
            many => Err(format!("{} are taken by other programs", many.join(", "))),
        }
    })
}

#[derive(Debug, PartialEq)]
enum Key {
    /// A key that types this character: where it sits depends on the keyboard layout.
    Char(char),
    /// A key with a fixed key code (`kVK_*`).
    Code(u32),
}

/// `SUPER + SHIFT + F` (the combo zunder records; ⌘ is SUPER) → Carbon modifiers and the key.
fn hotkey_for(combo: &str) -> Option<(u32, Key)> {
    let mut parts: Vec<&str> = combo.split(" + ").collect();
    let key = parts.pop()?;
    let mut mods = 0;
    for m in parts {
        mods |= match m {
            "SUPER" => CMD,
            "CTRL" => CONTROL,
            "ALT" => OPTION,
            "SHIFT" => SHIFT,
            _ => return None,
        };
    }
    let mut chars = key.chars();
    let key = match (chars.next(), chars.next()) {
        (Some(c @ ('A'..='Z' | '0'..='9')), None) => Key::Char(c.to_ascii_lowercase()),
        _ => match key {
            "comma" => Key::Char(','),
            "period" => Key::Char('.'),
            "minus" => Key::Char('-'),
            "equal" | "plus" => Key::Char('='),
            "semicolon" => Key::Char(';'),
            "slash" => Key::Char('/'),
            "grave" => Key::Char('`'),
            "bracketleft" => Key::Char('['),
            "backslash" => Key::Char('\\'),
            "bracketright" => Key::Char(']'),
            "apostrophe" => Key::Char('\''),
            "Return" => Key::Code(0x24),
            "space" => Key::Code(SPACE),
            "Tab" => Key::Code(0x30),
            "Escape" => Key::Code(0x35),
            "BackSpace" => Key::Code(0x33),
            "Delete" => Key::Code(0x75),
            "Insert" | "Help" => Key::Code(0x72),
            "Home" => Key::Code(0x73),
            "End" => Key::Code(0x77),
            "Page_Up" | "Prior" => Key::Code(0x74),
            "Page_Down" | "Next" => Key::Code(0x79),
            "Left" => Key::Code(0x7B),
            "Right" => Key::Code(0x7C),
            "Down" => Key::Code(0x7D),
            "Up" => Key::Code(0x7E),
            // macOS has key codes up to F20
            f => {
                const F: [u32; 20] = [
                    0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F, 0x69, 0x6B, 0x71, 0x6A, 0x40, 0x4F,
                    0x50, 0x5A,
                ];
                let n = f.strip_prefix('F')?.parse::<usize>().ok()?;
                Key::Code(*F.get(n.checked_sub(1)?)?)
            }
        },
    };
    Some((mods, key))
}

/// Where `c` sits on a US keyboard, for when the current layout can't be read.
fn ansi_code(c: char) -> Option<u32> {
    const KEYS: &str = "asdfhgzxcv\0bqweryt123465=97-80]ou[ip\0lj'k;\\,/nm.\0\0`";
    KEYS.chars().position(|k| k == c && k != '\0').map(|i| i as u32)
}

/// The current keyboard layout, to find the key that types a character: on a Swiss or German
/// keyboard ⌘Z is a different key than on a US one.
struct Layout {
    source: Ref,
    data: *const c_void,
    keyboard_type: u32,
}

impl Layout {
    fn current() -> Option<Self> {
        // SAFETY: main thread; the copied source is released in Drop, and the layout data is
        // owned by it.
        unsafe {
            let source = TISCopyCurrentKeyboardLayoutInputSource();
            if source.is_null() {
                return None;
            }
            let mut layout = Layout { source, data: std::ptr::null(), keyboard_type: u32::from(LMGetKbdType()) };
            let data = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData);
            if data.is_null() {
                return None;
            }
            layout.data = CFDataGetBytePtr(data).cast();
            Some(layout)
        }
    }

    /// The first key (below the keypad) that types `c` without modifiers.
    fn code(&self, c: char) -> Option<u32> {
        (0..0x40u16).find(|&code| {
            let mut dead = 0;
            let mut len = 0;
            let mut out = [0u16; 4];
            // SAFETY: `data` is the layout's `uchr` table, alive while `source` is; the output
            // buffer is as long as we say.
            let status = unsafe {
                UCKeyTranslate(
                    self.data,
                    code,
                    K_UC_KEY_ACTION_DOWN,
                    0,
                    self.keyboard_type,
                    K_UC_KEY_TRANSLATE_NO_DEAD_KEYS,
                    &mut dead,
                    out.len(),
                    &mut len,
                    out.as_mut_ptr(),
                )
            };
            status == 0 && char::decode_utf16(out[..len.min(out.len())].iter().copied()).flatten().eq([c])
        })
        .map(u32::from)
    }
}

impl Drop for Layout {
    fn drop(&mut self) {
        // SAFETY: the source came from a Copy function, so we own one reference.
        unsafe { CFRelease(self.source) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_combos_into_hotkeys() {
        assert_eq!(hotkey_for("SUPER + SHIFT + F"), Some((CMD | SHIFT, Key::Char('f'))));
        assert_eq!(hotkey_for("CTRL + ALT + 1"), Some((CONTROL | OPTION, Key::Char('1'))));
        assert_eq!(hotkey_for("ALT + Return"), Some((OPTION, Key::Code(0x24))));
        assert_eq!(hotkey_for("SUPER + comma"), Some((CMD, Key::Char(','))));
        assert_eq!(hotkey_for("F5"), Some((0, Key::Code(0x60))));
        assert_eq!(hotkey_for("CTRL + F20"), Some((CONTROL, Key::Code(0x5A))));
        assert_eq!(hotkey_for("CTRL + F21"), None);
        assert_eq!(hotkey_for("CTRL + F0"), None);
        assert_eq!(hotkey_for("CTRL + dead_acute"), None);
        assert_eq!(hotkey_for("HYPER + A"), None);
    }

    #[test]
    fn knows_the_us_layout() {
        assert_eq!(ansi_code('a'), Some(0x00));
        assert_eq!(ansi_code('b'), Some(0x0B));
        assert_eq!(ansi_code('z'), Some(0x06));
        assert_eq!(ansi_code('5'), Some(0x17));
        assert_eq!(ansi_code('0'), Some(0x1D));
        assert_eq!(ansi_code('\''), Some(0x27));
        assert_eq!(ansi_code('`'), Some(0x32));
        assert_eq!(ansi_code('\0'), None);
    }
}
