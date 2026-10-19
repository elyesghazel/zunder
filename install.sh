#!/bin/sh
# Install zunder into ~/.local/bin, from a release tarball or a source checkout.
#
#   ./install.sh            install (builds first if there's no prebuilt binary)
#   ./install.sh --plugins  also link the example plugins into ~/.config/zunder/plugins
#   ./install.sh --remove   uninstall (keeps your config)
set -eu

here=$(cd "$(dirname "$0")" && pwd)
bin="$HOME/.local/bin"
plugins="${XDG_CONFIG_HOME:-$HOME/.config}/zunder/plugins"

if [ "${1:-}" = "--remove" ]; then
    zunder quit 2>/dev/null || true
    rm -f "$bin/zunder"
    echo "removed $bin/zunder (config in ~/.config/zunder is untouched)"
    exit 0
fi

missing=""
for lib in gtk4 gtk4-layer-shell-0; do
    pkg-config --exists "$lib" 2>/dev/null || missing="$missing $lib"
done
if [ -n "$missing" ]; then
    echo "missing libraries:$missing"
    echo "  arch:   sudo pacman -S gtk4 gtk4-layer-shell"
    echo "  fedora: sudo dnf install gtk4 gtk4-layer-shell"
    exit 1
fi

if [ -x "$here/zunder" ]; then
    src="$here/zunder"                       # release tarball
else
    command -v cargo >/dev/null || { echo "cargo not found, install rust first"; exit 1; }
    (cd "$here" && cargo build --release)
    src="$here/target/release/zunder"
fi

install -Dm755 "$src" "$bin/zunder"
echo "installed $bin/zunder ($("$bin/zunder" --version))"

if [ "${1:-}" = "--plugins" ]; then
    mkdir -p "$plugins"
    for p in "$here"/examples/plugins/*; do
        ln -sf "$p" "$plugins/"
    done
    echo "linked example plugins into $plugins"
fi

for tool in grim cliphist wl-copy; do
    command -v "$tool" >/dev/null || echo "optional: $tool is not installed (see docs/installation.md)"
done
case ":$PATH:" in *":$bin:"*) ;; *) echo "note: $bin is not in your PATH" ;; esac

cat <<'EOF'

next: start it with Hyprland and bind the keys (docs/setup.md)

  hl.exec_cmd("zunder server")
  hl.bind("ALT + TAB",   hl.dsp.exec_cmd("zunder switch"))
  hl.bind("ALT + SPACE", hl.dsp.exec_cmd("zunder toggle"))
EOF
