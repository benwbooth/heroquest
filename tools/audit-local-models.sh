#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd -- "$(dirname -- "$0")/.." && pwd)
manifest="$repo_dir/assets/model-sources.json"
default_model_dir="$repo_dir/assets/local/editions/original-us/models"
model_dir=${1:-${HEROQUEST_MODEL_DIR:-$default_model_dir}}
art_dir=$(dirname -- "$model_dir")

if [[ ! -f "$manifest" ]]; then
  printf 'model source manifest not found: %s\n' "$manifest" >&2
  exit 2
fi
if ! command -v jq >/dev/null 2>&1; then
  printf 'jq is required to audit the model manifest\n' >&2
  exit 2
fi

missing=0
ready=0
while IFS=$'\t' read -r id category path count additional_paths runtime_root; do
  base_dir=$model_dir
  if [[ "$runtime_root" == art ]]; then
    base_dir=$art_dir
  fi
  slot_missing=0
  paths=("$path")
  if [[ "$additional_paths" != - ]]; then
    IFS='|' read -r -a extra <<< "$additional_paths"
    paths+=("${extra[@]}")
  fi
  bytes=0
  for required_path in "${paths[@]}"; do
    if [[ ! -f "$base_dir/$required_path" ]]; then
      slot_missing=1
      break
    fi
    bytes=$((bytes + $(stat -c '%s' "$base_dir/$required_path")))
  done
  if (( slot_missing == 0 )); then
    printf 'ready    %-38s %8s bytes  %s\n' "$id" "$bytes" "${paths[*]}"
    ready=$((ready + 1))
  else
    printf 'missing  %-38s count=%-3s  %s\n' "$id" "$count" "${paths[*]}"
    missing=$((missing + 1))
  fi
done < <(
  jq -r '.slots[] | select(.category != "optional-enhancement") | [.id, .category, .runtime_path, (.physical_count | tostring), ((.additional_runtime_paths // []) | if length == 0 then "-" else join("|") end), (.runtime_root // "models")] | @tsv' "$manifest"
)

optional_missing=0
while IFS=$'\t' read -r id path; do
  if [[ -f "$model_dir/$path" ]]; then
    printf 'optional %-38s ready       %s\n' "$id" "$path"
  else
    printf 'optional %-38s not built   %s\n' "$id" "$path"
    optional_missing=$((optional_missing + 1))
  fi
done < <(
  jq -r '.slots[] | select(.category == "optional-enhancement") | [.id, .runtime_path] | @tsv' "$manifest"
)

expected=$(jq '[.slots[] | select(.category != "optional-enhancement")] | length' "$manifest")
printf '\nRuntime models: %s/%s ready; %s required slots missing; %s optional enhancements missing.\n' \
  "$ready" "$expected" "$missing" "$optional_missing"

if (( missing > 0 )); then
  exit 1
fi
