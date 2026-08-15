//! Machine-readable provenance for the private original-US scans.
//!
//! `scan_page` is one-based in the named PDF. A range is inclusive. Card and
//! sheet sources use `item` to identify the exact printed face within a page.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OriginalUsSource {
    pub document: &'static str,
    pub scan_page: u16,
    pub scan_page_end: u16,
    pub item: &'static str,
}

impl OriginalUsSource {
    pub const fn new(
        document: &'static str,
        scan_page: u16,
        scan_page_end: u16,
        item: &'static str,
    ) -> Self {
        Self {
            document,
            scan_page,
            scan_page_end,
            item,
        }
    }

    pub const fn card(scan_page: u16, item: &'static str) -> Self {
        Self::new("Cards.pdf", scan_page, scan_page, item)
    }

    pub const fn is_complete(self) -> bool {
        !self.document.is_empty()
            && self.scan_page > 0
            && self.scan_page_end >= self.scan_page
            && !self.item.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OriginalUsRuleSection {
    ComponentsAndPreparation,
    GameSetup,
    TurnSequence,
    MovementDoorsLooking,
    CombatDeathEquipment,
    HeroSpells,
    TreasurePotionsArtifacts,
    SecretDoors,
    Traps,
    Zargon,
    QuestEndingCampaign,
}

impl OriginalUsRuleSection {
    pub const fn id(self) -> &'static str {
        match self {
            Self::ComponentsAndPreparation => "components_and_preparation",
            Self::GameSetup => "game_setup",
            Self::TurnSequence => "turn_sequence",
            Self::MovementDoorsLooking => "movement_doors_looking",
            Self::CombatDeathEquipment => "combat_death_equipment",
            Self::HeroSpells => "hero_spells",
            Self::TreasurePotionsArtifacts => "treasure_potions_artifacts",
            Self::SecretDoors => "secret_doors",
            Self::Traps => "traps",
            Self::Zargon => "zargon",
            Self::QuestEndingCampaign => "quest_ending_campaign",
        }
    }

    pub const fn source(self) -> OriginalUsSource {
        match self {
            Self::ComponentsAndPreparation => OriginalUsSource::new(
                "Instruction Booklet - Computer View.pdf",
                3,
                6,
                "contents, preparation, character records, and Zargon screen",
            ),
            Self::GameSetup => OriginalUsSource::new(
                "Instruction Booklet - Computer View.pdf",
                4,
                7,
                "game setup and spell-group selection",
            ),
            Self::TurnSequence => OriginalUsSource::new(
                "Instruction Booklet - Computer View.pdf",
                7,
                7,
                "Hero and Zargon turn sequence",
            ),
            Self::MovementDoorsLooking => OriginalUsSource::new(
                "Instruction Booklet - Computer View.pdf",
                7,
                8,
                "movement, doors, looking, and line of sight",
            ),
            Self::CombatDeathEquipment => OriginalUsSource::new(
                "Instruction Booklet - Computer View.pdf",
                8,
                12,
                "combat, defense, death, weapons, and armor",
            ),
            Self::HeroSpells => OriginalUsSource::new(
                "Instruction Booklet - Computer View.pdf",
                8,
                9,
                "casting elemental spells",
            ),
            Self::TreasurePotionsArtifacts => OriginalUsSource::new(
                "Instruction Booklet - Computer View.pdf",
                9,
                10,
                "treasure search, Treasure cards, potions, and artifacts",
            ),
            Self::SecretDoors => OriginalUsSource::new(
                "Instruction Booklet - Computer View.pdf",
                9,
                9,
                "searching for and opening secret doors",
            ),
            Self::Traps => OriginalUsSource::new(
                "Instruction Booklet - Computer View.pdf",
                10,
                11,
                "trap search, springing, jumping, and disarming",
            ),
            Self::Zargon => OriginalUsSource::new(
                "Instruction Booklet - Computer View.pdf",
                11,
                12,
                "monster movement, attacks, spells, and component limits",
            ),
            Self::QuestEndingCampaign => OriginalUsSource::new(
                "Instruction Booklet - Computer View.pdf",
                12,
                12,
                "quest ending, rewards, character records, and Armory",
            ),
        }
    }
}

pub const ORIGINAL_US_RULE_SECTIONS: [OriginalUsRuleSection; 11] = [
    OriginalUsRuleSection::ComponentsAndPreparation,
    OriginalUsRuleSection::GameSetup,
    OriginalUsRuleSection::TurnSequence,
    OriginalUsRuleSection::MovementDoorsLooking,
    OriginalUsRuleSection::CombatDeathEquipment,
    OriginalUsRuleSection::HeroSpells,
    OriginalUsRuleSection::TreasurePotionsArtifacts,
    OriginalUsRuleSection::SecretDoors,
    OriginalUsRuleSection::Traps,
    OriginalUsRuleSection::Zargon,
    OriginalUsRuleSection::QuestEndingCampaign,
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use serde::Deserialize;

    use super::*;
    use crate::{
        cards::{
            ORIGINAL_US_ARTIFACTS, ORIGINAL_US_CHAOS_SPELLS, ORIGINAL_US_HERO_SPELLS,
            original_us_treasure_cards,
        },
        equipment::ORIGINAL_US_ARMORY,
        game::Game,
        model::ORIGINAL_US_MONSTER_CARDS,
        quest::QuestDefinition,
    };

    #[derive(Debug, Deserialize)]
    struct ReplayIndex {
        format: u32,
        rule_suites: Vec<RuleReplaySuite>,
        quest_suites: Vec<QuestReplaySuite>,
    }

    #[derive(Debug, Deserialize)]
    struct RuleReplaySuite {
        section: String,
        seed: String,
        fixtures: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    struct QuestReplaySuite {
        quest: usize,
        page: u16,
        seed: String,
        notes: Vec<String>,
        fixtures: Vec<String>,
    }

    fn parse_seed(seed: &str) -> u64 {
        u64::from_str_radix(
            seed.strip_prefix("0x").expect("seed must use 0x notation"),
            16,
        )
        .expect("seed must be a valid u64")
    }

    fn assert_fixture_exists(corpus: &str, fixture: &str) {
        assert!(
            corpus.contains(&format!("fn {fixture}(")),
            "replay index references missing deterministic fixture {fixture}"
        );
    }

    #[test]
    fn every_rules_engine_section_names_an_exact_local_scan_range() {
        assert!(
            ORIGINAL_US_RULE_SECTIONS
                .into_iter()
                .all(|section| section.source().is_complete())
        );
    }

    #[test]
    fn every_printed_gameplay_card_and_armory_item_has_machine_readable_provenance() {
        let sources = original_us_treasure_cards()
            .into_iter()
            .map(|card| card.source())
            .chain(ORIGINAL_US_HERO_SPELLS.map(|card| card.source()))
            .chain(ORIGINAL_US_CHAOS_SPELLS.map(|card| card.source()))
            .chain(ORIGINAL_US_ARTIFACTS.map(|card| card.source()))
            .chain(ORIGINAL_US_MONSTER_CARDS.into_iter().map(|card| {
                card.card_source()
                    .expect("the eight public Monsters have cards")
            }))
            .chain(ORIGINAL_US_ARMORY.map(|listing| listing.source()))
            .collect::<Vec<_>>();
        assert_eq!(sources.len(), 24 + 12 + 12 + 10 + 8 + 12);
        assert!(sources.into_iter().all(OriginalUsSource::is_complete));
    }

    #[test]
    fn seeded_replay_index_covers_every_rule_section_quest_and_printed_plot_point() {
        let index: ReplayIndex = serde_json::from_str(include_str!(
            "../tests/oracles/original-us-replay-index.json"
        ))
        .unwrap();
        assert_eq!(index.format, 1);
        let test_corpus = concat!(
            include_str!("campaign.rs"),
            include_str!("cards.rs"),
            include_str!("dice.rs"),
            include_str!("equipment.rs"),
            include_str!("game.rs"),
            include_str!("input.rs"),
            include_str!("quest.rs"),
            include_str!("renderer.rs"),
            include_str!("startup.rs"),
        );

        let expected_sections = ORIGINAL_US_RULE_SECTIONS
            .into_iter()
            .map(|section| section.id())
            .collect::<HashSet<_>>();
        let actual_sections = index
            .rule_suites
            .iter()
            .map(|suite| suite.section.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(actual_sections, expected_sections);
        assert_eq!(actual_sections.len(), index.rule_suites.len());
        for suite in &index.rule_suites {
            let seed = parse_seed(&suite.seed);
            assert_ne!(seed, 0);
            Game::demo(seed).expect("every rule replay seed must construct a deterministic game");
            assert!(!suite.fixtures.is_empty());
            for fixture in &suite.fixtures {
                assert_fixture_exists(test_corpus, fixture);
            }
        }

        assert_eq!(
            index.quest_suites.len(),
            QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS
        );
        for (quest_index, suite) in index.quest_suites.iter().enumerate() {
            assert_eq!(suite.quest, quest_index + 1);
            let seed = parse_seed(&suite.seed);
            assert_ne!(seed, 0);
            assert!(!suite.notes.is_empty());
            assert!(suite.notes.iter().all(|note| !note.trim().is_empty()));
            assert!(!suite.fixtures.is_empty());
            let quest = QuestDefinition::original_us_game_system(quest_index).unwrap();
            assert_eq!(quest.source.as_ref().unwrap().page, suite.page);
            Game::from_quest(quest, seed)
                .expect("every quest replay seed must construct its exact authored chapter");
            for fixture in &suite.fixtures {
                assert_fixture_exists(test_corpus, fixture);
            }
        }
    }
}
