// release builds on Windows are a GUI app: no console window next to the launcher
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod config;
mod engine;
mod history;
mod import;
mod ipc;
mod item;
mod platform;
mod providers;
mod settings_window;
mod store;
mod theme;
mod ui;

use std::process::ExitCode;

const USAGE: &str = "zunder — a terminal-styled launcher for Hyprland

usage:
  zunder                 toggle the launcher (starts the server if needed)
  zunder server          run the server (put this in autostart)
  zunder toggle          show or hide the launcher
  zunder show [query]   show, optionally prefilled (e.g. `zunder show 'kill '`)
  zunder hide
  zunder switch          window switcher: opens on the previous window, next on repeat
  zunder switch-prev     same, backwards
  zunder settings [page] open the settings window, e.g. `zunder settings appearance`
  zunder run <key>       run a saved keybind (what the generated Hyprland binds call)
  zunder reload          rescan apps and plugins
  zunder import vicinae  take over favorites and launch counts from vicinae
  zunder quit            stop the server";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("server") => ui::run(None),
        None => match ipc::send("toggle") {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ui::run(Some("show")),
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
        Some(cmd @ ("toggle" | "show" | "hide" | "switch" | "switch-prev" | "reload" | "quit" | "settings" | "run")) => {
            let line = match (cmd, args.get(1)) {
                ("settings", Some(page)) => format!("open-settings {page}"),
                ("settings", None) => "open-settings".to_owned(),
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
