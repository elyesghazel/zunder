# zunder

A launcher and ALT+TAB window switcher for Hyprland that looks like a terminal.

![zunder](assets/screenshot.png)

zunder runs as a small daemon. A keybind sends it a command over a unix socket, and it shows a
layer-shell overlay on top of everything: one prompt, results grouped in sections, monospace,
colored app icons. Open windows, apps, a calculator, a world clock, clipboard history, shell
commands, power actions and your own scripts all live in the same list. ALT+TAB shows a live
screenshot of each window, even ones on other workspaces. Anything in the list can be pinned to
the start screen or get its own global shortcut. Plugins are just executables that print lines,
so you can write them in whatever language you like.

Written in Rust with GTK4 and gtk4-layer-shell. Built and tested on Arch with Hyprland (Lua config).

## Install

Needs `gtk4`, `gtk4-layer-shell`, `wl-clipboard` and a Nerd Font. `grim` 1.5+ for window
previews and `cliphist` for clipboard history are optional. JetBrainsMono is the default font,
change it in the settings if you use another one.

```sh
sudo pacman -S --needed rust gtk4 gtk4-layer-shell wl-clipboard ttf-jetbrains-mono-nerd grim cliphist

git clone https://github.com/elyesghazel/zunder
cd zunder
cargo build --release
install -Dm755 target/release/zunder ~/.local/bin/zunder
```

Then start the daemon with Hyprland and bind the keys. In a Lua config:

```lua
-- conf/autostart.lua
hl.exec_cmd("zunder server")
hl.exec_cmd("wl-paste --type text --watch cliphist store")   -- for clipboard history

-- conf/binds.lua
hl.bind("ALT + TAB",         hl.dsp.exec_cmd("zunder switch"))
hl.bind("ALT + SHIFT + TAB", hl.dsp.exec_cmd("zunder switch-prev"))
hl.bind("ALT + SPACE",       hl.dsp.exec_cmd("zunder toggle"))
hl.bind("SUPER + V",         hl.dsp.exec_cmd("zunder show 'clip '"))

-- hyprland.lua, after the binds: shortcuts you set inside zunder
pcall(require, "zunder")
```

Old `hyprland.conf` style works the same way: `exec-once = zunder server`,
`bind = ALT, TAB, exec, zunder switch` and `source = ~/.config/hypr/zunder.conf`.

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
zunder settings         # open the settings page
zunder reload           # reread config.toml, rescan apps and plugins
zunder run <key>        # run a saved shortcut (the generated binds call this)
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
| `Ctrl+F` | add to / remove from favorites |
| `Ctrl+B` | give it a global shortcut (then press the keys, `⌫` removes it, `Esc` cancels) |
| `Esc` or click outside | close |

If you type something after ALT+TAB, releasing Alt no longer switches, so you can search instead.

## Features

What you type decides what shows up:

| Type | You get |
|------|---------|
| nothing | favorites, open windows, then your most launched apps topped up with suggestions |
| `firefox` | fuzzy search over windows, apps, system commands and plugins, best section first |
| `2^10/3`, `sqrt(2)*pi`, `(4+4)*12%5` | calculator, `Enter` copies the result |
| `time in ny`, `tokyo time`, `time in 2h in london` | the time there, with offset and how far ahead or behind you it is |
| `3pm in ny`, `9am tokyo`, `3pm ny in zurich` | convert a time between places, "in" alone means from here |
| `clip`, `clip ssh` | clipboard history, `Enter` copies the entry again |
| `history`, `history firefox` | everything you ever ran from zunder, newest first |
| `settings` | the settings page |
| `theme`, `theme nord` | color schemes, `Enter` switches and keeps the list open |
| `> htop` | run a command in your terminal (stays open after it exits) or in the background |
| `kill zen` | a search plugin, `<prefix> <query>` |
| anything | a "Search on DuckDuckGo" result at the bottom |

All of it, in short:

- **Window switcher**: every Hyprland window with its app icon, class and workspace, sorted by
  focus history. `Enter` focuses it and jumps to its workspace. In ALT+TAB mode a screenshot of
  the selected window sits next to the list. They're taken with `grim -T` in the background every
  time the switcher opens, so windows on other workspaces work too, and the last screenshot shows
  until the new one is ready.
- **App launcher**: reads `.desktop` files from `~/.local/share/applications` and every
  `$XDG_DATA_DIRS` entry (flatpak too). Matches name, generic name, keywords and the binary name.
  `Terminal=true` apps open in `$TERMINAL`. Hidden and `NoDisplay` entries are skipped, and
  duplicates are merged.
- **Start screen**: favorites, open windows and apps. Apps you launch go first; before you have
  any history it suggests browsers, terminals, file managers, editors and chat apps from their
  desktop categories, so it's never empty.
- **Frecency**: everything you launch gets counted in `~/.local/share/zunder/history.json`, and
  things you use often rank higher.
- **Favorites**: `Ctrl+F` on any app, command, plugin result or shell command pins it to the top
  of the start screen. Saved in `~/.config/zunder/favorites.json`.
- **Global shortcuts**: `Ctrl+B` on any result, then press e.g. `SUPER+F2`. zunder writes
  `~/.config/hypr/zunder.lua` (or `zunder.conf`) and reloads Hyprland. The shortcut works from
  anywhere, zunder doesn't have to be open. Results with a shortcut show it on the right. Pick
  combos Hyprland doesn't already use, because those never reach zunder.
- **Launch history**: every launch is appended to `~/.local/share/zunder/launches.jsonl` and
  nothing ever deletes it. `history` searches it, `Enter` runs an entry again.
- **World clock**: `time in <place>` for 400+ IANA cities plus short names like `ny`, `la`, `sf`,
  `uk`, `tunisia`, `cet`. Add a duration (`2h`, `1h30m`, `3 days`) to look ahead, or convert a
  clock time between places.
- **Clipboard**: `clip` lists cliphist history, images included, fuzzy searchable.
- **Calculator**: `+ - * / ^ %`, parentheses, `sqrt sin cos tan ln exp abs floor ceil`,
  `pi` and `e`.
- **Shell**: `>` followed by any command. The first result runs it in a terminal, the second
  runs it silently in the background.
- **System**: Lock Screen, Suspend, Reboot, Shut Down, Log Out, Reload zunder, Quit zunder.
- **Web search**: fallback for any query, the engine is a setting.
- **Plugins**: scripts in any language, see below.
- **Settings**: `settings` opens a settings page inside zunder, see below.
- **Themes**: `theme` switches between twelve color schemes, your own are a small CSS file.

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

## Settings

Type `settings` (or run `zunder settings`). The page is grouped like the list below. `Enter`
toggles an on/off setting. For values, `Enter` puts `settings <key> ` in the prompt; type the new
value and press `Enter` again, e.g. `settings font_size 13`.

| Group | Key | Default | What it does |
|-------|-----|---------|--------------|
| Switcher | `alt_release` | on | releasing Alt switches to the selected window |
| | `previews` | on | window screenshots in ALT+TAB |
| Start screen | `start_windows` | on | list open windows when the query is empty |
| | `start_apps` | 24 | how many recent and suggested apps |
| Appearance | `icons` | on | colored app icons instead of glyphs |
| | `font` | JetBrainsMono Nerd Font | any installed font |
| | `font_size` | 11 | in pt |
| | `theme` | graphite | color scheme, see [Themes](#themes) |
| | `accent` | theme | hex color like `#f9e2af` |
| | `dim` | 0.35 | backdrop darkness, 0 to 1 |
| | `width` / `height` | 820 / 460 | panel width, result list height |
| General | `terminal` | `$TERMINAL`, then kitty | for `>` commands and terminal apps |
| | `search_url` | DuckDuckGo | web search, `{q}` is the query |
| Providers | | all on | turn any section off, e.g. Web or System |

Under Files there are shortcuts to open `config.toml`, `style.css` and the plugins folder.
Everything is saved to `~/.config/zunder/config.toml`, which you can also edit by hand and then
`zunder reload`:

```toml
alt_release = true
previews = true
icons = true
font = "JetBrainsMono Nerd Font"
font_size = 11.0
accent = "#f9e2af"
search_url = "https://www.google.com/search?q={q}"
disabled = ["Web"]
```

## Files

| Path | What |
|------|------|
| `~/.config/zunder/config.toml` | settings |
| `~/.config/zunder/style.css` | your CSS on top of the theme |
| `~/.config/zunder/themes/` | your own color themes, one `<name>.css` each |
| `~/.config/zunder/plugins/` | plugins |
| `~/.config/zunder/favorites.json` | favorites |
| `~/.config/zunder/binds.json` | global shortcuts, the source for the generated Hyprland file |
| `~/.config/hypr/zunder.lua` | generated binds, don't edit (`zunder.conf` for classic configs) |
| `~/.local/share/zunder/history.json` | launch counts for ranking |
| `~/.local/share/zunder/launches.jsonl` | the full launch history, one JSON line per launch |
| `$XDG_RUNTIME_DIR/zunder-previews/` | window screenshots, gone after a reboot |

## Themes

Type `theme` to list the color schemes. `Enter` applies one right away and keeps the list open,
so you can step through them. `theme` in the settings (or `theme = "nord"` in `config.toml`)
does the same.

Built in: `graphite` (the default), `catppuccin-mocha`, `catppuccin-latte` (light), `rose-pine`,
`rose-pine-dawn` (light), `gruvbox-dark`, `tokyo-night`, `nord`, `everforest-dark`, `dracula`,
`kanagawa` and `one-dark`.

Your own theme is a file in `~/.config/zunder/themes/`, named after the theme, with one
`@define-color` line per color from the table below. It shows up in the list next to the
built-ins, and a file with a built-in's name replaces that built-in. Colors you leave out stay
graphite.

```css
/* ~/.config/zunder/themes/solarized.css */
@define-color bg     #002b36;
@define-color fg     #eee8d5;
@define-color muted  #93a1a1;
@define-color faint  #657b83;
@define-color ghost  #2a4a54;
@define-color accent #b58900;
@define-color green  #859900;
@define-color sel    #0d3a46;
@define-color hover  #07323d;
```

The `accent` setting wins over the theme's accent.

## Custom CSS

The layout is [`src/style.css`](src/style.css), and every color in it is one of the names below.
`~/.config/zunder/style.css` is loaded on top of the theme, so only write what you want to change:

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
| `green` | not used by the built-in CSS, there for your own rules |
| `sel` / `hover` | selected / hovered row |

The overlay covers the whole screen and dims it (`window.zunder`). Font, size, accent and dim are
easier to change in the settings. Theme changes apply right away; restart the daemon after
changing `style.css`.

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
of sections. Give items an `id` so they can be favorited and bound to a key, and an `icon` to
show an app icon. Setting `group` splits one provider's results into several sections. Register the new provider in `providers::all()` in `src/providers/mod.rs`.
[`calc.rs`](src/providers/calc.rs) is the shortest one to copy from.

## How it works

```
keybind ─► zunder switch ─► $XDG_RUNTIME_DIR/zunder.sock ─► daemon
                                                              │
                     hyprctl clients -j ◄─────────────────────┤ on open
                     .desktop files     ◄─────────────────────┤
                     grim -T <window>   ◄─────────────────────┘ ALT+TAB only, in threads
```

The daemon keeps GTK and the app index loaded, so opening it is instant. The CLI only writes a
word to the socket and exits. The overlay is a full-screen layer-shell surface on the overlay
layer with exclusive keyboard focus, and the panel is centered inside it. Windows are focused
with `hyprctl dispatch 'hl.dsp.focus({ window = "address:…" })'`, which is the Lua dispatch
syntax. Window previews use the window's `stableId` from `hyprctl clients -j`, which is the
identifier grim 1.5 takes for `-T`.

## Todo

- emoji and symbol picker
- file search
- quicklinks with a `{query}` placeholder
- secondary actions on `Ctrl+Enter` (copy, open in terminal…)
- paste right away after picking a clipboard entry
