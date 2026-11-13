//! `zunder plugin …`: install plugins from the registry at elyesghazel/zunder-plugins.
//!
//! The registry's CI publishes `index.json` (cached here for an hour). A plugin is downloaded at
//! the commit the index names, checked against its sha256 and written to the plugins folder.
//! `.installed.json` in that folder records what came from the registry; any other file there is
//! the user's own and is never touched. `update` only replaces a file whose sha256 still matches
//! the recorded one, so a hand-edited plugin survives it.
//!
//! `ZUNDER_PLUGIN_INDEX=<url or path>` points at another index, e.g. a local checkout of the
//! registry; with a path, plugin files are read relative to the index's folder.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const INDEX_URL: &str = "https://raw.githubusercontent.com/elyesghazel/zunder-plugins/main/index.json";
const RAW_BASE: &str = "https://raw.githubusercontent.com/elyesghazel/zunder-plugins";
const CACHE_FOR: Duration = Duration::from_secs(3600);
const INSTALLED: &str = ".installed.json";

pub const USAGE: &str = "usage:
  zunder plugin search [query]     plugins in the registry
  zunder plugin install <name>...
  zunder plugin update [name]...   all installed ones when no name is given
  zunder plugin remove <name>...
  zunder plugin list               installed from the registry, with commit";

#[derive(Debug, Deserialize)]
pub struct Index {
    pub version: u32,
    #[serde(default)]
    pub plugins: Vec<Entry>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub name: String,
    pub title: String,
    pub description: String,
    pub file: String,
    pub commit: String,
    pub sha256: String,
    pub requires: Vec<String>,
    pub platforms: Vec<String>,
    pub settings: Vec<Setting>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Setting {
    pub key: String,
    pub help: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Installed {
    pub commit: String,
    pub sha256: String,
}

/// What install, update or remove did, line by line: the CLI prints it, the settings window shows it.
#[derive(Debug)]
pub struct Report {
    /// false when something asked for wasn't done.
    pub ok: bool,
    pub lines: Vec<Line>,
}

#[derive(Debug, PartialEq)]
pub enum Line {
    Out(String),
    Err(String),
}

impl Report {
    fn new() -> Self {
        Report { ok: true, lines: Vec::new() }
    }

    fn out(&mut self, line: String) {
        self.lines.push(Line::Out(line));
    }

    fn warn(&mut self, line: String) {
        self.lines.push(Line::Err(line));
    }

    fn fail(&mut self, line: String) {
        self.ok = false;
        self.warn(line);
    }

    /// The lines as one sentence for a status line, what happened before the warnings.
    pub fn summary(&self) -> String {
        let outs = self.lines.iter().filter_map(|l| if let Line::Out(s) = l { Some(s.as_str()) } else { None });
        let errs = self.lines.iter().filter_map(|l| if let Line::Err(s) = l { Some(s.as_str()) } else { None });
        outs.chain(errs).collect::<Vec<_>>().join("; ")
    }

    fn print(&self) {
        for line in &self.lines {
            match line {
                Line::Out(s) => println!("{s}"),
                Line::Err(s) => eprintln!("zunder: {s}"),
            }
        }
    }
}

/// The index fell back to the cache because it couldn't be downloaded.
#[derive(Clone, Debug)]
pub struct Stale {
    pub error: String,
    /// How old the cached index is.
    pub age: Option<Duration>,
}

impl Stale {
    pub fn describe(&self) -> String {
        match self.age {
            Some(age) => format!("offline, showing the plugin index from {} ago ({})", ago(age), self.error),
            None => format!("offline, showing the cached plugin index ({})", self.error),
        }
    }
}

fn ago(age: Duration) -> String {
    let min = age.as_secs() / 60;
    match min {
        0 => "less than a minute".to_owned(),
        1..=59 => format!("{min} min"),
        60..=2879 => format!("{} h", min / 60),
        _ => format!("{} days", min / 1440),
    }
}

/// Everything the settings window shows about the registry, read off the GTK thread.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub plugins: Vec<Entry>,
    pub installed: BTreeMap<String, Installed>,
    pub stale: Option<Stale>,
    /// Binaries from `requires` that aren't on PATH, by plugin name.
    pub missing: BTreeMap<String, Vec<String>>,
    /// Names whose file is in the plugins folder without being from the registry.
    pub taken: BTreeSet<String>,
}

impl Catalog {
    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.plugins.iter().find(|p| p.name == name)
    }

    /// Installed from the registry and the index has different bytes.
    pub fn has_update(&self, name: &str) -> bool {
        match (self.installed.get(name), self.entry(name)) {
            (Some(rec), Some(entry)) => !rec.sha256.eq_ignore_ascii_case(&entry.sha256),
            _ => false,
        }
    }
}

impl Entry {
    /// Empty `platforms` means every OS.
    pub fn runs_here(&self) -> bool {
        self.platforms.is_empty() || self.platforms.iter().any(|p| p == std::env::consts::OS)
    }
}

pub fn parse_index(bytes: &[u8]) -> Result<Index, String> {
    let index: Index = serde_json::from_slice(bytes).map_err(|e| format!("can't read the plugin index: {e}"))?;
    if index.version != 1 {
        return Err(format!("plugin index version {} is newer than this zunder understands, update zunder", index.version));
    }
    // names become file names in the plugins folder
    if let Some(p) = index.plugins.iter().find(|p| !valid_name(&p.name)) {
        return Err(format!("plugin index has an invalid name `{}`", p.name));
    }
    Ok(index)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// What `update` may do with an installed plugin's file.
#[derive(Debug, PartialEq)]
pub enum LocalState {
    /// Same bytes as installed.
    Unchanged,
    /// Changed since install: leave it alone.
    Edited,
    /// Gone from the folder.
    Missing,
}

pub fn local_state(path: &Path, rec: &Installed) -> LocalState {
    match fs::read(path) {
        Ok(bytes) if sha256_hex(&bytes) == rec.sha256 => LocalState::Unchanged,
        Ok(_) => LocalState::Edited,
        Err(_) => LocalState::Missing,
    }
}

/// Where the index comes from and where plugins go; tests point it at temp folders.
pub struct Registry {
    index: String,
    cache: Option<PathBuf>,
    dir: PathBuf,
}

impl Registry {
    pub fn from_env() -> Self {
        let custom = std::env::var("ZUNDER_PLUGIN_INDEX").ok().filter(|s| !s.is_empty());
        Registry {
            cache: custom.is_none().then(|| dirs::cache_dir().unwrap_or_default().join("zunder/plugins-index.json")),
            index: custom.unwrap_or_else(|| INDEX_URL.to_owned()),
            dir: crate::providers::plugins::dir(),
        }
    }

    fn is_url(&self) -> bool {
        self.index.starts_with("http://") || self.index.starts_with("https://")
    }

    /// For the CLI: says on stderr when it falls back to the cache.
    fn load_index(&self) -> Result<Index, String> {
        let (index, stale) = self.load_index_checked()?;
        if let Some(stale) = stale {
            eprintln!("zunder: {}; using the cached index", stale.error);
        }
        Ok(index)
    }

    fn load_index_checked(&self) -> Result<(Index, Option<Stale>), String> {
        if let Some(cache) = &self.cache {
            let fresh = fs::metadata(cache)
                .and_then(|m| m.modified())
                .is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < CACHE_FOR);
            if fresh && let Ok(index) = fs::read(cache).map_err(|e| e.to_string()).and_then(|b| parse_index(&b)) {
                return Ok((index, None));
            }
        }
        match fetch(&self.index).and_then(|b| parse_index(&b).map(|i| (i, b))) {
            Ok((index, bytes)) => {
                if let Some(cache) = &self.cache {
                    let _ = fs::create_dir_all(cache.parent().unwrap_or(Path::new(".")));
                    let _ = fs::write(cache, bytes);
                }
                Ok((index, None))
            }
            // offline: an old index is better than none
            Err(e) => match self.cache.as_ref().and_then(|c| fs::read(c).ok()).and_then(|b| parse_index(&b).ok()) {
                Some(index) => {
                    let age = self
                        .cache
                        .as_ref()
                        .and_then(|c| fs::metadata(c).and_then(|m| m.modified()).ok())
                        .and_then(|t| SystemTime::now().duration_since(t).ok());
                    Ok((index, Some(Stale { error: e, age })))
                }
                None => Err(e),
            },
        }
    }

    fn file_source(&self, entry: &Entry) -> String {
        if self.is_url() {
            format!("{RAW_BASE}/{}/{}", entry.commit, entry.file)
        } else {
            Path::new(&self.index).parent().unwrap_or(Path::new(".")).join(&entry.file).to_string_lossy().into_owned()
        }
    }

    fn installed(&self) -> BTreeMap<String, Installed> {
        fs::read(self.dir.join(INSTALLED)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn save_installed(&self, map: &BTreeMap<String, Installed>) -> Result<(), String> {
        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
        fs::write(self.dir.join(INSTALLED), json + "\n").map_err(|e| format!("can't write {INSTALLED}: {e}"))
    }

    /// Download, verify and write one plugin.
    fn download(&self, entry: &Entry) -> Result<Installed, String> {
        let bytes = fetch(&self.file_source(entry))?;
        let sum = sha256_hex(&bytes);
        if !entry.sha256.eq_ignore_ascii_case(&sum) {
            return Err(format!("{}: checksum mismatch (index {}, downloaded {sum}), not installed", entry.name, entry.sha256));
        }
        fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let path = self.dir.join(&entry.name);
        let tmp = self.dir.join(format!(".{}.part", entry.name));
        fs::write(&tmp, &bytes).map_err(|e| format!("can't write {}: {e}", tmp.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        }
        fs::rename(&tmp, &path).map_err(|e| format!("can't write {}: {e}", path.display()))?;
        Ok(Installed { commit: entry.commit.clone(), sha256: sum })
    }

    pub fn search(&self, query: &str) -> Result<Vec<String>, String> {
        let index = self.load_index()?;
        let installed = self.installed();
        let q = query.to_lowercase();
        Ok(index
            .plugins
            .iter()
            .filter(|p| [&p.name, &p.title, &p.description].iter().any(|s| s.to_lowercase().contains(&q)))
            .map(|p| {
                let marker = match installed.get(&p.name) {
                    Some(rec) if rec.commit != p.commit => "  [update available]",
                    Some(_) => "  [installed]",
                    None if !p.runs_here() => "  [not for this system]",
                    None => "",
                };
                format!("{:<16} {}{marker}", p.name, p.description)
            })
            .collect())
    }

    /// The index (cached when fresh), what is installed and what each plugin is missing.
    pub fn catalog(&self) -> Result<Catalog, String> {
        let (index, stale) = self.load_index_checked()?;
        let installed = self.installed();
        let missing = index
            .plugins
            .iter()
            .map(|p| (p.name.clone(), p.requires.iter().filter(|b| !on_path(b)).cloned().collect::<Vec<_>>()))
            .filter(|(_, bins)| !bins.is_empty())
            .collect();
        let taken = index
            .plugins
            .iter()
            .filter(|p| !installed.contains_key(&p.name) && self.dir.join(&p.name).exists())
            .map(|p| p.name.clone())
            .collect();
        Ok(Catalog { plugins: index.plugins, installed, stale, missing, taken })
    }

    fn index_for(&self, report: &mut Report) -> Result<Index, String> {
        let (index, stale) = self.load_index_checked()?;
        if let Some(stale) = stale {
            report.warn(format!("{}; using the cached index", stale.error));
        }
        Ok(index)
    }

    pub fn install(&self, names: &[String]) -> Result<Report, String> {
        let mut report = Report::new();
        let index = self.index_for(&mut report)?;
        let mut installed = self.installed();
        for name in names {
            let Some(entry) = index.plugins.iter().find(|p| &p.name == name) else {
                report.fail(format!("no plugin `{name}` in the registry (try `zunder plugin search`)"));
                continue;
            };
            if !entry.runs_here() {
                report.fail(format!("{name} runs on {}, not {}", entry.platforms.join(", "), std::env::consts::OS));
                continue;
            }
            if installed.contains_key(name) {
                report.out(format!("{name} is already installed, `zunder plugin update {name}` gets the newest"));
                continue;
            }
            if self.dir.join(name).exists() {
                report.fail(format!("{} already exists and isn't from the registry, left alone", self.dir.join(name).display()));
                continue;
            }
            for bin in &entry.requires {
                if !on_path(bin) {
                    report.warn(format!("warning: {name} needs `{bin}`, which isn't on your PATH"));
                }
            }
            match self.download(entry) {
                Ok(rec) => {
                    report.out(format!("installed {name} ({})", short(&rec.commit)));
                    installed.insert(name.clone(), rec);
                    self.save_installed(&installed)?;
                }
                Err(e) => report.fail(e),
            }
        }
        Ok(report)
    }

    pub fn update(&self, names: &[String]) -> Result<Report, String> {
        let mut report = Report::new();
        let mut installed = self.installed();
        let names: Vec<String> = if names.is_empty() { installed.keys().cloned().collect() } else { names.to_vec() };
        if names.is_empty() {
            report.out("no plugins installed from the registry".to_owned());
            return Ok(report);
        }
        let index = self.index_for(&mut report)?;
        for name in &names {
            let Some(rec) = installed.get(name).cloned() else {
                report.fail(format!("{name} wasn't installed from the registry, skipped"));
                continue;
            };
            let Some(entry) = index.plugins.iter().find(|p| &p.name == name) else {
                report.out(format!("{name} is no longer in the registry, kept as it is"));
                continue;
            };
            let path = self.dir.join(name);
            match local_state(&path, &rec) {
                LocalState::Edited => {
                    report.out(format!("{name} was edited locally, skipped (remove it and install again to get the registry version)"));
                    continue;
                }
                LocalState::Unchanged if rec.commit == entry.commit && rec.sha256.eq_ignore_ascii_case(&entry.sha256) => {
                    report.out(format!("{name} is up to date ({})", short(&rec.commit)));
                    continue;
                }
                LocalState::Unchanged | LocalState::Missing => {}
            }
            match self.download(entry) {
                Ok(new) => {
                    report.out(format!("updated {name} {} -> {}", short(&rec.commit), short(&new.commit)));
                    installed.insert(name.clone(), new);
                    self.save_installed(&installed)?;
                }
                Err(e) => report.fail(e),
            }
        }
        Ok(report)
    }

    pub fn remove(&self, names: &[String]) -> Result<Report, String> {
        let mut report = Report::new();
        let mut installed = self.installed();
        for name in names {
            if installed.remove(name).is_none() {
                report.fail(format!("{name} wasn't installed from the registry, left alone"));
                continue;
            }
            match fs::remove_file(self.dir.join(name)) {
                Ok(()) => report.out(format!("removed {name}")),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => report.out(format!("removed {name} (the file was already gone)")),
                Err(e) => {
                    report.fail(format!("can't remove {name}: {e}"));
                    continue;
                }
            }
            self.save_installed(&installed)?;
        }
        Ok(report)
    }

    pub fn list(&self) -> Vec<String> {
        self.installed()
            .iter()
            .map(|(name, rec)| {
                let state = match local_state(&self.dir.join(name), rec) {
                    LocalState::Unchanged => "",
                    LocalState::Edited => "  [edited locally]",
                    LocalState::Missing => "  [file missing]",
                };
                format!("{name:<16} {}{state}", short(&rec.commit))
            })
            .collect()
    }
}

fn short(commit: &str) -> &str {
    &commit[..commit.len().min(7)]
}

/// An http(s) URL with curl, anything else as a local file.
fn fetch(src: &str) -> Result<Vec<u8>, String> {
    if !(src.starts_with("http://") || src.starts_with("https://")) {
        return fs::read(src).map_err(|e| format!("can't read {src}: {e}"));
    }
    let out = crate::platform::no_window(Command::new("curl").args(["-fsSL", "-m", "30", src]))
        .output()
        .map_err(|e| format!("can't run curl: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("can't download {src}: {}", err.trim()));
    }
    Ok(out.stdout)
}

pub fn on_path(bin: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    let exts: &[&str] = if cfg!(windows) { &["", ".exe", ".cmd", ".bat"] } else { &[""] };
    std::env::split_paths(&path).any(|dir| exts.iter().any(|ext| dir.join(format!("{bin}{ext}")).is_file()))
}

/// `zunder plugin <args>`.
pub fn run(args: &[String]) -> Result<bool, String> {
    let reg = Registry::from_env();
    let (cmd, rest) = match args.split_first() {
        Some((cmd, rest)) => (cmd.as_str(), rest),
        None => return Err(USAGE.to_owned()),
    };
    let report = match cmd {
        "search" => {
            let lines = reg.search(&rest.join(" "))?;
            if lines.is_empty() {
                println!("no plugins match");
            }
            lines.iter().for_each(|l| println!("{l}"));
            return Ok(true);
        }
        "list" => {
            let lines = reg.list();
            if lines.is_empty() {
                println!("no plugins installed from the registry");
            }
            lines.iter().for_each(|l| println!("{l}"));
            return Ok(true);
        }
        "install" | "remove" if rest.is_empty() => return Err(format!("usage: zunder plugin {cmd} <name>...")),
        "install" => reg.install(rest)?,
        "update" => reg.update(rest)?,
        "remove" => reg.remove(rest)?,
        _ => return Err(USAGE.to_owned()),
    };
    report.print();
    // the daemon rescans the folder on reload; not running is fine
    let _ = crate::ipc::send("reload");
    Ok(report.ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT_V1: &[u8] = b"#!/bin/sh\n# @zunder.name Wi-Fi\necho v1\n";
    const SCRIPT_V2: &[u8] = b"#!/bin/sh\n# @zunder.name Wi-Fi\necho v2\n";

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zunder-registry-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A registry checkout in `root`: index.json plus wifi/wifi.
    fn fixture(root: &Path, script: &[u8], commit: &str, platforms: &str) -> Registry {
        fs::create_dir_all(root.join("repo/wifi")).unwrap();
        fs::write(root.join("repo/wifi/wifi"), script).unwrap();
        let index = format!(
            r#"{{"version": 1, "plugins": [{{"name": "wifi", "title": "Wi-Fi", "description": "Connect to Wi-Fi networks",
               "file": "wifi/wifi", "commit": "{commit}", "sha256": "{}", "requires": ["definitely-not-a-binary"],
               "platforms": {platforms}, "settings": [{{"key": "iface", "help": "interface"}}]}}]}}"#,
            sha256_hex(script)
        );
        fs::write(root.join("repo/index.json"), index).unwrap();
        Registry { index: root.join("repo/index.json").to_string_lossy().into_owned(), cache: None, dir: root.join("plugins") }
    }

    #[test]
    fn parses_index() {
        let index = parse_index(
            br#"{"version": 1, "plugins": [
                {"name": "wifi", "title": "Wi-Fi", "description": "Connect", "file": "wifi/wifi", "commit": "abc",
                 "sha256": "00", "requires": ["nmcli"], "platforms": ["linux"], "settings": [{"key": "k", "help": "h"}]},
                {"name": "bare", "file": "bare/bare", "commit": "def", "sha256": "11"}]}"#,
        )
        .unwrap();
        assert_eq!(index.plugins.len(), 2);
        assert_eq!(index.plugins[0].requires, ["nmcli"]);
        assert_eq!(index.plugins[0].settings[0].key, "k");
        assert!(index.plugins[1].platforms.is_empty() && index.plugins[1].runs_here());
        assert!(parse_index(br#"{"version": 2, "plugins": []}"#).is_err());
        assert!(parse_index(b"not json").is_err());
    }

    #[test]
    fn rejects_unsafe_names() {
        let ok = r#"{"version": 1, "plugins": [{"name": "wifi", "file": "x/x", "commit": "c", "sha256": "s"}]}"#;
        assert!(parse_index(ok.as_bytes()).is_ok());
        for name in ["../evil", "a/b", ".hidden", ""] {
            let json = format!(r#"{{"version": 1, "plugins": [{{"name": "{name}", "file": "x/x", "commit": "c", "sha256": "s"}}]}}"#);
            assert!(parse_index(json.as_bytes()).is_err(), "{name}");
        }
    }

    #[test]
    fn sha256_matches_known_value() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn detects_local_edits() {
        let dir = tmp("state");
        let path = dir.join("p");
        let rec = Installed { commit: "c".into(), sha256: sha256_hex(SCRIPT_V1) };
        assert_eq!(local_state(&path, &rec), LocalState::Missing);
        fs::write(&path, SCRIPT_V1).unwrap();
        assert_eq!(local_state(&path, &rec), LocalState::Unchanged);
        fs::write(&path, b"#!/bin/sh\necho mine\n").unwrap();
        assert_eq!(local_state(&path, &rec), LocalState::Edited);
    }

    #[test]
    fn install_update_remove() {
        let root = tmp("flow");
        let reg = fixture(&root, SCRIPT_V1, "1111111aaaa", "[]");
        let wifi = root.join("plugins/wifi");

        assert!(reg.search("wi-fi").unwrap()[0].starts_with("wifi"));
        assert!(reg.install(&["wifi".into()]).unwrap().ok);
        assert_eq!(fs::read(&wifi).unwrap(), SCRIPT_V1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&wifi).unwrap().permissions().mode() & 0o777, 0o755);
        }
        assert_eq!(reg.installed()["wifi"].commit, "1111111aaaa");
        assert!(reg.list()[0].contains("1111111"));
        assert!(reg.search("").unwrap()[0].ends_with("[installed]"));

        // a new version is picked up
        let reg = fixture(&root, SCRIPT_V2, "2222222bbbb", "[]");
        assert!(reg.search("wifi").unwrap()[0].ends_with("[update available]"));
        assert!(reg.update(&[]).unwrap().ok);
        assert_eq!(fs::read(&wifi).unwrap(), SCRIPT_V2);
        assert_eq!(reg.installed()["wifi"].commit, "2222222bbbb");

        // a hand edit survives the next update
        fs::write(&wifi, b"#!/bin/sh\necho mine\n").unwrap();
        let reg = fixture(&root, SCRIPT_V1, "3333333cccc", "[]");
        assert!(reg.update(&[]).unwrap().ok);
        assert_eq!(fs::read(&wifi).unwrap(), b"#!/bin/sh\necho mine\n");
        assert_eq!(reg.installed()["wifi"].commit, "2222222bbbb");
        assert!(reg.list()[0].contains("edited locally"));

        assert!(reg.remove(&["wifi".into()]).unwrap().ok);
        assert!(!wifi.exists());
        assert!(reg.installed().is_empty());
    }

    #[test]
    fn catalog_for_the_settings_window() {
        let root = tmp("catalog");
        let reg = fixture(&root, SCRIPT_V1, "1111111", "[]");
        let c = reg.catalog().unwrap();
        assert_eq!(c.plugins.len(), 1);
        assert!(c.installed.is_empty() && c.taken.is_empty() && c.stale.is_none());
        assert_eq!(c.missing["wifi"], ["definitely-not-a-binary"]);

        let report = reg.install(&["wifi".into()]).unwrap();
        assert!(report.ok);
        assert!(report.summary().contains("installed wifi (1111111)"), "{}", report.summary());
        assert!(!reg.catalog().unwrap().has_update("wifi"));
        let reg = fixture(&root, SCRIPT_V2, "2222222", "[]");
        assert!(reg.catalog().unwrap().has_update("wifi"));

        assert!(reg.remove(&["wifi".into()]).unwrap().ok);
        fs::write(root.join("plugins/wifi"), b"mine").unwrap();
        assert!(reg.catalog().unwrap().taken.contains("wifi"));
    }

    #[test]
    fn falls_back_to_a_stale_cache() {
        let root = tmp("stale");
        let mut reg = fixture(&root, SCRIPT_V1, "1111111", "[]");
        let cache = root.join("cache.json");
        fs::copy(root.join("repo/index.json"), &cache).unwrap();
        // older than CACHE_FOR, and the index itself is gone
        let old = SystemTime::now() - Duration::from_secs(3 * 3600);
        fs::File::options().write(true).open(&cache).unwrap().set_modified(old).unwrap();
        fs::remove_file(root.join("repo/index.json")).unwrap();
        reg.cache = Some(cache);
        let stale = reg.catalog().unwrap().stale.expect("stale");
        assert!(stale.describe().contains("from 3 h ago"), "{}", stale.describe());
    }

    #[test]
    fn leaves_user_files_alone() {
        let root = tmp("own");
        let reg = fixture(&root, SCRIPT_V1, "1111111", "[]");
        fs::create_dir_all(root.join("plugins")).unwrap();
        fs::write(root.join("plugins/wifi"), b"mine").unwrap();
        fs::write(root.join("plugins/mine"), b"mine").unwrap();
        assert!(!reg.install(&["wifi".into()]).unwrap().ok);
        assert!(!reg.update(&["mine".into()]).unwrap().ok);
        assert!(!reg.remove(&["mine".into(), "wifi".into()]).unwrap().ok);
        assert_eq!(fs::read(root.join("plugins/wifi")).unwrap(), b"mine");
        assert_eq!(fs::read(root.join("plugins/mine")).unwrap(), b"mine");
    }

    #[test]
    fn refuses_bad_checksum_and_other_platforms() {
        let root = tmp("refuse");
        let reg = fixture(&root, SCRIPT_V1, "1111111", r#"["plan9"]"#);
        assert!(!reg.install(&["wifi".into()]).unwrap().ok);
        assert!(!root.join("plugins/wifi").exists());

        let reg = fixture(&root, SCRIPT_V1, "1111111", "[]");
        fs::write(root.join("repo/wifi/wifi"), SCRIPT_V2).unwrap();
        assert!(!reg.install(&["wifi".into()]).unwrap().ok);
        assert!(!root.join("plugins/wifi").exists());
        assert!(reg.installed().is_empty());
    }
}
