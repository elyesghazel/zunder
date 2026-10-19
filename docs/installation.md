# Installation

zunder needs Hyprland, GTK 4.14 or newer and gtk4-layer-shell. Everything else is optional.

| Dependency | Needed for | Arch package |
|------------|------------|--------------|
| `gtk4` (4.14+) | the UI | `gtk4` |
| `gtk4-layer-shell` | the overlay | `gtk4-layer-shell` |
| `wl-clipboard` | copying results | `wl-clipboard` |
| a Nerd Font | glyphs, the default font | `ttf-jetbrains-mono-nerd` |
| `grim` 1.5+ | window previews in ALT+TAB | `grim` |
| `cliphist` | clipboard history | `cliphist` |

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

Package: install the new release package. Source: `git pull && ./install.sh`. Restart the daemon
afterwards with `zunder quit && zunder server &`, or log out and back in.

## Uninstalling

```sh
sudo pacman -R zunder           # package
./install.sh --remove           # source / tarball install
```

Your settings stay in `~/.config/zunder` and the history in `~/.local/share/zunder`. Delete those
too for a clean slate, and remove the zunder lines from your Hyprland config.
