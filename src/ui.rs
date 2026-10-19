//! The overlay: a full-screen dimmed layer surface with a terminal-styled panel in the middle.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gtk::{gdk, glib, graphene, prelude::*};
use gtk4 as gtk;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::config;
use crate::store;
use crate::engine::{Engine, Section};
use crate::item::Item;
use crate::providers::windows::{self, Client};
use crate::ipc;

const PREVIEW_WIDTH: i32 = 420;
const ICON_SIZE: i32 = 20;

struct State {
    engine: Engine,
    windows: Vec<Client>,
    items: Vec<Item>,
    /// One per item: the row and its `▏` marker.
    rows: Vec<(gtk::ListBoxRow, gtk::Label)>,
    selected: usize,
    switch: bool,
    /// Bumped on every query so stale plugin results are dropped.
    seq: u64,
}

struct Ui {
    window: gtk::ApplicationWindow,
    panel: gtk::Box,
    entry: gtk::Entry,
    list: gtk::ListBox,
    scroll: gtk::ScrolledWindow,
    status: gtk::Label,
    preview: Preview,
    state: RefCell<State>,
    plugin_tx: async_channel::Sender<(u64, String, Vec<Item>)>,
    capture_tx: async_channel::Sender<String>,
}

/// Right-hand pane in ALT+TAB mode: a screenshot of the selected window.
struct Preview {
    pane: gtk::Box,
    picture: gtk::Picture,
    title: gtk::Label,
    meta: gtk::Label,
}

pub fn run(initial: Option<&'static str>) -> ExitCode {
    let (tx, rx) = async_channel::unbounded::<String>();
    if let Err(e) = ipc::listen(tx) {
        eprintln!("zunder: {e}");
        return ExitCode::FAILURE;
    }
    let app = gtk::Application::builder()
        .application_id("dev.zunder.Zunder")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        std::mem::forget(app.hold());
        load_css();
        let ui = Ui::build(app);
        if let Some(cmd) = initial {
            ui.command(cmd);
        }
        let rx = rx.clone();
        glib::spawn_future_local(async move {
            while let Ok(cmd) = rx.recv().await {
                ui.command(&cmd);
            }
        });
    });
    let code = app.run_with_args::<&str>(&[]);
    let _ = std::fs::remove_file(ipc::socket_path());
    if code == glib::ExitCode::SUCCESS { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

thread_local! {
    static CONFIG_CSS: gtk::CssProvider = gtk::CssProvider::new();
}

/// Font, size, dim and accent from config.toml, between the built-in theme and style.css.
fn apply_config_css() {
    let cfg = config::get();
    let mut css = format!(
        ".panel {{ font-family: \"{}\"; font-size: {}pt; }}\nwindow.zunder {{ background: alpha(black, {}); }}\n",
        cfg.font.replace('"', ""),
        cfg.font_size,
        cfg.dim.clamp(0.0, 1.0)
    );
    if !cfg.accent.is_empty() {
        let a = cfg.accent.replace([';', '}'], "");
        css += &format!(
            ".caret, .heading-title, row:selected .marker, row:selected .glyph {{ color: {a}; }}\n\
             entry.query {{ caret-color: {a}; }}\n"
        );
    }
    CONFIG_CSS.with(|p| p.load_from_string(&css));
}

fn load_css() {
    let display = gdk::Display::default().expect("no display");
    CONFIG_CSS.with(|p| {
        gtk::style_context_add_provider_for_display(&display, p, gtk::STYLE_PROVIDER_PRIORITY_USER);
    });
    apply_config_css();
    let base = gtk::CssProvider::new();
    base.load_from_string(include_str!("style.css"));
    // USER priority so ~/.config/gtk-4.0/gtk.css themes do not leak in
    gtk::style_context_add_provider_for_display(&display, &base, gtk::STYLE_PROVIDER_PRIORITY_USER);
    // ~/.config/zunder/style.css overrides anything above
    let user = dirs::config_dir().unwrap_or_default().join("zunder/style.css");
    if user.exists() {
        let custom = gtk::CssProvider::new();
        custom.load_from_path(&user);
        gtk::style_context_add_provider_for_display(&display, &custom, gtk::STYLE_PROVIDER_PRIORITY_USER + 1);
    }
}

fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class(class);
    l.set_xalign(0.0);
    l
}

impl Ui {
    fn build(app: &gtk::Application) -> Rc<Self> {
        let window = gtk::ApplicationWindow::builder().application(app).title("zunder").build();
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_namespace(Some("zunder"));
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        window.set_exclusive_zone(-1);
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(edge, true);
        }
        window.add_css_class("zunder");

        let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
        panel.add_css_class("panel");
        panel.set_halign(gtk::Align::Center);
        panel.set_valign(gtk::Align::Center);

        let prompt = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        prompt.add_css_class("prompt");
        let entry = gtk::Entry::new();
        entry.add_css_class("query");
        entry.set_hexpand(true);
        entry.set_placeholder_text(Some("search apps, windows, commands…"));
        let status = label("", "status");
        prompt.append(&label("❯", "caret"));
        prompt.append(&entry);
        prompt.append(&status);

        let list = gtk::ListBox::new();
        list.add_css_class("results");
        list.set_selection_mode(gtk::SelectionMode::Single);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&list)
            .build();

        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        footer.add_css_class("footer");
        for (key, desc) in [("↵", "open"), ("⇥", "next"), (">", "shell"), ("=", "calc"), ("esc", "close")] {
            footer.append(&label(key, "key"));
            footer.append(&label(desc, "desc"));
        }

        let preview = Preview::new();
        let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        scroll.set_hexpand(true);
        body.append(&scroll);
        body.append(&preview.pane);

        panel.append(&prompt);
        panel.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        panel.append(&body);
        panel.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        panel.append(&footer);
        window.set_child(Some(&panel));

        let (plugin_tx, plugin_rx) = async_channel::unbounded();
        let (capture_tx, capture_rx) = async_channel::unbounded::<String>();
        let ui = Rc::new(Ui {
            window,
            panel,
            entry,
            list,
            scroll,
            status,
            preview,
            state: RefCell::new(State {
                engine: Engine::new(),
                windows: Vec::new(),
                items: Vec::new(),
                rows: Vec::new(),
                selected: 0,
                switch: false,
                seq: 0,
            }),
            plugin_tx,
            capture_tx,
        });
        ui.connect();

        // a fresh screenshot landed: redraw if it belongs to the selected window
        let ui2 = ui.clone();
        glib::spawn_future_local(async move {
            while let Ok(id) = capture_rx.recv().await {
                let selected = {
                    let s = ui2.state.borrow();
                    s.items.get(s.selected).and_then(|i| i.preview.clone())
                };
                if selected.as_deref() == Some(id.as_str()) {
                    ui2.show_preview();
                }
            }
        });

        let ui2 = ui.clone();
        glib::spawn_future_local(async move {
            while let Ok((seq, title, items)) = plugin_rx.recv().await {
                if ui2.state.borrow().seq == seq {
                    ui2.render(vec![Section { title, items }]);
                }
            }
        });
        ui
    }

    fn connect(self: &Rc<Self>) {
        let ui = self.clone();
        self.entry.connect_changed(move |_| ui.refresh());

        let ui = self.clone();
        self.list.connect_row_activated(move |_, row| {
            let i = ui.state.borrow().rows.iter().position(|(r, _)| r == row);
            if let Some(i) = i {
                ui.activate(i);
            }
        });

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let ui = self.clone();
        keys.connect_key_pressed(move |_, key, _, mods| {
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            match key {
                gdk::Key::Escape => ui.hide(),
                gdk::Key::Down | gdk::Key::Tab => ui.step(1),
                gdk::Key::Up | gdk::Key::ISO_Left_Tab => ui.step(-1),
                gdk::Key::j | gdk::Key::n if ctrl => ui.step(1),
                gdk::Key::k | gdk::Key::p if ctrl => ui.step(-1),
                gdk::Key::Return | gdk::Key::KP_Enter => {
                    let i = ui.state.borrow().selected;
                    ui.activate(i);
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        // ALT+TAB: letting go of Alt switches to the selected window
        let ui = self.clone();
        keys.connect_key_released(move |_, key, _, _| {
            if matches!(key, gdk::Key::Alt_L | gdk::Key::Alt_R) && ui.alt_release_switches() {
                let i = ui.state.borrow().selected;
                ui.activate(i);
            }
        });
        self.window.add_controller(keys);

        // Alt may already be up by the time we get focus (a quick tap): switch right away
        let ui = self.clone();
        self.window.connect_is_active_notify(move |w| {
            if !w.is_active() || !ui.alt_release_switches() {
                return;
            }
            let ui = ui.clone();
            glib::timeout_add_local_once(Duration::from_millis(40), move || {
                if ui.window.is_visible() && ui.alt_release_switches() && !alt_held(&ui.window) {
                    let i = ui.state.borrow().selected;
                    ui.activate(i);
                }
            });
        });

        // click on the dimmed backdrop closes
        let click = gtk::GestureClick::new();
        let ui = self.clone();
        click.connect_released(move |_, _, x, y| {
            let p = ui.window.compute_point(&ui.panel, &graphene::Point::new(x as f32, y as f32));
            if p.is_some_and(|p| !ui.panel.contains(f64::from(p.x()), f64::from(p.y()))) {
                ui.hide();
            }
        });
        self.window.add_controller(click);
    }

    fn alt_release_switches(&self) -> bool {
        config::get().alt_release && self.state.borrow().switch && self.entry.text().is_empty()
    }

    fn command(self: &Rc<Self>, cmd: &str) {
        let visible = self.window.is_visible();
        if let Some(query) = cmd.strip_prefix("show ") {
            self.show(false);
            self.entry.set_text(query);
            self.entry.set_position(-1);
            return;
        }
        match cmd {
            "toggle" if visible => self.hide(),
            "toggle" | "show" => self.show(false),
            "hide" => self.hide(),
            "switch" | "switch-prev" if visible => self.step(if cmd == "switch" { 1 } else { -1 }),
            "switch" => self.show(true),
            "switch-prev" => {
                self.show(true);
                let n = self.state.borrow().items.len();
                self.select(n.saturating_sub(1));
            }
            "reload" => {
                config::reload();
                apply_config_css();
                self.state.borrow_mut().engine.reload();
            }
            "quit" => {
                if let Some(app) = self.window.application() {
                    app.quit();
                }
            }
            other => eprintln!("zunder: unknown command `{other}`"),
        }
    }

    fn show(self: &Rc<Self>, switch: bool) {
        {
            let mut s = self.state.borrow_mut();
            s.switch = switch;
            s.windows = windows::list();
            if !switch {
                s.engine.reload();
            }
        }
        let (width, height, previews) = {
            let cfg = config::get();
            (cfg.width, cfg.height, cfg.previews)
        };
        if switch && previews {
            capture_all(&self.state.borrow().windows, &self.capture_tx);
        }
        let width = if switch && previews { width + PREVIEW_WIDTH } else { width };
        self.panel.set_size_request(width, -1);
        self.scroll.set_min_content_height(height);
        self.scroll.set_max_content_height(height);
        self.entry.set_text("");
        self.refresh();
        self.window.present();
        self.entry.grab_focus();
    }

    fn hide(&self) {
        self.window.set_visible(false);
    }

    fn refresh(self: &Rc<Self>) {
        let query = self.entry.text().to_string();
        let (sections, call, seq) = {
            let mut s = self.state.borrow_mut();
            s.seq += 1;
            let sections = s.engine.search(&query, &s.windows, s.switch);
            (sections, s.engine.plugins.call(query.trim_start()), s.seq)
        };
        if let Some(call) = call {
            let tx = self.plugin_tx.clone();
            std::thread::spawn(move || {
                let items = call.run();
                let _ = tx.send_blocking((seq, call.plugin.name.clone(), items));
            });
        }
        self.render(sections);
    }

    fn render(&self, sections: Vec<Section>) {
        self.list.remove_all();
        let mut items = Vec::new();
        let mut rows = Vec::new();
        for section in sections {
            self.list.append(&heading(&section.title));
            for item in section.items {
                let (row, marker) = item_row(&item);
                self.list.append(&row);
                rows.push((row, marker));
                items.push(item);
            }
        }
        if items.is_empty() {
            let row = gtk::ListBoxRow::builder().selectable(false).activatable(false).build();
            row.set_child(Some(&label("  nothing here", "empty")));
            self.list.append(&row);
        }
        let count = items.len();
        self.status.set_text(&if count == 1 { "1 result".into() } else { format!("{count} results") });
        let first = {
            let mut s = self.state.borrow_mut();
            s.items = items;
            s.rows = rows;
            // ALT+TAB lands on the previous window, like every other switcher
            usize::from(s.switch && count > 1 && self.entry.text().is_empty())
        };
        self.scroll.vadjustment().set_value(0.0);
        self.select(first);
    }

    fn step(&self, delta: isize) {
        let (selected, n) = {
            let s = self.state.borrow();
            (s.selected as isize, s.items.len() as isize)
        };
        if n > 0 {
            self.select((selected + delta).rem_euclid(n) as usize);
        }
    }

    fn select(&self, i: usize) {
        let mut s = self.state.borrow_mut();
        if let Some((_, old)) = s.rows.get(s.selected) {
            old.set_text("▏");
        }
        s.selected = i;
        let Some((row, marker)) = s.rows.get(i) else {
            drop(s);
            self.show_preview();
            return;
        };
        marker.set_text("▌");
        self.list.select_row(Some(row));
        // keep the selection in view (the first item also reveals its heading)
        let adj = self.scroll.vadjustment();
        if i == 0 {
            adj.set_value(0.0);
        } else if let Some(b) = row.compute_bounds(&self.list) {
            let (y, h) = (f64::from(b.y()), f64::from(b.height()));
            if y < adj.value() {
                adj.set_value(y - h);
            } else if y + h > adj.value() + adj.page_size() {
                adj.set_value(y + h - adj.page_size());
            }
        }
        drop(s);
        self.show_preview();
    }

    fn show_preview(&self) {
        let s = self.state.borrow();
        let item = s.items.get(s.selected).filter(|_| s.switch && config::get().previews);
        let Some((item, id)) = item.and_then(|i| Some((i, i.preview.as_ref()?))) else {
            self.preview.pane.set_visible(false);
            return;
        };
        let path = preview_path(id);
        if path.exists() {
            self.preview.picture.set_filename(Some(&path));
        } else {
            self.preview.picture.set_paintable(None::<&gdk::Paintable>);
        }
        self.preview.title.set_text(&item.title);
        self.preview.meta.set_text(&format!("{}  ·  {}", item.subtitle, item.hint));
        self.preview.pane.set_visible(true);
    }

    fn activate(self: &Rc<Self>, i: usize) {
        let Some(item) = self.state.borrow().items.get(i).cloned() else { return };
        if let crate::item::Action::Fill(text) = &item.action {
            self.entry.set_text(text);
            self.entry.set_position(-1);
            return;
        }
        if let crate::item::Action::Set { key, value } = &item.action {
            self.set_setting(key, value.as_deref());
            return;
        }
        if item.action.keeps_open() {
            return;
        }
        self.hide();
        self.state.borrow_mut().engine.history.bump(&item.id);
        store::log_launch(&item);
        // run after the surface is gone so focus lands where the action puts it
        glib::idle_add_local_once(move || item.action.execute());
    }
}

impl Preview {
    fn new() -> Self {
        let pane = gtk::Box::new(gtk::Orientation::Vertical, 0);
        pane.add_css_class("preview");
        pane.set_size_request(PREVIEW_WIDTH, -1);
        pane.set_visible(false);
        let picture = gtk::Picture::new();
        picture.set_content_fit(gtk::ContentFit::Contain);
        picture.set_can_shrink(true);
        picture.set_vexpand(true);
        let title = label("", "preview-title");
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let meta = label("", "preview-meta");
        meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
        pane.append(&picture);
        pane.append(&title);
        pane.append(&meta);
        Preview { pane, picture, title, meta }
    }
}

fn preview_path(id: &str) -> PathBuf {
    dirs::runtime_dir().unwrap_or_else(std::env::temp_dir).join("zunder-previews").join(format!("{id}.jpg"))
}

/// Screenshot every window in the background (`grim -T`, works across workspaces).
/// The previous capture stays on screen until the new one is written.
fn capture_all(windows: &[Client], tx: &async_channel::Sender<String>) {
    let ids: Vec<String> = windows.iter().map(|c| c.stable_id.clone()).filter(|id| !id.is_empty()).collect();
    if let Some(dir) = preview_path("x").parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    for id in ids {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let path = preview_path(&id);
            let tmp = path.with_extension("tmp");
            let ok = std::process::Command::new("grim")
                .args(["-T", &id, "-s", "0.5", "-t", "jpeg", "-q", "80"])
                .arg(&tmp)
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if ok && std::fs::rename(&tmp, &path).is_ok() {
                let _ = tx.send_blocking(id);
            }
        });
    }
}

fn icon(item: &Item) -> Option<gtk::Image> {
    let icon = item.icon.as_str();
    if icon.is_empty() {
        return None;
    }
    let image = if icon.starts_with('/') {
        Path::new(icon).exists().then(|| gtk::Image::from_file(icon))?
    } else {
        let theme = gtk::IconTheme::for_display(&gdk::Display::default()?);
        theme.has_icon(icon).then(|| gtk::Image::from_icon_name(icon))?
    };
    image.set_pixel_size(ICON_SIZE);
    image.add_css_class("icon");
    Some(image)
}

impl Ui {
    fn set_setting(self: &Rc<Self>, key: &str, value: Option<&str>) {
        let mut result = Ok(());
        config::update(|c| match key.strip_prefix("provider:") {
            Some(provider) => c.toggle_provider(provider),
            None => result = c.set(key, value),
        });
        apply_config_css();
        let (width, height) = {
            let cfg = config::get();
            (cfg.width, cfg.height)
        };
        self.panel.set_size_request(width, -1);
        self.scroll.set_min_content_height(height);
        self.scroll.set_max_content_height(height);
        // back to the list, keeping the selection on the changed row
        let selected = self.state.borrow().selected;
        if value.is_some() {
            self.entry.set_text("settings ");
            self.entry.set_position(-1);
        } else {
            self.refresh();
            self.select(selected);
        }
        if let Err(e) = result {
            self.status.set_text(&e);
        }
    }
}

fn alt_held(window: &gtk::ApplicationWindow) -> bool {
    WidgetExt::display(window)
        .default_seat()
        .and_then(|seat| seat.keyboard())
        .is_some_and(|kb| kb.modifier_state().contains(gdk::ModifierType::ALT_MASK))
}

fn heading(title: &str) -> gtk::ListBoxRow {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.add_css_class("heading");
    b.append(&label(title, "heading-title"));
    let rule = gtk::Separator::new(gtk::Orientation::Horizontal);
    rule.set_hexpand(true);
    rule.set_valign(gtk::Align::Center);
    b.append(&rule);
    gtk::ListBoxRow::builder().selectable(false).activatable(false).child(&b).build()
}

fn item_row(item: &Item) -> (gtk::ListBoxRow, gtk::Label) {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.add_css_class("item");
    let marker = label("▏", "marker");
    let glyph: gtk::Widget = match icon(item).filter(|_| config::get().icons) {
        Some(image) => image.upcast(),
        None => {
            let glyph = label(&item.glyph, "glyph");
            glyph.set_xalign(0.5);
            glyph.upcast()
        }
    };
    glyph.set_size_request(ICON_SIZE, -1);
    let title = label(&item.title, "title");
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(52);
    let sub = label(&item.subtitle, "sub");
    sub.set_hexpand(true);
    sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let hint = label(&item.hint, "hint");
    b.append(&marker);
    b.append(&glyph);
    for w in [&title, &sub, &hint] {
        b.append(w);
    }
    (gtk::ListBoxRow::builder().child(&b).build(), marker)
}
