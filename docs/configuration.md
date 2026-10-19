# Configuration

## Settings window

`zunder settings`, or select "Zunder Settings" in the launcher. `zunder settings appearance` opens
straight on a page. Changes are saved right away and most apply immediately.

| Page | What's on it |
|------|--------------|
| General | terminal, web search engine, currency |
| Switcher | release Alt to switch, window previews |
| Start screen | show windows, number of apps |
| Appearance | icons, font, font size, theme, accent, backdrop dim, width, height |
| Providers | turn sections like Web, System or Clipboard on and off |
| Favorites | everything you pinned, with remove buttons |
| Shortcuts | every global shortcut, with remove buttons |
| Plugins | installed plugins, how to call them, open the folder |
| About | version, open config.toml and style.css |

There is also a quick version inside the launcher: type `settings`. Enter toggles on/off
settings; for values, Enter types `settings <key> ` and you add the value, e.g.
`settings width 900`.

## config.toml

`~/.config/zunder/config.toml`. Everything is optional; missing keys use the default. After
editing by hand, run `zunder reload`.

```toml
theme = "graphite"                           # see `theme` in the launcher for the list
terminal = ""                                # empty = $TERMINAL, then kitty
search_url = "https://duckduckgo.com/?q={q}"
currency = ""                                # e.g. "CHF", empty = guessed from your time zone
font = "JetBrainsMono Nerd Font"
font_size = 11.0
width = 820                                  # panel width, ALT+TAB adds 420 for the preview
height = 460                                 # result list height
dim = 0.35                                   # backdrop, 0 = clear, 1 = black
accent = ""                                  # e.g. "#f9e2af", empty = theme accent
icons = true
previews = true
alt_release = true
start_windows = true
start_apps = 24
disabled = []                                # e.g. ["Web", "System"]
hide_windows = []                            # window classes to leave out, e.g. ["teams-for-linux"]
```

| Key | Type | What it does |
|-----|------|--------------|
| `theme` | text | color scheme: a built-in or a file in `~/.config/zunder/themes/` |
| `terminal` | text | terminal for `>` commands and `Terminal=true` apps, called as `<terminal> -e sh -c …` |
| `search_url` | text | web search fallback, `{q}` is replaced by the URL-encoded query |
| `currency` | text | your currency for conversions, e.g. `CHF` |
| `font` | text | panel font, any installed family |
| `font_size` | number | in pt |
| `width` | number | panel width in px |
| `height` | number | height of the result list in px |
| `dim` | number | darkness of the backdrop |
| `accent` | text | overrides the theme's accent color |
| `icons` | on/off | app icons from your icon theme instead of glyphs |
| `previews` | on/off | window screenshots in ALT+TAB |
| `alt_release` | on/off | releasing Alt switches windows |
| `start_windows` | on/off | windows on the start screen |
| `start_apps` | number | apps on the start screen |
| `hide_windows` | list | window classes the switcher and search never show (`hyprctl clients` shows the class) |
| `disabled` | list | section titles to hide: `Calculator`, `Time`, `Convert`, `Windows`, `Apps`, `System`, `Shell`, `Clipboard`, `History`, `Themes`, `Emoji`, `Files`, `Web`, `Plugins` |

Search engines for `search_url`:

```
https://duckduckgo.com/?q={q}
https://www.google.com/search?q={q}
https://search.brave.com/search?q={q}
https://www.startpage.com/do/search?q={q}
https://kagi.com/search?q={q}
```

## Files

| Path | What |
|------|------|
| `~/.config/zunder/config.toml` | settings |
| `~/.config/zunder/style.css` | your CSS on top of the theme |
| `~/.config/zunder/themes/` | your own color themes |
| `~/.config/zunder/plugins/` | plugins |
| `~/.config/zunder/favorites.json` | favorites |
| `~/.config/zunder/binds.json` | global shortcuts, source of the Hyprland file |
| `~/.config/hypr/zunder.lua` / `zunder.conf` | generated, don't edit |
| `~/.local/share/zunder/history.json` | launch counts for ranking |
| `~/.local/share/zunder/launches.jsonl` | full launch history |
| `$XDG_RUNTIME_DIR/zunder.sock` | control socket |
| `$XDG_RUNTIME_DIR/zunder-previews/` | window screenshots, cleared on reboot |
| `~/.cache/zunder/rates.json` | exchange rates, refreshed every 12 hours |

The config folder is safe to keep in your dotfiles. Leave `~/.local/share/zunder` out, it's
machine-specific.

## CSS

The built-in look is [`src/style.css`](../src/style.css) for the launcher and
[`src/settings.css`](../src/settings.css) for the settings window. `~/.config/zunder/style.css`
loads on top of both, so you only write what you want to change. Restart the daemon after
editing it.

Colors are GTK named colors:

| Name | Used for |
|------|----------|
| `bg` | backgrounds |
| `fg` | titles, the query |
| `muted` | key hints, selected hints |
| `faint` | subtitles, placeholders, glyphs |
| `ghost` | borders, separators, row markers |
| `accent` | prompt, section titles, selection |
| `sel` / `hover` | selected / hovered rows |

Useful selectors:

| Selector | Element |
|----------|---------|
| `window.zunder` | the dimmed full-screen backdrop |
| `.panel` | the launcher box |
| `.prompt`, `.caret`, `entry.query`, `.status` | the top line |
| `.heading`, `.heading-title` | section titles |
| `list.results > row`, `.item`, `.marker`, `.glyph`, `.icon`, `.title`, `.sub`, `.hint` | result rows |
| `.preview`, `.preview-title`, `.preview-meta` | the ALT+TAB preview |
| `.footer`, `.key`, `.desc` | the key hints at the bottom |
| `window.zunder-settings`, `.side`, `.page`, `.setting` | the settings window |

Example: rounder panel, no footer, bigger rows.

```css
.panel  { border-radius: 16px; }
.footer { opacity: 0; min-height: 0; padding: 0; }
.item   { padding-top: 6px; padding-bottom: 6px; }
```
