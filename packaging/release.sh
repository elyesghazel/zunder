#!/bin/sh
# Build the release files into dist/:
#   zunder-<ver>-x86_64-linux.tar.gz   binary + install.sh + docs + example plugins
#   zunder-<ver>-1-x86_64.pkg.tar.zst  Arch package, built with makepkg from packaging/arch
#
#   packaging/release.sh           uses the pushed tag v<ver> on GitHub (and pins its checksum)
#   LOCAL=1 packaging/release.sh   builds the Arch package from HEAD instead, for testing
set -eu
cd "$(dirname "$0")/.."
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
arch=$(uname -m)
dist=$PWD/dist
rm -rf "$dist" && mkdir -p "$dist"

echo "── binary tarball"
cargo build --release --locked
stage="$dist/zunder-$ver"
mkdir -p "$stage"
cp target/release/zunder install.sh README.md LICENSE "$stage/"
cp -r docs examples "$stage/"
tar -C "$dist" -czf "$dist/zunder-$ver-$arch-linux.tar.gz" "zunder-$ver"
rm -rf "$stage"

echo "── arch package"
build=$(mktemp -d)
cp packaging/arch/PKGBUILD "$build/"
src="$build/zunder-$ver.tar.gz"
if [ "${LOCAL:-}" = 1 ]; then
    git archive --format=tar.gz --prefix="zunder-$ver/" HEAD > "$src"
else
    curl -fsSL "https://github.com/elyesghazel/zunder/archive/refs/tags/v$ver.tar.gz" -o "$src"
    sum=$(sha256sum "$src" | cut -d' ' -f1)
    sed -i "s/^sha256sums=.*/sha256sums=('$sum')/" packaging/arch/PKGBUILD "$build/PKGBUILD"
    echo "pinned sha256 $sum in packaging/arch/PKGBUILD"
fi
(cd "$build" && makepkg -f --noconfirm >/dev/null)
cp "$build"/zunder-"$ver"-*.pkg.tar.zst "$dist/"
rm -rf "$build"

ls -lh "$dist"
