# Plugins

A plugin is any executable file in `~/.config/zunder/plugins/`: a shell script, Python, a
compiled binary, anything. zunder finds it when it opens, so there's nothing to register.

## Header

A few comments near the top (first 30 lines) configure it. All of them are optional.

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
| `glyph` | puzzle piece | Nerd Font icon, the character itself or `\uXXXX` ([cheat sheet](https://www.nerdfonts.com/cheat-sheet)) |
| `mode` | `search` | `search` or `command` |

## search mode

Typing `<prefix> <query>` runs `script "<query>"` on every keystroke, in a background thread.
Results from an older keystroke that arrive late are thrown away, so a slow plugin never shows
stale results.

Every line on stdout is one result, in order.

**Plain text**: Enter copies the line. That makes almost anything a plugin:

```sh
#!/bin/sh
# @zunder.prefix note
# @zunder.glyph  
grep -ri -- "$1" ~/notes | head -50
```

**JSON**: one object per line for full control:

```json
{"title": "zen-bin", "subtitle": "pid 4242", "hint": "6% · 712 MB", "glyph": "", "action": {"run": "kill 4242"}}
```

| Field | Required | What it is |
|-------|----------|------------|
| `title` | yes | main text |
| `subtitle` | no | dim text after the title |
| `hint` | no | right-aligned text |
| `glyph` | no | icon for this line, defaults to the plugin's |
| `icon` | no | icon theme name or absolute path, e.g. `firefox` |
| `image` | no | path to an image, shown in the preview pane when the line is selected |
| `preview` | no | text shown in the preview pane, e.g. a whole note or a command's output |
| `action` | no | what Enter does, see below. Without one, Enter does nothing |

| Action | Enter does |
|--------|------------|
| `{"run": "cmd"}` | runs `cmd` with `sh -c` in the background |
| `{"term": "cmd"}` | runs `cmd` in your terminal, which stays open after it exits |
| `{"open": "url or path"}` | `xdg-open` |
| `{"copy": "text"}` | copies to the clipboard |
| `{"focus": "0x55a3…"}` | focuses a Hyprland window by address |
| `{"fill": "text"}` | replaces the query and keeps zunder open, for menus and sub-pages |

Results get an id from the plugin name and title, so they can be favorites (Ctrl+F) and get
global shortcuts (Ctrl+B) like apps.

If the script exits non-zero without printing anything, the last line of its stderr is shown
as the result, so `echo "need an API key" >&2; exit 1` gives a readable error.

## command mode

```sh
#!/bin/sh
# @zunder.name  Toggle VPN
# @zunder.mode  command
# @zunder.glyph 
if nmcli -t connection show --active | grep -q wg0; then nmcli connection down wg0; else nmcli connection up wg0; fi
```

Command plugins show up in normal search under their name. Enter runs the script once, with no
arguments, detached.

## Examples

In [`examples/plugins`](../examples/plugins):

- `kill`: search your processes by name, sorted by CPU, Enter kills one
- `ip`: command plugin, copies your local IP

```sh
mkdir -p ~/.config/zunder/plugins
ln -s "$PWD"/examples/plugins/* ~/.config/zunder/plugins/
zunder reload
```

A few more ideas, each a handful of lines:

**GitHub repos** (needs `gh`):

```sh
#!/bin/sh
# @zunder.name   GitHub
# @zunder.prefix gh
# @zunder.glyph  
gh repo list --limit 100 --json nameWithOwner,description,url \
  --jq ".[] | select(.nameWithOwner | test(\"$1\"; \"i\")) |
        {title: .nameWithOwner, subtitle: (.description // \"\"), action: {open: .url}}" -c
```

**Quicklinks with a query**:

```python
#!/usr/bin/env python3
# @zunder.name   Quicklinks
# @zunder.prefix q
# @zunder.glyph  
import json, sys, urllib.parse
q = sys.argv[1] if len(sys.argv) > 1 else ""
links = {
    "yt": ("YouTube", "https://www.youtube.com/results?search_query={}"),
    "wiki": ("Wikipedia", "https://en.wikipedia.org/w/index.php?search={}"),
    "aur": ("AUR", "https://aur.archlinux.org/packages?K={}"),
    "crates": ("crates.io", "https://crates.io/search?q={}"),
}
key, _, rest = q.partition(" ")
for k, (name, url) in links.items():
    if k.startswith(key):
        print(json.dumps({"title": f"{name}: {rest}" if rest else name, "hint": k,
                          "action": {"open": url.format(urllib.parse.quote(rest))} if rest
                          else {"fill": f"q {k} "}}))
```

**A menu inside a plugin** with `fill`: the first level prints `{"title": "Wi-Fi", "action":
{"fill": "net wifi "}}`, and the script handles `wifi …` on the next call.

## Give a plugin its own key

```lua
hl.bind("SUPER + X", hl.dsp.exec_cmd("zunder show 'kill '"))
```

Or select the plugin in zunder and press Ctrl+B.

## Tips

- Keep it fast: it runs on every keystroke. Cache slow lookups in a file under `/tmp`.
- Print at most a few hundred lines; zunder doesn't truncate plugin output.
- `zunder reload` picks up new plugins and header changes immediately. Script changes don't need
  it, the file runs fresh every time.
