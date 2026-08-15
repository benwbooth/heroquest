#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd -- "$(dirname -- "$0")/.." && pwd)
default_art_dir="$repo_dir/assets/local/editions/original-us"
art_dir=${1:-${HEROQUEST_ART_DIR:-$default_art_dir}}
front_sheet="$art_dir/tile-sheets/page-1.png"
back_sheet="$art_dir/tile-sheets/page-2.png"
screen_dir="$art_dir/screen"

if ! command -v magick >/dev/null 2>&1; then
  printf 'ImageMagick (magick) is required to extract the Information Screen\n' >&2
  exit 2
fi
for sheet in "$front_sheet" "$back_sheet"; do
  if [[ ! -f "$sheet" ]]; then
    printf 'original-US tile sheet not found: %s\n' "$sheet" >&2
    exit 1
  fi
done

mkdir -p "$screen_dir"
mask=$(mktemp --suffix=.png)
trap 'rm -f -- "$mask"' EXIT

# Both sides occupy the same die-cut outline on the first two 5765x3598 local
# tile-sheet renders. The mask removes the unrelated punch-board components
# above the curved screen. Output is deliberately local and gitignored.
magick -size 5680x2730 xc:none \
  -fill white -stroke none \
  -draw 'path "M 15,735 L 1750,735 C 2050,735 2100,125 2825,55 C 3550,125 3650,735 3935,735 L 5665,735 L 5665,2500 C 4750,2710 950,2710 15,2500 Z"' \
  "$mask"

extract_side() {
  source=$1
  output=$2
  magick "$source" -crop 5680x2730+50+850 +repage \
    "$mask" -alpha off -compose CopyOpacity -composite \
    -resize 2048x "$output"
}

extract_side "$front_sheet" "$screen_dir/information-screen-front.png"
extract_side "$back_sheet" "$screen_dir/information-screen-back.png"

printf 'Extracted original-US Information Screen textures:\n'
printf '  %s\n' "$screen_dir/information-screen-front.png"
printf '  %s\n' "$screen_dir/information-screen-back.png"
