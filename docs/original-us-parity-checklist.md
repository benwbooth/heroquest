# Original US Game System parity checklist

This is the implementation contract for the copyright-1989, 1990 Milton
Bradley North American Game System. The private scans under
`assets/local/editions/original-us/scans/` are the authority. Rules and quest
notes are summarized here as mechanics, not reproduced as replacement text.

Status keys:

- `[x]` implemented and covered by a focused automated test
- `[-]` partially implemented; the remaining acceptance criteria are listed
- `[ ]` not implemented

A feature is not complete merely because its data type, menu, or placeholder
exists. It must affect a playable quest, preserve hidden information, render
the correct physical component, and have a deterministic regression test.

## Edition, content, and conformance gates

- [x] Use the 26 by 19 original board coordinate system.
- [x] Use original-US Hero and monster names and base statistics.
- [x] Keep 1989 UK, original-US, and 2021 remake content separated. Runtime
  loads only the original-US asset root and rejects quest metadata naming any
  other edition or a source outside original-US Quest Book pages 3-16.
- [x] Add a source reference (`book`, scan page, printed page, note/card name)
  to every rule, card, equipment item, quest trigger, and placement record.
  `src/source.rs` indexes every rules-engine section by local scan range; each
  Treasure, elemental spell, Chaos spell, Artifact, Monster, and Armory entry
  returns its exact PDF page and printed face. Every quest placement/objective
  inherits its validated Quest Book page, while typed events additionally cite
  their printed note id.
- [x] Reject quest data that overlaps figures/furniture/traps, duplicates a
  door edge, exceeds finite furniture/door/marker counts, or references
  edition-invalid content. The single scan-derived 22-room/corridor topology
  and its canonical physical wall-edge catalog are now enforced across all 14
  quests for both normal and secret doors; this audit corrected eighteen
  scan-verified one-square door transcription errors. Runtime reveal-order
  exhaustion can no longer hide or disable a logical Monster: the finite
  inventory remains capped and recyclable while overflow uses the correct
  classic sculpt. Static reveal-batch analysis rejects an authored room or
  corridor that alone needs more same-color figures than the complete box;
  out-of-board figures, duplicate blocked squares, accidental trap/component
  overlap, and a playable stairway that disagrees with its physical footprint
  are rejected before play rather than panicking or silently desynchronizing.
  Intentional printed furniture traps are accepted only when an exact typed
  event declares that same trap square.
- [x] Add a seeded replay fixture for every rule and every quest note. The
  machine-checked `tests/oracles/original-us-replay-index.json` supplies stable
  seeds for all eleven sourced rules-engine sections and all fourteen chapters,
  enumerates every printed plot point, and binds each suite to focused
  deterministic fixture functions. Its validator constructs every seeded game,
  rejects missing sections/pages/notes/tests, and runs alongside the full suite.
- [x] Add a campaign oracle save at the beginning and successful ending of all
  14 quests. The engine-driven campaign chain in
  `tests/oracles/original-us-campaign.json` records stable full-save
  fingerprints plus readable gold, artifact, lost-artifact, and Champion
  outcomes; campaign persistence now rejects unfinished, retreated, or lost
  games instead of trusting a caller-supplied success.

## Physical contents and presentation

- [x] Render the owned original-US board scan with calibrated playable bounds.
- [x] Render the original-US box and double-sided three-panel Information
  Screen from local scans.
- [x] Render two red movement dice and six white combat dice as physical
  rigid bodies. Earth-scaled Rapier throws, face distributions, exact
  scan-derived skull/lion-shield/monster-shield art, recessed round movement
  pips, all eight owned rack pieces at every Hero station, and force-scaled
  low-latency die-on-wood collision audio all work. Procedural event SFX are
  rules-driven as well: movement steps, weapon attacks, damage, spell casts,
  doors, searches, turn handoffs, and quest completion each have a distinct
  nonblocking cue.
- [x] Import and render the 35 real classic figure pieces as 18 distinct GLBs:
  four Heroes; four Orc sculpts; three Goblin sculpts; Fimir; Chaos Warrior;
  Chaos Warlock; Gargoyle; Skeleton; Zombie; Mummy. The notched Orc uses the
  classic scan body plus a photo-matched blade reconstruction; Grak's separate
  quest-only staff kitbash is installed in addition to those 18 box sculpts.
- [x] Allocate only the 35 finite box figures as areas are placed; reserve the
  correct notched-sword/staff Orc slots for Ulag and Grak, cycle the four Orc
  and three Goblin sculpts at their printed quantities, and keep logical stats
  separate from a same-color substitute sculpt. If a legal reveal temporarily
  outpaces defeated-piece recycling, keep that Monster visible, blocking,
  targetable, and AI-controlled with a virtual copy of its correct real GLB;
  never silently remove an actor because the physical allocator is full.
- [x] Import and render every assembled furniture piece: two tables, throne,
  alchemist's bench, three chests, tomb, sorcerer's table, two bookcases,
  torture rack, fireplace, weapons rack, and cupboard. Quest instances use the
  real local GLBs, with scan-derived cardboard faces on every printed insert.
- [x] Render the furniture dressing pieces: the assembled GLBs retain their
  candlesticks, bottles, scales, and molded skull details; four removable brown
  rats and four ivory skull pegs use photograph-matched project-authored GLBs
  and a finite attachment allocator over visible bookcases, cupboard, and
  fireplace.
- [x] Import/render all 16 open and 5 closed door pieces using the real open and
  project-authored low plastic stands plus one thin, two-sided scan-derived
  cardboard arch per door.
- [x] Import/render the stairs, blocked-square, pit, secret-door,
  falling-block, and skull markers with their correct scan faces. The allocator
  reserves the two double blocked-square tiles before using single-square backs.
- [x] Give multi-square furniture exact footprints; figures cannot occupy or
  pass through furniture squares.
- [x] Animate figure movement as lifted hops and combat as facing/lunge,
  damage recoil, and a visible toppling defeat before removal.
- [x] Display four table-edge scanned Character Cards and individual Character
  Sheets with personal dice controls, active-player highlight, Body, gold, and
  potion tokens. Every card/sheet is now ray-clickable; its ornamented,
  multi-page OSD reads live Body, Mind, gold, conditions, equipment, defense,
  artifacts, potions, spell hand, and carried-object state. Physical spell
  cards enter the real cast/target flow; two additional protected pages show
  the public quest objective and five newest public event-log entries. Used
  Elf/Wizard spells now leave their hands for separate scan-faced physical
  discard piles, including exact Spell Ring second-use behavior. Direct field
  editing is available for the free-form Hero-name field and saves immediately
  to the campaign; Body, Mind, gold, equipment, and cards deliberately remain
  rules-engine managed so the physical sheet cannot desynchronize game state.

## Game setup and campaign setup (rulebook scan pages 4-7)

- [x] Support one to four human players controlling all four Heroes while the
  computer controls Zargon.
- [x] Default seating/turn order to Barbarian, Dwarf, Elf, Wizard.
- [x] Allow players to arrange any clockwise Hero order before a quest with
  clickable Earlier/Later controls (Q/E keyboard equivalents); apply that
  order to turns, player stations, and dice while preserving each Hero's
  identity, campaign sheet, start square, owner, name, and spell cards.
- [x] Offer all 14 playable quest titles, player ownership, Hero naming, and
  elemental spell division; deal the selected Elf group and remaining Wizard
  groups as physical original-US spell cards.
- [x] Enforce The Trial as the first quest for a new campaign and unlock the
  next numbered chapter only after success (completed chapters remain replayable).
- [x] Read/show only each scan-cropped public parchment at quest start; keep
  map and notes unavailable to human Hero players.
- [x] Place only the starting-room components before play; keep all other
  doors, monsters, furniture, markers, traps, and secret doors concealed.
- [x] Shuffle the exact 24-card Treasure deck before every quest and again
  before each draw as directed by the original-US card rules.
- [x] Keep Artifact and Chaos Spell cards in Zargon's hidden state and show
  Monster cards face up as public reference. The four Zargon stacks, all eight
  face-up Monster cards, and scan-backed Quest Book are ray-clickable. Artifact,
  Chaos Spell, and Quest Book clicks protect secret faces/content; Treasure can
  only draw through a legal search; face-up Monster cards show public stats.
  Cast Chaos cards enter a separate shared scan-faced discard pile in exact
  cast order, exposing only cards that have already resolved.
- [x] Wizard selects one elemental group first, Elf selects one remaining
  group, Wizard receives the other two; selection, physical card hands,
  once-per-quest play, and card removal all follow the chosen groups.
- [x] Use the first-quest suggestion (Wizard Fire, Elf Earth) only as a
  default, never as a restriction.
- [x] Persist names, gold, equipment, artifacts, potions, Champion status, and
  completed quest numbers on versioned campaign character sheets.

## Turn sequence and action economy (rulebook scan page 7)

- [x] Run one turn for each living Hero, then one Zargon turn, repeatedly.
- [x] Permit `move then action` or `action then move`, never action midway
  through movement.
- [x] Permit a Hero to take exactly one of: attack, cast spell, search for
  treasure, search for secret doors, search for traps, disarm a trap.
- [x] Permit every revealed monster to move and take exactly one of: attack or
  cast a Chaos spell.
- [x] Allow a player to decline movement, decline an action, or use less than
  the maximum movement.
- [x] Make opening doors, looking, drinking potions, picking up items, and
  springing traps non-actions.
- [x] Prevent ending a Hero turn while illegally sharing a normal square.

## Hero movement, doors, looking, and reveal (rulebook scan pages 7-8)

- [x] Determine normal Hero movement from the settled upward faces of two
  physical red d6.
- [x] Move orthogonally, square by square; block solid rock, walls, closed
  doors, monsters, and board edges; diagonal movement is forbidden.
- [x] Allow Heroes to pass over other Heroes but not end on them.
- [x] Allow Heroes to share occupied stairs and sprung pit squares under the
  original exception while Monsters remain unable to share any square; fan
  shared physical figures apart and lower pit occupants visibly.
- [x] Apply one-red-die movement while wearing Plate Mail or carrying one of
  Prince Magnus' chests; allow owned Chain/Plate Mail to be switched between quests.
- [x] Opening an adjacent door is free, and an opened door never closes.
- [x] Let the player select which adjacent door to open.
- [x] Reveal closed doors, blocked squares, and monsters only when they enter
  unobstructed sight down a corridor.
- [x] On opening a door, reveal the room's monsters, furniture, chests,
  blocked squares, and doors, but never concealed traps, secret doors, or
  treasure.
- [x] Implement exact center-to-center sight: walls, closed doors, and figures
  block the line; merely touching a corner or wall edge does not.
- [x] Recalculate visibility as figures move; keep previously placed physical
  components on the board while restricting target visibility correctly.
- [x] Reveal quest-note text/events only at their typed room, door, search, or
  named-defeat trigger, and resolve each event only once.

## Attacks, defense, death, and weapons (rulebook scan pages 8, 12)

- [x] Adjacent melee attacks use four-way adjacency and occur at most once per
  attacker turn.
- [x] Each attack skull is a hit; each Hero white shield or monster black
  shield cancels one hit; remaining hits remove Body Points.
- [x] Use three skull, two white shield, one black shield faces on every
  physical combat die.
- [x] Let the player choose the target and equipped weapon.
- [x] Permit only one weapon per attack.
- [x] Implement diagonal Staff and Longsword attacks.
- [x] Implement Dagger throwing: visible nonadjacent target, one attack die,
  dagger permanently consumed.
- [x] Implement Crossbow attacks: visible nonadjacent target, three dice,
  unlimited arrows, forbidden against adjacent targets.
- [x] Implement Battle Axe/Staff incompatibility with Shield.
- [x] Enforce Wizard restrictions on Shortsword, Broadsword, Longsword,
  Battle Axe, Crossbow, Helmet, Shield, Chain Mail, and Plate Mail.
- [x] Apply Helmet, Shield, Chain Mail, Plate Mail, Borin's Armor, Wizard's
  Cloak, and spell modifiers to defense.
- [x] Apply pit penalty of one attack and defense die, with a minimum of one.
- [x] Represent multi-Body monster damage with skull markers.
- [x] Monsters attack adjacent Heroes using their natural Attack dice and
  cannot counterattack immediately. Zargon presents movement as a visible hop,
  rolls attack/defense in the physical tray, chooses the shortest legal attack
  route, and breaks equal-distance/adjacent ties by Body, defense, then threat.
- [x] At zero Body, offer the dying Hero an owned Healing Potion immediately.
- [x] Let a dying spellcaster self-cast an unused healing spell only if the
  spellcaster has not already acted on that turn.
- [x] Remove an unsaved dead Hero for the rest of the quest.
- [x] Let another Hero in the same room/corridor pick up a dead Hero's
  weapons, armor, artifacts, gold, and potions.
- [x] If no Hero is present, let a monster in the area claim and remove those
  possessions without using them.

## Hero spell rules and all 12 elemental cards (rulebook scan pages 8-9)

- [x] Cast only on the Elf/Wizard's own turn as that turn's one action.
- [x] Require exact line of sight except where a card explicitly says
  otherwise; allow self-targeting where stated.
- [x] Discard each spell after one use for the remainder of the quest.
- [x] Air—Genie: either open any door and reveal beyond it, or attack one
  visible monster with five combat dice.
- [x] Air—Swift Wind: target Hero rolls twice the normal red movement dice on
  their next move.
- [x] Air—Tempest: target monster misses its next turn.
- [x] Earth—Heal Body: restore up to four Body without exceeding starting Body.
- [x] Earth—Pass Through Rock: target Hero may cross walls on their next move;
  ending in shaded solid rock traps the Hero forever.
- [x] Earth—Rock Skin: +1 defense die until the target suffers one Body damage.
- [x] Fire—Ball of Flame: two Body damage, each 5/6 on two red dice cancels one.
- [x] Fire—Courage: +2 attack dice on the target's next attack; ends once the
  target can no longer see a monster.
- [x] Fire—Fire of Wrath: one Body damage unless target monster rolls 5/6 on
  one red die.
- [x] Water—Sleep: monster cannot move/attack/defend; at once and on future
  turns it rolls one red die per Mind point and wakes on a 6; undead immune.
- [x] Water—Water of Healing: restore up to four Body without exceeding start.
- [x] Water—Veil of Mist: target Hero may pass through monster-occupied squares
  during their next movement.

## Treasure search, deck, potions, and artifacts (rulebook scan pages 9-10)

- [x] Search for treasure only in a room with no monsters present.
- [x] Allow each Hero to search each room at most once; corridors cannot be
  searched for treasure.
- [x] Resolve the room's unclaimed quest-note special treasure first; only
  otherwise draw from the Treasure deck.
- [x] Trigger an undisarmed chest/furniture trap before granting treasure and
  end the searcher's turn as specified by the quest note.
- [x] Remove valuable Treasure cards until the next quest; return Hazard and
  Wandering Monster cards to the bottom and shuffle before each later draw.
- [x] Implement exact deck quantities: 2 Gem (35 gold), 2 Gold Coins (15),
  2 Jewels (25), 2 Jewels (50), 2 arrow Hazards, 2 pit Hazards, 6 Wandering
  Monsters, 1 Heroic Brew, 1 Potion of Defense, 3 Potions of Healing, and
  1 Potion of Strength.
- [x] Arrow Hazard: lose one Body and end turn.
- [x] Pit Hazard: lose one Body, end turn, climb out/move normally next turn.
- [x] Wandering Monster: use quest type; place adjacent to searcher if possible
  (otherwise closest square in room), attack searcher immediately, remain in
  play, and allow the Hero's turn to continue.
- [x] Heroic Brew: drink before an attack to make two attacks once.
- [x] Potion of Defense: +2 combat dice on next defense once.
- [x] Potion of Healing: roll one red die and restore that many Body, capped at
  starting Body, including immediate death prevention.
- [x] Potion of Strength: +2 combat dice on next attack once.
- [x] Potions may be consumed at any time; multiple potions may be active.
- [x] A Hero may give a potion to another Hero only during the giver's turn.
- [x] Allow voluntary sharing of gold.
- [x] Implement all 10 Artifact cards and restrictions: Elixir of Life,
  Borin's Armor, Orc's Bane, Ring of Return, Spell Ring, Spirit Blade,
  Talisman of Lore, Wand of Magic, Wizard's Cloak, Wizard's Staff.
- [x] Elixir of Life: revive one dead Hero at full Body and Mind, once.
- [x] Borin's Armor: four defense dice, no Plate Mail slowdown, not Wizard.
- [x] Orc's Bane: two attack dice, attack twice against Orcs, not Wizard.
- [x] Ring of Return: return all visible Heroes to quest start, once.
- [x] Spell Ring: owner chooses one spell at quest start and may cast it twice.
- [x] Spirit Blade: three attack dice, four against Skeleton/Zombie/Mummy, not
  Wizard; only qualifying weapon against Witch Lord.
- [x] Talisman of Lore: +1 Mind while possessed.
- [x] Wand of Magic: Elf/Wizard may cast two different spells in one turn.
- [x] Wizard's Cloak: Wizard-only +1 defense die.
- [x] Wizard's Staff: Wizard-only two attack dice and diagonal attack.

## Secret doors (rulebook scan page 9)

- [x] Search only when no monster is visible to the active Hero.
- [x] Reveal every secret door in the Hero's current room or corridor without
  moving the Hero.
- [x] A found secret door stays closed until an adjacent Hero opens it.
- [x] Opening it is free, permanent, and reveals what lies beyond while keeping
  traps and treasure concealed.
- [x] Support quest-note exceptions such as Melar's key revealing a door only
  after a treasure search.

## Trap search, springing, jumping, and disarming (rulebook scan pages 10-11)

- [x] Search only when no monster is visible; reveal locations in the current
  room/corridor while leaving unsprung trap tiles off the board.
- [x] Traps immediately beyond a door remain undiscoverable until the Hero is
  inside the room.
- [x] Monsters neither spring hidden traps nor search/disarm them.
- [x] Pit: entering undiscovered trap loses one Body, places pit tile, ends
  turn; pit can no longer be disarmed but can be jumped.
- [x] In a pit, treat the square as its own searchable room; figures may
  attack/defend at -1 die (minimum one) and normally climb out next turn.
- [x] Falling block: place tile, roll three combat dice and suffer one Body per
  skull with no defense, choose an empty square ahead/behind, end turn, and
  permanently block the trap square. The three owned dice visibly leave the
  active Hero's rack, roll under the action camera, and resolve only after rest.
- [x] Spear: roll one combat die; skull loses one Body and ends turn, either
  shield dodges and movement continues; remove trap forever; no tile. The
  physical roll is likewise visible and blocks further input until settled.
- [x] Chest/furniture trap: treasure search springs quest-specific effect and
  ends turn; a successful disarm permits treasure search on a later turn.
- [x] Jump requires two movement points and a vacant adjacent landing beyond;
  one combat die succeeds on either shield and spends two movement.
- [x] Failed jump springs the trap, moves Hero onto its square, applies damage,
  places applicable tile, and ends turn.
- [x] Sprung falling blocks cannot be jumped; sprung pits can.
- [x] Monsters with sufficient movement and a vacant landing automatically
  jump pits; voluntarily entering a pit causes no monster damage.
- [x] Disarm is an action declared before movement and requires stepping onto a
  discovered unsprung trap.
- [x] Tool Kit disarm succeeds on either shield and springs on a skull.
- [x] Dwarf needs no Tool Kit, succeeds on skull or white shield, and springs
  only on a black shield.
- [x] A disarmed pit becomes an ordinary square; all disarmed traps disappear.

## Zargon movement, spells, and component limits (rulebook scan pages 11-12)

- [x] Move only monsters that have been placed/revealed on the board.
- [x] Monsters use fixed maximum movement, may move less, and do not roll dice.
- [x] Monsters cannot move diagonally, pass Heroes, cross walls, open/close
  doors, share normal squares, or perform Hero searches/disarms.
- [x] Permit monster action before or after movement but never mid-movement.
- [x] Assign Chaos spell cards only to monsters named by quest notes; each is
  cast once per quest on a visible target unless its card says otherwise.
- [x] Implement all 12 Chaos cards: Ball of Flame, Cloud of Chaos, Command,
  Escape, Fear, Firestorm, Lightning Bolt, Rust, Sleep, Summon Orcs, Summon
  Undead, Tempest.
- [x] Ball of Flame: two Body, each 5/6 on two red dice cancels one.
- [x] Cloud of Chaos: room targets cannot move/attack/defend; each victim rolls
  one red die per Mind and breaks on a 6.
- [x] Command: Zargon controls target Hero on its turn until a Mind-die 6;
  controlled Hero moves/attacks as a monster.
- [x] Escape: teleport caster to quest-marked secret destination, preserving
  concealment until its room is opened.
- [x] Fear: target attacks with one die until a Mind-die 6 on a future turn.
- [x] Firestorm: room only, three Body to all other figures in room, each 5/6
  on two red dice cancels one; caster immune; unusable in corridors.
- [x] Lightning Bolt: horizontal/vertical/diagonal ray to wall/closed door,
  dealing two Body to every figure in its path.
- [x] Rust: permanently destroy one non-artifact metal sword or helmet.
- [x] Sleep: Hero cannot move/attack/defend until a Mind-die 6.
- [x] Summon Orcs: one red die creates 4/5/6 Orcs around caster per card.
- [x] Summon Undead: one red die creates the exact Skeleton/Zombie/Mummy group
  specified by the card around caster.
- [x] Tempest: target Hero misses next turn.
- [x] Reuse killed figures when a quest later calls for them; if the requested
  sculpt type is exhausted, substitute an available same-color physical
  monster without changing the logical monster's rules or stats.

## Quest ending and between-quest campaign state (rulebook scan page 12)

- [x] Complete each quest only after its typed objective and printed return or
  escape condition is satisfied, including the quest-specific exceptions.
- [x] Let Heroes voluntarily end an unfinished quest only after every surviving
  Hero returns to the stairway; confirm the choice and grant no completion or
  final reward.
- [x] End in defeat when all four Heroes die.
- [x] Grant and split each quest's exact final reward only on successful completion.
- [x] Record the completed quest number for each surviving Hero and the campaign.
- [x] Restore starting Body and Mind and return all elemental spells after a
  successfully completed quest by constructing the next quest from the saved sheet.
- [x] Preserve found treasure, potions, equipment, and artifacts between quests;
  a dead Hero returns next quest only as a fresh character with starting equipment.
- [x] Open the scanned Armory between quests; deduct gold immediately and
  enforce prices, Wizard restrictions, body-armor conflicts, and Plate Mail movement.
- [x] Implement Tool Kit (250), Dagger (25), Staff (100), Crossbow (350),
  Shortsword (150), Broadsword (250), Longsword (350), Battle Axe (450),
  Helmet (125), Shield (150), Chain Mail (500), Plate Mail (850).
- [x] If a required artifact is lost to monsters, persist that loss and inject
  the card as early special treasure in the next quest that requires it,
  without consuming the Treasure deck or displacing printed quest treasure.
- [x] Leave the durable campaign unchanged after failure or an unfinished exit,
  allowing the same unlocked quest to be replayed without false completion/rewards.

## Quest 1—The Trial (quest-book scan page 3)

- [x] Replace the approximate JSON with every printed door, monster, furniture,
  blocked square, start, and letter marker at exact board coordinates.
- [x] Explicitly declare that this quest contains no traps or secret doors.
- [x] A: weapons rack contains nothing useful.
- [x] B: specified chest is empty.
- [x] C: Fellmarg's Mummy guardian attacks with four dice instead of three.
- [x] D: first treasure search finds 84 gold in specified chest/room.
- [x] E: first treasure search finds 120 gold in specified chest/room.
- [x] Wandering Monster: Orc.
- [x] Objective: defeat Verag and return surviving Heroes to stairs.

## Quest 2—The Rescue of Sir Ragnar (quest-book scan page 4)

- [x] Encode every printed placement and concealed feature exactly.
- [x] A: poison-needle chest trap loses one Body; chest is empty.
- [x] B: first search finds 60 gold and a potion healing up to four Body.
- [x] Finding Ragnar sounds alarm: reveal/place every remaining door, monster,
  and furniture; open all doors; prohibit treasure search in cell.
- [x] Represent Ragnar with Chaos Warlock figure; opener controls him after the
  Hero's regular turn with one red movement die; no attack, two defense, two
  remaining Body.
- [x] Pay/split 240 gold only if Ragnar reaches stairs alive; no reward if dead.
- [x] Wandering Monster: Orc.

## Quest 3—Lair of the Orc Warlord (quest-book scan page 5)

- [x] Encode every printed placement and concealed feature exactly.
- [x] A: first search finds an Armory-equivalent Staff.
- [x] B: first search finds 24 gold and a potion healing up to four Body.
- [x] Select the notched/large-sword Orc slot for Ulag and apply stats Move 10,
  Attack 4, Defend 5, Body 2, Mind 3. The installed scan-derived GLB has the
  photographed enlarged blade, keyhole notch, heavy tip, and square base.
- [x] On Ulag's destruction, split 180 gold; found treasure remains individual.
- [x] Wandering Monster: Orc.

## Quest 4—Prince Magnus' Gold (quest-book scan page 6)

- [x] Encode every printed placement and concealed feature exactly.
- [x] Mark all three royal chests, each holding 250 quest gold plus valuables.
- [x] A Hero carries at most one chest and rolls only one movement die while
  carrying it; allow dropping/transferring under a defined legal interaction.
- [x] Require all three chests returned; Heroes cannot keep their contents.
- [x] Pay/split 240 gold on success.
- [x] Treat Gulthor as the map's named Chaos Warrior leader.
- [x] Wandering Monster: Fimir.

## Quest 5—Melar's Maze (quest-book scan page 7)

- [x] Encode every printed placement and concealed feature exactly.
- [x] A: first search finds potion healing up to two Body.
- [x] B: stone Gargoyle stays inert and immune until the specified next-room
  door opens; remains immune until it has moved or attacked.
- [x] C: poisonous-gas chest loses two Body if undisarmed and contains 144
  gold; no other treasure in room.
- [x] D: first search finds Talisman of Lore artifact.
- [x] E: secret-door search finds nothing; treasure search finds Melar's Key,
  removes key on touch, slides throne, and reveals secret door.
- [x] Objective: return Talisman to safety.
- [x] Wandering Monster: Zombie.

## Quest 6—Legacy of the Orc Warlord (quest-book scan page 8)

- [x] Encode every printed placement and concealed feature exactly, including
  non-stair prison-cell start and separate exit stairs.
- [x] Start all Heroes with equipment, potions, and spells inaccessible; base
  unarmed/unarmored attack one and defense two.
- [x] A: searching cupboard recovers equipment; each Hero must enter room to
  reclaim their own; Elf/Wizard regain spellcasting only then.
- [x] Each Hero escapes independently on reaching stairs.
- [x] Use staff Orc sculpt for Grak; Move 8, Attack 4, Defend 4, Body 3, Mind 3.
  The installed `orc-staff.glb` retains the classic scan body and replaces its
  sword with a clean, crooked, quest-specific staff.
- [x] Give Grak one-use Fear, Sleep, Tempest; if killed, award Wizard's Cloak.
- [x] Wandering Monster: Fimir.

## Quest 7—The Lost Wizard (quest-book scan page 9)

- [x] Encode every printed placement and concealed feature exactly.
- [x] A: all Chaos Warriors are stone and gain one defense die.
- [x] B: first weapons-room search finds Borin's Armor.
- [x] C: poison needle loses two Body; chest potion is unidentified until
  consumed, then immobilizes Hero for five of their turns while invulnerable.
- [x] D: robed Zombie is Wardoz; after destroying him, first search finds 144
  gold and reveals the proof of his transformation.
- [x] Pay each returning Hero 100 gold.
- [x] Wandering Monster: Mummy.

## Quest 8—The Fire Mage (quest-book scan page 10)

- [x] Encode every printed placement and concealed feature exactly.
- [x] Use Chaos Warlock for Balur; fire spells cannot affect him.
- [x] Balur Move 8, Attack 2, Defend 5, Body 3, Mind 7.
- [x] Give Balur one-use Ball of Flame, Firestorm, Tempest, Summon Orcs, Fear,
  Escape; Escape targets marked X and stays concealed until room opens.
- [x] A: chest contains 150 gold and Wand of Magic.
- [x] Pay each Hero 100 gold for Balur's destruction and safe return.
- [x] Wandering Monster: Fimir.

## Quest 9—Race Against Time (quest-book scan page 11)

- [x] Encode every printed placement and concealed feature exactly, including
  special start room A and remote stairs.
- [x] B: each marked chest contains 100 gold.
- [x] C: poison-gas chest loses three Body if undisarmed; contains Elixir of
  Life.
- [x] Objective: escape to stairs; no named enemy requirement.
- [x] Wandering Monster: Fimir.

## Quest 10—Castle of Mystery (quest-book scan page 12)

- [x] Encode every printed placement, numbered teleport square, and concealed
  feature exactly.
- [x] Passing any door ends movement, rolls two red dice, and teleports to the
  identically numbered square; at most one door per Hero turn.
- [x] Occupied destination: landed-on figure loses one Body and, if alive,
  teleports by 2d6; reroll same destination; original Hero remains.
- [x] A: after both specified Chaos Warriors die, first search awards Ring of
  Return from one warrior.
- [x] B: mine entrance grants 5,000 carried gold; carrier cannot attack or
  defend; dropping it returns/disappears into mine.
- [x] End when all monsters die or all Heroes leave via stairs on roll 2 or 12;
  reveal that mine gold is worthless while other treasure stays real.
- [x] Wandering event: Ollar's ghost appears/laughs/disappears, no monster.

## Quest 11—Bastion of Chaos (quest-book scan page 13)

- [x] Encode every printed placement and concealed feature exactly.
- [x] A: first Armory search finds a Shield; other weapons unusable.
- [x] B: stone Gargoyle and chest trap; treasure search before disarm animates
  and immediately attacks with Gargoyle; it is immune until moved/attacked.
- [x] C: specified Chaos Warrior drops Orc's Bane to its killer.
- [x] Require every monster defeated; award bounty to killer: Goblin 10, Orc
  20, Fimir 30, Chaos Warrior 50 gold.
- [x] Wandering Monster: Fimir.

## Quest 12—Barak Tor—Barrow of the Witch Lord (quest-book scan page 14)

- [x] Encode every printed placement and concealed feature exactly.
- [x] A: false doors can never open.
- [x] B: Star of the West is carried by specified Zombie.
- [x] C: special falling block triggers after last Hero passes, causes no entry
  damage, and permanently blocks return path.
- [x] D: entering tomb releases Witch Lord, reveals public warning, and uses
  Chaos Warlock figure.
- [x] Witch Lord is immune to every weapon/spell except Spirit Blade; Move 1,
  Attack 2; give Summon Undead, Fear, Command, Ball of Flame.
- [x] E: first search behind bookcase finds Wizard's Staff.
- [x] Return Star safely and split 200 gold.
- [x] Wandering Monster: Skeleton.

## Quest 13—Quest for the Spirit Blade (quest-book scan page 15)

- [x] Encode every printed placement and concealed feature exactly.
- [x] Override falling-block squares: do not place/block; entering rolls one
  red die and loses one Body on 4-6, or only 6 while wearing Helmet; monsters
  unaffected.
- [x] A: first search finds Spirit Blade.
- [x] B: chest contains 200 gold.
- [x] Objective: return Spirit Blade safely.
- [x] Wandering Monster: Chaos Warrior.

## Quest 14—Return to Barak Tor (quest-book scan page 16)

- [x] Encode every printed placement and concealed feature exactly; Witch
  Lord's former tomb A is empty.
- [x] Use Chaos Warlock for Witch Lord; only Spirit Blade affects him; Move 10,
  Attack 5, Defend 6, Body 4, Mind 6.
- [x] Give Summon Undead, Ball of Flame, Command, Tempest, and two legal casts
  of Fear.
- [x] On defeat, remove him in the scripted effect and award Spell Ring.
- [x] Successfully surviving Heroes receive the campaign title Champion.
- [x] Wandering Monster: Mummy.

## Completion gate

- [x] `tools/audit-local-models.sh` reports every required slot ready (44/44).
- [x] All 14 quest files pass schema, placement, component-count, visibility,
  trigger, reward, and deterministic replay validation. The disk-file
  conformance gate reparses every JSON file, validates provenance, board
  topology, typed plot references, component demand, furniture/trap/stair
  placement, and runtime construction; focused rule/quest fixtures plus the
  full-campaign oracle cover reveal behavior and exact outcomes.
- [x] Every checklist line above is `[x]`; no `[-]`, procedural piece fallback,
  placeholder quest, silent no-op action, or test-only implementation remains.
  Required classic figure, furniture, door, marker, dressing, movement-die,
  and combat-die meshes fail fast when absent; authored references and every
  bracketed OSD control are validated against live dispatch paths.
- [x] A fresh campaign can be played from opening the box through the Champion
  reward with only in-game controls and without consulting source code. The
  clickable box/setup/Armory flow, context-sensitive action bindings,
  completion/replay handoffs, and all-14-quest campaign save oracle guard the
  complete route.
