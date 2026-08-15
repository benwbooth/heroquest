use crate::{cards::HeroSpell, model::HeroKind};

pub const QUEST_TITLES: [&str; 14] = [
    "The Trial",
    "The Rescue of Sir Ragnar",
    "Lair of the Orc Warlord",
    "Prince Magnus' Gold",
    "Melar's Maze",
    "Legacy of the Orc Warlord",
    "The Lost Wizard",
    "The Fire Mage",
    "Race Against Time",
    "Castle of Mystery",
    "Bastion of Chaos",
    "Barak Tor - Barrow of the Witch Lord",
    "Quest for the Spirit Blade",
    "Return to Barak Tor",
];

const QUEST_BLURBS: [&str; 14] = [
    "Seek out and destroy Verag, the foul Gargoyle hidden in Fellmarg's catacombs.",
    "Find Sir Ragnar in Ulag's prison and bring the wounded knight back to the stairway.",
    "Hunt Ulag, the Orc Warlord, in his underground fortress.",
    "Recover the Emperor's stolen gold from Gulthor and his Orc band.",
    "Enter Melar's laboratory and recover the Talisman of Lore.",
    "Escape captivity, recover your equipment, and take revenge on Grak.",
    "Discover what became of Wardoz, the Emperor's missing wizard.",
    "Defeat Balur, the Fire Mage, deep beneath Black Fire Crag.",
    "Escape the traitor's maze and return alive to the stairway.",
    "Survive Ollar's magical doors and find the legendary gold mine.",
    "Break through the fortress and confront the servants of Chaos.",
    "Find the Star of the West in the Witch Lord's ancient barrow.",
    "Recover the Spirit Blade, the only weapon that can harm the Witch Lord.",
    "Return to Barak Tor and destroy the Witch Lord before his army rises.",
];

pub const HERO_ORDER: [HeroKind; 4] = [
    HeroKind::Barbarian,
    HeroKind::Dwarf,
    HeroKind::Elf,
    HeroKind::Wizard,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupStage {
    Box,
    Armory,
    QuestSelection,
    PlayerSetup,
    WizardSpellChoice,
    ElfSpellChoice,
    Ready,
    Playing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellGroup {
    Air,
    Fire,
    Water,
    Earth,
}

impl SpellGroup {
    pub const ALL: [Self; 4] = [Self::Air, Self::Fire, Self::Water, Self::Earth];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Air => "Air",
            Self::Fire => "Fire",
            Self::Water => "Water",
            Self::Earth => "Earth",
        }
    }

    pub const fn spells(self) -> [HeroSpell; 3] {
        match self {
            Self::Air => [HeroSpell::Genie, HeroSpell::SwiftWind, HeroSpell::Tempest],
            Self::Fire => [
                HeroSpell::BallOfFlame,
                HeroSpell::Courage,
                HeroSpell::FireOfWrath,
            ],
            Self::Water => [
                HeroSpell::Sleep,
                HeroSpell::WaterOfHealing,
                HeroSpell::VeilOfMist,
            ],
            Self::Earth => [
                HeroSpell::HealBody,
                HeroSpell::PassThroughRock,
                HeroSpell::RockSkin,
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeroSetup {
    pub hero: HeroKind,
    pub player_number: u8,
    pub hero_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupHotspot {
    OpenBox,
    Back,
    Confirm,
    PreviousQuest,
    NextQuest,
    Hero(usize),
    HeroOwner(usize),
    MoveHeroEarlier,
    MoveHeroLater,
    RemovePlayer,
    AddPlayer,
    Spell(SpellGroup),
    ArmoryHero(usize),
    ArmoryItem(usize),
    ArmoryPurchase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupFlow {
    pub stage: StartupStage,
    pub selected_quest: usize,
    /// Campaign chapters from The Trial through this zero-based index are
    /// available. A new sheet therefore starts at zero, not thirteen.
    pub quest_ceiling: usize,
    pub player_count: u8,
    pub heroes: [HeroSetup; 4],
    pub active_hero: usize,
    pub wizard_first: SpellGroup,
    pub elf_spells: SpellGroup,
    pub armory_hero: usize,
    pub armory_item: usize,
    pub armory_message: String,
    pub armory_revision: u32,
    pub hovered: Option<StartupHotspot>,
}

impl Default for StartupFlow {
    fn default() -> Self {
        Self {
            stage: StartupStage::Box,
            selected_quest: 0,
            quest_ceiling: 0,
            player_count: 1,
            heroes: std::array::from_fn(|index| HeroSetup {
                hero: HERO_ORDER[index],
                player_number: 1,
                hero_name: HERO_ORDER[index].name().to_owned(),
            }),
            active_hero: 0,
            wizard_first: SpellGroup::Fire,
            elf_spells: SpellGroup::Earth,
            armory_hero: 0,
            armory_item: 0,
            armory_message: "Choose a Hero and an item from the original-US Armory.".to_owned(),
            armory_revision: 0,
            hovered: None,
        }
    }
}

impl StartupFlow {
    pub fn advance(&mut self) {
        self.hovered = None;
        self.stage = match self.stage {
            StartupStage::Box => StartupStage::QuestSelection,
            StartupStage::Armory => StartupStage::QuestSelection,
            StartupStage::QuestSelection => StartupStage::PlayerSetup,
            StartupStage::PlayerSetup => StartupStage::WizardSpellChoice,
            StartupStage::WizardSpellChoice => StartupStage::ElfSpellChoice,
            StartupStage::ElfSpellChoice => StartupStage::Ready,
            StartupStage::Ready => StartupStage::Playing,
            StartupStage::Playing => StartupStage::Playing,
        };
    }

    pub fn back(&mut self) {
        self.hovered = None;
        self.stage = match self.stage {
            StartupStage::Box => StartupStage::Box,
            StartupStage::Armory => StartupStage::Box,
            StartupStage::QuestSelection => StartupStage::Box,
            StartupStage::PlayerSetup => StartupStage::QuestSelection,
            StartupStage::WizardSpellChoice => StartupStage::PlayerSetup,
            StartupStage::ElfSpellChoice => StartupStage::WizardSpellChoice,
            StartupStage::Ready => StartupStage::ElfSpellChoice,
            StartupStage::Playing => StartupStage::Playing,
        };
    }

    pub fn next(&mut self) {
        match self.stage {
            StartupStage::QuestSelection => {
                self.selected_quest = (self.selected_quest + 1) % (self.quest_ceiling + 1);
            }
            StartupStage::Armory => {
                self.armory_item = (self.armory_item + 1) % 12;
            }
            StartupStage::PlayerSetup => {
                self.active_hero = (self.active_hero + 1) % self.heroes.len();
            }
            StartupStage::WizardSpellChoice => {
                let index = SpellGroup::ALL
                    .iter()
                    .position(|&spell| spell == self.wizard_first)
                    .unwrap_or_default();
                self.wizard_first = SpellGroup::ALL[(index + 1) % SpellGroup::ALL.len()];
                if self.elf_spells == self.wizard_first {
                    self.elf_spells = self.remaining_spell_groups()[0];
                }
            }
            StartupStage::ElfSpellChoice => {
                let available = self.remaining_spell_groups();
                let index = available
                    .iter()
                    .position(|&spell| spell == self.elf_spells)
                    .unwrap_or_default();
                self.elf_spells = available[(index + 1) % available.len()];
            }
            _ => {}
        }
    }

    pub fn previous(&mut self) {
        match self.stage {
            StartupStage::QuestSelection => {
                self.selected_quest = self
                    .selected_quest
                    .checked_sub(1)
                    .unwrap_or(self.quest_ceiling);
            }
            StartupStage::Armory => {
                self.armory_item = self.armory_item.checked_sub(1).unwrap_or(11);
            }
            StartupStage::PlayerSetup => {
                self.active_hero = self
                    .active_hero
                    .checked_sub(1)
                    .unwrap_or(self.heroes.len() - 1);
            }
            StartupStage::WizardSpellChoice | StartupStage::ElfSpellChoice => {
                for _ in 0..3 {
                    self.next();
                }
            }
            _ => {}
        }
    }

    pub fn add_player(&mut self) {
        self.player_count = (self.player_count + 1).min(4);
        self.assign_default_players();
    }

    pub fn remove_player(&mut self) {
        self.player_count = self.player_count.saturating_sub(1).max(1);
        self.assign_default_players();
    }

    pub fn cycle_active_hero_player(&mut self) {
        let hero = &mut self.heroes[self.active_hero];
        hero.player_number = hero.player_number % self.player_count + 1;
    }

    pub fn move_active_hero_earlier(&mut self) {
        let destination = self
            .active_hero
            .checked_sub(1)
            .unwrap_or(self.heroes.len() - 1);
        self.heroes.swap(self.active_hero, destination);
        self.active_hero = destination;
    }

    pub fn move_active_hero_later(&mut self) {
        let destination = (self.active_hero + 1) % self.heroes.len();
        self.heroes.swap(self.active_hero, destination);
        self.active_hero = destination;
    }

    pub fn update_pointer(&mut self, x: f32, y: f32, width: u32, height: u32) {
        self.hovered = self.hotspot_at(x, y, width, height);
    }

    pub fn click_pointer(
        &mut self,
        x: f32,
        y: f32,
        width: u32,
        height: u32,
    ) -> Option<StartupHotspot> {
        let Some(hotspot) = self.hotspot_at(x, y, width, height) else {
            return None;
        };
        match hotspot {
            StartupHotspot::OpenBox | StartupHotspot::Confirm => self.advance(),
            StartupHotspot::Back => self.back(),
            StartupHotspot::PreviousQuest => self.previous(),
            StartupHotspot::NextQuest => self.next(),
            StartupHotspot::Hero(index) => self.active_hero = index,
            StartupHotspot::HeroOwner(index) => {
                self.active_hero = index;
                self.cycle_active_hero_player();
            }
            StartupHotspot::MoveHeroEarlier => self.move_active_hero_earlier(),
            StartupHotspot::MoveHeroLater => self.move_active_hero_later(),
            StartupHotspot::RemovePlayer => self.remove_player(),
            StartupHotspot::AddPlayer => self.add_player(),
            StartupHotspot::Spell(group) => {
                if self.stage == StartupStage::WizardSpellChoice {
                    self.wizard_first = group;
                    if self.elf_spells == group {
                        self.elf_spells = self.remaining_spell_groups()[0];
                    }
                } else if self.stage == StartupStage::ElfSpellChoice && group != self.wizard_first {
                    self.elf_spells = group;
                }
            }
            StartupHotspot::ArmoryHero(index) => self.armory_hero = index,
            StartupHotspot::ArmoryItem(index) => self.armory_item = index,
            StartupHotspot::ArmoryPurchase => {}
        }
        self.hovered = self.hotspot_at(x, y, width, height);
        Some(hotspot)
    }

    pub fn hotspot_at(
        &self,
        window_x: f32,
        window_y: f32,
        width: u32,
        height: u32,
    ) -> Option<StartupHotspot> {
        if self.stage == StartupStage::Box {
            return Some(StartupHotspot::OpenBox);
        }
        let (x, y) = canvas_pointer(window_x, window_y, width, height)?;
        let inside = |left: f32, top: f32, right: f32, bottom: f32| {
            x >= left && x <= right && y >= top && y <= bottom
        };
        if self.stage != StartupStage::Armory && inside(90.0, 1810.0, 520.0, 1990.0) {
            return Some(StartupHotspot::Back);
        }
        match self.stage {
            StartupStage::Armory => {
                for index in 0..4 {
                    let left = 955.0 + index as f32 * 250.0;
                    if inside(left, 150.0, left + 220.0, 300.0) {
                        return Some(StartupHotspot::ArmoryHero(index));
                    }
                }
                for index in 0..12 {
                    let column = index % 2;
                    let row = index / 2;
                    let left = 970.0 + column as f32 * 500.0;
                    let top = 410.0 + row as f32 * 180.0;
                    if inside(left, top, left + 455.0, top + 145.0) {
                        return Some(StartupHotspot::ArmoryItem(index));
                    }
                }
                if inside(1010.0, 1740.0, 1430.0, 1990.0) {
                    return Some(StartupHotspot::ArmoryPurchase);
                }
                inside(1490.0, 1740.0, 1950.0, 1990.0).then_some(StartupHotspot::Confirm)
            }
            StartupStage::QuestSelection => {
                if inside(660.0, 1780.0, 910.0, 1990.0) {
                    return Some(StartupHotspot::PreviousQuest);
                }
                if inside(980.0, 1780.0, 1230.0, 1990.0) {
                    return Some(StartupHotspot::NextQuest);
                }
                inside(1400.0, 1780.0, 1950.0, 1990.0).then_some(StartupHotspot::Confirm)
            }
            StartupStage::PlayerSetup => {
                if inside(1510.0, 170.0, 1660.0, 300.0) {
                    return Some(StartupHotspot::RemovePlayer);
                }
                if inside(1720.0, 170.0, 1870.0, 300.0) {
                    return Some(StartupHotspot::AddPlayer);
                }
                for index in 0..self.heroes.len() {
                    let top = 490.0 + index as f32 * 185.0;
                    let bottom = top + 150.0;
                    if inside(1640.0, top, 1945.0, bottom) {
                        return Some(StartupHotspot::HeroOwner(index));
                    }
                    if inside(955.0, top, 1640.0, bottom) {
                        return Some(StartupHotspot::Hero(index));
                    }
                }
                if inside(980.0, 1580.0, 1235.0, 1725.0) {
                    return Some(StartupHotspot::MoveHeroEarlier);
                }
                if inside(1270.0, 1580.0, 1525.0, 1725.0) {
                    return Some(StartupHotspot::MoveHeroLater);
                }
                inside(1450.0, 1780.0, 1950.0, 1990.0).then_some(StartupHotspot::Confirm)
            }
            StartupStage::WizardSpellChoice | StartupStage::ElfSpellChoice => {
                for (index, group) in SpellGroup::ALL.into_iter().enumerate() {
                    let left = 160.0 + (index % 2) as f32 * 920.0;
                    let top = 430.0 + (index / 2) as f32 * 560.0;
                    if inside(left, top, left + 800.0, top + 440.0)
                        && (self.stage == StartupStage::WizardSpellChoice
                            || group != self.wizard_first)
                    {
                        return Some(StartupHotspot::Spell(group));
                    }
                }
                inside(1450.0, 1780.0, 1950.0, 1990.0).then_some(StartupHotspot::Confirm)
            }
            StartupStage::Ready => {
                inside(1320.0, 1740.0, 1950.0, 1990.0).then_some(StartupHotspot::Confirm)
            }
            StartupStage::Box | StartupStage::Playing => None,
        }
    }

    pub fn quest_title(&self) -> &'static str {
        QUEST_TITLES[self.selected_quest]
    }

    pub fn quest_blurb(&self) -> &'static str {
        QUEST_BLURBS[self.selected_quest]
    }

    pub const fn quest_page_number(&self) -> usize {
        self.selected_quest + 3
    }

    pub fn remaining_spell_groups(&self) -> Vec<SpellGroup> {
        SpellGroup::ALL
            .into_iter()
            .filter(|&group| group != self.wizard_first)
            .collect()
    }

    pub fn wizard_spells(&self) -> Vec<SpellGroup> {
        SpellGroup::ALL
            .into_iter()
            .filter(|&group| group != self.elf_spells)
            .collect()
    }

    fn assign_default_players(&mut self) {
        for (index, hero) in self.heroes.iter_mut().enumerate() {
            hero.player_number = index as u8 % self.player_count + 1;
        }
    }
}

fn canvas_pointer(x: f32, y: f32, width: u32, height: u32) -> Option<(f32, f32)> {
    let side = width.min(height) as f32;
    if side <= 0.0 {
        return None;
    }
    let left = (width as f32 - side) * 0.5;
    let top = (height as f32 - side) * 0.5;
    if x < left || y < top || x > left + side || y > top + side {
        return None;
    }
    Some(((x - left) * 2048.0 / side, (y - top) * 2048.0 / side))
}

#[cfg(test)]
mod tests {
    use super::{SpellGroup, StartupFlow, StartupHotspot, StartupStage};
    use crate::model::HeroKind;

    #[test]
    fn original_campaign_starts_with_the_trial() {
        let startup = StartupFlow::default();
        assert_eq!(startup.selected_quest, 0);
        assert_eq!(startup.quest_title(), "The Trial");
    }

    #[test]
    fn quest_selection_cycles_through_implemented_quests() {
        let mut startup = StartupFlow::default();
        startup.stage = StartupStage::QuestSelection;
        startup.quest_ceiling = crate::quest::QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS - 1;
        startup.next();
        assert_eq!(startup.selected_quest, 1);
        startup.previous();
        assert_eq!(startup.selected_quest, 0);
        startup.previous();
        assert_eq!(
            startup.selected_quest,
            crate::quest::QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS - 1
        );
    }

    #[test]
    fn all_four_heroes_are_assigned_even_for_one_player() {
        let startup = StartupFlow::default();
        assert!(startup.heroes.iter().all(|hero| hero.player_number == 1));
    }

    #[test]
    fn wizard_gets_three_groups_and_elf_gets_the_other_one() {
        let mut startup = StartupFlow::default();
        startup.wizard_first = SpellGroup::Water;
        startup.elf_spells = SpellGroup::Air;
        let wizard = startup.wizard_spells();
        assert_eq!(wizard.len(), 3);
        assert!(!wizard.contains(&SpellGroup::Air));
        assert_eq!(startup.elf_spells, SpellGroup::Air);
    }

    #[test]
    fn startup_cannot_skip_directly_from_box_to_play() {
        let mut startup = StartupFlow::default();
        startup.advance();
        assert_eq!(startup.stage, StartupStage::QuestSelection);
    }

    #[test]
    fn a_fresh_campaign_reaches_the_board_through_clickable_in_game_setup_controls() {
        let mut startup = StartupFlow::default();
        assert_eq!(
            startup.click_pointer(1024.0, 1024.0, 2048, 2048),
            Some(StartupHotspot::OpenBox)
        );
        for expected in [
            StartupStage::PlayerSetup,
            StartupStage::WizardSpellChoice,
            StartupStage::ElfSpellChoice,
            StartupStage::Ready,
            StartupStage::Playing,
        ] {
            assert_eq!(
                startup.click_pointer(1700.0, 1860.0, 2048, 2048),
                Some(StartupHotspot::Confirm)
            );
            assert_eq!(startup.stage, expected);
        }
        assert_eq!(startup.selected_quest, 0);
        assert_eq!(startup.wizard_spells().len(), 3);
        assert_eq!(startup.elf_spells, SpellGroup::Earth);

        startup.stage = StartupStage::Armory;
        assert_eq!(
            startup.click_pointer(1700.0, 1860.0, 2048, 2048),
            Some(StartupHotspot::Confirm)
        );
        assert_eq!(startup.stage, StartupStage::QuestSelection);
    }

    #[test]
    fn pointer_mapping_accounts_for_widescreen_pillarboxes() {
        let mut startup = StartupFlow::default();
        startup.stage = StartupStage::QuestSelection;
        assert_eq!(
            startup.hotspot_at(1000.0, 820.0, 1440, 900),
            Some(StartupHotspot::Confirm)
        );
        assert_eq!(startup.hotspot_at(20.0, 820.0, 1440, 900), None);
    }

    #[test]
    fn owner_button_changes_the_clicked_hero() {
        let mut startup = StartupFlow::default();
        startup.stage = StartupStage::PlayerSetup;
        startup.add_player();
        startup.click_pointer(1050.0, 220.0, 1440, 900);
        assert_eq!(startup.active_hero, 0);
        assert_eq!(startup.heroes[0].player_number, 2);
    }

    #[test]
    fn player_setup_reorders_clockwise_seats_without_detaching_identity() {
        let mut startup = StartupFlow::default();
        startup.stage = StartupStage::PlayerSetup;
        startup.heroes[0].hero_name = "Conan".to_owned();
        startup.heroes[0].player_number = 3;
        startup.active_hero = 0;

        startup.move_active_hero_later();
        assert_eq!(startup.active_hero, 1);
        assert_eq!(
            startup.heroes.each_ref().map(|setup| setup.hero),
            [
                HeroKind::Dwarf,
                HeroKind::Barbarian,
                HeroKind::Elf,
                HeroKind::Wizard,
            ]
        );
        assert_eq!(startup.heroes[1].hero_name, "Conan");
        assert_eq!(startup.heroes[1].player_number, 3);

        assert_eq!(
            startup.click_pointer(1100.0, 1650.0, 2048, 2048),
            Some(StartupHotspot::MoveHeroEarlier)
        );
        assert_eq!(startup.active_hero, 0);
        assert_eq!(startup.heroes[0].hero, HeroKind::Barbarian);
    }

    #[test]
    fn a_new_campaign_cannot_scroll_past_the_trial() {
        let mut startup = StartupFlow::default();
        startup.stage = StartupStage::QuestSelection;
        startup.next();
        assert_eq!(startup.selected_quest, 0);
        startup.previous();
        assert_eq!(startup.selected_quest, 0);
    }

    #[test]
    fn armory_pointer_selects_physical_rows_and_purchase_button() {
        let mut startup = StartupFlow::default();
        startup.stage = StartupStage::Armory;
        assert_eq!(
            startup.click_pointer(1065.0, 200.0, 2048, 2048),
            Some(StartupHotspot::ArmoryHero(0))
        );
        assert_eq!(
            startup.click_pointer(1660.0, 680.0, 2048, 2048),
            Some(StartupHotspot::ArmoryItem(3))
        );
        assert_eq!(
            startup.click_pointer(1200.0, 1850.0, 2048, 2048),
            Some(StartupHotspot::ArmoryPurchase)
        );
    }
}
