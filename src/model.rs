use serde::{Deserialize, Serialize};

use crate::source::OriginalUsSource;

pub const BOARD_WIDTH: u8 = 26;
pub const BOARD_HEIGHT: u8 = 19;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Pos {
    pub x: u8,
    pub y: u8,
}

impl Pos {
    pub const fn new(x: u8, y: u8) -> Self {
        Self { x, y }
    }

    pub fn offset(self, direction: Direction) -> Option<Self> {
        let (dx, dy) = direction.delta();
        let x = self.x as i16 + dx;
        let y = self.y as i16 + dy;
        (x >= 0 && y >= 0 && x < BOARD_WIDTH as i16 && y < BOARD_HEIGHT as i16)
            .then_some(Self::new(x as u8, y as u8))
    }

    pub fn is_adjacent(self, other: Self) -> bool {
        self.x.abs_diff(other.x) + self.y.abs_diff(other.y) == 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    North,
    East,
    South,
    West,
}

impl Direction {
    pub const ALL: [Self; 4] = [Self::North, Self::East, Self::South, Self::West];

    pub const fn delta(self) -> (i16, i16) {
        match self {
            Self::North => (0, -1),
            Self::East => (1, 0),
            Self::South => (0, 1),
            Self::West => (-1, 0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeroKind {
    Barbarian,
    Dwarf,
    Elf,
    Wizard,
}

impl HeroKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Barbarian => "Barbarian",
            Self::Dwarf => "Dwarf",
            Self::Elf => "Elf",
            Self::Wizard => "Wizard",
        }
    }

    pub const fn color(self) -> [f32; 3] {
        match self {
            Self::Barbarian => [0.72, 0.11, 0.08],
            Self::Dwarf => [0.12, 0.36, 0.76],
            Self::Elf => [0.10, 0.58, 0.28],
            Self::Wizard => [0.88, 0.70, 0.12],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonsterKind {
    Goblin,
    Orc,
    #[serde(alias = "abomination")]
    Fimir,
    Skeleton,
    Zombie,
    Mummy,
    #[serde(alias = "dread_warrior")]
    ChaosWarrior,
    Gargoyle,
    #[serde(alias = "dread_sorcerer")]
    ChaosSorcerer,
}

impl MonsterKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Goblin => "Goblin",
            Self::Orc => "Orc",
            Self::Fimir => "Fimir",
            Self::Skeleton => "Skeleton",
            Self::Zombie => "Zombie",
            Self::Mummy => "Mummy",
            Self::ChaosWarrior => "Chaos Warrior",
            Self::Gargoyle => "Gargoyle",
            Self::ChaosSorcerer => "Chaos Sorcerer",
        }
    }

    pub const fn color(self) -> [f32; 3] {
        match self {
            Self::Goblin => [0.28, 0.64, 0.18],
            Self::Orc => [0.18, 0.48, 0.13],
            Self::Fimir => [0.12, 0.55, 0.50],
            Self::Skeleton => [0.82, 0.79, 0.64],
            Self::Zombie => [0.46, 0.53, 0.36],
            Self::Mummy => [0.72, 0.65, 0.47],
            Self::ChaosWarrior => [0.16, 0.16, 0.19],
            Self::Gargoyle => [0.35, 0.35, 0.40],
            Self::ChaosSorcerer => [0.38, 0.10, 0.46],
        }
    }

    pub const fn card_source(self) -> Option<OriginalUsSource> {
        match self {
            Self::ChaosWarrior | Self::Fimir | Self::Gargoyle | Self::Goblin | Self::Mummy => {
                Some(OriginalUsSource::card(14, self.name()))
            }
            Self::Orc | Self::Skeleton | Self::Zombie => {
                Some(OriginalUsSource::card(16, self.name()))
            }
            Self::ChaosSorcerer => None,
        }
    }
}

pub const ORIGINAL_US_MONSTER_CARDS: [MonsterKind; 8] = [
    MonsterKind::ChaosWarrior,
    MonsterKind::Fimir,
    MonsterKind::Gargoyle,
    MonsterKind::Goblin,
    MonsterKind::Mummy,
    MonsterKind::Orc,
    MonsterKind::Skeleton,
    MonsterKind::Zombie,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FigureKind {
    Hero(HeroKind),
    Monster(MonsterKind),
}

impl FigureKind {
    pub const fn faction(self) -> Faction {
        match self {
            Self::Hero(_) => Faction::Hero,
            Self::Monster(_) => Faction::Monster,
        }
    }

    pub const fn color(self) -> [f32; 3] {
        match self {
            Self::Hero(kind) => kind.color(),
            Self::Monster(kind) => kind.color(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Faction {
    Hero,
    Monster,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub movement: u8,
    pub attack: u8,
    pub defend: u8,
    pub body: u8,
    pub mind: u8,
}

pub const fn hero_stats(kind: HeroKind) -> Stats {
    match kind {
        HeroKind::Barbarian => Stats {
            movement: 0,
            attack: 3,
            defend: 2,
            body: 8,
            mind: 2,
        },
        HeroKind::Dwarf => Stats {
            movement: 0,
            attack: 2,
            defend: 2,
            body: 7,
            mind: 3,
        },
        HeroKind::Elf => Stats {
            movement: 0,
            attack: 2,
            defend: 2,
            body: 6,
            mind: 4,
        },
        HeroKind::Wizard => Stats {
            movement: 0,
            attack: 1,
            defend: 2,
            body: 4,
            mind: 6,
        },
    }
}

pub const fn monster_stats(kind: MonsterKind) -> Stats {
    match kind {
        MonsterKind::Goblin => Stats {
            movement: 10,
            attack: 2,
            defend: 1,
            body: 1,
            mind: 1,
        },
        MonsterKind::Orc => Stats {
            movement: 8,
            attack: 3,
            defend: 2,
            body: 1,
            mind: 2,
        },
        MonsterKind::Fimir => Stats {
            movement: 6,
            attack: 3,
            defend: 3,
            body: 1,
            mind: 3,
        },
        MonsterKind::Skeleton => Stats {
            movement: 6,
            attack: 2,
            defend: 2,
            body: 1,
            mind: 0,
        },
        MonsterKind::Zombie => Stats {
            movement: 5,
            attack: 2,
            defend: 3,
            body: 1,
            mind: 0,
        },
        MonsterKind::Mummy => Stats {
            movement: 4,
            attack: 3,
            defend: 4,
            body: 1,
            mind: 0,
        },
        MonsterKind::ChaosWarrior => Stats {
            movement: 7,
            attack: 4,
            defend: 4,
            body: 1,
            mind: 3,
        },
        MonsterKind::Gargoyle => Stats {
            movement: 6,
            attack: 4,
            defend: 5,
            body: 3,
            mind: 4,
        },
        MonsterKind::ChaosSorcerer => Stats {
            movement: 8,
            attack: 3,
            defend: 4,
            body: 3,
            mind: 6,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropKind {
    Stairs,
    Table,
    Chest,
    Bookcase,
    Throne,
    WeaponRack,
    AlchemistsBench,
    Tomb,
    SorcerersTable,
    TortureRack,
    Fireplace,
    Cupboard,
    StarOfWest,
}

impl PropKind {
    /// Assembled component count in the original North-American Game System.
    ///
    /// `None` means the quest object has no separate physical component. The
    /// Star of the West exists in Quest 12's prose (in the Zombie's hand), but
    /// it is not printed anywhere on the four original-US tile sheets.
    pub const fn original_us_physical_count(self) -> Option<usize> {
        Some(match self {
            Self::Table | Self::Bookcase => 2,
            Self::Chest => 3,
            Self::Stairs
            | Self::Throne
            | Self::AlchemistsBench
            | Self::Tomb
            | Self::SorcerersTable
            | Self::TortureRack
            | Self::Fireplace
            | Self::WeaponRack
            | Self::Cupboard => 1,
            Self::StarOfWest => return None,
        })
    }

    pub const fn is_original_us_physical_component(self) -> bool {
        self.original_us_physical_count().is_some()
    }

    /// Number of board squares covered by the assembled component at the
    /// orientation printed in the original-US Quest Book. Quest coordinates
    /// name the upper-left occupied square; odd quarter turns swap the axes.
    pub const fn footprint(self, rotation_quarters: u8) -> (u8, u8) {
        let (width, height) = match self {
            Self::Stairs => (2, 2),
            Self::Table | Self::AlchemistsBench | Self::SorcerersTable => (3, 2),
            Self::Tomb | Self::TortureRack => (2, 3),
            Self::Bookcase | Self::WeaponRack | Self::Cupboard => (3, 1),
            Self::Fireplace => (2, 1),
            Self::Chest | Self::Throne | Self::StarOfWest => (1, 1),
        };
        if rotation_quarters % 2 == 0 {
            (width, height)
        } else {
            (height, width)
        }
    }

    pub fn footprint_squares(self, anchor: Pos, rotation_quarters: u8) -> Vec<Pos> {
        let (width, height) = self.footprint(rotation_quarters);
        (0..height)
            .flat_map(|dy| (0..width).map(move |dx| Pos::new(anchor.x + dx, anchor.y + dy)))
            .collect()
    }

    /// The stairway is a flat, walkable component. The Star is a logical quest
    /// object, not furniture. Every assembled furniture piece occupies its
    /// printed floor squares.
    pub const fn blocks_movement(self) -> bool {
        !matches!(self, Self::Stairs | Self::StarOfWest)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrapKind {
    Pit,
    FallingBlock,
    Spear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatFace {
    Skull,
    WhiteShield,
    BlackShield,
}
