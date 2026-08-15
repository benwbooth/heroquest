use rand::seq::SliceRandom;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use crate::source::OriginalUsSource;

/// The distinct faces in the original-US 24-card Treasure deck.
///
/// Quantities are supplied by [`original_us_treasure_deck`]. Keeping repeated
/// cards as repeated physical entries makes seeded shuffles and replays match a
/// real deck instead of treating cards as weighted abstract outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreasureCard {
    Gem35,
    GoldCoins15,
    Jewels25,
    Jewels50,
    ArrowHazard,
    PitHazard,
    WanderingMonster,
    HeroicBrew,
    PotionOfDefense,
    PotionOfHealing,
    PotionOfStrength,
}

impl TreasureCard {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Gem35 => "Gem",
            Self::GoldCoins15 => "Gold Coins",
            Self::Jewels25 | Self::Jewels50 => "Jewels",
            Self::ArrowHazard | Self::PitHazard => "Hazard",
            Self::WanderingMonster => "Wandering Monster",
            Self::HeroicBrew => "Heroic Brew",
            Self::PotionOfDefense => "Potion of Defense",
            Self::PotionOfHealing => "Potion of Healing",
            Self::PotionOfStrength => "Potion of Strength",
        }
    }

    pub const fn gold(self) -> Option<u16> {
        match self {
            Self::Gem35 => Some(35),
            Self::GoldCoins15 => Some(15),
            Self::Jewels25 => Some(25),
            Self::Jewels50 => Some(50),
            _ => None,
        }
    }

    /// Hazards and Wandering Monsters return to the deck. All valuables stay
    /// out until the next quest.
    pub const fn returns_to_deck(self) -> bool {
        matches!(
            self,
            Self::ArrowHazard | Self::PitHazard | Self::WanderingMonster
        )
    }

    pub const fn source(self) -> OriginalUsSource {
        match self {
            Self::Gem35 => OriginalUsSource::card(2, "Gem - 35 gold coins"),
            Self::GoldCoins15 => OriginalUsSource::card(2, "Gold - 15 gold coins"),
            Self::Jewels25 => OriginalUsSource::card(4, "Jewels - 25 gold coins"),
            Self::Jewels50 => OriginalUsSource::card(4, "Jewels - 50 gold coins"),
            Self::ArrowHazard => OriginalUsSource::card(2, "Hazard - arrow"),
            Self::PitHazard => OriginalUsSource::card(2, "Hazard - pit"),
            Self::WanderingMonster => OriginalUsSource::card(6, "Wandering Monster"),
            Self::HeroicBrew => OriginalUsSource::card(2, "Heroic Brew"),
            Self::PotionOfDefense => OriginalUsSource::card(4, "Potion of Defense"),
            Self::PotionOfHealing => OriginalUsSource::card(4, "Potion of Healing"),
            Self::PotionOfStrength => OriginalUsSource::card(4, "Potion of Strength"),
        }
    }
}

pub fn original_us_treasure_cards() -> Vec<TreasureCard> {
    use TreasureCard::*;
    let mut cards = Vec::with_capacity(24);
    cards.extend([Gem35; 2]);
    cards.extend([GoldCoins15; 2]);
    cards.extend([Jewels25; 2]);
    cards.extend([Jewels50; 2]);
    cards.extend([ArrowHazard; 2]);
    cards.extend([PitHazard; 2]);
    cards.extend([WanderingMonster; 6]);
    cards.push(HeroicBrew);
    cards.push(PotionOfDefense);
    cards.extend([PotionOfHealing; 3]);
    cards.push(PotionOfStrength);
    debug_assert_eq!(cards.len(), 24);
    cards
}

#[derive(Debug, Clone)]
pub struct TreasureDeck {
    cards: Vec<TreasureCard>,
    shuffle_before_draw: bool,
}

impl TreasureDeck {
    pub fn original_us(rng: &mut ChaCha8Rng) -> Self {
        let mut cards = original_us_treasure_cards();
        cards.shuffle(rng);
        Self {
            cards,
            shuffle_before_draw: true,
        }
    }

    /// Constructs a replay deck. The first element is the next card drawn.
    pub fn from_draw_order(cards: impl IntoIterator<Item = TreasureCard>) -> Self {
        let mut cards: Vec<_> = cards.into_iter().collect();
        cards.reverse();
        Self {
            cards,
            shuffle_before_draw: false,
        }
    }

    pub fn remaining(&self) -> usize {
        self.cards.len()
    }

    pub fn draw(&mut self, rng: &mut ChaCha8Rng) -> Option<TreasureCard> {
        // The US rules explicitly shuffle before every draw. Replay decks are
        // the deliberate exception, used to reproduce a recorded game.
        if self.shuffle_before_draw {
            self.cards.shuffle(rng);
        }
        let card = self.cards.pop()?;
        if card.returns_to_deck() {
            self.cards.insert(0, card);
        }
        Some(card)
    }

    /// Replay/test draw that preserves a caller-provided order while retaining
    /// the real return/remove behavior of the card.
    pub fn draw_in_order(&mut self) -> Option<TreasureCard> {
        let card = self.cards.pop()?;
        if card.returns_to_deck() {
            self.cards.insert(0, card);
        }
        Some(card)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeroSpell {
    Genie,
    SwiftWind,
    Tempest,
    HealBody,
    PassThroughRock,
    RockSkin,
    BallOfFlame,
    Courage,
    FireOfWrath,
    Sleep,
    WaterOfHealing,
    VeilOfMist,
}

impl HeroSpell {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Genie => "Genie",
            Self::SwiftWind => "Swift Wind",
            Self::Tempest => "Tempest",
            Self::HealBody => "Heal Body",
            Self::PassThroughRock => "Pass Through Rock",
            Self::RockSkin => "Rock Skin",
            Self::BallOfFlame => "Ball of Flame",
            Self::Courage => "Courage",
            Self::FireOfWrath => "Fire of Wrath",
            Self::Sleep => "Sleep",
            Self::WaterOfHealing => "Water of Healing",
            Self::VeilOfMist => "Veil of Mist",
        }
    }

    pub const fn rules_summary(self) -> &'static str {
        match self {
            Self::Genie => "Open any door, or attack any Monster anywhere with 5 combat dice.",
            Self::SwiftWind => {
                "A chosen Hero rolls twice the usual red movement dice on the next move."
            }
            Self::Tempest => "A chosen Monster misses its next turn.",
            Self::HealBody | Self::WaterOfHealing => {
                "Restore up to 4 lost Body Points to the caster or a chosen visible Hero."
            }
            Self::PassThroughRock => {
                "A chosen Hero may pass through walls on the next move, but must not finish in solid rock."
            }
            Self::RockSkin => {
                "A chosen Hero rolls 1 extra defend die until that Hero suffers damage."
            }
            Self::BallOfFlame => {
                "Inflict 2 Body Points; each 5 or 6 on two red dice cancels 1 point."
            }
            Self::Courage => {
                "A chosen Hero rolls 2 extra combat dice on the next attack while a Monster remains visible."
            }
            Self::FireOfWrath => {
                "Inflict 1 Body Point unless the target Monster rolls a 5 or 6 on one red die."
            }
            Self::Sleep => {
                "Put a living Monster to sleep; each Mind die showing 6 breaks the spell."
            }
            Self::VeilOfMist => {
                "A chosen Hero may pass through Monster-occupied squares on the next move."
            }
        }
    }

    pub const fn source(self) -> OriginalUsSource {
        match self {
            Self::Genie | Self::SwiftWind | Self::Tempest => OriginalUsSource::card(6, self.name()),
            Self::HealBody
            | Self::PassThroughRock
            | Self::RockSkin
            | Self::BallOfFlame
            | Self::Courage
            | Self::FireOfWrath
            | Self::Sleep
            | Self::WaterOfHealing
            | Self::VeilOfMist => OriginalUsSource::card(8, self.name()),
        }
    }
}

pub const ORIGINAL_US_HERO_SPELLS: [HeroSpell; 12] = [
    HeroSpell::Genie,
    HeroSpell::SwiftWind,
    HeroSpell::Tempest,
    HeroSpell::HealBody,
    HeroSpell::PassThroughRock,
    HeroSpell::RockSkin,
    HeroSpell::BallOfFlame,
    HeroSpell::Courage,
    HeroSpell::FireOfWrath,
    HeroSpell::Sleep,
    HeroSpell::WaterOfHealing,
    HeroSpell::VeilOfMist,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChaosSpell {
    BallOfFlame,
    CloudOfChaos,
    Command,
    Escape,
    Fear,
    Firestorm,
    LightningBolt,
    Rust,
    Sleep,
    SummonOrcs,
    SummonUndead,
    Tempest,
}

impl ChaosSpell {
    pub const fn name(self) -> &'static str {
        match self {
            Self::BallOfFlame => "Ball of Flame",
            Self::CloudOfChaos => "Cloud of Chaos",
            Self::Command => "Command",
            Self::Escape => "Escape",
            Self::Fear => "Fear",
            Self::Firestorm => "Firestorm",
            Self::LightningBolt => "Lightning Bolt",
            Self::Rust => "Rust",
            Self::Sleep => "Sleep",
            Self::SummonOrcs => "Summon Orcs",
            Self::SummonUndead => "Summon Undead",
            Self::Tempest => "Tempest",
        }
    }

    pub const fn rules_summary(self) -> &'static str {
        match self {
            Self::BallOfFlame => {
                "Deal 2 Body Points; each 5 or 6 on two red dice cancels one point."
            }
            Self::CloudOfChaos => {
                "Paralyze every Hero in the caster's area until that Hero rolls a 6 with Mind dice."
            }
            Self::Command => {
                "Zargon controls a Hero until that Hero breaks free by rolling a 6 with Mind dice."
            }
            Self::Escape => "Move the caster to the secret destination printed on the Quest Map.",
            Self::Fear => {
                "Reduce a Hero to one attack die until a future Mind-die roll contains a 6."
            }
            Self::Firestorm => {
                "Deal 3 Body Points to every other figure in the room; two red dice can reduce damage."
            }
            Self::LightningBolt => {
                "Deal 2 undefended Body Points to every figure in one straight line until blocked."
            }
            Self::Rust => "Permanently destroy one eligible non-artifact metal sword or Helmet.",
            Self::Sleep => {
                "Put a Hero to sleep until a Mind-die roll contains a 6; sleeping Heroes cannot act or defend."
            }
            Self::SummonOrcs => "Roll one red die and place the card's group of four to six Orcs.",
            Self::SummonUndead => {
                "Roll one red die and place the matching Skeleton, Zombie, and Mummy group."
            }
            Self::Tempest => "The target Hero misses the next turn.",
        }
    }

    pub const fn source(self) -> OriginalUsSource {
        match self {
            Self::BallOfFlame
            | Self::CloudOfChaos
            | Self::Command
            | Self::Escape
            | Self::Fear
            | Self::Firestorm
            | Self::LightningBolt
            | Self::Rust
            | Self::Sleep => OriginalUsSource::card(10, self.name()),
            Self::SummonOrcs | Self::SummonUndead | Self::Tempest => {
                OriginalUsSource::card(12, self.name())
            }
        }
    }
}

pub const ORIGINAL_US_CHAOS_SPELLS: [ChaosSpell; 12] = [
    ChaosSpell::BallOfFlame,
    ChaosSpell::CloudOfChaos,
    ChaosSpell::Command,
    ChaosSpell::Escape,
    ChaosSpell::Fear,
    ChaosSpell::Firestorm,
    ChaosSpell::LightningBolt,
    ChaosSpell::Rust,
    ChaosSpell::Sleep,
    ChaosSpell::SummonOrcs,
    ChaosSpell::SummonUndead,
    ChaosSpell::Tempest,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Artifact {
    ElixirOfLife,
    BorinsArmor,
    OrcsBane,
    RingOfReturn,
    SpellRing,
    SpiritBlade,
    TalismanOfLore,
    WandOfMagic,
    WizardsCloak,
    WizardsStaff,
}

impl Artifact {
    pub const fn name(self) -> &'static str {
        match self {
            Self::ElixirOfLife => "Elixir of Life",
            Self::BorinsArmor => "Borin's Armor",
            Self::OrcsBane => "Orc's Bane",
            Self::RingOfReturn => "Ring of Return",
            Self::SpellRing => "Spell Ring",
            Self::SpiritBlade => "Spirit Blade",
            Self::TalismanOfLore => "Talisman of Lore",
            Self::WandOfMagic => "Wand of Magic",
            Self::WizardsCloak => "Wizard's Cloak",
            Self::WizardsStaff => "Wizard's Staff",
        }
    }

    pub const fn rules_summary(self) -> &'static str {
        match self {
            Self::ElixirOfLife => {
                "Bring one dead Hero back to life with full Body and Mind Points. Use once."
            }
            Self::BorinsArmor => {
                "Roll four base defense dice without Plate Mail slowdown. May not be used by Wizard."
            }
            Self::OrcsBane => {
                "Attack with two combat dice; attack twice against an Orc. May not be used by Wizard."
            }
            Self::RingOfReturn => {
                "Return every Hero the wearer can see to the Quest starting point. Use once."
            }
            Self::SpellRing => {
                "At Quest start, an Elf or Wizard stores one spell in the ring and may cast it twice."
            }
            Self::SpiritBlade => {
                "Attack with three combat dice, or four against undead. May not be used by Wizard."
            }
            Self::TalismanOfLore => "Increase Mind Points by one while worn.",
            Self::WandOfMagic => {
                "An Elf or Wizard may cast two separate and different spells on one turn."
            }
            Self::WizardsCloak => "Wizard only: roll one extra combat die in defense.",
            Self::WizardsStaff => "Wizard only: attack with two combat dice and strike diagonally.",
        }
    }

    pub const fn source(self) -> OriginalUsSource {
        match self {
            Self::ElixirOfLife
            | Self::BorinsArmor
            | Self::OrcsBane
            | Self::RingOfReturn
            | Self::SpellRing
            | Self::SpiritBlade => OriginalUsSource::card(12, self.name()),
            Self::TalismanOfLore | Self::WandOfMagic | Self::WizardsCloak | Self::WizardsStaff => {
                OriginalUsSource::card(14, self.name())
            }
        }
    }
}

pub const ORIGINAL_US_ARTIFACTS: [Artifact; 10] = [
    Artifact::ElixirOfLife,
    Artifact::BorinsArmor,
    Artifact::OrcsBane,
    Artifact::RingOfReturn,
    Artifact::SpellRing,
    Artifact::SpiritBlade,
    Artifact::TalismanOfLore,
    Artifact::WandOfMagic,
    Artifact::WizardsCloak,
    Artifact::WizardsStaff,
];

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn original_us_treasure_deck_has_exact_physical_quantities() {
        let cards = original_us_treasure_cards();
        let counts = cards.into_iter().fold(HashMap::new(), |mut counts, card| {
            *counts.entry(card).or_insert(0) += 1;
            counts
        });
        assert_eq!(counts[&TreasureCard::WanderingMonster], 6);
        assert_eq!(counts[&TreasureCard::PotionOfHealing], 3);
        assert_eq!(counts[&TreasureCard::ArrowHazard], 2);
        assert_eq!(counts[&TreasureCard::PitHazard], 2);
        assert_eq!(counts.values().sum::<usize>(), 24);
    }

    #[test]
    fn bad_cards_return_but_valuables_remain_out_for_the_quest() {
        let mut deck =
            TreasureDeck::from_draw_order([TreasureCard::ArrowHazard, TreasureCard::Gem35]);
        assert_eq!(deck.draw_in_order(), Some(TreasureCard::ArrowHazard));
        assert_eq!(deck.remaining(), 2);
        assert_eq!(deck.draw_in_order(), Some(TreasureCard::Gem35));
        assert_eq!(deck.remaining(), 1);
    }

    #[test]
    fn spell_and_artifact_catalogs_match_the_scanned_card_counts() {
        assert_eq!(ORIGINAL_US_HERO_SPELLS.len(), 12);
        assert_eq!(ORIGINAL_US_CHAOS_SPELLS.len(), 12);
        assert_eq!(ORIGINAL_US_ARTIFACTS.len(), 10);
        assert!(
            original_us_treasure_cards()
                .into_iter()
                .all(|card| card.source().is_complete())
        );
        assert!(
            ORIGINAL_US_HERO_SPELLS
                .into_iter()
                .all(|card| card.source().is_complete())
        );
        assert!(
            ORIGINAL_US_CHAOS_SPELLS
                .into_iter()
                .all(|card| card.source().is_complete())
        );
        assert!(
            ORIGINAL_US_ARTIFACTS
                .into_iter()
                .all(|card| card.source().is_complete())
        );
    }

    #[test]
    fn every_original_us_artifact_has_its_printed_identity_and_rule_text() {
        assert_eq!(
            ORIGINAL_US_ARTIFACTS.map(Artifact::name),
            [
                "Elixir of Life",
                "Borin's Armor",
                "Orc's Bane",
                "Ring of Return",
                "Spell Ring",
                "Spirit Blade",
                "Talisman of Lore",
                "Wand of Magic",
                "Wizard's Cloak",
                "Wizard's Staff",
            ]
        );
        assert!(
            ORIGINAL_US_ARTIFACTS
                .iter()
                .all(|artifact| !artifact.rules_summary().is_empty())
        );
    }
}
