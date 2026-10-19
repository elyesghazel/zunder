pub mod apps;
pub mod windows;

use crate::engine::Provider;

/// Registered providers. With an empty query sections appear in this order.
pub fn all() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(windows::Windows),
        Box::new(apps::Apps::load()),
    ]
}
