//! Color themes: built-in palettes plus `~/.config/zunder/themes/<name>.css`.
//!
//! A theme is a block of `@define-color` lines for the names `src/style.css` uses. It is loaded
//! above the built-in CSS and below `~/.config/zunder/style.css`, so it recolors everything
//! without touching the rules.

use std::fs;
use std::path::PathBuf;

/// The color names every theme defines, in this order.
pub const COLORS: [&str; 9] = ["bg", "fg", "muted", "faint", "ghost", "accent", "green", "sel", "hover"];

pub const DEFAULT: &str = "graphite";

struct Builtin {
    name: &'static str,
    /// Same order as `COLORS`.
    colors: [&'static str; 9],
}

#[rustfmt::skip]
const BUILTIN: &[Builtin] = &[
    //                           bg         fg         muted      faint      ghost      accent     green      sel        hover
    Builtin { name: "graphite",         colors: ["#0e0e14", "#e6e6ee", "#9a9aac", "#5a5a6a", "#34343f", "#b4befe", "#a6e3a1", "#262636", "#1b1b27"] },
    Builtin { name: "catppuccin-mocha", colors: ["#1e1e2e", "#cdd6f4", "#a6adc8", "#7f849c", "#45475a", "#cba6f7", "#a6e3a1", "#313244", "#28283a"] },
    Builtin { name: "catppuccin-latte", colors: ["#eff1f5", "#4c4f69", "#6c6f85", "#8c8fa1", "#bcc0cc", "#8839ef", "#40a02b", "#ccd0da", "#e6e9ef"] },
    Builtin { name: "rose-pine",        colors: ["#191724", "#e0def4", "#908caa", "#6e6a86", "#403d52", "#c4a7e7", "#9ccfd8", "#26233a", "#1f1d2e"] },
    Builtin { name: "rose-pine-dawn",   colors: ["#faf4ed", "#575279", "#797593", "#9893a5", "#cecacd", "#907aa9", "#286983", "#dfdad9", "#f2e9e1"] },
    Builtin { name: "gruvbox-dark",     colors: ["#282828", "#ebdbb2", "#bdae93", "#928374", "#504945", "#fabd2f", "#b8bb26", "#3c3836", "#32302f"] },
    Builtin { name: "tokyo-night",      colors: ["#1a1b26", "#c0caf5", "#a9b1d6", "#565f89", "#3b4261", "#7aa2f7", "#9ece6a", "#292e42", "#1f2335"] },
    Builtin { name: "nord",             colors: ["#2e3440", "#eceff4", "#d8dee9", "#7b88a1", "#4c566a", "#88c0d0", "#a3be8c", "#3b4252", "#353c4a"] },
    Builtin { name: "everforest-dark",  colors: ["#2d353b", "#d3c6aa", "#9da9a0", "#7a8478", "#4f585e", "#a7c080", "#a7c080", "#3d484d", "#343f44"] },
    Builtin { name: "dracula",          colors: ["#282a36", "#f8f8f2", "#bfc4de", "#6272a4", "#44475a", "#bd93f9", "#50fa7b", "#44475a", "#343746"] },
    Builtin { name: "kanagawa",         colors: ["#1f1f28", "#dcd7ba", "#c8c093", "#727169", "#54546d", "#7e9cd8", "#98bb6c", "#2d4f67", "#2a2a37"] },
    Builtin { name: "one-dark",         colors: ["#282c34", "#abb2bf", "#828997", "#5c6370", "#4b5263", "#61afef", "#98c379", "#3e4451", "#2c313c"] },
];

/// `~/.config/zunder/themes/`, one `<name>.css` per theme.
pub fn dir() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("zunder/themes")
}

fn user_css(name: &str) -> Option<String> {
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return None;
    }
    fs::read_to_string(dir().join(format!("{name}.css"))).ok()
}

fn builtin_css(t: &Builtin) -> String {
    COLORS.iter().zip(t.colors).map(|(name, value)| format!("@define-color {name} {value};\n")).collect()
}

/// The `@define-color` block for `name`. A user theme wins over a built-in of the same name,
/// an unknown name gives graphite.
pub fn css(name: &str) -> String {
    if let Some(text) = user_css(name) {
        return text;
    }
    builtin_css(BUILTIN.iter().find(|t| t.name == name).unwrap_or(&BUILTIN[0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_define_every_color() {
        for t in BUILTIN {
            let css = builtin_css(t);
            for c in COLORS {
                assert!(css.contains(&format!("@define-color {c} #")), "{} misses {c}", t.name);
            }
            for v in t.colors {
                assert!(v.len() == 7 && v.starts_with('#') && v[1..].chars().all(|c| c.is_ascii_hexdigit()), "{}: {v}", t.name);
            }
        }
    }

    #[test]
    fn unknown_falls_back_to_graphite() {
        assert_eq!(css("no-such-theme-xyz"), builtin_css(&BUILTIN[0]));
        assert_eq!(css("../etc/passwd"), builtin_css(&BUILTIN[0]));
    }

    #[test]
    fn graphite_matches_style_css() {
        let style = include_str!("style.css");
        for (name, value) in COLORS.iter().zip(BUILTIN[0].colors) {
            let line = style.lines().find(|l| l.starts_with(&format!("@define-color {name} "))).unwrap();
            assert!(line.contains(value), "{line}");
        }
    }
}
