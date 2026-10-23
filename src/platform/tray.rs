//! Linux tray icon (StatusNotifierItem over D-Bus, shown by waybar and most bars), so you can see
//! the server runs. Its menu sends the same commands as the CLI through the IPC channel.

use std::sync::{Mutex, OnceLock};

use ksni::blocking::{Handle, TrayMethods};

struct Tray {
    tx: async_channel::Sender<String>,
}

impl Tray {
    fn send(&self, cmd: &str) {
        let _ = self.tx.send_blocking(cmd.to_owned());
    }
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "zunder".into()
    }

    fn title(&self) -> String {
        "zunder".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: "zunder".into(), description: "running · click to open".into(), ..Default::default() }
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        ICONS.iter().filter_map(|(_, png)| icon(png)).collect()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send("toggle");
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        let item = |label: &str, cmd: &'static str| -> ksni::MenuItem<Self> {
            StandardItem { label: label.into(), activate: Box::new(move |t: &mut Self| t.send(cmd)), ..Default::default() }
                .into()
        };
        vec![
            item("Open", "show"),
            item("Settings", "open-settings"),
            item("Reload", "reload"),
            ksni::MenuItem::Separator,
            item("Quit", "quit"),
        ]
    }
}

static TX: OnceLock<async_channel::Sender<String>> = OnceLock::new();
static HANDLE: Mutex<Option<Handle<Tray>>> = Mutex::new(None);

/// Called once at startup with the channel the menu sends commands to.
pub fn init_tray(tx: async_channel::Sender<String>) {
    let _ = TX.set(tx);
    sync_tray();
}

/// Show or remove the icon to match the `tray` setting.
pub fn sync_tray() {
    let on = crate::config::get().tray;
    let Some(tx) = TX.get() else { return };
    let Ok(mut handle) = HANDLE.lock() else { return };
    match (on, handle.is_some()) {
        // registers whenever the bar's tray shows up, so starting before waybar is fine
        (true, false) => match (Tray { tx: tx.clone() }).assume_sni_available(true).spawn() {
            Ok(h) => *handle = Some(h),
            // no session bus or no tray host: zunder works the same without the icon
            Err(e) => eprintln!("zunder: no tray icon: {e}"),
        },
        (false, true) => {
            if let Some(h) = handle.take() {
                let _ = h.shutdown();
            }
        }
        _ => {}
    }
}

/// The zunder logo (`assets/logo.svg`), rendered to PNG at the sizes trays ask for and built
/// into the binary, so there is no icon file to install.
const ICONS: &[(i32, &[u8])] = &[
    (16, include_bytes!("../../assets/icons/zunder-16.png")),
    (22, include_bytes!("../../assets/icons/zunder-22.png")),
    (32, include_bytes!("../../assets/icons/zunder-32.png")),
    (48, include_bytes!("../../assets/icons/zunder-48.png")),
    (64, include_bytes!("../../assets/icons/zunder-64.png")),
];

/// One PNG as the ARGB32 (big-endian) pixmap StatusNotifierItem wants.
fn icon(png: &[u8]) -> Option<ksni::Icon> {
    use gtk4::gdk_pixbuf::Pixbuf;
    use gtk4::gio::MemoryInputStream;
    let stream = MemoryInputStream::from_bytes(&gtk4::glib::Bytes::from(png));
    let pixbuf = Pixbuf::from_stream(&stream, None::<&gtk4::gio::Cancellable>).ok()?.add_alpha(false, 0, 0, 0).ok()?;
    let (width, height, stride) = (pixbuf.width(), pixbuf.height(), pixbuf.rowstride() as usize);
    let pixels = pixbuf.read_pixel_bytes();
    let mut data = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height as usize {
        for px in pixels[y * stride..][..width as usize * 4].as_chunks::<4>().0 {
            data.extend([px[3], px[0], px[1], px[2]]);
        }
    }
    Some(ksni::Icon { width, height, data })
}

#[cfg(test)]
mod tests {
    #[test]
    fn icons_decode_to_argb() {
        for (size, png) in super::ICONS {
            let icon = super::icon(png).expect("png decodes");
            assert_eq!((icon.width, icon.height), (*size, *size));
            assert_eq!(icon.data.len(), (size * size * 4) as usize);
            // the rounded corner is transparent, the middle is opaque
            assert_eq!(icon.data[0], 0);
            let mid = ((size / 2 * size + size / 2) * 4) as usize;
            assert_eq!(icon.data[mid], 255);
        }
    }
}
