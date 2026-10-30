//! The changelog window: every release in the sidebar, what changed in it on the right, and an
//! Update button while a newer one is out. Same look as the settings window. Shows the releases
//! cached from last time at once, then asks GitHub.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::{gdk, glib, prelude::*};
use gtk4 as gtk;

use crate::item::Action;
use crate::providers::update::{self, Release};
use crate::settings_window::{label, page_box};

thread_local! {
    static WINDOW: RefCell<Option<gtk::Window>> = const { RefCell::new(None) };
}

/// Open (or raise) the changelog window. `fill` puts a query in the launcher, for the Windows
/// update, which runs as `update install` there.
pub fn open(app: &gtk::Application, fill: Rc<dyn Fn(&str)>) {
    crate::settings_window::load_css();
    if let Some(window) = WINDOW.with(|w| w.borrow().clone()) {
        window.present();
        return;
    }

    let window = gtk::Window::builder()
        .application(app)
        .title("zunder changelog")
        .default_width(900)
        .default_height(620)
        .build();
    window.add_css_class("zunder-settings");

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_hexpand(true);
    let sidebar = gtk::ListBox::new();
    sidebar.add_css_class("sidebar");
    sidebar.set_selection_mode(gtk::SelectionMode::Single);
    let side_scroll = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).child(&sidebar).build();

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    header.add_css_class("brand");
    header.append(&label("❯", "caret"));
    header.append(&label("changelog", "brand-name"));
    let side = gtk::Box::new(gtk::Orientation::Vertical, 0);
    side.add_css_class("side");
    side.append(&header);
    side.append(&side_scroll);
    let status = label("", "side-path");
    side.append(&status);

    let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    body.add_css_class("settings-body");
    body.append(&side);
    body.append(&stack);
    window.set_child(Some(&body));

    let view = Rc::new(View { sidebar, stack, markers: RefCell::default(), tags: RefCell::default(), window: window.clone(), fill });
    let v = view.clone();
    view.sidebar.connect_row_selected(move |_, row| {
        let Some(row) = row else { return };
        let i = row.index() as usize;
        for (j, m) in v.markers.borrow().iter().enumerate() {
            m.set_text(if i == j { "▌" } else { "▏" });
        }
        if let Some(tag) = v.tags.borrow().get(i) {
            v.stack.set_visible_child_name(tag);
        }
    });

    let cached = update::cached_releases();
    if cached.is_empty() {
        let page = page_box("Getting the releases…", "asking GitHub");
        view.stack.add_named(&page, Some("loading"));
    } else {
        view.fill(&cached);
    }
    status.set_text("checking GitHub…");
    let (tx, rx) = async_channel::bounded::<Option<Vec<Release>>>(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(update::fetch_releases());
    });
    let v = view.clone();
    glib::spawn_future_local(async move {
        match rx.recv().await.ok().flatten() {
            Some(releases) => {
                status.set_text(&format!("you have {}", env!("CARGO_PKG_VERSION")));
                v.fill(&releases);
            }
            None => {
                status.set_text("couldn't reach GitHub");
                if v.tags.borrow().is_empty() {
                    let page = page_box("Couldn't reach GitHub", "no connection, or GitHub is rate-limiting; try again in a minute");
                    v.stack.add_named(&page, Some("offline"));
                    v.stack.set_visible_child_name("offline");
                }
            }
        }
    });

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

struct View {
    sidebar: gtk::ListBox,
    stack: gtk::Stack,
    markers: RefCell<Vec<gtk::Label>>,
    /// The tag of each sidebar row, which is also its page's name in the stack.
    tags: RefCell<Vec<String>>,
    window: gtk::Window,
    fill: Rc<dyn Fn(&str)>,
}

impl View {
    /// (Re)build the sidebar and pages, keeping the release that was selected.
    fn fill(&self, releases: &[Release]) {
        let current = env!("CARGO_PKG_VERSION");
        let selected = self.sidebar.selected_row().and_then(|r| self.tags.borrow().get(r.index() as usize).cloned());
        while let Some(child) = self.stack.first_child() {
            self.stack.remove(&child);
        }
        self.sidebar.remove_all();
        self.markers.borrow_mut().clear();
        self.tags.borrow_mut().clear();

        for release in releases {
            let version = release.tag_name.trim_start_matches('v');
            let state = if version == current {
                Some(("installed", "badge-installed"))
            } else if update::newer(&release.tag_name, current) {
                Some(("new", "badge-new"))
            } else {
                None
            };
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            row.add_css_class("side-item");
            let marker = label("▏", "marker");
            row.append(&marker);
            let name = label(version, "side-title");
            name.set_hexpand(true);
            row.append(&name);
            if let Some((text, class)) = state {
                row.append(&label(text, class));
            }
            self.sidebar.append(&row);
            self.markers.borrow_mut().push(marker);
            self.tags.borrow_mut().push(release.tag_name.clone());

            let page = self.page(release, state.map(|s| s.0));
            let scroll = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).child(&page).build();
            self.stack.add_named(&scroll, Some(&release.tag_name));
        }

        // the release you were looking at, else the one you have, else the newest
        let tags = self.tags.borrow();
        let start = selected
            .and_then(|t| tags.iter().position(|x| *x == t))
            .or_else(|| tags.iter().position(|t| t.trim_start_matches('v') == current))
            .unwrap_or(0);
        drop(tags);
        if let Some(row) = self.sidebar.row_at_index(start as i32) {
            self.sidebar.select_row(Some(&row));
        }
    }

    fn page(&self, release: &Release, state: Option<&str>) -> gtk::Box {
        let version = release.tag_name.trim_start_matches('v');
        let date = chrono::DateTime::parse_from_rfc3339(&release.published_at).map(|d| d.format("%-d %b %Y").to_string()).unwrap_or_default();
        let intro = match state {
            Some("installed") => format!("{date} · the version you have"),
            Some(_) => format!("{date} · newer than yours ({})", env!("CARGO_PKG_VERSION")),
            None => date,
        };
        let page = page_box(&format!("zunder {version}"), &intro);

        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        buttons.add_css_class("changelog-buttons");
        if state == Some("new") {
            let install = gtk::Button::with_label(&format!("Update to {version}"));
            install.add_css_class("flat-button");
            install.add_css_class("primary");
            let (w, fill) = (self.window.clone(), self.fill.clone());
            install.connect_clicked(move |_| {
                match crate::platform::update_action() {
                    Action::Fill(query) => fill(&query),
                    action => action.execute(),
                }
                w.close();
            });
            buttons.append(&install);
        }
        let github = gtk::Button::with_label("On GitHub");
        github.add_css_class("flat-button");
        let url = update::release_page(&release.tag_name);
        github.connect_clicked(move |_| crate::platform::open(&url));
        buttons.append(&github);
        page.append(&buttons);

        let sections = update::sections(&release.body);
        if sections.is_empty() {
            page.append(&label("No notes for this release.", "empty"));
        }
        for section in sections {
            page.append(&label(&section.heading, "changelog-heading"));
            for (text, scope) in section.entries {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                row.add_css_class("changelog-entry");
                let bullet = label("·", "changelog-bullet");
                bullet.set_valign(gtk::Align::Start);
                row.append(&bullet);
                let text = label(&text, "changelog-text");
                text.set_wrap(true);
                text.set_hexpand(true);
                row.append(&text);
                if let Some(scope) = scope {
                    let scope = label(&scope, "changelog-scope");
                    scope.set_valign(gtk::Align::Start);
                    row.append(&scope);
                }
                page.append(&row);
            }
        }
        page
    }
}
