#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd -- "$(dirname -- "$0")/.." && pwd)
default_art_dir="$repo_dir/assets/local/editions/original-us"
if [[ ! -d "$default_art_dir" ]]; then
  default_art_dir="$repo_dir/assets/local"
fi
art_dir=${1:-${HEROQUEST_ART_DIR:-$default_art_dir}}
missing=0

check_one() {
  label=$1
  shift
  for candidate in "$@"; do
    if [[ -f "$art_dir/$candidate" ]]; then
      printf 'ok       %-18s %s\n' "$label" "$candidate"
      return
    fi
  done
  printf 'missing  %-18s %s\n' "$label" "$*"
  missing=$((missing + 1))
}

# A loaded GLB supersedes the optional camera-facing PNG for the same runtime
# key. Report the actual renderer selection rather than calling a populated 3D
# slot a procedural fallback merely because its old 2D safety net is absent.
check_model_or_sprite() {
  label=$1
  model=$2
  shift 2
  if [[ -f "$art_dir/models/$model" ]]; then
    printf 'model    %-18s %s\n' "$label" "models/$model"
    return
  fi
  check_one "$label" "$@"
}

check_one board board-scan.png board-scan.jpg board-scan.jpeg
check_model_or_sprite barbarian figures/barbarian.glb figures/barbarian.png
check_model_or_sprite dwarf figures/dwarf.glb figures/dwarf.png
check_model_or_sprite elf figures/elf.glb figures/elf.png
check_model_or_sprite wizard figures/wizard.glb figures/wizard.png
check_model_or_sprite goblin figures/goblin-sword.glb figures/goblin.png
check_model_or_sprite orc figures/orc-sword.glb figures/orc.png
check_model_or_sprite fimir figures/fimir.glb figures/fimir.png figures/abomination.png
check_model_or_sprite skeleton figures/skeleton.glb figures/skeleton.png
check_model_or_sprite zombie figures/zombie.glb figures/zombie.png
check_model_or_sprite mummy figures/mummy.glb figures/mummy.png
check_model_or_sprite chaos-warrior figures/chaos-warrior.glb figures/chaos-warrior.png figures/dread-warrior.png
check_model_or_sprite gargoyle figures/gargoyle.glb figures/gargoyle.png
check_model_or_sprite chaos-sorcerer figures/chaos-warlock.glb figures/chaos-sorcerer.png figures/dread-sorcerer.png
check_one stairs components/markers/stairs.png
check_model_or_sprite table furniture/table.glb props/table.png
check_model_or_sprite chest furniture/treasure-chest.glb props/chest.png
check_model_or_sprite bookcase furniture/bookcase.glb props/bookcase.png
check_model_or_sprite throne furniture/throne.glb props/throne.png
check_model_or_sprite weapon-rack furniture/weapons-rack.glb props/weapon-rack.png
check_model_or_sprite alchemists-bench furniture/alchemists-bench.glb props/alchemists-bench.png
check_model_or_sprite tomb furniture/tomb.glb props/tomb.png
check_model_or_sprite sorcerers-table furniture/sorcerers-table.glb props/sorcerers-table.png
check_model_or_sprite torture-rack furniture/torture-rack.glb props/torture-rack.png
check_model_or_sprite fireplace furniture/fireplace.glb props/fireplace.png
check_model_or_sprite cupboard furniture/cupboard.glb props/cupboard.png
check_one dressing-rat models/dressing/rat.glb
check_one dressing-skull models/dressing/skull.glb
check_one door-open components/doors/open.png
check_one door-closed components/doors/closed.png
check_one insert-table components/furniture/table.png
check_one insert-chest components/furniture/chest.png
check_one insert-bookcase components/furniture/bookcase.png
check_one insert-throne components/furniture/throne.png
check_one insert-alchemist components/furniture/alchemists-bench.png
check_one insert-tomb components/furniture/tomb.png
check_one insert-sorcerer components/furniture/sorcerers-table.png
check_one insert-rack components/furniture/torture-rack.png
check_one insert-fireplace components/furniture/fireplace.png
check_one insert-cupboard components/furniture/cupboard.png
check_one marker-blocked components/markers/blocked-square.png
check_one marker-blocked-2x components/markers/blocked-double.png
check_one marker-pit components/markers/pit.png
check_one marker-falling components/markers/falling-block.png
check_one marker-secret components/markers/secret-door.png
check_one marker-skull components/markers/skull.png
check_one screen-front screen/information-screen-front.png
check_one screen-back screen/information-screen-back.png
check_one die-skull dice/skull.png
check_one die-hero-shield dice/white-shield.png
check_one die-monster-shield dice/black-shield.png
check_one die-movement-pip dice/movement-pip.png

quest_pages=0
rulebook_pages=0
card_sheets=0
tile_sheets=0
if [[ -d "$art_dir/quest-pages" ]]; then
  quest_pages=$(find "$art_dir/quest-pages" -maxdepth 1 -type f -name 'page-*.png' | wc -l)
fi
if [[ -d "$art_dir/rulebook-pages" ]]; then
  rulebook_pages=$(find "$art_dir/rulebook-pages" -maxdepth 1 -type f -name 'page-*.png' | wc -l)
fi
if [[ -d "$art_dir/card-sheets" ]]; then
  card_sheets=$(find "$art_dir/card-sheets" -maxdepth 1 -type f -name 'page-*.png' | wc -l)
fi
if [[ -d "$art_dir/tile-sheets" ]]; then
  tile_sheets=$(find "$art_dir/tile-sheets" -maxdepth 1 -type f -name 'page-*.png' | wc -l)
fi
printf 'pages     %-18s %s\n' quest-book "$quest_pages"
printf 'pages     %-18s %s\n' rulebook "$rulebook_pages"
printf 'pages     %-18s %s\n' card-sheets "$card_sheets"
printf 'pages     %-18s %s\n' tile-sheets "$tile_sheets"

if (( missing > 0 )); then
  printf '\n%s required runtime visual slots are still using procedural fallbacks.\n' "$missing"
  exit 1
fi
printf '\nAll audited runtime visual slots are populated by scans, decals, or 3D models.\n'
