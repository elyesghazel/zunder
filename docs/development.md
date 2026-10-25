# Development

## How it works

```
keybind ─► zunder switch ─► $XDG_RUNTIME_DIR/zunder.sock ─► daemon
                                                              │
                     hyprctl clients -j ◄─────────────────────┤ on open
                     .desktop files     ◄─────────────────────┤
                     grim -T <window>   ◄─────────────────────┘ ALT+TAB only, in threads
```

The daemon keeps GTK and the app index loaded, so opening is instant; the CLI only writes a word
to the socket. The overlay is a full-screen layer-shell surface with exclusive keyboard focus.
Results come from providers, one Rust file each in `src/providers/`, and adding one is about 30
lines.

Measured on a laptop with Hyprland (release build, `ZUNDER_TIMING=1 zunder server` prints these):

| Step | Time |
|------|------|
| open, start screen unchanged since last time | 3–8 ms |
| open, start screen changed | about 25 ms |
| search per keystroke | 0.3–2.6 ms |

The window list comes from Hyprland's event socket instead of `hyprctl` on every open, apps are
rescanned only when an `applications/` folder changes, and unchanged results reuse their rows.

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

Add `XDG_CONFIG_HOME=$(mktemp -d)` to test with a clean config, and `ZUNDER_TIMING=1` to the
server to print how long each step of opening and searching takes. Keep opening under 30 ms and
a keystroke under 5 ms.

## Layout

```
src/
  main.rs             CLI: parses the command, sends it to the socket or starts the server
  ipc.rs              unix socket, one command word per connection
  ui.rs               the overlay: layer-shell window, list, keys, preview pane, keybind recording
  settings_window.rs  the settings window
  setup.rs            `zunder setup`: detects the Hyprland config, writes zunder-setup.lua
  actions.rs          the Ctrl+K menu: every action a result has besides Enter
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

`Item` fields: `id` (needed for favorites, shortcuts and ranking; an id starting with `pending:`
marks a placeholder that a `deferred` section with the same title replaces), `title`, `subtitle`, `hint`,
`glyph`, `icon`, `preview` (a window `stableId`), `group` (split into several sections),
`action`, `score`.

Optional trait methods: `exclusive(query)` to take over the list for a prefix, `reload()` to
rescan when zunder opens.

## Commits

Conventional Commits, subject only: `feat(time): add world clock`. One change per commit.

## Releasing

```sh
packaging/bump.sh 0.2.0
```

That sets the version in `Cargo.toml`, `Cargo.lock` and the PKGBUILD, runs the tests, commits
`chore(release): v0.2.0`, tags `v0.2.0` and pushes. The tag starts
[`release.yml`](../.github/workflows/release.yml), which builds on GitHub's machines and publishes
a release with:

| File | Built on | What's in it |
|------|----------|--------------|
| `zunder-<v>-1-x86_64.pkg.tar.zst` | Arch container | Arch package (`pacman -U`) |
| `zunder-<v>-x86_64-linux.tar.gz` | Arch container | binary, `install.sh`, docs, example plugins |
| `zunder-<v>-windows-x86_64.zip` | Windows, MSYS2 | `zunder.exe` with the GTK DLLs, icons and schemas; `zunder.cmd` starts it |
| `zunder-<v>-macos-arm64.dmg` | macOS (Apple Silicon) | `zunder.app` with the GTK libraries bundled by dylibbundler |

Release notes are generated from the commit subjects since the last tag. Run the workflow by hand
from the Actions tab to build the files without publishing anything.

After the release workflow has published the files, `packaging/aur/update.sh <version>` pins
their checksums, writes `.SRCINFO` and pushes `zunder` (builds from source) and `zunder-bin` (the
release binary) to the AUR. It needs the AUR SSH key in `~/.ssh/aur`.

Nothing releases on a normal push; `ci.yml` only builds and tests on all three systems.

`packaging/release.sh` still builds the Linux files locally, for testing the package before a
release.
