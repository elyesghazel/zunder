# Troubleshooting

## Nothing happens when I press the key

```sh
zunder toggle
```

- `server not reachable`: the daemon isn't running. Start it with `zunder server` and check the
  output. Make sure `hl.exec_cmd("zunder server")` is in your autostart.
- It opens from the terminal but not from the key: Hyprland can't find the binary. Hyprland's
  `PATH` doesn't always include `~/.local/bin`. Use the full path in the bind
  (`/home/you/.local/bin/zunder switch`) or install the package to `/usr/bin`.
- `hyprctl configerrors` shows typos in your binds.

## The daemon exits right away

Run `zunder server` in a terminal and read the error.

- `server already running`: another daemon owns the socket. `zunder quit` first.
- An error about layer shell, or the launcher shows up as a normal tiled window: your
  compositor doesn't support wlr-layer-shell, or gtk4-layer-shell isn't installed.

## ALT+TAB switches instantly / never switches

zunder switches when you release Alt. A very quick tap releases Alt before zunder even has
keyboard focus; it checks the modifier state when it gets focus and switches right away then.
If that misfires on your setup, turn off "Release Alt to switch" in the settings (Switcher page)
and use Enter instead, and open an issue.

## No window previews

- `grim --version` must be 1.5 or newer (it added `-T`).
- Hyprland must be recent enough to report `stableId` in `hyprctl clients -j`.
- Test by hand: `grim -T <stableId> /tmp/test.png`.

## No app icons, or the wrong ones

Icons come from your GTK icon theme (`gtk-icon-theme-name` in `~/.config/gtk-4.0/settings.ini`).
Window icons are matched by window class against `StartupWMClass` and the desktop file name. If
an app doesn't set either, zunder shows its glyph. Turn icons off in the settings if your theme
looks wrong at small sizes.

## Glyphs show as boxes

Install a Nerd Font (`ttf-jetbrains-mono-nerd`) or set `font` in the settings to one you have.

## Clipboard history is empty

cliphist only knows what was copied while `wl-paste --watch cliphist store` was running. Check
with `cliphist list | head`.

## Shortcuts from Ctrl+B don't work

- Lua config: is `pcall(require, "zunder")` in `hyprland.lua`, after your binds?
- Classic config: is `source = ~/.config/hypr/zunder.conf` there?
- Does `~/.config/hypr/zunder.lua` contain the bind? `hyprctl binds | grep zunder` should list it.
- The combo might already be bound elsewhere in Hyprland. Hyprland's own binds win.

## Theme or CSS looks off

Move `~/.config/zunder/style.css` away and restart the daemon to see the built-in look. A GTK
theme in `~/.config/gtk-4.0/gtk.css` can leak into some widgets; zunder loads its CSS at user
priority to win over it.

## Reporting a bug

Open an issue with `zunder --version`, `hyprctl version | head -1`, your distro, and the output
of `zunder server` run in a terminal while reproducing it.
