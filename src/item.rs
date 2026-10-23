use serde::{Deserialize, Serialize};

use crate::platform;

/// One selectable result row.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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
    /// An image file to show in the preview pane (image files, clipboard images).
    #[serde(skip)]
    pub image: Option<String>,
    /// Shell command that writes `image` when it doesn't exist yet, run on selection.
    #[serde(skip)]
    pub image_cmd: Option<String>,
    /// Full text for the preview pane, e.g. a clipboard entry longer than its one-line title.
    #[serde(skip)]
    pub text: Option<Text>,
    /// A cliphist list line: Ctrl+F pins the decoded content instead of this entry, which
    /// cliphist may drop later.
    #[serde(skip)]
    pub pin: Option<String>,
    /// Splits a provider's results into several sections (the settings page uses it).
    #[serde(skip)]
    pub group: String,
    pub action: Action,
    #[serde(skip)]
    pub score: i64,
}

/// Preview-pane text: given as is, or the output of a command run when the item is selected.
#[derive(Clone, Debug, PartialEq)]
pub enum Text {
    Literal(String),
    Command(String),
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
            Action::Run(cmd) => platform::run_shell(cmd),
            Action::Term(cmd) => platform::run_in_terminal(cmd),
            Action::Focus(id) => platform::focus_window(id),
            Action::Copy(text) => platform::copy(text),
            Action::Open(target) => platform::open(target),
            Action::Fill(_) | Action::Set { .. } | Action::None => {}
        }
    }
}

/// Create `path` (an empty file, or a folder) if it's missing, then open it.
pub fn ensure_and_open(path: &std::path::Path, folder: bool) {
    if folder {
        let _ = std::fs::create_dir_all(path);
    } else if !path.exists() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, "");
    }
    platform::open(&path.to_string_lossy());
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
