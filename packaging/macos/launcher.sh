#!/bin/sh
# zunder.app/Contents/MacOS/zunder: point GTK at the files bundled in the app, then start zunder.
here="$(cd "$(dirname "$0")" && pwd)"
res="$(cd "$here/../Resources" && pwd)"
export XDG_DATA_DIRS="$res/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
export GSETTINGS_SCHEMA_DIR="$res/share/glib-2.0/schemas"
export GTK_EXE_PREFIX="$res"
# the image loader cache is shipped with @RES@ placeholders: fill in where this app is
cache="${TMPDIR:-/tmp}/zunder-loaders.cache"
sed "s|@RES@|$res|g" "$res/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache.in" > "$cache" 2>/dev/null && export GDK_PIXBUF_MODULE_FILE="$cache"
exec "$here/zunder-bin" "$@"
