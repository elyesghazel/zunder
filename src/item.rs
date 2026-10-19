use std::process::{Command, Stdio};
use std::thread;

use serde::{Deserialize, Serialize};

/// One selectable result row.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Item {
    /// Stable key for frecency, e.g. `app:firefox.desktop`.
    pub id: String,
    pub title: String,
    /// Dim text after the title.
    pub subtitle: String,
    /// Right-aligned text.
    pub hint: String,
    /// A Nerd Font glyph shown before the title.
    pub glyph: String,
    /// Icon theme name or absolute path; replaces the glyph when it resolves.
    pub icon: String,
    /// Hyprland `stableId` of a window to show a live preview of.
    #[serde(skip)]
    pub preview: Option<String>,
    /// Splits a provider's results into several sections (the settings page uses it).
    #[serde(skip)]
    pub group: String,
    pub action: Action,
    #[serde(skip)]
    pub score: i64,
}

/// What Enter does. Plugins emit the same shape as JSON, e.g. `{"open": "https://…"}`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Run a shell command in the background.
    Run(String),
    /// Run a shell command in a terminal that stays open.
    Term(String),
    /// Focus a Hyprland window by address.
    Focus(String),
    /// Put text on the clipboard.
    Copy(String),
    /// Open a URL or file with xdg-open.
    Open(String),
    /// Replace the query and keep the launcher open.
    Fill(String),
    /// Change a setting (`None` toggles); `provider:<title>` turns a section on or off.
    Set { key: String, value: Option<String> },
    #[default]
    None,
}

impl Action {
    pub fn keeps_open(&self) -> bool {
        matches!(self, Action::Fill(_) | Action::Set { .. } | Action::None)
    }

    pub fn execute(&self) {
        match self {
            Action::Run(cmd) => spawn(&["sh", "-c", cmd]),
            Action::Term(cmd) => {
                let held = format!("{cmd}; printf '\\n\\033[2m[exit %s] enter to close\\033[0m' $?; read _");
                spawn(&[&terminal(), "-e", "sh", "-c", &held]);
            }
            Action::Focus(addr) => {
                let lua = format!("hl.dsp.focus({{ window = \"address:{addr}\" }})");
                spawn(&["hyprctl", "dispatch", &lua]);
            }
            Action::Copy(text) => spawn(&["wl-copy", "--", text]),
            Action::Open(target) => spawn(&["xdg-open", target]),
            Action::Fill(_) | Action::Set { .. } | Action::None => {}
        }
    }
}

pub fn terminal() -> String {
    crate::config::terminal()
}

/// Start a fully detached process (own session, no stdio) and reap it.
pub fn spawn(argv: &[&str]) {
    let child = Command::new("setsid")
        .arg("-f")
        .args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match child {
        Ok(mut c) => {
            thread::spawn(move || c.wait());
        }
        Err(e) => eprintln!("zunder: failed to run {argv:?}: {e}"),
    }
}

pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
