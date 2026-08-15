#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
art_root=${1:-${HEROQUEST_ART_DIR:-"$repo_root/assets/local/editions/original-us"}}
missing=0

check_file() {
    local label=$1
    local relative=$2
    if [[ -f "$art_root/$relative" ]]; then
        printf 'ok       %-28s %s\n' "$label" "$relative"
    else
        printf 'missing  %-28s %s\n' "$label" "$relative"
        missing=$((missing + 1))
    fi
}

check_count() {
    local label=$1
    local directory=$2
    local pattern=$3
    local expected=$4
    local actual=0
    if [[ -d "$art_root/$directory" ]]; then
        actual=$(find "$art_root/$directory" -maxdepth 1 -type f -name "$pattern" | wc -l)
    fi
    if (( actual == expected )); then
        printf 'count    %-28s %s/%s\n' "$label" "$actual" "$expected"
    else
        printf 'missing  %-28s %s/%s files matching %s/%s\n' \
            "$label" "$actual" "$expected" "$directory" "$pattern"
        missing=$((missing + 1))
    fi
}

source_documents=(
    'Box.pdf'
    'Cards.pdf'
    'Character Sheet.pdf'
    'Gameboard.pdf'
    'Hero Quest Quest Packs Order Form.pdf'
    'Identification Guide and Armory.pdf'
    'Instruction Booklet - Booklet Print.pdf'
    'Instruction Booklet - Computer View.pdf'
    'Poster 1.pdf'
    'Poster 2.pdf'
    'Poster 3.pdf'
    'Poster 4.pdf'
    'Quest Book - Booklet Print.pdf'
    'Quest Book - Computer View.pdf'
    'Survey Form.pdf'
    'Tile Sheets.pdf'
)
for document in "${source_documents[@]}"; do
    check_file "source.$document" "scans/$document"
done

check_file board.full-resolution board-scan.jpg
check_file board.runtime board-runtime.jpg
check_file board.calibration board-calibration.json
check_file board.runtime-calibration board-runtime-calibration.json
check_count pages.card-sheets card-sheets 'page-*.png' 16
check_count pages.quest-book quest-pages 'page-*.png' 19
check_count pages.rulebook rulebook-pages 'page-*.png' 13
check_count pages.tile-sheets tile-sheets 'page-*.png' 4
check_count pages.box box-pages 'page-*.png' 2
check_count pages.posters poster-pages 'poster-*-page-*.png' 9
check_count pages.extras extras-pages '*.png' 3
check_file pages.character-sheet character-sheet/character-sheet.png
check_file pages.armory armory-pages/identification-guide-and-armory.png

for face in top bottom north south east west; do
    check_file "startup.box-$face" "startup/box/$face.jpg"
done
for hero in barbarian dwarf elf wizard; do
    check_file "startup.hero-$hero" "startup/heroes/$hero.jpg"
done
check_count startup.quest-intros startup/quests 'quest-*.jpg' 14

check_file tabletop.character-sheet tabletop/player/character-sheet.png
check_file tabletop.armory tabletop/player/armory.jpg
check_count tabletop.elemental-spells tabletop/spells '*.jpg' 12
check_count tabletop.zargon-decks tabletop/zargon '*.jpg' 4
check_count tabletop.monster-cards tabletop/monsters '*.jpg' 8
check_count tabletop.quest-book tabletop/quest-book 'quest-*.jpg' 14

check_file screen.front screen/information-screen-front.png
check_file screen.back screen/information-screen-back.png
check_file dice.skull dice/skull.png
check_file dice.hero-shield dice/white-shield.png
check_file dice.monster-shield dice/black-shield.png
check_file dice.movement-pip dice/movement-pip.png

check_file door.open components/doors/open.png
check_file door.closed components/doors/closed.png
for furniture in \
    table chest bookcase throne alchemists-bench tomb sorcerers-table \
    torture-rack fireplace cupboard; do
    check_file "furniture.$furniture" "components/furniture/$furniture.png"
done
for marker in stairs blocked-square blocked-double pit falling-block secret-door skull; do
    check_file "marker.$marker" "components/markers/$marker.png"
done

if (( missing > 0 )); then
    printf '\n%s original-US scan asset groups are incomplete.\n' "$missing" >&2
    exit 1
fi
printf '\nEvery original-US source document and scan-derived runtime asset is present.\n'
