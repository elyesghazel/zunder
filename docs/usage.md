# Usage

## Keys

| Key | What it does |
|-----|--------------|
| `ALT+TAB` | open the switcher on the previous window |
| `Tab` while holding Alt | next window (`ALT+SHIFT+TAB` goes back) |
| release Alt | switch to the selected window |
| `↑` `↓`, `Tab` `Shift+Tab`, `Ctrl+J` `Ctrl+K`, `Ctrl+N` `Ctrl+P` | move |
| `Enter` or click | run the selected result |
| `Ctrl+F` | add to or remove from favorites |
| `Ctrl+B` | give the selected result a global shortcut |
| `Ctrl+,` | open the settings window (`Ctrl+.` works too) |
| `Esc` or click outside | close |

Once you type something after ALT+TAB, releasing Alt no longer switches, so you can search
instead. You can turn release-to-switch off in the settings.

## What you can type

| Type | You get |
|------|---------|
| nothing | favorites, open windows, recent and suggested apps |
| `firefox` | fuzzy search over windows, apps, commands and plugins |
| `2^10/3`, `sqrt(2)*pi` | calculator |
| `time in ny`, `tokyo time` | world clock |
| `time in 2h in london`, `in 90 min in la` | the time there, later |
| `3pm in ny`, `9am tokyo`, `3pm ny in zurich` | time conversion |
| `clip`, `clip ssh` | clipboard history |
| `history`, `history firefox` | everything you ran from zunder |
| `> htop` | run a command |
| `theme`, `theme nord` | color schemes, Enter applies live and keeps the list open |
| `settings` | quick settings in the launcher |
| `<plugin prefix> query` | a plugin, e.g. `kill zen` |
| anything | web search as the last result |

## Switcher

ALT+TAB lists every window, most recently focused first, with its app icon, class and workspace.
The previous window is preselected, so a quick ALT+TAB tap goes back and forth between two
windows like on any other desktop.

Next to the list is a screenshot of the selected window. zunder takes them with `grim -T` every
time the switcher opens, in the background, one thread per window. Windows on other workspaces
work too. Until the fresh screenshot is ready, you see the one from last time.

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
removes it. Calculator and clipboard results can't be favorites, they change too often.

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

## Clipboard

`clip` shows cliphist's history, newest first, images included. Text after `clip ` fuzzy searches
it. Enter copies the entry again. zunder reads cliphist and doesn't store anything itself, so
cliphist's own `-max-items` decides how far back it goes.

## Shell

`> cmd` gives two results: run in your terminal (stays open after the command exits, shows the
exit code) or run silently in the background.

## System

Lock Screen, Suspend, Reboot, Shut Down, Log Out, Reload zunder, Quit zunder, Zunder Settings,
Launch History, Clipboard History. Type part of the name.
