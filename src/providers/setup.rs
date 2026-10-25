//! `setup`: `zunder setup` as a page in the launcher. Enter toggles a choice, Apply writes it.
//! The terminal version (`src/setup.rs`) does the same work and stays for scripts.

use std::sync::Mutex;

use crate::config;
use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item, Text};
use crate::setup::{Choice, LAUNCHER_KEYS, Out, Plan};

const PREFIX: &str = "setup";
const GLYPH: &str = "\u{f0ad}";

pub struct Setup;

/// The page's state between keystrokes: what was found, what's picked, what Apply did.
struct Page {
    plan: Option<Plan>,
    choice: Option<Choice>,
    /// Lines from the last Apply; the page shows them until you go back.
    report: Option<Vec<String>>,
}

static PAGE: Mutex<Option<Page>> = Mutex::new(None);

/// Look at the config again the next time the page shows (called when the launcher opens).
pub fn reset() {
    *PAGE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn with_page<T>(f: impl FnOnce(&mut Page) -> T) -> T {
    let mut guard = PAGE.lock().unwrap_or_else(|e| e.into_inner());
    let page = guard.get_or_insert_with(|| {
        let plan = if cfg!(target_os = "linux") { Plan::detect() } else { None };
        let choice = plan.as_ref().map(Plan::defaults);
        Page { plan, choice, report: None }
    });
    f(page)
}

/// Enter on a row of the page: toggle it, or apply. Returns the note for the status line.
pub fn set(key: &str) -> String {
    with_page(|page| {
        if key == "again" {
            page.report = None;
            return String::new();
        }
        let (Some(plan), Some(choice)) = (&page.plan, &mut page.choice) else { return String::new() };
        let flip = |b: &mut bool| {
            *b = !*b;
            String::new()
        };
        match key {
            "switcher" => flip(&mut choice.switcher),
            "overview" => flip(&mut choice.overview),
            "clipboard_key" => flip(&mut choice.clipboard_key),
            "autostart" => flip(&mut choice.autostart),
            "clipboard_watch" => flip(&mut choice.clipboard_watch),
            "switch_over" => flip(&mut choice.switch_over),
            "import_vicinae" => flip(&mut choice.import_vicinae),
            "examples" => flip(&mut choice.examples),
            "launcher" => {
                // ALT+SPACE → SUPER+SPACE → none → ALT+SPACE
                let at = choice.launcher.as_deref().and_then(|k| LAUNCHER_KEYS.iter().position(|x| *x == k));
                choice.launcher = match at {
                    Some(i) => LAUNCHER_KEYS.get(i + 1).map(|k| k.to_string()),
                    None => Some(LAUNCHER_KEYS[0].to_owned()),
                };
                String::new()
            }
            "apply" => {
                let mut lines = Vec::new();
                let mut log = |line: String| lines.push(line);
                let result = crate::setup::apply(plan, choice, &mut Out { dry: false, log: &mut log });
                let note = match &result {
                    Ok(()) => "set up ✓".to_owned(),
                    Err(e) => format!("setup failed: {e}"),
                };
                if let Err(e) = result {
                    lines.push(format!("stopped: {e}"));
                }
                page.report = Some(lines);
                note
            }
            _ => String::new(),
        }
    })
}

fn on_off(on: bool) -> String {
    if on { "● on".into() } else { "○ off".into() }
}

impl Provider for Setup {
    fn title(&self) -> &str {
        "Setup"
    }

    fn exclusive(&self, query: &str) -> bool {
        query == PREFIX || query.starts_with("setup ")
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        if !self.exclusive(cx.query) {
            if cx.query.is_empty() {
                return Vec::new();
            }
            return cx
                .score("Set up zunder")
                .map(|score| Item {
                    id: "cmd:setup".into(),
                    title: "Set up zunder".into(),
                    subtitle: "keys, autostart, switching over from another launcher".into(),
                    hint: "page".into(),
                    glyph: GLYPH.into(),
                    action: Action::Fill(PREFIX.into()),
                    score,
                    ..Default::default()
                })
                .into_iter()
                .collect();
        }
        let mut items = with_page(|page| rows(page));
        let n = items.len() as i64;
        for (i, item) in items.iter_mut().enumerate() {
            item.score = n - i as i64;
            if item.glyph.is_empty() {
                item.glyph = GLYPH.into();
            }
        }
        items
    }
}

fn row(group: &str, title: &str, subtitle: &str, hint: String, action: Action, explain: &str) -> Item {
    Item {
        title: title.into(),
        subtitle: subtitle.into(),
        hint,
        action,
        group: group.into(),
        text: Some(Text::Literal(explain.to_owned())),
        ..Default::default()
    }
}

fn set_key(key: &str) -> Action {
    Action::Set { key: format!("setup:{key}"), value: None }
}

fn rows(page: &Page) -> Vec<Item> {
    let theme = config::get().theme.clone();
    let theme_row = row("Look", "Theme", "Enter applies live, keep trying", theme, Action::Fill("theme ".into()), "Every built-in theme, and your own in\n~/.config/zunder/themes/<name>.css");

    if !cfg!(target_os = "linux") {
        let (key, start) = if cfg!(windows) {
            ("ALT+SPACE opens zunder", "The installer adds zunder to autostart and the Start menu.")
        } else {
            ("Open zunder from the menu bar icon", "Add zunder.app to System Settings → General → Login Items to start it with your Mac.")
        };
        return vec![
            row("Setup", key, "set by the installer", String::new(), Action::None, start),
            theme_row,
            row("Setup", "Settings", "everything else", "window".into(), Action::Run("zunder settings".into()), "Ctrl+, from anywhere in zunder."),
        ];
    }

    if let Some(report) = &page.report {
        let mut items: Vec<Item> = report
            .iter()
            .map(|line| Item {
                title: line.lines().next().unwrap_or_default().to_owned(),
                glyph: "\u{f00c}".into(),
                group: "Done".into(),
                text: Some(Text::Literal(line.clone())),
                ..Default::default()
            })
            .collect();
        items.push(row("Done", "Back to the setup page", "change something and apply again", String::new(), set_key("again"), "Setup only rewrites its own file, so applying again never doubles a bind."));
        return items;
    }

    let (Some(plan), Some(c)) = (&page.plan, &page.choice) else {
        return vec![row(
            "Setup",
            "No Hyprland config found",
            "setup knows Hyprland so far: ~/.config/hypr/hyprland.lua or .conf",
            String::new(),
            Action::Open("https://github.com/elyesghazel/zunder/blob/main/docs/setup.md".into()),
            "Enter opens the setup guide, with the binds to add by hand for other compositors.",
        )];
    };

    let taken = |combo: &str, sub: &str| {
        if plan.taken(combo) { format!("{sub} · already bound in Hyprland") } else { sub.to_owned() }
    };
    let mut items = Vec::new();

    // ── keys ──
    if !plan.by_hand.is_empty() {
        items.push(row("Keys", "zunder is already bound in your config", "turn on what you want on top", String::new(), Action::None, &plan.by_hand.iter().map(|l| l.trim()).collect::<Vec<_>>().join("\n")));
    }
    items.push(row("Keys", "Window switcher", &taken("ALT + TAB", "ALT+TAB, ALT+SHIFT+TAB goes back"), on_off(c.switcher), set_key("switcher"), "ALT+TAB opens a grid of every window with live thumbnails, on the previous window. Let go of Alt to switch."));
    items.push(row("Keys", "Every window by workspace", &taken("SUPER + TAB", "SUPER+TAB"), on_off(c.overview), set_key("overview"), "All windows, grouped by workspace, with thumbnails.\nStays open until you pick one.\n\n^W close  ^M minimize  ^1-9 move\n^↵ bring it to this workspace\n^K every other action"));
    let reused = plan.reused_key || (c.switch_over && plan.takes_over_key());
    if reused {
        items.push(row("Keys", "Launcher key", "your old launcher's key opens zunder", String::new(), Action::None, "Switching over points the key that opened your old launcher at zunder, so there's no second key."));
    } else {
        let key = c.launcher.clone();
        let sub = key.as_deref().map_or("Enter picks ALT+SPACE, SUPER+SPACE or none".to_owned(), |k| taken(k, "Enter picks another"));
        items.push(row("Keys", "Launcher key", &sub, key.unwrap_or_else(|| "none".into()), set_key("launcher"), "The key that opens the launcher."));
    }
    if plan.cliphist {
        items.push(row("Keys", "Clipboard history", &taken("SUPER + V", "SUPER+V"), on_off(c.clipboard_key), set_key("clipboard_key"), "SUPER+V opens zunder on your clipboard history. Ctrl+F pins an entry for good."));
    } else {
        let mut install = crate::providers::missing_tool("cliphist", "clipboard history", " on SUPER+V");
        install.group = "Keys".into();
        items.push(install);
    }

    // ── autostart ──
    let sub = if plan.by_hand.iter().any(|l| l.contains("zunder server")) { "already in your config" } else { "the server, so zunder opens instantly" };
    items.push(row("Start with Hyprland", "zunder", sub, on_off(c.autostart), set_key("autostart"), "Starts `zunder server` when Hyprland starts."));
    if plan.cliphist && !plan.cliphist_running {
        items.push(row("Start with Hyprland", "cliphist", "isn't running, so nothing gets recorded", on_off(c.clipboard_watch), set_key("clipboard_watch"), "wl-paste --type text --watch cliphist store\nwl-paste --type image --watch cliphist store"));
    }

    // ── other launchers ──
    if !plan.others.is_empty() {
        let mut names: Vec<&str> = plan.others.iter().map(|o| o.3).collect();
        names.dedup();
        let lines: Vec<String> = plan.others.iter().map(|(f, i, l, _)| format!("{}:{}\n  {}", f.display(), i + 1, l.trim())).collect();
        items.push(row(
            "Other launchers",
            &format!("Switch over from {}", names.join(", ")),
            &format!("{} line(s): daemon commented out, its key opens zunder", plan.others.len()),
            on_off(c.switch_over),
            set_key("switch_over"),
            &format!("Each changed file keeps a .bak copy.\n\n{}", lines.join("\n\n")),
        ));
    }
    if plan.vicinae {
        items.push(row("Other launchers", "Take over vicinae favorites", "and launch counts", on_off(c.import_vicinae), set_key("import_vicinae"), "Reads ~/.config/vicinae and adds its favorites and how often you opened each app."));
    }

    // ── extras ──
    if !plan.examples_present {
        items.push(row("Look", "Example plugins", "`kill <name>` and Copy Local IP", on_off(c.examples), set_key("examples"), "Two small scripts in ~/.config/zunder/plugins to read and copy from.\nA plugin is any executable that prints lines."));
    }
    items.push(theme_row);

    // ── apply, with the dry run in the preview pane ──
    let mut preview = Vec::new();
    let mut log = |line: String| preview.push(line);
    let _ = crate::setup::apply(plan, c, &mut Out { dry: true, log: &mut log });
    items.push(Item {
        glyph: "\u{f00c}".into(),
        ..row("Apply", "Apply", "writes hypr/zunder-setup.lua and reloads Hyprland", "↵".into(), set_key("apply"), &format!("What Apply writes:\n\n{}", preview.join("\n\n")))
    });
    items
}
