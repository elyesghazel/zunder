//! Everything that differs between Linux, macOS and Windows. Each OS module provides the same
//! functions; the rest of zunder only talks to `platform::…`.
//!
//! | function             | what it does                                               |
//! |----------------------|------------------------------------------------------------|
//! | `init_overlay`       | turn the launcher window into an overlay                   |
//! | `list_windows`       | open windows, most recently focused first                  |
//! | `focus_window`       | bring a window to the front                                |
//! | `capture_window`     | screenshot of one window into a file                       |
//! | `scan_apps`          | installed applications                                     |
//! | `open` / `copy`      | open a file or URL / put text on the clipboard             |
//! | `run_in_terminal`    | run a command in a terminal that stays open                |
//! | `terminal_command`   | wrap an app's command so it runs in a terminal             |
//! | `system_commands`    | lock, sleep, restart…                                      |
//! | `install_binds`      | turn zunder's keybinds into global shortcuts               |
//! | `is_executable`      | whether a file in the plugins folder can be run            |
//! | `init_tray` / `sync_tray` | tray icon with open, settings, reload, quit (Linux)   |

use std::process::{Command, Stdio};
use std::thread;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "linux")]
mod tray;
#[cfg(target_os = "linux")]
pub use tray::{init_tray, sync_tray};

/// macOS and Windows have no tray icon yet.
#[cfg(not(target_os = "linux"))]
pub fn init_tray(_tx: async_channel::Sender<String>) {}
#[cfg(not(target_os = "linux"))]
pub fn sync_tray() {}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

/// An entry for the System section.
pub struct SystemCommand {
    pub title: &'static str,
    pub keywords: &'static str,
    pub glyph: &'static str,
    pub command: &'static str,
}

/// Start a detached process (no stdio, survives zunder) and reap it.
pub fn spawn(argv: &[&str]) {
    let Some((program, args)) = argv.split_first() else { return };
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    detach(&mut cmd);
    match cmd.spawn() {
        Ok(mut child) => {
            thread::spawn(move || child.wait());
        }
        Err(e) => eprintln!("zunder: failed to run {argv:?}: {e}"),
    }
}

#[cfg(unix)]
fn detach(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}

#[cfg(windows)]
fn detach(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
}

/// The system shell, for command lines from plugins, `>` and actions.
pub fn shell(cmd: &str) -> Command {
    #[cfg(unix)]
    let mut c = {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    };
    #[cfg(windows)]
    let mut c = {
        // raw: Rust's argument escaping (\") isn't what cmd.exe expects, which broke quotes
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("cmd");
        c.raw_arg(format!("/C {cmd}"));
        c
    };
    c.stdin(Stdio::null());
    c
}

/// Run a command line in the background.
pub fn run_shell(cmd: &str) {
    let mut c = shell(cmd);
    c.stdout(Stdio::null()).stderr(Stdio::null());
    detach(&mut c);
    match c.spawn() {
        Ok(mut child) => {
            thread::spawn(move || child.wait());
        }
        Err(e) => eprintln!("zunder: failed to run {cmd}: {e}"),
    }
}

/// Put text on the clipboard by piping it into a helper program.
#[allow(dead_code)]
fn pipe_to(program: &str, args: &[&str], text: &str) {
    use std::io::Write;
    let child = Command::new(program).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    if let Ok(mut child) = child {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        thread::spawn(move || child.wait());
    }
}

/// The command that installs `tool` with this system's package manager, run in a terminal from
/// a "… is not installed" result. `None` when we don't know how.
pub fn install_command(tool: &str) -> Option<String> {
    let has = |program: &str| {
        std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
    };
    if cfg!(windows) {
        // fd ships with the Windows installer; this is for the zip. Scoop needs no admin and
        // works where winget is turned off (company laptops).
        if has("scoop.cmd") || has("scoop.ps1") {
            return Some(format!("scoop install {tool}"));
        }
        let id = match tool {
            "fd" => "sharkdp.fd",
            _ => return None,
        };
        return has("winget.exe").then(|| format!("winget install -e --id {id}"));
    }
    if cfg!(target_os = "macos") {
        return has("brew").then(|| format!("brew install {tool}"));
    }
    // Linux: the package is called differently on Debian and Fedora
    let (manager, package) = if has("pacman") {
        ("sudo pacman -S --needed", tool)
    } else if has("apt-get") {
        ("sudo apt install", if tool == "fd" { "fd-find" } else { tool })
    } else if has("dnf") {
        ("sudo dnf install", if tool == "fd" { "fd-find" } else { tool })
    } else if has("zypper") {
        ("sudo zypper install", tool)
    } else {
        return None;
    };
    Some(format!("{manager} {package}"))
}

/// The local IANA time zone, e.g. `Europe/Zurich`, on every OS.
pub fn time_zone() -> Option<String> {
    iana_time_zone::get_timezone().ok()
}
