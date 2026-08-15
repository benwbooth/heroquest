#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
edition_root="${HEROQUEST_ART_ROOT:-$repo_root/assets/local/editions/original-us}"
page_1="$edition_root/tile-sheets/page-1.png"
page_2="$edition_root/tile-sheets/page-2.png"
page_3="$edition_root/tile-sheets/page-3.png"
output_dir="$edition_root/components"

for source in "$page_1" "$page_2" "$page_3"; do
    if [[ ! -f "$source" ]]; then
        printf 'missing original-US tile-sheet scan: %s\n' "$source" >&2
        exit 1
    fi
done

mkdir -p "$output_dir/doors" "$output_dir/furniture" "$output_dir/markers"

crop_panel() {
    local source=$1
    local geometry=$2
    local destination=$3
    shift 3
    magick "$source" -crop "$geometry" +repage "$@" \
        -resize '1400x1400>' -strip "$destination"
}

crop_silhouette() {
    local source=$1
    local geometry=$2
    local destination=$3
    shift 3
    # Remove only the connected near-black scanner bed surrounding a die-cut
    # component. Printed black mortar and line work inside the component stay.
    magick "$source" -crop "$geometry" +repage \
        -alpha set -bordercolor '#050505' -border 2 -fuzz 12% \
        -fill none -draw 'alpha 0,0 floodfill' -shave 2x2 \
        "$@" -trim +repage -resize '1400x1400>' -strip "$destination"
}

# Door fronts. The open arch needs the connected pale punch-out removed as
# well as the black scanner bed. Coordinates are on the first of eight arches.
crop_silhouette "$page_1" '430x720+805+35' \
    "$output_dir/doors/closed.png"
crop_silhouette "$page_3" '520x735+65+2260' \
    "$output_dir/doors/open.png" \
    -fuzz 18% -fill none -draw 'alpha 175,275 floodfill'

# Furniture cardboard inserts. These are the printed faces/tops that slot into
# the injection-moulded plastic frames represented by the runtime GLBs.
crop_panel "$page_3" '255x1370+5200+40' \
    "$output_dir/furniture/table.png" -rotate -90
crop_panel "$page_3" '190x735+5160+1555' \
    "$output_dir/furniture/chest.png" -rotate -90
crop_panel "$page_3" '740x535+3510+2360' \
    "$output_dir/furniture/bookcase.png"
crop_panel "$page_1" '585x315+1450+800' \
    "$output_dir/furniture/throne.png"
crop_panel "$page_3" '1940x285+3150+1980' \
    "$output_dir/furniture/alchemists-bench.png"
crop_panel "$page_3" '1560x285+55+1980' \
    "$output_dir/furniture/tomb.png"
crop_panel "$page_3" '1660x320+3440+1550' \
    "$output_dir/furniture/sorcerers-table.png"
crop_panel "$page_3" '1425x285+1665+1980' \
    "$output_dir/furniture/torture-rack.png"
crop_silhouette "$page_1" '900x875+2910+15' \
    "$output_dir/furniture/fireplace.png"
crop_panel "$page_1" '1835x720+3860+25' \
    "$output_dir/furniture/cupboard.png"

# Thin, double-sided cardboard tiles. One representative face is sufficient
# for instancing all copies of that exact printed component type. The four-up
# fan-shaped crop at the top-left of page 1 is a set of secret-door tiles; the
# actual 2x2 stairway is the separate square cutout lower on the same sheet.
crop_panel "$page_1" '565x570+128+804' \
    "$output_dir/markers/stairs.png"
crop_panel "$page_2" '270x260+100+955' \
    "$output_dir/markers/blocked-square.png"
crop_panel "$page_1" '530x240+850+870' \
    "$output_dir/markers/blocked-double.png"
crop_panel "$page_1" '315x310+3860+725' \
    "$output_dir/markers/pit.png"
crop_panel "$page_2" '300x315+5050+90' \
    "$output_dir/markers/falling-block.png"
crop_panel "$page_2" '300x330+1250+710' \
    "$output_dir/markers/secret-door.png"
crop_panel "$page_3" '285x265+55+1640' \
    "$output_dir/markers/skull.png"

printf 'extracted original-US component faces to %s\n' "$output_dir"
