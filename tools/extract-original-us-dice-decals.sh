#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
edition_root="${HEROQUEST_ART_ROOT:-$repo_root/assets/local/editions/original-us}"
combat_page="$edition_root/rulebook-pages/page-08.png"
hero_defense_page="$edition_root/rulebook-pages/page-12.png"
output_dir="$edition_root/dice"
work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT

for source in "$combat_page" "$hero_defense_page"; do
    if [[ ! -f "$source" ]]; then
        echo "missing original-US scan: $source" >&2
        exit 1
    fi
done

mkdir -p "$output_dir"

# These coordinates isolate the ink inside the illustrated dice, excluding
# the example-die outline and nearby copy. They refer to the original-US scan:
# skull and monster shield on printed page 14 (scan page 08), and the lion Hero
# shield on printed page 22 (scan page 12). Soft levels preserve the printed
# edge instead of reducing the symbols to jagged threshold silhouettes.
magick "$combat_page" \
    -crop 106x132+172+2298 +repage \
    -colorspace Gray -negate -level 5%,78% -trim +repage \
    -resize 310x350 -gravity center -background black -extent 512x512 \
    "$work_dir/skull-mask.png"

magick "$hero_defense_page" \
    -crop 102x122+175+2085 +repage \
    -colorspace Gray -negate -level 5%,78% \
    -fill black -draw 'rectangle 0,108 14,122' -trim +repage \
    -resize 350x390 -gravity center -background black -extent 512x512 \
    "$work_dir/white-shield-mask.png"

# The monster symbol is circular. Multiplying by the source-sized circle drops
# the two tiny pieces of the surrounding illustrated die that touch this crop.
magick "$combat_page" \
    -crop 122x130+983+654 +repage \
    -colorspace Gray -negate -level 5%,78% \
    "$work_dir/black-shield-source-mask.png"
magick -size 122x130 xc:black -fill white \
    -draw 'ellipse 61,65 59,62 0,360' \
    "$work_dir/black-shield-circle-mask.png"
magick "$work_dir/black-shield-source-mask.png" \
    "$work_dir/black-shield-circle-mask.png" \
    -compose Multiply -composite \
    "$work_dir/black-shield-cropped-mask.png"
magick "$work_dir/black-shield-cropped-mask.png" \
    -trim +repage \
    -resize 400x400 -gravity center -background black -extent 512x512 \
    "$work_dir/black-shield-mask.png"

for symbol in skull white-shield black-shield; do
    magick -size 512x512 'xc:#201813' "$work_dir/$symbol-mask.png" \
        -alpha off -compose CopyOpacity -composite \
        "$output_dir/$symbol.png"
done

# The movement dice use ordinary round ivory pips rather than printed art. A
# dark outer well and smaller painted center make them read as recessed pips on
# the red plastic without adding block geometry above the die face.
magick -size 512x512 xc:none \
    -fill '#470b08' -draw 'circle 256,256 256,54' \
    -fill '#f7dda3' -draw 'circle 256,256 256,82' \
    "$output_dir/movement-pip.png"

echo "extracted original-US die decals to $output_dir"
