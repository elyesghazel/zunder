#!/bin/sh
# Install or update zunder:
#
#   curl -fsSL https://raw.githubusercontent.com/elyesghazel/zunder/main/get.sh | sh
#
# Arch: installs the release package with pacman. Other distros: puts the binary in ~/.local/bin
# (needs gtk4 4.14+ and gtk4-layer-shell from your distro, it tells you the command).
# Then runs `zunder setup` for the keybinds. Run it again to update.
#
#   … | sh -s -- --uninstall      remove zunder (your config stays)
#   … | sh -s -- --no-setup       install only
set -eu

REPO="elyesghazel/zunder"
BIN_DIR="${ZUNDER_BIN_DIR:-$HOME/.local/bin}"

say() { printf '\033[1;38;2;180;190;254m❯\033[0m %s\n' "$*"; }
warn() { printf '\033[38;2;249;226;175m!\033[0m %s\n' "$*" >&2; }
die() { printf '\033[38;2;243;139;168m✗\033[0m %s\n' "$*" >&2; exit 1; }
has() { command -v "$1" >/dev/null 2>&1; }

setup=yes
for arg in "$@"; do
    case "$arg" in
        --uninstall) action=uninstall ;;
        --no-setup) setup=no ;;
        *) die "unknown option $arg" ;;
    esac
done

SUDO=""
[ "$(id -u)" = 0 ] || SUDO="sudo"

if [ "${action:-}" = uninstall ]; then
    has zunder && zunder quit 2>/dev/null || true
    if has pacman && pacman -Qq zunder >/dev/null 2>&1; then
        $SUDO pacman -R --noconfirm zunder
    fi
    rm -f "$BIN_DIR/zunder"
    say "zunder removed. Your settings are still in ~/.config/zunder"
    exit 0
fi

has curl || die "curl is needed"
has tar || die "tar is needed"
[ "$(uname -s)" = Linux ] || die "this script is for Linux; macOS and Windows builds are on the releases page"
[ "$(uname -m)" = x86_64 ] || die "only x86_64 builds exist so far, build from source: https://github.com/$REPO#from-source"

tag=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)
[ -n "$tag" ] || die "couldn't find the latest release (GitHub API rate limit?)"
ver="${tag#v}"
base="https://github.com/$REPO/releases/download/$tag"

if has zunder && [ "$(zunder --version 2>/dev/null)" = "zunder $ver" ]; then
    say "zunder $ver is already installed"
else
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT

    fetch() { curl -fsSL --retry 2 -o "$tmp/$1" "$base/$1" || die "download failed: $base/$1"; }
    verify() {
        if curl -fsSL -o "$tmp/SHA256SUMS" "$base/SHA256SUMS" 2>/dev/null; then
            (cd "$tmp" && grep " $1\$" SHA256SUMS | sha256sum -c --quiet -) || die "checksum mismatch for $1"
        else
            warn "this release has no checksums, skipping verification"
        fi
    }

    if has pacman; then
        file="zunder-$ver-1-x86_64.pkg.tar.zst"
        say "downloading zunder $ver for Arch"
        fetch "$file"
        verify "$file"
        say "installing with pacman"
        $SUDO pacman -U --noconfirm --needed "$tmp/$file"
    else
        # the binary is built on Debian 13 and needs its glibc (2.41) or newer
        glibc=$(ldd --version 2>/dev/null | head -1 | grep -o '[0-9]*\.[0-9]*$' || echo 0)
        major=${glibc%%.*}; minor=${glibc#*.}
        if [ "$major" -lt 2 ] || { [ "$major" -eq 2 ] && [ "$minor" -lt 41 ]; }; then
            die "your glibc is $glibc, zunder's Linux build needs 2.41+ (Debian 13, Ubuntu 25.04, Fedora 42 or newer). Build from source: https://github.com/$REPO#from-source"
        fi
        # it links against the system GTK; check the libraries before unpacking
        missing=""
        for lib in libgtk-4.so.1 libgtk4-layer-shell.so; do
            ldconfig -p 2>/dev/null | grep -q "$lib" || missing="$missing $lib"
        done
        if [ -n "$missing" ]; then
            warn "missing libraries:$missing"
            if has apt-get; then echo "  sudo apt install libgtk-4-1 libgtk4-layer-shell0 wl-clipboard   # Debian 13 / Ubuntu 25.04+"
            elif has dnf; then echo "  sudo dnf install gtk4 gtk4-layer-shell wl-clipboard"
            elif has zypper; then echo "  sudo zypper install gtk4 gtk4-layer-shell wl-clipboard"
            elif has xbps-install; then echo "  sudo xbps-install gtk4 gtk4-layer-shell wl-clipboard"
            else echo "  install gtk4 (4.14+) and gtk4-layer-shell with your package manager"
            fi
            die "install those, then run this again"
        fi
        file="zunder-$ver-x86_64-linux.tar.gz"
        say "downloading zunder $ver"
        fetch "$file"
        verify "$file"
        tar -xzf "$tmp/$file" -C "$tmp"
        mkdir -p "$BIN_DIR"
        install -m755 "$tmp/zunder-$ver/zunder" "$BIN_DIR/zunder"
        say "installed $BIN_DIR/zunder"
        case ":$PATH:" in
            *":$BIN_DIR:"*) ;;
            *) warn "$BIN_DIR is not in your PATH; add it, or Hyprland won't find zunder" ;;
        esac
    fi

    # restart a running daemon so the new version is the one that answers
    if has zunder && zunder quit 2>/dev/null; then
        sleep 0.3
        if has setsid; then setsid -f zunder server >/dev/null 2>&1; else nohup zunder server >/dev/null 2>&1 & fi
        say "zunder $ver is ready and running"
    else
        say "zunder $ver is ready"
    fi
fi

zbin=$(command -v zunder || echo "$BIN_DIR/zunder")
if [ "$setup" = yes ] && "$zbin" help 2>/dev/null | grep -q "zunder setup"; then
    if [ -r /dev/tty ]; then
        say "starting setup"
        "$zbin" setup </dev/tty
    else
        say "run 'zunder setup' to set up the keybinds"
    fi
else
    echo
    echo "next: add zunder to Hyprland, see https://github.com/$REPO#setup"
fi
