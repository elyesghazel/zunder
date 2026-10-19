//! ~/.config/zunder/config.toml, editable by hand or from the settings page (`settings`).

use std::fs;
use std::path::PathBuf;
use std::sync::{LazyLock, RwLock, RwLockReadGuard};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Terminal for `>` commands and Terminal=true apps. Empty = $TERMINAL, then kitty.
    pub terminal: String,
    /// Web search fallback, `{q}` is replaced by the query.
    pub search_url: String,
    /// Your currency for conversions, e.g. `CHF`. Empty = guessed from time zone and locale.
    pub currency: String,
    pub font: String,
    pub font_size: f64,
    /// Panel size in px; ALT+TAB adds the preview width on top.
    pub width: i32,
    pub height: i32,
    /// Backdrop darkness, 0 to 1.
    pub dim: f64,
    /// Color scheme: a built-in name or a file in ~/.config/zunder/themes/.
    pub theme: String,
    /// Accent color like `#b4befe`. Empty = theme default.
    pub accent: String,
    /// Colored app icons instead of glyphs.
    pub icons: bool,
    /// Window screenshots next to the ALT+TAB list.
    pub previews: bool,
    /// Releasing Alt switches to the selected window.
    pub alt_release: bool,
    /// Open windows on the start screen.
    pub start_windows: bool,
    /// How many apps the start screen shows.
    pub start_apps: usize,
    /// Section titles of providers that are turned off, e.g. ["Web"].
    pub disabled: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            terminal: String::new(),
            search_url: "https://duckduckgo.com/?q={q}".into(),
            currency: String::new(),
            font: "JetBrainsMono Nerd Font".into(),
            font_size: 11.0,
            width: 820,
            height: 460,
            dim: 0.35,
            theme: crate::theme::DEFAULT.into(),
            accent: String::new(),
            icons: true,
            previews: true,
            alt_release: true,
            start_windows: true,
            start_apps: 24,
            disabled: Vec::new(),
        }
    }
}

static CONFIG: LazyLock<RwLock<Config>> = LazyLock::new(|| RwLock::new(load()));

pub fn path() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("zunder/config.toml")
}

fn load() -> Config {
    match fs::read_to_string(path()) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            eprintln!("zunder: config.toml: {e}");
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}

pub fn get() -> RwLockReadGuard<'static, Config> {
    CONFIG.read().unwrap_or_else(|e| e.into_inner())
}

pub fn reload() {
    *CONFIG.write().unwrap_or_else(|e| e.into_inner()) = load();
}

/// Change the config and write it to disk.
pub fn update(f: impl FnOnce(&mut Config)) {
    let mut cfg = CONFIG.write().unwrap_or_else(|e| e.into_inner());
    f(&mut cfg);
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    match toml::to_string_pretty(&*cfg) {
        Ok(text) => {
            let _ = fs::write(&path, text);
        }
        Err(e) => eprintln!("zunder: can't save config: {e}"),
    }
}

pub fn terminal() -> String {
    let t = get().terminal.clone();
    if !t.is_empty() {
        return t;
    }
    std::env::var("TERMINAL").unwrap_or_else(|_| "kitty".into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Int,
    Float,
    Text,
}

/// One editable setting, as shown on the settings page.
pub struct Setting {
    pub key: &'static str,
    pub group: &'static str,
    pub title: &'static str,
    pub help: &'static str,
}

pub const SETTINGS: &[Setting] = &[
    Setting { key: "alt_release", group: "Switcher", title: "Release Alt to switch", help: "ALT+TAB switches when you let go of Alt" },
    Setting { key: "previews", group: "Switcher", title: "Window previews", help: "screenshot of the selected window next to the list" },
    Setting { key: "start_windows", group: "Start screen", title: "Show open windows", help: "list open windows when the query is empty" },
    Setting { key: "start_apps", group: "Start screen", title: "Apps shown", help: "recent and suggested apps when the query is empty" },
    Setting { key: "icons", group: "Appearance", title: "App icons", help: "colored icons instead of glyphs" },
    Setting { key: "font", group: "Appearance", title: "Font", help: "any installed font, Nerd Fonts show the glyphs" },
    Setting { key: "font_size", group: "Appearance", title: "Font size", help: "in pt" },
    Setting { key: "theme", group: "Appearance", title: "Theme", help: "color scheme, built-in or ~/.config/zunder/themes/*.css" },
    Setting { key: "accent", group: "Appearance", title: "Accent color", help: "hex like #f9e2af, empty for the theme default" },
    Setting { key: "dim", group: "Appearance", title: "Backdrop dim", help: "0 = clear, 1 = black" },
    Setting { key: "width", group: "Appearance", title: "Width", help: "panel width in px" },
    Setting { key: "height", group: "Appearance", title: "Height", help: "result list height in px" },
    Setting { key: "terminal", group: "General", title: "Terminal", help: "for > commands, empty = $TERMINAL or kitty" },
    Setting { key: "search_url", group: "General", title: "Search engine", help: "URL with {q} for the query" },
    Setting { key: "currency", group: "General", title: "Currency", help: "for `100 usd`, e.g. CHF; empty = from your time zone" },
];

/// Allowed values for a setting that is a pick from a list, `None` for free-form ones.
pub fn choices(key: &str) -> Option<Vec<String>> {
    match key {
        "theme" => Some(crate::theme::names()),
        _ => None,
    }
}

impl Config {
    /// Current value of a setting as text, `None` for unknown keys.
    pub fn value(&self, key: &str) -> Option<String> {
        let v = toml::Value::try_from(self).ok()?;
        Some(match v.get(key)? {
            toml::Value::String(s) => s.clone(),
            toml::Value::Boolean(b) => if *b { "on" } else { "off" }.into(),
            other => other.to_string(),
        })
    }

    pub fn is_bool(&self, key: &str) -> bool {
        self.kind(key) == Kind::Bool
    }

    pub fn kind(&self, key: &str) -> Kind {
        match toml::Value::try_from(self).ok().and_then(|v| v.get(key).cloned()) {
            Some(toml::Value::Boolean(_)) => Kind::Bool,
            Some(toml::Value::Integer(_)) => Kind::Int,
            Some(toml::Value::Float(_)) => Kind::Float,
            _ => Kind::Text,
        }
    }

    /// Set a key from text. `None` toggles a boolean.
    pub fn set(&mut self, key: &str, value: Option<&str>) -> Result<(), String> {
        let mut table = toml::Value::try_from(&*self).map_err(|e| e.to_string())?;
        let slot = table.get_mut(key).ok_or_else(|| format!("unknown setting {key}"))?;
        *slot = match (&*slot, value) {
            (toml::Value::Boolean(b), None) => toml::Value::Boolean(!b),
            (toml::Value::Boolean(_), Some(v)) => toml::Value::Boolean(matches!(v, "on" | "true" | "yes" | "1")),
            (toml::Value::Integer(_), Some(v)) => toml::Value::Integer(v.trim().parse().map_err(|_| "needs a whole number")?),
            (toml::Value::Float(_), Some(v)) => toml::Value::Float(v.trim().parse().map_err(|_| "needs a number")?),
            (toml::Value::String(_), Some(v)) => toml::Value::String(v.trim().to_owned()),
            _ => return Err("needs a value".into()),
        };
        *self = table.try_into().map_err(|e: toml::de::Error| e.to_string())?;
        Ok(())
    }

    pub fn enabled(&self, provider: &str) -> bool {
        !self.disabled.iter().any(|d| d.eq_ignore_ascii_case(provider))
    }

    pub fn toggle_provider(&mut self, provider: &str) {
        if self.enabled(provider) {
            self.disabled.push(provider.to_owned());
        } else {
            self.disabled.retain(|d| !d.eq_ignore_ascii_case(provider));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn sets_values() {
        let mut c = Config::default();
        c.set("icons", None).unwrap();
        assert!(!c.icons);
        c.set("font_size", Some("13.5")).unwrap();
        assert_eq!(c.font_size, 13.5);
        c.set("width", Some("900")).unwrap();
        assert_eq!(c.width, 900);
        assert!(c.set("width", Some("wide")).is_err());
        assert_eq!(c.value("previews").as_deref(), Some("on"));
        c.toggle_provider("Web");
        assert!(!c.enabled("web"));
    }

    #[test]
    fn theme_setting() {
        let mut c = Config::default();
        assert_eq!(c.theme, "graphite");
        assert_eq!(c.value("theme").as_deref(), Some("graphite"));
        c.set("theme", Some("nord")).unwrap();
        assert_eq!(c.theme, "nord");
        let old: Config = toml::from_str("font_size = 12.0").unwrap();
        assert_eq!(old.theme, "graphite");
        assert!(super::choices("theme").unwrap().contains(&"catppuccin-latte".to_owned()));
        assert!(super::choices("font").is_none());
    }
}
