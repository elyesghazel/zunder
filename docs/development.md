# Development

## Build and run

```sh
cargo build
cargo test
./target/debug/zunder server       # stop your installed one first: zunder quit
./target/debug/zunder show
```

To run a dev build next to your installed daemon, give it its own runtime dir (the socket and
previews live there) and point it at the Wayland socket explicitly. Keep the path short, unix
sockets have a ~100 character limit.

```sh
RT=$(mktemp -d /tmp/zd.XXXX)
XDG_RUNTIME_DIR=$RT WAYLAND_DISPLAY=$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY ./target/debug/zunder server &
XDG_RUNTIME_DIR=$RT ./target/debug/zunder show
```

Add `XDG_CONFIG_HOME=$(mktemp -d)` to test with a clean config.

## Layout

```
src/
  main.rs             CLI: parses the command, sends it to the socket or starts the server
  ipc.rs              unix socket, one command word per connection
  ui.rs               the overlay: layer-shell window, list, keys, preview pane, keybind recording
  settings_window.rs  the settings window
  engine.rs           runs providers, groups and ranks results into sections
  item.rs             Item and Action (what Enter does), spawning processes
  config.rs           config.toml, the settings table
  store.rs            favorites, keybinds (+ the generated Hyprland file), launch log
  history.rs          launch counts for ranking
  style.css           launcher look
  settings.css        settings window look
  providers/          one file per kind of result
```

## How a keypress becomes results

1. `zunder toggle` writes `toggle` to `$XDG_RUNTIME_DIR/zunder.sock` and exits.
2. The daemon's socket thread passes it to the GTK main loop over a channel.
3. `Ui::show` asks Hyprland for windows (`hyprctl clients -j`), rescans apps and plugins, and
   presents the layer surface with exclusive keyboard focus.
4. Every change of the query calls `Engine::search`. If one provider is `exclusive` for the
   query (`>`, `clip`, `settings`…), only it runs; otherwise all enabled providers do.
5. Each provider returns `Item`s with a score. Items are sorted, cut to 12 per section (with a
   relevance cutoff), split into sections by `group`, and sections are sorted by their best score.
6. Favorites are prepended on an empty query; shortcut combos and ★ are written into the hints.
7. Enter runs the item's `Action` after the surface is gone, bumps its launch count and appends
   it to the launch log.

Search-mode plugins run in a thread; their results come back over a channel tagged with a
sequence number, and anything older than the current query is dropped.

## Adding a provider

```rust
// src/providers/dice.rs
use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item};

pub struct Dice;

impl Provider for Dice {
    fn title(&self) -> &str {
        "Dice"
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        if cx.score("roll dice").is_none() || cx.query.is_empty() {
            return Vec::new();
        }
        let n = 1 + std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() % 6;
        vec![Item {
            title: format!("rolled {n}"),
            glyph: "\u{f522}".into(),
            action: Action::Copy(n.to_string()),
            score: 100,
            ..Default::default()
        }]
    }
}
```

Register it in `providers::all()` in `src/providers/mod.rs`. It automatically gets an on/off
switch in the settings.

`Ctx` gives you `query`, `windows`, `history`, `switch` (ALT+TAB mode), `score(text)` for fuzzy
matching against the query and `matcher_score(pattern, text)` for matching part of it.

`Item` fields: `id` (needed for favorites, shortcuts and ranking), `title`, `subtitle`, `hint`,
`glyph`, `icon`, `preview` (a window `stableId`), `group` (split into several sections),
`action`, `score`.

Optional trait methods: `exclusive(query)` to take over the list for a prefix, `reload()` to
rescan when zunder opens.

## Commits

Conventional Commits, subject only: `feat(time): add world clock`. One change per commit.

## Releasing

1. Bump `version` in `Cargo.toml` and `pkgver` in `packaging/arch/PKGBUILD`, commit.
2. Tag and push: `git tag v0.2.0 && git push origin main v0.2.0`.
3. `packaging/release.sh` builds the tarball and the Arch package into `dist/` and fills in
   the PKGBUILD checksum.
4. `gh release create v0.2.0 dist/* --title v0.2.0 --notes-file …`
