//! The overlay: a full-screen dimmed layer surface with a terminal-styled panel in the middle.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::Duration;

use gtk::{gdk, glib, graphene, prelude::*};
use gtk4 as gtk;

use crate::actions;
use crate::config;
use crate::settings_window;
use crate::store;
use crate::theme;
use crate::engine::{Engine, Section};
use crate::item::Item;
use crate::providers::windows::{self, Client};
use crate::ipc;

const SWITCH_HINT: &str = "↵ focus   ^↵ bring here   ^W close   ^M minimize   ^1-9 move   ^K more   type to search";
const PLACEHOLDER: &str = "search apps, windows, commands…";
const PREVIEW_WIDTH: i32 = 420;
/// The switcher's window screenshots get a wider column than launcher image previews.
const SWITCH_PREVIEW_WIDTH: i32 = 760;
const ICON_SIZE: i32 = 20;
/// ALT+TAB thumbnail size; the screenshot is fitted inside.
const TILE_WIDTH: i32 = 280;
const TILE_HEIGHT: i32 = 175;
const TILE_COLUMNS: usize = 5;
/// The overview's thumbnails are smaller: every workspace has to fit next to each other.
const OVERVIEW_TILE: (i32, i32) = (220, 138);
const OVERVIEW_GROUPS_PER_LINE: u32 = 6;

struct State {
    engine: Engine,
    windows: Vec<Client>,
    items: Vec<Item>,
    /// One per item: the row and its `▏` marker.
    rows: Vec<(gtk::ListBoxRow, gtk::Label)>,
    selected: usize,
    switch: bool,
    /// Opened with `overview`: the switcher stays open until a window is picked (no Alt to release).
    hold: bool,
    /// What's on screen, so late results from background jobs can be merged in.
    sections: Vec<Section>,
    /// Bumped on every query so stale background results are dropped.
    seq: u64,
    /// The window got focus since it was shown (closing on focus loss waits for this).
    had_focus: bool,
    /// The preview column is open (ALT+TAB with previews, or images in the results).
    preview_column: bool,
    /// Ctrl+B was pressed: the next key combo becomes a global keybind for this item.
    recording: Option<Item>,
    /// When Hyprland last said Alt went up while the switcher wasn't open yet: on a quick tap
    /// the release can arrive before `switch` does.
    alt_up: Option<std::time::Instant>,
    /// ALT+TAB shows the thumbnail grid instead of the panel (until you type).
    strip: bool,
    /// One per item while `strip` is on.
    tiles: Vec<Tile>,
    /// Ctrl+K: the action menu of an item is showing instead of the results.
    menu: Option<Menu>,
}

/// What Ctrl+K opened, and what to go back to.
struct Menu {
    item: Item,
    query: String,
    from_strip: bool,
}

/// One window in the ALT+TAB grid: its screenshot, or the app icon until there is one.
struct Tile {
    child: gtk::FlowBoxChild,
    picture: gtk::Picture,
    placeholder: gtk::Widget,
    preview: Option<String>,
    /// Thumbnail size: the ALT+TAB grid's or the overview's.
    size: (i32, i32),
}

/// Turns the ALT+TAB grid into the overview's workspace groups.
struct Group {
    title: String,
    items: Vec<Item>,
}

struct Ui {
    window: gtk::ApplicationWindow,
    panel: gtk::Box,
    entry: gtk::Entry,
    list: gtk::ListBox,
    scroll: gtk::ScrolledWindow,
    status: gtk::Label,
    preview: Preview,
    switcher: gtk::Box,
    /// The grid, or one titled grid per workspace in the overview.
    groups: gtk::Box,
    groups_scroll: gtk::ScrolledWindow,
    /// Window keys under the overview.
    switch_hint: gtk::Label,
    state: RefCell<State>,
    plugin_tx: async_channel::Sender<(u64, String, Vec<Item>)>,
    capture_tx: async_channel::Sender<String>,
}

/// Right-hand pane in ALT+TAB mode: a screenshot of the selected window.
struct Preview {
    pane: gtk::Box,
    picture: gtk::Picture,
    text: gtk::TextView,
    text_scroll: gtk::ScrolledWindow,
    /// command → output, so moving through the list doesn't rerun it
    text_cache: RefCell<std::collections::HashMap<String, String>>,
    title: gtk::Label,
    meta: gtk::Label,
}

pub fn run(initial: Option<&'static str>) -> ExitCode {
    crate::platform::init();
    let (tx, rx) = async_channel::unbounded::<String>();
    if let Err(e) = ipc::listen(tx.clone()) {
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
        // after GTK is up: the macOS menu bar item needs the running NSApplication
        crate::platform::init_tray(tx.clone());
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
    crate::platform::stop_tray();
    let _ = std::fs::remove_file(ipc::socket_path());
    if code == glib::ExitCode::SUCCESS { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

thread_local! {
    static CONFIG_CSS: gtk::CssProvider = gtk::CssProvider::new();
    static THEME_CSS: gtk::CssProvider = gtk::CssProvider::new();
}

/// Font, size, dim, theme and accent from config.toml, between the built-in CSS and style.css.
fn apply_config_css() {
    let cfg = config::get();
    let css = format!(
        ".panel, .switcher {{ font-family: \"{}\", \"JetBrainsMono NF\", monospace; font-size: {}pt; }}\nwindow.zunder {{ background: alpha(black, {}); }}\n",
        cfg.font.replace('"', ""),
        cfg.font_size,
        // no full-screen backdrop on Windows: the window only wraps the panel and stays clear
        if cfg!(windows) { 0.0 } else { cfg.dim.clamp(0.0, 1.0) }
    );
    CONFIG_CSS.with(|p| p.load_from_string(&css));

    // @define-color in a higher-priority provider recolors the rules in style.css
    let mut colors = theme::css(&cfg.theme);
    if !cfg.accent.is_empty() {
        colors += &format!("\n@define-color accent {};\n", cfg.accent.replace([';', '}', '{'], ""));
    }
    THEME_CSS.with(|p| p.load_from_string(&colors));
}

/// Fonts shipped next to the binary (`<prefix>/share/fonts`, the Windows bundle) go straight into
/// GTK's font map, so they work without being installed and without the OS looking them up.
fn add_bundled_fonts() {
    use gtk::pango::prelude::FontMapExt;
    // Windows: <prefix>\share\fonts; macOS app: Contents/Resources/share/fonts
    let Some(base) = std::env::current_exe().ok().and_then(|e| Some(e.parent()?.parent()?.to_path_buf())) else {
        return;
    };
    let Some(dir) = [base.join("share").join("fonts"), base.join("Resources").join("share").join("fonts")]
        .into_iter()
        .find(|d| d.is_dir())
    else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    let font_map = gtk::Label::new(None).pango_context().font_map();
    let Some(font_map) = font_map else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("otf"))
            && let Err(e) = font_map.add_font_file(&path)
        {
            eprintln!("zunder: couldn't load {}: {e}", path.display());
        }
    }
}

fn load_css() {
    add_bundled_fonts();
    let display = gdk::Display::default().expect("no display");
    CONFIG_CSS.with(|p| {
        gtk::style_context_add_provider_for_display(&display, p, gtk::STYLE_PROVIDER_PRIORITY_USER);
    });
    THEME_CSS.with(|p| {
        gtk::style_context_add_provider_for_display(&display, p, gtk::STYLE_PROVIDER_PRIORITY_USER + 1);
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
        gtk::style_context_add_provider_for_display(&display, &custom, gtk::STYLE_PROVIDER_PRIORITY_USER + 2);
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
        crate::platform::init_overlay(&window);
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
        entry.set_placeholder_text(Some(PLACEHOLDER));
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
        for (key, desc) in [("↵", "open"), ("^K", "actions"), ("^F", "favorite"), ("^B", "keybind"), ("^,", "settings"), ("esc", "close")] {
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
        let groups = gtk::Box::new(gtk::Orientation::Vertical, 0);
        groups.add_css_class("groups");
        let groups_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .propagate_natural_width(true)
            .child(&groups)
            .build();
        let switch_hint = label(SWITCH_HINT, "switch-hint");
        switch_hint.set_xalign(0.5);
        let switcher = gtk::Box::new(gtk::Orientation::Vertical, 0);
        switcher.add_css_class("switcher");
        switcher.set_halign(gtk::Align::Center);
        switcher.set_valign(gtk::Align::Center);
        switcher.set_visible(false);
        switcher.append(&groups_scroll);
        switcher.append(&switch_hint);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        for w in [&panel, &switcher] {
            w.set_hexpand(true);
            w.set_vexpand(true);
            root.append(w);
        }
        window.set_child(Some(&root));

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
            switcher,
            groups,
            groups_scroll,
            switch_hint,
            state: RefCell::new(State {
                engine: Engine::new(),
                windows: Vec::new(),
                items: Vec::new(),
                rows: Vec::new(),
                selected: 0,
                switch: false,
                hold: false,
                seq: 0,
                sections: Vec::new(),
                recording: None,
                alt_up: None,
                preview_column: false,
                had_focus: false,
                strip: false,
                tiles: Vec::new(),
                menu: None,
            }),
            plugin_tx,
            capture_tx,
        });
        ui.connect();

        // a fresh screenshot landed: redraw if it belongs to the selected window
        let ui2 = ui.clone();
        glib::spawn_future_local(async move {
            while let Ok(id) = capture_rx.recv().await {
                if ui2.state.borrow().strip {
                    ui2.update_tile(&id);
                    continue;
                }
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
                if ui2.state.borrow().seq != seq || items.is_empty() {
                    continue;
                }
                let mut sections = ui2.state.borrow().sections.clone();
                // a "pending:" placeholder (e.g. "checking GitHub…") makes room for the real answer
                let placeholder = sections
                    .iter()
                    .position(|s| s.title == title && s.items.iter().all(|i| i.id.starts_with("pending:")));
                let at = match placeholder {
                    Some(i) => {
                        sections.remove(i);
                        i
                    }
                    None => sections.iter().position(|s| s.title == "Other").unwrap_or(sections.len()),
                };
                sections.insert(at, Section { title, items });
                ui2.render(sections, true);
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
            if ui.state.borrow().recording.is_some() {
                ui.record(key, mods);
                return glib::Propagation::Stop;
            }
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            if ui.state.borrow().menu.is_some() && (key == gdk::Key::Escape || (ctrl && key == gdk::Key::k)) {
                ui.close_menu();
                return glib::Propagation::Stop;
            }
            if ctrl && key == gdk::Key::k {
                ui.open_menu();
                return glib::Propagation::Stop;
            }
            if ctrl && ui.window_key(key) {
                return glib::Propagation::Stop;
            }
            if ui.state.borrow().strip {
                let hold = ui.state.borrow().hold;
                match key {
                    // ALT+TAB walks the grid in order; the overview moves by position
                    gdk::Key::Right if !hold => return ui.step_stop(1),
                    gdk::Key::Left if !hold => return ui.step_stop(-1),
                    gdk::Key::Right => return ui.step_toward(1.0, 0.0),
                    gdk::Key::Left => return ui.step_toward(-1.0, 0.0),
                    gdk::Key::Down => return ui.step_toward(0.0, 1.0),
                    gdk::Key::Up => return ui.step_toward(0.0, -1.0),
                    _ => {}
                }
                // typing turns the grid into the searchable list
                if let Some(c) = key.to_unicode().filter(|c| !c.is_control() && !ctrl) {
                    ui.leave_strip(&c.to_string());
                    return glib::Propagation::Stop;
                }
            }
            match key {
                gdk::Key::Escape => ui.hide(),
                gdk::Key::f if ctrl => ui.toggle_favorite(),
                gdk::Key::b if ctrl => ui.start_recording(),
                gdk::Key::comma | gdk::Key::period if ctrl => ui.open_settings(None),
                gdk::Key::Down | gdk::Key::Tab => ui.step(1),
                gdk::Key::Up | gdk::Key::ISO_Left_Tab => ui.step(-1),
                gdk::Key::j | gdk::Key::n if ctrl => ui.step(1),
                gdk::Key::p if ctrl => ui.step(-1),
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

        // without a full-screen overlay there's no backdrop to click: close when focus leaves
        // only once it really had focus: a console or file manager taking focus back right after
        // the launch shouldn't close it before you've seen it
        if !crate::platform::FULL_SCREEN_OVERLAY {
            let ui = self.clone();
            self.window.connect_is_active_notify(move |w| {
                if w.is_active() {
                    ui.state.borrow_mut().had_focus = true;
                } else if w.is_visible() && ui.state.borrow().had_focus && ui.state.borrow().recording.is_none() {
                    ui.hide();
                }
            });
        }

        // Alt+F4 or "Close" in the taskbar (Windows, macOS) would destroy the one window zunder
        // has, and showing it again afterwards crashes GTK: closing only hides it
        let ui = self.clone();
        self.window.connect_close_request(move |_| {
            ui.hide();
            glib::Propagation::Stop
        });

        // click on the dimmed backdrop closes
        let click = gtk::GestureClick::new();
        let ui = self.clone();
        click.connect_released(move |_, _, x, y| {
            let target = if ui.state.borrow().strip { ui.switcher.clone() } else { ui.panel.clone() };
            let p = ui.window.compute_point(&target, &graphene::Point::new(x as f32, y as f32));
            if p.is_some_and(|p| !target.contains(f64::from(p.x()), f64::from(p.y()))) {
                ui.hide();
            }
        });
        self.window.add_controller(click);
    }

    /// A quick tap: Alt was already up (by Hyprland's release bind) when `switch` arrived, so
    /// nothing is left to release. Switch right away.
    fn released_early(self: &Rc<Self>) {
        let at = self.state.borrow_mut().alt_up.take();
        if at.is_some_and(|t| t.elapsed() < Duration::from_millis(250)) && self.alt_release_switches() {
            let i = self.state.borrow().selected;
            self.activate(i);
        }
    }

    fn alt_release_switches(&self) -> bool {
        let s = self.state.borrow();
        config::get().alt_release && s.switch && !s.hold && s.menu.is_none() && self.entry.text().is_empty()
    }

    fn command(self: &Rc<Self>, cmd: &str) {
        let visible = self.window.is_visible();
        if let Some(query) = cmd.strip_prefix("show ") {
            self.show(false);
            self.entry.set_text(query);
            self.entry.set_position(-1);
            return;
        }
        if let Some(key) = cmd.strip_prefix("run ") {
            self.run_bind(key);
            return;
        }
        match cmd {
            "toggle" if visible => self.hide(),
            "toggle" | "show" => self.show(false),
            "hide" => self.hide(),
            "switch" | "switch-prev" if visible => self.step(if cmd == "switch" { 1 } else { -1 }),
            "switch" => {
                self.show(true);
                self.released_early();
            }
            "alt-up" if visible => {
                if self.alt_release_switches() {
                    let i = self.state.borrow().selected;
                    self.activate(i);
                }
            }
            "alt-up" => self.state.borrow_mut().alt_up = Some(std::time::Instant::now()),
            "overview" if visible => self.hide(),
            "overview" => {
                self.state.borrow_mut().hold = true;
                self.show(true);
            }
            "switch-prev" => {
                self.show(true);
                let n = self.state.borrow().items.len();
                self.select(n.saturating_sub(1));
                self.released_early();
            }
            "open-settings" => self.open_settings(None),
            "open-changelog" => self.open_changelog(),
            cmd if cmd.starts_with("open-settings ") => self.open_settings(cmd.strip_prefix("open-settings ")),
            "reload" => {
                config::reload();
                apply_config_css();
                crate::platform::sync_tray();
                self.state.borrow_mut().engine.reload();
                if visible {
                    self.refresh();
                }
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
        let started = std::time::Instant::now();
        {
            let mut s = self.state.borrow_mut();
            s.switch = switch;
            s.strip = switch;
            s.had_focus = false;
            s.menu = None;
            s.windows = windows::list();
            timing("windows", started);
            crate::providers::setup::reset();
            if !switch {
                let t = std::time::Instant::now();
                s.engine.reload();
                timing("reload", t);
            }
        }
        let (width, height, previews) = {
            let cfg = config::get();
            (cfg.width, cfg.height, cfg.previews)
        };
        if switch && previews {
            capture_all(&self.state.borrow().windows, &self.capture_tx);
        }
        let _ = width;
        self.panel.set_visible(!switch);
        self.switcher.set_visible(switch);
        self.switch_hint.set_visible(self.state.borrow().hold);
        self.entry.set_placeholder_text(Some(PLACEHOLDER));
        self.fit_panel(switch && previews);
        self.scroll.set_min_content_height(height);
        self.scroll.set_max_content_height(height);
        self.preview.text_cache.borrow_mut().clear();
        self.entry.set_text("");
        self.refresh();
        let t = std::time::Instant::now();
        self.window.present();
        crate::platform::after_show(&self.window);
        if !switch {
            self.entry.grab_focus();
        }
        timing("present", t);
        timing("show", started);
    }

    /// From the ALT+TAB grid to the full panel, searching for `text`.
    fn leave_strip(self: &Rc<Self>, text: &str) {
        {
            let mut s = self.state.borrow_mut();
            s.strip = false;
            s.tiles.clear();
        }
        clear(&self.groups);
        self.switcher.set_visible(false);
        self.panel.set_visible(true);
        self.entry.set_text(text);
        self.entry.grab_focus();
        self.entry.set_position(-1);
    }

    fn hide(&self) {
        {
            let mut s = self.state.borrow_mut();
            s.hold = false;
            s.menu = None;
        }
        self.window.set_visible(false);
    }

    fn refresh(self: &Rc<Self>) {
        let query = self.entry.text().to_string();
        if self.state.borrow().menu.is_some() {
            self.render_menu(&query);
            return;
        }
        let (sections, jobs, seq) = {
            let mut s = self.state.borrow_mut();
            s.seq += 1;
            let t = std::time::Instant::now();
            let (sections, jobs) = s.engine.search(&query, &s.windows, s.switch);
            timing(&format!("search {query:?}"), t);
            (sections, jobs, s.seq)
        };
        if self.state.borrow().strip {
            let t = std::time::Instant::now();
            self.render_strip(sections);
            timing("render tiles", t);
            return;
        }
        for (title, job) in jobs {
            let tx = self.plugin_tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send_blocking((seq, title, job()));
            });
        }
        let t = std::time::Instant::now();
        self.render(sections, false);
        timing("render", t);
    }

    /// Show `sections`. `keep` holds the selection and scroll (late results merging in).
    fn render(&self, sections: Vec<Section>, keep: bool) {
        // same results as on screen (reopening the start screen): keep the rows, just reset
        if !keep && !sections.is_empty() && self.state.borrow().sections == sections {
            let first = {
                let s = self.state.borrow();
                usize::from(s.switch && s.menu.is_none() && s.items.len() > 1 && self.entry.text().is_empty())
            };
            let column = {
                let s = self.state.borrow();
                s.items.iter().any(|i| i.image.is_some() || i.text.is_some())
                    || (s.switch && config::get().previews && s.items.iter().any(|i| i.preview.is_some()))
            };
            self.fit_panel(column);
            self.scroll.vadjustment().set_value(0.0);
            self.select(first);
            return;
        }
        let (previous, scroll) = (self.state.borrow().selected, self.scroll.vadjustment().value());
        self.state.borrow_mut().sections = sections.clone();
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
        let column = items.iter().any(|i| i.image.is_some() || i.text.is_some())
            || (self.state.borrow().switch && config::get().previews && items.iter().any(|i| i.preview.is_some()));
        self.fit_panel(column);
        self.status.set_text(&if count == 1 { "1 result".into() } else { format!("{count} results") });
        let first = {
            let mut s = self.state.borrow_mut();
            s.items = items;
            s.rows = rows;
            // ALT+TAB lands on the previous window, like every other switcher
            usize::from(s.switch && s.menu.is_none() && count > 1 && self.entry.text().is_empty())
        };
        if keep {
            self.select(previous.min(count.saturating_sub(1)));
            self.scroll.vadjustment().set_value(scroll);
        } else {
            self.scroll.vadjustment().set_value(0.0);
            self.select(first);
        }
    }

    /// The ALT+TAB grid: one thumbnail per window, the previous one selected. The overview
    /// (`hold`) splits it into one titled grid per workspace.
    fn render_strip(self: &Rc<Self>, sections: Vec<Section>) {
        let items: Vec<Item> = sections
            .into_iter()
            .flat_map(|s| s.items)
            .filter(|i| matches!(i.action, crate::item::Action::Focus(_)))
            .collect();
        let (overview, windows) = {
            let s = self.state.borrow();
            (s.hold, s.windows.clone())
        };
        let groups = if overview { by_workspace(items, &windows) } else { vec![Group { title: String::new(), items }] };
        clear(&self.groups);
        let previews = config::get().previews;
        let size = if overview { OVERVIEW_TILE } else { (TILE_WIDTH, TILE_HEIGHT) };
        let (mut items, mut tiles) = (Vec::new(), Vec::new());
        // the overview: one column per workspace, side by side, wrapping after a few
        let columns = gtk::FlowBox::new();
        columns.add_css_class("workspaces");
        columns.set_selection_mode(gtk::SelectionMode::None);
        columns.set_homogeneous(false);
        columns.set_halign(gtk::Align::Center);
        columns.set_valign(gtk::Align::Start);
        columns.set_column_spacing(18);
        columns.set_row_spacing(18);
        columns.set_max_children_per_line(OVERVIEW_GROUPS_PER_LINE.min(groups.len().max(1) as u32));
        columns.set_min_children_per_line(OVERVIEW_GROUPS_PER_LINE.min(groups.len().max(1) as u32));
        for group in groups {
            let grid = gtk::FlowBox::new();
            grid.add_css_class("tiles");
            grid.set_selection_mode(gtk::SelectionMode::None);
            grid.set_homogeneous(true);
            grid.set_activate_on_single_click(true);
            grid.set_column_spacing(10);
            grid.set_row_spacing(10);
            grid.set_halign(gtk::Align::Center);
            grid.set_valign(gtk::Align::Start);
            let per_line = if overview { if group.items.len() > 3 { 2 } else { 1 } } else { TILE_COLUMNS };
            let n = group.items.len().clamp(1, per_line) as u32;
            grid.set_min_children_per_line(n);
            grid.set_max_children_per_line(n);
            let ui = self.clone();
            grid.connect_child_activated(move |_, child| {
                let i = ui.state.borrow().tiles.iter().position(|t| &t.child == child);
                if let Some(i) = i {
                    ui.activate(i);
                }
            });
            for item in group.items {
                let t = tile(self, &item, size);
                grid.append(&t.child);
                if previews && let Some(id) = &t.preview {
                    t.load(&preview_path(id));
                }
                tiles.push(t);
                items.push(item);
            }
            if overview {
                let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
                column.add_css_class("workspace");
                column.set_valign(gtk::Align::Start);
                column.append(&label(&group.title, "group-title"));
                column.append(&grid);
                let child = gtk::FlowBoxChild::new();
                child.set_focusable(false);
                child.set_valign(gtk::Align::Start);
                child.set_child(Some(&column));
                columns.append(&child);
            } else {
                self.groups.append(&grid);
            }
        }
        if overview {
            self.groups.append(&columns);
        }
        // as tall as the screen allows, then it scrolls
        let max = self.window.surface().and_then(|surface| WidgetExt::display(&self.window).monitor_at_surface(&surface));
        let max = max.map_or(860, |m| m.geometry().height() * 85 / 100);
        self.groups_scroll.set_max_content_height(max);
        let count = items.len();
        // the previous window, like every other switcher
        let first = match windows.iter().find(|c| c.focus == 1) {
            Some(prev) if overview => {
                items.iter().position(|i| i.action == crate::item::Action::Focus(prev.address.clone())).unwrap_or(0)
            }
            _ => usize::from(count > 1),
        };
        {
            let mut s = self.state.borrow_mut();
            s.items = items;
            s.tiles = tiles;
            // the list's rows no longer match `items`: the next render must rebuild them
            s.rows.clear();
            s.sections.clear();
        }
        if count == 0 {
            self.groups.append(&label("no open windows", "empty"));
            return;
        }
        self.select(first);
    }

    /// A fresh screenshot of `id` landed.
    fn update_tile(&self, id: &str) {
        let s = self.state.borrow();
        for t in s.tiles.iter().filter(|t| t.preview.as_deref() == Some(id)) {
            t.load(&preview_path(id));
        }
    }

    fn step_stop(&self, delta: isize) -> glib::Propagation {
        self.step(delta);
        glib::Propagation::Stop
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
        if s.strip {
            if let Some(t) = s.tiles.get(s.selected) {
                t.child.remove_css_class("selected");
            }
            s.selected = i;
            if let Some(t) = s.tiles.get(i) {
                t.child.add_css_class("selected");
                let adj = self.groups_scroll.vadjustment();
                if let Some(b) = t.child.compute_bounds(&self.groups) {
                    let (y, h) = (f64::from(b.y()), f64::from(b.height()));
                    if y < adj.value() {
                        adj.set_value(y - 40.0);
                    } else if y + h > adj.value() + adj.page_size() {
                        adj.set_value(y + h - adj.page_size());
                    }
                }
            }
            return;
        }
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

    /// The list keeps the configured width; the preview column gets exactly the extra space.
    fn fit_panel(&self, column: bool) {
        let width = config::get().width;
        // on the list itself: with no horizontal scrollbar GTK ignores min-content-width
        self.list.set_size_request(width, -1);
        let preview = self.preview_width();
        self.preview.pane.set_size_request(preview, -1);
        self.panel.set_size_request(if column { width + preview } else { width }, -1);
        self.state.borrow_mut().preview_column = column;
        if !column {
            self.preview.pane.set_visible(false);
        }
    }

    fn preview_width(&self) -> i32 {
        if self.state.borrow().switch { SWITCH_PREVIEW_WIDTH } else { PREVIEW_WIDTH }
    }

    fn show_preview(&self) {
        let s = self.state.borrow();
        if !s.preview_column {
            self.preview.pane.set_visible(false);
            return;
        }
        let Some(item) = s.items.get(s.selected) else { return };
        if let Some(text) = &item.text {
            let body = match text {
                crate::item::Text::Literal(body) => body.clone(),
                crate::item::Text::Command(cmd) => self.preview_text(cmd),
            };
            let lines = body.lines().count();
            self.preview.text.buffer().set_text(&body);
            self.preview.text_scroll.vadjustment().set_value(0.0);
            self.preview.picture.set_visible(false);
            self.preview.text_scroll.set_visible(true);
            self.preview.title.set_text(&format!("{} {}", lines, if lines == 1 { "line" } else { "lines" }));
            self.preview.meta.set_text(&format!("{} characters", body.chars().count()));
            self.preview.pane.set_visible(true);
            return;
        }
        self.preview.text_scroll.set_visible(false);
        self.preview.picture.set_visible(true);
        // an image result, or the window screenshot in ALT+TAB
        if let (Some(img), Some(cmd)) = (&item.image, &item.image_cmd)
            && !Path::new(img).exists()
        {
            let _ = crate::platform::shell(cmd).status();
        }
        let (path, meta) = match (&item.image, &item.preview) {
            (Some(img), _) => (Some(PathBuf::from(img)), image_meta(Path::new(img))),
            (None, Some(id)) if s.switch => (Some(preview_path(id)), format!("{}  ·  {}", item.subtitle, item.hint)),
            _ => (None, "no preview".to_owned()),
        };
        let texture = path.filter(|p| p.exists()).and_then(|p| {
            scaled_texture(&p, self.preview_width() - 36, config::get().height, self.preview.picture.scale_factor())
        });
        self.preview.picture.set_paintable(texture.as_ref());
        self.preview.title.set_text(&item.title);
        self.preview.meta.set_text(&meta);
        self.preview.pane.set_visible(true);
    }

    /// Output of a text-preview command, cut at 50k characters.
    fn preview_text(&self, cmd: &str) -> String {
        if let Some(cached) = self.preview.text_cache.borrow().get(cmd) {
            return cached.clone();
        }
        let out = crate::platform::shell(cmd).output();
        let text: String = out.map(|o| String::from_utf8_lossy(&o.stdout).chars().take(50_000).collect()).unwrap_or_default();
        self.preview.text_cache.borrow_mut().insert(cmd.to_owned(), text.clone());
        text
    }

    fn activate(self: &Rc<Self>, i: usize) {
        let Some(item) = self.state.borrow().items.get(i).cloned() else { return };
        if let crate::item::Action::Set { key, .. } = &item.action
            && let Some(target) = self.state.borrow().menu.as_ref().map(|m| m.item.clone())
            && (key == actions::FAVORITE || key == actions::KEYBIND)
        {
            self.close_menu();
            if key == actions::FAVORITE {
                self.toggle_favorite_item(Some(target));
            } else {
                self.start_recording_item(Some(target));
            }
            return;
        }
        if let crate::item::Action::Window { address, op } = &item.action {
            self.window_action(address, op);
            return;
        }
        let item = match self.state.borrow().menu.as_ref() {
            // history and the launch log keep the menu's item, not "Copy path"
            Some(menu) => Item { action: item.action, ..menu.item.clone() },
            None => item,
        };
        if matches!(item.action, crate::item::Action::Fill(_)) {
            self.state.borrow_mut().menu = None;
            self.entry.set_placeholder_text(Some(PLACEHOLDER));
        }
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
        pane.set_hexpand(false);
        pane.set_visible(false);
        let picture = gtk::Picture::new();
        picture.set_content_fit(gtk::ContentFit::Contain);
        picture.set_can_shrink(true);
        picture.set_vexpand(true);
        // ellipsized labels still ask for their full width; cap it so long titles can't grow the pane
        let title = label("", "preview-title");
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_max_width_chars(36);
        let meta = label("", "preview-meta");
        meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
        meta.set_max_width_chars(36);
        let text = gtk::TextView::builder()
            .editable(false)
            .cursor_visible(false)
            .wrap_mode(gtk::WrapMode::WordChar)
            .monospace(true)
            .build();
        text.add_css_class("preview-text");
        let text_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&text)
            .build();
        text_scroll.add_css_class("preview-text-box");
        pane.append(&picture);
        pane.append(&text_scroll);
        pane.append(&title);
        pane.append(&meta);
        Preview { pane, picture, text, text_scroll, text_cache: RefCell::default(), title, meta }
    }
}

/// The image shrunk to the preview pane (in device pixels, so it stays sharp on HiDPI),
/// so big photos don't stall the list.
fn scaled_texture(path: &Path, width: i32, height: i32, scale: i32) -> Option<gdk::Texture> {
    let scale = scale.max(1);
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(path, width * scale, height * scale, true).ok()?;
    #[allow(deprecated)]
    Some(gdk::Texture::for_pixbuf(&pixbuf))
}

/// `1920×1080 · png · 2.4 MB`
fn image_meta(path: &Path) -> String {
    let mut parts = Vec::new();
    if let Some((format, w, h)) = gtk::gdk_pixbuf::Pixbuf::file_info(path) {
        parts.push(format!("{w}×{h}"));
        if let Some(name) = format.name() {
            parts.push(name.to_string());
        }
    }
    if let Ok(meta) = std::fs::metadata(path) {
        let kb = meta.len() as f64 / 1024.0;
        parts.push(if kb < 1024.0 { format!("{kb:.0} KB") } else { format!("{:.1} MB", kb / 1024.0) });
    }
    parts.join("  ·  ")
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
            let ok = crate::platform::capture_window(&id, &tmp);
            if ok && std::fs::rename(&tmp, &path).is_ok() {
                let _ = tx.send_blocking(id);
            }
        });
    }
}

impl Tile {
    /// Show the screenshot at `path`, decoded at the tile's size so a big window stays cheap.
    /// Not times the scale factor: a texture's pixel size is also the picture's natural size.
    fn load(&self, path: &Path) {
        let Some(texture) = scaled_texture(path, self.size.0, self.size.1, 1) else { return };
        self.picture.set_paintable(Some(&texture));
        self.placeholder.set_visible(false);
    }
}

fn tile(ui: &Rc<Ui>, item: &Item, size: (i32, i32)) -> Tile {
    let picture = gtk::Picture::new();
    picture.set_content_fit(gtk::ContentFit::Contain);
    picture.set_can_shrink(true);
    picture.set_size_request(size.0, size.1);
    let placeholder: gtk::Widget = match icon(item) {
        Some(image) => {
            image.set_pixel_size(64);
            image.upcast()
        }
        None => label(&item.glyph, "tile-glyph").upcast(),
    };
    placeholder.set_halign(gtk::Align::Center);
    placeholder.set_valign(gtk::Align::Center);
    let thumb = gtk::Overlay::new();
    thumb.add_css_class("thumb");
    thumb.set_child(Some(&picture));
    thumb.add_overlay(&placeholder);
    // × on hover closes the window, like Mission Control
    if let crate::item::Action::Focus(address) = &item.action {
        let close = gtk::Button::with_label("✕");
        close.add_css_class("tile-close");
        close.set_halign(gtk::Align::End);
        close.set_valign(gtk::Align::Start);
        close.set_focusable(false);
        close.set_tooltip_text(Some("close  ^W"));
        let (ui, address) = (ui.clone(), address.clone());
        close.connect_clicked(move |_| ui.window_action(&address, &crate::item::WindowOp::Close));
        thumb.add_overlay(&close);
    }

    let caption = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    caption.add_css_class("caption");
    caption.set_halign(gtk::Align::Center);
    if let Some(image) = icon(item) {
        image.set_pixel_size(16);
        caption.append(&image);
    }
    let title = label(&item.title, "tile-title");
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(if size.0 < TILE_WIDTH { 18 } else { 24 });
    caption.append(&title);

    let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
    b.append(&thumb);
    b.append(&caption);
    let child = gtk::FlowBoxChild::new();
    child.add_css_class("tile");
    child.set_child(Some(&b));
    Tile { child, picture, placeholder, preview: item.preview.clone(), size }
}

/// Windows per workspace in workspace order, special ones (minimized, scratchpads) last.
fn by_workspace(items: Vec<Item>, windows: &[Client]) -> Vec<Group> {
    let current = windows.iter().find(|c| c.focus == 0).map(|c| c.workspace.id);
    let mut groups: Vec<(i64, Group)> = Vec::new();
    for item in items {
        let client = match &item.action {
            crate::item::Action::Focus(address) => windows.iter().find(|c| &c.address == address),
            _ => None,
        };
        let (id, name) = client.map_or((i64::MAX, String::new()), |c| (c.workspace.id, c.workspace.name.clone()));
        let order = if id < 0 { i64::MAX / 2 - id } else { id };
        match groups.iter_mut().find(|(o, _)| *o == order) {
            Some((_, g)) => g.items.push(item),
            None => {
                let title = match name.strip_prefix("special:") {
                    Some(special) => special.to_owned(),
                    None if Some(id) == current => format!("workspace {name}  ·  here"),
                    None => format!("workspace {name}"),
                };
                groups.push((order, Group { title, items: vec![item] }));
            }
        }
    }
    groups.sort_by_key(|(order, _)| *order);
    groups.into_iter().map(|(_, g)| g).collect()
}

fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn icon(item: &Item) -> Option<gtk::Image> {
    let icon = item.icon.as_str();
    if icon.is_empty() {
        return None;
    }
    let image = if Path::new(icon).is_absolute() {
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
    fn open_settings(self: &Rc<Self>, page: Option<&str>) {
        let Some(app) = self.window.application() else { return };
        let info = {
            let s = self.state.borrow();
            settings_window::Info { providers: s.engine.provider_titles(), plugins: s.engine.plugins.list().to_vec() }
        };
        let ui = self.clone();
        let changed: Rc<dyn Fn()> = Rc::new(move || {
            apply_config_css();
            crate::platform::sync_tray();
            let mut s = ui.state.borrow_mut();
            s.engine.favorites = store::Favorites::load();
            s.engine.binds = store::Binds::load();
        });
        self.hide();
        settings_window::open(&app, info, changed, page);
    }

    fn open_changelog(self: &Rc<Self>) {
        let Some(app) = self.window.application() else { return };
        let ui = self.clone();
        self.hide();
        crate::changelog_window::open(&app, Rc::new(move |query: &str| ui.command(&format!("show {query}"))));
    }

    fn selected_item(&self) -> Option<Item> {
        let s = self.state.borrow();
        s.items.get(s.selected).cloned()
    }

    fn toggle_favorite(self: &Rc<Self>) {
        self.toggle_favorite_item(self.selected_item());
    }

    fn toggle_favorite_item(self: &Rc<Self>, item: Option<Item>) {
        // a clipboard entry is pinned by its content, decoded now
        let item = item.and_then(|i| match &i.pin {
            Some(line) => crate::providers::clipboard::pinned(line, &i),
            None => Some(i),
        });
        let Some(item) = item.filter(|i| !i.id.is_empty()) else {
            self.status.set_text("can't favorite this one");
            return;
        };
        let added = self.state.borrow_mut().engine.favorites.toggle(&item);
        let selected = self.state.borrow().selected;
        self.refresh();
        self.select(selected);
        self.status.set_text(if added { "★ added to favorites" } else { "removed from favorites" });
    }

    fn start_recording(&self) {
        self.start_recording_item(self.selected_item());
    }

    fn start_recording_item(&self, item: Option<Item>) {
        let Some(item) = item.filter(|i| !i.id.is_empty()) else {
            self.status.set_text("can't bind this one");
            return;
        };
        self.status.set_text("press a shortcut · ⌫ remove · esc cancel");
        self.state.borrow_mut().recording = Some(item);
    }

    /// A key arrived while recording a keybind.
    fn record(self: &Rc<Self>, key: gdk::Key, mods: gdk::ModifierType) {
        let item = match key {
            gdk::Key::Escape => {
                self.state.borrow_mut().recording = None;
                self.status.set_text("cancelled");
                return;
            }
            gdk::Key::BackSpace => {
                let Some(item) = self.state.borrow_mut().recording.take() else { return };
                let result = self.state.borrow_mut().engine.binds.remove(&item.id);
                self.finish_bind(result.map(|_| format!("removed shortcut of {}", item.title)));
                return;
            }
            _ => match combo(key, mods) {
                Some(combo) => (combo, self.state.borrow_mut().recording.take()),
                None => return, // a lone modifier, or no modifier at all: keep waiting
            },
        };
        let (combo, Some(item)) = item else { return };
        let result = self.state.borrow_mut().engine.binds.set(&combo, &item);
        self.finish_bind(result.map(|_| format!("{combo} → {}", item.title)));
    }

    fn finish_bind(self: &Rc<Self>, result: Result<String, String>) {
        let selected = self.state.borrow().selected;
        self.refresh();
        self.select(selected);
        match result {
            Ok(msg) => self.status.set_text(&msg),
            Err(e) if cfg!(target_os = "linux") => self.status.set_text(&format!("hyprland: {e}")),
            Err(e) => self.status.set_text(&e),
        }
    }

    /// `zunder run <key>`, called by the generated Hyprland binds (on Windows, by the hotkey thread).
    fn run_bind(self: &Rc<Self>, key: &str) {
        let Some(item) = self.state.borrow().engine.binds.find(key).cloned() else {
            eprintln!("zunder: no keybind {key}");
            return;
        };
        match &item.action {
            crate::item::Action::Fill(text) => self.command(&format!("show {text}")),
            action => {
                self.state.borrow_mut().engine.history.bump(&item.id);
                store::log_launch(&item);
                action.execute();
            }
        }
    }

    /// Ctrl+K: the selected item's actions replace the results; typing filters them.
    fn open_menu(self: &Rc<Self>) {
        let Some(item) = self.selected_item() else { return };
        let from_strip = self.state.borrow().strip;
        if from_strip {
            {
                let mut s = self.state.borrow_mut();
                s.strip = false;
                s.tiles.clear();
            }
            clear(&self.groups);
            self.switcher.set_visible(false);
            self.panel.set_visible(true);
        }
        let query = self.entry.text().to_string();
        self.entry.set_placeholder_text(Some(&format!("actions for {}", item.title.replace('\n', " "))));
        self.state.borrow_mut().menu = Some(Menu { item, query, from_strip });
        // set_text("") doesn't fire `changed` when it's already empty
        self.entry.set_text("");
        self.refresh();
        self.entry.grab_focus();
    }

    /// Back from the menu to the results (or the grid) it was opened on.
    fn close_menu(self: &Rc<Self>) {
        let Some(menu) = self.state.borrow_mut().menu.take() else { return };
        self.entry.set_placeholder_text(Some(PLACEHOLDER));
        if menu.from_strip {
            self.state.borrow_mut().strip = true;
            self.panel.set_visible(false);
            self.switcher.set_visible(true);
        }
        self.entry.set_text(&menu.query);
        self.entry.set_position(-1);
        self.refresh();
        let address = match &menu.item.action {
            crate::item::Action::Focus(a) => Some(a.clone()),
            _ => None,
        };
        let back = self.state.borrow().items.iter().position(|i| i == &menu.item || address.as_ref().is_some_and(|a| i.action == crate::item::Action::Focus(a.clone())));
        if let Some(i) = back {
            self.select(i);
        }
    }

    fn render_menu(&self, query: &str) {
        let (item, windows) = {
            let s = self.state.borrow();
            let Some(menu) = &s.menu else { return };
            (menu.item.clone(), s.windows.clone())
        };
        let favorite = self.state.borrow().engine.favorites.contains(&item.id);
        let entries = actions::menu(&item, &windows, favorite, query);
        let title = item.title.lines().next().unwrap_or_default().chars().take(60).collect::<String>();
        self.render(vec![Section { title, items: entries }], false);
        self.status.set_text("esc back");
    }

    /// Ctrl+W / Ctrl+M / Ctrl+1…9 / Ctrl+Enter on a selected window. False if it isn't one.
    fn window_key(self: &Rc<Self>, key: gdk::Key) -> bool {
        use crate::item::WindowOp;
        if self.state.borrow().menu.is_some() {
            return false;
        }
        let Some(crate::item::Action::Focus(address)) = self.selected_item().map(|i| i.action) else { return false };
        let op = match key {
            gdk::Key::w => WindowOp::Close,
            gdk::Key::m => WindowOp::Minimize,
            gdk::Key::Return | gdk::Key::KP_Enter => WindowOp::Pull,
            k => match k.to_unicode().and_then(|c| c.to_digit(10)).filter(|d| *d > 0) {
                Some(n) => WindowOp::Move(n.to_string()),
                None => return false,
            },
        };
        self.window_action(&address, &op);
        true
    }

    /// Close, move or minimize stay in zunder so you can tidy up more windows; the rest
    /// leave it like Enter does.
    fn window_action(self: &Rc<Self>, address: &str, op: &crate::item::WindowOp) {
        use crate::item::WindowOp;
        let action = crate::item::Action::Window { address: address.to_owned(), op: op.clone() };
        if !op.keeps_open() {
            self.hide();
            glib::idle_add_local_once(move || action.execute());
            return;
        }
        let title = {
            let s = self.state.borrow();
            s.windows.iter().find(|c| c.address == address).map(|c| if c.title.is_empty() { c.class.clone() } else { c.title.clone() })
        };
        action.execute();
        let selected = self.state.borrow().selected;
        self.close_menu();
        let what = match op {
            WindowOp::Close => "closed".to_owned(),
            WindowOp::Minimize => "minimized".to_owned(),
            WindowOp::Move(ws) => format!("moved to workspace {ws}"),
            WindowOp::Pull | WindowOp::Fullscreen => String::new(),
        };
        let note = format!("{} {what}", title.unwrap_or_default().chars().take(40).collect::<String>());
        // Hyprland's event reaches the window list cache a moment later
        let ui = self.clone();
        let address = address.to_owned();
        glib::timeout_add_local_once(Duration::from_millis(200), move || {
            if !ui.window.is_visible() {
                return;
            }
            ui.state.borrow_mut().windows = windows::list();
            ui.refresh();
            // a moved window is still there: stay on it; a closed one hands over to its neighbour
            let (n, same) = {
                let s = ui.state.borrow();
                let focus = crate::item::Action::Focus(address.clone());
                (s.items.len(), s.items.iter().position(|i| i.action == focus))
            };
            if n > 0 {
                ui.select(same.unwrap_or(selected.min(n - 1)));
            }
            ui.status.set_text(&note);
            if ui.state.borrow().strip {
                ui.flash_hint(&note);
            }
        });
    }

    /// A short note under the grid (there's no status line there), then the key hints again.
    fn flash_hint(self: &Rc<Self>, note: &str) {
        self.switch_hint.set_text(note);
        self.switch_hint.set_visible(true);
        let ui = self.clone();
        glib::timeout_add_local_once(Duration::from_millis(2500), move || {
            ui.switch_hint.set_text(SWITCH_HINT);
            ui.switch_hint.set_visible(ui.state.borrow().hold);
        });
    }

    /// Arrow keys in the overview: the nearest tile in that direction, by where tiles are on
    /// screen (workspaces have different numbers of windows, so there are no rows to count).
    fn step_toward(&self, dx: f32, dy: f32) -> glib::Propagation {
        let center = |t: &Tile| {
            t.child.compute_bounds(&self.groups).map(|b| (b.x() + b.width() / 2.0, b.y() + b.height() / 2.0))
        };
        let best = {
            let s = self.state.borrow();
            let Some(from) = s.tiles.get(s.selected).and_then(center) else { return glib::Propagation::Stop };
            s.tiles
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != s.selected)
                .filter_map(|(i, t)| {
                    let (x, y) = center(t)?;
                    let (ox, oy) = (x - from.0, y - from.1);
                    // along the arrow, and the sideways drift counts double
                    let ahead = ox * dx + oy * dy;
                    let side = (ox * dy).abs() + (oy * dx).abs();
                    (ahead > 1.0).then_some((ahead + 2.0 * side, i))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, i)| i)
        };
        if let Some(i) = best {
            self.select(i);
        }
        glib::Propagation::Stop
    }

    fn set_setting(self: &Rc<Self>, key: &str, value: Option<&str>) {
        if let Some(name) = key.strip_prefix("setup:") {
            let note = crate::providers::setup::set(name);
            if name == "apply" {
                // setup may have changed the config (first run ends) and added plugins
                config::reload();
                apply_config_css();
                crate::platform::sync_tray();
                self.state.borrow_mut().engine.reload();
            }
            let selected = self.state.borrow().selected;
            self.refresh();
            let n = self.state.borrow().items.len();
            self.select(if name == "apply" || name == "again" { 0 } else { selected.min(n.saturating_sub(1)) });
            if !note.is_empty() {
                self.status.set_text(&note);
            }
            return;
        }
        let mut result = Ok(());
        config::update(|c| match key.strip_prefix("provider:") {
            Some(provider) => c.toggle_provider(provider),
            None => result = c.set(key, value),
        });
        apply_config_css();
        crate::platform::sync_tray();
        let height = config::get().height;
        // read first: the borrow in the argument would still be alive inside fit_panel's borrow_mut
        let column = self.state.borrow().preview_column;
        self.fit_panel(column);
        self.scroll.set_min_content_height(height);
        self.scroll.set_max_content_height(height);
        // back to the list, keeping the selection on the changed row
        let selected = self.state.borrow().selected;
        if value.is_some() && self.entry.text().starts_with("settings") {
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

/// Hyprland combo for a key event, e.g. `SUPER + SHIFT + F`. Needs a modifier (F-keys don't).
fn combo(key: gdk::Key, mods: gdk::ModifierType) -> Option<String> {
    let name = key.to_lower().name()?.to_string();
    let modifier_keys = ["Shift", "Control", "Alt", "Super", "Meta", "Hyper", "ISO_Level3", "Caps_Lock"];
    if modifier_keys.iter().any(|m| name.starts_with(m)) {
        return None;
    }
    let mut parts = Vec::new();
    if mods.intersects(gdk::ModifierType::SUPER_MASK | gdk::ModifierType::META_MASK) {
        parts.push("SUPER".to_owned());
    }
    if mods.contains(gdk::ModifierType::CONTROL_MASK) {
        parts.push("CTRL".to_owned());
    }
    if mods.contains(gdk::ModifierType::ALT_MASK) {
        parts.push("ALT".to_owned());
    }
    let real_modifier = !parts.is_empty();
    if mods.contains(gdk::ModifierType::SHIFT_MASK) {
        parts.push("SHIFT".to_owned());
    }
    let fkey = name.strip_prefix('F').is_some_and(|n| n.parse::<u8>().is_ok());
    if !real_modifier && !fkey {
        return None;
    }
    parts.push(if name.chars().count() == 1 { name.to_uppercase() } else { name });
    Some(parts.join(" + "))
}

/// `ZUNDER_TIMING=1 zunder server` prints how long each step takes, for keeping zunder fast.
fn timing(what: &str, since: std::time::Instant) {
    static ON: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var_os("ZUNDER_TIMING").is_some());
    if *ON {
        eprintln!("zunder: {what:<24} {:>7.2} ms", since.elapsed().as_secs_f64() * 1000.0);
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

#[cfg(test)]
mod tests {
    use super::{combo, gdk};
    use gdk::ModifierType as M;

    #[test]
    fn builds_hyprland_combos() {
        assert_eq!(combo(gdk::Key::f, M::SUPER_MASK).as_deref(), Some("SUPER + F"));
        assert_eq!(combo(gdk::Key::F, M::SUPER_MASK | M::SHIFT_MASK).as_deref(), Some("SUPER + SHIFT + F"));
        assert_eq!(combo(gdk::Key::F5, M::empty()).as_deref(), Some("F5"));
        assert_eq!(combo(gdk::Key::Return, M::CONTROL_MASK | M::ALT_MASK).as_deref(), Some("CTRL + ALT + Return"));
        assert_eq!(combo(gdk::Key::a, M::empty()), None);
        assert_eq!(combo(gdk::Key::A, M::SHIFT_MASK), None);
        assert_eq!(combo(gdk::Key::Super_L, M::SUPER_MASK), None);
    }
}
