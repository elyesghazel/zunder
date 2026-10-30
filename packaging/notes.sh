#!/bin/sh
# Release notes from the commit subjects since the previous tag: packaging/notes.sh v0.3.5
#
# feat, fix and perf commits, grouped, scope in brackets. zunder shows these in its update
# notice, so they're written for people using it, not for git.
set -eu
cd "$(dirname "$0")/.."
tag="${1:?usage: packaging/notes.sh <tag>}"
prev=$(git describe --tags --abbrev=0 "$tag^" 2>/dev/null || true)
range="${prev:+$prev..}$tag"

section() {
    lines=$(git log --no-merges --format=%s "$range" | sed -nE "s/^$1(\(([^)]*)\))?!?: (.*)/- \3 [\2]/p" | sed 's/ \[\]$//')
    [ -n "$lines" ] && printf '## %s\n\n%s\n\n' "$2" "$lines"
    return 0
}

section feat "New"
section fix "Fixed"
section perf "Faster"
