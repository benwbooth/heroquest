#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 || $# -gt 3 ]]; then
    printf 'usage: %s DESTINATION BINARY [BINARY_NAME]\n' "$0" >&2
    exit 2
fi

destination=$1
binary=$2
binary_name=${3:-heroquest}

mkdir -p "$destination/assets" "$destination/tools"
install -m 0755 "$binary" "$destination/$binary_name"

# The private scan/model pack is intentionally not part of this tree. These
# are the project-authored/runtime files that every release can redistribute;
# the adjacent installer retrieves the optional local pack on first run.
cp -a assets/environment "$destination/assets/"
cp -a assets/fonts "$destination/assets/"
cp -a assets/quests "$destination/assets/"
cp -a assets/asset-sources.json assets/model-sources.json \
    assets/model-pack-sha256.txt "$destination/assets/"
cp -a tools/. "$destination/tools/"
cp -a LICENSE NOTICE.md README.md "$destination/"
