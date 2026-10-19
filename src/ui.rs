//! The overlay: a full-screen dimmed layer surface with a terminal-styled panel in the middle.

use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gtk::{gdk, glib, graphene, prelude::*};
use gtk4 as gtk;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::engine::{Engine, Section};
use crate::item::Item;
use crate::providers::windows::{self, Client};
use crate::ipc;

const PANEL_WIDTH: i32 = 820;
const LIST_HEIGHT: i32 = 460;

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
    state: RefCell<State>,
    plugin_tx: async_channel::Sender<(u64, String, Vec<Item>)>,
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

fn load_css() {
    let display = gdk::Display::default().expect("no display");
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
        panel.set_size_request(PANEL_WIDTH, -1);

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
            .min_content_height(LIST_HEIGHT)
            .max_content_height(LIST_HEIGHT)
            .child(&list)
            .build();

        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        footer.add_css_class("footer");
        for (key, desc) in [("↵", "open"), ("⇥", "next"), (">", "shell"), ("=", "calc"), ("esc", "close")] {
            footer.append(&label(key, "key"));
            footer.append(&label(desc, "desc"));
        }

        panel.append(&prompt);
        panel.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        panel.append(&scroll);
        panel.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        panel.append(&footer);
        window.set_child(Some(&panel));

        let (plugin_tx, plugin_rx) = async_channel::unbounded();
        let ui = Rc::new(Ui {
            window,
            panel,
            entry,
            list,
            scroll,
            status,
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
        });
        ui.connect();

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
        self.state.borrow().switch && self.entry.text().is_empty()
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
            "reload" => self.state.borrow_mut().engine.reload(),
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
        let Some((row, marker)) = s.rows.get(i) else { return };
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
    }

    fn activate(&self, i: usize) {
        let Some(item) = self.state.borrow().items.get(i).cloned() else { return };
        if let crate::item::Action::Fill(text) = &item.action {
            self.entry.set_text(text);
            self.entry.set_position(-1);
            return;
        }
        if item.action.keeps_open() {
            return;
        }
        self.hide();
        self.state.borrow_mut().engine.history.bump(&item.id);
        // run after the surface is gone so focus lands where the action puts it
        glib::idle_add_local_once(move || item.action.execute());
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
    let glyph = label(&item.glyph, "glyph");
    glyph.set_width_chars(2);
    let title = label(&item.title, "title");
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(52);
    let sub = label(&item.subtitle, "sub");
    sub.set_hexpand(true);
    sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let hint = label(&item.hint, "hint");
    for w in [&marker, &glyph, &title, &sub, &hint] {
        b.append(w);
    }
    (gtk::ListBoxRow::builder().child(&b).build(), marker)
}
