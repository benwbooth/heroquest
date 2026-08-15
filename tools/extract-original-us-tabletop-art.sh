#!/usr/bin/env bash
set -euo pipefail

root="${HEROQUEST_ART_ROOT:-assets/local/editions/original-us}"
output="$root/tabletop"

required=(
    "$root/character-sheet/character-sheet.png"
    "$root/armory-pages/identification-guide-and-armory.png"
    "$root/card-sheets/page-01.png"
    "$root/card-sheets/page-06.png"
    "$root/card-sheets/page-08.png"
    "$root/card-sheets/page-09.png"
    "$root/card-sheets/page-11.png"
    "$root/card-sheets/page-14.png"
    "$root/card-sheets/page-15.png"
    "$root/card-sheets/page-16.png"
)
for path in "${required[@]}"; do
    if [[ ! -f "$path" ]]; then
        printf 'missing original-US scan: %s\n' "$path" >&2
        exit 1
    fi
done

mkdir -p \
    "$output/player" \
    "$output/spells" \
    "$output/zargon" \
    "$output/monsters" \
    "$output/quest-book"

crop_card() {
    local source=$1
    local column=$2
    local row=$3
    local destination=$4
    local x=$((145 + column * 780))
    local y=$((175 + row * 1060))
    magick "$source" -crop "625x930+${x}+${y}" +repage \
        -resize 400x600 -strip -quality 90 "$destination"
}

magick "$root/character-sheet/character-sheet.png" \
    -crop 1130x1690+38+38 +repage -resize 670x1000 -strip \
    "$output/player/character-sheet.png"
magick "$root/armory-pages/identification-guide-and-armory.png" \
    -resize 1200x1000 -strip -quality 90 "$output/player/armory.jpg"

for column in 0 1 2; do
    card=$((column + 1))
    crop_card "$root/card-sheets/page-06.png" "$column" 2 \
        "$output/spells/air-$card.jpg"
    crop_card "$root/card-sheets/page-08.png" "$column" 1 \
        "$output/spells/fire-$card.jpg"
    crop_card "$root/card-sheets/page-08.png" "$column" 2 \
        "$output/spells/water-$card.jpg"
    crop_card "$root/card-sheets/page-08.png" "$column" 0 \
        "$output/spells/earth-$card.jpg"
done

crop_card "$root/card-sheets/page-01.png" 0 0 "$output/zargon/treasure-back.jpg"
crop_card "$root/card-sheets/page-11.png" 0 0 "$output/zargon/artifact-back.jpg"
crop_card "$root/card-sheets/page-09.png" 0 0 "$output/zargon/dread-spell-back.jpg"
crop_card "$root/card-sheets/page-15.png" 0 0 "$output/zargon/monster-back.jpg"

crop_card "$root/card-sheets/page-14.png" 1 1 "$output/monsters/chaos-warrior.jpg"
crop_card "$root/card-sheets/page-14.png" 2 1 "$output/monsters/fimir.jpg"
crop_card "$root/card-sheets/page-14.png" 0 2 "$output/monsters/gargoyle.jpg"
crop_card "$root/card-sheets/page-14.png" 1 2 "$output/monsters/goblin.jpg"
crop_card "$root/card-sheets/page-14.png" 2 2 "$output/monsters/mummy.jpg"
crop_card "$root/card-sheets/page-16.png" 0 0 "$output/monsters/orc.jpg"
crop_card "$root/card-sheets/page-16.png" 1 0 "$output/monsters/skeleton.jpg"
crop_card "$root/card-sheets/page-16.png" 2 0 "$output/monsters/zombie.jpg"

for quest in $(seq 1 14); do
    page=$(printf '%02d' $((quest + 2)))
    destination=$(printf '%s/quest-%02d.jpg' "$output/quest-book" "$quest")
    magick "$root/quest-pages/page-$page.png" \
        -resize '900x1200>' -strip -quality 88 "$destination"
done

printf 'Extracted original-US tabletop art under %s\n' "$output"
