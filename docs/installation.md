# Installation

The quick way, on any distro:

```sh
curl -fsSL https://raw.githubusercontent.com/elyesghazel/zunder/main/get.sh | sh
```

| Option | What it does |
|--------|--------------|
| `… \| sh` | install or update, then run `zunder setup` |
| `… \| sh -s -- --no-setup` | install or update only |
| `… \| sh -s -- --uninstall` | remove zunder; your config stays |
| `ZUNDER_BIN_DIR=/path … \| sh` | where the binary goes on non-Arch distros (default `~/.local/bin`) |

Arch gets the release package through pacman. Everything else gets the binary built on
Debian 13, which needs glibc 2.41+ (Debian 13, Ubuntu 25.04, Fedora 42, openSUSE Tumbleweed or
newer) plus gtk4 and gtk4-layer-shell from the distro; the script checks both and prints the
install command. Downloads are checked against the release's `SHA256SUMS`.

The sections below are the manual ways.

zunder needs Hyprland, GTK 4.14 or newer and gtk4-layer-shell. Everything else is optional.

| Dependency | Needed for | Arch package |
|------------|------------|--------------|
| `gtk4` (4.14+) | the UI | `gtk4` |
| `gtk4-layer-shell` | the overlay | `gtk4-layer-shell` |
| `wl-clipboard` | copying results | `wl-clipboard` |
| a Nerd Font | glyphs, the default font | `ttf-jetbrains-mono-nerd` |
| `grim` 1.5+ | window previews in ALT+TAB | `grim` |
| `cliphist` | clipboard history | `cliphist` |
| `fd` | file search | `fd` |
| `curl` | exchange rates | `curl` |

## Arch Linux: release package

Every [release](https://github.com/elyesghazel/zunder/releases) has a prebuilt package:

```sh
sudo pacman -U https://github.com/elyesghazel/zunder/releases/download/v0.1.0/zunder-0.1.0-1-x86_64.pkg.tar.zst
sudo pacman -S --needed grim cliphist ttf-jetbrains-mono-nerd   # optional extras
```

## Arch Linux: build the package yourself

The PKGBUILD is in [`packaging/arch`](../packaging/arch/PKGBUILD). It builds the tagged release
from source and runs the tests:

```sh
git clone https://github.com/elyesghazel/zunder
cd zunder/packaging/arch
makepkg -si
```

## Any distro: release tarball

The tarball has a binary linked against the system GTK, so you still need gtk4 and
gtk4-layer-shell from your distro.

```sh
curl -LO https://github.com/elyesghazel/zunder/releases/download/v0.1.0/zunder-0.1.0-x86_64-linux.tar.gz
tar xf zunder-0.1.0-x86_64-linux.tar.gz
cd zunder-0.1.0
./install.sh --plugins      # binary to ~/.local/bin, example plugins linked
```

Fedora: `sudo dnf install gtk4 gtk4-layer-shell`. On Debian and Ubuntu, check that your release
has GTK 4.14+ and packages gtk4-layer-shell (`apt search gtk4-layer-shell`); older releases
don't.

## macOS (early)

Apple Silicon only for now. Download `zunder-<version>-macos-arm64.dmg` from the
[latest release](https://github.com/elyesghazel/zunder/releases), open it and drag `zunder` to
Applications. The app isn't signed, so the first time right-click it and choose Open (or run
`xattr -dr com.apple.quarantine /Applications/zunder.app`).

The launcher, apps, search, calculator, conversion, time, emoji, files, plugins, themes and
settings work. The window switcher, previews and global shortcuts don't exist on macOS yet, so
start it from Spotlight or a hotkey tool like skhd (`zunder toggle`).

## Windows

Download [zunder-windows-setup.exe](https://github.com/elyesghazel/zunder/releases/latest/download/zunder-windows-setup.exe)
(about 16 MB) and run it. Windows warns because it isn't signed: **More info → Run anyway**.

- Installs for your user only, to `%LOCALAPPDATA%\Programs\zunder`, so no admin is needed.
- Options: start with Windows, Ctrl+Alt+Space on the Start menu entry, desktop shortcut.
- **ALT+SPACE** opens zunder (zunder registers it itself; if another program has it, use
  Ctrl+Alt+Space).
- File search works out of the box: `fd` comes with the installer.
- Uninstall from Settings → Apps.
- Updates: when a new version is out, the start screen says so and Enter installs it silently.

Without the window switcher, previews and Ctrl+B shortcuts so far. A portable zip
(`zunder-<version>-windows-x86_64.zip`) is on the release page too: unzip it anywhere and run
`zunder.cmd`; `zunder-debug.cmd` shows errors in a console.

## From source

```sh
sudo pacman -S --needed rust gtk4 gtk4-layer-shell wl-clipboard   # or your distro's equivalents
git clone https://github.com/elyesghazel/zunder
cd zunder
./install.sh --plugins
```

`install.sh` builds with `cargo build --release` and copies the binary to `~/.local/bin`. Make
sure that's in your `PATH`, and in the `PATH` Hyprland sees.

## Check it

```sh
zunder --version
zunder              # starts the daemon if needed and opens the launcher
```

Then go to [Setup](setup.md) to bind the keys.

## Updating

When a new version is out, zunder's start screen says "zunder X is out" (it checks GitHub once
a day; `update_check = false` turns that off). Press Enter on it: a terminal runs the install
line again, which updates zunder (on Arch through pacman, so it asks for your password) and
restarts the running daemon, so the new version is live right away.

By hand it's the same line: `curl -fsSL https://raw.githubusercontent.com/elyesghazel/zunder/main/get.sh | sh`.
From source: `git pull && ./install.sh`, then `zunder quit` and start `zunder server` again.

## Uninstalling

```sh
sudo pacman -R zunder           # package
./install.sh --remove           # source / tarball install
```

Your settings stay in `~/.config/zunder` and the history in `~/.local/share/zunder`. Delete those
too for a clean slate, and remove the zunder lines from your Hyprland config.
