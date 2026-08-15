#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
edition_root=${HEROQUEST_ART_DIR:-"$repo_root/assets/local/editions/original-us"}
source_url='http://heroquestadventure.com/scans/HQ%20Game%20System%20US/HQ%20Game%20System%20US.rar'
expected_sha256='a7d3ae6c71c8de3c517dd6667d7ab4a8cce3046e80e96ad2ef470f5ef8ec74db'
archive="$edition_root/source/HQ Game System US.rar"
partial_archive="$archive.part"

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        printf 'Neither sha256sum nor shasum is available.\n' >&2
        return 1
    fi
}

is_installed() {
    [[ -f "$edition_root/board-runtime.jpg" ]] \
        && [[ -f "$edition_root/startup/box/top.jpg" ]] \
        && [[ -f "$edition_root/screen/information-screen-front.png" ]] \
        && [[ -f "$edition_root/tabletop/player/character-sheet.png" ]] \
        && [[ -f "$edition_root/components/doors/open.png" ]] \
        && [[ -f "$edition_root/dice/skull.png" ]]
}

if [[ ${1:-} == --check ]]; then
    if is_installed; then
        printf 'Original-US scan-backed runtime art is installed under %s\n' "$edition_root"
        exit 0
    fi
    printf 'Original-US scan-backed runtime art is not installed under %s\n' "$edition_root" >&2
    exit 1
fi

if [[ ${1:-} != --accept-liability ]]; then
    cat >&2 <<'EOF'
This installer downloads the original-US scan archive directly from
heroquestadventure.com. Run it only after reviewing the in-game warning:

  tools/install-original-us-scan-pack.sh --accept-liability
EOF
    exit 2
fi

if is_installed; then
    printf 'Original-US scan-backed runtime art is already installed under %s\n' "$edition_root"
    exit 0
fi

for command in curl pdftoppm magick; do
    if ! command -v "$command" >/dev/null 2>&1; then
        printf 'Required asset-preparation command is unavailable: %s\n' "$command" >&2
        exit 3
    fi
done
if command -v unrar >/dev/null 2>&1; then
    rar_tool=unrar
elif command -v unrar-free >/dev/null 2>&1; then
    rar_tool=unrar-free
else
    printf 'Required asset-preparation command is unavailable: unrar or unrar-free\n' >&2
    exit 3
fi

mkdir -p "$edition_root/source"
if [[ -f "$archive" ]]; then
    actual_sha256=$(sha256_file "$archive")
    if [[ $actual_sha256 != "$expected_sha256" ]]; then
        printf 'Existing archive failed SHA-256 verification.\nExpected: %s\nActual:   %s\nFile: %s\n' \
            "$expected_sha256" "$actual_sha256" "$archive" >&2
        exit 4
    fi
else
    printf 'Downloading HQ Game System US directly from heroquestadventure.com...\n'
    curl --fail --location --continue-at - --progress-bar \
        --output "$partial_archive" "$source_url"
    actual_sha256=$(sha256_file "$partial_archive")
    if [[ $actual_sha256 != "$expected_sha256" ]]; then
        printf 'Downloaded archive failed SHA-256 verification.\nExpected: %s\nActual:   %s\nFile retained for inspection: %s\n' \
            "$expected_sha256" "$actual_sha256" "$partial_archive" >&2
        exit 5
    fi
    mv -- "$partial_archive" "$archive"
fi

printf 'Extracting the verified archive...\n'
mkdir -p "$edition_root/scans"
if [[ $rar_tool == unrar ]]; then
    unrar t -idq "$archive"
    unrar x -o+ -idq "$archive" "$edition_root/scans/"
else
    unrar-free --list "$archive" >/dev/null
    unrar-free --extract --force "$archive" "$edition_root/scans/"
fi

mkdir -p \
    "$edition_root/card-sheets" \
    "$edition_root/quest-pages" \
    "$edition_root/rulebook-pages" \
    "$edition_root/tile-sheets" \
    "$edition_root/box-pages" \
    "$edition_root/character-sheet" \
    "$edition_root/armory-pages" \
    "$edition_root/poster-pages" \
    "$edition_root/extras-pages"

printf 'Rendering runtime pages from the source PDFs...\n'
pdftoppm -jpeg -jpegopt quality=95 -r 300 -singlefile \
    "$edition_root/scans/Gameboard.pdf" "$edition_root/board-scan"
magick "$edition_root/board-scan.jpg" -resize 50% -strip -quality 92 \
    "$edition_root/board-runtime.jpg"
pdftoppm -png -r 300 "$edition_root/scans/Cards.pdf" \
    "$edition_root/card-sheets/page"
pdftoppm -png -r 300 "$edition_root/scans/Quest Book - Computer View.pdf" \
    "$edition_root/quest-pages/page"
pdftoppm -png -r 300 "$edition_root/scans/Instruction Booklet - Computer View.pdf" \
    "$edition_root/rulebook-pages/page"
pdftoppm -png -r 300 "$edition_root/scans/Tile Sheets.pdf" \
    "$edition_root/tile-sheets/page"
pdftoppm -png -r 300 "$edition_root/scans/Box.pdf" \
    "$edition_root/box-pages/page"
pdftoppm -png -r 300 -singlefile "$edition_root/scans/Character Sheet.pdf" \
    "$edition_root/character-sheet/character-sheet"
pdftoppm -png -r 300 -singlefile "$edition_root/scans/Identification Guide and Armory.pdf" \
    "$edition_root/armory-pages/identification-guide-and-armory"
for poster in 1 2 3 4; do
    pdftoppm -png -r 300 "$edition_root/scans/Poster $poster.pdf" \
        "$edition_root/poster-pages/poster-$poster-page"
done
pdftoppm -png -r 300 -singlefile "$edition_root/scans/Hero Quest Quest Packs Order Form.pdf" \
    "$edition_root/extras-pages/quest-packs-order-form"
pdftoppm -png -r 300 "$edition_root/scans/Survey Form.pdf" \
    "$edition_root/extras-pages/survey-page"

cat >"$edition_root/board-calibration.json" <<'EOF'
{
  "playable_bounds_px": {
    "left": 80,
    "top": 64,
    "right": 7082,
    "bottom": 5122
  }
}
EOF
cat >"$edition_root/board-runtime-calibration.json" <<'EOF'
{
  "playable_bounds_px": {
    "left": 40,
    "top": 32,
    "right": 3541,
    "bottom": 2561
  }
}
EOF

printf 'Extracting optimized board-game textures...\n'
HEROQUEST_ART_ROOT="$edition_root" "$repo_root/tools/extract-original-us-tabletop-art.sh"
HEROQUEST_ART_DIR="$edition_root" "$repo_root/tools/extract-original-us-screen.sh"
HEROQUEST_ART_ROOT="$edition_root" "$repo_root/tools/extract-original-us-startup-art.sh"
HEROQUEST_ART_ROOT="$edition_root" "$repo_root/tools/extract-original-us-dice-decals.sh"
HEROQUEST_ART_ROOT="$edition_root" "$repo_root/tools/extract-original-us-components.sh"

if ! is_installed; then
    printf 'The scan archive was processed, but the runtime-art audit is incomplete.\n' >&2
    exit 6
fi

printf 'Original-US scan-backed runtime art is ready under %s\n' "$edition_root"
