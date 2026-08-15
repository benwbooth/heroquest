#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
art_root=${HEROQUEST_ART_DIR:-"$repo_root/assets/local/editions/original-us"}
model_root=${HEROQUEST_MODEL_DIR:-"$art_root/models"}
missing=0

check_file() {
    local label=$1
    local path=$2
    if [[ -f "$path" ]]; then
        printf 'ok       %-30s %s\n' "$label" "${path#"$repo_root/"}"
    else
        printf 'missing  %-30s %s\n' "$label" "${path#"$repo_root/"}"
        missing=$((missing + 1))
    fi
}

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        return 2
    fi
}

check_sha256() {
    local label=$1
    local path=$2
    local expected=$3
    local actual
    if [[ ! -f "$path" ]]; then
        check_file "$label" "$path"
        return
    fi
    if ! actual=$(sha256_file "$path"); then
        printf 'missing  %-30s SHA-256 tool unavailable\n' "$label"
        missing=$((missing + 1))
    elif [[ $actual == "$expected" ]]; then
        printf 'verified %-30s %s\n' "$label" "${path#"$repo_root/"}"
    else
        printf 'invalid  %-30s expected %s, got %s\n' "$label" "$expected" "$actual"
        missing=$((missing + 1))
    fi
}

if ! command -v jq >/dev/null 2>&1; then
    printf 'jq is required for the complete asset audit.\n' >&2
    exit 2
fi

check_file ledger "$repo_root/assets/asset-sources.json"
check_file model-provenance "$repo_root/assets/model-sources.json"
check_file model-checksums "$repo_root/assets/model-pack-sha256.txt"
if [[ -f "$repo_root/assets/asset-sources.json" ]]; then
    jq empty "$repo_root/assets/asset-sources.json" || missing=$((missing + 1))
fi
if [[ -f "$repo_root/assets/model-sources.json" ]]; then
    jq empty "$repo_root/assets/model-sources.json" || missing=$((missing + 1))
fi

if ! HEROQUEST_ART_DIR="$art_root" "$repo_root/tools/audit-original-us-scan-art.sh"; then
    missing=$((missing + 1))
fi
if ! HEROQUEST_MODEL_DIR="$model_root" "$repo_root/tools/audit-local-models.sh"; then
    missing=$((missing + 1))
fi
check_file model.wall-1x1 "$model_root/walls/wall-1x1.glb"
check_file model.wall-1x2 "$model_root/walls/wall-1x2.glb"

environment_root="$repo_root/assets/environment"
for relative in \
    README.md \
    castle-great-hall-reference.png \
    castle-great-hall-matte-v1.png \
    castle-great-hall-panorama-v1.png \
    castle-great-hall-preview.png \
    castle-great-hall.blend \
    textures/dark-oak.png \
    textures/flagstone.png \
    textures/gothic-stone.png \
    textures/heraldic-rug.png; do
    check_file "environment.$relative" "$environment_root/$relative"
done
panorama_sha=$(jq -r '.families[] | select(.id == "castle-room-environment") | .panorama_sha256' \
    "$repo_root/assets/asset-sources.json")
room_sha=$(jq -r '.families[] | select(.id == "castle-room-environment") | .glb_sha256' \
    "$repo_root/assets/asset-sources.json")
check_sha256 environment.panorama \
    "$environment_root/castle-great-hall-panorama-v1-4x.png" "$panorama_sha"
check_sha256 environment.glb "$environment_root/castle-great-hall.glb" "$room_sha"

font_sha=$(jq -r '.families[] | select(.id == "ui-font") | .sha256' \
    "$repo_root/assets/asset-sources.json")
check_sha256 font.almendra "$repo_root/assets/fonts/Almendra-Regular.ttf" "$font_sha"
check_file font.license "$repo_root/assets/fonts/Almendra-OFL.txt"
check_file audio.procedural "$repo_root/src/audio.rs"

quest_count=$(find "$repo_root/assets/quests" -maxdepth 1 -type f \
    -name 'original_us_*.json' | wc -l)
if (( quest_count == 14 )); then
    printf 'count    %-30s %s/14\n' quests.original-us "$quest_count"
else
    printf 'missing  %-30s %s/14\n' quests.original-us "$quest_count"
    missing=$((missing + 1))
fi
while IFS= read -r quest; do
    jq empty "$quest" || missing=$((missing + 1))
done < <(find "$repo_root/assets/quests" -maxdepth 1 -type f \
    -name 'original_us_*.json' | sort)

if (( missing > 0 )); then
    printf '\nThe complete asset audit found %s incomplete groups.\n' "$missing" >&2
    exit 1
fi
printf '\nComplete asset coverage verified across every recorded family.\n'
