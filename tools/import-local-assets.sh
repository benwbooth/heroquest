#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 || $# -gt 3 ]]; then
  echo "usage: $0 QUEST_BOOK.pdf [BOARD_SCAN] [RULEBOOK.pdf]" >&2
  exit 2
fi

quest_pdf=$1
board_scan=${2:-}
rulebook_pdf=${3:-}
repo_dir=$(cd -- "$(dirname -- "$0")/.." && pwd)
local_dir=${HEROQUEST_ART_DIR:-"$repo_dir/assets/local/editions/original-us"}

if [[ ! -f "$quest_pdf" ]]; then
  echo "quest book not found: $quest_pdf" >&2
  exit 1
fi

mkdir -p "$local_dir/quest-pages"
nix shell nixpkgs#poppler-utils -c pdftoppm \
  -png -r 300 "$quest_pdf" "$local_dir/quest-pages/page"

if [[ -n "$rulebook_pdf" ]]; then
  if [[ ! -f "$rulebook_pdf" ]]; then
    echo "rulebook not found: $rulebook_pdf" >&2
    exit 1
  fi
  mkdir -p "$local_dir/rulebook-pages"
  nix shell nixpkgs#poppler-utils -c pdftoppm \
    -png -r 300 "$rulebook_pdf" "$local_dir/rulebook-pages/page"
fi

if [[ -n "$board_scan" ]]; then
  if [[ ! -f "$board_scan" ]]; then
    echo "board scan not found: $board_scan" >&2
    exit 1
  fi
  extension=$(printf '%s' "${board_scan##*.}" | tr '[:upper:]' '[:lower:]')
  case "$extension" in
    png|jpg|jpeg) ;;
    *)
      echo "board scan must be PNG or JPEG: $board_scan" >&2
      exit 1
      ;;
  esac
  cp -- "$board_scan" "$local_dir/board-scan.$extension"
fi

echo "Imported local-only assets under $local_dir"
echo "These files are gitignored and will not be redistributed."
