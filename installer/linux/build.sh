#!/bin/sh
# Збирає Altherra-Setup-linux-x64.run: скрипт setup.sh + архів із двома збірками й іконкою.
#   sh installer/linux/build.sh <бінарник-glibc> <бінарник-musl> <icon.png> <вихідний.run>
set -eu
GNU=$1; MUSL=$2; ICON=$3; OUT=$4
HERE=$(cd "$(dirname "$0")" && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
cp "$GNU" "$TMP/altherra-launcher-gnu"
cp "$MUSL" "$TMP/altherra-launcher-musl"
cp "$ICON" "$TMP/icon.png"
chmod 755 "$TMP"/altherra-launcher-*
cat "$HERE/setup.sh" > "$OUT"
tar czf - -C "$TMP" altherra-launcher-gnu altherra-launcher-musl icon.png >> "$OUT"
chmod 755 "$OUT"
echo "$OUT: $(du -h "$OUT" | cut -f1)"
