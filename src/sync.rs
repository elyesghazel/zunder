//! `zunder sync`: config, favorites, themes, plugins and keybinds through one private gist.
//!
//! The gist is found by its description (`zunder-sync`) and created on the first push. Files:
//!
//! | gist                 | local                                                        |
//! |----------------------|--------------------------------------------------------------|
//! | `config.toml`        | config.toml without `[plugins.*]` (plugin tokens stay local) |
//! | `favorites.json`     | favorites.json                                               |
//! | `theme--<name>.css`  | themes/<name>.css                                            |
//! | `plugins.json`       | registry plugin names and commits from plugins/.installed.json |
//! | `binds-<os>.json`    | binds.json, one per system (combos differ between them)      |
//!
//! Merge rule, per file: the hash both sides had after the last sync is kept in
//! `<data dir>/zunder/sync.json`. A side that still has that hash didn't change, so the other one
//! wins. When both changed, the newer one wins: the gist's `updated_at` against the local mtime.
//! The overwritten side is kept next to the local file as `<file>.bak`. Deletions don't sync.
//!
//! The server pulls on start and every hour and pushes ~5 s after one of the files changed.
//! Nothing touches the network while `sync = false` or without a GitHub token.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::registry::{Installed, Line, Registry, sha256_hex};

const DESCRIPTION: &str = "zunder-sync";
const API: &str = "https://api.github.com/gists";
const STATE: &str = "sync.json";
const THEME_PREFIX: &str = "theme--";

pub const USAGE: &str = "usage:
  zunder sync          pull what changed elsewhere, push what changed here
  zunder sync --pull   take the gist's version of every file that differs
  zunder sync --push   put this machine's version of every file that differs in the gist";

fn token() -> Option<String> {
    crate::auth::token()
}

/// Sync is on and there is a token; otherwise nothing may go over the network.
pub fn active() -> bool {
    crate::config::get().sync && token().is_some()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The merge rule decides.
    Both,
    /// The gist wins every difference.
    Pull,
    /// This machine wins every difference.
    Push,
}

// ── file mapping ─────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Bytes as they are.
    Plain,
    /// config.toml, synced without the `plugins` table.
    Config,
    /// Derived from plugins/.installed.json; a pull installs what's missing.
    Plugins,
    /// binds.json; a pull reinstalls the shortcuts.
    Binds,
}

/// One synced file: its name in the gist and where it lives here.
#[derive(Clone, Debug, PartialEq)]
struct Mapped {
    name: String,
    path: PathBuf,
    kind: Kind,
}

fn binds_name() -> String {
    format!("binds-{}.json", std::env::consts::OS)
}

fn valid_theme(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\']) && !name.contains("..")
}

/// Where a gist file goes under `root` (the zunder config dir). `None` for names this machine
/// doesn't sync: other systems' binds, unknown or unsafe names.
fn local_for(root: &Path, name: &str) -> Option<Mapped> {
    let (path, kind) = match name {
        "config.toml" => (root.join("config.toml"), Kind::Config),
        "favorites.json" => (root.join("favorites.json"), Kind::Plain),
        "plugins.json" => (root.join("plugins").join(".installed.json"), Kind::Plugins),
        n if n == binds_name() => (root.join("binds.json"), Kind::Binds),
        n => {
            let theme = n.strip_prefix(THEME_PREFIX)?.strip_suffix(".css")?;
            if !valid_theme(theme) {
                return None;
            }
            (root.join("themes").join(format!("{theme}.css")), Kind::Plain)
        }
    };
    Some(Mapped { name: name.to_owned(), path, kind })
}

/// Every file under `root` that exists here and is synced.
fn local_files(root: &Path) -> Vec<Mapped> {
    let mut names: Vec<String> = ["config.toml", "favorites.json", "plugins.json"].map(String::from).to_vec();
    names.push(binds_name());
    let mut themes: Vec<String> = fs::read_dir(root.join("themes"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "css"))
        .filter_map(|p| p.file_stem().map(|s| format!("{THEME_PREFIX}{}.css", s.to_string_lossy())))
        .collect();
    themes.sort();
    names.extend(themes);
    names.iter().filter_map(|n| local_for(root, n)).filter(|m| m.path.is_file()).collect()
}

/// The text this file contributes to the gist, `None` when it doesn't exist (or is empty: a
/// gist can't hold empty files).
fn read_local(m: &Mapped) -> Option<String> {
    let bytes = fs::read(&m.path).ok()?;
    let text = match m.kind {
        Kind::Plain | Kind::Binds => String::from_utf8(bytes).ok()?,
        Kind::Config => {
            let text = String::from_utf8(bytes).ok()?;
            match text.parse::<toml::Table>() {
                Ok(mut table) if table.contains_key("plugins") => {
                    table.remove("plugins");
                    toml::to_string_pretty(&table).ok()?
                }
                _ => text,
            }
        }
        Kind::Plugins => {
            let installed: BTreeMap<String, Installed> = serde_json::from_slice(&bytes).ok()?;
            let pins: BTreeMap<String, String> = installed.into_iter().map(|(name, rec)| (name, rec.commit)).collect();
            serde_json::to_string_pretty(&pins).ok()? + "\n"
        }
    };
    (!text.trim().is_empty()).then_some(text)
}

fn mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Write `text` from the gist to `m`, keeping what was there as `<file>.bak`. Plugins aren't
/// written: the caller installs them.
fn write_local(m: &Mapped, text: &str) -> Result<(), String> {
    let text = match m.kind {
        Kind::Plugins => return Ok(()),
        // the plugin values (tokens) never leave this machine: keep them
        Kind::Config => match fs::read_to_string(&m.path).ok().and_then(|t| t.parse::<toml::Table>().ok()).and_then(|mut t| t.remove("plugins")) {
            Some(plugins) => {
                let mut remote = text.parse::<toml::Table>().map_err(|e| format!("config.toml in the gist: {e}"))?;
                remote.insert("plugins".into(), plugins);
                toml::to_string_pretty(&remote).map_err(|e| e.to_string())?
            }
            None => text.to_owned(),
        },
        Kind::Plain | Kind::Binds => text.to_owned(),
    };
    if let Some(dir) = m.path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    }
    if m.path.exists() {
        backup(&m.path, &fs::read(&m.path).unwrap_or_default())?;
    }
    let tmp = m.path.with_extension("sync-part");
    fs::write(&tmp, text).map_err(|e| format!("can't write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, &m.path).map_err(|e| format!("can't write {}: {e}", m.path.display()))
}

fn bak_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

fn backup(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let bak = bak_path(path);
    fs::write(&bak, bytes).map_err(|e| format!("can't write {}: {e}", bak.display()))
}

// ── merge rule ───────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
enum Step {
    Same,
    /// The gist's version replaces the local one (kept as .bak).
    Pull,
    /// The local version goes to the gist; `keep_remote` saves the gist's as .bak first.
    Push { keep_remote: bool },
}

/// One side of a file: the hash of its synced text and when it last changed.
#[derive(Clone, Copy)]
struct Side<'a> {
    hash: &'a str,
    at: SystemTime,
}

fn decide(local: Option<Side>, remote: Option<Side>, base: Option<&str>, mode: Mode) -> Step {
    match (local, remote) {
        (None, None) => Step::Same,
        (Some(_), None) if mode == Mode::Pull => Step::Same,
        (Some(_), None) => Step::Push { keep_remote: false },
        (None, Some(_)) if mode == Mode::Push => Step::Same,
        (None, Some(_)) => Step::Pull,
        (Some(l), Some(r)) if l.hash == r.hash => Step::Same,
        (Some(_), Some(_)) if mode == Mode::Pull => Step::Pull,
        (Some(_), Some(_)) if mode == Mode::Push => Step::Push { keep_remote: true },
        (Some(l), Some(r)) => match base {
            Some(b) if b == l.hash => Step::Pull,
            Some(b) if b == r.hash => Step::Push { keep_remote: false },
            // both changed since the last sync: last write wins
            _ if l.at > r.at => Step::Push { keep_remote: true },
            _ => Step::Pull,
        },
    }
}

// ── state ────────────────────────────────────────────────

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    gist: String,
    /// sha256 of each file's text as both sides had it after the last sync.
    files: BTreeMap<String, String>,
}

fn state_path() -> PathBuf {
    crate::store::data_dir().join(STATE)
}

fn root() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("zunder")
}

// ── gist API ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Gist {
    id: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    files: BTreeMap<String, Option<GistFile>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct GistFile {
    content: Option<String>,
    truncated: bool,
    raw_url: String,
}

/// One GitHub API call with curl. The token goes in through stdin, so it never shows up in `ps`.
fn api(token: &str, method: &str, url: &str, body: Option<&serde_json::Value>) -> Result<(u16, Vec<u8>), String> {
    let mut cmd = Command::new("curl");
    cmd.args(["-sS", "-m", "30", "-X", method, "-K", "-", "-w", "\n%{http_code}", url]);
    let body_file = match body {
        Some(json) => {
            let path = std::env::temp_dir().join(format!("zunder-sync-{}.json", std::process::id()));
            fs::write(&path, json.to_string()).map_err(|e| format!("can't write {}: {e}", path.display()))?;
            cmd.arg("--data-binary").arg(format!("@{}", path.display()));
            Some(path)
        }
        None => None,
    };
    let config = format!(
        "header = \"Authorization: Bearer {token}\"\nheader = \"Accept: application/vnd.github+json\"\nheader = \"X-GitHub-Api-Version: 2022-11-28\"\nuser-agent = \"zunder/{}\"\n",
        env!("CARGO_PKG_VERSION")
    );
    let result = (|| {
        let mut child = crate::platform::no_window(cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()))
            .spawn()
            .map_err(|e| format!("can't run curl: {e}"))?;
        child.stdin.take().ok_or("curl has no stdin")?.write_all(config.as_bytes()).map_err(|e| e.to_string())?;
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!("can't reach GitHub: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        let mut stdout = out.stdout;
        let split = stdout.iter().rposition(|&b| b == b'\n').ok_or("curl gave no status")?;
        let code = String::from_utf8_lossy(&stdout[split + 1..]).trim().parse().map_err(|_| "curl gave no status")?;
        stdout.truncate(split);
        Ok((code, stdout))
    })();
    if let Some(path) = body_file {
        let _ = fs::remove_file(path);
    }
    result
}

fn api_ok(token: &str, method: &str, url: &str, body: Option<&serde_json::Value>) -> Result<Vec<u8>, String> {
    let (code, bytes) = api(token, method, url, body)?;
    match code {
        200..=299 => Ok(bytes),
        401 => Err("GitHub refused the token (401), log in again".into()),
        403 | 404 if url == API || url.starts_with(&format!("{API}?")) => {
            Err(format!("GitHub refused to list gists ({code}): the token needs the `gist` scope"))
        }
        _ => {
            let msg = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(str::to_owned))
                .unwrap_or_default();
            Err(format!("GitHub said {code} for {method} {url}: {msg}"))
        }
    }
}

fn get_gist(token: &str, id: &str) -> Result<Option<Gist>, String> {
    let (code, bytes) = api(token, "GET", &format!("{API}/{id}"), None)?;
    match code {
        200 => serde_json::from_slice(&bytes).map(Some).map_err(|e| format!("can't read the gist: {e}")),
        404 => Ok(None),
        _ => api_ok(token, "GET", &format!("{API}/{id}"), None).map(|_| None),
    }
}

/// The sync gist: the remembered id first, then by description among the user's gists.
fn find_gist(token: &str, known: &str) -> Result<Option<Gist>, String> {
    if !known.is_empty()
        && let Some(gist) = get_gist(token, known)?
        && gist.description.as_deref() == Some(DESCRIPTION)
    {
        return Ok(Some(gist));
    }
    for page in 1..=30 {
        let bytes = api_ok(token, "GET", &format!("{API}?per_page=100&page={page}"), None)?;
        let list: Vec<Gist> = serde_json::from_slice(&bytes).map_err(|e| format!("can't read the gist list: {e}"))?;
        if let Some(found) = list.iter().find(|g| g.description.as_deref() == Some(DESCRIPTION)) {
            return get_gist(token, &found.id);
        }
        if list.len() < 100 {
            break;
        }
    }
    Ok(None)
}

/// The text of every file in the gist; big files come from their raw URL.
fn gist_texts(gist: &Gist) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for (name, file) in &gist.files {
        let Some(file) = file else { continue };
        let text = match &file.content {
            Some(text) if !file.truncated => text.clone(),
            _ => {
                let out = crate::platform::no_window(Command::new("curl").args(["-fsSL", "-m", "30", &file.raw_url]))
                    .output()
                    .map_err(|e| format!("can't run curl: {e}"))?;
                if !out.status.success() {
                    return Err(format!("can't download {name} from the gist"));
                }
                String::from_utf8_lossy(&out.stdout).into_owned()
            }
        };
        out.insert(name.clone(), text);
    }
    Ok(out)
}

fn parse_time(s: &str) -> SystemTime {
    chrono::DateTime::parse_from_rfc3339(s).map(SystemTime::from).unwrap_or(SystemTime::UNIX_EPOCH)
}

// ── sync ─────────────────────────────────────────────────

/// What a sync did.
#[derive(Debug, Default)]
pub struct Outcome {
    pub lines: Vec<String>,
    pub ok: bool,
    /// Local files changed: the server reloads.
    pub pulled: bool,
    /// binds.json changed: the shortcuts are reinstalled.
    pub binds: bool,
}

/// What to do with each file, given the gist's texts.
fn plan(root: &Path, remote: &BTreeMap<String, String>, remote_at: SystemTime, base: &BTreeMap<String, String>, mode: Mode) -> Vec<(Mapped, Option<String>, Step)> {
    let mut files = local_files(root);
    for name in remote.keys() {
        if !files.iter().any(|f| &f.name == name)
            && let Some(m) = local_for(root, name)
        {
            files.push(m);
        }
    }
    files
        .into_iter()
        .map(|m| {
            let mut text = read_local(&m);
            if m.kind == Kind::Plugins
                && let Some(r) = remote.get(&m.name)
            {
                text = union_pins(text.as_deref(), r);
            }
            let local_hash = text.as_deref().map(|t| sha256_hex(t.as_bytes()));
            let remote_hash = remote.get(&m.name).map(|t| sha256_hex(t.as_bytes()));
            let local = local_hash.as_deref().map(|hash| Side { hash, at: mtime(&m.path).unwrap_or(SystemTime::UNIX_EPOCH) });
            let remote = remote_hash.as_deref().map(|hash| Side { hash, at: remote_at });
            let step = decide(local, remote, base.get(&m.name).map(String::as_str), mode);
            (m, text, step)
        })
        .collect()
}

/// The gist's plugins plus this machine's (its commits win): a plugin that can't run here, or
/// that isn't installed here yet, stays in the list for the other machines.
fn union_pins(local: Option<&str>, remote: &str) -> Option<String> {
    let mut pins: BTreeMap<String, String> = serde_json::from_str(remote).unwrap_or_default();
    pins.extend(local.and_then(|l| serde_json::from_str::<BTreeMap<String, String>>(l).ok()).unwrap_or_default());
    (!pins.is_empty()).then(|| serde_json::to_string_pretty(&pins).unwrap_or_default() + "\n")
}

pub fn sync(mode: Mode) -> Result<Outcome, String> {
    if !crate::config::get().sync {
        return Err("sync is off (`sync = false` in config.toml)".into());
    }
    let token = token().ok_or("not logged in to GitHub: run `zunder login`")?;
    let mut state: State = crate::store::read_json(&state_path());
    let gist = find_gist(&token, &state.gist)?;
    let (remote, remote_at) = match &gist {
        Some(g) => (gist_texts(g)?, parse_time(&g.updated_at)),
        None => (BTreeMap::new(), SystemTime::UNIX_EPOCH),
    };
    let root = root();
    let mut out = Outcome { ok: true, ..Outcome::default() };
    let mut push = serde_json::Map::new();
    for (m, text, step) in plan(&root, &remote, remote_at, &state.files, mode) {
        match step {
            Step::Same => {
                if let Some(t) = &text {
                    state.files.insert(m.name.clone(), sha256_hex(t.as_bytes()));
                }
            }
            Step::Pull => {
                let Some(t) = remote.get(&m.name) else { continue };
                if m.kind == Kind::Plugins {
                    // nothing to write: the installs below update .installed.json
                } else if let Err(e) = write_local(&m, t) {
                    out.ok = false;
                    out.lines.push(e);
                    continue;
                } else {
                    out.pulled = true;
                    out.binds |= m.kind == Kind::Binds;
                    out.lines.push(format!("pulled {}", m.name));
                }
                state.files.insert(m.name.clone(), sha256_hex(t.as_bytes()));
            }
            Step::Push { keep_remote } => {
                let Some(t) = text else { continue };
                if keep_remote
                    && m.kind != Kind::Plugins
                    && let Some(old) = remote.get(&m.name)
                {
                    backup(&m.path, old.as_bytes())?;
                }
                push.insert(m.name.clone(), serde_json::json!({ "content": t }));
                state.files.insert(m.name.clone(), sha256_hex(t.as_bytes()));
                out.lines.push(format!("pushed {}", m.name));
            }
        }
    }
    if !push.is_empty() {
        let created = match &gist {
            Some(g) => api_ok(&token, "PATCH", &format!("{API}/{}", g.id), Some(&serde_json::json!({ "files": push })))?,
            None => api_ok(&token, "POST", API, Some(&serde_json::json!({ "description": DESCRIPTION, "public": false, "files": push })))?,
        };
        if gist.is_none() {
            let g: Gist = serde_json::from_slice(&created).map_err(|e| format!("can't read the new gist: {e}"))?;
            out.lines.push(format!("created the private gist {DESCRIPTION} ({})", g.id));
            state.gist = g.id;
        }
    }
    if let Some(g) = &gist {
        state.gist = g.id.clone();
    }
    crate::store::write_json(&state_path(), &state);
    if mode != Mode::Push
        && let Some(pins) = remote.get("plugins.json").and_then(|t| serde_json::from_str::<BTreeMap<String, String>>(t).ok())
    {
        install_missing(&pins, &mut out);
    }
    if out.lines.is_empty() {
        out.lines.push("everything is in sync".into());
    }
    Ok(out)
}

/// Registry plugins the gist lists that aren't installed here, at the index's current commit.
fn install_missing(pins: &BTreeMap<String, String>, out: &mut Outcome) {
    let installed: BTreeMap<String, Installed> = crate::store::read_json(&crate::providers::plugins::dir().join(".installed.json"));
    let mut missing: Vec<String> = pins.keys().filter(|n| !installed.contains_key(*n)).cloned().collect();
    if missing.is_empty() {
        return;
    }
    let registry = Registry::from_env();
    match registry.catalog() {
        // only what this system can run and nobody's own file has taken the name of
        Ok(cat) => missing.retain(|n| cat.entry(n).is_some_and(|e| e.runs_here()) && !cat.taken.contains(n)),
        Err(e) => {
            out.lines.push(format!("can't install plugins from the gist: {e}"));
            return;
        }
    }
    if missing.is_empty() {
        return;
    }
    match registry.install(&missing) {
        Ok(report) => {
            out.pulled = true;
            for line in report.lines {
                match line {
                    Line::Out(l) | Line::Err(l) => out.lines.push(l),
                }
            }
        }
        Err(e) => out.lines.push(format!("can't install plugins from the gist: {e}")),
    }
}

/// Tell a running server what a sync changed; with none running, install the binds here.
fn announce(out: &Outcome, server: bool) {
    if out.pulled {
        let _ = crate::ipc::send("reload");
    }
    if out.binds
        && crate::ipc::send("reload-binds").is_err()
        && !server
        && cfg!(target_os = "linux")
        // headless, or no Hyprland here: the binds wait in binds.json
        && dirs::config_dir().is_some_and(|d| d.join("hypr").is_dir())
        && let Err(e) = crate::platform::install_binds(&crate::store::Binds::load().list)
    {
        eprintln!("zunder: can't install the synced keybinds: {e}");
    }
}

/// `zunder sync [--pull|--push]`. Works without a display: nothing here touches GTK.
pub fn run(args: &[String]) -> Result<bool, String> {
    let mut mode = Mode::Both;
    for arg in args {
        mode = match arg.as_str() {
            "--pull" => Mode::Pull,
            "--push" => Mode::Push,
            "--no-ui" | "--headless" => mode,
            _ => return Err(USAGE.to_owned()),
        };
    }
    let out = sync(mode)?;
    for line in &out.lines {
        println!("{line}");
    }
    announce(&out, false);
    Ok(out.ok)
}

// ── server ───────────────────────────────────────────────

const EVERY: Duration = Duration::from_secs(3600);
const SETTLE: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_secs(2);

type Stamps = Vec<(PathBuf, Option<SystemTime>, u64)>;

fn stamps(root: &Path) -> Stamps {
    local_files(root)
        .into_iter()
        .map(|m| {
            let meta = fs::metadata(&m.path).ok();
            (m.path, meta.as_ref().and_then(|m| m.modified().ok()), meta.map(|m| m.len()).unwrap_or(0))
        })
        .collect()
}

/// For `zunder server`: pull now and every hour, push once the files have been still for ~5 s.
pub fn spawn() {
    std::thread::spawn(|| {
        let root = root();
        let mut seen = stamps(&root);
        let mut changed: Option<Instant> = None;
        let mut last: Option<Instant> = None;
        loop {
            let now = stamps(&root);
            if now != seen {
                seen = now;
                changed = Some(Instant::now());
            }
            let due = last.is_none_or(|t| t.elapsed() >= EVERY) || changed.is_some_and(|t| t.elapsed() >= SETTLE);
            if due && active() {
                match sync(Mode::Both) {
                    Ok(out) => {
                        if out.pulled || out.lines.iter().any(|l| l.starts_with("pushed")) {
                            eprintln!("zunder: sync: {}", out.lines.join(", "));
                        }
                        announce(&out, true);
                    }
                    Err(e) => eprintln!("zunder: sync: {e}"),
                }
                seen = stamps(&root);
                changed = None;
                last = Some(Instant::now());
            }
            std::thread::sleep(POLL);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zunder-sync-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn side(hash: &str, secs: u64) -> Option<Side<'_>> {
        Some(Side { hash, at: at(secs) })
    }

    #[test]
    fn unchanged_side_loses() {
        // only the gist changed since the last sync
        assert_eq!(decide(side("a", 900), side("b", 100), Some("a"), Mode::Both), Step::Pull);
        // only this machine changed
        assert_eq!(decide(side("b", 100), side("a", 900), Some("a"), Mode::Both), Step::Push { keep_remote: false });
        assert_eq!(decide(side("a", 1), side("a", 2), Some("x"), Mode::Both), Step::Same);
    }

    #[test]
    fn both_changed_last_write_wins() {
        assert_eq!(decide(side("l", 200), side("r", 100), Some("base"), Mode::Both), Step::Push { keep_remote: true });
        assert_eq!(decide(side("l", 100), side("r", 200), Some("base"), Mode::Both), Step::Pull);
        // first sync, no base: same rule
        assert_eq!(decide(side("l", 200), side("r", 100), None, Mode::Both), Step::Push { keep_remote: true });
        assert_eq!(decide(side("l", 100), side("r", 100), None, Mode::Both), Step::Pull);
    }

    #[test]
    fn one_sided_files_and_forced_modes() {
        assert_eq!(decide(side("l", 1), None, None, Mode::Both), Step::Push { keep_remote: false });
        assert_eq!(decide(None, side("r", 1), Some("r"), Mode::Both), Step::Pull);
        assert_eq!(decide(side("l", 1), None, None, Mode::Pull), Step::Same);
        assert_eq!(decide(None, side("r", 1), None, Mode::Push), Step::Same);
        assert_eq!(decide(side("l", 900), side("r", 1), Some("r"), Mode::Pull), Step::Pull);
        assert_eq!(decide(side("l", 1), side("r", 900), Some("l"), Mode::Push), Step::Push { keep_remote: true });
    }

    #[test]
    fn maps_gist_names() {
        let root = Path::new("/c/zunder");
        let m = local_for(root, "theme--nord-ish.css").unwrap();
        assert_eq!(m.path, root.join("themes/nord-ish.css"));
        assert_eq!(local_for(root, "config.toml").unwrap().kind, Kind::Config);
        assert_eq!(local_for(root, "plugins.json").unwrap().path, root.join("plugins/.installed.json"));
        assert_eq!(local_for(root, &binds_name()).unwrap().path, root.join("binds.json"));
        let other = if cfg!(target_os = "linux") { "binds-windows.json" } else { "binds-linux.json" };
        assert!(local_for(root, other).is_none());
        for bad in ["theme--.css", "theme--../x.css", "theme--a/b.css", "theme--.hidden.css", "theme--x.txt", "readme.md"] {
            assert!(local_for(root, bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn lists_local_files() {
        let root = tmp("list");
        fs::create_dir_all(root.join("themes")).unwrap();
        fs::write(root.join("favorites.json"), "[]").unwrap();
        fs::write(root.join("themes/mine.css"), "@define-color bg #000;\n").unwrap();
        fs::write(root.join("themes/notes.txt"), "x").unwrap();
        fs::write(root.join("themes/mine.css.bak"), "x").unwrap();
        let names: Vec<String> = local_files(&root).into_iter().map(|m| m.name).collect();
        assert_eq!(names, ["favorites.json", "theme--mine.css"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn config_keeps_plugin_values_local() {
        let root = tmp("config");
        let m = local_for(&root, "config.toml").unwrap();
        fs::write(&m.path, "width = 900\n\n[plugins.github]\ntoken = \"secret\"\n").unwrap();
        let text = read_local(&m).unwrap();
        assert!(!text.contains("secret") && text.contains("width = 900"));
        write_local(&m, "width = 1000\n").unwrap();
        let now = fs::read_to_string(&m.path).unwrap();
        assert!(now.contains("width = 1000") && now.contains("secret"));
        assert!(fs::read_to_string(bak_path(&m.path)).unwrap().contains("width = 900"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plugins_are_names_and_commits() {
        let root = tmp("plugins");
        fs::create_dir_all(root.join("plugins")).unwrap();
        fs::write(root.join("plugins/.installed.json"), r#"{"wifi":{"commit":"abc123","sha256":"ff"}}"#).unwrap();
        let text = read_local(&local_for(&root, "plugins.json").unwrap()).unwrap();
        let pins: BTreeMap<String, String> = serde_json::from_str(&text).unwrap();
        assert_eq!(pins.get("wifi").map(String::as_str), Some("abc123"));
        assert!(!text.contains("sha256"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plugin_lists_are_merged() {
        let merged = union_pins(Some(r#"{"wifi":"new"}"#), r#"{"wifi":"old","mac-only":"c1"}"#).unwrap();
        let pins: BTreeMap<String, String> = serde_json::from_str(&merged).unwrap();
        assert_eq!(pins.get("wifi").map(String::as_str), Some("new"));
        assert_eq!(pins.get("mac-only").map(String::as_str), Some("c1"));
        assert!(union_pins(None, "{}").is_none());
    }

    #[test]
    fn plan_pulls_new_and_backs_up() {
        let root = tmp("plan");
        fs::write(root.join("favorites.json"), "[1]").unwrap();
        let remote: BTreeMap<String, String> =
            [("favorites.json", "[2]"), ("theme--sea.css", "@define-color bg #013;\n"), ("binds-plan9.json", "[]")].map(|(k, v)| (k.to_owned(), v.to_owned())).into();
        let base: BTreeMap<String, String> = [("favorites.json".to_owned(), sha256_hex(b"[1]"))].into();
        let steps: Vec<(String, Step)> = plan(&root, &remote, at(1), &base, Mode::Both).into_iter().map(|(m, _, s)| (m.name, s)).collect();
        assert_eq!(steps, [("favorites.json".to_owned(), Step::Pull), ("theme--sea.css".to_owned(), Step::Pull)]);
        let fav = local_for(&root, "favorites.json").unwrap();
        write_local(&fav, "[2]").unwrap();
        assert_eq!(fs::read_to_string(&fav.path).unwrap(), "[2]");
        assert_eq!(fs::read_to_string(bak_path(&fav.path)).unwrap(), "[1]");
        fs::remove_dir_all(root).unwrap();
    }
}
