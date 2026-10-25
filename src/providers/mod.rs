pub mod apps;
pub mod calc;
pub mod emoji;
pub mod files;
pub mod clipboard;
pub mod convert;
pub mod launches;
pub mod plugins;
pub mod settings;
pub mod setup;
pub mod shell;
pub mod system;
pub mod themes;
pub mod time;
pub mod update;
pub mod welcome;
pub mod web;
pub mod windows;

use crate::engine::Provider;

/// Registered providers. With an empty query sections appear in this order.
/// Plugins are appended by the engine.
pub fn all() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(welcome::Welcome),
        Box::new(setup::Setup),
        Box::new(update::Update),
        Box::new(calc::Calc),
        Box::new(time::Time),
        Box::new(convert::Convert),
        Box::new(windows::Windows::load()),
        Box::new(apps::Apps::load()),
        Box::new(system::System),
        Box::new(shell::Shell),
        Box::new(clipboard::Clipboard),
        Box::new(launches::Launches),
        Box::new(themes::Themes),
        Box::new(emoji::Emoji),
        Box::new(files::Files),
        Box::new(web::Web),
    ]
}

/// "<tool> is not installed": Enter installs it in a terminal when we know how.
pub fn missing_tool(tool: &str, purpose: &str, after: &str) -> crate::item::Item {
    use crate::item::{Action, Item};
    match crate::platform::install_command(tool) {
        Some(cmd) => Item {
            title: format!("Install {tool}"),
            subtitle: format!("not installed yet, {purpose}{after} · {cmd}"),
            hint: "install".into(),
            glyph: "\u{f019}".into(),
            action: Action::Term(cmd),
            ..Default::default()
        },
        None => Item {
            title: format!("{tool} is not installed"),
            subtitle: format!("{purpose}{after} · install it with your package manager"),
            glyph: "\u{f071}".into(),
            ..Default::default()
        },
    }
}
