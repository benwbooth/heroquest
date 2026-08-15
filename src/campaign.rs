//! Persistent original-US character sheets and between-quest Armory rules.
//!
//! The 1989 North-American rulebook says that a successful Hero restores
//! starting Body/Mind, circles the completed quest number, keeps treasure for
//! the next quest, and may then visit the Armory. A dead Hero may return in the
//! next quest only as a new character, without the former character's gear.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    cards::Artifact,
    equipment::{Armor, ArmoryItem, Weapon, listing},
    game::{Game, GamePhase, Inventory},
    model::{FigureKind, HeroKind},
    quest::QuestDefinition,
    startup::HERO_ORDER,
};

pub const CAMPAIGN_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CampaignHero {
    pub hero: HeroKind,
    pub name: String,
    pub inventory: Inventory,
    pub completed_quests: Vec<u8>,
    pub champion: bool,
}

impl CampaignHero {
    fn new(hero: HeroKind) -> Self {
        Self {
            hero,
            name: hero.name().to_owned(),
            inventory: Inventory::for_hero(hero),
            completed_quests: Vec::new(),
            champion: false,
        }
    }

    fn reset_as_new_character(&mut self) {
        *self = Self::new(self.hero);
    }
}

impl Default for CampaignHero {
    fn default() -> Self {
        Self::new(HeroKind::Barbarian)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Campaign {
    pub version: u32,
    pub completed_quests: Vec<u8>,
    pub heroes: [CampaignHero; 4],
    /// Artifact cards that a monster took from a fallen Hero. They stay here
    /// until a later quest explicitly requires and the party recovers them.
    pub lost_artifacts: Vec<Artifact>,
}

impl Default for Campaign {
    fn default() -> Self {
        Self {
            version: CAMPAIGN_FORMAT_VERSION,
            completed_quests: Vec::new(),
            heroes: HERO_ORDER.map(CampaignHero::new),
            lost_artifacts: Vec::new(),
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ArmoryError {
    #[error("that Hero is not on this campaign sheet")]
    UnknownHero,
    #[error("the Wizard may not use {0}")]
    WizardRestriction(&'static str),
    #[error("{0} is already recorded on this character sheet")]
    AlreadyOwned(&'static str),
    #[error("{0} is not recorded on this character sheet")]
    NotOwned(&'static str),
    #[error("{item} costs {cost} gold, but this Hero has only {available}")]
    InsufficientGold {
        item: &'static str,
        cost: u16,
        available: u16,
    },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CharacterSheetError {
    #[error("that Hero is not on this campaign sheet")]
    UnknownHero,
    #[error("a Hero name must contain 1 to 24 visible characters")]
    InvalidName,
}

fn valid_hero_name(name: &str) -> bool {
    let name = name.trim();
    let length = name.chars().count();
    (1..=24).contains(&length) && name.chars().all(|character| !character.is_control())
}

impl Campaign {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == CAMPAIGN_FORMAT_VERSION,
            "unsupported campaign format version {}",
            self.version
        );
        for (index, expected) in HERO_ORDER.into_iter().enumerate() {
            let sheet = &self.heroes[index];
            ensure!(
                sheet.hero == expected,
                "campaign Hero slot {} must contain {}",
                index + 1,
                expected.name()
            );
            ensure!(
                valid_hero_name(&sheet.name),
                "{} has an invalid Hero name",
                expected.name()
            );
            ensure!(
                sheet
                    .inventory
                    .equipped_weapon
                    .is_none_or(|weapon| sheet.inventory.weapons.contains(&weapon)),
                "{} has equipped a weapon that is not owned",
                sheet.name
            );
            ensure!(
                sheet
                    .inventory
                    .equipped_body_armor
                    .is_none_or(|armor| sheet.inventory.armor.contains(&armor)),
                "{} has equipped body armor that is not owned",
                sheet.name
            );
            ensure!(
                sheet.completed_quests.iter().all(|&quest| (1
                    ..=QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS as u8)
                    .contains(&quest)),
                "{} contains an invalid completed quest number",
                sheet.name
            );
        }
        ensure!(
            self.completed_quests.iter().all(|&quest| (1
                ..=QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS as u8)
                .contains(&quest)),
            "campaign contains an invalid quest number"
        );
        ensure!(
            self.lost_artifacts
                .iter()
                .enumerate()
                .all(|(index, artifact)| !self.lost_artifacts[..index].contains(artifact)),
            "campaign contains the same lost artifact more than once"
        );
        ensure!(
            self.lost_artifacts.iter().all(|artifact| self
                .heroes
                .iter()
                .all(|sheet| !sheet.inventory.artifacts.contains(artifact))),
            "a lost artifact cannot also be recorded on a Hero sheet"
        );
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let bytes = fs::read(path)
            .with_context(|| format!("failed to read campaign {}", path.display()))?;
        let campaign: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse campaign {}", path.display()))?;
        campaign.validate()?;
        Ok(campaign)
    }

    pub fn load_or_new(path: &Path) -> Result<Self> {
        if path.exists() {
            Self::load(path)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create campaign directory {}", parent.display())
            })?;
        }
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(".new");
        let temporary = PathBuf::from(temporary);
        let bytes = serde_json::to_vec_pretty(self)?;
        fs::write(&temporary, bytes).with_context(|| {
            format!(
                "failed to write campaign temporary file {}",
                temporary.display()
            )
        })?;
        fs::rename(&temporary, path).with_context(|| {
            format!(
                "failed to replace campaign {} with {}",
                path.display(),
                temporary.display()
            )
        })?;
        Ok(())
    }

    /// The first not-yet-completed chapter. A complete campaign stays on the
    /// final chapter so it can still be inspected or replayed.
    pub fn next_quest_index(&self) -> usize {
        (0..QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS)
            .find(|index| !self.completed_quests.contains(&(*index as u8 + 1)))
            .unwrap_or(QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS - 1)
    }

    pub fn quest_is_selectable(&self, index: usize) -> bool {
        index < QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS
            && (index == self.next_quest_index()
                || self.completed_quests.contains(&(index as u8 + 1)))
    }

    pub fn apply_names_to_setup(&self, startup: &mut crate::startup::StartupFlow) {
        for setup in &mut startup.heroes {
            if let Some(sheet) = self.heroes.iter().find(|sheet| sheet.hero == setup.hero) {
                setup.hero_name.clone_from(&sheet.name);
            }
        }
        startup.quest_ceiling = self.next_quest_index();
        if !self.quest_is_selectable(startup.selected_quest) {
            startup.selected_quest = self.next_quest_index();
        }
    }

    /// Write the one free-form field on the physical character sheet. All
    /// numerical values and possessions remain owned by the rules engine.
    pub fn rename_hero(
        &mut self,
        hero: HeroKind,
        name: &str,
    ) -> std::result::Result<(), CharacterSheetError> {
        let name = name.trim();
        if !valid_hero_name(name) {
            return Err(CharacterSheetError::InvalidName);
        }
        let sheet = self
            .heroes
            .iter_mut()
            .find(|sheet| sheet.hero == hero)
            .ok_or(CharacterSheetError::UnknownHero)?;
        sheet.name = name.to_owned();
        Ok(())
    }

    /// Restore the persistent sheet into a newly constructed quest while
    /// retaining any artifact explicitly injected by that quest definition.
    pub fn apply_to_game(&self, game: &mut Game) {
        let mut required_lost_artifacts = Vec::new();
        for unit in &mut game.units {
            let FigureKind::Hero(kind) = unit.figure else {
                continue;
            };
            let Some(sheet) = self.heroes.iter().find(|sheet| sheet.hero == kind) else {
                continue;
            };
            let quest_artifacts = unit.inventory.artifacts.clone();
            unit.name.clone_from(&sheet.name);
            unit.inventory = sheet.inventory.clone();
            unit.inventory.fools_gold = 0;
            for artifact in quest_artifacts {
                if self.lost_artifacts.contains(&artifact) {
                    if !required_lost_artifacts.contains(&artifact) {
                        required_lost_artifacts.push(artifact);
                    }
                } else if !unit.inventory.artifacts.contains(&artifact) {
                    unit.inventory.artifacts.push(artifact);
                }
            }
            if unit
                .inventory
                .equipped_weapon
                .is_some_and(|weapon| !unit.inventory.weapons.contains(&weapon))
            {
                unit.inventory.equipped_weapon = unit.inventory.weapons.first().copied();
            }
            unit.champion = sheet.champion;
        }
        game.lost_artifact_treasure = required_lost_artifacts.into();
    }

    /// Commit a successful quest. Surviving sheets retain possessions and get
    /// the circled quest number; a dead Hero becomes a fresh character for the
    /// next chapter, exactly as the rulebook directs.
    pub fn record_success(&mut self, quest_index: usize, game: &Game) -> Result<()> {
        ensure!(
            quest_index < QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS,
            "quest index is outside the original-US campaign"
        );
        ensure!(
            game.phase == GamePhase::Won,
            "campaign progress can only be recorded after the quest engine reports victory"
        );
        let quest_number = quest_index as u8 + 1;
        let recovered_artifacts = game
            .units
            .iter()
            .filter(|unit| unit.alive && matches!(unit.figure, FigureKind::Hero(_)))
            .flat_map(|unit| unit.inventory.artifacts.iter().copied())
            .collect::<Vec<_>>();
        self.lost_artifacts
            .retain(|artifact| !recovered_artifacts.contains(artifact));
        for &artifact in &game.monster_stolen_artifacts {
            if !recovered_artifacts.contains(&artifact) && !self.lost_artifacts.contains(&artifact)
            {
                self.lost_artifacts.push(artifact);
            }
        }
        for sheet in &mut self.heroes {
            let unit = game
                .units
                .iter()
                .find(|unit| unit.figure == FigureKind::Hero(sheet.hero));
            if let Some(unit) = unit.filter(|unit| unit.alive) {
                sheet.name.clone_from(&unit.name);
                sheet.inventory = unit.inventory.clone();
                sheet.inventory.fools_gold = 0;
                sheet.champion = unit.champion;
                if !sheet.completed_quests.contains(&quest_number) {
                    sheet.completed_quests.push(quest_number);
                    sheet.completed_quests.sort_unstable();
                }
            } else {
                sheet.reset_as_new_character();
            }
        }
        if !self.completed_quests.contains(&quest_number) {
            self.completed_quests.push(quest_number);
            self.completed_quests.sort_unstable();
        }
        Ok(())
    }

    pub fn purchase(&mut self, hero: HeroKind, item: ArmoryItem) -> Result<(), ArmoryError> {
        let sheet = self
            .heroes
            .iter_mut()
            .find(|sheet| sheet.hero == hero)
            .ok_or(ArmoryError::UnknownHero)?;
        let item_name = armory_item_name(item);
        match item {
            ArmoryItem::Weapon(weapon) if !weapon.allowed_by(hero) => {
                return Err(ArmoryError::WizardRestriction(item_name));
            }
            ArmoryItem::Armor(armor) if !armor.allowed_by(hero) => {
                return Err(ArmoryError::WizardRestriction(item_name));
            }
            ArmoryItem::Armor(armor) if sheet.inventory.armor.contains(&armor) => {
                return Err(ArmoryError::AlreadyOwned(item_name));
            }
            _ => {}
        }
        let cost = listing(item).gold;
        if sheet.inventory.gold < cost {
            return Err(ArmoryError::InsufficientGold {
                item: item_name,
                cost,
                available: sheet.inventory.gold,
            });
        }
        sheet.inventory.gold -= cost;
        match item {
            ArmoryItem::ToolKit => {
                sheet.inventory.tool_kits = sheet.inventory.tool_kits.saturating_add(1)
            }
            ArmoryItem::Weapon(weapon) => {
                sheet.inventory.weapons.push(weapon);
                sheet.inventory.equipped_weapon = Some(weapon);
            }
            ArmoryItem::Armor(armor) => {
                sheet.inventory.armor.push(armor);
                if matches!(armor, Armor::ChainMail | Armor::PlateMail) {
                    sheet.inventory.equipped_body_armor = Some(armor);
                }
            }
        }
        Ok(())
    }

    pub fn equip_body_armor(&mut self, hero: HeroKind, armor: Armor) -> Result<(), ArmoryError> {
        let sheet = self
            .heroes
            .iter_mut()
            .find(|sheet| sheet.hero == hero)
            .ok_or(ArmoryError::UnknownHero)?;
        let name = armory_item_name(ArmoryItem::Armor(armor));
        if !matches!(armor, Armor::ChainMail | Armor::PlateMail)
            || !sheet.inventory.armor.contains(&armor)
        {
            return Err(ArmoryError::NotOwned(name));
        }
        sheet.inventory.equipped_body_armor = Some(armor);
        Ok(())
    }

    pub fn equip_weapon(&mut self, hero: HeroKind, weapon: Weapon) -> Result<(), ArmoryError> {
        let sheet = self
            .heroes
            .iter_mut()
            .find(|sheet| sheet.hero == hero)
            .ok_or(ArmoryError::UnknownHero)?;
        if !weapon.allowed_by(hero) {
            return Err(ArmoryError::WizardRestriction(weapon.name()));
        }
        if !sheet.inventory.weapons.contains(&weapon) {
            return Err(ArmoryError::NotOwned(weapon.name()));
        }
        sheet.inventory.equipped_weapon = Some(weapon);
        Ok(())
    }
}

pub const fn armory_item_name(item: ArmoryItem) -> &'static str {
    match item {
        ArmoryItem::ToolKit => "Tool Kit",
        ArmoryItem::Weapon(weapon) => weapon.name(),
        ArmoryItem::Armor(Armor::Helmet) => "Helmet",
        ArmoryItem::Armor(Armor::Shield) => "Shield",
        ArmoryItem::Armor(Armor::ChainMail) => "Chain Mail",
        ArmoryItem::Armor(Armor::PlateMail) => "Plate Mail",
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::{game::GamePhase, quest::QuestDefinition};

    #[test]
    fn a_fresh_campaign_unlocks_only_the_trial() {
        let campaign = Campaign::default();
        assert_eq!(campaign.next_quest_index(), 0);
        assert!(campaign.quest_is_selectable(0));
        assert!(!campaign.quest_is_selectable(1));
    }

    #[test]
    fn campaign_names_follow_hero_identity_after_clockwise_reordering() {
        let mut campaign = Campaign::default();
        campaign.heroes[0].name = "Conan".to_owned();
        campaign.heroes[3].name = "Merlin".to_owned();
        let mut startup = crate::startup::StartupFlow::default();
        startup.active_hero = 3;
        startup.move_active_hero_earlier();
        startup.move_active_hero_earlier();
        startup.move_active_hero_earlier();

        campaign.apply_names_to_setup(&mut startup);

        assert_eq!(startup.heroes[0].hero, HeroKind::Wizard);
        assert_eq!(startup.heroes[0].hero_name, "Merlin");
        assert_eq!(startup.heroes[1].hero, HeroKind::Barbarian);
        assert_eq!(startup.heroes[1].hero_name, "Conan");
    }

    #[test]
    fn character_sheet_name_edit_is_trimmed_validated_and_persistent() {
        let mut campaign = Campaign::default();
        campaign.rename_hero(HeroKind::Elf, "  Lindir  ").unwrap();
        assert_eq!(campaign.heroes[2].name, "Lindir");

        assert_eq!(
            campaign.rename_hero(HeroKind::Elf, "   "),
            Err(CharacterSheetError::InvalidName)
        );
        assert_eq!(
            campaign.rename_hero(HeroKind::Elf, &"x".repeat(25)),
            Err(CharacterSheetError::InvalidName)
        );
        assert_eq!(campaign.heroes[2].name, "Lindir");
        campaign.validate().unwrap();
    }

    #[test]
    fn serialized_campaign_rejects_an_invalid_character_sheet_name() {
        let mut campaign = Campaign::default();
        campaign.heroes[0].name.clear();
        assert!(
            campaign
                .validate()
                .unwrap_err()
                .to_string()
                .contains("invalid Hero name")
        );
    }

    #[test]
    fn successful_survivors_keep_their_sheet_and_unlock_the_next_quest() {
        let mut campaign = Campaign::default();
        let mut game = Game::from_quest(QuestDefinition::original_us_trial().unwrap(), 4).unwrap();
        let barbarian = game.hero_order[0];
        let hero = game
            .units
            .iter_mut()
            .find(|unit| unit.id == barbarian)
            .unwrap();
        hero.name = "Conan".to_owned();
        hero.inventory.gold = 275;
        hero.inventory.heroic_brew = 1;
        game.phase = GamePhase::Won;

        campaign.record_success(0, &game).unwrap();
        assert_eq!(campaign.next_quest_index(), 1);
        assert_eq!(campaign.heroes[0].name, "Conan");
        assert_eq!(campaign.heroes[0].inventory.gold, 275);
        assert_eq!(campaign.heroes[0].completed_quests, [1]);
        assert!(campaign.quest_is_selectable(0));
        assert!(campaign.quest_is_selectable(1));
        assert!(!campaign.quest_is_selectable(2));
    }

    #[test]
    fn campaign_refuses_to_record_an_unfinished_or_failed_quest() {
        let original = Campaign::default();
        for phase in [
            GamePhase::HeroTurn { order_index: 0 },
            GamePhase::Retreated,
            GamePhase::Lost,
        ] {
            let mut campaign = original.clone();
            let mut game =
                Game::from_quest(QuestDefinition::original_us_trial().unwrap(), 0x4f52_4143)
                    .unwrap();
            game.phase = phase;
            let error = campaign.record_success(0, &game).unwrap_err();
            assert!(error.to_string().contains("engine reports victory"));
            assert_eq!(campaign, original);
        }
    }

    #[test]
    fn a_dead_hero_returns_only_as_a_fresh_character() {
        let mut campaign = Campaign::default();
        campaign.heroes[1].name = "Grimbeard".to_owned();
        campaign.heroes[1].inventory.gold = 900;
        let mut game = Game::from_quest(QuestDefinition::original_us_trial().unwrap(), 5).unwrap();
        let dwarf = game.hero_order[1];
        let unit = game.units.iter_mut().find(|unit| unit.id == dwarf).unwrap();
        unit.alive = false;
        unit.inventory = Inventory::default();
        game.phase = GamePhase::Won;

        campaign.record_success(0, &game).unwrap();
        let dwarf = &campaign.heroes[1];
        assert_eq!(dwarf.name, "Dwarf");
        assert_eq!(dwarf.inventory.gold, 0);
        assert_eq!(dwarf.inventory.weapons, [Weapon::Shortsword]);
        assert!(dwarf.completed_quests.is_empty());
    }

    #[test]
    fn campaign_inventory_is_applied_without_losing_quest_required_artifacts() {
        let mut campaign = Campaign::default();
        campaign.heroes[0].name = "Conan".to_owned();
        campaign.heroes[0].inventory.gold = 123;
        let mut game = Game::from_quest(
            QuestDefinition::original_us_return_to_barak_tor().unwrap(),
            6,
        )
        .unwrap();
        campaign.apply_to_game(&mut game);
        let hero = game
            .units
            .iter()
            .find(|unit| unit.figure == FigureKind::Hero(HeroKind::Barbarian))
            .unwrap();
        assert_eq!(hero.name, "Conan");
        assert_eq!(hero.inventory.gold, 123);
        assert!(
            hero.inventory
                .artifacts
                .contains(&crate::cards::Artifact::SpiritBlade)
        );
    }

    #[test]
    fn monster_stolen_required_artifact_becomes_special_treasure_in_later_quest() {
        let mut campaign = Campaign::default();
        let mut prior_quest = Game::from_quest(
            QuestDefinition::original_us_quest_for_the_spirit_blade().unwrap(),
            61,
        )
        .unwrap();
        prior_quest.phase = GamePhase::Won;
        prior_quest
            .monster_stolen_artifacts
            .push(Artifact::SpiritBlade);
        campaign.record_success(12, &prior_quest).unwrap();
        assert_eq!(campaign.lost_artifacts, [Artifact::SpiritBlade]);

        let mut final_quest = Game::from_quest(
            QuestDefinition::original_us_return_to_barak_tor().unwrap(),
            62,
        )
        .unwrap();
        campaign.apply_to_game(&mut final_quest);
        assert!(final_quest.units.iter().all(|unit| {
            !matches!(unit.figure, FigureKind::Hero(_))
                || !unit.inventory.artifacts.contains(&Artifact::SpiritBlade)
        }));
        assert_eq!(
            final_quest
                .lost_artifact_treasure
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            [Artifact::SpiritBlade]
        );

        let finder = final_quest.hero_order[2];
        final_quest
            .units
            .iter_mut()
            .find(|unit| unit.id == finder)
            .unwrap()
            .inventory
            .artifacts
            .push(Artifact::SpiritBlade);
        final_quest.lost_artifact_treasure.clear();
        final_quest.phase = GamePhase::Won;
        campaign.record_success(13, &final_quest).unwrap();
        assert!(campaign.lost_artifacts.is_empty());
        assert!(
            campaign.heroes[2]
                .inventory
                .artifacts
                .contains(&Artifact::SpiritBlade)
        );
    }

    #[test]
    fn armory_deducts_exact_prices_and_enforces_wizard_and_body_armor_rules() {
        let mut campaign = Campaign::default();
        campaign.heroes[0].inventory.gold = 1_000;
        campaign.heroes[1].inventory.gold = 2_000;
        campaign.heroes[3].inventory.gold = 1_000;
        campaign
            .purchase(HeroKind::Barbarian, ArmoryItem::Weapon(Weapon::BattleAxe))
            .unwrap();
        assert_eq!(campaign.heroes[0].inventory.gold, 550);
        assert!(
            campaign.heroes[0]
                .inventory
                .weapons
                .contains(&Weapon::BattleAxe)
        );
        assert_eq!(
            campaign.heroes[0].inventory.equipped_weapon,
            Some(Weapon::BattleAxe)
        );
        campaign
            .equip_weapon(HeroKind::Barbarian, Weapon::Broadsword)
            .unwrap();
        assert_eq!(
            campaign.heroes[0].inventory.equipped_weapon,
            Some(Weapon::Broadsword)
        );
        assert_eq!(
            campaign.purchase(HeroKind::Wizard, ArmoryItem::Armor(Armor::Helmet)),
            Err(ArmoryError::WizardRestriction("Helmet"))
        );
        campaign
            .purchase(HeroKind::Dwarf, ArmoryItem::Armor(Armor::ChainMail))
            .unwrap();
        campaign
            .purchase(HeroKind::Dwarf, ArmoryItem::Armor(Armor::PlateMail))
            .unwrap();
        assert_eq!(
            campaign.heroes[1].inventory.equipped_body_armor,
            Some(Armor::PlateMail)
        );
        campaign
            .equip_body_armor(HeroKind::Dwarf, Armor::ChainMail)
            .unwrap();
        assert_eq!(
            campaign.heroes[1].inventory.equipped_body_armor,
            Some(Armor::ChainMail)
        );
    }

    #[test]
    fn campaign_save_round_trips_atomically() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("heroquest-campaign-{stamp}.json"));
        let mut campaign = Campaign::default();
        campaign.heroes[2].name = "Lindir".to_owned();
        campaign.heroes[2].inventory.gold = 72;
        campaign.completed_quests = vec![1, 2];
        campaign.save(&path).unwrap();
        assert_eq!(Campaign::load(&path).unwrap(), campaign);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn owned_chain_and_plate_mail_can_be_switched_between_quests() {
        let mut campaign = Campaign::default();
        campaign.heroes[1].inventory.gold = 2_000;
        campaign
            .purchase(HeroKind::Dwarf, ArmoryItem::Armor(Armor::ChainMail))
            .unwrap();
        campaign
            .purchase(HeroKind::Dwarf, ArmoryItem::Armor(Armor::PlateMail))
            .unwrap();
        let mut game = Game::from_quest(QuestDefinition::original_us_trial().unwrap(), 71).unwrap();
        campaign.apply_to_game(&mut game);
        game.phase = GamePhase::HeroTurn { order_index: 1 };
        assert_eq!(game.active_movement_dice_count(), 1);
        assert_eq!(game.active_hero().unwrap().effective_defense_dice(), 4);

        campaign
            .equip_body_armor(HeroKind::Dwarf, Armor::ChainMail)
            .unwrap();
        campaign.apply_to_game(&mut game);
        assert_eq!(game.active_movement_dice_count(), 2);
        assert_eq!(game.active_hero().unwrap().effective_defense_dice(), 3);
    }
}
