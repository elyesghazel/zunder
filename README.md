# zunder

A launcher and ALT+TAB window switcher for Hyprland that looks like a terminal.

![zunder](assets/screenshot.png)

zunder runs as a small daemon. A keybind sends it a command over a unix socket, and it shows a
layer-shell overlay on top of everything: one prompt, results grouped in sections, monospace,
no icons. Open windows, apps, a calculator, shell commands, power actions and your own scripts
all live in the same list. Plugins are just executables that print lines, so you can write them
in whatever language you like.

Written in Rust with GTK4 and gtk4-layer-shell. Built and tested on Arch with Hyprland (Lua config).

## Install

Needs `gtk4`, `gtk4-layer-shell`, `wl-clipboard` and a Nerd Font. JetBrainsMono is the default,
change it in the theme if you use another one.

```sh
sudo pacman -S --needed rust gtk4 gtk4-layer-shell wl-clipboard ttf-jetbrains-mono-nerd

git clone https://github.com/elyesghazel/zunder
cd zunder
cargo build --release
install -Dm755 target/release/zunder ~/.local/bin/zunder
```

Then start the daemon with Hyprland and bind the keys. In a Lua config:

```lua
-- conf/autostart.lua
hl.exec_cmd("zunder server")

-- conf/binds.lua
hl.bind("ALT + TAB",         hl.dsp.exec_cmd("zunder switch"))
hl.bind("ALT + SHIFT + TAB", hl.dsp.exec_cmd("zunder switch-prev"))
hl.bind("ALT + SPACE",       hl.dsp.exec_cmd("zunder toggle"))
```

Old `hyprland.conf` style works the same way: `exec-once = zunder server` and
`bind = ALT, TAB, exec, zunder switch`.

## Usage

```sh
zunder                  # toggle; starts the daemon first if it isn't running
zunder server           # run the daemon (autostart)
zunder toggle           # show / hide the launcher
zunder show             # show
zunder show 'kill '     # show with the query already typed
zunder hide
zunder switch           # window switcher, or next window if it's already open
zunder switch-prev      # same, backwards
zunder reload           # rescan apps and plugins
zunder quit             # stop the daemon
```

### Keys

| Key | What it does |
|-----|--------------|
| `ALT+TAB` | open the switcher on the previous window, like every other OS |
| `Tab` while holding Alt | next window |
| release Alt | switch to the selected window |
| `↑` `↓` / `Tab` `Shift+Tab` / `Ctrl+J` `Ctrl+K` / `Ctrl+N` `Ctrl+P` | move |
| `Enter` | run the selected result |
| `Esc` or click outside | close |

If you type something after ALT+TAB, releasing Alt no longer switches, so you can search instead.

## Features

What you type decides what shows up:

| Type | You get |
|------|---------|
| nothing | open windows, most recently used first, then your most launched apps |
| `firefox` | fuzzy search over windows, apps, system commands and plugins, best section first |
| `2^10/3`, `sqrt(2)*pi`, `(4+4)*12%5` | calculator, `Enter` copies the result |
| `> htop` | run a command in your terminal (stays open after it exits) or in the background |
| `kill zen` | a search plugin, `<prefix> <query>` |
| anything | a "Search on DuckDuckGo" result at the bottom |

All of it, in short:

- **Window switcher**: every Hyprland window with class and workspace, sorted by focus history.
  `Enter` focuses it and jumps to its workspace.
- **App launcher**: reads `.desktop` files from `~/.local/share/applications` and every
  `$XDG_DATA_DIRS` entry (flatpak too). Matches name, generic name, keywords and the binary name.
  `Terminal=true` apps open in `$TERMINAL`. Hidden and `NoDisplay` entries are skipped, and
  duplicates are merged.
- **Frecency**: everything you launch gets counted in `~/.local/share/zunder/history.json`, and
  things you use often rank higher.
- **Calculator**: `+ - * / ^ %`, parentheses, `sqrt sin cos tan ln exp abs floor ceil`,
  `pi` and `e`.
- **Shell**: `>` followed by any command. The first result runs it in a terminal, the second
  runs it silently in the background.
- **System**: Lock Screen, Suspend, Reboot, Shut Down, Log Out, Reload zunder, Quit zunder.
- **Web search**: fallback for any query.
- **Plugins**: scripts in any language, see below.
- **Theme**: everything is one CSS file you can override.

Apps are rescanned every time the launcher opens, so new installs show up right away.

## Plugins

A plugin is any executable file in `~/.config/zunder/plugins/`. A few comments near the top
configure it:

```sh
#!/bin/sh
# @zunder.name   Kill Process
# @zunder.prefix kill
# @zunder.glyph  
# @zunder.mode   search
```

| Key | Default | What it does |
|-----|---------|--------------|
| `name` | file name | shown in search and as the section title |
| `prefix` | file name | typing `<prefix> ` (with the space) hands the query to the plugin |
| `glyph` | puzzle piece | Nerd Font icon, as the character itself or `\uXXXX` |
| `mode` | `search` | `search` or `command` |

**search** plugins are run as `script "<query>"` on every keystroke, in the background, and
results that come back late are thrown away. Each line on stdout is one result. A plain text line
gets copied when you press Enter, so `ls ~/notes` or `cat list.txt` already work as plugins. For
more control, print JSON instead:

```json
{"title": "zen-bin", "subtitle": "pid 4242", "hint": "6% · 712 MB", "action": {"run": "kill 4242"}}
```

| Action | Enter does |
|--------|------------|
| `{"run": "cmd"}` | runs `cmd` with `sh -c`, in the background |
| `{"term": "cmd"}` | runs `cmd` in a terminal |
| `{"open": "url or path"}` | `xdg-open` |
| `{"copy": "text"}` | copies to the clipboard |
| `{"focus": "0x55a3…"}` | focuses a Hyprland window by address |
| `{"fill": "text"}` | replaces the query and keeps zunder open, for menus inside a plugin |

`glyph` can be set per line too. If the script fails without printing anything, its last stderr
line is shown as the result.

**command** plugins show up in the normal search under their name, and `Enter` runs them once.
Good for one-off actions like "copy my IP" or "toggle VPN".

Two examples are in [`examples/plugins`](examples/plugins):

- `kill`: search your processes by name, sorted by CPU, `Enter` kills one
- `ip`: command plugin, copies your local IP

```sh
mkdir -p ~/.config/zunder/plugins
ln -s $PWD/examples/plugins/* ~/.config/zunder/plugins/
zunder reload
```

To give a plugin its own key, open zunder with the prefix already typed:

```lua
hl.bind("SUPER + X", hl.dsp.exec_cmd("zunder show 'kill '"))
```

## Theme

The look is [`src/style.css`](src/style.css), a graphite palette with a lavender accent.
`~/.config/zunder/style.css` is loaded on top of it, so only write what you want to change:

```css
@define-color accent #f9e2af;
@define-color bg     #11111b;

.panel { font-family: "FiraCode Nerd Font"; font-size: 12pt; }
```

| Color | Used for |
|-------|----------|
| `bg` | panel background |
| `fg` | titles and the query |
| `muted` | key hints, selected hint |
| `faint` | subtitles, placeholder, glyphs |
| `ghost` | border, separators, row markers |
| `accent` | prompt, section titles, selection marker |
| `sel` / `hover` | selected / hovered row |

The overlay covers the whole screen and dims it (`window.zunder`). Restart the daemon after
changing the CSS.

## Adding a built-in

For things that need to be fast or keep state, add a provider in Rust. Each one is a file in
`src/providers/` that implements this trait from `src/engine.rs`:

```rust
pub trait Provider {
    fn title(&self) -> &str;                            // section title
    fn search(&self, cx: &Ctx) -> Vec<Item>;            // results for cx.query
    fn exclusive(&self, _query: &str) -> bool { false } // true = only show this section
    fn reload(&mut self) {}                             // called when zunder opens
}
```

`cx.score(text)` does the fuzzy matching, `cx.windows` has the open windows and `cx.history` the
launch counts. `Item.score` sorts results inside a section, and the best score decides the order
of sections. Register the new provider in `providers::all()` in `src/providers/mod.rs`.
[`calc.rs`](src/providers/calc.rs) is the shortest one to copy from.

## How it works

```
keybind ─► zunder switch ─► $XDG_RUNTIME_DIR/zunder.sock ─► daemon
                                                              │
                     hyprctl clients -j ◄─────────────────────┤ on open
                     .desktop files     ◄─────────────────────┘
```

The daemon keeps GTK and the app index loaded, so opening it is instant. The CLI only writes a
word to the socket and exits. The overlay is a full-screen layer-shell surface on the overlay
layer with exclusive keyboard focus, and the panel is centered inside it. Windows are focused
with `hyprctl dispatch 'hl.dsp.focus({ window = "address:…" })'`, which is the Lua dispatch
syntax.

## Todo

- clipboard history (cliphist)
- emoji and symbol picker
- file search
- quicklinks with a `{query}` placeholder
- secondary actions on `Ctrl+Enter` (copy, open in terminal…)
- config file for terminal, search engine and panel size
