#!/bin/sh
# Збирає інсталятор .run: скрипт setup.sh + архів із двома збірками (glibc, musl) й іконкою.
#   sh installer/linux/build.sh <назва> <id> <бінарник-glibc> <бінарник-musl> <icon.png> <вихідний.run>
#   напр.: sh installer/linux/build.sh "Altherra" altherra-launcher gnu musl icon.png Altherra-Setup-linux-x64.run
set -eu
APP=$1; ID=$2; GNU=$3; MUSL=$4; ICON=$5; OUT=$6
case "$ID" in *[!a-z0-9-]*|"") echo "погане id: $ID" >&2; exit 1 ;; esac
HERE=$(cd "$(dirname "$0")" && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
cp "$GNU" "$TMP/$ID-gnu"
cp "$MUSL" "$TMP/$ID-musl"
cp "$ICON" "$TMP/icon.png"
chmod 755 "$TMP/$ID-gnu" "$TMP/$ID-musl"
sed -e "s|@APP@|$APP|g" -e "s|@ID@|$ID|g" "$HERE/setup.sh" > "$OUT"
tar czf - -C "$TMP" "$ID-gnu" "$ID-musl" icon.png >> "$OUT"
chmod 755 "$OUT"
echo "$OUT: $(du -h "$OUT" | cut -f1)"
