# Original US physical-piece and 3D-model inventory

This inventory targets the Milton Bradley North American/US Game System box,
released in 1990 with copyright-1989 quest material. It is based on the contents
page in the installed original-US instruction scan, the four local tile-sheet
scans, and a visual check of the original component galleries. It does not mix
in the 1989 UK rules, the 2021 remake, or expansion pieces.

The machine-readable source and license matrix is
`assets/model-sources.json`. Model files themselves belong in the gitignored
`assets/local/editions/original-us/models/` tree.

## Figures: 35 physical pieces, 18 distinct meshes

| Piece | Box count | Distinct classic sculpts | Located geometry |
| --- | ---: | ---: | --- |
| Barbarian | 1 | 1 | Carlbark classic scan, CC BY 4.0 |
| Dwarf | 1 | 1 | Carlbark classic scan, CC BY 4.0 |
| Elf | 1 | 1 | Carlbark classic scan, CC BY 4.0 |
| Wizard | 1 | 1 | Carlbark classic scan, CC BY 4.0 |
| Orc | 8 | 4 | Sword (3), flail (2), cleaver (2), notched sword (1); CC BY 4.0 scans located |
| Goblin | 6 | 3 | Sword (2), axe (2), scimitar/short-sword (2); CC BY 4.0 scans/remix located |
| Fimir | 3 | 1 | Carlbark classic scan, CC BY 4.0 |
| Chaos Warrior | 4 | 1 | Carlbark classic scan, CC BY 4.0 |
| Chaos Warlock | 1 | 1 | Listed as “Chaos Mage” by the scan source; CC BY 4.0 |
| Gargoyle | 1 | 1 | Body and wings STL pair; CC BY 4.0 |
| Skeleton | 4 | 1 | Carlbark classic scan, CC BY 4.0 |
| Zombie | 2 | 1 | Carlbark classic scan, CC BY 4.0 |
| Mummy | 2 | 1 | Carlbark classic scan, CC BY 4.0 |

The fourth Orc matters: the quest book uses the large notched-sword sculpt for
named Orc warlords. A single generic Orc mesh would therefore lose real game
information as well as physical fidelity.

Quest 6 separately describes Grak as holding an Armory staff even though the
physical box contains no staff-Orc sculpt. The runtime therefore reserves a
quest-specific `figures/orc-staff.glb` kitbash slot; the installed local model
retains the licensed classic sword-Orc scan while replacing the sword with a
clean project-authored staff. It is not counted as a ninth physical Orc.

The installed notched-sword runtime model similarly retains the classic
square-base Orc scan and replaces only the blade. Its larger, straighter blade,
keyhole notch, and heavy tip were measured from original miniature photographs;
the reproducible build lives in `tools/build-orc-variants.py`. This is an honest
scan-derived reconstruction, not a claim that the unavailable exact STL was
photogrammetrically captured in this workspace.

All located figure files are print-oriented STL geometry. They have no UVs,
textures, materials, skeletons, or animation rigs. For an authentic unpainted
box appearance, the digital assets should use slightly rough injection-molded
plastic materials: red heroes, green Orcs/Goblins/Fimir, ivory undead, and dark
gray Chaos figures. Hand-painted PBR variants can be an optional later skin.

## Furniture: 15 assembled pieces, 11 distinct meshes

| Piece | Count | Preferred print source file |
| --- | ---: | --- |
| Table | 2 | `table.stl` |
| Throne | 1 | `throne.stl` |
| Alchemist’s bench | 1 | `AlchemyTable.stl` |
| Treasure chest | 3 | `Chest.stl` |
| Tomb | 1 | `Tomb.stl` |
| Sorcerer’s table | 1 | `ScorcererTable.stl` |
| Bookcase | 2 | `Bookcase.stl` |
| Torture rack | 1 | `TotureRack.stl` |
| Fireplace | 1 | `Fireplace.stl` |
| Weapons rack | 1 | `Weaponrack.stl` |
| Cupboard | 1 | `Buffet.stl` |

The original set also includes two candlesticks, one bottle set, one scale set,
four skulls, and four rats attached to furniture. Treat these as submeshes or
small dressing props, not 12 unrelated board units. The Tronic3100 base-game
pack supplies geometry for every assembled furniture type under CC BY-NC 4.0.
It is STL-only and must stay local/noncommercial. The original cardboard insert
art should be cropped from the private tile sheets and applied to the matching
3D frames.

The candlesticks, bottles, scales, and several skull details are already molded
into those assembled GLBs. The four removable rat and four removable skull pegs
are separate project-authored GLBs built by `tools/build-project-models.py` from
multiple original-component photographs. Runtime placement keeps each set
finite and uses the common bookcase, cupboard, and fireplace mounting holes;
the fittings have no quest rules and remain decorative.

## Doors, dice, screen, and markers

- 21 assembled doors: 16 open and 5 closed, each with one thin cardboard arch in a
  plastic base. Both 3D types are in the furniture pack; use the original scan
  for the door art.
- 6 white combat dice: each has three skulls, two white shields, and one black
  shield. Exact printable geometry exists, but its current source license could
  not be confirmed reliably. Authoring the rounded cube and scan-derived face
  decals in-project is safer and retains the existing Rapier physics. The
  runtime decals are extracted from the illustrated dice on original-US
  rulebook printed page 14 (skull and monster shield) and printed page 22
  (Hero lion shield), preserving their actual silhouettes.
- 2 red movement dice: conventional d6, best authored in-project so collision
  geometry, mass, pips, and visual mesh agree. The runtime uses circular ivory
  pip decals with a dark inset well rather than raised block geometry. Both die
  types emit SDL tabletop impacts from Rapier contact-force events; audio gain
  follows measured collision energy rather than a roll timer.
- 1 Information Screen: this is the folded cardboard GM/Zargon screen the user
  called the “shield.” It is not a plastic miniature and did not need a plastic
  stand in the original box. Build a thin three-panel mesh, slightly folded,
  with tile-sheet page 1 on the players’ side and page 2 on Zargon’s side.
- 33 double-sided cardboard markers: 12 skull/single-block markers, 2 double
  blocked-square markers, 6 pit markers whose backs supply 3 secret doors and 3
  single blocks, 1 stairway, 4 secret-door/falling-block markers, and 8
  falling-block/single-block markers.

The project-authored low door stand replaces thick printable stone arches so
the two scanned faces read as one cardboard insert rather than two slabs
sandwiching a second frame.

The furniture pack covers one- and two-square rock markers, secret doors,
traps, and skulls. These should remain very thin relief meshes with the scanned
tile faces. The separate stairway is a curved 2x2 relief following the Quest
Book symbol; the four fan-shaped pieces grouped on tile-sheet page 1 are secret
doors and must not be composited over it.

## Board walls

There are no freestanding wall pieces in the original US box. Walls are printed
on the board. The requested 3D walls are therefore an optional presentation
layer, not part of the physical inventory. Enfenix’s one- and two-square
HeroQuest wall tiles are a close fit and are CC BY-NC-SA 4.0. The renderer can
instance those two meshes along the board’s existing wall graph, with a low-wall
mode so pieces and scanned room art remain visible.

## Flat components that do not need separate 3D models

- 1 gameboard
- 66 playing cards: 24 Treasure, 10 Artifact, 8 Monster, 12 Chaos Spell, and
  three each of Air, Fire, Earth, and Water spells
- 4 character cards
- 1 instruction booklet
- 1 14-quest book
- 1 Identification Guide and Armory
- 1 pad of character sheets

These are already represented by the installed original-US scan pack. They
need UI surfaces and interaction, not photogrammetry or AI mesh conversion.

## Conversion pipeline

1. Download each licensed STL manually from its recorded model page into
   `models/source/`; retain author, URL, license, and original filename.
2. Repair non-manifold scan geometry, join multipart figures such as the
   Gargoyle, and restore the common classic square base without changing the
   silhouette.
3. Normalize scale against a 22 mm board square and orient +Y up, facing -Z.
4. Create a decimated gameplay mesh and a simpler convex or capsule collider.
5. Assign molded-plastic materials. UV unwrap only cardboard inserts, screen
   panels, doors, markers, and optional painted skins.
6. Export optimized GLB files to the paths in `assets/model-sources.json` and
   run `tools/audit-local-models.sh`.

The two weapon variants are reproducible from the installed scan with:

```sh
nix develop -c blender --background --python tools/build-orc-variants.py -- \
  --source assets/local/editions/original-us/models/figures/orc-sword.glb \
  --output-root assets/local/editions/original-us/models
```

AI image-to-mesh conversion is not useful for the base set now that direct
classic scans exist. It would be appropriate only for a genuinely missing
piece, and even then would need several photographs around the physical piece;
one catalog image cannot reconstruct hidden surfaces accurately.
