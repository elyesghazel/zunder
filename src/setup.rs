//! `zunder setup`: wire zunder into Hyprland with a few questions.
//!
//! Writes ~/.config/hypr/zunder-setup.lua (or .conf), adds one line to the main config to load it,
//! and can switch you over from another launcher. Everything it changes in your own files is
//! shown first and backed up as `<file>.bak`. Run it again any time; it only rewrites its own file.
//!
//!   zunder setup             ask, with defaults on Enter
//!   zunder setup --yes       take every default
//!   zunder setup --dry-run   show what would change, write nothing

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;

use crate::{config, import, theme};

/// Launchers zunder can replace; matched against exec and bind lines.
const OTHERS: &[&str] = &["vicinae", "rofi", "wofi", "fuzzel", "walker", "anyrun", "ulauncher", "tofi"];

const KILL_PLUGIN: &str = include_str!("../examples/plugins/kill");
const IP_PLUGIN: &str = include_str!("../examples/plugins/ip");

struct Opts {
    yes: bool,
    dry: bool,
}

/// The color helpers print plain text when stdout isn't a terminal.
fn paint(hex: &str, text: &str) -> String {
    let rgb = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0xb4befe);
    format!("\x1b[38;2;{};{};{}m{text}\x1b[0m", rgb >> 16, (rgb >> 8) & 0xff, rgb & 0xff)
}

fn say(text: &str) {
    println!("{} {text}", paint("#b4befe", "❯"));
}

fn heading(text: &str) {
    println!("\n{} {}", paint("#b4befe", text), paint("#34343f", &"─".repeat(48usize.saturating_sub(text.len()))));
}

fn ask(opts: &Opts, question: &str, default: &str) -> String {
    if opts.yes {
        println!("  {question} {}", paint("#5a5a6a", default));
        return default.to_owned();
    }
    print!("  {question} {} ", paint("#5a5a6a", &format!("[{default}]")));
    let _ = io::stdout().flush();
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).unwrap_or(0) == 0 {
        println!();
        return default.to_owned();
    }
    let line = line.trim();
    if line.is_empty() { default.to_owned() } else { line.to_owned() }
}

fn yes_no(opts: &Opts, question: &str, default: bool) -> bool {
    let answer = ask(opts, question, if default { "Y/n" } else { "y/N" });
    match answer.to_lowercase().as_str() {
        "y" | "yes" | "j" | "ja" => true,
        "n" | "no" | "nein" => false,
        _ => default,
    }
}

fn has(program: &str) -> bool {
    Command::new("sh").args(["-c", &format!("command -v {program}")]).output().is_ok_and(|o| o.status.success())
}

fn running(pattern: &str) -> bool {
    Command::new("pgrep").args(["-f", pattern]).output().is_ok_and(|o| o.status.success())
}

/// All config files of the Hyprland setup: the main file plus everything in its folder tree.
fn config_files(hypr: &Path, lua: bool) -> Vec<PathBuf> {
    let ext = if lua { "lua" } else { "conf" };
    let mut out = Vec::new();
    let mut stack = vec![hypr.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == ext) && !name.starts_with("zunder") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Lines that start or bind another launcher (comments skipped).
fn other_launcher_lines(files: &[PathBuf], lua: bool) -> Vec<(PathBuf, usize, String, &'static str)> {
    let comment = if lua { "--" } else { "#" };
    let mut found = Vec::new();
    for file in files {
        let Ok(text) = fs::read_to_string(file) else { continue };
        for (i, line) in text.lines().enumerate() {
            // the code part only: a trailing comment mentioning a launcher doesn't count,
            // and lines setup already changed are left alone
            let t = line.split(comment).next().unwrap_or("").trim();
            if t.is_empty() || line.contains("zunder setup") {
                continue;
            }
            let lower = t.to_lowercase();
            let runs = lower.contains("exec") || lower.contains("bind") || lower.contains("local ");
            if let Some(name) = OTHERS.iter().find(|n| runs && lower.contains(*n)) {
                found.push((file.clone(), i, line.to_owned(), *name));
            }
        }
    }
    found
}

/// How a line of another launcher gets switched over.
enum Fix {
    /// It starts the other launcher's daemon: comment it out.
    Comment,
    /// It opens the other launcher (a bind, or a variable a bind uses): point it at zunder.
    Replace(String),
}

fn fix_for(line: &str, name: &str, lua: bool) -> Fix {
    let lower = line.to_lowercase();
    let starts_daemon = lower.contains("exec-once") || lower.contains("server") || lower.contains("daemon") || lower.contains("--gapplication-service");
    let opens = lower.contains("bind") || lower.trim_start().starts_with("local ") || lower.contains("$menu") || lower.contains("toggle") || lower.contains("show");
    if starts_daemon && !opens {
        return Fix::Comment;
    }
    let comment = if lua { "--" } else { "#" };
    if lua {
        // the quoted string that mentions the launcher becomes "zunder toggle"
        let quoted = Regex::new(&format!(r#""[^"]*{}[^"]*""#, regex::escape(name))).unwrap();
        if quoted.is_match(line) {
            let new = quoted.replace(line, "\"zunder toggle\"");
            return Fix::Replace(format!("{new}  {comment} was {name}, changed by zunder setup"));
        }
    } else if let Some(i) = lower.rfind("exec,") {
        return Fix::Replace(format!("{} zunder toggle  {comment} was {name}, changed by zunder setup", &line[..i + 5]));
    } else if let Some((left, _)) = line.split_once('=').filter(|_| lower.trim_start().starts_with('$')) {
        return Fix::Replace(format!("{}= zunder toggle  {comment} was {name}, changed by zunder setup", left));
    }
    Fix::Comment
}

/// zunder lines the user already wrote by hand (outside our generated files).
fn existing_zunder(files: &[PathBuf], lua: bool) -> Vec<String> {
    let comment = if lua { "--" } else { "#" };
    files
        .iter()
        .filter_map(|f| fs::read_to_string(f).ok())
        .flat_map(|t| t.lines().map(str::to_owned).collect::<Vec<_>>())
        .filter(|l| {
            let code = l.split(comment).next().unwrap_or("");
            // a command that runs zunder, not e.g. the window rule's title "zunder settings"
            let runs = code.contains("\"zunder ") || code.contains("exec, zunder ") || code.contains("= zunder ");
            runs && !l.contains("zunder setup") && !code.contains("require") && !code.contains("source")
        })
        .collect()
}

/// Bound combos as Hyprland reports them, e.g. "8 SPACE" (modmask + key).
fn taken_binds() -> Vec<(u64, String)> {
    let Ok(out) = Command::new("hyprctl").args(["binds", "-j"]).output() else { return Vec::new() };
    let binds: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap_or_default();
    binds
        .iter()
        .filter_map(|b| Some((b.get("modmask")?.as_u64()?, b.get("key")?.as_str()?.to_uppercase())))
        .collect()
}

/// "ALT + SPACE" → (8, "SPACE")
fn modmask(combo: &str) -> (u64, String) {
    let parts: Vec<&str> = combo.split('+').map(str::trim).collect();
    let (key, mods) = parts.split_last().unwrap_or((&"", &[]));
    let mask = mods.iter().map(|m| match m.to_uppercase().as_str() {
        "SHIFT" => 1,
        "CTRL" | "CONTROL" => 4,
        "ALT" => 8,
        "SUPER" | "MOD4" | "WIN" => 64,
        _ => 0,
    });
    (mask.sum(), key.to_uppercase())
}

/// What `zunder setup` decides, by questions in the terminal or toggles on the `setup` page.
#[derive(Clone, Debug)]
pub struct Choice {
    /// Start the daemon with Hyprland (off when the config already does).
    pub autostart: bool,
    /// Float the settings window (off when the config already has a rule for it).
    pub window_rule: bool,
    pub switcher: bool,
    /// SUPER+TAB: every window, grouped by workspace.
    pub overview: bool,
    pub launcher: Option<String>,
    pub clipboard_key: bool,
    pub clipboard_watch: bool,
    /// Point the other launchers' lines at zunder.
    pub switch_over: bool,
    pub import_vicinae: bool,
    pub examples: bool,
}

/// What's there before setup changes anything.
pub struct Plan {
    pub lua: bool,
    hypr: PathBuf,
    main: PathBuf,
    files: Vec<PathBuf>,
    /// Lines that start or open another launcher: file, line index, the line, which launcher.
    pub others: Vec<(PathBuf, usize, String, &'static str)>,
    pub vicinae: bool,
    /// zunder lines the user wrote by hand.
    pub by_hand: Vec<String>,
    /// The old launcher's key opens zunder already.
    pub reused_key: bool,
    taken: Vec<(u64, String)>,
    pub cliphist: bool,
    pub cliphist_running: bool,
    pub examples_present: bool,
}

pub const LAUNCHER_KEYS: &[&str] = &["ALT + SPACE", "SUPER + SPACE"];

impl Plan {
    /// Look at the Hyprland config. `None` when there is none (not Hyprland).
    pub fn detect() -> Option<Plan> {
        let hypr = dirs::config_dir().unwrap_or_default().join("hypr");
        let lua = hypr.join("hyprland.lua").exists();
        let main = hypr.join(if lua { "hyprland.lua" } else { "hyprland.conf" });
        if !main.exists() {
            return None;
        }
        let files = config_files(&hypr, lua);
        let others = other_launcher_lines(&files, lua);
        let reused_key = files.iter().filter_map(|f| fs::read_to_string(f).ok()).any(|t| t.contains("changed by zunder setup"));
        Some(Plan {
            lua,
            by_hand: existing_zunder(&files, lua),
            vicinae: dirs::config_dir().unwrap_or_default().join("vicinae/settings.json").exists(),
            taken: taken_binds(),
            cliphist: has("cliphist"),
            cliphist_running: running("cliphist store"),
            examples_present: crate::providers::plugins::dir().join("kill").exists(),
            others,
            reused_key,
            hypr,
            main,
            files,
        })
    }

    /// Already bound to something in Hyprland.
    pub fn taken(&self, combo: &str) -> bool {
        self.taken.contains(&modmask(combo))
    }

    /// Switching over turns the key that opened the old launcher into zunder's.
    pub fn takes_over_key(&self) -> bool {
        self.others.iter().any(|(_, _, line, name)| matches!(fix_for(line, name, self.lua), Fix::Replace(_)))
    }

    /// The defaults: every key that's free, nothing that doubles what's there.
    pub fn defaults(&self) -> Choice {
        let binds = self.by_hand.is_empty();
        let free = |combo: &str| binds && !self.taken(combo);
        let switch_over = !self.others.is_empty();
        let reused = self.reused_key || (switch_over && self.takes_over_key());
        Choice {
            autostart: !self.by_hand.iter().any(|l| l.contains("zunder server")),
            window_rule: !self.files.iter().filter_map(|f| fs::read_to_string(f).ok()).any(|t| t.contains("zunder settings")),
            switcher: free("ALT + TAB"),
            overview: free("SUPER + TAB"),
            launcher: if reused { None } else { LAUNCHER_KEYS.iter().find(|k| free(k)).map(|k| k.to_string()) },
            clipboard_key: self.cliphist && free("SUPER + V"),
            clipboard_watch: self.cliphist && !self.cliphist_running,
            switch_over,
            import_vicinae: self.vicinae,
            examples: !self.examples_present,
        }
    }
}

fn generated(lua: bool, c: &Choice) -> String {
    let mut binds: Vec<(String, String)> = Vec::new();
    if c.switcher {
        binds.push(("ALT + TAB".into(), "zunder switch".into()));
        binds.push(("ALT + SHIFT + TAB".into(), "zunder switch-prev".into()));
    }
    if c.overview {
        binds.push(("SUPER + TAB".into(), "zunder overview".into()));
    }
    if let Some(key) = &c.launcher {
        binds.push((key.clone(), "zunder toggle".into()));
    }
    if c.clipboard_key {
        binds.push(("SUPER + V".into(), "zunder show 'clip '".into()));
    }
    let mut start: Vec<String> = if c.autostart { vec!["zunder server".to_owned()] } else { Vec::new() };
    if c.clipboard_watch {
        start.push("wl-paste --type text --watch cliphist store".into());
        start.push("wl-paste --type image --watch cliphist store".into());
    }
    if lua {
        let mut s = String::from("-- written by `zunder setup`; run it again to change this, or edit it freely\n\n");
        if !start.is_empty() {
            s += "hl.on(\"hyprland.start\", function()\n";
            for cmd in &start {
                s += &format!("    hl.exec_cmd(\"{cmd}\")\n");
            }
            s += "end)\n\n";
        }
        for (key, cmd) in &binds {
            s += &format!("hl.bind(\"{key}\", hl.dsp.exec_cmd(\"{cmd}\"))\n");
        }
        if c.window_rule {
            s += "\nhl.window_rule({\n    name   = \"zunder-settings\",\n    match  = { title = \"^zunder settings$\" },\n    float  = true,\n    size   = \"980 660\",\n    center = true,\n})\n";
        }
        // a launcher should fade in, not slide in like other layers
        s += "\nhl.layer_rule({\n    name      = \"zunder-fade\",\n    match     = { namespace = \"^zunder$\" },\n    animation = \"fade\",\n})\n";
        s
    } else {
        let mut s = String::from("# written by `zunder setup`; run it again to change this, or edit it freely\n\n");
        for cmd in &start {
            s += &format!("exec-once = {cmd}\n");
        }
        s += "\n";
        for (key, cmd) in &binds {
            let (mods, k) = key.rsplit_once(" + ").unwrap_or(("", key));
            s += &format!("bind = {}, {k}, exec, {cmd}\n", mods.replace(" + ", " "));
        }
        if c.window_rule {
            s += "\nwindowrule = float, title:^zunder settings$\nwindowrule = size 980 660, title:^zunder settings$\nwindowrule = center, title:^zunder settings$\n";
        }
        s += "\nlayerrule = animation fade, zunder\n";
        s
    }
}

/// Where setup reports to: the terminal, or a list for the setup page.
pub struct Out<'a> {
    /// Show what would change, write nothing.
    pub dry: bool,
    /// One line per change.
    pub log: &'a mut dyn FnMut(String),
}

impl Out<'_> {
    fn say(&mut self, line: String) {
        (self.log)(line);
    }

    fn write(&mut self, path: &Path, text: &str) -> io::Result<()> {
        if self.dry {
            self.say(format!("── {} ──\n{}", path.display(), text.trim_end()));
            return Ok(());
        }
        if path.exists() && !path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("zunder")) {
            fs::copy(path, path.with_extension(format!("{}.bak", path.extension().unwrap_or_default().to_string_lossy())))?;
        }
        fs::write(path, text)
    }
}

/// Write everything `choice` asks for, then reload Hyprland (not on a dry run).
pub fn apply(plan: &Plan, choice: &Choice, out: &mut Out) -> Result<(), String> {
    let (lua, hypr, main) = (plan.lua, &plan.hypr, &plan.main);
    let comment = if lua { "--" } else { "#" };

    if choice.import_vicinae && plan.vicinae {
        if out.dry {
            out.say("import favorites and launch counts from vicinae".into());
        } else {
            match import::vicinae() {
                Ok(report) => report.lines().for_each(|l| out.say(l.to_owned())),
                Err(e) => out.say(format!("couldn't import from vicinae: {e}")),
            }
        }
    }

    // group by file so each file is written (and backed up) once
    type Changes<'a> = Vec<(usize, String, &'a str)>;
    let mut by_file: Vec<(PathBuf, Changes)> = Vec::new();
    if choice.switch_over {
        for (file, i, line, name) in &plan.others {
            match by_file.iter_mut().find(|(f, _)| f == file) {
                Some((_, v)) => v.push((*i, line.clone(), name)),
                None => by_file.push((file.clone(), vec![(*i, line.clone(), name)])),
            }
        }
    }
    for (file, changes) in &by_file {
        let text = fs::read_to_string(file).map_err(|e| e.to_string())?;
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        for (i, original, name) in changes {
            let Some(line) = lines.get_mut(*i) else { continue };
            *line = match fix_for(original, name, lua) {
                Fix::Comment => format!("{comment} {original}  {comment} disabled by zunder setup"),
                Fix::Replace(new) => new,
            };
            out.say(format!("{}:{}  {}", file.display(), i + 1, line.trim()));
        }
        if !out.dry {
            out.write(file, &(lines.join("\n") + "\n")).map_err(|e| e.to_string())?;
        }
    }

    let own = hypr.join(if lua { "zunder-setup.lua" } else { "zunder-setup.conf" });
    out.write(&own, &generated(lua, choice)).map_err(|e| e.to_string())?;
    if !out.dry {
        out.say(format!("wrote {}", own.display()));
    }

    // load our file and the Ctrl+B shortcuts file from the main config, once
    let text = fs::read_to_string(main).map_err(|e| e.to_string())?;
    let includes: Vec<&str> = if lua {
        vec!["pcall(require, \"zunder-setup\")", "pcall(require, \"zunder\") -- shortcuts made with Ctrl+B in zunder"]
    } else {
        vec!["source = ~/.config/hypr/zunder-setup.conf", "source = ~/.config/hypr/zunder.conf"]
    };
    let missing: Vec<&str> = includes.iter().filter(|l| !text.contains(l.split(" --").next().unwrap_or(l))).copied().collect();
    if !missing.is_empty() {
        if out.dry {
            out.say(format!("add to {}:\n{}", main.display(), missing.join("\n")));
        } else {
            let new = format!("{}\n{}\n", text.trim_end(), missing.join("\n"));
            out.write(main, &new).map_err(|e| e.to_string())?;
            out.say(format!("added {} line(s) to {}", missing.len(), main.display()));
        }
    }
    // a classic config errors on a missing `source` file
    if !lua && !hypr.join("zunder.conf").exists() && !out.dry {
        let _ = fs::write(hypr.join("zunder.conf"), "# shortcuts made with Ctrl+B in zunder\n");
    }

    let plugins_dir = crate::providers::plugins::dir();
    if choice.examples && !plan.examples_present {
        if out.dry {
            out.say(format!("add the example plugins kill and ip to {}", plugins_dir.display()));
        } else {
            let _ = fs::create_dir_all(&plugins_dir);
            for (name, body) in [("kill", KILL_PLUGIN), ("ip", IP_PLUGIN)] {
                let path = plugins_dir.join(name);
                if fs::write(&path, body).is_ok() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o755));
                    }
                }
            }
            out.say(format!("example plugins in {}", plugins_dir.display()));
        }
    }

    if out.dry {
        return Ok(());
    }
    // writing the config also ends the first-run screen
    config::update(|_| {});
    let _ = Command::new("hyprctl").arg("reload").output();
    let errors = Command::new("hyprctl").arg("configerrors").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()).unwrap_or_default();
    if !errors.is_empty() && !errors.contains("no errors") {
        out.say(format!("Hyprland reports config errors:\n{errors}"));
    }
    Ok(())
}

pub fn run(args: &[String]) -> Result<(), String> {
    let opts = Opts { yes: args.iter().any(|a| a == "--yes" || a == "-y"), dry: args.iter().any(|a| a == "--dry-run") };
    let Some(plan) = Plan::detect() else {
        return Err("no Hyprland config in ~/.config/hypr; zunder setup only knows Hyprland so far, see https://github.com/elyesghazel/zunder/blob/main/docs/setup.md".into());
    };

    println!("\n{}  sets up zunder for Hyprland ({})", paint("#b4befe", "zunder setup"), plan.main.display());
    if opts.dry {
        say("dry run: nothing gets written");
    }
    let mut choice = plan.defaults();

    // ── other launchers ──
    if !plan.others.is_empty() || plan.vicinae {
        heading("Other launchers");
        for (file, i, line, name) in &plan.others {
            println!("  {} {}:{}  {}", paint("#f9e2af", name), file.display(), i + 1, line.trim());
        }
        choice.import_vicinae = plan.vicinae && yes_no(&opts, "Take over favorites and launch counts from vicinae?", true);
        choice.switch_over = !plan.others.is_empty()
            && yes_no(&opts, "Switch those over to zunder? (a .bak copy of each file is kept)", true);
    }

    // ── keys ──
    heading("Keys");
    if !plan.by_hand.is_empty() {
        say("zunder is already bound in your config:");
        for l in &plan.by_hand {
            println!("    {}", l.trim());
        }
    }
    let skip_binds = !plan.by_hand.is_empty() && !yes_no(&opts, "Add the default binds anyway?", false);
    let reused_key = plan.reused_key || (choice.switch_over && plan.takes_over_key());
    if reused_key {
        say("your old launcher key opens zunder now");
    }
    let free = |combo: &str| {
        let ok = !plan.taken(combo);
        if !ok {
            println!("    {} {combo} is already bound in Hyprland", paint("#f9e2af", "!"));
        }
        ok
    };
    if skip_binds {
        (choice.switcher, choice.overview, choice.launcher, choice.clipboard_key) = (false, false, None, false);
    } else {
        choice.switcher = yes_no(&opts, "Window switcher on ALT+TAB?", true) && (free("ALT + TAB") || yes_no(&opts, "Bind it anyway?", false));
        choice.overview = yes_no(&opts, "Every window by workspace on SUPER+TAB?", true)
            && (free("SUPER + TAB") || yes_no(&opts, "Bind it anyway?", false));
        let launcher = if reused_key {
            None
        } else {
            match ask(&opts, "Launcher key: 1 ALT+SPACE  2 SUPER+SPACE  3 none", "1").as_str() {
                "2" => Some("SUPER + SPACE".to_owned()),
                "3" => None,
                _ => Some("ALT + SPACE".to_owned()),
            }
        };
        choice.launcher = launcher.filter(|k| free(k) || yes_no(&opts, "Bind it anyway?", false));
        choice.clipboard_key = plan.cliphist
            && yes_no(&opts, "Clipboard history on SUPER+V?", true)
            && (free("SUPER + V") || yes_no(&opts, "Bind it anyway?", false));
    }
    choice.clipboard_watch = plan.cliphist
        && !plan.cliphist_running
        && yes_no(&opts, "cliphist isn't running; start it with Hyprland?", true);

    // ── theme ──
    heading("Theme");
    let names = theme::names();
    for (i, name) in names.iter().enumerate() {
        let swatch: String = ["bg", "fg", "accent", "green"]
            .iter()
            .map(|c| paint(&theme::color(name, c).unwrap_or_default(), "██"))
            .collect();
        println!("  {:>2} {swatch} {name}", i + 1);
    }
    let current = config::get().theme.clone();
    let default = names.iter().position(|n| *n == current).map_or(1, |i| i + 1);
    let picked = ask(&opts, "Theme number", &default.to_string()).parse::<usize>().ok().and_then(|n| names.get(n.wrapping_sub(1)));

    // ── plugins ──
    heading("Plugins");
    choice.examples = !plan.examples_present && yes_no(&opts, "Add the example plugins? (`kill <name>` and Copy Local IP)", true);

    // ── write ──
    heading("Writing");
    let mut log = |line: String| say(&line);
    apply(&plan, &choice, &mut Out { dry: opts.dry, log: &mut log })?;
    if let Some(name) = picked.filter(|n| **n != current) {
        if !opts.dry {
            config::update(|c| c.theme = name.clone());
        }
        say(&format!("theme: {name}"));
    }
    if opts.dry {
        say("dry run done, nothing changed");
        return Ok(());
    }
    // start the daemon under Hyprland if it isn't running (autostart only runs at login)
    if crate::ipc::send("reload").is_err() {
        let cmd = if plan.lua { "hl.dsp.exec_cmd(\"zunder server\")".to_owned() } else { "exec zunder server".to_owned() };
        let _ = Command::new("hyprctl").args(["dispatch", &cmd]).output();
    }

    heading("Done");
    if choice.switcher {
        say("ALT+TAB   switch windows");
    }
    if choice.overview {
        say("SUPER+TAB every window, by workspace");
    }
    if let Some(key) = &choice.launcher {
        say(&format!("{key}   launcher"));
    }
    if choice.clipboard_key {
        say("SUPER+V   clipboard history");
    }
    say("Ctrl+K    actions · Ctrl+F favorite · Ctrl+B global shortcut · Ctrl+, settings");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_modmasks() {
        assert_eq!(modmask("ALT + SPACE"), (8, "SPACE".into()));
        assert_eq!(modmask("ALT + SHIFT + TAB"), (9, "TAB".into()));
        assert_eq!(modmask("SUPER + V"), (64, "V".into()));
    }

    #[test]
    fn switches_lines_over() {
        let r = |l: &str, n: &str, lua: bool| match fix_for(l, n, lua) {
            Fix::Comment => "COMMENT".to_owned(),
            Fix::Replace(s) => s,
        };
        assert_eq!(r("    hl.exec_cmd(\"vicinae server\")", "vicinae", true), "COMMENT");
        assert!(r("local menu        = \"vicinae toggle\"", "vicinae", true).starts_with("local menu        = \"zunder toggle\""));
        assert!(r("hl.bind(\"SUPER + R\", hl.dsp.exec_cmd(\"rofi -show drun\"))", "rofi", true)
            .starts_with("hl.bind(\"SUPER + R\", hl.dsp.exec_cmd(\"zunder toggle\"))"));
        assert!(r("bind = SUPER, R, exec, rofi -show drun", "rofi", false).starts_with("bind = SUPER, R, exec, zunder toggle"));
        assert!(r("$menu = wofi --show drun", "wofi", false).starts_with("$menu = zunder toggle"));
        assert_eq!(r("exec-once = vicinae server", "vicinae", false), "COMMENT");
    }

    #[test]
    fn generates_both_formats() {
        let c = Choice {
            autostart: true,
            window_rule: true,
            switcher: true,
            overview: true,
            launcher: Some("ALT + SPACE".into()),
            clipboard_key: true,
            clipboard_watch: false,
            switch_over: false,
            import_vicinae: false,
            examples: false,
        };
        let lua = generated(true, &c);
        assert!(lua.contains("hl.bind(\"ALT + TAB\", hl.dsp.exec_cmd(\"zunder switch\"))"));
        assert!(lua.contains("hl.exec_cmd(\"zunder server\")"));
        assert!(lua.contains("hl.bind(\"SUPER + TAB\", hl.dsp.exec_cmd(\"zunder overview\"))"));
        let conf = generated(false, &c);
        assert!(conf.contains("bind = ALT SHIFT, TAB, exec, zunder switch-prev"));
        assert!(conf.contains("exec-once = zunder server"));
    }
}
