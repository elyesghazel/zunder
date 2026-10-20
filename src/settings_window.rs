//! The settings window: a sidebar of pages on the left, the settings of the page on the right.
//! A normal toplevel window (not the overlay), styled like the launcher.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::{gdk, glib, prelude::*};
use gtk4 as gtk;

use crate::config::{self, Kind, SETTINGS};
use crate::item::{Action, Item};
use crate::providers::plugins::{self, Plugin};
use crate::store::{Binds, Favorites};

/// What the window can't read from disk by itself.
pub struct Info {
    pub providers: Vec<String>,
    pub plugins: Vec<Plugin>,
}

thread_local! {
    static WINDOW: RefCell<Option<gtk::Window>> = const { RefCell::new(None) };
    static CSS: gtk::CssProvider = gtk::CssProvider::new();
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
    ("About", "\u{f05a}"),
];

/// Open (or rebuild and raise) the settings window. `changed` runs after anything is saved.
pub fn open(app: &gtk::Application, info: Info, changed: Rc<dyn Fn()>, page: Option<&str>) {
    CSS.with(|css| {
        if css.to_str().is_empty() {
            css.load_from_string(include_str!("settings.css"));
            if let Some(display) = gdk::Display::default() {
                gtk::style_context_add_provider_for_display(&display, css, gtk::STYLE_PROVIDER_PRIORITY_USER);
            }
        }
    });
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

fn home() -> String {
    dirs::home_dir().unwrap_or_default().display().to_string()
}

fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class(class);
    l.set_xalign(0.0);
    l
}

fn page_box(title: &str, intro: &str) -> gtk::Box {
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
        "Plugins" => plugins_page(info),
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

fn plugins_page(info: &Rc<Info>) -> gtk::Box {
    let page = page_box("Plugins", "Executables in the plugins folder. Their header comments set name, prefix, glyph and mode.");
    let open = gtk::Button::with_label("open folder");
    open.add_css_class("flat-button");
    let dir = plugins::dir();
    open.connect_clicked(move |_| crate::item::ensure_and_open(&dir, true));
    page.append(&row("Plugins folder", &plugins::dir().display().to_string().replace(&home(), "~"), &open));
    if info.plugins.is_empty() {
        page.append(&label("no plugins installed", "empty"));
    }
    for p in &info.plugins {
        let how = if p.command { "command, runs once".to_owned() } else { format!("type `{} <query>`", p.prefix) };
        let path = label(&p.path.display().to_string().replace(&home(), "~"), "setting-help");
        page.append(&row(&format!("{}  {}", p.glyph, p.name), &how, &path));
    }
    page
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
