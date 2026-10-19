//! Script plugins: any executable in ~/.config/zunder/plugins, in any language.
//!
//! Header comments (all optional) describe the plugin:
//!
//!     # @zunder.name   Kill Process
//!     # @zunder.prefix kill            typing `kill <query>` runs the script
//!     # @zunder.glyph  \uf120          a Nerd Font glyph, literal or \uXXXX
//!     # @zunder.mode   search          `search` (default) or `command`
//!
//! search:  called as `script <query>`; each stdout line is a result, either plain text
//!          (Enter copies it) or JSON: {"title", "subtitle", "hint", "glyph", "action": {"open": "…"}}
//!          where action is one of run, term, open, copy, focus, fill.
//! command: shows up in the main search by name; Enter runs it once.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use serde::Deserialize;

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item, shell_quote};

#[derive(Clone, Debug)]
pub struct Plugin {
    pub name: String,
    pub prefix: String,
    pub glyph: String,
    pub command: bool,
    pub path: PathBuf,
}

pub struct Plugins {
    list: Vec<Plugin>,
}

/// A pending search-mode run: `path arg`.
pub struct Call {
    pub plugin: Plugin,
    pub arg: String,
}

pub fn dir() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("zunder/plugins")
}

impl Plugins {
    pub fn load() -> Self {
        Plugins { list: scan() }
    }

    pub fn list(&self) -> &[Plugin] {
        &self.list
    }

    /// The search-mode plugin whose `prefix ` the query starts with.
    pub fn call(&self, query: &str) -> Option<Call> {
        self.list.iter().filter(|p| !p.command).find_map(|p| {
            let rest = query.strip_prefix(&p.prefix)?.strip_prefix(' ')?;
            Some(Call { plugin: p.clone(), arg: rest.trim().to_owned() })
        })
    }
}

impl Provider for Plugins {
    fn title(&self) -> &str {
        "Plugins"
    }

    fn reload(&mut self) {
        self.list = scan();
    }

    fn exclusive(&self, query: &str) -> bool {
        self.call(query).is_some()
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        if let Some(call) = self.call(cx.query) {
            return vec![Item { title: "…".into(), glyph: call.plugin.glyph, ..Default::default() }];
        }
        if cx.query.is_empty() {
            return Vec::new();
        }
        self.list
            .iter()
            .filter_map(|p| {
                let score = cx.score(&p.name).max(cx.score(&p.prefix))?;
                let id = format!("plugin:{}", p.name);
                let (hint, action) = if p.command {
                    ("script".to_owned(), Action::Run(shell_quote(&p.path.to_string_lossy())))
                } else {
                    (format!("{} ␣", p.prefix), Action::Fill(format!("{} ", p.prefix)))
                };
                Some(Item {
                    title: p.name.clone(),
                    subtitle: String::new(),
                    hint,
                    glyph: p.glyph.clone(),
                    action,
                    score: score + cx.history.boost(&id),
                    id,
                    ..Default::default()
                })
            })
            .collect()
    }
}

#[derive(Deserialize)]
struct Line {
    title: String,
    #[serde(default)]
    subtitle: String,
    #[serde(default)]
    hint: String,
    glyph: Option<String>,
    #[serde(default)]
    action: Action,
}

impl Call {
    /// Run the script (blocking; call from a worker thread).
    pub fn run(&self) -> Vec<Item> {
        let out = match Command::new(&self.plugin.path).arg(&self.arg).output() {
            Ok(out) => out,
            Err(e) => return vec![error_item(&format!("failed to run: {e}"))],
        };
        if !out.status.success() && out.stdout.is_empty() {
            let err = String::from_utf8_lossy(&out.stderr);
            return vec![error_item(err.lines().last().unwrap_or("plugin failed"))];
        }
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .enumerate()
            .map(|(i, line)| {
                let score = -(i as i64);
                match serde_json::from_str::<Line>(line) {
                    Ok(l) => Item {
                        id: format!("plugin:{}:{}", self.plugin.name, l.title),
                        title: l.title,
                        subtitle: l.subtitle,
                        hint: l.hint,
                        glyph: l.glyph.unwrap_or_else(|| self.plugin.glyph.clone()),
                        action: l.action,
                        score,
                        ..Default::default()
                    },
                    Err(_) => Item {
                        title: line.to_owned(),
                        hint: "copy".into(),
                        glyph: self.plugin.glyph.clone(),
                        action: Action::Copy(line.to_owned()),
                        score,
                        ..Default::default()
                    },
                }
            })
            .collect()
    }
}

fn error_item(msg: &str) -> Item {
    Item { title: msg.to_owned(), glyph: "\u{f071}".into(), hint: "error".into(), ..Default::default() }
}

/// `\uf120` → the char, so headers stay plain ASCII.
fn unescape(s: &str) -> String {
    s.strip_prefix("\\u")
        .and_then(|hex| u32::from_str_radix(hex, 16).ok())
        .and_then(char::from_u32)
        .map_or_else(|| s.to_owned(), String::from)
}

fn scan() -> Vec<Plugin> {
    let Ok(entries) = fs::read_dir(dir()) else { return Vec::new() };
    let mut list: Vec<Plugin> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let meta = fs::metadata(&path).ok()?;
            if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
                return None;
            }
            let stem = path.file_stem()?.to_string_lossy().into_owned();
            let mut p = Plugin { name: stem.clone(), prefix: stem, glyph: "\u{f12e}".into(), command: false, path: path.clone() };
            let head = fs::read(&path).ok()?;
            for line in String::from_utf8_lossy(&head[..head.len().min(4096)]).lines().take(30) {
                let Some((_, rest)) = line.split_once("@zunder.") else { continue };
                let (key, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                let value = value.trim().to_owned();
                match key {
                    "name" => p.name = value,
                    "prefix" => p.prefix = value,
                    "glyph" => p.glyph = unescape(&value),
                    "mode" => p.command = value == "command",
                    _ => {}
                }
            }
            Some(p)
        })
        .collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

#[cfg(test)]
mod tests {
    use super::unescape;

    #[test]
    fn unescapes_glyphs() {
        assert_eq!(unescape("\\uf05e"), "\u{f05e}");
        assert_eq!(unescape("x"), "x");
        assert_eq!(unescape("\\uzz"), "\\uzz");
    }
}
