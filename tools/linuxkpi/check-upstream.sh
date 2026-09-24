#!/usr/bin/env bash
set -euo pipefail

DRIVER_DIR="${1:?usage: check-upstream.sh drivers/<name>}"
source "$DRIVER_DIR/UPSTREAM"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

status=0
for file in $FILES $REMOVED_FILES; do
    curl -s -f -m 60 -o "$WORK/$file" "https://raw.githubusercontent.com/torvalds/linux/$VERSION/$PATH_IN_TREE/$file"
done
for file in $REMOVED_FILES; do
    if [ -e "$DRIVER_DIR/src/$file" ]; then
        echo "$file is listed as removed but still present"
        status=1
    fi
done
for file in $FILES; do
    if [ ! -f "$DRIVER_DIR/src/$file" ]; then
        echo "$file is missing"
        status=1
        continue
    fi
    added="$(diff "$WORK/$file" "$DRIVER_DIR/src/$file" | grep -c '^>' || true)"
    removed="$(diff "$WORK/$file" "$DRIVER_DIR/src/$file" | grep -c '^<' || true)"
    if [ "$added" != "0" ]; then
        echo "$file: $added added or rewritten line(s)"
        diff "$WORK/$file" "$DRIVER_DIR/src/$file" | grep '^>' | head -n 20
        status=1
    elif [ "$removed" != "0" ]; then
        echo "$file: $removed line(s) removed, nothing added"
    fi
done
if [ "$status" = "0" ]; then
    echo "$DRIVER_DIR matches Linux $VERSION except for removed parts"
fi
exit "$status"
