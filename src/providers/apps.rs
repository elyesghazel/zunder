//! Desktop applications from the XDG data dirs.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item, terminal};

#[derive(Clone, Debug)]
pub struct App {
    pub id: String,
    pub name: String,
    pub generic: String,
    pub comment: String,
    pub keywords: String,
    pub exec: String,
    pub icon: String,
    pub wm_class: String,
    pub categories: String,
    pub terminal: bool,
}

pub struct Apps {
    apps: Vec<App>,
}

const RECENT: usize = 8;

impl Apps {
    pub fn load() -> Self {
        Apps { apps: scan_all() }
    }

    fn item(&self, app: &App, score: i64) -> Item {
        let cmd = if app.terminal { format!("{} -e {}", terminal(), app.exec) } else { app.exec.clone() };
        let subtitle = [&app.comment, &app.generic].into_iter().find(|s| !s.is_empty()).cloned().unwrap_or_default();
        Item {
            id: format!("app:{}", app.id),
            title: app.name.clone(),
            subtitle,
            hint: "app".into(),
            glyph: "\u{f135}".into(),
            icon: app.icon.clone(),
            action: Action::Run(cmd),
            score,
            ..Default::default()
        }
    }
}

impl Provider for Apps {
    fn title(&self) -> &str {
        "Apps"
    }

    fn reload(&mut self) {
        self.apps = scan_all();
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        if cx.query.is_empty() {
            if cx.switch {
                return Vec::new();
            }
            let mut recent: Vec<(u32, &App)> = self
                .apps
                .iter()
                .map(|a| (cx.history.count(&format!("app:{}", a.id)), a))
                .filter(|(n, _)| *n > 0)
                .collect();
            recent.sort_by(|a, b| b.0.cmp(&a.0));
            return recent.into_iter().take(RECENT).map(|(n, a)| self.item(a, i64::from(n))).collect();
        }
        self.apps
            .iter()
            .filter_map(|a| {
                let best = [
                    cx.score(&a.name).map(|s| s * 2),
                    cx.score(&a.generic),
                    cx.score(&a.keywords),
                    cx.score(a.exec.split_whitespace().next().unwrap_or_default()),
                ]
                .into_iter()
                .flatten()
                .max()?;
                Some(self.item(a, best + cx.history.boost(&format!("app:{}", a.id))))
            })
            .collect()
    }
}

fn data_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = dirs::data_dir().into_iter().collect();
    let sys = std::env::var("XDG_DATA_DIRS").ok().filter(|s| !s.is_empty());
    dirs.extend(sys.as_deref().unwrap_or("/usr/local/share:/usr/share").split(':').map(PathBuf::from));
    dirs
}

pub fn scan_all() -> Vec<App> {
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in data_dirs() {
        let root = dir.join("applications");
        scan(&root, &root, &mut seen, &mut apps);
    }
    // the same app often ships several entries (flatpak + native, aliases): keep the first
    let mut names = HashSet::new();
    apps.retain(|a| names.insert(a.name.to_lowercase()));
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

fn scan(root: &Path, dir: &Path, seen: &mut HashSet<String>, apps: &mut Vec<App>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan(root, &path, seen, apps);
            continue;
        }
        if path.extension().is_none_or(|e| e != "desktop") {
            continue;
        }
        // desktop file id: path relative to applications/, '/' → '-'; first one found wins
        let id = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('/', "-");
        if seen.insert(id.clone())
            && let Some(app) = parse(&path, id)
        {
            apps.push(app);
        }
    }
}

fn parse(path: &Path, id: String) -> Option<App> {
    let text = fs::read_to_string(path).ok()?;
    let mut fields: HashMap<&str, &str> = HashMap::new();
    let mut in_entry = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry
            && !line.starts_with('#')
            && let Some((k, v)) = line.split_once('=')
        {
            let k = k.trim();
            if !k.contains('[') {
                fields.entry(k).or_insert(v.trim());
            }
        }
    }
    let flag = |k: &str| fields.get(k).is_some_and(|v| *v == "true");
    if fields.get("Type") != Some(&"Application") || flag("NoDisplay") || flag("Hidden") {
        return None;
    }
    let get = |k: &str| fields.get(k).copied().unwrap_or_default().to_owned();
    Some(App {
        id,
        name: fields.get("Name")?.to_string(),
        generic: get("GenericName"),
        comment: get("Comment"),
        keywords: get("Keywords").replace(';', " "),
        exec: clean_exec(fields.get("Exec")?),
        icon: get("Icon"),
        wm_class: get("StartupWMClass"),
        categories: get("Categories"),
        terminal: flag("Terminal"),
    })
}

/// Drop the `%f %U …` field codes from an Exec line.
fn clean_exec(exec: &str) -> String {
    let mut out = String::with_capacity(exec.len());
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            if let Some('%') = chars.next() {
                out.push('%');
            }
        } else {
            out.push(c);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::clean_exec;

    #[test]
    fn strips_field_codes() {
        assert_eq!(clean_exec("firefox %u"), "firefox");
        assert_eq!(clean_exec("app --flag %F --x"), "app --flag --x");
        assert_eq!(clean_exec("printf 100%%"), "printf 100%");
    }
}
