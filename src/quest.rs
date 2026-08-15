use std::{collections::HashSet, path::Path};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::cards::{Artifact, ChaosSpell};
use crate::equipment::{Armor, Weapon};
use crate::model::{HeroKind, MonsterKind, Pos, PropKind, TrapKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestDefinition {
    pub title: String,
    pub blurb: String,
    #[serde(default)]
    pub source: Option<QuestSourceDef>,
    pub rooms: Vec<RoomDef>,
    pub corridors: Vec<CorridorDef>,
    #[serde(default)]
    pub blocked: Vec<Pos>,
    #[serde(default)]
    pub solid_rock: Vec<String>,
    pub doors: Vec<DoorDef>,
    #[serde(default)]
    pub secret_doors: Vec<DoorDef>,
    pub hero_starts: Vec<HeroStartDef>,
    pub stairs: Vec<Pos>,
    #[serde(default)]
    pub monsters: Vec<MonsterDef>,
    #[serde(default)]
    pub allies: Vec<AllyDef>,
    #[serde(default)]
    pub props: Vec<PropDef>,
    #[serde(default)]
    pub quest_items: Vec<QuestItemDef>,
    #[serde(default)]
    pub traps: Vec<TrapDef>,
    #[serde(default = "default_wandering_monster")]
    pub wandering_monster: MonsterKind,
    #[serde(default)]
    pub wandering_event_message: Option<String>,
    #[serde(default)]
    pub events: Vec<QuestEventDef>,
    pub objective: ObjectiveDef,
    #[serde(default)]
    pub heroes_captured: bool,
    #[serde(default)]
    pub forbidden_treasure_rooms: Vec<String>,
    #[serde(default)]
    pub teleport_network: Option<TeleportNetworkDef>,
    #[serde(default)]
    pub mine: Option<MineDef>,
    #[serde(default)]
    pub monster_bounties: Vec<MonsterBountyDef>,
    #[serde(default)]
    pub delayed_falling_block: Option<DelayedFallingBlockDef>,
    /// Quest 13 leaves its printed falling-rock squares open and rolls a red
    /// die every time a Hero enters one instead of placing a blocking tile.
    #[serde(default)]
    pub collapsing_ceiling_hazards: Vec<Pos>,
}

const fn default_wandering_monster() -> MonsterKind {
    MonsterKind::Orc
}

impl QuestDefinition {
    pub const IMPLEMENTED_ORIGINAL_US_QUESTS: usize = 14;
    pub const ORIGINAL_US_QUEST_FILES: [&'static str; 14] = [
        "original_us_01_the_trial.json",
        "original_us_02_the_rescue_of_sir_ragnar.json",
        "original_us_03_lair_of_the_orc_warlord.json",
        "original_us_04_prince_magnus_gold.json",
        "original_us_05_melars_maze.json",
        "original_us_06_legacy_of_the_orc_warlord.json",
        "original_us_07_the_lost_wizard.json",
        "original_us_08_the_fire_mage.json",
        "original_us_09_race_against_time.json",
        "original_us_10_castle_of_mystery.json",
        "original_us_11_bastion_of_chaos.json",
        "original_us_12_barak_tor.json",
        "original_us_13_quest_for_the_spirit_blade.json",
        "original_us_14_return_to_barak_tor.json",
    ];

    /// The printed 1989/1990 North-American game board has one immutable
    /// room/corridor layout. Quest maps shade unused squares as solid rock;
    /// they never move a wall or subdivide a printed room.
    pub const ORIGINAL_US_ROOM_AREAS: [Rect; 22] = [
        Rect::new(1, 1, 4, 3),
        Rect::new(5, 1, 4, 3),
        Rect::new(1, 4, 4, 5),
        Rect::new(5, 4, 4, 5),
        Rect::new(9, 1, 3, 5),
        Rect::new(14, 1, 3, 5),
        Rect::new(17, 1, 4, 4),
        Rect::new(21, 1, 4, 4),
        Rect::new(17, 5, 4, 4),
        Rect::new(21, 5, 4, 4),
        Rect::new(10, 7, 6, 5),
        Rect::new(1, 10, 4, 4),
        Rect::new(5, 10, 2, 3),
        Rect::new(7, 10, 2, 3),
        Rect::new(1, 14, 4, 4),
        Rect::new(5, 13, 4, 5),
        Rect::new(9, 13, 3, 5),
        Rect::new(14, 13, 4, 5),
        Rect::new(18, 10, 3, 4),
        Rect::new(21, 10, 4, 4),
        Rect::new(18, 14, 3, 4),
        Rect::new(21, 14, 4, 4),
    ];

    pub const ORIGINAL_US_ROOM_ADDITIONAL_AREAS: [Option<Rect>; 22] = [
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(Rect::new(17, 10, 1, 3)),
        None,
        None,
        None,
    ];

    pub const ORIGINAL_US_CORRIDOR_AREAS: [Rect; 1] = [Rect::new(0, 0, 26, 19)];

    /// Every physical wall segment on the immutable original-US board.  An
    /// edge is present exactly where two orthogonally adjacent board squares
    /// belong to different printed rooms/corridor.  Edges are normalized so
    /// quest-map door direction cannot affect validation.
    pub fn original_us_wall_edges() -> HashSet<(Pos, Pos)> {
        let mut regions =
            [0_u8; crate::model::BOARD_WIDTH as usize * crate::model::BOARD_HEIGHT as usize];
        for (room_index, (&area, additional)) in Self::ORIGINAL_US_ROOM_AREAS
            .iter()
            .zip(Self::ORIGINAL_US_ROOM_ADDITIONAL_AREAS)
            .enumerate()
        {
            for pos in std::iter::once(area)
                .chain(additional)
                .flat_map(Rect::positions)
            {
                regions[pos.y as usize * crate::model::BOARD_WIDTH as usize + pos.x as usize] =
                    room_index as u8 + 1;
            }
        }

        let mut edges = HashSet::new();
        for y in 0..crate::model::BOARD_HEIGHT {
            for x in 0..crate::model::BOARD_WIDTH {
                let a = Pos::new(x, y);
                let a_region =
                    regions[a.y as usize * crate::model::BOARD_WIDTH as usize + a.x as usize];
                for b in [
                    (x + 1 < crate::model::BOARD_WIDTH).then(|| Pos::new(x + 1, y)),
                    (y + 1 < crate::model::BOARD_HEIGHT).then(|| Pos::new(x, y + 1)),
                ]
                .into_iter()
                .flatten()
                {
                    let b_region =
                        regions[b.y as usize * crate::model::BOARD_WIDTH as usize + b.x as usize];
                    if a_region != b_region {
                        edges.insert(normalized_edge(a, b));
                    }
                }
            }
        }
        edges
    }

    /// Original-US map records inherit their Quest Book page from the quest
    /// root; lettered triggers additionally use their event id as the printed
    /// note identifier. This keeps hundreds of placements attributable
    /// without duplicating a drift-prone citation object on every coordinate.
    pub fn validate_original_us_source_coverage(&self) -> Result<()> {
        let source = self
            .source
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("original-US quest is missing its source reference"))?;
        ensure!(
            source.edition == "HeroQuest original US release",
            "quest source edition is not the original US release: {}",
            source.edition
        );
        ensure!(
            source.book == "Quest Book" && (3..=16).contains(&source.page),
            "original-US quest source must name Quest Book pages 3 through 16"
        );
        ensure!(
            !self.title.trim().is_empty() && !self.blurb.trim().is_empty(),
            "original-US quest title and public introduction must be source-identifiable"
        );
        ensure!(
            self.rooms.iter().all(|room| !room.name.trim().is_empty()),
            "every sourced room placement needs a stable room name"
        );
        ensure!(
            self.events.iter().all(|event| !event.id.trim().is_empty()),
            "every sourced quest trigger needs a printed note identifier"
        );
        Ok(())
    }

    pub fn source_reference(&self, detail: &str) -> Option<String> {
        self.source.as_ref().map(|source| source.reference(detail))
    }

    /// Validate every authored cross-reference before the runtime consumes the
    /// definition. This turns misspelled room/note/figure ids and detached
    /// rewards into load errors instead of silent no-op quest plot points.
    pub fn validate_authored_references(&self) -> Result<()> {
        let rooms = self
            .rooms
            .iter()
            .map(|room| room.name.as_str())
            .collect::<HashSet<_>>();
        let monster_names = self
            .monsters
            .iter()
            .map(|monster| {
                monster
                    .name
                    .as_deref()
                    .unwrap_or_else(|| monster.monster.name())
            })
            .collect::<HashSet<_>>();
        let ally_names = self
            .allies
            .iter()
            .map(|ally| ally.name.as_str())
            .collect::<HashSet<_>>();
        let hero_kinds = self
            .hero_starts
            .iter()
            .map(|hero| hero.hero)
            .collect::<HashSet<_>>();
        let event_ids = self
            .events
            .iter()
            .map(|event| event.id.as_str())
            .collect::<HashSet<_>>();
        ensure!(
            event_ids.len() == self.events.len(),
            "quest event ids must be unique"
        );
        let door_edges = self
            .doors
            .iter()
            .chain(&self.secret_doors)
            .map(|door| normalized_edge(door.a, door.b))
            .collect::<HashSet<_>>();
        let secret_edges = self
            .secret_doors
            .iter()
            .map(|door| normalized_edge(door.a, door.b))
            .collect::<HashSet<_>>();
        let trap_positions = self
            .traps
            .iter()
            .map(|trap| trap.pos)
            .collect::<HashSet<_>>();

        for event in &self.events {
            match &event.trigger {
                QuestTriggerDef::SearchTreasure { room } | QuestTriggerDef::RevealRoom { room } => {
                    ensure!(
                        rooms.contains(room.as_str()),
                        "quest event {} references unknown room {room}",
                        event.id
                    )
                }
                QuestTriggerDef::SearchTreasureAfterDefeat { room, name } => {
                    ensure!(
                        rooms.contains(room.as_str()),
                        "quest event {} references unknown room {room}",
                        event.id
                    );
                    ensure!(
                        monster_names.contains(name.as_str()),
                        "quest event {} references unknown Monster {name}",
                        event.id
                    );
                }
                QuestTriggerDef::DefeatNamed { name } => ensure!(
                    monster_names.contains(name.as_str()),
                    "quest event {} references unknown Monster {name}",
                    event.id
                ),
                QuestTriggerDef::OpenDoor { a, b } => ensure!(
                    door_edges.contains(&normalized_edge(*a, *b)),
                    "quest event {} references a door edge absent from the map",
                    event.id
                ),
            }
            validate_effect_references(
                &event.effect,
                &monster_names,
                &ally_names,
                &hero_kinds,
                &secret_edges,
                &trap_positions,
            )?;
            validate_trigger_effect_compatibility(&event.trigger, &event.effect).with_context(
                || {
                    format!(
                        "quest event {} has an unsupported trigger/effect pair",
                        event.id
                    )
                },
            )?;
        }

        match &self.objective {
            ObjectiveDef::DefeatNamed { name, .. }
            | ObjectiveDef::DefeatNamedAndReturn { name, .. } => ensure!(
                monster_names.contains(name.as_str()),
                "quest objective references unknown Monster {name}"
            ),
            ObjectiveDef::RescueNamedAndReturn { name, reward_gold } => {
                ensure!(
                    ally_names.contains(name.as_str()),
                    "quest objective references unknown ally {name}"
                );
                ensure!(*reward_gold > 0, "rescue reward must be positive");
            }
            ObjectiveDef::ReturnQuestItems { count, reward_gold } => {
                ensure!(
                    *count > 0 && usize::from(*count) <= self.quest_items.len(),
                    "quest-item objective count exceeds authored quest items"
                );
                ensure!(*reward_gold > 0, "quest-item reward must be positive");
            }
            ObjectiveDef::FindArtifactAndReturn { artifact } => ensure!(
                self.events
                    .iter()
                    .any(|event| effect_grants_artifact(&event.effect, *artifact))
                    || self
                        .hero_starts
                        .iter()
                        .any(|hero| hero.artifacts.contains(artifact)),
                "artifact objective has no authored source for {}",
                artifact.name()
            ),
            ObjectiveDef::ResolveEventAndReturn { event, .. } => ensure!(
                event_ids.contains(event.as_str()),
                "quest objective references unknown event {event}"
            ),
            ObjectiveDef::ReachStairs
            | ObjectiveDef::EscapeIndependently
            | ObjectiveDef::DefeatAllOrEscapeIndependently => {
                ensure!(
                    !self.stairs.is_empty(),
                    "escape objective requires a stairway"
                )
            }
            ObjectiveDef::DefeatAllAndReturn | ObjectiveDef::DefeatAll => {}
        }
        Ok(())
    }

    pub fn validate_original_us_board_topology(&self) -> Result<()> {
        let mut actual_rooms = self
            .rooms
            .iter()
            .map(|room| {
                let mut areas = std::iter::once(room.area.key())
                    .chain(room.additional_areas.iter().map(|rect| rect.key()))
                    .collect::<Vec<_>>();
                areas.sort_unstable();
                areas
            })
            .collect::<Vec<_>>();
        let mut expected_rooms = Self::ORIGINAL_US_ROOM_AREAS
            .iter()
            .zip(Self::ORIGINAL_US_ROOM_ADDITIONAL_AREAS)
            .map(|(rect, additional)| {
                let mut areas = vec![rect.key()];
                if let Some(additional) = additional {
                    areas.push(additional.key());
                }
                areas.sort_unstable();
                areas
            })
            .collect::<Vec<_>>();
        actual_rooms.sort_unstable();
        expected_rooms.sort_unstable();
        ensure!(
            actual_rooms == expected_rooms,
            "quest room rectangles do not match the immutable original-US board"
        );

        let mut actual_corridors = self
            .corridors
            .iter()
            .map(|corridor| corridor.area.key())
            .collect::<Vec<_>>();
        let mut expected_corridors = Self::ORIGINAL_US_CORRIDOR_AREAS
            .iter()
            .map(|rect| rect.key())
            .collect::<Vec<_>>();
        actual_corridors.sort_unstable();
        expected_corridors.sort_unstable();
        ensure!(
            actual_corridors == expected_corridors,
            "quest corridors do not match the immutable original-US board"
        );

        let printed_wall_edges = Self::original_us_wall_edges();
        for (door, kind) in self
            .doors
            .iter()
            .map(|door| (door, "door"))
            .chain(self.secret_doors.iter().map(|door| (door, "secret door")))
        {
            ensure!(
                door.a.x < crate::model::BOARD_WIDTH
                    && door.a.y < crate::model::BOARD_HEIGHT
                    && door.b.x < crate::model::BOARD_WIDTH
                    && door.b.y < crate::model::BOARD_HEIGHT,
                "{kind} is outside the immutable original-US board"
            );
            ensure!(
                door.a.is_adjacent(door.b),
                "{kind} endpoints must be orthogonally adjacent"
            );
            ensure!(
                printed_wall_edges.contains(&normalized_edge(door.a, door.b)),
                "{kind} between ({}, {}) and ({}, {}) is not on a printed original-US wall edge",
                door.a.x + 1,
                door.a.y + 1,
                door.b.x + 1,
                door.b.y + 1
            );
        }
        Ok(())
    }

    pub fn demo() -> Result<Self> {
        serde_json::from_str(include_str!("../assets/quests/torchlit_cellar.json"))
            .context("the built-in demo quest is invalid")
    }

    pub fn original_us_trial() -> Result<Self> {
        serde_json::from_str(include_str!(
            "../assets/quests/original_us_01_the_trial.json"
        ))
        .context("the built-in original-US Trial quest is invalid")
    }

    pub fn original_us_rescue_of_sir_ragnar() -> Result<Self> {
        serde_json::from_str(include_str!(
            "../assets/quests/original_us_02_the_rescue_of_sir_ragnar.json"
        ))
        .context("the built-in original-US Rescue of Sir Ragnar quest is invalid")
    }

    pub fn original_us_lair_of_the_orc_warlord() -> Result<Self> {
        serde_json::from_str(include_str!(
            "../assets/quests/original_us_03_lair_of_the_orc_warlord.json"
        ))
        .context("the built-in original-US Lair of the Orc Warlord quest is invalid")
    }

    pub fn original_us_prince_magnus_gold() -> Result<Self> {
        serde_json::from_str(include_str!(
            "../assets/quests/original_us_04_prince_magnus_gold.json"
        ))
        .context("the built-in original-US Prince Magnus' Gold quest is invalid")
    }

    pub fn original_us_melars_maze() -> Result<Self> {
        serde_json::from_str(include_str!(
            "../assets/quests/original_us_05_melars_maze.json"
        ))
        .context("the built-in original-US Melar's Maze quest is invalid")
    }

    pub fn original_us_legacy_of_the_orc_warlord() -> Result<Self> {
        serde_json::from_str(include_str!(
            "../assets/quests/original_us_06_legacy_of_the_orc_warlord.json"
        ))
        .context("the built-in original-US Legacy of the Orc Warlord quest is invalid")
    }

    pub fn original_us_lost_wizard() -> Result<Self> {
        serde_json::from_str(include_str!(
            "../assets/quests/original_us_07_the_lost_wizard.json"
        ))
        .context("the built-in original-US Lost Wizard quest is invalid")
    }

    pub fn original_us_fire_mage() -> Result<Self> {
        serde_json::from_str(include_str!(
            "../assets/quests/original_us_08_the_fire_mage.json"
        ))
        .context("the built-in original-US Fire Mage quest is invalid")
    }

    pub fn original_us_race_against_time() -> Result<Self> {
        serde_json::from_str(include_str!(
            "../assets/quests/original_us_09_race_against_time.json"
        ))
        .context("the built-in original-US Race Against Time quest is invalid")
    }

    pub fn original_us_castle_of_mystery() -> Result<Self> {
        serde_json::from_str(include_str!(
            "../assets/quests/original_us_10_castle_of_mystery.json"
        ))
        .context("the built-in original-US Castle of Mystery quest is invalid")
    }

    pub fn original_us_bastion_of_chaos() -> Result<Self> {
        serde_json::from_str(include_str!(
            "../assets/quests/original_us_11_bastion_of_chaos.json"
        ))
        .context("the built-in original-US Bastion of Chaos quest is invalid")
    }

    pub fn original_us_barak_tor() -> Result<Self> {
        serde_json::from_str(include_str!(
            "../assets/quests/original_us_12_barak_tor.json"
        ))
        .context("the built-in original-US Barak Tor quest is invalid")
    }

    pub fn original_us_quest_for_the_spirit_blade() -> Result<Self> {
        serde_json::from_str(include_str!(
            "../assets/quests/original_us_13_quest_for_the_spirit_blade.json"
        ))
        .context("the built-in original-US Quest for the Spirit Blade is invalid")
    }

    pub fn original_us_return_to_barak_tor() -> Result<Self> {
        serde_json::from_str(include_str!(
            "../assets/quests/original_us_14_return_to_barak_tor.json"
        ))
        .context("the built-in original-US Return to Barak Tor quest is invalid")
    }

    pub fn original_us_game_system(index: usize) -> Result<Self> {
        match index {
            0 => Self::original_us_trial(),
            1 => Self::original_us_rescue_of_sir_ragnar(),
            2 => Self::original_us_lair_of_the_orc_warlord(),
            3 => Self::original_us_prince_magnus_gold(),
            4 => Self::original_us_melars_maze(),
            5 => Self::original_us_legacy_of_the_orc_warlord(),
            6 => Self::original_us_lost_wizard(),
            7 => Self::original_us_fire_mage(),
            8 => Self::original_us_race_against_time(),
            9 => Self::original_us_castle_of_mystery(),
            10 => Self::original_us_bastion_of_chaos(),
            11 => Self::original_us_barak_tor(),
            12 => Self::original_us_quest_for_the_spirit_blade(),
            13 => Self::original_us_return_to_barak_tor(),
            _ => anyhow::bail!(
                "original-US quest {} is outside the complete {}-quest Game System campaign",
                index + 1,
                Self::IMPLEMENTED_ORIGINAL_US_QUESTS
            ),
        }
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let json = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read quest {}", path.display()))?;
        serde_json::from_str(&json)
            .with_context(|| format!("quest {} is not valid HeroQuest JSON", path.display()))
    }
}

fn normalized_edge(a: Pos, b: Pos) -> (Pos, Pos) {
    if (a.y, a.x) <= (b.y, b.x) {
        (a, b)
    } else {
        (b, a)
    }
}

fn validate_effect_references(
    effect: &QuestEffectDef,
    monster_names: &HashSet<&str>,
    ally_names: &HashSet<&str>,
    hero_kinds: &HashSet<HeroKind>,
    secret_edges: &HashSet<(Pos, Pos)>,
    trap_positions: &HashSet<Pos>,
) -> Result<()> {
    match effect {
        QuestEffectDef::Gold { amount } | QuestEffectDef::SplitGold { amount } => {
            ensure!(*amount > 0, "quest gold effect must be positive")
        }
        QuestEffectDef::DamageUnlessTrapDisarmed { pos, amount } => {
            ensure!(*amount > 0, "furniture-trap damage must be positive");
            ensure!(
                trap_positions.contains(pos),
                "furniture-trap effect references a missing trap square"
            );
        }
        QuestEffectDef::PotionOfHealing { count }
        | QuestEffectDef::PetrificationPotion { count } => {
            ensure!(*count > 0, "quest potion count must be positive")
        }
        QuestEffectDef::HealingPotion { count, restore } => ensure!(
            *count > 0 && *restore > 0,
            "quest healing potion count and restoration must be positive"
        ),
        QuestEffectDef::ArtifactToHero { hero, .. } => ensure!(
            hero_kinds.contains(hero),
            "quest artifact effect references a Hero absent from setup"
        ),
        QuestEffectDef::ActivateNamed { name } => ensure!(
            monster_names.contains(name.as_str()),
            "activation effect references unknown Monster {name}"
        ),
        QuestEffectDef::AwakenGuardianUnlessTrapDisarmed { name, pos } => {
            ensure!(
                monster_names.contains(name.as_str()),
                "guardian effect references unknown Monster {name}"
            );
            ensure!(
                trap_positions.contains(pos),
                "guardian effect references a missing trap square"
            );
        }
        QuestEffectDef::RevealSecretDoor { a, b } => ensure!(
            secret_edges.contains(&normalized_edge(*a, *b)),
            "quest effect references a secret-door edge absent from the map"
        ),
        QuestEffectDef::Bundle { effects } => {
            ensure!(!effects.is_empty(), "quest effect bundle may not be empty");
            for effect in effects {
                validate_effect_references(
                    effect,
                    monster_names,
                    ally_names,
                    hero_kinds,
                    secret_edges,
                    trap_positions,
                )?;
            }
        }
        QuestEffectDef::Alarm { ally, .. } => ensure!(
            ally_names.contains(ally.as_str()),
            "alarm effect references unknown ally {ally}"
        ),
        QuestEffectDef::Weapon { .. }
        | QuestEffectDef::Armor { .. }
        | QuestEffectDef::Empty
        | QuestEffectDef::Message
        | QuestEffectDef::Artifact { .. }
        | QuestEffectDef::ArtifactToKiller { .. }
        | QuestEffectDef::ForbidFurtherTreasure
        | QuestEffectDef::RevealStoredEquipment => {}
    }
    Ok(())
}

fn validate_trigger_effect_compatibility(
    trigger: &QuestTriggerDef,
    effect: &QuestEffectDef,
) -> Result<()> {
    let searchable = |effect: &QuestEffectDef| -> bool {
        fn supported(effect: &QuestEffectDef) -> bool {
            match effect {
                // Search resolution has no killer context; accepting this
                // card here would silently discard a printed reward.
                QuestEffectDef::ArtifactToKiller { .. } => false,
                QuestEffectDef::Bundle { effects } => effects.iter().all(supported),
                _ => true,
            }
        }
        supported(effect)
    };
    let world = |effect: &QuestEffectDef| -> bool {
        fn supported(effect: &QuestEffectDef) -> bool {
            match effect {
                QuestEffectDef::ActivateNamed { .. }
                | QuestEffectDef::RevealSecretDoor { .. }
                | QuestEffectDef::SplitGold { .. }
                | QuestEffectDef::ArtifactToHero { .. }
                | QuestEffectDef::Message => true,
                QuestEffectDef::Bundle { effects } => effects.iter().all(supported),
                _ => false,
            }
        }
        supported(effect)
    };

    let supported = match trigger {
        QuestTriggerDef::SearchTreasure { .. }
        | QuestTriggerDef::SearchTreasureAfterDefeat { .. } => searchable(effect),
        QuestTriggerDef::RevealRoom { .. } => {
            matches!(effect, QuestEffectDef::Alarm { .. }) || world(effect)
        }
        QuestTriggerDef::OpenDoor { .. } => world(effect),
        QuestTriggerDef::DefeatNamed { .. } => {
            matches!(effect, QuestEffectDef::ArtifactToKiller { .. }) || world(effect)
        }
    };
    ensure!(
        supported,
        "the selected trigger cannot resolve this effect without a silent no-op"
    );
    Ok(())
}

fn effect_grants_artifact(effect: &QuestEffectDef, artifact: Artifact) -> bool {
    match effect {
        QuestEffectDef::Artifact { artifact: granted }
        | QuestEffectDef::ArtifactToHero {
            artifact: granted, ..
        }
        | QuestEffectDef::ArtifactToKiller { artifact: granted } => *granted == artifact,
        QuestEffectDef::Bundle { effects } => effects
            .iter()
            .any(|effect| effect_grants_artifact(effect, artifact)),
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Rect {
    pub x: u8,
    pub y: u8,
    pub width: u8,
    pub height: u8,
}

impl Rect {
    pub const fn new(x: u8, y: u8, width: u8, height: u8) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    const fn key(self) -> (u8, u8, u8, u8) {
        (self.x, self.y, self.width, self.height)
    }

    pub fn positions(self) -> impl Iterator<Item = Pos> {
        (self.y..self.y + self.height)
            .flat_map(move |y| (self.x..self.x + self.width).map(move |x| Pos::new(x, y)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomDef {
    pub name: String,
    pub area: Rect,
    #[serde(default)]
    pub additional_areas: Vec<Rect>,
    pub tint: [f32; 3],
}

impl RoomDef {
    pub fn positions(&self) -> impl Iterator<Item = Pos> + '_ {
        std::iter::once(self.area)
            .chain(self.additional_areas.iter().copied())
            .flat_map(Rect::positions)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorridorDef {
    pub area: Rect,
    pub tint: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoorDef {
    pub a: Pos,
    pub b: Pos,
    #[serde(default)]
    pub open: bool,
    #[serde(default = "default_true")]
    pub searchable: bool,
    #[serde(default)]
    pub false_door: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeroStartDef {
    pub hero: HeroKind,
    pub pos: Pos,
    /// Artifacts carried into a quest whose story explicitly assumes they
    /// were recovered in a preceding campaign chapter.
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonsterDef {
    pub monster: MonsterKind,
    pub pos: Pos,
    #[serde(default)]
    pub model_variant: Option<MonsterModelVariant>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub attack: Option<u8>,
    #[serde(default)]
    pub defend: Option<u8>,
    #[serde(default)]
    pub body: Option<u8>,
    #[serde(default)]
    pub mind: Option<u8>,
    #[serde(default)]
    pub movement: Option<u8>,
    #[serde(default)]
    pub dormant: bool,
    #[serde(default)]
    pub invulnerable_until_acts: bool,
    #[serde(default)]
    pub chaos_spells: Vec<ChaosSpell>,
    #[serde(default)]
    pub escape_target: Option<Pos>,
    #[serde(default)]
    pub immune_to_fire_spells: bool,
    #[serde(default)]
    pub diagonal_attack: bool,
    #[serde(default)]
    pub hidden_until_activated: bool,
    #[serde(default)]
    pub immune_except_spirit_blade: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MonsterModelVariant {
    GoblinSword,
    GoblinAxe,
    GoblinScimitar,
    OrcNotchedSword,
    OrcStaff,
    OrcSword,
    OrcFlail,
    OrcCleaver,
}

impl MonsterModelVariant {
    pub const fn asset_stem(self) -> &'static str {
        match self {
            Self::GoblinSword => "goblin-sword",
            Self::GoblinAxe => "goblin-axe",
            Self::GoblinScimitar => "goblin-scimitar",
            Self::OrcNotchedSword => "orc-notched-sword",
            Self::OrcStaff => "orc-staff",
            Self::OrcSword => "orc-sword",
            Self::OrcFlail => "orc-flail",
            Self::OrcCleaver => "orc-cleaver",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllyDef {
    pub name: String,
    pub figure: MonsterKind,
    pub pos: Pos,
    pub attack: u8,
    pub defend: u8,
    pub body: u8,
    pub mind: u8,
    #[serde(default = "default_ally_movement")]
    pub movement: u8,
}

const fn default_ally_movement() -> u8 {
    6
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropDef {
    pub prop: PropKind,
    pub pos: Pos,
    #[serde(default)]
    pub rotation_quarters: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuestItemDef {
    pub id: String,
    pub prop: PropKind,
    pub pos: Pos,
    #[serde(default)]
    pub sealed_gold: u16,
    #[serde(default)]
    pub held_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrapDef {
    pub trap: TrapKind,
    pub pos: Pos,
    #[serde(default = "default_true")]
    pub trigger_on_entry: bool,
    #[serde(default = "default_true")]
    pub disarmable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TeleportNetworkDef {
    pub destinations: Vec<TeleportDestinationDef>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct TeleportDestinationDef {
    pub total: u8,
    pub pos: Pos,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MineDef {
    pub room: String,
    pub pos: Pos,
    pub amount: u16,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MonsterBountyDef {
    pub monster: MonsterKind,
    pub gold: u16,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct DelayedFallingBlockDef {
    pub pos: Pos,
    pub exit: Pos,
}

const fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuestSourceDef {
    pub edition: String,
    pub book: String,
    pub page: u16,
}

impl QuestSourceDef {
    pub fn reference(&self, detail: &str) -> String {
        format!(
            "{}; {}; scan page {}; {}",
            self.edition, self.book, self.page, detail
        )
    }
}

impl QuestEventDef {
    pub fn source_detail(&self) -> String {
        format!("printed quest note {}", self.id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuestEventDef {
    pub id: String,
    #[serde(default)]
    pub marker: Option<Pos>,
    pub trigger: QuestTriggerDef,
    pub effect: QuestEffectDef,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuestTriggerDef {
    SearchTreasure { room: String },
    SearchTreasureAfterDefeat { room: String, name: String },
    RevealRoom { room: String },
    DefeatNamed { name: String },
    OpenDoor { a: Pos, b: Pos },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuestEffectDef {
    Gold { amount: u16 },
    SplitGold { amount: u16 },
    Weapon { weapon: Weapon },
    Armor { armor: Armor },
    Empty,
    Message,
    DamageUnlessTrapDisarmed { pos: Pos, amount: u8 },
    PotionOfHealing { count: u8 },
    HealingPotion { count: u8, restore: u8 },
    PetrificationPotion { count: u8 },
    Artifact { artifact: Artifact },
    ArtifactToHero { hero: HeroKind, artifact: Artifact },
    ArtifactToKiller { artifact: Artifact },
    ActivateNamed { name: String },
    AwakenGuardianUnlessTrapDisarmed { name: String, pos: Pos },
    RevealSecretDoor { a: Pos, b: Pos },
    ForbidFurtherTreasure,
    RevealStoredEquipment,
    Bundle { effects: Vec<QuestEffectDef> },
    Alarm { ally: String, forbid_treasure: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObjectiveDef {
    DefeatNamed {
        name: String,
        #[serde(default)]
        award_champion_title: bool,
    },
    DefeatNamedAndReturn {
        name: String,
        #[serde(default)]
        reward_gold_per_hero: u16,
    },
    DefeatAllAndReturn,
    DefeatAll,
    ReachStairs,
    RescueNamedAndReturn {
        name: String,
        reward_gold: u16,
    },
    ReturnQuestItems {
        count: u8,
        reward_gold: u16,
    },
    FindArtifactAndReturn {
        artifact: Artifact,
    },
    ResolveEventAndReturn {
        event: String,
        #[serde(default)]
        reward_gold_per_hero: u16,
    },
    EscapeIndependently,
    DefeatAllOrEscapeIndependently,
}

#[cfg(test)]
mod tests {
    use super::{MonsterModelVariant, QuestDefinition};
    use crate::cards::{Artifact, ChaosSpell};
    use crate::game::Game;
    use crate::model::{MonsterKind, Pos, PropKind, TrapKind};

    #[test]
    fn loads_a_quest_from_disk() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/quests/torchlit_cellar.json");
        let quest = QuestDefinition::from_path(path).unwrap();
        assert_eq!(quest.hero_starts.len(), 4);
        assert_eq!(quest.title, "The Torchlit Cellar");
    }

    #[test]
    fn every_original_us_quest_uses_the_one_printed_board_topology() {
        for quest_index in 0..QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS {
            QuestDefinition::original_us_game_system(quest_index)
                .unwrap()
                .validate_original_us_board_topology()
                .unwrap_or_else(|error| {
                    panic!(
                        "quest {} changed the printed board: {error}",
                        quest_index + 1
                    )
                });
        }

        let mut malformed = QuestDefinition::original_us_trial().unwrap();
        malformed.rooms[0].area.height += 1;
        assert!(
            malformed
                .validate_original_us_board_topology()
                .unwrap_err()
                .to_string()
                .contains("immutable original-US board")
        );
    }

    #[test]
    fn every_original_us_placement_trigger_and_objective_inherits_an_exact_quest_book_page() {
        for quest_index in 0..QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS {
            let quest = QuestDefinition::original_us_game_system(quest_index).unwrap();
            quest
                .validate_original_us_source_coverage()
                .unwrap_or_else(|error| {
                    panic!(
                        "quest {} has incomplete provenance: {error}",
                        quest_index + 1
                    )
                });
            let source = quest.source.as_ref().unwrap();
            assert_eq!(source.page, quest_index as u16 + 3);

            for detail in [
                "room and corridor geometry",
                "door and secret-door placement",
                "Hero and Monster placement",
                "furniture, marker, and trap placement",
                "wandering Monster and objective",
            ] {
                let reference = quest.source_reference(detail).unwrap();
                assert!(reference.contains("HeroQuest original US release"));
                assert!(reference.contains(&format!("scan page {}", source.page)));
                assert!(reference.ends_with(detail));
            }
            for event in &quest.events {
                let detail = event.source_detail();
                let reference = quest.source_reference(&detail).unwrap();
                assert!(reference.ends_with(&format!("printed quest note {}", event.id)));
            }
        }
    }

    #[test]
    fn every_original_us_plot_reference_resolves_and_malformed_no_op_content_is_rejected() {
        for quest_index in 0..QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS {
            QuestDefinition::original_us_game_system(quest_index)
                .unwrap()
                .validate_authored_references()
                .unwrap_or_else(|error| {
                    panic!(
                        "quest {} has a detached plot reference: {error}",
                        quest_index + 1
                    )
                });
        }

        let mut missing_objective = QuestDefinition::original_us_trial().unwrap();
        missing_objective.objective = super::ObjectiveDef::DefeatNamed {
            name: "Imaginary Villain".to_owned(),
            award_champion_title: false,
        };
        assert!(
            missing_objective
                .validate_authored_references()
                .unwrap_err()
                .to_string()
                .contains("unknown Monster")
        );

        let mut missing_room = QuestDefinition::original_us_trial().unwrap();
        missing_room.events[0].trigger = super::QuestTriggerDef::SearchTreasure {
            room: "Imaginary Room".to_owned(),
        };
        assert!(
            missing_room
                .validate_authored_references()
                .unwrap_err()
                .to_string()
                .contains("unknown room")
        );

        let mut missing_trap = QuestDefinition::original_us_rescue_of_sir_ragnar().unwrap();
        missing_trap.traps.clear();
        assert!(
            missing_trap
                .validate_authored_references()
                .unwrap_err()
                .to_string()
                .contains("missing trap square")
        );

        let mut reveal_gold_no_op = QuestDefinition::original_us_trial().unwrap();
        let reveal_room = match &reveal_gold_no_op.events[0].trigger {
            super::QuestTriggerDef::SearchTreasure { room } => room.clone(),
            other => panic!("unexpected Trial fixture trigger: {other:?}"),
        };
        reveal_gold_no_op.events[0].trigger =
            super::QuestTriggerDef::RevealRoom { room: reveal_room };
        reveal_gold_no_op.events[0].effect = super::QuestEffectDef::Gold { amount: 50 };
        assert!(
            reveal_gold_no_op
                .validate_authored_references()
                .unwrap_err()
                .to_string()
                .contains("unsupported trigger/effect pair")
        );

        let mut search_killer_no_op = QuestDefinition::original_us_trial().unwrap();
        search_killer_no_op.events[0].effect = super::QuestEffectDef::ArtifactToKiller {
            artifact: Artifact::TalismanOfLore,
        };
        assert!(
            search_killer_no_op
                .validate_authored_references()
                .unwrap_err()
                .to_string()
                .contains("unsupported trigger/effect pair")
        );
    }

    #[test]
    fn all_fourteen_disk_quest_files_pass_the_complete_static_conformance_gate() {
        let quest_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/quests");
        for (quest_index, filename) in QuestDefinition::ORIGINAL_US_QUEST_FILES
            .into_iter()
            .enumerate()
        {
            let quest =
                QuestDefinition::from_path(quest_root.join(filename)).unwrap_or_else(|error| {
                    panic!("quest {} failed schema load: {error}", quest_index + 1)
                });
            quest.validate_original_us_source_coverage().unwrap();
            quest.validate_original_us_board_topology().unwrap();
            quest.validate_authored_references().unwrap();
            let game = Game::from_quest(quest, 0x434f_4e46_4f52_4d00 + quest_index as u64)
                .unwrap_or_else(|error| {
                    panic!(
                        "quest {} failed placement/component validation: {error}",
                        quest_index + 1
                    )
                });
            assert_eq!(
                game.title,
                QuestDefinition::original_us_game_system(quest_index)
                    .unwrap()
                    .title
            );
        }
    }

    #[test]
    fn canonical_wall_catalog_contains_only_real_orthogonal_region_boundaries() {
        let edges = QuestDefinition::original_us_wall_edges();
        assert!(!edges.is_empty());
        assert!(edges.iter().all(|(a, b)| a.is_adjacent(*b)));
        assert!(edges.contains(&(Pos::new(4, 7), Pos::new(5, 7))));
        assert!(edges.contains(&(Pos::new(8, 12), Pos::new(9, 12))));
        assert!(!edges.contains(&(Pos::new(1, 1), Pos::new(2, 1))));
        assert!(!edges.contains(&(Pos::new(12, 4), Pos::new(13, 4))));
    }

    #[test]
    fn topology_rejects_normal_and_secret_doors_drawn_inside_a_printed_room() {
        let mut malformed_normal = QuestDefinition::original_us_trial().unwrap();
        malformed_normal.doors[0].a = Pos::new(1, 1);
        malformed_normal.doors[0].b = Pos::new(2, 1);
        assert!(
            malformed_normal
                .validate_original_us_board_topology()
                .unwrap_err()
                .to_string()
                .contains("not on a printed original-US wall edge")
        );

        let mut malformed_secret = QuestDefinition::original_us_rescue_of_sir_ragnar().unwrap();
        malformed_secret.secret_doors[0].a = Pos::new(5, 13);
        malformed_secret.secret_doors[0].b = Pos::new(6, 13);
        assert!(
            malformed_secret
                .validate_original_us_board_topology()
                .unwrap_err()
                .to_string()
                .contains("not on a printed original-US wall edge")
        );
    }

    #[test]
    fn built_in_campaign_begins_with_the_trial_and_verag() {
        let quest = QuestDefinition::original_us_trial().unwrap();
        assert_eq!(quest.title, "The Trial");
        assert!(
            quest
                .monsters
                .iter()
                .any(|monster| monster.name.as_deref() == Some("Verag"))
        );
        let expected_stairs = [
            crate::model::Pos::new(1, 14),
            crate::model::Pos::new(2, 14),
            crate::model::Pos::new(1, 15),
            crate::model::Pos::new(2, 15),
        ];
        assert_eq!(quest.stairs.as_slice(), expected_stairs.as_slice());
        assert!(
            quest
                .hero_starts
                .iter()
                .all(|hero| expected_stairs.contains(&hero.pos))
        );
    }

    #[test]
    fn the_trial_matches_every_printed_map_object() {
        let quest = QuestDefinition::original_us_trial().unwrap();
        assert_eq!(quest.doors.len(), 12);
        assert_eq!(quest.monsters.len(), 24);
        assert_eq!(quest.props.len(), 16);
        assert_eq!(quest.blocked.len(), 5);
        assert!(quest.traps.is_empty());
        assert!(quest.secret_doors.is_empty());
        assert_eq!(
            quest
                .solid_rock
                .iter()
                .flat_map(|row| row.bytes())
                .filter(|&cell| cell == b'#')
                .count(),
            219
        );

        let doors: Vec<_> = quest.doors.iter().map(|door| (door.a, door.b)).collect();
        assert_eq!(
            doors,
            [
                (Pos::new(4, 2), Pos::new(5, 2)),
                (Pos::new(8, 2), Pos::new(9, 2)),
                (Pos::new(3, 3), Pos::new(3, 4)),
                (Pos::new(3, 8), Pos::new(3, 9)),
                (Pos::new(7, 8), Pos::new(7, 9)),
                (Pos::new(15, 9), Pos::new(16, 9)),
                (Pos::new(0, 11), Pos::new(1, 11)),
                (Pos::new(10, 12), Pos::new(10, 13)),
                (Pos::new(8, 15), Pos::new(9, 15)),
                (Pos::new(13, 15), Pos::new(14, 15)),
                (Pos::new(3, 17), Pos::new(3, 18)),
                (Pos::new(7, 17), Pos::new(7, 18)),
            ]
        );

        let monsters: Vec<_> = quest
            .monsters
            .iter()
            .map(|monster| (monster.monster, monster.pos))
            .collect();
        assert_eq!(
            monsters,
            [
                (MonsterKind::Zombie, Pos::new(6, 1)),
                (MonsterKind::Skeleton, Pos::new(9, 1)),
                (MonsterKind::Skeleton, Pos::new(2, 2)),
                (MonsterKind::Skeleton, Pos::new(3, 2)),
                (MonsterKind::Mummy, Pos::new(6, 2)),
                (MonsterKind::Zombie, Pos::new(6, 3)),
                (MonsterKind::Skeleton, Pos::new(9, 3)),
                (MonsterKind::Mummy, Pos::new(9, 4)),
                (MonsterKind::Orc, Pos::new(4, 5)),
                (MonsterKind::Goblin, Pos::new(3, 6)),
                (MonsterKind::Goblin, Pos::new(6, 7)),
                (MonsterKind::Goblin, Pos::new(7, 7)),
                (MonsterKind::Gargoyle, Pos::new(12, 8)),
                (MonsterKind::Orc, Pos::new(14, 8)),
                (MonsterKind::Goblin, Pos::new(2, 11)),
                (MonsterKind::Orc, Pos::new(11, 11)),
                (MonsterKind::ChaosWarrior, Pos::new(14, 11)),
                (MonsterKind::Orc, Pos::new(2, 12)),
                (MonsterKind::Orc, Pos::new(7, 14)),
                (MonsterKind::Goblin, Pos::new(10, 14)),
                (MonsterKind::Orc, Pos::new(8, 15)),
                (MonsterKind::ChaosWarrior, Pos::new(16, 15)),
                (MonsterKind::Fimir, Pos::new(10, 16)),
                (MonsterKind::ChaosWarrior, Pos::new(15, 16)),
            ]
        );

        let props: Vec<_> = quest
            .props
            .iter()
            .map(|prop| (prop.prop, prop.pos, prop.rotation_quarters))
            .collect();
        assert_eq!(
            props,
            [
                (PropKind::Tomb, Pos::new(10, 1), 0),
                (PropKind::SorcerersTable, Pos::new(1, 5), 3),
                (PropKind::Table, Pos::new(6, 5), 0),
                (PropKind::Chest, Pos::new(10, 5), 2),
                (PropKind::Chest, Pos::new(11, 7), 0),
                (PropKind::Fireplace, Pos::new(12, 7), 0),
                (PropKind::Throne, Pos::new(10, 8), 1),
                (PropKind::Table, Pos::new(11, 9), 0),
                (PropKind::TortureRack, Pos::new(3, 10), 0),
                (PropKind::Bookcase, Pos::new(5, 13), 2),
                (PropKind::Cupboard, Pos::new(15, 13), 2),
                (PropKind::Stairs, Pos::new(1, 14), 1),
                (PropKind::AlchemistsBench, Pos::new(5, 15), 1),
                (PropKind::WeaponRack, Pos::new(11, 15), 1),
                (PropKind::Chest, Pos::new(17, 16), 3),
                (PropKind::Bookcase, Pos::new(15, 17), 0),
            ]
        );

        let guardian = quest
            .monsters
            .iter()
            .find(|monster| monster.name.as_deref() == Some("Guardian of Fellmarg's Tomb"))
            .unwrap();
        assert_eq!(guardian.pos, Pos::new(6, 2));
        assert_eq!(guardian.attack, Some(4));
        assert_eq!(
            quest
                .events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            ["A", "B", "C", "D", "E"]
        );
        assert_eq!(
            quest
                .events
                .iter()
                .map(|event| event.marker.unwrap())
                .collect::<Vec<_>>(),
            [
                Pos::new(11, 14),
                Pos::new(17, 15),
                Pos::new(7, 2),
                Pos::new(11, 5),
                Pos::new(10, 7),
            ]
        );
    }

    #[test]
    fn rescue_of_sir_ragnar_loads_its_ally_alarm_and_poisoned_chest() {
        let quest = QuestDefinition::original_us_rescue_of_sir_ragnar().unwrap();
        assert_eq!(quest.title, "The Rescue of Sir Ragnar");
        assert_eq!(quest.doors.len(), 10);
        assert_eq!(quest.secret_doors.len(), 1);
        assert_eq!(quest.monsters.len(), 21);
        assert_eq!(quest.props.len(), 7);
        assert_eq!(quest.allies.len(), 1);
        assert_eq!(quest.allies[0].name, "Sir Ragnar");
        assert_eq!(quest.allies[0].pos, Pos::new(5, 11));
        assert_eq!(quest.traps.len(), 1);
        assert!(!quest.traps[0].trigger_on_entry);
        assert_eq!(
            quest
                .secret_doors
                .iter()
                .map(|door| (door.a, door.b))
                .collect::<Vec<_>>(),
            [(Pos::new(8, 12), Pos::new(8, 13))]
        );
        assert_eq!(
            quest
                .solid_rock
                .iter()
                .flat_map(|row| row.bytes())
                .filter(|&cell| cell == b'#')
                .count(),
            278
        );
        assert_eq!(
            quest
                .events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            ["A", "B", "FINDING_SIR_RAGNAR"]
        );
    }

    #[test]
    fn lair_of_the_orc_warlord_matches_every_printed_map_object() {
        let quest = QuestDefinition::original_us_lair_of_the_orc_warlord().unwrap();
        assert_eq!(quest.title, "Lair of the Orc Warlord");
        assert_eq!(quest.doors.len(), 9);
        assert!(quest.secret_doors.is_empty());
        assert_eq!(quest.monsters.len(), 19);
        assert_eq!(quest.props.len(), 7);
        assert_eq!(quest.blocked.len(), 3);
        assert_eq!(quest.traps.len(), 1);
        assert_eq!(quest.traps[0].trap, crate::model::TrapKind::Pit);
        assert_eq!(quest.traps[0].pos, Pos::new(8, 11));
        assert_eq!(
            quest
                .solid_rock
                .iter()
                .flat_map(|row| row.bytes())
                .filter(|&cell| cell == b'#')
                .count(),
            346
        );

        let doors: Vec<_> = quest.doors.iter().map(|door| (door.a, door.b)).collect();
        assert_eq!(
            doors,
            [
                (Pos::new(0, 2), Pos::new(1, 2)),
                (Pos::new(8, 4), Pos::new(9, 4)),
                (Pos::new(4, 8), Pos::new(5, 8)),
                (Pos::new(2, 8), Pos::new(2, 9)),
                (Pos::new(5, 9), Pos::new(5, 10)),
                (Pos::new(4, 12), Pos::new(5, 12)),
                (Pos::new(8, 12), Pos::new(8, 13)),
                (Pos::new(2, 13), Pos::new(2, 14)),
                (Pos::new(4, 16), Pos::new(5, 16)),
            ]
        );

        let monsters: Vec<_> = quest
            .monsters
            .iter()
            .map(|monster| (monster.monster, monster.pos))
            .collect();
        assert_eq!(
            monsters,
            [
                (MonsterKind::Fimir, Pos::new(3, 1)),
                (MonsterKind::Orc, Pos::new(2, 2)),
                (MonsterKind::Goblin, Pos::new(7, 4)),
                (MonsterKind::Goblin, Pos::new(8, 5)),
                (MonsterKind::Goblin, Pos::new(5, 6)),
                (MonsterKind::Goblin, Pos::new(7, 8)),
                (MonsterKind::Fimir, Pos::new(4, 8)),
                (MonsterKind::Orc, Pos::new(6, 10)),
                (MonsterKind::Orc, Pos::new(5, 11)),
                (MonsterKind::Goblin, Pos::new(2, 12)),
                (MonsterKind::Goblin, Pos::new(3, 13)),
                (MonsterKind::Orc, Pos::new(6, 14)),
                (MonsterKind::Orc, Pos::new(1, 15)),
                (MonsterKind::Orc, Pos::new(4, 15)),
                (MonsterKind::Goblin, Pos::new(5, 15)),
                (MonsterKind::ChaosWarrior, Pos::new(6, 15)),
                (MonsterKind::Fimir, Pos::new(2, 16)),
                (MonsterKind::Orc, Pos::new(3, 16)),
                (MonsterKind::Goblin, Pos::new(5, 17)),
            ]
        );

        let props: Vec<_> = quest
            .props
            .iter()
            .map(|prop| (prop.prop, prop.pos, prop.rotation_quarters))
            .collect();
        assert_eq!(
            props,
            [
                (PropKind::WeaponRack, Pos::new(4, 1), 3),
                (PropKind::Stairs, Pos::new(9, 1), 1),
                (PropKind::Fireplace, Pos::new(1, 4), 0),
                (PropKind::Table, Pos::new(6, 5), 1),
                (PropKind::Cupboard, Pos::new(1, 10), 3),
                (PropKind::Chest, Pos::new(8, 10), 3),
                (PropKind::Table, Pos::new(7, 15), 1),
            ]
        );

        let ulag = quest
            .monsters
            .iter()
            .find(|monster| monster.name.as_deref() == Some("Ulag"))
            .unwrap();
        assert_eq!(ulag.pos, Pos::new(4, 15));
        assert_eq!(
            ulag.model_variant,
            Some(MonsterModelVariant::OrcNotchedSword)
        );
        assert_eq!(
            (
                ulag.movement,
                ulag.attack,
                ulag.defend,
                ulag.body,
                ulag.mind
            ),
            (Some(10), Some(4), Some(5), Some(2), Some(3))
        );
        assert_eq!(
            quest
                .events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            ["A", "B", "ULAG_BOUNTY"]
        );
    }

    #[test]
    fn prince_magnus_gold_matches_the_printed_components_and_royal_chests() {
        let quest = QuestDefinition::original_us_prince_magnus_gold().unwrap();
        assert_eq!(quest.title, "Prince Magnus' Gold");
        assert_eq!(quest.doors.len(), 14);
        assert_eq!(quest.secret_doors.len(), 1);
        assert_eq!(quest.monsters.len(), 20);
        assert_eq!(quest.props.len(), 8);
        assert_eq!(quest.blocked.len(), 13);
        assert_eq!(quest.traps.len(), 7);
        assert_eq!(quest.quest_items.len(), 3);
        assert!(
            quest
                .quest_items
                .iter()
                .all(|item| { item.prop == PropKind::Chest && item.sealed_gold == 250 })
        );
        assert_eq!(
            quest
                .solid_rock
                .iter()
                .flat_map(|row| row.bytes())
                .filter(|&cell| cell == b'#')
                .count(),
            249
        );
        let gulthor = quest
            .monsters
            .iter()
            .find(|monster| monster.name.as_deref() == Some("Gulthor"))
            .unwrap();
        assert_eq!(gulthor.monster, MonsterKind::ChaosWarrior);
        assert_eq!(gulthor.pos, Pos::new(11, 9));
        assert_eq!(quest.wandering_monster, MonsterKind::Fimir);

        let traps: Vec<_> = quest
            .traps
            .iter()
            .map(|trap| (trap.trap, trap.pos))
            .collect();
        assert_eq!(
            traps,
            [
                (crate::model::TrapKind::Spear, Pos::new(25, 1)),
                (crate::model::TrapKind::Spear, Pos::new(9, 6)),
                (crate::model::TrapKind::Spear, Pos::new(15, 6)),
                (crate::model::TrapKind::Pit, Pos::new(9, 7)),
                (crate::model::TrapKind::Pit, Pos::new(16, 7)),
                (crate::model::TrapKind::Pit, Pos::new(6, 9)),
                (crate::model::TrapKind::Spear, Pos::new(4, 17)),
            ]
        );
    }

    #[test]
    fn melars_maze_matches_the_printed_components_and_notes() {
        let quest = QuestDefinition::original_us_melars_maze().unwrap();
        assert_eq!(quest.title, "Melar's Maze");
        assert_eq!(quest.doors.len(), 13);
        assert_eq!(quest.secret_doors.len(), 2);
        assert_eq!(quest.monsters.len(), 17);
        assert_eq!(quest.props.len(), 9);
        assert_eq!(quest.blocked.len(), 10);
        assert_eq!(quest.traps.len(), 8);
        assert_eq!(
            quest
                .solid_rock
                .iter()
                .flat_map(|row| row.bytes())
                .filter(|&cell| cell == b'#')
                .count(),
            222
        );
        let gargoyle = quest
            .monsters
            .iter()
            .find(|monster| monster.name.as_deref() == Some("Stone Gargoyle"))
            .unwrap();
        assert_eq!(gargoyle.pos, Pos::new(4, 12));
        assert!(gargoyle.dormant);
        assert!(gargoyle.invulnerable_until_acts);
        assert_eq!(
            quest
                .events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            ["A", "B", "C", "D", "E"]
        );
        assert_eq!(quest.wandering_monster, MonsterKind::Zombie);
    }

    #[test]
    fn legacy_of_the_orc_warlord_matches_the_printed_components_and_grak() {
        let quest = QuestDefinition::original_us_legacy_of_the_orc_warlord().unwrap();
        assert_eq!(quest.title, "Legacy of the Orc Warlord");
        assert_eq!(quest.doors.len(), 10);
        assert!(quest.secret_doors.is_empty());
        assert_eq!(quest.monsters.len(), 26);
        assert_eq!(quest.props.len(), 7);
        assert_eq!(quest.blocked.len(), 6);
        assert_eq!(quest.traps.len(), 3);
        assert!(quest.heroes_captured);
        assert_eq!(quest.forbidden_treasure_rooms, ["Southwest Armory"]);
        assert_eq!(
            quest
                .solid_rock
                .iter()
                .flat_map(|row| row.bytes())
                .filter(|&cell| cell == b'#')
                .count(),
            236
        );
        let grak = quest
            .monsters
            .iter()
            .find(|monster| monster.name.as_deref() == Some("Grak"))
            .unwrap();
        assert_eq!(grak.pos, Pos::new(6, 15));
        assert_eq!(grak.model_variant, Some(MonsterModelVariant::OrcStaff));
        assert_eq!(
            (
                grak.movement,
                grak.attack,
                grak.defend,
                grak.body,
                grak.mind
            ),
            (Some(8), Some(4), Some(4), Some(3), Some(3))
        );
        assert_eq!(
            grak.chaos_spells,
            [ChaosSpell::Fear, ChaosSpell::Sleep, ChaosSpell::Tempest]
        );
        assert_eq!(quest.wandering_monster, MonsterKind::Fimir);
        assert!(matches!(
            quest.objective,
            super::ObjectiveDef::EscapeIndependently
        ));
    }

    #[test]
    fn lost_wizard_matches_the_printed_map_and_wardoz_notes() {
        let quest = QuestDefinition::original_us_lost_wizard().unwrap();
        assert_eq!(quest.title, "The Lost Wizard");
        assert_eq!(quest.doors.len(), 11);
        assert!(quest.secret_doors.is_empty());
        assert_eq!(quest.monsters.len(), 17);
        assert_eq!(quest.props.len(), 7);
        assert_eq!(quest.blocked.len(), 8);
        assert_eq!(quest.traps.len(), 3);
        assert_eq!(
            quest
                .solid_rock
                .iter()
                .flat_map(|row| row.bytes())
                .filter(|&cell| cell == b'#')
                .count(),
            282
        );
        assert_eq!(
            quest
                .monsters
                .iter()
                .filter(|monster| monster.monster == MonsterKind::ChaosWarrior)
                .map(|monster| monster.defend)
                .collect::<Vec<_>>(),
            [Some(5); 4]
        );
        let wardoz = quest
            .monsters
            .iter()
            .find(|monster| monster.name.as_deref() == Some("Wardoz"))
            .unwrap();
        assert_eq!(wardoz.monster, MonsterKind::Zombie);
        assert_eq!(wardoz.pos, Pos::new(16, 14));
        assert_eq!(quest.wandering_monster, MonsterKind::Mummy);
        assert_eq!(
            quest
                .events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            ["B", "C", "D"]
        );
    }

    #[test]
    fn fire_mage_matches_the_printed_map_balur_and_rewards() {
        let quest = QuestDefinition::original_us_fire_mage().unwrap();
        assert_eq!(QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS, 14);
        assert_eq!(
            QuestDefinition::original_us_game_system(7).unwrap().title,
            "The Fire Mage"
        );
        assert_eq!(quest.doors.len(), 16);
        assert_eq!(quest.secret_doors.len(), 2);
        assert_eq!(quest.monsters.len(), 24);
        assert_eq!(quest.props.len(), 6);
        assert_eq!(quest.blocked.len(), 11);
        assert_eq!(quest.traps.len(), 10);
        assert_eq!(
            quest
                .solid_rock
                .iter()
                .flat_map(|row| row.bytes())
                .filter(|&cell| cell == b'#')
                .count(),
            158
        );
        assert_eq!(
            quest
                .doors
                .iter()
                .map(|door| (door.a, door.b))
                .collect::<Vec<_>>(),
            [
                (Pos::new(2, 0), Pos::new(2, 1)),
                (Pos::new(3, 3), Pos::new(3, 4)),
                (Pos::new(0, 6), Pos::new(1, 6)),
                (Pos::new(24, 6), Pos::new(25, 6)),
                (Pos::new(2, 8), Pos::new(2, 9)),
                (Pos::new(4, 9), Pos::new(4, 10)),
                (Pos::new(15, 9), Pos::new(16, 9)),
                (Pos::new(19, 9), Pos::new(19, 10)),
                (Pos::new(24, 11), Pos::new(25, 11)),
                (Pos::new(3, 13), Pos::new(3, 14)),
                (Pos::new(19, 13), Pos::new(19, 14)),
                (Pos::new(15, 17), Pos::new(15, 18)),
                (Pos::new(19, 17), Pos::new(19, 18)),
                (Pos::new(2, 17), Pos::new(2, 18)),
                (Pos::new(5, 17), Pos::new(5, 18)),
                (Pos::new(7, 17), Pos::new(7, 18)),
            ]
        );
        assert_eq!(
            quest
                .secret_doors
                .iter()
                .map(|door| (door.a, door.b))
                .collect::<Vec<_>>(),
            [
                (Pos::new(6, 10), Pos::new(7, 10)),
                (Pos::new(8, 12), Pos::new(9, 12)),
            ]
        );
        assert_eq!(
            quest
                .traps
                .iter()
                .map(|trap| (trap.trap, trap.pos))
                .collect::<Vec<_>>(),
            [
                (TrapKind::Pit, Pos::new(1, 0)),
                (TrapKind::Pit, Pos::new(9, 6)),
                (TrapKind::Pit, Pos::new(6, 17)),
                (TrapKind::Pit, Pos::new(7, 18)),
                (TrapKind::Spear, Pos::new(0, 5)),
                (TrapKind::Spear, Pos::new(16, 8)),
                (TrapKind::Spear, Pos::new(25, 9)),
                (TrapKind::Spear, Pos::new(25, 10)),
                (TrapKind::Spear, Pos::new(0, 12)),
                (TrapKind::Spear, Pos::new(1, 18)),
            ]
        );
        let balur = quest
            .monsters
            .iter()
            .find(|monster| monster.name.as_deref() == Some("Balur"))
            .unwrap();
        assert_eq!(balur.monster, MonsterKind::ChaosSorcerer);
        assert_eq!(balur.pos, Pos::new(2, 3));
        assert_eq!(
            (
                balur.movement,
                balur.attack,
                balur.defend,
                balur.body,
                balur.mind
            ),
            (Some(8), Some(2), Some(5), Some(3), Some(7))
        );
        assert_eq!(
            balur.chaos_spells,
            [
                ChaosSpell::BallOfFlame,
                ChaosSpell::Firestorm,
                ChaosSpell::Tempest,
                ChaosSpell::SummonOrcs,
                ChaosSpell::Fear,
                ChaosSpell::Escape,
            ]
        );
        assert_eq!(balur.escape_target, Some(Pos::new(12, 9)));
        assert!(balur.immune_to_fire_spells);
        assert_eq!(quest.wandering_monster, MonsterKind::Fimir);
        assert!(matches!(
            &quest.events[0].effect,
            super::QuestEffectDef::Bundle { effects }
                if matches!(effects.as_slice(), [
                    super::QuestEffectDef::Gold { amount: 150 },
                    super::QuestEffectDef::Artifact { artifact: Artifact::WandOfMagic }
                ])
        ));
        assert!(matches!(
            quest.objective,
            super::ObjectiveDef::DefeatNamedAndReturn {
                ref name,
                reward_gold_per_hero: 100
            } if name == "Balur"
        ));
    }

    #[test]
    fn race_against_time_matches_the_printed_map_and_three_chests() {
        let quest = QuestDefinition::original_us_race_against_time().unwrap();
        assert_eq!(
            QuestDefinition::original_us_game_system(8).unwrap().title,
            "Race Against Time"
        );
        assert_eq!(quest.doors.len(), 12);
        assert_eq!(quest.secret_doors.len(), 2);
        assert_eq!(quest.monsters.len(), 24);
        assert_eq!(quest.props.len(), 6);
        assert_eq!(quest.blocked.len(), 5);
        assert_eq!(quest.traps.len(), 1);
        assert!(!quest.traps[0].trigger_on_entry);
        assert_eq!(quest.traps[0].pos, Pos::new(8, 11));
        assert_eq!(
            quest
                .solid_rock
                .iter()
                .flat_map(|row| row.bytes())
                .filter(|&cell| cell == b'#')
                .count(),
            240
        );
        assert_eq!(
            quest
                .doors
                .iter()
                .map(|door| (door.a, door.b))
                .collect::<Vec<_>>(),
            [
                (Pos::new(2, 0), Pos::new(2, 1)),
                (Pos::new(3, 3), Pos::new(3, 4)),
                (Pos::new(4, 5), Pos::new(5, 5)),
                (Pos::new(4, 8), Pos::new(4, 9)),
                (Pos::new(4, 11), Pos::new(5, 11)),
                (Pos::new(2, 13), Pos::new(2, 14)),
                (Pos::new(19, 13), Pos::new(19, 14)),
                (Pos::new(11, 14), Pos::new(12, 14)),
                (Pos::new(4, 15), Pos::new(5, 15)),
                (Pos::new(8, 15), Pos::new(9, 15)),
                (Pos::new(17, 16), Pos::new(18, 16)),
                (Pos::new(20, 16), Pos::new(21, 16)),
            ]
        );
        assert_eq!(
            quest
                .secret_doors
                .iter()
                .map(|door| (door.a, door.b))
                .collect::<Vec<_>>(),
            [
                (Pos::new(6, 11), Pos::new(7, 11)),
                (Pos::new(19, 17), Pos::new(19, 18)),
            ]
        );
        assert_eq!(
            quest
                .hero_starts
                .iter()
                .map(|start| start.pos)
                .collect::<Vec<_>>(),
            [
                Pos::new(18, 14),
                Pos::new(20, 14),
                Pos::new(18, 17),
                Pos::new(20, 17),
            ]
        );
        assert_eq!(
            quest
                .monsters
                .iter()
                .map(|monster| (monster.monster, monster.pos))
                .collect::<Vec<_>>(),
            [
                (MonsterKind::Orc, Pos::new(1, 0)),
                (MonsterKind::Goblin, Pos::new(2, 4)),
                (MonsterKind::Goblin, Pos::new(4, 4)),
                (MonsterKind::Goblin, Pos::new(3, 5)),
                (MonsterKind::Goblin, Pos::new(2, 6)),
                (MonsterKind::Orc, Pos::new(4, 9)),
                (MonsterKind::Orc, Pos::new(5, 10)),
                (MonsterKind::Orc, Pos::new(5, 12)),
                (MonsterKind::ChaosWarrior, Pos::new(7, 12)),
                (MonsterKind::Orc, Pos::new(18, 12)),
                (MonsterKind::Orc, Pos::new(19, 12)),
                (MonsterKind::Orc, Pos::new(20, 12)),
                (MonsterKind::Fimir, Pos::new(7, 14)),
                (MonsterKind::Goblin, Pos::new(10, 14)),
                (MonsterKind::Orc, Pos::new(13, 14)),
                (MonsterKind::Fimir, Pos::new(7, 15)),
                (MonsterKind::ChaosWarrior, Pos::new(9, 15)),
                (MonsterKind::Goblin, Pos::new(10, 15)),
                (MonsterKind::Goblin, Pos::new(17, 15)),
                (MonsterKind::Goblin, Pos::new(21, 15)),
                (MonsterKind::Orc, Pos::new(16, 16)),
                (MonsterKind::Orc, Pos::new(22, 16)),
                (MonsterKind::Goblin, Pos::new(17, 17)),
                (MonsterKind::Goblin, Pos::new(21, 17)),
            ]
        );
        assert_eq!(
            quest
                .props
                .iter()
                .map(|prop| (prop.prop, prop.pos, prop.rotation_quarters))
                .collect::<Vec<_>>(),
            [
                (PropKind::Stairs, Pos::new(7, 4), 2),
                (PropKind::Chest, Pos::new(6, 10), 3),
                (PropKind::Chest, Pos::new(8, 11), 3),
                (PropKind::Chest, Pos::new(6, 12), 3),
                (PropKind::Fireplace, Pos::new(1, 15), 1),
                (PropKind::Table, Pos::new(5, 16), 0),
            ]
        );
        assert_eq!(
            quest
                .events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            ["B1", "B2", "C"]
        );
        assert!(matches!(
            &quest.events[2].effect,
            super::QuestEffectDef::Bundle { effects }
                if matches!(effects.as_slice(), [
                    super::QuestEffectDef::DamageUnlessTrapDisarmed {
                        pos,
                        amount: 3
                    },
                    super::QuestEffectDef::Artifact {
                        artifact: Artifact::ElixirOfLife
                    }
                ] if *pos == Pos::new(8, 11))
        ));
        assert!(matches!(quest.objective, super::ObjectiveDef::ReachStairs));
        assert_eq!(quest.wandering_monster, MonsterKind::Fimir);
    }

    #[test]
    fn castle_of_mystery_matches_the_printed_map_and_numbered_squares() {
        let quest = QuestDefinition::original_us_castle_of_mystery().unwrap();
        assert_eq!(
            QuestDefinition::original_us_game_system(9).unwrap().title,
            "Castle of Mystery"
        );
        assert_eq!(quest.doors.len(), 10);
        assert!(quest.secret_doors.is_empty());
        assert_eq!(quest.monsters.len(), 20);
        assert_eq!(quest.props.len(), 1);
        assert!(quest.blocked.is_empty());
        assert!(quest.traps.is_empty());
        assert_eq!(
            quest
                .solid_rock
                .iter()
                .flat_map(|row| row.bytes())
                .filter(|&cell| cell == b'#')
                .count(),
            325
        );
        assert_eq!(
            quest
                .doors
                .iter()
                .map(|door| (door.a, door.b))
                .collect::<Vec<_>>(),
            [
                (Pos::new(8, 2), Pos::new(9, 2)),
                (Pos::new(16, 2), Pos::new(17, 2)),
                (Pos::new(22, 4), Pos::new(22, 5)),
                (Pos::new(4, 6), Pos::new(5, 6)),
                (Pos::new(15, 9), Pos::new(16, 9)),
                (Pos::new(6, 11), Pos::new(7, 11)),
                (Pos::new(4, 13), Pos::new(4, 14)),
                (Pos::new(22, 13), Pos::new(22, 14)),
                (Pos::new(8, 15), Pos::new(9, 15)),
                (Pos::new(17, 16), Pos::new(18, 16)),
            ]
        );
        assert_eq!(
            quest
                .monsters
                .iter()
                .map(|monster| (monster.monster, monster.pos))
                .collect::<Vec<_>>(),
            [
                (MonsterKind::Skeleton, Pos::new(10, 1)),
                (MonsterKind::Orc, Pos::new(22, 1)),
                (MonsterKind::Orc, Pos::new(21, 2)),
                (MonsterKind::Zombie, Pos::new(10, 3)),
                (MonsterKind::Orc, Pos::new(24, 3)),
                (MonsterKind::Zombie, Pos::new(9, 4)),
                (MonsterKind::Orc, Pos::new(3, 5)),
                (MonsterKind::Orc, Pos::new(2, 6)),
                (MonsterKind::Goblin, Pos::new(13, 8)),
                (MonsterKind::Goblin, Pos::new(12, 9)),
                (MonsterKind::ChaosWarrior, Pos::new(5, 10)),
                (MonsterKind::Goblin, Pos::new(11, 10)),
                (MonsterKind::Goblin, Pos::new(13, 11)),
                (MonsterKind::ChaosWarrior, Pos::new(22, 11)),
                (MonsterKind::ChaosWarrior, Pos::new(24, 12)),
                (MonsterKind::Mummy, Pos::new(16, 13)),
                (MonsterKind::Skeleton, Pos::new(15, 15)),
                (MonsterKind::Mummy, Pos::new(10, 16)),
                (MonsterKind::Skeleton, Pos::new(14, 16)),
                (MonsterKind::Skeleton, Pos::new(16, 17)),
            ]
        );
        let network = quest.teleport_network.as_ref().unwrap();
        assert_eq!(
            network
                .destinations
                .iter()
                .map(|destination| (destination.total, destination.pos))
                .collect::<Vec<_>>(),
            [
                (2, Pos::new(4, 14)),
                (3, Pos::new(9, 2)),
                (4, Pos::new(3, 6)),
                (5, Pos::new(5, 11)),
                (6, Pos::new(9, 15)),
                (7, Pos::new(14, 9)),
                (8, Pos::new(15, 2)),
                (9, Pos::new(16, 16)),
                (10, Pos::new(22, 12)),
                (11, Pos::new(22, 3)),
                (12, Pos::new(4, 14)),
            ]
        );
        let mine = quest.mine.as_ref().unwrap();
        assert_eq!(mine.room, "Northern Shrine");
        assert_eq!(mine.pos, Pos::new(14, 4));
        assert_eq!(mine.amount, 5_000);
        assert_eq!(
            quest
                .monsters
                .iter()
                .filter(|monster| monster.name.as_deref() == Some("Ring Guardian"))
                .count(),
            2
        );
        assert_eq!(
            quest.wandering_event_message.as_deref(),
            Some("Ollar's ghost appears, chuckles madly, and disappears.")
        );
        assert!(matches!(
            quest.objective,
            super::ObjectiveDef::DefeatAllOrEscapeIndependently
        ));
    }

    #[test]
    fn bastion_of_chaos_matches_the_printed_map_and_bounties() {
        let quest = QuestDefinition::original_us_bastion_of_chaos().unwrap();
        assert_eq!(
            QuestDefinition::original_us_game_system(10).unwrap().title,
            "Bastion of Chaos"
        );
        assert_eq!(quest.doors.len(), 16);
        assert_eq!(quest.secret_doors.len(), 1);
        assert_eq!(quest.monsters.len(), 23);
        assert_eq!(quest.props.len(), 11);
        assert_eq!(quest.blocked.len(), 9);
        assert_eq!(quest.traps.len(), 5);
        assert_eq!(
            quest
                .secret_doors
                .iter()
                .map(|door| (door.a, door.b))
                .collect::<Vec<_>>(),
            [(Pos::new(6, 11), Pos::new(7, 11))]
        );
        assert_eq!(
            quest
                .solid_rock
                .iter()
                .flat_map(|row| row.bytes())
                .filter(|&cell| cell == b'#')
                .count(),
            189
        );
        assert_eq!(
            quest
                .monsters
                .iter()
                .fold(std::collections::HashMap::new(), |mut counts, monster| {
                    *counts.entry(monster.monster).or_insert(0) += 1;
                    counts
                }),
            std::collections::HashMap::from([
                (MonsterKind::Goblin, 9),
                (MonsterKind::Orc, 7),
                (MonsterKind::Fimir, 2),
                (MonsterKind::ChaosWarrior, 4),
                (MonsterKind::Gargoyle, 1),
            ])
        );
        let gargoyle = quest
            .monsters
            .iter()
            .find(|monster| monster.name.as_deref() == Some("Stone Gargoyle"))
            .unwrap();
        assert_eq!(gargoyle.pos, Pos::new(5, 12));
        assert!(gargoyle.dormant && gargoyle.invulnerable_until_acts);
        assert_eq!(
            quest
                .monster_bounties
                .iter()
                .map(|bounty| (bounty.monster, bounty.gold))
                .collect::<Vec<_>>(),
            [
                (MonsterKind::Goblin, 10),
                (MonsterKind::Orc, 20),
                (MonsterKind::Fimir, 30),
                (MonsterKind::ChaosWarrior, 50),
            ]
        );
        assert!(matches!(
            quest.events[0].effect,
            super::QuestEffectDef::Armor {
                armor: crate::equipment::Armor::Shield
            }
        ));
        assert!(matches!(quest.objective, super::ObjectiveDef::DefeatAll));
        assert_eq!(quest.wandering_monster, MonsterKind::Fimir);
    }

    #[test]
    fn barak_tor_matches_the_printed_map_star_and_witch_lord() {
        let quest = QuestDefinition::original_us_barak_tor().unwrap();
        assert_eq!(
            QuestDefinition::original_us_game_system(11).unwrap().title,
            "Barak Tor - Barrow of the Witch Lord"
        );
        assert_eq!(quest.doors.len(), 15);
        assert_eq!(quest.doors.iter().filter(|door| door.false_door).count(), 3);
        assert_eq!(quest.secret_doors.len(), 4);
        assert_eq!(quest.monsters.len(), 14);
        assert_eq!(quest.props.len(), 6);
        assert_eq!(quest.blocked.len(), 12);
        assert_eq!(quest.traps.len(), 6);
        assert_eq!(
            quest
                .secret_doors
                .iter()
                .map(|door| (door.a, door.b))
                .collect::<Vec<_>>(),
            [
                (Pos::new(11, 4), Pos::new(12, 4)),
                (Pos::new(15, 6), Pos::new(15, 7)),
                (Pos::new(4, 16), Pos::new(5, 16)),
                (Pos::new(8, 16), Pos::new(9, 16)),
            ]
        );
        assert_eq!(
            quest
                .solid_rock
                .iter()
                .flat_map(|row| row.bytes())
                .filter(|&cell| cell == b'#')
                .count(),
            206
        );
        let star = &quest.quest_items[0];
        assert_eq!(star.id, "Star of the West");
        assert_eq!(star.held_by.as_deref(), Some("Star Bearer"));
        assert_eq!(star.prop, PropKind::StarOfWest);
        let witch_lord = quest
            .monsters
            .iter()
            .find(|monster| monster.name.as_deref() == Some("Witch Lord"))
            .unwrap();
        assert_eq!(witch_lord.monster, MonsterKind::ChaosSorcerer);
        assert_eq!(witch_lord.pos, Pos::new(1, 10));
        assert_eq!(witch_lord.movement, Some(1));
        assert_eq!(witch_lord.attack, Some(2));
        assert!(
            witch_lord.dormant
                && witch_lord.hidden_until_activated
                && witch_lord.immune_except_spirit_blade
        );
        assert_eq!(
            witch_lord.chaos_spells,
            [
                ChaosSpell::SummonUndead,
                ChaosSpell::Fear,
                ChaosSpell::Command,
                ChaosSpell::BallOfFlame,
            ]
        );
        assert_eq!(
            quest.delayed_falling_block.as_ref().unwrap().pos,
            Pos::new(0, 0)
        );
        assert!(matches!(
            quest.objective,
            super::ObjectiveDef::ReturnQuestItems {
                count: 1,
                reward_gold: 200
            }
        ));
        assert_eq!(quest.wandering_monster, MonsterKind::Skeleton);
    }

    #[test]
    fn spirit_blade_quest_matches_every_printed_map_object_and_note() {
        let quest = QuestDefinition::original_us_quest_for_the_spirit_blade().unwrap();
        assert_eq!(
            QuestDefinition::original_us_game_system(12).unwrap().title,
            "Quest for the Spirit Blade"
        );
        assert_eq!(quest.doors.len(), 13);
        assert!(quest.secret_doors.is_empty());
        assert_eq!(quest.monsters.len(), 20);
        assert_eq!(quest.props.len(), 4);
        assert_eq!(quest.blocked.len(), 8);
        assert_eq!(quest.traps.len(), 2);
        assert_eq!(quest.collapsing_ceiling_hazards.len(), 6);
        assert_eq!(
            quest
                .solid_rock
                .iter()
                .flat_map(|row| row.bytes())
                .filter(|&cell| cell == b'#')
                .count(),
            277
        );
        assert_eq!(
            quest
                .monsters
                .iter()
                .fold(std::collections::HashMap::new(), |mut counts, monster| {
                    *counts.entry(monster.monster).or_insert(0) += 1;
                    counts
                }),
            std::collections::HashMap::from([
                (MonsterKind::Orc, 7),
                (MonsterKind::ChaosWarrior, 2),
                (MonsterKind::Fimir, 3),
                (MonsterKind::Goblin, 3),
                (MonsterKind::Skeleton, 4),
                (MonsterKind::Mummy, 1),
            ])
        );
        assert_eq!(
            quest.collapsing_ceiling_hazards,
            [
                Pos::new(0, 9),
                Pos::new(19, 9),
                Pos::new(9, 11),
                Pos::new(12, 14),
                Pos::new(13, 16),
                Pos::new(19, 17),
            ]
        );
        assert!(matches!(
            quest.events[0].effect,
            super::QuestEffectDef::Artifact {
                artifact: Artifact::SpiritBlade
            }
        ));
        assert!(matches!(
            quest.events[1].effect,
            super::QuestEffectDef::Gold { amount: 200 }
        ));
        assert!(matches!(
            quest.objective,
            super::ObjectiveDef::FindArtifactAndReturn {
                artifact: Artifact::SpiritBlade
            }
        ));
        assert_eq!(quest.wandering_monster, MonsterKind::ChaosWarrior);
    }

    #[test]
    fn return_to_barak_tor_matches_the_final_printed_map_and_witch_lord() {
        let quest = QuestDefinition::original_us_return_to_barak_tor().unwrap();
        assert_eq!(QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS, 14);
        assert_eq!(
            QuestDefinition::original_us_game_system(13).unwrap().title,
            "Return to Barak Tor"
        );
        assert_eq!(quest.doors.len(), 14);
        assert_eq!(quest.secret_doors.len(), 1);
        assert_eq!(
            (quest.secret_doors[0].a, quest.secret_doors[0].b),
            (Pos::new(11, 4), Pos::new(12, 4))
        );
        assert_eq!(quest.monsters.len(), 23);
        assert_eq!(quest.props.len(), 3);
        assert_eq!(quest.blocked.len(), 10);
        assert_eq!(quest.traps.len(), 2);
        assert_eq!(
            quest
                .solid_rock
                .iter()
                .flat_map(|row| row.bytes())
                .filter(|&cell| cell == b'#')
                .count(),
            242
        );
        assert_eq!(
            quest
                .monsters
                .iter()
                .fold(std::collections::HashMap::new(), |mut counts, monster| {
                    *counts.entry(monster.monster).or_insert(0) += 1;
                    counts
                }),
            std::collections::HashMap::from([
                (MonsterKind::Skeleton, 13),
                (MonsterKind::Zombie, 5),
                (MonsterKind::Mummy, 4),
                (MonsterKind::ChaosSorcerer, 1),
            ])
        );
        let witch_lord = quest
            .monsters
            .iter()
            .find(|monster| monster.name.as_deref() == Some("Witch Lord"))
            .unwrap();
        assert_eq!(witch_lord.pos, Pos::new(23, 3));
        assert_eq!(witch_lord.movement, Some(10));
        assert_eq!(witch_lord.attack, Some(5));
        assert_eq!(witch_lord.defend, Some(6));
        assert_eq!(witch_lord.body, Some(4));
        assert_eq!(witch_lord.mind, Some(6));
        assert!(witch_lord.immune_except_spirit_blade);
        assert_eq!(
            witch_lord.chaos_spells,
            [
                ChaosSpell::SummonUndead,
                ChaosSpell::Fear,
                ChaosSpell::Fear,
                ChaosSpell::BallOfFlame,
                ChaosSpell::Command,
                ChaosSpell::Tempest,
            ]
        );
        assert!(
            quest.hero_starts[0]
                .artifacts
                .contains(&Artifact::SpiritBlade)
        );
        assert!(matches!(
            quest.events[0].effect,
            super::QuestEffectDef::ArtifactToKiller {
                artifact: Artifact::SpellRing
            }
        ));
        assert!(matches!(
            quest.objective,
            super::ObjectiveDef::DefeatNamed {
                ref name,
                award_champion_title: true
            } if name == "Witch Lord"
        ));
        assert_eq!(quest.wandering_monster, MonsterKind::Mummy);
    }
}
