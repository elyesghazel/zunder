#!/bin/sh
# Update the AUR packages to a released version and push them:
#   packaging/aur/update.sh 0.2.7            pin, write .SRCINFO, push
#   packaging/aur/update.sh 0.2.7 --dry-run  pin and write .SRCINFO only
#
# Sets pkgver, pins the sha256 of the release files, writes .SRCINFO (in an archlinux container,
# so this works from any distro with docker) and pushes zunder and zunder-bin to the AUR.
# Needs an SSH key registered with the AUR account (~/.ssh/aur, see docs/development.md).
set -eu
cd "$(dirname "$0")"
ver="${1:?usage: packaging/aur/update.sh <version>}"
ver="${ver#v}"
dry="${2:-}"
url=https://github.com/elyesghazel/zunder

sum() { curl -fsSL -A zunder-aur "$1" | sha256sum | cut -d' ' -f1; }
src_sum=$(sum "$url/archive/refs/tags/v$ver.tar.gz")
bin_sum=$(sum "$url/releases/download/v$ver/zunder-$ver-x86_64-linux.tar.gz")

for pkg in zunder zunder-bin; do
    case $pkg in zunder) s=$src_sum ;; *) s=$bin_sum ;; esac
    sed -i "s/^pkgver=.*/pkgver=$ver/; s/^pkgrel=.*/pkgrel=1/; s/^sha256sums=.*/sha256sums=('$s')/" "$pkg/PKGBUILD"
    docker run --rm -v "$PWD/$pkg:/pkg" -w /pkg archlinux:latest sh -c \
        'useradd -m b && chown -R b /pkg && su b -c "makepkg --printsrcinfo > .SRCINFO"; chown -R '"$(id -u):$(id -g)"' /pkg'
done

[ "$dry" = --dry-run ] && { echo "dry run: PKGBUILDs and .SRCINFO updated, nothing pushed"; exit 0; }

for pkg in zunder zunder-bin; do
    dir=$(mktemp -d)
    GIT_SSH_COMMAND="ssh -i $HOME/.ssh/aur -o IdentitiesOnly=yes" git clone -q "ssh://aur@aur.archlinux.org/$pkg.git" "$dir"
    cp "$pkg/PKGBUILD" "$pkg/.SRCINFO" "$dir/"
    git -C "$dir" add PKGBUILD .SRCINFO
    if git -C "$dir" diff --cached --quiet; then
        echo "$pkg: already at $ver"
    else
        git -C "$dir" commit -q -m "$pkg $ver"
        GIT_SSH_COMMAND="ssh -i $HOME/.ssh/aur -o IdentitiesOnly=yes" git -C "$dir" push -q origin HEAD:master
        echo "$pkg: pushed $ver"
    fi
    rm -rf "$dir"
done
