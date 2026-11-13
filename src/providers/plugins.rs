//! Script plugins: any executable in ~/.config/zunder/plugins, in any language.
//!
//! Header comments (all optional) describe the plugin:
//!
//!     # @zunder.name   Kill Process
//!     # @zunder.prefix kill            typing `kill <query>` runs the script
//!     # @zunder.glyph  \uf120          a Nerd Font glyph, literal or \uXXXX
//!     # @zunder.mode   search          `search` (default) or `command`
//!     # @zunder.setting token "GitHub token"   a value set in Settings → Plugins, passed
//!                                              to the script as $ZUNDER_TOKEN
//!
//! search:  called as `script <query>`; each stdout line is a result, either plain text
//!          (Enter copies it) or JSON: {"title", "subtitle", "hint", "glyph", "action": {"open": "…"}}
//!          where action is one of run, term, open, copy, focus, fill, and an optional
//!          "actions": [{"title": "Copy id", "copy": "42"}] adds entries to the Ctrl+K menu.
//! command: shows up in the main search by name; Enter runs it once.

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::engine::{Ctx, Job, Provider, Sink};
use crate::item::{Action, Item, shell_quote};

#[derive(Clone, Debug)]
pub struct Plugin {
    pub name: String,
    pub prefix: String,
    pub glyph: String,
    pub command: bool,
    pub path: PathBuf,
    /// `@zunder.setting` lines, in header order.
    pub settings: Vec<PluginSetting>,
}

/// A value the user fills in for a plugin, stored in `[plugins.<file stem>]` in config.toml.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginSetting {
    pub key: String,
    pub help: String,
}

impl PluginSetting {
    /// Keys that hold secrets get a password entry.
    pub fn secret(&self) -> bool {
        matches!(self.key.as_str(), "token" | "password") || self.key.ends_with("_key")
    }
}

impl Plugin {
    /// Where its values live in config.toml: the file name without extension.
    pub fn stem(&self) -> String {
        stem(&self.path)
    }
}

fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `greeting` → `ZUNDER_GREETING`, `api-url` → `ZUNDER_API_URL`.
pub fn env_name(key: &str) -> String {
    format!("ZUNDER_{}", key.to_uppercase().replace('-', "_"))
}

/// The env vars a plugin gets: every value stored for it in config.toml.
pub fn env(path: &Path) -> Vec<(String, String)> {
    crate::config::get().plugin_values(&stem(path)).iter().map(|(k, v)| (env_name(k), v.clone())).collect()
}

/// For a command-mode run (`'/…/plugins/x'`, also from favorites and shortcuts): the plugin's
/// env vars. Resolved when it runs, so values never end up in favorites.json or binds.
pub fn command_env(cmd: &str) -> Vec<(String, String)> {
    let Some(path) = cmd.strip_prefix('\'').and_then(|c| c.strip_suffix('\'')) else { return Vec::new() };
    let path = PathBuf::from(path.replace(r"'\''", "'"));
    if path.parent() != Some(dir().as_path()) {
        return Vec::new();
    }
    env(&path)
}

/// `token "GitHub token"` → key and help; quotes around the help are optional.
fn parse_setting(value: &str) -> Option<PluginSetting> {
    let (key, help) = value.split_once(char::is_whitespace).unwrap_or((value, ""));
    if key.is_empty() {
        return None;
    }
    let help = help.trim();
    let help = help.strip_prefix('"').and_then(|h| h.strip_suffix('"')).unwrap_or(help);
    Some(PluginSetting { key: key.to_owned(), help: help.to_owned() })
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

    fn deferred(&self, cx: &Ctx) -> Option<(String, Job)> {
        let call = self.call(cx.query)?;
        Some((call.plugin.name.clone(), Box::new(move |sink: &Sink| call.run(sink))))
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        if self.call(cx.query).is_some() {
            return Vec::new(); // results come from `deferred`
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
    icon: String,
    /// Image file shown in the preview pane.
    image: Option<String>,
    /// Text shown in the preview pane.
    preview: Option<String>,
    #[serde(default)]
    action: Action,
    /// Extra Ctrl+K entries: `[{"title": "Copy id", "copy": "42"}]`.
    #[serde(default)]
    actions: Vec<crate::item::MenuAction>,
}

/// A batch goes out after this many lines or this long after its first line, whichever is first.
const BATCH_LINES: usize = 20;
const BATCH_WAIT: Duration = Duration::from_millis(50);

impl Call {
    /// Run the script (blocking; call from a worker thread). Lines go to `sink` in batches as
    /// they're printed; the script is killed as soon as the query changes.
    pub fn run(&self, sink: &Sink) -> Vec<Item> {
        self.run_with(sink, env(&self.plugin.path))
    }

    fn run_with(&self, sink: &Sink, env: Vec<(String, String)>) -> Vec<Item> {
        let mut cmd = Command::new(&self.plugin.path);
        cmd.arg(&self.arg).envs(env).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0); // kill takes its children too
        let mut child = match spawn(&mut cmd) {
            Ok(child) => child,
            Err(e) => return vec![error_item(&format!("failed to run: {e}"))],
        };
        let (Some(stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return Vec::new();
        };
        // stderr is drained on its own thread so a chatty script can't fill the pipe and stall
        let errors = std::thread::spawn(move || {
            let mut err = Vec::new();
            let _ = stderr.read_to_end(&mut err);
            String::from_utf8_lossy(&err).into_owned()
        });
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            let mut out = BufReader::new(stdout);
            let mut buf = Vec::new();
            while out.read_until(b'\n', &mut buf).is_ok_and(|n| n > 0) {
                let line = String::from_utf8_lossy(&buf).trim_end_matches(['\n', '\r']).to_owned();
                buf.clear();
                if !line.trim().is_empty() && tx.send(line).is_err() {
                    break;
                }
            }
        });

        let (mut batch, mut count, mut since) = (Vec::new(), 0, None::<Instant>);
        loop {
            if sink.stale() {
                kill(&mut child);
                return Vec::new();
            }
            let wait = since.map_or(BATCH_WAIT, |t| BATCH_WAIT.saturating_sub(t.elapsed()));
            match lines.recv_timeout(wait) {
                Ok(line) => {
                    batch.push(self.item(&line, count));
                    count += 1;
                    since.get_or_insert_with(Instant::now);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if batch.len() >= BATCH_LINES || since.is_some_and(|t| t.elapsed() >= BATCH_WAIT) {
                sink.send(std::mem::take(&mut batch));
                since = None;
            }
        }
        let failed = !child.wait().is_ok_and(|s| s.success());
        if failed && count == 0 {
            let err = errors.join().unwrap_or_default();
            return vec![error_item(err.lines().last().unwrap_or("plugin failed"))];
        }
        batch
    }

    /// One stdout line as a result: JSON or plain text. `i` is its position in the output.
    fn item(&self, line: &str, i: usize) -> Item {
        let score = -(i as i64);
        match serde_json::from_str::<Line>(line) {
            Ok(l) => Item {
                id: format!("plugin:{}:{}", self.plugin.name, l.title),
                title: l.title,
                subtitle: l.subtitle,
                hint: l.hint,
                glyph: l.glyph.unwrap_or_else(|| self.plugin.glyph.clone()),
                icon: l.icon,
                image: l.image,
                text: l.preview.map(crate::item::Text::Literal),
                action: l.action,
                actions: l.actions,
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
    }
}

/// Stop a stale run: the whole process group on unix, so `find` or `curl` started by the
/// script go too.
/// Spawn, retrying briefly on "text file busy": a script that was just written (by
/// `zunder plugin install`, an editor, a test) can't be exec'd while a forked child elsewhere
/// still holds its write handle.
fn spawn(cmd: &mut Command) -> std::io::Result<Child> {
    const ETXTBSY: i32 = 26; // same on Linux and macOS
    let mut tries = 0;
    loop {
        match crate::platform::no_window(cmd).spawn() {
            Err(e) if cfg!(unix) && e.raw_os_error() == Some(ETXTBSY) && tries < 5 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(10));
            }
            r => return r,
        }
    }
}

fn kill(child: &mut Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill").args(["-TERM", "--", &format!("-{}", child.id())]).status();
    }
    let _ = child.kill();
    let _ = child.wait();
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

/// Every open rescans the folder; a plugin's header is only read again when its file changed.
fn scan() -> Vec<Plugin> {
    use std::collections::HashMap;
    use std::sync::Mutex;
    type Stamp = (Option<std::time::SystemTime>, u64);
    static SEEN: Mutex<Option<HashMap<PathBuf, (Stamp, Plugin)>>> = Mutex::new(None);

    let Ok(entries) = fs::read_dir(dir()) else { return Vec::new() };
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let old = seen.take().unwrap_or_default();
    let mut fresh = HashMap::new();
    let mut list: Vec<Plugin> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let meta = fs::metadata(&path).ok()?;
            if !meta.is_file() || !crate::platform::is_executable(&meta, &path) {
                return None;
            }
            let stamp = (meta.modified().ok(), meta.len());
            let plugin = match old.get(&path) {
                Some((s, p)) if *s == stamp => p.clone(),
                _ => read_plugin(path.clone())?,
            };
            fresh.insert(path, (stamp, plugin.clone()));
            Some(plugin)
        })
        .collect();
    *seen = Some(fresh);
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

/// A plugin with what its header comments (the first 4 KiB) say about it.
fn read_plugin(path: PathBuf) -> Option<Plugin> {
    use std::io::Read;
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let mut head = Vec::new();
    fs::File::open(&path).ok()?.take(4096).read_to_end(&mut head).ok()?;
    let mut p = Plugin { name: stem.clone(), prefix: stem, glyph: "\u{f12e}".into(), command: false, path, settings: Vec::new() };
    for line in String::from_utf8_lossy(&head).lines().take(30) {
        let Some((_, rest)) = line.split_once("@zunder.") else { continue };
        let (key, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let value = value.trim().to_owned();
        match key {
            "name" => p.name = value,
            "prefix" => p.prefix = value,
            "glyph" => p.glyph = unescape(&value),
            "mode" => p.command = value == "command",
            "setting" => p.settings.extend(parse_setting(&value)),
            _ => {}
        }
    }
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn call(name: &str, script: &str) -> Call {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("zunder-plugin-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        let plugin = Plugin { name: "t".into(), prefix: "t".into(), glyph: String::new(), command: false, path, settings: Vec::new() };
        Call { plugin, arg: "q".into() }
    }

    #[cfg(unix)]
    #[test]
    fn streams_lines_as_they_come() {
        let call = call("stream", "#!/bin/sh\nfor i in 1 2 3; do echo \"$1 $i\"; sleep 0.3; done\n");
        let (tx, rx) = mpsc::channel();
        let start = Instant::now();
        let sink = Sink::new(move |items: Vec<Item>| tx.send((start.elapsed(), items)).unwrap(), || false);
        let rest = call.run(&sink);
        let batches: Vec<_> = rx.try_iter().collect();
        assert!(batches[0].0 < Duration::from_millis(250), "first line took {:?}", batches[0].0);
        let titles: Vec<String> = batches.into_iter().flat_map(|(_, b)| b).chain(rest).map(|i| i.title).collect();
        assert_eq!(titles, ["q 1", "q 2", "q 3"]);
    }

    #[cfg(unix)]
    #[test]
    fn stale_run_is_killed() {
        let call = call("stale", "#!/bin/sh\necho first\nsleep 5\necho never\n");
        let start = Instant::now();
        let sink = Sink::new(|_| {}, move || start.elapsed() > Duration::from_millis(200));
        assert!(call.run(&sink).is_empty());
        assert!(start.elapsed() < Duration::from_secs(2), "took {:?}", start.elapsed());
    }

    #[cfg(unix)]
    #[test]
    fn failure_shows_stderr() {
        let call = call("fail", "#!/bin/sh\necho nope >&2\nexit 1\n");
        let items = call.run(&Sink::new(|_| {}, || false));
        assert_eq!(items[0].title, "nope");
    }

    #[test]
    fn parses_settings() {
        let s = parse_setting(r#"token "GitHub token with repo scope""#).unwrap();
        assert_eq!(s, PluginSetting { key: "token".into(), help: "GitHub token with repo scope".into() });
        assert!(s.secret());
        let s = parse_setting("limit   How many (default 100)").unwrap();
        assert_eq!((s.key.as_str(), s.help.as_str()), ("limit", "How many (default 100)"));
        assert!(!s.secret());
        assert_eq!(parse_setting("greeting").unwrap().help, "");
        assert!(parse_setting("api_key \"x\"").unwrap().secret());
        assert!(parse_setting("").is_none());
    }

    #[test]
    fn env_names() {
        assert_eq!(env_name("greeting"), "ZUNDER_GREETING");
        assert_eq!(env_name("api-url"), "ZUNDER_API_URL");
        assert_eq!(env_name("Api_Key"), "ZUNDER_API_KEY");
    }

    /// The done-when of #1: a plugin with `@zunder.setting greeting` prints the stored value.
    #[cfg(unix)]
    #[test]
    fn script_gets_settings_as_env() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("zunder-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hello");
        std::fs::write(&path, "#!/bin/sh\n# @zunder.setting greeting \"What to say\"\necho \"$ZUNDER_GREETING $1\"\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let plugin = super::Plugin {
            name: "hello".into(),
            prefix: "hello".into(),
            glyph: String::new(),
            command: false,
            path: path.clone(),
            settings: Vec::new(),
        };
        let mut values = std::collections::BTreeMap::new();
        values.insert("greeting".to_owned(), "hoi".to_owned());
        let env = values.iter().map(|(k, v)| (env_name(k), v.clone())).collect();
        let (tx, rx) = mpsc::channel();
        let sink = Sink::new(move |items: Vec<Item>| tx.send(items).unwrap(), || false);
        let rest = super::Call { plugin, arg: "world".into() }.run_with(&sink, env);
        let items: Vec<Item> = rx.try_iter().flatten().chain(rest).collect();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(items[0].title, "hoi world");
    }

    #[test]
    fn command_env_only_for_plugins() {
        assert!(super::command_env("'/usr/bin/true'").is_empty());
        assert!(super::command_env("echo hi").is_empty());
    }

    #[test]
    fn unescapes_glyphs() {
        assert_eq!(unescape("\\uf05e"), "\u{f05e}");
        assert_eq!(unescape("x"), "x");
        assert_eq!(unescape("\\uzz"), "\\uzz");
    }
}
