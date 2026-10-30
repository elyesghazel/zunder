// release builds on Windows are a GUI app: no console window next to the launcher
#![cfg_attr(all(windows, not(debug_assertions), not(feature = "console")), windows_subsystem = "windows")]

mod actions;
mod changelog_window;
// Linux keeps its history in cliphist; the tests run everywhere
#[cfg(any(not(target_os = "linux"), test))]
#[cfg_attr(target_os = "linux", allow(dead_code))]
mod clips;
mod config;
mod engine;
mod history;
mod import;
mod ipc;
mod item;
mod platform;
mod providers;
mod settings_window;
mod setup;
mod store;
mod theme;
mod ui;

use std::process::ExitCode;

const USAGE: &str = "zunder — a terminal-styled launcher for Hyprland

usage:
  zunder                 toggle the launcher (starts the server if needed)
  zunder setup           set up keybinds, autostart and theme for Hyprland (--dry-run, --yes);
                         `zunder show setup` is the same as a page
  zunder server          run the server (put this in autostart)
  zunder toggle          show or hide the launcher
  zunder show [query]   show, optionally prefilled (e.g. `zunder show 'kill '`)
  zunder hide
  zunder switch          window switcher: opens on the previous window, next on repeat
  zunder switch-prev     same, backwards
  zunder alt-up          Alt was let go: the switcher focuses its window (a Hyprland release bind)
  zunder overview        every window by workspace, stays open until you pick one
  zunder settings [page] open the settings window, e.g. `zunder settings appearance`
  zunder changelog       what changed in each release, and the update when one is out
  zunder run <key>       run a saved keybind (what the generated Hyprland binds call)
  zunder reload          rescan apps and plugins
  zunder import vicinae  take over favorites and launch counts from vicinae
  zunder quit            stop the server";

fn main() -> ExitCode {
    log_panics();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("server") => ui::run(None),
        None => match ipc::send("toggle") {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ui::run(Some("show")),
        },
        Some("setup") => match setup::run(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("zunder: {e}");
                ExitCode::FAILURE
            }
        },
        Some("import") => match args.get(1).map(String::as_str) {
            Some("vicinae") => match import::vicinae() {
                Ok(report) => {
                    println!("{report}");
                    let _ = ipc::send("reload");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("zunder: {e}");
                    ExitCode::FAILURE
                }
            },
            _ => {
                eprintln!("zunder: usage: zunder import vicinae");
                ExitCode::FAILURE
            }
        },
        Some("-V" | "--version" | "version") => {
            println!("zunder {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(cmd @ ("toggle" | "show" | "hide" | "switch" | "switch-prev" | "alt-up" | "overview" | "reload" | "quit" | "settings" | "changelog" | "run")) => {
            let line = match (cmd, args.get(1)) {
                ("settings", Some(page)) => format!("open-settings {page}"),
                ("settings", None) => "open-settings".to_owned(),
                ("changelog", _) => "open-changelog".to_owned(),
                ("run", Some(key)) => format!("run {key}"),
                ("show", Some(query)) => format!("show {query}"),
                _ => cmd.to_owned(),
            };
            match ipc::send(&line) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("zunder: server not reachable ({e}); start it with `zunder server`");
                    ExitCode::FAILURE
                }
            }
        }
        Some(other) => {
            eprintln!("zunder: unknown command `{other}`\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// Panics also go to <data dir>/zunder/crash.log: a GUI build on Windows or a macOS app has no
/// terminal to print them to.
fn log_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let dir = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("zunder");
        let _ = std::fs::create_dir_all(&dir);
        let line = format!("{} zunder {}: {info}\n", chrono::Local::now().to_rfc3339(), env!("CARGO_PKG_VERSION"));
        let _ = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("crash.log")).and_then(|mut f| {
            use std::io::Write;
            f.write_all(line.as_bytes())
        });
        default(info);
    }));
}
