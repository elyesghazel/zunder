pub mod apps;
pub mod calc;
pub mod plugins;
pub mod shell;
pub mod system;
pub mod web;
pub mod windows;

use crate::engine::Provider;

/// Registered providers. With an empty query sections appear in this order.
/// Plugins are appended by the engine.
pub fn all() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(calc::Calc),
        Box::new(windows::Windows::load()),
        Box::new(apps::Apps::load()),
        Box::new(system::System),
        Box::new(shell::Shell),
        Box::new(web::Web),
    ]
}
