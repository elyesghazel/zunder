# Usage

## At a glance

| | |
|---|---|
| **Window switcher** | ALT+TAB as a grid of live window thumbnails, any workspace |
| **Overview** | SUPER+TAB: every window grouped by workspace; close, move and minimize from the keyboard |
| **Actions** | `Ctrl+K` on any result: everything besides Enter, like open folder, copy path, close window |
| **App launcher** | every `.desktop` app (flatpak too), fuzzy matched on name, keywords and binary |
| **Start screen** | favorites, open windows, your most used apps, suggestions so it's never empty |
| **Favorites** | `Ctrl+F` pins any result to the start screen |
| **Global shortcuts** | `Ctrl+B` gives any result a system-wide shortcut |
| **Launch history** | `history`: everything you ever ran, searchable, never cleared |
| **World clock** | `time in ny`, `time in 2h in tokyo`, `3pm ny in zurich` |
| **Calculator** | `2^10/3`, `sqrt(2)*pi`, Enter copies |
| **Conversion** | `5 km in mi`, `30c`, `2 gb in mib`, `100 usd in chf`: units and 160 currencies |
| **Files** | files from your home folder show up as you type; `f name` for only files, images get a preview |
| **Emoji** | `fire` already shows 🔥; `:heart` for the whole picker |
| **Clipboard** | `clip`: clipboard history with fuzzy search; full text or image on the right; `Ctrl+F` pins a clip |
| **Shell** | `> cmd` runs in your terminal or in the background |
| **System** | lock, suspend, reboot, shut down, log out |
| **Other** | fallback for anything: web search (engine of your choice), or Ask AI when an `ai` plugin is installed |
| **Plugins** | any script that prints lines, e.g. `kill zen` |
| **Themes** | 12 built in (catppuccin, rose-pine, gruvbox, tokyo-night, nord…), or your own |
| **Settings** | a settings window with a sidebar, plus `settings` inside the launcher |

## Keys

| Key | What it does |
|-----|--------------|
| `ALT+TAB` | open the switcher on the previous window |
| `Tab` while holding Alt | next window (`ALT+SHIFT+TAB` goes back) |
| release Alt | switch to the selected window |
| `↑` `↓`, `Tab` `Shift+Tab`, `Ctrl+J` `Ctrl+P`, `Ctrl+N` | move |
| `Enter` or click | run the selected result |
| `Ctrl+K` | the actions of the selected result; `Esc` goes back |
| `Ctrl+W` `Ctrl+M` `Ctrl+1…9` | on a window: close, minimize, move to that workspace |
| `Ctrl+Enter` | on a window: bring it to the workspace you're on |
| `Ctrl+F` | add to or remove from favorites |
| `Ctrl+B` | give the selected result a global shortcut |
| `Ctrl+,` | open the settings window (`Ctrl+.` works too) |
| `Esc` or click outside | close |

Once you type something after ALT+TAB, releasing Alt no longer switches, so you can search
instead. You can turn release-to-switch off in the settings.

## Commands

```sh
zunder                  # toggle; starts the daemon first if it isn't running
zunder setup            # keybinds, autostart and theme in the terminal (--dry-run, --yes)
zunder show setup       # the same as a page in the launcher
zunder server           # run the daemon (autostart)
zunder toggle           # show / hide
zunder show [query]     # show, optionally with the query typed, e.g. zunder show 'time in '
zunder hide
zunder switch           # window switcher, or the next window if it's open
zunder switch-prev      # same, backwards
zunder overview         # every window by workspace, stays open until you pick one (SUPER+TAB)
zunder settings [page]  # settings window, e.g. zunder settings appearance
zunder reload           # reread config.toml, rescan apps and plugins
zunder import vicinae   # take over vicinae's favorites and launch counts
zunder run <key>        # run a saved shortcut (the generated binds call this)
zunder quit             # stop the daemon
zunder --version
```

## What you can type

| Type | You get |
|------|---------|
| nothing | favorites, open windows, recent and suggested apps |
| `firefox` | fuzzy search over windows, apps, commands and plugins |
| `2^10/3`, `sqrt(2)*pi` | calculator |
| `5 km in mi`, `30c`, `100 usd in chf` | unit and currency conversion |
| `readme`, `f readme` | files |
| `fire`, `:heart` | emoji |
| `time in ny`, `tokyo time` | world clock |
| `time in 2h in london`, `in 90 min in la` | the time there, later |
| `3pm in ny`, `9am tokyo`, `3pm ny in zurich` | time conversion |
| `clip`, `clip ssh` | clipboard history |
| `history`, `history firefox` | everything you ran from zunder |
| `> htop` | run a command |
| `theme`, `theme nord` | color schemes, Enter applies live and keeps the list open |
| `settings` | quick settings in the launcher |
| `setup` | keys, autostart and switching over from another launcher, then Apply |
| `update` | check for a new zunder now (the start screen also says so once a day) |
| `<plugin prefix> query` | a plugin, e.g. `kill zen` |
| anything | web search as the last result |

## Switcher

ALT+TAB shows a grid of thumbnails, one per window, most recently focused first, with the app
icon and title underneath. The previous window is preselected, so a quick ALT+TAB tap goes back
and forth between two windows like on any other desktop. `Tab` and the arrow keys move, a click
switches, and typing anything turns the grid into the normal searchable list.

## Overview

`zunder overview` (SUPER+TAB after `setup`) shows every window like Mission Control: one column
per workspace, side by side, with thumbnails. It stays open until you pick a window, so you can
tidy up first:

| Key | What it does |
|-----|--------------|
| arrows | move to the nearest window in that direction |
| `Enter` or click | focus it |
| `Ctrl+Enter` | bring it to the workspace you're on |
| `Ctrl+W` or the `✕` on the thumbnail | close it |
| `Ctrl+M` | minimize it (to the `special:minimized` workspace) |
| `Ctrl+1…9` | move it to that workspace, without following it |
| `Ctrl+K` | every other action |
| typing | search instead, like the launcher |

Closing and moving keep the overview open and redraw it. The same keys work in the ALT+TAB grid
and on windows in the search results.

## Actions

`Ctrl+K` on a result lists what else you can do with it. Type to filter, `Enter` runs one, `Esc`
goes back to your results.

| Result | Actions |
|--------|---------|
| window | focus, bring here, fullscreen, minimize, close, move to workspace 1–9, copy title or class |
| file or folder | open, open containing folder, open a terminal there, copy path, copy name |
| app | launch, launch in a terminal, open or copy its desktop file, copy the command |
| command | run, run in a terminal, copy it |
| link | open, copy link |
| anything with an id | add to favorites, set a global shortcut |

Plugins add their own with an `actions` list, see [plugins](plugins.md).

zunder takes the screenshots with `grim -T` every time the switcher opens, in the background,
one thread per window. Windows on other workspaces work too. Until the fresh screenshot is
ready, you see the one from last time, or the app icon if there is none yet.

## Start screen

With an empty query you get:

1. **Favorites**, in the order you added them
2. **Windows**, most recent first
3. **Apps**: the ones you launch most, topped up with suggestions (browsers, terminals, file
   managers, editors, chat apps) so it isn't empty on day one

All three can be turned off or resized in the settings.

## Favorites

Select anything with an id (apps, system commands, plugin results, shell commands, settings
entries) and press Ctrl+F. It shows up at the top of the start screen with a ★. Ctrl+F again
removes it. Calculator results can't be favorites, they change too often. Clipboard entries
can: see [Clipboard](#clipboard).

## Global shortcuts

Select a result, press Ctrl+B, then press the shortcut you want, e.g. SUPER+F2. zunder:

1. saves it in `~/.config/zunder/binds.json`
2. writes `~/.config/hypr/zunder.lua` with an `hl.bind(...)` line that runs `zunder run <id>`
3. runs `hyprctl reload`

The shortcut works anywhere, zunder doesn't need to be open. The result now shows the shortcut on
its right. Ctrl+B then Backspace removes it, and the Shortcuts page in the settings lists all of
them.

A shortcut needs Super, Ctrl or Alt, or has to be an F-key. Pick one Hyprland doesn't use yet:
keys Hyprland already binds never reach zunder.

A shortcut on a plugin or `settings` entry opens zunder with that prefix typed.

On Windows and macOS there is no config file: zunder registers the shortcuts with the system
itself while it runs. A key another program already has can't be taken; zunder says so at the
bottom. On macOS, SUPER is ⌘ and ALT is Option.

## Launch history

Every launch is appended to `~/.local/share/zunder/launches.jsonl`. Nothing trims it. `history`
lists it newest first with the time, `history <text>` searches it, and Enter runs an entry again.

## World clock

Places are every IANA city (`tokyo`, `zurich`, `sao paulo`, `ho chi minh`), plus countries and
short names: `ny`, `nyc`, `la`, `sf`, `dc`, `uk`, `uae`, `nz`, `hk`, `tunisia`, `switzerland`,
`est`, `pst`, `cet`, `utc` and more. Typing the first three letters of a city works too.

Durations: `2h`, `90 min`, `1h30m`, `2 hours and 15 minutes`, `3 days`.

Times: `3pm`, `3:30 pm`, `15:00`, `noon`, `midnight`. A bare number like `5` isn't a time.

- `time in ny` → current time in New York, its UTC offset, how far ahead/behind you it is
- `time in 2h in ny` → the time there in 2 hours
- `3pm in ny` → when it's 3pm here, what time is it in New York
- `3pm ny` → 3pm in New York, in your time
- `3pm ny in tokyo` → 3pm in New York, in Tokyo time

Enter copies the time.

## Calculator

`+ - * / ^ %`, parentheses, `sqrt sin cos tan ln exp abs floor ceil`, `pi`, `e`. A leading `=`
is fine. Enter copies the result.

## Conversion

`<number> <unit> [in|to|as <unit>]`. Without a target, metric goes to imperial and back (`10 km`
→ miles, `30c` → °F, `5 lb` → kg). Enter copies the number.

| Kind | Units |
|------|-------|
| length | mm cm m km in ft yd mi nmi |
| mass | mg g kg t oz lb st |
| volume | ml cl dl l m3 tsp tbsp floz cup pt qt gal |
| area | cm2 m2 km2 ha in2 ft2 acre mi2 |
| speed | m/s km/h mph kn |
| time | ms s min h day week year |
| data | bit B KB MB GB TB KiB MiB GiB TiB |
| energy | J kJ cal kcal Wh kWh MJ |
| temperature | c f k (also °C, celsius…) |

Full names and plurals work too (`5 miles in kilometers`), and so does `1,5` for `1.5`.

**Currency**: ISO codes (`usd`, `chf`, `tnd`…), symbols (`$ € £ ¥`) and a few words (`dollars`,
`euros`, `francs`). `100 usd` without a target shows your currency and dollars (euros for
dollars). Your currency is the `currency` setting, or guessed from your time zone. Rates come
from open.er-api.com, are cached in `~/.cache/zunder/rates.json` and refreshed in the
background every 12 hours. The very first conversion needs internet. "pound" is a weight; use
`£` or `gbp` for money.

## Files

From 3 letters on, the best 5 files and folders from your home folder show up under every search
(only for queries starting with a letter). `f <name>` or `file <name>` shows only files, up to 60.
More words narrow it down by path: `f readme zunder`.

It uses `fd`, so hidden files and anything in a `.gitignore` are skipped, plus `node_modules`,
`target`, `go/pkg`, `venv`, `vendor`, `dist` and `build`. Exact name matches and shallow paths
rank first. Enter opens the file with its default app. Files can be favorites and get shortcuts.

Image results (`f png`, `f wallpaper`, or images among the inline results) open a preview pane
with the picture, its size and format.

When `fd` isn't installed, `f <name>` shows an **Install fd** result instead: Enter opens a
terminal with the right command for your system (`pacman`, `apt`, `dnf`, `zypper`, `winget` or
`brew`). `clip` does the same for cliphist.

## Emoji

From 3 letters on, the 3 best emoji show up in every search (`fire` → 🔥). `:name` or
`emoji name` opens the whole picker; `:` alone lists them all. Words match the start of the name
or the GitHub shortcode: `:red hea` → ❤️. Enter copies.

## Clipboard

`clip` shows your clipboard history, newest first, images included. Text after `clip ` fuzzy
searches it. Enter copies the entry again. The pane on the right shows the selected entry in full:
the whole text, scrollable and wrapped, or the image.

Where the history comes from:

- **Linux:** cliphist. zunder doesn't keep a history of its own there, so cliphist's own
  `-max-items` decides how far back it goes. Entries are decoded the first time you select them,
  so the list itself opens instantly.
- **Windows and macOS:** zunder keeps its own while the server runs: text and images (as PNG),
  the last 300 entries, in `clipboard` under zunder's data folder (`%APPDATA%\zunder` on Windows,
  `~/Library/Application Support/zunder` on macOS). Copying something again moves it to the top.
  Turn it off with `clipboard_history = false` or in Settings → General.

On Windows, **Win+V** opens it in place of the Windows clipboard panel. To get the Windows one
back, turn off "Win+V opens zunder" in Settings → General (`win_v = false`).

What password managers mark as private is never recorded: on Windows content flagged for
clipboard history tools (`ExcludeClipboardContentFromMonitorProcessing`,
`CanIncludeInClipboardHistory` = 0, `Clipboard Viewer Ignore`), on macOS the
`org.nspasteboard.ConcealedType` and `TransientType` markers. On Linux that's up to cliphist.

Ctrl+F pins an entry. zunder saves its content (text in `favorites.json`, images in
`~/.local/share/zunder/clips`), so it stays even after the history drops it. Pinned clips sit at the
top of `clip` and on the start screen with your other favorites; Enter copies them again, Ctrl+F
unpins.

## Shell

`> cmd` gives two results: run in your terminal (stays open after the command exits, shows the
exit code) or run silently in the background.

## System

Lock Screen, Suspend, Reboot, Shut Down, Log Out, Reload zunder, Quit zunder, Zunder Settings,
Launch History, Clipboard History. Type part of the name.
