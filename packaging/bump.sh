#!/bin/sh
# Cut a release: packaging/bump.sh 0.2.0
#
# Sets the version in Cargo.toml, Cargo.lock and the PKGBUILD, commits, tags v<version> and
# pushes. The tag starts .github/workflows/release.yml, which builds the Arch package, the Linux
# tarball, the Windows zip and the macOS dmg and publishes them as a GitHub release.
set -eu
cd "$(dirname "$0")/.."
ver="${1:?usage: packaging/bump.sh <version>, e.g. 0.2.0}"
ver="${ver#v}"

[ -z "$(git status --porcelain)" ] || { echo "commit or stash your changes first"; exit 1; }
[ "$(git branch --show-current)" = main ] || { echo "release from main"; exit 1; }
git rev-parse "v$ver" >/dev/null 2>&1 && { echo "v$ver already exists"; exit 1; }

sed -i "0,/^version = \".*\"/s//version = \"$ver\"/" Cargo.toml
sed -i "s/^pkgver=.*/pkgver=$ver/; s/^pkgrel=.*/pkgrel=1/; s/^sha256sums=.*/sha256sums=('SKIP')/" packaging/arch/PKGBUILD
cargo update --workspace --offline >/dev/null 2>&1 || cargo update --workspace >/dev/null
cargo test --locked --quiet >/dev/null

git add Cargo.toml Cargo.lock packaging/arch/PKGBUILD
git commit -q -m "chore(release): v$ver"
git tag -a "v$ver" -m "v$ver"
git push -q origin main "v$ver"
echo "pushed v$ver: https://github.com/elyesghazel/zunder/actions/workflows/release.yml"
