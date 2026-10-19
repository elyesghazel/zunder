//! `zunder import vicinae`: take over favorites and launch counts from vicinae.

use std::fs;

use regex::Regex;

use crate::history::History;
use crate::item::{Action, Item};
use crate::providers::apps;
use crate::store::Favorites;

pub fn vicinae() -> Result<String, String> {
    let config = dirs::config_dir().unwrap_or_default().join("vicinae/settings.json");
    let settings = fs::read_to_string(&config).map_err(|e| format!("{}: {e}", config.display()))?;
    let apps = apps::scan_all();

    // vicinae ids look like `applications:dev.zed.Zed`; zunder's like `app:dev.zed.Zed.desktop`
    let find_app = |id: &str| {
        let want = format!("{id}.desktop");
        apps.iter()
            .find(|a| a.id == want)
            .or_else(|| apps.iter().find(|a| a.id.starts_with(&format!("{id}-")) && !a.id.contains("url-handler")))
    };
    let to_item = |vid: &str| -> Option<Item> {
        if let Some(id) = vid.strip_prefix("applications:") {
            return find_app(id).map(apps::item);
        }
        match vid {
            "clipboard:history" => Some(Item {
                id: "cmd:clipboard".into(),
                title: "Clipboard History".into(),
                subtitle: "everything you copied".into(),
                hint: "clip ␣".into(),
                glyph: "\u{f0ea}".into(),
                action: Action::Fill("clip ".into()),
                ..Default::default()
            }),
            _ => None,
        }
    };

    // settings.json allows comments, so pull the array out instead of parsing the whole file
    let list = Regex::new(r#""favorites"\s*:\s*\[([^\]]*)\]"#).unwrap();
    let ids: Vec<String> = list
        .captures(&settings)
        .map(|c| Regex::new(r#""([^"]+)""#).unwrap().captures_iter(&c[1]).map(|m| m[1].to_owned()).collect())
        .unwrap_or_default();

    let mut favorites = Favorites::load();
    let (mut added, mut skipped) = (Vec::new(), Vec::new());
    for vid in &ids {
        match to_item(vid) {
            Some(item) if !favorites.contains(&item.id) => {
                added.push(item.title.clone());
                favorites.toggle(&item);
            }
            Some(_) => {}
            None => skipped.push(vid.clone()),
        }
    }

    // launch counts from metadata.json feed the ranking
    let mut history = History::load();
    let mut counted = 0;
    let metadata = dirs::data_dir().unwrap_or_default().join("vicinae/metadata.json");
    if let Some(visited) = fs::read_to_string(metadata)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("visited").cloned())
        .and_then(|v| v.as_object().cloned())
    {
        for (vid, entry) in visited {
            let count = entry.get("visit_count").and_then(|c| c.as_u64()).unwrap_or(0) as u32;
            if let Some(item) = to_item(&vid).filter(|_| count > 0) {
                history.raise_to(&item.id, count);
                counted += 1;
            }
        }
        history.save();
    }

    let mut report = format!("favorites added: {}\n", if added.is_empty() { "none".into() } else { added.join(", ") });
    if !skipped.is_empty() {
        report += &format!("not available in zunder: {}\n", skipped.join(", "));
    }
    report += &format!("launch counts taken over for {counted} items");
    Ok(report)
}
