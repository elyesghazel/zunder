pub mod apps;
pub mod calc;
pub mod emoji;
pub mod files;
pub mod clipboard;
pub mod launches;
pub mod plugins;
pub mod settings;
pub mod shell;
pub mod system;
pub mod themes;
pub mod time;
pub mod web;
pub mod windows;

use crate::engine::Provider;

/// Registered providers. With an empty query sections appear in this order.
/// Plugins are appended by the engine.
pub fn all() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(calc::Calc),
        Box::new(time::Time),
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
