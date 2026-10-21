# Setup

```sh
zunder setup              # asks, defaults on Enter
zunder setup --dry-run    # shows every change, writes nothing
zunder setup --yes        # takes every default
```

It finds your Hyprland config (Lua or classic) and:

- switches over from vicinae, rofi, wofi, fuzzel, walker, anyrun, ulauncher or tofi: their
  daemons get commented out, and the key that opened them now opens zunder (each changed file
  keeps a `.bak`)
- imports vicinae's favorites and launch counts
- binds ALT+TAB, a launcher key and SUPER+V, telling you when a key is already taken
- starts cliphist with Hyprland if it isn't running
- picks a theme and adds the example plugins

Its own lines go to `~/.config/hypr/zunder-setup.lua` (or `.conf`), loaded by one line it adds
to `hyprland.lua`. Running it again is safe: it rewrites only that file and doesn't add things
your config already has. Everything below is what it does, for doing it by hand.

zunder is a daemon plus a CLI that talks to it. Hyprland starts the daemon and the keybinds call
the CLI. Opening is instant because the daemon already has everything loaded.

## Hyprland, Lua config

```lua
-- conf/autostart.lua
hl.on("hyprland.start", function()
    hl.exec_cmd("zunder server")
    hl.exec_cmd("wl-paste --type text --watch cliphist store")    -- clipboard history
    hl.exec_cmd("wl-paste --type image --watch cliphist store")
end)

-- conf/binds.lua
hl.bind("ALT + TAB",         hl.dsp.exec_cmd("zunder switch"))
hl.bind("ALT + SHIFT + TAB", hl.dsp.exec_cmd("zunder switch-prev"))
hl.bind("ALT + SPACE",       hl.dsp.exec_cmd("zunder toggle"))
hl.bind("SUPER + V",         hl.dsp.exec_cmd("zunder show 'clip '"))

-- conf/windowrules.lua: the settings window floats instead of tiling
hl.window_rule({
    name   = "zunder-settings",
    match  = { title = "^zunder settings$" },
    float  = true,
    size   = "980 660",
    center = true,
})

-- conf/windowrules.lua: zunder appears instantly instead of sliding in like other layers
hl.layer_rule({
    name    = "zunder-no-anim",
    match   = { namespace = "^zunder$" },
    no_anim = true,
})

-- hyprland.lua, after your binds: the shortcuts you make inside zunder (Ctrl+B)
pcall(require, "zunder")
```

`pcall` keeps Hyprland happy before you've made your first shortcut, when the file doesn't
exist yet.

## Hyprland, classic hyprland.conf

```ini
exec-once = zunder server
exec-once = wl-paste --type text --watch cliphist store

bind = ALT, TAB, exec, zunder switch
bind = ALT SHIFT, TAB, exec, zunder switch-prev
bind = ALT, SPACE, exec, zunder toggle
bind = SUPER, V, exec, zunder show 'clip '

windowrule = float, title:^zunder settings$
windowrule = size 980 660, title:^zunder settings$
windowrule = center, title:^zunder settings$
layerrule = noanim, zunder

source = ~/.config/hypr/zunder.conf
```

zunder writes `zunder.conf` instead of `zunder.lua` when there is no `hyprland.lua`. Create an
empty one once (`touch ~/.config/hypr/zunder.conf`) so the `source` line doesn't error before
your first shortcut.

## Reload

```sh
hyprctl reload
zunder server &          # or log out and back in
```

Press ALT+SPACE. If nothing shows up, see [Troubleshooting](troubleshooting.md).

## Other useful binds

`zunder show <query>` opens with the query already typed, so any prefix gets its own key:

```lua
hl.bind("SUPER + C",     hl.dsp.exec_cmd("zunder show '='"))          -- calculator
hl.bind("SUPER + T",     hl.dsp.exec_cmd("zunder show 'time in '"))   -- world clock
hl.bind("SUPER + H",     hl.dsp.exec_cmd("zunder show 'history '"))   -- launch history
hl.bind("SUPER + X",     hl.dsp.exec_cmd("zunder show 'kill '"))      -- a plugin
hl.bind("SUPER + COMMA", hl.dsp.exec_cmd("zunder settings"))
```

Binds for single apps and commands are easier to make from inside zunder: select the result,
press Ctrl+B, press the keys.

## Coming from vicinae, rofi, wofi, fuzzel

Replace their binds with the ones above and remove their autostart line. From vicinae,

```sh
zunder import vicinae
```

takes over your favorites (apps and clipboard history) and launch counts, so ranking already
knows what you use. Raycast-store extensions have no equivalent and are listed as skipped. Two
more things to know:

- vicinae registers SUPER+V for clipboard history by itself. Once its server isn't started any
  more, bind SUPER+V to `zunder show 'clip '` like above.
- dmenu-style scripts (`rofi -dmenu`) become zunder plugins almost unchanged: a plugin prints one
  result per line. See [Plugins](plugins.md).

## Settings

Run `zunder settings` (or type `settings` in zunder) for the settings window. Everything is
covered in [Configuration](configuration.md).
