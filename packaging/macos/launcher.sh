#!/bin/sh
# zunder.app/Contents/MacOS/zunder: point GTK at the files bundled in the app, then start zunder.
here="$(cd "$(dirname "$0")" && pwd)"
res="$here/../Resources"
export XDG_DATA_DIRS="$res/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
export GSETTINGS_SCHEMA_DIR="$res/share/glib-2.0/schemas"
export GDK_PIXBUF_MODULE_FILE="$res/lib/gdk-pixbuf-2.0/loaders.cache"
export GTK_EXE_PREFIX="$res"
exec "$here/zunder-bin" "$@"
