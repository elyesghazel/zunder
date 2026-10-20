//! Files in your home folder, via `fd`. A few show up in every search from 3 letters on;
//! `f <name>` or `file <name>` searches only files and shows more.

use std::path::Path;
use std::process::Command;

use crate::engine::{Ctx, Job, Provider};
use crate::item::{Action, Item};

pub struct Files;

const PREFIXES: &[&str] = &["file ", "f "];
/// Results in the normal search / with the prefix.
const INLINE: usize = 5;
const FULL: usize = 60;
/// Folders full of files nobody searches for by name.
const SKIP: &[&str] = &[
    "node_modules", "target", "go/pkg", "__pycache__", ".venv", "venv", "site-packages",
    "vendor", "dist", "build", ".git", "Steam", "snap",
];

fn prefixed(query: &str) -> Option<&str> {
    PREFIXES.iter().find_map(|p| query.strip_prefix(p)).map(str::trim)
}

impl Provider for Files {
    fn title(&self) -> &str {
        "Files"
    }

    fn exclusive(&self, query: &str) -> bool {
        prefixed(query).is_some()
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        match prefixed(cx.query) {
            Some("") => vec![Item {
                title: "type a file or folder name".into(),
                subtitle: "searches your home folder with fd".into(),
                glyph: "\u{f15b}".into(),
                ..Default::default()
            }],
            _ => Vec::new(), // results come from `deferred`
        }
    }

    fn deferred(&self, cx: &Ctx) -> Option<(String, Job)> {
        let (term, limit) = match prefixed(cx.query) {
            Some(term) if !term.is_empty() => (term.to_owned(), FULL),
            Some(_) => return None,
            // inline only for word-like queries: `30c` or `2+2` shouldn't dig through files
            None if cx.query.chars().count() >= 3 && !cx.switch && cx.query.starts_with(char::is_alphabetic) => {
                (cx.query.trim().to_owned(), INLINE)
            }
            None => return None,
        };
        Some(("Files".into(), Box::new(move || find(&term, limit))))
    }
}

/// `fd` over $HOME (hidden and git-ignored files skipped), ranked by how well the name matches.
fn find(term: &str, limit: usize) -> Vec<Item> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut words = term.split_whitespace().map(str::to_lowercase);
    let Some(first) = words.next() else { return Vec::new() };
    let rest: Vec<String> = words.collect();
    let mut fd = Command::new("fd");
    fd.args(["--ignore-case", "--absolute-path", "--max-results", "300"]);
    for dir in SKIP {
        fd.args(["--exclude", dir]);
    }
    fd.args(["--", &regex::escape(&first)]).arg(&home);
    let out = fd.output().or_else(|_| {
        // Debian and Ubuntu ship fd as `fdfind`
        let mut alt = Command::new("fdfind");
        alt.args(fd.get_args()).output()
    });
    let Ok(out) = out else {
        return vec![Item {
            title: "fd is not installed".into(),
            subtitle: "pacman -S fd".into(),
            glyph: "\u{f071}".into(),
            ..Default::default()
        }];
    };
    let home_str = home.to_string_lossy().into_owned();
    let mut found: Vec<(i64, String)> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|p| p.trim_end_matches('/').to_owned())
        .filter(|p| {
            let lower = p.to_lowercase();
            rest.iter().all(|w| lower.contains(w.as_str()))
        })
        .map(|p| {
            let name = Path::new(&p).file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            let stem = Path::new(&p).file_stem().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            let depth = p.matches('/').count() as i64;
            let score = if name == first || stem == first {
                300
            } else if name.starts_with(&first) {
                200
            } else {
                100
            } - depth * 10
                - name.len() as i64 / 4;
            (score, p)
        })
        .collect();
    found.sort_by_key(|x| std::cmp::Reverse(x.0));
    found
        .into_iter()
        .take(limit)
        .map(|(score, path)| {
            let p = Path::new(&path);
            let is_dir = p.is_dir();
            let content_type = if is_dir { String::new() } else { gtk4::gio::content_type_guess(Some(&path), None).0.to_string() };
            let parent = p.parent().map(|d| d.to_string_lossy().replacen(&home_str, "~", 1)).unwrap_or_default();
            Item {
                id: format!("file:{path}"),
                title: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone()),
                subtitle: parent,
                hint: if is_dir { "folder".into() } else { "file".into() },
                glyph: if is_dir { "\u{f07b}".into() } else { "\u{f15b}".into() },
                icon: icon_name(&content_type, is_dir),
                image: content_type.starts_with("image/").then(|| path.clone()),
                action: Action::Open(path.clone()),
                score,
                ..Default::default()
            }
        })
        .collect()
}

fn icon_name(content_type: &str, is_dir: bool) -> String {
    if is_dir {
        return "folder".into();
    }
    gtk4::gio::content_type_get_generic_icon_name(content_type).map(|s| s.to_string()).unwrap_or_default()
}
