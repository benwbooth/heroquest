use crate::model::HeroKind;
use crate::source::OriginalUsSource;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weapon {
    Dagger,
    Staff,
    Crossbow,
    Shortsword,
    Broadsword,
    Longsword,
    BattleAxe,
}

impl Weapon {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Dagger => "Dagger",
            Self::Staff => "Staff",
            Self::Crossbow => "Crossbow",
            Self::Shortsword => "Shortsword",
            Self::Broadsword => "Broadsword",
            Self::Longsword => "Longsword",
            Self::BattleAxe => "Battle Axe",
        }
    }

    pub const fn attack_dice(self) -> u8 {
        match self {
            Self::Dagger | Self::Staff => 1,
            Self::Shortsword => 2,
            Self::Crossbow | Self::Broadsword | Self::Longsword => 3,
            Self::BattleAxe => 4,
        }
    }

    pub const fn permits_diagonal_attack(self) -> bool {
        matches!(self, Self::Staff | Self::Longsword)
    }

    pub const fn permits_ranged_attack(self) -> bool {
        matches!(self, Self::Dagger | Self::Crossbow)
    }

    pub const fn consumed_when_thrown(self) -> bool {
        matches!(self, Self::Dagger)
    }

    pub const fn permits_shield(self) -> bool {
        !matches!(self, Self::Staff | Self::BattleAxe)
    }

    pub fn allowed_by(self, hero: HeroKind) -> bool {
        hero != HeroKind::Wizard || matches!(self, Self::Dagger | Self::Staff)
    }

    pub const fn source(self) -> OriginalUsSource {
        OriginalUsSource::new("Identification Guide and Armory.pdf", 1, 1, self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Armor {
    Helmet,
    Shield,
    ChainMail,
    PlateMail,
}

impl Armor {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Helmet => "Helmet",
            Self::Shield => "Shield",
            Self::ChainMail => "Chain Mail",
            Self::PlateMail => "Plate Mail",
        }
    }

    pub const fn defense_bonus(self) -> u8 {
        match self {
            Self::Helmet | Self::Shield | Self::ChainMail => 1,
            Self::PlateMail => 2,
        }
    }

    pub fn allowed_by(self, hero: HeroKind) -> bool {
        hero != HeroKind::Wizard
    }

    pub const fn source(self) -> OriginalUsSource {
        OriginalUsSource::new("Identification Guide and Armory.pdf", 1, 1, self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArmoryItem {
    ToolKit,
    Weapon(Weapon),
    Armor(Armor),
}

impl ArmoryItem {
    pub const fn source(self) -> OriginalUsSource {
        match self {
            Self::ToolKit => {
                OriginalUsSource::new("Identification Guide and Armory.pdf", 1, 1, "Tool Kit")
            }
            Self::Weapon(weapon) => weapon.source(),
            Self::Armor(armor) => armor.source(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArmoryListing {
    pub item: ArmoryItem,
    pub gold: u16,
}

impl ArmoryListing {
    pub const fn source(self) -> OriginalUsSource {
        self.item.source()
    }
}

pub const ORIGINAL_US_ARMORY: [ArmoryListing; 12] = [
    ArmoryListing {
        item: ArmoryItem::ToolKit,
        gold: 250,
    },
    ArmoryListing {
        item: ArmoryItem::Weapon(Weapon::Dagger),
        gold: 25,
    },
    ArmoryListing {
        item: ArmoryItem::Weapon(Weapon::Staff),
        gold: 100,
    },
    ArmoryListing {
        item: ArmoryItem::Weapon(Weapon::Crossbow),
        gold: 350,
    },
    ArmoryListing {
        item: ArmoryItem::Weapon(Weapon::Shortsword),
        gold: 150,
    },
    ArmoryListing {
        item: ArmoryItem::Weapon(Weapon::Broadsword),
        gold: 250,
    },
    ArmoryListing {
        item: ArmoryItem::Weapon(Weapon::Longsword),
        gold: 350,
    },
    ArmoryListing {
        item: ArmoryItem::Weapon(Weapon::BattleAxe),
        gold: 450,
    },
    ArmoryListing {
        item: ArmoryItem::Armor(Armor::Helmet),
        gold: 125,
    },
    ArmoryListing {
        item: ArmoryItem::Armor(Armor::Shield),
        gold: 150,
    },
    ArmoryListing {
        item: ArmoryItem::Armor(Armor::ChainMail),
        gold: 500,
    },
    ArmoryListing {
        item: ArmoryItem::Armor(Armor::PlateMail),
        gold: 850,
    },
];

pub const fn starting_weapon(hero: HeroKind) -> Weapon {
    match hero {
        HeroKind::Barbarian => Weapon::Broadsword,
        HeroKind::Dwarf | HeroKind::Elf => Weapon::Shortsword,
        HeroKind::Wizard => Weapon::Dagger,
    }
}

pub fn listing(item: ArmoryItem) -> &'static ArmoryListing {
    ORIGINAL_US_ARMORY
        .iter()
        .find(|listing| listing.item == item)
        .expect("every original-US Armory item has a listing")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_match_the_original_us_armory_scan() {
        assert_eq!(ORIGINAL_US_ARMORY.len(), 12);
        assert!(
            ORIGINAL_US_ARMORY
                .into_iter()
                .all(|listing| listing.source().is_complete())
        );
        assert_eq!(listing(ArmoryItem::ToolKit).gold, 250);
        assert_eq!(listing(ArmoryItem::Weapon(Weapon::BattleAxe)).gold, 450);
        assert_eq!(listing(ArmoryItem::Armor(Armor::PlateMail)).gold, 850);
    }

    #[test]
    fn weapon_properties_cover_diagonal_ranged_and_shield_rules() {
        assert!(Weapon::Staff.permits_diagonal_attack());
        assert!(Weapon::Longsword.permits_diagonal_attack());
        assert!(Weapon::Crossbow.permits_ranged_attack());
        assert!(Weapon::Dagger.consumed_when_thrown());
        assert!(!Weapon::BattleAxe.permits_shield());
        assert!(!Weapon::Staff.permits_shield());
    }

    #[test]
    fn wizard_restrictions_match_the_character_and_armory_cards() {
        assert!(Weapon::Dagger.allowed_by(HeroKind::Wizard));
        assert!(Weapon::Staff.allowed_by(HeroKind::Wizard));
        assert!(!Weapon::Shortsword.allowed_by(HeroKind::Wizard));
        assert!(!Armor::Helmet.allowed_by(HeroKind::Wizard));
    }
}
