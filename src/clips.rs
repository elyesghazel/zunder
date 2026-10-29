//! zunder's own clipboard history, for Windows and macOS (Linux has cliphist). The platform
//! watchers add to it, `clip` lists it. Kept in ~/.local/share/zunder/clipboard: `index.json`
//! holds the entries newest first with text inline, images are `<id>.png` next to it.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Entries kept; older ones fall off the end, with their image files.
const CAP: usize = 300;
/// Longer texts aren't kept: the whole index is rewritten on every copy.
const MAX_TEXT: usize = 1 << 20;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// Hash of the content, so copying the same thing again finds the entry.
    pub id: String,
    /// Unix seconds of the last copy.
    pub at: i64,
    pub content: Content,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Content {
    Text(String),
    /// Saved as `<id>.png`, see `image_path`.
    Image { width: u32, height: u32 },
}

fn dir() -> PathBuf {
    crate::store::data_dir().join("clipboard")
}

fn index() -> PathBuf {
    dir().join("index.json")
}

pub fn image_path(id: &str) -> PathBuf {
    dir().join(format!("{id}.png"))
}

/// The history, newest first.
pub fn list() -> Vec<Clip> {
    crate::store::read_json(&index())
}

pub fn find(id: &str) -> Option<Clip> {
    list().into_iter().find(|c| c.id == id)
}

/// Something was copied as text.
pub fn add_text(text: &str) {
    if text.trim().is_empty() || text.len() > MAX_TEXT {
        return;
    }
    let id = key(b't', text.as_bytes());
    save(Clip { id, at: now(), content: Content::Text(text.to_owned()) });
}

/// Something was copied as an image. `data` identifies it (the pixels, or the bytes the
/// clipboard had); `encode` writes it as PNG to the given path and returns its size. It only
/// runs for images that aren't in the history yet.
pub fn add_image(data: Vec<u8>, encode: impl FnOnce(Vec<u8>, &Path) -> Option<(u32, u32)>) {
    let id = key(b'i', &data);
    let path = image_path(&id);
    let content = match find(&id) {
        Some(Clip { content: c @ Content::Image { .. }, .. }) if path.exists() => c,
        _ => {
            let _ = fs::create_dir_all(dir());
            let Some((width, height)) = encode(data, &path) else { return };
            Content::Image { width, height }
        }
    };
    save(Clip { id, at: now(), content });
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The kind goes into the hash so a text and an image can't share an id.
fn key(kind: u8, bytes: &[u8]) -> String {
    let mut tagged = Vec::with_capacity(bytes.len() + 1);
    tagged.push(kind);
    tagged.extend_from_slice(bytes);
    format!("{:016x}", crate::store::fnv1a(&tagged))
}

/// Only the watcher thread writes; the rename keeps `clip` from reading half a file.
fn save(clip: Clip) {
    let mut clips = list();
    let dropped = push(&mut clips, clip, CAP);
    let tmp = dir().join("index.json.tmp");
    crate::store::write_json(&tmp, &clips);
    if fs::rename(&tmp, index()).is_err() {
        return;
    }
    for old in dropped {
        if matches!(old.content, Content::Image { .. }) {
            let _ = fs::remove_file(image_path(&old.id));
        }
    }
}

/// Put `clip` on top, replacing an older copy of the same content, and keep at most `cap`
/// entries. Returns the ones that fell off.
fn push(clips: &mut Vec<Clip>, clip: Clip, cap: usize) -> Vec<Clip> {
    clips.retain(|c| c.id != clip.id);
    clips.insert(0, clip);
    clips.split_off(cap.min(clips.len()))
}

/// One line for the result list: whitespace collapsed, cut at `width` characters.
pub fn summary(text: &str, width: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(width) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(id: &str) -> Clip {
        Clip { id: id.into(), at: 0, content: Content::Text(id.into()) }
    }

    fn ids(clips: &[Clip]) -> Vec<&str> {
        clips.iter().map(|c| c.id.as_str()).collect()
    }

    #[test]
    fn newest_first() {
        let mut clips = Vec::new();
        for id in ["a", "b", "c"] {
            assert!(push(&mut clips, text(id), 10).is_empty());
        }
        assert_eq!(ids(&clips), ["c", "b", "a"]);
    }

    #[test]
    fn a_recopy_moves_to_the_top() {
        let mut clips = vec![text("c"), text("b"), text("a")];
        let again = Clip { at: 5, ..text("a") };
        assert!(push(&mut clips, again, 10).is_empty());
        assert_eq!(ids(&clips), ["a", "c", "b"]);
        assert_eq!(clips[0].at, 5);
    }

    #[test]
    fn caps_and_returns_what_fell_off() {
        let mut clips = vec![text("c"), text("b"), text("a")];
        let dropped = push(&mut clips, text("d"), 3);
        assert_eq!(ids(&clips), ["d", "c", "b"]);
        assert_eq!(ids(&dropped), ["a"]);
        // a recopy at the cap drops nothing
        assert!(push(&mut clips, text("b"), 3).is_empty());
        assert_eq!(ids(&clips), ["b", "d", "c"]);
    }

    #[test]
    fn ids_depend_on_content_and_kind() {
        assert_eq!(key(b't', b"hi"), key(b't', b"hi"));
        assert_ne!(key(b't', b"hi"), key(b't', b"ho"));
        assert_ne!(key(b't', b"hi"), key(b'i', b"hi"));
    }

    #[test]
    fn index_format() {
        let clips = vec![text("a"), Clip { id: "b".into(), at: 1, content: Content::Image { width: 2, height: 3 } }];
        let json = serde_json::to_string(&clips).unwrap();
        assert_eq!(
            json,
            r#"[{"id":"a","at":0,"content":{"text":"a"}},{"id":"b","at":1,"content":{"image":{"width":2,"height":3}}}]"#
        );
        assert_eq!(serde_json::from_str::<Vec<Clip>>(&json).unwrap(), clips);
    }

    #[test]
    fn summaries() {
        assert_eq!(summary("  ssh  root@host\n\tls ", 80), "ssh root@host ls");
        assert_eq!(summary("héllo wörld", 5), "héllo…");
        assert_eq!(summary("short", 5), "short");
    }
}
