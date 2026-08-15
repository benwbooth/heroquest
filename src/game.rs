use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, HashSet, VecDeque},
};

use anyhow::{Result, ensure};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use thiserror::Error;

use crate::cards::{Artifact, ChaosSpell, HeroSpell, TreasureCard, TreasureDeck};
use crate::dice::{DiceTray, DieResult};
use crate::equipment::{Armor, ArmoryItem, Weapon, listing, starting_weapon};
use crate::model::{
    BOARD_HEIGHT, BOARD_WIDTH, CombatFace, Direction, Faction, FigureKind, MonsterKind, Pos,
    PropKind, Stats, TrapKind, hero_stats, monster_stats,
};
use crate::quest::{
    MonsterModelVariant, ObjectiveDef, QuestDefinition, QuestEffectDef, QuestTriggerDef,
};

pub type UnitId = u32;

#[derive(Debug, Clone)]
pub struct Cell {
    pub passable: bool,
    pub region: i16,
    pub tint: [f32; 3],
    pub revealed: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            passable: false,
            region: -1,
            tint: [0.075, 0.055, 0.065],
            revealed: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Door {
    pub a: Pos,
    pub b: Pos,
    pub open: bool,
    pub secret: bool,
    pub discovered: bool,
    pub searchable: bool,
    pub false_door: bool,
}

impl Door {
    pub fn connects(&self, a: Pos, b: Pos) -> bool {
        (self.a == a && self.b == b) || (self.a == b && self.b == a)
    }

    pub fn touches(&self, pos: Pos) -> bool {
        self.a == pos || self.b == pos
    }

    pub fn other_side(&self, pos: Pos) -> Option<Pos> {
        if self.a == pos {
            Some(self.b)
        } else if self.b == pos {
            Some(self.a)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct Unit {
    pub id: UnitId,
    pub name: String,
    pub figure: FigureKind,
    /// The finite box miniature currently representing this logical figure.
    /// Monsters receive one only when their area is placed; the logical kind
    /// still controls stats when a same-color substitute sculpt is required.
    /// `None` on a revealed living Monster means every compatible box piece is
    /// assigned; the renderer then shows a virtual copy of the logical sculpt
    /// so the rules engine never loses an actor to presentation scarcity.
    pub physical_figure: Option<FigureKind>,
    pub faction: Faction,
    pub model_variant: Option<MonsterModelVariant>,
    pub pos: Pos,
    pub stats: Stats,
    pub body: i16,
    pub alive: bool,
    pub in_pit: bool,
    pub inventory: Inventory,
    pub carried_quest_item: Option<usize>,
    pub dormant: bool,
    pub invulnerable_until_acts: bool,
    pub equipment_available: bool,
    pub spellcasting_available: bool,
    pub escaped: bool,
    pub chaos_spells: Vec<ChaosSpell>,
    /// Chaos cards already cast this quest, in physical discard order.
    pub discarded_chaos_spells: Vec<ChaosSpell>,
    pub hero_spells: Vec<HeroSpell>,
    /// Elemental spell cards already cast this quest, in physical discard order.
    pub discarded_hero_spells: Vec<HeroSpell>,
    pub spell_ring_spell: Option<HeroSpell>,
    pub spell_ring_casts_left: u8,
    pub escape_target: Option<Pos>,
    pub immune_to_fire_spells: bool,
    pub fearful: bool,
    pub sleeping: bool,
    pub clouded: bool,
    pub hero_sleep_caster: Option<UnitId>,
    pub skip_turns: u8,
    pub petrified_turns: u8,
    pub diagonal_attack: bool,
    pub hidden_until_activated: bool,
    pub immune_except_spirit_blade: bool,
    pub champion: bool,
    pub commanded: bool,
    pub swift_wind: bool,
    pub courage: bool,
    pub rock_skin: bool,
    pub pass_through_rock: bool,
    pub veil_of_mist: bool,
    pub potion_defense_bonus: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Inventory {
    pub gold: u16,
    pub fools_gold: u16,
    pub heroic_brew: u8,
    pub potion_of_defense: u8,
    pub potion_of_healing: u8,
    pub healing_potion_strengths: Vec<u8>,
    pub potion_of_strength: u8,
    pub petrification_potion: u8,
    pub tool_kits: u8,
    pub artifacts: Vec<Artifact>,
    pub weapons: Vec<Weapon>,
    pub equipped_weapon: Option<Weapon>,
    pub armor: Vec<Armor>,
    pub equipped_body_armor: Option<Armor>,
}

impl Inventory {
    pub(crate) fn for_hero(hero: crate::model::HeroKind) -> Self {
        let weapon = starting_weapon(hero);
        Self {
            weapons: vec![weapon],
            equipped_weapon: Some(weapon),
            ..Self::default()
        }
    }

    fn movement_dice(&self) -> u8 {
        if self.body_armor() == Some(Armor::PlateMail) {
            1
        } else {
            2
        }
    }

    fn weapon_attack_dice(&self, fallback: u8) -> u8 {
        self.equipped_weapon
            .map(Weapon::attack_dice)
            .unwrap_or(fallback)
    }

    fn body_armor_bonus(&self) -> u8 {
        self.body_armor().map_or(0, Armor::defense_bonus)
    }

    fn body_armor(&self) -> Option<Armor> {
        self.equipped_body_armor
            .filter(|armor| {
                matches!(armor, Armor::ChainMail | Armor::PlateMail) && self.armor.contains(armor)
            })
            .or_else(|| {
                // Backward compatibility for old campaign sheets and quest
                // tests written before worn body armor was tracked separately.
                if self.armor.contains(&Armor::PlateMail) {
                    Some(Armor::PlateMail)
                } else if self.armor.contains(&Armor::ChainMail) {
                    Some(Armor::ChainMail)
                } else {
                    None
                }
            })
    }

    fn defense_accessory_bonus(&self) -> u8 {
        let helmet = self.armor.contains(&Armor::Helmet) as u8;
        let shield = (self.armor.contains(&Armor::Shield)
            && self.equipped_weapon.is_none_or(Weapon::permits_shield)) as u8;
        helmet + shield
    }

    fn defense_dice(&self, base: u8) -> u8 {
        base + self.body_armor_bonus() + self.defense_accessory_bonus()
    }
}

impl Unit {
    pub fn effective_mind(&self) -> u8 {
        self.stats.mind
            + u8::from(
                self.equipment_available
                    && self.inventory.artifacts.contains(&Artifact::TalismanOfLore),
            )
    }

    fn uses_borins_armor(&self) -> bool {
        self.faction == Faction::Hero
            && self.equipment_available
            && !matches!(
                self.figure,
                FigureKind::Hero(crate::model::HeroKind::Wizard)
            )
            && self.inventory.artifacts.contains(&Artifact::BorinsArmor)
    }

    fn uses_wand_of_magic(&self) -> bool {
        self.faction == Faction::Hero
            && self.equipment_available
            && self.spellcasting_available
            && matches!(
                self.figure,
                FigureKind::Hero(crate::model::HeroKind::Elf | crate::model::HeroKind::Wizard)
            )
            && self.inventory.artifacts.contains(&Artifact::WandOfMagic)
    }

    fn uses_spell_ring(&self) -> bool {
        self.faction == Faction::Hero
            && self.equipment_available
            && self.spellcasting_available
            && matches!(
                self.figure,
                FigureKind::Hero(crate::model::HeroKind::Elf | crate::model::HeroKind::Wizard)
            )
            && self.inventory.artifacts.contains(&Artifact::SpellRing)
    }

    fn effective_movement_dice(&self) -> u8 {
        if self.uses_borins_armor() {
            2
        } else if matches!(
            self.figure,
            FigureKind::Hero(crate::model::HeroKind::Wizard)
        ) {
            // The Wizard may carry recovered equipment but cannot wear Plate
            // Mail or gain its one-die movement restriction.
            2
        } else {
            self.inventory.movement_dice()
        }
    }

    pub fn effective_defense_dice(&self) -> u8 {
        if self.sleeping || self.clouded || self.petrified_turns > 0 {
            return 0;
        }
        if self.inventory.fools_gold > 0 {
            return 0;
        }
        if self.faction == Faction::Hero && !self.equipment_available {
            return 2;
        }
        (if matches!(
            self.figure,
            FigureKind::Hero(crate::model::HeroKind::Wizard)
        ) {
            // Every Armory armor card is forbidden to the Wizard. Artifact
            // defense such as the Wizard's Cloak is added separately below.
            self.stats.defend
        } else if self.uses_borins_armor() {
            self.stats.defend + 2 + self.inventory.defense_accessory_bonus()
        } else {
            self.inventory.defense_dice(self.stats.defend)
        }) + u8::from(
            matches!(
                self.figure,
                FigureKind::Hero(crate::model::HeroKind::Wizard)
            ) && self.inventory.artifacts.contains(&Artifact::WizardsCloak),
        ) + u8::from(self.rock_skin)
            + self.potion_defense_bonus
    }

    fn permits_diagonal_attack(&self) -> bool {
        self.diagonal_attack
            || (self.equipment_available
                && matches!(
                    self.figure,
                    FigureKind::Hero(crate::model::HeroKind::Wizard)
                )
                && self.inventory.artifacts.contains(&Artifact::WizardsStaff))
            || (self.equipment_available
                && self
                    .inventory
                    .equipped_weapon
                    .is_some_and(Weapon::permits_diagonal_attack))
    }

    pub fn is_immune_to_hero_spell(&self, spell: HeroSpell) -> bool {
        self.immune_except_spirit_blade
            || (self.immune_to_fire_spells
                && matches!(spell, HeroSpell::BallOfFlame | HeroSpell::FireOfWrath))
    }
}

#[derive(Debug, Clone)]
pub struct Prop {
    pub kind: PropKind,
    pub pos: Pos,
    pub rotation_quarters: u8,
    pub visible: bool,
    pub carried_by: Option<UnitId>,
}

impl Prop {
    pub fn footprint_squares(&self) -> Vec<Pos> {
        self.kind
            .footprint_squares(self.pos, self.rotation_quarters)
    }

    pub fn occupies_square(&self, pos: Pos) -> bool {
        let (width, height) = self.kind.footprint(self.rotation_quarters);
        pos.x >= self.pos.x
            && pos.y >= self.pos.y
            && pos.x < self.pos.x + width
            && pos.y < self.pos.y + height
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestItem {
    pub id: String,
    pub prop_index: usize,
    pub sealed_gold: u16,
    pub holder: Option<UnitId>,
    pub delivered: bool,
}

#[derive(Debug, Clone)]
pub struct Trap {
    pub kind: TrapKind,
    pub pos: Pos,
    pub discovered: bool,
    pub sprung: bool,
    pub disarmed: bool,
    pub trigger_on_entry: bool,
    pub disarmable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamePhase {
    HeroTurn {
        order_index: usize,
    },
    AllyTurn {
        ally: UnitId,
        controller_order_index: usize,
    },
    ZargonTurn,
    Won,
    Retreated,
    Lost,
}

#[derive(Debug, Clone, Default)]
pub struct HeroTurnState {
    pub movement_roll: Option<u8>,
    pub movement_left: u8,
    pub moved_steps: u8,
    pub action_used: bool,
    pub resistance_resolved: bool,
    pub resolved_chaos_resistances: Vec<ChaosSpell>,
    pub door_passed: bool,
    pub orcs_bane_follow_up: bool,
    pub heroic_brew_ready: bool,
    pub heroic_brew_follow_up: bool,
    pub potion_strength_bonus: u8,
    pub wand_first_spell: Option<HeroSpell>,
    pub wand_follow_up_used: bool,
}

#[derive(Debug, Clone, Copy)]
struct MovementStepInfo {
    may_enter: bool,
    may_end: bool,
    may_continue: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackSource {
    Natural,
    Unarmed,
    Weapon(Weapon),
    ThrownDagger,
    OrcsBane,
    SpiritBlade,
    WizardsStaff,
}

impl AttackSource {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Natural => "Natural attack",
            Self::Unarmed => "Unarmed",
            Self::Weapon(weapon) => weapon.name(),
            Self::ThrownDagger => "Thrown Dagger",
            Self::OrcsBane => "Orc's Bane",
            Self::SpiritBlade => "Spirit Blade",
            Self::WizardsStaff => "Wizard's Staff",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackPlan {
    pub attacker: UnitId,
    pub defender: UnitId,
    pub source: AttackSource,
    pub attack_dice: u8,
    pub defend_dice: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatVisualEvent {
    pub sequence: u64,
    pub attacker: UnitId,
    pub defender: UnitId,
    pub damage: u8,
    pub defender_died: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZargonStep {
    Moved {
        unit: UnitId,
        from: Pos,
        to: Pos,
    },
    Attack(AttackPlan),
    Cast {
        caster: UnitId,
        target: UnitId,
        spell: ChaosSpell,
        resistance_dice: u8,
    },
    HeroSpellRoll(HeroSpellRoll),
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeroSpellTarget {
    Unit(UnitId),
    Door(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeroSpellDiceKind {
    Red,
    Combat { attack_dice: u8, defend_dice: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeroSpellRoll {
    pub caster: UnitId,
    pub target: UnitId,
    pub spell: HeroSpell,
    pub dice_count: u8,
    pub kind: HeroSpellDiceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeroSpellCast {
    Resolved,
    Roll(HeroSpellRoll),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealingPotionUse {
    Restored { hero: UnitId, body: u8 },
    RollRedDie { hero: UnitId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PotionKind {
    HeroicBrew,
    Defense,
    Healing,
    Strength,
    Petrification,
}

impl PotionKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::HeroicBrew => "Heroic Brew",
            Self::Defense => "Potion of Defense",
            Self::Healing => "Potion of Healing",
            Self::Strength => "Potion of Strength",
            Self::Petrification => "Mysterious Purple Potion",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeroDeathChoice {
    HealingPotion,
    HealBody,
    WaterOfHealing,
    AcceptDeath,
}

impl HeroDeathChoice {
    pub const fn label(self) -> &'static str {
        match self {
            Self::HealingPotion => "Potion of Healing",
            Self::HealBody => "Heal Body",
            Self::WaterOfHealing => "Water of Healing",
            Self::AcceptDeath => "Accept death",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingHeroDeath {
    pub hero: UnitId,
    pub potion_roll_pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPossessionPickup {
    pub dead_hero: UnitId,
    pub eligible_heroes: Vec<UnitId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatOutcome {
    pub skulls: u8,
    pub blocks: u8,
    pub damage: u8,
    pub defender_died: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreasureDiscovery {
    Card(TreasureCard),
    Artifact(Artifact),
    Gold(u16),
    Empty,
    QuestEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreasureSearchOutcome {
    pub discovery: TreasureDiscovery,
    pub wandering_monster: Option<UnitId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum QuestTrigger {
    SearchTreasure { region: i16 },
    SearchTreasureAfterDefeat { region: i16, name: String },
    RevealRoom { region: i16 },
    DefeatNamed { name: String },
    OpenDoor { a: Pos, b: Pos },
}

#[derive(Debug, Clone)]
struct QuestEvent {
    id: String,
    trigger: QuestTrigger,
    effect: QuestEffectDef,
    message: Option<String>,
    resolved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingFallingBlock {
    pub hero: UnitId,
    pub trap: Pos,
    pub back: Option<Pos>,
    pub ahead: Option<Pos>,
}

/// A floor trap whose printed combat dice must be thrown on the visible
/// tabletop before its result can be applied.  Keeping the original entry
/// geometry in the request lets a falling block offer the exact ahead/back
/// escape squares after the dice settle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingTrapRoll {
    pub hero: UnitId,
    pub trap: Pos,
    pub from: Pos,
    pub direction: Direction,
    pub kind: TrapKind,
}

impl PendingTrapRoll {
    pub const fn dice_count(self) -> u8 {
        match self.kind {
            TrapKind::Spear => 1,
            TrapKind::FallingBlock => 3,
            TrapKind::Pit => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisarmPlan {
    pub hero: UnitId,
    pub trap_index: usize,
    pub from: Pos,
    pub trap: Pos,
    pub direction: Direction,
    pub dwarf: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JumpPlan {
    pub hero: UnitId,
    pub trap_index: usize,
    pub from: Pos,
    pub trap: Pos,
    pub landing: Pos,
    pub direction: Direction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingChaosSpellRoll {
    caster: UnitId,
    target: UnitId,
    spell: ChaosSpell,
    dice_count: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RustedEquipment {
    Sword(Weapon),
    Helmet,
}

impl RustedEquipment {
    fn name(self) -> &'static str {
        match self {
            Self::Sword(weapon) => weapon.name(),
            Self::Helmet => "Helmet",
        }
    }

    fn value(self) -> u16 {
        let item = match self {
            Self::Sword(weapon) => ArmoryItem::Weapon(weapon),
            Self::Helmet => ArmoryItem::Armor(Armor::Helmet),
        };
        listing(item).gold
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingTeleportRoll {
    pub subject: UnitId,
    pub forbidden_destination: Option<Pos>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RuleError {
    #[error("it is not a hero turn")]
    NotHeroTurn,
    #[error("movement dice have already been rolled")]
    AlreadyRolled,
    #[error("roll the movement dice first")]
    MovementNotRolled,
    #[error("no movement remains")]
    NoMovement,
    #[error("a moving Hero may pass through that figure but may not finish on its square")]
    MustFinishOccupiedMove,
    #[error("movement cannot continue after acting midway through a move")]
    MovementEndedByAction,
    #[error("that square is outside the board")]
    OutsideBoard,
    #[error("solid rock or a wall blocks that square")]
    Blocked,
    #[error("another figure occupies that square")]
    Occupied,
    #[error("the door is closed; open it first")]
    ClosedDoor,
    #[error("there is no closed door beside the active hero")]
    NoDoor,
    #[error("this is a false door and can never be opened")]
    FalseDoor,
    #[error("the active hero has already acted")]
    AlreadyActed,
    #[error("the active hero has no Potion of Healing")]
    NoHealingPotion,
    #[error("the active Hero has no Heroic Brew")]
    NoHeroicBrew,
    #[error("the active Hero has no Potion of Strength")]
    NoPotionOfStrength,
    #[error("that Hero has no Potion of Defense")]
    NoPotionOfDefense,
    #[error("that potion is already waiting to affect the next roll")]
    PotionAlreadyActive,
    #[error("the active hero has no mysterious purple potion")]
    NoPetrificationPotion,
    #[error("the active hero is already at full Body Points")]
    FullBody,
    #[error("the active hero's equipment is still held by Zargon")]
    EquipmentUnavailable,
    #[error("the active hero is incapacitated and cannot act")]
    Incapacitated,
    #[error("that spell-resistance roll no longer applies")]
    StaleSpellResistance,
    #[error("that immediate Chaos-spell roll no longer applies")]
    StaleChaosSpellRoll,
    #[error("the active Hero does not possess that spell card")]
    NoHeroSpell,
    #[error("the active Hero cannot cast spells right now")]
    SpellcastingUnavailable,
    #[error("that figure or door is not a legal target for this spell")]
    InvalidHeroSpellTarget,
    #[error("that Hero-spell dice roll no longer applies")]
    StaleHeroSpellRoll,
    #[error("a spell roll must be resolved before play continues")]
    HeroSpellRollPending,
    #[error("the active hero is already carrying a quest item")]
    AlreadyCarryingQuestItem,
    #[error("there is no unclaimed quest item here")]
    NoQuestItem,
    #[error("the active hero is not carrying a quest item")]
    NotCarryingQuestItem,
    #[error("there is no adjacent empty-handed Hero to receive the quest item")]
    NoQuestItemRecipient,
    #[error("there is no adjacent monster to attack")]
    NoTarget,
    #[error("a Hero carrying the mine's gold cannot attack")]
    CarryingFoolsGold,
    #[error("the attack request no longer matches the current turn")]
    StaleAttack,
    #[error("wrong number or type of dice results")]
    InvalidDice,
    #[error("treasure can only be searched for in a room")]
    TreasureOnlyInRoom,
    #[error("a room containing a monster cannot be searched for treasure")]
    MonstersInRoom,
    #[error("this hero has already searched this room for treasure")]
    AlreadySearchedRoom,
    #[error("this room cannot be searched for treasure")]
    TreasureForbidden,
    #[error("the quest condition for searching this room has not been met")]
    QuestConditionNotMet,
    #[error("the treasure deck is empty")]
    EmptyTreasureDeck,
    #[error("a visible monster prevents that search")]
    VisibleMonster,
    #[error("every surviving Hero must return to the stairway before ending the quest")]
    RetreatRequiresStairs,
    #[error("there are no secret doors in this room or corridor")]
    NoSecretDoors,
    #[error("there are no traps in this room or corridor")]
    NoTraps,
    #[error("choose the empty square ahead of or behind the falling block")]
    InvalidFallingBlockChoice,
    #[error("there is no adjacent discovered unsprung trap to disarm")]
    NoDisarmableTrap,
    #[error("only the Dwarf can disarm a trap without a Tool Kit")]
    ToolKitRequired,
    #[error("a trap must be disarmed before beginning movement")]
    DisarmBeforeMoving,
    #[error("there is no jumpable trap and empty landing square in that direction")]
    NoJumpableTrap,
    #[error("the jump request no longer matches the current move")]
    StaleJump,
    #[error("the magical-door roll no longer applies")]
    StaleTeleport,
    #[error("the active Hero is not in the mine entrance")]
    NotAtMine,
    #[error("the active Hero is already carrying the mine's gold")]
    AlreadyCarryingFoolsGold,
    #[error("the active Hero is not carrying the mine's gold")]
    NotCarryingFoolsGold,
    #[error("the active Hero does not possess the Ring of Return")]
    NoRingOfReturn,
    #[error("the active Hero does not possess an available Elixir of Life")]
    NoElixirOfLife,
    #[error("choose a dead Hero to revive with the Elixir of Life")]
    InvalidElixirTarget,
    #[error("the active Elf or Wizard has no available undeclared Spell Ring")]
    SpellRingUnavailable,
    #[error("that spell cannot be stored in the Spell Ring")]
    InvalidSpellRingSpell,
    #[error("wait for the collapsing-ceiling red die to settle")]
    CollapsingCeilingRollPending,
    #[error("there is no matching collapsing-ceiling roll to resolve")]
    StaleCollapsingCeilingRoll,
    #[error("wait for the trap's combat dice to settle")]
    TrapRollPending,
    #[error("there is no matching trap roll to resolve")]
    StaleTrapRoll,
    #[error("a dying Hero's rescue choice must be resolved first")]
    HeroDeathDecisionPending,
    #[error("that Hero is not awaiting a death-saving potion roll")]
    StaleHealingPotionRoll,
    #[error("that death-saving option is not available")]
    InvalidHeroDeathChoice,
    #[error("choose which present Hero receives the fallen Hero's possessions")]
    PossessionPickupPending,
    #[error("that Hero cannot receive these possessions")]
    InvalidPossessionRecipient,
    #[error("there is no other living Hero to receive that gift")]
    NoGiftRecipient,
    #[error("the active Hero does not own that potion")]
    NoPotionToGive,
    #[error("the requested gold gift must be positive and no greater than the giver's gold")]
    InvalidGoldTransfer,
}

pub struct Game {
    pub title: String,
    pub blurb: String,
    pub cells: Vec<Cell>,
    pub doors: Vec<Door>,
    pub units: Vec<Unit>,
    pub props: Vec<Prop>,
    pub quest_items: Vec<QuestItem>,
    pub traps: Vec<Trap>,
    /// Squares covered by the finite cardboard blocked-square markers. These
    /// stay distinct from the board's printed solid-rock mask so the renderer
    /// can place the scanned physical tiles only when their area is revealed.
    pub blocked_markers: Vec<Pos>,
    pub hero_order: Vec<UnitId>,
    pub stairs: Vec<Pos>,
    pub objective: ObjectiveDef,
    pub phase: GamePhase,
    pub hero_turn: HeroTurnState,
    pub log: VecDeque<String>,
    pub treasure_deck: TreasureDeck,
    pub searched_treasure: HashSet<(UnitId, i16)>,
    pub wandering_monster: MonsterKind,
    pub wandering_event_message: Option<String>,
    pub forbidden_treasure_regions: HashSet<i16>,
    pub escorted_ally: Option<(UnitId, UnitId)>,
    pub equipment_recovery_region: Option<i16>,
    pub pending_falling_block: Option<PendingFallingBlock>,
    pub pending_teleport_roll: Option<PendingTeleportRoll>,
    pub pending_hero_death: Option<PendingHeroDeath>,
    pub pending_possession_pickup: Option<PendingPossessionPickup>,
    /// Artifact cards taken from a fallen Hero by a monster during this quest.
    /// Campaign persistence uses this exact list; ordinary lost equipment is
    /// not eligible for the rulebook's later special-treasure recovery rule.
    pub monster_stolen_artifacts: Vec<Artifact>,
    /// Face-up Chaos cards in the order Zargon's casters used them. Individual
    /// caster hands remain private; this shared pile is public once cards act.
    pub discarded_chaos_spells: Vec<ChaosSpell>,
    /// Previously stolen artifacts that this quest explicitly requires. They
    /// replace the next ordinary Treasure-card draw, after any printed room
    /// treasure has been resolved, so quest-note treasure is never displaced.
    pub lost_artifact_treasure: VecDeque<Artifact>,
    pending_collapsing_ceiling_roll: Option<UnitId>,
    pending_trap_roll: Option<PendingTrapRoll>,
    pending_healing_potion_roll: Option<UnitId>,
    pending_forced_attack: Option<AttackPlan>,
    pub last_combat_visual: Option<CombatVisualEvent>,
    pub hero_start_positions: Vec<Pos>,
    teleport_destinations: HashMap<u8, Pos>,
    mine_region: Option<i16>,
    pub mine_entrance: Option<Pos>,
    mine_gold_amount: u16,
    monster_bounties: HashMap<MonsterKind, u16>,
    delayed_falling_block: Option<(Pos, Pos)>,
    delayed_block_crossers: HashSet<UnitId>,
    collapsing_ceiling_hazards: HashSet<Pos>,
    visual_sequence: u64,
    zargon_turn_started: bool,
    zargon_queue: VecDeque<UnitId>,
    zargon_active: Option<UnitId>,
    zargon_commanded_queue: VecDeque<UnitId>,
    zargon_commanded_active: Option<UnitId>,
    pending_chaos_spell_rolls: VecDeque<PendingChaosSpellRoll>,
    pending_hero_spell_roll: Option<HeroSpellRoll>,
    heroes_acted_this_round: HashSet<UnitId>,
    quest_events: Vec<QuestEvent>,
    rng: ChaCha8Rng,
}

const GREEN_MONSTER_FIGURES: &[MonsterKind] =
    &[MonsterKind::Orc, MonsterKind::Goblin, MonsterKind::Fimir];
const UNDEAD_MONSTER_FIGURES: &[MonsterKind] = &[
    MonsterKind::Skeleton,
    MonsterKind::Zombie,
    MonsterKind::Mummy,
];
const CHAOS_MONSTER_FIGURES: &[MonsterKind] = &[
    MonsterKind::ChaosWarrior,
    MonsterKind::Gargoyle,
    MonsterKind::ChaosSorcerer,
];

pub const fn original_us_monster_figure_count(kind: MonsterKind) -> usize {
    match kind {
        MonsterKind::Goblin => 6,
        MonsterKind::Orc => 8,
        MonsterKind::Fimir => 3,
        MonsterKind::Skeleton => 4,
        MonsterKind::Zombie => 2,
        MonsterKind::Mummy => 2,
        MonsterKind::ChaosWarrior => 4,
        MonsterKind::Gargoyle => 1,
        MonsterKind::ChaosSorcerer => 1,
    }
}

fn same_color_monster_figures(kind: MonsterKind) -> &'static [MonsterKind] {
    match kind {
        MonsterKind::Orc | MonsterKind::Goblin | MonsterKind::Fimir => GREEN_MONSTER_FIGURES,
        MonsterKind::Skeleton | MonsterKind::Zombie | MonsterKind::Mummy => UNDEAD_MONSTER_FIGURES,
        MonsterKind::ChaosWarrior | MonsterKind::Gargoyle | MonsterKind::ChaosSorcerer => {
            CHAOS_MONSTER_FIGURES
        }
    }
}

fn monster_figure_group(kind: MonsterKind) -> (&'static str, u8, usize) {
    match kind {
        MonsterKind::Orc | MonsterKind::Goblin | MonsterKind::Fimir => ("green", 0, 17),
        MonsterKind::Skeleton | MonsterKind::Zombie | MonsterKind::Mummy => ("undead", 1, 8),
        MonsterKind::ChaosWarrior | MonsterKind::Gargoyle | MonsterKind::ChaosSorcerer => {
            ("gray", 2, 6)
        }
    }
}

fn quest_effect_declares_furniture_trap(effect: &QuestEffectDef, pos: Pos) -> bool {
    match effect {
        QuestEffectDef::DamageUnlessTrapDisarmed { pos: trap_pos, .. }
        | QuestEffectDef::AwakenGuardianUnlessTrapDisarmed { pos: trap_pos, .. } => {
            *trap_pos == pos
        }
        QuestEffectDef::Bundle { effects } => effects
            .iter()
            .any(|effect| quest_effect_declares_furniture_trap(effect, pos)),
        _ => false,
    }
}

const ORC_MODEL_SLOTS: &[MonsterModelVariant] = &[
    MonsterModelVariant::OrcSword,
    MonsterModelVariant::OrcSword,
    MonsterModelVariant::OrcSword,
    MonsterModelVariant::OrcFlail,
    MonsterModelVariant::OrcFlail,
    MonsterModelVariant::OrcCleaver,
    MonsterModelVariant::OrcCleaver,
    MonsterModelVariant::OrcNotchedSword,
];
const GOBLIN_MODEL_SLOTS: &[MonsterModelVariant] = &[
    MonsterModelVariant::GoblinSword,
    MonsterModelVariant::GoblinSword,
    MonsterModelVariant::GoblinAxe,
    MonsterModelVariant::GoblinAxe,
    MonsterModelVariant::GoblinScimitar,
    MonsterModelVariant::GoblinScimitar,
];

fn model_slots(kind: MonsterKind) -> &'static [MonsterModelVariant] {
    match kind {
        MonsterKind::Orc => ORC_MODEL_SLOTS,
        MonsterKind::Goblin => GOBLIN_MODEL_SLOTS,
        _ => &[],
    }
}

fn model_slot_class(variant: MonsterModelVariant) -> MonsterModelVariant {
    match variant {
        // Grak's staff is a quest kitbash made from one of the three ordinary
        // sword-Orc pieces; it is not a ninth Orc miniature.
        MonsterModelVariant::OrcStaff => MonsterModelVariant::OrcSword,
        other => other,
    }
}

impl Game {
    pub fn demo(seed: u64) -> Result<Self> {
        Self::from_quest(QuestDefinition::demo()?, seed)
    }

    pub fn from_quest(def: QuestDefinition, seed: u64) -> Result<Self> {
        def.validate_authored_references()?;
        if def.source.is_some() {
            def.validate_original_us_source_coverage()?;
            def.validate_original_us_board_topology()?;
        }
        let heroes_captured = def.heroes_captured;
        let hero_start_positions = def.hero_starts.iter().map(|start| start.pos).collect();
        let blocked_markers = def.blocked.clone();
        ensure!(
            def.hero_starts.len() == 4,
            "a base quest requires exactly four heroes"
        );
        ensure!(
            def.doors.len() + def.secret_doors.len() <= 21,
            "quest exceeds the original-US supply of 21 assembled doors"
        );
        ensure!(
            def.secret_doors.len() <= 7,
            "quest exceeds the original-US supply of seven secret-door faces"
        );
        ensure!(
            def.blocked.len() <= 27,
            "quest exceeds the original-US blocked-marker coverage"
        );
        ensure!(
            def.traps
                .iter()
                .filter(|trap| trap.trap == TrapKind::Pit)
                .count()
                <= 6,
            "quest exceeds the original-US supply of six pit markers"
        );
        ensure!(
            def.traps
                .iter()
                .filter(|trap| trap.trap == TrapKind::FallingBlock)
                .count()
                <= 12,
            "quest exceeds the original-US supply of twelve falling-block faces"
        );
        for kind in [
            PropKind::Stairs,
            PropKind::Table,
            PropKind::Throne,
            PropKind::AlchemistsBench,
            PropKind::Chest,
            PropKind::Tomb,
            PropKind::SorcerersTable,
            PropKind::Bookcase,
            PropKind::TortureRack,
            PropKind::Fireplace,
            PropKind::WeaponRack,
            PropKind::Cupboard,
            PropKind::StarOfWest,
        ] {
            if let Some(physical_count) = kind.original_us_physical_count() {
                ensure!(
                    def.props.iter().filter(|prop| prop.prop == kind).count() <= physical_count,
                    "quest exceeds the original-US physical count for {kind:?}"
                );
            }
        }
        let mut cells = vec![Cell::default(); BOARD_WIDTH as usize * BOARD_HEIGHT as usize];

        for corridor in &def.corridors {
            for pos in corridor.area.positions() {
                ensure!(
                    Self::in_bounds(pos),
                    "corridor rectangle extends beyond the board"
                );
                cells[Self::cell_index(pos)] = Cell {
                    passable: true,
                    region: 0,
                    tint: corridor.tint,
                    revealed: false,
                };
            }
        }
        for (room_index, room) in def.rooms.iter().enumerate() {
            for pos in room.positions() {
                ensure!(
                    Self::in_bounds(pos),
                    "room {} extends beyond the board",
                    room.name
                );
                cells[Self::cell_index(pos)] = Cell {
                    passable: true,
                    region: room_index as i16 + 1,
                    tint: room.tint,
                    revealed: false,
                };
            }
        }
        let mut teleport_destinations = HashMap::new();
        if let Some(network) = &def.teleport_network {
            for destination in &network.destinations {
                ensure!(
                    (2..=12).contains(&destination.total),
                    "teleport destination total must be between 2 and 12"
                );
                ensure!(
                    Self::in_bounds(destination.pos),
                    "teleport destination is outside the board"
                );
                ensure!(
                    teleport_destinations
                        .insert(destination.total, destination.pos)
                        .is_none(),
                    "teleport destination total {} is duplicated",
                    destination.total
                );
            }
            ensure!(
                (2..=12).all(|total| teleport_destinations.contains_key(&total)),
                "a magical-door network needs destinations for every 2d6 total"
            );
        }
        let mut room_regions = HashMap::new();
        for (room_index, room) in def.rooms.iter().enumerate() {
            ensure!(
                room_regions
                    .insert(room.name.clone(), room_index as i16 + 1)
                    .is_none(),
                "room names must be unique: {}",
                room.name
            );
        }
        let mine_region =
            def.mine
                .as_ref()
                .map(|mine| {
                    room_regions.get(&mine.room).copied().ok_or_else(|| {
                        anyhow::anyhow!("mine references unknown room: {}", mine.room)
                    })
                })
                .transpose()?;
        let mine_gold_amount = def.mine.as_ref().map_or(0, |mine| mine.amount);
        let mine_entrance = def.mine.as_ref().map(|mine| mine.pos);
        let delayed_falling_block = def
            .delayed_falling_block
            .map(|block| (block.pos, block.exit));
        let collapsing_ceiling_hazards: HashSet<_> =
            def.collapsing_ceiling_hazards.iter().copied().collect();
        ensure!(
            collapsing_ceiling_hazards.len() == def.collapsing_ceiling_hazards.len(),
            "collapsing-ceiling hazard squares must be unique"
        );
        let mut monster_bounties = HashMap::new();
        for bounty in &def.monster_bounties {
            ensure!(bounty.gold > 0, "monster bounties must be positive");
            ensure!(
                monster_bounties
                    .insert(bounty.monster, bounty.gold)
                    .is_none(),
                "monster bounty is duplicated for {}",
                bounty.monster.name()
            );
        }
        let forbidden_treasure_regions = def
            .forbidden_treasure_rooms
            .iter()
            .map(|room| {
                room_regions
                    .get(room)
                    .copied()
                    .ok_or_else(|| anyhow::anyhow!("unknown treasure-forbidden room: {room}"))
            })
            .collect::<Result<HashSet<_>>>()?;
        let mut event_ids = HashSet::new();
        let quest_events = def
            .events
            .into_iter()
            .map(|event| {
                ensure!(
                    event_ids.insert(event.id.clone()),
                    "quest event ids must be unique: {}",
                    event.id
                );
                let trigger = match event.trigger {
                    QuestTriggerDef::SearchTreasure { room } => QuestTrigger::SearchTreasure {
                        region: *room_regions.get(&room).ok_or_else(|| {
                            anyhow::anyhow!("quest event references unknown room: {room}")
                        })?,
                    },
                    QuestTriggerDef::SearchTreasureAfterDefeat { room, name } => {
                        QuestTrigger::SearchTreasureAfterDefeat {
                            region: *room_regions.get(&room).ok_or_else(|| {
                                anyhow::anyhow!("quest event references unknown room: {room}")
                            })?,
                            name,
                        }
                    }
                    QuestTriggerDef::RevealRoom { room } => QuestTrigger::RevealRoom {
                        region: *room_regions.get(&room).ok_or_else(|| {
                            anyhow::anyhow!("quest event references unknown room: {room}")
                        })?,
                    },
                    QuestTriggerDef::DefeatNamed { name } => QuestTrigger::DefeatNamed { name },
                    QuestTriggerDef::OpenDoor { a, b } => {
                        ensure!(a.is_adjacent(b), "open-door quest trigger is not adjacent");
                        QuestTrigger::OpenDoor { a, b }
                    }
                };
                if let Some(marker) = event.marker {
                    ensure!(
                        Self::in_bounds(marker),
                        "quest event marker is outside the board"
                    );
                    let expected_region = match &trigger {
                        QuestTrigger::SearchTreasure { region }
                        | QuestTrigger::SearchTreasureAfterDefeat { region, .. }
                        | QuestTrigger::RevealRoom { region } => Some(*region),
                        QuestTrigger::DefeatNamed { .. } | QuestTrigger::OpenDoor { .. } => None,
                    };
                    if let Some(expected_region) = expected_region {
                        ensure!(
                            cells[Self::cell_index(marker)].region == expected_region,
                            "quest event {} marker is outside its trigger room",
                            event.id
                        );
                    }
                }
                Ok(QuestEvent {
                    id: event.id,
                    trigger,
                    effect: event.effect,
                    message: event.message,
                    resolved: false,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let mut blocked_positions = HashSet::new();
        for &pos in &def.blocked {
            ensure!(Self::in_bounds(pos), "blocked square is outside the board");
            ensure!(
                blocked_positions.insert(pos),
                "blocked-square positions must be unique"
            );
            cells[Self::cell_index(pos)].passable = false;
        }
        if !def.solid_rock.is_empty() {
            ensure!(
                def.solid_rock.len() == BOARD_HEIGHT as usize,
                "solid-rock mask must contain exactly {BOARD_HEIGHT} rows"
            );
            for (y, row) in def.solid_rock.iter().enumerate() {
                ensure!(
                    row.len() == BOARD_WIDTH as usize,
                    "solid-rock row {y} must contain exactly {BOARD_WIDTH} cells"
                );
                for (x, cell) in row.bytes().enumerate() {
                    ensure!(
                        matches!(cell, b'.' | b'#'),
                        "solid-rock mask may contain only '.' and '#'"
                    );
                    if cell == b'#' {
                        cells[Self::cell_index(Pos::new(x as u8, y as u8))].passable = false;
                    }
                }
            }
        }
        for (&total, &pos) in &teleport_destinations {
            ensure!(
                cells[Self::cell_index(pos)].passable,
                "teleport destination {total} is in solid rock"
            );
        }
        if let Some(pos) = mine_entrance {
            ensure!(Self::in_bounds(pos), "mine entrance is outside the board");
            ensure!(
                cells[Self::cell_index(pos)].passable
                    && Some(cells[Self::cell_index(pos)].region) == mine_region,
                "mine entrance is outside its named room"
            );
        }
        if let Some((pos, exit)) = delayed_falling_block {
            ensure!(
                Self::in_bounds(pos),
                "delayed falling block is outside the board"
            );
            ensure!(
                Self::in_bounds(exit) && pos.is_adjacent(exit),
                "delayed falling block exit must be an adjacent board square"
            );
            ensure!(
                cells[Self::cell_index(pos)].passable && cells[Self::cell_index(exit)].passable,
                "delayed falling block or its exit is in solid rock"
            );
        }
        for &pos in &collapsing_ceiling_hazards {
            ensure!(
                Self::in_bounds(pos),
                "collapsing-ceiling hazard is outside the board"
            );
            ensure!(
                cells[Self::cell_index(pos)].passable,
                "collapsing-ceiling hazard is in solid rock or on a blocked square"
            );
        }

        // A room is placed as one indivisible reveal batch. Reject authored
        // content that would require more same-color physical figures in that
        // one batch than the complete original-US box can supply. Larger
        // quest totals remain legal because defeated figures are recycled.
        let mut reveal_batch_figures = HashMap::<(i16, u8), usize>::new();
        for (kind, pos) in def
            .monsters
            .iter()
            .map(|monster| (monster.monster, monster.pos))
            .chain(def.allies.iter().map(|ally| (ally.figure, ally.pos)))
        {
            ensure!(Self::in_bounds(pos), "figure is outside the board");
            let region = cells[Self::cell_index(pos)].region;
            let (group_name, group, capacity) = monster_figure_group(kind);
            let demand = reveal_batch_figures.entry((region, group)).or_default();
            *demand += 1;
            ensure!(
                *demand <= capacity,
                "one reveal batch requires {} {group_name} figures, but the original-US box supplies only {capacity}",
                *demand
            );
        }

        let mut door_edges = HashSet::new();
        let doors = def
            .doors
            .into_iter()
            .map(|door| (door, false))
            .chain(def.secret_doors.into_iter().map(|door| (door, true)))
            .map(|(door, secret)| {
                ensure!(
                    door.a.is_adjacent(door.b),
                    "door endpoints must be orthogonally adjacent"
                );
                ensure!(
                    Self::in_bounds(door.a) && Self::in_bounds(door.b),
                    "door is outside the board"
                );
                let edge = if (door.a.y, door.a.x) <= (door.b.y, door.b.x) {
                    (door.a, door.b)
                } else {
                    (door.b, door.a)
                };
                ensure!(
                    door_edges.insert(edge),
                    "more than one door occupies the same wall edge"
                );
                Ok(Door {
                    a: door.a,
                    b: door.b,
                    open: door.open,
                    secret,
                    discovered: !secret,
                    searchable: door.searchable,
                    false_door: door.false_door,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let mut units = Vec::new();
        let mut hero_order = Vec::new();
        let mut occupied = HashSet::new();
        let mut next_id = 1;
        for hero in def.hero_starts {
            ensure!(Self::in_bounds(hero.pos), "hero start is outside the board");
            ensure!(
                cells[Self::cell_index(hero.pos)].passable,
                "hero starts in solid rock"
            );
            ensure!(
                occupied.insert(hero.pos) || def.stairs.contains(&hero.pos),
                "two heroes share a non-stair starting square"
            );
            let stats = hero_stats(hero.hero);
            let mut inventory = Inventory::for_hero(hero.hero);
            for artifact in hero.artifacts {
                if !inventory.artifacts.contains(&artifact) {
                    inventory.artifacts.push(artifact);
                }
            }
            units.push(Unit {
                id: next_id,
                name: hero.hero.name().to_owned(),
                figure: FigureKind::Hero(hero.hero),
                physical_figure: Some(FigureKind::Hero(hero.hero)),
                faction: Faction::Hero,
                model_variant: None,
                pos: hero.pos,
                stats,
                body: stats.body as i16,
                alive: true,
                in_pit: false,
                inventory,
                carried_quest_item: None,
                dormant: false,
                invulnerable_until_acts: false,
                equipment_available: !heroes_captured,
                spellcasting_available: !heroes_captured,
                escaped: false,
                chaos_spells: Vec::new(),
                discarded_chaos_spells: Vec::new(),
                hero_spells: Vec::new(),
                discarded_hero_spells: Vec::new(),
                spell_ring_spell: None,
                spell_ring_casts_left: 0,
                escape_target: None,
                immune_to_fire_spells: false,
                fearful: false,
                sleeping: false,
                clouded: false,
                hero_sleep_caster: None,
                skip_turns: 0,
                petrified_turns: 0,
                diagonal_attack: false,
                hidden_until_activated: false,
                immune_except_spirit_blade: false,
                champion: false,
                commanded: false,
                swift_wind: false,
                courage: false,
                rock_skin: false,
                pass_through_rock: false,
                veil_of_mist: false,
                potion_defense_bonus: 0,
            });
            hero_order.push(next_id);
            next_id += 1;
        }
        for monster in def.monsters {
            ensure!(
                Self::in_bounds(monster.pos),
                "monster starts outside the board"
            );
            ensure!(
                monster.model_variant.is_none()
                    || matches!(
                        (monster.monster, monster.model_variant),
                        (
                            MonsterKind::Orc,
                            Some(
                                MonsterModelVariant::OrcNotchedSword
                                    | MonsterModelVariant::OrcStaff
                                    | MonsterModelVariant::OrcSword
                                    | MonsterModelVariant::OrcFlail
                                    | MonsterModelVariant::OrcCleaver
                            )
                        ) | (
                            MonsterKind::Goblin,
                            Some(
                                MonsterModelVariant::GoblinSword
                                    | MonsterModelVariant::GoblinAxe
                                    | MonsterModelVariant::GoblinScimitar
                            )
                        )
                    ),
                "a monster model variant must match its logical figure type"
            );
            ensure!(
                cells[Self::cell_index(monster.pos)].passable,
                "monster starts in solid rock"
            );
            if let Some(target) = monster.escape_target {
                ensure!(
                    Self::in_bounds(target),
                    "monster escape target is outside the board"
                );
                ensure!(
                    cells[Self::cell_index(target)].passable,
                    "monster escape target is in solid rock"
                );
            }
            ensure!(
                occupied.insert(monster.pos),
                "two figures share a starting square"
            );
            let base = monster_stats(monster.monster);
            let stats = Stats {
                movement: monster.movement.unwrap_or(base.movement),
                attack: monster.attack.unwrap_or(base.attack),
                defend: monster.defend.unwrap_or(base.defend),
                body: monster.body.unwrap_or(base.body),
                mind: monster.mind.unwrap_or(base.mind),
            };
            units.push(Unit {
                id: next_id,
                name: monster
                    .name
                    .unwrap_or_else(|| monster.monster.name().to_owned()),
                figure: FigureKind::Monster(monster.monster),
                physical_figure: None,
                faction: Faction::Monster,
                model_variant: monster.model_variant,
                pos: monster.pos,
                stats,
                body: stats.body as i16,
                alive: true,
                in_pit: false,
                inventory: Inventory::default(),
                carried_quest_item: None,
                dormant: monster.dormant,
                invulnerable_until_acts: monster.invulnerable_until_acts,
                equipment_available: true,
                spellcasting_available: true,
                escaped: false,
                chaos_spells: monster.chaos_spells,
                discarded_chaos_spells: Vec::new(),
                hero_spells: Vec::new(),
                discarded_hero_spells: Vec::new(),
                spell_ring_spell: None,
                spell_ring_casts_left: 0,
                escape_target: monster.escape_target,
                immune_to_fire_spells: monster.immune_to_fire_spells,
                fearful: false,
                sleeping: false,
                clouded: false,
                hero_sleep_caster: None,
                skip_turns: 0,
                petrified_turns: 0,
                diagonal_attack: monster.diagonal_attack,
                hidden_until_activated: monster.hidden_until_activated,
                immune_except_spirit_blade: monster.immune_except_spirit_blade,
                champion: false,
                commanded: false,
                swift_wind: false,
                courage: false,
                rock_skin: false,
                pass_through_rock: false,
                veil_of_mist: false,
                potion_defense_bonus: 0,
            });
            next_id += 1;
        }
        for ally in def.allies {
            ensure!(Self::in_bounds(ally.pos), "ally starts outside the board");
            ensure!(
                cells[Self::cell_index(ally.pos)].passable,
                "ally starts in solid rock"
            );
            ensure!(
                occupied.insert(ally.pos),
                "two figures share a starting square"
            );
            let stats = Stats {
                movement: ally.movement,
                attack: ally.attack,
                defend: ally.defend,
                body: ally.body,
                mind: ally.mind,
            };
            units.push(Unit {
                id: next_id,
                name: ally.name,
                figure: FigureKind::Monster(ally.figure),
                physical_figure: None,
                faction: Faction::Hero,
                model_variant: None,
                pos: ally.pos,
                stats,
                body: stats.body as i16,
                alive: true,
                in_pit: false,
                inventory: Inventory::default(),
                carried_quest_item: None,
                dormant: false,
                invulnerable_until_acts: false,
                equipment_available: true,
                spellcasting_available: false,
                escaped: false,
                chaos_spells: Vec::new(),
                discarded_chaos_spells: Vec::new(),
                hero_spells: Vec::new(),
                discarded_hero_spells: Vec::new(),
                spell_ring_spell: None,
                spell_ring_casts_left: 0,
                escape_target: None,
                immune_to_fire_spells: false,
                fearful: false,
                sleeping: false,
                clouded: false,
                hero_sleep_caster: None,
                skip_turns: 0,
                petrified_turns: 0,
                diagonal_attack: false,
                hidden_until_activated: false,
                immune_except_spirit_blade: false,
                champion: false,
                commanded: false,
                swift_wind: false,
                courage: false,
                rock_skin: false,
                pass_through_rock: false,
                veil_of_mist: false,
                potion_defense_bonus: 0,
            });
            next_id += 1;
        }

        let mut furniture_squares = HashSet::new();
        let mut props: Vec<_> = def
            .props
            .into_iter()
            .map(|prop| {
                ensure!(
                    Self::in_bounds(prop.pos),
                    "furniture anchor is outside the board"
                );
                ensure!(
                    prop.rotation_quarters < 4,
                    "furniture rotation must be between 0 and 3 quarter turns"
                );
                for square in prop
                    .prop
                    .footprint_squares(prop.pos, prop.rotation_quarters)
                {
                    ensure!(
                        Self::in_bounds(square),
                        "furniture footprint leaves the board"
                    );
                    ensure!(
                        cells[Self::cell_index(square)].passable,
                        "furniture footprint enters solid rock at {}, {}",
                        square.x + 1,
                        square.y + 1
                    );
                    if prop.prop.blocks_movement() {
                        ensure!(
                            !occupied.contains(&square),
                            "a figure starts on furniture at {}, {}",
                            square.x + 1,
                            square.y + 1
                        );
                        ensure!(
                            furniture_squares.insert(square),
                            "furniture footprints overlap at {}, {}",
                            square.x + 1,
                            square.y + 1
                        );
                    }
                }
                Ok(Prop {
                    kind: prop.prop,
                    pos: prop.pos,
                    rotation_quarters: prop.rotation_quarters,
                    visible: true,
                    carried_by: None,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let stair_squares = def.stairs.iter().copied().collect::<HashSet<_>>();
        ensure!(
            stair_squares.len() == def.stairs.len(),
            "stairway squares must be unique"
        );
        for &square in &stair_squares {
            ensure!(
                Self::in_bounds(square),
                "stairway square is outside the board"
            );
            ensure!(
                cells[Self::cell_index(square)].passable,
                "stairway square is in solid rock or blocked"
            );
        }
        let rendered_stair_squares = props
            .iter()
            .filter(|prop| prop.kind == PropKind::Stairs)
            .flat_map(Prop::footprint_squares)
            .collect::<HashSet<_>>();
        ensure!(
            rendered_stair_squares == stair_squares,
            "the physical stairway footprint must match every playable stairway square"
        );
        for (&total, &destination) in &teleport_destinations {
            ensure!(
                !furniture_squares.contains(&destination),
                "teleport destination {total} is occupied by furniture"
            );
        }
        for unit in &units {
            if let Some(destination) = unit.escape_target {
                ensure!(
                    !furniture_squares.contains(&destination),
                    "{}'s escape target is occupied by furniture",
                    unit.name
                );
            }
        }
        let mut quest_item_ids = HashSet::new();
        let mut claimed_props = HashSet::new();
        let quest_items = def
            .quest_items
            .into_iter()
            .map(|item| {
                ensure!(
                    quest_item_ids.insert(item.id.clone()),
                    "quest item ids must be unique: {}",
                    item.id
                );
                let prop_index = props
                    .iter()
                    .enumerate()
                    .find_map(|(index, prop)| {
                        (prop.kind == item.prop
                            && prop.pos == item.pos
                            && !claimed_props.contains(&index))
                        .then_some(index)
                    })
                    .ok_or_else(|| {
                        anyhow::anyhow!("quest item {} has no matching furniture prop", item.id)
                    })?;
                claimed_props.insert(prop_index);
                props[prop_index].visible = true;
                let holder = item
                    .held_by
                    .as_deref()
                    .map(|name| {
                        units
                            .iter()
                            .find(|unit| unit.name == name)
                            .map(|unit| unit.id)
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "quest item {} references unknown holder {name}",
                                    item.id
                                )
                            })
                    })
                    .transpose()?;
                Ok(QuestItem {
                    id: item.id,
                    prop_index,
                    sealed_gold: item.sealed_gold,
                    holder,
                    delivered: false,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        for (item_index, item) in quest_items.iter().enumerate() {
            if let Some(holder) = item.holder {
                let holder_pos = units
                    .iter()
                    .find(|unit| unit.id == holder)
                    .expect("validated quest-item holder exists")
                    .pos;
                let prop = &mut props[item.prop_index];
                prop.pos = holder_pos;
                prop.carried_by = Some(holder);
                units
                    .iter_mut()
                    .find(|unit| unit.id == holder)
                    .expect("validated quest-item holder exists")
                    .carried_quest_item = Some(item_index);
            }
        }

        let mut trap_positions = HashSet::new();
        let traps = def
            .traps
            .into_iter()
            .map(|trap| {
                ensure!(Self::in_bounds(trap.pos), "trap is outside the board");
                ensure!(
                    trap_positions.insert(trap.pos),
                    "more than one trap occupies the same square"
                );
                ensure!(
                    cells[Self::cell_index(trap.pos)].passable,
                    "trap is in solid rock"
                );
                ensure!(
                    !occupied.contains(&trap.pos),
                    "a trap overlaps a starting figure at {}, {}",
                    trap.pos.x + 1,
                    trap.pos.y + 1
                );
                if props
                    .iter()
                    .any(|prop| prop.footprint_squares().contains(&trap.pos))
                {
                    ensure!(
                        quest_events.iter().any(|event| {
                            quest_effect_declares_furniture_trap(&event.effect, trap.pos)
                        }),
                        "a trap overlaps a physical prop without a typed furniture-trap event at {}, {}",
                        trap.pos.x + 1,
                        trap.pos.y + 1
                    );
                }
                Ok(Trap {
                    kind: trap.trap,
                    pos: trap.pos,
                    discovered: false,
                    sprung: false,
                    disarmed: false,
                    trigger_on_entry: trap.trigger_on_entry,
                    disarmable: trap.disarmable,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let treasure_deck = TreasureDeck::original_us(&mut rng);
        let mut game = Self {
            title: def.title,
            blurb: def.blurb,
            cells,
            doors,
            units,
            props,
            quest_items,
            traps,
            blocked_markers,
            hero_order,
            stairs: def.stairs,
            objective: def.objective,
            phase: GamePhase::HeroTurn { order_index: 0 },
            hero_turn: HeroTurnState::default(),
            log: VecDeque::new(),
            treasure_deck,
            searched_treasure: HashSet::new(),
            wandering_monster: def.wandering_monster,
            wandering_event_message: def.wandering_event_message,
            forbidden_treasure_regions,
            escorted_ally: None,
            equipment_recovery_region: None,
            pending_falling_block: None,
            pending_teleport_roll: None,
            pending_hero_death: None,
            pending_possession_pickup: None,
            monster_stolen_artifacts: Vec::new(),
            discarded_chaos_spells: Vec::new(),
            lost_artifact_treasure: VecDeque::new(),
            pending_collapsing_ceiling_roll: None,
            pending_trap_roll: None,
            pending_healing_potion_roll: None,
            pending_forced_attack: None,
            last_combat_visual: None,
            hero_start_positions,
            teleport_destinations,
            mine_region,
            mine_entrance,
            mine_gold_amount,
            monster_bounties,
            delayed_falling_block,
            delayed_block_crossers: HashSet::new(),
            collapsing_ceiling_hazards,
            visual_sequence: 0,
            zargon_turn_started: false,
            zargon_queue: VecDeque::new(),
            zargon_active: None,
            zargon_commanded_queue: VecDeque::new(),
            zargon_commanded_active: None,
            pending_chaos_spell_rolls: VecDeque::new(),
            pending_hero_spell_roll: None,
            heroes_acted_this_round: HashSet::new(),
            quest_events,
            rng,
        };
        let hero_positions: Vec<_> = game
            .hero_order
            .iter()
            .filter_map(|&id| game.unit(id).map(|unit| unit.pos))
            .collect();
        for pos in hero_positions {
            game.reveal_from(pos);
        }
        game.push_log(format!("The heroes begin {}.", game.title));
        Ok(game)
    }

    fn unit_needs_physical_placement(&self, unit: &Unit) -> bool {
        unit.alive
            && !unit.escaped
            && !unit.hidden_until_activated
            && unit.physical_figure.is_none()
            && matches!(unit.figure, FigureKind::Monster(_))
            && self.cells[Self::cell_index(unit.pos)].revealed
    }

    fn allocate_pending_visible_figures(&mut self) {
        let mut pending = self
            .units
            .iter()
            .filter(|unit| self.unit_needs_physical_placement(unit))
            .map(|unit| (unit.model_variant.is_none(), unit.id))
            .collect::<Vec<_>>();
        // Named/specified sculpts reserve and receive their exact miniature
        // before generic monsters in the newly placed area.
        pending.sort_unstable();
        for (_, id) in pending {
            self.allocate_physical_piece(id);
        }
    }

    fn assigned_physical_count(&self, kind: MonsterKind) -> usize {
        self.units
            .iter()
            .filter(|unit| unit.alive && unit.physical_figure == Some(FigureKind::Monster(kind)))
            .count()
    }

    fn assigned_model_class_count(&self, kind: MonsterKind, class: MonsterModelVariant) -> usize {
        self.units
            .iter()
            .filter(|unit| {
                unit.alive
                    && unit.physical_figure == Some(FigureKind::Monster(kind))
                    && unit.model_variant.map(model_slot_class) == Some(class)
            })
            .count()
    }

    fn reserved_model_class_count(
        &self,
        kind: MonsterKind,
        class: MonsterModelVariant,
        exclude: UnitId,
    ) -> usize {
        self.units
            .iter()
            .filter(|unit| {
                unit.id != exclude
                    && unit.alive
                    && unit.physical_figure.is_none()
                    && unit.figure == FigureKind::Monster(kind)
                    && unit.model_variant.map(model_slot_class) == Some(class)
            })
            .count()
    }

    fn available_model_variant(
        &self,
        kind: MonsterKind,
        preferred: Option<MonsterModelVariant>,
        target: UnitId,
    ) -> Option<Option<MonsterModelVariant>> {
        let slots = model_slots(kind);
        if slots.is_empty() {
            return Some(None);
        }
        if let Some(preferred) = preferred {
            let class = model_slot_class(preferred);
            let capacity = slots
                .iter()
                .filter(|&&slot| model_slot_class(slot) == class)
                .count();
            if self.assigned_model_class_count(kind, class) < capacity {
                return Some(Some(preferred));
            }
        }
        for &variant in slots {
            let class = model_slot_class(variant);
            let capacity = slots
                .iter()
                .filter(|&&slot| model_slot_class(slot) == class)
                .count();
            let assigned = self.assigned_model_class_count(kind, class);
            let reserved = self.reserved_model_class_count(kind, class, target);
            if assigned + reserved < capacity {
                return Some(Some(variant));
            }
        }
        None
    }

    fn allocate_physical_piece(&mut self, target: UnitId) -> bool {
        let Some(target_index) = self.units.iter().position(|unit| unit.id == target) else {
            return false;
        };
        if self.units[target_index].physical_figure.is_some() {
            return true;
        }
        let FigureKind::Monster(requested) = self.units[target_index].figure else {
            return false;
        };
        let preferred = self.units[target_index].model_variant;
        let same_color = same_color_monster_figures(requested);

        let exact_preferred_source = preferred.and_then(|preferred| {
            let class = model_slot_class(preferred);
            self.units.iter().position(|unit| {
                !unit.alive
                    && unit.physical_figure == Some(FigureKind::Monster(requested))
                    && unit.model_variant.map(model_slot_class) == Some(class)
            })
        });
        let reusable_source = exact_preferred_source
            .or_else(|| {
                self.units.iter().position(|unit| {
                    !unit.alive
                        && unit.physical_figure == Some(FigureKind::Monster(requested))
                })
            })
            .or_else(|| {
                self.units.iter().position(|unit| {
                    !unit.alive
                        && unit.physical_figure.is_some_and(|figure| {
                            matches!(figure, FigureKind::Monster(kind) if same_color.contains(&kind))
                        })
                })
            });
        if let Some(source_index) = reusable_source {
            let physical = self.units[source_index]
                .physical_figure
                .take()
                .expect("reusable source owns a physical figure");
            let source_variant = self.units[source_index].model_variant.take();
            let physical_kind = match physical {
                FigureKind::Monster(kind) => kind,
                FigureKind::Hero(_) => unreachable!("monster allocator selected a Hero piece"),
            };
            let variant = if physical_kind == requested
                && preferred.is_some_and(|preferred| {
                    source_variant.map(model_slot_class) == Some(model_slot_class(preferred))
                }) {
                preferred
            } else {
                source_variant
            };
            self.units[target_index].physical_figure = Some(physical);
            self.units[target_index].model_variant = variant;
            return true;
        }

        let mut candidate_kinds = Vec::with_capacity(same_color.len());
        candidate_kinds.push(requested);
        candidate_kinds.extend(same_color.iter().copied().filter(|&kind| kind != requested));
        for physical_kind in candidate_kinds {
            if self.assigned_physical_count(physical_kind)
                >= original_us_monster_figure_count(physical_kind)
            {
                continue;
            }
            let requested_variant = (physical_kind == requested).then_some(preferred).flatten();
            let Some(variant) =
                self.available_model_variant(physical_kind, requested_variant, target)
            else {
                continue;
            };
            self.units[target_index].physical_figure = Some(FigureKind::Monster(physical_kind));
            self.units[target_index].model_variant = variant;
            return true;
        }
        false
    }

    pub fn apply_hero_setup(
        &mut self,
        heroes: &[crate::startup::HeroSetup; 4],
        wizard_groups: &[crate::startup::SpellGroup],
        elf_group: crate::startup::SpellGroup,
    ) {
        let ordered_ids = heroes
            .iter()
            .filter_map(|setup| {
                self.units
                    .iter()
                    .find(|unit| unit.figure == FigureKind::Hero(setup.hero))
                    .map(|unit| unit.id)
            })
            .collect::<Vec<_>>();
        if ordered_ids.len() == 4 && ordered_ids.iter().copied().collect::<HashSet<_>>().len() == 4
        {
            self.hero_order = ordered_ids;
            self.phase = GamePhase::HeroTurn { order_index: 0 };
            self.hero_turn = HeroTurnState::default();
        }
        for setup in heroes {
            if let Some(unit) = self
                .units
                .iter_mut()
                .find(|unit| unit.figure == FigureKind::Hero(setup.hero))
            {
                let display_name = setup.hero_name.trim();
                unit.name = if display_name.is_empty() {
                    setup.hero.name().to_owned()
                } else {
                    display_name.to_owned()
                };
            }
        }
        for unit in self
            .units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Hero)
        {
            unit.hero_spells.clear();
            unit.discarded_hero_spells.clear();
            unit.spell_ring_spell = None;
            unit.spell_ring_casts_left = 0;
            match unit.figure {
                FigureKind::Hero(crate::model::HeroKind::Wizard) => {
                    unit.hero_spells
                        .extend(wizard_groups.iter().flat_map(|group| group.spells()));
                }
                FigureKind::Hero(crate::model::HeroKind::Elf) => {
                    unit.hero_spells.extend(elf_group.spells());
                }
                _ => {}
            }
        }
    }

    pub fn active_hero_spells(&self) -> &[HeroSpell] {
        self.active_hero()
            .map(|hero| hero.hero_spells.as_slice())
            .unwrap_or(&[])
    }

    /// Spell cards the active Hero may legally cast as the next action. A
    /// bearer of the Wand of Magic receives one follow-up cast, but it must be
    /// a different spell from the first cast this turn.
    pub fn active_castable_hero_spells(&self) -> Vec<HeroSpell> {
        let Some(caster) = self.active_hero() else {
            return Vec::new();
        };
        if caster.sleeping
            || caster.clouded
            || caster.petrified_turns > 0
            || !caster.spellcasting_available
            || self.pending_hero_spell_roll.is_some()
        {
            return Vec::new();
        }
        caster
            .hero_spells
            .iter()
            .copied()
            .filter(|&spell| self.hero_spell_action_allowed(caster, spell))
            .collect()
    }

    fn hero_spell_action_allowed(&self, caster: &Unit, spell: HeroSpell) -> bool {
        if !self.hero_turn.action_used {
            return true;
        }
        caster.uses_wand_of_magic()
            && !self.hero_turn.wand_follow_up_used
            && self
                .hero_turn
                .wand_first_spell
                .is_some_and(|first| first != spell)
    }

    /// Spell cards that may be declared as stored in the active Elf or
    /// Wizard's Spell Ring. Declaration happens before that Hero begins moving
    /// or acting in the Quest.
    pub fn spell_ring_storable_spells(&self) -> Vec<HeroSpell> {
        let Some(hero) = self.active_hero() else {
            return Vec::new();
        };
        if !hero.alive
            || hero.escaped
            || hero.sleeping
            || hero.clouded
            || hero.petrified_turns > 0
            || !hero.uses_spell_ring()
            || hero.spell_ring_spell.is_some()
            || self.hero_turn.movement_roll.is_some()
            || self.hero_turn.moved_steps > 0
            || self.hero_turn.action_used
        {
            return Vec::new();
        }
        hero.hero_spells.clone()
    }

    pub fn store_active_spell_in_ring(&mut self, spell: HeroSpell) -> Result<(), RuleError> {
        if !self.spell_ring_storable_spells().contains(&spell) {
            return Err(if self.active_hero().is_some_and(Unit::uses_spell_ring) {
                RuleError::InvalidSpellRingSpell
            } else {
                RuleError::SpellRingUnavailable
            });
        }
        let hero_id = self.active_awake_hero_id()?;
        let hero = self
            .units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .expect("active Hero exists");
        hero.spell_ring_spell = Some(spell);
        hero.spell_ring_casts_left = 2;
        let name = hero.name.clone();
        self.push_log(format!(
            "{name} stored {} in the Spell Ring; it may be cast twice this Quest.",
            spell.name()
        ));
        Ok(())
    }

    pub fn valid_hero_spell_targets(&self, spell: HeroSpell) -> Vec<HeroSpellTarget> {
        let Some(caster) = self.active_hero() else {
            return Vec::new();
        };
        if caster.sleeping
            || caster.clouded
            || caster.petrified_turns > 0
            || !caster.spellcasting_available
            || !caster.hero_spells.contains(&spell)
            || self.pending_hero_spell_roll.is_some()
            || !self.hero_spell_action_allowed(caster, spell)
        {
            return Vec::new();
        }

        if spell == HeroSpell::Genie {
            let mut targets = self
                .units
                .iter()
                .filter(|unit| {
                    unit.alive
                        && unit.faction == Faction::Monster
                        && !unit.dormant
                        && self.is_visible(unit)
                        && !unit.is_immune_to_hero_spell(spell)
                })
                .map(|unit| HeroSpellTarget::Unit(unit.id))
                .collect::<Vec<_>>();
            targets.extend(
                self.doors
                    .iter()
                    .enumerate()
                    .filter(|(_, door)| {
                        !door.open
                            && !door.false_door
                            && (!door.secret || door.discovered)
                            && (self.cells[Self::cell_index(door.a)].revealed
                                || self.cells[Self::cell_index(door.b)].revealed)
                    })
                    .map(|(index, _)| HeroSpellTarget::Door(index)),
            );
            return targets;
        }

        let target_faction = match spell {
            HeroSpell::SwiftWind
            | HeroSpell::HealBody
            | HeroSpell::PassThroughRock
            | HeroSpell::RockSkin
            | HeroSpell::Courage
            | HeroSpell::WaterOfHealing
            | HeroSpell::VeilOfMist => Faction::Hero,
            HeroSpell::Tempest
            | HeroSpell::BallOfFlame
            | HeroSpell::FireOfWrath
            | HeroSpell::Sleep => Faction::Monster,
            HeroSpell::Genie => unreachable!("Genie handled above"),
        };
        self.units
            .iter()
            .filter(|unit| {
                unit.alive
                    && !unit.escaped
                    && unit.faction == target_faction
                    && !unit.dormant
                    && self.is_visible(unit)
                    && self.can_see(caster.pos, unit.pos)
                    && !unit.is_immune_to_hero_spell(spell)
            })
            .filter(|unit| {
                !matches!(spell, HeroSpell::HealBody | HeroSpell::WaterOfHealing)
                    || unit.body < unit.stats.body as i16
            })
            .filter(|unit| {
                spell != HeroSpell::Sleep
                    || !matches!(
                        unit.figure,
                        FigureKind::Monster(
                            MonsterKind::Skeleton | MonsterKind::Zombie | MonsterKind::Mummy
                        )
                    )
            })
            .filter(|unit| spell != HeroSpell::Courage || self.unit_can_see_monster(unit.id))
            .map(|unit| HeroSpellTarget::Unit(unit.id))
            .collect()
    }

    pub fn cast_active_hero_spell(
        &mut self,
        spell: HeroSpell,
        target: HeroSpellTarget,
    ) -> Result<HeroSpellCast, RuleError> {
        if self.pending_hero_spell_roll.is_some() {
            return Err(RuleError::HeroSpellRollPending);
        }
        let caster_id = self.active_awake_hero_id()?;
        let caster = self.unit(caster_id).expect("active Hero exists");
        if !caster.spellcasting_available {
            return Err(RuleError::SpellcastingUnavailable);
        }
        if !self.hero_spell_action_allowed(caster, spell) {
            return Err(RuleError::AlreadyActed);
        }
        let spell_index = caster
            .hero_spells
            .iter()
            .position(|&card| card == spell)
            .ok_or(RuleError::NoHeroSpell)?;
        if !self.valid_hero_spell_targets(spell).contains(&target) {
            return Err(RuleError::InvalidHeroSpellTarget);
        }

        let caster_name = self
            .unit(caster_id)
            .expect("active Hero exists")
            .name
            .clone();
        let was_follow_up = self.hero_turn.action_used;
        let caster = self
            .units
            .iter_mut()
            .find(|unit| unit.id == caster_id)
            .expect("active Hero exists");
        let uses_spell_ring = caster.uses_spell_ring()
            && caster.spell_ring_spell == Some(spell)
            && caster.spell_ring_casts_left > 0;
        let discarded = if uses_spell_ring {
            caster.spell_ring_casts_left -= 1;
            if caster.spell_ring_casts_left == 0 {
                Some(caster.hero_spells.remove(spell_index))
            } else {
                None
            }
        } else {
            Some(caster.hero_spells.remove(spell_index))
        };
        if let Some(discarded) = discarded {
            caster.discarded_hero_spells.push(discarded);
        }
        if was_follow_up {
            self.hero_turn.wand_follow_up_used = true;
        } else {
            self.hero_turn.action_used = true;
            if caster.uses_wand_of_magic() {
                self.hero_turn.wand_first_spell = Some(spell);
            }
        }
        if self.hero_turn.moved_steps > 0 {
            self.hero_turn.movement_left = 0;
        }

        let outcome = match target {
            HeroSpellTarget::Door(index) => {
                debug_assert_eq!(spell, HeroSpell::Genie);
                let (a, b) = self.open_door_index(index)?;
                self.push_log(format!(
                    "{caster_name} cast Genie; the door at {},{} - {},{} flew open.",
                    a.x + 1,
                    a.y + 1,
                    b.x + 1,
                    b.y + 1
                ));
                HeroSpellCast::Resolved
            }
            HeroSpellTarget::Unit(target_id) => {
                let target_index = self
                    .units
                    .iter()
                    .position(|unit| unit.id == target_id)
                    .ok_or(RuleError::InvalidHeroSpellTarget)?;
                let target_name = self.units[target_index].name.clone();
                match spell {
                    HeroSpell::SwiftWind => {
                        self.units[target_index].swift_wind = true;
                        self.push_log(format!(
                            "{caster_name} cast Swift Wind on {target_name}; the next movement roll uses twice the normal red dice."
                        ));
                        HeroSpellCast::Resolved
                    }
                    HeroSpell::Tempest => {
                        self.units[target_index].skip_turns =
                            self.units[target_index].skip_turns.saturating_add(1);
                        self.push_log(format!(
                            "{caster_name} cast Tempest; {target_name} will miss the next turn."
                        ));
                        HeroSpellCast::Resolved
                    }
                    HeroSpell::HealBody | HeroSpell::WaterOfHealing => {
                        let before = self.units[target_index].body;
                        self.units[target_index].body =
                            (before + 4).min(self.units[target_index].stats.body as i16);
                        let restored = self.units[target_index].body - before;
                        self.push_log(format!(
                            "{caster_name} cast {} on {target_name}, restoring {restored} Body Points.",
                            spell.name()
                        ));
                        HeroSpellCast::Resolved
                    }
                    HeroSpell::PassThroughRock => {
                        self.units[target_index].pass_through_rock = true;
                        self.push_log(format!(
                            "{caster_name} cast Pass Through Rock on {target_name}; walls may be crossed during the next move."
                        ));
                        HeroSpellCast::Resolved
                    }
                    HeroSpell::RockSkin => {
                        self.units[target_index].rock_skin = true;
                        self.push_log(format!(
                            "{caster_name} cast Rock Skin on {target_name}; one extra defend die applies until damage is suffered."
                        ));
                        HeroSpellCast::Resolved
                    }
                    HeroSpell::Courage => {
                        self.units[target_index].courage = true;
                        self.push_log(format!(
                            "{caster_name} cast Courage on {target_name}; the next attack gains two combat dice."
                        ));
                        HeroSpellCast::Resolved
                    }
                    HeroSpell::VeilOfMist => {
                        self.units[target_index].veil_of_mist = true;
                        self.push_log(format!(
                            "{caster_name} cast Veil of Mist on {target_name}; Monsters may be passed through during the next move."
                        ));
                        HeroSpellCast::Resolved
                    }
                    HeroSpell::BallOfFlame | HeroSpell::FireOfWrath | HeroSpell::Sleep => {
                        if spell == HeroSpell::Sleep {
                            self.units[target_index].sleeping = true;
                            self.units[target_index].hero_sleep_caster = Some(caster_id);
                        }
                        let dice_count = match spell {
                            HeroSpell::BallOfFlame => 2,
                            HeroSpell::FireOfWrath => 1,
                            HeroSpell::Sleep => self.units[target_index].effective_mind(),
                            _ => unreachable!(),
                        };
                        let roll = HeroSpellRoll {
                            caster: caster_id,
                            target: target_id,
                            spell,
                            dice_count,
                            kind: HeroSpellDiceKind::Red,
                        };
                        self.pending_hero_spell_roll = Some(roll);
                        self.push_log(format!(
                            "{caster_name} cast {} on {target_name}; {dice_count} physical red dice must resolve the spell.",
                            spell.name()
                        ));
                        HeroSpellCast::Roll(roll)
                    }
                    HeroSpell::Genie => {
                        let defend_dice = self.units[target_index].effective_defense_dice();
                        let roll = HeroSpellRoll {
                            caster: caster_id,
                            target: target_id,
                            spell,
                            dice_count: 5 + defend_dice,
                            kind: HeroSpellDiceKind::Combat {
                                attack_dice: 5,
                                defend_dice,
                            },
                        };
                        self.pending_hero_spell_roll = Some(roll);
                        self.push_log(format!(
                            "{caster_name} summoned the Genie to attack {target_name} with five combat dice."
                        ));
                        HeroSpellCast::Roll(roll)
                    }
                }
            }
        };
        Ok(outcome)
    }

    pub fn resolve_hero_spell_red_roll(
        &mut self,
        roll: HeroSpellRoll,
        dice: &[u8],
    ) -> Result<(), RuleError> {
        if self.pending_hero_spell_roll != Some(roll)
            || roll.kind != HeroSpellDiceKind::Red
            || dice.len() != roll.dice_count as usize
            || dice.iter().any(|face| !(1..=6).contains(face))
        {
            return Err(
                if dice.len() != roll.dice_count as usize
                    || dice.iter().any(|face| !(1..=6).contains(face))
                {
                    RuleError::InvalidDice
                } else {
                    RuleError::StaleHeroSpellRoll
                },
            );
        }
        self.pending_hero_spell_roll = None;
        let caster_name = self
            .unit(roll.caster)
            .map(|unit| unit.name.clone())
            .unwrap_or_else(|| "The caster".to_owned());
        let target_name = self
            .unit(roll.target)
            .map(|unit| unit.name.clone())
            .ok_or(RuleError::StaleHeroSpellRoll)?;
        match roll.spell {
            HeroSpell::BallOfFlame => {
                let saves = dice.iter().filter(|&&face| face >= 5).count() as u8;
                let damage = 2u8.saturating_sub(saves);
                self.damage_from_hero_spell(roll.caster, roll.target, roll.spell, damage);
                self.push_log(format!(
                    "{target_name} rolled {saves} Ball of Flame saves and suffered {damage} Body Points."
                ));
            }
            HeroSpell::FireOfWrath => {
                let saved = dice[0] >= 5;
                let damage = u8::from(!saved);
                self.damage_from_hero_spell(roll.caster, roll.target, roll.spell, damage);
                self.push_log(format!(
                    "{target_name} rolled {} against Fire of Wrath; {}.",
                    dice[0],
                    if saved {
                        "the flame was extinguished"
                    } else {
                        "1 Body Point was lost"
                    }
                ));
            }
            HeroSpell::Sleep => {
                let awakened = dice.contains(&6);
                if let Some(target) = self.units.iter_mut().find(|unit| unit.id == roll.target) {
                    target.sleeping = !awakened;
                    if awakened {
                        target.hero_sleep_caster = None;
                    }
                }
                self.push_log(format!(
                    "{caster_name}'s Sleep spell {} {target_name}.",
                    if awakened {
                        "was broken immediately by"
                    } else {
                        "sent"
                    }
                ));
            }
            _ => return Err(RuleError::StaleHeroSpellRoll),
        }
        self.check_terminal();
        Ok(())
    }

    pub fn resolve_hero_spell_combat_roll(
        &mut self,
        roll: HeroSpellRoll,
        attack_faces: &[CombatFace],
        defend_faces: &[CombatFace],
    ) -> Result<CombatOutcome, RuleError> {
        let HeroSpellDiceKind::Combat {
            attack_dice,
            defend_dice,
        } = roll.kind
        else {
            return Err(RuleError::StaleHeroSpellRoll);
        };
        if self.pending_hero_spell_roll != Some(roll) {
            return Err(RuleError::StaleHeroSpellRoll);
        }
        if attack_faces.len() != attack_dice as usize || defend_faces.len() != defend_dice as usize
        {
            return Err(RuleError::InvalidDice);
        }
        self.pending_hero_spell_roll = None;
        let skulls = attack_faces
            .iter()
            .filter(|&&face| face == CombatFace::Skull)
            .count() as u8;
        let blocks = defend_faces
            .iter()
            .filter(|&&face| face == CombatFace::BlackShield)
            .count() as u8;
        let damage = skulls.saturating_sub(blocks);
        let target_name = self
            .unit(roll.target)
            .map(|unit| unit.name.clone())
            .ok_or(RuleError::StaleHeroSpellRoll)?;
        self.damage_from_hero_spell(roll.caster, roll.target, roll.spell, damage);
        let defender_died = self.unit(roll.target).is_none_or(|unit| !unit.alive);
        self.push_log(format!(
            "The Genie attacked {target_name}: {skulls} skulls, {blocks} blocks, {damage} damage{}. ",
            if defender_died { " - defeated" } else { "" }
        ));
        self.check_terminal();
        Ok(CombatOutcome {
            skulls,
            blocks,
            damage,
            defender_died,
        })
    }

    pub const fn in_bounds(pos: Pos) -> bool {
        pos.x < BOARD_WIDTH && pos.y < BOARD_HEIGHT
    }

    pub const fn cell_index(pos: Pos) -> usize {
        pos.y as usize * BOARD_WIDTH as usize + pos.x as usize
    }

    const fn pos_from_cell_index(index: usize) -> Pos {
        Pos::new(
            (index % BOARD_WIDTH as usize) as u8,
            (index / BOARD_WIDTH as usize) as u8,
        )
    }

    pub fn cell(&self, pos: Pos) -> Option<&Cell> {
        Self::in_bounds(pos).then(|| &self.cells[Self::cell_index(pos)])
    }

    pub fn unit(&self, id: UnitId) -> Option<&Unit> {
        self.units.iter().find(|unit| unit.id == id)
    }

    pub fn active_hero_id(&self) -> Option<UnitId> {
        match self.phase {
            GamePhase::HeroTurn { order_index } => self.hero_order.get(order_index).copied(),
            _ => None,
        }
    }

    pub fn active_hero(&self) -> Option<&Unit> {
        self.active_hero_id().and_then(|id| self.unit(id))
    }

    fn active_awake_hero_id(&self) -> Result<UnitId, RuleError> {
        let hero_id = self.active_hero_id().ok_or(RuleError::NotHeroTurn)?;
        if self
            .unit(hero_id)
            .is_some_and(|hero| hero.sleeping || hero.clouded || hero.petrified_turns > 0)
        {
            Err(RuleError::Incapacitated)
        } else {
            Ok(hero_id)
        }
    }

    pub fn active_mover_id(&self) -> Option<UnitId> {
        match self.phase {
            GamePhase::HeroTurn { .. } => self.active_hero_id(),
            GamePhase::AllyTurn { ally, .. } => Some(ally),
            _ => None,
        }
    }

    pub fn is_visible(&self, unit: &Unit) -> bool {
        !unit.escaped
            && !unit.hidden_until_activated
            // A legal reveal can temporarily outpace the reusable physical
            // box supply (most visibly when Sir Ragnar's alarm places every
            // remaining monster). Alive logical monsters must never vanish
            // merely because every same-color miniature is already assigned.
            // The renderer uses the correct classic sculpt as a virtual
            // overflow copy until a defeated physical piece can be recycled.
            // Dead units keep only their actual piece for the topple animation.
            && (unit.alive || unit.physical_figure.is_some())
            && (matches!(unit.figure, FigureKind::Hero(_))
                || self.cells[Self::cell_index(unit.pos)].revealed)
    }

    pub fn is_door_visible(&self, door: &Door) -> bool {
        (!door.secret || door.discovered)
            && (self.cells[Self::cell_index(door.a)].revealed
                || self.cells[Self::cell_index(door.b)].revealed)
    }

    pub fn is_prop_visible(&self, prop: &Prop) -> bool {
        prop.visible && self.cells[Self::cell_index(prop.pos)].revealed
    }

    pub fn blocking_prop_at(&self, pos: Pos) -> Option<&Prop> {
        self.props.iter().find(|prop| {
            prop.visible
                && prop.carried_by.is_none()
                && prop.kind.blocks_movement()
                && prop.occupies_square(pos)
        })
    }

    pub fn is_furniture_square(&self, pos: Pos) -> bool {
        self.blocking_prop_at(pos).is_some()
    }

    pub fn is_trap_marker_visible(&self, trap: &Trap) -> bool {
        trap.sprung && trap.kind != TrapKind::Spear
    }

    fn unit_can_see_monster(&self, observer_id: UnitId) -> bool {
        let Some(observer) = self.unit(observer_id) else {
            return false;
        };
        self.units.iter().any(|unit| {
            unit.alive
                && unit.faction == Faction::Monster
                && !unit.dormant
                && self.is_visible(unit)
                && self.can_see(observer.pos, unit.pos)
        })
    }

    /// Applies the original center-of-square sight rule. A crossed wall or
    /// closed door blocks sight, as does an intervening living figure. Merely
    /// touching a grid corner advances diagonally and does not count as
    /// crossing either wall edge.
    pub fn can_see(&self, observer: Pos, target: Pos) -> bool {
        if observer == target {
            return true;
        }
        let dx = target.x as i16 - observer.x as i16;
        let dy = target.y as i16 - observer.y as i16;
        let step_x = dx.signum();
        let step_y = dy.signum();
        let delta_x = if dx == 0 {
            f32::INFINITY
        } else {
            1.0 / (dx.unsigned_abs() as f32)
        };
        let delta_y = if dy == 0 {
            f32::INFINITY
        } else {
            1.0 / (dy.unsigned_abs() as f32)
        };
        let mut max_x = delta_x * 0.5;
        let mut max_y = delta_y * 0.5;
        let mut cursor = observer;

        while cursor != target {
            let next = if (max_x - max_y).abs() < f32::EPSILON * 8.0 {
                max_x += delta_x;
                max_y += delta_y;
                Pos::new(
                    (cursor.x as i16 + step_x) as u8,
                    (cursor.y as i16 + step_y) as u8,
                )
            } else if max_x < max_y {
                max_x += delta_x;
                let next = Pos::new((cursor.x as i16 + step_x) as u8, cursor.y);
                if !self.boundary_is_open(cursor, next) {
                    return false;
                }
                next
            } else {
                max_y += delta_y;
                let next = Pos::new(cursor.x, (cursor.y as i16 + step_y) as u8);
                if !self.boundary_is_open(cursor, next) {
                    return false;
                }
                next
            };
            cursor = next;
            if cursor != target && self.occupied_by_alive(cursor, None).is_some() {
                return false;
            }
        }
        true
    }

    pub fn active_hero_can_see(&self, target: Pos) -> bool {
        self.active_hero()
            .is_some_and(|hero| self.can_see(hero.pos, target))
    }

    pub fn has_door(&self, a: Pos, b: Pos) -> Option<&Door> {
        self.doors.iter().find(|door| door.connects(a, b))
    }

    pub fn boundary_is_open(&self, a: Pos, b: Pos) -> bool {
        let Some(a_cell) = self.cell(a) else {
            return false;
        };
        let Some(b_cell) = self.cell(b) else {
            return false;
        };
        if !a_cell.passable || !b_cell.passable {
            return false;
        }
        if let Some(door) = self.has_door(a, b) {
            return door.open;
        }
        a_cell.region == b_cell.region
    }

    fn collapse_delayed_block_after_last_hero(&mut self, hero_id: UnitId, from: Pos, to: Pos) {
        let Some((block, exit)) = self.delayed_falling_block else {
            return;
        };
        if from != block || to != exit || !self.cell(block).is_some_and(|cell| cell.passable) {
            return;
        }
        self.delayed_block_crossers.insert(hero_id);
        let every_living_hero_has_passed = self
            .hero_order
            .iter()
            .copied()
            .filter(|&id| {
                self.unit(id)
                    .is_some_and(|hero| hero.alive && !hero.escaped)
            })
            .all(|id| self.delayed_block_crossers.contains(&id));
        if !every_living_hero_has_passed {
            return;
        }
        self.cells[Self::cell_index(block)].passable = false;
        if let Some(trap) = self
            .traps
            .iter_mut()
            .find(|trap| trap.pos == block && trap.kind == TrapKind::FallingBlock)
        {
            trap.discovered = true;
            trap.sprung = true;
        }
        self.push_log(
            "The special falling block crashes down after the last Hero passes; the route to the stairway is sealed forever."
                .to_owned(),
        );
    }

    pub fn roll_movement_random(&mut self) -> Result<u8, RuleError> {
        let dice: Vec<_> = (0..self.active_movement_dice_count())
            .map(|_| self.rng.random_range(1..=6))
            .collect();
        self.apply_movement_roll(&dice)
    }

    pub fn active_movement_dice_count(&self) -> u8 {
        match self.phase {
            GamePhase::AllyTurn { .. } => 1,
            _ => self
                .active_hero()
                .map(|hero| {
                    let normal = if hero.carried_quest_item.is_some() {
                        1
                    } else if !hero.equipment_available {
                        2
                    } else {
                        hero.effective_movement_dice()
                    };
                    if hero.swift_wind {
                        normal.saturating_mul(2)
                    } else {
                        normal
                    }
                })
                .unwrap_or(2),
        }
    }

    /// Returns the immediately reachable squares that may be selected for the
    /// next one-square movement hop. Keeping this in the rules engine makes
    /// keyboard movement, rendered highlights, and pointer input agree.
    pub fn active_move_targets(&self) -> Vec<(Pos, Direction)> {
        if self.pending_trap_roll.is_some() {
            return Vec::new();
        }
        if let Some(pending) = self.pending_falling_block {
            return Direction::ALL
                .into_iter()
                .filter_map(|direction| {
                    let pos = pending.trap.offset(direction)?;
                    (Some(pos) == pending.back || Some(pos) == pending.ahead)
                        .then_some((pos, direction))
                })
                .collect();
        }
        if self.hero_turn.movement_roll.is_none() || self.hero_turn.movement_left == 0 {
            return Vec::new();
        }
        let Some(mover_id) = self.active_mover_id() else {
            return Vec::new();
        };
        let Some(mover) = self.unit(mover_id) else {
            return Vec::new();
        };
        if mover.sleeping || mover.clouded || mover.petrified_turns > 0 {
            return Vec::new();
        }
        Direction::ALL
            .into_iter()
            .filter_map(|direction| {
                let pos = mover.pos.offset(direction)?;
                let info = self.movement_step_info(mover_id, mover.pos, pos);
                (info.may_end || (self.hero_turn.movement_left > 1 && info.may_enter))
                    .then_some((pos, direction))
            })
            .collect()
    }

    pub fn active_move_direction_to(&self, target: Pos) -> Option<Direction> {
        self.active_move_targets()
            .into_iter()
            .find_map(|(pos, direction)| (pos == target).then_some(direction))
    }

    /// Every legal destination the active mover can reach with the remaining
    /// movement roll. Friendly figures may be crossed but are omitted as end
    /// points, except on a stair or sprung pit square. Unknown squares may be entered
    /// one step deep, but path planning never reveals or routes through hidden
    /// board information.
    pub fn active_move_destinations(&self) -> Vec<Pos> {
        if self.pending_trap_roll.is_some() {
            return Vec::new();
        }
        if let Some(pending) = self.pending_falling_block {
            return [pending.back, pending.ahead]
                .into_iter()
                .flatten()
                .collect();
        }
        let budget = self.hero_turn.movement_left;
        if self.hero_turn.movement_roll.is_none() || budget == 0 {
            return Vec::new();
        }
        let Some(mover_id) = self.active_mover_id() else {
            return Vec::new();
        };
        let Some(mover) = self.unit(mover_id) else {
            return Vec::new();
        };
        if mover.sleeping || mover.clouded || mover.petrified_turns > 0 {
            return Vec::new();
        }

        let start = mover.pos;
        let mut best = vec![u8::MAX; self.cells.len()];
        best[Self::cell_index(start)] = 0;
        let mut frontier = VecDeque::from([(start, 0_u8)]);
        let mut destinations = HashMap::<Pos, u8>::new();

        while let Some((from, spent)) = frontier.pop_front() {
            if spent >= budget {
                continue;
            }
            for direction in Direction::ALL {
                let Some(to) = from.offset(direction) else {
                    continue;
                };
                let info = self.movement_step_info(mover_id, from, to);
                if !info.may_enter {
                    continue;
                }
                let next_spent = spent + 1;
                if to != start && info.may_end {
                    destinations
                        .entry(to)
                        .and_modify(|cost| *cost = (*cost).min(next_spent))
                        .or_insert(next_spent);
                }
                if next_spent < budget && info.may_continue {
                    let index = Self::cell_index(to);
                    if next_spent < best[index] {
                        best[index] = next_spent;
                        frontier.push_back((to, next_spent));
                    }
                }
            }
        }

        let mut destinations = destinations.into_iter().collect::<Vec<_>>();
        destinations.sort_by_key(|(pos, cost)| (*cost, pos.y, pos.x));
        destinations.into_iter().map(|(pos, _)| pos).collect()
    }

    /// Find a shortest legal path to a highlighted movement destination using
    /// A* with Manhattan distance. The returned directions are consumed one
    /// at a time by the app so every normal square-entry rule still fires.
    pub fn active_move_path_to(&self, target: Pos) -> Option<Vec<Direction>> {
        if !self.active_move_destinations().contains(&target) {
            return None;
        }
        let mover_id = self.active_mover_id()?;
        let start = self.unit(mover_id)?.pos;
        let budget = self.hero_turn.movement_left;
        let start_index = Self::cell_index(start);
        let target_index = Self::cell_index(target);
        let mut scores = vec![u8::MAX; self.cells.len()];
        let mut parents = vec![None::<(usize, Direction)>; self.cells.len()];
        let mut frontier = BinaryHeap::new();
        scores[start_index] = 0;
        frontier.push(Reverse((
            manhattan(start, target) as u16,
            0_u8,
            start_index,
        )));

        while let Some(Reverse((_, spent, index))) = frontier.pop() {
            if scores[index] != spent {
                continue;
            }
            if index == target_index {
                break;
            }
            let from = pos_from_cell_index(index);
            for direction in Direction::ALL {
                let Some(to) = from.offset(direction) else {
                    continue;
                };
                let next_spent = spent.saturating_add(1);
                if next_spent > budget {
                    continue;
                }
                let info = self.movement_step_info(mover_id, from, to);
                if !info.may_enter
                    || (to == target && !info.may_end)
                    || (to != target && !info.may_continue)
                {
                    continue;
                }
                let next_index = Self::cell_index(to);
                if next_spent >= scores[next_index] {
                    continue;
                }
                scores[next_index] = next_spent;
                parents[next_index] = Some((index, direction));
                let estimate = next_spent as u16 + manhattan(to, target) as u16;
                frontier.push(Reverse((estimate, next_spent, next_index)));
            }
        }

        if scores[target_index] == u8::MAX {
            return None;
        }
        let mut cursor = target_index;
        let mut reverse_path = Vec::with_capacity(scores[target_index] as usize);
        while cursor != start_index {
            let (parent, direction) = parents[cursor]?;
            reverse_path.push(direction);
            cursor = parent;
        }
        reverse_path.reverse();
        Some(reverse_path)
    }

    fn movement_step_info(&self, mover_id: UnitId, from: Pos, to: Pos) -> MovementStepInfo {
        let Some(mover) = self.unit(mover_id) else {
            return MovementStepInfo {
                may_enter: false,
                may_end: false,
                may_continue: false,
            };
        };
        let magical_door = !self.teleport_destinations.is_empty()
            && self.has_door(from, to).is_some_and(|door| door.open);
        let physical_square = self.cell(to).is_some_and(|cell| cell.passable);
        let furniture_blocks = self.is_furniture_square(to);
        let crosses_open_boundary = mover.pass_through_rock || self.boundary_is_open(from, to);
        let terrain_allows = (mover.pass_through_rock || physical_square) && !furniture_blocks;
        let occupant = self.occupied_by_alive(to, Some(mover_id));
        let may_pass_occupant = occupant.is_some_and(|id| {
            self.unit(id).is_some_and(|unit| {
                (mover.faction == Faction::Hero && unit.faction == Faction::Hero)
                    || (mover.veil_of_mist && unit.faction == Faction::Monster)
            })
        });
        let hero_sharing_exception = self.hero_may_share_square(mover_id, to);
        let may_end =
            physical_square && !furniture_blocks && (occupant.is_none() || hero_sharing_exception);
        let may_enter = magical_door
            || (terrain_allows
                && crosses_open_boundary
                && (occupant.is_none() || may_pass_occupant || hero_sharing_exception));
        let known_stopping_trap = self
            .traps
            .iter()
            .any(|trap| trap.pos == to && trap.discovered && !trap.disarmed);
        let revealed = self.cell(to).is_some_and(|cell| cell.revealed);
        MovementStepInfo {
            may_enter,
            may_end: magical_door || may_end,
            may_continue: may_enter && !magical_door && revealed && !known_stopping_trap,
        }
    }

    pub fn apply_movement_roll(&mut self, dice: &[u8]) -> Result<u8, RuleError> {
        if !matches!(
            self.phase,
            GamePhase::HeroTurn { .. } | GamePhase::AllyTurn { .. }
        ) {
            return Err(RuleError::NotHeroTurn);
        }
        if self.hero_turn.movement_roll.is_some() {
            return Err(RuleError::AlreadyRolled);
        }
        if self
            .active_hero()
            .is_some_and(|hero| hero.sleeping || hero.clouded)
        {
            return Err(RuleError::Incapacitated);
        }
        if dice.len() != self.active_movement_dice_count() as usize
            || dice.iter().any(|face| !(1..=6).contains(face))
        {
            return Err(RuleError::InvalidDice);
        }
        let total = dice.iter().sum();
        self.hero_turn.movement_roll = Some(total);
        self.hero_turn.movement_left = total;
        if let Some(mover) = self.active_mover_id()
            && let Some(hero) = self.units.iter_mut().find(|unit| unit.id == mover)
        {
            hero.swift_wind = false;
        }
        let hero = self
            .active_mover_id()
            .and_then(|id| self.unit(id))
            .map(|unit| unit.name.clone())
            .unwrap_or_default();
        let faces = dice
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(" + ");
        self.push_log(format!("{hero} rolled {faces} = {total} movement."));
        Ok(total)
    }

    fn finish_spell_movement(&mut self, hero_id: UnitId) {
        let Some(index) = self.units.iter().position(|unit| unit.id == hero_id) else {
            return;
        };
        let used_pass_through_rock = self.units[index].pass_through_rock;
        let used_veil_of_mist = self.units[index].veil_of_mist;
        self.units[index].pass_through_rock = false;
        self.units[index].veil_of_mist = false;
        if used_pass_through_rock
            && !self
                .cell(self.units[index].pos)
                .is_some_and(|cell| cell.passable)
        {
            let name = self.units[index].name.clone();
            self.units[index].body = 0;
            self.units[index].alive = false;
            self.drop_carried_quest_item_at(hero_id);
            self.push_log(format!(
                "{name} ended Pass Through Rock inside solid stone and is trapped forever."
            ));
        } else if used_pass_through_rock || used_veil_of_mist {
            let name = self.units[index].name.clone();
            self.push_log(format!("{name}'s movement spell ended."));
        }
    }

    fn break_courage_without_visible_monster(&mut self, hero_id: UnitId) {
        if !self
            .unit(hero_id)
            .is_some_and(|hero| hero.courage && !self.unit_can_see_monster(hero_id))
        {
            return;
        }
        let hero = self
            .units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .expect("validated Hero exists");
        hero.courage = false;
        let name = hero.name.clone();
        self.push_log(format!(
            "Courage left {name} when no Monster remained in sight."
        ));
    }

    pub fn move_active(&mut self, direction: Direction) -> Result<Pos, RuleError> {
        if self.pending_trap_roll.is_some() {
            return Err(RuleError::TrapRollPending);
        }
        if self.pending_falling_block.is_some() {
            return self.resolve_falling_block_escape(direction);
        }
        if self.pending_collapsing_ceiling_roll.is_some() {
            return Err(RuleError::CollapsingCeilingRollPending);
        }
        if self.pending_teleport_roll.is_some() {
            return Err(RuleError::StaleTeleport);
        }
        let hero_id = self.active_mover_id().ok_or(RuleError::NotHeroTurn)?;
        if self.hero_turn.movement_roll.is_none() {
            return Err(RuleError::MovementNotRolled);
        }
        if self.hero_turn.movement_left == 0 {
            return Err(RuleError::NoMovement);
        }
        let from = self.unit(hero_id).expect("active hero exists").pos;
        let to = from.offset(direction).ok_or(RuleError::OutsideBoard)?;
        let crossed_door = self.has_door(from, to);
        if crossed_door.is_some_and(|door| !door.open) {
            return Err(RuleError::ClosedDoor);
        }
        let crosses_magical_door = !self.teleport_destinations.is_empty() && crossed_door.is_some();
        if crosses_magical_door {
            self.hero_turn.movement_left = 0;
            self.hero_turn.moved_steps = self.hero_turn.moved_steps.saturating_add(1);
            self.hero_turn.door_passed = true;
            self.pending_teleport_roll = Some(PendingTeleportRoll {
                subject: hero_id,
                forbidden_destination: None,
            });
            let hero_name = self.unit(hero_id).expect("active Hero exists").name.clone();
            self.push_log(format!(
                "{hero_name} passed through a magical door and must roll two red dice."
            ));
            return Ok(from);
        }
        let (pass_through_rock, veil_of_mist) = self
            .unit(hero_id)
            .map(|hero| (hero.pass_through_rock, hero.veil_of_mist))
            .unwrap_or_default();
        if !pass_through_rock && !self.cell(to).is_some_and(|cell| cell.passable) {
            return Err(RuleError::Blocked);
        }
        if !pass_through_rock && !self.boundary_is_open(from, to) {
            return Err(RuleError::Blocked);
        }
        if self.is_furniture_square(to) {
            return Err(RuleError::Blocked);
        }
        if let Some(occupant) = self.occupied_by_alive(to, Some(hero_id)) {
            let passing_figure = self.hero_may_share_square(hero_id, to)
                || (self.hero_turn.movement_left > 1
                    && self.unit(occupant).is_some_and(|unit| {
                        unit.faction == Faction::Hero
                            || (veil_of_mist && unit.faction == Faction::Monster)
                    }));
            if !passing_figure {
                return Err(RuleError::Occupied);
            }
        }

        let hero_name = {
            let hero = self
                .units
                .iter_mut()
                .find(|unit| unit.id == hero_id)
                .expect("active hero exists");
            hero.pos = to;
            hero.in_pit = false;
            hero.name.clone()
        };
        self.hero_turn.movement_left -= 1;
        self.hero_turn.moved_steps += 1;
        if let Some(item_index) = self.unit(hero_id).and_then(|hero| hero.carried_quest_item) {
            let prop_index = self.quest_items[item_index].prop_index;
            self.props[prop_index].pos = to;
        }
        self.reveal_from(to);
        self.restore_recovered_equipment(hero_id);
        self.push_log(format!("{hero_name} moved to {}, {}.", to.x + 1, to.y + 1));
        self.trigger_square_trap(hero_id, from, to, direction, false);
        self.trigger_collapsing_ceiling_hazard(hero_id, to);
        self.collapse_delayed_block_after_last_hero(hero_id, from, to);
        self.deliver_carried_quest_item(hero_id);
        if self.hero_turn.movement_left == 0 {
            self.finish_spell_movement(hero_id);
        }
        self.break_courage_without_visible_monster(hero_id);
        if self.escape_hero_on_stairs(hero_id) {
            self.check_terminal();
            if !matches!(
                self.phase,
                GamePhase::Won | GamePhase::Retreated | GamePhase::Lost
            ) {
                self.end_hero_turn()?;
            }
            return Ok(to);
        }
        self.check_terminal();
        Ok(to)
    }

    fn trigger_collapsing_ceiling_hazard(&mut self, hero_id: UnitId, pos: Pos) {
        if !self.collapsing_ceiling_hazards.contains(&pos)
            || !self
                .unit(hero_id)
                .is_some_and(|hero| hero.alive && hero.faction == Faction::Hero)
        {
            return;
        }
        self.pending_collapsing_ceiling_roll = Some(hero_id);
        let hero_name = self.unit(hero_id).expect("hazard Hero exists").name.clone();
        self.push_log(format!(
            "Loose stone rains down above {hero_name}; roll one physical red die."
        ));
    }

    pub fn pending_collapsing_ceiling_subject(&self) -> Option<UnitId> {
        self.pending_collapsing_ceiling_roll
    }

    /// Resolves Quest 13's reusable falling-rock hazard. A Helmet reduces the
    /// damaging faces from 4–6 to only 6; unlike an ordinary falling block,
    /// the square remains open and movement may continue.
    pub fn resolve_collapsing_ceiling_roll(
        &mut self,
        hero_id: UnitId,
        die: u8,
    ) -> Result<bool, RuleError> {
        if !(1..=6).contains(&die) {
            return Err(RuleError::InvalidDice);
        }
        if self.pending_collapsing_ceiling_roll != Some(hero_id) {
            return Err(RuleError::StaleCollapsingCeilingRoll);
        }
        self.pending_collapsing_ceiling_roll = None;
        let (hero_name, helmet) = self
            .unit(hero_id)
            .map(|hero| {
                (
                    hero.name.clone(),
                    hero.equipment_available && hero.inventory.armor.contains(&Armor::Helmet),
                )
            })
            .ok_or(RuleError::StaleCollapsingCeilingRoll)?;
        let hit = die == 6 || (!helmet && die >= 4);
        if hit {
            self.damage_without_defense(hero_id, 1);
            self.push_log(format!(
                "{hero_name} rolled {die}; falling stone inflicts 1 Body Point with no defense."
            ));
            if !self.unit(hero_id).is_some_and(|hero| hero.alive) {
                self.hero_turn.movement_left = 0;
                self.end_forced_hero_turn();
            } else {
                self.check_terminal();
            }
        } else {
            let protection = if helmet {
                "; the Helmet protects the Hero"
            } else {
                ""
            };
            self.push_log(format!(
                "{hero_name} rolled {die}{protection}; no Body Point is lost and movement may continue."
            ));
        }
        Ok(hit)
    }

    pub fn pending_teleport_subject(&self) -> Option<UnitId> {
        self.pending_teleport_roll.map(|pending| pending.subject)
    }

    pub fn resolve_teleport_roll(&mut self, dice: &[u8]) -> Result<Pos, RuleError> {
        if dice.len() != 2 || dice.iter().any(|face| !(1..=6).contains(face)) {
            return Err(RuleError::InvalidDice);
        }
        let pending = self.pending_teleport_roll.ok_or(RuleError::StaleTeleport)?;
        let total = dice.iter().sum::<u8>();
        let destination = *self
            .teleport_destinations
            .get(&total)
            .ok_or(RuleError::StaleTeleport)?;
        let subject_name = self
            .unit(pending.subject)
            .filter(|unit| unit.alive && !unit.escaped)
            .map(|unit| unit.name.clone())
            .ok_or(RuleError::StaleTeleport)?;
        if pending.forbidden_destination == Some(destination) {
            self.push_log(format!(
                "{subject_name} rolled {total}, the occupied square just left; roll again."
            ));
            return Ok(destination);
        }

        let landed_on = self.occupied_by_alive(destination, Some(pending.subject));
        self.pending_teleport_roll = None;
        let subject_index = self
            .units
            .iter()
            .position(|unit| unit.id == pending.subject)
            .ok_or(RuleError::StaleTeleport)?;
        self.units[subject_index].pos = destination;
        self.units[subject_index].in_pit = false;
        if let Some(item_index) = self.units[subject_index].carried_quest_item {
            self.props[self.quest_items[item_index].prop_index].pos = destination;
        }
        if self.units[subject_index].faction == Faction::Hero {
            self.reveal_from(destination);
        }
        self.push_log(format!(
            "{subject_name} rolled {total} and teleported to {}, {}.",
            destination.x + 1,
            destination.y + 1
        ));

        if let Some(displaced) = landed_on {
            let displaced_name = self
                .unit(displaced)
                .map(|unit| unit.name.clone())
                .unwrap_or_else(|| "The landed-on figure".to_owned());
            self.damage_without_defense(displaced, 1);
            let displaced_alive = self.unit(displaced).is_some_and(|unit| unit.alive);
            self.visual_sequence = self.visual_sequence.wrapping_add(1);
            self.last_combat_visual = Some(CombatVisualEvent {
                sequence: self.visual_sequence,
                attacker: pending.subject,
                defender: displaced,
                damage: 1,
                defender_died: !displaced_alive,
            });
            if displaced_alive {
                self.pending_teleport_roll = Some(PendingTeleportRoll {
                    subject: displaced,
                    forbidden_destination: Some(destination),
                });
                self.push_log(format!(
                    "{displaced_name} loses 1 Body Point and must roll two red dice to be displaced."
                ));
            } else {
                self.push_log(format!(
                    "{displaced_name} loses 1 Body Point and is destroyed by the collision."
                ));
            }
        }
        self.check_terminal();
        Ok(destination)
    }

    pub fn open_adjacent_door(&mut self) -> Result<(Pos, Pos), RuleError> {
        let door_index = *self
            .adjacent_closed_door_indices()?
            .first()
            .ok_or(RuleError::NoDoor)?;
        self.open_selected_adjacent_door(door_index)
    }

    /// Returns every physical closed door the active Hero may choose to open.
    /// The indices are stable for the quest and can be carried by the UI while
    /// it highlights the square on the far side of each doorway.
    pub fn adjacent_closed_door_indices(&self) -> Result<Vec<usize>, RuleError> {
        let hero_id = self.active_awake_hero_id()?;
        let hero_pos = self.unit(hero_id).expect("active hero exists").pos;
        let doors = self
            .doors
            .iter()
            .enumerate()
            .filter_map(|(index, door)| {
                (!door.open && (!door.secret || door.discovered) && door.touches(hero_pos))
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        (!doors.is_empty())
            .then_some(doors)
            .ok_or(RuleError::NoDoor)
    }

    /// Opens the chosen adjacent door. This revalidates the selection so a
    /// stale pointer cannot open a remote door after the Hero has moved.
    pub fn open_selected_adjacent_door(
        &mut self,
        door_index: usize,
    ) -> Result<(Pos, Pos), RuleError> {
        let hero_id = self.active_awake_hero_id()?;
        let hero_pos = self.unit(hero_id).expect("active hero exists").pos;
        let door = self.doors.get(door_index).ok_or(RuleError::NoDoor)?;
        if door.open || (door.secret && !door.discovered) || !door.touches(hero_pos) {
            return Err(RuleError::NoDoor);
        }
        if door.false_door {
            return Err(RuleError::FalseDoor);
        }
        let (a, b) = self.open_door_index(door_index)?;
        let hero_name = self.unit(hero_id).expect("active hero exists").name.clone();
        self.push_log(format!("{hero_name} opened a door."));
        Ok((a, b))
    }

    fn open_door_index(&mut self, door_index: usize) -> Result<(Pos, Pos), RuleError> {
        let door = self.doors.get(door_index).ok_or(RuleError::NoDoor)?;
        if door.open {
            return Err(RuleError::NoDoor);
        }
        if door.false_door {
            return Err(RuleError::FalseDoor);
        }
        if door.secret && !door.discovered {
            return Err(RuleError::InvalidHeroSpellTarget);
        }
        let (a, b) = (door.a, door.b);
        self.doors[door_index].open = true;
        self.reveal_from(a);
        self.reveal_from(b);
        self.resolve_open_door_events(a, b);
        Ok((a, b))
    }

    /// Installs an exact draw order for a deterministic replay. Normal games
    /// use the shuffled physical 24-card deck created with the quest.
    pub fn set_treasure_replay_order(&mut self, cards: impl IntoIterator<Item = TreasureCard>) {
        self.treasure_deck = TreasureDeck::from_draw_order(cards);
    }

    pub fn search_treasure(&mut self) -> Result<TreasureSearchOutcome, RuleError> {
        if self.hero_turn.action_used {
            return Err(RuleError::AlreadyActed);
        }
        let hero_id = self.active_awake_hero_id()?;
        let (hero_pos, hero_in_pit) = self
            .unit(hero_id)
            .map(|hero| (hero.pos, hero.in_pit))
            .expect("active hero exists");
        let board_region = self.cell(hero_pos).map(|cell| cell.region);
        let search_region = if hero_in_pit {
            // A sprung pit is a separate searchable room, even when its board
            // square lies in a corridor or inside a larger room.
            -2 - Self::cell_index(hero_pos) as i16
        } else {
            board_region
                .filter(|&region| region > 0)
                .ok_or(RuleError::TreasureOnlyInRoom)?
        };
        if !hero_in_pit
            && self.units.iter().any(|unit| {
                unit.alive
                    && unit.faction == Faction::Monster
                    && !unit.dormant
                    && self
                        .cell(unit.pos)
                        .is_some_and(|cell| cell.region == search_region)
            })
        {
            return Err(RuleError::MonstersInRoom);
        }
        if !hero_in_pit && self.forbidden_treasure_regions.contains(&search_region) {
            return Err(RuleError::TreasureForbidden);
        }
        if !hero_in_pit
            && self.quest_events.iter().any(|event| {
                !event.resolved
                    && matches!(
                        &event.trigger,
                        QuestTrigger::SearchTreasureAfterDefeat {
                            region: event_region,
                            name,
                        } if *event_region == search_region
                            && !self.all_named_units_defeated(name)
                    )
            })
        {
            return Err(RuleError::QuestConditionNotMet);
        }
        if !self.searched_treasure.insert((hero_id, search_region)) {
            return Err(RuleError::AlreadySearchedRoom);
        }

        self.hero_turn.action_used = true;
        if self.hero_turn.moved_steps > 0 {
            self.hero_turn.movement_left = 0;
        }
        let matching_events: Vec<_> = self
            .quest_events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| {
                if event.resolved {
                    return None;
                }
                let matches = match &event.trigger {
                    QuestTrigger::SearchTreasure {
                        region: event_region,
                    } => *event_region == search_region,
                    QuestTrigger::SearchTreasureAfterDefeat {
                        region: event_region,
                        name,
                    } => *event_region == search_region && self.all_named_units_defeated(name),
                    _ => false,
                };
                matches.then_some(index)
            })
            .collect();
        if !matching_events.is_empty() {
            let sprung_traps_before = self.traps.iter().filter(|trap| trap.sprung).count();
            let events: Vec<_> = matching_events
                .into_iter()
                .map(|event_index| {
                    let event = &mut self.quest_events[event_index];
                    event.resolved = true;
                    (
                        event.id.clone(),
                        event.effect.clone(),
                        event.message.clone(),
                    )
                })
                .collect();
            let hero_name = self.unit(hero_id).expect("active hero exists").name.clone();
            let discovery = if events.len() == 1 {
                match events[0].1 {
                    QuestEffectDef::Gold { amount } => TreasureDiscovery::Gold(amount),
                    QuestEffectDef::Empty | QuestEffectDef::Message => TreasureDiscovery::Empty,
                    _ => TreasureDiscovery::QuestEvent,
                }
            } else {
                TreasureDiscovery::QuestEvent
            };
            for (event_id, effect, message) in events {
                self.apply_quest_search_effect(hero_id, &effect);
                if let Some(message) = message {
                    self.push_log(message);
                } else {
                    match effect {
                        QuestEffectDef::Gold { amount } => {
                            self.push_log(format!("{hero_name} found {amount} gold coins."));
                        }
                        QuestEffectDef::Empty | QuestEffectDef::Message => {
                            self.push_log(format!(
                                "{hero_name} found nothing useful ({event_id})."
                            ));
                        }
                        _ => {}
                    }
                }
            }
            if self.traps.iter().filter(|trap| trap.sprung).count() > sprung_traps_before {
                self.hero_turn.movement_left = 0;
                self.push_log(
                    "The chest/furniture trap was sprung; the searching Hero's turn ends."
                        .to_owned(),
                );
                self.end_forced_hero_turn();
            }
            self.check_terminal();
            return Ok(TreasureSearchOutcome {
                discovery,
                wandering_monster: None,
            });
        }
        if !hero_in_pit && let Some(artifact) = self.lost_artifact_treasure.pop_front() {
            let inventory = &mut self
                .units
                .iter_mut()
                .find(|unit| unit.id == hero_id)
                .expect("active hero exists")
                .inventory;
            if !inventory.artifacts.contains(&artifact) {
                inventory.artifacts.push(artifact);
            }
            let hero_name = self.unit(hero_id).expect("active hero exists").name.clone();
            self.push_log(format!(
                "{hero_name} found the lost {} as special treasure; the Treasure deck was not drawn.",
                artifact.name()
            ));
            self.check_terminal();
            return Ok(TreasureSearchOutcome {
                discovery: TreasureDiscovery::Artifact(artifact),
                wandering_monster: None,
            });
        }
        let card = self
            .treasure_deck
            .draw(&mut self.rng)
            .ok_or(RuleError::EmptyTreasureDeck)?;
        let mut wandering_monster = None;
        let hero_name = self.unit(hero_id).expect("active hero exists").name.clone();

        if let Some(gold) = card.gold() {
            let inventory = &mut self
                .units
                .iter_mut()
                .find(|unit| unit.id == hero_id)
                .expect("active hero exists")
                .inventory;
            inventory.gold = inventory.gold.saturating_add(gold);
            self.push_log(format!(
                "{hero_name} searched the room and found {} worth {gold} gold coins.",
                card.name()
            ));
        } else {
            match card {
                TreasureCard::HeroicBrew => {
                    self.inventory_mut(hero_id).heroic_brew += 1;
                    self.push_log(format!("{hero_name} found a Heroic Brew."));
                }
                TreasureCard::PotionOfDefense => {
                    self.inventory_mut(hero_id).potion_of_defense += 1;
                    self.push_log(format!("{hero_name} found a Potion of Defense."));
                }
                TreasureCard::PotionOfHealing => {
                    let inventory = self.inventory_mut(hero_id);
                    inventory.potion_of_healing += 1;
                    // Zero denotes the original Treasure-card potion whose
                    // healing is determined by one physical red die.
                    inventory.healing_potion_strengths.push(0);
                    self.push_log(format!("{hero_name} found a Potion of Healing."));
                }
                TreasureCard::PotionOfStrength => {
                    self.inventory_mut(hero_id).potion_of_strength += 1;
                    self.push_log(format!("{hero_name} found a Potion of Strength."));
                }
                TreasureCard::ArrowHazard => {
                    self.damage_without_defense(hero_id, 1);
                    self.hero_turn.movement_left = 0;
                    self.push_log(format!(
                        "A hidden arrow struck {hero_name} for 1 Body Point; the turn ends."
                    ));
                    self.end_forced_hero_turn();
                }
                TreasureCard::PitHazard => {
                    self.damage_without_defense(hero_id, 1);
                    self.hero_turn.movement_left = 0;
                    self.push_log(format!(
                        "The floor gave way under {hero_name}: 1 Body Point lost; the turn ends."
                    ));
                    self.end_forced_hero_turn();
                }
                TreasureCard::WanderingMonster => {
                    if let Some(message) = self.wandering_event_message.clone() {
                        self.push_log(message);
                    } else {
                        wandering_monster = self.spawn_wandering_monster(hero_id);
                        if let Some(monster_id) = wandering_monster {
                            let monster_name = self
                                .unit(monster_id)
                                .expect("spawned monster exists")
                                .name
                                .clone();
                            self.push_log(format!(
                                "A wandering {monster_name} appeared beside {hero_name}!"
                            ));
                            if self
                                .unit(monster_id)
                                .zip(self.unit(hero_id))
                                .is_some_and(|(monster, hero)| monster.pos.is_adjacent(hero.pos))
                            {
                                let plan = self
                                    .monster_attack_plan(monster_id, hero_id)
                                    .expect("a freshly spawned monster and searcher are valid");
                                self.pending_forced_attack = Some(plan);
                                self.push_log(format!(
                                    "The wandering {monster_name} immediately attacks {hero_name}; physical combat dice must be rolled."
                                ));
                            }
                        }
                    }
                }
                TreasureCard::Gem35
                | TreasureCard::GoldCoins15
                | TreasureCard::Jewels25
                | TreasureCard::Jewels50 => unreachable!("gold cards handled above"),
            }
        }
        self.check_terminal();
        Ok(TreasureSearchOutcome {
            discovery: TreasureDiscovery::Card(card),
            wandering_monster,
        })
    }

    fn all_named_units_defeated(&self, name: &str) -> bool {
        let mut matching = self
            .units
            .iter()
            .filter(|unit| unit.name == name)
            .peekable();
        matching.peek().is_some() && matching.all(|unit| !unit.alive)
    }

    /// Begins drinking a Potion of Healing without consuming an action. Quest
    /// potions with a printed fixed value resolve immediately; the original
    /// Treasure-card potion requests one physical red die from the Hero's rack.
    pub fn begin_active_healing_potion(&mut self) -> Result<HealingPotionUse, RuleError> {
        let hero_id = self.active_awake_hero_id()?;
        let hero = self.unit(hero_id).expect("active hero exists");
        if hero.body >= hero.stats.body as i16 {
            return Err(RuleError::FullBody);
        }
        self.consume_healing_potion(hero_id)
    }

    pub fn drink_heroic_brew(&mut self) -> Result<(), RuleError> {
        let hero_id = self.active_awake_hero_id()?;
        if self.hero_turn.action_used
            && !self.hero_turn.orcs_bane_follow_up
            && !self.hero_turn.heroic_brew_follow_up
        {
            return Err(RuleError::AlreadyActed);
        }
        self.active_attack_options()?;
        let hero = self.unit(hero_id).expect("active Hero exists");
        if !hero.equipment_available {
            return Err(RuleError::EquipmentUnavailable);
        }
        if hero.inventory.heroic_brew == 0 {
            return Err(RuleError::NoHeroicBrew);
        }
        if self.hero_turn.heroic_brew_ready || self.hero_turn.heroic_brew_follow_up {
            return Err(RuleError::PotionAlreadyActive);
        }
        let hero = self
            .units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .expect("validated Hero exists");
        hero.inventory.heroic_brew -= 1;
        let name = hero.name.clone();
        self.hero_turn.heroic_brew_ready = true;
        self.push_log(format!(
            "{name} drank a Heroic Brew and may make two attacks with the next attack action."
        ));
        Ok(())
    }

    pub fn drink_potion_of_strength(&mut self) -> Result<(), RuleError> {
        let hero_id = self.active_awake_hero_id()?;
        if self.hero_turn.action_used
            && !self.hero_turn.orcs_bane_follow_up
            && !self.hero_turn.heroic_brew_follow_up
        {
            return Err(RuleError::AlreadyActed);
        }
        self.active_attack_options()?;
        let hero = self.unit(hero_id).expect("active Hero exists");
        if !hero.equipment_available {
            return Err(RuleError::EquipmentUnavailable);
        }
        if hero.inventory.potion_of_strength == 0 {
            return Err(RuleError::NoPotionOfStrength);
        }
        let hero = self
            .units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .expect("validated Hero exists");
        hero.inventory.potion_of_strength -= 1;
        let name = hero.name.clone();
        self.hero_turn.potion_strength_bonus =
            self.hero_turn.potion_strength_bonus.saturating_add(2);
        self.push_log(format!(
            "{name} drank a Potion of Strength; the next attack gains two combat dice."
        ));
        Ok(())
    }

    pub fn drink_potion_of_defense_for(&mut self, hero_id: UnitId) -> Result<(), RuleError> {
        let hero = self.unit(hero_id).ok_or(RuleError::NoPotionOfDefense)?;
        if !hero.alive || hero.escaped || hero.faction != Faction::Hero {
            return Err(RuleError::NoPotionOfDefense);
        }
        if !hero.equipment_available {
            return Err(RuleError::EquipmentUnavailable);
        }
        if hero.inventory.potion_of_defense == 0 {
            return Err(RuleError::NoPotionOfDefense);
        }
        let hero = self
            .units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .expect("validated Hero exists");
        hero.inventory.potion_of_defense -= 1;
        hero.potion_defense_bonus = hero.potion_defense_bonus.saturating_add(2);
        let name = hero.name.clone();
        self.push_log(format!(
            "{name} drank a Potion of Defense; the next defense gains two combat dice."
        ));
        Ok(())
    }

    pub fn drink_active_potion_of_defense(&mut self) -> Result<(), RuleError> {
        let hero_id = self.active_awake_hero_id()?;
        self.drink_potion_of_defense_for(hero_id)
    }

    pub fn refresh_attack_defense_dice(
        &self,
        mut plan: AttackPlan,
    ) -> Result<AttackPlan, RuleError> {
        let defender = self.unit(plan.defender).ok_or(RuleError::StaleAttack)?;
        plan.defend_dice = pit_adjusted_dice(defender.effective_defense_dice(), defender.in_pit);
        Ok(plan)
    }

    pub fn active_owned_potion_kinds(&self) -> Vec<PotionKind> {
        let Some(hero) = self.active_hero() else {
            return Vec::new();
        };
        let mut potions = Vec::new();
        if hero.inventory.heroic_brew > 0 {
            potions.push(PotionKind::HeroicBrew);
        }
        if hero.inventory.potion_of_defense > 0 {
            potions.push(PotionKind::Defense);
        }
        if hero.inventory.potion_of_healing > 0 {
            potions.push(PotionKind::Healing);
        }
        if hero.inventory.potion_of_strength > 0 {
            potions.push(PotionKind::Strength);
        }
        if hero.inventory.petrification_potion > 0 {
            potions.push(PotionKind::Petrification);
        }
        potions
    }

    pub fn living_other_heroes(&self) -> Vec<UnitId> {
        let giver = self.active_hero_id();
        self.hero_order
            .iter()
            .copied()
            .filter(|&hero_id| {
                Some(hero_id) != giver
                    && self
                        .unit(hero_id)
                        .is_some_and(|hero| hero.alive && !hero.escaped)
            })
            .collect()
    }

    pub fn give_active_potion(
        &mut self,
        recipient: UnitId,
        potion: PotionKind,
    ) -> Result<(), RuleError> {
        let giver = self.active_awake_hero_id()?;
        if !self.living_other_heroes().contains(&recipient) {
            return Err(RuleError::NoGiftRecipient);
        }
        let giver_index = self
            .units
            .iter()
            .position(|unit| unit.id == giver)
            .ok_or(RuleError::NotHeroTurn)?;
        let recipient_index = self
            .units
            .iter()
            .position(|unit| unit.id == recipient)
            .ok_or(RuleError::NoGiftRecipient)?;
        if !self.units[giver_index].equipment_available {
            return Err(RuleError::EquipmentUnavailable);
        }
        let healing_strength = match potion {
            PotionKind::HeroicBrew => {
                decrement_owned(&mut self.units[giver_index].inventory.heroic_brew)?;
                None
            }
            PotionKind::Defense => {
                decrement_owned(&mut self.units[giver_index].inventory.potion_of_defense)?;
                None
            }
            PotionKind::Healing => {
                decrement_owned(&mut self.units[giver_index].inventory.potion_of_healing)?;
                Some(
                    self.units[giver_index]
                        .inventory
                        .healing_potion_strengths
                        .pop()
                        .unwrap_or(0),
                )
            }
            PotionKind::Strength => {
                decrement_owned(&mut self.units[giver_index].inventory.potion_of_strength)?;
                None
            }
            PotionKind::Petrification => {
                decrement_owned(&mut self.units[giver_index].inventory.petrification_potion)?;
                None
            }
        };
        match potion {
            PotionKind::HeroicBrew => {
                self.units[recipient_index].inventory.heroic_brew += 1;
            }
            PotionKind::Defense => {
                self.units[recipient_index].inventory.potion_of_defense += 1;
            }
            PotionKind::Healing => {
                self.units[recipient_index].inventory.potion_of_healing += 1;
                self.units[recipient_index]
                    .inventory
                    .healing_potion_strengths
                    .push(healing_strength.unwrap_or(0));
            }
            PotionKind::Strength => {
                self.units[recipient_index].inventory.potion_of_strength += 1;
            }
            PotionKind::Petrification => {
                self.units[recipient_index].inventory.petrification_potion += 1;
            }
        }
        let giver_name = self.units[giver_index].name.clone();
        let recipient_name = self.units[recipient_index].name.clone();
        self.push_log(format!(
            "{giver_name} gave {} to {recipient_name}.",
            potion.name()
        ));
        Ok(())
    }

    pub fn give_active_gold(&mut self, recipient: UnitId, amount: u16) -> Result<(), RuleError> {
        let giver = self.active_awake_hero_id()?;
        if !self.living_other_heroes().contains(&recipient) {
            return Err(RuleError::NoGiftRecipient);
        }
        let giver_index = self
            .units
            .iter()
            .position(|unit| unit.id == giver)
            .ok_or(RuleError::NotHeroTurn)?;
        let recipient_index = self
            .units
            .iter()
            .position(|unit| unit.id == recipient)
            .ok_or(RuleError::NoGiftRecipient)?;
        if !self.units[giver_index].equipment_available {
            return Err(RuleError::EquipmentUnavailable);
        }
        let available = self.units[giver_index].inventory.gold;
        if amount == 0 || amount > available {
            return Err(RuleError::InvalidGoldTransfer);
        }
        self.units[giver_index].inventory.gold -= amount;
        self.units[recipient_index].inventory.gold = self.units[recipient_index]
            .inventory
            .gold
            .saturating_add(amount);
        let giver_name = self.units[giver_index].name.clone();
        let recipient_name = self.units[recipient_index].name.clone();
        self.push_log(format!(
            "{giver_name} gave {amount} gold coins to {recipient_name}."
        ));
        Ok(())
    }

    /// Deterministic convenience used by engine tests and non-animated hosts.
    /// The SDL game uses `begin_active_healing_potion` and shows the red die.
    pub fn use_healing_potion(&mut self) -> Result<u8, RuleError> {
        match self.begin_active_healing_potion()? {
            HealingPotionUse::Restored { body, .. } => Ok(body),
            HealingPotionUse::RollRedDie { hero } => {
                let face = self.rng.random_range(1..=6);
                self.resolve_healing_potion_roll(hero, face)
            }
        }
    }

    fn consume_healing_potion(&mut self, hero_id: UnitId) -> Result<HealingPotionUse, RuleError> {
        let hero = self.unit(hero_id).ok_or(RuleError::NoHealingPotion)?;
        if !hero.equipment_available {
            return Err(RuleError::EquipmentUnavailable);
        }
        if hero.inventory.potion_of_healing == 0 {
            return Err(RuleError::NoHealingPotion);
        }
        let restore_limit = hero
            .inventory
            .healing_potion_strengths
            .last()
            .copied()
            .unwrap_or(0);
        let hero = self
            .units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .expect("validated Hero exists");
        hero.inventory.potion_of_healing -= 1;
        hero.inventory.healing_potion_strengths.pop();
        if restore_limit == 0 {
            self.pending_healing_potion_roll = Some(hero_id);
            let hero_name = hero.name.clone();
            self.push_log(format!(
                "{hero_name} drinks a Potion of Healing; one physical red die determines the Body Points restored."
            ));
            return Ok(HealingPotionUse::RollRedDie { hero: hero_id });
        }
        let restored = (hero.stats.body as i16 - hero.body).min(restore_limit as i16) as u8;
        hero.body += restored as i16;
        hero.alive = hero.body > 0;
        let hero_name = hero.name.clone();
        self.push_log(format!(
            "{hero_name} drank a Potion of Healing and restored {restored} Body Points."
        ));
        Ok(HealingPotionUse::Restored {
            hero: hero_id,
            body: restored,
        })
    }

    pub fn resolve_healing_potion_roll(
        &mut self,
        hero_id: UnitId,
        face: u8,
    ) -> Result<u8, RuleError> {
        if self.pending_healing_potion_roll != Some(hero_id) {
            return Err(RuleError::StaleHealingPotionRoll);
        }
        if !(1..=6).contains(&face) {
            return Err(RuleError::InvalidDice);
        }
        let hero = self
            .units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .ok_or(RuleError::StaleHealingPotionRoll)?;
        let restored = (hero.stats.body as i16 - hero.body).min(face as i16) as u8;
        hero.body += restored as i16;
        hero.alive = hero.body > 0;
        let hero_name = hero.name.clone();
        self.pending_healing_potion_roll = None;
        if self
            .pending_hero_death
            .is_some_and(|pending| pending.hero == hero_id)
        {
            self.pending_hero_death = None;
        }
        self.push_log(format!(
            "{hero_name}'s Potion of Healing rolled {face} and restored {restored} Body Points."
        ));
        Ok(restored)
    }

    /// Drinks the unknown purple liquid from The Lost Wizard. The Hero turns
    /// to invulnerable stone immediately and misses their next five turns.
    pub fn use_petrification_potion(&mut self) -> Result<(), RuleError> {
        let hero_id = self.active_awake_hero_id()?;
        let hero = self.unit(hero_id).expect("active hero exists");
        if !hero.equipment_available {
            return Err(RuleError::EquipmentUnavailable);
        }
        if hero.inventory.petrification_potion == 0 {
            return Err(RuleError::NoPetrificationPotion);
        }
        let hero_name = hero.name.clone();
        let hero = self
            .units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .expect("active hero exists");
        hero.inventory.petrification_potion -= 1;
        hero.petrified_turns = 5;
        self.hero_turn.action_used = true;
        self.hero_turn.movement_left = 0;
        self.push_log(format!(
            "{hero_name} drank the purple liquid and turned to invulnerable stone for five turns."
        ));
        Ok(())
    }

    pub fn can_take_fools_gold(&self) -> bool {
        let Some(hero) = self.active_hero() else {
            return false;
        };
        self.mine_region.is_some_and(|region| {
            hero.alive
                && !hero.escaped
                && hero.inventory.fools_gold == 0
                && self
                    .cell(hero.pos)
                    .is_some_and(|cell| cell.region == region)
        })
    }

    pub fn take_fools_gold(&mut self) -> Result<u16, RuleError> {
        let hero_id = self.active_awake_hero_id()?;
        let hero = self.unit(hero_id).expect("active Hero exists");
        if !self.mine_region.is_some_and(|region| {
            self.cell(hero.pos)
                .is_some_and(|cell| cell.region == region)
        }) {
            return Err(RuleError::NotAtMine);
        }
        if hero.inventory.fools_gold > 0 {
            return Err(RuleError::AlreadyCarryingFoolsGold);
        }
        let amount = self.mine_gold_amount;
        let hero = self
            .units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .expect("active Hero exists");
        hero.inventory.fools_gold = amount;
        let name = hero.name.clone();
        self.push_log(format!(
            "{name} took {amount} gold coins from the mine and can no longer attack or defend."
        ));
        Ok(amount)
    }

    pub fn drop_fools_gold(&mut self) -> Result<u16, RuleError> {
        let hero_id = self.active_awake_hero_id()?;
        let hero = self
            .units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .expect("active Hero exists");
        if hero.inventory.fools_gold == 0 {
            return Err(RuleError::NotCarryingFoolsGold);
        }
        let amount = std::mem::take(&mut hero.inventory.fools_gold);
        let name = hero.name.clone();
        self.push_log(format!(
            "{name} put down the mine's {amount} gold coins; they vanished back into the mine."
        ));
        Ok(amount)
    }

    pub fn elixir_of_life_targets(&self) -> Vec<UnitId> {
        let Some(owner) = self.active_hero() else {
            return Vec::new();
        };
        if !owner.alive
            || owner.escaped
            || owner.sleeping
            || owner.clouded
            || owner.petrified_turns > 0
            || !owner.equipment_available
            || !owner.inventory.artifacts.contains(&Artifact::ElixirOfLife)
        {
            return Vec::new();
        }
        self.hero_order
            .iter()
            .copied()
            .filter(|&id| {
                self.unit(id)
                    .is_some_and(|hero| !hero.alive && !hero.escaped)
            })
            .collect()
    }

    pub fn use_elixir_of_life(&mut self, target: UnitId) -> Result<Pos, RuleError> {
        let owner = self.active_awake_hero_id()?;
        if !self.unit(owner).is_some_and(|hero| {
            hero.equipment_available && hero.inventory.artifacts.contains(&Artifact::ElixirOfLife)
        }) {
            return Err(RuleError::NoElixirOfLife);
        }
        if !self.elixir_of_life_targets().contains(&target) {
            return Err(RuleError::InvalidElixirTarget);
        }
        let fallen_pos = self.unit(target).expect("validated dead Hero exists").pos;
        let revival_pos = self
            .nearest_open_revival_square(fallen_pos, target)
            .ok_or(RuleError::InvalidElixirTarget)?;

        let owner_name = self.unit(owner).expect("Elixir owner exists").name.clone();
        self.units
            .iter_mut()
            .find(|unit| unit.id == owner)
            .expect("Elixir owner exists")
            .inventory
            .artifacts
            .retain(|artifact| *artifact != Artifact::ElixirOfLife);
        let revived = self
            .units
            .iter_mut()
            .find(|unit| unit.id == target)
            .expect("validated dead Hero exists");
        revived.pos = revival_pos;
        revived.body = revived.stats.body as i16;
        revived.alive = true;
        revived.in_pit = false;
        revived.escaped = false;
        revived.fearful = false;
        revived.sleeping = false;
        revived.clouded = false;
        revived.hero_sleep_caster = None;
        revived.skip_turns = 0;
        revived.petrified_turns = 0;
        revived.commanded = false;
        revived.swift_wind = false;
        revived.courage = false;
        revived.rock_skin = false;
        revived.pass_through_rock = false;
        revived.veil_of_mist = false;
        revived.potion_defense_bonus = 0;
        let revived_name = revived.name.clone();
        self.pending_hero_death = self
            .pending_hero_death
            .filter(|pending| pending.hero != target);
        self.pending_possession_pickup = self
            .pending_possession_pickup
            .take()
            .filter(|pending| pending.dead_hero != target);
        self.reveal_from(revival_pos);
        self.push_log(format!(
            "{owner_name} used the Elixir of Life. {revived_name} returned with full Body and Mind Points."
        ));
        Ok(revival_pos)
    }

    fn nearest_open_revival_square(&self, origin: Pos, target: UnitId) -> Option<Pos> {
        let mut queue = VecDeque::from([origin]);
        let mut visited = HashSet::from([origin]);
        while let Some(pos) = queue.pop_front() {
            if self.square_is_open_for_figure(pos, Some(target)) {
                return Some(pos);
            }
            for direction in Direction::ALL {
                let Some(next) = pos.offset(direction) else {
                    continue;
                };
                if visited.insert(next) && self.boundary_is_open(pos, next) {
                    queue.push_back(next);
                }
            }
        }
        None
    }

    pub fn can_use_ring_of_return(&self) -> bool {
        self.active_hero().is_some_and(|hero| {
            hero.alive
                && !hero.escaped
                && hero.equipment_available
                && hero.inventory.artifacts.contains(&Artifact::RingOfReturn)
        })
    }

    pub fn use_ring_of_return(&mut self) -> Result<Vec<UnitId>, RuleError> {
        let owner = self.active_awake_hero_id()?;
        if !self.unit(owner).is_some_and(|hero| {
            hero.equipment_available && hero.inventory.artifacts.contains(&Artifact::RingOfReturn)
        }) {
            return Err(RuleError::NoRingOfReturn);
        }
        let owner_pos = self.unit(owner).expect("Ring owner exists").pos;
        let visible: Vec<_> = self
            .hero_order
            .iter()
            .copied()
            .filter(|&id| {
                self.unit(id).is_some_and(|hero| {
                    hero.alive && !hero.escaped && self.can_see(owner_pos, hero.pos)
                })
            })
            .collect();
        self.units
            .iter_mut()
            .find(|unit| unit.id == owner)
            .expect("Ring owner exists")
            .inventory
            .artifacts
            .retain(|artifact| *artifact != Artifact::RingOfReturn);

        for &hero_id in &visible {
            let order_index = self
                .hero_order
                .iter()
                .position(|&id| id == hero_id)
                .expect("returning Hero is in turn order");
            let destination = self.hero_start_positions[order_index];
            let hero = self
                .units
                .iter_mut()
                .find(|unit| unit.id == hero_id)
                .expect("returning Hero exists");
            hero.pos = destination;
            hero.in_pit = false;
            if matches!(
                self.objective,
                ObjectiveDef::EscapeIndependently | ObjectiveDef::DefeatAllOrEscapeIndependently
            ) && self.stairs.contains(&destination)
            {
                hero.escaped = true;
            }
            if let Some(item_index) = hero.carried_quest_item {
                self.props[self.quest_items[item_index].prop_index].pos = destination;
            }
            self.reveal_from(destination);
        }
        self.hero_turn.movement_left = 0;
        self.push_log(format!(
            "The Ring of Return carried {} visible Hero{} back to the Quest entrance.",
            visible.len(),
            if visible.len() == 1 { "" } else { "es" }
        ));
        self.check_terminal();
        Ok(visible)
    }

    /// Picks up a royal/quest chest on the Hero's square or an orthogonally
    /// adjacent square. This is a free interaction and never exposes the
    /// sealed contents as spendable Hero gold.
    pub fn take_quest_item(&mut self) -> Result<String, RuleError> {
        let hero_id = self.active_awake_hero_id()?;
        let hero = self.unit(hero_id).expect("active hero exists");
        if hero.carried_quest_item.is_some() {
            return Err(RuleError::AlreadyCarryingQuestItem);
        }
        let hero_pos = hero.pos;
        let item_index = self
            .quest_items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.holder.is_none() && !item.delivered)
            .filter(|(_, item)| {
                let prop = &self.props[item.prop_index];
                prop.visible
                    && (prop.pos == hero_pos
                        || (hero_pos.is_adjacent(prop.pos)
                            && self.boundary_is_open(hero_pos, prop.pos)))
            })
            .min_by_key(|(_, item)| item.id.as_str())
            .map(|(index, _)| index)
            .ok_or(RuleError::NoQuestItem)?;
        let item_name = self.quest_items[item_index].id.clone();
        let prop_index = self.quest_items[item_index].prop_index;
        self.quest_items[item_index].holder = Some(hero_id);
        self.props[prop_index].pos = hero_pos;
        self.props[prop_index].carried_by = Some(hero_id);
        self.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .expect("active hero exists")
            .carried_quest_item = Some(item_index);
        let hero_name = self.unit(hero_id).expect("active hero exists").name.clone();
        self.push_log(format!("{hero_name} picked up {item_name}."));
        self.deliver_carried_quest_item(hero_id);
        self.check_terminal();
        Ok(item_name)
    }

    pub fn drop_quest_item(&mut self) -> Result<String, RuleError> {
        let hero_id = self.active_awake_hero_id()?;
        let item_index = self
            .unit(hero_id)
            .expect("active hero exists")
            .carried_quest_item
            .ok_or(RuleError::NotCarryingQuestItem)?;
        let item_name = self.drop_carried_quest_item_at(hero_id);
        debug_assert_eq!(self.quest_items[item_index].id, item_name);
        let hero_name = self.unit(hero_id).expect("active hero exists").name.clone();
        self.push_log(format!("{hero_name} put down {item_name}."));
        Ok(item_name)
    }

    pub fn transfer_quest_item_to_adjacent_hero(&mut self) -> Result<UnitId, RuleError> {
        let hero_id = self.active_awake_hero_id()?;
        let hero = self.unit(hero_id).expect("active hero exists");
        let item_index = hero
            .carried_quest_item
            .ok_or(RuleError::NotCarryingQuestItem)?;
        let hero_pos = hero.pos;
        let recipient = self
            .units
            .iter()
            .filter(|unit| {
                unit.id != hero_id
                    && unit.alive
                    && !unit.escaped
                    && matches!(unit.figure, FigureKind::Hero(_))
                    && unit.carried_quest_item.is_none()
                    && hero_pos.is_adjacent(unit.pos)
                    && self.boundary_is_open(hero_pos, unit.pos)
            })
            .min_by_key(|unit| unit.id)
            .map(|unit| unit.id)
            .ok_or(RuleError::NoQuestItemRecipient)?;
        let recipient_pos = self.unit(recipient).expect("recipient exists").pos;
        self.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .expect("active hero exists")
            .carried_quest_item = None;
        self.units
            .iter_mut()
            .find(|unit| unit.id == recipient)
            .expect("recipient exists")
            .carried_quest_item = Some(item_index);
        self.quest_items[item_index].holder = Some(recipient);
        let prop_index = self.quest_items[item_index].prop_index;
        self.props[prop_index].pos = recipient_pos;
        self.props[prop_index].carried_by = Some(recipient);
        let item_name = self.quest_items[item_index].id.clone();
        let giver_name = self.unit(hero_id).expect("active hero exists").name.clone();
        let recipient_name = self.unit(recipient).expect("recipient exists").name.clone();
        self.push_log(format!(
            "{giver_name} passed {item_name} to {recipient_name}."
        ));
        self.deliver_carried_quest_item(recipient);
        self.check_terminal();
        Ok(recipient)
    }

    pub fn search_secret_doors(&mut self) -> Result<Vec<(Pos, Pos)>, RuleError> {
        self.begin_search_action()?;
        let hero_pos = self.active_hero().expect("active hero exists").pos;
        let matches: Vec<_> = self
            .doors
            .iter()
            .enumerate()
            .filter(|(_, door)| {
                door.secret
                    && door.searchable
                    && !door.discovered
                    && (self.in_current_search_area(hero_pos, door.a)
                        || self.in_current_search_area(hero_pos, door.b))
            })
            .map(|(index, _)| index)
            .collect();
        let mut found = Vec::with_capacity(matches.len());
        for index in matches {
            let door = &mut self.doors[index];
            door.discovered = true;
            found.push((door.a, door.b));
        }
        self.push_log(if found.is_empty() {
            "No secret doors were found in this area.".to_owned()
        } else {
            format!("{} secret door(s) were found.", found.len())
        });
        Ok(found)
    }

    pub fn search_traps(&mut self) -> Result<Vec<Pos>, RuleError> {
        self.begin_search_action()?;
        let hero_pos = self.active_hero().expect("active hero exists").pos;
        let matches: Vec<_> = self
            .traps
            .iter()
            .enumerate()
            .filter(|(_, trap)| {
                !trap.sprung && !trap.disarmed && self.in_current_search_area(hero_pos, trap.pos)
            })
            .map(|(index, _)| index)
            .collect();
        let mut found = Vec::with_capacity(matches.len());
        for index in matches {
            let trap = &mut self.traps[index];
            trap.discovered = true;
            found.push(trap.pos);
        }
        self.push_log(if found.is_empty() {
            "No traps were found in this area.".to_owned()
        } else {
            format!("{} concealed trap location(s) were found.", found.len())
        });
        Ok(found)
    }

    pub fn active_disarm_plan(&self) -> Result<DisarmPlan, RuleError> {
        let hero = self.active_hero().ok_or(RuleError::NotHeroTurn)?;
        if hero.sleeping || hero.clouded {
            return Err(RuleError::Incapacitated);
        }
        if self.hero_turn.action_used {
            return Err(RuleError::AlreadyActed);
        }
        if self.hero_turn.moved_steps > 0 {
            return Err(RuleError::DisarmBeforeMoving);
        }
        if self.hero_turn.movement_roll.is_none() {
            return Err(RuleError::MovementNotRolled);
        }
        if self.hero_turn.movement_left == 0 {
            return Err(RuleError::NoMovement);
        }
        let dwarf = matches!(hero.figure, FigureKind::Hero(crate::model::HeroKind::Dwarf));
        if !dwarf && !hero.equipment_available && hero.inventory.tool_kits > 0 {
            return Err(RuleError::EquipmentUnavailable);
        }
        if !dwarf && hero.inventory.tool_kits == 0 {
            return Err(RuleError::ToolKitRequired);
        }
        Direction::ALL
            .into_iter()
            .filter_map(|direction| {
                let pos = hero.pos.offset(direction)?;
                let trap_index = self.traps.iter().position(|trap| {
                    trap.pos == pos
                        && trap.disarmable
                        && trap.discovered
                        && !trap.sprung
                        && !trap.disarmed
                })?;
                let target_is_clear = if self.is_furniture_square(pos) {
                    self.occupied_by_alive(pos, Some(hero.id)).is_none()
                } else {
                    self.square_is_open_for_figure(pos, Some(hero.id))
                };
                (self.boundary_is_open(hero.pos, pos) && target_is_clear).then_some(DisarmPlan {
                    hero: hero.id,
                    trap_index,
                    from: hero.pos,
                    trap: pos,
                    direction,
                    dwarf,
                })
            })
            .next()
            .ok_or(RuleError::NoDisarmableTrap)
    }

    pub fn resolve_disarm(
        &mut self,
        plan: DisarmPlan,
        face: CombatFace,
    ) -> Result<bool, RuleError> {
        if self.active_hero_id() != Some(plan.hero)
            || self.hero_turn.action_used
            || self.hero_turn.moved_steps > 0
            || self.hero_turn.movement_left == 0
            || self.traps.get(plan.trap_index).is_none_or(|trap| {
                trap.pos != plan.trap || !trap.discovered || trap.sprung || trap.disarmed
            })
        {
            return Err(RuleError::StaleAttack);
        }
        let success = if plan.dwarf {
            face != CombatFace::BlackShield
        } else {
            face != CombatFace::Skull
        };
        let furniture_trap = self.is_furniture_square(plan.trap);
        let hero_name = {
            let hero = self
                .units
                .iter_mut()
                .find(|unit| unit.id == plan.hero)
                .expect("validated Hero exists");
            // Floor traps are disarmed by stepping onto their square. A
            // chest/furniture trap is worked on from the adjacent square;
            // the assembled furniture remains physically impassable.
            if !furniture_trap {
                hero.pos = plan.trap;
            }
            hero.in_pit = false;
            hero.name.clone()
        };
        self.hero_turn.action_used = true;
        self.hero_turn.moved_steps += 1;
        self.hero_turn.movement_left -= 1;
        if success {
            let trap = &mut self.traps[plan.trap_index];
            trap.disarmed = true;
            self.push_log(format!(
                "{hero_name} disarmed the trap and may continue moving."
            ));
        } else {
            self.push_log(format!("{hero_name} failed to disarm the trap!"));
            self.trigger_square_trap(plan.hero, plan.from, plan.trap, plan.direction, true);
        }
        Ok(success)
    }

    pub fn active_jump_plan(&self, direction: Direction) -> Result<JumpPlan, RuleError> {
        let hero = self.active_hero().ok_or(RuleError::NotHeroTurn)?;
        if hero.sleeping || hero.clouded {
            return Err(RuleError::Incapacitated);
        }
        if self.hero_turn.movement_roll.is_none() {
            return Err(RuleError::MovementNotRolled);
        }
        if self.hero_turn.movement_left < 2 {
            return Err(RuleError::NoMovement);
        }
        let trap = hero
            .pos
            .offset(direction)
            .ok_or(RuleError::NoJumpableTrap)?;
        let landing = trap.offset(direction).ok_or(RuleError::NoJumpableTrap)?;
        let trap_index = self
            .traps
            .iter()
            .position(|candidate| {
                candidate.pos == trap
                    && !candidate.disarmed
                    && ((candidate.discovered && !candidate.sprung)
                        || (candidate.sprung && candidate.kind == TrapKind::Pit))
            })
            .ok_or(RuleError::NoJumpableTrap)?;
        if self.is_furniture_square(trap)
            || !self.boundary_is_open(hero.pos, trap)
            || !self.boundary_is_open(trap, landing)
            || !self.square_is_open_for_figure(landing, Some(hero.id))
        {
            return Err(RuleError::NoJumpableTrap);
        }
        Ok(JumpPlan {
            hero: hero.id,
            trap_index,
            from: hero.pos,
            trap,
            landing,
            direction,
        })
    }

    pub fn resolve_jump(&mut self, plan: JumpPlan, face: CombatFace) -> Result<bool, RuleError> {
        if self.active_hero_id() != Some(plan.hero)
            || self.hero_turn.movement_left < 2
            || self
                .unit(plan.hero)
                .is_none_or(|hero| hero.pos != plan.from)
            || self.traps.get(plan.trap_index).is_none_or(|trap| {
                trap.pos != plan.trap
                    || trap.disarmed
                    || (!trap.discovered && !trap.sprung)
                    || (trap.sprung && trap.kind != TrapKind::Pit)
            })
            || self.is_furniture_square(plan.trap)
            || !self.square_is_open_for_figure(plan.landing, Some(plan.hero))
        {
            return Err(RuleError::StaleJump);
        }
        self.hero_turn.movement_left -= 2;
        self.hero_turn.moved_steps += 2;
        let hero_name = self
            .unit(plan.hero)
            .expect("validated Hero exists")
            .name
            .clone();
        if face != CombatFace::Skull {
            if let Some(hero) = self.units.iter_mut().find(|unit| unit.id == plan.hero) {
                hero.pos = plan.landing;
                hero.in_pit = false;
            }
            self.reveal_from(plan.landing);
            self.push_log(format!(
                "{hero_name} jumped the trap and spent two movement squares."
            ));
            Ok(true)
        } else {
            if let Some(hero) = self.units.iter_mut().find(|unit| unit.id == plan.hero) {
                hero.pos = plan.trap;
            }
            let sprung_pit = self.traps[plan.trap_index].sprung
                && self.traps[plan.trap_index].kind == TrapKind::Pit;
            if sprung_pit {
                if let Some(hero) = self.units.iter_mut().find(|unit| unit.id == plan.hero) {
                    hero.in_pit = true;
                }
                self.damage_without_defense(plan.hero, 1);
                self.hero_turn.movement_left = 0;
                self.push_log(format!(
                    "{hero_name} failed the jump and fell into the pit for 1 Body Point."
                ));
                self.end_forced_hero_turn();
            } else {
                self.push_log(format!("{hero_name} failed the jump and sprung the trap!"));
                self.trigger_square_trap(plan.hero, plan.from, plan.trap, plan.direction, true);
            }
            Ok(false)
        }
    }

    pub fn active_attack_options(&self) -> Result<Vec<AttackPlan>, RuleError> {
        if self.hero_turn.action_used
            && !self.hero_turn.orcs_bane_follow_up
            && !self.hero_turn.heroic_brew_follow_up
        {
            return Err(RuleError::AlreadyActed);
        }
        let attacker = self.active_hero().ok_or(RuleError::NotHeroTurn)?;
        if attacker.sleeping || attacker.clouded {
            return Err(RuleError::Incapacitated);
        }
        if attacker.inventory.fools_gold > 0 {
            return Err(RuleError::CarryingFoolsGold);
        }
        let mut sources = Vec::new();
        if !attacker.equipment_available || attacker.fearful {
            sources.push(AttackSource::Unarmed);
        } else {
            let hero_kind = match attacker.figure {
                FigureKind::Hero(hero) => Some(hero),
                FigureKind::Monster(_) => None,
            };
            for &weapon in &attacker.inventory.weapons {
                if hero_kind.is_some_and(|hero| !weapon.allowed_by(hero)) {
                    continue;
                }
                let source = AttackSource::Weapon(weapon);
                if !sources.contains(&source) {
                    sources.push(source);
                }
                if weapon == Weapon::Dagger {
                    sources.push(AttackSource::ThrownDagger);
                }
            }
            let wizard = matches!(
                attacker.figure,
                FigureKind::Hero(crate::model::HeroKind::Wizard)
            );
            if !wizard && attacker.inventory.artifacts.contains(&Artifact::OrcsBane) {
                sources.push(AttackSource::OrcsBane);
            }
            if !wizard
                && attacker
                    .inventory
                    .artifacts
                    .contains(&Artifact::SpiritBlade)
            {
                sources.push(AttackSource::SpiritBlade);
            }
            if wizard
                && attacker
                    .inventory
                    .artifacts
                    .contains(&Artifact::WizardsStaff)
            {
                sources.push(AttackSource::WizardsStaff);
            }
        }
        if self.hero_turn.orcs_bane_follow_up {
            sources.retain(|source| *source == AttackSource::OrcsBane);
        }

        let mut options = Vec::new();
        for defender in self.units.iter().filter(|unit| {
            unit.alive
                && unit.faction == Faction::Monster
                && !unit.dormant
                && self.is_visible(unit)
                && (!self.hero_turn.orcs_bane_follow_up
                    || unit.figure == FigureKind::Monster(MonsterKind::Orc))
        }) {
            for &source in &sources {
                if !self.attack_source_reaches(attacker, defender, source) {
                    continue;
                }
                let base_dice = match source {
                    AttackSource::Natural => attacker.stats.attack,
                    AttackSource::Unarmed => 1,
                    AttackSource::Weapon(weapon) => weapon.attack_dice(),
                    AttackSource::ThrownDagger => Weapon::Dagger.attack_dice(),
                    AttackSource::OrcsBane | AttackSource::WizardsStaff => 2,
                    AttackSource::SpiritBlade => {
                        if matches!(
                            defender.figure,
                            FigureKind::Monster(
                                MonsterKind::Skeleton | MonsterKind::Zombie | MonsterKind::Mummy
                            )
                        ) {
                            4
                        } else {
                            3
                        }
                    }
                };
                options.push(AttackPlan {
                    attacker: attacker.id,
                    defender: defender.id,
                    source,
                    attack_dice: pit_adjusted_dice(
                        base_dice
                            .saturating_add(u8::from(attacker.courage) * 2)
                            .saturating_add(self.hero_turn.potion_strength_bonus),
                        attacker.in_pit,
                    ),
                    defend_dice: pit_adjusted_dice(
                        defender.effective_defense_dice(),
                        defender.in_pit,
                    ),
                });
            }
        }
        if options.is_empty() {
            Err(RuleError::NoTarget)
        } else {
            Ok(options)
        }
    }

    pub fn active_attack_plan(&self) -> Result<AttackPlan, RuleError> {
        self.active_attack_options()?
            .into_iter()
            .max_by_key(|plan| {
                (
                    u8::from(plan.source == AttackSource::OrcsBane),
                    plan.attack_dice,
                    std::cmp::Reverse(
                        self.unit(plan.defender)
                            .map(|unit| unit.body)
                            .unwrap_or(i16::MAX),
                    ),
                    std::cmp::Reverse(plan.defender),
                )
            })
            .ok_or(RuleError::NoTarget)
    }

    pub fn attack_active_random(&mut self) -> Result<CombatOutcome, RuleError> {
        let plan = self.active_attack_plan()?;
        let attack = self.random_combat_faces(plan.attack_dice);
        let defend = self.random_combat_faces(plan.defend_dice);
        self.resolve_attack(plan, &attack, &defend)
    }

    pub fn resolve_attack(
        &mut self,
        plan: AttackPlan,
        attack_faces: &[CombatFace],
        defend_faces: &[CombatFace],
    ) -> Result<CombatOutcome, RuleError> {
        if attack_faces.len() != plan.attack_dice as usize
            || defend_faces.len() != plan.defend_dice as usize
        {
            return Err(RuleError::InvalidDice);
        }
        let attacker = self.unit(plan.attacker).ok_or(RuleError::StaleAttack)?;
        let defender = self.unit(plan.defender).ok_or(RuleError::StaleAttack)?;
        if !attacker.alive
            || !defender.alive
            || attacker.escaped
            || defender.escaped
            || !self.attack_source_reaches(attacker, defender, plan.source)
        {
            return Err(RuleError::StaleAttack);
        }
        let commanded_on_zargon_turn = self.phase == GamePhase::ZargonTurn && attacker.commanded;
        if attacker.faction == Faction::Hero
            && !commanded_on_zargon_turn
            && (self.active_hero_id() != Some(attacker.id)
                || (self.hero_turn.action_used
                    && !self.hero_turn.orcs_bane_follow_up
                    && !self.hero_turn.heroic_brew_follow_up))
        {
            return Err(RuleError::StaleAttack);
        }

        let defender_faction = defender.faction;
        let skulls = attack_faces
            .iter()
            .filter(|&&face| face == CombatFace::Skull)
            .count() as u8;
        let blocks = defend_faces
            .iter()
            .filter(|&&face| match defender_faction {
                Faction::Hero => face == CombatFace::WhiteShield,
                Faction::Monster => face == CombatFace::BlackShield,
            })
            .count() as u8;
        let uses_spirit_blade = defender.immune_except_spirit_blade
            && attacker.faction == Faction::Hero
            && plan.source == AttackSource::SpiritBlade;
        let damage = if defender.invulnerable_until_acts
            || defender.petrified_turns > 0
            || (defender.immune_except_spirit_blade && !uses_spirit_blade)
        {
            0
        } else {
            skulls.saturating_sub(blocks)
        };
        let attacker_name = attacker.name.clone();
        let attacker_faction = attacker.faction;
        let defender_name = defender.name.clone();
        let defender_figure = defender.figure;
        let defender_unit = self
            .units
            .iter_mut()
            .find(|unit| unit.id == plan.defender)
            .expect("validated defender exists");
        defender_unit.body -= damage as i16;
        if damage > 0 {
            defender_unit.rock_skin = false;
        }
        defender_unit.potion_defense_bonus = 0;
        let defender_died = self.resolve_zero_body(plan.defender);
        if attacker_faction == Faction::Hero {
            self.commit_hero_attack_source(plan.attacker, plan.source);
        }

        self.visual_sequence = self.visual_sequence.wrapping_add(1);
        self.last_combat_visual = Some(CombatVisualEvent {
            sequence: self.visual_sequence,
            attacker: plan.attacker,
            defender: plan.defender,
            damage,
            defender_died,
        });
        if attacker_faction == Faction::Hero
            && let Some(hero) = self.units.iter_mut().find(|unit| unit.id == plan.attacker)
        {
            hero.courage = false;
        }

        if self.active_hero_id() == Some(plan.attacker) {
            let was_orcs_bane_follow_up = self.hero_turn.orcs_bane_follow_up;
            let was_heroic_brew_follow_up = self.hero_turn.heroic_brew_follow_up;
            let was_follow_up = was_orcs_bane_follow_up || was_heroic_brew_follow_up;
            let earns_orcs_bane_follow_up = !was_follow_up
                && defender_figure == FigureKind::Monster(MonsterKind::Orc)
                && plan.source == AttackSource::OrcsBane
                && self.unit(plan.attacker).is_some_and(|hero| {
                    hero.equipment_available
                        && !matches!(
                            hero.figure,
                            FigureKind::Hero(crate::model::HeroKind::Wizard)
                        )
                        && hero.inventory.artifacts.contains(&Artifact::OrcsBane)
                });
            let earns_heroic_brew_follow_up = !was_follow_up && self.hero_turn.heroic_brew_ready;
            self.hero_turn.action_used = true;
            self.hero_turn.orcs_bane_follow_up = earns_orcs_bane_follow_up;
            self.hero_turn.heroic_brew_follow_up =
                earns_heroic_brew_follow_up && !earns_orcs_bane_follow_up;
            self.hero_turn.heroic_brew_ready = false;
            self.hero_turn.potion_strength_bonus = 0;
            if self.hero_turn.moved_steps > 0 {
                self.hero_turn.movement_left = 0;
            }
            if earns_orcs_bane_follow_up {
                self.push_log(
                    "Orc's Bane permits one immediate second attack against an adjacent Orc."
                        .to_owned(),
                );
            } else if earns_heroic_brew_follow_up {
                self.push_log(
                    "Heroic Brew permits one immediate second attack with any legal weapon."
                        .to_owned(),
                );
            }
        }
        self.push_log(format!(
            "{attacker_name} attacked {defender_name}: {skulls} skulls, {blocks} blocks, {damage} damage{}.",
            if defender_died { " — defeated" } else { "" }
        ));
        if defender_died {
            self.award_monster_bounty(plan.attacker, defender_figure);
            self.resolve_defeat_events(&defender_name, Some(plan.attacker));
        }
        self.check_terminal();
        Ok(CombatOutcome {
            skulls,
            blocks,
            damage,
            defender_died,
        })
    }

    pub fn end_hero_turn(&mut self) -> Result<(), RuleError> {
        if self.pending_trap_roll.is_some() {
            return Err(RuleError::TrapRollPending);
        }
        if self.pending_hero_death.is_some() {
            return Err(RuleError::HeroDeathDecisionPending);
        }
        if self.pending_possession_pickup.is_some() {
            return Err(RuleError::PossessionPickupPending);
        }
        if let Some(hero_id) = self.active_hero_id()
            && self.hero_turn.movement_roll.is_some()
        {
            let pos = self.unit(hero_id).expect("active Hero exists").pos;
            if self.occupied_by_alive(pos, Some(hero_id)).is_some()
                && !self.hero_may_share_square(hero_id, pos)
            {
                return Err(RuleError::MustFinishOccupiedMove);
            }
            self.finish_spell_movement(hero_id);
        }
        if let GamePhase::HeroTurn { order_index } = self.phase
            && self.hero_turn.action_used
        {
            self.heroes_acted_this_round
                .insert(self.hero_order[order_index]);
        }
        let order_index = match self.phase {
            GamePhase::HeroTurn { order_index } => {
                let controller = self.hero_order[order_index];
                if let Some((ally, ally_controller)) = self.escorted_ally
                    && controller == ally_controller
                    && self
                        .unit(ally)
                        .is_some_and(|unit| unit.alive && !self.stairs.contains(&unit.pos))
                {
                    self.phase = GamePhase::AllyTurn {
                        ally,
                        controller_order_index: order_index,
                    };
                    self.hero_turn = HeroTurnState::default();
                    let name = self
                        .unit(ally)
                        .map(|unit| unit.name.clone())
                        .unwrap_or_else(|| "Ally".to_owned());
                    self.push_log(format!("{name}'s escorted movement turn."));
                    return Ok(());
                }
                order_index
            }
            GamePhase::AllyTurn {
                controller_order_index,
                ..
            } => controller_order_index,
            _ => return Err(RuleError::NotHeroTurn),
        };
        self.check_terminal();
        if matches!(
            self.phase,
            GamePhase::Won | GamePhase::Retreated | GamePhase::Lost
        ) {
            return Ok(());
        }

        if let Some(next) = self.next_hero_turn_index(order_index + 1) {
            self.phase = GamePhase::HeroTurn { order_index: next };
            self.hero_turn = HeroTurnState::default();
            let name = self
                .active_hero()
                .expect("next living hero exists")
                .name
                .clone();
            self.push_log(format!("{name}'s turn."));
        } else {
            self.phase = GamePhase::ZargonTurn;
            self.hero_turn = HeroTurnState::default();
            self.zargon_turn_started = false;
            self.zargon_queue.clear();
            self.zargon_active = None;
            self.zargon_commanded_queue.clear();
            self.zargon_commanded_active = None;
            self.pending_chaos_spell_rolls.clear();
            self.push_log("The computer game master begins its turn.".to_owned());
        }
        Ok(())
    }

    pub fn can_voluntarily_retreat(&self) -> bool {
        matches!(self.phase, GamePhase::HeroTurn { .. })
            && self.pending_hero_death.is_none()
            && self.pending_possession_pickup.is_none()
            && self.living_hero_count() > 0
            && self.hero_order.iter().all(|&id| {
                self.unit(id)
                    .is_none_or(|hero| !hero.alive || self.stairs.contains(&hero.pos))
            })
    }

    pub fn voluntarily_retreat(&mut self) -> Result<(), RuleError> {
        if !self.can_voluntarily_retreat() {
            return Err(RuleError::RetreatRequiresStairs);
        }
        self.phase = GamePhase::Retreated;
        self.hero_turn = HeroTurnState::default();
        self.push_log(
            "The surviving Heroes voluntarily ended the unfinished quest at the stairway; no completion or final reward was recorded."
                .to_owned(),
        );
        Ok(())
    }

    /// Advances the computer game master by one visible presentation step.
    /// Movement and attack planning are deliberately separated so the SDL
    /// loop can animate a miniature first, then throw its physical combat dice.
    pub fn advance_zargon_turn(&mut self) -> Result<ZargonStep> {
        ensure!(
            self.phase == GamePhase::ZargonTurn,
            "it is not the computer game master's turn"
        );
        ensure!(
            self.pending_hero_death.is_none() && self.pending_possession_pickup.is_none(),
            "a fallen Hero's rescue and possessions must be resolved first"
        );
        self.allocate_pending_visible_figures();
        if let Some(roll) = self.pending_hero_spell_roll {
            return Ok(ZargonStep::HeroSpellRoll(roll));
        }
        if let Some(pending) = self.pending_chaos_spell_rolls.front().copied() {
            return Ok(ZargonStep::Cast {
                caster: pending.caster,
                target: pending.target,
                spell: pending.spell,
                resistance_dice: pending.dice_count,
            });
        }
        if !self.zargon_turn_started {
            self.zargon_commanded_queue = self
                .hero_order
                .iter()
                .copied()
                .filter(|&id| {
                    self.unit(id)
                        .is_some_and(|hero| hero.alive && !hero.escaped && hero.commanded)
                })
                .collect();
            self.zargon_queue = self
                .units
                .iter()
                .filter(|unit| {
                    unit.alive
                        && unit.faction == Faction::Monster
                        && !unit.dormant
                        && self.cells[Self::cell_index(unit.pos)].revealed
                })
                .map(|unit| unit.id)
                .collect();
            self.zargon_turn_started = true;
        }

        loop {
            if self.active_hero_count() == 0 {
                self.check_terminal();
                return Ok(ZargonStep::Finished);
            }

            if let Some(hero_id) = self.zargon_commanded_active.take()
                && self
                    .unit(hero_id)
                    .is_some_and(|hero| hero.alive && !hero.escaped && hero.commanded)
                && let Some(target) = self.adjacent_other_hero(hero_id)
            {
                return Ok(ZargonStep::Attack(
                    self.monster_attack_plan(hero_id, target)?,
                ));
            }

            if let Some(hero_id) = self.zargon_commanded_queue.pop_front()
                && self
                    .unit(hero_id)
                    .is_some_and(|hero| hero.alive && !hero.escaped && hero.commanded)
            {
                if let Some(target) = self.adjacent_other_hero(hero_id) {
                    return Ok(ZargonStep::Attack(
                        self.monster_attack_plan(hero_id, target)?,
                    ));
                }
                let from = self.unit(hero_id).expect("commanded Hero exists").pos;
                if let Some(to) = self
                    .path_to_nearest_other_hero(hero_id)
                    .and_then(|path| path.into_iter().take(12).last())
                    && to != from
                {
                    let lands_in_pit = self.is_sprung_pit(to);
                    self.units
                        .iter_mut()
                        .find(|unit| unit.id == hero_id)
                        .map(|hero| {
                            hero.pos = to;
                            hero.in_pit = lands_in_pit;
                        })
                        .expect("commanded Hero exists");
                    self.zargon_commanded_active = Some(hero_id);
                    let name = self
                        .unit(hero_id)
                        .expect("commanded Hero exists")
                        .name
                        .clone();
                    self.push_log(format!(
                        "Zargon used Command to move {name} toward another Hero."
                    ));
                    return Ok(ZargonStep::Moved {
                        unit: hero_id,
                        from,
                        to,
                    });
                }
            }

            if let Some(monster_id) = self.zargon_active.take() {
                if self.unit(monster_id).is_some_and(|monster| monster.alive) {
                    if self
                        .unit(monster_id)
                        .is_some_and(|monster| monster.sleeping)
                    {
                        let name = self
                            .unit(monster_id)
                            .map(|monster| monster.name.clone())
                            .unwrap_or_else(|| "Monster".to_owned());
                        self.push_log(format!("{name} remains asleep and misses the turn."));
                        continue;
                    }
                    if let Some(spell) = self.try_cast_chaos_spell(monster_id) {
                        return Ok(spell);
                    }
                    if let Some(hero_id) = self.adjacent_hero(monster_id) {
                        return Ok(ZargonStep::Attack(
                            self.monster_attack_plan(monster_id, hero_id)?,
                        ));
                    }
                }
            }

            let Some(monster_id) = self.zargon_queue.pop_front() else {
                self.finish_zargon_turn();
                return Ok(ZargonStep::Finished);
            };
            if self.unit(monster_id).is_none_or(|monster| !monster.alive) {
                continue;
            }
            if self
                .unit(monster_id)
                .is_some_and(|monster| monster.skip_turns > 0)
            {
                let monster = self
                    .units
                    .iter_mut()
                    .find(|unit| unit.id == monster_id)
                    .expect("queued Monster exists");
                monster.skip_turns -= 1;
                monster.invulnerable_until_acts = false;
                let name = monster.name.clone();
                self.push_log(format!(
                    "Tempest holds {name}; the Monster misses this turn."
                ));
                continue;
            }
            if self
                .unit(monster_id)
                .is_some_and(|monster| monster.sleeping)
            {
                let (caster, dice_count, name) = self
                    .unit(monster_id)
                    .map(|monster| {
                        (
                            monster.hero_sleep_caster.unwrap_or(monster_id),
                            monster.effective_mind(),
                            monster.name.clone(),
                        )
                    })
                    .expect("queued Monster exists");
                let roll = HeroSpellRoll {
                    caster,
                    target: monster_id,
                    spell: HeroSpell::Sleep,
                    dice_count,
                    kind: HeroSpellDiceKind::Red,
                };
                self.pending_hero_spell_roll = Some(roll);
                self.zargon_active = Some(monster_id);
                self.push_log(format!(
                    "{name} rolls {} physical Mind dice to awaken from Sleep.",
                    roll.dice_count
                ));
                return Ok(ZargonStep::HeroSpellRoll(roll));
            }
            if let Some(spell) = self.try_cast_chaos_spell(monster_id) {
                return Ok(spell);
            }
            if let Some(hero_id) = self.adjacent_hero(monster_id) {
                if let Some(monster) = self.units.iter_mut().find(|unit| unit.id == monster_id) {
                    monster.invulnerable_until_acts = false;
                }
                return Ok(ZargonStep::Attack(
                    self.monster_attack_plan(monster_id, hero_id)?,
                ));
            }

            let movement = self
                .unit(monster_id)
                .map(|unit| unit.stats.movement)
                .unwrap_or(0) as usize;
            let from = self.unit(monster_id).expect("validated monster exists").pos;
            let destination = self
                .path_to_nearest_hero(monster_id)
                .and_then(|path| path.into_iter().take(movement).last());
            if let Some(to) = destination
                && to != from
            {
                let lands_in_pit = self.is_sprung_pit(to);
                if let Some(monster) = self.units.iter_mut().find(|unit| unit.id == monster_id) {
                    monster.pos = to;
                    monster.in_pit = lands_in_pit;
                    monster.invulnerable_until_acts = false;
                }
                self.zargon_active = Some(monster_id);
                let name = self
                    .unit(monster_id)
                    .map(|monster| monster.name.clone())
                    .unwrap_or_else(|| "Monster".to_owned());
                self.push_log(format!("{name} moved."));
                return Ok(ZargonStep::Moved {
                    unit: monster_id,
                    from,
                    to,
                });
            }
        }
    }

    pub fn run_zargon_turn(&mut self) -> Result<()> {
        loop {
            match self.advance_zargon_turn()? {
                ZargonStep::Moved { .. } => {}
                ZargonStep::Attack(plan) => {
                    self.resolve_monster_attack_random(plan)?;
                }
                ZargonStep::Cast {
                    target,
                    spell,
                    resistance_dice,
                    ..
                } => {
                    if resistance_dice > 0 {
                        let dice = (0..resistance_dice)
                            .map(|_| self.rng.random_range(1..=6))
                            .collect::<Vec<_>>();
                        self.resolve_chaos_spell_resistance(target, spell, &dice)?;
                    }
                }
                ZargonStep::HeroSpellRoll(roll) => match roll.kind {
                    HeroSpellDiceKind::Red => {
                        let dice = (0..roll.dice_count)
                            .map(|_| self.rng.random_range(1..=6))
                            .collect::<Vec<_>>();
                        self.resolve_hero_spell_red_roll(roll, &dice)?;
                    }
                    HeroSpellDiceKind::Combat {
                        attack_dice,
                        defend_dice,
                    } => {
                        let attack = self.random_combat_faces(attack_dice);
                        let defend = self.random_combat_faces(defend_dice);
                        self.resolve_hero_spell_combat_roll(roll, &attack, &defend)?;
                    }
                },
                ZargonStep::Finished => return Ok(()),
            }
        }
    }

    fn finish_zargon_turn(&mut self) {
        self.check_terminal();
        if matches!(
            self.phase,
            GamePhase::Won | GamePhase::Retreated | GamePhase::Lost
        ) {
            return;
        }
        self.zargon_turn_started = false;
        self.zargon_queue.clear();
        self.zargon_active = None;
        self.zargon_commanded_queue.clear();
        self.zargon_commanded_active = None;
        self.pending_chaos_spell_rolls.clear();
        self.heroes_acted_this_round.clear();
        if let Some(first_living) = self.next_hero_turn_index(0) {
            self.phase = GamePhase::HeroTurn {
                order_index: first_living,
            };
            self.hero_turn = HeroTurnState::default();
            let name = self
                .active_hero()
                .map(|unit| unit.name.clone())
                .unwrap_or_default();
            self.push_log(format!("{name}'s turn."));
        } else {
            self.phase = GamePhase::ZargonTurn;
            self.hero_turn = HeroTurnState::default();
            self.push_log(
                "Tempest leaves no Hero able to act; the computer game master continues."
                    .to_owned(),
            );
        }
    }

    fn next_hero_turn_index(&mut self, start: usize) -> Option<usize> {
        for index in start..self.hero_order.len() {
            let hero_id = self.hero_order[index];
            let Some(hero) = self.unit(hero_id) else {
                continue;
            };
            if !hero.alive || hero.escaped {
                continue;
            }
            if hero.petrified_turns > 0 {
                let hero_name = hero.name.clone();
                let remaining = hero.petrified_turns - 1;
                self.units
                    .iter_mut()
                    .find(|unit| unit.id == hero_id)
                    .expect("turn-order Hero exists")
                    .petrified_turns = remaining;
                self.push_log(if remaining == 0 {
                    format!("{hero_name} spends a fifth turn as stone, then returns to normal.")
                } else {
                    format!("{hero_name} is stone and misses the turn ({remaining} remaining).")
                });
                continue;
            }
            if hero.skip_turns == 0 {
                self.break_courage_without_visible_monster(hero_id);
                return Some(index);
            }
            let hero_name = hero.name.clone();
            self.units
                .iter_mut()
                .find(|unit| unit.id == hero_id)
                .expect("turn-order Hero exists")
                .skip_turns -= 1;
            self.push_log(format!(
                "{hero_name} is trapped by Tempest and misses the turn."
            ));
        }
        None
    }

    pub fn status_line(&self) -> String {
        let latest = self.log.back().map(String::as_str).unwrap_or("");
        match self.phase {
            GamePhase::HeroTurn { .. } => {
                let hero = self.active_hero().expect("active hero exists");
                format!(
                    "{} — {} | Body {}/{} | Move {} | {}",
                    self.title,
                    hero.name,
                    hero.body,
                    hero.stats.body,
                    self.hero_turn.movement_left,
                    latest
                )
            }
            GamePhase::AllyTurn { ally, .. } => {
                let ally = self.unit(ally).expect("active ally exists");
                format!(
                    "{} — Escort {} | Body {}/{} | Move {} | {}",
                    self.title,
                    ally.name,
                    ally.body,
                    ally.stats.body,
                    self.hero_turn.movement_left,
                    latest
                )
            }
            GamePhase::ZargonTurn => format!("{} — Computer game master | {latest}", self.title),
            GamePhase::Won => format!("{} — Quest complete! | {latest}", self.title),
            GamePhase::Retreated => {
                format!("{} — Quest ended without completion | {latest}", self.title)
            }
            GamePhase::Lost => format!("{} — The heroes have fallen | {latest}", self.title),
        }
    }

    pub fn notify(&mut self, message: impl Into<String>) {
        self.push_log(message.into());
    }

    fn begin_search_action(&mut self) -> Result<(), RuleError> {
        let hero = self.active_hero().ok_or(RuleError::NotHeroTurn)?;
        if hero.sleeping || hero.clouded {
            return Err(RuleError::Incapacitated);
        }
        if self.hero_turn.action_used {
            return Err(RuleError::AlreadyActed);
        }
        if self.units.iter().any(|unit| {
            unit.alive
                && unit.faction == Faction::Monster
                && !unit.dormant
                && self.is_visible(unit)
                && self.can_see(hero.pos, unit.pos)
        }) {
            return Err(RuleError::VisibleMonster);
        }
        self.hero_turn.action_used = true;
        if self.hero_turn.moved_steps > 0 {
            self.hero_turn.movement_left = 0;
        }
        Ok(())
    }

    fn in_current_search_area(&self, observer: Pos, target: Pos) -> bool {
        if self
            .active_hero()
            .is_some_and(|hero| hero.pos == observer && hero.in_pit)
        {
            return target == observer;
        }
        let Some(observer_cell) = self.cell(observer) else {
            return false;
        };
        let Some(target_cell) = self.cell(target) else {
            return false;
        };
        if observer_cell.region > 0 {
            target_cell.region == observer_cell.region
        } else {
            target_cell.region == 0 && self.can_see(observer, target)
        }
    }

    fn is_sprung_pit(&self, pos: Pos) -> bool {
        self.traps.iter().any(|trap| {
            trap.pos == pos && trap.kind == TrapKind::Pit && trap.sprung && !trap.disarmed
        })
    }

    fn trigger_square_trap(
        &mut self,
        hero_id: UnitId,
        from: Pos,
        trap_pos: Pos,
        direction: Direction,
        forced_spear_hit: bool,
    ) {
        if self.traps.iter().any(|trap| {
            trap.pos == trap_pos && trap.kind == TrapKind::Pit && trap.sprung && !trap.disarmed
        }) {
            if let Some(hero) = self.units.iter_mut().find(|unit| unit.id == hero_id) {
                hero.in_pit = true;
            }
            let hero_name = self.unit(hero_id).expect("active hero exists").name.clone();
            self.damage_without_defense(hero_id, 1);
            self.hero_turn.movement_left = 0;
            self.push_log(format!(
                "{hero_name} voluntarily entered the open pit: 1 Body Point lost; the turn ends."
            ));
            self.end_forced_hero_turn();
            return;
        }
        let Some(trap_index) = self.traps.iter().position(|trap| {
            trap.pos == trap_pos && trap.trigger_on_entry && !trap.sprung && !trap.disarmed
        }) else {
            return;
        };
        let kind = self.traps[trap_index].kind;
        self.traps[trap_index].discovered = true;
        self.traps[trap_index].sprung = true;
        let hero_name = self.unit(hero_id).expect("active hero exists").name.clone();

        match kind {
            TrapKind::Pit => {
                if let Some(hero) = self.units.iter_mut().find(|unit| unit.id == hero_id) {
                    hero.in_pit = true;
                }
                self.damage_without_defense(hero_id, 1);
                self.hero_turn.movement_left = 0;
                self.push_log(format!(
                    "{hero_name} fell into a pit: 1 Body Point lost; the turn ends."
                ));
                self.end_forced_hero_turn();
            }
            TrapKind::Spear => {
                if forced_spear_hit {
                    self.damage_without_defense(hero_id, 1);
                    self.hero_turn.movement_left = 0;
                    self.push_log(format!(
                        "A spear struck {hero_name}: 1 Body Point lost; the turn ends."
                    ));
                    self.end_forced_hero_turn();
                } else {
                    self.pending_trap_roll = Some(PendingTrapRoll {
                        hero: hero_id,
                        trap: trap_pos,
                        from,
                        direction,
                        kind,
                    });
                    self.push_log(format!(
                        "A spear shoots toward {hero_name}; roll one physical combat die."
                    ));
                }
            }
            TrapKind::FallingBlock => {
                self.hero_turn.movement_left = 0;
                self.pending_trap_roll = Some(PendingTrapRoll {
                    hero: hero_id,
                    trap: trap_pos,
                    from,
                    direction,
                    kind,
                });
                self.push_log(format!(
                    "A falling block descends on {hero_name}; roll three physical combat dice."
                ));
            }
        }
    }

    pub fn pending_trap_roll(&self) -> Option<PendingTrapRoll> {
        self.pending_trap_roll
    }

    /// Applies a trap roll only after the visible tabletop dice have settled.
    /// Spear traps use one die and hit only on a skull; falling blocks use
    /// three dice, inflict one Body Point per skull, then permanently seal the
    /// trap square and force the printed ahead/back escape choice.
    pub fn resolve_trap_roll(
        &mut self,
        pending: PendingTrapRoll,
        faces: &[CombatFace],
    ) -> Result<(), RuleError> {
        if self.pending_trap_roll != Some(pending) {
            return Err(RuleError::StaleTrapRoll);
        }
        if faces.len() != pending.dice_count() as usize {
            return Err(RuleError::InvalidDice);
        }
        self.pending_trap_roll = None;
        let hero_name = self
            .unit(pending.hero)
            .map(|hero| hero.name.clone())
            .ok_or(RuleError::StaleTrapRoll)?;

        match pending.kind {
            TrapKind::Spear => {
                if faces[0] == CombatFace::Skull {
                    self.damage_without_defense(pending.hero, 1);
                    self.hero_turn.movement_left = 0;
                    self.push_log(format!(
                        "A spear struck {hero_name}: 1 Body Point lost; the turn ends."
                    ));
                    self.end_forced_hero_turn();
                } else {
                    self.push_log(format!(
                        "{hero_name} dodged the spear and may continue moving."
                    ));
                    self.check_terminal();
                }
            }
            TrapKind::FallingBlock => {
                let damage = faces
                    .iter()
                    .filter(|&&face| face == CombatFace::Skull)
                    .count() as u8;
                self.damage_without_defense(pending.hero, damage);
                self.hero_turn.movement_left = 0;
                let ahead = pending.trap.offset(pending.direction).filter(|&pos| {
                    self.boundary_is_open(pending.trap, pos)
                        && self.square_is_open_for_figure(pos, Some(pending.hero))
                });
                let back = self
                    .square_is_open_for_figure(pending.from, Some(pending.hero))
                    .then_some(pending.from);
                self.cells[Self::cell_index(pending.trap)].passable = false;
                self.push_log(format!(
                    "The ceiling collapsed on {hero_name}: {damage} Body Point(s), with no defense. Choose ahead or back."
                ));
                if self.unit(pending.hero).is_some_and(|hero| hero.alive) {
                    self.pending_falling_block = Some(PendingFallingBlock {
                        hero: pending.hero,
                        trap: pending.trap,
                        back,
                        ahead,
                    });
                    if back.is_none() || ahead.is_none() {
                        let only = back.or(ahead).expect("the entered square remains behind");
                        let escape_direction = Direction::ALL
                            .into_iter()
                            .find(|&candidate| pending.trap.offset(candidate) == Some(only))
                            .expect("falling block escape is adjacent");
                        self.resolve_falling_block_escape(escape_direction)
                            .expect("the only falling block escape is valid");
                    }
                } else {
                    self.end_forced_hero_turn();
                }
            }
            TrapKind::Pit => return Err(RuleError::StaleTrapRoll),
        }
        Ok(())
    }

    fn resolve_falling_block_escape(&mut self, direction: Direction) -> Result<Pos, RuleError> {
        let pending = self
            .pending_falling_block
            .ok_or(RuleError::InvalidFallingBlockChoice)?;
        let chosen = pending
            .trap
            .offset(direction)
            .filter(|pos| Some(*pos) == pending.back || Some(*pos) == pending.ahead)
            .ok_or(RuleError::InvalidFallingBlockChoice)?;
        let hero_name = {
            let hero = self
                .units
                .iter_mut()
                .find(|unit| unit.id == pending.hero)
                .expect("pending Hero exists");
            hero.pos = chosen;
            hero.in_pit = false;
            hero.name.clone()
        };
        self.pending_falling_block = None;
        self.push_log(format!(
            "{hero_name} escaped the collapse; the fallen stone now blocks the path."
        ));
        self.end_forced_hero_turn();
        Ok(chosen)
    }

    fn inventory_mut(&mut self, unit_id: UnitId) -> &mut Inventory {
        &mut self
            .units
            .iter_mut()
            .find(|unit| unit.id == unit_id)
            .expect("validated unit exists")
            .inventory
    }

    fn commit_hero_attack_source(&mut self, hero_id: UnitId, source: AttackSource) {
        let Some(hero) = self.units.iter_mut().find(|unit| unit.id == hero_id) else {
            return;
        };
        match source {
            AttackSource::Weapon(weapon) => hero.inventory.equipped_weapon = Some(weapon),
            AttackSource::ThrownDagger => {
                if let Some(index) = hero
                    .inventory
                    .weapons
                    .iter()
                    .position(|&weapon| weapon == Weapon::Dagger)
                {
                    hero.inventory.weapons.remove(index);
                }
                if hero.inventory.equipped_weapon == Some(Weapon::Dagger) {
                    hero.inventory.equipped_weapon = hero.inventory.weapons.first().copied();
                }
                let name = hero.name.clone();
                self.push_log(format!("{name}'s thrown Dagger is lost."));
            }
            AttackSource::OrcsBane => hero.inventory.equipped_weapon = Some(Weapon::Shortsword),
            AttackSource::SpiritBlade => hero.inventory.equipped_weapon = Some(Weapon::Broadsword),
            AttackSource::WizardsStaff => hero.inventory.equipped_weapon = Some(Weapon::Staff),
            AttackSource::Natural | AttackSource::Unarmed => {}
        }
    }

    pub fn pending_hero_death_choices(&self) -> Vec<HeroDeathChoice> {
        let Some(pending) = self.pending_hero_death else {
            return Vec::new();
        };
        if pending.potion_roll_pending {
            return Vec::new();
        }
        let Some(hero) = self.unit(pending.hero) else {
            return Vec::new();
        };
        let mut choices = Vec::new();
        if hero.equipment_available && hero.inventory.potion_of_healing > 0 {
            choices.push(HeroDeathChoice::HealingPotion);
        }
        let spell_save_allowed = hero.spellcasting_available
            && match self.phase {
                GamePhase::HeroTurn { .. } if self.active_hero_id() == Some(hero.id) => {
                    !self.hero_turn.action_used
                }
                GamePhase::ZargonTurn => !self.heroes_acted_this_round.contains(&hero.id),
                _ => false,
            };
        if spell_save_allowed && hero.hero_spells.contains(&HeroSpell::HealBody) {
            choices.push(HeroDeathChoice::HealBody);
        }
        if spell_save_allowed && hero.hero_spells.contains(&HeroSpell::WaterOfHealing) {
            choices.push(HeroDeathChoice::WaterOfHealing);
        }
        choices.push(HeroDeathChoice::AcceptDeath);
        choices
    }

    pub fn choose_pending_hero_death(
        &mut self,
        choice: HeroDeathChoice,
    ) -> Result<Option<HealingPotionUse>, RuleError> {
        let pending = self
            .pending_hero_death
            .ok_or(RuleError::InvalidHeroDeathChoice)?;
        if pending.potion_roll_pending || !self.pending_hero_death_choices().contains(&choice) {
            return Err(RuleError::InvalidHeroDeathChoice);
        }
        match choice {
            HeroDeathChoice::HealingPotion => {
                let use_result = self.consume_healing_potion(pending.hero)?;
                match use_result {
                    HealingPotionUse::Restored { .. } => self.pending_hero_death = None,
                    HealingPotionUse::RollRedDie { .. } => {
                        self.pending_hero_death = Some(PendingHeroDeath {
                            hero: pending.hero,
                            potion_roll_pending: true,
                        });
                    }
                }
                Ok(Some(use_result))
            }
            HeroDeathChoice::HealBody | HeroDeathChoice::WaterOfHealing => {
                let spell = match choice {
                    HeroDeathChoice::HealBody => HeroSpell::HealBody,
                    HeroDeathChoice::WaterOfHealing => HeroSpell::WaterOfHealing,
                    _ => unreachable!(),
                };
                let hero = self
                    .units
                    .iter_mut()
                    .find(|unit| unit.id == pending.hero)
                    .ok_or(RuleError::InvalidHeroDeathChoice)?;
                let card_index = hero
                    .hero_spells
                    .iter()
                    .position(|&card| card == spell)
                    .ok_or(RuleError::InvalidHeroDeathChoice)?;
                let discarded = hero.hero_spells.remove(card_index);
                hero.discarded_hero_spells.push(discarded);
                hero.body = 4.min(hero.stats.body) as i16;
                hero.alive = true;
                let name = hero.name.clone();
                self.pending_hero_death = None;
                if self.active_hero_id() == Some(pending.hero) {
                    self.hero_turn.action_used = true;
                }
                self.heroes_acted_this_round.insert(pending.hero);
                self.push_log(format!(
                    "{name} cast {} at zero Body Points and survived with {} Body Points.",
                    spell.name(),
                    self.unit(pending.hero).map_or(0, |hero| hero.body)
                ));
                Ok(None)
            }
            HeroDeathChoice::AcceptDeath => {
                self.pending_hero_death = None;
                self.finalize_hero_death(pending.hero);
                Ok(None)
            }
        }
    }

    pub fn choose_possession_recipient(&mut self, recipient: UnitId) -> Result<(), RuleError> {
        let pending = self
            .pending_possession_pickup
            .clone()
            .ok_or(RuleError::InvalidPossessionRecipient)?;
        if !pending.eligible_heroes.contains(&recipient) {
            return Err(RuleError::InvalidPossessionRecipient);
        }
        let dead_index = self
            .units
            .iter()
            .position(|unit| unit.id == pending.dead_hero)
            .ok_or(RuleError::InvalidPossessionRecipient)?;
        let recipient_index = self
            .units
            .iter()
            .position(|unit| unit.id == recipient)
            .ok_or(RuleError::InvalidPossessionRecipient)?;
        let loot = std::mem::take(&mut self.units[dead_index].inventory);
        let recipient_inventory = &mut self.units[recipient_index].inventory;
        recipient_inventory.gold = recipient_inventory.gold.saturating_add(loot.gold);
        recipient_inventory.fools_gold = recipient_inventory
            .fools_gold
            .saturating_add(loot.fools_gold);
        recipient_inventory.heroic_brew = recipient_inventory
            .heroic_brew
            .saturating_add(loot.heroic_brew);
        recipient_inventory.potion_of_defense = recipient_inventory
            .potion_of_defense
            .saturating_add(loot.potion_of_defense);
        recipient_inventory.potion_of_healing = recipient_inventory
            .potion_of_healing
            .saturating_add(loot.potion_of_healing);
        recipient_inventory.potion_of_strength = recipient_inventory
            .potion_of_strength
            .saturating_add(loot.potion_of_strength);
        recipient_inventory.petrification_potion = recipient_inventory
            .petrification_potion
            .saturating_add(loot.petrification_potion);
        recipient_inventory.tool_kits =
            recipient_inventory.tool_kits.saturating_add(loot.tool_kits);
        recipient_inventory
            .healing_potion_strengths
            .extend(loot.healing_potion_strengths);
        recipient_inventory.artifacts.extend(loot.artifacts);
        recipient_inventory.weapons.extend(loot.weapons);
        recipient_inventory.armor.extend(loot.armor);
        let dead_name = self.units[dead_index].name.clone();
        let recipient_name = self.units[recipient_index].name.clone();
        self.pending_possession_pickup = None;
        self.push_log(format!(
            "{recipient_name} picked up {dead_name}'s weapons, armor, artifacts, gold, and potions."
        ));
        Ok(())
    }

    fn resolve_zero_body(&mut self, unit_id: UnitId) -> bool {
        let Some(index) = self.units.iter().position(|unit| unit.id == unit_id) else {
            return false;
        };
        if self.units[index].body > 0 {
            self.units[index].alive = true;
            return false;
        }
        self.units[index].body = 0;
        if self.units[index].faction != Faction::Hero
            || !matches!(self.units[index].figure, FigureKind::Hero(_))
        {
            self.units[index].alive = false;
            self.drop_carried_quest_item_at(unit_id);
            return true;
        }
        self.units[index].alive = true;
        self.pending_hero_death = Some(PendingHeroDeath {
            hero: unit_id,
            potion_roll_pending: false,
        });
        let choices = self.pending_hero_death_choices();
        if choices == [HeroDeathChoice::AcceptDeath] {
            self.pending_hero_death = None;
            self.finalize_hero_death(unit_id);
            return true;
        }
        let name = self.units[index].name.clone();
        self.push_log(format!(
            "{name} reached zero Body Points. Choose an available Potion of Healing or unused healing spell now."
        ));
        false
    }

    fn finalize_hero_death(&mut self, hero_id: UnitId) {
        let Some(dead_index) = self.units.iter().position(|unit| unit.id == hero_id) else {
            return;
        };
        self.units[dead_index].body = 0;
        self.units[dead_index].alive = false;
        self.drop_carried_quest_item_at(hero_id);
        let pos = self.units[dead_index].pos;
        let region = self.cell(pos).map(|cell| cell.region);
        let eligible_heroes = self
            .units
            .iter()
            .filter(|unit| {
                unit.id != hero_id
                    && unit.alive
                    && !unit.escaped
                    && unit.faction == Faction::Hero
                    && region.is_some_and(|region| {
                        self.cell(unit.pos)
                            .is_some_and(|cell| cell.region == region)
                    })
            })
            .map(|unit| unit.id)
            .collect::<Vec<_>>();
        if !eligible_heroes.is_empty() {
            self.pending_possession_pickup = Some(PendingPossessionPickup {
                dead_hero: hero_id,
                eligible_heroes,
            });
        } else if let Some(monster_name) = self
            .units
            .iter()
            .find(|unit| {
                unit.alive
                    && !unit.dormant
                    && unit.faction == Faction::Monster
                    && region.is_some_and(|region| {
                        self.cell(unit.pos)
                            .is_some_and(|cell| cell.region == region)
                    })
            })
            .map(|monster| monster.name.clone())
        {
            for &artifact in &self.units[dead_index].inventory.artifacts {
                if !self.monster_stolen_artifacts.contains(&artifact) {
                    self.monster_stolen_artifacts.push(artifact);
                }
            }
            self.units[dead_index].inventory = Inventory::default();
            self.push_log(format!(
                "{monster_name} claimed the fallen Hero's possessions; they are removed from the game."
            ));
        }
        self.visual_sequence = self.visual_sequence.wrapping_add(1);
        self.last_combat_visual = Some(CombatVisualEvent {
            sequence: self.visual_sequence,
            attacker: hero_id,
            defender: hero_id,
            damage: 0,
            defender_died: true,
        });
        let name = self.units[dead_index].name.clone();
        self.push_log(format!(
            "{name} has fallen and is removed for the rest of the quest."
        ));
        self.check_terminal();
    }

    fn damage_without_defense(&mut self, unit_id: UnitId, damage: u8) {
        if let Some(unit) = self.units.iter_mut().find(|unit| unit.id == unit_id) {
            if unit.petrified_turns > 0 {
                return;
            }
            unit.body = (unit.body - damage as i16).max(0);
            if damage > 0 {
                unit.rock_skin = false;
            }
        }
        self.resolve_zero_body(unit_id);
    }

    fn damage_from_hero_spell(
        &mut self,
        caster_id: UnitId,
        target_id: UnitId,
        _spell: HeroSpell,
        damage: u8,
    ) {
        let Some(target_index) = self.units.iter().position(|unit| unit.id == target_id) else {
            return;
        };
        let target_name = self.units[target_index].name.clone();
        self.units[target_index].body = (self.units[target_index].body - damage as i16).max(0);
        if damage > 0 {
            self.units[target_index].rock_skin = false;
        }
        let defender_died = self.resolve_zero_body(target_id);
        self.visual_sequence = self.visual_sequence.wrapping_add(1);
        self.last_combat_visual = Some(CombatVisualEvent {
            sequence: self.visual_sequence,
            attacker: caster_id,
            defender: target_id,
            damage,
            defender_died,
        });
        if defender_died {
            let figure = self
                .unit(target_id)
                .map(|unit| unit.figure)
                .unwrap_or(FigureKind::Monster(MonsterKind::Goblin));
            if let FigureKind::Monster(kind) = figure {
                self.award_monster_bounty(caster_id, FigureKind::Monster(kind));
            }
            self.resolve_defeat_events(&target_name, Some(caster_id));
        }
    }

    fn drop_carried_quest_item_at(&mut self, unit_id: UnitId) -> String {
        let Some(unit_index) = self.units.iter().position(|unit| unit.id == unit_id) else {
            return String::new();
        };
        let Some(item_index) = self.units[unit_index].carried_quest_item.take() else {
            return String::new();
        };
        let pos = self.units[unit_index].pos;
        let item = &mut self.quest_items[item_index];
        item.holder = None;
        let name = item.id.clone();
        let prop = &mut self.props[item.prop_index];
        prop.pos = pos;
        prop.visible = true;
        prop.carried_by = None;
        name
    }

    fn deliver_carried_quest_item(&mut self, unit_id: UnitId) {
        let Some(unit_index) = self.units.iter().position(|unit| unit.id == unit_id) else {
            return;
        };
        if !self.stairs.contains(&self.units[unit_index].pos) {
            return;
        }
        let Some(item_index) = self.units[unit_index].carried_quest_item.take() else {
            return;
        };
        let hero_name = self.units[unit_index].name.clone();
        let item = &mut self.quest_items[item_index];
        item.holder = None;
        item.delivered = true;
        let item_name = item.id.clone();
        let prop = &mut self.props[item.prop_index];
        prop.visible = false;
        prop.carried_by = None;
        self.push_log(format!("{hero_name} returned {item_name} to the stairway."));
    }

    fn end_forced_hero_turn(&mut self) {
        self.check_terminal();
        if self.pending_hero_death.is_some() || self.pending_possession_pickup.is_some() {
            return;
        }
        if matches!(self.phase, GamePhase::HeroTurn { .. }) {
            self.end_hero_turn()
                .expect("a forced end starts from a valid Hero turn");
        }
    }

    fn spawn_wandering_monster(&mut self, searcher_id: UnitId) -> Option<UnitId> {
        let searcher = self.unit(searcher_id)?;
        let searcher_pos = searcher.pos;
        let region = self.cell(searcher_pos)?.region;
        let adjacent = Direction::ALL.into_iter().filter_map(|direction| {
            let pos = searcher_pos.offset(direction)?;
            (self.boundary_is_open(searcher_pos, pos) && self.square_is_open_for_figure(pos, None))
                .then_some(pos)
        });
        let closest_in_room = self.cells.iter().enumerate().filter_map(|(index, cell)| {
            if !cell.passable || cell.region != region {
                return None;
            }
            let pos = Pos::new(
                (index % BOARD_WIDTH as usize) as u8,
                (index / BOARD_WIDTH as usize) as u8,
            );
            self.square_is_open_for_figure(pos, None).then_some(pos)
        });
        let pos = adjacent.chain(closest_in_room).min_by_key(|pos| {
            (
                pos.x.abs_diff(searcher_pos.x) + pos.y.abs_diff(searcher_pos.y),
                pos.y,
                pos.x,
            )
        })?;
        let kind = self.wandering_monster;
        self.spawn_monster_at(kind, pos)
    }

    pub fn pending_hero_spell_resistance(&self) -> Option<(UnitId, ChaosSpell, u8)> {
        let hero = self.active_hero()?;
        let spell = [
            (hero.sleeping, ChaosSpell::Sleep),
            (hero.clouded, ChaosSpell::CloudOfChaos),
            (hero.fearful, ChaosSpell::Fear),
            (hero.commanded, ChaosSpell::Command),
        ]
        .into_iter()
        .find_map(|(applies, spell)| {
            (applies && !self.hero_turn.resolved_chaos_resistances.contains(&spell))
                .then_some(spell)
        })?;
        Some((hero.id, spell, hero.effective_mind()))
    }

    pub fn resolve_chaos_spell_resistance(
        &mut self,
        hero_id: UnitId,
        spell: ChaosSpell,
        dice: &[u8],
    ) -> Result<bool, RuleError> {
        let is_immediate_persistent_spell =
            matches!(spell, ChaosSpell::CloudOfChaos | ChaosSpell::Command)
                && self
                    .pending_chaos_spell_rolls
                    .front()
                    .is_some_and(|pending| pending.target == hero_id && pending.spell == spell);
        if is_immediate_persistent_spell
            || matches!(
                spell,
                ChaosSpell::BallOfFlame
                    | ChaosSpell::Firestorm
                    | ChaosSpell::SummonOrcs
                    | ChaosSpell::SummonUndead
            )
        {
            return self.resolve_immediate_chaos_spell_roll(hero_id, spell, dice);
        }
        let hero = self.unit(hero_id).ok_or(RuleError::StaleSpellResistance)?;
        let status_applies = match spell {
            ChaosSpell::Fear => hero.fearful,
            ChaosSpell::Sleep => hero.sleeping,
            ChaosSpell::CloudOfChaos => hero.clouded,
            ChaosSpell::Command => hero.commanded,
            _ => false,
        };
        if !hero.alive
            || hero.escaped
            || !status_applies
            || dice.len() != hero.effective_mind() as usize
            || dice.iter().any(|face| !(1..=6).contains(face))
        {
            return Err(RuleError::StaleSpellResistance);
        }
        let resisted = dice.contains(&6);
        let hero_name = hero.name.clone();
        if resisted {
            let hero = self
                .units
                .iter_mut()
                .find(|unit| unit.id == hero_id)
                .expect("validated Hero exists");
            match spell {
                ChaosSpell::Fear => hero.fearful = false,
                ChaosSpell::Sleep => hero.sleeping = false,
                ChaosSpell::CloudOfChaos => hero.clouded = false,
                ChaosSpell::Command => hero.commanded = false,
                _ => unreachable!("only persistent spells request resistance"),
            }
            self.push_log(format!("{hero_name} rolled a 6 and broke {spell:?}."));
        } else {
            self.push_log(format!("{hero_name} failed to break {spell:?}."));
        }
        if self.active_hero_id() == Some(hero_id) {
            self.hero_turn.resistance_resolved = true;
            if !self.hero_turn.resolved_chaos_resistances.contains(&spell) {
                self.hero_turn.resolved_chaos_resistances.push(spell);
            }
            if !resisted
                && matches!(
                    spell,
                    ChaosSpell::Sleep | ChaosSpell::CloudOfChaos | ChaosSpell::Command
                )
            {
                self.push_log(if spell == ChaosSpell::Sleep {
                    format!("{hero_name} remains asleep and misses the turn.")
                } else if spell == ChaosSpell::CloudOfChaos {
                    format!(
                        "{hero_name} remains paralyzed by the Cloud of Chaos and misses the turn."
                    )
                } else {
                    format!("{hero_name} remains under Zargon's Command and misses the Hero turn.")
                });
                self.end_forced_hero_turn();
            }
        }
        Ok(resisted)
    }

    fn resolve_immediate_chaos_spell_roll(
        &mut self,
        target_id: UnitId,
        spell: ChaosSpell,
        dice: &[u8],
    ) -> Result<bool, RuleError> {
        let pending = self
            .pending_chaos_spell_rolls
            .front()
            .copied()
            .ok_or(RuleError::StaleChaosSpellRoll)?;
        if pending.target != target_id
            || pending.spell != spell
            || dice.len() != pending.dice_count as usize
            || dice.iter().any(|face| !(1..=6).contains(face))
        {
            return Err(RuleError::StaleChaosSpellRoll);
        }
        self.pending_chaos_spell_rolls.pop_front();

        match spell {
            ChaosSpell::BallOfFlame | ChaosSpell::Firestorm => {
                let base_damage: u8 = if spell == ChaosSpell::BallOfFlame {
                    2
                } else {
                    3
                };
                let saves = dice.iter().filter(|&&face| face >= 5).count() as u8;
                let damage = base_damage.saturating_sub(saves);
                self.apply_chaos_spell_damage(pending.caster, target_id, spell, damage);
                Ok(damage == 0)
            }
            ChaosSpell::SummonOrcs => {
                let requested = match dice[0] {
                    1..=3 => 4,
                    4..=5 => 5,
                    _ => 6,
                };
                let spawned = self.summon_orcs_around(pending.caster, requested);
                let caster_name = self
                    .unit(pending.caster)
                    .map(|unit| unit.name.clone())
                    .unwrap_or_else(|| "The caster".to_owned());
                self.push_log(format!(
                    "{caster_name} summoned {spawned} of {requested} Orcs from a roll of {}.",
                    dice[0]
                ));
                Ok(spawned > 0)
            }
            ChaosSpell::SummonUndead => {
                let requested = match dice[0] {
                    1..=2 => vec![MonsterKind::Skeleton; 4],
                    3..=4 => {
                        let mut group = vec![MonsterKind::Skeleton; 3];
                        group.extend([MonsterKind::Zombie; 2]);
                        group
                    }
                    _ => {
                        let mut group = vec![MonsterKind::Zombie; 2];
                        group.extend([MonsterKind::Mummy; 2]);
                        group
                    }
                };
                let requested_count = requested.len();
                let spawned = self.summon_monsters_around(pending.caster, &requested);
                let caster_name = self
                    .unit(pending.caster)
                    .map(|unit| unit.name.clone())
                    .unwrap_or_else(|| "The caster".to_owned());
                self.push_log(format!(
                    "{caster_name} rolled {} and summoned {spawned} of {requested_count} Undead.",
                    dice[0]
                ));
                Ok(spawned > 0)
            }
            ChaosSpell::Command => {
                let resisted = dice.contains(&6);
                let target_name = self
                    .unit(target_id)
                    .map(|unit| unit.name.clone())
                    .unwrap_or_else(|| "The Hero".to_owned());
                if resisted {
                    if let Some(hero) = self.units.iter_mut().find(|unit| unit.id == target_id) {
                        hero.commanded = false;
                    }
                    self.push_log(format!(
                        "{target_name} rolled a 6 and broke Command immediately."
                    ));
                } else {
                    self.zargon_commanded_queue.push_front(target_id);
                    self.push_log(format!(
                        "{target_name} failed to break Command; Zargon now moves and attacks with that Hero."
                    ));
                }
                Ok(resisted)
            }
            ChaosSpell::CloudOfChaos => {
                let resisted = dice.contains(&6);
                let target_name = self
                    .unit(target_id)
                    .map(|unit| unit.name.clone())
                    .unwrap_or_else(|| "The Hero".to_owned());
                if let Some(hero) = self.units.iter_mut().find(|unit| unit.id == target_id) {
                    hero.clouded = !resisted;
                }
                self.push_log(if resisted {
                    format!("{target_name} rolled a 6 and broke the Cloud of Chaos immediately.")
                } else {
                    format!("{target_name} failed to break the Cloud of Chaos and is paralyzed.")
                });
                Ok(resisted)
            }
            _ => Err(RuleError::StaleChaosSpellRoll),
        }
    }

    fn try_cast_chaos_spell(&mut self, caster_id: UnitId) -> Option<ZargonStep> {
        let spells = self.unit(caster_id)?.chaos_spells.clone();
        for (spell_index, spell) in spells.into_iter().enumerate() {
            if let Some(step) = self.cast_chaos_spell(caster_id, spell_index, spell) {
                return Some(step);
            }
        }
        None
    }

    fn cast_chaos_spell(
        &mut self,
        caster_id: UnitId,
        spell_index: usize,
        spell: ChaosSpell,
    ) -> Option<ZargonStep> {
        let caster_pos = self.unit(caster_id)?.pos;
        let visible_heroes = self
            .units
            .iter()
            .filter(|unit| {
                unit.alive
                    && !unit.escaped
                    && matches!(unit.figure, FigureKind::Hero(_))
                    && unit.petrified_turns == 0
                    && self.can_see(caster_pos, unit.pos)
            })
            .map(|hero| hero.id)
            .collect::<Vec<_>>();
        let caster_name = self.unit(caster_id)?.name.clone();

        match spell {
            ChaosSpell::BallOfFlame => {
                let target = visible_heroes
                    .iter()
                    .filter_map(|&id| self.unit(id))
                    .max_by_key(|hero| (hero.stats.attack, std::cmp::Reverse(hero.id)))?
                    .id;
                self.consume_chaos_spell(caster_id, spell_index)?;
                self.pending_chaos_spell_rolls
                    .push_back(PendingChaosSpellRoll {
                        caster: caster_id,
                        target,
                        spell,
                        dice_count: 2,
                    });
                let target_name = self.unit(target)?.name.clone();
                self.push_log(format!(
                    "{caster_name} cast Ball of Flame on {target_name}; two red dice can reduce its 2 Body Points of damage."
                ));
                return self.pending_chaos_spell_step();
            }
            ChaosSpell::CloudOfChaos => {
                let region = self.cell(caster_pos)?.region;
                let mut victims = self
                    .units
                    .iter()
                    .filter(|unit| {
                        unit.alive
                            && !unit.escaped
                            && matches!(unit.figure, FigureKind::Hero(_))
                            && self
                                .cell(unit.pos)
                                .is_some_and(|cell| cell.region == region)
                    })
                    .map(|hero| hero.id)
                    .collect::<Vec<_>>();
                if victims.is_empty() {
                    return None;
                }
                victims.sort_unstable();
                self.consume_chaos_spell(caster_id, spell_index)?;
                for &target in &victims {
                    if let Some(hero) = self.units.iter_mut().find(|unit| unit.id == target) {
                        hero.clouded = true;
                    }
                }
                let rolls = victims
                    .into_iter()
                    .filter_map(|target| {
                        self.unit(target).map(|hero| PendingChaosSpellRoll {
                            caster: caster_id,
                            target,
                            spell,
                            dice_count: hero.effective_mind(),
                        })
                    })
                    .collect::<Vec<_>>();
                self.pending_chaos_spell_rolls.extend(rolls);
                self.push_log(format!(
                    "{caster_name} cast Cloud of Chaos; every Hero in the same room or corridor is paralyzed until rolling a 6 with Mind dice."
                ));
                return self.pending_chaos_spell_step();
            }
            ChaosSpell::Firestorm => {
                let region = self.cell(caster_pos)?.region;
                if region <= 0
                    || !visible_heroes.iter().any(|&id| {
                        self.unit(id)
                            .and_then(|hero| self.cell(hero.pos))
                            .is_some_and(|cell| cell.region == region)
                    })
                {
                    return None;
                }
                let mut victims = self
                    .units
                    .iter()
                    .filter(|unit| {
                        unit.id != caster_id
                            && unit.alive
                            && !unit.escaped
                            && self
                                .cell(unit.pos)
                                .is_some_and(|cell| cell.region == region)
                    })
                    .map(|unit| unit.id)
                    .collect::<Vec<_>>();
                victims.sort_unstable();
                self.consume_chaos_spell(caster_id, spell_index)?;
                self.pending_chaos_spell_rolls
                    .extend(victims.into_iter().map(|target| PendingChaosSpellRoll {
                        caster: caster_id,
                        target,
                        spell,
                        dice_count: 2,
                    }));
                self.push_log(format!(
                    "{caster_name} cast Firestorm throughout the room; every other figure faces 3 Body Points of damage."
                ));
                return self.pending_chaos_spell_step();
            }
            ChaosSpell::LightningBolt => {
                let (target, victims) = self.best_lightning_bolt(caster_id)?;
                self.consume_chaos_spell(caster_id, spell_index)?;
                let victim_count = victims.len();
                self.push_log(format!(
                    "{caster_name} cast Lightning Bolt in a straight line through {victim_count} figure(s); each suffers 2 Body Points with no defense."
                ));
                for victim in victims {
                    self.apply_chaos_spell_damage(caster_id, victim, spell, 2);
                }
                return Some(ZargonStep::Cast {
                    caster: caster_id,
                    target,
                    spell,
                    resistance_dice: 0,
                });
            }
            ChaosSpell::Rust => {
                let (target, equipment) = visible_heroes
                    .iter()
                    .filter_map(|&id| {
                        let hero = self.unit(id)?;
                        let equipment = Self::best_rust_target(hero)?;
                        Some((id, equipment))
                    })
                    .max_by_key(|&(id, equipment)| {
                        let attack = self.unit(id).map_or(0, |hero| hero.stats.attack);
                        (equipment.value(), attack, Reverse(id))
                    })?;
                self.consume_chaos_spell(caster_id, spell_index)?;
                let target_name = self.unit(target)?.name.clone();
                let equipment_name = equipment.name();
                self.destroy_rusted_equipment(target, equipment)?;
                self.push_log(format!(
                    "{caster_name} cast Rust on {target_name}; the non-artifact {equipment_name} is brittle, useless, and permanently discarded."
                ));
                return Some(ZargonStep::Cast {
                    caster: caster_id,
                    target,
                    spell,
                    resistance_dice: 0,
                });
            }
            ChaosSpell::SummonOrcs => {
                let region = self.cell(caster_pos)?.region;
                let has_space = self.cells.iter().enumerate().any(|(index, cell)| {
                    if !cell.passable || cell.region != region {
                        return false;
                    }
                    let pos = Self::pos_from_cell_index(index);
                    self.square_is_open_for_figure(pos, None)
                });
                if !has_space {
                    return None;
                }
                self.consume_chaos_spell(caster_id, spell_index)?;
                self.pending_chaos_spell_rolls
                    .push_back(PendingChaosSpellRoll {
                        caster: caster_id,
                        target: caster_id,
                        spell,
                        dice_count: 1,
                    });
                self.push_log(format!(
                    "{caster_name} cast Summon Orcs; one red die determines whether 4, 5, or 6 appear."
                ));
                return self.pending_chaos_spell_step();
            }
            ChaosSpell::SummonUndead => {
                let region = self.cell(caster_pos)?.region;
                let has_space = self.cells.iter().enumerate().any(|(index, cell)| {
                    if !cell.passable || cell.region != region {
                        return false;
                    }
                    let pos = Self::pos_from_cell_index(index);
                    self.square_is_open_for_figure(pos, None)
                });
                if !has_space {
                    return None;
                }
                self.consume_chaos_spell(caster_id, spell_index)?;
                self.pending_chaos_spell_rolls
                    .push_back(PendingChaosSpellRoll {
                        caster: caster_id,
                        target: caster_id,
                        spell,
                        dice_count: 1,
                    });
                self.push_log(format!(
                    "{caster_name} cast Summon Undead; one red die determines the exact Skeleton, Zombie, and Mummy group."
                ));
                return self.pending_chaos_spell_step();
            }
            ChaosSpell::Command => {
                let target = visible_heroes
                    .iter()
                    .filter_map(|&id| self.unit(id))
                    .filter(|hero| !hero.commanded && !hero.sleeping && !hero.clouded)
                    .max_by_key(|hero| (hero.stats.attack, std::cmp::Reverse(hero.id)))?
                    .id;
                self.consume_chaos_spell(caster_id, spell_index)?;
                let resistance_dice = self.unit(target)?.effective_mind();
                self.units
                    .iter_mut()
                    .find(|unit| unit.id == target)?
                    .commanded = true;
                self.pending_chaos_spell_rolls
                    .push_back(PendingChaosSpellRoll {
                        caster: caster_id,
                        target,
                        spell,
                        dice_count: resistance_dice,
                    });
                let target_name = self.unit(target)?.name.clone();
                self.push_log(format!(
                    "{caster_name} cast Command on {target_name}; Mind dice may break it immediately."
                ));
                return self.pending_chaos_spell_step();
            }
            ChaosSpell::Escape => {
                let target = self.unit(caster_id)?.escape_target?;
                if !self.square_is_open_for_figure(target, Some(caster_id)) {
                    return None;
                }
                self.consume_chaos_spell(caster_id, spell_index)?;
                self.units.iter_mut().find(|unit| unit.id == caster_id)?.pos = target;
                self.push_log(format!(
                    "{caster_name} cast Escape and vanished to the secret destination on Zargon's Quest Map."
                ));
                return Some(ZargonStep::Cast {
                    caster: caster_id,
                    target: caster_id,
                    spell,
                    resistance_dice: 0,
                });
            }
            _ => {}
        }

        let target = visible_heroes
            .iter()
            .filter_map(|&id| self.unit(id))
            .filter(|hero| match spell {
                ChaosSpell::Fear => !hero.fearful && !hero.sleeping && !hero.clouded,
                ChaosSpell::Sleep => !hero.sleeping && !hero.fearful && !hero.clouded,
                ChaosSpell::Tempest => hero.skip_turns == 0 && !hero.clouded,
                _ => false,
            })
            .min_by_key(|hero| match spell {
                ChaosSpell::Sleep => (hero.effective_mind(), hero.id),
                _ => (u8::MAX - hero.stats.attack, hero.id),
            })?
            .id;
        let target_name = self.unit(target)?.name.clone();
        self.consume_chaos_spell(caster_id, spell_index)?;
        let hero = self.units.iter_mut().find(|unit| unit.id == target)?;
        let resistance_dice = match spell {
            ChaosSpell::Fear => {
                hero.fearful = true;
                0
            }
            ChaosSpell::Sleep => {
                hero.sleeping = true;
                hero.effective_mind()
            }
            ChaosSpell::Tempest => {
                hero.skip_turns = hero.skip_turns.saturating_add(1);
                0
            }
            _ => return None,
        };
        self.push_log(format!("{caster_name} cast {spell:?} on {target_name}."));
        Some(ZargonStep::Cast {
            caster: caster_id,
            target,
            spell,
            resistance_dice,
        })
    }

    fn best_lightning_bolt(&self, caster_id: UnitId) -> Option<(UnitId, Vec<UnitId>)> {
        let caster = self.unit(caster_id)?;
        const DIRECTIONS: [(i16, i16); 8] = [
            (0, -1),
            (1, -1),
            (1, 0),
            (1, 1),
            (0, 1),
            (-1, 1),
            (-1, 0),
            (-1, -1),
        ];
        DIRECTIONS
            .into_iter()
            .enumerate()
            .filter_map(|(direction_index, (dx, dy))| {
                let victims = self
                    .lightning_ray(caster.pos, dx, dy)
                    .into_iter()
                    .filter_map(|pos| self.occupied_by_alive(pos, Some(caster_id)))
                    .collect::<Vec<_>>();
                let target = victims.iter().copied().find(|&id| {
                    self.unit(id)
                        .is_some_and(|unit| unit.faction == Faction::Hero)
                })?;
                let (hero_damage, hero_kills, monster_damage, monster_kills) = victims
                    .iter()
                    .filter_map(|&id| self.unit(id))
                    .fold((0_i32, 0_i32, 0_i32, 0_i32), |score, unit| {
                        let damage = unit.body.clamp(0, 2) as i32;
                        let killed = i32::from(unit.body <= 2);
                        if unit.faction == Faction::Hero {
                            (score.0 + damage, score.1 + killed, score.2, score.3)
                        } else {
                            (score.0, score.1, score.2 + damage, score.3 + killed)
                        }
                    });
                let score =
                    hero_damage * 10 + hero_kills * 100 - monster_damage * 3 - monster_kills * 30;
                Some((score, Reverse(direction_index), target, victims))
            })
            .max_by_key(|(score, direction, _, _)| (*score, *direction))
            .map(|(_, _, target, victims)| (target, victims))
    }

    fn lightning_ray(&self, origin: Pos, dx: i16, dy: i16) -> Vec<Pos> {
        let mut ray = Vec::new();
        let mut cursor = origin;
        loop {
            let next_x = cursor.x as i16 + dx;
            let next_y = cursor.y as i16 + dy;
            if !(0..BOARD_WIDTH as i16).contains(&next_x)
                || !(0..BOARD_HEIGHT as i16).contains(&next_y)
            {
                break;
            }
            let next = Pos::new(next_x as u8, next_y as u8);
            let open = if dx == 0 || dy == 0 {
                self.boundary_is_open(cursor, next)
            } else {
                // A perfectly diagonal center-to-center ray only touches the
                // shared corner; like the printed sight rule it does not cross
                // either orthogonal wall edge.
                self.cell(next).is_some_and(|cell| cell.passable)
            };
            if !open {
                break;
            }
            ray.push(next);
            cursor = next;
        }
        ray
    }

    fn best_rust_target(hero: &Unit) -> Option<RustedEquipment> {
        if !hero.equipment_available {
            return None;
        }
        hero.inventory
            .weapons
            .iter()
            .copied()
            .filter(|weapon| {
                matches!(
                    weapon,
                    Weapon::Shortsword | Weapon::Broadsword | Weapon::Longsword
                )
            })
            .map(RustedEquipment::Sword)
            .chain(
                hero.inventory
                    .armor
                    .contains(&Armor::Helmet)
                    .then_some(RustedEquipment::Helmet),
            )
            .max_by_key(|equipment| equipment.value())
    }

    fn destroy_rusted_equipment(
        &mut self,
        target_id: UnitId,
        equipment: RustedEquipment,
    ) -> Option<()> {
        let inventory = &mut self
            .units
            .iter_mut()
            .find(|unit| unit.id == target_id)?
            .inventory;
        match equipment {
            RustedEquipment::Sword(weapon) => {
                let index = inventory
                    .weapons
                    .iter()
                    .position(|&candidate| candidate == weapon)?;
                inventory.weapons.remove(index);
                if inventory.equipped_weapon == Some(weapon) && !inventory.weapons.contains(&weapon)
                {
                    inventory.equipped_weapon = inventory.weapons.first().copied();
                }
            }
            RustedEquipment::Helmet => {
                let index = inventory
                    .armor
                    .iter()
                    .position(|&armor| armor == Armor::Helmet)?;
                inventory.armor.remove(index);
            }
        }
        Some(())
    }

    fn consume_chaos_spell(&mut self, caster_id: UnitId, spell_index: usize) -> Option<ChaosSpell> {
        let spell = {
            let caster = self.units.iter_mut().find(|unit| unit.id == caster_id)?;
            caster.invulnerable_until_acts = false;
            if spell_index >= caster.chaos_spells.len() {
                return None;
            }
            let spell = caster.chaos_spells.remove(spell_index);
            caster.discarded_chaos_spells.push(spell);
            spell
        };
        self.discarded_chaos_spells.push(spell);
        Some(spell)
    }

    fn pending_chaos_spell_step(&self) -> Option<ZargonStep> {
        let pending = self.pending_chaos_spell_rolls.front()?;
        Some(ZargonStep::Cast {
            caster: pending.caster,
            target: pending.target,
            spell: pending.spell,
            resistance_dice: pending.dice_count,
        })
    }

    fn apply_chaos_spell_damage(
        &mut self,
        caster_id: UnitId,
        target_id: UnitId,
        spell: ChaosSpell,
        damage: u8,
    ) {
        let caster_name = self
            .unit(caster_id)
            .map(|unit| unit.name.clone())
            .unwrap_or_else(|| "The caster".to_owned());
        let Some(target_index) = self.units.iter().position(|unit| unit.id == target_id) else {
            return;
        };
        let target_name = self.units[target_index].name.clone();
        self.units[target_index].body -= damage as i16;
        if damage > 0 {
            self.units[target_index].rock_skin = false;
        }
        let target_died = self.resolve_zero_body(target_id);

        self.visual_sequence = self.visual_sequence.wrapping_add(1);
        self.last_combat_visual = Some(CombatVisualEvent {
            sequence: self.visual_sequence,
            attacker: caster_id,
            defender: target_id,
            damage,
            defender_died: target_died,
        });
        self.push_log(format!(
            "{target_name} suffered {damage} Body Points from {caster_name}'s {spell:?}{}.",
            if target_died { " and was defeated" } else { "" }
        ));
        if target_died {
            self.resolve_defeat_events(&target_name, Some(caster_id));
        }
        if self.pending_chaos_spell_rolls.is_empty() {
            self.check_terminal();
        }
    }

    fn summon_orcs_around(&mut self, caster_id: UnitId, requested: u8) -> u8 {
        self.summon_monsters_around(caster_id, &vec![MonsterKind::Orc; requested as usize])
    }

    fn summon_monsters_around(&mut self, caster_id: UnitId, requested: &[MonsterKind]) -> u8 {
        let Some(caster) = self.unit(caster_id) else {
            return 0;
        };
        let caster_pos = caster.pos;
        let Some(region) = self.cell(caster_pos).map(|cell| cell.region) else {
            return 0;
        };
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        for direction in Direction::ALL {
            if let Some(pos) = caster_pos.offset(direction)
                && self
                    .cell(pos)
                    .is_some_and(|cell| cell.passable && cell.region == region)
                && self.square_is_open_for_figure(pos, None)
                && seen.insert(pos)
            {
                candidates.push(pos);
            }
        }
        let mut remaining = self
            .cells
            .iter()
            .enumerate()
            .filter_map(|(index, cell)| {
                if !cell.passable || cell.region != region {
                    return None;
                }
                let pos = Self::pos_from_cell_index(index);
                (self.square_is_open_for_figure(pos, None) && seen.insert(pos)).then_some(pos)
            })
            .collect::<Vec<_>>();
        remaining.sort_unstable_by_key(|pos| {
            (
                pos.x.abs_diff(caster_pos.x) + pos.y.abs_diff(caster_pos.y),
                pos.y,
                pos.x,
            )
        });
        candidates.extend(remaining);

        let mut spawned = 0;
        for (pos, &kind) in candidates.into_iter().zip(requested.iter()) {
            if self.spawn_monster_at(kind, pos).is_some() {
                spawned += 1;
            }
        }
        spawned
    }

    fn spawn_monster_at(&mut self, kind: MonsterKind, pos: Pos) -> Option<UnitId> {
        if !self.square_is_open_for_figure(pos, None) {
            return None;
        }
        let stats = monster_stats(kind);
        let id = self.units.iter().map(|unit| unit.id).max().unwrap_or(0) + 1;
        self.units.push(Unit {
            id,
            name: kind.name().to_owned(),
            figure: FigureKind::Monster(kind),
            physical_figure: None,
            faction: Faction::Monster,
            model_variant: None,
            pos,
            stats,
            body: stats.body as i16,
            alive: true,
            in_pit: false,
            inventory: Inventory::default(),
            carried_quest_item: None,
            dormant: false,
            invulnerable_until_acts: false,
            equipment_available: true,
            spellcasting_available: false,
            escaped: false,
            chaos_spells: Vec::new(),
            discarded_chaos_spells: Vec::new(),
            hero_spells: Vec::new(),
            discarded_hero_spells: Vec::new(),
            spell_ring_spell: None,
            spell_ring_casts_left: 0,
            escape_target: None,
            immune_to_fire_spells: false,
            fearful: false,
            sleeping: false,
            clouded: false,
            hero_sleep_caster: None,
            skip_turns: 0,
            petrified_turns: 0,
            diagonal_attack: false,
            hidden_until_activated: false,
            immune_except_spirit_blade: false,
            champion: false,
            commanded: false,
            swift_wind: false,
            courage: false,
            rock_skin: false,
            pass_through_rock: false,
            veil_of_mist: false,
            potion_defense_bonus: 0,
        });
        if self.allocate_physical_piece(id) {
            Some(id)
        } else {
            self.units.pop();
            None
        }
    }

    fn monster_attack_plan(&self, monster_id: UnitId, hero_id: UnitId) -> Result<AttackPlan> {
        let attacker = self
            .unit(monster_id)
            .ok_or_else(|| anyhow::anyhow!("missing monster"))?;
        let defender = self
            .unit(hero_id)
            .ok_or_else(|| anyhow::anyhow!("missing hero"))?;
        Ok(AttackPlan {
            attacker: monster_id,
            defender: hero_id,
            source: AttackSource::Natural,
            attack_dice: pit_adjusted_dice(
                attacker.inventory.weapon_attack_dice(attacker.stats.attack),
                attacker.in_pit,
            ),
            defend_dice: pit_adjusted_dice(defender.effective_defense_dice(), defender.in_pit),
        })
    }

    fn resolve_monster_attack_random(&mut self, plan: AttackPlan) -> Result<()> {
        let mut tray = DiceTray::combat(self.rng.random(), plan.attack_dice + plan.defend_dice);
        let results = tray.simulate_to_rest();
        let mut faces = results.into_iter().filter_map(|result| match result {
            DieResult::Combat(face) => Some(face),
            DieResult::Movement(_) => None,
        });
        let attack: Vec<_> = faces.by_ref().take(plan.attack_dice as usize).collect();
        let defend: Vec<_> = faces.take(plan.defend_dice as usize).collect();
        self.resolve_attack(plan, &attack, &defend)?;
        Ok(())
    }

    fn random_combat_faces(&mut self, count: u8) -> Vec<CombatFace> {
        (0..count)
            .map(|_| match self.rng.random_range(0..6) {
                0..=2 => CombatFace::Skull,
                3..=4 => CombatFace::WhiteShield,
                _ => CombatFace::BlackShield,
            })
            .collect()
    }

    fn occupied_by_alive(&self, pos: Pos, except: Option<UnitId>) -> Option<UnitId> {
        self.units
            .iter()
            .find(|unit| unit.alive && !unit.escaped && unit.pos == pos && Some(unit.id) != except)
            .map(|unit| unit.id)
    }

    fn square_is_open_for_figure(&self, pos: Pos, except: Option<UnitId>) -> bool {
        self.cell(pos).is_some_and(|cell| cell.passable)
            && self.occupied_by_alive(pos, except).is_none()
            && !self.is_furniture_square(pos)
    }

    /// The US Hero movement rules permit a Hero to share an otherwise occupied
    /// square only on the stairway or in a sprung pit. This exception belongs
    /// to the moving Hero; the Monster rules separately forbid Monsters from
    /// sharing any square.
    fn hero_may_share_square(&self, mover_id: UnitId, pos: Pos) -> bool {
        self.unit(mover_id)
            .is_some_and(|mover| mover.faction == Faction::Hero)
            && (self.stairs.contains(&pos) || self.is_sprung_pit(pos))
    }

    fn adjacent_hero(&self, monster_id: UnitId) -> Option<UnitId> {
        let monster = self.unit(monster_id)?;
        self.units
            .iter()
            .filter(|unit| {
                unit.faction == Faction::Hero
                    && unit.alive
                    && !unit.escaped
                    && unit.petrified_turns == 0
            })
            .filter(|hero| self.attack_reaches(monster, hero))
            .min_by_key(|hero| self.zargon_hero_priority(hero))
            .map(|hero| hero.id)
    }

    fn adjacent_other_hero(&self, commanded_id: UnitId) -> Option<UnitId> {
        let commanded = self.unit(commanded_id)?;
        self.units
            .iter()
            .filter(|unit| {
                unit.id != commanded_id
                    && unit.faction == Faction::Hero
                    && unit.alive
                    && !unit.escaped
                    && unit.petrified_turns == 0
            })
            .filter(|hero| self.attack_reaches(commanded, hero))
            .min_by_key(|hero| self.zargon_hero_priority(hero))
            .map(|hero| hero.id)
    }

    /// Stable tactical preference for the computer game master. Zargon first
    /// presses Heroes closest to defeat, then those with fewer defense dice,
    /// and finally the more dangerous attackers. Unit id is the deterministic
    /// final tie-break, so a seeded replay never depends on allocation order in
    /// a hash collection.
    fn zargon_hero_priority(&self, hero: &Unit) -> (i16, u8, Reverse<u8>, UnitId) {
        let threat = if hero.inventory.fools_gold > 0 {
            0
        } else if !hero.equipment_available || hero.fearful {
            1
        } else {
            hero.inventory
                .weapon_attack_dice(hero.stats.attack)
                .saturating_add(u8::from(hero.courage) * 2)
        };
        (
            hero.body.max(0),
            pit_adjusted_dice(hero.effective_defense_dice(), hero.in_pit),
            Reverse(threat),
            hero.id,
        )
    }

    fn attack_reaches(&self, attacker: &Unit, defender: &Unit) -> bool {
        let dx = attacker.pos.x.abs_diff(defender.pos.x);
        let dy = attacker.pos.y.abs_diff(defender.pos.y);
        if attacker.pos.is_adjacent(defender.pos) {
            self.boundary_is_open(attacker.pos, defender.pos)
                && self.can_see(attacker.pos, defender.pos)
        } else {
            attacker.permits_diagonal_attack()
                && dx == 1
                && dy == 1
                && self.can_see(attacker.pos, defender.pos)
        }
    }

    fn attack_source_reaches(
        &self,
        attacker: &Unit,
        defender: &Unit,
        source: AttackSource,
    ) -> bool {
        if source == AttackSource::Natural {
            return self.attack_reaches(attacker, defender);
        }
        let dx = attacker.pos.x.abs_diff(defender.pos.x);
        let dy = attacker.pos.y.abs_diff(defender.pos.y);
        let orthogonally_adjacent = attacker.pos.is_adjacent(defender.pos)
            && self.boundary_is_open(attacker.pos, defender.pos)
            && self.can_see(attacker.pos, defender.pos);
        let diagonally_adjacent = dx == 1 && dy == 1 && self.can_see(attacker.pos, defender.pos);
        match source {
            AttackSource::Natural => unreachable!("handled above"),
            AttackSource::Unarmed
            | AttackSource::Weapon(
                Weapon::Dagger | Weapon::Shortsword | Weapon::Broadsword | Weapon::BattleAxe,
            )
            | AttackSource::OrcsBane
            | AttackSource::SpiritBlade => orthogonally_adjacent,
            AttackSource::Weapon(Weapon::Staff | Weapon::Longsword)
            | AttackSource::WizardsStaff => orthogonally_adjacent || diagonally_adjacent,
            AttackSource::Weapon(Weapon::Crossbow) | AttackSource::ThrownDagger => {
                (dx > 1 || dy > 1) && self.can_see(attacker.pos, defender.pos)
            }
        }
    }

    fn path_to_nearest_hero(&self, monster_id: UnitId) -> Option<Vec<Pos>> {
        self.units
            .iter()
            .filter(|hero| {
                hero.faction == Faction::Hero
                    && hero.alive
                    && !hero.escaped
                    && hero.petrified_turns == 0
            })
            .filter_map(|hero| {
                self.path_to_attack_target(monster_id, hero.id)
                    .map(|path| (path, self.zargon_hero_priority(hero)))
            })
            .min_by_key(|(path, priority)| (path.len(), *priority))
            .map(|(path, _)| path)
    }

    fn path_to_nearest_other_hero(&self, commanded_id: UnitId) -> Option<Vec<Pos>> {
        self.hero_order
            .iter()
            .copied()
            .filter(|&id| id != commanded_id)
            .filter(|&id| {
                self.unit(id)
                    .is_some_and(|hero| hero.alive && !hero.escaped && hero.petrified_turns == 0)
            })
            .filter_map(|target| self.path_to_attack_target(commanded_id, target))
            .min_by_key(|path| path.len())
    }

    fn path_to_attack_target(&self, monster_id: UnitId, target_id: UnitId) -> Option<Vec<Pos>> {
        let monster = self.unit(monster_id)?;
        let target = self.unit(target_id)?;
        let mut queue = VecDeque::from([monster.pos]);
        let mut previous: HashMap<Pos, Pos> = HashMap::new();
        let mut visited = HashSet::from([monster.pos]);
        let mut goal = None;

        while let Some(pos) = queue.pop_front() {
            if pos.is_adjacent(target.pos) && self.boundary_is_open(pos, target.pos) {
                goal = Some(pos);
                break;
            }
            for direction in Direction::ALL {
                let Some(next) = pos.offset(direction) else {
                    continue;
                };
                if visited.contains(&next)
                    || !self.boundary_is_open(pos, next)
                    || !self.square_is_open_for_figure(next, Some(monster_id))
                {
                    continue;
                }
                visited.insert(next);
                previous.insert(next, pos);
                queue.push_back(next);
            }
        }
        let mut cursor = goal?;
        let mut reversed = Vec::new();
        while cursor != monster.pos {
            reversed.push(cursor);
            cursor = *previous.get(&cursor)?;
        }
        reversed.reverse();
        Some(reversed)
    }

    fn reveal_from(&mut self, pos: Pos) {
        let index = Self::cell_index(pos);
        let region = self.cells[index].region;
        if region > 0 {
            for cell in &mut self.cells {
                if cell.region == region {
                    cell.revealed = true;
                }
            }
            let events: Vec<_> =
                self.quest_events
                    .iter_mut()
                    .filter(|event| {
                        !event.resolved && event.trigger == (QuestTrigger::RevealRoom { region })
                    })
                    .map(|event| {
                        event.resolved = true;
                        (
                            event.effect.clone(),
                            event.message.clone().unwrap_or_else(|| {
                                format!("Quest event {} was revealed.", event.id)
                            }),
                        )
                    })
                    .collect();
            for (effect, message) in events {
                match effect {
                    QuestEffectDef::Alarm {
                        ally,
                        forbid_treasure,
                    } => self.trigger_alarm(&ally, region, forbid_treasure),
                    other => self.apply_world_effect(&other),
                }
                self.push_log(message);
            }
        } else {
            let visible_corridors: Vec<_> = (0..BOARD_HEIGHT)
                .flat_map(|y| (0..BOARD_WIDTH).map(move |x| Pos::new(x, y)))
                .filter(|&target| {
                    let cell = &self.cells[Self::cell_index(target)];
                    cell.passable && cell.region == 0 && self.can_see(pos, target)
                })
                .collect();
            for target in visible_corridors {
                self.cells[Self::cell_index(target)].revealed = true;
            }
        }
        self.allocate_pending_visible_figures();
    }

    fn living_hero_count(&self) -> usize {
        self.hero_order
            .iter()
            .filter(|&&id| self.unit(id).is_some_and(|unit| unit.alive))
            .count()
    }

    fn active_hero_count(&self) -> usize {
        self.hero_order
            .iter()
            .filter(|&&id| {
                self.unit(id)
                    .is_some_and(|unit| unit.alive && !unit.escaped)
            })
            .count()
    }

    fn escaped_hero_count(&self) -> usize {
        self.hero_order
            .iter()
            .filter(|&&id| self.unit(id).is_some_and(|unit| unit.escaped))
            .count()
    }

    fn restore_recovered_equipment(&mut self, hero_id: UnitId) {
        let Some(recovery_region) = self.equipment_recovery_region else {
            return;
        };
        let should_restore = self.unit(hero_id).is_some_and(|hero| {
            hero.alive
                && !hero.escaped
                && matches!(hero.figure, FigureKind::Hero(_))
                && !hero.equipment_available
                && self
                    .cell(hero.pos)
                    .is_some_and(|cell| cell.region == recovery_region)
        });
        if !should_restore {
            return;
        }
        let hero = self
            .units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .expect("equipment owner exists");
        hero.equipment_available = true;
        hero.spellcasting_available = true;
        let hero_name = hero.name.clone();
        self.push_log(format!(
            "{hero_name} reclaimed all weapons, armor, potions, and spells."
        ));
    }

    fn escape_hero_on_stairs(&mut self, hero_id: UnitId) -> bool {
        if !matches!(
            self.objective,
            ObjectiveDef::EscapeIndependently | ObjectiveDef::DefeatAllOrEscapeIndependently
        ) {
            return false;
        }
        let should_escape = self
            .unit(hero_id)
            .is_some_and(|hero| hero.alive && !hero.escaped && self.stairs.contains(&hero.pos));
        if !should_escape {
            return false;
        }
        let hero = self
            .units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .expect("escaping Hero exists");
        hero.escaped = true;
        let hero_name = hero.name.clone();
        self.hero_turn.movement_left = 0;
        self.hero_turn.action_used = true;
        self.push_log(format!(
            "{hero_name} reached the stairway and escaped the dungeon."
        ));
        true
    }

    fn apply_quest_search_effect(&mut self, hero_id: UnitId, effect: &QuestEffectDef) {
        match effect {
            QuestEffectDef::Gold { amount } => {
                let inventory = &mut self
                    .units
                    .iter_mut()
                    .find(|unit| unit.id == hero_id)
                    .expect("active hero exists")
                    .inventory;
                inventory.gold = inventory.gold.saturating_add(*amount);
            }
            QuestEffectDef::PotionOfHealing { count } => {
                let inventory = &mut self
                    .units
                    .iter_mut()
                    .find(|unit| unit.id == hero_id)
                    .expect("active hero exists")
                    .inventory;
                inventory.potion_of_healing = inventory.potion_of_healing.saturating_add(*count);
                inventory
                    .healing_potion_strengths
                    .extend(std::iter::repeat_n(4, *count as usize));
            }
            QuestEffectDef::HealingPotion { count, restore } => {
                let inventory = &mut self
                    .units
                    .iter_mut()
                    .find(|unit| unit.id == hero_id)
                    .expect("active hero exists")
                    .inventory;
                inventory.potion_of_healing = inventory.potion_of_healing.saturating_add(*count);
                inventory
                    .healing_potion_strengths
                    .extend(std::iter::repeat_n(*restore, *count as usize));
            }
            QuestEffectDef::PetrificationPotion { count } => {
                let inventory = &mut self
                    .units
                    .iter_mut()
                    .find(|unit| unit.id == hero_id)
                    .expect("active hero exists")
                    .inventory;
                inventory.petrification_potion =
                    inventory.petrification_potion.saturating_add(*count);
            }
            QuestEffectDef::Artifact { artifact } => {
                let inventory = &mut self
                    .units
                    .iter_mut()
                    .find(|unit| unit.id == hero_id)
                    .expect("active hero exists")
                    .inventory;
                if !inventory.artifacts.contains(artifact) {
                    inventory.artifacts.push(*artifact);
                }
            }
            QuestEffectDef::ArtifactToHero { hero, artifact } => {
                self.grant_artifact_to_hero(*hero, *artifact);
            }
            QuestEffectDef::ArtifactToKiller { .. } => {}
            QuestEffectDef::ActivateNamed { name } => {
                self.activate_named_unit(name);
            }
            QuestEffectDef::AwakenGuardianUnlessTrapDisarmed { name, pos } => {
                self.awaken_guardian_from_trapped_treasure(hero_id, name, *pos);
            }
            QuestEffectDef::RevealSecretDoor { a, b } => {
                self.reveal_specific_secret_door(*a, *b);
            }
            QuestEffectDef::ForbidFurtherTreasure => {
                if let Some(region) = self
                    .unit(hero_id)
                    .and_then(|unit| self.cell(unit.pos))
                    .map(|cell| cell.region)
                    .filter(|&region| region > 0)
                {
                    self.forbidden_treasure_regions.insert(region);
                }
            }
            QuestEffectDef::RevealStoredEquipment => {
                self.equipment_recovery_region = self
                    .unit(hero_id)
                    .and_then(|unit| self.cell(unit.pos))
                    .map(|cell| cell.region)
                    .filter(|&region| region > 0);
                self.restore_recovered_equipment(hero_id);
            }
            QuestEffectDef::Weapon { weapon } => {
                let inventory = &mut self
                    .units
                    .iter_mut()
                    .find(|unit| unit.id == hero_id)
                    .expect("active hero exists")
                    .inventory;
                inventory.weapons.push(*weapon);
            }
            QuestEffectDef::Armor { armor } => {
                let inventory = &mut self
                    .units
                    .iter_mut()
                    .find(|unit| unit.id == hero_id)
                    .expect("active hero exists")
                    .inventory;
                if !inventory.armor.contains(armor) {
                    inventory.armor.push(*armor);
                }
            }
            QuestEffectDef::SplitGold { amount } => {
                self.split_gold_among_living_heroes(*amount);
            }
            QuestEffectDef::DamageUnlessTrapDisarmed { pos, amount } => {
                let trap_is_disarmed = self
                    .traps
                    .iter()
                    .find(|trap| trap.pos == *pos)
                    .is_some_and(|trap| trap.disarmed);
                if !trap_is_disarmed {
                    if let Some(trap) = self.traps.iter_mut().find(|trap| trap.pos == *pos) {
                        trap.discovered = true;
                        trap.sprung = true;
                    }
                    self.damage_without_defense(hero_id, *amount);
                }
            }
            QuestEffectDef::Bundle { effects } => {
                for effect in effects {
                    self.apply_quest_search_effect(hero_id, effect);
                }
            }
            QuestEffectDef::Alarm {
                ally,
                forbid_treasure,
            } => {
                let region = self
                    .unit(hero_id)
                    .and_then(|unit| self.cell(unit.pos))
                    .map(|cell| cell.region)
                    .unwrap_or(-1);
                self.trigger_alarm(ally, region, *forbid_treasure);
            }
            QuestEffectDef::Empty | QuestEffectDef::Message => {}
        }
    }

    fn resolve_open_door_events(&mut self, a: Pos, b: Pos) {
        let events: Vec<_> = self
            .quest_events
            .iter_mut()
            .filter(|event| {
                !event.resolved
                    && matches!(
                        &event.trigger,
                        QuestTrigger::OpenDoor { a: event_a, b: event_b }
                            if (*event_a == a && *event_b == b)
                                || (*event_a == b && *event_b == a)
                    )
            })
            .map(|event| {
                event.resolved = true;
                (event.effect.clone(), event.message.clone())
            })
            .collect();
        for (effect, message) in events {
            self.apply_world_effect(&effect);
            if let Some(message) = message {
                self.push_log(message);
            }
        }
    }

    fn apply_world_effect(&mut self, effect: &QuestEffectDef) {
        match effect {
            QuestEffectDef::ActivateNamed { name } => self.activate_named_unit(name),
            QuestEffectDef::AwakenGuardianUnlessTrapDisarmed { .. } => {}
            QuestEffectDef::RevealSecretDoor { a, b } => self.reveal_specific_secret_door(*a, *b),
            QuestEffectDef::SplitGold { amount } => self.split_gold_among_living_heroes(*amount),
            QuestEffectDef::ArtifactToHero { hero, artifact } => {
                self.grant_artifact_to_hero(*hero, *artifact);
            }
            QuestEffectDef::ArtifactToKiller { .. } => {}
            QuestEffectDef::Bundle { effects } => {
                for effect in effects {
                    self.apply_world_effect(effect);
                }
            }
            _ => {}
        }
    }

    fn activate_named_unit(&mut self, name: &str) {
        if let Some(unit) = self.units.iter_mut().find(|unit| unit.name == name) {
            unit.dormant = false;
            unit.hidden_until_activated = false;
        }
        self.allocate_pending_visible_figures();
    }

    fn awaken_guardian_from_trapped_treasure(
        &mut self,
        searcher: UnitId,
        name: &str,
        trap_pos: Pos,
    ) {
        let disarmed = self
            .traps
            .iter()
            .find(|trap| trap.pos == trap_pos)
            .is_some_and(|trap| trap.disarmed);
        if disarmed {
            self.push_log(format!(
                "The chest trap was safely disarmed. Zargon reveals that {name} would have sprung to life and attacked."
            ));
            return;
        }
        if let Some(trap) = self.traps.iter_mut().find(|trap| trap.pos == trap_pos) {
            trap.discovered = true;
            trap.sprung = true;
        }
        let Some(guardian_id) = self
            .units
            .iter()
            .find(|unit| unit.name == name && unit.alive)
            .map(|unit| unit.id)
        else {
            return;
        };
        self.activate_named_unit(name);

        let movement = self
            .unit(guardian_id)
            .map(|guardian| guardian.stats.movement as usize)
            .unwrap_or(0);
        if let Some(path) = self.path_to_attack_target(guardian_id, searcher)
            && let Some(to) = path.into_iter().take(movement).last()
        {
            let from = self.unit(guardian_id).expect("guardian exists").pos;
            if to != from {
                let lands_in_pit = self.is_sprung_pit(to);
                if let Some(guardian) = self.units.iter_mut().find(|unit| unit.id == guardian_id) {
                    guardian.pos = to;
                    guardian.in_pit = lands_in_pit;
                    guardian.invulnerable_until_acts = false;
                }
                self.push_log(format!("{name} springs to life and moves to attack!"));
            }
        }
        if self
            .unit(guardian_id)
            .zip(self.unit(searcher))
            .is_some_and(|(guardian, hero)| self.attack_reaches(guardian, hero))
        {
            if let Some(guardian) = self.units.iter_mut().find(|unit| unit.id == guardian_id) {
                guardian.invulnerable_until_acts = false;
            }
            if let Ok(plan) = self.monster_attack_plan(guardian_id, searcher) {
                self.pending_forced_attack = Some(plan);
                self.push_log(format!("{name} immediately attacks the searching Hero."));
            }
        }
    }

    pub fn take_pending_forced_attack(&mut self) -> Option<AttackPlan> {
        self.pending_forced_attack.take()
    }

    fn reveal_specific_secret_door(&mut self, a: Pos, b: Pos) {
        if let Some(door) = self
            .doors
            .iter_mut()
            .find(|door| door.secret && door.connects(a, b))
        {
            door.discovered = true;
        }
    }

    fn resolve_defeat_events(&mut self, defeated_name: &str, killer: Option<UnitId>) {
        let events: Vec<_> = self
            .quest_events
            .iter_mut()
            .filter(|event| {
                !event.resolved
                    && event.trigger
                        == (QuestTrigger::DefeatNamed {
                            name: defeated_name.to_owned(),
                        })
            })
            .map(|event| {
                event.resolved = true;
                (event.effect.clone(), event.message.clone())
            })
            .collect();
        for (effect, message) in events {
            if let QuestEffectDef::ArtifactToKiller { artifact } = effect {
                if let Some(killer) = killer {
                    self.grant_artifact_to_unit(killer, artifact);
                }
            } else {
                self.apply_world_effect(&effect);
            }
            if let Some(message) = message {
                self.push_log(message);
            }
        }
    }

    fn grant_artifact_to_unit(&mut self, unit_id: UnitId, artifact: Artifact) {
        let Some(unit) = self
            .units
            .iter_mut()
            .find(|unit| unit.id == unit_id && unit.alive && unit.faction == Faction::Hero)
        else {
            return;
        };
        if !unit.inventory.artifacts.contains(&artifact) {
            unit.inventory.artifacts.push(artifact);
        }
    }

    fn award_monster_bounty(&mut self, killer: UnitId, defeated: FigureKind) {
        let FigureKind::Monster(monster) = defeated else {
            return;
        };
        let Some(&gold) = self.monster_bounties.get(&monster) else {
            return;
        };
        let Some(hero) = self
            .units
            .iter_mut()
            .find(|unit| unit.id == killer && unit.alive && unit.faction == Faction::Hero)
        else {
            return;
        };
        hero.inventory.gold = hero.inventory.gold.saturating_add(gold);
        let hero_name = hero.name.clone();
        self.push_log(format!(
            "{hero_name} earned the Emperor's {gold}-gold bounty for defeating a {}.",
            monster.name()
        ));
    }

    fn grant_artifact_to_hero(&mut self, hero_kind: crate::model::HeroKind, artifact: Artifact) {
        let Some(hero) = self
            .units
            .iter_mut()
            .find(|unit| unit.alive && unit.figure == FigureKind::Hero(hero_kind))
        else {
            return;
        };
        if !hero.inventory.artifacts.contains(&artifact) {
            hero.inventory.artifacts.push(artifact);
        }
    }

    fn split_gold_among_living_heroes(&mut self, amount: u16) {
        let living: Vec<_> = self
            .hero_order
            .iter()
            .copied()
            .filter(|&id| self.unit(id).is_some_and(|unit| unit.alive))
            .collect();
        if living.is_empty() {
            return;
        }
        let share = amount / living.len() as u16;
        let mut remainder = amount % living.len() as u16;
        for id in living {
            let bonus = u16::from(remainder > 0);
            remainder = remainder.saturating_sub(1);
            let inventory = &mut self
                .units
                .iter_mut()
                .find(|unit| unit.id == id)
                .expect("living hero exists")
                .inventory;
            inventory.gold = inventory.gold.saturating_add(share + bonus);
        }
    }

    fn trigger_alarm(&mut self, ally_name: &str, region: i16, forbid_treasure: bool) {
        if let Some(ally_id) = self
            .units
            .iter()
            .find(|unit| unit.name == ally_name && unit.faction == Faction::Hero)
            .map(|unit| unit.id)
            && let Some(controller) = self.active_hero_id()
        {
            self.escorted_ally = Some((ally_id, controller));
        }
        if forbid_treasure {
            self.forbidden_treasure_regions.insert(region);
        }
        for door in &mut self.doors {
            door.open = true;
            door.discovered = true;
        }
        for cell in &mut self.cells {
            if cell.passable {
                cell.revealed = true;
            }
        }
    }

    fn check_terminal(&mut self) {
        if matches!(
            self.phase,
            GamePhase::Won | GamePhase::Retreated | GamePhase::Lost
        ) {
            return;
        }
        if self.pending_hero_death.is_some() {
            return;
        }
        if matches!(
            self.objective,
            ObjectiveDef::EscapeIndependently | ObjectiveDef::DefeatAllOrEscapeIndependently
        ) {
            if matches!(self.objective, ObjectiveDef::DefeatAllOrEscapeIndependently)
                && self
                    .units
                    .iter()
                    .filter(|unit| unit.faction == Faction::Monster)
                    .all(|unit| !unit.alive)
            {
                self.discard_fools_gold_at_quest_end();
                self.phase = GamePhase::Won;
                self.push_log("Every monster trapped in the castle has been defeated.".to_owned());
                return;
            }
            if self.active_hero_count() == 0 {
                if self.escaped_hero_count() > 0 {
                    if matches!(self.objective, ObjectiveDef::DefeatAllOrEscapeIndependently) {
                        self.discard_fools_gold_at_quest_end();
                    }
                    self.phase = GamePhase::Won;
                    self.push_log("Every surviving Hero escaped independently.".to_owned());
                } else {
                    self.phase = GamePhase::Lost;
                    self.push_log("No Hero escaped the dungeon.".to_owned());
                }
            }
            return;
        }
        if self.living_hero_count() == 0 {
            self.phase = GamePhase::Lost;
            self.push_log("No hero remains standing.".to_owned());
            return;
        }
        let all_living_on_stairs = self.hero_order.iter().all(|&id| {
            self.unit(id)
                .is_none_or(|hero| !hero.alive || self.stairs.contains(&hero.pos))
        });
        let objective_done = match &self.objective {
            ObjectiveDef::DefeatNamed { name, .. }
            | ObjectiveDef::DefeatNamedAndReturn { name, .. } => self
                .units
                .iter()
                .find(|unit| unit.name == *name)
                .is_some_and(|unit| !unit.alive),
            ObjectiveDef::DefeatAllAndReturn => self
                .units
                .iter()
                .filter(|unit| unit.faction == Faction::Monster)
                .all(|unit| !unit.alive || unit.dormant),
            ObjectiveDef::DefeatAll => self
                .units
                .iter()
                .filter(|unit| unit.faction == Faction::Monster)
                .all(|unit| !unit.alive || unit.dormant),
            ObjectiveDef::ReachStairs => true,
            ObjectiveDef::RescueNamedAndReturn { name, .. } => self
                .units
                .iter()
                .find(|unit| unit.name == *name && unit.faction == Faction::Hero)
                .is_some_and(|unit| unit.alive && self.stairs.contains(&unit.pos)),
            ObjectiveDef::ReturnQuestItems { count, .. } => {
                self.quest_items
                    .iter()
                    .filter(|item| item.delivered)
                    .count()
                    >= *count as usize
            }
            ObjectiveDef::FindArtifactAndReturn { artifact } => self.hero_order.iter().any(|&id| {
                self.unit(id)
                    .is_some_and(|hero| hero.alive && hero.inventory.artifacts.contains(artifact))
            }),
            ObjectiveDef::ResolveEventAndReturn { event, .. } => self
                .quest_events
                .iter()
                .find(|quest_event| quest_event.id == *event)
                .is_some_and(|quest_event| quest_event.resolved),
            ObjectiveDef::EscapeIndependently | ObjectiveDef::DefeatAllOrEscapeIndependently => {
                unreachable!("handled before return objectives")
            }
        };
        let heroes_must_return = !matches!(
            self.objective,
            ObjectiveDef::DefeatNamed { .. }
                | ObjectiveDef::RescueNamedAndReturn { .. }
                | ObjectiveDef::DefeatAll
        );
        if objective_done && (!heroes_must_return || all_living_on_stairs) {
            let rescue_reward = match &self.objective {
                ObjectiveDef::RescueNamedAndReturn { reward_gold, .. }
                | ObjectiveDef::ReturnQuestItems { reward_gold, .. } => Some(*reward_gold),
                _ => None,
            };
            if let Some(reward_gold) = rescue_reward {
                if self.living_hero_count() > 0 {
                    self.split_gold_among_living_heroes(reward_gold);
                    self.push_log(format!(
                        "Prince Magnus awarded the surviving Heroes {reward_gold} gold coins."
                    ));
                }
            }
            let reward_gold_per_hero = match &self.objective {
                ObjectiveDef::DefeatNamedAndReturn {
                    reward_gold_per_hero,
                    ..
                }
                | ObjectiveDef::ResolveEventAndReturn {
                    reward_gold_per_hero,
                    ..
                } => *reward_gold_per_hero,
                _ => 0,
            };
            if reward_gold_per_hero > 0 {
                for hero_id in self.hero_order.clone() {
                    if let Some(hero) = self
                        .units
                        .iter_mut()
                        .find(|unit| unit.id == hero_id && unit.alive)
                    {
                        hero.inventory.gold =
                            hero.inventory.gold.saturating_add(reward_gold_per_hero);
                    }
                }
                self.push_log(format!(
                    "Each Hero returning safely receives {reward_gold_per_hero} gold coins."
                ));
            }
            if matches!(
                self.objective,
                ObjectiveDef::DefeatNamed {
                    award_champion_title: true,
                    ..
                }
            ) {
                for &hero_id in &self.hero_order {
                    if let Some(hero) = self
                        .units
                        .iter_mut()
                        .find(|unit| unit.id == hero_id && unit.alive)
                    {
                        hero.champion = true;
                    }
                }
                self.push_log(
                    "The Emperor awards every surviving Hero the title of Champion.".to_owned(),
                );
            }
            self.phase = GamePhase::Won;
            self.push_log(match &self.objective {
                ObjectiveDef::DefeatNamed { .. } => {
                    "The Witch Lord has vanished in foul black smoke; the campaign is won."
                        .to_owned()
                }
                ObjectiveDef::DefeatAll => {
                    "Every awakened monster in the Bastion of Chaos has been defeated.".to_owned()
                }
                _ => "The surviving heroes return safely to the stairway.".to_owned(),
            });
        }
    }

    fn discard_fools_gold_at_quest_end(&mut self) {
        let mut discarded = 0_u32;
        for &hero_id in &self.hero_order {
            if let Some(hero) = self.units.iter_mut().find(|unit| unit.id == hero_id) {
                discarded += u32::from(std::mem::take(&mut hero.inventory.fools_gold));
            }
        }
        if discarded > 0 {
            self.push_log(format!(
                "Ollar's mine treasure was fool's gold: all {discarded} carried coins vanish, while other treasure remains real."
            ));
        }
    }

    fn push_log(&mut self, message: String) {
        if self.log.len() == 16 {
            self.log.pop_front();
        }
        self.log.push_back(message);
    }
}

fn manhattan(a: Pos, b: Pos) -> u8 {
    a.x.abs_diff(b.x).saturating_add(a.y.abs_diff(b.y))
}

fn pos_from_cell_index(index: usize) -> Pos {
    Pos::new(
        (index % BOARD_WIDTH as usize) as u8,
        (index / BOARD_WIDTH as usize) as u8,
    )
}

fn pit_adjusted_dice(base: u8, in_pit: bool) -> u8 {
    if in_pit {
        base.saturating_sub(1).max(1)
    } else {
        base
    }
}

fn decrement_owned(count: &mut u8) -> Result<(), RuleError> {
    if *count == 0 {
        return Err(RuleError::NoPotionToGive);
    }
    *count -= 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::campaign::Campaign;
    use crate::quest::{DoorDef, QuestEventDef, QuestTriggerDef, TrapDef};

    fn game_for_hero_spell(spells: &[HeroSpell]) -> (Game, UnitId, UnitId) {
        let mut game = Game::demo(0x5350_454c_4c).unwrap();
        let wizard = game.hero_order[3];
        game.phase = GamePhase::HeroTurn { order_index: 3 };
        game.hero_turn = HeroTurnState::default();
        game.cells.iter_mut().for_each(|cell| cell.revealed = true);
        game.doors.iter_mut().for_each(|door| door.open = true);
        let target = game
            .units
            .iter()
            .find(|unit| unit.name == "Crypt Warden")
            .unwrap()
            .id;
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster && unit.id != target)
            .for_each(|unit| unit.alive = false);
        let wizard_unit = game
            .units
            .iter_mut()
            .find(|unit| unit.id == wizard)
            .unwrap();
        wizard_unit.pos = Pos::new(14, 2);
        wizard_unit.spellcasting_available = true;
        wizard_unit.hero_spells = spells.to_vec();
        let monster = game
            .units
            .iter_mut()
            .find(|unit| unit.id == target)
            .unwrap();
        monster.pos = Pos::new(15, 2);
        monster.body = monster.stats.body as i16;
        game.allocate_pending_visible_figures();
        (game, wizard, target)
    }

    #[test]
    fn movement_uses_two_physical_d6_results() {
        let mut game = Game::demo(7).unwrap();
        assert_eq!(game.apply_movement_roll(&[2, 5]).unwrap(), 7);
        assert_eq!(game.hero_turn.movement_left, 7);
        assert_eq!(
            game.apply_movement_roll(&[1, 1]).unwrap_err().to_string(),
            "movement dice have already been rolled"
        );
    }

    #[test]
    fn quest_validation_rejects_wrong_editions_duplicate_edges_and_box_overflow() {
        let mut wrong_edition = QuestDefinition::demo().unwrap();
        wrong_edition.source = Some(crate::quest::QuestSourceDef {
            edition: "HeroQuest 2021 remake".to_owned(),
            book: "Quest Book".to_owned(),
            page: 3,
        });
        let error = Game::from_quest(wrong_edition, 1).err().unwrap();
        assert!(error.to_string().contains("not the original US release"));

        let mut duplicate_door = QuestDefinition::demo().unwrap();
        duplicate_door.doors.push(duplicate_door.doors[0].clone());
        let error = Game::from_quest(duplicate_door, 1).err().unwrap();
        assert!(error.to_string().contains("same wall edge"));

        let mut too_many_chests = QuestDefinition::demo().unwrap();
        let chest = too_many_chests
            .props
            .iter()
            .find(|prop| prop.prop == PropKind::Chest)
            .cloned()
            .unwrap();
        while too_many_chests
            .props
            .iter()
            .filter(|prop| prop.prop == PropKind::Chest)
            .count()
            <= PropKind::Chest
                .original_us_physical_count()
                .expect("the treasure chests are physical components")
        {
            too_many_chests.props.push(chest.clone());
        }
        let error = Game::from_quest(too_many_chests, 1).err().unwrap();
        assert!(error.to_string().contains("physical count for Chest"));

        let mut impossible_reveal = QuestDefinition::original_us_trial().unwrap();
        let template = impossible_reveal.monsters[0].clone();
        impossible_reveal.monsters.clear();
        impossible_reveal.objective = ObjectiveDef::DefeatAll;
        for (index, pos) in (7..10)
            .flat_map(|y| (10..16).map(move |x| Pos::new(x, y)))
            .enumerate()
        {
            let mut monster = template.clone();
            monster.monster = MonsterKind::Orc;
            monster.pos = pos;
            monster.name = None;
            monster.model_variant = None;
            impossible_reveal.monsters.push(monster);
            assert!(index < 18);
        }
        let error = Game::from_quest(impossible_reveal, 1).err().unwrap();
        assert!(
            error
                .to_string()
                .contains("one reveal batch requires 18 green figures")
        );

        let mut trap_on_figure = QuestDefinition::original_us_trial().unwrap();
        trap_on_figure.traps.push(TrapDef {
            trap: TrapKind::Pit,
            pos: trap_on_figure.monsters[0].pos,
            trigger_on_entry: true,
            disarmable: true,
        });
        let error = Game::from_quest(trap_on_figure, 1).err().unwrap();
        assert!(
            error
                .to_string()
                .contains("trap overlaps a starting figure")
        );

        let mut out_of_bounds = QuestDefinition::original_us_trial().unwrap();
        out_of_bounds.monsters[0].pos = Pos::new(255, 255);
        let error = Game::from_quest(out_of_bounds, 1).err().unwrap();
        assert!(error.to_string().contains("figure is outside the board"));

        let mut mismatched_stairs = QuestDefinition::original_us_trial().unwrap();
        mismatched_stairs.stairs.pop();
        let error = Game::from_quest(mismatched_stairs, 1).err().unwrap();
        assert!(
            error
                .to_string()
                .contains("physical stairway footprint must match")
        );
    }

    #[test]
    fn a_closed_door_blocks_even_when_both_endpoints_share_a_region() {
        let mut game = Game::demo(0x444f_4f52_5741_4c4c).unwrap();
        let door = game.doors[0].clone();
        let region = game.cell(door.a).unwrap().region;
        game.cells[Game::cell_index(door.b)].region = region;
        game.doors[0].open = false;
        assert!(!game.boundary_is_open(door.a, door.b));
        game.doors[0].open = true;
        assert!(game.boundary_is_open(door.a, door.b));
    }

    #[test]
    fn highlighted_move_targets_are_exactly_the_clickable_next_hops() {
        let mut game = Game::demo(7).unwrap();
        assert!(game.active_move_targets().is_empty());
        game.apply_movement_roll(&[2, 5]).unwrap();
        let (target, direction) = game
            .active_move_targets()
            .into_iter()
            .next()
            .expect("the starting Hero has an open neighboring square");
        assert_eq!(game.active_move_direction_to(target), Some(direction));
        assert_eq!(game.move_active(direction).unwrap(), target);
    }

    #[test]
    fn heroes_may_pass_through_each_other_and_may_share_the_stairway() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_return_to_barak_tor().unwrap(),
            0x5041_5353,
        )
        .unwrap();
        let barbarian = game.hero_order[0];
        let dwarf = game.hero_order[1];
        assert_eq!(game.unit(barbarian).unwrap().pos, Pos::new(1, 14));
        assert_eq!(game.unit(dwarf).unwrap().pos, Pos::new(2, 14));

        game.apply_movement_roll(&[3, 4]).unwrap();
        assert_eq!(
            game.active_move_direction_to(Pos::new(2, 14)),
            Some(Direction::East)
        );
        game.move_active(Direction::East).unwrap();
        assert!(game.end_hero_turn().is_ok());
        assert_eq!(game.unit(barbarian).unwrap().pos, Pos::new(2, 14));
        assert_eq!(game.unit(dwarf).unwrap().pos, Pos::new(2, 14));
    }

    #[test]
    fn a_hero_may_end_in_an_occupied_sprung_pit_but_a_monster_may_not_share() {
        let mut game = Game::demo(0x5049_5453_4841_5245).unwrap();
        let barbarian = game.hero_order[0];
        let dwarf = game.hero_order[1];
        let monster = game
            .units
            .iter()
            .find(|unit| unit.faction == Faction::Monster)
            .unwrap()
            .id;
        let start = Pos::new(10, 10);
        let pit = Pos::new(11, 10);

        for cell in &mut game.cells {
            cell.passable = false;
            cell.revealed = true;
        }
        for pos in [start, pit, Pos::new(12, 10)] {
            let cell = &mut game.cells[Game::cell_index(pos)];
            cell.passable = true;
            cell.region = 77;
        }
        game.props.clear();
        game.traps.clear();
        game.traps.push(Trap {
            kind: TrapKind::Pit,
            pos: pit,
            discovered: true,
            sprung: true,
            disarmed: false,
            trigger_on_entry: true,
            disarmable: false,
        });
        for unit in &mut game.units {
            unit.alive = [barbarian, dwarf, monster].contains(&unit.id);
            unit.escaped = false;
            if unit.id == barbarian {
                unit.pos = start;
                unit.in_pit = false;
            } else if unit.id == dwarf || unit.id == monster {
                unit.pos = pit;
                unit.in_pit = true;
            }
        }
        game.phase = GamePhase::HeroTurn { order_index: 0 };
        game.hero_turn = HeroTurnState::default();

        assert!(game.hero_may_share_square(barbarian, pit));
        assert!(!game.hero_may_share_square(monster, pit));
        game.apply_movement_roll(&[1, 1]).unwrap();
        assert!(game.active_move_destinations().contains(&pit));
        let body_before = game.unit(barbarian).unwrap().body;
        assert_eq!(game.move_active(Direction::East).unwrap(), pit);
        assert_eq!(game.unit(barbarian).unwrap().body, body_before - 1);
        assert!(game.unit(barbarian).unwrap().in_pit);
        assert_eq!(
            game.units
                .iter()
                .filter(|unit| unit.alive && !unit.escaped && unit.pos == pit)
                .count(),
            3
        );
        assert!(!game.square_is_open_for_figure(pit, Some(monster)));
    }

    #[test]
    fn voluntary_retreat_requires_every_survivor_on_stairs_and_awards_nothing() {
        let mut game = Game::demo(0x5245_5452_4541_54).unwrap();
        let stair = game.stairs[0];
        let heroes = game.hero_order.clone();
        for &hero in &heroes {
            let unit = game.units.iter_mut().find(|unit| unit.id == hero).unwrap();
            unit.pos = stair;
            unit.inventory.gold += 37;
        }
        let party_gold_before = heroes
            .iter()
            .map(|&hero| u32::from(game.unit(hero).unwrap().inventory.gold))
            .sum::<u32>();

        let missing = heroes[1];
        game.units
            .iter_mut()
            .find(|unit| unit.id == missing)
            .unwrap()
            .pos = Pos::new(0, 0);
        assert!(!game.can_voluntarily_retreat());
        assert_eq!(
            game.voluntarily_retreat().unwrap_err().to_string(),
            "every surviving Hero must return to the stairway before ending the quest"
        );

        game.units
            .iter_mut()
            .find(|unit| unit.id == missing)
            .unwrap()
            .pos = stair;
        assert!(game.can_voluntarily_retreat());
        game.voluntarily_retreat().unwrap();
        assert_eq!(game.phase, GamePhase::Retreated);
        assert_eq!(
            heroes
                .iter()
                .map(|&hero| u32::from(game.unit(hero).unwrap().inventory.gold))
                .sum::<u32>(),
            party_gold_before
        );
        assert!(
            game.log
                .back()
                .unwrap()
                .contains("no completion or final reward")
        );
    }

    #[test]
    fn opening_barbarian_has_selectable_routes_through_the_other_three_heroes() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_return_to_barak_tor().unwrap(),
            0x5354_4149_5253,
        )
        .unwrap();
        game.apply_movement_roll(&[1, 1]).unwrap();

        let targets = game.active_move_targets();
        assert!(targets.contains(&(Pos::new(2, 14), Direction::East)));
        assert!(targets.contains(&(Pos::new(1, 15), Direction::South)));

        game.move_active(Direction::East).unwrap();
        assert_eq!(game.move_active(Direction::East).unwrap(), Pos::new(3, 14));
    }

    #[test]
    fn full_roll_exposes_every_reachable_destination_and_astar_shortest_path() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_return_to_barak_tor().unwrap(),
            0x4153_5441_52,
        )
        .unwrap();
        game.apply_movement_roll(&[3, 4]).unwrap();

        let destination = Pos::new(4, 17);
        assert!(game.active_move_destinations().contains(&destination));
        let path = game
            .active_move_path_to(destination)
            .expect("highlighted destination has an A* path");
        assert_eq!(path.len(), 6);
        let mut cursor = game.active_hero().unwrap().pos;
        for direction in path.iter().copied() {
            cursor = cursor.offset(direction).unwrap();
        }
        assert_eq!(cursor, destination);
        for direction in path {
            game.move_active(direction).unwrap();
        }
        assert_eq!(game.active_hero().unwrap().pos, destination);
        assert_eq!(game.hero_turn.moved_steps, 6);
        assert_eq!(game.hero_turn.movement_left, 1);
    }

    #[test]
    fn astar_crosses_a_friendly_figure_but_never_offers_it_as_an_endpoint() {
        let mut game = Game::demo(0x4652_4945_4e44).unwrap();
        let barbarian = game.hero_order[0];
        let dwarf = game.hero_order[1];
        for cell in &mut game.cells {
            cell.passable = false;
            cell.revealed = true;
        }
        for x in 10..=13 {
            let cell = &mut game.cells[Game::cell_index(Pos::new(x, 10))];
            cell.passable = true;
            cell.region = 42;
        }
        for unit in &mut game.units {
            if unit.id != barbarian && unit.id != dwarf {
                unit.alive = false;
            }
        }
        game.units
            .iter_mut()
            .find(|unit| unit.id == barbarian)
            .unwrap()
            .pos = Pos::new(10, 10);
        game.units
            .iter_mut()
            .find(|unit| unit.id == dwarf)
            .unwrap()
            .pos = Pos::new(11, 10);
        game.apply_movement_roll(&[2, 2]).unwrap();

        let destinations = game.active_move_destinations();
        assert!(!destinations.contains(&Pos::new(11, 10)));
        assert!(destinations.contains(&Pos::new(13, 10)));
        assert_eq!(
            game.active_move_path_to(Pos::new(13, 10)).unwrap(),
            vec![Direction::East, Direction::East, Direction::East]
        );
    }

    #[test]
    fn astar_does_not_plan_past_a_known_trap_or_cheat_about_a_hidden_one() {
        let mut game = Game::demo(0x5452_4150).unwrap();
        let barbarian = game.hero_order[0];
        for cell in &mut game.cells {
            cell.passable = false;
            cell.revealed = true;
        }
        for x in 10..=13 {
            let cell = &mut game.cells[Game::cell_index(Pos::new(x, 10))];
            cell.passable = true;
            cell.region = 42;
        }
        for unit in &mut game.units {
            if unit.id != barbarian {
                unit.alive = false;
            }
        }
        game.units
            .iter_mut()
            .find(|unit| unit.id == barbarian)
            .unwrap()
            .pos = Pos::new(10, 10);
        game.traps.push(Trap {
            kind: TrapKind::Pit,
            pos: Pos::new(12, 10),
            discovered: true,
            sprung: false,
            disarmed: false,
            trigger_on_entry: true,
            disarmable: false,
        });
        game.apply_movement_roll(&[2, 2]).unwrap();
        assert!(game.active_move_destinations().contains(&Pos::new(12, 10)));
        assert!(!game.active_move_destinations().contains(&Pos::new(13, 10)));

        game.traps.last_mut().unwrap().discovered = false;
        assert!(game.active_move_destinations().contains(&Pos::new(13, 10)));
        let path = game.active_move_path_to(Pos::new(13, 10)).unwrap();
        for direction in path {
            if game.active_mover_id() != Some(barbarian) || game.hero_turn.movement_left == 0 {
                break;
            }
            game.move_active(direction).unwrap();
        }
        let barbarian = game.unit(barbarian).unwrap();
        assert_eq!(barbarian.pos, Pos::new(12, 10));
        assert!(barbarian.in_pit);
    }

    #[test]
    fn setup_deals_the_elf_three_scanned_spells_and_the_wizard_the_other_nine() {
        let startup = crate::startup::StartupFlow::default();
        let mut game = Game::demo(0x4445_414c).unwrap();
        game.apply_hero_setup(
            &startup.heroes,
            &startup.wizard_spells(),
            startup.elf_spells,
        );
        let elf = game
            .units
            .iter()
            .find(|unit| unit.figure == FigureKind::Hero(crate::model::HeroKind::Elf))
            .unwrap();
        let wizard = game
            .units
            .iter()
            .find(|unit| unit.figure == FigureKind::Hero(crate::model::HeroKind::Wizard))
            .unwrap();
        assert_eq!(elf.hero_spells, crate::startup::SpellGroup::Earth.spells());
        assert_eq!(wizard.hero_spells.len(), 9);
        assert!(
            crate::startup::SpellGroup::ALL
                .into_iter()
                .filter(|&group| group != crate::startup::SpellGroup::Earth)
                .flat_map(crate::startup::SpellGroup::spells)
                .all(|spell| wizard.hero_spells.contains(&spell))
        );
    }

    #[test]
    fn selected_clockwise_order_changes_turns_not_hero_identity_or_start_square() {
        let mut game = Game::from_quest(QuestDefinition::original_us_trial().unwrap(), 90).unwrap();
        let original_starts = game
            .units
            .iter()
            .filter_map(|unit| match unit.figure {
                FigureKind::Hero(hero) => Some((hero, unit.pos)),
                FigureKind::Monster(_) => None,
            })
            .collect::<HashMap<_, _>>();
        let mut startup = crate::startup::StartupFlow::default();
        startup.active_hero = 3;
        startup.move_active_hero_earlier();
        startup.move_active_hero_earlier();
        startup.move_active_hero_earlier();
        startup.heroes[0].hero_name = "Merlin".to_owned();

        game.apply_hero_setup(
            &startup.heroes,
            &[
                crate::startup::SpellGroup::Air,
                crate::startup::SpellGroup::Fire,
                crate::startup::SpellGroup::Water,
            ],
            crate::startup::SpellGroup::Earth,
        );

        assert_eq!(
            game.hero_order
                .iter()
                .map(|&id| game.unit(id).unwrap().figure)
                .collect::<Vec<_>>(),
            [
                FigureKind::Hero(crate::model::HeroKind::Wizard),
                FigureKind::Hero(crate::model::HeroKind::Barbarian),
                FigureKind::Hero(crate::model::HeroKind::Dwarf),
                FigureKind::Hero(crate::model::HeroKind::Elf),
            ]
        );
        assert_eq!(game.active_hero().unwrap().name, "Merlin");
        for unit in game
            .units
            .iter()
            .filter(|unit| matches!(unit.figure, FigureKind::Hero(_)))
        {
            let FigureKind::Hero(hero) = unit.figure else {
                unreachable!()
            };
            assert_eq!(unit.pos, original_starts[&hero]);
        }
        assert_eq!(game.active_hero().unwrap().hero_spells.len(), 9);
        let elf = game
            .units
            .iter()
            .find(|unit| unit.figure == FigureKind::Hero(crate::model::HeroKind::Elf))
            .unwrap();
        assert_eq!(elf.hero_spells, crate::startup::SpellGroup::Earth.spells());
    }

    #[test]
    fn swift_wind_courage_rock_skin_and_healing_apply_then_expire_exactly() {
        let (mut swift, wizard, _) = game_for_hero_spell(&[HeroSpell::SwiftWind]);
        assert_eq!(
            swift
                .cast_active_hero_spell(HeroSpell::SwiftWind, HeroSpellTarget::Unit(wizard))
                .unwrap(),
            HeroSpellCast::Resolved
        );
        assert_eq!(swift.active_movement_dice_count(), 4);
        swift.apply_movement_roll(&[1, 2, 3, 4]).unwrap();
        assert!(!swift.unit(wizard).unwrap().swift_wind);
        assert!(swift.unit(wizard).unwrap().hero_spells.is_empty());

        let (mut courage, wizard, target) = game_for_hero_spell(&[HeroSpell::Courage]);
        courage
            .cast_active_hero_spell(HeroSpell::Courage, HeroSpellTarget::Unit(wizard))
            .unwrap();
        courage.hero_turn = HeroTurnState::default();
        let plan = courage.active_attack_plan().unwrap();
        assert_eq!(plan.defender, target);
        assert_eq!(plan.attack_dice, 3);
        courage
            .resolve_attack(
                plan,
                &[CombatFace::Skull; 3],
                &vec![CombatFace::WhiteShield; plan.defend_dice as usize],
            )
            .unwrap();
        assert!(!courage.unit(wizard).unwrap().courage);

        let (mut rock, wizard, _) = game_for_hero_spell(&[HeroSpell::RockSkin]);
        let before = rock.unit(wizard).unwrap().effective_defense_dice();
        rock.cast_active_hero_spell(HeroSpell::RockSkin, HeroSpellTarget::Unit(wizard))
            .unwrap();
        assert_eq!(
            rock.unit(wizard).unwrap().effective_defense_dice(),
            before + 1
        );
        rock.damage_without_defense(wizard, 1);
        assert!(!rock.unit(wizard).unwrap().rock_skin);

        let (mut healing, wizard, _) = game_for_hero_spell(&[HeroSpell::HealBody]);
        healing
            .units
            .iter_mut()
            .find(|unit| unit.id == wizard)
            .unwrap()
            .body = 1;
        healing
            .cast_active_hero_spell(HeroSpell::HealBody, HeroSpellTarget::Unit(wizard))
            .unwrap();
        assert_eq!(healing.unit(wizard).unwrap().body, 4);
    }

    #[test]
    fn pass_through_rock_and_veil_of_mist_cross_forbidden_squares_but_end_safely() {
        let (mut rock, wizard, target) = game_for_hero_spell(&[HeroSpell::PassThroughRock]);
        rock.units
            .iter_mut()
            .find(|unit| unit.id == wizard)
            .unwrap()
            .pos = Pos::new(10, 10);
        rock.units
            .iter_mut()
            .find(|unit| unit.id == target)
            .unwrap()
            .pos = Pos::new(15, 2);
        rock.cells[Game::cell_index(Pos::new(11, 10))].passable = false;
        rock.cells[Game::cell_index(Pos::new(12, 10))].passable = true;
        rock.cast_active_hero_spell(HeroSpell::PassThroughRock, HeroSpellTarget::Unit(wizard))
            .unwrap();
        rock.apply_movement_roll(&[1, 1]).unwrap();
        assert_eq!(rock.move_active(Direction::East).unwrap(), Pos::new(11, 10));
        assert_eq!(rock.move_active(Direction::East).unwrap(), Pos::new(12, 10));
        assert!(rock.unit(wizard).unwrap().alive);
        assert!(!rock.unit(wizard).unwrap().pass_through_rock);

        let (mut mist, wizard, target) = game_for_hero_spell(&[HeroSpell::VeilOfMist]);
        mist.units
            .iter_mut()
            .find(|unit| unit.id == wizard)
            .unwrap()
            .pos = Pos::new(10, 10);
        mist.units
            .iter_mut()
            .find(|unit| unit.id == target)
            .unwrap()
            .pos = Pos::new(11, 10);
        mist.cells[Game::cell_index(Pos::new(10, 10))].passable = true;
        mist.cells[Game::cell_index(Pos::new(11, 10))].passable = true;
        mist.cells[Game::cell_index(Pos::new(12, 10))].passable = true;
        mist.cast_active_hero_spell(HeroSpell::VeilOfMist, HeroSpellTarget::Unit(wizard))
            .unwrap();
        mist.apply_movement_roll(&[1, 1]).unwrap();
        assert_eq!(mist.move_active(Direction::East).unwrap(), Pos::new(11, 10));
        assert_eq!(
            mist.end_hero_turn().unwrap_err(),
            RuleError::MustFinishOccupiedMove
        );
        assert_eq!(mist.move_active(Direction::East).unwrap(), Pos::new(12, 10));
        assert!(!mist.unit(wizard).unwrap().veil_of_mist);
    }

    #[test]
    fn fire_spells_use_their_exact_physical_red_save_dice() {
        let (mut ball, _, target) = game_for_hero_spell(&[HeroSpell::BallOfFlame]);
        ball.units
            .iter_mut()
            .find(|unit| unit.id == target)
            .unwrap()
            .body = 3;
        let HeroSpellCast::Roll(roll) = ball
            .cast_active_hero_spell(HeroSpell::BallOfFlame, HeroSpellTarget::Unit(target))
            .unwrap()
        else {
            panic!("Ball of Flame must request physical save dice");
        };
        assert_eq!(roll.dice_count, 2);
        ball.resolve_hero_spell_red_roll(roll, &[5, 1]).unwrap();
        assert_eq!(ball.unit(target).unwrap().body, 2);

        let (mut wrath, _, target) = game_for_hero_spell(&[HeroSpell::FireOfWrath]);
        let before = wrath.unit(target).unwrap().body;
        let HeroSpellCast::Roll(roll) = wrath
            .cast_active_hero_spell(HeroSpell::FireOfWrath, HeroSpellTarget::Unit(target))
            .unwrap()
        else {
            panic!("Fire of Wrath must request a physical save die");
        };
        assert_eq!(roll.dice_count, 1);
        wrath.resolve_hero_spell_red_roll(roll, &[6]).unwrap();
        assert_eq!(wrath.unit(target).unwrap().body, before);
    }

    #[test]
    fn tempest_and_sleep_remove_monster_turns_until_their_card_rules_release_them() {
        let (mut tempest, _, target) = game_for_hero_spell(&[HeroSpell::Tempest]);
        let start = tempest.unit(target).unwrap().pos;
        tempest
            .cast_active_hero_spell(HeroSpell::Tempest, HeroSpellTarget::Unit(target))
            .unwrap();
        tempest.phase = GamePhase::ZargonTurn;
        tempest.zargon_turn_started = false;
        assert_eq!(tempest.advance_zargon_turn().unwrap(), ZargonStep::Finished);
        assert_eq!(tempest.unit(target).unwrap().pos, start);
        assert_eq!(tempest.unit(target).unwrap().skip_turns, 0);

        let (mut sleep, _, target) = game_for_hero_spell(&[HeroSpell::Sleep]);
        let HeroSpellCast::Roll(initial) = sleep
            .cast_active_hero_spell(HeroSpell::Sleep, HeroSpellTarget::Unit(target))
            .unwrap()
        else {
            panic!("Sleep must request immediate Mind dice");
        };
        sleep
            .resolve_hero_spell_red_roll(initial, &vec![1; initial.dice_count as usize])
            .unwrap();
        assert!(sleep.unit(target).unwrap().sleeping);
        sleep.phase = GamePhase::ZargonTurn;
        sleep.zargon_turn_started = false;
        let ZargonStep::HeroSpellRoll(awakening) = sleep.advance_zargon_turn().unwrap() else {
            panic!("a sleeping Monster must roll Mind dice on its future turn");
        };
        let mut awakening_dice = vec![1; awakening.dice_count as usize];
        awakening_dice[0] = 6;
        sleep
            .resolve_hero_spell_red_roll(awakening, &awakening_dice)
            .unwrap();
        assert!(!sleep.unit(target).unwrap().sleeping);
        assert!(matches!(
            sleep.advance_zargon_turn().unwrap(),
            ZargonStep::Attack(_)
        ));
    }

    #[test]
    fn genie_opens_a_chosen_door_or_attacks_any_placed_monster_with_five_dice() {
        let (mut door_game, _, _) = game_for_hero_spell(&[HeroSpell::Genie]);
        door_game.doors[0].open = false;
        let door_target = HeroSpellTarget::Door(0);
        assert!(
            door_game
                .valid_hero_spell_targets(HeroSpell::Genie)
                .contains(&door_target)
        );
        door_game
            .cast_active_hero_spell(HeroSpell::Genie, door_target)
            .unwrap();
        assert!(door_game.doors[0].open);

        let (mut attack_game, _, target) = game_for_hero_spell(&[HeroSpell::Genie]);
        attack_game
            .units
            .iter_mut()
            .find(|unit| unit.id == target)
            .unwrap()
            .body = 8;
        let HeroSpellCast::Roll(roll) = attack_game
            .cast_active_hero_spell(HeroSpell::Genie, HeroSpellTarget::Unit(target))
            .unwrap()
        else {
            panic!("the Genie's attack must roll physical combat dice");
        };
        let HeroSpellDiceKind::Combat {
            attack_dice,
            defend_dice,
        } = roll.kind
        else {
            panic!("the Genie must use combat dice");
        };
        assert_eq!(attack_dice, 5);
        let outcome = attack_game
            .resolve_hero_spell_combat_roll(
                roll,
                &[CombatFace::Skull; 5],
                &vec![CombatFace::WhiteShield; defend_dice as usize],
            )
            .unwrap();
        assert_eq!(outcome.damage, 5);
    }

    #[test]
    fn a_closed_door_is_free_to_open_but_blocks_movement() {
        let mut game = Game::demo(7).unwrap();
        game.units
            .iter_mut()
            .find(|unit| unit.pos == Pos::new(16, 2))
            .unwrap()
            .alive = false;
        game.apply_movement_roll(&[6, 6]).unwrap();
        game.move_active(Direction::East).unwrap();
        assert_eq!(
            game.move_active(Direction::East).unwrap_err(),
            RuleError::ClosedDoor
        );
        game.open_adjacent_door().unwrap();
        assert!(
            game.doors
                .iter()
                .any(|door| door.open && door.connects(Pos::new(15, 2), Pos::new(16, 2)))
        );
        assert!(game.active_move_destinations().contains(&Pos::new(16, 2)));
        assert_eq!(
            game.active_move_path_to(Pos::new(16, 2)).unwrap(),
            vec![Direction::East]
        );
        assert_eq!(game.move_active(Direction::East).unwrap(), Pos::new(16, 2));
    }

    #[test]
    fn player_selects_the_exact_adjacent_door_to_open() {
        let mut quest = QuestDefinition::demo().unwrap();
        quest.doors.push(DoorDef {
            a: Pos::new(15, 2),
            b: Pos::new(15, 3),
            open: false,
            searchable: false,
            false_door: false,
        });
        let mut game = Game::from_quest(quest, 7).unwrap();
        game.apply_movement_roll(&[6, 6]).unwrap();
        game.move_active(Direction::East).unwrap();

        let choices = game.adjacent_closed_door_indices().unwrap();
        assert_eq!(choices.len(), 2);
        let south = choices
            .into_iter()
            .find(|&index| game.doors[index].connects(Pos::new(15, 2), Pos::new(15, 3)))
            .unwrap();
        game.open_selected_adjacent_door(south).unwrap();

        assert!(game.doors[south].open);
        assert!(
            game.doors
                .iter()
                .any(|door| !door.open && door.connects(Pos::new(15, 2), Pos::new(16, 2)))
        );
    }

    #[test]
    fn hero_and_monster_block_different_combat_faces() {
        let mut game = Game::demo(7).unwrap();
        game.apply_movement_roll(&[6, 6]).unwrap();
        game.move_active(Direction::East).unwrap();
        game.open_adjacent_door().unwrap();
        let plan = game.active_attack_plan().unwrap();
        let outcome = game
            .resolve_attack(
                plan,
                &[
                    CombatFace::Skull,
                    CombatFace::Skull,
                    CombatFace::WhiteShield,
                ],
                &[CombatFace::WhiteShield],
            )
            .unwrap();
        assert_eq!(
            outcome.damage, 2,
            "a monster only blocks with black shields"
        );
        assert!(outcome.defender_died);
    }

    #[test]
    fn attack_options_enforce_each_original_weapon_shape_and_range() {
        let mut game = Game::demo(0x5745_4150_4f4e).unwrap();
        let hero = game.active_hero_id().unwrap();
        let target = game
            .units
            .iter()
            .find(|unit| unit.name == "Crypt Warden")
            .unwrap()
            .id;
        game.cells.iter_mut().for_each(|cell| cell.revealed = true);
        game.doors.iter_mut().for_each(|door| door.open = true);
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster && unit.id != target)
            .for_each(|unit| unit.alive = false);
        let hero_unit = game.units.iter_mut().find(|unit| unit.id == hero).unwrap();
        hero_unit.pos = Pos::new(10, 10);
        hero_unit.inventory.weapons = vec![
            Weapon::Dagger,
            Weapon::Crossbow,
            Weapon::Broadsword,
            Weapon::Longsword,
        ];
        let target_unit = game
            .units
            .iter_mut()
            .find(|unit| unit.id == target)
            .unwrap();
        target_unit.pos = Pos::new(12, 10);
        target_unit.dormant = false;
        target_unit.hidden_until_activated = false;
        game.allocate_pending_visible_figures();

        let ranged = game.active_attack_options().unwrap();
        assert!(
            ranged
                .iter()
                .any(|plan| plan.source == AttackSource::Weapon(Weapon::Crossbow))
        );
        assert!(
            ranged
                .iter()
                .any(|plan| plan.source == AttackSource::ThrownDagger)
        );
        assert!(
            ranged
                .iter()
                .all(|plan| plan.source != AttackSource::Weapon(Weapon::Broadsword))
        );

        game.units
            .iter_mut()
            .find(|unit| unit.id == target)
            .unwrap()
            .pos = Pos::new(11, 11);
        let diagonal = game.active_attack_options().unwrap();
        assert!(
            diagonal
                .iter()
                .any(|plan| plan.source == AttackSource::Weapon(Weapon::Longsword))
        );
        assert!(
            diagonal
                .iter()
                .all(|plan| plan.source != AttackSource::Weapon(Weapon::Broadsword))
        );

        game.units
            .iter_mut()
            .find(|unit| unit.id == target)
            .unwrap()
            .pos = Pos::new(11, 10);
        let adjacent = game.active_attack_options().unwrap();
        assert!(
            adjacent
                .iter()
                .any(|plan| plan.source == AttackSource::Weapon(Weapon::Broadsword))
        );
        assert!(
            adjacent
                .iter()
                .all(|plan| plan.source != AttackSource::Weapon(Weapon::Crossbow))
        );
    }

    #[test]
    fn a_thrown_dagger_is_selected_explicitly_and_removed_after_the_roll() {
        let mut game = Game::demo(0x4441_4747_4552).unwrap();
        let hero = game.active_hero_id().unwrap();
        let target = game
            .units
            .iter()
            .find(|unit| unit.name == "Crypt Warden")
            .unwrap()
            .id;
        game.cells.iter_mut().for_each(|cell| cell.revealed = true);
        game.doors.iter_mut().for_each(|door| door.open = true);
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster && unit.id != target)
            .for_each(|unit| unit.alive = false);
        let hero_unit = game.units.iter_mut().find(|unit| unit.id == hero).unwrap();
        hero_unit.pos = Pos::new(10, 10);
        hero_unit.inventory.weapons = vec![Weapon::Dagger, Weapon::Broadsword];
        hero_unit.inventory.equipped_weapon = Some(Weapon::Dagger);
        let target_unit = game
            .units
            .iter_mut()
            .find(|unit| unit.id == target)
            .unwrap();
        target_unit.pos = Pos::new(12, 10);
        target_unit.body = 5;
        target_unit.dormant = false;
        target_unit.hidden_until_activated = false;
        game.allocate_pending_visible_figures();

        let plan = game
            .active_attack_options()
            .unwrap()
            .into_iter()
            .find(|plan| plan.source == AttackSource::ThrownDagger)
            .unwrap();
        game.resolve_attack(
            plan,
            &[CombatFace::Skull],
            &vec![CombatFace::WhiteShield; plan.defend_dice as usize],
        )
        .unwrap();
        assert!(
            !game
                .unit(hero)
                .unwrap()
                .inventory
                .weapons
                .contains(&Weapon::Dagger)
        );
        assert_eq!(
            game.unit(hero).unwrap().inventory.equipped_weapon,
            Some(Weapon::Broadsword)
        );
    }

    #[test]
    fn four_hero_turns_are_followed_by_the_computer_game_master() {
        let mut game = Game::demo(7).unwrap();
        for _ in 0..3 {
            game.end_hero_turn().unwrap();
            assert!(matches!(game.phase, GamePhase::HeroTurn { .. }));
        }
        game.end_hero_turn().unwrap();
        assert_eq!(game.phase, GamePhase::ZargonTurn);
        game.run_zargon_turn().unwrap();
        assert!(matches!(game.phase, GamePhase::HeroTurn { order_index: 0 }));
    }

    #[test]
    fn zargon_turn_yields_an_attack_before_resolving_it() {
        let mut game = Game::demo(7).unwrap();
        let monster_id = game
            .units
            .iter()
            .find(|unit| matches!(unit.figure, FigureKind::Monster(_)))
            .unwrap()
            .id;
        game.units
            .iter_mut()
            .find(|unit| unit.id == monster_id)
            .unwrap()
            .pos = Pos::new(15, 2);
        game.cells[Game::cell_index(Pos::new(15, 2))].revealed = true;
        for _ in 0..4 {
            game.end_hero_turn().unwrap();
        }

        let ZargonStep::Attack(plan) = game.advance_zargon_turn().unwrap() else {
            panic!("an adjacent visible monster should yield a renderable attack");
        };
        assert_eq!(plan.attacker, monster_id);
        assert_eq!(plan.defender, game.hero_order[0]);
        assert!(game.last_combat_visual.is_none());

        game.resolve_attack(
            plan,
            &vec![CombatFace::WhiteShield; plan.attack_dice as usize],
            &vec![CombatFace::WhiteShield; plan.defend_dice as usize],
        )
        .unwrap();
        let event = game.last_combat_visual.unwrap();
        assert_eq!(event.attacker, monster_id);
        assert_eq!(event.defender, game.hero_order[0]);
    }

    #[test]
    fn zargon_tactics_choose_shortest_routes_then_the_most_vulnerable_hero() {
        let mut game = Game::demo(0x5a41_5247_4f4e_4149).unwrap();
        let monster = game
            .units
            .iter()
            .find(|unit| unit.faction == Faction::Monster)
            .unwrap()
            .id;
        let barbarian = game.hero_order[0];
        let dwarf = game.hero_order[1];

        for cell in &mut game.cells {
            cell.passable = true;
            cell.revealed = true;
            cell.region = 90;
        }
        game.doors.clear();
        game.props.clear();
        game.traps.clear();
        for unit in &mut game.units {
            unit.alive = [monster, barbarian, dwarf].contains(&unit.id);
            unit.escaped = false;
            unit.dormant = false;
        }
        game.units
            .iter_mut()
            .find(|unit| unit.id == monster)
            .unwrap()
            .pos = Pos::new(10, 10);
        {
            let hero = game
                .units
                .iter_mut()
                .find(|unit| unit.id == barbarian)
                .unwrap();
            hero.pos = Pos::new(11, 10);
            hero.body = hero.stats.body as i16;
        }
        {
            let hero = game.units.iter_mut().find(|unit| unit.id == dwarf).unwrap();
            hero.pos = Pos::new(10, 11);
            hero.body = 1;
        }
        game.phase = GamePhase::ZargonTurn;
        game.zargon_turn_started = false;

        let ZargonStep::Attack(plan) = game.advance_zargon_turn().unwrap() else {
            panic!("an adjacent monster should attack before moving");
        };
        assert_eq!(plan.defender, dwarf);

        game.units
            .iter_mut()
            .find(|unit| unit.id == barbarian)
            .unwrap()
            .pos = Pos::new(12, 10);
        game.units
            .iter_mut()
            .find(|unit| unit.id == dwarf)
            .unwrap()
            .pos = Pos::new(10, 12);
        assert_eq!(
            game.path_to_nearest_hero(monster).unwrap(),
            vec![Pos::new(10, 11)],
            "equal-length routes should pursue the lower-Body Hero"
        );

        game.units
            .iter_mut()
            .find(|unit| unit.id == dwarf)
            .unwrap()
            .pos = Pos::new(10, 13);
        assert_eq!(
            game.path_to_nearest_hero(monster).unwrap(),
            vec![Pos::new(11, 10)],
            "a shorter legal attack route takes priority over vulnerability"
        );
    }

    #[test]
    fn treasure_search_adds_valuable_card_and_is_once_per_hero_per_room() {
        let mut game = Game::demo(7).unwrap();
        let hero_id = game.active_hero_id().unwrap();
        game.set_treasure_replay_order([TreasureCard::Gem35]);
        let outcome = game.search_treasure().unwrap();
        assert_eq!(
            outcome.discovery,
            TreasureDiscovery::Card(TreasureCard::Gem35)
        );
        assert_eq!(game.unit(hero_id).unwrap().inventory.gold, 35);

        game.phase = GamePhase::HeroTurn { order_index: 0 };
        game.hero_turn = HeroTurnState::default();
        assert_eq!(
            game.search_treasure().unwrap_err(),
            RuleError::AlreadySearchedRoom
        );
    }

    #[test]
    fn quest_specific_treasure_is_resolved_once_before_the_deck() {
        let mut quest = QuestDefinition::demo().unwrap();
        quest.events.push(QuestEventDef {
            id: "test-gold".to_owned(),
            marker: None,
            trigger: QuestTriggerDef::SearchTreasure {
                room: "Stair Chamber".to_owned(),
            },
            effect: QuestEffectDef::Gold { amount: 84 },
            message: Some("A quest-specific cache contains 84 gold coins.".to_owned()),
        });
        let mut game = Game::from_quest(quest, 7).unwrap();
        let hero_id = game.active_hero_id().unwrap();
        game.set_treasure_replay_order([TreasureCard::Gem35]);

        let outcome = game.search_treasure().unwrap();
        assert_eq!(outcome.discovery, TreasureDiscovery::Gold(84));
        assert_eq!(game.unit(hero_id).unwrap().inventory.gold, 84);

        game.hero_turn.action_used = false;
        game.hero_turn.movement_roll = None;
        game.hero_turn.movement_left = 0;
        game.end_hero_turn().unwrap();
        let second_hero = game.active_hero_id().unwrap();
        let outcome = game.search_treasure().unwrap();
        assert_eq!(
            outcome.discovery,
            TreasureDiscovery::Card(TreasureCard::Gem35)
        );
        assert_eq!(game.unit(second_hero).unwrap().inventory.gold, 35);
    }

    #[test]
    fn required_lost_artifact_is_special_treasure_before_an_ordinary_deck_draw() {
        let mut game = Game::demo(0x4c4f_5354_4152_54).unwrap();
        let hero = game.active_hero_id().unwrap();
        game.lost_artifact_treasure.push_back(Artifact::SpiritBlade);
        game.set_treasure_replay_order([TreasureCard::Gem35]);
        let deck_before = game.treasure_deck.remaining();

        let outcome = game.search_treasure().unwrap();

        assert_eq!(
            outcome.discovery,
            TreasureDiscovery::Artifact(Artifact::SpiritBlade)
        );
        assert!(
            game.unit(hero)
                .unwrap()
                .inventory
                .artifacts
                .contains(&Artifact::SpiritBlade)
        );
        assert_eq!(game.treasure_deck.remaining(), deck_before);
        assert!(game.lost_artifact_treasure.is_empty());
        assert!(game.log.back().unwrap().contains("special treasure"));
    }

    #[test]
    fn reveal_room_event_is_logged_only_once() {
        let mut quest = QuestDefinition::demo().unwrap();
        quest.events.push(QuestEventDef {
            id: "test-reveal".to_owned(),
            marker: None,
            trigger: QuestTriggerDef::RevealRoom {
                room: "Stair Chamber".to_owned(),
            },
            effect: QuestEffectDef::Message,
            message: Some("A special room rule is now active.".to_owned()),
        });
        let game = Game::from_quest(quest, 7).unwrap();
        assert_eq!(
            game.log
                .iter()
                .filter(|line| line.as_str() == "A special room rule is now active.")
                .count(),
            1
        );
    }

    #[test]
    fn the_trial_resolves_all_five_printed_notes() {
        let mut game = Game::from_quest(QuestDefinition::original_us_trial().unwrap(), 7).unwrap();
        let hero_id = game.active_hero_id().unwrap();

        let search_at = |game: &mut Game, pos: Pos| {
            let region = game.cell(pos).unwrap().region;
            game.units
                .iter_mut()
                .filter(|unit| {
                    matches!(unit.figure, FigureKind::Monster(_))
                        && game.cells[Game::cell_index(unit.pos)].region == region
                })
                .for_each(|unit| unit.alive = false);
            game.units
                .iter_mut()
                .find(|unit| unit.id == hero_id)
                .unwrap()
                .pos = pos;
            game.hero_turn.action_used = false;
            game.search_treasure().unwrap().discovery
        };

        assert_eq!(
            search_at(&mut game, Pos::new(10, 15)),
            TreasureDiscovery::Empty
        );
        assert_eq!(
            search_at(&mut game, Pos::new(17, 15)),
            TreasureDiscovery::Empty
        );
        assert_eq!(
            search_at(&mut game, Pos::new(10, 5)),
            TreasureDiscovery::Gold(84)
        );
        assert_eq!(
            search_at(&mut game, Pos::new(11, 7)),
            TreasureDiscovery::Gold(120)
        );
        assert_eq!(game.unit(hero_id).unwrap().inventory.gold, 204);

        let guardian = game
            .units
            .iter()
            .find(|unit| unit.name == "Guardian of Fellmarg's Tomb")
            .unwrap();
        assert_eq!(guardian.stats.attack, 4);
        assert_eq!(game.wandering_monster, MonsterKind::Orc);
    }

    #[test]
    fn sir_ragnar_alarm_reveals_the_board_opens_doors_and_forbids_cell_treasure() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_rescue_of_sir_ragnar().unwrap(),
            7,
        )
        .unwrap();
        let controller = game.active_hero_id().unwrap();
        let ragnar = game
            .units
            .iter()
            .find(|unit| unit.name == "Sir Ragnar")
            .unwrap();
        assert_eq!(ragnar.faction, Faction::Hero);
        assert_eq!(ragnar.stats.attack, 0);
        assert_eq!(ragnar.stats.defend, 2);
        assert_eq!(ragnar.body, 2);
        let ragnar_id = ragnar.id;

        game.reveal_from(Pos::new(5, 11));
        assert_eq!(game.escorted_ally, Some((ragnar_id, controller)));
        assert!(game.doors.iter().all(|door| door.open && door.discovered));
        assert!(
            game.cells
                .iter()
                .filter(|cell| cell.passable)
                .all(|cell| cell.revealed)
        );

        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
            .for_each(|unit| unit.alive = false);
        game.units
            .iter_mut()
            .find(|unit| unit.id == controller)
            .unwrap()
            .pos = Pos::new(5, 11);
        assert_eq!(
            game.search_treasure().unwrap_err(),
            RuleError::TreasureForbidden
        );
    }

    #[test]
    fn sir_ragnar_quest_chests_and_rescue_reward_follow_the_printed_notes() {
        let quest = QuestDefinition::original_us_rescue_of_sir_ragnar().unwrap();
        let mut game = Game::from_quest(quest.clone(), 7).unwrap();
        let hero_id = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
            .for_each(|unit| unit.alive = false);
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .pos = Pos::new(2, 13);
        let body_before = game.unit(hero_id).unwrap().body;
        assert_eq!(
            game.search_treasure().unwrap().discovery,
            TreasureDiscovery::QuestEvent
        );
        assert_eq!(game.unit(hero_id).unwrap().body, body_before - 1);

        game.phase = GamePhase::HeroTurn { order_index: 0 };
        game.hero_turn = HeroTurnState::default();
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .pos = Pos::new(19, 8);
        assert_eq!(
            game.search_treasure().unwrap().discovery,
            TreasureDiscovery::QuestEvent
        );
        assert_eq!(game.unit(hero_id).unwrap().inventory.gold, 60);
        assert_eq!(game.unit(hero_id).unwrap().inventory.potion_of_healing, 1);

        let mut disarmed_game = Game::from_quest(quest, 9).unwrap();
        let disarmed_hero = disarmed_game.active_hero_id().unwrap();
        disarmed_game
            .units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
            .for_each(|unit| unit.alive = false);
        disarmed_game.traps[0].disarmed = true;
        disarmed_game
            .units
            .iter_mut()
            .find(|unit| unit.id == disarmed_hero)
            .unwrap()
            .pos = Pos::new(2, 13);
        let body_before = disarmed_game.unit(disarmed_hero).unwrap().body;
        disarmed_game.search_treasure().unwrap();
        assert_eq!(disarmed_game.unit(disarmed_hero).unwrap().body, body_before);

        let ragnar_id = disarmed_game
            .units
            .iter()
            .find(|unit| unit.name == "Sir Ragnar")
            .unwrap()
            .id;
        disarmed_game
            .units
            .iter_mut()
            .find(|unit| unit.id == ragnar_id)
            .unwrap()
            .pos = disarmed_game.stairs[0];
        disarmed_game.check_terminal();
        assert_eq!(disarmed_game.phase, GamePhase::Won);
        assert!(
            disarmed_game
                .hero_order
                .iter()
                .all(|&id| { disarmed_game.unit(id).unwrap().inventory.gold == 60 })
        );
    }

    #[test]
    fn sir_ragnar_moves_on_one_die_after_his_controllers_turn_and_cannot_attack() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_rescue_of_sir_ragnar().unwrap(),
            7,
        )
        .unwrap();
        game.reveal_from(Pos::new(5, 11));
        let (ragnar_id, controller) = game.escorted_ally.unwrap();
        assert_eq!(controller, game.active_hero_id().unwrap());

        game.end_hero_turn().unwrap();
        assert!(matches!(
            game.phase,
            GamePhase::AllyTurn { ally, .. } if ally == ragnar_id
        ));
        assert_eq!(game.active_movement_dice_count(), 1);
        assert_eq!(game.apply_movement_roll(&[5]).unwrap(), 5);
        assert_eq!(
            game.active_attack_plan().unwrap_err(),
            RuleError::NotHeroTurn
        );
        assert_eq!(game.move_active(Direction::East).unwrap(), Pos::new(6, 11));

        game.end_hero_turn().unwrap();
        assert!(matches!(game.phase, GamePhase::HeroTurn { order_index: 1 }));
    }

    #[test]
    fn ulag_quest_treasure_healing_and_bounty_follow_the_printed_notes() {
        let quest = QuestDefinition::original_us_lair_of_the_orc_warlord().unwrap();
        let mut treasure_game = Game::from_quest(quest.clone(), 7).unwrap();
        let hero_id = treasure_game.active_hero_id().unwrap();
        treasure_game
            .units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
            .for_each(|unit| unit.alive = false);

        treasure_game
            .units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .pos = Pos::new(3, 3);
        assert_eq!(
            treasure_game.search_treasure().unwrap().discovery,
            TreasureDiscovery::QuestEvent
        );
        assert!(
            treasure_game
                .unit(hero_id)
                .unwrap()
                .inventory
                .weapons
                .contains(&Weapon::Staff)
        );

        treasure_game.hero_turn.action_used = false;
        treasure_game
            .units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .pos = Pos::new(2, 10);
        assert_eq!(
            treasure_game.search_treasure().unwrap().discovery,
            TreasureDiscovery::QuestEvent
        );
        assert_eq!(treasure_game.unit(hero_id).unwrap().inventory.gold, 24);
        assert_eq!(
            treasure_game
                .unit(hero_id)
                .unwrap()
                .inventory
                .potion_of_healing,
            1
        );
        treasure_game
            .units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .body = 2;
        assert_eq!(treasure_game.use_healing_potion().unwrap(), 4);
        assert_eq!(treasure_game.unit(hero_id).unwrap().body, 6);
        assert_eq!(
            treasure_game
                .unit(hero_id)
                .unwrap()
                .inventory
                .potion_of_healing,
            0
        );

        let mut combat_game = Game::from_quest(quest, 11).unwrap();
        let attacker_id = combat_game.active_hero_id().unwrap();
        let ulag_id = combat_game
            .units
            .iter()
            .find(|unit| unit.name == "Ulag")
            .unwrap()
            .id;
        combat_game
            .units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster && unit.id != ulag_id)
            .for_each(|unit| unit.alive = false);
        combat_game
            .units
            .iter_mut()
            .find(|unit| unit.id == attacker_id)
            .unwrap()
            .pos = Pos::new(4, 14);
        combat_game
            .cells
            .iter_mut()
            .filter(|cell| cell.passable)
            .for_each(|cell| cell.revealed = true);
        combat_game.allocate_pending_visible_figures();

        let plan = combat_game.active_attack_plan().unwrap();
        assert_eq!(plan.defender, ulag_id);
        assert_eq!(plan.attack_dice, 3);
        assert_eq!(plan.defend_dice, 5);
        let outcome = combat_game
            .resolve_attack(plan, &[CombatFace::Skull; 3], &[CombatFace::WhiteShield; 5])
            .unwrap();
        assert!(outcome.defender_died);
        assert!(
            combat_game
                .hero_order
                .iter()
                .all(|&id| combat_game.unit(id).unwrap().inventory.gold == 45)
        );
        assert!(!matches!(combat_game.phase, GamePhase::Won));

        for (index, hero_id) in combat_game.hero_order.clone().into_iter().enumerate() {
            combat_game
                .units
                .iter_mut()
                .find(|unit| unit.id == hero_id)
                .unwrap()
                .pos = combat_game.stairs[index];
        }
        combat_game.check_terminal();
        assert_eq!(combat_game.phase, GamePhase::Won);
    }

    #[test]
    fn royal_chests_can_be_taken_dropped_and_passed_but_never_become_hero_gold() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_prince_magnus_gold().unwrap(),
            7,
        )
        .unwrap();
        let barbarian = game.hero_order[0];
        let dwarf = game.hero_order[1];
        game.units
            .iter_mut()
            .find(|unit| unit.id == barbarian)
            .unwrap()
            .pos = Pos::new(10, 7);
        game.units
            .iter_mut()
            .find(|unit| unit.id == dwarf)
            .unwrap()
            .pos = Pos::new(11, 7);

        assert_eq!(game.take_quest_item().unwrap(), "Royal Chest 1");
        assert_eq!(game.active_movement_dice_count(), 1);
        assert_eq!(game.unit(barbarian).unwrap().inventory.gold, 0);
        assert_eq!(game.quest_items[0].sealed_gold, 250);
        assert_eq!(game.transfer_quest_item_to_adjacent_hero().unwrap(), dwarf);
        assert!(game.unit(barbarian).unwrap().carried_quest_item.is_none());
        assert_eq!(game.unit(dwarf).unwrap().carried_quest_item, Some(0));

        game.phase = GamePhase::HeroTurn { order_index: 1 };
        assert_eq!(game.active_movement_dice_count(), 1);
        assert_eq!(game.drop_quest_item().unwrap(), "Royal Chest 1");
        assert!(game.unit(dwarf).unwrap().carried_quest_item.is_none());
        assert_eq!(
            game.props[game.quest_items[0].prop_index].pos,
            Pos::new(11, 7)
        );
        assert!(game.props[game.quest_items[0].prop_index].visible);
    }

    #[test]
    fn all_three_royal_chests_must_reach_the_stairs_for_the_240_gold_reward() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_prince_magnus_gold().unwrap(),
            9,
        )
        .unwrap();
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
            .for_each(|unit| unit.alive = false);
        let hero_id = game.active_hero_id().unwrap();
        let chest_positions = [Pos::new(10, 8), Pos::new(10, 9), Pos::new(10, 10)];

        for (index, chest_pos) in chest_positions.into_iter().enumerate() {
            game.hero_turn = HeroTurnState::default();
            game.units
                .iter_mut()
                .find(|unit| unit.id == hero_id)
                .unwrap()
                .pos = chest_pos;
            game.take_quest_item().unwrap();
            assert_eq!(game.active_movement_dice_count(), 1);
            game.units
                .iter_mut()
                .find(|unit| unit.id == hero_id)
                .unwrap()
                .pos = Pos::new(18, 14);
            game.apply_movement_roll(&[1]).unwrap();
            assert_eq!(game.move_active(Direction::East).unwrap(), Pos::new(19, 14));
            assert!(game.quest_items[index].delivered);
            if index < 2 {
                assert!(!matches!(game.phase, GamePhase::Won));
            }
        }

        assert_eq!(game.phase, GamePhase::Won);
        assert!(game.quest_items.iter().all(|item| item.delivered));
        assert!(
            game.hero_order
                .iter()
                .all(|&id| game.unit(id).unwrap().inventory.gold == 60)
        );
        assert!(
            game.quest_items
                .iter()
                .all(|item| !game.props[item.prop_index].visible)
        );
    }

    #[test]
    fn variable_healing_and_talisman_rewards_apply_their_printed_values() {
        let mut quest = QuestDefinition::demo().unwrap();
        quest.events.push(QuestEventDef {
            id: "MELAR_REWARDS".to_owned(),
            marker: None,
            trigger: QuestTriggerDef::SearchTreasure {
                room: "Stair Chamber".to_owned(),
            },
            effect: QuestEffectDef::Bundle {
                effects: vec![
                    QuestEffectDef::HealingPotion {
                        count: 1,
                        restore: 2,
                    },
                    QuestEffectDef::Artifact {
                        artifact: Artifact::TalismanOfLore,
                    },
                ],
            },
            message: None,
        });
        quest.objective = ObjectiveDef::FindArtifactAndReturn {
            artifact: Artifact::TalismanOfLore,
        };
        let mut game = Game::from_quest(quest, 7).unwrap();
        let hero_id = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
            .for_each(|unit| unit.alive = false);
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .body = 2;
        let second_hero = game.hero_order[1];
        game.units
            .iter_mut()
            .find(|unit| unit.id == second_hero)
            .unwrap()
            .pos = Pos::new(10, 7);
        let base_mind = game.unit(hero_id).unwrap().stats.mind;

        game.search_treasure().unwrap();
        assert_eq!(game.unit(hero_id).unwrap().effective_mind(), base_mind + 1);
        assert_eq!(game.use_healing_potion().unwrap(), 2);
        assert_eq!(game.unit(hero_id).unwrap().body, 4);
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .equipment_available = false;
        assert_eq!(game.unit(hero_id).unwrap().effective_mind(), base_mind);
    }

    #[test]
    fn event_only_secret_door_ignores_secret_search_until_treasure_reveals_it() {
        let mut quest = QuestDefinition::demo().unwrap();
        let hidden = DoorDef {
            a: Pos::new(14, 2),
            b: Pos::new(14, 3),
            open: false,
            searchable: false,
            false_door: false,
        };
        quest.secret_doors.push(hidden.clone());
        quest.events.push(QuestEventDef {
            id: "MELARS_KEY".to_owned(),
            marker: None,
            trigger: QuestTriggerDef::SearchTreasure {
                room: "Stair Chamber".to_owned(),
            },
            effect: QuestEffectDef::RevealSecretDoor {
                a: hidden.a,
                b: hidden.b,
            },
            message: None,
        });
        let mut game = Game::from_quest(quest, 7).unwrap();
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
            .for_each(|unit| unit.alive = false);

        assert!(game.search_secret_doors().unwrap().is_empty());
        assert!(!game.has_door(hidden.a, hidden.b).unwrap().discovered);
        game.hero_turn.action_used = false;
        game.search_treasure().unwrap();
        assert!(game.has_door(hidden.a, hidden.b).unwrap().discovered);
    }

    #[test]
    fn dormant_gargoyle_activates_on_door_open_and_is_immune_until_it_acts() {
        let mut quest = QuestDefinition::demo().unwrap();
        let gargoyle = &mut quest.monsters[0];
        gargoyle.monster = MonsterKind::Gargoyle;
        gargoyle.name = Some("Stone Gargoyle".to_owned());
        gargoyle.dormant = true;
        gargoyle.invulnerable_until_acts = true;
        let trigger_door = quest.doors[0].clone();
        quest.events.push(QuestEventDef {
            id: "AWAKEN_GARGOYLE".to_owned(),
            marker: None,
            trigger: QuestTriggerDef::OpenDoor {
                a: trigger_door.a,
                b: trigger_door.b,
            },
            effect: QuestEffectDef::ActivateNamed {
                name: "Stone Gargoyle".to_owned(),
            },
            message: None,
        });
        let mut game = Game::from_quest(quest, 7).unwrap();
        let hero_id = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .pos = trigger_door.a;
        game.open_adjacent_door().unwrap();
        let gargoyle_id = game
            .units
            .iter()
            .find(|unit| unit.name == "Stone Gargoyle")
            .unwrap()
            .id;
        assert!(!game.unit(gargoyle_id).unwrap().dormant);

        let plan = game.active_attack_plan().unwrap();
        let body_before = game.unit(gargoyle_id).unwrap().body;
        let attack_faces = vec![CombatFace::Skull; plan.attack_dice as usize];
        let defend_faces = vec![CombatFace::WhiteShield; plan.defend_dice as usize];
        let outcome = game
            .resolve_attack(plan, &attack_faces, &defend_faces)
            .unwrap();
        assert_eq!(outcome.damage, 0);
        assert_eq!(game.unit(gargoyle_id).unwrap().body, body_before);

        game.phase = GamePhase::ZargonTurn;
        game.zargon_turn_started = false;
        let ZargonStep::Attack(attack) = game.advance_zargon_turn().unwrap() else {
            panic!("the awakened adjacent Gargoyle should attack");
        };
        assert_eq!(attack.attacker, gargoyle_id);
        assert!(!game.unit(gargoyle_id).unwrap().invulnerable_until_acts);
    }

    #[test]
    fn melars_maze_resolves_all_five_printed_notes_and_the_artifact_exit() {
        let mut game =
            Game::from_quest(QuestDefinition::original_us_melars_maze().unwrap(), 17).unwrap();
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
            .for_each(|unit| unit.alive = false);
        let hero_id = game.active_hero_id().unwrap();

        game.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .pos = Pos::new(8, 5);
        game.search_treasure().unwrap();
        assert_eq!(
            game.unit(hero_id)
                .unwrap()
                .inventory
                .healing_potion_strengths,
            [2]
        );

        let gargoyle_id = game
            .units
            .iter()
            .find(|unit| unit.name == "Stone Gargoyle")
            .unwrap()
            .id;
        game.hero_turn.action_used = false;
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .pos = Pos::new(3, 13);
        game.open_adjacent_door().unwrap();
        assert!(!game.unit(gargoyle_id).unwrap().dormant);
        assert!(game.unit(gargoyle_id).unwrap().invulnerable_until_acts);

        game.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .pos = Pos::new(2, 16);
        let body_before = game.unit(hero_id).unwrap().body;
        game.search_treasure().unwrap();
        assert_eq!(game.unit(hero_id).unwrap().body, body_before - 2);
        assert_eq!(game.unit(hero_id).unwrap().inventory.gold, 144);
        game.phase = GamePhase::HeroTurn { order_index: 0 };
        game.hero_turn = HeroTurnState::default();
        assert_eq!(game.use_healing_potion().unwrap(), 2);

        let second_hero = game.hero_order[1];
        game.phase = GamePhase::HeroTurn { order_index: 1 };
        game.hero_turn = HeroTurnState::default();
        game.units
            .iter_mut()
            .find(|unit| unit.id == second_hero)
            .unwrap()
            .pos = Pos::new(2, 16);
        assert_eq!(
            game.search_treasure().unwrap_err(),
            RuleError::TreasureForbidden
        );

        game.phase = GamePhase::HeroTurn { order_index: 0 };
        game.hero_turn = HeroTurnState::default();
        let event_door = (Pos::new(4, 7), Pos::new(5, 7));
        assert!(
            !game
                .has_door(event_door.0, event_door.1)
                .unwrap()
                .discovered
        );
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .pos = Pos::new(3, 7);
        game.search_treasure().unwrap();
        assert!(
            game.has_door(event_door.0, event_door.1)
                .unwrap()
                .discovered
        );

        game.hero_turn = HeroTurnState::default();
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .pos = Pos::new(10, 9);
        game.search_treasure().unwrap();
        assert!(
            game.unit(hero_id)
                .unwrap()
                .inventory
                .artifacts
                .contains(&Artifact::TalismanOfLore)
        );

        for (index, id) in game.hero_order.clone().into_iter().enumerate() {
            game.units
                .iter_mut()
                .find(|unit| unit.id == id)
                .unwrap()
                .pos = game.stairs[index];
        }
        game.check_terminal();
        assert_eq!(game.phase, GamePhase::Won);
    }

    #[test]
    fn treasure_hazard_deals_unblocked_damage_and_ends_the_turn() {
        let mut game = Game::demo(7).unwrap();
        let hero_id = game.active_hero_id().unwrap();
        let starting_body = game.unit(hero_id).unwrap().body;
        game.set_treasure_replay_order([TreasureCard::ArrowHazard]);
        game.search_treasure().unwrap();
        assert_eq!(game.unit(hero_id).unwrap().body, starting_body - 1);
        assert!(matches!(game.phase, GamePhase::HeroTurn { order_index: 1 }));
    }

    #[test]
    fn wandering_monster_is_placed_and_queues_its_visible_immediate_attack() {
        let mut game = Game::demo(7).unwrap();
        let hero_id = game.active_hero_id().unwrap();
        let region = game.cell(game.unit(hero_id).unwrap().pos).unwrap().region;
        let body_before = game.unit(hero_id).unwrap().body;
        game.set_treasure_replay_order([TreasureCard::WanderingMonster]);
        let outcome = game.search_treasure().unwrap();
        let monster_id = outcome.wandering_monster.unwrap();
        let monster = game.unit(monster_id).unwrap();
        assert!(monster.alive);
        assert_eq!(game.cell(monster.pos).unwrap().region, region);
        assert!(monster.pos.is_adjacent(game.unit(hero_id).unwrap().pos));
        assert_eq!(game.unit(hero_id).unwrap().body, body_before);
        let plan = game.take_pending_forced_attack().unwrap();
        assert_eq!(plan.attacker, monster_id);
        assert_eq!(plan.defender, hero_id);
    }

    #[test]
    fn line_of_sight_is_blocked_by_closed_doors_and_opened_permanently() {
        let mut game = Game::demo(7).unwrap();
        game.apply_movement_roll(&[6, 6]).unwrap();
        game.move_active(Direction::East).unwrap();
        assert!(!game.can_see(Pos::new(15, 2), Pos::new(16, 2)));
        game.open_adjacent_door().unwrap();
        assert!(game.can_see(Pos::new(15, 2), Pos::new(16, 2)));
    }

    #[test]
    fn figures_block_sight_but_a_line_touching_only_a_corner_does_not() {
        let game = Game::demo(7).unwrap();
        assert!(!game.can_see(Pos::new(13, 1), Pos::new(15, 1)));
        assert!(game.can_see(Pos::new(13, 1), Pos::new(14, 2)));
    }

    #[test]
    fn corridor_placement_reveals_exact_current_sight_and_keeps_prior_pieces_placed() {
        let mut game = Game::from_quest(QuestDefinition::original_us_trial().unwrap(), 7).unwrap();
        game.cells.iter_mut().for_each(|cell| cell.revealed = false);
        let corridors = (0..BOARD_HEIGHT)
            .flat_map(|y| (0..BOARD_WIDTH).map(move |x| Pos::new(x, y)))
            .filter(|&pos| {
                let cell = game.cell(pos).unwrap();
                cell.passable && cell.region == 0
            })
            .collect::<Vec<_>>();
        let first = corridors[0];
        let first_sight = corridors
            .iter()
            .copied()
            .filter(|&target| game.can_see(first, target))
            .collect::<HashSet<_>>();
        game.reveal_from(first);
        for &target in &corridors {
            assert_eq!(
                game.cell(target).unwrap().revealed,
                first_sight.contains(&target),
                "corridor square {target:?} did not match center-to-center sight"
            );
        }
        assert!(
            game.cells
                .iter()
                .filter(|cell| cell.passable && cell.region > 0)
                .all(|cell| !cell.revealed),
            "looking down a corridor must not place a closed room's contents"
        );

        let second = corridors
            .iter()
            .copied()
            .max_by_key(|&observer| {
                corridors
                    .iter()
                    .filter(|&&target| {
                        !first_sight.contains(&target) && game.can_see(observer, target)
                    })
                    .count()
            })
            .unwrap();
        let second_sight = corridors
            .iter()
            .copied()
            .filter(|&target| game.can_see(second, target))
            .collect::<HashSet<_>>();
        game.reveal_from(second);
        for &target in &corridors {
            assert_eq!(
                game.cell(target).unwrap().revealed,
                first_sight.contains(&target) || second_sight.contains(&target),
                "a placed corridor component must remain after the Hero looks elsewhere"
            );
        }
    }

    #[test]
    fn quest_setup_places_only_the_starting_rooms_public_components() {
        let game = Game::from_quest(
            QuestDefinition::original_us_rescue_of_sir_ragnar().unwrap(),
            81,
        )
        .unwrap();
        let starting_regions = game
            .hero_order
            .iter()
            .filter_map(|&id| game.unit(id))
            .map(|hero| game.cell(hero.pos).unwrap().region)
            .collect::<HashSet<_>>();
        assert!(starting_regions.iter().all(|&region| region > 0));
        for cell in &game.cells {
            if cell.passable {
                assert_eq!(
                    cell.revealed,
                    starting_regions.contains(&cell.region),
                    "a non-starting map region leaked during setup"
                );
            }
        }
        assert!(game.units.iter().all(|unit| {
            matches!(unit.figure, FigureKind::Hero(_))
                || game.is_visible(unit)
                    == starting_regions.contains(&game.cell(unit.pos).unwrap().region)
        }));
        assert!(game.props.iter().all(|prop| {
            game.is_prop_visible(prop)
                == starting_regions.contains(&game.cell(prop.pos).unwrap().region)
        }));
        assert!(game.traps.iter().all(|trap| {
            !trap.discovered && !trap.sprung && !game.is_trap_marker_visible(trap)
        }));
        assert!(
            game.doors
                .iter()
                .filter(|door| door.secret)
                .all(|door| !game.is_door_visible(door))
        );
    }

    #[test]
    fn opening_a_door_places_only_that_rooms_public_components() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_rescue_of_sir_ragnar().unwrap(),
            8,
        )
        .unwrap();
        game.cells.iter_mut().for_each(|cell| cell.revealed = false);
        let (door_index, corridor, room, room_region) = game
            .doors
            .iter()
            .enumerate()
            .find_map(|(index, door)| {
                let a_region = game.cell(door.a)?.region;
                let b_region = game.cell(door.b)?.region;
                let (corridor, room, room_region) = if a_region == 0 && b_region > 0 {
                    (door.a, door.b, b_region)
                } else if b_region == 0 && a_region > 0 {
                    (door.b, door.a, a_region)
                } else {
                    return None;
                };
                let has_contents = game.units.iter().any(|unit| {
                    game.cell(unit.pos)
                        .is_some_and(|cell| cell.region == room_region)
                }) || game.props.iter().any(|prop| {
                    game.cell(prop.pos)
                        .is_some_and(|cell| cell.region == room_region)
                });
                has_contents.then_some((index, corridor, room, room_region))
            })
            .expect("quest has a furnished room entered from a corridor");

        game.reveal_from(corridor);
        let door = game.doors[door_index].clone();
        assert!(game.is_door_visible(&door));
        assert!(!game.cell(room).unwrap().revealed);
        assert!(
            game.units
                .iter()
                .filter(|unit| {
                    unit.faction == Faction::Monster
                        && game
                            .cell(unit.pos)
                            .is_some_and(|cell| cell.region == room_region)
                })
                .all(|unit| !game.is_visible(unit))
        );
        assert!(
            game.props
                .iter()
                .filter(|prop| game
                    .cell(prop.pos)
                    .is_some_and(|cell| cell.region == room_region))
                .all(|prop| !game.is_prop_visible(prop))
        );

        game.open_door_index(door_index).unwrap();
        assert!(
            game.cells
                .iter()
                .filter(|cell| cell.region == room_region)
                .all(|cell| cell.revealed)
        );
        assert!(
            game.units
                .iter()
                .filter(|unit| {
                    unit.faction == Faction::Monster
                        && game
                            .cell(unit.pos)
                            .is_some_and(|cell| cell.region == room_region)
                })
                .all(|unit| game.is_visible(unit))
        );
        assert!(
            game.props
                .iter()
                .filter(|prop| game
                    .cell(prop.pos)
                    .is_some_and(|cell| cell.region == room_region))
                .all(|prop| game.is_prop_visible(prop))
        );
        assert!(
            game.traps
                .iter()
                .all(|trap| !trap.discovered && !game.is_trap_marker_visible(trap)),
            "opening a room must not reveal any concealed trap"
        );
        assert!(
            game.doors
                .iter()
                .filter(|door| door.secret)
                .all(|door| !game.is_door_visible(door)),
            "opening a room must not place its secret doors"
        );
        assert!(
            game.cells
                .iter()
                .any(|cell| cell.passable && !cell.revealed)
        );
    }

    #[test]
    fn secret_door_search_reveals_but_does_not_open_the_door() {
        let mut quest = QuestDefinition::demo().unwrap();
        quest.secret_doors.push(DoorDef {
            a: Pos::new(14, 2),
            b: Pos::new(14, 3),
            open: false,
            searchable: true,
            false_door: false,
        });
        let mut game = Game::from_quest(quest, 7).unwrap();
        let found = game.search_secret_doors().unwrap();
        assert_eq!(found, vec![(Pos::new(14, 2), Pos::new(14, 3))]);
        let door = game.has_door(Pos::new(14, 2), Pos::new(14, 3)).unwrap();
        assert!(door.discovered);
        assert!(!door.open);
        game.open_adjacent_door().unwrap();
        assert!(
            game.has_door(Pos::new(14, 2), Pos::new(14, 3))
                .unwrap()
                .open
        );
    }

    #[test]
    fn searched_pit_still_springs_when_entered_and_ends_the_turn() {
        let mut quest = QuestDefinition::demo().unwrap();
        quest.traps.push(TrapDef {
            trap: TrapKind::Pit,
            pos: Pos::new(15, 2),
            trigger_on_entry: true,
            disarmable: true,
        });
        let mut game = Game::from_quest(quest, 7).unwrap();
        assert_eq!(game.search_traps().unwrap(), vec![Pos::new(15, 2)]);
        game.apply_movement_roll(&[3, 4]).unwrap();
        let hero_id = game.active_hero_id().unwrap();
        game.move_active(Direction::East).unwrap();
        assert!(game.unit(hero_id).unwrap().in_pit);
        assert_eq!(game.unit(hero_id).unwrap().body, 7);
        assert!(matches!(game.phase, GamePhase::HeroTurn { order_index: 1 }));
    }

    #[test]
    fn a_trap_immediately_beyond_a_door_cannot_be_found_from_the_other_room() {
        let mut quest = QuestDefinition::demo().unwrap();
        quest
            .monsters
            .retain(|monster| monster.pos != Pos::new(16, 2));
        quest.traps.push(TrapDef {
            trap: TrapKind::Spear,
            pos: Pos::new(16, 2),
            trigger_on_entry: true,
            disarmable: true,
        });
        let mut game = Game::from_quest(quest, 7).unwrap();
        game.units
            .iter_mut()
            .find(|unit| unit.id == game.hero_order[0])
            .unwrap()
            .pos = Pos::new(15, 2);
        game.open_adjacent_door().unwrap();

        assert!(game.search_traps().unwrap().is_empty());
        assert!(!game.traps.last().unwrap().discovered);
    }

    #[test]
    fn a_sprung_pit_is_its_own_treasure_and_secret_door_search_area() {
        let mut quest = QuestDefinition::demo().unwrap();
        quest.traps.push(TrapDef {
            trap: TrapKind::Pit,
            pos: Pos::new(15, 2),
            trigger_on_entry: true,
            disarmable: true,
        });
        quest.secret_doors.push(DoorDef {
            a: Pos::new(15, 2),
            b: Pos::new(15, 3),
            open: false,
            searchable: true,
            false_door: false,
        });
        let mut game = Game::from_quest(quest, 7).unwrap();
        let hero = game.hero_order[0];
        game.set_treasure_replay_order([TreasureCard::Gem35, TreasureCard::GoldCoins15]);
        game.apply_movement_roll(&[2, 2]).unwrap();
        game.move_active(Direction::East).unwrap();
        assert!(game.unit(hero).unwrap().in_pit);

        game.phase = GamePhase::HeroTurn { order_index: 0 };
        game.hero_turn = HeroTurnState::default();
        assert_eq!(
            game.search_treasure().unwrap().discovery,
            TreasureDiscovery::Card(TreasureCard::Gem35)
        );
        assert_eq!(game.unit(hero).unwrap().inventory.gold, 35);

        game.hero_turn = HeroTurnState::default();
        assert_eq!(
            game.search_secret_doors().unwrap(),
            vec![(Pos::new(15, 2), Pos::new(15, 3))]
        );

        game.hero_turn = HeroTurnState::default();
        game.apply_movement_roll(&[1, 1]).unwrap();
        game.move_active(Direction::West).unwrap();
        assert!(!game.unit(hero).unwrap().in_pit);
        assert_eq!(
            game.search_treasure().unwrap().discovery,
            TreasureDiscovery::Card(TreasureCard::GoldCoins15)
        );
        assert_eq!(game.unit(hero).unwrap().inventory.gold, 50);
    }

    #[test]
    fn walking_into_an_open_pit_causes_a_new_fall_and_ends_the_turn() {
        let mut quest = QuestDefinition::demo().unwrap();
        quest.traps.push(TrapDef {
            trap: TrapKind::Pit,
            pos: Pos::new(15, 2),
            trigger_on_entry: true,
            disarmable: true,
        });
        let mut game = Game::from_quest(quest, 7).unwrap();
        let hero = game.hero_order[0];
        game.traps[0].discovered = true;
        game.traps[0].sprung = true;
        let body_before = game.unit(hero).unwrap().body;

        game.apply_movement_roll(&[3, 4]).unwrap();
        game.move_active(Direction::East).unwrap();
        assert!(game.unit(hero).unwrap().in_pit);
        assert_eq!(game.unit(hero).unwrap().body, body_before - 1);
        assert!(matches!(game.phase, GamePhase::HeroTurn { order_index: 1 }));
    }

    #[test]
    fn monsters_jump_a_sprung_pit_when_the_landing_is_reachable_or_enter_without_damage() {
        let mut game = Game::demo(7).unwrap();
        let monster = game
            .units
            .iter()
            .find(|unit| unit.faction == Faction::Monster)
            .unwrap()
            .id;
        let hero = game.hero_order[0];
        for x in 1..=4 {
            let cell = &mut game.cells[Game::cell_index(Pos::new(x, 1))];
            cell.passable = true;
            cell.region = 42;
            cell.revealed = true;
        }
        game.traps.push(Trap {
            kind: TrapKind::Pit,
            pos: Pos::new(2, 1),
            discovered: true,
            sprung: true,
            disarmed: false,
            trigger_on_entry: true,
            disarmable: false,
        });
        for unit in &mut game.units {
            if unit.faction == Faction::Monster {
                unit.alive = unit.id == monster;
            } else if matches!(unit.figure, FigureKind::Hero(_)) {
                unit.escaped = unit.id != hero;
            }
        }
        {
            let unit = game
                .units
                .iter_mut()
                .find(|unit| unit.id == monster)
                .unwrap();
            unit.pos = Pos::new(1, 1);
            unit.stats.movement = 2;
        }
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(4, 1);
        game.phase = GamePhase::ZargonTurn;
        game.zargon_turn_started = false;
        let body_before = game.unit(monster).unwrap().body;

        assert!(matches!(
            game.advance_zargon_turn().unwrap(),
            ZargonStep::Moved {
                unit,
                from: Pos { x: 1, y: 1 },
                to: Pos { x: 3, y: 1 }
            } if unit == monster
        ));
        assert!(!game.unit(monster).unwrap().in_pit);
        assert_eq!(game.unit(monster).unwrap().body, body_before);

        {
            let unit = game
                .units
                .iter_mut()
                .find(|unit| unit.id == monster)
                .unwrap();
            unit.pos = Pos::new(1, 1);
            unit.stats.movement = 1;
            unit.in_pit = false;
        }
        game.phase = GamePhase::ZargonTurn;
        game.zargon_turn_started = false;
        game.zargon_queue.clear();
        game.zargon_active = None;
        assert!(matches!(
            game.advance_zargon_turn().unwrap(),
            ZargonStep::Moved {
                unit,
                to: Pos { x: 2, y: 1 },
                ..
            } if unit == monster
        ));
        assert!(game.unit(monster).unwrap().in_pit);
        assert_eq!(game.unit(monster).unwrap().body, body_before);
    }

    #[test]
    fn spear_trap_waits_for_one_visible_combat_die_before_resolving() {
        let mut quest = QuestDefinition::demo().unwrap();
        quest.traps.push(TrapDef {
            trap: TrapKind::Spear,
            pos: Pos::new(15, 2),
            trigger_on_entry: true,
            disarmable: true,
        });
        let mut game = Game::from_quest(quest.clone(), 7).unwrap();
        let hero = game.active_hero_id().unwrap();
        let body_before = game.unit(hero).unwrap().body;
        game.apply_movement_roll(&[4, 4]).unwrap();
        game.move_active(Direction::East).unwrap();
        let pending = game.pending_trap_roll().unwrap();
        assert_eq!(pending.kind, TrapKind::Spear);
        assert_eq!(pending.dice_count(), 1);
        assert_eq!(game.unit(hero).unwrap().body, body_before);
        assert!(game.active_move_destinations().is_empty());
        assert_eq!(
            game.move_active(Direction::North),
            Err(RuleError::TrapRollPending)
        );
        game.resolve_trap_roll(pending, &[CombatFace::WhiteShield])
            .unwrap();
        assert_eq!(game.unit(hero).unwrap().body, body_before);
        assert_eq!(game.hero_turn.movement_left, 7);

        let mut hit_game = Game::from_quest(quest, 8).unwrap();
        let hit_hero = hit_game.active_hero_id().unwrap();
        let hit_body = hit_game.unit(hit_hero).unwrap().body;
        hit_game.apply_movement_roll(&[4, 4]).unwrap();
        hit_game.move_active(Direction::East).unwrap();
        let hit_pending = hit_game.pending_trap_roll().unwrap();
        hit_game
            .resolve_trap_roll(hit_pending, &[CombatFace::Skull])
            .unwrap();
        assert_eq!(hit_game.unit(hit_hero).unwrap().body, hit_body - 1);
        assert!(matches!(
            hit_game.phase,
            GamePhase::HeroTurn { order_index: 1 }
        ));
    }

    #[test]
    fn falling_block_waits_for_three_visible_dice_and_permanently_blocks_its_square() {
        let mut quest = QuestDefinition::demo().unwrap();
        quest.traps.push(TrapDef {
            trap: TrapKind::FallingBlock,
            pos: Pos::new(15, 2),
            trigger_on_entry: true,
            disarmable: true,
        });
        let mut game = Game::from_quest(quest, 7).unwrap();
        let hero_id = game.active_hero_id().unwrap();
        let body_before = game.unit(hero_id).unwrap().body;
        game.apply_movement_roll(&[4, 4]).unwrap();
        game.move_active(Direction::East).unwrap();
        let pending = game.pending_trap_roll().unwrap();
        assert_eq!(pending.kind, TrapKind::FallingBlock);
        assert_eq!(pending.dice_count(), 3);
        assert_eq!(game.unit(hero_id).unwrap().body, body_before);
        assert!(game.cell(Pos::new(15, 2)).unwrap().passable);
        game.resolve_trap_roll(
            pending,
            &[
                CombatFace::Skull,
                CombatFace::WhiteShield,
                CombatFace::BlackShield,
            ],
        )
        .unwrap();
        assert!(!game.cell(Pos::new(15, 2)).unwrap().passable);
        assert_eq!(game.unit(hero_id).unwrap().pos, Pos::new(14, 2));
        assert_eq!(game.unit(hero_id).unwrap().body, body_before - 1);
        assert!(game.traps[0].sprung);
    }

    #[test]
    fn pit_penalty_never_reduces_attack_or_defense_below_one_die() {
        assert_eq!(pit_adjusted_dice(3, true), 2);
        assert_eq!(pit_adjusted_dice(1, true), 1);
        assert_eq!(pit_adjusted_dice(4, false), 4);
    }

    #[test]
    fn tool_kit_disarms_on_a_shield_and_movement_may_continue() {
        let mut quest = QuestDefinition::demo().unwrap();
        quest.traps.push(TrapDef {
            trap: TrapKind::Pit,
            pos: Pos::new(15, 2),
            trigger_on_entry: true,
            disarmable: true,
        });
        let mut game = Game::from_quest(quest, 7).unwrap();
        let hero_id = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .inventory
            .tool_kits = 1;
        game.traps[0].discovered = true;
        game.apply_movement_roll(&[3, 4]).unwrap();
        let plan = game.active_disarm_plan().unwrap();
        assert!(game.resolve_disarm(plan, CombatFace::WhiteShield).unwrap());
        assert!(game.traps[0].disarmed);
        assert_eq!(game.unit(hero_id).unwrap().pos, Pos::new(15, 2));
        assert_eq!(game.hero_turn.movement_left, 6);
        game.move_active(Direction::North).unwrap();
        assert_eq!(game.unit(hero_id).unwrap().pos, Pos::new(15, 1));
    }

    #[test]
    fn tool_kit_failure_springs_the_trap() {
        let mut quest = QuestDefinition::demo().unwrap();
        quest.traps.push(TrapDef {
            trap: TrapKind::Pit,
            pos: Pos::new(15, 2),
            trigger_on_entry: true,
            disarmable: true,
        });
        let mut game = Game::from_quest(quest, 7).unwrap();
        let hero_id = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .inventory
            .tool_kits = 1;
        game.traps[0].discovered = true;
        game.apply_movement_roll(&[3, 4]).unwrap();
        let plan = game.active_disarm_plan().unwrap();
        assert!(!game.resolve_disarm(plan, CombatFace::Skull).unwrap());
        assert!(game.traps[0].sprung);
        assert!(game.unit(hero_id).unwrap().in_pit);
    }

    #[test]
    fn successful_jump_spends_two_moves_and_lands_beyond_the_trap() {
        let mut quest = QuestDefinition::demo().unwrap();
        quest.traps.push(TrapDef {
            trap: TrapKind::Pit,
            pos: Pos::new(15, 2),
            trigger_on_entry: true,
            disarmable: true,
        });
        let mut game = Game::from_quest(quest, 7).unwrap();
        game.traps[0].discovered = true;
        game.doors
            .iter_mut()
            .find(|door| door.connects(Pos::new(15, 2), Pos::new(16, 2)))
            .unwrap()
            .open = true;
        game.units
            .iter_mut()
            .find(|unit| unit.pos == Pos::new(16, 2))
            .unwrap()
            .alive = false;
        let hero_id = game.active_hero_id().unwrap();
        game.apply_movement_roll(&[3, 4]).unwrap();
        let plan = game.active_jump_plan(Direction::East).unwrap();
        assert!(game.resolve_jump(plan, CombatFace::WhiteShield).unwrap());
        assert_eq!(game.unit(hero_id).unwrap().pos, Pos::new(16, 2));
        assert_eq!(game.hero_turn.movement_left, 5);
    }

    #[test]
    fn failed_jump_springs_the_trap_and_ends_on_its_square() {
        let mut quest = QuestDefinition::demo().unwrap();
        quest.traps.push(TrapDef {
            trap: TrapKind::Pit,
            pos: Pos::new(15, 2),
            trigger_on_entry: true,
            disarmable: true,
        });
        let mut game = Game::from_quest(quest, 7).unwrap();
        game.traps[0].discovered = true;
        game.doors
            .iter_mut()
            .find(|door| door.connects(Pos::new(15, 2), Pos::new(16, 2)))
            .unwrap()
            .open = true;
        game.units
            .iter_mut()
            .find(|unit| unit.pos == Pos::new(16, 2))
            .unwrap()
            .alive = false;
        let hero_id = game.active_hero_id().unwrap();
        game.apply_movement_roll(&[3, 4]).unwrap();
        let plan = game.active_jump_plan(Direction::East).unwrap();
        assert!(!game.resolve_jump(plan, CombatFace::Skull).unwrap());
        assert_eq!(game.unit(hero_id).unwrap().pos, Pos::new(15, 2));
        assert!(game.unit(hero_id).unwrap().in_pit);
        assert!(game.traps[0].sprung);
    }

    #[test]
    fn an_action_before_movement_allows_the_entire_remaining_move() {
        let mut game = Game::demo(7).unwrap();
        game.set_treasure_replay_order([TreasureCard::Gem35]);
        game.search_treasure().unwrap();
        game.apply_movement_roll(&[3, 4]).unwrap();
        game.move_active(Direction::East).unwrap();
        game.move_active(Direction::North).unwrap();
        assert_eq!(game.hero_turn.movement_left, 5);
    }

    #[test]
    fn heroes_begin_with_their_original_weapons() {
        let game = Game::demo(7).unwrap();
        let weapons: Vec<_> = game
            .hero_order
            .iter()
            .map(|&id| game.unit(id).unwrap().inventory.equipped_weapon)
            .collect();
        assert_eq!(
            weapons,
            vec![
                Some(Weapon::Broadsword),
                Some(Weapon::Shortsword),
                Some(Weapon::Shortsword),
                Some(Weapon::Dagger),
            ]
        );
    }

    #[test]
    fn plate_mail_uses_one_physical_movement_die() {
        let mut game = Game::demo(7).unwrap();
        let hero_id = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .inventory
            .armor
            .push(Armor::PlateMail);
        assert_eq!(game.active_movement_dice_count(), 1);
        assert_eq!(
            game.apply_movement_roll(&[2, 5]).unwrap_err(),
            RuleError::InvalidDice
        );
        assert_eq!(game.apply_movement_roll(&[5]).unwrap(), 5);
    }

    #[test]
    fn armor_and_weapon_conflicts_change_defense_dice() {
        let mut inventory = Inventory::default();
        inventory.armor = vec![Armor::Helmet, Armor::Shield, Armor::ChainMail];
        inventory.equipped_weapon = Some(Weapon::Broadsword);
        assert_eq!(inventory.defense_dice(2), 5);
        inventory.equipped_weapon = Some(Weapon::BattleAxe);
        assert_eq!(inventory.defense_dice(2), 4);
    }

    #[test]
    fn borins_armor_replaces_body_armor_and_never_slows_its_legal_wearer() {
        let mut game = Game::demo(0x424f_5249_4e).unwrap();
        let barbarian = game.hero_order[0];
        let hero = game
            .units
            .iter_mut()
            .find(|unit| unit.id == barbarian)
            .unwrap();
        hero.inventory.artifacts.push(Artifact::BorinsArmor);
        hero.inventory.armor = vec![
            Armor::ChainMail,
            Armor::PlateMail,
            Armor::Helmet,
            Armor::Shield,
        ];
        assert_eq!(hero.effective_defense_dice(), hero.stats.defend + 4);
        hero.inventory.artifacts.push(Artifact::WizardsCloak);
        assert_eq!(
            hero.effective_defense_dice(),
            hero.stats.defend + 4,
            "the Wizard's Cloak does nothing for another Hero"
        );
        assert_eq!(game.active_movement_dice_count(), 2);

        let wizard = game.hero_order[3];
        game.phase = GamePhase::HeroTurn { order_index: 3 };
        game.hero_turn = HeroTurnState::default();
        let hero = game
            .units
            .iter_mut()
            .find(|unit| unit.id == wizard)
            .unwrap();
        hero.inventory.artifacts.push(Artifact::BorinsArmor);
        hero.inventory.artifacts.push(Artifact::WizardsCloak);
        hero.inventory.armor.push(Armor::PlateMail);
        assert_eq!(
            hero.effective_defense_dice(),
            hero.stats.defend + 1,
            "the Wizard ignores forbidden Plate Mail but may use the Wizard's Cloak"
        );
        assert_eq!(game.active_movement_dice_count(), 2);
    }

    #[test]
    fn wizard_cannot_use_any_forbidden_armory_weapon_or_armor_even_if_carried() {
        let mut game = Game::demo(0x5749_5a41_5244).unwrap();
        let wizard = game.hero_order[3];
        let monster = game
            .units
            .iter()
            .find(|unit| unit.faction == Faction::Monster)
            .unwrap()
            .id;
        for hero in game.hero_order[..3].iter().copied() {
            game.units
                .iter_mut()
                .find(|unit| unit.id == hero)
                .unwrap()
                .escaped = true;
        }
        for unit in game
            .units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
        {
            unit.alive = unit.id == monster;
        }
        {
            let hero = game
                .units
                .iter_mut()
                .find(|unit| unit.id == wizard)
                .unwrap();
            hero.pos = Pos::new(14, 2);
            hero.inventory.weapons = vec![
                Weapon::Dagger,
                Weapon::Staff,
                Weapon::Crossbow,
                Weapon::Shortsword,
                Weapon::Broadsword,
                Weapon::Longsword,
                Weapon::BattleAxe,
            ];
            hero.inventory.equipped_weapon = Some(Weapon::BattleAxe);
            hero.inventory.armor = vec![
                Armor::Helmet,
                Armor::Shield,
                Armor::ChainMail,
                Armor::PlateMail,
            ];
        }
        game.units
            .iter_mut()
            .find(|unit| unit.id == monster)
            .unwrap()
            .pos = Pos::new(15, 2);
        game.cells[Game::cell_index(Pos::new(15, 2))].revealed = true;
        game.allocate_pending_visible_figures();
        game.phase = GamePhase::HeroTurn { order_index: 3 };
        game.hero_turn = HeroTurnState::default();

        let hero = game.unit(wizard).unwrap();
        assert_eq!(hero.effective_defense_dice(), hero.stats.defend);
        assert_eq!(game.active_movement_dice_count(), 2);
        let sources = game
            .active_attack_options()
            .unwrap()
            .into_iter()
            .map(|plan| plan.source)
            .collect::<Vec<_>>();
        assert!(sources.contains(&AttackSource::Weapon(Weapon::Dagger)));
        assert!(sources.contains(&AttackSource::Weapon(Weapon::Staff)));
        assert!(sources.iter().all(|source| matches!(
            source,
            AttackSource::Weapon(Weapon::Dagger | Weapon::Staff) | AttackSource::WizardsStaff
        )));
    }

    #[test]
    fn elixir_of_life_revives_one_dead_hero_at_full_body_and_is_consumed() {
        let mut game = Game::demo(0x454c_4958_4952).unwrap();
        let owner = game.hero_order[0];
        let fallen = game.hero_order[1];
        {
            let owner = game.units.iter_mut().find(|unit| unit.id == owner).unwrap();
            owner.inventory.artifacts.push(Artifact::ElixirOfLife);
        }
        {
            let fallen = game
                .units
                .iter_mut()
                .find(|unit| unit.id == fallen)
                .unwrap();
            fallen.body = 0;
            fallen.alive = false;
            fallen.in_pit = true;
            fallen.sleeping = true;
            fallen.petrified_turns = 3;
            fallen.fearful = true;
        }

        assert_eq!(game.elixir_of_life_targets(), vec![fallen]);
        let revival_pos = game.use_elixir_of_life(fallen).unwrap();
        let revived = game.unit(fallen).unwrap();
        assert_eq!(revived.pos, revival_pos);
        assert_eq!(revived.body, revived.stats.body as i16);
        assert!(revived.alive);
        assert!(!revived.in_pit);
        assert!(!revived.sleeping);
        assert_eq!(revived.petrified_turns, 0);
        assert!(!revived.fearful);
        assert!(
            !game
                .unit(owner)
                .unwrap()
                .inventory
                .artifacts
                .contains(&Artifact::ElixirOfLife)
        );
        assert_eq!(
            game.use_elixir_of_life(fallen).unwrap_err(),
            RuleError::NoElixirOfLife
        );
    }

    #[test]
    fn spell_ring_declares_one_spell_and_preserves_it_for_exactly_two_casts() {
        let (mut game, wizard, _) = game_for_hero_spell(&[HeroSpell::RockSkin]);
        game.units
            .iter_mut()
            .find(|unit| unit.id == wizard)
            .unwrap()
            .inventory
            .artifacts
            .push(Artifact::SpellRing);

        assert_eq!(game.spell_ring_storable_spells(), vec![HeroSpell::RockSkin]);
        game.store_active_spell_in_ring(HeroSpell::RockSkin)
            .unwrap();
        assert_eq!(game.unit(wizard).unwrap().spell_ring_casts_left, 2);
        game.cast_active_hero_spell(HeroSpell::RockSkin, HeroSpellTarget::Unit(wizard))
            .unwrap();
        assert!(
            game.unit(wizard)
                .unwrap()
                .hero_spells
                .contains(&HeroSpell::RockSkin)
        );
        assert_eq!(game.unit(wizard).unwrap().spell_ring_casts_left, 1);
        assert!(game.unit(wizard).unwrap().discarded_hero_spells.is_empty());

        game.hero_turn = HeroTurnState::default();
        game.cast_active_hero_spell(HeroSpell::RockSkin, HeroSpellTarget::Unit(wizard))
            .unwrap();
        assert!(
            !game
                .unit(wizard)
                .unwrap()
                .hero_spells
                .contains(&HeroSpell::RockSkin)
        );
        assert_eq!(game.unit(wizard).unwrap().spell_ring_casts_left, 0);
        assert_eq!(
            game.unit(wizard).unwrap().discarded_hero_spells,
            [HeroSpell::RockSkin]
        );
    }

    #[test]
    fn wand_of_magic_allows_two_different_spells_but_never_the_same_spell() {
        let (mut game, wizard, _) =
            game_for_hero_spell(&[HeroSpell::RockSkin, HeroSpell::SwiftWind]);
        game.units
            .iter_mut()
            .find(|unit| unit.id == wizard)
            .unwrap()
            .inventory
            .artifacts
            .extend([Artifact::WandOfMagic, Artifact::SpellRing]);
        game.store_active_spell_in_ring(HeroSpell::RockSkin)
            .unwrap();

        game.cast_active_hero_spell(HeroSpell::RockSkin, HeroSpellTarget::Unit(wizard))
            .unwrap();
        assert!(
            game.unit(wizard)
                .unwrap()
                .hero_spells
                .contains(&HeroSpell::RockSkin),
            "the Spell Ring deliberately keeps the first card available"
        );
        assert_eq!(
            game.active_castable_hero_spells(),
            vec![HeroSpell::SwiftWind]
        );
        assert!(
            game.valid_hero_spell_targets(HeroSpell::RockSkin)
                .is_empty()
        );
        game.cast_active_hero_spell(HeroSpell::SwiftWind, HeroSpellTarget::Unit(wizard))
            .unwrap();
        assert!(game.active_castable_hero_spells().is_empty());
        assert!(game.hero_turn.wand_follow_up_used);
        assert_eq!(
            game.unit(wizard).unwrap().discarded_hero_spells,
            [HeroSpell::SwiftWind]
        );
    }

    #[test]
    fn captured_heroes_fight_unequipped_and_reclaim_belongings_individually() {
        let mut quest = QuestDefinition::demo().unwrap();
        quest.heroes_captured = true;
        quest.events.push(QuestEventDef {
            id: "RECOVER_EQUIPMENT".to_owned(),
            marker: Some(Pos::new(14, 2)),
            trigger: QuestTriggerDef::SearchTreasure {
                room: "Stair Chamber".to_owned(),
            },
            effect: QuestEffectDef::RevealStoredEquipment,
            message: None,
        });
        let mut game = Game::from_quest(quest, 29).unwrap();
        let barbarian = game.hero_order[0];
        let dwarf = game.hero_order[1];
        let goblin = game
            .units
            .iter()
            .find(|unit| unit.faction == Faction::Monster)
            .unwrap()
            .id;
        game.units
            .iter_mut()
            .find(|unit| unit.id == goblin)
            .unwrap()
            .pos = Pos::new(15, 2);
        game.cells[Game::cell_index(Pos::new(15, 2))].revealed = true;
        game.allocate_pending_visible_figures();

        assert_eq!(game.active_attack_plan().unwrap().attack_dice, 1);
        assert_eq!(
            game.monster_attack_plan(goblin, barbarian)
                .unwrap()
                .defend_dice,
            2
        );
        assert_eq!(game.active_movement_dice_count(), 2);
        {
            let hero = game
                .units
                .iter_mut()
                .find(|unit| unit.id == barbarian)
                .unwrap();
            hero.body -= 1;
            hero.inventory.potion_of_healing = 1;
            hero.inventory.healing_potion_strengths.push(4);
        }
        assert_eq!(
            game.use_healing_potion().unwrap_err(),
            RuleError::EquipmentUnavailable
        );

        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
            .for_each(|unit| unit.alive = false);
        assert_eq!(
            game.search_treasure().unwrap().discovery,
            TreasureDiscovery::QuestEvent
        );
        assert!(game.unit(barbarian).unwrap().equipment_available);
        assert!(game.unit(barbarian).unwrap().spellcasting_available);
        assert!(!game.unit(dwarf).unwrap().equipment_available);

        game.phase = GamePhase::HeroTurn { order_index: 1 };
        game.hero_turn = HeroTurnState::default();
        game.units
            .iter_mut()
            .filter(|unit| unit.id != dwarf && unit.pos == Pos::new(13, 2))
            .for_each(|unit| unit.pos = Pos::new(10, 7));
        game.units
            .iter_mut()
            .find(|unit| unit.id == dwarf)
            .unwrap()
            .pos = Pos::new(13, 1);
        game.apply_movement_roll(&[1, 1]).unwrap();
        game.move_active(Direction::South).unwrap();
        assert!(game.unit(dwarf).unwrap().equipment_available);
        assert!(game.unit(dwarf).unwrap().spellcasting_available);
    }

    #[test]
    fn escape_objective_removes_each_hero_until_the_survivors_are_safe() {
        let mut quest = QuestDefinition::demo().unwrap();
        quest.objective = ObjectiveDef::EscapeIndependently;
        let mut game = Game::from_quest(quest, 31).unwrap();
        let hero_order = game.hero_order.clone();
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
            .for_each(|unit| unit.alive = false);
        let (origin, direction, destination) = (0..BOARD_HEIGHT)
            .flat_map(|y| (0..BOARD_WIDTH).map(move |x| Pos::new(x, y)))
            .find_map(|origin| {
                Direction::ALL.into_iter().find_map(|direction| {
                    let destination = origin.offset(direction)?;
                    (game.cell(origin).is_some_and(|cell| cell.passable)
                        && !game.stairs.contains(&origin)
                        && game.stairs.contains(&destination)
                        && game.boundary_is_open(origin, destination)
                        && !game.is_furniture_square(origin))
                    .then_some((origin, direction, destination))
                })
            })
            .expect("the physical stairway needs an open approach square");

        for (order_index, hero_id) in hero_order.into_iter().enumerate() {
            game.phase = GamePhase::HeroTurn { order_index };
            game.hero_turn = HeroTurnState::default();
            game.units
                .iter_mut()
                .find(|unit| unit.id == hero_id)
                .unwrap()
                .pos = origin;
            game.apply_movement_roll(&[1, 1]).unwrap();
            assert_eq!(game.move_active(direction).unwrap(), destination);
            assert!(game.unit(hero_id).unwrap().escaped);
            assert!(!game.is_visible(game.unit(hero_id).unwrap()));
            assert_ne!(game.occupied_by_alive(destination, None), Some(hero_id));
            if order_index + 1 < game.hero_order.len() {
                assert!(!matches!(game.phase, GamePhase::Won));
            }
        }
        assert_eq!(game.phase, GamePhase::Won);
    }

    #[test]
    fn grak_casts_each_printed_spell_once_and_drops_the_wizards_cloak() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_legacy_of_the_orc_warlord().unwrap(),
            37,
        )
        .unwrap();
        let grak = game
            .units
            .iter()
            .find(|unit| unit.name == "Grak")
            .unwrap()
            .id;
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster && unit.id != grak)
            .for_each(|unit| unit.alive = false);
        for cell in &mut game.cells {
            if cell.passable {
                cell.revealed = true;
            }
        }
        let hero_positions = [
            Pos::new(5, 14),
            Pos::new(7, 14),
            Pos::new(5, 15),
            Pos::new(7, 15),
        ];
        for (&hero_id, pos) in game.hero_order.clone().iter().zip(hero_positions) {
            game.units
                .iter_mut()
                .find(|unit| unit.id == hero_id)
                .unwrap()
                .pos = pos;
        }
        game.phase = GamePhase::ZargonTurn;

        let ZargonStep::Cast {
            target: feared,
            spell: ChaosSpell::Fear,
            resistance_dice: 0,
            ..
        } = game.advance_zargon_turn().unwrap()
        else {
            panic!("Grak should cast Fear first");
        };
        assert!(game.unit(feared).unwrap().fearful);
        assert!(
            game.resolve_chaos_spell_resistance(feared, ChaosSpell::Fear, &[6, 1])
                .unwrap()
        );

        game.zargon_turn_started = false;
        game.zargon_queue.clear();
        let ZargonStep::Cast {
            target: sleeping,
            spell: ChaosSpell::Sleep,
            resistance_dice,
            ..
        } = game.advance_zargon_turn().unwrap()
        else {
            panic!("Grak should cast Sleep second");
        };
        assert_eq!(
            resistance_dice,
            game.unit(sleeping).unwrap().effective_mind()
        );
        assert!(
            !game
                .resolve_chaos_spell_resistance(sleeping, ChaosSpell::Sleep, &[1, 1])
                .unwrap()
        );
        assert!(game.unit(sleeping).unwrap().sleeping);
        assert_eq!(game.unit(sleeping).unwrap().effective_defense_dice(), 0);

        game.zargon_turn_started = false;
        game.zargon_queue.clear();
        let ZargonStep::Cast {
            target: tempested,
            spell: ChaosSpell::Tempest,
            resistance_dice: 0,
            ..
        } = game.advance_zargon_turn().unwrap()
        else {
            panic!("Grak should cast Tempest third");
        };
        assert_eq!(game.unit(tempested).unwrap().skip_turns, 1);
        assert!(game.unit(grak).unwrap().chaos_spells.is_empty());
        assert_eq!(
            game.unit(grak).unwrap().discarded_chaos_spells,
            [ChaosSpell::Fear, ChaosSpell::Sleep, ChaosSpell::Tempest]
        );
        assert_eq!(
            game.discarded_chaos_spells,
            [ChaosSpell::Fear, ChaosSpell::Sleep, ChaosSpell::Tempest]
        );

        game.units
            .iter_mut()
            .filter(|unit| matches!(unit.figure, FigureKind::Hero(_)))
            .for_each(|unit| {
                unit.sleeping = false;
                unit.fearful = false;
                if unit.id != tempested {
                    unit.alive = false;
                }
            });
        assert_eq!(game.next_hero_turn_index(0), None);
        assert_eq!(game.unit(tempested).unwrap().skip_turns, 0);
        game.units
            .iter_mut()
            .find(|unit| unit.id == tempested)
            .unwrap()
            .pos = Pos::new(5, 14);
        assert_eq!(game.adjacent_hero(grak), Some(tempested));

        let wizard = game
            .units
            .iter()
            .find(|unit| {
                matches!(
                    unit.figure,
                    FigureKind::Hero(crate::model::HeroKind::Wizard)
                )
            })
            .unwrap()
            .id;
        game.units
            .iter_mut()
            .find(|unit| unit.id == wizard)
            .unwrap()
            .alive = true;
        game.units
            .iter_mut()
            .find(|unit| unit.id == wizard)
            .unwrap()
            .equipment_available = true;
        let defense_before = game.unit(wizard).unwrap().effective_defense_dice();
        game.resolve_defeat_events("Grak", None);
        assert!(
            game.unit(wizard)
                .unwrap()
                .inventory
                .artifacts
                .contains(&Artifact::WizardsCloak)
        );
        assert_eq!(
            game.unit(wizard).unwrap().effective_defense_dice(),
            defense_before + 1
        );
    }

    #[test]
    fn lost_wizard_requires_wardozs_papers_and_rewards_each_returning_hero() {
        let mut game =
            Game::from_quest(QuestDefinition::original_us_lost_wizard().unwrap(), 7).unwrap();
        let hero_id = game.hero_order[0];
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .pos = Pos::new(15, 16);

        assert_eq!(
            game.search_treasure().unwrap_err(),
            RuleError::MonstersInRoom
        );
        let wardoz = game
            .units
            .iter_mut()
            .find(|unit| unit.name == "Wardoz")
            .unwrap();
        wardoz.alive = false;
        wardoz.body = 0;

        assert_eq!(
            game.search_treasure().unwrap().discovery,
            TreasureDiscovery::Gold(144)
        );
        assert_eq!(game.unit(hero_id).unwrap().inventory.gold, 144);
        assert!(!matches!(game.phase, GamePhase::Won));

        for (index, id) in game.hero_order.clone().into_iter().enumerate() {
            game.units
                .iter_mut()
                .find(|unit| unit.id == id)
                .unwrap()
                .pos = game.stairs[index];
        }
        game.check_terminal();
        assert_eq!(game.phase, GamePhase::Won);
        assert_eq!(game.unit(hero_id).unwrap().inventory.gold, 244);
        assert!(
            game.hero_order[1..]
                .iter()
                .all(|&id| game.unit(id).unwrap().inventory.gold == 100)
        );
    }

    #[test]
    fn lost_wizard_purple_liquid_petrifies_for_five_invulnerable_turns() {
        let mut game =
            Game::from_quest(QuestDefinition::original_us_lost_wizard().unwrap(), 7).unwrap();
        let hero_id = game.hero_order[0];
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .pos = Pos::new(8, 13);
        let body_before = game.unit(hero_id).unwrap().body;

        game.search_treasure().unwrap();
        assert_eq!(game.unit(hero_id).unwrap().body, body_before - 2);
        assert_eq!(
            game.unit(hero_id).unwrap().inventory.petrification_potion,
            1
        );
        assert!(matches!(game.phase, GamePhase::HeroTurn { order_index: 1 }));
        game.phase = GamePhase::HeroTurn { order_index: 0 };
        game.hero_turn = HeroTurnState::default();
        game.use_petrification_potion().unwrap();
        assert_eq!(game.unit(hero_id).unwrap().petrified_turns, 5);
        assert_eq!(game.unit(hero_id).unwrap().effective_defense_dice(), 0);

        let stone_body = game.unit(hero_id).unwrap().body;
        game.damage_without_defense(hero_id, 3);
        assert_eq!(game.unit(hero_id).unwrap().body, stone_body);
        for remaining in (0..5).rev() {
            assert_eq!(game.next_hero_turn_index(0), Some(1));
            assert_eq!(game.unit(hero_id).unwrap().petrified_turns, remaining);
        }
        assert_eq!(game.next_hero_turn_index(0), Some(0));
    }

    #[test]
    fn lost_wizard_borins_armor_adds_two_defend_dice_for_a_legal_wearer() {
        let mut game =
            Game::from_quest(QuestDefinition::original_us_lost_wizard().unwrap(), 7).unwrap();
        let hero_id = game.hero_order[0];
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .pos = Pos::new(21, 14);
        let room = game.cell(Pos::new(21, 14)).unwrap().region;
        for unit in &mut game.units {
            if unit.faction == Faction::Monster
                && game.cells[Game::cell_index(unit.pos)].region == room
            {
                unit.alive = false;
                unit.body = 0;
            }
        }
        let defense_before = game.unit(hero_id).unwrap().effective_defense_dice();

        game.search_treasure().unwrap();
        assert!(
            game.unit(hero_id)
                .unwrap()
                .inventory
                .artifacts
                .contains(&Artifact::BorinsArmor)
        );
        assert_eq!(
            game.unit(hero_id).unwrap().effective_defense_dice(),
            defense_before + 2
        );
    }

    fn balur_spell_test_game(spell: ChaosSpell) -> (Game, UnitId, UnitId) {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_fire_mage().unwrap(),
            0x4241_4c55,
        )
        .unwrap();
        let balur = game
            .units
            .iter()
            .find(|unit| unit.name == "Balur")
            .unwrap()
            .id;
        let hero = game.hero_order[0];
        for unit in &mut game.units {
            if unit.faction == Faction::Monster && unit.id != balur {
                unit.alive = false;
                unit.body = 0;
            }
            if matches!(unit.figure, FigureKind::Hero(_)) && unit.id != hero {
                unit.escaped = true;
            }
        }
        let balur_unit = game.units.iter_mut().find(|unit| unit.id == balur).unwrap();
        balur_unit.pos = Pos::new(10, 7);
        balur_unit.chaos_spells = vec![spell];
        let hero_unit = game.units.iter_mut().find(|unit| unit.id == hero).unwrap();
        hero_unit.pos = Pos::new(11, 7);
        for cell in &mut game.cells {
            if cell.passable {
                cell.revealed = true;
            }
        }
        game.phase = GamePhase::ZargonTurn;
        game.zargon_turn_started = false;
        game.zargon_queue.clear();
        (game, balur, hero)
    }

    #[test]
    fn balurs_ball_of_flame_uses_two_red_saves_against_two_body_points() {
        let (mut game, balur, hero) = balur_spell_test_game(ChaosSpell::BallOfFlame);
        let body_before = game.unit(hero).unwrap().body;
        let ZargonStep::Cast {
            caster,
            target,
            spell,
            resistance_dice,
        } = game.advance_zargon_turn().unwrap()
        else {
            panic!("Balur should yield a physical Ball of Flame save");
        };
        assert_eq!(
            (caster, target, spell, resistance_dice),
            (balur, hero, ChaosSpell::BallOfFlame, 2)
        );
        assert!(game.unit(balur).unwrap().chaos_spells.is_empty());
        assert!(
            !game
                .resolve_chaos_spell_resistance(hero, spell, &[5, 1])
                .unwrap()
        );
        assert_eq!(game.unit(hero).unwrap().body, body_before - 1);
        assert!(game.pending_chaos_spell_rolls.is_empty());
        assert_eq!(game.last_combat_visual.unwrap().damage, 1);

        let (mut fully_saved, _, hero) = balur_spell_test_game(ChaosSpell::BallOfFlame);
        let ZargonStep::Cast { spell, .. } = fully_saved.advance_zargon_turn().unwrap() else {
            unreachable!()
        };
        let body_before = fully_saved.unit(hero).unwrap().body;
        assert!(
            fully_saved
                .resolve_chaos_spell_resistance(hero, spell, &[5, 6])
                .unwrap()
        );
        assert_eq!(fully_saved.unit(hero).unwrap().body, body_before);
    }

    #[test]
    fn cloud_of_chaos_paralyzes_every_hero_in_the_room_until_each_rolls_a_six() {
        let (mut game, balur, first_hero) = balur_spell_test_game(ChaosSpell::CloudOfChaos);
        let second_hero = game.hero_order[1];
        let outside_hero = game.hero_order[2];
        {
            let hero = game
                .units
                .iter_mut()
                .find(|unit| unit.id == second_hero)
                .unwrap();
            hero.escaped = false;
            hero.pos = Pos::new(11, 8);
        }
        {
            let hero = game
                .units
                .iter_mut()
                .find(|unit| unit.id == outside_hero)
                .unwrap();
            hero.escaped = false;
            hero.pos = Pos::new(15, 2);
        }

        let step = game.try_cast_chaos_spell(balur).unwrap();
        assert!(matches!(
            step,
            ZargonStep::Cast {
                caster,
                target,
                spell: ChaosSpell::CloudOfChaos,
                resistance_dice: 2
            } if caster == balur && target == first_hero
        ));
        assert_eq!(game.pending_chaos_spell_rolls.len(), 2);
        assert!(game.unit(first_hero).unwrap().clouded);
        assert!(game.unit(second_hero).unwrap().clouded);
        assert!(!game.unit(outside_hero).unwrap().clouded);

        let first_count = game.unit(first_hero).unwrap().effective_mind() as usize;
        assert!(
            !game
                .resolve_chaos_spell_resistance(
                    first_hero,
                    ChaosSpell::CloudOfChaos,
                    &vec![1; first_count],
                )
                .unwrap()
        );
        let ZargonStep::Cast {
            target,
            resistance_dice,
            ..
        } = game.advance_zargon_turn().unwrap()
        else {
            panic!("the second Cloud victim must receive an immediate Mind roll");
        };
        assert_eq!(target, second_hero);
        let mut second_dice = vec![1; resistance_dice as usize];
        *second_dice.last_mut().unwrap() = 6;
        assert!(
            game.resolve_chaos_spell_resistance(
                second_hero,
                ChaosSpell::CloudOfChaos,
                &second_dice,
            )
            .unwrap()
        );
        assert!(!game.unit(second_hero).unwrap().clouded);
        assert!(game.unit(first_hero).unwrap().clouded);
        assert_eq!(game.unit(first_hero).unwrap().effective_defense_dice(), 0);

        game.phase = GamePhase::HeroTurn { order_index: 0 };
        game.hero_turn = HeroTurnState::default();
        assert_eq!(
            game.pending_hero_spell_resistance(),
            Some((first_hero, ChaosSpell::CloudOfChaos, first_count as u8))
        );
        assert_eq!(
            game.apply_movement_roll(&[1, 1]).unwrap_err(),
            RuleError::Incapacitated
        );
        assert_eq!(
            game.active_attack_options().unwrap_err(),
            RuleError::Incapacitated
        );
        let mut future_dice = vec![1; first_count];
        *future_dice.last_mut().unwrap() = 6;
        assert!(
            game.resolve_chaos_spell_resistance(
                first_hero,
                ChaosSpell::CloudOfChaos,
                &future_dice,
            )
            .unwrap()
        );
        assert!(!game.unit(first_hero).unwrap().clouded);
    }

    #[test]
    fn lightning_bolt_hits_every_figure_on_its_best_ray_and_stops_at_a_closed_door() {
        let (mut game, balur, first_hero) = balur_spell_test_game(ChaosSpell::LightningBolt);
        let second_hero = game.hero_order[1];
        let behind_door = game.hero_order[2];
        let victim_monster = game
            .units
            .iter()
            .find(|unit| unit.faction == Faction::Monster && unit.id != balur)
            .unwrap()
            .id;
        for x in 1..=6 {
            let cell = &mut game.cells[Game::cell_index(Pos::new(x, 1))];
            cell.passable = true;
            cell.region = if x <= 5 { 42 } else { 43 };
            cell.revealed = true;
        }
        game.doors
            .retain(|door| !door.connects(Pos::new(5, 1), Pos::new(6, 1)));
        game.doors.push(Door {
            a: Pos::new(5, 1),
            b: Pos::new(6, 1),
            open: false,
            secret: false,
            discovered: true,
            searchable: false,
            false_door: false,
        });
        for unit in &mut game.units {
            if matches!(unit.figure, FigureKind::Hero(_)) {
                unit.escaped = ![first_hero, second_hero, behind_door].contains(&unit.id);
            }
        }
        game.units
            .iter_mut()
            .find(|unit| unit.id == balur)
            .unwrap()
            .pos = Pos::new(1, 1);
        let first_body = {
            let hero = game
                .units
                .iter_mut()
                .find(|unit| unit.id == first_hero)
                .unwrap();
            hero.pos = Pos::new(3, 1);
            hero.body
        };
        let second_body = {
            let hero = game
                .units
                .iter_mut()
                .find(|unit| unit.id == second_hero)
                .unwrap();
            hero.pos = Pos::new(5, 1);
            hero.body
        };
        let behind_body = {
            let hero = game
                .units
                .iter_mut()
                .find(|unit| unit.id == behind_door)
                .unwrap();
            hero.pos = Pos::new(6, 1);
            hero.body
        };
        let monster_body = {
            let monster = game
                .units
                .iter_mut()
                .find(|unit| unit.id == victim_monster)
                .unwrap();
            monster.alive = true;
            monster.body = 3;
            monster.pos = Pos::new(4, 1);
            monster.body
        };

        let step = game.try_cast_chaos_spell(balur).unwrap();
        assert!(matches!(
            step,
            ZargonStep::Cast {
                caster,
                target,
                spell: ChaosSpell::LightningBolt,
                resistance_dice: 0
            } if caster == balur && target == first_hero
        ));
        assert_eq!(game.unit(first_hero).unwrap().body, first_body - 2);
        assert_eq!(game.unit(victim_monster).unwrap().body, monster_body - 2);
        assert_eq!(game.unit(second_hero).unwrap().body, second_body - 2);
        assert_eq!(game.unit(behind_door).unwrap().body, behind_body);
        assert!(game.unit(balur).unwrap().chaos_spells.is_empty());
    }

    #[test]
    fn rust_permanently_discards_a_metal_sword_or_helmet_but_never_an_artifact() {
        let (mut game, balur, hero_id) = balur_spell_test_game(ChaosSpell::Rust);
        {
            let hero = game
                .units
                .iter_mut()
                .find(|unit| unit.id == hero_id)
                .unwrap();
            hero.inventory.weapons = vec![Weapon::Broadsword, Weapon::Longsword];
            hero.inventory.equipped_weapon = Some(Weapon::Longsword);
            hero.inventory.armor = vec![Armor::Helmet];
            hero.inventory.artifacts = vec![Artifact::SpiritBlade];
        }
        let step = game.try_cast_chaos_spell(balur).unwrap();
        assert!(matches!(
            step,
            ZargonStep::Cast {
                caster,
                target,
                spell: ChaosSpell::Rust,
                resistance_dice: 0
            } if caster == balur && target == hero_id
        ));
        let hero = game.unit(hero_id).unwrap();
        assert!(!hero.inventory.weapons.contains(&Weapon::Longsword));
        assert!(hero.inventory.weapons.contains(&Weapon::Broadsword));
        assert!(hero.inventory.armor.contains(&Armor::Helmet));
        assert!(hero.inventory.artifacts.contains(&Artifact::SpiritBlade));

        let (mut helmet_game, balur, hero_id) = balur_spell_test_game(ChaosSpell::Rust);
        {
            let hero = helmet_game
                .units
                .iter_mut()
                .find(|unit| unit.id == hero_id)
                .unwrap();
            hero.inventory.weapons = vec![Weapon::Dagger];
            hero.inventory.equipped_weapon = Some(Weapon::Dagger);
            hero.inventory.armor = vec![Armor::Helmet];
            hero.inventory.artifacts = vec![Artifact::SpiritBlade];
        }
        helmet_game.try_cast_chaos_spell(balur).unwrap();
        let hero = helmet_game.unit(hero_id).unwrap();
        assert!(!hero.inventory.armor.contains(&Armor::Helmet));
        assert!(hero.inventory.artifacts.contains(&Artifact::SpiritBlade));

        let (mut artifact_only, balur, hero_id) = balur_spell_test_game(ChaosSpell::Rust);
        {
            let hero = artifact_only
                .units
                .iter_mut()
                .find(|unit| unit.id == hero_id)
                .unwrap();
            hero.inventory.weapons = vec![Weapon::Dagger];
            hero.inventory.armor.clear();
            hero.inventory.artifacts = vec![Artifact::SpiritBlade];
        }
        assert!(artifact_only.try_cast_chaos_spell(balur).is_none());
        assert_eq!(
            artifact_only.unit(balur).unwrap().chaos_spells,
            vec![ChaosSpell::Rust]
        );
    }

    #[test]
    fn balurs_firestorm_queues_every_other_figure_in_the_room() {
        let (mut game, balur, first_hero) = balur_spell_test_game(ChaosSpell::Firestorm);
        let second_hero = game.hero_order[1];
        let victim_monster = game
            .units
            .iter()
            .find(|unit| unit.faction == Faction::Monster && unit.id != balur)
            .unwrap()
            .id;
        {
            let second = game
                .units
                .iter_mut()
                .find(|unit| unit.id == second_hero)
                .unwrap();
            second.escaped = false;
            second.pos = Pos::new(11, 8);
        }
        {
            let monster = game
                .units
                .iter_mut()
                .find(|unit| unit.id == victim_monster)
                .unwrap();
            monster.alive = true;
            monster.body = 1;
            monster.pos = Pos::new(12, 8);
        }
        let balur_body = game.unit(balur).unwrap().body;
        let first_body = game.unit(first_hero).unwrap().body;
        let second_body = game.unit(second_hero).unwrap().body;

        let firestorm_step = game
            .try_cast_chaos_spell(balur)
            .expect("Balur can cast Firestorm in the shared room");
        let ZargonStep::Cast {
            target,
            spell: ChaosSpell::Firestorm,
            resistance_dice: 2,
            ..
        } = firestorm_step
        else {
            panic!("Firestorm should begin its victim roll queue, got {firestorm_step:?}");
        };
        assert_eq!(target, first_hero);
        assert_eq!(game.pending_chaos_spell_rolls.len(), 3);
        game.resolve_chaos_spell_resistance(first_hero, ChaosSpell::Firestorm, &[5, 6])
            .unwrap();
        assert_eq!(game.unit(first_hero).unwrap().body, first_body - 1);

        let ZargonStep::Cast { target, .. } = game.advance_zargon_turn().unwrap() else {
            unreachable!()
        };
        assert_eq!(target, second_hero);
        game.resolve_chaos_spell_resistance(second_hero, ChaosSpell::Firestorm, &[1, 1])
            .unwrap();
        assert_eq!(game.unit(second_hero).unwrap().body, second_body - 3);

        let ZargonStep::Cast { target, .. } = game.advance_zargon_turn().unwrap() else {
            unreachable!()
        };
        assert_eq!(target, victim_monster);
        game.resolve_chaos_spell_resistance(victim_monster, ChaosSpell::Firestorm, &[6, 6])
            .unwrap();
        assert!(!game.unit(victim_monster).unwrap().alive);
        assert_eq!(game.unit(balur).unwrap().body, balur_body);
        assert!(game.pending_chaos_spell_rolls.is_empty());
    }

    #[test]
    fn balurs_summon_orcs_roll_places_four_to_six_real_orcs_around_him() {
        for (face, expected) in [(1, 4), (4, 5), (6, 6)] {
            let (mut game, balur, _) = balur_spell_test_game(ChaosSpell::SummonOrcs);
            let orcs_before = game
                .units
                .iter()
                .filter(|unit| unit.alive && unit.figure == FigureKind::Monster(MonsterKind::Orc))
                .count();
            let ZargonStep::Cast {
                target,
                spell: ChaosSpell::SummonOrcs,
                resistance_dice: 1,
                ..
            } = game.advance_zargon_turn().unwrap()
            else {
                panic!("Summon Orcs should request one physical red die");
            };
            assert_eq!(target, balur);
            game.resolve_chaos_spell_resistance(balur, ChaosSpell::SummonOrcs, &[face])
                .unwrap();
            let spawned = game
                .units
                .iter()
                .filter(|unit| unit.alive && unit.figure == FigureKind::Monster(MonsterKind::Orc))
                .collect::<Vec<_>>();
            assert_eq!(spawned.len() - orcs_before, expected);
            let region = game.cell(game.unit(balur).unwrap().pos).unwrap().region;
            assert!(
                spawned
                    .iter()
                    .all(|orc| game.cell(orc.pos).unwrap().region == region)
            );
        }
    }

    #[test]
    fn balurs_escape_hides_him_at_xx_until_the_middle_room_is_opened() {
        let (mut game, balur, hero) = balur_spell_test_game(ChaosSpell::Escape);
        let target = game.unit(balur).unwrap().escape_target.unwrap();
        let target_region = game.cell(target).unwrap().region;
        for cell in &mut game.cells {
            if cell.region == target_region {
                cell.revealed = false;
            }
        }
        let escape_step = game
            .try_cast_chaos_spell(balur)
            .expect("Balur has an unoccupied printed Escape destination");
        let ZargonStep::Cast {
            caster,
            target: cast_target,
            spell: ChaosSpell::Escape,
            resistance_dice: 0,
        } = escape_step
        else {
            panic!("Escape should be a visible zero-die cast action, got {escape_step:?}");
        };
        assert_eq!((caster, cast_target), (balur, balur));
        assert_eq!(game.unit(balur).unwrap().pos, target);
        assert!(!game.is_visible(game.unit(balur).unwrap()));
        assert!(game.unit(balur).unwrap().chaos_spells.is_empty());

        game.phase = GamePhase::HeroTurn { order_index: 0 };
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(16, 9);
        assert_eq!(
            game.open_adjacent_door().unwrap(),
            (Pos::new(15, 9), Pos::new(16, 9))
        );
        assert!(game.is_visible(game.unit(balur).unwrap()));
    }

    #[test]
    fn fire_mage_wand_immunity_and_return_rewards_follow_the_quest_notes() {
        let mut game =
            Game::from_quest(QuestDefinition::original_us_fire_mage().unwrap(), 47).unwrap();
        let hero = game.hero_order[0];
        let balur = game
            .units
            .iter()
            .find(|unit| unit.name == "Balur")
            .unwrap()
            .id;
        assert!(
            game.unit(balur)
                .unwrap()
                .is_immune_to_hero_spell(HeroSpell::BallOfFlame)
        );
        assert!(
            game.unit(balur)
                .unwrap()
                .is_immune_to_hero_spell(HeroSpell::FireOfWrath)
        );
        assert!(
            !game
                .unit(balur)
                .unwrap()
                .is_immune_to_hero_spell(HeroSpell::Genie)
        );

        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(21, 10);
        let library = game.cell(Pos::new(21, 10)).unwrap().region;
        for unit in &mut game.units {
            if unit.faction == Faction::Monster
                && game.cells[Game::cell_index(unit.pos)].region == library
            {
                unit.alive = false;
                unit.body = 0;
            }
        }
        assert_eq!(
            game.search_treasure().unwrap().discovery,
            TreasureDiscovery::QuestEvent
        );
        let inventory = &game.unit(hero).unwrap().inventory;
        assert_eq!(inventory.gold, 150);
        assert!(inventory.artifacts.contains(&Artifact::WandOfMagic));

        let balur_unit = game.units.iter_mut().find(|unit| unit.id == balur).unwrap();
        balur_unit.alive = false;
        balur_unit.body = 0;
        for (index, id) in game.hero_order.clone().into_iter().enumerate() {
            game.units
                .iter_mut()
                .find(|unit| unit.id == id)
                .unwrap()
                .pos = game.stairs[index];
        }
        game.check_terminal();
        assert_eq!(game.phase, GamePhase::Won);
        assert_eq!(game.unit(hero).unwrap().inventory.gold, 250);
        assert!(
            game.hero_order[1..]
                .iter()
                .all(|&id| game.unit(id).unwrap().inventory.gold == 100)
        );
    }

    #[test]
    fn race_against_time_starts_in_room_a_and_requires_the_remote_stairway() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_race_against_time().unwrap(),
            48,
        )
        .unwrap();
        let start_region = game.cell(Pos::new(19, 16)).unwrap().region;
        assert!(start_region > 0);
        assert!(game.hero_order.iter().all(|&hero| {
            game.unit(hero)
                .is_some_and(|unit| game.cell(unit.pos).unwrap().region == start_region)
        }));
        assert!(!game.boundary_is_open(Pos::new(17, 16), Pos::new(18, 16)));
        assert!(!game.boundary_is_open(Pos::new(20, 16), Pos::new(21, 16)));
        assert!(!game.boundary_is_open(Pos::new(19, 17), Pos::new(19, 18)));
        assert_eq!(game.phase, GamePhase::HeroTurn { order_index: 0 });

        for (index, hero) in game.hero_order.clone().into_iter().enumerate() {
            game.units
                .iter_mut()
                .find(|unit| unit.id == hero)
                .unwrap()
                .pos = game.stairs[index];
        }
        game.check_terminal();
        assert_eq!(game.phase, GamePhase::Won);
    }

    #[test]
    fn race_against_time_resolves_both_gold_chests_in_room_b() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_race_against_time().unwrap(),
            49,
        )
        .unwrap();
        for monster in game
            .units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
        {
            monster.alive = false;
            monster.body = 0;
        }
        let hero = game.hero_order[0];
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(5, 11);

        assert_eq!(
            game.search_treasure().unwrap().discovery,
            TreasureDiscovery::QuestEvent
        );
        assert_eq!(game.unit(hero).unwrap().inventory.gold, 200);
        assert!(
            game.quest_events
                .iter()
                .filter(|event| matches!(event.id.as_str(), "B1" | "B2"))
                .all(|event| event.resolved)
        );
        assert!(
            !game
                .quest_events
                .iter()
                .find(|event| event.id == "C")
                .unwrap()
                .resolved
        );
    }

    #[test]
    fn race_against_time_poison_gas_and_elixir_follow_note_c() {
        for disarmed in [false, true] {
            let mut game = Game::from_quest(
                QuestDefinition::original_us_race_against_time().unwrap(),
                50 + u64::from(disarmed),
            )
            .unwrap();
            for monster in game
                .units
                .iter_mut()
                .filter(|unit| unit.faction == Faction::Monster)
            {
                monster.alive = false;
                monster.body = 0;
            }
            game.traps
                .iter_mut()
                .find(|trap| trap.pos == Pos::new(8, 11))
                .unwrap()
                .disarmed = disarmed;
            let hero = game.hero_order[0];
            game.units
                .iter_mut()
                .find(|unit| unit.id == hero)
                .unwrap()
                .pos = Pos::new(7, 10);
            let body_before = game.unit(hero).unwrap().body;

            assert_eq!(
                game.search_treasure().unwrap().discovery,
                TreasureDiscovery::QuestEvent
            );
            let hero = game.unit(hero).unwrap();
            assert_eq!(hero.body, body_before - if disarmed { 0 } else { 3 });
            assert!(hero.inventory.artifacts.contains(&Artifact::ElixirOfLife));
            let trap = game
                .traps
                .iter()
                .find(|trap| trap.pos == Pos::new(8, 11))
                .unwrap();
            assert_eq!(trap.sprung, !disarmed);
            assert_eq!(trap.discovered, !disarmed);
            assert!(if disarmed {
                matches!(game.phase, GamePhase::HeroTurn { order_index: 0 })
            } else {
                matches!(game.phase, GamePhase::HeroTurn { order_index: 1 })
            });
        }
    }

    #[test]
    fn castle_of_mystery_magical_door_stops_movement_and_uses_two_red_dice() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_castle_of_mystery().unwrap(),
            51,
        )
        .unwrap();
        let hero = game.hero_order[0];
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(4, 14);
        game.doors
            .iter_mut()
            .find(|door| door.connects(Pos::new(4, 13), Pos::new(4, 14)))
            .unwrap()
            .open = true;
        game.hero_turn.movement_roll = Some(8);
        game.hero_turn.movement_left = 8;

        assert_eq!(game.move_active(Direction::North).unwrap(), Pos::new(4, 14));
        assert_eq!(game.hero_turn.movement_left, 0);
        assert!(game.hero_turn.door_passed);
        assert_eq!(game.pending_teleport_subject(), Some(hero));
        assert_eq!(game.resolve_teleport_roll(&[1, 2]).unwrap(), Pos::new(9, 2));
        assert_eq!(game.unit(hero).unwrap().pos, Pos::new(9, 2));
        assert!(game.pending_teleport_roll.is_none());
        assert!(game.cell(Pos::new(9, 2)).unwrap().revealed);
    }

    #[test]
    fn castle_of_mystery_collision_displaces_the_occupant_and_rerolls_same_square() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_castle_of_mystery().unwrap(),
            52,
        )
        .unwrap();
        let hero = game.hero_order[0];
        let displaced = game.hero_order[1];
        game.units
            .iter_mut()
            .find(|unit| unit.id == displaced)
            .unwrap()
            .pos = Pos::new(22, 12);
        let displaced_body = game.unit(displaced).unwrap().body;
        game.pending_teleport_roll = Some(PendingTeleportRoll {
            subject: hero,
            forbidden_destination: None,
        });

        game.resolve_teleport_roll(&[4, 6]).unwrap();
        assert_eq!(game.unit(hero).unwrap().pos, Pos::new(22, 12));
        assert_eq!(game.unit(displaced).unwrap().body, displaced_body - 1);
        assert_eq!(game.pending_teleport_subject(), Some(displaced));

        game.resolve_teleport_roll(&[4, 6]).unwrap();
        assert_eq!(game.pending_teleport_subject(), Some(displaced));
        assert_eq!(game.unit(displaced).unwrap().pos, Pos::new(22, 12));

        game.resolve_teleport_roll(&[1, 2]).unwrap();
        assert_eq!(game.unit(hero).unwrap().pos, Pos::new(22, 12));
        assert_eq!(game.unit(displaced).unwrap().pos, Pos::new(9, 2));
        assert!(game.pending_teleport_roll.is_none());
    }

    #[test]
    fn castle_of_mystery_collision_kills_a_one_body_monster_without_another_roll() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_castle_of_mystery().unwrap(),
            56,
        )
        .unwrap();
        let hero = game.hero_order[0];
        let orc = game
            .units
            .iter()
            .find(|unit| unit.figure == FigureKind::Monster(MonsterKind::Orc))
            .unwrap()
            .id;
        game.units
            .iter_mut()
            .find(|unit| unit.id == orc)
            .unwrap()
            .pos = Pos::new(22, 12);
        game.pending_teleport_roll = Some(PendingTeleportRoll {
            subject: hero,
            forbidden_destination: None,
        });
        game.resolve_teleport_roll(&[4, 6]).unwrap();
        assert!(!game.unit(orc).unwrap().alive);
        assert!(game.pending_teleport_roll.is_none());
        assert_eq!(game.unit(hero).unwrap().pos, Pos::new(22, 12));
    }

    #[test]
    fn castle_of_mystery_ring_requires_both_guardians_and_returns_visible_heroes() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_castle_of_mystery().unwrap(),
            53,
        )
        .unwrap();
        let hero = game.hero_order[0];
        let companion = game.hero_order[1];
        let ring_room = game.cell(Pos::new(23, 11)).unwrap().region;
        for unit in &mut game.units {
            if unit.faction == Faction::Monster
                && game.cells[Game::cell_index(unit.pos)].region == ring_room
            {
                unit.alive = false;
                unit.body = 0;
                break;
            }
        }
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(23, 11);
        assert_eq!(
            game.search_treasure().unwrap_err(),
            RuleError::MonstersInRoom
        );

        for unit in &mut game.units {
            if unit.name == "Ring Guardian" {
                unit.alive = false;
                unit.body = 0;
            }
        }
        assert_eq!(
            game.search_treasure().unwrap().discovery,
            TreasureDiscovery::QuestEvent
        );
        assert!(
            game.unit(hero)
                .unwrap()
                .inventory
                .artifacts
                .contains(&Artifact::RingOfReturn)
        );
        game.units
            .iter_mut()
            .find(|unit| unit.id == companion)
            .unwrap()
            .pos = Pos::new(23, 12);
        let returned = game.use_ring_of_return().unwrap();
        assert!(returned.contains(&hero));
        assert!(returned.contains(&companion));
        assert!(game.unit(hero).unwrap().escaped);
        assert!(game.unit(companion).unwrap().escaped);
        assert!(!game.can_use_ring_of_return());
    }

    #[test]
    fn castle_of_mystery_mine_gold_blocks_combat_and_vanishes_at_quest_end() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_castle_of_mystery().unwrap(),
            54,
        )
        .unwrap();
        let hero = game.hero_order[0];
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(14, 4);
        assert!(game.can_take_fools_gold());
        assert_eq!(game.take_fools_gold().unwrap(), 5_000);
        assert_eq!(game.unit(hero).unwrap().inventory.gold, 0);
        assert_eq!(game.unit(hero).unwrap().inventory.fools_gold, 5_000);
        assert_eq!(game.unit(hero).unwrap().effective_defense_dice(), 0);
        assert_eq!(
            game.active_attack_plan().unwrap_err(),
            RuleError::CarryingFoolsGold
        );
        assert_eq!(game.drop_fools_gold().unwrap(), 5_000);
        assert_eq!(game.unit(hero).unwrap().inventory.fools_gold, 0);
        game.take_fools_gold().unwrap();

        for monster in game
            .units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
        {
            monster.alive = false;
            monster.body = 0;
        }
        game.check_terminal();
        assert_eq!(game.phase, GamePhase::Won);
        assert_eq!(game.unit(hero).unwrap().inventory.fools_gold, 0);
        assert_eq!(game.unit(hero).unwrap().inventory.gold, 0);
    }

    #[test]
    fn castle_of_mystery_wandering_card_is_ollars_ghost_not_a_monster() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_castle_of_mystery().unwrap(),
            55,
        )
        .unwrap();
        game.set_treasure_replay_order([TreasureCard::WanderingMonster]);
        let unit_count = game.units.len();
        let outcome = game.search_treasure().unwrap();
        assert_eq!(
            outcome.discovery,
            TreasureDiscovery::Card(TreasureCard::WanderingMonster)
        );
        assert_eq!(outcome.wandering_monster, None);
        assert_eq!(game.units.len(), unit_count);
        assert!(
            game.log
                .back()
                .is_some_and(|message| message.contains("Ollar's ghost"))
        );
    }

    #[test]
    fn castle_of_mystery_also_ends_when_every_surviving_hero_uses_the_stairs() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_castle_of_mystery().unwrap(),
            57,
        )
        .unwrap();
        for hero in game.hero_order.clone() {
            assert!(game.escape_hero_on_stairs(hero));
        }
        game.check_terminal();
        assert_eq!(game.phase, GamePhase::Won);
        assert_eq!(game.escaped_hero_count(), 4);
        assert!(
            game.units
                .iter()
                .any(|unit| { unit.faction == Faction::Monster && unit.alive })
        );
    }

    #[test]
    fn bastion_armory_search_awards_the_only_usable_shield() {
        let mut game =
            Game::from_quest(QuestDefinition::original_us_bastion_of_chaos().unwrap(), 58).unwrap();
        let hero = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
            .for_each(|unit| unit.alive = false);
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(19, 14);

        assert_eq!(
            game.search_treasure().unwrap().discovery,
            TreasureDiscovery::QuestEvent
        );
        assert!(
            game.unit(hero)
                .unwrap()
                .inventory
                .armor
                .contains(&Armor::Shield)
        );
    }

    #[test]
    fn bastion_chest_search_animates_the_stone_gargoyle_and_queues_its_attack() {
        let mut game =
            Game::from_quest(QuestDefinition::original_us_bastion_of_chaos().unwrap(), 59).unwrap();
        let hero = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster && unit.name != "Stone Gargoyle")
            .for_each(|unit| unit.alive = false);
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(6, 11);

        game.search_treasure().unwrap();
        let gargoyle = game
            .units
            .iter()
            .find(|unit| unit.name == "Stone Gargoyle")
            .unwrap();
        let gargoyle_id = gargoyle.id;
        assert!(!gargoyle.dormant);
        assert!(!gargoyle.invulnerable_until_acts);
        assert!(
            game.traps
                .iter()
                .find(|trap| trap.pos == Pos::new(5, 10))
                .unwrap()
                .sprung
        );
        let plan = game.take_pending_forced_attack().unwrap();
        assert_eq!(plan.attacker, gargoyle_id);
        assert_eq!(plan.defender, hero);
        let body_before = game.unit(hero).unwrap().body;
        game.resolve_attack(
            plan,
            &vec![CombatFace::Skull; plan.attack_dice as usize],
            &vec![CombatFace::BlackShield; plan.defend_dice as usize],
        )
        .unwrap();
        assert!(game.unit(hero).unwrap().body < body_before);
    }

    #[test]
    fn disarming_bastion_chest_leaves_the_gargoyle_a_statue_and_does_not_block_victory() {
        let mut game =
            Game::from_quest(QuestDefinition::original_us_bastion_of_chaos().unwrap(), 60).unwrap();
        let hero = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster && unit.name != "Stone Gargoyle")
            .for_each(|unit| unit.alive = false);
        game.traps
            .iter_mut()
            .find(|trap| trap.pos == Pos::new(5, 10))
            .unwrap()
            .disarmed = true;
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(6, 11);

        game.search_treasure().unwrap();
        assert!(
            game.units
                .iter()
                .find(|unit| unit.name == "Stone Gargoyle")
                .unwrap()
                .dormant
        );
        assert!(game.take_pending_forced_attack().is_none());
        assert_eq!(game.phase, GamePhase::Won);
    }

    #[test]
    fn furniture_trap_is_disarmed_from_beside_the_impassable_chest() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_bastion_of_chaos().unwrap(),
            600,
        )
        .unwrap();
        let dwarf = game.hero_order[1];
        game.phase = GamePhase::HeroTurn { order_index: 1 };
        game.hero_turn.movement_roll = Some(6);
        game.hero_turn.movement_left = 6;
        game.units
            .iter_mut()
            .find(|unit| unit.id == dwarf)
            .unwrap()
            .pos = Pos::new(6, 10);
        let trap = game
            .traps
            .iter_mut()
            .find(|trap| trap.pos == Pos::new(5, 10))
            .unwrap();
        trap.discovered = true;
        assert!(game.is_furniture_square(Pos::new(5, 10)));

        let plan = game.active_disarm_plan().unwrap();
        assert_eq!(plan.trap, Pos::new(5, 10));
        assert!(game.resolve_disarm(plan, CombatFace::WhiteShield).unwrap());
        assert_eq!(game.unit(dwarf).unwrap().pos, Pos::new(6, 10));
        assert!(game.traps[plan.trap_index].disarmed);
        assert!(game.is_furniture_square(Pos::new(5, 10)));
    }

    #[test]
    fn bastion_bounties_and_orcs_bane_go_to_each_monsters_killer() {
        let mut game =
            Game::from_quest(QuestDefinition::original_us_bastion_of_chaos().unwrap(), 61).unwrap();
        let hero = game.active_hero_id().unwrap();
        let targets = [
            (MonsterKind::Goblin, None),
            (MonsterKind::Orc, None),
            (MonsterKind::Fimir, None),
            (MonsterKind::ChaosWarrior, Some("Orc's Bane Bearer")),
        ];
        for (kind, name) in targets {
            let target = game
                .units
                .iter()
                .find(|unit| {
                    unit.alive
                        && unit.figure == FigureKind::Monster(kind)
                        && name.is_none_or(|name| unit.name == name)
                })
                .unwrap()
                .id;
            game.units
                .iter_mut()
                .find(|unit| unit.id == hero)
                .unwrap()
                .pos = Pos::new(12, 9);
            let target_unit = game
                .units
                .iter_mut()
                .find(|unit| unit.id == target)
                .unwrap();
            target_unit.pos = Pos::new(13, 9);
            target_unit.body = 1;
            game.hero_turn.action_used = false;
            let plan = AttackPlan {
                attacker: hero,
                defender: target,
                source: AttackSource::Natural,
                attack_dice: 1,
                defend_dice: 0,
            };
            assert!(
                game.resolve_attack(plan, &[CombatFace::Skull], &[])
                    .unwrap()
                    .defender_died
            );
        }
        assert_eq!(game.unit(hero).unwrap().inventory.gold, 110);
        assert!(
            game.unit(hero)
                .unwrap()
                .inventory
                .artifacts
                .contains(&Artifact::OrcsBane)
        );

        let orcs = game
            .units
            .iter()
            .filter(|unit| unit.alive && unit.figure == FigureKind::Monster(MonsterKind::Orc))
            .take(2)
            .map(|unit| unit.id)
            .collect::<Vec<_>>();
        assert_eq!(orcs.len(), 2);
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(12, 9);
        for (orc, pos) in [(orcs[0], Pos::new(13, 9)), (orcs[1], Pos::new(12, 8))] {
            let unit = game.units.iter_mut().find(|unit| unit.id == orc).unwrap();
            unit.pos = pos;
            unit.body = 1;
            unit.dormant = false;
            unit.hidden_until_activated = false;
            game.cells[Game::cell_index(pos)].revealed = true;
        }
        game.allocate_pending_visible_figures();
        game.hero_turn = HeroTurnState::default();

        let first = game.active_attack_plan().unwrap();
        assert_eq!(first.attack_dice, 2);
        game.resolve_attack(
            first,
            &[CombatFace::Skull, CombatFace::Skull],
            &vec![CombatFace::WhiteShield; first.defend_dice as usize],
        )
        .unwrap();
        assert!(game.hero_turn.action_used);
        assert!(game.hero_turn.orcs_bane_follow_up);
        let second = game.active_attack_plan().unwrap();
        assert_ne!(first.defender, second.defender);
        assert_eq!(second.attack_dice, 2);
        game.resolve_attack(
            second,
            &[CombatFace::Skull, CombatFace::Skull],
            &vec![CombatFace::WhiteShield; second.defend_dice as usize],
        )
        .unwrap();
        assert!(!game.hero_turn.orcs_bane_follow_up);
        assert_eq!(
            game.active_attack_plan().unwrap_err(),
            RuleError::AlreadyActed
        );
    }

    #[test]
    fn barak_tor_false_doors_can_never_be_opened() {
        let mut game =
            Game::from_quest(QuestDefinition::original_us_barak_tor().unwrap(), 62).unwrap();
        let hero = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(18, 0);
        assert_eq!(game.open_adjacent_door().unwrap_err(), RuleError::FalseDoor);
        let door = game.has_door(Pos::new(18, 0), Pos::new(18, 1)).unwrap();
        assert!(door.false_door);
        assert!(!door.open);
    }

    #[test]
    fn barak_tor_star_starts_on_its_zombie_drops_and_returns_for_200_gold() {
        let mut game =
            Game::from_quest(QuestDefinition::original_us_barak_tor().unwrap(), 63).unwrap();
        let bearer = game
            .units
            .iter()
            .find(|unit| unit.name == "Star Bearer")
            .unwrap()
            .id;
        let item_index = game.unit(bearer).unwrap().carried_quest_item.unwrap();
        assert_eq!(game.quest_items[item_index].id, "Star of the West");
        assert_eq!(game.quest_items[item_index].holder, Some(bearer));
        assert_eq!(
            game.props[game.quest_items[item_index].prop_index].carried_by,
            Some(bearer)
        );

        let drop_pos = game.unit(bearer).unwrap().pos;
        game.damage_without_defense(bearer, 1);
        assert_eq!(game.quest_items[item_index].holder, None);
        let prop_index = game.quest_items[item_index].prop_index;
        assert_eq!(game.props[prop_index].pos, drop_pos);
        assert!(game.props[prop_index].visible);

        let hero = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = drop_pos;
        assert_eq!(game.take_quest_item().unwrap(), "Star of the West");
        for (index, hero_id) in game.hero_order.clone().into_iter().enumerate() {
            let stair = game.stairs[index];
            game.units
                .iter_mut()
                .find(|unit| unit.id == hero_id)
                .unwrap()
                .pos = stair;
        }
        game.deliver_carried_quest_item(hero);
        game.check_terminal();
        assert_eq!(game.phase, GamePhase::Won);
        assert!(game.quest_items[item_index].delivered);
        assert!(
            game.hero_order
                .iter()
                .all(|&id| game.unit(id).unwrap().inventory.gold == 50)
        );
    }

    #[test]
    fn barak_tor_block_falls_only_after_the_last_living_hero_passes() {
        let mut game =
            Game::from_quest(QuestDefinition::original_us_barak_tor().unwrap(), 64).unwrap();
        let block = Pos::new(0, 0);
        let exit = Pos::new(1, 0);
        for hero in game.hero_order.clone().into_iter().take(3) {
            game.collapse_delayed_block_after_last_hero(hero, block, exit);
            assert!(game.cell(block).unwrap().passable);
        }
        let last = game.hero_order[3];
        game.collapse_delayed_block_after_last_hero(last, block, exit);
        assert!(!game.cell(block).unwrap().passable);
        let trap = game.traps.iter().find(|trap| trap.pos == block).unwrap();
        assert!(trap.sprung);
        assert!(!trap.disarmable);
        assert!(
            game.hero_order
                .iter()
                .all(|&id| game.unit(id).unwrap().body == game.unit(id).unwrap().stats.body as i16)
        );
    }

    #[test]
    fn entering_the_tomb_releases_the_spirit_blade_immune_witch_lord() {
        let mut game =
            Game::from_quest(QuestDefinition::original_us_barak_tor().unwrap(), 65).unwrap();
        let witch = game
            .units
            .iter()
            .find(|unit| unit.name == "Witch Lord")
            .unwrap()
            .id;
        assert!(game.unit(witch).unwrap().dormant);
        assert!(game.unit(witch).unwrap().hidden_until_activated);
        assert!(!game.is_visible(game.unit(witch).unwrap()));

        game.reveal_from(Pos::new(2, 11));
        let witch_lord = game.unit(witch).unwrap();
        assert!(!witch_lord.dormant);
        assert!(!witch_lord.hidden_until_activated);
        assert!(witch_lord.immune_except_spirit_blade);
        assert_eq!(witch_lord.stats.movement, 1);
        assert_eq!(witch_lord.stats.attack, 2);

        let hero = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(2, 10);
        let plan = game.active_attack_plan().unwrap();
        assert_eq!(plan.defender, witch);
        let outcome = game
            .resolve_attack(
                plan,
                &vec![CombatFace::Skull; plan.attack_dice as usize],
                &vec![CombatFace::WhiteShield; plan.defend_dice as usize],
            )
            .unwrap();
        assert_eq!(outcome.damage, 0);
        assert!(game.unit(witch).unwrap().alive);

        game.hero_turn.action_used = false;
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .inventory
            .artifacts
            .push(Artifact::SpiritBlade);
        let plan = game.active_attack_plan().unwrap();
        let outcome = game
            .resolve_attack(
                plan,
                &vec![CombatFace::Skull; plan.attack_dice as usize],
                &vec![CombatFace::WhiteShield; plan.defend_dice as usize],
            )
            .unwrap();
        assert!(outcome.defender_died);
    }

    #[test]
    fn barak_tor_hidden_bookcase_awards_the_wizards_staff_and_its_diagonal_attack() {
        let mut game =
            Game::from_quest(QuestDefinition::original_us_barak_tor().unwrap(), 66).unwrap();
        let wizard = game.hero_order[3];
        game.phase = GamePhase::HeroTurn { order_index: 3 };
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
            .for_each(|unit| unit.alive = false);
        game.units
            .iter_mut()
            .find(|unit| unit.id == wizard)
            .unwrap()
            .pos = Pos::new(5, 17);
        game.search_treasure().unwrap();
        assert!(
            game.unit(wizard)
                .unwrap()
                .inventory
                .artifacts
                .contains(&Artifact::WizardsStaff)
        );

        let target = game
            .units
            .iter_mut()
            .find(|unit| unit.faction == Faction::Monster)
            .unwrap();
        target.alive = true;
        target.body = 1;
        target.pos = Pos::new(6, 16);
        target.dormant = false;
        target.hidden_until_activated = false;
        let target_id = target.id;
        game.cells[Game::cell_index(Pos::new(6, 16))].revealed = true;
        game.allocate_pending_visible_figures();
        game.hero_turn = HeroTurnState::default();
        let plan = game.active_attack_plan().unwrap();
        assert_eq!(plan.defender, target_id);
        assert_eq!(plan.attack_dice, 2);
    }

    #[test]
    fn spirit_blade_quest_ceiling_squares_stay_open_and_retrigger_with_physical_red_results() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_quest_for_the_spirit_blade().unwrap(),
            67,
        )
        .unwrap();
        let hero = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(13, 14);
        game.apply_movement_roll(&[6, 6]).unwrap();
        assert_eq!(game.move_active(Direction::West).unwrap(), Pos::new(12, 14));
        assert_eq!(game.pending_collapsing_ceiling_subject(), Some(hero));
        assert!(game.cell(Pos::new(12, 14)).unwrap().passable);
        let starting_body = game.unit(hero).unwrap().body;
        assert!(game.resolve_collapsing_ceiling_roll(hero, 4).unwrap());
        assert_eq!(game.unit(hero).unwrap().body, starting_body - 1);
        assert!(game.cell(Pos::new(12, 14)).unwrap().passable);

        assert_eq!(game.move_active(Direction::East).unwrap(), Pos::new(13, 14));
        assert_eq!(game.move_active(Direction::West).unwrap(), Pos::new(12, 14));
        assert_eq!(game.pending_collapsing_ceiling_subject(), Some(hero));
        assert!(!game.resolve_collapsing_ceiling_roll(hero, 3).unwrap());
        assert_eq!(game.unit(hero).unwrap().body, starting_body - 1);
    }

    #[test]
    fn spirit_blade_quest_helmet_changes_ceiling_damage_to_only_a_six() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_quest_for_the_spirit_blade().unwrap(),
            68,
        )
        .unwrap();
        let hero = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .inventory
            .armor
            .push(Armor::Helmet);
        let starting_body = game.unit(hero).unwrap().body;
        game.trigger_collapsing_ceiling_hazard(hero, Pos::new(0, 9));
        assert!(!game.resolve_collapsing_ceiling_roll(hero, 4).unwrap());
        assert_eq!(game.unit(hero).unwrap().body, starting_body);
        game.trigger_collapsing_ceiling_hazard(hero, Pos::new(19, 9));
        assert!(game.resolve_collapsing_ceiling_roll(hero, 6).unwrap());
        assert_eq!(game.unit(hero).unwrap().body, starting_body - 1);
    }

    #[test]
    fn spirit_blade_searches_award_the_artifact_and_chest_gold_then_allow_return() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_quest_for_the_spirit_blade().unwrap(),
            69,
        )
        .unwrap();
        let hero = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
            .for_each(|unit| unit.alive = false);

        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(23, 15);
        game.search_treasure().unwrap();
        assert!(
            game.unit(hero)
                .unwrap()
                .inventory
                .artifacts
                .contains(&Artifact::SpiritBlade)
        );

        game.hero_turn.action_used = false;
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(6, 12);
        game.search_treasure().unwrap();
        assert_eq!(game.unit(hero).unwrap().inventory.gold, 200);

        for (index, hero_id) in game.hero_order.clone().into_iter().enumerate() {
            let stair = game.stairs[index];
            game.units
                .iter_mut()
                .find(|unit| unit.id == hero_id)
                .unwrap()
                .pos = stair;
        }
        game.check_terminal();
        assert_eq!(game.phase, GamePhase::Won);
    }

    #[test]
    fn spirit_blade_rolls_four_dice_against_undead_three_otherwise_and_excludes_wizard() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_quest_for_the_spirit_blade().unwrap(),
            70,
        )
        .unwrap();
        let barbarian = game.hero_order[0];
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
            .for_each(|unit| unit.alive = false);
        game.units
            .iter_mut()
            .find(|unit| unit.id == barbarian)
            .unwrap()
            .inventory
            .artifacts
            .push(Artifact::SpiritBlade);
        let target = game
            .units
            .iter_mut()
            .find(|unit| unit.figure == FigureKind::Monster(MonsterKind::Skeleton))
            .unwrap();
        target.alive = true;
        target.body = 1;
        target.pos = Pos::new(13, 7);
        target.dormant = false;
        target.hidden_until_activated = false;
        let target_id = target.id;
        game.units
            .iter_mut()
            .find(|unit| unit.id == barbarian)
            .unwrap()
            .pos = Pos::new(14, 7);
        game.cells[Game::cell_index(Pos::new(13, 7))].revealed = true;
        game.allocate_pending_visible_figures();
        assert_eq!(game.active_attack_plan().unwrap().attack_dice, 4);

        game.units
            .iter_mut()
            .find(|unit| unit.id == target_id)
            .unwrap()
            .figure = FigureKind::Monster(MonsterKind::Fimir);
        assert_eq!(game.active_attack_plan().unwrap().attack_dice, 3);

        let wizard = game.hero_order[3];
        game.phase = GamePhase::HeroTurn { order_index: 3 };
        game.units
            .iter_mut()
            .find(|unit| unit.id == wizard)
            .unwrap()
            .inventory
            .artifacts
            .push(Artifact::SpiritBlade);
        game.units
            .iter_mut()
            .find(|unit| unit.id == wizard)
            .unwrap()
            .pos = Pos::new(14, 7);
        assert_eq!(game.active_attack_plan().unwrap().attack_dice, 1);
    }

    #[test]
    fn final_witch_lord_requires_spirit_blade_drops_spell_ring_and_makes_survivors_champions() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_return_to_barak_tor().unwrap(),
            71,
        )
        .unwrap();
        let barbarian = game.hero_order[0];
        let witch = game
            .units
            .iter()
            .find(|unit| unit.name == "Witch Lord")
            .unwrap()
            .id;
        assert!(
            game.unit(barbarian)
                .unwrap()
                .inventory
                .artifacts
                .contains(&Artifact::SpiritBlade)
        );
        assert_eq!(game.unit(witch).unwrap().stats.movement, 10);
        assert_eq!(game.unit(witch).unwrap().stats.attack, 5);
        assert_eq!(game.unit(witch).unwrap().stats.defend, 6);
        assert_eq!(game.unit(witch).unwrap().body, 4);
        assert_eq!(game.unit(witch).unwrap().stats.mind, 6);

        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster && unit.id != witch)
            .for_each(|unit| unit.alive = false);
        game.units
            .iter_mut()
            .find(|unit| unit.id == barbarian)
            .unwrap()
            .pos = Pos::new(22, 3);
        game.units
            .iter_mut()
            .find(|unit| unit.id == witch)
            .map(|unit| {
                unit.pos = Pos::new(23, 3);
                unit.dormant = false;
                unit.hidden_until_activated = false;
            })
            .unwrap();
        game.cells[Game::cell_index(Pos::new(23, 3))].revealed = true;
        game.allocate_pending_visible_figures();
        let first = game.active_attack_plan().unwrap();
        assert_eq!(first.attack_dice, 3);
        let outcome = game
            .resolve_attack(
                first,
                &vec![CombatFace::Skull; first.attack_dice as usize],
                &vec![CombatFace::WhiteShield; first.defend_dice as usize],
            )
            .unwrap();
        assert_eq!(outcome.damage, 3);
        assert!(!outcome.defender_died);

        game.hero_turn.action_used = false;
        let second = game.active_attack_plan().unwrap();
        let outcome = game
            .resolve_attack(
                second,
                &vec![CombatFace::Skull; second.attack_dice as usize],
                &vec![CombatFace::WhiteShield; second.defend_dice as usize],
            )
            .unwrap();
        assert!(outcome.defender_died);
        assert_eq!(game.phase, GamePhase::Won);
        assert!(
            game.unit(barbarian)
                .unwrap()
                .inventory
                .artifacts
                .contains(&Artifact::SpellRing)
        );
        assert!(
            game.hero_order
                .iter()
                .all(|&id| !game.unit(id).unwrap().alive || game.unit(id).unwrap().champion)
        );
        assert!(
            game.log
                .iter()
                .any(|line| line.contains("foul black smoke"))
        );
    }

    #[test]
    fn zero_body_interrupts_play_and_uses_the_heroes_physical_healing_die() {
        let mut game = Game::demo(81).unwrap();
        let hero = game.active_hero_id().unwrap();
        let unit = game.units.iter_mut().find(|unit| unit.id == hero).unwrap();
        unit.body = 1;
        unit.inventory.potion_of_healing = 1;
        unit.inventory.healing_potion_strengths.push(0);

        game.damage_without_defense(hero, 1);
        assert_eq!(game.unit(hero).unwrap().body, 0);
        assert!(game.unit(hero).unwrap().alive);
        assert_eq!(game.pending_hero_death.unwrap().hero, hero);
        assert!(
            game.pending_hero_death_choices()
                .contains(&HeroDeathChoice::HealingPotion)
        );

        assert_eq!(
            game.choose_pending_hero_death(HeroDeathChoice::HealingPotion)
                .unwrap(),
            Some(HealingPotionUse::RollRedDie { hero })
        );
        assert!(game.pending_hero_death.unwrap().potion_roll_pending);
        assert_eq!(game.resolve_healing_potion_roll(hero, 3).unwrap(), 3);
        assert_eq!(game.unit(hero).unwrap().body, 3);
        assert!(game.unit(hero).unwrap().alive);
        assert!(game.pending_hero_death.is_none());
    }

    #[test]
    fn heroic_brew_and_strength_potion_modify_exactly_the_next_attack_action() {
        let mut game = Game::demo(86).unwrap();
        game.cells.iter_mut().for_each(|cell| cell.revealed = true);
        let hero = game.active_hero_id().unwrap();
        let unit = game.units.iter_mut().find(|unit| unit.id == hero).unwrap();
        unit.pos = Pos::new(10, 10);
        unit.inventory.heroic_brew = 1;
        unit.inventory.potion_of_strength = 1;
        let monsters = game
            .units
            .iter()
            .filter(|unit| unit.faction == Faction::Monster)
            .take(2)
            .map(|unit| unit.id)
            .collect::<Vec<_>>();
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
            .for_each(|unit| unit.alive = monsters.contains(&unit.id));
        for (monster, pos) in [
            (monsters[0], Pos::new(11, 10)),
            (monsters[1], Pos::new(10, 11)),
        ] {
            let unit = game
                .units
                .iter_mut()
                .find(|unit| unit.id == monster)
                .unwrap();
            unit.pos = pos;
            unit.body = 9;
            unit.dormant = false;
            unit.hidden_until_activated = false;
        }
        game.allocate_pending_visible_figures();

        game.drink_heroic_brew().unwrap();
        game.drink_potion_of_strength().unwrap();
        let first = game.active_attack_plan().unwrap();
        assert_eq!(first.attack_dice, 5);
        game.resolve_attack(
            first,
            &vec![CombatFace::WhiteShield; first.attack_dice as usize],
            &vec![CombatFace::WhiteShield; first.defend_dice as usize],
        )
        .unwrap();
        assert!(game.hero_turn.heroic_brew_follow_up);
        assert_eq!(game.hero_turn.potion_strength_bonus, 0);

        let second = game.active_attack_plan().unwrap();
        assert_eq!(second.attack_dice, 3);
        game.resolve_attack(
            second,
            &vec![CombatFace::WhiteShield; second.attack_dice as usize],
            &vec![CombatFace::WhiteShield; second.defend_dice as usize],
        )
        .unwrap();
        assert!(!game.hero_turn.heroic_brew_follow_up);
        assert_eq!(
            game.active_attack_plan().unwrap_err(),
            RuleError::AlreadyActed
        );
    }

    #[test]
    fn defense_potion_refreshes_the_staged_plan_and_expires_after_that_defense() {
        let mut game = Game::demo(87).unwrap();
        let hero = game.active_hero_id().unwrap();
        let monster = game
            .units
            .iter()
            .find(|unit| unit.faction == Faction::Monster)
            .unwrap()
            .id;
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(10, 10);
        let hero_unit = game.units.iter_mut().find(|unit| unit.id == hero).unwrap();
        hero_unit.inventory.potion_of_defense = 1;
        let monster_unit = game
            .units
            .iter_mut()
            .find(|unit| unit.id == monster)
            .unwrap();
        monster_unit.pos = Pos::new(11, 10);
        monster_unit.dormant = false;
        monster_unit.hidden_until_activated = false;
        let base_defense = game.unit(hero).unwrap().effective_defense_dice();
        let stale = AttackPlan {
            attacker: monster,
            defender: hero,
            source: AttackSource::Natural,
            attack_dice: 1,
            defend_dice: base_defense,
        };

        game.drink_potion_of_defense_for(hero).unwrap();
        let refreshed = game.refresh_attack_defense_dice(stale).unwrap();
        assert_eq!(refreshed.defend_dice, base_defense + 2);
        game.resolve_attack(
            refreshed,
            &[CombatFace::WhiteShield],
            &vec![CombatFace::WhiteShield; refreshed.defend_dice as usize],
        )
        .unwrap();
        assert_eq!(game.unit(hero).unwrap().potion_defense_bonus, 0);
        assert_eq!(
            game.unit(hero).unwrap().effective_defense_dice(),
            base_defense
        );
    }

    #[test]
    fn active_hero_gives_an_exact_potion_card_and_gold_amount_to_another_hero() {
        let mut game = Game::demo(88).unwrap();
        let giver = game.active_hero_id().unwrap();
        let recipient = game.hero_order[1];
        let giver_inventory = &mut game
            .units
            .iter_mut()
            .find(|unit| unit.id == giver)
            .unwrap()
            .inventory;
        giver_inventory.potion_of_healing = 1;
        giver_inventory.healing_potion_strengths.push(2);
        giver_inventory.gold = 275;

        game.give_active_potion(recipient, PotionKind::Healing)
            .unwrap();
        game.give_active_gold(recipient, 125).unwrap();

        let giver_inventory = &game.unit(giver).unwrap().inventory;
        assert_eq!(giver_inventory.potion_of_healing, 0);
        assert_eq!(giver_inventory.gold, 150);
        let recipient_inventory = &game.unit(recipient).unwrap().inventory;
        assert_eq!(recipient_inventory.potion_of_healing, 1);
        assert_eq!(recipient_inventory.healing_potion_strengths, vec![2]);
        assert_eq!(recipient_inventory.gold, 125);

        game.phase = GamePhase::ZargonTurn;
        assert_eq!(
            game.give_active_gold(recipient, 1).unwrap_err(),
            RuleError::NotHeroTurn
        );
    }

    #[test]
    fn unused_self_healing_spell_saves_a_spellcaster_but_an_used_action_does_not() {
        let mut saved = Game::demo(82).unwrap();
        let wizard = saved.hero_order[3];
        saved.phase = GamePhase::HeroTurn { order_index: 3 };
        let unit = saved
            .units
            .iter_mut()
            .find(|unit| unit.id == wizard)
            .unwrap();
        unit.body = 1;
        unit.hero_spells = vec![HeroSpell::HealBody];
        unit.spellcasting_available = true;
        saved.damage_without_defense(wizard, 1);
        assert!(
            saved
                .pending_hero_death_choices()
                .contains(&HeroDeathChoice::HealBody)
        );
        saved
            .choose_pending_hero_death(HeroDeathChoice::HealBody)
            .unwrap();
        assert_eq!(saved.unit(wizard).unwrap().body, 4);
        assert!(saved.unit(wizard).unwrap().hero_spells.is_empty());
        assert!(saved.hero_turn.action_used);

        let mut acted = Game::demo(83).unwrap();
        let wizard = acted.hero_order[3];
        acted.phase = GamePhase::HeroTurn { order_index: 3 };
        acted.hero_turn.action_used = true;
        let unit = acted
            .units
            .iter_mut()
            .find(|unit| unit.id == wizard)
            .unwrap();
        unit.body = 1;
        unit.hero_spells = vec![HeroSpell::WaterOfHealing];
        unit.spellcasting_available = true;
        acted.damage_without_defense(wizard, 1);
        assert!(!acted.unit(wizard).unwrap().alive);
        assert!(acted.pending_hero_death.is_none());
    }

    #[test]
    fn present_hero_receives_every_transferable_possession_from_the_fallen() {
        let mut game = Game::demo(84).unwrap();
        let fallen = game.hero_order[0];
        let recipient = game.hero_order[1];
        let fallen_pos = game.unit(fallen).unwrap().pos;
        let recipient_pos = fallen_pos.offset(Direction::South).unwrap();
        game.cells[Game::cell_index(recipient_pos)].passable = true;
        let region = game.cell(fallen_pos).unwrap().region;
        game.cells[Game::cell_index(recipient_pos)].region = region;
        game.units
            .iter_mut()
            .find(|unit| unit.id == recipient)
            .unwrap()
            .pos = recipient_pos;
        let unit = game
            .units
            .iter_mut()
            .find(|unit| unit.id == fallen)
            .unwrap();
        unit.body = 1;
        unit.inventory.gold = 123;
        unit.inventory.potion_of_strength = 1;
        unit.inventory.artifacts.push(Artifact::TalismanOfLore);
        unit.inventory.weapons.push(Weapon::Longsword);

        game.damage_without_defense(fallen, 1);
        let pickup = game.pending_possession_pickup.clone().unwrap();
        assert!(pickup.eligible_heroes.contains(&recipient));
        game.choose_possession_recipient(recipient).unwrap();

        let inventory = &game.unit(recipient).unwrap().inventory;
        assert_eq!(inventory.gold, 123);
        assert_eq!(inventory.potion_of_strength, 1);
        assert!(inventory.artifacts.contains(&Artifact::TalismanOfLore));
        assert!(inventory.weapons.contains(&Weapon::Longsword));
        assert_eq!(game.unit(fallen).unwrap().inventory, Inventory::default());
    }

    #[test]
    fn monster_claims_and_removes_possessions_when_no_other_hero_is_present() {
        let mut game = Game::demo(85).unwrap();
        let fallen = game.hero_order[0];
        let pos = game.unit(fallen).unwrap().pos;
        for &hero in &game.hero_order[1..] {
            game.units
                .iter_mut()
                .find(|unit| unit.id == hero)
                .unwrap()
                .escaped = true;
        }
        let monster = game
            .units
            .iter_mut()
            .find(|unit| unit.faction == Faction::Monster)
            .unwrap();
        monster.pos = pos;
        monster.dormant = false;
        monster.hidden_until_activated = false;
        let unit = game
            .units
            .iter_mut()
            .find(|unit| unit.id == fallen)
            .unwrap();
        unit.body = 1;
        unit.inventory.gold = 400;
        unit.inventory.armor.push(Armor::Helmet);
        unit.inventory.artifacts.push(Artifact::SpiritBlade);

        game.damage_without_defense(fallen, 1);

        assert!(!game.unit(fallen).unwrap().alive);
        assert_eq!(game.unit(fallen).unwrap().inventory, Inventory::default());
        assert!(game.pending_possession_pickup.is_none());
        assert_eq!(game.monster_stolen_artifacts, [Artifact::SpiritBlade]);
        assert!(
            game.log
                .iter()
                .any(|line| line.contains("claimed the fallen Hero's possessions"))
        );
    }

    #[test]
    fn return_to_barak_tor_tomb_a_is_empty_and_wandering_monster_is_mummy() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_return_to_barak_tor().unwrap(),
            72,
        )
        .unwrap();
        let hero = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster)
            .for_each(|unit| unit.alive = false);
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(3, 10);
        let outcome = game.search_treasure().unwrap();
        assert_eq!(outcome.discovery, TreasureDiscovery::Empty);
        assert_eq!(game.wandering_monster, MonsterKind::Mummy);
        assert!(
            game.log
                .iter()
                .any(|line| line.contains("former tomb is now empty"))
        );
    }

    #[test]
    fn summon_undead_physical_die_creates_each_exact_card_group() {
        for (face, expected) in [
            (
                1,
                std::collections::HashMap::from([(MonsterKind::Skeleton, 4)]),
            ),
            (
                3,
                std::collections::HashMap::from([
                    (MonsterKind::Skeleton, 3),
                    (MonsterKind::Zombie, 2),
                ]),
            ),
            (
                6,
                std::collections::HashMap::from([
                    (MonsterKind::Zombie, 2),
                    (MonsterKind::Mummy, 2),
                ]),
            ),
        ] {
            let mut game = Game::from_quest(
                QuestDefinition::original_us_return_to_barak_tor().unwrap(),
                73 + u64::from(face),
            )
            .unwrap();
            let witch = game
                .units
                .iter()
                .find(|unit| unit.name == "Witch Lord")
                .unwrap()
                .id;
            let before = [
                MonsterKind::Skeleton,
                MonsterKind::Zombie,
                MonsterKind::Mummy,
            ]
            .into_iter()
            .map(|kind| {
                (
                    kind,
                    game.units
                        .iter()
                        .filter(|unit| unit.alive && unit.figure == FigureKind::Monster(kind))
                        .count(),
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
            let step = game.try_cast_chaos_spell(witch).unwrap();
            assert!(matches!(
                step,
                ZargonStep::Cast {
                    caster,
                    target,
                    spell: ChaosSpell::SummonUndead,
                    resistance_dice: 1
                } if caster == witch && target == witch
            ));
            game.resolve_chaos_spell_resistance(witch, ChaosSpell::SummonUndead, &[face])
                .unwrap();
            for kind in [
                MonsterKind::Skeleton,
                MonsterKind::Zombie,
                MonsterKind::Mummy,
            ] {
                let after = game
                    .units
                    .iter()
                    .filter(|unit| unit.alive && unit.figure == FigureKind::Monster(kind))
                    .count();
                assert_eq!(
                    after - before[&kind],
                    expected.get(&kind).copied().unwrap_or(0)
                );
            }
        }
    }

    #[test]
    fn every_official_quest_placement_respects_the_finite_box_figure_supply() {
        let mut quests_requiring_visible_overflow = 0;
        for quest_index in 0..QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS {
            let mut game = Game::from_quest(
                QuestDefinition::original_us_game_system(quest_index).unwrap(),
                0x5048_5953_4943_414c + quest_index as u64,
            )
            .unwrap();
            game.cells
                .iter_mut()
                .filter(|cell| cell.passable)
                .for_each(|cell| cell.revealed = true);
            game.units
                .iter_mut()
                .filter(|unit| matches!(unit.figure, FigureKind::Monster(_)))
                .for_each(|unit| unit.hidden_until_activated = false);
            game.allocate_pending_visible_figures();

            for kind in [
                MonsterKind::Goblin,
                MonsterKind::Orc,
                MonsterKind::Fimir,
                MonsterKind::Skeleton,
                MonsterKind::Zombie,
                MonsterKind::Mummy,
                MonsterKind::ChaosWarrior,
                MonsterKind::Gargoyle,
                MonsterKind::ChaosSorcerer,
            ] {
                assert!(
                    game.assigned_physical_count(kind) <= original_us_monster_figure_count(kind),
                    "quest {} exceeded the {:?} figure supply",
                    quest_index + 1,
                    kind
                );
            }
            for unit in game.units.iter().filter(|unit| {
                unit.alive
                    && matches!(unit.figure, FigureKind::Monster(_))
                    && unit.physical_figure.is_none()
            }) {
                assert!(
                    game.is_visible(unit),
                    "quest {} hid logical monster {} when the reusable box supply was exhausted",
                    quest_index + 1,
                    unit.name
                );
                let FigureKind::Monster(kind) = unit.figure else {
                    unreachable!()
                };
                let group = same_color_monster_figures(kind);
                let assigned = group
                    .iter()
                    .map(|&kind| game.assigned_physical_count(kind))
                    .sum::<usize>();
                let capacity = group
                    .iter()
                    .map(|&kind| original_us_monster_figure_count(kind))
                    .sum::<usize>();
                assert_eq!(
                    assigned,
                    capacity,
                    "quest {} left a logical {:?} unplaced before its color group was exhausted",
                    quest_index + 1,
                    kind
                );
            }
            if game.units.iter().any(|unit| {
                unit.alive
                    && matches!(unit.figure, FigureKind::Monster(_))
                    && unit.physical_figure.is_none()
            }) {
                quests_requiring_visible_overflow += 1;
            }

            if let Some(ulag) = game.units.iter().find(|unit| unit.name == "Ulag") {
                assert_eq!(
                    ulag.physical_figure,
                    Some(FigureKind::Monster(MonsterKind::Orc))
                );
                assert_eq!(
                    ulag.model_variant,
                    Some(MonsterModelVariant::OrcNotchedSword)
                );
            }
            if let Some(grak) = game.units.iter().find(|unit| unit.name == "Grak") {
                assert_eq!(
                    grak.physical_figure,
                    Some(FigureKind::Monster(MonsterKind::Orc))
                );
                assert_eq!(grak.model_variant, Some(MonsterModelVariant::OrcStaff));
            }
        }
        assert!(
            quests_requiring_visible_overflow > 0,
            "the audit must exercise at least one official quest whose total reveal exceeds a color group"
        );
    }

    #[test]
    fn runtime_spawns_reuse_a_killed_piece_before_allocating_another() {
        let mut game = Game::demo(0x5245_5553_4544).unwrap();
        game.cells
            .iter_mut()
            .filter(|cell| cell.passable)
            .for_each(|cell| cell.revealed = true);
        game.allocate_pending_visible_figures();
        let fallen = game
            .units
            .iter()
            .find(|unit| {
                unit.alive && unit.physical_figure == Some(FigureKind::Monster(MonsterKind::Orc))
            })
            .map(|unit| unit.id)
            .unwrap();
        let variant = game.unit(fallen).unwrap().model_variant;
        game.units
            .iter_mut()
            .find(|unit| unit.id == fallen)
            .unwrap()
            .alive = false;
        let spawn_pos = game
            .cells
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.passable)
            .map(|(index, _)| Game::pos_from_cell_index(index))
            .find(|&pos| game.occupied_by_alive(pos, None).is_none())
            .unwrap();

        let spawned = game.spawn_monster_at(MonsterKind::Orc, spawn_pos).unwrap();

        assert!(game.unit(fallen).unwrap().physical_figure.is_none());
        assert_eq!(
            game.unit(spawned).unwrap().physical_figure,
            Some(FigureKind::Monster(MonsterKind::Orc))
        );
        assert_eq!(game.unit(spawned).unwrap().model_variant, variant);
    }

    #[test]
    fn exhausted_orcs_use_a_green_substitute_without_changing_orc_rules() {
        let mut game = Game::demo(0x5355_4253_5449_5455).unwrap();
        for unit in game
            .units
            .iter_mut()
            .filter(|unit| matches!(unit.figure, FigureKind::Monster(_)))
        {
            unit.alive = false;
            unit.physical_figure = None;
            unit.model_variant = None;
        }
        let positions = game
            .cells
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.passable)
            .map(|(index, _)| Game::pos_from_cell_index(index))
            .filter(|&pos| game.occupied_by_alive(pos, None).is_none())
            .take(9)
            .collect::<Vec<_>>();
        assert_eq!(positions.len(), 9);
        let spawned = positions
            .into_iter()
            .map(|pos| game.spawn_monster_at(MonsterKind::Orc, pos).unwrap())
            .collect::<Vec<_>>();

        assert!(spawned[..8].iter().all(|&id| {
            game.unit(id).unwrap().physical_figure == Some(FigureKind::Monster(MonsterKind::Orc))
        }));
        let substitute = game.unit(spawned[8]).unwrap();
        assert_eq!(substitute.figure, FigureKind::Monster(MonsterKind::Orc));
        assert_eq!(substitute.stats, monster_stats(MonsterKind::Orc));
        assert_eq!(
            substitute.physical_figure,
            Some(FigureKind::Monster(MonsterKind::Goblin))
        );
        assert!(matches!(
            substitute.model_variant,
            Some(
                MonsterModelVariant::GoblinSword
                    | MonsterModelVariant::GoblinAxe
                    | MonsterModelVariant::GoblinScimitar
            )
        ));
    }

    #[test]
    fn original_furniture_footprints_match_the_scanned_quest_symbols() {
        assert_eq!(PropKind::Stairs.footprint(0), (2, 2));
        assert_eq!(PropKind::Table.footprint(0), (3, 2));
        assert_eq!(PropKind::Table.footprint(1), (2, 3));
        assert_eq!(PropKind::AlchemistsBench.footprint(3), (2, 3));
        assert_eq!(PropKind::Tomb.footprint(0), (2, 3));
        assert_eq!(PropKind::Tomb.footprint(1), (3, 2));
        assert_eq!(PropKind::TortureRack.footprint(2), (2, 3));
        assert_eq!(PropKind::Bookcase.footprint(0), (3, 1));
        assert_eq!(PropKind::WeaponRack.footprint(1), (1, 3));
        assert_eq!(PropKind::Fireplace.footprint(0), (2, 1));
        assert_eq!(PropKind::Cupboard.footprint(3), (1, 3));
        assert_eq!(PropKind::Chest.footprint(2), (1, 1));
        assert_eq!(PropKind::Throne.footprint(3), (1, 1));
        assert!(!PropKind::Stairs.blocks_movement());
        assert!(!PropKind::StarOfWest.blocks_movement());
        assert!(PropKind::Table.blocks_movement());
    }

    #[test]
    fn every_official_quest_has_valid_nonoverlapping_furniture_and_exact_stairs() {
        for quest_index in 0..QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS {
            let game = Game::from_quest(
                QuestDefinition::original_us_game_system(quest_index).unwrap(),
                0x4655_524e_4954_5552 + quest_index as u64,
            )
            .unwrap();
            let mut furniture = HashSet::new();
            for prop in game.props.iter().filter(|prop| {
                prop.visible && prop.carried_by.is_none() && prop.kind.blocks_movement()
            }) {
                for square in prop.footprint_squares() {
                    assert!(Game::in_bounds(square));
                    assert!(game.cell(square).unwrap().passable);
                    assert!(
                        furniture.insert(square),
                        "quest {} overlaps furniture at {square:?}",
                        quest_index + 1
                    );
                    assert!(
                        game.occupied_by_alive(square, None).is_none(),
                        "quest {} places a figure on furniture at {square:?}",
                        quest_index + 1
                    );
                }
            }
            let printed_stairs = game.stairs.iter().copied().collect::<HashSet<_>>();
            let rendered_stairs = game
                .props
                .iter()
                .filter(|prop| prop.kind == PropKind::Stairs)
                .flat_map(Prop::footprint_squares)
                .collect::<HashSet<_>>();
            assert_eq!(
                rendered_stairs,
                printed_stairs,
                "quest {} stair marker and playable stair squares disagree",
                quest_index + 1
            );
        }
    }

    #[test]
    fn furniture_blocks_click_paths_monsters_and_runtime_spawns() {
        let mut game =
            Game::from_quest(QuestDefinition::original_us_trial().unwrap(), 0x424c_4f43).unwrap();
        let hero_id = game.hero_order[0];
        let monster_id = game
            .units
            .iter()
            .find(|unit| matches!(unit.figure, FigureKind::Monster(_)))
            .unwrap()
            .id;
        for unit in &mut game.units {
            unit.alive = unit.id == hero_id || unit.id == monster_id;
            unit.escaped = false;
        }
        game.cells.iter_mut().for_each(|cell| cell.revealed = true);
        game.doors.iter_mut().for_each(|door| door.open = true);
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .pos = Pos::new(6, 7);
        game.units
            .iter_mut()
            .find(|unit| unit.id == monster_id)
            .unwrap()
            .pos = Pos::new(5, 7);
        game.phase = GamePhase::HeroTurn { order_index: 0 };
        game.hero_turn = HeroTurnState::default();
        game.apply_movement_roll(&[3, 3]).unwrap();

        let table_squares = game
            .props
            .iter()
            .find(|prop| prop.kind == PropKind::Table && prop.pos == Pos::new(6, 5))
            .unwrap()
            .footprint_squares();
        assert!(
            table_squares
                .iter()
                .all(|&pos| game.is_furniture_square(pos))
        );
        assert!(
            game.active_move_destinations()
                .iter()
                .all(|pos| !table_squares.contains(pos))
        );
        assert_eq!(game.active_move_path_to(Pos::new(6, 6)), None);
        assert_eq!(game.move_active(Direction::North), Err(RuleError::Blocked));
        assert_eq!(
            game.spawn_monster_at(MonsterKind::Orc, Pos::new(7, 5)),
            None
        );

        game.units
            .iter_mut()
            .find(|unit| unit.id == hero_id)
            .unwrap()
            .pos = Pos::new(6, 4);
        let path = game.path_to_nearest_hero(monster_id).unwrap();
        assert!(path.iter().all(|&pos| !game.is_furniture_square(pos)));
    }

    #[test]
    fn carried_quest_chest_stops_occupying_the_floor_until_dropped() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_prince_magnus_gold().unwrap(),
            0x4348_4553_54,
        )
        .unwrap();
        let hero = game.hero_order[0];
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(10, 7);
        game.reveal_from(Pos::new(10, 7));
        assert!(game.is_furniture_square(Pos::new(10, 8)));

        game.take_quest_item().unwrap();
        assert!(!game.is_furniture_square(Pos::new(10, 8)));
        assert!(!game.is_furniture_square(Pos::new(10, 7)));

        game.drop_quest_item().unwrap();
        assert!(game.is_furniture_square(Pos::new(10, 7)));
    }

    #[test]
    fn command_uses_immediate_and_future_mind_rolls_then_makes_zargon_attack_with_the_hero() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_return_to_barak_tor().unwrap(),
            80,
        )
        .unwrap();
        let witch = game
            .units
            .iter()
            .find(|unit| unit.name == "Witch Lord")
            .unwrap()
            .id;
        let barbarian = game.hero_order[0];
        let dwarf = game.hero_order[1];
        game.units
            .iter_mut()
            .find(|unit| unit.id == witch)
            .unwrap()
            .pos = Pos::new(10, 7);
        game.units
            .iter_mut()
            .find(|unit| unit.id == barbarian)
            .unwrap()
            .pos = Pos::new(12, 7);
        game.units
            .iter_mut()
            .find(|unit| unit.id == dwarf)
            .unwrap()
            .pos = Pos::new(13, 7);
        game.units
            .iter_mut()
            .find(|unit| unit.id == witch)
            .unwrap()
            .chaos_spells = vec![ChaosSpell::Command];

        let step = game.try_cast_chaos_spell(witch).unwrap();
        assert!(matches!(
            step,
            ZargonStep::Cast {
                caster,
                target,
                spell: ChaosSpell::Command,
                resistance_dice: 2
            } if caster == witch && target == barbarian
        ));
        assert!(game.unit(barbarian).unwrap().commanded);
        assert!(
            !game
                .resolve_chaos_spell_resistance(barbarian, ChaosSpell::Command, &[1, 5])
                .unwrap()
        );

        game.phase = GamePhase::ZargonTurn;
        let attack = game.advance_zargon_turn().unwrap();
        let ZargonStep::Attack(plan) = attack else {
            panic!("Command should make the adjacent Hero attack for Zargon");
        };
        assert_eq!(plan.attacker, barbarian);
        assert_eq!(plan.defender, dwarf);
        game.resolve_attack(
            plan,
            &vec![CombatFace::Skull; plan.attack_dice as usize],
            &vec![CombatFace::WhiteShield; plan.defend_dice as usize],
        )
        .unwrap();

        game.phase = GamePhase::HeroTurn { order_index: 0 };
        assert_eq!(
            game.pending_hero_spell_resistance(),
            Some((barbarian, ChaosSpell::Command, 2))
        );
        assert!(
            game.resolve_chaos_spell_resistance(barbarian, ChaosSpell::Command, &[6, 1])
                .unwrap()
        );
        assert!(!game.unit(barbarian).unwrap().commanded);
    }

    fn place_living_heroes_on_oracle_stairs(game: &mut Game) {
        assert!(!game.stairs.is_empty());
        for (index, hero_id) in game.hero_order.clone().into_iter().enumerate() {
            if game.unit(hero_id).is_some_and(|hero| hero.alive) {
                game.units
                    .iter_mut()
                    .find(|unit| unit.id == hero_id)
                    .unwrap()
                    .pos = game.stairs[index % game.stairs.len()];
            }
        }
    }

    fn defeat_named_for_campaign_oracle(game: &mut Game, name: &str) -> UnitId {
        let attacker = game.hero_order[0];
        let defender = game
            .units
            .iter()
            .find(|unit| unit.name == name && unit.faction == Faction::Monster)
            .map(|unit| unit.id)
            .unwrap_or_else(|| panic!("campaign oracle could not find {name}"));

        for monster_id in game
            .units
            .iter()
            .filter(|unit| unit.faction == Faction::Monster && unit.id != defender && unit.alive)
            .map(|unit| unit.id)
            .collect::<Vec<_>>()
        {
            game.damage_without_defense(monster_id, u8::MAX);
        }
        for (index, hero_id) in game.hero_order.clone().into_iter().enumerate().skip(1) {
            game.units
                .iter_mut()
                .find(|unit| unit.id == hero_id)
                .unwrap()
                .pos = game.stairs[index % game.stairs.len()];
        }
        game.cells
            .iter_mut()
            .filter(|cell| cell.passable)
            .for_each(|cell| cell.revealed = true);
        game.doors.iter_mut().for_each(|door| {
            door.open = true;
            door.discovered = true;
        });

        let occupied_by_other = |pos: Pos| {
            game.units.iter().any(|unit| {
                unit.alive && unit.id != attacker && unit.id != defender && unit.pos == pos
            })
        };
        let (attacker_pos, defender_pos) = (0..BOARD_HEIGHT)
            .flat_map(|y| (0..BOARD_WIDTH).map(move |x| Pos::new(x, y)))
            .filter(|&pos| {
                game.cell(pos).is_some_and(|cell| cell.passable)
                    && !game.is_furniture_square(pos)
                    && !game.stairs.contains(&pos)
                    && !occupied_by_other(pos)
            })
            .find_map(|attacker_pos| {
                Direction::ALL.into_iter().find_map(|direction| {
                    let defender_pos = attacker_pos.offset(direction)?;
                    (game.cell(defender_pos).is_some_and(|cell| cell.passable)
                        && !game.is_furniture_square(defender_pos)
                        && !game.stairs.contains(&defender_pos)
                        && !occupied_by_other(defender_pos)
                        && game.boundary_is_open(attacker_pos, defender_pos))
                    .then_some((attacker_pos, defender_pos))
                })
            })
            .expect("the canonical board must contain an open adjacent combat pair");

        game.phase = GamePhase::HeroTurn { order_index: 0 };
        game.hero_turn = HeroTurnState::default();
        game.units
            .iter_mut()
            .find(|unit| unit.id == attacker)
            .unwrap()
            .pos = attacker_pos;
        let target = game
            .units
            .iter_mut()
            .find(|unit| unit.id == defender)
            .unwrap();
        target.pos = defender_pos;
        target.body = 1;
        target.alive = true;
        target.dormant = false;
        target.hidden_until_activated = false;
        target.invulnerable_until_acts = false;
        game.allocate_pending_visible_figures();

        let options = game.active_attack_options().unwrap();
        let plan = if game.unit(defender).unwrap().immune_except_spirit_blade {
            options
                .into_iter()
                .find(|plan| plan.defender == defender && plan.source == AttackSource::SpiritBlade)
                .expect("the final campaign oracle must attack with the recovered Spirit Blade")
        } else {
            options
                .into_iter()
                .filter(|plan| plan.defender == defender)
                .max_by_key(|plan| plan.attack_dice)
                .expect("the named monster must be attackable")
        };
        let outcome = game
            .resolve_attack(
                plan,
                &vec![CombatFace::Skull; plan.attack_dice as usize],
                &vec![CombatFace::WhiteShield; plan.defend_dice as usize],
            )
            .unwrap();
        assert!(
            outcome.defender_died,
            "{name} did not die in the oracle combat"
        );
        attacker
    }

    fn defeat_all_monsters_for_campaign_oracle(game: &mut Game) {
        for monster_id in game
            .units
            .iter()
            .filter(|unit| unit.faction == Faction::Monster && unit.alive)
            .map(|unit| unit.id)
            .collect::<Vec<_>>()
        {
            game.damage_without_defense(monster_id, u8::MAX);
        }
        game.check_terminal();
    }

    fn finish_original_us_quest_for_campaign_oracle(game: &mut Game, quest_index: usize) {
        match quest_index {
            0 => {
                defeat_named_for_campaign_oracle(game, "Verag");
                place_living_heroes_on_oracle_stairs(game);
                game.check_terminal();
            }
            1 => {
                game.reveal_from(Pos::new(5, 11));
                let ragnar = game
                    .units
                    .iter()
                    .find(|unit| unit.name == "Sir Ragnar")
                    .map(|unit| unit.id)
                    .unwrap();
                game.units
                    .iter_mut()
                    .find(|unit| unit.id == ragnar)
                    .unwrap()
                    .pos = game.stairs[0];
                game.check_terminal();
            }
            2 => {
                defeat_named_for_campaign_oracle(game, "Ulag");
                place_living_heroes_on_oracle_stairs(game);
                game.check_terminal();
            }
            3 => {
                defeat_all_monsters_for_campaign_oracle(game);
                let carrier = game.hero_order[0];
                for item_index in 0..game.quest_items.len() {
                    game.phase = GamePhase::HeroTurn { order_index: 0 };
                    game.hero_turn = HeroTurnState::default();
                    let item_pos = game.props[game.quest_items[item_index].prop_index].pos;
                    game.units
                        .iter_mut()
                        .find(|unit| unit.id == carrier)
                        .unwrap()
                        .pos = item_pos;
                    game.take_quest_item().unwrap();
                    game.units
                        .iter_mut()
                        .find(|unit| unit.id == carrier)
                        .unwrap()
                        .pos = game.stairs[0];
                    game.deliver_carried_quest_item(carrier);
                }
                place_living_heroes_on_oracle_stairs(game);
                game.check_terminal();
            }
            4 => {
                defeat_all_monsters_for_campaign_oracle(game);
                let finder = game.hero_order[0];
                game.phase = GamePhase::HeroTurn { order_index: 0 };
                game.hero_turn = HeroTurnState::default();
                game.units
                    .iter_mut()
                    .find(|unit| unit.id == finder)
                    .unwrap()
                    .pos = Pos::new(10, 9);
                game.search_treasure().unwrap();
                place_living_heroes_on_oracle_stairs(game);
                game.check_terminal();
            }
            5 => {
                defeat_named_for_campaign_oracle(game, "Grak");
                for (index, hero_id) in game.hero_order.clone().into_iter().enumerate() {
                    game.units
                        .iter_mut()
                        .find(|unit| unit.id == hero_id)
                        .unwrap()
                        .pos = game.stairs[index % game.stairs.len()];
                    assert!(game.escape_hero_on_stairs(hero_id));
                }
                game.check_terminal();
            }
            6 => {
                defeat_named_for_campaign_oracle(game, "Wardoz");
                let finder = game.hero_order[0];
                game.phase = GamePhase::HeroTurn { order_index: 0 };
                game.hero_turn = HeroTurnState::default();
                game.units
                    .iter_mut()
                    .find(|unit| unit.id == finder)
                    .unwrap()
                    .pos = Pos::new(15, 16);
                game.search_treasure().unwrap();
                place_living_heroes_on_oracle_stairs(game);
                game.check_terminal();
            }
            7 => {
                defeat_named_for_campaign_oracle(game, "Balur");
                place_living_heroes_on_oracle_stairs(game);
                game.check_terminal();
            }
            8 => {
                place_living_heroes_on_oracle_stairs(game);
                game.check_terminal();
            }
            9 => {
                for (index, hero_id) in game.hero_order.clone().into_iter().enumerate() {
                    game.units
                        .iter_mut()
                        .find(|unit| unit.id == hero_id)
                        .unwrap()
                        .pos = game.stairs[index % game.stairs.len()];
                    assert!(game.escape_hero_on_stairs(hero_id));
                }
                game.check_terminal();
            }
            10 => defeat_all_monsters_for_campaign_oracle(game),
            11 => {
                let bearer = game
                    .units
                    .iter()
                    .find(|unit| unit.name == "Star Bearer")
                    .map(|unit| unit.id)
                    .unwrap();
                let drop_pos = game.unit(bearer).unwrap().pos;
                game.damage_without_defense(bearer, u8::MAX);
                let carrier = game.hero_order[0];
                game.phase = GamePhase::HeroTurn { order_index: 0 };
                game.hero_turn = HeroTurnState::default();
                game.units
                    .iter_mut()
                    .find(|unit| unit.id == carrier)
                    .unwrap()
                    .pos = drop_pos;
                game.take_quest_item().unwrap();
                game.units
                    .iter_mut()
                    .find(|unit| unit.id == carrier)
                    .unwrap()
                    .pos = game.stairs[0];
                game.deliver_carried_quest_item(carrier);
                place_living_heroes_on_oracle_stairs(game);
                game.check_terminal();
            }
            12 => {
                defeat_all_monsters_for_campaign_oracle(game);
                let finder = game.hero_order[0];
                game.phase = GamePhase::HeroTurn { order_index: 0 };
                game.hero_turn = HeroTurnState::default();
                game.units
                    .iter_mut()
                    .find(|unit| unit.id == finder)
                    .unwrap()
                    .pos = Pos::new(23, 15);
                game.search_treasure().unwrap();
                place_living_heroes_on_oracle_stairs(game);
                game.check_terminal();
            }
            13 => {
                defeat_named_for_campaign_oracle(game, "Witch Lord");
            }
            _ => panic!("the original-US campaign has exactly fourteen quests"),
        }
        assert_eq!(
            game.phase,
            GamePhase::Won,
            "quest {} did not reach the engine's victory state",
            quest_index + 1
        );
    }

    fn original_us_campaign_oracle() -> serde_json::Value {
        let mut campaign = Campaign::default();
        let mut chapters = Vec::new();
        for quest_index in 0..QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS {
            assert_eq!(campaign.next_quest_index(), quest_index);
            let beginning = campaign.clone();
            let mut game = Game::from_quest(
                QuestDefinition::original_us_game_system(quest_index).unwrap(),
                0x4f52_4143_4c45_0000 + quest_index as u64,
            )
            .unwrap();
            campaign.apply_to_game(&mut game);
            finish_original_us_quest_for_campaign_oracle(&mut game, quest_index);
            campaign.record_success(quest_index, &game).unwrap();
            campaign.validate().unwrap();
            assert_eq!(
                campaign.completed_quests,
                (1..=quest_index as u8 + 1).collect::<Vec<_>>()
            );
            chapters.push(serde_json::json!({
                "quest_number": quest_index + 1,
                "title": game.title,
                "objective": format!("{:?}", game.objective),
                "terminal_phase": "Won",
                "terminal_log": game.log.back(),
                "begin_save_fingerprint_fnv1a64": campaign_oracle_fingerprint(&beginning),
                "end_save_fingerprint_fnv1a64": campaign_oracle_fingerprint(&campaign),
                "end_gold": campaign.heroes.iter().map(|hero| hero.inventory.gold).collect::<Vec<_>>(),
                "end_artifacts": campaign.heroes.iter().map(|hero| &hero.inventory.artifacts).collect::<Vec<_>>(),
                "end_champion": campaign.heroes.iter().map(|hero| hero.champion).collect::<Vec<_>>(),
                "end_lost_artifacts": campaign.lost_artifacts,
            }));
        }
        assert!(campaign.heroes.iter().all(|hero| hero.champion));
        serde_json::Value::Array(chapters)
    }

    fn campaign_oracle_fingerprint(campaign: &Campaign) -> String {
        // FNV-1a over canonical serde_json observes every persisted field while
        // the oracle keeps important rewards adjacent and readable. This makes
        // a complete begin/end save regression compact enough to review.
        let fingerprint = serde_json::to_vec(campaign)
            .unwrap()
            .into_iter()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
            });
        format!("{fingerprint:016x}")
    }

    #[test]
    fn all_fourteen_campaign_chapters_match_engine_produced_begin_and_end_saves() {
        let actual = original_us_campaign_oracle();
        if std::env::var_os("HEROQUEST_PRINT_CAMPAIGN_ORACLE").is_some() {
            println!("{}", serde_json::to_string(&actual).unwrap());
            return;
        }
        let expected: serde_json::Value =
            serde_json::from_str(include_str!("../tests/oracles/original-us-campaign.json"))
                .unwrap();
        assert_eq!(actual, expected);
    }
}
