#!/usr/bin/env bash
set -euo pipefail

api_root="https://api.worldlabs.ai/marble/v1"
image_path="${1:-assets/local/concept/castle-great-hall-reference.png}"
output_dir="${2:-assets/local/worldlabs/castle-great-hall}"
api_key="${WORLDLABS_API_KEY:-${WLT_API_KEY:-}}"

if [[ -z "$api_key" ]]; then
    printf 'Set WORLDLABS_API_KEY (or WLT_API_KEY) before running this command.\n' >&2
    exit 2
fi
if [[ ! -f "$image_path" ]]; then
    printf 'Reference image does not exist: %s\n' "$image_path" >&2
    exit 2
fi
for command in curl jq; do
    if ! command -v "$command" >/dev/null 2>&1; then
        printf 'Required command is unavailable: %s\n' "$command" >&2
        exit 2
    fi
done

mkdir -p "$output_dir"
file_name=$(basename "$image_path")
extension=${file_name##*.}
extension=$(printf '%s' "$extension" | tr '[:upper:]' '[:lower:]')
case "$extension" in
    jpg|jpeg|png|webp) ;;
    *)
        printf 'World Labs requires a jpg, jpeg, png, or webp reference image.\n' >&2
        exit 2
        ;;
esac

prompt='Preserve the reference image composition and architecture as closely as possible: an enormous ominous Gothic castle great hall surrounding a central HeroQuest gaming table. Reconstruct the exact ribbed stone vaults, carved columns and capitals, pointed arches, massive fireplace, circular iron chandelier, heraldic tapestries, tall moonlit windows, floor stones, rugs, braziers, blue moonbeams, warm firelight, deep shadows, atmospheric haze, and intricate medieval ornament. Keep the central table rectangular, level, unobstructed, and large enough for the complete HeroQuest board and miniatures. Do not modernize, simplify, brighten, or change the visual style.'

printf 'Preparing authenticated image upload...\n'
prepare_payload=$(jq -n \
    --arg file_name "$file_name" \
    --arg extension "$extension" \
    '{file_name: $file_name, kind: "image", extension: $extension}')
prepare_response=$(curl --fail-with-body --silent --show-error \
    -X POST "$api_root/media-assets:prepare_upload" \
    -H 'Content-Type: application/json' \
    -H "WLT-Api-Key: $api_key" \
    --data "$prepare_payload")
media_asset_id=$(jq -er '.media_asset.id' <<<"$prepare_response")
upload_url=$(jq -er '.upload_info.upload_url' <<<"$prepare_response")
upload_method=$(jq -er '.upload_info.upload_method // "PUT"' <<<"$prepare_response")

upload_args=(--fail-with-body --silent --show-error -X "$upload_method" "$upload_url")
while IFS=$'\t' read -r header value; do
    upload_args+=(-H "$header: $value")
done < <(jq -r '.upload_info.required_headers // {} | to_entries[] | [.key, .value] | @tsv' <<<"$prepare_response")
upload_args+=(--data-binary "@$image_path")
curl "${upload_args[@]}" >/dev/null

printf 'Starting Marble 1.1 world generation...\n'
generation_payload=$(jq -n \
    --arg media_asset_id "$media_asset_id" \
    --arg prompt "$prompt" \
    '{
        display_name: "HeroQuest Gothic Great Hall",
        model: "marble-1.1",
        world_prompt: {
            type: "image",
            image_prompt: {
                source: "media_asset",
                media_asset_id: $media_asset_id
            },
            text_prompt: $prompt
        }
    }')
operation=$(curl --fail-with-body --silent --show-error \
    -X POST "$api_root/worlds:generate" \
    -H 'Content-Type: application/json' \
    -H "WLT-Api-Key: $api_key" \
    --data "$generation_payload")
operation_id=$(jq -er '.operation_id' <<<"$operation")

printf 'Generation operation: %s\n' "$operation_id"
while [[ $(jq -r '.done' <<<"$operation") != true ]]; do
    status=$(jq -r '.metadata.progress.description // .metadata.progress.status // "in progress"' <<<"$operation")
    printf '%s\n' "$status"
    sleep 10
    operation=$(curl --fail-with-body --silent --show-error \
        -H "WLT-Api-Key: $api_key" \
        "$api_root/operations/$operation_id")
done

if [[ $(jq -r '.error != null' <<<"$operation") == true ]]; then
    jq '.error' <<<"$operation" >&2
    exit 1
fi

world_id=$(jq -er '.metadata.world_id // .response.id' <<<"$operation")
world=$(curl --fail-with-body --silent --show-error \
    -H "WLT-Api-Key: $api_key" \
    "$api_root/worlds/$world_id")
printf '%s\n' "$world" | jq '.' >"$output_dir/world.json"

download_asset() {
    local query=$1
    local destination=$2
    local url
    url=$(jq -r "$query // empty" <<<"$world")
    if [[ -n "$url" ]]; then
        printf 'Downloading %s...\n' "$destination"
        curl --fail-with-body --location --silent --show-error \
            "$url" -o "$output_dir/$destination"
    fi
}

download_asset '.world.assets.splats.spz_urls.full_res' 'castle-great-hall-full.spz'
download_asset '.world.assets.splats.spz_urls."500k"' 'castle-great-hall-500k.spz'
download_asset '.world.assets.splats.spz_urls."100k"' 'castle-great-hall-100k.spz'
download_asset '.world.assets.mesh.collider_mesh_url' 'castle-great-hall-collider.glb'
download_asset '.world.assets.imagery.pano_url' 'castle-great-hall-pano.png'

world_url=$(jq -r '.world.world_marble_url // empty' <<<"$world")
printf 'Castle world downloaded to %s\n' "$output_dir"
if [[ -n "$world_url" ]]; then
    printf 'Marble preview: %s\n' "$world_url"
fi
