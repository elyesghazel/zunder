# Themes

![themes](../assets/themes.png)

Type `theme` to list the color schemes. `Enter` applies one right away and keeps the list open,
so you can step through them. The Theme dropdown in the settings (or `theme = "nord"` in
`config.toml`) does the same.

Built in: `graphite` (the default), `catppuccin-mocha`, `catppuccin-latte` (light), `rose-pine`,
`rose-pine-dawn` (light), `gruvbox-dark`, `tokyo-night`, `nord`, `everforest-dark`, `dracula`,
`kanagawa` and `one-dark`.

Your own theme is a file in `~/.config/zunder/themes/`, named after the theme, with one
`@define-color` line per color. It shows up in the list next to the built-ins, and a file with a
built-in's name replaces that built-in. Colors you leave out stay graphite.

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

| Color | Used for |
|-------|----------|
| `bg` | backgrounds |
| `fg` | titles and the query |
| `muted` | key hints, selected hint |
| `faint` | subtitles, placeholder, glyphs |
| `ghost` | borders, separators, row markers |
| `accent` | prompt, section titles, selection |
| `green` | not used by the built-in CSS, there for your own rules |
| `sel` / `hover` | selected / hovered row |

The `accent` setting wins over the theme's accent. For anything beyond colors,
`~/.config/zunder/style.css` loads on top of everything; see
[configuration](configuration.md#css) for the selectors.
