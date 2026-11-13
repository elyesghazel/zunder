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

use std::collections::BTreeMap;
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

impl Entry {
    /// Empty `platforms` means every OS.
    fn runs_here(&self) -> bool {
        self.platforms.is_empty() || self.platforms.iter().any(|p| p == std::env::consts::OS)
    }
}

pub fn parse_index(bytes: &[u8]) -> Result<Index, String> {
    let index: Index = serde_json::from_slice(bytes).map_err(|e| format!("can't read the plugin index: {e}"))?;
    if index.version != 1 {
        return Err(format!("plugin index version {} is newer than this zunder understands, update zunder", index.version));
    }
    Ok(index)
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

    fn load_index(&self) -> Result<Index, String> {
        if let Some(cache) = &self.cache {
            let fresh = fs::metadata(cache)
                .and_then(|m| m.modified())
                .is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < CACHE_FOR);
            if fresh && let Ok(index) = fs::read(cache).map_err(|e| e.to_string()).and_then(|b| parse_index(&b)) {
                return Ok(index);
            }
        }
        match fetch(&self.index).and_then(|b| parse_index(&b).map(|i| (i, b))) {
            Ok((index, bytes)) => {
                if let Some(cache) = &self.cache {
                    let _ = fs::create_dir_all(cache.parent().unwrap_or(Path::new(".")));
                    let _ = fs::write(cache, bytes);
                }
                Ok(index)
            }
            // offline: an old index is better than none
            Err(e) => match self.cache.as_ref().and_then(|c| fs::read(c).ok()).and_then(|b| parse_index(&b).ok()) {
                Some(index) => {
                    eprintln!("zunder: {e}; using the cached index");
                    Ok(index)
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

    pub fn install(&self, names: &[String]) -> Result<bool, String> {
        let index = self.load_index()?;
        let mut installed = self.installed();
        let mut ok = true;
        for name in names {
            let Some(entry) = index.plugins.iter().find(|p| &p.name == name) else {
                eprintln!("zunder: no plugin `{name}` in the registry (try `zunder plugin search`)");
                ok = false;
                continue;
            };
            if !entry.runs_here() {
                eprintln!("zunder: {name} runs on {}, not {}", entry.platforms.join(", "), std::env::consts::OS);
                ok = false;
                continue;
            }
            if installed.contains_key(name) {
                println!("{name} is already installed, `zunder plugin update {name}` gets the newest");
                continue;
            }
            if self.dir.join(name).exists() {
                eprintln!("zunder: {} already exists and isn't from the registry, left alone", self.dir.join(name).display());
                ok = false;
                continue;
            }
            for bin in &entry.requires {
                if !on_path(bin) {
                    eprintln!("zunder: warning: {name} needs `{bin}`, which isn't on your PATH");
                }
            }
            match self.download(entry) {
                Ok(rec) => {
                    println!("installed {name} ({})", short(&rec.commit));
                    installed.insert(name.clone(), rec);
                    self.save_installed(&installed)?;
                }
                Err(e) => {
                    eprintln!("zunder: {e}");
                    ok = false;
                }
            }
        }
        Ok(ok)
    }

    pub fn update(&self, names: &[String]) -> Result<bool, String> {
        let mut installed = self.installed();
        let names: Vec<String> = if names.is_empty() { installed.keys().cloned().collect() } else { names.to_vec() };
        if names.is_empty() {
            println!("no plugins installed from the registry");
            return Ok(true);
        }
        let index = self.load_index()?;
        let mut ok = true;
        for name in &names {
            let Some(rec) = installed.get(name).cloned() else {
                eprintln!("zunder: {name} wasn't installed from the registry, skipped");
                ok = false;
                continue;
            };
            let Some(entry) = index.plugins.iter().find(|p| &p.name == name) else {
                println!("{name} is no longer in the registry, kept as it is");
                continue;
            };
            let path = self.dir.join(name);
            match local_state(&path, &rec) {
                LocalState::Edited => {
                    println!("{name} was edited locally, skipped (remove it and install again to get the registry version)");
                    continue;
                }
                LocalState::Unchanged if rec.commit == entry.commit && rec.sha256.eq_ignore_ascii_case(&entry.sha256) => {
                    println!("{name} is up to date ({})", short(&rec.commit));
                    continue;
                }
                LocalState::Unchanged | LocalState::Missing => {}
            }
            match self.download(entry) {
                Ok(new) => {
                    println!("updated {name} {} -> {}", short(&rec.commit), short(&new.commit));
                    installed.insert(name.clone(), new);
                    self.save_installed(&installed)?;
                }
                Err(e) => {
                    eprintln!("zunder: {e}");
                    ok = false;
                }
            }
        }
        Ok(ok)
    }

    pub fn remove(&self, names: &[String]) -> Result<bool, String> {
        let mut installed = self.installed();
        let mut ok = true;
        for name in names {
            if installed.remove(name).is_none() {
                eprintln!("zunder: {name} wasn't installed from the registry, left alone");
                ok = false;
                continue;
            }
            match fs::remove_file(self.dir.join(name)) {
                Ok(()) => println!("removed {name}"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => println!("removed {name} (the file was already gone)"),
                Err(e) => {
                    eprintln!("zunder: can't remove {name}: {e}");
                    ok = false;
                    continue;
                }
            }
            self.save_installed(&installed)?;
        }
        Ok(ok)
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

fn on_path(bin: &str) -> bool {
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
    let changed = match cmd {
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
    // the daemon rescans the folder on reload; not running is fine
    let _ = crate::ipc::send("reload");
    Ok(changed)
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
        assert!(reg.install(&["wifi".into()]).unwrap());
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
        assert!(reg.update(&[]).unwrap());
        assert_eq!(fs::read(&wifi).unwrap(), SCRIPT_V2);
        assert_eq!(reg.installed()["wifi"].commit, "2222222bbbb");

        // a hand edit survives the next update
        fs::write(&wifi, b"#!/bin/sh\necho mine\n").unwrap();
        let reg = fixture(&root, SCRIPT_V1, "3333333cccc", "[]");
        assert!(reg.update(&[]).unwrap());
        assert_eq!(fs::read(&wifi).unwrap(), b"#!/bin/sh\necho mine\n");
        assert_eq!(reg.installed()["wifi"].commit, "2222222bbbb");
        assert!(reg.list()[0].contains("edited locally"));

        assert!(reg.remove(&["wifi".into()]).unwrap());
        assert!(!wifi.exists());
        assert!(reg.installed().is_empty());
    }

    #[test]
    fn leaves_user_files_alone() {
        let root = tmp("own");
        let reg = fixture(&root, SCRIPT_V1, "1111111", "[]");
        fs::create_dir_all(root.join("plugins")).unwrap();
        fs::write(root.join("plugins/wifi"), b"mine").unwrap();
        fs::write(root.join("plugins/mine"), b"mine").unwrap();
        assert!(!reg.install(&["wifi".into()]).unwrap());
        assert!(!reg.update(&["mine".into()]).unwrap());
        assert!(!reg.remove(&["mine".into(), "wifi".into()]).unwrap());
        assert_eq!(fs::read(root.join("plugins/wifi")).unwrap(), b"mine");
        assert_eq!(fs::read(root.join("plugins/mine")).unwrap(), b"mine");
    }

    #[test]
    fn refuses_bad_checksum_and_other_platforms() {
        let root = tmp("refuse");
        let reg = fixture(&root, SCRIPT_V1, "1111111", r#"["plan9"]"#);
        assert!(!reg.install(&["wifi".into()]).unwrap());
        assert!(!root.join("plugins/wifi").exists());

        let reg = fixture(&root, SCRIPT_V1, "1111111", "[]");
        fs::write(root.join("repo/wifi/wifi"), SCRIPT_V2).unwrap();
        assert!(!reg.install(&["wifi".into()]).unwrap());
        assert!(!root.join("plugins/wifi").exists());
        assert!(reg.installed().is_empty());
    }
}
