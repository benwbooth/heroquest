#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
edition_root=${HEROQUEST_ART_DIR:-"$repo_root/assets/local/editions/original-us"}
model_root=${HEROQUEST_MODEL_DIR:-"$edition_root/models"}
model_source_root=${HEROQUEST_MODEL_SOURCE_DIR:-"$repo_root/assets/local/sources/greengreenwine-community-pack"}
model_source_url='https://drive.google.com/drive/folders/1t-CAUKRnYzHFjuWopB0L0RKoyBeyW-MD'
model_checksum_manifest="$repo_root/assets/model-pack-sha256.txt"

emit_progress() {
    if [[ -n ${HEROQUEST_PROGRESS_FILE:-} ]]; then
        printf '%s\t%s\n' "$1" "$2" >>"$HEROQUEST_PROGRESS_FILE"
    fi
}

verify_model_source_pack() {
    if command -v sha256sum >/dev/null 2>&1; then
        (cd -- "$model_source_root" && sha256sum --check --quiet "$model_checksum_manifest")
    elif command -v shasum >/dev/null 2>&1; then
        (cd -- "$model_source_root" && shasum -a 256 --check "$model_checksum_manifest" >/dev/null)
    else
        printf 'Neither sha256sum nor shasum is available.\n' >&2
        return 1
    fi
}

models_are_installed() {
    HEROQUEST_MODEL_DIR="$model_root" "$repo_root/tools/audit-local-models.sh" >/dev/null 2>&1 \
        && [[ -f "$model_root/walls/wall-1x1.glb" ]] \
        && [[ -f "$model_root/walls/wall-1x2.glb" ]]
}

environment_is_installed() {
    local panorama
    local room
    for panorama in \
        "$repo_root/assets/local/environment/castle-great-hall-panorama-v1-4x.png" \
        "$repo_root/assets/environment/castle-great-hall-panorama-v1-4x.png"; do
        [[ -f "$panorama" ]] && break
    done
    for room in \
        "$repo_root/assets/local/environment/castle-great-hall.glb" \
        "$repo_root/assets/environment/castle-great-hall.glb"; do
        [[ -f "$room" ]] && break
    done
    [[ -f "$panorama" && -f "$room" ]]
}

all_assets_are_installed() {
    HEROQUEST_ART_DIR="$edition_root" \
        HEROQUEST_MODEL_DIR="$model_root" \
        "$repo_root/tools/audit-all-assets.sh" >/dev/null 2>&1
}

if [[ ${1:-} == --check ]]; then
    if all_assets_are_installed; then
        printf 'All required and optional HeroQuest runtime assets are installed.\n'
        exit 0
    fi
    printf 'The complete HeroQuest runtime asset set is not installed.\n' >&2
    exit 1
fi

if [[ ${1:-} != --accept-liability ]]; then
    cat >&2 <<'EOF'
This installer retrieves the scan and classic-model collections directly from
their current source sites. Run it only after reviewing the in-game warning:

  tools/install-all-assets.sh --accept-liability
EOF
    exit 2
fi

emit_progress 0.01 'Preparing original-US printed art'
HEROQUEST_ART_DIR="$edition_root" \
    "$repo_root/tools/install-original-us-scan-pack.sh" --accept-liability

if ! models_are_installed; then
    for command in uv blender jq; do
        if ! command -v "$command" >/dev/null 2>&1; then
            printf 'Required model-preparation command is unavailable: %s\n' "$command" >&2
            exit 3
        fi
    done

    mkdir -p "$(dirname -- "$model_source_root")" "$model_root"
    emit_progress 0.73 'Downloading classic STL source collection'
    printf 'Downloading the classic STL collection directly from Google Drive...\n'
    uvx --from gdown==6.1.0 gdown --folder --continue \
        "$model_source_url" -O "$model_source_root"
    emit_progress 0.84 'Verifying classic STL source collection'
    if ! verify_model_source_pack; then
        printf 'The downloaded model source pack failed SHA-256 verification.\n' >&2
        exit 4
    fi

    emit_progress 0.86 'Converting classic figures, furniture, traps, and walls'
    printf 'Converting classic figures, furniture, traps, and wall tiles...\n'
    blender --background --python "$repo_root/tools/import-classic-stl-pack.py" -- \
        --source-root "$model_source_root" \
        --output-root "$model_root"

    emit_progress 0.94 'Building quest-specific Orc variants'
    printf 'Building quest-specific Orc variants...\n'
    blender --background --python "$repo_root/tools/build-orc-variants.py" -- \
        --source "$model_root/figures/orc-sword.glb" \
        --output-root "$model_root"

    emit_progress 0.96 'Building dice, doors, markers, and fittings'
    printf 'Building project-authored dice, doors, markers, and fittings...\n'
    blender --background --python "$repo_root/tools/build-project-models.py" -- \
        --output-root "$model_root"
fi

if ! environment_is_installed; then
    printf 'The bundled castle environment is missing from this checkout.\n' >&2
    exit 5
fi

emit_progress 0.98 'Auditing every asset family'
printf 'Running complete asset audits...\n'
HEROQUEST_ART_DIR="$edition_root" HEROQUEST_MODEL_DIR="$model_root" \
    "$repo_root/tools/audit-all-assets.sh"

if ! all_assets_are_installed; then
    printf 'Asset preparation completed, but the final complete-set audit failed.\n' >&2
    exit 6
fi

emit_progress 1.00 'All HeroQuest assets are ready'
printf 'All HeroQuest runtime assets are ready.\n'
