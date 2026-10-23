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
        [22, 32, 48, 64].into_iter().map(icon).collect()
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
        (true, false) => match (Tray { tx: tx.clone() }).spawn() {
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

/// The icon, drawn here so there is no image file to install: an accent `❯` on a dark rounded
/// square, like the launcher's prompt. ARGB32, big-endian.
fn icon(size: i32) -> ksni::Icon {
    let s = size as f32;
    let (bg, fg): ([u8; 3], [u8; 3]) = ([0x0e, 0x0e, 0x14], [0xb4, 0xbe, 0xfe]);
    let radius = s * 0.22;
    let stroke = s * 0.085;
    // the chevron: two strokes meeting at the right
    let (a, b, c) = ((s * 0.36, s * 0.27), (s * 0.62, s * 0.5), (s * 0.36, s * 0.73));
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            // rounded square coverage
            let qx = (px - s / 2.0).abs() - (s / 2.0 - radius);
            let qy = (py - s / 2.0).abs() - (s / 2.0 - radius);
            let outside = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
            let alpha = (0.5 - outside).clamp(0.0, 1.0);
            let d = segment_distance((px, py), a, b).min(segment_distance((px, py), b, c));
            let ink = (stroke - d + 0.5).clamp(0.0, 1.0);
            let mix = |i: usize| (f32::from(bg[i]) + (f32::from(fg[i]) - f32::from(bg[i])) * ink).round() as u8;
            data.extend([(alpha * 255.0).round() as u8, mix(0), mix(1), mix(2)]);
        }
    }
    ksni::Icon { width: size, height: size, data }
}

fn segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * abx + (p.1 - a.1) * aby) / (abx * abx + aby * aby)).clamp(0.0, 1.0);
    (p.0 - a.0 - abx * t).hypot(p.1 - a.1 - aby * t)
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_is_argb_of_the_right_size() {
        let icon = super::icon(32);
        assert_eq!(icon.data.len(), 32 * 32 * 4);
        // corners are transparent, the middle of the left stroke is accent-colored
        assert_eq!(icon.data[0], 0);
        let at = |x: usize, y: usize| &icon.data[(y * 32 + x) * 4..][..4];
        assert_eq!(at(16, 16)[0], 255);
        assert!(at(15, 13)[3] > 0xd0, "{:?}", at(15, 13));
    }
}
