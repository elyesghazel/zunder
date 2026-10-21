//! First run: until there's a config.toml, the start screen shows where to begin.
//! `zunder setup` (or saving any setting) writes the config, and this goes away.

use crate::config;
use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item, Text};

const SETUP: &str = "Opens a terminal and asks.
Enter takes the default.

Other launchers
  vicinae rofi wofi fuzzel walker…
  · their daemon line: commented out
  · the key that opened them: zunder
  · vicinae favorites come along
  · changed files keep a .bak

Keys
  ALT+TAB     window switcher
  ALT+SPACE   launcher
  SUPER+V     clipboard
  warns when a key is taken

Also
  · cliphist autostart if missing
  · a theme
  · the example plugins

Writes hypr/zunder-setup.lua, adds
one line to hyprland.lua. Safe to
run again: nothing doubles.
Look first: zunder setup --dry-run";

const KEYS: &str = "In zunder

  ↑ ↓  Tab        move
  Enter           run
  Ctrl+F          favorite
  Ctrl+B          global shortcut
  Ctrl+,          settings
  Esc             close

Type

  firefox             apps, windows, files
  2^10/3              calculator
  5 km in mi          units
  100 usd in chf      currency
  time in tokyo       world clock
  :fire               emoji
  f readme            only files
  clip                clipboard history
  history             everything you ran
  > htop              shell command
  theme               color schemes
  kill zen            a plugin";

pub struct Welcome;

impl Provider for Welcome {
    fn title(&self) -> &str {
        "Welcome"
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        if !cx.query.is_empty() || cx.switch || config::path().exists() {
            return Vec::new();
        }
        let item = |title: &str, subtitle: &str, glyph: &str, action: Action, score: i64| Item {
            title: title.into(),
            subtitle: subtitle.into(),
            glyph: glyph.into(),
            action,
            score,
            ..Default::default()
        };
        // what each step is about, in the preview pane on the right
        let explained = |item: Item, text: &str| Item { text: Some(Text::Literal(text.to_owned())), ..item };
        let themes = crate::theme::names().join("\n");
        let pages = "General      terminal, search, money\nSwitcher     Alt release, previews\nStart screen windows, apps\nAppearance   font, theme, size\nProviders    sections on / off\nFavorites    what you pinned\nShortcuts    global keys\nPlugins      installed scripts\nAbout        version, files";
        let mut items = vec![
            explained(item("Run setup", "keybinds, autostart and theme, a few questions", "\u{f0ad}", Action::Term("zunder setup".into()), 4), SETUP),
            explained(item("Pick a theme", "Enter applies live, keep trying", "\u{f1fc}", Action::Fill("theme ".into()), 3), &format!("Built in\n\n{themes}\n\nOr your own: ~/.config/zunder/themes/<name>.css")),
            explained(item("Settings", "Ctrl+, from anywhere in zunder", "\u{f013}", Action::Run("zunder settings".into()), 2), &format!("The settings window\n\n{pages}")),
            explained(
                item("Keys and tricks", "Enter opens the full guide", "\u{f11c}", Action::Open("https://github.com/elyesghazel/zunder/blob/main/docs/usage.md".into()), 1),
                KEYS,
            ),
        ];
        // `zunder setup` is Hyprland setup; the Windows installer and the macOS app do it themselves
        if !cfg!(target_os = "linux") {
            items.remove(0);
        }
        items
    }
}
