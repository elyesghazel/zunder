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

struct Choice {
    /// Start the daemon with Hyprland (off when the config already does).
    autostart: bool,
    /// Float the settings window (off when the config already has a rule for it).
    window_rule: bool,
    switcher: bool,
    launcher: Option<String>,
    clipboard_key: bool,
    clipboard_watch: bool,
}

fn generated(lua: bool, c: &Choice) -> String {
    let mut binds: Vec<(String, String)> = Vec::new();
    if c.switcher {
        binds.push(("ALT + TAB".into(), "zunder switch".into()));
        binds.push(("ALT + SHIFT + TAB".into(), "zunder switch-prev".into()));
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
        s
    }
}

fn write(opts: &Opts, path: &Path, text: &str) -> io::Result<()> {
    if opts.dry {
        println!("{}", paint("#5a5a6a", &format!("── would write {} ──", path.display())));
        println!("{text}");
        return Ok(());
    }
    if path.exists() && !path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("zunder")) {
        fs::copy(path, path.with_extension(format!("{}.bak", path.extension().unwrap_or_default().to_string_lossy())))?;
    }
    fs::write(path, text)
}

pub fn run(args: &[String]) -> Result<(), String> {
    let opts = Opts { yes: args.iter().any(|a| a == "--yes" || a == "-y"), dry: args.iter().any(|a| a == "--dry-run") };
    let hypr = dirs::config_dir().unwrap_or_default().join("hypr");
    let main_lua = hypr.join("hyprland.lua");
    let main_conf = hypr.join("hyprland.conf");
    let lua = main_lua.exists();
    let main = if lua { main_lua } else { main_conf };
    if !main.exists() {
        return Err(format!(
            "no Hyprland config at {}; zunder setup only knows Hyprland so far, see https://github.com/elyesghazel/zunder#setup",
            main.display()
        ));
    }

    println!("\n{}  sets up zunder for Hyprland ({})", paint("#b4befe", "zunder setup"), main.display());
    if opts.dry {
        say("dry run: nothing gets written");
    }

    let files = config_files(&hypr, lua);
    let comment = if lua { "--" } else { "#" };

    // ── other launchers ──
    let others = other_launcher_lines(&files, lua);
    let mut disable = Vec::new();
    let vicinae_cfg = dirs::config_dir().unwrap_or_default().join("vicinae/settings.json");
    if !others.is_empty() || vicinae_cfg.exists() {
        heading("Other launchers");
        for (file, i, line, name) in &others {
            println!("  {} {}:{}  {}", paint("#f9e2af", name), file.display(), i + 1, line.trim());
        }
        if vicinae_cfg.exists() && yes_no(&opts, "Take over favorites and launch counts from vicinae?", true) {
            if opts.dry {
                say("would import from vicinae");
            } else {
                match import::vicinae() {
                    Ok(report) => report.lines().for_each(|l| println!("  {l}")),
                    Err(e) => println!("  couldn't import: {e}"),
                }
            }
        }
        if !others.is_empty() && yes_no(&opts, "Switch those over to zunder? (a .bak copy of each file is kept)", true) {
            disable = others;
        }
    }

    // ── keys ──
    heading("Keys");
    let by_hand = existing_zunder(&files, lua);
    if !by_hand.is_empty() {
        say("zunder is already bound in your config:");
        for l in &by_hand {
            println!("    {}", l.trim());
        }
    }
    let skip_binds = !by_hand.is_empty() && !yes_no(&opts, "Add the default binds anyway?", false);
    // a launcher key that used to open the old launcher opens zunder now; don't bind a second one
    let reused_key = disable.iter().any(|(_, _, line, name)| matches!(fix_for(line, name, lua), Fix::Replace(_)))
        || files.iter().filter_map(|f| fs::read_to_string(f).ok()).any(|t| t.contains("changed by zunder setup"));
    if reused_key {
        say("your old launcher key opens zunder now");
    }
    let taken = taken_binds();
    let free = |combo: &str| {
        let ok = !taken.contains(&modmask(combo));
        if !ok {
            println!("    {} {combo} is already bound in Hyprland", paint("#f9e2af", "!"));
        }
        ok
    };
    let (switcher, launcher, clipboard_key) = if skip_binds {
        (false, None, false)
    } else {
        let switcher = yes_no(&opts, "Window switcher on ALT+TAB?", true) && (free("ALT + TAB") || yes_no(&opts, "Bind it anyway?", false));
        let launcher = if reused_key {
            None
        } else {
            match ask(&opts, "Launcher key: 1 ALT+SPACE  2 SUPER+SPACE  3 none", "1").as_str() {
                "2" => Some("SUPER + SPACE".to_owned()),
                "3" => None,
                _ => Some("ALT + SPACE".to_owned()),
            }
        };
        let launcher = launcher.filter(|k| free(k) || yes_no(&opts, "Bind it anyway?", false));
        let clipboard_key = has("cliphist")
            && yes_no(&opts, "Clipboard history on SUPER+V?", true)
            && (free("SUPER + V") || yes_no(&opts, "Bind it anyway?", false));
        (switcher, launcher, clipboard_key)
    };
    let clipboard_watch = has("cliphist")
        && !running("cliphist store")
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
    let plugins_dir = crate::providers::plugins::dir();
    let examples = !plugins_dir.join("kill").exists()
        && yes_no(&opts, "Add the example plugins? (`kill <name>` and Copy Local IP)", true);

    // ── write ──
    heading("Writing");
    // group by file so each file is written (and backed up) once
    type Changes<'a> = Vec<(usize, String, &'a str)>;
    let mut by_file: Vec<(PathBuf, Changes)> = Vec::new();
    for (file, i, line, name) in &disable {
        match by_file.iter_mut().find(|(f, _)| f == file) {
            Some((_, v)) => v.push((*i, line.clone(), name)),
            None => by_file.push((file.clone(), vec![(*i, line.clone(), name)])),
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
            say(&format!("{}:{}  {}", file.display(), i + 1, line.trim()));
        }
        write(&opts, file, &(lines.join("\n") + "\n")).map_err(|e| e.to_string())?;
    }

    let autostart = !by_hand.iter().any(|l| l.contains("zunder server"));
    let window_rule = !files.iter().filter_map(|f| fs::read_to_string(f).ok()).any(|t| t.contains("zunder settings"));
    let choice = Choice { autostart, window_rule, switcher, launcher, clipboard_key, clipboard_watch };
    let own = hypr.join(if lua { "zunder-setup.lua" } else { "zunder-setup.conf" });
    write(&opts, &own, &generated(lua, &choice)).map_err(|e| e.to_string())?;
    say(&format!("wrote {}", own.display()));

    // load our file and the Ctrl+B shortcuts file from the main config, once
    let text = fs::read_to_string(&main).map_err(|e| e.to_string())?;
    let includes: Vec<&str> = if lua {
        vec!["pcall(require, \"zunder-setup\")", "pcall(require, \"zunder\") -- shortcuts made with Ctrl+B in zunder"]
    } else {
        vec!["source = ~/.config/hypr/zunder-setup.conf", "source = ~/.config/hypr/zunder.conf"]
    };
    let missing: Vec<&str> = includes.iter().filter(|l| !text.contains(l.split(" --").next().unwrap_or(l))).copied().collect();
    if !missing.is_empty() {
        let new = format!("{}\n{}\n", text.trim_end(), missing.join("\n"));
        write(&opts, &main, &new).map_err(|e| e.to_string())?;
        say(&format!("added {} line(s) to {}", missing.len(), main.display()));
    }
    // a classic config errors on a missing `source` file
    if !lua && !hypr.join("zunder.conf").exists() && !opts.dry {
        let _ = fs::write(hypr.join("zunder.conf"), "# shortcuts made with Ctrl+B in zunder\n");
    }

    if let Some(name) = picked.filter(|n| **n != current) {
        if !opts.dry {
            config::update(|c| c.theme = name.clone());
        }
        say(&format!("theme: {name}"));
    }
    if examples && !opts.dry {
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
        say(&format!("example plugins in {}", plugins_dir.display()));
    }

    if opts.dry {
        say("dry run done, nothing changed");
        return Ok(());
    }
    // writing the config also ends the first-run screen
    config::update(|_| {});

    // apply now: reload Hyprland and start the daemon under it (autostart only runs at login)
    let _ = Command::new("hyprctl").arg("reload").output();
    let errors = Command::new("hyprctl").arg("configerrors").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()).unwrap_or_default();
    if !errors.is_empty() && !errors.contains("no errors") {
        println!("  {} Hyprland reports config errors:\n{errors}", paint("#f38ba8", "✗"));
    }
    if crate::ipc::send("reload").is_err() {
        let cmd = if lua { "hl.dsp.exec_cmd(\"zunder server\")".to_owned() } else { "exec zunder server".to_owned() };
        let _ = Command::new("hyprctl").args(["dispatch", &cmd]).output();
    }

    heading("Done");
    if switcher {
        say("ALT+TAB   switch windows");
    }
    if let Some(key) = &choice.launcher {
        say(&format!("{key}   launcher"));
    }
    if choice.clipboard_key {
        say("SUPER+V   clipboard history");
    }
    say("Ctrl+,    settings · Ctrl+F favorite · Ctrl+B global shortcut");
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
        let c = Choice { autostart: true, window_rule: true, switcher: true, launcher: Some("ALT + SPACE".into()), clipboard_key: true, clipboard_watch: false };
        let lua = generated(true, &c);
        assert!(lua.contains("hl.bind(\"ALT + TAB\", hl.dsp.exec_cmd(\"zunder switch\"))"));
        assert!(lua.contains("hl.exec_cmd(\"zunder server\")"));
        let conf = generated(false, &c);
        assert!(conf.contains("bind = ALT SHIFT, TAB, exec, zunder switch-prev"));
        assert!(conf.contains("exec-once = zunder server"));
    }
}
