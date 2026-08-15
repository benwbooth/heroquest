#!/usr/bin/env bash
set -euo pipefail

root="${HEROQUEST_ART_ROOT:-assets/local/editions/original-us}"
box="$root/box-pages/page-1.png"
box_back="$root/box-pages/page-2.png"
tiles="$root/tile-sheets/page-4.png"
output="$root/startup"

for source in "$box" "$box_back" "$tiles"; do
    if [[ ! -f "$source" ]]; then
        printf 'missing original-US scan: %s\n' "$source" >&2
        exit 1
    fi
done

mkdir -p "$output/box" "$output/heroes" "$output/quests"

# The box PDF is a flattened packaging dieline. These crops preserve the six
# printed faces while removing the glue/bleed areas that do not belong on the
# closed box model.
magick "$box" -crop 5960x3780+660+660 +repage -resize '1536x1536>' -quality 92 "$output/box/top.jpg"
magick "$box_back" -crop 5930x3760+646+646 +repage -resize '1536x1536>' -quality 92 "$output/box/bottom.jpg"
magick "$box" -crop 5960x650+660+0 +repage -rotate 180 -resize '1536x1536>' -quality 92 "$output/box/north.jpg"
magick "$box" -crop 5960x638+660+4448 +repage -resize '1536x1536>' -quality 92 "$output/box/south.jpg"
magick "$box" -crop 660x3780+0+660 +repage -rotate -90 -resize '1536x1536>' -quality 92 "$output/box/west.jpg"
magick "$box" -crop 657x3780+6620+660 +repage -rotate 90 -resize '1536x1536>' -quality 92 "$output/box/east.jpg"

# The four character cards are printed together on tile sheet four. Keep the
# original card faces intact so the setup screen shows the real stats, weapons,
# armor restrictions, and character illustrations.
magick "$tiles" -crop 1150x1500+720+40 +repage -resize '600x900>' -quality 92 "$output/heroes/barbarian.jpg"
magick "$tiles" -crop 1160x1500+1900+40 +repage -resize '600x900>' -quality 92 "$output/heroes/elf.jpg"
magick "$tiles" -crop 1220x1500+3090+40 +repage -resize '600x900>' -quality 92 "$output/heroes/dwarf.jpg"
magick "$tiles" -crop 1390x1500+4310+40 +repage -resize '600x900>' -quality 92 "$output/heroes/wizard.jpg"

# Quest selection must show only the parchment text that Zargon reads aloud.
# The upper map and lower Quest Notes remain hidden from the hero player.
for quest in $(seq 1 14); do
    page=$(printf '%02d' $((quest + 2)))
    magick "$root/quest-pages/page-$page.png" \
        -crop 2540x850+92+1770 +repage \
        -resize '1200x500>' -quality 92 \
        "$output/quests/quest-$(printf '%02d' "$quest").jpg"
done

printf 'Extracted original-US startup art under %s\n' "$output"
