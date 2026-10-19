mod engine;
mod history;
mod ipc;
mod item;
mod providers;
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
  zunder reload          rescan apps and plugins
  zunder quit            stop the server";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("server") => ui::run(None),
        None => match ipc::send("toggle") {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ui::run(Some("show")),
        },
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(cmd @ ("toggle" | "show" | "hide" | "switch" | "switch-prev" | "reload" | "quit")) => {
            let line = match (cmd, args.get(1)) {
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
