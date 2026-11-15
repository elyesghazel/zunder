//! The settings window: a sidebar of pages on the left, the settings of the page on the right.
//! A normal toplevel window (not the overlay), styled like the launcher.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gtk::{gdk, glib, prelude::*};
use gtk4 as gtk;

use crate::config::{self, Kind, SETTINGS};
use crate::{auth, sync};
use crate::item::{Action, Item};
use crate::providers::plugins::{self, Plugin};
use crate::registry::{Catalog, Registry, Report};
use crate::store::{Binds, Favorites};

/// What the window can't read from disk by itself.
pub struct Info {
    pub providers: Vec<String>,
    pub plugins: Vec<Plugin>,
    /// Rescan the plugins folder in the launcher and return what it found.
    pub reload_plugins: Rc<dyn Fn() -> Vec<Plugin>>,
}

thread_local! {
    static WINDOW: RefCell<Option<gtk::Window>> = const { RefCell::new(None) };
    static CSS: gtk::CssProvider = gtk::CssProvider::new();
    /// Per-plugin colours on the Plugins page, rewritten on every redraw.
    static TINTS: gtk::CssProvider = gtk::CssProvider::new();
}

const PAGES: &[(&str, &str)] = &[
    ("General", "\u{f013}"),
    ("Switcher", "\u{f2d0}"),
    ("Start screen", "\u{f015}"),
    ("Appearance", "\u{f1fc}"),
    ("Providers", "\u{f0c9}"),
    ("Favorites", "\u{f005}"),
    ("Shortcuts", "\u{f11c}"),
    ("Plugins", "\u{f12e}"),
    ("Account", "\u{f2bd}"),
    ("About", "\u{f05a}"),
];

/// Open (or rebuild and raise) the settings window. `changed` runs after anything is saved.
pub fn open(app: &gtk::Application, info: Info, changed: Rc<dyn Fn()>, page: Option<&str>) {
    load_css();
    if let Some(old) = WINDOW.with(|w| w.borrow_mut().take()) {
        old.destroy();
    }

    let window = gtk::Window::builder()
        .application(app)
        .title("zunder settings")
        .default_width(980)
        .default_height(660)
        .build();
    window.add_css_class("zunder-settings");

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_hexpand(true);
    let info = Rc::new(info);
    for (name, _) in PAGES {
        let page = build_page(name, &info, &changed);
        let scroll = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).child(&page).build();
        stack.add_named(&scroll, Some(name));
    }

    let sidebar = gtk::ListBox::new();
    sidebar.add_css_class("sidebar");
    sidebar.set_selection_mode(gtk::SelectionMode::Single);
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    header.add_css_class("brand");
    header.append(&label("❯", "caret"));
    header.append(&label("zunder", "brand-name"));
    let markers: Rc<Vec<gtk::Label>> = Rc::new(
        PAGES
            .iter()
            .map(|(name, glyph)| {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                row.add_css_class("side-item");
                let marker = label("▏", "marker");
                row.append(&marker);
                row.append(&label(glyph, "glyph"));
                row.append(&label(name, "side-title"));
                sidebar.append(&row);
                marker
            })
            .collect(),
    );
    let stack2 = stack.clone();
    let markers2 = markers.clone();
    sidebar.connect_row_selected(move |_, row| {
        let Some(row) = row else { return };
        let i = row.index() as usize;
        for (j, m) in markers2.iter().enumerate() {
            m.set_text(if i == j { "▌" } else { "▏" });
        }
        stack2.set_visible_child_name(PAGES[i].0);
    });
    let start = page
        .and_then(|p| PAGES.iter().position(|(name, _)| name.to_lowercase().starts_with(&p.to_lowercase())))
        .unwrap_or(0);
    sidebar.select_row(sidebar.row_at_index(start as i32).as_ref());

    let side = gtk::Box::new(gtk::Orientation::Vertical, 0);
    side.add_css_class("side");
    side.append(&header);
    side.append(&sidebar);
    let hint = label(&config::path().display().to_string().replace(&home(), "~"), "side-path");
    hint.set_vexpand(true);
    hint.set_valign(gtk::Align::End);
    hint.set_wrap(true);
    side.append(&hint);

    let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    body.add_css_class("settings-body");
    body.append(&side);
    body.append(&stack);
    window.set_child(Some(&body));

    let keys = gtk::EventControllerKey::new();
    let w = window.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            w.close();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(keys);
    window.connect_close_request(|_| {
        WINDOW.with(|w| w.borrow_mut().take());
        glib::Propagation::Proceed
    });

    window.present();
    WINDOW.with(|w| *w.borrow_mut() = Some(window));
}

/// The window styles, shared with the changelog window. Loaded once.
pub fn load_css() {
    CSS.with(|css| {
        if css.to_str().is_empty() {
            css.load_from_string(include_str!("settings.css"));
            if let Some(display) = gdk::Display::default() {
                gtk::style_context_add_provider_for_display(&display, css, gtk::STYLE_PROVIDER_PRIORITY_USER);
                TINTS.with(|t| gtk::style_context_add_provider_for_display(&display, t, gtk::STYLE_PROVIDER_PRIORITY_USER + 1));
            }
        }
    });
}

fn home() -> String {
    dirs::home_dir().unwrap_or_default().display().to_string()
}

pub fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class(class);
    l.set_xalign(0.0);
    l
}

pub fn page_box(title: &str, intro: &str) -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.add_css_class("page");
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    head.add_css_class("page-head");
    head.append(&label(title, "page-title"));
    let rule = gtk::Separator::new(gtk::Orientation::Horizontal);
    rule.set_hexpand(true);
    rule.set_valign(gtk::Align::Center);
    head.append(&rule);
    page.append(&head);
    if !intro.is_empty() {
        let intro = label(intro, "page-intro");
        intro.set_wrap(true);
        page.append(&intro);
    }
    page
}

/// One setting line: title and help on the left, the control on the right.
fn row(title: &str, help: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    row.add_css_class("setting");
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.append(&label(title, "setting-title"));
    if !help.is_empty() {
        let help = label(help, "setting-help");
        help.set_wrap(true);
        text.append(&help);
    }
    row.append(&text);
    control.set_valign(gtk::Align::Center);
    row.append(control);
    row
}

fn save(key: &str, value: Option<&str>, changed: &Rc<dyn Fn()>, status: &gtk::Label) {
    let mut result = Ok(());
    config::update(|c| result = c.set(key, value));
    match result {
        Ok(()) => {
            status.set_text("saved");
            changed();
        }
        Err(e) => status.set_text(&e),
    }
}

fn setting_control(key: &'static str, changed: &Rc<dyn Fn()>, status: &gtk::Label) -> gtk::Widget {
    let cfg = config::get();
    let value = cfg.value(key).unwrap_or_default();
    let kind = cfg.kind(key);
    drop(cfg);
    let changed = changed.clone();
    let status = status.clone();
    if let Some(choices) = config::choices(key) {
        let strings: Vec<&str> = choices.iter().map(String::as_str).collect();
        let drop = gtk::DropDown::from_strings(&strings);
        if let Some(i) = choices.iter().position(|c| *c == value) {
            drop.set_selected(i as u32);
        }
        drop.connect_selected_notify(move |d| {
            if let Some(choice) = choices.get(d.selected() as usize) {
                save(key, Some(choice), &changed, &status);
            }
        });
        return drop.upcast();
    }
    match kind {
        Kind::Bool => {
            let sw = gtk::Switch::new();
            sw.set_active(value == "on");
            sw.connect_state_set(move |_, on| {
                save(key, Some(if on { "on" } else { "off" }), &changed, &status);
                glib::Propagation::Proceed
            });
            sw.upcast()
        }
        Kind::Int | Kind::Float => {
            let (min, max, step, digits) = match key {
                "start_apps" => (0.0, 200.0, 1.0, 0),
                "width" => (480.0, 3000.0, 10.0, 0),
                "height" => (160.0, 2000.0, 10.0, 0),
                "font_size" => (6.0, 32.0, 0.5, 1),
                "dim" => (0.0, 1.0, 0.05, 2),
                _ => (0.0, 10_000.0, 1.0, if kind == Kind::Float { 2 } else { 0 }),
            };
            let spin = gtk::SpinButton::with_range(min, max, step);
            spin.set_digits(digits);
            spin.set_value(value.parse().unwrap_or(min));
            spin.connect_value_changed(move |s| {
                let v = if digits == 0 { format!("{}", s.value() as i64) } else { format!("{}", s.value()) };
                save(key, Some(&v), &changed, &status);
            });
            spin.upcast()
        }
        Kind::Text => {
            let entry = gtk::Entry::new();
            entry.set_text(&value);
            entry.set_width_chars(28);
            entry.set_placeholder_text(Some("default"));
            let apply = move |e: &gtk::Entry| save(key, Some(&e.text()), &changed, &status);
            let apply2 = apply.clone();
            entry.connect_activate(move |e| apply(e));
            let focus = gtk::EventControllerFocus::new();
            let e2 = entry.clone();
            focus.connect_leave(move |_| apply2(&e2));
            entry.add_controller(focus);
            entry.upcast()
        }
    }
}

fn build_page(name: &str, info: &Rc<Info>, changed: &Rc<dyn Fn()>) -> gtk::Box {
    let status = label("", "status");
    let page = match name {
        "Providers" => providers_page(info, changed, &status),
        "Favorites" => favorites_page(changed),
        "Shortcuts" => shortcuts_page(changed, &status),
        "Plugins" => plugins_page(info, changed, &status),
        "Account" => account_page(changed, &status),
        "About" => about_page(),
        group => {
            let intro = match group {
                "General" => "Programs zunder hands things to.",
                "Switcher" => "How ALT+TAB behaves.",
                "Start screen" => "What you see before you type anything.",
                "Appearance" => "Changes apply right away. For more, edit style.css (About page).",
                _ => "",
            };
            let page = page_box(group, intro);
            for s in SETTINGS.iter().filter(|s| s.group == group) {
                page.append(&row(s.title, s.help, &setting_control(s.key, changed, &status)));
            }
            page
        }
    };
    page.append(&status);
    page
}

fn providers_page(info: &Rc<Info>, changed: &Rc<dyn Fn()>, status: &gtk::Label) -> gtk::Box {
    let page = page_box("Providers", "Turn whole sections of results on or off.");
    for p in &info.providers {
        let sw = gtk::Switch::new();
        sw.set_active(config::get().enabled(p));
        let (p2, changed, status) = (p.clone(), changed.clone(), status.clone());
        sw.connect_state_set(move |_, on| {
            config::update(|c| {
                if c.enabled(&p2) != on {
                    c.toggle_provider(&p2);
                }
            });
            status.set_text("saved");
            changed();
            glib::Propagation::Proceed
        });
        page.append(&row(p, "", &sw));
    }
    page
}

/// A list row with a remove button that deletes itself when clicked.
fn removable(title: &str, sub: &str, on_remove: impl Fn() + 'static) -> gtk::Box {
    let button = gtk::Button::with_label("remove");
    button.add_css_class("flat-button");
    let row = row(title, sub, &button);
    let r2 = row.clone();
    button.connect_clicked(move |_| {
        on_remove();
        r2.set_visible(false);
    });
    row
}

fn item_sub(item: &Item) -> String {
    match &item.action {
        Action::Run(c) | Action::Term(c) => c.clone(),
        Action::Open(u) | Action::Copy(u) | Action::Fill(u) => u.clone(),
        _ => item.subtitle.clone(),
    }
}

fn favorites_page(changed: &Rc<dyn Fn()>) -> gtk::Box {
    let page = page_box("Favorites", "Pinned to the start screen. Add more with Ctrl+F on any result.");
    let favs = Favorites::load();
    if favs.items.is_empty() {
        page.append(&label("nothing pinned yet", "empty"));
    }
    for item in favs.items {
        let changed = changed.clone();
        let it = item.clone();
        page.append(&removable(&item.title, &item_sub(&item), move || {
            Favorites::load().toggle(&it);
            changed();
        }));
    }
    page
}

fn shortcuts_page(changed: &Rc<dyn Fn()>, status: &gtk::Label) -> gtk::Box {
    let page = page_box(
        "Shortcuts",
        "Global keybinds, written to ~/.config/hypr/zunder.lua. Add more with Ctrl+B on any result.",
    );
    let binds = Binds::load();
    if binds.list.is_empty() {
        page.append(&label("no shortcuts yet", "empty"));
    }
    for bind in binds.list {
        let (changed, status) = (changed.clone(), status.clone());
        let id = bind.item.id.clone();
        page.append(&removable(&format!("{}   {}", bind.combo, bind.item.title), &item_sub(&bind.item), move || {
            match Binds::load().remove(&id) {
                Ok(_) => status.set_text("removed, hyprland reloaded"),
                Err(e) => status.set_text(&format!("hyprland: {e}")),
            }
            changed();
        }));
    }
    page
}

fn open_button(text: &str, target: std::path::PathBuf) -> gtk::Button {
    let button = gtk::Button::with_label(text);
    button.add_css_class("flat-button");
    button.connect_clicked(move |_| crate::item::ensure_and_open(&target, false));
    button
}

/// A text entry for one `@zunder.setting`, saved on Enter and when focus leaves.
fn plugin_setting_control(plugin: &Plugin, s: &plugins::PluginSetting, changed: &Rc<dyn Fn()>, status: &gtk::Label) -> gtk::Widget {
    let stem = plugin.stem();
    let value = config::get().plugin_values(&stem).get(&s.key).cloned().unwrap_or_default();
    let (key, changed, status) = (s.key.clone(), changed.clone(), status.clone());
    let save = move |text: &str| {
        if config::get().plugin_values(&stem).get(&key).map_or("", String::as_str) == text.trim() {
            return;
        }
        config::update(|c| c.set_plugin(&stem, &key, text));
        status.set_text("saved");
        changed();
    };
    let widget: gtk::Widget = if s.secret() {
        let entry = gtk::PasswordEntry::new();
        entry.set_show_peek_icon(true);
        entry.set_text(&value);
        entry.set_width_chars(28);
        let save2 = save.clone();
        entry.connect_activate(move |e| save2(&e.text()));
        let focus = gtk::EventControllerFocus::new();
        let e2 = entry.clone();
        focus.connect_leave(move |_| save(&e2.text()));
        entry.add_controller(focus);
        entry.upcast()
    } else {
        let entry = gtk::Entry::new();
        entry.set_text(&value);
        entry.set_width_chars(28);
        entry.set_placeholder_text(Some("not set"));
        let save2 = save.clone();
        entry.connect_activate(move |e| save2(&e.text()));
        let focus = gtk::EventControllerFocus::new();
        let e2 = entry.clone();
        focus.connect_leave(move |_| save(&e2.text()));
        entry.add_controller(focus);
        entry.upcast()
    };
    widget
}

fn plugins_page(info: &Rc<Info>, changed: &Rc<dyn Fn()>, status: &gtk::Label) -> gtk::Box {
    let page = page_box(
        "Plugins",
        "Executables in the plugins folder. Their header comments set name, prefix, glyph, colour, mode and settings.",
    );
    let open = gtk::Button::with_label("open folder");
    open.add_css_class("flat-button");
    let dir = plugins::dir();
    open.connect_clicked(move |_| crate::item::ensure_and_open(&dir, true));
    page.append(&row("Plugins folder", &plugins::dir().display().to_string().replace(&home(), "~"), &open));
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("search plugins"));
    search.add_css_class("plugin-search");
    // typing anywhere on the page searches
    search.set_key_capture_widget(Some(&page));
    page.append(&search);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.append(&body);
    let view = Rc::new(PluginsView {
        body,
        plugins: RefCell::new(info.plugins.clone()),
        catalog: RefCell::new(None),
        busy: Cell::new(false),
        query: RefCell::new(String::new()),
        other_open: Cell::new(false),
        open_settings: RefCell::new(BTreeSet::new()),
        reload: info.reload_plugins.clone(),
        changed: changed.clone(),
        status: status.clone(),
    });
    let v = view.clone();
    search.connect_search_changed(move |e| {
        *v.query.borrow_mut() = e.text().trim().to_lowercase();
        v.render();
    });
    view.render();
    view.run(None);
    page
}

enum Op {
    Install,
    Update,
    Remove,
}

/// One plugin card: tile, title, chip, badges and the button(s) on the right.
struct Card {
    /// File name in the plugins folder, also the registry name and the colour key.
    key: String,
    glyph: String,
    title: String,
    description: String,
    /// `wifi` for search plugins, None for command ones.
    prefix: Option<String>,
    color: String,
    badges: Vec<(String, &'static str)>,
    control: gtk::Box,
}

impl Card {
    fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || [&self.key, &self.title, &self.description].iter().any(|s| s.to_lowercase().contains(query))
            || self.prefix.as_ref().is_some_and(|p| p.to_lowercase().starts_with(query))
    }
}

/// `#89b4fa` → `89b4fa`, a CSS class suffix. Only ever called with a `theme::parse_hex` result.
fn tint_class(color: &str) -> String {
    format!("tint-{}", color.trim_start_matches('#'))
}

/// One rule set per colour in use: the tile, the chip and the card's border on hover and focus.
fn load_tints(colors: &BTreeSet<String>) {
    let css: String = colors
        .iter()
        .map(|c| {
            let class = tint_class(c);
            format!(
                ".plugin-tile.{class} {{ background: {c}; }}\n\
                 .chip.{class} {{ color: {c}; border-color: alpha({c}, 0.45); background: alpha({c}, 0.08); }}\n\
                 .plugin-card.{class}:hover, .plugin-card.{class}:focus-within {{ border-color: alpha({c}, 0.7); }}\n"
            )
        })
        .collect();
    TINTS.with(|p| p.load_from_string(&css));
}

/// The two plugin lists, redrawn after the registry answers, a plugin was added or removed, or
/// the search changed.
struct PluginsView {
    body: gtk::Box,
    plugins: RefCell<Vec<Plugin>>,
    /// None while the registry is being read.
    catalog: RefCell<Option<Result<Catalog, String>>>,
    busy: Cell<bool>,
    /// Lowercased search text.
    query: RefCell<String>,
    /// The "Not for <os>" expander, kept across redraws.
    other_open: Cell<bool>,
    /// Plugins whose settings are unfolded.
    open_settings: RefCell<BTreeSet<String>>,
    reload: Rc<dyn Fn() -> Vec<Plugin>>,
    changed: Rc<dyn Fn()>,
    status: gtk::Label,
}

impl PluginsView {
    /// Run `op` (or nothing) and re-read the registry on a worker thread, then redraw.
    fn run(self: &Rc<Self>, op: Option<(Op, String)>) {
        if self.busy.replace(true) {
            return;
        }
        if let Some((op, name)) = &op {
            let verb = match op {
                Op::Install => "installing",
                Op::Update => "updating",
                Op::Remove => "removing",
            };
            self.status.set_text(&format!("{verb} {name}…"));
            self.render();
        }
        let (tx, rx) = async_channel::bounded::<(Option<Result<Report, String>>, Result<Catalog, String>)>(1);
        std::thread::spawn(move || {
            let reg = Registry::from_env();
            let report = op.map(|(op, name)| {
                let names = [name];
                match op {
                    Op::Install => reg.install(&names),
                    Op::Update => reg.update(&names),
                    Op::Remove => reg.remove(&names),
                }
            });
            let _ = tx.send_blocking((report, reg.catalog()));
        });
        let view = self.clone();
        glib::spawn_future_local(async move {
            let Ok((report, catalog)) = rx.recv().await else { return };
            view.busy.set(false);
            if let Some(report) = report {
                match report {
                    Ok(r) => view.status.set_text(&r.summary()),
                    Err(e) => view.status.set_text(&e),
                }
                *view.plugins.borrow_mut() = (view.reload)();
                (view.changed)();
            }
            *view.catalog.borrow_mut() = Some(catalog);
            view.render();
        });
    }

    fn button(self: &Rc<Self>, text: &str, op: fn() -> Op, name: &str) -> gtk::Button {
        let button = gtk::Button::with_label(text);
        button.add_css_class("flat-button");
        button.set_sensitive(!self.busy.get());
        let (view, name) = (self.clone(), name.to_owned());
        button.connect_clicked(move |_| view.run(Some((op(), name.clone()))));
        button
    }

    fn card(self: &Rc<Self>, c: &Card, settings: Option<&Plugin>) -> gtk::Box {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("plugin-card");
        card.add_css_class(&tint_class(&c.color));
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 14);

        let tile = label(&c.glyph, "plugin-glyph");
        tile.set_xalign(0.5);
        let tile_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        tile_box.add_css_class("plugin-tile");
        tile_box.add_css_class(&tint_class(&c.color));
        tile_box.set_valign(gtk::Align::Center);
        tile.set_vexpand(true);
        tile_box.append(&tile);
        top.append(&tile_box);

        let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        line.append(&label(&c.title, "plugin-title"));
        let chip = label(c.prefix.as_deref().unwrap_or("command"), "chip");
        chip.add_css_class(&tint_class(&c.color));
        chip.set_valign(gtk::Align::Center);
        line.append(&chip);
        let toggle = settings.filter(|p| !p.settings.is_empty()).map(|p| {
            let n = p.settings.len();
            let t = gtk::ToggleButton::with_label(&format!("\u{f013} {n} setting{}", if n == 1 { "" } else { "s" }));
            t.add_css_class("settings-toggle");
            t.set_valign(gtk::Align::Center);
            t.set_active(self.open_settings.borrow().contains(&c.key));
            line.append(&t);
            t
        });
        for (badge, class) in &c.badges {
            let b = label(badge, "badge");
            b.add_css_class(class);
            b.set_valign(gtk::Align::Center);
            line.append(&b);
        }
        text.append(&line);
        if !c.description.is_empty() {
            let d = label(&c.description, "plugin-desc");
            d.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text.append(&d);
        }
        top.append(&text);
        c.control.set_valign(gtk::Align::Center);
        top.append(&c.control);
        card.append(&top);

        if let (Some(p), Some(toggle)) = (settings, toggle) {
            let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
            list.add_css_class("plugin-settings");
            for s in &p.settings {
                let help = if s.help.is_empty() { format!("${}", plugins::env_name(&s.key)) } else { format!("{}  ·  ${}", s.help, plugins::env_name(&s.key)) };
                list.append(&row(&s.key, &help, &plugin_setting_control(p, s, &self.changed, &self.status)));
            }
            list.set_visible(toggle.is_active());
            let (view, key, shown) = (self.clone(), c.key.clone(), list.clone());
            toggle.connect_toggled(move |t| {
                shown.set_visible(t.is_active());
                let mut open = view.open_settings.borrow_mut();
                if t.is_active() { open.insert(key.clone()) } else { open.remove(&key) };
            });
            card.append(&list);
        }
        card
    }

    fn section(&self, title: &str, count: usize) -> gtk::Box {
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        head.add_css_class("plugin-section");
        head.append(&label(title, "section-title"));
        head.append(&label(&count.to_string(), "section-count"));
        head
    }

    fn render(self: &Rc<Self>) {
        while let Some(child) = self.body.first_child() {
            self.body.remove(&child);
        }
        let palette = crate::theme::current_palette();
        let query = self.query.borrow().clone();
        let catalog = self.catalog.borrow();
        let ok = catalog.as_ref().and_then(|c| c.as_ref().ok());
        let mut colors = BTreeSet::new();

        // installed: whatever is in the folder, described by the registry when it's from there
        let plugins = self.plugins.borrow();
        let installed: Vec<(Card, &Plugin)> = plugins
            .iter()
            .map(|p| {
                // `wifi.ps1` on Windows is the registry's `wifi`
                let registry_name = ok.and_then(|c| c.registry_name(&p.path)).map(str::to_owned);
                let from_registry = registry_name.is_some();
                let file = registry_name
                    .unwrap_or_else(|| p.path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default());
                let entry = ok.and_then(|c| c.entry(&file));
                let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                let mut badges = Vec::new();
                if from_registry {
                    if ok.is_some_and(|c| c.has_update(&file)) {
                        badges.push(("update".to_owned(), "badge-new"));
                        let update = self.button("update", || Op::Update, &file);
                        update.add_css_class("primary");
                        controls.append(&update);
                    }
                    controls.append(&self.button("remove", || Op::Remove, &file));
                } else {
                    badges.push(("local".to_owned(), "badge-plain"));
                }
                let description = match entry {
                    Some(e) if !e.description.is_empty() => e.description.clone(),
                    _ if from_registry => String::new(),
                    _ => p.path.display().to_string().replace(&home(), "~"),
                };
                let color = crate::theme::pick(&p.stem(), p.color.as_deref().or(entry.and_then(|e| e.color.as_deref())), &palette);
                let card = Card {
                    key: file,
                    glyph: p.glyph.clone(),
                    title: p.name.clone(),
                    description,
                    prefix: (!p.command).then(|| p.prefix.clone()),
                    color,
                    badges,
                    control: controls,
                };
                (card, p)
            })
            .filter(|(c, _)| c.matches(&query))
            .collect();

        self.body.append(&self.section("Installed", installed.len()));
        if installed.is_empty() {
            self.body.append(&label(if query.is_empty() { "no plugins installed" } else { "no installed plugin matches" }, "empty"));
        }
        for (c, p) in &installed {
            colors.insert(c.color.clone());
            self.body.append(&self.card(c, Some(p)));
        }

        let catalog = match catalog.as_ref() {
            None => {
                self.body.append(&self.section("Available", 0));
                self.body.append(&label("reading the plugin registry…", "empty"));
                load_tints(&colors);
                return;
            }
            Some(Err(e)) => {
                self.body.append(&self.section("Available", 0));
                let l = label(&format!("can't read the plugin registry: {e}"), "empty");
                l.set_wrap(true);
                self.body.append(&l);
                load_tints(&colors);
                return;
            }
            Some(Ok(c)) => c,
        };
        let loaded: Vec<String> = plugins.iter().filter_map(|p| p.path.file_name().map(|f| f.to_string_lossy().into_owned())).collect();
        let (mut here, mut elsewhere) = (Vec::new(), Vec::new());
        for e in catalog.plugins.iter().filter(|e| !catalog.installed.contains_key(&e.name) && !loaded.contains(&e.name)) {
            let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let mut badges = Vec::new();
            let missing = catalog.missing.get(&e.name);
            // only what's missing: a found binary is nothing to act on
            for b in e.here().requires.iter().filter(|b| e.runs_here() && missing.is_some_and(|m| m.contains(b))) {
                badges.push((format!("needs {b}"), "badge-warn"));
            }
            if !e.settings.is_empty() {
                let n = e.settings.len();
                badges.push((format!("{n} setting{}", if n == 1 { "" } else { "s" }), "badge-plain"));
            }
            if !e.runs_here() {
                badges.push((format!("{} only", e.platforms.join(", ")), "badge-plain"));
            } else if catalog.taken.contains(&e.name) {
                badges.push(("name taken in folder".to_owned(), "badge-warn"));
            } else {
                controls.append(&self.button("install", || Op::Install, &e.name));
            }
            let card = Card {
                key: e.name.clone(),
                glyph: if e.glyph.is_empty() { "\u{f12e}".to_owned() } else { e.glyph.clone() },
                title: if e.title.is_empty() { e.name.clone() } else { e.title.clone() },
                description: e.description.clone(),
                prefix: (!e.prefix.is_empty()).then(|| e.prefix.clone()),
                color: crate::theme::pick(&e.name, e.color.as_deref(), &palette),
                badges,
                control: controls,
            };
            if !card.matches(&query) {
                continue;
            }
            if e.runs_here() { here.push(card) } else { elsewhere.push(card) }
        }

        self.body.append(&self.section("Available", here.len()));
        if let Some(stale) = &catalog.stale {
            let l = label(&stale.describe(), "setting-help");
            l.set_wrap(true);
            self.body.append(&l);
        }
        if here.is_empty() {
            let text = if !query.is_empty() { "no available plugin matches" } else { "every plugin in the registry is installed" };
            self.body.append(&label(text, "empty"));
        }
        for c in &here {
            colors.insert(c.color.clone());
            self.body.append(&self.card(c, None));
        }

        if !elsewhere.is_empty() {
            let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
            list.add_css_class("plugin-other");
            for c in &elsewhere {
                colors.insert(c.color.clone());
                list.append(&self.card(c, None));
            }
            let other = gtk::Expander::new(None);
            other.set_label_widget(Some(&self.section(&format!("Not for {}", os_name()), elsewhere.len())));
            other.add_css_class("plugin-expander");
            other.set_child(Some(&list));
            // a search that hits only these shows them without a click
            other.set_expanded(self.other_open.get() || (!query.is_empty() && here.is_empty() && installed.is_empty()));
            let view = self.clone();
            other.connect_expanded_notify(move |x| view.other_open.set(x.is_expanded()));
            self.body.append(&other);
        }
        load_tints(&colors);
    }
}

/// `linux` → `Linux`, how the "Not for …" section names this OS.
fn os_name() -> &'static str {
    match std::env::consts::OS {
        "linux" => "Linux",
        "macos" => "macOS",
        "windows" => "Windows",
        other => other,
    }
}

fn account_page(changed: &Rc<dyn Fn()>, status: &gtk::Label) -> gtk::Box {
    let page = page_box(
        "Account",
        "Log in with GitHub to sync config, favorites, themes, plugins and shortcuts through a private gist.",
    );
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.append(&body);
    let view = Rc::new(AccountView {
        body: body.clone(),
        state: RefCell::new(Account::Checking),
        busy: Cell::new(false),
        changed: changed.clone(),
        status: status.clone(),
    });
    // a login still waiting for github.com stops with the window
    let v2 = view.clone();
    body.connect_destroy(move |_| {
        if let Account::Waiting { cancel, .. } = &*v2.state.borrow() {
            cancel.store(true, Ordering::Relaxed);
        }
    });
    view.render();
    view.check();
    page
}

enum Account {
    Checking,
    LoggedOut,
    /// The device code is shown; a worker polls github.com until it's confirmed.
    Waiting { code: String, uri: String, cancel: Arc<AtomicBool> },
    LoggedIn { user: Option<String>, source: auth::Source },
}

/// What a worker thread tells the page.
enum Msg {
    State(Account),
    /// An error or a summary for the status line.
    Line(String),
    Synced(Result<sync::Outcome, String>),
}

struct AccountView {
    body: gtk::Box,
    state: RefCell<Account>,
    busy: Cell<bool>,
    changed: Rc<dyn Fn()>,
    status: gtk::Label,
}

fn who() -> Account {
    match auth::account() {
        Some((user, source)) => Account::LoggedIn { user, source },
        None => Account::LoggedOut,
    }
}

impl AccountView {
    /// Run `job` on a worker thread (the keyring and GitHub can take seconds) and apply what it
    /// sends on the UI thread, then redraw.
    fn spawn(self: &Rc<Self>, job: impl FnOnce(&async_channel::Sender<Msg>) + Send + 'static) {
        if self.busy.replace(true) {
            return;
        }
        self.render();
        let (tx, rx) = async_channel::unbounded::<Msg>();
        std::thread::spawn(move || job(&tx));
        let view = self.clone();
        glib::spawn_future_local(async move {
            while let Ok(msg) = rx.recv().await {
                match msg {
                    Msg::State(state) => *view.state.borrow_mut() = state,
                    Msg::Line(line) => view.status.set_text(&line),
                    Msg::Synced(Ok(out)) => {
                        view.status.set_text(&out.lines.join(", "));
                        if out.pulled {
                            (view.changed)();
                        }
                    }
                    Msg::Synced(Err(e)) => view.status.set_text(&e),
                }
                view.render();
            }
            view.busy.set(false);
            view.render();
        });
    }

    fn check(self: &Rc<Self>) {
        self.spawn(|tx| {
            let _ = tx.send_blocking(Msg::State(who()));
        });
    }

    fn login(self: &Rc<Self>) {
        self.status.set_text("asking GitHub for a code…");
        self.spawn(|tx| {
            let code = match auth::request_code() {
                Ok(code) => code,
                Err(e) => {
                    let _ = tx.send_blocking(Msg::Line(e));
                    return;
                }
            };
            let cancel = Arc::new(AtomicBool::new(false));
            let waiting = Account::Waiting { code: code.user_code.clone(), uri: code.verification_uri.clone(), cancel: cancel.clone() };
            let _ = tx.send_blocking(Msg::State(waiting));
            let _ = tx.send_blocking(Msg::Line(String::new()));
            match auth::finish_login(&code, &cancel) {
                Ok(name) => {
                    let line = match &name {
                        Some(n) => format!("logged in as {n}"),
                        None => "logged in".into(),
                    };
                    let _ = tx.send_blocking(Msg::State(who()));
                    let _ = tx.send_blocking(Msg::Line(line));
                }
                Err(e) => {
                    let _ = tx.send_blocking(Msg::State(Account::LoggedOut));
                    let _ = tx.send_blocking(Msg::Line(if e == auth::CANCELLED { String::new() } else { e }));
                }
            }
        });
    }

    fn logout(self: &Rc<Self>) {
        self.spawn(|tx| {
            let line = match auth::sign_out() {
                Ok(_) if auth::env_set() => "logged out, but ZUNDER_GITHUB_TOKEN is still set and keeps being used".into(),
                Ok(_) => "logged out".into(),
                Err(e) => e,
            };
            let _ = tx.send_blocking(Msg::State(who()));
            let _ = tx.send_blocking(Msg::Line(line));
        });
    }

    fn sync_now(self: &Rc<Self>) {
        self.status.set_text("syncing…");
        self.spawn(|tx| {
            let _ = tx.send_blocking(Msg::Synced(sync::sync_now()));
        });
    }

    fn button(self: &Rc<Self>, text: &str, on: fn(&Rc<Self>)) -> gtk::Button {
        let button = gtk::Button::with_label(text);
        button.add_css_class("flat-button");
        button.set_sensitive(!self.busy.get());
        let view = self.clone();
        button.connect_clicked(move |_| on(&view));
        button
    }

    fn render(self: &Rc<Self>) {
        while let Some(child) = self.body.first_child() {
            self.body.remove(&child);
        }
        match &*self.state.borrow() {
            Account::Checking => self.body.append(&label("checking the login…", "empty")),
            Account::LoggedOut => {
                let login = self.button("Log in with GitHub", Self::login);
                login.add_css_class("primary");
                self.body.append(&row("GitHub", "not logged in", &login));
            }
            Account::Waiting { code, uri, cancel } => {
                let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                let open = gtk::Button::with_label("copy and open github.com");
                open.add_css_class("flat-button");
                open.add_css_class("primary");
                let (code2, uri2) = (code.clone(), uri.clone());
                open.connect_clicked(move |b| {
                    b.clipboard().set_text(&code2);
                    crate::platform::open(&uri2);
                });
                controls.append(&open);
                let stop = gtk::Button::with_label("cancel");
                stop.add_css_class("flat-button");
                let cancel = cancel.clone();
                stop.connect_clicked(move |b| {
                    cancel.store(true, Ordering::Relaxed);
                    b.set_sensitive(false);
                });
                controls.append(&stop);
                let help = format!("enter it on {uri}, this page carries on by itself");
                self.body.append(&row("Waiting for github.com", &help, &controls));
                let code = label(code, "device-code");
                code.set_selectable(true);
                self.body.append(&code);
            }
            Account::LoggedIn { user, source } => {
                let who = match user {
                    Some(u) => format!("logged in as {u}, token from the {}", source.label()),
                    None => format!("logged in, token from the {}", source.label()),
                };
                let who = who.replace("the ZUNDER_GITHUB_TOKEN", "ZUNDER_GITHUB_TOKEN");
                self.body.append(&row("GitHub", &who, &self.button("Log out", Self::logout)));
                let last = sync::last_sync().map_or("never".to_owned(), when);
                self.body.append(&row("Last sync", &last, &self.button("Sync now", Self::sync_now)));
                if let Some(s) = SETTINGS.iter().find(|s| s.key == "sync") {
                    self.body.append(&row(s.title, s.help, &setting_control(s.key, &self.changed, &self.status)));
                }
            }
        }
    }
}

/// A sync time for people: "today 14:05", "yesterday 09:12", else the date.
fn when(t: std::time::SystemTime) -> String {
    let t: chrono::DateTime<chrono::Local> = t.into();
    let today = chrono::Local::now().date_naive();
    match (today - t.date_naive()).num_days() {
        0 => t.format("today %H:%M").to_string(),
        1 => t.format("yesterday %H:%M").to_string(),
        _ => t.format("%-d %b %Y %H:%M").to_string(),
    }
}

fn about_page() -> gtk::Box {
    let page = page_box("About", "");
    page.append(&row("zunder", &format!("version {}", env!("CARGO_PKG_VERSION")), &gtk::Box::new(gtk::Orientation::Horizontal, 0)));
    let dir = config::path().parent().map(|d| d.to_path_buf()).unwrap_or_default();
    page.append(&row("config.toml", "every setting in this window, as a file", &open_button("open", config::path())));
    page.append(&row("style.css", "your CSS on top of the theme", &open_button("open", dir.join("style.css"))));
    let link = gtk::LinkButton::with_label("https://github.com/elyesghazel/zunder", "github");
    link.add_css_class("flat-button");
    page.append(&row("Source", "bugs, ideas and docs", &link));
    page
}
