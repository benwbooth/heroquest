# HeroQuest 3D board project

This repository is a native Rust implementation of the original-US physical
dungeon board: SDL3 owns the window and input, wgpu renders the table and
pieces, Rapier simulates dice, and a deterministic rules core drives Heroes and
the computer game master.

The target is the 14-quest original North American/US Game System campaign,
with its rules, names, component mix, and physical presentation kept distinct
from both the original UK edition and the 2021 remake. The engine and original
demo content are source-controlled. Copyrighted scans, card faces, quest prose,
logos, and extracted textures are deliberately kept in `assets/local/`, which
is gitignored. This is a fan project and is not affiliated with or endorsed by
Hasbro or Avalon Hill.

## Implemented game

- A 26 by 19 room-and-corridor board loaded from JSON.
- Four heroes with their base movement, attack, defend, Body, and Mind stats.
- Orthogonal movement, room walls, free door opening, collision, turn order,
  combat-die resolution, death, return-to-stairs objective, and a first-pass
  computer game master that paths monsters toward heroes and attacks.
- The installed high-resolution original-US board scan rendered as an sRGB
  texture over calibrated 3D geometry.
- All required classic figure, furniture, door, dressing, movement-die, and
  combat-die GLBs, plus the original scan-backed cardboard markers and flat
  stairway cutout. Runtime fails with an actionable missing-asset error instead
  of substituting a procedural game piece.
- The original-US folded Information Screen, extracted from the private scan
  and rendered as a double-sided three-panel cardboard piece behind the board.
- 3D doors, figure bases, board relief, and physical dice above the scanned art.
- A real Rapier rigid-body dice tray. Outcomes come from the upward face after
  the dice settle; they are not selected before the throw. Hero and Zargon
  attacks both pass through the visible tray instead of resolving offscreen.
  Spear/falling-block traps and Wandering Monster attacks use the same visible
  path. Earth-scaled gravity, a high release, invisible table-edge containment,
  and force-derived die-on-wood impacts give every throw tabletop weight.
- Four persistent table-edge Hero stations at left, lower-left, lower-right,
  and right. Each uses the original scanned Character Card and an individual
  Character Sheet, plus movement/combat dice, an action-reference card, an
  active-player rim, and live Body/gold/potion tokens. The Elf and Wizard also
  receive their selected original-US elemental spell-card hands.
- A complete physical Zargon station behind the Information Screen: Treasure,
  Artifact, Dread/Chaos Spell, and Monster stacks; all eight face-up Monster
  cards; and an open Quest Book showing the selected quest's original-US map,
  read-aloud text, and hidden notes. The shared Identification Guide and Armory
  sits along the Hero edge.
- Time-based figure presentation: every move follows a lifted hop arc, figures
  turn and lunge on attacks, damage causes recoil, and defeated pieces topple
  before they leave the table.
- The exact 24-card US Treasure deck, including per-quest card removal,
  returning Hazard/Wandering Monster cards, potion and gold rewards, trap
  damage, and immediate Wandering Monster placement and attack.
- Center-to-center visibility with walls, closed doors, and figures blocking
  sight; room/corridor searches; concealed and discovered secret doors; and
  pit, falling-block, and spear traps with physical jump/disarm rolls.
- Original-US starting weapons, Armory catalog data, armor/weapon conflicts,
  Plate Mail movement, and pit combat penalties in the rules core.
- A scan-backed 3D box opening, quest selection, player assignment, spell
  division, and ready flow. Menus are mouse-operable and remain contained in
  resized windows. Game UI uses the bundled, readable medieval Almendra typeface
  under the SIL Open Font License 1.1.
- Exact built-in maps, placement data, objectives, and printed quest-note logic
  for all 14 original-US Game System quests, selectable from the opening quest
  book and playable as one persistent campaign through the Champion reward.
- A camera-matched Gothic great hall built as a hybrid set: the intricate
  fireplace, vaults, columns, tapestries, rugs, windows, braziers, moonlight,
  and chandelier come from a high-resolution room plate, while the oak table,
  board, screen, figures, dice, and their depth relationships remain real 3D.
  Warm pixels in the room plate flicker in the shader, steep inspection angles
  dim its overhead fixtures, and the orbit is constrained to the camera range
  where the perspective remains convincing. The old nested inner/outer walls
  are gone. The art-direction source and table-free room plate are stored at
  `assets/local/concept/castle-great-hall-reference.png` and
  `assets/local/environment/castle-great-hall-matte-v1.png`.

The scan-derived original-US acceptance matrix is in
`docs/original-us-parity-checklist.md`. Every listed rule, physical component,
quest plot point, campaign transition, and in-game control is implemented and
bound to deterministic regression coverage. `docs/roadmap.md` distinguishes
that completed parity baseline from optional post-parity enhancements.

## Build and run

```sh
nix develop
cargo test
cargo run
```

After importing the private original-US scans, run
`tools/extract-original-us-tabletop-art.sh` once. It creates a compact runtime
cache of the individual cards, record sheet, Armory, and Quest Book pages so
the renderer does not decode the huge master scans during startup.

The Nix shell supplies SDL3 and the Vulkan/Wayland/X11 libraries; Cargo owns the
Rust dependency graph.

Run any compatible local quest JSON with:

```sh
cargo run -- --quest assets/local/quests/quest-01.json
```

## Controls

Before play, click the box and use the labeled buttons, Hero rows, player-owner
buttons, and spell-group panels. `Enter` confirms and `Esc` goes back. During
Hero naming, ordinary letters and spaces are text input; only arrow keys
navigate the setup.

- `R`: physically roll the active hero's two movement dice
- Arrow keys or `W A S D`: move one square
- `O`: open an adjacent door (free, as in the board game)
- `F`: attack an adjacent monster
- `T`: search the current monster-free room for Treasure
- `C`: drink a Potion of Healing and restore up to four lost Body Points
- `B`: drink The Lost Wizard's unidentified purple potion (if carried)
- `K`: pick up an adjacent quest chest or other quest item
- `G`: put down the carried quest item
- `V`: pass the carried quest item to the lowest-turn-order adjacent empty-handed Hero
- `L`: search the current room/corridor for secret doors
- `P`: search the current room/corridor for traps
- `X`: disarm an adjacent discovered trap (Dwarf or Tool Kit; roll movement first)
- `J`, then a direction: physically roll to jump a discovered trap
- `E` or `Enter`: end the active hero's turn
- Left-drag: orbit the camera
- Mouse wheel: zoom
- Trackpad or touchscreen pinch: zoom
- `H`: reset the camera
- `Esc`: quit

The ornamented in-world OSD reports the active Hero, movement, Body points,
rules events, and context-sensitive clickable actions. The window title stays
limited to the game and quest name.

## Complete first-run assets

On a clean first run, the game requires the complete audited asset set rather
than stopping after the board artwork. After the warning is accepted it:

1. downloads `HQ Game System US.rar` directly from
   `heroquestadventure.com`, verifies its SHA-256, and creates every
   scan-backed texture;
2. downloads the classic STL collection directly from its public Google Drive
   folder, converts the figures, furniture, traps, and optional walls to GLB;
3. builds the project-authored dice, door stands, markers, fittings, weapon
   rack, and quest-specific Orc variants; and
4. audits every required and optional runtime slot before launching.

The Gothic room panorama and foreground GLB are bundled under
`assets/environment/`, so a clean installation does not depend on an AI service
or generated-image cache. The external collections are never hosted or relayed
by this repository. Run the complete installer explicitly with:

```sh
tools/install-all-assets.sh --accept-liability
```

External downloads total about 2.45 GiB and the prepared private pack needs
roughly 7 GiB. The Nix development shell supplies `curl`, `unrar-free`,
Poppler, ImageMagick, `uv`, `gdown`, Blender, and `jq` for the source-tree
installer. Transfers resume after interruption.

The scan-only lower-level command remains available as
`tools/install-original-us-scan-pack.sh`.

The complete source map is machine-readable at `assets/asset-sources.json`.
It accounts separately for the original-US printed art, external STL geometry,
project-authored 3D complements, castle environment, UI font, synthesized
audio, and executable rules/quest data. `tools/audit-original-us-scan-art.sh`
checks every source document and derived scan asset rather than sampling a few
files, while `tools/audit-all-assets.sh` verifies the complete cross-family
set and the stable bundled-asset hashes.

The default private art root is `assets/local/editions/original-us/`.

To use your own files instead, run
`tools/import-local-assets.sh` with a legally obtained quest-book PDF and,
optionally, a scan/photo of your physical board and a rulebook PDF:

```sh
tools/import-local-assets.sh /path/to/quest-book.pdf /path/to/board.png /path/to/rulebook.pdf
```

The manual importer never downloads anything, and no installer stages its
result for Git. A copied `board-scan.png`, `board-scan.jpg`, or
`board-scan.jpeg` in the selected edition directory is detected automatically
on the next run. You can instead point at any image without copying it:

```sh
HEROQUEST_BOARD_SCAN=/path/to/cropped-board.png cargo run
```

The image should be cropped to the outside of the playable board and oriented
with the top row at the top. It is mapped over the 26 by 19 board; doors,
figures, furniture, and physical dice remain 3D. See
`docs/local-art-pack.md`, `docs/content-boundary.md`, and `docs/roadmap.md`.

Audit every board, figure, and furniture slot with:

```sh
tools/audit-local-art.sh
```

Extract the two Information Screen faces from the already installed original-US
tile sheets with:

```sh
tools/extract-original-us-screen.sh
```

Extract and create optimized runtime derivatives for the box, Character Cards,
and quest-introduction parchment without modifying the source scans:

```sh
tools/extract-original-us-startup-art.sh
```

Regenerate the exact combat-die symbols from printed rulebook pages 14 and 22,
plus the recessed movement-die pip decal, with:

```sh
tools/extract-original-us-dice-decals.sh
```

Regenerate the cardboard faces fitted over the door, furniture, pit,
falling-block, secret-door, blocked-square, and skull GLBs from tile-sheet
pages 1 through 3 with:

```sh
tools/extract-original-us-components.sh
```

The exact original-US physical inventory, classic sculpt variants, model URLs,
licenses, expected GLB paths, and texture strategy are recorded in
`docs/original-us-piece-inventory.md` and `assets/model-sources.json`. Audit the
private model pack separately with:

```sh
tools/audit-local-models.sh
```

Rebuild the two quest-specific Orc weapon variants from the installed classic
sword-Orc scan, then render inspection images if their geometry changes:

```sh
nix develop -c blender --background --python tools/build-orc-variants.py -- \
  --source assets/local/editions/original-us/models/figures/orc-sword.glb \
  --output-root assets/local/editions/original-us/models
nix develop -c blender --background --python tools/render-model-preview.py -- \
  assets/local/editions/original-us/models/figures/orc-notched-sword.glb \
  /tmp/heroquest-orc-notched.png
```

Rebuild the project-authored physical pieces—including the photograph-matched
four rat and four skull furniture fittings—with:

```sh
nix develop -c blender --background --python tools/build-project-models.py -- \
  --output-root assets/local/editions/original-us/models
```

Set `HEROQUEST_ART_DIR` to test another edition without mixing its components
with the original US pack.

## Castle room set

Rebuild the local Blender foreground scene, GLB, and preview from the checked-in
modeling script:

```sh
blender --background --python tools/build-castle-room.py
```

The generated working files live under `assets/local/environment/` and are
gitignored. The stable runtime panorama, GLB, Blender source, preview, and
textures are also checked in under `assets/environment/`. The game prefers a
local working copy and otherwise uses the bundled set. To try another licensed
or locally generated foreground model or room panorama, set:

```sh
HEROQUEST_ROOM_MODEL=/path/to/castle-room.glb cargo run
HEROQUEST_ROOM_PANORAMA=/path/to/equirectangular-room.png cargo run
```

## License

The repository contributors dedicate their original contributions to the
public domain under [CC0 1.0 Universal](LICENSE). See [NOTICE.md](NOTICE.md) for
the precise scope and the third-party font, HeroQuest intellectual property,
scan-derived data, and private local-asset exclusions. CC0 does not grant rights
in material the contributors do not own.
