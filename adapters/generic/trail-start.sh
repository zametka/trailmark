#!/bin/sh
# trail-start.sh "<task>" [file]
# One argument writes the bundle to .trail/start.md. Two arguments write it to the file.
if [ -z "$1" ]; then
    echo "usage: trail-start.sh \"<task>\" [file]" >&2
    exit 2
fi
out=${2:-.trail/start.md}
mkdir -p "$(dirname "$out")" || exit 1
trailmark context "$1" > "$out"
