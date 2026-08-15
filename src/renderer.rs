use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Quat, Vec3};
use sdl3::video::Window;
use serde::Deserialize;
use wgpu::rwh::{HasDisplayHandle, HasWindowHandle};
use wgpu::util::DeviceExt;

use crate::campaign::{Campaign, armory_item_name};
use crate::cards::{ChaosSpell, HeroSpell, ORIGINAL_US_CHAOS_SPELLS};
use crate::dice::{DIE_HALF_EXTENT, DieKind, DiePose, PLAYER_DICE_RACK, PLAYER_DICE_RACK_LOCAL_Y};
use crate::equipment::{ArmoryItem, ORIGINAL_US_ARMORY};
use crate::game::{Game, GamePhase, HeroSpellTarget, Unit, UnitId};
use crate::model::{
    BOARD_HEIGHT, BOARD_WIDTH, Direction, Faction, FigureKind, HeroKind, MonsterKind, Pos,
    PropKind, TrapKind,
};
use crate::quest::QuestDefinition;
use crate::startup::{QUEST_TITLES, SpellGroup, StartupFlow, StartupHotspot, StartupStage};

const MAX_INSTANCES: usize = 8_192;
const MAX_SPRITE_INSTANCES: usize = 512;
const STARTUP_DESIGN_SIZE: u32 = 2048;
const STARTUP_TEXTURE_SIZE: u32 = 1024;
const HUD_TEXTURE_WIDTH: u32 = 1024;
const HUD_TEXTURE_HEIGHT: u32 = 640;
const ZARGON_DICE_ROLL_CENTER: Vec3 = Vec3::new(8.4, 0.08, 17.0);
const ZARGON_QUEST_BOOK_ANGLE: f32 = std::f32::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZargonDeckKind {
    Treasure,
    Artifact,
    ChaosSpell,
    Monster,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabletopSurface {
    HeroCard(HeroKind),
    CharacterSheet(HeroKind),
    ActionReference(HeroKind),
    Armory,
    HeroSpellCard {
        hero: HeroKind,
        spell: HeroSpell,
    },
    HeroSpellDiscard {
        hero: HeroKind,
        spell: HeroSpell,
        count: usize,
    },
    ZargonDeck(ZargonDeckKind),
    ChaosSpellDiscard {
        spell: ChaosSpell,
        count: usize,
    },
    MonsterCard(MonsterKind),
    QuestBook,
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
}

impl Vertex {
    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct InstanceRaw {
    model: [[f32; 4]; 4],
    color: [f32; 4],
}

impl InstanceRaw {
    fn new(model: Mat4, color: [f32; 3]) -> Self {
        Self::with_alpha(model, color, 1.0)
    }

    fn with_alpha(model: Mat4, color: [f32; 3], alpha: f32) -> Self {
        Self {
            model: model.to_cols_array_2d(),
            color: [color[0], color[1], color[2], alpha],
        }
    }

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 32,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 48,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 64,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    view_projection: [[f32; 4]; 4],
    animation: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BoardVertex {
    position: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct EnvironmentVertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

impl EnvironmentVertex {
    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 24,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct EnvironmentMaterialUniform {
    base_color: [f32; 4],
    emissive: [f32; 4],
}

struct EnvironmentMaterial {
    _texture: wgpu::Texture,
    _uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

struct EnvironmentPrimitive {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    material_index: usize,
}

struct EnvironmentMesh {
    pipeline: wgpu::RenderPipeline,
    primitives: Vec<EnvironmentPrimitive>,
    materials: Vec<EnvironmentMaterial>,
    path: PathBuf,
}

struct EnvironmentBackdrop {
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    path: PathBuf,
}

impl BoardVertex {
    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayDialog {
    pub title: String,
    pub body: String,
    pub hint: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameOverlay {
    pub heading: String,
    pub status: String,
    pub message: String,
    pub actions: Vec<String>,
    pub dialog: Option<OverlayDialog>,
}

struct HudOverlay {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    last_state: Option<GameOverlay>,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SpriteInstanceRaw {
    model: [[f32; 4]; 4],
}

impl SpriteInstanceRaw {
    fn new(model: Mat4) -> Self {
        Self {
            model: model.to_cols_array_2d(),
        }
    }

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 32,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: 48,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SpriteKey {
    Figure(FigureKind),
    Prop(PropKind),
    Die(DieKind),
    DoorOpen,
    DoorClosed,
    SecretDoor,
    Trap(TrapKind),
    BlockedSquare,
    BlockedDouble,
    DamageSkull,
    FurnitureRat,
    FurnitureSkull,
}

struct SpriteAsset {
    key: SpriteKey,
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    path: PathBuf,
}

struct SpriteBatch {
    asset_index: usize,
    instances: std::ops::Range<u32>,
}

struct SpriteArt {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    index_count: u32,
    assets: Vec<SpriteAsset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiceDecalKind {
    Skull,
    WhiteShield,
    BlackShield,
    MovementPip,
}

struct DiceDecalAsset {
    kind: DiceDecalKind,
    texture: StartupTexture,
    path: PathBuf,
}

struct DiceDecalBatch {
    asset_index: usize,
    instances: std::ops::Range<u32>,
}

struct DiceDecals {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    index_count: u32,
    assets: Vec<DiceDecalAsset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComponentDecalKey {
    Prop(PropKind),
    DoorOpen,
    DoorClosed,
    SecretDoor,
    Trap(TrapKind),
    BlockedSquare,
    BlockedDouble,
    DamageSkull,
}

struct ComponentDecalAsset {
    key: ComponentDecalKey,
    texture: StartupTexture,
    path: PathBuf,
}

struct ComponentDecalBatch {
    asset_index: usize,
    instances: std::ops::Range<u32>,
}

struct ComponentDecals {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    index_count: u32,
    assets: Vec<ComponentDecalAsset>,
}

struct PieceModelPrimitive {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
}

struct PieceModelAsset {
    key: SpriteKey,
    variant_index: usize,
    variant_count: usize,
    primitives: Vec<PieceModelPrimitive>,
    instance_buffer: wgpu::Buffer,
    path: PathBuf,
}

struct PieceModels {
    assets: Vec<PieceModelAsset>,
}

#[derive(Debug, Clone, Copy)]
struct PiecePose {
    translation: Vec3,
    rotation: Quat,
    scale: Vec3,
    visible: bool,
}

#[derive(Debug, Clone, Copy)]
struct AnimatedPiece {
    grid_pos: Pos,
    move_from: Vec3,
    move_to: Vec3,
    move_started: f32,
    move_duration: f32,
    facing: f32,
    attack_started: Option<f32>,
    attack_target: Vec3,
    hit_started: Option<f32>,
    death_started: Option<f32>,
    last_body: i16,
    was_alive: bool,
}

#[derive(Default)]
struct PieceAnimations {
    pieces: HashMap<UnitId, AnimatedPiece>,
    last_combat_sequence: u64,
}

#[derive(Debug, Clone, Copy)]
enum ActionCameraFocus {
    Unit(UnitId),
    Combat(UnitId, UnitId),
    Dice(Vec3),
    Board(Vec3),
}

struct ActionCameraDirector {
    focus: Option<ActionCameraFocus>,
    focus_started: Instant,
    last_update: Instant,
    last_combat_sequence: u64,
}

#[derive(Debug, Clone, Copy)]
struct DiceRollTransform {
    center: Vec3,
    rotation: Quat,
    station: Option<usize>,
}

impl DiceRollTransform {
    fn world_pose(self, pose: DiePose) -> DiePose {
        DiePose {
            kind: pose.kind,
            translation: self.center + self.rotation * pose.translation,
            rotation: self.rotation * pose.rotation,
        }
    }
}

impl ActionCameraDirector {
    fn new() -> Self {
        Self {
            focus: None,
            focus_started: Instant::now(),
            last_update: Instant::now(),
            last_combat_sequence: 0,
        }
    }

    fn focus_unit(&mut self, unit: UnitId) {
        self.focus = Some(ActionCameraFocus::Unit(unit));
        self.focus_started = Instant::now();
    }

    fn focus_combat(&mut self, attacker: UnitId, defender: UnitId) {
        self.focus = Some(ActionCameraFocus::Combat(attacker, defender));
        self.focus_started = Instant::now();
    }

    fn focus_dice(&mut self, center: Vec3) {
        self.focus = Some(ActionCameraFocus::Dice(center));
        self.focus_started = Instant::now();
    }

    fn focus_board(&mut self, center: Vec3) {
        self.focus = Some(ActionCameraFocus::Board(center));
        self.focus_started = Instant::now();
    }

    fn update(&mut self, camera: &mut Camera, game: &Game, poses: &HashMap<UnitId, PiecePose>) {
        if let Some(combat) = game.last_combat_visual
            && combat.sequence != self.last_combat_sequence
        {
            self.last_combat_sequence = combat.sequence;
            self.focus_combat(combat.attacker, combat.defender);
        }

        let pose_position = |id: UnitId| {
            poses
                .get(&id)
                .filter(|pose| pose.visible)
                .map(|pose| pose.translation)
                .or_else(|| {
                    game.unit(id)
                        .filter(|unit| unit.alive)
                        .map(|unit| board_world(unit.pos))
                })
        };
        let (desired, desired_distance, desired_pitch, zoom_duration) = match self.focus {
            Some(ActionCameraFocus::Unit(unit)) => (
                pose_position(unit),
                15.5,
                0.66,
                Duration::from_secs_f32(1.2),
            ),
            Some(ActionCameraFocus::Combat(attacker, defender)) => pose_position(attacker)
                .zip(pose_position(defender))
                .map(|(a, b)| (a + b) * 0.5)
                .map_or(
                    (None, 13.5, 0.62, Duration::from_secs_f32(1.5)),
                    |position| (Some(position), 13.5, 0.62, Duration::from_secs_f32(1.5)),
                ),
            Some(ActionCameraFocus::Dice(center)) => (
                // Frame the high release point as well as the tabletop.  A
                // low target cropped the lift and made the subsequent fall
                // difficult to read as a physical throw.
                Some(center + Vec3::Y * 2.15),
                13.75,
                0.72,
                Duration::from_secs_f32(2.5),
            ),
            Some(ActionCameraFocus::Board(center)) => {
                (Some(center), 15.5, 0.66, Duration::from_secs_f32(1.2))
            }
            None => (
                game.active_hero_id().and_then(pose_position),
                camera.distance,
                camera.pitch,
                Duration::ZERO,
            ),
        };

        let now = Instant::now();
        let dt = now.duration_since(self.last_update).as_secs_f32().min(0.1);
        self.last_update = now;
        if let Some(desired) = desired {
            camera.target =
                smooth_camera_target(camera.target, desired + Vec3::new(0.0, 0.38, 0.0), dt);
        }
        if now.duration_since(self.focus_started) <= zoom_duration {
            camera.distance = smooth_camera_value(camera.distance, desired_distance, dt, 9.0);
            camera.pitch = smooth_camera_value(camera.pitch, desired_pitch, dt, 9.0);
        }
    }
}

fn smooth_camera_target(current: Vec3, desired: Vec3, dt: f32) -> Vec3 {
    let response = 1.0 - (-7.5 * dt.max(0.0)).exp();
    current.lerp(desired, response.clamp(0.0, 1.0))
}

fn smooth_camera_value(current: f32, desired: f32, dt: f32, speed: f32) -> f32 {
    let response = 1.0 - (-speed * dt.max(0.0)).exp();
    current + (desired - current) * response.clamp(0.0, 1.0)
}

impl PieceAnimations {
    fn poses(&mut self, game: &Game, now: f32) -> HashMap<UnitId, PiecePose> {
        for unit in &game.units {
            let target = unit_board_world(game, unit);
            let initial_facing = preferred_monster_facing(game, unit).unwrap_or(0.0);
            let state = self.pieces.entry(unit.id).or_insert(AnimatedPiece {
                grid_pos: unit.pos,
                move_from: target,
                move_to: target,
                move_started: now,
                move_duration: 0.0,
                facing: initial_facing,
                attack_started: None,
                attack_target: target,
                hit_started: None,
                death_started: (!unit.alive).then_some(now),
                last_body: unit.body,
                was_alive: unit.alive,
            });
            if state.grid_pos != unit.pos || (state.move_to - target).length_squared() > 0.0001 {
                let current = sample_piece_translation(state, now);
                let delta = target - current;
                state.move_from = current;
                state.move_to = target;
                state.move_started = now;
                state.move_duration = (delta.length() * 0.105).clamp(0.26, 0.72);
                state.grid_pos = unit.pos;
                if let Some(facing) = facing_toward(current, target) {
                    state.facing = facing;
                }
            }
            let moving = now < state.move_started + state.move_duration;
            if !moving && unit.faction == Faction::Monster {
                let recent_attack_facing = state
                    .attack_started
                    .filter(|started| now - *started < 1.25)
                    .and_then(|_| facing_toward(target, state.attack_target));
                if let Some(facing) =
                    recent_attack_facing.or_else(|| preferred_monster_facing(game, unit))
                {
                    state.facing = facing;
                }
            }
            if unit.body < state.last_body {
                state.hit_started = Some(now);
            }
            if state.was_alive && !unit.alive {
                state.death_started = Some(now);
            }
            state.last_body = unit.body;
            state.was_alive = unit.alive;
        }

        if let Some(event) = game.last_combat_visual
            && event.sequence != self.last_combat_sequence
        {
            self.last_combat_sequence = event.sequence;
            let attacker_pos = game
                .unit(event.attacker)
                .map(|unit| unit_board_world(game, unit));
            let defender_pos = game
                .unit(event.defender)
                .map(|unit| unit_board_world(game, unit));
            if let (Some(attacker_pos), Some(defender_pos)) = (attacker_pos, defender_pos) {
                if let Some(attacker) = self.pieces.get_mut(&event.attacker) {
                    attacker.attack_started = Some(now);
                    attacker.attack_target = defender_pos;
                    if let Some(facing) = facing_toward(attacker_pos, defender_pos) {
                        attacker.facing = facing;
                    }
                }
                if event.damage > 0
                    && let Some(defender) = self.pieces.get_mut(&event.defender)
                {
                    defender.hit_started = Some(now);
                }
            }
        }

        game.units
            .iter()
            .filter_map(|unit| {
                let state = self.pieces.get(&unit.id)?;
                let mut translation = sample_piece_translation(state, now);
                let mut rotation = Quat::from_rotation_y(state.facing);
                let mut scale = Vec3::ONE;
                let mut visible = true;

                if let Some(started) = state.attack_started {
                    let t = ((now - started) / 0.46).clamp(0.0, 1.0);
                    if t < 1.0 {
                        let direction = (state.attack_target - translation).normalize_or_zero();
                        translation += direction * (std::f32::consts::PI * t).sin() * 0.34;
                        translation.y += (std::f32::consts::PI * t).sin() * 0.10;
                    }
                }
                if let Some(started) = state.hit_started {
                    let t = ((now - started) / 0.34).clamp(0.0, 1.0);
                    if t < 1.0 {
                        let shake = (t * std::f32::consts::TAU * 3.0).sin() * (1.0 - t);
                        translation.x += shake * 0.10;
                        rotation *= Quat::from_rotation_z(shake * 0.16);
                    }
                }
                if let Some(started) = state.death_started {
                    let t = ((now - started) / 0.82).clamp(0.0, 1.0);
                    rotation *= Quat::from_rotation_z(-t * std::f32::consts::FRAC_PI_2);
                    translation.y -= t * 0.18;
                    scale.y = 1.0 - t * 0.30;
                    visible = now - started < 1.15;
                }
                Some((
                    unit.id,
                    PiecePose {
                        translation,
                        rotation,
                        scale,
                        visible,
                    },
                ))
            })
            .collect()
    }
}

fn facing_toward(from: Vec3, to: Vec3) -> Option<f32> {
    let delta = to - from;
    (Vec3::new(delta.x, 0.0, delta.z).length_squared() > 0.001).then(|| delta.x.atan2(delta.z))
}

fn preferred_monster_facing(game: &Game, monster: &Unit) -> Option<f32> {
    if monster.faction != Faction::Monster || !monster.alive || monster.escaped {
        return None;
    }
    let from = unit_board_world(game, monster);
    game.units
        .iter()
        .filter(|unit| unit.faction == Faction::Hero && unit.alive && !unit.escaped)
        .min_by_key(|hero| monster.pos.x.abs_diff(hero.pos.x) + monster.pos.y.abs_diff(hero.pos.y))
        .and_then(|hero| facing_toward(from, unit_board_world(game, hero)))
}

fn sample_piece_translation(state: &AnimatedPiece, now: f32) -> Vec3 {
    if state.move_duration <= f32::EPSILON {
        return state.move_to;
    }
    let linear = ((now - state.move_started) / state.move_duration).clamp(0.0, 1.0);
    let smooth = linear * linear * (3.0 - 2.0 * linear);
    let mut position = state.move_from.lerp(state.move_to, smooth);
    position.y += (std::f32::consts::PI * linear).sin()
        * (0.32 + (state.move_to - state.move_from).length().min(5.0) * 0.055);
    position
}

fn piece_pose(poses: &HashMap<UnitId, PiecePose>, unit: &Unit) -> PiecePose {
    poses.get(&unit.id).copied().unwrap_or(PiecePose {
        translation: board_world(unit.pos),
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
        visible: unit.alive,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BlockedMarkerPlacement {
    center: Vec3,
    angle: f32,
    double: bool,
}

/// Allocate the two physical double blocked-square tiles first, then use the
/// single-block backs for every remaining revealed square. Quest data records
/// occupied squares rather than cardboard-punchboard identities, so stable
/// east-then-south pairing reconstructs a deterministic finite-piece layout.
fn blocked_marker_placements(game: &Game) -> Vec<BlockedMarkerPlacement> {
    let mut positions = game
        .blocked_markers
        .iter()
        .copied()
        .filter(|&pos| game.cells[Game::cell_index(pos)].revealed)
        .collect::<Vec<_>>();
    positions.sort_unstable_by_key(|pos| (pos.y, pos.x));
    let available = positions.iter().copied().collect::<HashSet<_>>();
    let mut used = HashSet::new();
    let mut output = Vec::new();
    let mut doubles = 0;
    for pos in &positions {
        if used.contains(pos) {
            continue;
        }
        let partner = (doubles < 2)
            .then(|| {
                [Direction::East, Direction::South]
                    .into_iter()
                    .filter_map(|direction| pos.offset(direction))
                    .find(|candidate| available.contains(candidate) && !used.contains(candidate))
            })
            .flatten();
        if let Some(partner) = partner {
            used.insert(*pos);
            used.insert(partner);
            doubles += 1;
            output.push(BlockedMarkerPlacement {
                center: (board_world(*pos) + board_world(partner)) * 0.5,
                angle: if partner.x != pos.x {
                    0.0
                } else {
                    std::f32::consts::FRAC_PI_2
                },
                double: true,
            });
        } else {
            used.insert(*pos);
            output.push(BlockedMarkerPlacement {
                center: board_world(*pos),
                angle: 0.0,
                double: false,
            });
        }
    }
    output
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FurnitureDressingPlacement {
    position: Vec3,
    rotation: Quat,
}

/// The classic box supplies four rats and four skulls as removable pegs rather
/// than quest-map objects. Original set photographs commonly place one pair on
/// each bookcase, the cupboard, and the fireplace. Keep those four physical
/// pairs finite and attach them to the assembled furniture instead of inventing
/// occupiable board pieces.
fn furniture_dressing_placements(
    game: &Game,
    dressing: SpriteKey,
) -> Vec<FurnitureDressingPlacement> {
    let rat = match dressing {
        SpriteKey::FurnitureRat => true,
        SpriteKey::FurnitureSkull => false,
        _ => return Vec::new(),
    };
    game.props
        .iter()
        .filter(|prop| prop.carried_by.is_none() && game.is_prop_visible(prop))
        .filter_map(|prop| {
            let (height, horizontal, depth) = match prop.kind {
                PropKind::Bookcase => (1.69, 0.43, 0.0),
                PropKind::Cupboard => (1.60, 0.42, 0.0),
                PropKind::Fireplace => (1.53, 0.45, 0.04),
                _ => return None,
            };
            let furniture_rotation = Quat::from_rotation_y(
                -(prop.rotation_quarters as f32) * std::f32::consts::FRAC_PI_2,
            );
            let local = Vec3::new(if rat { -horizontal } else { horizontal }, height, depth);
            Some(FurnitureDressingPlacement {
                position: prop_world_position(prop.kind, prop.pos, prop.rotation_quarters, false)
                    + furniture_rotation * local
                    + Vec3::Y * 0.025,
                rotation: furniture_rotation
                    * Quat::from_rotation_y(if rat {
                        std::f32::consts::FRAC_PI_2
                    } else {
                        0.0
                    }),
            })
        })
        .take(4)
        .collect()
}

impl PieceModels {
    fn has(&self, key: SpriteKey) -> bool {
        self.assets.iter().any(|asset| asset.key == key)
    }

    fn build_instances(
        &self,
        game: &Game,
        poses: &HashMap<UnitId, PiecePose>,
        dice: &[DiePose],
        rolling_station: Option<usize>,
    ) -> Vec<Vec<InstanceRaw>> {
        self.assets
            .iter()
            .map(|asset| match asset.key {
                SpriteKey::Figure(figure) => game
                    .units
                    .iter()
                    .filter(|unit| {
                        rendered_figure(unit) == figure
                            && game.is_visible(unit)
                            && piece_model_variant_matches(asset, unit, &self.assets)
                    })
                    .filter_map(|unit| {
                        let pose = piece_pose(poses, unit);
                        pose.visible.then_some((unit, pose))
                    })
                    .map(|(unit, pose)| {
                        InstanceRaw::new(
                            Mat4::from_scale_rotation_translation(
                                pose.scale,
                                pose.rotation,
                                pose.translation + Vec3::Y * 0.18,
                            ),
                            unit.physical_figure.unwrap_or(unit.figure).color(),
                        )
                    })
                    .collect(),
                SpriteKey::Prop(kind) => game
                    .props
                    .iter()
                    .filter(|prop| prop.kind == kind && game.is_prop_visible(prop))
                    .map(|prop| {
                        let carried = prop.carried_by.is_some();
                        InstanceRaw::new(
                            Mat4::from_scale_rotation_translation(
                                Vec3::splat(if carried { 0.34 } else { 1.0 }),
                                Quat::from_rotation_y(
                                    -(prop.rotation_quarters as f32) * std::f32::consts::FRAC_PI_2,
                                ) * prop_model_local_rotation(kind),
                                prop_world_position(
                                    prop.kind,
                                    prop.pos,
                                    prop.rotation_quarters,
                                    carried,
                                ) + Vec3::Y * if carried { 1.12 } else { 0.04 },
                            ),
                            match kind {
                                PropKind::Stairs => [0.46, 0.43, 0.38],
                                _ => [0.44, 0.23, 0.08],
                            },
                        )
                    })
                    .collect(),
                SpriteKey::Die(kind) => visible_die_poses(dice, rolling_station)
                    .into_iter()
                    .filter(move |die| die.kind == kind)
                    .map(|die| {
                        InstanceRaw::new(
                            die_model_transform(die.translation, die.rotation),
                            match kind {
                                DieKind::Movement => [0.73, 0.025, 0.018],
                                DieKind::Combat => [0.90, 0.86, 0.72],
                            },
                        )
                    })
                    .collect(),
                SpriteKey::DoorOpen | SpriteKey::DoorClosed | SpriteKey::SecretDoor => game
                    .doors
                    .iter()
                    .filter(|door| {
                        game.is_door_visible(door)
                            && match asset.key {
                                SpriteKey::DoorOpen => !door.secret && door.open,
                                SpriteKey::DoorClosed => !door.secret && !door.open,
                                SpriteKey::SecretDoor => door.secret && door.discovered,
                                _ => false,
                            }
                    })
                    .map(|door| {
                        let center = (board_world(door.a) + board_world(door.b)) * 0.5;
                        let rotation = if door.a.x != door.b.x {
                            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)
                        } else {
                            Quat::IDENTITY
                        };
                        InstanceRaw::new(
                            Mat4::from_scale_rotation_translation(
                                Vec3::ONE,
                                rotation,
                                center + Vec3::Y * 0.035,
                            ),
                            if door.secret {
                                [0.48, 0.45, 0.40]
                            } else {
                                [0.32, 0.30, 0.28]
                            },
                        )
                    })
                    .collect(),
                SpriteKey::Trap(kind) => game
                    .traps
                    .iter()
                    .filter(|trap| trap.kind == kind && game.is_trap_marker_visible(trap))
                    .map(|trap| {
                        InstanceRaw::new(
                            Mat4::from_translation(board_world(trap.pos) + Vec3::Y * 0.025),
                            [0.38, 0.34, 0.29],
                        )
                    })
                    .collect(),
                SpriteKey::BlockedSquare | SpriteKey::BlockedDouble => {
                    blocked_marker_placements(game)
                        .into_iter()
                        .filter(|placement| {
                            placement.double == matches!(asset.key, SpriteKey::BlockedDouble)
                        })
                        .map(|placement| {
                            InstanceRaw::new(
                                Mat4::from_rotation_translation(
                                    Quat::from_rotation_y(placement.angle),
                                    placement.center + Vec3::Y * 0.025,
                                ),
                                [0.42, 0.39, 0.35],
                            )
                        })
                        .collect()
                }
                SpriteKey::DamageSkull => game
                    .units
                    .iter()
                    .filter(|unit| game.is_visible(unit))
                    .flat_map(|unit| {
                        damage_marker_positions(piece_pose(poses, unit).translation, unit)
                            .into_iter()
                    })
                    .map(|position| {
                        InstanceRaw::new(Mat4::from_translation(position), [0.74, 0.68, 0.52])
                    })
                    .collect(),
                SpriteKey::FurnitureRat | SpriteKey::FurnitureSkull => {
                    furniture_dressing_placements(game, asset.key)
                        .into_iter()
                        .map(|placement| {
                            InstanceRaw::new(
                                Mat4::from_rotation_translation(
                                    placement.rotation,
                                    placement.position,
                                ),
                                if matches!(asset.key, SpriteKey::FurnitureRat) {
                                    [0.36, 0.17, 0.07]
                                } else {
                                    [0.78, 0.72, 0.54]
                                },
                            )
                        })
                        .collect()
                }
            })
            .collect()
    }
}

fn piece_model_variant_matches(
    asset: &PieceModelAsset,
    unit: &Unit,
    available: &[PieceModelAsset],
) -> bool {
    if let Some(variant) = unit.model_variant {
        let requested_is_loaded = available.iter().any(|candidate| {
            candidate.key == asset.key
                && candidate.path.file_stem().and_then(|stem| stem.to_str())
                    == Some(variant.asset_stem())
        });
        return if requested_is_loaded {
            asset.path.file_stem().and_then(|stem| stem.to_str()) == Some(variant.asset_stem())
        } else {
            asset.variant_index == 0
        };
    }
    asset.variant_count <= 1
        || (unit.id.saturating_sub(1) as usize % asset.variant_count == asset.variant_index)
}

fn rendered_figure(unit: &Unit) -> FigureKind {
    unit.physical_figure.unwrap_or(unit.figure)
}

impl SpriteArt {
    fn build_instances(
        &self,
        game: &Game,
        camera: &Camera,
        piece_models: &PieceModels,
        poses: &HashMap<UnitId, PiecePose>,
    ) -> (Vec<SpriteInstanceRaw>, Vec<SpriteBatch>) {
        let mut instances = Vec::new();
        let mut batches = Vec::new();
        let rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2 - camera.yaw);

        for (asset_index, asset) in self.assets.iter().enumerate() {
            if piece_models.has(asset.key) {
                continue;
            }
            let start = instances.len() as u32;
            match asset.key {
                SpriteKey::Figure(figure) => {
                    for unit in &game.units {
                        if rendered_figure(unit) != figure || !game.is_visible(unit) {
                            continue;
                        }
                        let pose = piece_pose(poses, unit);
                        if !pose.visible {
                            continue;
                        }
                        let model = Mat4::from_scale_rotation_translation(
                            Vec3::new(0.92, 1.48, 1.0) * pose.scale,
                            rotation * pose.rotation,
                            pose.translation + Vec3::Y * 0.22,
                        );
                        instances.push(SpriteInstanceRaw::new(model));
                    }
                }
                SpriteKey::Prop(kind) => {
                    if kind == PropKind::Stairs {
                        continue;
                    }
                    for prop in &game.props {
                        if prop.kind != kind || !game.is_prop_visible(prop) {
                            continue;
                        }
                        let (width, height) = prop_sprite_size(kind);
                        let carried_scale = if prop.carried_by.is_some() { 0.38 } else { 1.0 };
                        let model = Mat4::from_scale_rotation_translation(
                            Vec3::new(width, height, 1.0) * carried_scale,
                            rotation,
                            prop_world_position(
                                prop.kind,
                                prop.pos,
                                prop.rotation_quarters,
                                prop.carried_by.is_some(),
                            ) + Vec3::Y
                                * if prop.carried_by.is_some() {
                                    1.08
                                } else {
                                    0.03
                                },
                        );
                        instances.push(SpriteInstanceRaw::new(model));
                    }
                }
                SpriteKey::DoorOpen
                | SpriteKey::DoorClosed
                | SpriteKey::SecretDoor
                | SpriteKey::Trap(_)
                | SpriteKey::BlockedSquare
                | SpriteKey::BlockedDouble
                | SpriteKey::DamageSkull
                | SpriteKey::FurnitureRat
                | SpriteKey::FurnitureSkull
                | SpriteKey::Die(_) => {}
            }
            let end = instances.len() as u32;
            if end > start {
                batches.push(SpriteBatch {
                    asset_index,
                    instances: start..end,
                });
            }
        }
        (instances, batches)
    }
}

impl DiceDecals {
    fn build_instances(
        &self,
        dice: &[DiePose],
        rolling_station: Option<usize>,
    ) -> (Vec<SpriteInstanceRaw>, Vec<DiceDecalBatch>) {
        let mut by_asset = vec![Vec::new(); self.assets.len()];
        let mut add_die = |center: Vec3, rotation: Quat, kind: DieKind, scale: f32| match kind {
            DieKind::Combat => {
                for face in 1..=6 {
                    let kind = combat_decal_kind(face);
                    if let Some(asset_index) =
                        self.assets.iter().position(|asset| asset.kind == kind)
                    {
                        by_asset[asset_index]
                            .push(combat_decal_instance(center, rotation, face, scale));
                    }
                }
            }
            DieKind::Movement => {
                if let Some(asset_index) = self
                    .assets
                    .iter()
                    .position(|asset| asset.kind == DiceDecalKind::MovementPip)
                {
                    for face in 1..=6 {
                        for &(x, z) in movement_pips(face) {
                            by_asset[asset_index]
                                .push(movement_pip_instance(center, rotation, face, x, z, scale));
                        }
                    }
                }
            }
        };

        for die in dice {
            add_die(die.translation, die.rotation, die.kind, 1.0);
        }
        for station in 0..4 {
            for die_index in 0..PLAYER_DICE_RACK.len() {
                if rack_die_is_rolling(station, die_index, rolling_station, dice) {
                    continue;
                }
                let (center, rotation, kind, scale) = player_station_die_pose(station, die_index);
                add_die(center, rotation, kind, scale);
            }
        }

        let mut instances = Vec::new();
        let mut batches = Vec::new();
        for (asset_index, asset_instances) in by_asset.into_iter().enumerate() {
            let start = instances.len() as u32;
            instances.extend(asset_instances);
            let end = instances.len() as u32;
            if end > start {
                batches.push(DiceDecalBatch {
                    asset_index,
                    instances: start..end,
                });
            }
        }
        (instances, batches)
    }
}

impl ComponentDecals {
    fn build_instances(
        &self,
        game: &Game,
        poses: &HashMap<UnitId, PiecePose>,
    ) -> (Vec<SpriteInstanceRaw>, Vec<ComponentDecalBatch>) {
        let mut instances = Vec::new();
        let mut batches = Vec::new();
        for (asset_index, asset) in self.assets.iter().enumerate() {
            let start = instances.len() as u32;
            match asset.key {
                ComponentDecalKey::Prop(kind) => {
                    for prop in &game.props {
                        if prop.kind != kind
                            || !game.is_prop_visible(prop)
                            || prop.carried_by.is_some()
                        {
                            continue;
                        }
                        if let Some(model) =
                            component_prop_decal(prop.kind, prop.pos, prop.rotation_quarters)
                        {
                            for layer in component_cardboard_layers(model) {
                                instances.push(SpriteInstanceRaw::new(layer));
                            }
                        }
                    }
                }
                ComponentDecalKey::DoorOpen
                | ComponentDecalKey::DoorClosed
                | ComponentDecalKey::SecretDoor => {
                    for door in &game.doors {
                        let matches = match asset.key {
                            ComponentDecalKey::DoorOpen => !door.secret && door.open,
                            ComponentDecalKey::DoorClosed => !door.secret && !door.open,
                            ComponentDecalKey::SecretDoor => door.secret && door.discovered,
                            _ => false,
                        };
                        if !matches || !game.is_door_visible(door) {
                            continue;
                        }
                        let center = (board_world(door.a) + board_world(door.b)) * 0.5;
                        let angle = if door.a.x != door.b.x {
                            std::f32::consts::FRAC_PI_2
                        } else {
                            0.0
                        };
                        if door.secret {
                            instances.push(SpriteInstanceRaw::new(horizontal_component_surface(
                                center, angle, 0.88, 0.88, 0.355,
                            )));
                        } else {
                            // This is one thin, double-sided cardboard insert
                            // in a low plastic stand. Alpha-cut layers give the
                            // punchboard a visible edge without filling the
                            // open arch or bringing back the old nested stone
                            // replacement frame.
                            for model in component_door_layers(center, angle) {
                                instances.push(SpriteInstanceRaw::new(model));
                            }
                        }
                    }
                }
                ComponentDecalKey::Trap(kind) => {
                    for trap in &game.traps {
                        if trap.kind != kind || !game.is_trap_marker_visible(trap) {
                            continue;
                        }
                        instances.push(SpriteInstanceRaw::new(horizontal_component_surface(
                            board_world(trap.pos),
                            0.0,
                            0.88,
                            0.88,
                            0.155,
                        )));
                    }
                }
                ComponentDecalKey::BlockedSquare | ComponentDecalKey::BlockedDouble => {
                    let wants_double = asset.key == ComponentDecalKey::BlockedDouble;
                    for placement in blocked_marker_placements(game) {
                        if placement.double == wants_double {
                            instances.push(SpriteInstanceRaw::new(horizontal_component_surface(
                                placement.center,
                                placement.angle,
                                if wants_double { 1.78 } else { 0.88 },
                                0.88,
                                0.155,
                            )));
                        }
                    }
                }
                ComponentDecalKey::DamageSkull => {
                    for unit in &game.units {
                        if !game.is_visible(unit) {
                            continue;
                        }
                        for center in
                            damage_marker_positions(piece_pose(poses, unit).translation, unit)
                        {
                            instances.push(SpriteInstanceRaw::new(horizontal_component_surface(
                                center, 0.0, 0.205, 0.205, 0.052,
                            )));
                        }
                    }
                }
            }
            let end = instances.len() as u32;
            if end > start {
                batches.push(ComponentDecalBatch {
                    asset_index,
                    instances: start..end,
                });
            }
        }
        (instances, batches)
    }
}

fn horizontal_component_surface(
    center: Vec3,
    angle: f32,
    width: f32,
    depth: f32,
    height: f32,
) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::new(width, depth, 1.0),
        Quat::from_rotation_y(angle) * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        center + Vec3::Y * height,
    )
}

fn vertical_component_surface(
    center: Vec3,
    angle: f32,
    width: f32,
    height: f32,
    center_y: f32,
    forward: f32,
) -> Mat4 {
    let rotation = Quat::from_rotation_y(angle);
    Mat4::from_scale_rotation_translation(
        Vec3::new(width, height, 1.0),
        rotation,
        center + Vec3::Y * center_y + rotation * Vec3::Z * forward,
    )
}

// One board square represents roughly one inch. This 0.048-unit total depth
// reads as about 1.2 mm of physical punchboard. Two clean printed faces avoid
// the alpha/depth interference produced by stacking many copies of a scan.
const CARDBOARD_HALF_THICKNESS: f32 = 0.024;
const CARDBOARD_LAYER_COUNT: usize = 2;

fn component_cardboard_layers(surface: Mat4) -> [Mat4; CARDBOARD_LAYER_COUNT] {
    let normal = surface.transform_vector3(Vec3::Z).normalize_or_zero();
    std::array::from_fn(|index| {
        let amount = index as f32 / (CARDBOARD_LAYER_COUNT - 1) as f32;
        let offset = -CARDBOARD_HALF_THICKNESS + amount * CARDBOARD_HALF_THICKNESS * 2.0;
        Mat4::from_translation(normal * offset) * surface
    })
}

fn component_door_layers(center: Vec3, angle: f32) -> [Mat4; CARDBOARD_LAYER_COUNT] {
    std::array::from_fn(|index| {
        let amount = index as f32 / (CARDBOARD_LAYER_COUNT - 1) as f32;
        let offset = -CARDBOARD_HALF_THICKNESS + amount * CARDBOARD_HALF_THICKNESS * 2.0;
        if offset < 0.0 {
            vertical_component_surface(
                center,
                angle + std::f32::consts::PI,
                0.94,
                1.38,
                0.72,
                -offset,
            )
        } else {
            vertical_component_surface(center, angle, 0.94, 1.38, 0.72, offset)
        }
    })
}

fn component_prop_decal(kind: PropKind, pos: Pos, rotation_quarters: u8) -> Option<Mat4> {
    let center = prop_world_position(kind, pos, rotation_quarters, false);
    let angle = -(rotation_quarters as f32) * std::f32::consts::FRAC_PI_2;
    Some(match kind {
        // The original-US staircase is a flat 2x2 punchboard cutout, cropped
        // from tile-sheet page 1. It must not be replaced by a relief model.
        PropKind::Stairs => horizontal_component_surface(center, angle, 1.92, 1.92, 0.082),
        // These offsets hug the normalized GLB surfaces. The previous values
        // were presentation guesses (the table face was 0.35 units above its
        // top), which made the scans look like detached floating cards.
        PropKind::Table => horizontal_component_surface(center, angle, 2.02, 0.42, 0.74),
        PropKind::Chest => vertical_component_surface(center, angle, 0.88, 0.25, 0.50, 0.35),
        PropKind::Bookcase => vertical_component_surface(center, angle, 1.28, 0.82, 0.45, 0.36),
        PropKind::Throne => vertical_component_surface(center, angle, 0.82, 0.45, 0.92, 0.55),
        PropKind::AlchemistsBench => {
            vertical_component_surface(center, angle, 1.62, 0.24, 0.59, 0.50)
        }
        PropKind::Tomb => vertical_component_surface(center, angle, 1.72, 0.30, 0.54, 0.54),
        PropKind::SorcerersTable => {
            vertical_component_surface(center, angle, 1.32, 0.25, 0.59, 0.42)
        }
        PropKind::TortureRack => horizontal_component_surface(center, angle, 1.62, 0.34, 0.94),
        PropKind::Fireplace => vertical_component_surface(center, angle, 1.62, 1.54, 0.84, 0.29),
        PropKind::Cupboard => vertical_component_surface(center, angle, 1.32, 0.52, 0.88, 0.28),
        PropKind::WeaponRack | PropKind::StarOfWest => return None,
    })
}

struct ScannedBoard {
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    path: PathBuf,
}

struct InformationScreen {
    _front_texture: wgpu::Texture,
    _back_texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    front_path: PathBuf,
    back_path: PathBuf,
}

struct StartupTexture {
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

struct StartupScene {
    pipeline: wgpu::RenderPipeline,
    quad_vertex_buffer: wgpu::Buffer,
    quad_index_buffer: wgpu::Buffer,
    quad_index_count: u32,
    box_vertex_buffer: wgpu::Buffer,
    box_index_buffer: wgpu::Buffer,
    box_index_ranges: [std::ops::Range<u32>; 6],
    box_faces: Vec<StartupTexture>,
    quest_pages: Vec<image::RgbaImage>,
    hero_cards: Vec<image::RgbaImage>,
    armory_page: image::RgbaImage,
    panel_texture: wgpu::Texture,
    panel_bind_group: wgpu::BindGroup,
    panel_state: Option<StartupFlow>,
}

struct PlayerStations {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    fixed_surfaces: Vec<TexturedSurface>,
    elf_spell_ranges: [std::ops::Range<u32>; 3],
    wizard_spell_ranges: [std::ops::Range<u32>; 9],
    elf_discard_range: std::ops::Range<u32>,
    wizard_discard_range: std::ops::Range<u32>,
    spell_texture_base: usize,
    textures: Vec<StartupTexture>,
}

struct TexturedSurface {
    texture_index: usize,
    index_range: std::ops::Range<u32>,
}

struct ZargonTabletop {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    fixed_surfaces: Vec<TexturedSurface>,
    book_ranges: [std::ops::Range<u32>; 2],
    chaos_discard_range: std::ops::Range<u32>,
    chaos_texture_base: usize,
    quest_texture_base: usize,
    textures: Vec<StartupTexture>,
}

#[derive(Debug, Deserialize)]
struct BoardCalibration {
    playable_bounds_px: PixelBounds,
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct PixelBounds {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BoardPlane {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vec3,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            yaw: -0.68,
            pitch: 0.45,
            distance: 74.0,
            target: Vec3::new(0.0, -2.5, 3.0),
        }
    }
}

impl Camera {
    fn eye(&self) -> Vec3 {
        let direction = Vec3::new(
            self.pitch.cos() * self.yaw.cos(),
            self.pitch.sin(),
            self.pitch.cos() * self.yaw.sin(),
        );
        self.target + direction * self.distance
    }

    fn matrix(&self, aspect: f32) -> Mat4 {
        let eye = self.eye();
        glam::camera::rh::proj::directx::perspective(
            45.0_f32.to_radians(),
            aspect.max(0.01),
            0.1,
            600.0,
        ) * glam::camera::rh::view::look_at_mat4(eye, self.target, Vec3::Y)
    }

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw = (self.yaw - dx * 0.008 + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        self.pitch = (self.pitch + dy * 0.006).clamp(0.03, 1.53);
    }

    pub fn zoom(&mut self, delta: f32) {
        self.distance = (self.distance - delta * 1.3).clamp(15.0, 220.0);
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    fn screen_relative_direction(&self, intent: Direction, aspect: f32) -> Direction {
        let matrix = self.matrix(aspect);
        let center = matrix.project_point3(self.target);
        let intent = match intent {
            Direction::North => glam::Vec2::Y,
            Direction::East => glam::Vec2::X,
            Direction::South => -glam::Vec2::Y,
            Direction::West => -glam::Vec2::X,
        };
        Direction::ALL
            .into_iter()
            .max_by(|left, right| {
                let score = |direction| {
                    let world = direction_world_delta(direction);
                    let projected = matrix.project_point3(self.target + world);
                    let screen = glam::Vec2::new(projected.x - center.x, projected.y - center.y)
                        .normalize_or_zero();
                    screen.dot(intent)
                };
                score(*left).total_cmp(&score(*right))
            })
            .expect("the board always has four cardinal directions")
    }
}

struct DepthTarget {
    view: wgpu::TextureView,
}

impl DepthTarget {
    fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth texture"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        Self {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
        }
    }
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    highlight_pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    instance_buffer: wgpu::Buffer,
    highlight_instance_buffer: wgpu::Buffer,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    depth: DepthTarget,
    environment_backdrop: Option<EnvironmentBackdrop>,
    environment_mesh: Option<EnvironmentMesh>,
    scanned_board: Option<ScannedBoard>,
    information_screen: Option<InformationScreen>,
    sprite_art: Option<SpriteArt>,
    piece_models: PieceModels,
    player_stations: Option<PlayerStations>,
    zargon_tabletop: Option<ZargonTabletop>,
    component_decals: Option<ComponentDecals>,
    dice_decals: Option<DiceDecals>,
    startup_scene: Option<StartupScene>,
    hud_overlay: HudOverlay,
    piece_animations: PieceAnimations,
    action_camera: ActionCameraDirector,
    dice_roll_transform: DiceRollTransform,
    dice_roll_active: bool,
    selection_highlights: Vec<Pos>,
    hovered_move_target: Option<Pos>,
    pub camera: Camera,
    animation_started: Instant,
}

impl Renderer {
    pub fn new(window: &Window) -> Result<Self> {
        let (width, height) = window.size();
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = create_surface(&instance, window).map_err(|error| anyhow!(error))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        }))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("HeroQuest wgpu device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            }))?;

        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(capabilities.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            desired_maximum_frame_latency: 2,
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let (vertices, indices) = cube_mesh();
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cube vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cube indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene instances"),
            size: (MAX_INSTANCES * std::mem::size_of::<InstanceRaw>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let highlight_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("translucent tabletop highlight instances"),
            size: (MAX_INSTANCES * std::mem::size_of::<InstanceRaw>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let camera = Camera::default();
        let camera_uniform = CameraUniform {
            view_projection: camera
                .matrix(config.width as f32 / config.height as f32)
                .to_cols_array_2d(),
            animation: [0.0; 4],
        };
        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera uniform"),
            contents: bytemuck::bytes_of(&camera_uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera bind group"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lit cube shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader.wgsl"))),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("main pipeline layout"),
            bind_group_layouts: &[Some(&camera_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("main render pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(Vertex::layout()), Some(InstanceRaw::layout())],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let highlight_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("translucent tabletop highlight pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(Vertex::layout()), Some(InstanceRaw::layout())],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_highlight"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let environment_backdrop =
            load_environment_backdrop(&device, &queue, &camera_layout, format)?;
        if let Some(backdrop) = &environment_backdrop {
            log::info!("using spherical room panorama {}", backdrop.path.display());
        }
        let environment_mesh = load_environment_mesh(&device, &queue, &camera_layout, format)?;
        if let Some(room) = &environment_mesh {
            log::info!("using castle-room model {}", room.path.display());
        } else {
            log::info!(
                "no castle-room GLB found; using the procedural room (run blender --background --python tools/build-castle-room.py)"
            );
        }
        let scanned_board = load_scanned_board(&device, &queue, &camera_layout, format)?;
        if let Some(board) = &scanned_board {
            log::info!("using local board scan {}", board.path.display());
        } else {
            log::info!("no local board scan found; using the procedural board");
        }
        let information_screen = load_information_screen(&device, &queue, &camera_layout, format)?;
        if let Some(screen) = &information_screen {
            log::info!(
                "using original-US Information Screen {} / {}",
                screen.front_path.display(),
                screen.back_path.display()
            );
        } else {
            log::info!(
                "no local Information Screen textures found; run tools/extract-original-us-screen.sh"
            );
        }
        let sprite_art = load_sprite_art(&device, &queue, &camera_layout, format)?;
        if let Some(art) = &sprite_art {
            log::info!(
                "loaded {} local figure/furniture art slots",
                art.assets.len()
            );
            for asset in &art.assets {
                log::debug!("using local art {}", asset.path.display());
            }
        } else {
            log::info!("no optional figure/furniture cutouts found; using required classic GLBs");
        }
        let piece_models = load_piece_models(&device)?;
        log::info!(
            "loaded all {} required real classic piece models",
            piece_models.assets.len()
        );
        for asset in &piece_models.assets {
            log::debug!("using local piece model {}", asset.path.display());
        }
        let player_stations = load_player_stations(&device, &queue, &camera_layout, format)?;
        if player_stations.is_some() {
            log::info!(
                "loaded four original-US Hero stations with Character Cards, record sheets, action references, Armory, and elemental spell hands"
            );
        } else {
            log::info!("no Hero station scans found for the four in-world player stations");
        }
        let zargon_tabletop = load_zargon_tabletop(&device, &queue, &camera_layout, format)?;
        if zargon_tabletop.is_some() {
            log::info!(
                "loaded Zargon's original-US Treasure, Artifact, Dread/Chaos, and Monster cards plus the scan-backed Quest Book"
            );
        } else {
            log::info!("no original-US cards or Quest Book pages found for Zargon's station");
        }
        let component_decals = load_component_decals(&device, &queue, &camera_layout, format)?;
        if let Some(decals) = &component_decals {
            log::info!(
                "loaded {} scan-derived original-US door, furniture, and marker faces",
                decals.assets.len()
            );
            for asset in &decals.assets {
                log::debug!("using original-US component face {}", asset.path.display());
            }
        } else {
            log::info!(
                "no original-US component faces found; run tools/extract-original-us-components.sh"
            );
        }
        let dice_decals = load_dice_decals(&device, &queue, &camera_layout, format)?;
        if let Some(decals) = &dice_decals {
            for asset in &decals.assets {
                log::info!(
                    "using original-US {:?} die decal {}",
                    asset.kind,
                    asset.path.display()
                );
            }
        } else {
            log::info!(
                "no original-US die decals found; run tools/extract-original-us-dice-decals.sh"
            );
        }
        let startup_scene = load_startup_scene(&device, &queue, &camera_layout, format)?;
        if startup_scene.is_some() {
            log::info!("loaded original-US 3D box and pre-game setup art");
        } else {
            log::info!("no startup art found; run tools/extract-original-us-startup-art.sh");
        }
        let depth = DepthTarget::new(&device, config.width, config.height);
        let hud_overlay = create_hud_overlay(&device, &queue, format);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            highlight_pipeline,
            vertex_buffer,
            index_buffer,
            index_count: indices.len() as u32,
            instance_buffer,
            highlight_instance_buffer,
            camera_buffer,
            camera_bind_group,
            depth,
            environment_backdrop,
            environment_mesh,
            scanned_board,
            information_screen,
            sprite_art,
            piece_models,
            player_stations,
            zargon_tabletop,
            component_decals,
            dice_decals,
            startup_scene,
            hud_overlay,
            piece_animations: PieceAnimations::default(),
            action_camera: ActionCameraDirector::new(),
            dice_roll_transform: DiceRollTransform {
                center: ZARGON_DICE_ROLL_CENTER,
                rotation: Quat::from_rotation_y(std::f32::consts::PI),
                station: None,
            },
            dice_roll_active: false,
            selection_highlights: Vec::new(),
            hovered_move_target: None,
            camera,
            animation_started: Instant::now(),
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth = DepthTarget::new(&self.device, width, height);
    }

    pub fn focus_unit(&mut self, unit: UnitId) {
        self.action_camera.focus_unit(unit);
    }

    pub fn focus_combat(&mut self, attacker: UnitId, defender: UnitId) {
        self.action_camera.focus_combat(attacker, defender);
    }

    pub fn focus_board_pos(&mut self, pos: Pos) {
        self.action_camera.focus_board(board_world(pos));
    }

    pub fn set_selection_highlights(&mut self, positions: Vec<Pos>) {
        self.selection_highlights = positions;
    }

    pub fn set_hovered_move_target(&mut self, target: Option<Pos>) {
        self.hovered_move_target = target;
    }

    pub fn focus_dice(&mut self, game: &Game, roller: Option<UnitId>) {
        self.dice_roll_transform = dice_roll_transform(game, roller);
        self.dice_roll_active = true;
        self.action_camera
            .focus_dice(self.dice_roll_transform.center);
    }

    pub fn finish_dice_roll(&mut self) {
        self.dice_roll_active = false;
    }

    pub fn focus_active_hero(&mut self, game: &Game) {
        if let Some(hero) = game.active_mover_id() {
            self.focus_unit(hero);
        }
    }

    pub fn camera_relative_direction(&self, screen_direction: Direction) -> Direction {
        self.camera.screen_relative_direction(
            screen_direction,
            self.config.width as f32 / self.config.height.max(1) as f32,
        )
    }

    pub fn board_pos_at_screen(
        &self,
        x: f32,
        y: f32,
        input_width: u32,
        input_height: u32,
    ) -> Option<Pos> {
        board_pos_at_screen(
            &self.camera,
            self.config.width as f32 / self.config.height.max(1) as f32,
            x,
            y,
            input_width,
            input_height,
        )
    }

    pub fn hero_spell_target_at_screen(
        &self,
        game: &Game,
        spell: HeroSpell,
        x: f32,
        y: f32,
        input_width: u32,
        input_height: u32,
    ) -> Option<HeroSpellTarget> {
        hero_spell_target_at_screen(
            &self.camera,
            game,
            spell,
            self.config.width as f32 / self.config.height.max(1) as f32,
            x,
            y,
            input_width,
            input_height,
        )
    }

    /// Returns the physical card, sheet, deck, or book under a window-space
    /// pointer. The same world-space dimensions used to draw every scanned
    /// surface are used here, so interactions remain aligned while orbiting,
    /// zooming, or resizing the window.
    pub fn tabletop_surface_at_screen(
        &self,
        game: &Game,
        x: f32,
        y: f32,
        input_width: u32,
        input_height: u32,
    ) -> Option<TabletopSurface> {
        let target = tabletop_hit_target_at_screen(
            &self.camera,
            self.config.width as f32 / self.config.height.max(1) as f32,
            x,
            y,
            input_width,
            input_height,
        )?;
        match target {
            TabletopHitTarget::Player(surface) => self.player_stations.as_ref().map(|_| surface),
            TabletopHitTarget::ElfSpell(slot) => {
                self.player_stations.as_ref()?;
                let group = self
                    .startup_scene
                    .as_ref()
                    .and_then(|scene| scene.panel_state.as_ref())
                    .map_or(SpellGroup::Earth, |flow| flow.elf_spells);
                let spell = group.spells()[slot];
                game.units
                    .iter()
                    .find(|unit| unit.figure == FigureKind::Hero(HeroKind::Elf))
                    .filter(|unit| unit.hero_spells.contains(&spell))
                    .map(|_| TabletopSurface::HeroSpellCard {
                        hero: HeroKind::Elf,
                        spell,
                    })
            }
            TabletopHitTarget::WizardSpell(slot) => {
                self.player_stations.as_ref()?;
                let groups = self
                    .startup_scene
                    .as_ref()
                    .and_then(|scene| scene.panel_state.as_ref())
                    .map_or_else(
                        || vec![SpellGroup::Air, SpellGroup::Fire, SpellGroup::Water],
                        StartupFlow::wizard_spells,
                    );
                let spell = groups[slot / 3].spells()[slot % 3];
                game.units
                    .iter()
                    .find(|unit| unit.figure == FigureKind::Hero(HeroKind::Wizard))
                    .filter(|unit| unit.hero_spells.contains(&spell))
                    .map(|_| TabletopSurface::HeroSpellCard {
                        hero: HeroKind::Wizard,
                        spell,
                    })
            }
            TabletopHitTarget::ElfDiscard => {
                self.player_stations.as_ref()?;
                game.units
                    .iter()
                    .find(|unit| unit.figure == FigureKind::Hero(HeroKind::Elf))
                    .and_then(|unit| {
                        unit.discarded_hero_spells.last().copied().map(|spell| {
                            TabletopSurface::HeroSpellDiscard {
                                hero: HeroKind::Elf,
                                spell,
                                count: unit.discarded_hero_spells.len(),
                            }
                        })
                    })
            }
            TabletopHitTarget::WizardDiscard => {
                self.player_stations.as_ref()?;
                game.units
                    .iter()
                    .find(|unit| unit.figure == FigureKind::Hero(HeroKind::Wizard))
                    .and_then(|unit| {
                        unit.discarded_hero_spells.last().copied().map(|spell| {
                            TabletopSurface::HeroSpellDiscard {
                                hero: HeroKind::Wizard,
                                spell,
                                count: unit.discarded_hero_spells.len(),
                            }
                        })
                    })
            }
            TabletopHitTarget::ChaosDiscard => {
                self.zargon_tabletop.as_ref()?;
                game.discarded_chaos_spells.last().copied().map(|spell| {
                    TabletopSurface::ChaosSpellDiscard {
                        spell,
                        count: game.discarded_chaos_spells.len(),
                    }
                })
            }
            TabletopHitTarget::Zargon(surface) => self.zargon_tabletop.as_ref().map(|_| surface),
        }
    }

    /// Returns the OSD action button under a window-space pointer. The HUD is
    /// a full-window texture, so the same deterministic layout drives drawing
    /// and hit testing at every window size.
    pub fn hud_action_at_screen(
        &self,
        x: f32,
        y: f32,
        input_width: u32,
        input_height: u32,
    ) -> Option<String> {
        if input_width == 0 || input_height == 0 {
            return None;
        }
        let state = self.hud_overlay.last_state.as_ref()?;
        let hud_x = x * HUD_TEXTURE_WIDTH as f32 / input_width as f32;
        let hud_y = y * HUD_TEXTURE_HEIGHT as f32 / input_height as f32;
        hud_action_button_rects(state.actions.len())
            .into_iter()
            .enumerate()
            .find_map(|(index, (left, top, width, height))| {
                (hud_x >= left as f32
                    && hud_x < (left + width) as f32
                    && hud_y >= top as f32
                    && hud_y < (top + height) as f32)
                    .then(|| state.actions[index].clone())
            })
    }

    pub fn render(&mut self, game: &Game, dice: &[DiePose], overlay: &GameOverlay) -> Result<()> {
        self.hud_overlay.update(&self.queue, overlay);
        let elapsed = self.animation_started.elapsed().as_secs_f32();
        let piece_poses = self.piece_animations.poses(game, elapsed);
        self.action_camera
            .update(&mut self.camera, game, &piece_poses);
        let world_dice = dice
            .iter()
            .copied()
            .map(|pose| self.dice_roll_transform.world_pose(pose))
            .collect::<Vec<_>>();
        let rolling_station = self
            .dice_roll_active
            .then_some(self.dice_roll_transform.station)
            .flatten();
        let instances = build_scene(
            game,
            self.environment_mesh.is_some(),
            self.scanned_board.is_some(),
        );
        let highlight_instances = build_highlights(
            game,
            &self.selection_highlights,
            self.hovered_move_target,
            elapsed,
        );
        if instances.len() > MAX_INSTANCES {
            return Err(anyhow!(
                "scene has {} instances, exceeding {MAX_INSTANCES}",
                instances.len()
            ));
        }
        self.queue
            .write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&instances));
        if highlight_instances.len() > MAX_INSTANCES {
            return Err(anyhow!(
                "scene has {} translucent highlights, exceeding {MAX_INSTANCES}",
                highlight_instances.len()
            ));
        }
        if !highlight_instances.is_empty() {
            self.queue.write_buffer(
                &self.highlight_instance_buffer,
                0,
                bytemuck::cast_slice(&highlight_instances),
            );
        }
        let (sprite_instances, sprite_batches) = self
            .sprite_art
            .as_ref()
            .map(|art| art.build_instances(game, &self.camera, &self.piece_models, &piece_poses))
            .unwrap_or_default();
        let piece_instances =
            self.piece_models
                .build_instances(game, &piece_poses, &world_dice, rolling_station);
        for (asset, instances) in self.piece_models.assets.iter().zip(&piece_instances) {
            if instances.len() > MAX_SPRITE_INSTANCES {
                return Err(anyhow!(
                    "piece model {} has too many instances",
                    asset.path.display()
                ));
            }
            if !instances.is_empty() {
                self.queue
                    .write_buffer(&asset.instance_buffer, 0, bytemuck::cast_slice(instances));
            }
        }
        if sprite_instances.len() > MAX_SPRITE_INSTANCES {
            return Err(anyhow!(
                "scene has {} art instances, exceeding {MAX_SPRITE_INSTANCES}",
                sprite_instances.len()
            ));
        }
        if let Some(art) = &self.sprite_art
            && !sprite_instances.is_empty()
        {
            self.queue.write_buffer(
                &art.instance_buffer,
                0,
                bytemuck::cast_slice(&sprite_instances),
            );
        }
        let (component_decal_instances, component_decal_batches) = self
            .component_decals
            .as_ref()
            .map(|decals| decals.build_instances(game, &piece_poses))
            .unwrap_or_default();
        if component_decal_instances.len() > MAX_SPRITE_INSTANCES {
            return Err(anyhow!(
                "scene has {} component-face decals, exceeding {MAX_SPRITE_INSTANCES}",
                component_decal_instances.len()
            ));
        }
        if let Some(decals) = &self.component_decals
            && !component_decal_instances.is_empty()
        {
            self.queue.write_buffer(
                &decals.instance_buffer,
                0,
                bytemuck::cast_slice(&component_decal_instances),
            );
        }
        let (dice_decal_instances, dice_decal_batches) = self
            .dice_decals
            .as_ref()
            .map(|decals| {
                decals.build_instances(
                    &world_dice,
                    self.dice_roll_active
                        .then_some(self.dice_roll_transform.station)
                        .flatten(),
                )
            })
            .unwrap_or_default();
        if dice_decal_instances.len() > MAX_SPRITE_INSTANCES {
            return Err(anyhow!(
                "scene has {} die-face decals, exceeding {MAX_SPRITE_INSTANCES}",
                dice_decal_instances.len()
            ));
        }
        if let Some(decals) = &self.dice_decals
            && !dice_decal_instances.is_empty()
        {
            self.queue.write_buffer(
                &decals.instance_buffer,
                0,
                bytemuck::cast_slice(&dice_decal_instances),
            );
        }
        let aspect = self.config.width as f32 / self.config.height.max(1) as f32;
        let camera_uniform = CameraUniform {
            view_projection: self
                .camera
                .matrix(self.config.width as f32 / self.config.height as f32)
                .to_cols_array_2d(),
            animation: [elapsed, self.camera.yaw, self.camera.pitch, aspect],
        };
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            other => return Err(anyhow!("failed to acquire surface texture: {other:?}")),
        };
        let output = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("main encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.018,
                            g: 0.014,
                            b: 0.020,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(backdrop) = &self.environment_backdrop {
                pass.set_pipeline(&backdrop.pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_bind_group(1, &backdrop.bind_group, &[]);
                pass.set_vertex_buffer(0, backdrop.vertex_buffer.slice(..));
                pass.set_index_buffer(backdrop.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..backdrop.index_count, 0, 0..1);
            }
            if let Some(room) = &self.environment_mesh {
                pass.set_pipeline(&room.pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                for primitive in &room.primitives {
                    pass.set_bind_group(
                        1,
                        &room.materials[primitive.material_index].bind_group,
                        &[],
                    );
                    pass.set_vertex_buffer(0, primitive.vertex_buffer.slice(..));
                    pass.set_index_buffer(
                        primitive.index_buffer.slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    pass.draw_indexed(0..primitive.index_count, 0, 0..1);
                }
            }
            if let Some(board) = &self.scanned_board {
                pass.set_pipeline(&board.pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_bind_group(1, &board.bind_group, &[]);
                pass.set_vertex_buffer(0, board.vertex_buffer.slice(..));
                pass.set_index_buffer(board.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..board.index_count, 0, 0..1);
            }
            if let Some(stations) = &self.player_stations {
                pass.set_pipeline(&stations.pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_vertex_buffer(0, stations.vertex_buffer.slice(..));
                pass.set_index_buffer(stations.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                for surface in &stations.fixed_surfaces {
                    pass.set_bind_group(
                        1,
                        &stations.textures[surface.texture_index].bind_group,
                        &[],
                    );
                    pass.draw_indexed(surface.index_range.clone(), 0, 0..1);
                }

                let startup = self
                    .startup_scene
                    .as_ref()
                    .and_then(|scene| scene.panel_state.as_ref());
                let elf_group = startup.map_or(SpellGroup::Earth, |flow| flow.elf_spells);
                let elf_spells = game
                    .units
                    .iter()
                    .find(|unit| unit.figure == FigureKind::Hero(HeroKind::Elf))
                    .map(|unit| unit.hero_spells.as_slice())
                    .unwrap_or(&[]);
                for (card, range) in stations.elf_spell_ranges.iter().enumerate() {
                    if !elf_spells.contains(&elf_group.spells()[card]) {
                        continue;
                    }
                    let texture_index =
                        stations.spell_texture_base + spell_group_index(elf_group) * 3 + card;
                    pass.set_bind_group(1, &stations.textures[texture_index].bind_group, &[]);
                    pass.draw_indexed(range.clone(), 0, 0..1);
                }
                let wizard_groups = startup.map_or_else(
                    || vec![SpellGroup::Air, SpellGroup::Fire, SpellGroup::Water],
                    StartupFlow::wizard_spells,
                );
                let wizard_spells = game
                    .units
                    .iter()
                    .find(|unit| unit.figure == FigureKind::Hero(HeroKind::Wizard))
                    .map(|unit| unit.hero_spells.as_slice())
                    .unwrap_or(&[]);
                for (slot, range) in stations.wizard_spell_ranges.iter().enumerate() {
                    let group = wizard_groups[slot / 3];
                    if !wizard_spells.contains(&group.spells()[slot % 3]) {
                        continue;
                    }
                    let texture_index =
                        stations.spell_texture_base + spell_group_index(group) * 3 + slot % 3;
                    pass.set_bind_group(1, &stations.textures[texture_index].bind_group, &[]);
                    pass.draw_indexed(range.clone(), 0, 0..1);
                }
                for (hero, range) in [
                    (HeroKind::Elf, &stations.elf_discard_range),
                    (HeroKind::Wizard, &stations.wizard_discard_range),
                ] {
                    let Some(spell) = game
                        .units
                        .iter()
                        .find(|unit| unit.figure == FigureKind::Hero(hero))
                        .and_then(|unit| unit.discarded_hero_spells.last())
                        .copied()
                    else {
                        continue;
                    };
                    let Some(offset) = hero_spell_texture_offset(spell) else {
                        continue;
                    };
                    pass.set_bind_group(
                        1,
                        &stations.textures[stations.spell_texture_base + offset].bind_group,
                        &[],
                    );
                    pass.draw_indexed(range.clone(), 0, 0..1);
                }
            }
            if let Some(tabletop) = &self.zargon_tabletop {
                pass.set_pipeline(&tabletop.pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_vertex_buffer(0, tabletop.vertex_buffer.slice(..));
                pass.set_index_buffer(tabletop.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                for surface in &tabletop.fixed_surfaces {
                    pass.set_bind_group(
                        1,
                        &tabletop.textures[surface.texture_index].bind_group,
                        &[],
                    );
                    pass.draw_indexed(surface.index_range.clone(), 0, 0..1);
                }
                if let Some(spell) = game.discarded_chaos_spells.last()
                    && let Some(offset) = ORIGINAL_US_CHAOS_SPELLS
                        .iter()
                        .position(|candidate| candidate == spell)
                {
                    pass.set_bind_group(
                        1,
                        &tabletop.textures[tabletop.chaos_texture_base + offset].bind_group,
                        &[],
                    );
                    pass.draw_indexed(tabletop.chaos_discard_range.clone(), 0, 0..1);
                }
                let quest_index = QUEST_TITLES
                    .iter()
                    .position(|title| *title == game.title)
                    .unwrap_or_default();
                let loaded_quest_count = tabletop.textures.len() - tabletop.quest_texture_base;
                let quest_texture = &tabletop.textures
                    [tabletop.quest_texture_base + quest_index.min(loaded_quest_count - 1)];
                pass.set_bind_group(1, &quest_texture.bind_group, &[]);
                for range in &tabletop.book_ranges {
                    pass.draw_indexed(range.clone(), 0, 0..1);
                }
            }
            if let Some(screen) = &self.information_screen {
                pass.set_pipeline(&screen.pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_bind_group(1, &screen.bind_group, &[]);
                pass.set_vertex_buffer(0, screen.vertex_buffer.slice(..));
                pass.set_index_buffer(screen.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..screen.index_count, 0, 0..1);
            }
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
            pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..self.index_count, 0, 0..instances.len() as u32);
            if let Some(decals) = &self.dice_decals {
                pass.set_pipeline(&decals.pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_vertex_buffer(0, decals.vertex_buffer.slice(..));
                pass.set_vertex_buffer(1, decals.instance_buffer.slice(..));
                pass.set_index_buffer(decals.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                for batch in &dice_decal_batches {
                    pass.set_bind_group(
                        1,
                        &decals.assets[batch.asset_index].texture.bind_group,
                        &[],
                    );
                    pass.draw_indexed(0..decals.index_count, 0, batch.instances.clone());
                }
            }
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            for (asset, instances) in self.piece_models.assets.iter().zip(&piece_instances) {
                if instances.is_empty() {
                    continue;
                }
                pass.set_vertex_buffer(1, asset.instance_buffer.slice(..));
                for primitive in &asset.primitives {
                    pass.set_vertex_buffer(0, primitive.vertex_buffer.slice(..));
                    pass.set_index_buffer(
                        primitive.index_buffer.slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    pass.draw_indexed(0..primitive.index_count, 0, 0..instances.len() as u32);
                }
            }
            if let Some(decals) = &self.component_decals {
                pass.set_pipeline(&decals.pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_vertex_buffer(0, decals.vertex_buffer.slice(..));
                pass.set_vertex_buffer(1, decals.instance_buffer.slice(..));
                pass.set_index_buffer(decals.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                for batch in &component_decal_batches {
                    pass.set_bind_group(
                        1,
                        &decals.assets[batch.asset_index].texture.bind_group,
                        &[],
                    );
                    pass.draw_indexed(0..decals.index_count, 0, batch.instances.clone());
                }
            }
            if let Some(art) = &self.sprite_art {
                pass.set_pipeline(&art.pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_vertex_buffer(0, art.vertex_buffer.slice(..));
                pass.set_vertex_buffer(1, art.instance_buffer.slice(..));
                pass.set_index_buffer(art.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                for batch in &sprite_batches {
                    pass.set_bind_group(1, &art.assets[batch.asset_index].bind_group, &[]);
                    pass.draw_indexed(0..art.index_count, 0, batch.instances.clone());
                }
            }
            if !highlight_instances.is_empty() {
                pass.set_pipeline(&self.highlight_pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                pass.set_vertex_buffer(1, self.highlight_instance_buffer.slice(..));
                pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..self.index_count, 0, 0..highlight_instances.len() as u32);
            }
            pass.set_pipeline(&self.hud_overlay.pipeline);
            pass.set_bind_group(0, &self.hud_overlay.bind_group, &[]);
            pass.set_vertex_buffer(0, self.hud_overlay.vertex_buffer.slice(..));
            pass.set_index_buffer(
                self.hud_overlay.index_buffer.slice(..),
                wgpu::IndexFormat::Uint16,
            );
            pass.draw_indexed(0..6, 0, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        Ok(())
    }

    pub fn render_startup(
        &mut self,
        startup: &StartupFlow,
        campaign: &Campaign,
        elapsed: f32,
    ) -> Result<()> {
        let aspect = self.config.width as f32 / self.config.height as f32;
        let startup_camera = match startup.stage {
            StartupStage::Box => {
                let yaw = 0.52 + elapsed * 0.13;
                let pitch = 0.36 + (elapsed * 0.35).sin() * 0.025;
                let distance = if aspect < 1.0 { 23.0 / aspect } else { 23.0 };
                let direction = Vec3::new(
                    pitch.cos() * yaw.cos(),
                    pitch.sin(),
                    pitch.cos() * yaw.sin(),
                );
                let target = Vec3::new(0.0, 0.25, 0.0);
                glam::camera::rh::proj::directx::perspective(
                    39.0_f32.to_radians(),
                    aspect.max(0.01),
                    0.1,
                    120.0,
                ) * glam::camera::rh::view::look_at_mat4(
                    target + direction * distance,
                    target,
                    Vec3::Y,
                )
            }
            _ => {
                let (horizontal, vertical) = if aspect >= 1.0 {
                    (10.0 * aspect, 10.0)
                } else {
                    (10.0, 10.0 / aspect.max(0.01))
                };
                glam::camera::rh::proj::directx::orthographic(
                    -horizontal,
                    horizontal,
                    -vertical,
                    vertical,
                    0.0,
                    40.0,
                ) * glam::camera::rh::view::look_at_mat4(
                    Vec3::new(0.0, 0.0, 20.0),
                    Vec3::ZERO,
                    Vec3::Y,
                )
            }
        };
        let camera_uniform = CameraUniform {
            view_projection: startup_camera.to_cols_array_2d(),
            animation: [elapsed, 0.0, 0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));

        let Some(scene) = &mut self.startup_scene else {
            return self.render_startup_fallback(startup);
        };
        let mut panel_state = startup.clone();
        panel_state.hovered = None;
        if !matches!(startup.stage, StartupStage::Box | StartupStage::Playing)
            && scene.panel_state.as_ref() != Some(&panel_state)
        {
            update_startup_panel(&self.queue, scene, &panel_state, campaign)?;
            scene.panel_state = Some(panel_state);
        }

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            other => return Err(anyhow!("failed to acquire surface texture: {other:?}")),
        };
        let output = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("startup encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("startup pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.014,
                            g: 0.008,
                            b: 0.010,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&scene.pipeline);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            match startup.stage {
                StartupStage::Box => {
                    pass.set_vertex_buffer(0, scene.box_vertex_buffer.slice(..));
                    pass.set_index_buffer(
                        scene.box_index_buffer.slice(..),
                        wgpu::IndexFormat::Uint16,
                    );
                    for (texture, range) in
                        scene.box_faces.iter().zip(scene.box_index_ranges.iter())
                    {
                        pass.set_bind_group(1, &texture.bind_group, &[]);
                        pass.draw_indexed(range.clone(), 0, 0..1);
                    }
                }
                StartupStage::Armory | StartupStage::QuestSelection => {
                    draw_startup_quad(&mut pass, scene, &scene.panel_bind_group);
                }
                StartupStage::PlayerSetup => {
                    draw_startup_quad(&mut pass, scene, &scene.panel_bind_group);
                }
                StartupStage::WizardSpellChoice
                | StartupStage::ElfSpellChoice
                | StartupStage::Ready => {
                    draw_startup_quad(&mut pass, scene, &scene.panel_bind_group);
                }
                StartupStage::Playing => {}
            }
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        Ok(())
    }

    fn render_startup_fallback(&mut self, _startup: &StartupFlow) -> Result<()> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            _ => return Ok(()),
        };
        let output = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("startup fallback encoder"),
            });
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("startup fallback pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &output,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.05,
                        g: 0.018,
                        b: 0.018,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        }));
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        Ok(())
    }
}

impl HudOverlay {
    fn update(&mut self, queue: &wgpu::Queue, state: &GameOverlay) {
        if self.last_state.as_ref() == Some(state) {
            return;
        }
        let canvas = draw_hud_overlay(state);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            canvas.as_raw(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * HUD_TEXTURE_WIDTH),
                rows_per_image: Some(HUD_TEXTURE_HEIGHT),
            },
            wgpu::Extent3d {
                width: HUD_TEXTURE_WIDTH,
                height: HUD_TEXTURE_HEIGHT,
                depth_or_array_layers: 1,
            },
        );
        self.last_state = Some(state.clone());
    }
}

fn create_hud_overlay(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    output_format: wgpu::TextureFormat,
) -> HudOverlay {
    let transparent = image::RgbaImage::new(HUD_TEXTURE_WIDTH, HUD_TEXTURE_HEIGHT);
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("HeroQuest OSD texture"),
            size: wgpu::Extent3d {
                width: HUD_TEXTURE_WIDTH,
                height: HUD_TEXTURE_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        transparent.as_raw(),
    );
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("HeroQuest OSD texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("HeroQuest OSD sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("HeroQuest OSD bind group"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &texture.create_view(&wgpu::TextureViewDescriptor::default()),
                ),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("HeroQuest OSD shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("hud.wgsl"))),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("HeroQuest OSD pipeline layout"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("HeroQuest OSD pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_hud"),
            buffers: &[Some(BoardVertex::layout())],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_hud"),
            targets: &[Some(wgpu::ColorTargetState {
                format: output_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    let vertices = [
        BoardVertex {
            position: [-1.0, 1.0, 0.0],
            uv: [0.0, 0.0],
        },
        BoardVertex {
            position: [-1.0, -1.0, 0.0],
            uv: [0.0, 1.0],
        },
        BoardVertex {
            position: [1.0, -1.0, 0.0],
            uv: [1.0, 1.0],
        },
        BoardVertex {
            position: [1.0, 1.0, 0.0],
            uv: [1.0, 0.0],
        },
    ];
    let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("HeroQuest OSD vertices"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("HeroQuest OSD indices"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    HudOverlay {
        texture,
        bind_group,
        pipeline,
        vertex_buffer,
        index_buffer,
        last_state: None,
    }
}

fn draw_hud_overlay(state: &GameOverlay) -> image::RgbaImage {
    use image::Rgba;

    let mut canvas = image::RgbaImage::new(HUD_TEXTURE_WIDTH, HUD_TEXTURE_HEIGHT);
    let shadow = Rgba([4, 2, 3, 175]);
    let panel = Rgba([31, 14, 18, 224]);
    let modal_panel = Rgba([46, 20, 23, 247]);
    let gold = Rgba([211, 166, 72, 255]);
    let pale_gold = Rgba([242, 211, 139, 255]);
    let parchment = Rgba([244, 228, 192, 255]);
    let muted = Rgba([196, 170, 132, 255]);

    hud_fill_rect(&mut canvas, 26, 23, 978, 86, shadow);
    hud_fill_rect(&mut canvas, 20, 17, 978, 86, panel);
    hud_outline_rect(&mut canvas, 20, 17, 978, 86, 3, gold);
    hud_corner_ornaments(&mut canvas, 20, 17, 978, 86, gold);
    hud_draw_text(&mut canvas, &state.heading, 44, 34, 2, pale_gold);
    hud_draw_text(&mut canvas, &state.status, 44, 70, 2, parchment);

    hud_fill_rect(&mut canvas, 26, 492, 978, 126, shadow);
    hud_fill_rect(&mut canvas, 20, 486, 978, 126, panel);
    hud_outline_rect(&mut canvas, 20, 486, 978, 126, 3, gold);
    hud_corner_ornaments(&mut canvas, 20, 486, 978, 126, gold);
    let message = hud_wrap_text(&state.message, 51, 2);
    hud_draw_text(&mut canvas, &message, 44, 504, 2, parchment);
    if state.actions.is_empty() {
        hud_draw_text(&mut canvas, "Please wait...", 44, 568, 2, muted);
    } else {
        let button = Rgba([78, 35, 31, 246]);
        let button_shadow = Rgba([4, 2, 3, 210]);
        for (action, (x, y, width, height)) in state
            .actions
            .iter()
            .zip(hud_action_button_rects(state.actions.len()))
        {
            hud_fill_rect(&mut canvas, x + 3, y + 3, width, height, button_shadow);
            hud_fill_rect(&mut canvas, x, y, width, height, button);
            hud_outline_rect(&mut canvas, x, y, width, height, 2, gold);
            let columns = ((width.saturating_sub(12)) / 9).max(1) as usize;
            let max_lines = if height >= 38 { 2 } else { 1 };
            let label = hud_wrap_text(action, columns, max_lines);
            let line_count = label.lines().count() as u32;
            let (label_size, line_advance): (f32, i32) = if line_count == 1 && height >= 38 {
                (18.0, 19)
            } else if line_count == 1 {
                (15.0, 16)
            } else {
                (14.0, 15)
            };
            let text_height =
                label_size.ceil() as u32 + line_count.saturating_sub(1) * line_advance as u32;
            let label_y = y as i32 + height.saturating_sub(text_height) as i32 / 2;
            draw_medieval_text(
                &mut canvas,
                &label,
                (x + 7) as i32,
                label_y,
                label_size,
                line_advance,
                pale_gold,
            );
        }
    }

    if let Some(dialog) = &state.dialog {
        hud_fill_rect(&mut canvas, 205, 165, 626, 300, Rgba([0, 0, 0, 150]));
        hud_fill_rect(&mut canvas, 197, 157, 626, 300, modal_panel);
        hud_outline_rect(&mut canvas, 197, 157, 626, 300, 5, gold);
        hud_outline_rect(&mut canvas, 207, 167, 606, 280, 1, muted);
        hud_corner_ornaments(&mut canvas, 197, 157, 626, 300, pale_gold);
        let title = hud_wrap_text(&dialog.title, 20, 2);
        hud_draw_text(&mut canvas, &title, 244, 198, 3, pale_gold);
        let body = hud_wrap_text(&dialog.body, 29, 6);
        hud_draw_text(&mut canvas, &body, 244, 274, 2, parchment);
        let hint = hud_wrap_text(&dialog.hint, 29, 2);
        hud_draw_text(&mut canvas, &hint, 244, 404, 2, gold);
    }
    canvas
}

fn hud_action_button_rects(count: usize) -> Vec<(u32, u32, u32, u32)> {
    if count == 0 {
        return Vec::new();
    }
    let rows = if count <= 5 { 1 } else { 2 };
    let columns = count.div_ceil(rows);
    let left = 34_u32;
    let available_width = 956_u32;
    let gap = 6_u32;
    let width = (available_width - gap * (columns.saturating_sub(1) as u32)) / columns as u32;
    let (top, height, row_gap) = if rows == 1 {
        (552_u32, 46_u32, 0_u32)
    } else {
        (546_u32, 27_u32, 6_u32)
    };
    (0..count)
        .map(|index| {
            let row = index / columns;
            let column = index % columns;
            (
                left + column as u32 * (width + gap),
                top + row as u32 * (height + row_gap),
                width,
                height,
            )
        })
        .collect()
}

fn hud_fill_rect(
    canvas: &mut image::RgbaImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: image::Rgba<u8>,
) {
    for py in y..(y + height).min(canvas.height()) {
        for px in x..(x + width).min(canvas.width()) {
            canvas.put_pixel(px, py, color);
        }
    }
}

fn hud_outline_rect(
    canvas: &mut image::RgbaImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    thickness: u32,
    color: image::Rgba<u8>,
) {
    hud_fill_rect(canvas, x, y, width, thickness, color);
    hud_fill_rect(canvas, x, y + height - thickness, width, thickness, color);
    hud_fill_rect(canvas, x, y, thickness, height, color);
    hud_fill_rect(canvas, x + width - thickness, y, thickness, height, color);
}

fn hud_corner_ornaments(
    canvas: &mut image::RgbaImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: image::Rgba<u8>,
) {
    for (cx, cy) in [
        (x + 10, y + 10),
        (x + width - 11, y + 10),
        (x + 10, y + height - 11),
        (x + width - 11, y + height - 11),
    ] {
        for offset in 0..5 {
            hud_fill_rect(canvas, cx - offset, cy + offset, offset * 2 + 1, 1, color);
            hud_fill_rect(canvas, cx - offset, cy - offset, offset * 2 + 1, 1, color);
        }
    }
}

fn hud_draw_text(
    canvas: &mut image::RgbaImage,
    text: &str,
    x: i32,
    y: i32,
    scale: u32,
    color: image::Rgba<u8>,
) {
    draw_medieval_text(
        canvas,
        text,
        x,
        y,
        medieval_pixel_size(scale),
        medieval_line_advance(scale),
        color,
    );
}

fn medieval_font() -> &'static fontdue::Font {
    static FONT: std::sync::OnceLock<fontdue::Font> = std::sync::OnceLock::new();
    FONT.get_or_init(|| {
        fontdue::Font::from_bytes(
            include_bytes!("../assets/fonts/Almendra-Regular.ttf") as &[u8],
            fontdue::FontSettings::default(),
        )
        .expect("the bundled Almendra font is valid")
    })
}

fn medieval_pixel_size(scale: u32) -> f32 {
    (8 * scale + 4) as f32
}

fn medieval_line_advance(scale: u32) -> i32 {
    if scale == 1 { 12 } else { (10 * scale) as i32 }
}

/// Startup panels are rasterized at half their 2048-unit design resolution.
/// Preserve each requested size instead of rounding pairs of sizes down to the
/// same tiny integer scale (the old mapping made both scale 1 and scale 2 text
/// only 12 pixels tall).
fn startup_font_pixel_size(scale: u32) -> f32 {
    5.0 * scale.max(1) as f32 + 7.0
}

fn startup_font_line_advance(scale: u32) -> i32 {
    (startup_font_pixel_size(scale) * 1.10).ceil() as i32
}

fn medieval_text_width(text: &str, pixel_size: f32) -> f32 {
    let font = medieval_font();
    text.lines()
        .map(|line| {
            line.chars()
                .map(|character| font.metrics(character, pixel_size).advance_width)
                .sum::<f32>()
        })
        .fold(0.0, f32::max)
}

fn draw_medieval_text(
    canvas: &mut image::RgbaImage,
    text: &str,
    x: i32,
    y: i32,
    pixel_size: f32,
    line_advance: i32,
    color: image::Rgba<u8>,
) {
    let font = medieval_font();
    let ascent = font
        .horizontal_line_metrics(pixel_size)
        .map_or(pixel_size * 0.8, |metrics| metrics.ascent);
    let mut cursor_x = x as f32;
    let mut cursor_y = y;
    for character in text.chars() {
        if character == '\n' {
            cursor_x = x as f32;
            cursor_y += line_advance;
            continue;
        }
        let (metrics, bitmap) = font.rasterize(character, pixel_size);
        let baseline = cursor_y + ascent.ceil() as i32;
        let glyph_x = cursor_x.round() as i32 + metrics.xmin;
        let glyph_y = baseline - metrics.ymin - metrics.height as i32;
        for row in 0..metrics.height {
            for column in 0..metrics.width {
                let coverage = bitmap[row * metrics.width + column] as u32;
                if coverage == 0 {
                    continue;
                }
                let px = glyph_x + column as i32;
                let py = glyph_y + row as i32;
                if px < 0 || py < 0 || px >= canvas.width() as i32 || py >= canvas.height() as i32 {
                    continue;
                }
                let destination = *canvas.get_pixel(px as u32, py as u32);
                let source_alpha = color[3] as u32 * coverage / 255;
                let inverse_alpha = 255 - source_alpha;
                let mut blended = [0_u8; 4];
                for channel in 0..3 {
                    blended[channel] = ((color[channel] as u32 * source_alpha
                        + destination[channel] as u32 * inverse_alpha)
                        / 255) as u8;
                }
                blended[3] =
                    (source_alpha + destination[3] as u32 * inverse_alpha / 255).min(255) as u8;
                canvas.put_pixel(px as u32, py as u32, image::Rgba(blended));
            }
        }
        cursor_x += metrics.advance_width;
    }
}

fn hud_wrap_text(text: &str, width: usize, max_lines: usize) -> String {
    let normalized = text.replace('—', "-").replace('…', "...");
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in normalized.split_whitespace() {
        if !line.is_empty() && line.len() + word.len() + 1 > width {
            lines.push(std::mem::take(&mut line));
            if lines.len() == max_lines {
                break;
            }
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if lines.len() < max_lines && !line.is_empty() {
        lines.push(line);
    }
    lines.join("\n")
}

fn draw_startup_quad<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    scene: &'a StartupScene,
    bind_group: &'a wgpu::BindGroup,
) {
    pass.set_bind_group(1, bind_group, &[]);
    pass.set_vertex_buffer(0, scene.quad_vertex_buffer.slice(..));
    pass.set_index_buffer(scene.quad_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
    pass.draw_indexed(0..scene.quad_index_count, 0, 0..1);
}

fn load_player_stations(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    camera_layout: &wgpu::BindGroupLayout,
    output_format: wgpu::TextureFormat,
) -> Result<Option<PlayerStations>> {
    let root = local_art_root();
    let hero_paths = [
        root.join("startup/heroes/barbarian.jpg"),
        root.join("startup/heroes/dwarf.jpg"),
        root.join("startup/heroes/elf.jpg"),
        root.join("startup/heroes/wizard.jpg"),
    ];
    let sheet_path = root.join("character-sheet/character-sheet.png");
    let armory_path = root.join("armory-pages/identification-guide-and-armory.png");
    let air_path = root.join("card-sheets/page-06.png");
    let elemental_path = root.join("card-sheets/page-08.png");
    let required = hero_paths
        .iter()
        .chain([&sheet_path, &armory_path, &air_path, &elemental_path]);
    if required.clone().all(|path| !path.is_file()) {
        return Ok(None);
    }
    if let Some(path) = required.into_iter().find(|path| !path.is_file()) {
        return Err(anyhow!(
            "the four player stations need all Hero, sheet, Armory, and spell scans; missing {}",
            path.display()
        ));
    }

    let (texture_layout, sampler, pipeline) =
        create_tabletop_scan_pipeline(device, camera_layout, output_format, "Hero station scans");
    let mut textures = hero_paths
        .iter()
        .map(|path| load_startup_texture(device, queue, &texture_layout, &sampler, path))
        .collect::<Result<Vec<_>>>()?;

    // The source page is a four-up pad scan. Crop one intact record sheet and
    // share the resulting GPU texture across all four physical stations.
    let character_sheet_texture = textures.len();
    let cached_character_sheet = root.join("tabletop/player/character-sheet.png");
    if cached_character_sheet.is_file() {
        textures.push(load_startup_texture(
            device,
            queue,
            &texture_layout,
            &sampler,
            &cached_character_sheet,
        )?);
    } else {
        let character_sheet = image::open(&sheet_path)
            .with_context(|| format!("failed to decode {}", sheet_path.display()))?
            .crop_imm(38, 38, 1130, 1690)
            .resize(670, 1000, image::imageops::FilterType::Lanczos3)
            .into_rgba8();
        textures.push(load_startup_texture_from_rgba(
            device,
            queue,
            &texture_layout,
            &sampler,
            "original-US Character Sheet",
            &character_sheet,
        ));
    }

    let action_reference_texture = textures.len();
    let action_reference = build_hero_turn_reference_card();
    textures.push(load_startup_texture_from_rgba(
        device,
        queue,
        &texture_layout,
        &sampler,
        "Hero turn reference card",
        &action_reference,
    ));

    let armory_texture = textures.len();
    let cached_armory = root.join("tabletop/player/armory.jpg");
    if cached_armory.is_file() {
        textures.push(load_startup_texture(
            device,
            queue,
            &texture_layout,
            &sampler,
            &cached_armory,
        )?);
    } else {
        let armory = image::open(&armory_path)
            .with_context(|| format!("failed to decode {}", armory_path.display()))?
            .resize(1200, 1000, image::imageops::FilterType::Lanczos3)
            .into_rgba8();
        textures.push(load_startup_texture_from_rgba(
            device,
            queue,
            &texture_layout,
            &sampler,
            "original-US Identification Guide and Armory",
            &armory,
        ));
    }

    let spell_texture_base = textures.len();
    let mut cached_spell_paths = Vec::with_capacity(12);
    for group in ["air", "fire", "water", "earth"] {
        for card in 1..=3 {
            cached_spell_paths.push(root.join(format!("tabletop/spells/{group}-{card}.jpg")));
        }
    }
    if cached_spell_paths.iter().all(|path| path.is_file()) {
        for path in &cached_spell_paths {
            textures.push(load_startup_texture(
                device,
                queue,
                &texture_layout,
                &sampler,
                path,
            )?);
        }
    } else {
        let air_sheet = image::open(&air_path)
            .with_context(|| format!("failed to decode {}", air_path.display()))?;
        let elemental_sheet = image::open(&elemental_path)
            .with_context(|| format!("failed to decode {}", elemental_path.display()))?;
        // Texture order follows SpellGroup::ALL: Air, Fire, Water, Earth.
        for (sheet, row) in [
            (&air_sheet, 2_u32),
            (&elemental_sheet, 1),
            (&elemental_sheet, 2),
            (&elemental_sheet, 0),
        ] {
            for column in 0..3 {
                let card = crop_printed_card(sheet, column, row);
                textures.push(load_startup_texture_from_rgba(
                    device,
                    queue,
                    &texture_layout,
                    &sampler,
                    "original-US elemental spell card",
                    &card,
                ));
            }
        }
    }

    let mut vertices = Vec::with_capacity(100);
    let mut indices = Vec::with_capacity(150);
    let mut fixed_surfaces = Vec::with_capacity(13);
    for station in 0..4 {
        let (center, layout_angle) = player_station_layout(station);
        let card_angle = player_station_card_angle(station);
        let hero_center = station_point(center, layout_angle, -1.65, -0.20);
        let sheet_center = station_point(center, layout_angle, -0.05, -0.20);
        let reference_center = station_point(center, layout_angle, -1.65, 1.72);
        fixed_surfaces.push(TexturedSurface {
            texture_index: station,
            index_range: push_flat_scan_quad(
                &mut vertices,
                &mut indices,
                hero_center,
                card_angle,
                0.69,
                1.06,
                0.060,
                [0.0, 0.0, 1.0, 1.0],
            ),
        });
        fixed_surfaces.push(TexturedSurface {
            texture_index: character_sheet_texture,
            index_range: push_flat_scan_quad(
                &mut vertices,
                &mut indices,
                sheet_center,
                card_angle,
                0.78,
                1.18,
                0.062,
                [0.0, 0.0, 1.0, 1.0],
            ),
        });
        fixed_surfaces.push(TexturedSurface {
            texture_index: action_reference_texture,
            index_range: push_flat_scan_quad(
                &mut vertices,
                &mut indices,
                reference_center,
                card_angle,
                0.70,
                0.40,
                0.064,
                [0.0, 0.0, 1.0, 1.0],
            ),
        });
    }
    fixed_surfaces.push(TexturedSurface {
        texture_index: armory_texture,
        index_range: push_flat_scan_quad(
            &mut vertices,
            &mut indices,
            Vec3::new(0.0, 0.0, -17.45),
            std::f32::consts::PI,
            4.0,
            3.25,
            0.058,
            [0.0, 0.0, 1.0, 1.0],
        ),
    });

    let (elf_center, elf_layout_angle) = player_station_layout(2);
    let elf_angle = player_station_card_angle(2);
    let elf_spell_ranges = std::array::from_fn(|slot| {
        let center = station_point(
            elf_center,
            elf_layout_angle,
            -0.80 + slot as f32 * 0.80,
            -2.03,
        );
        push_flat_scan_quad(
            &mut vertices,
            &mut indices,
            center,
            elf_angle + (slot as f32 - 1.0) * 0.055,
            0.38,
            0.57,
            0.085 + slot as f32 * 0.002,
            [0.0, 0.0, 1.0, 1.0],
        )
    });
    let (wizard_center, wizard_layout_angle) = player_station_layout(3);
    let wizard_angle = player_station_card_angle(3);
    let wizard_spell_ranges = std::array::from_fn(|slot| {
        let center = station_point(
            wizard_center,
            wizard_layout_angle,
            -2.12 + slot as f32 * 0.52,
            -2.05 + (slot as f32 - 4.0).abs() * 0.025,
        );
        push_flat_scan_quad(
            &mut vertices,
            &mut indices,
            center,
            wizard_angle + (slot as f32 - 4.0) * 0.025,
            0.31,
            0.48,
            0.084 + slot as f32 * 0.001,
            [0.0, 0.0, 1.0, 1.0],
        )
    });
    let elf_discard_range = push_flat_scan_quad(
        &mut vertices,
        &mut indices,
        station_point(elf_center, elf_layout_angle, 1.75, -2.03),
        elf_angle + 0.08,
        0.38,
        0.57,
        0.095,
        [0.0, 0.0, 1.0, 1.0],
    );
    let wizard_discard_range = push_flat_scan_quad(
        &mut vertices,
        &mut indices,
        station_point(wizard_center, wizard_layout_angle, 2.90, -2.05),
        wizard_angle + 0.10,
        0.38,
        0.57,
        0.095,
        [0.0, 0.0, 1.0, 1.0],
    );

    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("player station scan vertices"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("player station scan indices"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    Ok(Some(PlayerStations {
        pipeline,
        vertex_buffer,
        index_buffer,
        fixed_surfaces,
        elf_spell_ranges,
        wizard_spell_ranges,
        elf_discard_range,
        wizard_discard_range,
        spell_texture_base,
        textures,
    }))
}

fn load_zargon_tabletop(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    camera_layout: &wgpu::BindGroupLayout,
    output_format: wgpu::TextureFormat,
) -> Result<Option<ZargonTabletop>> {
    let root = local_art_root();
    let card_paths = [
        root.join("card-sheets/page-01.png"), // Treasure
        root.join("card-sheets/page-11.png"), // Artifact
        root.join("card-sheets/page-09.png"), // Chaos/Dread Spell
        root.join("card-sheets/page-15.png"), // Monster
        root.join("card-sheets/page-14.png"), // Monster faces, part one
        root.join("card-sheets/page-16.png"), // Monster faces, part two
        root.join("card-sheets/page-10.png"), // Chaos Spell faces, first nine
        root.join("card-sheets/page-12.png"), // Chaos Spell faces, final three
    ];
    let quest_paths: Vec<_> = (3..3 + QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS as u32)
        .map(|page| root.join(format!("quest-pages/page-{page:02}.png")))
        .collect();
    if card_paths
        .iter()
        .chain(quest_paths.iter())
        .all(|path| !path.is_file())
    {
        return Ok(None);
    }
    if let Some(path) = card_paths
        .iter()
        .chain(quest_paths.iter())
        .find(|path| !path.is_file())
    {
        return Err(anyhow!(
            "Zargon's tabletop needs the complete original-US card and Quest Book scans; missing {}",
            path.display()
        ));
    }

    let (texture_layout, sampler, pipeline) = create_tabletop_scan_pipeline(
        device,
        camera_layout,
        output_format,
        "Zargon tabletop scans",
    );
    let cached_deck_paths = [
        root.join("tabletop/zargon/treasure-back.jpg"),
        root.join("tabletop/zargon/artifact-back.jpg"),
        root.join("tabletop/zargon/dread-spell-back.jpg"),
        root.join("tabletop/zargon/monster-back.jpg"),
    ];
    let mut textures = if cached_deck_paths.iter().all(|path| path.is_file()) {
        cached_deck_paths
            .iter()
            .map(|path| load_startup_texture(device, queue, &texture_layout, &sampler, path))
            .collect::<Result<Vec<_>>>()?
    } else {
        card_paths[..4]
            .iter()
            .map(|path| {
                let sheet = image::open(path)
                    .with_context(|| format!("failed to decode {}", path.display()))?;
                let card = crop_printed_card(&sheet, 0, 0);
                Ok(load_startup_texture_from_rgba(
                    device,
                    queue,
                    &texture_layout,
                    &sampler,
                    "original-US deck back",
                    &card,
                ))
            })
            .collect::<Result<Vec<_>>>()?
    };

    let cached_monster_paths = [
        "chaos-warrior",
        "fimir",
        "gargoyle",
        "goblin",
        "mummy",
        "orc",
        "skeleton",
        "zombie",
    ]
    .map(|monster| root.join(format!("tabletop/monsters/{monster}.jpg")));
    if cached_monster_paths.iter().all(|path| path.is_file()) {
        for path in &cached_monster_paths {
            textures.push(load_startup_texture(
                device,
                queue,
                &texture_layout,
                &sampler,
                path,
            )?);
        }
    } else {
        let monster_sheet_one = image::open(&card_paths[4])
            .with_context(|| format!("failed to decode {}", card_paths[4].display()))?;
        let monster_sheet_two = image::open(&card_paths[5])
            .with_context(|| format!("failed to decode {}", card_paths[5].display()))?;
        for (sheet, column, row) in [
            (&monster_sheet_one, 1, 1), // Chaos Warrior
            (&monster_sheet_one, 2, 1), // Fimir
            (&monster_sheet_one, 0, 2), // Gargoyle
            (&monster_sheet_one, 1, 2), // Goblin
            (&monster_sheet_one, 2, 2), // Mummy
            (&monster_sheet_two, 0, 0), // Orc
            (&monster_sheet_two, 1, 0), // Skeleton
            (&monster_sheet_two, 2, 0), // Zombie
        ] {
            let card = crop_printed_card(sheet, column, row);
            textures.push(load_startup_texture_from_rgba(
                device,
                queue,
                &texture_layout,
                &sampler,
                "original-US Monster reference card",
                &card,
            ));
        }
    }
    let chaos_texture_base = textures.len();
    let chaos_sheet_one = image::open(&card_paths[6])
        .with_context(|| format!("failed to decode {}", card_paths[6].display()))?;
    let chaos_sheet_two = image::open(&card_paths[7])
        .with_context(|| format!("failed to decode {}", card_paths[7].display()))?;
    for row in 0..3 {
        for column in 0..3 {
            let card = crop_printed_card(&chaos_sheet_one, column, row);
            textures.push(load_startup_texture_from_rgba(
                device,
                queue,
                &texture_layout,
                &sampler,
                "original-US Chaos Spell face",
                &card,
            ));
        }
    }
    for column in 0..3 {
        let card = crop_printed_card(&chaos_sheet_two, column, 0);
        textures.push(load_startup_texture_from_rgba(
            device,
            queue,
            &texture_layout,
            &sampler,
            "original-US Chaos Spell face",
            &card,
        ));
    }
    let quest_texture_base = textures.len();
    let cached_quest_paths = (1..=QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS)
        .map(|quest| root.join(format!("tabletop/quest-book/quest-{quest:02}.jpg")))
        .collect::<Vec<_>>();
    if cached_quest_paths.iter().all(|path| path.is_file()) {
        for path in &cached_quest_paths {
            textures.push(load_startup_texture(
                device,
                queue,
                &texture_layout,
                &sampler,
                path,
            )?);
        }
    } else {
        for path in &quest_paths {
            let page = image::open(path)
                .with_context(|| format!("failed to decode {}", path.display()))?
                .resize(900, 1200, image::imageops::FilterType::Lanczos3)
                .into_rgba8();
            textures.push(load_startup_texture_from_rgba(
                device,
                queue,
                &texture_layout,
                &sampler,
                "original-US Quest Book page",
                &page,
            ));
        }
    }

    let mut vertices = Vec::with_capacity(56);
    let mut indices = Vec::with_capacity(84);
    let mut fixed_surfaces = Vec::with_capacity(12);
    for (deck, x) in [-12.0_f32, -9.8, -7.6, -5.4].into_iter().enumerate() {
        fixed_surfaces.push(TexturedSurface {
            texture_index: deck,
            index_range: push_flat_scan_quad(
                &mut vertices,
                &mut indices,
                Vec3::new(x, 0.0, 17.55),
                0.0,
                0.62,
                0.94,
                0.175 + deck as f32 * 0.002,
                [0.0, 0.0, 1.0, 1.0],
            ),
        });
    }
    for monster in 0..8 {
        let column = monster % 4;
        let row = monster / 4;
        fixed_surfaces.push(TexturedSurface {
            texture_index: 4 + monster,
            index_range: push_flat_scan_quad(
                &mut vertices,
                &mut indices,
                Vec3::new(7.15 + column as f32 * 1.52, 0.0, 17.25 + row as f32 * 2.05),
                0.0,
                0.64,
                0.94,
                0.090 + monster as f32 * 0.001,
                [0.0, 0.0, 1.0, 1.0],
            ),
        });
    }
    let chaos_discard_range = push_flat_scan_quad(
        &mut vertices,
        &mut indices,
        Vec3::new(-7.6, 0.0, 19.65),
        0.08,
        0.62,
        0.94,
        0.103,
        [0.0, 0.0, 1.0, 1.0],
    );
    // The physical scan is one portrait quest page. Split it across the open
    // leaves and rotate the complete two-leaf composition toward Zargon's
    // seat. Swapping the source halves preserves the page when the entire
    // composition turns 180 degrees, and makes its map agree with Zargon's
    // view of the physical board.
    let book_ranges = [
        push_flat_scan_quad(
            &mut vertices,
            &mut indices,
            Vec3::new(-1.84, 0.0, 18.25),
            ZARGON_QUEST_BOOK_ANGLE,
            1.78,
            2.30,
            0.105,
            [0.0, 0.5, 1.0, 1.0],
        ),
        push_flat_scan_quad(
            &mut vertices,
            &mut indices,
            Vec3::new(1.84, 0.0, 18.25),
            ZARGON_QUEST_BOOK_ANGLE,
            1.78,
            2.30,
            0.107,
            [0.0, 0.0, 1.0, 0.5],
        ),
    ];
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Zargon tabletop scan vertices"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Zargon tabletop scan indices"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    Ok(Some(ZargonTabletop {
        pipeline,
        vertex_buffer,
        index_buffer,
        fixed_surfaces,
        book_ranges,
        chaos_discard_range,
        chaos_texture_base,
        quest_texture_base,
        textures,
    }))
}

fn create_tabletop_scan_pipeline(
    device: &wgpu::Device,
    camera_layout: &wgpu::BindGroupLayout,
    output_format: wgpu::TextureFormat,
    label: &str,
) -> (wgpu::BindGroupLayout, wgpu::Sampler, wgpu::RenderPipeline) {
    let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("board_texture.wgsl"))),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(camera_layout), Some(&texture_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_board"),
            buffers: &[Some(BoardVertex::layout())],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_board"),
            targets: &[Some(wgpu::ColorTargetState {
                format: output_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    (texture_layout, sampler, pipeline)
}

fn crop_printed_card(sheet: &image::DynamicImage, column: u32, row: u32) -> image::RgbaImage {
    // All 16 original-US card-sheet scans use the same three-by-three print
    // registration. Preserve a little white gutter so no rounded edge clips.
    let x = 145 + column * 780;
    let y = 175 + row * 1060;
    sheet
        .crop_imm(x, y, 625, 930)
        .resize(400, 600, image::imageops::FilterType::Lanczos3)
        .into_rgba8()
}

fn push_flat_scan_quad(
    vertices: &mut Vec<BoardVertex>,
    indices: &mut Vec<u16>,
    center: Vec3,
    angle: f32,
    half_width: f32,
    half_height: f32,
    y: f32,
    uv_bounds: [f32; 4],
) -> std::ops::Range<u32> {
    let [u0, v0, u1, v1] = uv_bounds;
    let right = Vec3::new(angle.cos(), 0.0, angle.sin());
    let down = Vec3::new(-angle.sin(), 0.0, angle.cos());
    let corners = [
        (-half_width, -half_height, [u0, v0]),
        (-half_width, half_height, [u0, v1]),
        (half_width, half_height, [u1, v1]),
        (half_width, -half_height, [u1, v0]),
    ];
    let base = u16::try_from(vertices.len()).expect("tabletop scan vertex count fits u16");
    for (x, z, uv) in corners {
        vertices.push(BoardVertex {
            position: (center + right * x + down * z + Vec3::Y * y).to_array(),
            uv,
        });
    }
    let start = indices.len() as u32;
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    start..indices.len() as u32
}

fn build_hero_turn_reference_card() -> image::RgbaImage {
    use image::Rgba;

    let parchment = Rgba([230, 207, 156, 255]);
    let ink = Rgba([72, 28, 24, 255]);
    let red = Rgba([119, 29, 37, 255]);
    let mut card = image::RgbaImage::from_pixel(600, 420, parchment);
    for x in 8..592 {
        for y in [8, 9, 410, 411] {
            card.put_pixel(x, y, red);
        }
    }
    for y in 8..412 {
        for x in [8, 9, 590, 591] {
            card.put_pixel(x, y, red);
        }
    }
    let mut draw = |text: &str, x: u32, y: u32, scale: u32, color: Rgba<u8>| {
        draw_medieval_text(
            &mut card,
            text,
            x as i32,
            y as i32,
            medieval_pixel_size(scale),
            medieval_line_advance(scale),
            color,
        );
    };
    let title_size = medieval_pixel_size(4);
    let title_x = ((600.0 - medieval_text_width("HERO TURN", title_size)) * 0.5).max(0.0) as u32;
    draw("HERO TURN", title_x, 28, 4, red);
    draw("MOVE BEFORE OR AFTER ONE ACTION", 40, 92, 2, ink);
    for (line, y) in [
        ("ATTACK", 145),
        ("CAST A SPELL", 190),
        ("SEARCH FOR TREASURE", 235),
        ("SEARCH FOR TRAPS & SECRET DOORS", 280),
        ("OPEN DOORS WHILE MOVING", 335),
    ] {
        draw(line, 42, y, 2, ink);
    }
    card
}

const fn spell_group_index(group: SpellGroup) -> usize {
    match group {
        SpellGroup::Air => 0,
        SpellGroup::Fire => 1,
        SpellGroup::Water => 2,
        SpellGroup::Earth => 3,
    }
}

fn hero_spell_texture_offset(spell: HeroSpell) -> Option<usize> {
    SpellGroup::ALL.into_iter().find_map(|group| {
        group
            .spells()
            .iter()
            .position(|candidate| *candidate == spell)
            .map(|card| spell_group_index(group) * 3 + card)
    })
}

fn load_dice_decals(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    camera_layout: &wgpu::BindGroupLayout,
    output_format: wgpu::TextureFormat,
) -> Result<Option<DiceDecals>> {
    let root = local_art_root().join("dice");
    let specs = [
        (DiceDecalKind::Skull, root.join("skull.png")),
        (DiceDecalKind::WhiteShield, root.join("white-shield.png")),
        (DiceDecalKind::BlackShield, root.join("black-shield.png")),
        (DiceDecalKind::MovementPip, root.join("movement-pip.png")),
    ];
    if specs.iter().all(|(_, path)| !path.is_file()) {
        return Ok(None);
    }
    if let Some((_, path)) = specs.iter().find(|(_, path)| !path.is_file()) {
        return Err(anyhow!(
            "the original-US dice need all four face decals; missing {} (run tools/extract-original-us-dice-decals.sh)",
            path.display()
        ));
    }

    let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("combat die decal texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("combat die decal sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });
    let assets = specs
        .into_iter()
        .map(|(kind, path)| {
            let texture = load_startup_texture(device, queue, &texture_layout, &sampler, &path)?;
            Ok(DiceDecalAsset {
                kind,
                texture,
                path,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("combat die decal shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("sprite.wgsl"))),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("combat die decal pipeline layout"),
        bind_group_layouts: &[Some(camera_layout), Some(&texture_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("combat die decal pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_sprite"),
            buffers: &[
                Some(BoardVertex::layout()),
                Some(SpriteInstanceRaw::layout()),
            ],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_sprite"),
            targets: &[Some(wgpu::ColorTargetState {
                format: output_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    let vertices = [
        BoardVertex {
            position: [-0.5, 0.0, 0.5],
            uv: [0.0, 1.0],
        },
        BoardVertex {
            position: [0.5, 0.0, 0.5],
            uv: [1.0, 1.0],
        },
        BoardVertex {
            position: [0.5, 0.0, -0.5],
            uv: [1.0, 0.0],
        },
        BoardVertex {
            position: [-0.5, 0.0, -0.5],
            uv: [0.0, 0.0],
        },
    ];
    let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("combat die decal vertices"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("combat die decal indices"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("combat die decal instances"),
        size: (MAX_SPRITE_INSTANCES * std::mem::size_of::<SpriteInstanceRaw>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    Ok(Some(DiceDecals {
        pipeline,
        vertex_buffer,
        index_buffer,
        instance_buffer,
        index_count: indices.len() as u32,
        assets,
    }))
}

fn load_component_decals(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    camera_layout: &wgpu::BindGroupLayout,
    output_format: wgpu::TextureFormat,
) -> Result<Option<ComponentDecals>> {
    let root = local_art_root().join("components");
    let specs = [
        (ComponentDecalKey::DoorOpen, root.join("doors/open.png")),
        (ComponentDecalKey::DoorClosed, root.join("doors/closed.png")),
        (
            ComponentDecalKey::Prop(PropKind::Stairs),
            root.join("markers/stairs.png"),
        ),
        (
            ComponentDecalKey::Prop(PropKind::Table),
            root.join("furniture/table.png"),
        ),
        (
            ComponentDecalKey::Prop(PropKind::Chest),
            root.join("furniture/chest.png"),
        ),
        (
            ComponentDecalKey::Prop(PropKind::Bookcase),
            root.join("furniture/bookcase.png"),
        ),
        (
            ComponentDecalKey::Prop(PropKind::Throne),
            root.join("furniture/throne.png"),
        ),
        (
            ComponentDecalKey::Prop(PropKind::AlchemistsBench),
            root.join("furniture/alchemists-bench.png"),
        ),
        (
            ComponentDecalKey::Prop(PropKind::Tomb),
            root.join("furniture/tomb.png"),
        ),
        (
            ComponentDecalKey::Prop(PropKind::SorcerersTable),
            root.join("furniture/sorcerers-table.png"),
        ),
        (
            ComponentDecalKey::Prop(PropKind::TortureRack),
            root.join("furniture/torture-rack.png"),
        ),
        (
            ComponentDecalKey::Prop(PropKind::Fireplace),
            root.join("furniture/fireplace.png"),
        ),
        (
            ComponentDecalKey::Prop(PropKind::Cupboard),
            root.join("furniture/cupboard.png"),
        ),
        (
            ComponentDecalKey::BlockedSquare,
            root.join("markers/blocked-square.png"),
        ),
        (
            ComponentDecalKey::BlockedDouble,
            root.join("markers/blocked-double.png"),
        ),
        (
            ComponentDecalKey::Trap(TrapKind::Pit),
            root.join("markers/pit.png"),
        ),
        (
            ComponentDecalKey::Trap(TrapKind::FallingBlock),
            root.join("markers/falling-block.png"),
        ),
        (
            ComponentDecalKey::SecretDoor,
            root.join("markers/secret-door.png"),
        ),
        (
            ComponentDecalKey::DamageSkull,
            root.join("markers/skull.png"),
        ),
    ];
    if specs.iter().all(|(_, path)| !path.is_file()) {
        return Ok(None);
    }
    if let Some((_, path)) = specs.iter().find(|(_, path)| !path.is_file()) {
        return Err(anyhow!(
            "the original-US component face set is incomplete; missing {} (run tools/extract-original-us-components.sh)",
            path.display()
        ));
    }

    let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("original-US component texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("original-US component sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });
    let assets = specs
        .into_iter()
        .map(|(key, path)| {
            let texture = load_startup_texture(device, queue, &texture_layout, &sampler, &path)?;
            Ok(ComponentDecalAsset { key, texture, path })
        })
        .collect::<Result<Vec<_>>>()?;

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("original-US component face shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("sprite.wgsl"))),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("original-US component face pipeline layout"),
        bind_group_layouts: &[Some(camera_layout), Some(&texture_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("original-US component face pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_sprite"),
            buffers: &[
                Some(BoardVertex::layout()),
                Some(SpriteInstanceRaw::layout()),
            ],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_sprite"),
            targets: &[Some(wgpu::ColorTargetState {
                format: output_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    let vertices = [
        BoardVertex {
            position: [-0.5, -0.5, 0.0],
            uv: [0.0, 1.0],
        },
        BoardVertex {
            position: [0.5, -0.5, 0.0],
            uv: [1.0, 1.0],
        },
        BoardVertex {
            position: [0.5, 0.5, 0.0],
            uv: [1.0, 0.0],
        },
        BoardVertex {
            position: [-0.5, 0.5, 0.0],
            uv: [0.0, 0.0],
        },
    ];
    let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("original-US component face vertices"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("original-US component face indices"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("original-US component face instances"),
        size: (MAX_SPRITE_INSTANCES * std::mem::size_of::<SpriteInstanceRaw>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    Ok(Some(ComponentDecals {
        pipeline,
        vertex_buffer,
        index_buffer,
        instance_buffer,
        index_count: indices.len() as u32,
        assets,
    }))
}

fn load_startup_scene(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    camera_layout: &wgpu::BindGroupLayout,
    output_format: wgpu::TextureFormat,
) -> Result<Option<StartupScene>> {
    let root = local_art_root().join("startup");
    let box_paths = [
        root.join("box/top.jpg"),
        root.join("box/bottom.jpg"),
        root.join("box/north.jpg"),
        root.join("box/south.jpg"),
        root.join("box/west.jpg"),
        root.join("box/east.jpg"),
    ];
    let quest_paths: Vec<_> = (1..=14)
        .map(|number| root.join(format!("quests/quest-{number:02}.jpg")))
        .collect();
    let hero_paths = [
        root.join("heroes/barbarian.jpg"),
        root.join("heroes/dwarf.jpg"),
        root.join("heroes/elf.jpg"),
        root.join("heroes/wizard.jpg"),
    ];
    let armory_path = local_art_root().join("armory-pages/identification-guide-and-armory.png");
    if box_paths
        .iter()
        .chain(quest_paths.iter())
        .chain(hero_paths.iter())
        .chain(std::iter::once(&armory_path))
        .all(|path| !path.is_file())
    {
        return Ok(None);
    }
    for path in box_paths
        .iter()
        .chain(quest_paths.iter())
        .chain(hero_paths.iter())
        .chain(std::iter::once(&armory_path))
    {
        if !path.is_file() {
            return Err(anyhow!(
                "startup art is incomplete; missing {} (run tools/extract-original-us-startup-art.sh)",
                path.display()
            ));
        }
    }

    let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("startup texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("startup art sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });
    let box_faces = box_paths
        .iter()
        .map(|path| load_startup_texture(device, queue, &texture_layout, &sampler, path))
        .collect::<Result<Vec<_>>>()?;
    // Decode exactly the quests the current engine can start. The remaining
    // original scans stay available to the physical Quest Book and will join
    // this selection as each quest definition becomes playable.
    let quest_pages = quest_paths
        .iter()
        .take(QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS)
        .map(|path| {
            image::open(path)
                .with_context(|| format!("failed to decode {}", path.display()))
                .map(|image| {
                    image
                        .resize(914, 550, image::imageops::FilterType::Lanczos3)
                        .into_rgba8()
                })
        })
        .collect::<Result<Vec<_>>>()?;
    let hero_cards = hero_paths
        .iter()
        .map(|path| {
            image::open(path)
                .with_context(|| format!("failed to decode {}", path.display()))
                .map(|image| {
                    image
                        .resize(390, 705, image::imageops::FilterType::Lanczos3)
                        .into_rgba8()
                })
        })
        .collect::<Result<Vec<_>>>()?;
    let armory_page = image::open(&armory_path)
        .with_context(|| format!("failed to decode {}", armory_path.display()))?
        .resize(810, 672, image::imageops::FilterType::Lanczos3)
        .into_rgba8();

    let panel_rgba = image::RgbaImage::new(STARTUP_TEXTURE_SIZE, STARTUP_TEXTURE_SIZE);
    let panel_texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("startup panel texture"),
            size: wgpu::Extent3d {
                width: STARTUP_TEXTURE_SIZE,
                height: STARTUP_TEXTURE_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        panel_rgba.as_raw(),
    );
    let panel_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("startup panel bind group"),
        layout: &texture_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &panel_texture.create_view(&wgpu::TextureViewDescriptor::default()),
                ),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("startup texture shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("startup_texture.wgsl"))),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("startup pipeline layout"),
        bind_group_layouts: &[Some(camera_layout), Some(&texture_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("startup render pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_startup"),
            buffers: &[Some(BoardVertex::layout())],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_startup"),
            targets: &[Some(wgpu::ColorTargetState {
                format: output_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });

    let quad_vertices = [
        BoardVertex {
            position: [-10.0, 10.0, 0.0],
            uv: [0.0, 0.0],
        },
        BoardVertex {
            position: [-10.0, -10.0, 0.0],
            uv: [0.0, 1.0],
        },
        BoardVertex {
            position: [10.0, -10.0, 0.0],
            uv: [1.0, 1.0],
        },
        BoardVertex {
            position: [10.0, 10.0, 0.0],
            uv: [1.0, 0.0],
        },
    ];
    let quad_indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
    let quad_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("startup quad vertices"),
        contents: bytemuck::cast_slice(&quad_vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let quad_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("startup quad indices"),
        contents: bytemuck::cast_slice(&quad_indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    let (box_vertices, box_indices, box_index_ranges) = startup_box_mesh();
    let box_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("startup box vertices"),
        contents: bytemuck::cast_slice(&box_vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let box_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("startup box indices"),
        contents: bytemuck::cast_slice(&box_indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    Ok(Some(StartupScene {
        pipeline,
        quad_vertex_buffer,
        quad_index_buffer,
        quad_index_count: quad_indices.len() as u32,
        box_vertex_buffer,
        box_index_buffer,
        box_index_ranges,
        box_faces,
        quest_pages,
        hero_cards,
        armory_page,
        panel_texture,
        panel_bind_group,
        panel_state: None,
    }))
}

fn load_startup_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    path: &Path,
) -> Result<StartupTexture> {
    let image =
        image::open(path).with_context(|| format!("failed to decode {}", path.display()))?;
    let max_dimension = device.limits().max_texture_dimension_2d;
    let image = if image.width() > max_dimension || image.height() > max_dimension {
        image.resize(
            max_dimension,
            max_dimension,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        image
    };
    let rgba = image.into_rgba8();
    Ok(load_startup_texture_from_rgba(
        device,
        queue,
        layout,
        sampler,
        "startup scan texture",
        &rgba,
    ))
}

fn load_startup_texture_from_rgba(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    label: &str,
    rgba: &image::RgbaImage,
) -> StartupTexture {
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: rgba.width(),
                height: rgba.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        rgba.as_raw(),
    );
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &texture.create_view(&wgpu::TextureViewDescriptor::default()),
                ),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    StartupTexture {
        _texture: texture,
        bind_group,
    }
}

fn startup_box_mesh() -> (Vec<BoardVertex>, Vec<u16>, [std::ops::Range<u32>; 6]) {
    let x = 7.2;
    let y0 = -0.75;
    let y1 = 0.75;
    let z = 4.55;
    let faces = [
        // top, bottom, north, south, west, east
        [[-x, y1, -z], [-x, y1, z], [x, y1, z], [x, y1, -z]],
        [[-x, y0, z], [-x, y0, -z], [x, y0, -z], [x, y0, z]],
        [[x, y0, z], [x, y1, z], [-x, y1, z], [-x, y0, z]],
        [[-x, y0, -z], [-x, y1, -z], [x, y1, -z], [x, y0, -z]],
        [[-x, y0, z], [-x, y1, z], [-x, y1, -z], [-x, y0, -z]],
        [[x, y0, -z], [x, y1, -z], [x, y1, z], [x, y0, z]],
    ];
    let horizontal_uv = [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]];
    // All four wall faces list their vertices from the viewer's right edge to
    // left edge and from bottom to top. Rotate their texture coordinates 180
    // degrees so scanned lettering is upright and reads left-to-right.
    let vertical_uv = [[1.0, 1.0], [1.0, 0.0], [0.0, 0.0], [0.0, 1.0]];
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    let mut ranges: [std::ops::Range<u32>; 6] = std::array::from_fn(|_| 0..0);
    for (face_index, face) in faces.iter().enumerate() {
        let base = vertices.len() as u16;
        let uv = if face_index < 2 {
            &horizontal_uv
        } else {
            &vertical_uv
        };
        for (position, uv) in face.iter().zip(uv.iter()) {
            vertices.push(BoardVertex {
                position: *position,
                uv: *uv,
            });
        }
        let start = indices.len() as u32;
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        ranges[face_index] = start..indices.len() as u32;
    }
    (vertices, indices, ranges)
}

fn update_startup_panel(
    queue: &wgpu::Queue,
    scene: &StartupScene,
    startup: &StartupFlow,
    campaign: &Campaign,
) -> Result<()> {
    use image::{GenericImage, Rgba, imageops};

    let mut canvas = image::RgbaImage::from_pixel(
        STARTUP_TEXTURE_SIZE,
        STARTUP_TEXTURE_SIZE,
        Rgba([18, 9, 11, 255]),
    );
    let gold = Rgba([229, 189, 94, 255]);
    let parchment = Rgba([238, 218, 171, 255]);
    let muted = Rgba([192, 164, 120, 255]);
    let white = Rgba([245, 236, 214, 255]);

    let draw_text =
        |canvas: &mut image::RgbaImage, text: &str, x: i32, y: i32, scale: u32, color: Rgba<u8>| {
            let origin_x = x * STARTUP_TEXTURE_SIZE as i32 / STARTUP_DESIGN_SIZE as i32;
            let origin_y = y * STARTUP_TEXTURE_SIZE as i32 / STARTUP_DESIGN_SIZE as i32;
            draw_medieval_text(
                canvas,
                text,
                origin_x,
                origin_y,
                startup_font_pixel_size(scale),
                startup_font_line_advance(scale),
                color,
            );
        };
    let wrap = |text: &str, width: usize| -> String {
        let mut output = String::new();
        let mut line = 0;
        for word in text.split_whitespace() {
            if line > 0 && line + word.len() + 1 > width {
                output.push('\n');
                line = 0;
            } else if line > 0 {
                output.push(' ');
                line += 1;
            }
            output.push_str(word);
            line += word.len();
        }
        output
    };
    let fill_rect = |canvas: &mut image::RgbaImage,
                     x: u32,
                     y: u32,
                     width: u32,
                     height: u32,
                     color: Rgba<u8>| {
        let x = x * STARTUP_TEXTURE_SIZE / STARTUP_DESIGN_SIZE;
        let y = y * STARTUP_TEXTURE_SIZE / STARTUP_DESIGN_SIZE;
        let width = (width * STARTUP_TEXTURE_SIZE / STARTUP_DESIGN_SIZE).max(1);
        let height = (height * STARTUP_TEXTURE_SIZE / STARTUP_DESIGN_SIZE).max(1);
        for py in y..(y + height).min(canvas.height()) {
            for px in x..(x + width).min(canvas.width()) {
                canvas.put_pixel(px, py, color);
            }
        }
    };
    let outline_rect = |canvas: &mut image::RgbaImage,
                        x: u32,
                        y: u32,
                        width: u32,
                        height: u32,
                        thickness: u32,
                        color: Rgba<u8>| {
        fill_rect(canvas, x, y, width, thickness, color);
        fill_rect(
            canvas,
            x,
            y + height.saturating_sub(thickness),
            width,
            thickness,
            color,
        );
        fill_rect(canvas, x, y, thickness, height, color);
        fill_rect(
            canvas,
            x + width.saturating_sub(thickness),
            y,
            thickness,
            height,
            color,
        );
    };
    let draw_button = |canvas: &mut image::RgbaImage,
                       label: &str,
                       x: u32,
                       y: u32,
                       width: u32,
                       height: u32,
                       scale: u32,
                       hotspot: StartupHotspot| {
        let hovered = startup.hovered == Some(hotspot);
        fill_rect(
            canvas,
            x,
            y,
            width,
            height,
            if hovered {
                Rgba([139, 57, 39, 255])
            } else {
                Rgba([72, 30, 31, 255])
            },
        );
        outline_rect(
            canvas,
            x,
            y,
            width,
            height,
            5,
            if hovered { white } else { gold },
        );
        let text_width = (medieval_text_width(label, startup_font_pixel_size(scale)).ceil() as u32)
            * STARTUP_DESIGN_SIZE
            / STARTUP_TEXTURE_SIZE;
        let text_x = x + width.saturating_sub(text_width) / 2;
        let text_height =
            startup_font_line_advance(scale) as u32 * STARTUP_DESIGN_SIZE / STARTUP_TEXTURE_SIZE;
        let text_y = y + height.saturating_sub(text_height) / 2;
        draw_text(
            canvas,
            label,
            text_x as i32,
            text_y as i32,
            scale,
            if hovered { white } else { parchment },
        );
    };
    let blit_fit = |canvas: &mut image::RgbaImage,
                    source: &image::RgbaImage,
                    x: u32,
                    y: u32,
                    width: u32,
                    height: u32| {
        let x = x * STARTUP_TEXTURE_SIZE / STARTUP_DESIGN_SIZE;
        let y = y * STARTUP_TEXTURE_SIZE / STARTUP_DESIGN_SIZE;
        let width = (width * STARTUP_TEXTURE_SIZE / STARTUP_DESIGN_SIZE).max(1);
        let height = (height * STARTUP_TEXTURE_SIZE / STARTUP_DESIGN_SIZE).max(1);
        let scale =
            (width as f32 / source.width() as f32).min(height as f32 / source.height() as f32);
        let target_width = (source.width() as f32 * scale).round() as u32;
        let target_height = (source.height() as f32 * scale).round() as u32;
        let offset_x = x + (width.saturating_sub(target_width)) / 2;
        let offset_y = y + (height.saturating_sub(target_height)) / 2;
        if source.width() == target_width && source.height() == target_height {
            let _ = canvas.copy_from(source, offset_x, offset_y);
        } else {
            let resized = imageops::resize(
                source,
                target_width,
                target_height,
                imageops::FilterType::Triangle,
            );
            let _ = canvas.copy_from(&resized, offset_x, offset_y);
        }
    };

    match startup.stage {
        StartupStage::Armory => {
            draw_text(&mut canvas, "BETWEEN QUESTS - THE ARMORY", 90, 65, 5, gold);
            fill_rect(&mut canvas, 70, 140, 830, 1540, Rgba([42, 22, 18, 255]));
            blit_fit(&mut canvas, &scene.armory_page, 90, 165, 790, 1490);

            for (index, sheet) in campaign.heroes.iter().enumerate() {
                let x = 955 + index as u32 * 250;
                let selected = startup.armory_hero == index;
                let hovered = startup.hovered == Some(StartupHotspot::ArmoryHero(index));
                fill_rect(
                    &mut canvas,
                    x,
                    150,
                    220,
                    150,
                    if hovered {
                        Rgba([139, 57, 39, 255])
                    } else if selected {
                        Rgba([104, 38, 37, 255])
                    } else {
                        Rgba([55, 29, 31, 255])
                    },
                );
                outline_rect(
                    &mut canvas,
                    x,
                    150,
                    220,
                    150,
                    if selected { 7 } else { 3 },
                    if selected || hovered { gold } else { muted },
                );
                draw_text(&mut canvas, sheet.hero.name(), x as i32 + 18, 184, 2, white);
                draw_text(
                    &mut canvas,
                    &format!("{} GOLD", sheet.inventory.gold),
                    x as i32 + 18,
                    245,
                    2,
                    gold,
                );
            }

            for (index, listing) in ORIGINAL_US_ARMORY.iter().enumerate() {
                let column = index % 2;
                let row = index / 2;
                let x = 970 + column as u32 * 500;
                let y = 410 + row as u32 * 180;
                let selected = startup.armory_item == index;
                let hovered = startup.hovered == Some(StartupHotspot::ArmoryItem(index));
                fill_rect(
                    &mut canvas,
                    x,
                    y,
                    455,
                    145,
                    if hovered {
                        Rgba([139, 57, 39, 255])
                    } else if selected {
                        Rgba([104, 38, 37, 255])
                    } else {
                        Rgba([55, 29, 31, 255])
                    },
                );
                outline_rect(
                    &mut canvas,
                    x,
                    y,
                    455,
                    145,
                    if selected { 6 } else { 3 },
                    if selected || hovered { gold } else { muted },
                );
                draw_text(
                    &mut canvas,
                    armory_item_name(listing.item),
                    x as i32 + 20,
                    y as i32 + 28,
                    2,
                    white,
                );
                draw_text(
                    &mut canvas,
                    &format!("{} GOLD", listing.gold),
                    x as i32 + 20,
                    y as i32 + 86,
                    2,
                    gold,
                );
            }

            let selected_sheet = &campaign.heroes[startup.armory_hero % campaign.heroes.len()];
            let selected_listing =
                ORIGINAL_US_ARMORY[startup.armory_item % ORIGINAL_US_ARMORY.len()];
            let owned = match selected_listing.item {
                ArmoryItem::ToolKit => selected_sheet.inventory.tool_kits as usize,
                ArmoryItem::Weapon(weapon) => selected_sheet
                    .inventory
                    .weapons
                    .iter()
                    .filter(|&&owned| owned == weapon)
                    .count(),
                ArmoryItem::Armor(armor) => {
                    usize::from(selected_sheet.inventory.armor.contains(&armor))
                }
            };
            let equipped = match selected_listing.item {
                ArmoryItem::Weapon(weapon) => {
                    selected_sheet.inventory.equipped_weapon == Some(weapon)
                }
                ArmoryItem::Armor(
                    armor @ (crate::equipment::Armor::ChainMail
                    | crate::equipment::Armor::PlateMail),
                ) => selected_sheet.inventory.equipped_body_armor == Some(armor),
                ArmoryItem::Armor(_) | ArmoryItem::ToolKit => false,
            };
            draw_text(
                &mut canvas,
                &format!(
                    "{} selects {} - owned: {}{}",
                    selected_sheet.name,
                    armory_item_name(selected_listing.item),
                    owned,
                    if equipped { " - EQUIPPED" } else { "" }
                ),
                970,
                1510,
                2,
                parchment,
            );
            draw_text(
                &mut canvas,
                &wrap(&startup.armory_message, 70),
                970,
                1580,
                2,
                muted,
            );
            draw_button(
                &mut canvas,
                if owned > 0
                    && matches!(
                        selected_listing.item,
                        ArmoryItem::Weapon(_)
                            | ArmoryItem::Armor(
                                crate::equipment::Armor::ChainMail
                                    | crate::equipment::Armor::PlateMail
                            )
                    )
                    && !(matches!(
                        selected_listing.item,
                        ArmoryItem::Weapon(crate::equipment::Weapon::Dagger)
                    ) && equipped)
                {
                    "EQUIP"
                } else {
                    "PURCHASE"
                },
                1010,
                1740,
                420,
                250,
                3,
                StartupHotspot::ArmoryPurchase,
            );
            draw_button(
                &mut canvas,
                "NEXT QUEST",
                1490,
                1740,
                460,
                250,
                3,
                StartupHotspot::Confirm,
            );
        }
        StartupStage::QuestSelection => {
            fill_rect(&mut canvas, 70, 90, 1908, 1180, Rgba([49, 18, 25, 255]));
            blit_fit(
                &mut canvas,
                &scene.quest_pages[startup.selected_quest],
                110,
                130,
                1828,
                1100,
            );
            draw_text(
                &mut canvas,
                &format!("QUEST {:02} OF 14", startup.selected_quest + 1),
                110,
                1330,
                5,
                gold,
            );
            draw_text(&mut canvas, startup.quest_title(), 110, 1400, 5, white);
            draw_text(
                &mut canvas,
                &wrap(startup.quest_blurb(), 63),
                110,
                1490,
                3,
                muted,
            );
            if startup.selected_quest == 0 {
                draw_text(&mut canvas, "NEW CAMPAIGNS BEGIN HERE", 110, 1710, 3, gold);
            }
            draw_button(
                &mut canvas,
                "BACK",
                90,
                1810,
                430,
                180,
                3,
                StartupHotspot::Back,
            );
            draw_button(
                &mut canvas,
                "PREVIOUS",
                660,
                1780,
                250,
                210,
                2,
                StartupHotspot::PreviousQuest,
            );
            draw_button(
                &mut canvas,
                "NEXT",
                980,
                1780,
                250,
                210,
                2,
                StartupHotspot::NextQuest,
            );
            draw_button(
                &mut canvas,
                "PLAY QUEST",
                1400,
                1780,
                550,
                210,
                3,
                StartupHotspot::Confirm,
            );
        }
        StartupStage::PlayerSetup => {
            fill_rect(&mut canvas, 90, 90, 820, 1450, Rgba([71, 35, 29, 255]));
            blit_fit(
                &mut canvas,
                &scene.hero_cards[crate::startup::HERO_ORDER
                    .iter()
                    .position(|&hero| hero == startup.heroes[startup.active_hero].hero)
                    .unwrap_or_default()],
                110,
                110,
                780,
                1410,
            );
            draw_text(&mut canvas, "CHOOSE YOUR ROLES", 980, 90, 4, gold);
            draw_text(
                &mut canvas,
                &format!("HERO PLAYERS: {}", startup.player_count),
                980,
                215,
                3,
                white,
            );
            draw_button(
                &mut canvas,
                "-",
                1510,
                170,
                150,
                130,
                5,
                StartupHotspot::RemovePlayer,
            );
            draw_button(
                &mut canvas,
                "+",
                1720,
                170,
                150,
                130,
                5,
                StartupHotspot::AddPlayer,
            );
            draw_text(
                &mut canvas,
                &wrap(
                    "The computer is Zargon. Every quest uses all four Heroes; one person may control more than one.",
                    52,
                ),
                980,
                340,
                2,
                muted,
            );
            for (index, hero) in startup.heroes.iter().enumerate() {
                let selected = index == startup.active_hero;
                let hovered = matches!(
                    startup.hovered,
                    Some(StartupHotspot::Hero(h) | StartupHotspot::HeroOwner(h)) if h == index
                );
                let y = 520 + index as i32 * 185;
                if selected || hovered {
                    fill_rect(
                        &mut canvas,
                        955,
                        (y - 28) as u32,
                        990,
                        142,
                        if hovered {
                            Rgba([126, 48, 39, 255])
                        } else {
                            Rgba([104, 38, 37, 255])
                        },
                    );
                }
                draw_text(
                    &mut canvas,
                    hero.hero.name(),
                    990,
                    y,
                    3,
                    if selected { gold } else { white },
                );
                draw_text(
                    &mut canvas,
                    &format!("Name: {}", hero.hero_name),
                    990,
                    y + 60,
                    2,
                    muted,
                );
                draw_button(
                    &mut canvas,
                    &format!("PLAYER {} >", hero.player_number),
                    1640,
                    (y - 28) as u32,
                    305,
                    142,
                    2,
                    StartupHotspot::HeroOwner(index),
                );
            }
            draw_text(
                &mut canvas,
                "Click a Hero to name it. Click PLAYER to change its owner.",
                980,
                1360,
                2,
                parchment,
            );
            draw_text(
                &mut canvas,
                "Type to edit the selected name; Backspace deletes.",
                980,
                1420,
                2,
                parchment,
            );
            draw_text(
                &mut canvas,
                "ORDER FROM ZARGON'S LEFT, THEN CLOCKWISE",
                980,
                1500,
                2,
                muted,
            );
            draw_button(
                &mut canvas,
                "Q  EARLIER",
                980,
                1580,
                255,
                145,
                2,
                StartupHotspot::MoveHeroEarlier,
            );
            draw_button(
                &mut canvas,
                "E  LATER",
                1270,
                1580,
                255,
                145,
                2,
                StartupHotspot::MoveHeroLater,
            );
            draw_button(
                &mut canvas,
                "BACK",
                90,
                1810,
                430,
                180,
                3,
                StartupHotspot::Back,
            );
            draw_button(
                &mut canvas,
                "CHOOSE SPELLS",
                1450,
                1780,
                500,
                210,
                3,
                StartupHotspot::Confirm,
            );
        }
        StartupStage::WizardSpellChoice | StartupStage::ElfSpellChoice => {
            let choosing_wizard = startup.stage == StartupStage::WizardSpellChoice;
            draw_text(
                &mut canvas,
                if choosing_wizard {
                    "DIVIDE THE HERO SPELLS - WIZARD CHOOSES FIRST"
                } else {
                    "DIVIDE THE HERO SPELLS - ELF CHOOSES NEXT"
                },
                130,
                150,
                4,
                gold,
            );
            draw_text(
                &mut canvas,
                if choosing_wizard {
                    "Choose one of the four elemental spell groups."
                } else {
                    "Choose one of the three remaining groups. The Wizard receives the final two."
                },
                130,
                250,
                2,
                muted,
            );
            for (index, group) in SpellGroup::ALL.iter().enumerate() {
                let available = choosing_wizard || *group != startup.wizard_first;
                let selected = if choosing_wizard {
                    *group == startup.wizard_first
                } else {
                    *group == startup.elf_spells
                };
                let x = 160 + (index % 2) as u32 * 920;
                let y = 430 + (index / 2) as u32 * 560;
                let hovered = startup.hovered == Some(StartupHotspot::Spell(*group));
                fill_rect(
                    &mut canvas,
                    x,
                    y,
                    800,
                    440,
                    if hovered {
                        Rgba([146, 59, 39, 255])
                    } else if selected {
                        Rgba([121, 46, 31, 255])
                    } else if available {
                        Rgba([60, 31, 34, 255])
                    } else {
                        Rgba([28, 23, 25, 255])
                    },
                );
                outline_rect(
                    &mut canvas,
                    x,
                    y,
                    800,
                    440,
                    if hovered { 8 } else { 4 },
                    if hovered || selected { gold } else { muted },
                );
                draw_text(
                    &mut canvas,
                    group.name(),
                    x as i32 + 70,
                    y as i32 + 120,
                    8,
                    if selected { gold } else { white },
                );
                if !available {
                    draw_text(
                        &mut canvas,
                        "Chosen by Wizard",
                        x as i32 + 70,
                        y as i32 + 260,
                        3,
                        muted,
                    );
                }
            }
            draw_button(
                &mut canvas,
                "BACK",
                90,
                1810,
                430,
                180,
                3,
                StartupHotspot::Back,
            );
            draw_button(
                &mut canvas,
                "CONFIRM",
                1450,
                1780,
                500,
                210,
                3,
                StartupHotspot::Confirm,
            );
        }
        StartupStage::Ready => {
            draw_text(&mut canvas, "READY FOR THE QUEST", 400, 170, 6, gold);
            draw_text(
                &mut canvas,
                &format!(
                    "QUEST {}: {}",
                    startup.selected_quest + 1,
                    startup.quest_title()
                ),
                170,
                340,
                4,
                white,
            );
            draw_text(
                &mut canvas,
                "The computer assumes the role of Zargon and keeps the Quest Map, traps,\nsecret doors, treasure, and Quest Notes hidden until they are revealed.",
                170,
                440,
                2,
                muted,
            );
            draw_text(&mut canvas, "HEROES", 170, 620, 4, gold);
            for (index, hero) in startup.heroes.iter().enumerate() {
                draw_text(
                    &mut canvas,
                    &format!(
                        "{} - {} - Player {}",
                        hero.hero.name(),
                        hero.hero_name,
                        hero.player_number
                    ),
                    200,
                    720 + index as i32 * 90,
                    3,
                    white,
                );
            }
            let wizard = startup
                .wizard_spells()
                .iter()
                .map(|group| group.name())
                .collect::<Vec<_>>()
                .join(", ");
            draw_text(
                &mut canvas,
                &format!("Wizard spells: {wizard}"),
                200,
                1160,
                3,
                white,
            );
            draw_text(
                &mut canvas,
                &format!("Elf spells: {}", startup.elf_spells.name()),
                200,
                1235,
                3,
                white,
            );
            draw_text(
                &mut canvas,
                "Only the parchment introduction will be revealed before play.",
                200,
                1390,
                2,
                muted,
            );
            draw_button(
                &mut canvas,
                "BACK",
                90,
                1810,
                430,
                180,
                3,
                StartupHotspot::Back,
            );
            draw_button(
                &mut canvas,
                "BEGIN QUEST",
                1320,
                1740,
                630,
                250,
                4,
                StartupHotspot::Confirm,
            );
        }
        _ => {}
    }

    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &scene.panel_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        canvas.as_raw(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * STARTUP_TEXTURE_SIZE),
            rows_per_image: Some(STARTUP_TEXTURE_SIZE),
        },
        wgpu::Extent3d {
            width: STARTUP_TEXTURE_SIZE,
            height: STARTUP_TEXTURE_SIZE,
            depth_or_array_layers: 1,
        },
    );
    Ok(())
}

fn board_world(pos: Pos) -> Vec3 {
    Vec3::new(
        pos.x as f32 - (BOARD_WIDTH as f32 - 1.0) * 0.5,
        0.0,
        pos.y as f32 - (BOARD_HEIGHT as f32 - 1.0) * 0.5,
    )
}

/// Shared stair and pit squares are legal in the US rules. Fan the physical
/// figures within that one-inch board square so every occupant remains
/// readable, and lower figures marked as being in a pit without hiding their
/// bases beneath the board surface.
fn unit_board_world(game: &Game, unit: &Unit) -> Vec3 {
    let mut target = board_world(unit.pos);
    let occupants = game
        .units
        .iter()
        .filter(|candidate| candidate.alive && !candidate.escaped && candidate.pos == unit.pos)
        .collect::<Vec<_>>();
    if occupants.len() > 1
        && let Some(index) = occupants
            .iter()
            .position(|candidate| candidate.id == unit.id)
    {
        let angle = std::f32::consts::TAU * index as f32 / occupants.len() as f32
            + std::f32::consts::FRAC_PI_4;
        target += Vec3::new(angle.cos() * 0.22, 0.0, angle.sin() * 0.22);
    }
    if unit.in_pit {
        target.y -= 0.12;
    }
    target
}

fn direction_world_delta(direction: Direction) -> Vec3 {
    match direction {
        Direction::North => -Vec3::Z,
        Direction::East => Vec3::X,
        Direction::South => Vec3::Z,
        Direction::West => -Vec3::X,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TabletopHitTarget {
    Player(TabletopSurface),
    ElfSpell(usize),
    WizardSpell(usize),
    ElfDiscard,
    WizardDiscard,
    ChaosDiscard,
    Zargon(TabletopSurface),
}

#[derive(Debug, Clone, Copy)]
struct PointerRay {
    near: Vec3,
    delta: Vec3,
}

fn pointer_ray_at_screen(
    camera: &Camera,
    aspect: f32,
    x: f32,
    y: f32,
    input_width: u32,
    input_height: u32,
) -> Option<PointerRay> {
    if input_width == 0 || input_height == 0 {
        return None;
    }
    let ndc_x = x / input_width as f32 * 2.0 - 1.0;
    let ndc_y = 1.0 - y / input_height as f32 * 2.0;
    let inverse = camera.matrix(aspect).inverse();
    let near = inverse.project_point3(Vec3::new(ndc_x, ndc_y, 0.0));
    let far = inverse.project_point3(Vec3::new(ndc_x, ndc_y, 1.0));
    Some(PointerRay {
        near,
        delta: far - near,
    })
}

fn pointer_ray_hits_quad(
    ray: PointerRay,
    center: Vec3,
    angle: f32,
    half_width: f32,
    half_height: f32,
    plane_y: f32,
) -> bool {
    if ray.delta.y.abs() < 0.0001 {
        return false;
    }
    let distance = (plane_y - ray.near.y) / ray.delta.y;
    if distance < 0.0 {
        return false;
    }
    let hit = ray.near + ray.delta * distance;
    let offset = hit - center;
    let right = Vec3::new(angle.cos(), 0.0, angle.sin());
    let down = Vec3::new(-angle.sin(), 0.0, angle.cos());
    offset.dot(right).abs() <= half_width && offset.dot(down).abs() <= half_height
}

fn tabletop_hit_target_at_screen(
    camera: &Camera,
    aspect: f32,
    x: f32,
    y: f32,
    input_width: u32,
    input_height: u32,
) -> Option<TabletopHitTarget> {
    let ray = pointer_ray_at_screen(camera, aspect, x, y, input_width, input_height)?;

    // Spell cards sit above the other player materials and are tested first.
    let (elf_center, elf_layout_angle) = player_station_layout(2);
    let elf_angle = player_station_card_angle(2);
    for slot in 0..3 {
        let center = station_point(
            elf_center,
            elf_layout_angle,
            -0.80 + slot as f32 * 0.80,
            -2.03,
        );
        if pointer_ray_hits_quad(
            ray,
            center,
            elf_angle + (slot as f32 - 1.0) * 0.055,
            0.38,
            0.57,
            0.085 + slot as f32 * 0.002,
        ) {
            return Some(TabletopHitTarget::ElfSpell(slot));
        }
    }
    let (wizard_center, wizard_layout_angle) = player_station_layout(3);
    let wizard_angle = player_station_card_angle(3);
    for slot in 0..9 {
        let center = station_point(
            wizard_center,
            wizard_layout_angle,
            -2.12 + slot as f32 * 0.52,
            -2.05 + (slot as f32 - 4.0).abs() * 0.025,
        );
        if pointer_ray_hits_quad(
            ray,
            center,
            wizard_angle + (slot as f32 - 4.0) * 0.025,
            0.31,
            0.48,
            0.084 + slot as f32 * 0.001,
        ) {
            return Some(TabletopHitTarget::WizardSpell(slot));
        }
    }
    if pointer_ray_hits_quad(
        ray,
        station_point(elf_center, elf_layout_angle, 1.75, -2.03),
        elf_angle + 0.08,
        0.38,
        0.57,
        0.095,
    ) {
        return Some(TabletopHitTarget::ElfDiscard);
    }
    if pointer_ray_hits_quad(
        ray,
        station_point(wizard_center, wizard_layout_angle, 2.90, -2.05),
        wizard_angle + 0.10,
        0.38,
        0.57,
        0.095,
    ) {
        return Some(TabletopHitTarget::WizardDiscard);
    }

    let heroes = [
        HeroKind::Barbarian,
        HeroKind::Dwarf,
        HeroKind::Elf,
        HeroKind::Wizard,
    ];
    for (station, hero) in heroes.into_iter().enumerate() {
        let (center, layout_angle) = player_station_layout(station);
        let card_angle = player_station_card_angle(station);
        for (surface, local_x, local_z, half_width, half_height, plane_y) in [
            (
                TabletopSurface::HeroCard(hero),
                -1.65,
                -0.20,
                0.69,
                1.06,
                0.060,
            ),
            (
                TabletopSurface::CharacterSheet(hero),
                -0.05,
                -0.20,
                0.78,
                1.18,
                0.062,
            ),
            (
                TabletopSurface::ActionReference(hero),
                -1.65,
                1.72,
                0.70,
                0.40,
                0.064,
            ),
        ] {
            if pointer_ray_hits_quad(
                ray,
                station_point(center, layout_angle, local_x, local_z),
                card_angle,
                half_width,
                half_height,
                plane_y,
            ) {
                return Some(TabletopHitTarget::Player(surface));
            }
        }
    }
    if pointer_ray_hits_quad(
        ray,
        Vec3::new(0.0, 0.0, -17.45),
        std::f32::consts::PI,
        4.0,
        3.25,
        0.058,
    ) {
        return Some(TabletopHitTarget::Player(TabletopSurface::Armory));
    }

    if pointer_ray_hits_quad(ray, Vec3::new(-7.6, 0.0, 19.65), 0.08, 0.62, 0.94, 0.103) {
        return Some(TabletopHitTarget::ChaosDiscard);
    }

    for (deck, x) in [
        (ZargonDeckKind::Treasure, -12.0_f32),
        (ZargonDeckKind::Artifact, -9.8),
        (ZargonDeckKind::ChaosSpell, -7.6),
        (ZargonDeckKind::Monster, -5.4),
    ] {
        if pointer_ray_hits_quad(ray, Vec3::new(x, 0.0, 17.55), 0.0, 0.62, 0.94, 0.181) {
            return Some(TabletopHitTarget::Zargon(TabletopSurface::ZargonDeck(deck)));
        }
    }
    let monsters = crate::model::ORIGINAL_US_MONSTER_CARDS;
    for (index, monster) in monsters.into_iter().enumerate() {
        let column = index % 4;
        let row = index / 4;
        if pointer_ray_hits_quad(
            ray,
            Vec3::new(7.15 + column as f32 * 1.52, 0.0, 17.25 + row as f32 * 2.05),
            0.0,
            0.64,
            0.94,
            0.097,
        ) {
            return Some(TabletopHitTarget::Zargon(TabletopSurface::MonsterCard(
                monster,
            )));
        }
    }
    if pointer_ray_hits_quad(
        ray,
        Vec3::new(0.0, 0.0, 18.25),
        ZARGON_QUEST_BOOK_ANGLE,
        3.62,
        2.30,
        0.107,
    ) {
        return Some(TabletopHitTarget::Zargon(TabletopSurface::QuestBook));
    }
    None
}

fn board_pos_at_screen(
    camera: &Camera,
    aspect: f32,
    x: f32,
    y: f32,
    input_width: u32,
    input_height: u32,
) -> Option<Pos> {
    let ray = pointer_ray_at_screen(camera, aspect, x, y, input_width, input_height)?;
    if ray.delta.y.abs() < 0.0001 {
        return None;
    }
    let distance = (0.06 - ray.near.y) / ray.delta.y;
    if distance < 0.0 {
        return None;
    }
    let hit = ray.near + ray.delta * distance;
    let board_x = (hit.x + (BOARD_WIDTH as f32 - 1.0) * 0.5).round();
    let board_y = (hit.z + (BOARD_HEIGHT as f32 - 1.0) * 0.5).round();
    if !(0.0..BOARD_WIDTH as f32).contains(&board_x)
        || !(0.0..BOARD_HEIGHT as f32).contains(&board_y)
        || (hit.x - (board_x - (BOARD_WIDTH as f32 - 1.0) * 0.5)).abs() > 0.5
        || (hit.z - (board_y - (BOARD_HEIGHT as f32 - 1.0) * 0.5)).abs() > 0.5
    {
        return None;
    }
    Some(Pos::new(board_x as u8, board_y as u8))
}

/// Finds a legal spell target by the visible figure or door itself rather
/// than by intersecting the pointer ray with the board beneath it. At an
/// oblique camera angle the latter can place the upper half of a miniature
/// several squares behind its base.
fn hero_spell_target_at_screen(
    camera: &Camera,
    game: &Game,
    spell: HeroSpell,
    aspect: f32,
    x: f32,
    y: f32,
    input_width: u32,
    input_height: u32,
) -> Option<HeroSpellTarget> {
    if input_width == 0 || input_height == 0 {
        return None;
    }

    let matrix = camera.matrix(aspect);
    let pointer = glam::Vec2::new(x, y);
    let screen_point = |world: Vec3| {
        let projected = matrix.project_point3(world);
        if !projected.is_finite() || !(0.0..=1.0).contains(&projected.z) {
            return None;
        }
        Some(glam::Vec2::new(
            (projected.x + 1.0) * 0.5 * input_width as f32,
            (1.0 - projected.y) * 0.5 * input_height as f32,
        ))
    };
    let distance_to_segment = |point: glam::Vec2, start: glam::Vec2, end: glam::Vec2| {
        let delta = end - start;
        let length_squared = delta.length_squared();
        if length_squared <= f32::EPSILON {
            return point.distance(start);
        }
        let amount = ((point - start).dot(delta) / length_squared).clamp(0.0, 1.0);
        point.distance(start + delta * amount)
    };

    game.valid_hero_spell_targets(spell)
        .into_iter()
        .filter_map(|target| {
            let (base, height, width_factor) = match target {
                HeroSpellTarget::Unit(id) => {
                    let unit = game.unit(id)?;
                    (unit_board_world(game, unit), 1.45, 0.30)
                }
                HeroSpellTarget::Door(index) => {
                    let door = game.doors.get(index)?;
                    (
                        (board_world(door.a) + board_world(door.b)) * 0.5,
                        1.38,
                        0.42,
                    )
                }
            };
            let screen_base = screen_point(base + Vec3::Y * 0.06)?;
            let screen_top = screen_point(base + Vec3::Y * height)?;
            let projected_height = screen_base.distance(screen_top);
            let radius = (projected_height * width_factor).clamp(14.0, 48.0);
            let distance = distance_to_segment(pointer, screen_base, screen_top);
            (distance <= radius).then_some((target, distance / radius))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(target, _)| target)
}

fn prop_world_position(kind: PropKind, pos: Pos, rotation_quarters: u8, carried: bool) -> Vec3 {
    let anchor = board_world(pos);
    if carried {
        return anchor;
    }
    // Quest data stores the upper-left occupied square. Center the model over
    // every square printed beneath the assembled component, after rotation.
    let (width, height) = kind.footprint(rotation_quarters);
    anchor + Vec3::new((width - 1) as f32 * 0.5, 0.0, (height - 1) as f32 * 0.5)
}

fn prop_model_local_rotation(kind: PropKind) -> Quat {
    // These source GLBs were authored with their long side on local Z, while
    // Quest placements and scan inserts use local X as the printed width.
    if matches!(kind, PropKind::Table | PropKind::Bookcase | PropKind::Tomb) {
        Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)
    } else {
        Quat::IDENTITY
    }
}

fn player_station_layout(index: usize) -> (Vec3, f32) {
    match index {
        0 => (Vec3::new(-16.2, 0.0, -3.7), std::f32::consts::FRAC_PI_2),
        1 => (Vec3::new(-7.1, 0.0, -12.35), 0.0),
        2 => (Vec3::new(7.1, 0.0, -12.35), 0.0),
        3 => (Vec3::new(16.2, 0.0, -3.7), -std::f32::consts::FRAC_PI_2),
        _ => unreachable!("there are exactly four Hero stations"),
    }
}

fn dice_roll_transform(game: &Game, roller: Option<UnitId>) -> DiceRollTransform {
    let hero_station = roller
        .and_then(|roller| game.hero_order.iter().position(|&hero| hero == roller))
        .or(match (game.phase, roller) {
            (
                GamePhase::AllyTurn {
                    ally,
                    controller_order_index,
                },
                Some(roller),
            ) if ally == roller => Some(controller_order_index),
            _ => None,
        });
    if let Some(station) = hero_station {
        let (center, angle) = player_station_layout(station);
        DiceRollTransform {
            center: center + Vec3::Y * 0.08,
            rotation: Quat::from_rotation_y(-angle),
            station: Some(station),
        }
    } else {
        DiceRollTransform {
            center: ZARGON_DICE_ROLL_CENTER,
            rotation: Quat::from_rotation_y(std::f32::consts::PI),
            station: None,
        }
    }
}

fn player_station_card_angle(index: usize) -> f32 {
    let (_, angle) = player_station_layout(index);
    if matches!(index, 1 | 2) {
        angle + std::f32::consts::PI
    } else {
        angle
    }
}

fn player_station_die_pose(station: usize, die_index: usize) -> (Vec3, Quat, DieKind, f32) {
    let (center, angle) = player_station_layout(station);
    let (x, z, kind) = PLAYER_DICE_RACK[die_index];
    (
        station_point(center, angle, x, z) + Vec3::Y * (0.08 + PLAYER_DICE_RACK_LOCAL_Y),
        Quat::from_rotation_y(-angle) * Quat::from_rotation_y(die_index as f32 * 0.31),
        kind,
        1.0,
    )
}

fn rack_die_is_rolling(
    station: usize,
    die_index: usize,
    rolling_station: Option<usize>,
    rolling_dice: &[DiePose],
) -> bool {
    if rolling_station != Some(station) {
        return false;
    }
    let kind = PLAYER_DICE_RACK[die_index].2;
    let same_kind_before = PLAYER_DICE_RACK[..die_index]
        .iter()
        .filter(|entry| entry.2 == kind)
        .count();
    let rolling_count = rolling_dice.iter().filter(|die| die.kind == kind).count();
    same_kind_before < rolling_count
}

fn visible_die_poses(dice: &[DiePose], rolling_station: Option<usize>) -> Vec<DiePose> {
    let mut visible = dice.to_vec();
    for station in 0..4 {
        for die_index in 0..PLAYER_DICE_RACK.len() {
            if rack_die_is_rolling(station, die_index, rolling_station, dice) {
                continue;
            }
            let (translation, rotation, kind, _) = player_station_die_pose(station, die_index);
            visible.push(DiePose {
                kind,
                translation,
                rotation,
            });
        }
    }
    visible
}

fn station_point(center: Vec3, angle: f32, x: f32, z: f32) -> Vec3 {
    let right = Vec3::new(angle.cos(), 0.0, angle.sin());
    let down = Vec3::new(-angle.sin(), 0.0, angle.cos());
    center + right * x + down * z
}

fn cube(
    instances: &mut Vec<InstanceRaw>,
    center: Vec3,
    scale: Vec3,
    rotation: Quat,
    color: [f32; 3],
) {
    let model = Mat4::from_scale_rotation_translation(scale, rotation, center);
    instances.push(InstanceRaw::new(model, color));
}

fn die_face_rotation(face: u8) -> Quat {
    match face {
        1 => Quat::IDENTITY,
        2 => Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        3 => Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2),
        4 => Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
        5 => Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        6 => Quat::from_rotation_x(std::f32::consts::PI),
        _ => unreachable!("a die has exactly six faces"),
    }
}

/// Piece GLBs are normalized with their origin at the bottom center, while a
/// Rapier die pose is its rigid-body center. The bottom-center offset must be
/// rotated with the body. Subtracting world Y after rotation made the mesh
/// orbit a different point than its face decals, so the printed symbols
/// visibly detached whenever a die tumbled.
fn die_model_transform(center: Vec3, rotation: Quat) -> Mat4 {
    Mat4::from_rotation_translation(rotation, center + rotation * (-Vec3::Y * DIE_HALF_EXTENT))
}

fn combat_decal_kind(face: u8) -> DiceDecalKind {
    match face {
        1..=3 => DiceDecalKind::Skull,
        4 | 5 => DiceDecalKind::WhiteShield,
        6 => DiceDecalKind::BlackShield,
        _ => unreachable!("a die has exactly six faces"),
    }
}

fn combat_decal_instance(
    center: Vec3,
    die_rotation: Quat,
    face: u8,
    scale: f32,
) -> SpriteInstanceRaw {
    let face_rotation = die_face_rotation(face);
    let rotation = die_rotation * face_rotation;
    let translation = center + die_rotation * (face_rotation * Vec3::new(0.0, 0.326 * scale, 0.0));
    SpriteInstanceRaw::new(Mat4::from_scale_rotation_translation(
        Vec3::new(0.47 * scale, 1.0, 0.47 * scale),
        rotation,
        translation,
    ))
}

fn movement_pip_instance(
    center: Vec3,
    die_rotation: Quat,
    face: u8,
    x: f32,
    z: f32,
    scale: f32,
) -> SpriteInstanceRaw {
    let face_rotation = die_face_rotation(face);
    let rotation = die_rotation * face_rotation;
    let translation =
        center + die_rotation * (face_rotation * Vec3::new(x * scale, 0.326 * scale, z * scale));
    SpriteInstanceRaw::new(Mat4::from_scale_rotation_translation(
        Vec3::new(0.118 * scale, 1.0, 0.118 * scale),
        rotation,
        translation,
    ))
}

fn movement_pips(face: u8) -> &'static [(f32, f32)] {
    const ONE: &[(f32, f32)] = &[(0.0, 0.0)];
    const TWO: &[(f32, f32)] = &[(-0.105, -0.105), (0.105, 0.105)];
    const THREE: &[(f32, f32)] = &[(-0.115, -0.115), (0.0, 0.0), (0.115, 0.115)];
    const FOUR: &[(f32, f32)] = &[(-0.11, -0.11), (0.11, -0.11), (-0.11, 0.11), (0.11, 0.11)];
    const FIVE: &[(f32, f32)] = &[
        (-0.12, -0.12),
        (0.12, -0.12),
        (0.0, 0.0),
        (-0.12, 0.12),
        (0.12, 0.12),
    ];
    const SIX: &[(f32, f32)] = &[
        (-0.115, -0.13),
        (0.115, -0.13),
        (-0.115, 0.0),
        (0.115, 0.0),
        (-0.115, 0.13),
        (0.115, 0.13),
    ];
    match face {
        1 => ONE,
        2 => TWO,
        3 => THREE,
        4 => FOUR,
        5 => FIVE,
        6 => SIX,
        _ => unreachable!("a die has exactly six faces"),
    }
}

fn build_scene(
    game: &Game,
    has_environment_mesh: bool,
    has_scanned_board: bool,
) -> Vec<InstanceRaw> {
    let mut out = Vec::with_capacity(3_000);
    if !has_environment_mesh {
        add_castle_room(&mut out);
    }
    add_player_station_props(&mut out, game);
    add_zargon_station_props(&mut out);
    let stone = [0.12, 0.085, 0.09];
    for y in 0..BOARD_HEIGHT {
        for x in 0..BOARD_WIDTH {
            let pos = Pos::new(x, y);
            let cell = &game.cells[Game::cell_index(pos)];
            let mut color = cell.tint;
            if !cell.passable {
                color = [0.07, 0.052, 0.062];
            }
            if !has_scanned_board {
                cube(
                    &mut out,
                    board_world(pos) + Vec3::new(0.0, -0.09, 0.0),
                    Vec3::new(0.47, 0.09, 0.47),
                    Quat::IDENTITY,
                    color,
                );
            }
        }
    }

    // The scan already contains the exact printed room boundaries. Generated
    // relief walls are only useful with the synthetic fallback board; drawing
    // them over the scan creates a second, inevitably misregistered wall map.
    if !has_scanned_board {
        for y in 0..BOARD_HEIGHT {
            for x in 0..BOARD_WIDTH {
                let pos = Pos::new(x, y);
                let cell = &game.cells[Game::cell_index(pos)];
                if !cell.passable {
                    continue;
                }
                for direction in [Direction::East, Direction::South] {
                    let Some(next) = pos.offset(direction) else {
                        continue;
                    };
                    let next_cell = &game.cells[Game::cell_index(next)];
                    let door_exists = game.has_door(pos, next).is_some();
                    let needs_wall =
                        !next_cell.passable || (cell.region != next_cell.region && !door_exists);
                    if needs_wall {
                        let a = board_world(pos);
                        let b = board_world(next);
                        let center = (a + b) * 0.5 + Vec3::Y * 0.075;
                        let scale = if direction == Direction::East {
                            Vec3::new(0.065, 0.075, 0.49)
                        } else {
                            Vec3::new(0.49, 0.075, 0.065)
                        };
                        cube(&mut out, center, scale, Quat::IDENTITY, stone);
                    }
                }
            }
        }
    }

    let frame = [0.28, 0.13, 0.055];
    if !has_scanned_board {
        cube(
            &mut out,
            Vec3::new(0.0, 0.02, -9.63),
            Vec3::new(13.65, 0.24, 0.18),
            Quat::IDENTITY,
            frame,
        );
        cube(
            &mut out,
            Vec3::new(0.0, 0.02, 9.63),
            Vec3::new(13.65, 0.24, 0.18),
            Quat::IDENTITY,
            frame,
        );
        cube(
            &mut out,
            Vec3::new(-13.13, 0.02, 0.0),
            Vec3::new(0.18, 0.24, 9.45),
            Quat::IDENTITY,
            frame,
        );
        cube(
            &mut out,
            Vec3::new(13.13, 0.02, 0.0),
            Vec3::new(0.18, 0.24, 9.45),
            Quat::IDENTITY,
            frame,
        );
    }

    // Figures, furniture, doors, markers, damage counters, and removable
    // dressing are all required classic GLBs rendered by PieceModels. Missing
    // physical content is a load error; this scene no longer invents cubes as
    // substitutes for original-US pieces.

    if let Some(pos) = game.mine_entrance
        && game.cells[Game::cell_index(pos)].revealed
    {
        let center = board_world(pos);
        cube(
            &mut out,
            center + Vec3::Y * 0.032,
            Vec3::new(0.44, 0.022, 0.44),
            Quat::IDENTITY,
            [0.055, 0.040, 0.025],
        );
        for step in 0..4 {
            cube(
                &mut out,
                center + Vec3::new(0.0, 0.055 + step as f32 * 0.008, -0.28 + step as f32 * 0.16),
                Vec3::new(0.34 - step as f32 * 0.025, 0.018, 0.065),
                Quat::IDENTITY,
                [0.38, 0.24, 0.075],
            );
        }
    }

    out
}

fn build_highlights(
    game: &Game,
    selection_highlights: &[Pos],
    hovered_move_target: Option<Pos>,
    animation_time: f32,
) -> Vec<InstanceRaw> {
    let mut out = Vec::new();
    let move_destinations = game.active_move_destinations();
    let hovered_move_target =
        hovered_move_target.filter(|target| move_destinations.contains(target));
    if selection_highlights.is_empty() {
        for target in move_destinations
            .iter()
            .copied()
            .filter(|target| Some(*target) != hovered_move_target)
        {
            add_square_highlight(&mut out, target, [1.00, 0.42, 0.015], animation_time);
        }
        if let Some(hero_pos) = game.active_hero().map(|hero| hero.pos) {
            for target in game
                .adjacent_closed_door_indices()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|index| game.doors.get(index)?.other_side(hero_pos))
            {
                add_square_highlight(&mut out, target, [0.28, 0.84, 1.00], animation_time);
            }
        }
    }
    for &target in selection_highlights {
        add_square_highlight(&mut out, target, [0.28, 0.84, 1.00], animation_time);
    }
    if let Some(target) = hovered_move_target {
        add_square_highlight(&mut out, target, [0.16, 0.66, 1.00], animation_time + 0.24);
    }
    for target in visible_nonphysical_quest_item_positions(game) {
        add_square_highlight(&mut out, target, [1.00, 0.76, 0.08], animation_time + 0.68);
    }
    if let Some(hero) = game.active_hero().filter(|hero| game.is_visible(hero)) {
        add_active_hero_halo(&mut out, hero.pos, animation_time);
    }
    out
}

fn visible_nonphysical_quest_item_positions(game: &Game) -> Vec<Pos> {
    game.quest_items
        .iter()
        .filter(|item| !item.delivered && item.holder.is_none())
        .filter_map(|item| game.props.get(item.prop_index))
        .filter(|prop| {
            !prop.kind.is_original_us_physical_component()
                && prop.carried_by.is_none()
                && game.is_prop_visible(prop)
        })
        .map(|prop| prop.pos)
        .collect()
}

fn monster_damage_marker_count(unit: &Unit) -> u8 {
    if !unit.alive || unit.faction != crate::model::Faction::Monster || unit.stats.body <= 1 {
        return 0;
    }
    (unit.stats.body as i16 - unit.body).clamp(0, u8::MAX as i16) as u8
}

fn damage_marker_positions(center: Vec3, unit: &Unit) -> Vec<Vec3> {
    (0..monster_damage_marker_count(unit))
        .map(|marker| {
            let column = marker % 3;
            let row = marker / 3;
            center
                + Vec3::new(
                    -0.25 + column as f32 * 0.25,
                    0.075,
                    0.34 - row as f32 * 0.22,
                )
        })
        .collect()
}

fn add_square_highlight(
    out: &mut Vec<InstanceRaw>,
    target: Pos,
    color: [f32; 3],
    animation_time: f32,
) {
    // Fill the entire legal square while retaining the board art underneath.
    // Both emitted brightness and alpha breathe, making the valid area much
    // louder without reintroducing an X or an opaque tile.
    let wave = (animation_time * 4.6).sin().mul_add(0.5, 0.5);
    let pulse = 0.70 + 0.30 * wave;
    let glow = [
        (color[0] * pulse).min(1.0),
        (color[1] * pulse).min(1.0),
        (color[2] * pulse).min(1.0),
    ];
    let center = board_world(target) + Vec3::Y * (0.075 + wave * 0.006);
    out.push(InstanceRaw::with_alpha(
        Mat4::from_scale_rotation_translation(
            Vec3::new(0.455, 0.008, 0.455),
            Quat::IDENTITY,
            center,
        ),
        glow,
        0.18 + wave * 0.30,
    ));
}

fn add_active_hero_halo(out: &mut Vec<InstanceRaw>, target: Pos, animation_time: f32) {
    let wave = (animation_time * 3.8).sin().mul_add(0.5, 0.5);
    // Keep the ring above either the scanned or procedural board, but well
    // below the movement overlay and the figure base. The former 0.094 height
    // put its faces almost coplanar with the movement highlight.
    let center = board_world(target) + Vec3::Y * 0.035;
    let radius = 0.43 + wave * 0.025;
    let color = [1.0, 0.76 + wave * 0.20, 0.08];
    let alpha = 0.66 + wave * 0.28;
    const SEGMENTS: usize = 24;
    let tangent_half_length = radius * (std::f32::consts::PI / SEGMENTS as f32).tan() * 0.90;
    for segment in 0..SEGMENTS {
        let angle = std::f32::consts::TAU * segment as f32 / SEGMENTS as f32;
        let position = center + Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius);
        out.push(InstanceRaw::with_alpha(
            Mat4::from_scale_rotation_translation(
                // Local X is radial and local Z is tangential at this angle.
                // A small gap between tangent spans prevents adjacent pieces
                // from overlapping and fighting one another in the depth test.
                Vec3::new(0.045, 0.004, tangent_half_length),
                Quat::from_rotation_y(-angle),
                position,
            ),
            color,
            alpha,
        ));
    }
}

fn add_player_station_props(out: &mut Vec<InstanceRaw>, game: &Game) {
    let mat = [0.075, 0.030, 0.034];
    let mat_edge = [0.42, 0.245, 0.065];
    let dim_health = [0.18, 0.035, 0.03];
    for station in 0..4 {
        let (center, angle) = player_station_layout(station);
        let rotation = Quat::from_rotation_y(-angle);
        cube(
            out,
            center + Vec3::Y * 0.012,
            Vec3::new(3.15, 0.025, 2.70),
            rotation,
            mat,
        );
        let active = game.active_hero_id() == game.hero_order.get(station).copied();
        let edge_color = if active { [0.98, 0.71, 0.10] } else { mat_edge };
        for z in [-2.66_f32, 2.66] {
            cube(
                out,
                station_point(center, angle, 0.0, z) + Vec3::Y * 0.045,
                Vec3::new(3.12, 0.028, 0.035),
                rotation,
                edge_color,
            );
        }
        for x in [-3.11_f32, 3.11] {
            cube(
                out,
                station_point(center, angle, x, 0.0) + Vec3::Y * 0.045,
                Vec3::new(0.035, 0.028, 2.63),
                rotation,
                edge_color,
            );
        }

        if let Some(hero) = game.hero_order.get(station).and_then(|&id| game.unit(id)) {
            for pip in 0..hero.stats.body {
                let row = pip / 4;
                let column = pip % 4;
                let position = station_point(
                    center,
                    angle,
                    1.02 + column as f32 * 0.25,
                    1.52 + row as f32 * 0.27,
                ) + Vec3::Y * 0.13;
                cube(
                    out,
                    position,
                    Vec3::new(0.085, 0.075, 0.085),
                    rotation,
                    if pip < hero.body.max(0) as u8 {
                        [0.78, 0.035, 0.025]
                    } else {
                        dim_health
                    },
                );
            }
            if hero.inventory.gold > 0 {
                cube(
                    out,
                    station_point(center, angle, 1.12, -1.62) + Vec3::Y * 0.13,
                    Vec3::new(0.16, 0.075, 0.16),
                    rotation,
                    [0.94, 0.67, 0.08],
                );
            }
            let potion_count = hero.inventory.heroic_brew
                + hero.inventory.potion_of_defense
                + hero.inventory.potion_of_healing
                + hero.inventory.potion_of_strength
                + hero.inventory.petrification_potion;
            for potion in 0..potion_count.min(4) {
                cube(
                    out,
                    station_point(center, angle, 1.02 + potion as f32 * 0.27, -1.92)
                        + Vec3::Y * 0.18,
                    Vec3::new(0.08, 0.13, 0.08),
                    rotation,
                    [0.16, 0.40, 0.78],
                );
            }
        }
    }
}

fn add_zargon_station_props(out: &mut Vec<InstanceRaw>) {
    let deck_colors = [
        [0.25, 0.08, 0.05],
        [0.48, 0.20, 0.04],
        [0.17, 0.12, 0.32],
        [0.19, 0.17, 0.15],
    ];
    for (index, x) in [-12.0_f32, -9.8, -7.6, -5.4].into_iter().enumerate() {
        cube(
            out,
            Vec3::new(x, 0.068, 17.55),
            Vec3::new(0.65, 0.065 + index as f32 * 0.008, 0.97),
            Quat::IDENTITY,
            deck_colors[index],
        );
    }

    let leather = [0.29, 0.055, 0.045];
    let page_edge = [0.72, 0.59, 0.39];
    for x in [-1.84_f32, 1.84] {
        cube(
            out,
            Vec3::new(x, 0.032, 18.25),
            Vec3::new(1.84, 0.038, 2.38),
            Quat::IDENTITY,
            leather,
        );
        cube(
            out,
            Vec3::new(x, 0.070, 18.25),
            Vec3::new(1.79, 0.012, 2.31),
            Quat::IDENTITY,
            page_edge,
        );
    }
    cube(
        out,
        Vec3::new(0.0, 0.090, 18.25),
        Vec3::new(0.075, 0.055, 2.39),
        Quat::IDENTITY,
        [0.18, 0.025, 0.025],
    );
}

fn add_castle_room(out: &mut Vec<InstanceRaw>) {
    let dark_oak = [0.19, 0.075, 0.025];
    let oak = [0.34, 0.145, 0.045];
    let oak_edge = [0.48, 0.225, 0.070];
    let mortar = [0.17, 0.15, 0.16];

    // A broad oak gaming table supports the board, all four Hero stations,
    // the dice tray, and Zargon screen with a generous playable margin.
    cube(
        out,
        Vec3::new(0.0, -0.48, 0.0),
        Vec3::new(24.98, 0.38, 21.70),
        Quat::IDENTITY,
        oak,
    );
    for z in [-21.40_f32, 21.40] {
        cube(
            out,
            Vec3::new(0.0, -0.04, z),
            Vec3::new(25.16, 0.10, 0.22),
            Quat::IDENTITY,
            oak_edge,
        );
    }
    for x in [-24.68_f32, 24.68] {
        cube(
            out,
            Vec3::new(x, -0.04, 0.0),
            Vec3::new(0.22, 0.10, 21.22),
            Quat::IDENTITY,
            oak_edge,
        );
    }
    for (index, x) in (-7..=7).enumerate() {
        cube(
            out,
            Vec3::new(x as f32 * 3.16, -0.075, 0.0),
            Vec3::new(0.025, 0.018, 20.98),
            Quat::IDENTITY,
            if index % 2 == 0 { dark_oak } else { oak_edge },
        );
    }
    for x in [-21.87_f32, 21.87] {
        for z in [-18.14_f32, 18.14] {
            cube(
                out,
                Vec3::new(x, -3.25, z),
                Vec3::new(0.82, 2.75, 0.82),
                Quat::IDENTITY,
                dark_oak,
            );
            cube(
                out,
                Vec3::new(x, -0.92, z),
                Vec3::new(1.05, 0.20, 1.05),
                Quat::IDENTITY,
                oak_edge,
            );
        }
    }

    // Flagstone floor and block-laid walls enclose the playable diorama.
    cube(
        out,
        Vec3::new(0.0, -6.10, 4.0),
        Vec3::new(34.0, 0.20, 40.0),
        Quat::IDENTITY,
        [0.075, 0.070, 0.082],
    );
    for row in 0..16 {
        let y = -5.0 + row as f32 * 1.72;
        let offset = if row % 2 == 0 { 0.0 } else { 2.05 };
        for column in -9_i32..=9 {
            let x = column as f32 * 4.10 + offset;
            if x.abs() > 33.5 {
                continue;
            }
            let shade = if (row + column.rem_euclid(3) as usize) % 3 == 0 {
                [0.24, 0.225, 0.235]
            } else {
                [0.19, 0.18, 0.195]
            };
            cube(
                out,
                Vec3::new(x, y, 37.7),
                Vec3::new(2.00, 0.80, 0.50),
                Quat::IDENTITY,
                shade,
            );
        }
    }
    for side in [-1.0_f32, 1.0] {
        for row in 0..16 {
            let y = -5.0 + row as f32 * 1.72;
            let offset = if row % 2 == 0 { 0.0 } else { 2.0 };
            for column in -10_i32..=9 {
                let z = column as f32 * 4.0 + offset;
                let shade = if (row + column.rem_euclid(2) as usize) % 2 == 0 {
                    [0.205, 0.19, 0.205]
                } else {
                    [0.165, 0.155, 0.17]
                };
                cube(
                    out,
                    Vec3::new(side * 33.5, y, z),
                    Vec3::new(0.50, 0.80, 1.95),
                    Quat::IDENTITY,
                    shade,
                );
            }
        }
    }
    // Dark mortar courses make the block pattern legible under low light.
    for row in 0..17 {
        cube(
            out,
            Vec3::new(0.0, -5.85 + row as f32 * 1.72, 37.15),
            Vec3::new(33.0, 0.035, 0.035),
            Quat::IDENTITY,
            mortar,
        );
    }

    // Raised hearth and glowing fire on the rear wall.
    cube(
        out,
        Vec3::new(-18.0, 0.0, 36.95),
        Vec3::new(4.6, 3.7, 0.34),
        Quat::IDENTITY,
        [0.025, 0.018, 0.020],
    );
    for x in [-22.4_f32, -13.6] {
        cube(
            out,
            Vec3::new(x, 0.30, 36.35),
            Vec3::new(0.88, 4.25, 0.72),
            Quat::IDENTITY,
            [0.31, 0.285, 0.27],
        );
    }
    cube(
        out,
        Vec3::new(-18.0, 4.55, 36.35),
        Vec3::new(5.25, 0.82, 0.72),
        Quat::IDENTITY,
        [0.33, 0.30, 0.28],
    );
    cube(
        out,
        Vec3::new(-18.0, -3.20, 35.95),
        Vec3::new(5.25, 0.45, 1.65),
        Quat::IDENTITY,
        [0.27, 0.245, 0.23],
    );
    for (x, y, scale, color) in [
        (
            -19.3,
            -0.45,
            Vec3::new(0.42, 1.20, 0.26),
            [1.25, 0.19, 0.015],
        ),
        (
            -18.0,
            0.10,
            Vec3::new(0.48, 1.75, 0.28),
            [1.35, 0.42, 0.025],
        ),
        (
            -16.7,
            -0.35,
            Vec3::new(0.38, 1.30, 0.25),
            [1.20, 0.14, 0.01],
        ),
    ] {
        cube(
            out,
            Vec3::new(x, y, 35.92),
            scale,
            Quat::from_rotation_z((x + 18.0) * 0.18),
            color,
        );
    }
    for x in [-19.5_f32, -16.5] {
        cube(
            out,
            Vec3::new(x, -1.15, 35.45),
            Vec3::new(1.25, 0.18, 0.18),
            Quat::from_rotation_y(if x < -18.0 { -0.2 } else { 0.2 }),
            dark_oak,
        );
    }

    // Two heraldic cloths add color without depending on external textures.
    add_tapestry(
        out,
        Vec3::new(18.0, 8.0, 37.05),
        Vec3::new(4.4, 6.25, 0.15),
        [0.30, 0.025, 0.040],
        false,
    );
    add_tapestry(
        out,
        Vec3::new(-32.85, 7.0, 5.0),
        Vec3::new(0.15, 5.8, 4.2),
        [0.035, 0.075, 0.28],
        true,
    );

    // Monumental columns and arch bands establish the larger great-hall scale.
    for x in [-28.0_f32, -14.0, 0.0, 14.0, 28.0] {
        add_castle_column(out, Vec3::new(x, 5.0, 36.45), false);
    }
    for center_x in [-21.0_f32, -7.0, 7.0, 21.0] {
        add_back_arch(out, center_x, 36.1);
    }
    for side in [-1.0_f32, 1.0] {
        for z in [-24.0_f32, -8.0, 8.0, 24.0] {
            add_castle_column(out, Vec3::new(side * 32.75, 5.0, z), true);
        }
    }

    add_chandelier(out, Vec3::new(0.0, 15.0, 2.0));
    for position in [
        Vec3::new(-26.0, -3.0, 17.0),
        Vec3::new(26.0, -3.0, 17.0),
        Vec3::new(-26.0, -3.0, -14.0),
        Vec3::new(26.0, -3.0, -14.0),
    ] {
        add_brazier(out, position);
    }
}

fn add_castle_column(out: &mut Vec<InstanceRaw>, center: Vec3, side_wall: bool) {
    let stone = [0.145, 0.135, 0.155];
    let dark = [0.085, 0.078, 0.095];
    let shaft_scale = if side_wall {
        Vec3::new(0.85, 9.0, 1.15)
    } else {
        Vec3::new(1.15, 9.0, 0.85)
    };
    cube(out, center, shaft_scale, Quat::IDENTITY, stone);
    for y in [-4.0_f32, 13.8] {
        cube(
            out,
            Vec3::new(center.x, y, center.z),
            if side_wall {
                Vec3::new(1.25, 0.55, 1.65)
            } else {
                Vec3::new(1.65, 0.55, 1.25)
            },
            Quat::IDENTITY,
            dark,
        );
    }
}

fn add_back_arch(out: &mut Vec<InstanceRaw>, center_x: f32, z: f32) {
    let radius = 6.8_f32;
    for segment in 0..=12 {
        let angle = std::f32::consts::PI * segment as f32 / 12.0;
        cube(
            out,
            Vec3::new(
                center_x + radius * angle.cos(),
                12.0 + radius * angle.sin(),
                z,
            ),
            Vec3::new(0.54, 1.05, 0.72),
            Quat::from_rotation_z(angle - std::f32::consts::FRAC_PI_2),
            [0.20, 0.185, 0.205],
        );
    }
}

fn add_chandelier(out: &mut Vec<InstanceRaw>, center: Vec3) {
    let iron = [0.035, 0.030, 0.034];
    for y in [center.y + 4.0, center.y + 6.0] {
        cube(
            out,
            Vec3::new(center.x, y, center.z),
            Vec3::new(0.10, 1.0, 0.10),
            Quat::IDENTITY,
            iron,
        );
    }
    for segment in 0..12 {
        let angle = std::f32::consts::TAU * segment as f32 / 12.0;
        cube(
            out,
            center + Vec3::new(angle.cos() * 4.2, 0.0, angle.sin() * 4.2),
            Vec3::new(1.08, 0.12, 0.12),
            Quat::from_rotation_y(-angle),
            iron,
        );
        if segment % 2 == 0 {
            cube(
                out,
                center + Vec3::new(angle.cos() * 4.2, 0.55, angle.sin() * 4.2),
                Vec3::new(0.10, 0.58, 0.10),
                Quat::IDENTITY,
                [0.95, 0.50, 0.07],
            );
        }
    }
}

fn add_brazier(out: &mut Vec<InstanceRaw>, center: Vec3) {
    let iron = [0.045, 0.035, 0.040];
    cube(
        out,
        center,
        Vec3::new(0.22, 2.2, 0.22),
        Quat::IDENTITY,
        iron,
    );
    cube(
        out,
        center + Vec3::Y * 2.2,
        Vec3::new(1.15, 0.22, 1.15),
        Quat::IDENTITY,
        iron,
    );
    for offset in [-0.45_f32, 0.0, 0.45] {
        cube(
            out,
            center + Vec3::new(offset, 3.05 + offset.abs(), 0.0),
            Vec3::new(0.25, 0.85, 0.25),
            Quat::from_rotation_z(offset * 0.4),
            [1.30, 0.27 + offset.abs() * 0.25, 0.015],
        );
    }
}

fn add_tapestry(
    out: &mut Vec<InstanceRaw>,
    center: Vec3,
    scale: Vec3,
    cloth: [f32; 3],
    side_wall: bool,
) {
    let gold = [0.70, 0.47, 0.10];
    cube(out, center, scale, Quat::IDENTITY, cloth);
    if side_wall {
        for z in [center.z - scale.z + 0.20, center.z + scale.z - 0.20] {
            cube(
                out,
                Vec3::new(center.x - 0.17, center.y, z),
                Vec3::new(0.10, scale.y, 0.14),
                Quat::IDENTITY,
                gold,
            );
        }
        cube(
            out,
            Vec3::new(center.x - 0.18, center.y, center.z),
            Vec3::new(0.09, 0.26, scale.z * 0.72),
            Quat::from_rotation_x(0.62),
            gold,
        );
    } else {
        for x in [center.x - scale.x + 0.20, center.x + scale.x - 0.20] {
            cube(
                out,
                Vec3::new(x, center.y, center.z - 0.17),
                Vec3::new(0.14, scale.y, 0.10),
                Quat::IDENTITY,
                gold,
            );
        }
        cube(
            out,
            Vec3::new(center.x, center.y, center.z - 0.18),
            Vec3::new(scale.x * 0.72, 0.26, 0.09),
            Quat::from_rotation_z(0.62),
            gold,
        );
    }
}

struct EnvironmentPrimitiveCpu {
    vertices: Vec<EnvironmentVertex>,
    indices: Vec<u32>,
    material_index: usize,
}

fn load_environment_backdrop(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    camera_layout: &wgpu::BindGroupLayout,
    output_format: wgpu::TextureFormat,
) -> Result<Option<EnvironmentBackdrop>> {
    let explicit = std::env::var_os("HEROQUEST_ROOM_PANORAMA")
        .or_else(|| std::env::var_os("HEROQUEST_ROOM_MATTE"))
        .map(PathBuf::from);
    let path = explicit.clone().unwrap_or_else(|| {
        [
            PathBuf::from("assets/local/environment/castle-great-hall-panorama-v1-4x.png"),
            PathBuf::from("assets/environment/castle-great-hall-panorama-v1-4x.png"),
        ]
        .into_iter()
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from("assets/environment/castle-great-hall-panorama-v1-4x.png"))
    });
    if !path.is_file() {
        if explicit.is_some() {
            return Err(anyhow!(
                "room panorama environment variable points to a missing file: {}",
                path.display()
            ));
        }
        return Ok(None);
    }

    let rgba = image::open(&path)
        .with_context(|| format!("failed to decode {}", path.display()))?
        .into_rgba8();
    let (width, height) = rgba.dimensions();
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("spherical room panorama"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        rgba.as_raw(),
    );
    let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("room panorama texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("room panorama sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("room panorama bind group"),
        layout: &texture_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &texture.create_view(&wgpu::TextureViewDescriptor::default()),
                ),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });

    let vertices = [
        BoardVertex {
            position: [-1.0, -1.0, 0.0],
            uv: [0.0, 1.0],
        },
        BoardVertex {
            position: [1.0, -1.0, 0.0],
            uv: [1.0, 1.0],
        },
        BoardVertex {
            position: [1.0, 1.0, 0.0],
            uv: [1.0, 0.0],
        },
        BoardVertex {
            position: [-1.0, 1.0, 0.0],
            uv: [0.0, 0.0],
        },
    ];
    let indices = [0_u16, 1, 2, 0, 2, 3];
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("room panorama vertices"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("room panorama indices"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("spherical room panorama shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("environment_backdrop.wgsl"))),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("room panorama pipeline layout"),
        bind_group_layouts: &[Some(camera_layout), Some(&texture_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("room panorama pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_backdrop"),
            buffers: &[Some(BoardVertex::layout())],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_backdrop"),
            targets: &[Some(wgpu::ColorTargetState {
                format: output_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });

    Ok(Some(EnvironmentBackdrop {
        _texture: texture,
        bind_group,
        pipeline,
        vertex_buffer,
        index_buffer,
        index_count: indices.len() as u32,
        path,
    }))
}

fn load_environment_mesh(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    camera_layout: &wgpu::BindGroupLayout,
    output_format: wgpu::TextureFormat,
) -> Result<Option<EnvironmentMesh>> {
    let explicit = std::env::var_os("HEROQUEST_ROOM_MODEL").map(PathBuf::from);
    let path = explicit.clone().unwrap_or_else(|| {
        [
            PathBuf::from("assets/local/environment/castle-great-hall.glb"),
            PathBuf::from("assets/environment/castle-great-hall.glb"),
        ]
        .into_iter()
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from("assets/environment/castle-great-hall.glb"))
    });
    if !path.is_file() {
        if explicit.is_some() {
            return Err(anyhow!(
                "HEROQUEST_ROOM_MODEL points to a missing file: {}",
                path.display()
            ));
        }
        return Ok(None);
    }

    let (document, buffers, images) =
        gltf::import(&path).with_context(|| format!("failed to import {}", path.display()))?;
    let default_material_index = document.materials().count();
    let mut cpu_primitives = Vec::new();

    fn visit_node(
        node: gltf::Node<'_>,
        parent_transform: Mat4,
        buffers: &[gltf::buffer::Data],
        default_material_index: usize,
        output: &mut Vec<EnvironmentPrimitiveCpu>,
    ) -> Result<()> {
        let local_transform = Mat4::from_cols_array_2d(&node.transform().matrix());
        let world_transform = parent_transform * local_transform;
        if let Some(mesh) = node.mesh() {
            for primitive in mesh.primitives() {
                let reader = primitive.reader(|buffer| Some(buffers[buffer.index()].0.as_slice()));
                let positions = reader
                    .read_positions()
                    .ok_or_else(|| anyhow!("environment primitive has no positions"))?
                    .collect::<Vec<_>>();
                let normals = reader
                    .read_normals()
                    .map(|values| values.collect::<Vec<_>>())
                    .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);
                let texcoords = reader
                    .read_tex_coords(0)
                    .map(|values| values.into_f32().collect::<Vec<_>>())
                    .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);
                let vertices = positions
                    .into_iter()
                    .zip(normals)
                    .zip(texcoords)
                    .map(|((position, normal), uv)| {
                        let position = world_transform.transform_point3(Vec3::from_array(position));
                        let normal = world_transform
                            .transform_vector3(Vec3::from_array(normal))
                            .normalize_or_zero();
                        EnvironmentVertex {
                            position: position.to_array(),
                            normal: normal.to_array(),
                            uv,
                        }
                    })
                    .collect::<Vec<_>>();
                let indices = reader
                    .read_indices()
                    .map(|values| values.into_u32().collect::<Vec<_>>())
                    .unwrap_or_else(|| (0..vertices.len() as u32).collect());
                output.push(EnvironmentPrimitiveCpu {
                    vertices,
                    indices,
                    material_index: primitive
                        .material()
                        .index()
                        .unwrap_or(default_material_index),
                });
            }
        }
        for child in node.children() {
            visit_node(
                child,
                world_transform,
                buffers,
                default_material_index,
                output,
            )?;
        }
        Ok(())
    }

    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next())
        .ok_or_else(|| anyhow!("environment GLB has no scene"))?;
    for node in scene.nodes() {
        visit_node(
            node,
            Mat4::IDENTITY,
            &buffers,
            default_material_index,
            &mut cpu_primitives,
        )?;
    }
    if cpu_primitives.is_empty() {
        return Err(anyhow!("environment GLB contains no renderable primitives"));
    }

    let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("environment material layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("environment sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });

    fn image_to_rgba(image: &gltf::image::Data) -> Result<Vec<u8>> {
        use gltf::image::Format;
        let pixel_count = image.width as usize * image.height as usize;
        match image.format {
            Format::R8G8B8A8 => Ok(image.pixels.clone()),
            Format::R8G8B8 => {
                let mut rgba = Vec::with_capacity(pixel_count * 4);
                for rgb in image.pixels.chunks_exact(3) {
                    rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                }
                Ok(rgba)
            }
            Format::R8G8 => {
                let mut rgba = Vec::with_capacity(pixel_count * 4);
                for gray_alpha in image.pixels.chunks_exact(2) {
                    rgba.extend_from_slice(&[
                        gray_alpha[0],
                        gray_alpha[0],
                        gray_alpha[0],
                        gray_alpha[1],
                    ]);
                }
                Ok(rgba)
            }
            Format::R8 => Ok(image
                .pixels
                .iter()
                .flat_map(|value| [*value, *value, *value, 255])
                .collect()),
            other => Err(anyhow!("unsupported environment texture format {other:?}")),
        }
    }

    fn make_material(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        image: Option<&gltf::image::Data>,
        uniform: EnvironmentMaterialUniform,
        label: &str,
    ) -> Result<EnvironmentMaterial> {
        let (width, height, pixels) = if let Some(image) = image {
            (image.width, image.height, image_to_rgba(image)?)
        } else {
            (1, 1, vec![255, 255, 255, 255])
        };
        let texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &pixels,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("environment material uniform"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("environment material bind group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
        });
        Ok(EnvironmentMaterial {
            _texture: texture,
            _uniform_buffer: uniform_buffer,
            bind_group,
        })
    }

    let mut materials = Vec::with_capacity(default_material_index + 1);
    for (material_index, material) in document.materials().enumerate() {
        let pbr = material.pbr_metallic_roughness();
        let base_color = pbr.base_color_factor();
        let emissive = material.emissive_factor();
        let texture_image = pbr
            .base_color_texture()
            .and_then(|info| images.get(info.texture().source().index()));
        materials.push(make_material(
            device,
            queue,
            &material_layout,
            &sampler,
            texture_image,
            EnvironmentMaterialUniform {
                base_color,
                emissive: [emissive[0], emissive[1], emissive[2], 0.0],
            },
            &format!("environment material {material_index}"),
        )?);
    }
    materials.push(make_material(
        device,
        queue,
        &material_layout,
        &sampler,
        None,
        EnvironmentMaterialUniform {
            base_color: [0.20, 0.18, 0.20, 1.0],
            emissive: [0.0; 4],
        },
        "environment default material",
    )?);

    let primitives = cpu_primitives
        .into_iter()
        .enumerate()
        .map(|(index, primitive)| {
            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("environment vertices"),
                contents: bytemuck::cast_slice(&primitive.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("environment indices"),
                contents: bytemuck::cast_slice(&primitive.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            if primitive.material_index >= materials.len() {
                return Err(anyhow!(
                    "environment primitive {index} references missing material {}",
                    primitive.material_index
                ));
            }
            Ok(EnvironmentPrimitive {
                vertex_buffer,
                index_buffer,
                index_count: primitive.indices.len() as u32,
                material_index: primitive.material_index,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("environment shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("environment.wgsl"))),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("environment pipeline layout"),
        bind_group_layouts: &[Some(camera_layout), Some(&material_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("environment pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_environment"),
            buffers: &[Some(EnvironmentVertex::layout())],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_environment"),
            targets: &[Some(wgpu::ColorTargetState {
                format: output_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });

    Ok(Some(EnvironmentMesh {
        pipeline,
        primitives,
        materials,
        path,
    }))
}

fn load_scanned_board(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    camera_layout: &wgpu::BindGroupLayout,
    output_format: wgpu::TextureFormat,
) -> Result<Option<ScannedBoard>> {
    let explicit = std::env::var_os("HEROQUEST_BOARD_SCAN").map(PathBuf::from);
    let path = explicit.clone().or_else(|| {
        let root = local_art_root();
        [
            root.join("board-runtime.jpg"),
            root.join("board-scan.png"),
            root.join("board-scan.jpg"),
            root.join("board-scan.jpeg"),
        ]
        .into_iter()
        .find(|candidate| candidate.is_file())
    });
    let Some(path) = path else {
        return Ok(None);
    };
    if !path.is_file() {
        return Err(anyhow!(
            "HEROQUEST_BOARD_SCAN points to a missing file: {}",
            path.display()
        ));
    }

    let mut image = image::open(&path)
        .with_context(|| format!("failed to decode board scan {}", path.display()))?;
    let source_dimensions = (image.width(), image.height());
    let plane = load_board_plane(&path, source_dimensions)?;
    let limit = device.limits().max_texture_dimension_2d;
    if image.width() > limit || image.height() > limit {
        let scale = (limit as f64 / image.width() as f64).min(limit as f64 / image.height() as f64);
        let width = (image.width() as f64 * scale).round().max(1.0) as u32;
        let height = (image.height() as f64 * scale).round().max(1.0) as u32;
        log::warn!(
            "resizing board scan from {}x{} to {width}x{height} for this GPU",
            image.width(),
            image.height()
        );
        image = image.resize_exact(width, height, image::imageops::FilterType::Lanczos3);
    }
    let rgba = image.into_rgba8();
    let dimensions = rgba.dimensions();
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("local board scan"),
            size: wgpu::Extent3d {
                width: dimensions.0,
                height: dimensions.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        rgba.as_raw(),
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("board scan sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });
    let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("board scan texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("board scan texture bind group"),
        layout: &texture_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("board scan shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("board_texture.wgsl"))),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("board scan pipeline layout"),
        bind_group_layouts: &[Some(camera_layout), Some(&texture_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("board scan pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_board"),
            buffers: &[Some(BoardVertex::layout())],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_board"),
            targets: &[Some(wgpu::ColorTargetState {
                format: output_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    let vertices = [
        BoardVertex {
            position: [plane.left, -0.01, plane.top],
            uv: [0.0, 0.0],
        },
        BoardVertex {
            position: [plane.left, -0.01, plane.bottom],
            uv: [0.0, 1.0],
        },
        BoardVertex {
            position: [plane.right, -0.01, plane.bottom],
            uv: [1.0, 1.0],
        },
        BoardVertex {
            position: [plane.right, -0.01, plane.top],
            uv: [1.0, 0.0],
        },
    ];
    let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("board scan vertices"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("board scan indices"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    Ok(Some(ScannedBoard {
        _texture: texture,
        bind_group,
        pipeline,
        vertex_buffer,
        index_buffer,
        index_count: indices.len() as u32,
        path,
    }))
}

fn load_information_screen(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    camera_layout: &wgpu::BindGroupLayout,
    output_format: wgpu::TextureFormat,
) -> Result<Option<InformationScreen>> {
    let root = local_art_root();
    let front_path = root.join("screen/information-screen-front.png");
    let back_path = root.join("screen/information-screen-back.png");
    match (front_path.is_file(), back_path.is_file()) {
        (false, false) => return Ok(None),
        (true, false) | (false, true) => {
            return Err(anyhow!(
                "Information Screen needs both {} and {}",
                front_path.display(),
                back_path.display()
            ));
        }
        (true, true) => {}
    }

    let front_rgba = image::open(&front_path)
        .with_context(|| format!("failed to decode {}", front_path.display()))?
        .into_rgba8();
    let back_rgba = image::open(&back_path)
        .with_context(|| format!("failed to decode {}", back_path.display()))?
        .into_rgba8();
    if front_rgba.dimensions() != back_rgba.dimensions() {
        return Err(anyhow!(
            "Information Screen textures have different dimensions: {} is {:?}, {} is {:?}",
            front_path.display(),
            front_rgba.dimensions(),
            back_path.display(),
            back_rgba.dimensions()
        ));
    }
    let dimensions = front_rgba.dimensions();
    ensure_texture_fits(device, dimensions, &front_path)?;

    let texture_descriptor = |label: &'static str| wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: dimensions.0,
            height: dimensions.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    };
    let front_texture = device.create_texture_with_data(
        queue,
        &texture_descriptor("Information Screen players-side texture"),
        wgpu::util::TextureDataOrder::LayerMajor,
        front_rgba.as_raw(),
    );
    let back_texture = device.create_texture_with_data(
        queue,
        &texture_descriptor("Information Screen Zargon-side texture"),
        wgpu::util::TextureDataOrder::LayerMajor,
        back_rgba.as_raw(),
    );
    let front_view = front_texture.create_view(&wgpu::TextureViewDescriptor::default());
    let back_view = back_texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("Information Screen sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });
    let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Information Screen texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Information Screen texture bind group"),
        layout: &texture_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&front_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&back_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Information Screen shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("screen.wgsl"))),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Information Screen pipeline layout"),
        bind_group_layouts: &[Some(camera_layout), Some(&texture_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Information Screen render pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_screen"),
            buffers: &[Some(BoardVertex::layout())],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_screen"),
            targets: &[Some(wgpu::ColorTargetState {
                format: output_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });

    // A shallow three-panel fold. The screen sits behind the board, just beyond
    // the dice tray, with the illustrated side facing the table and heroes.
    let screen_width = 17.0_f32;
    let screen_height = 4.6_f32;
    let y0 = 0.08_f32;
    let z0 = 15.15_f32;
    let fold = 0.70_f32;
    // The original has a broad center panel and two narrow wings; use the
    // visible crease positions in the scan rather than three equal thirds.
    let x = [
        -screen_width * 0.5,
        -screen_width * 0.31,
        screen_width * 0.31,
        screen_width * 0.5,
    ];
    let z = [z0 - fold, z0, z0, z0 - fold];
    let u = [0.0_f32, 0.19, 0.81, 1.0];
    let mut vertices = Vec::with_capacity(8);
    for index in 0..4 {
        vertices.push(BoardVertex {
            position: [x[index], y0, z[index]],
            uv: [u[index], 1.0],
        });
        vertices.push(BoardVertex {
            position: [x[index], y0 + screen_height, z[index]],
            uv: [u[index], 0.0],
        });
    }
    let indices: [u16; 18] = [0, 1, 3, 0, 3, 2, 2, 3, 5, 2, 5, 4, 4, 5, 7, 4, 7, 6];
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Information Screen vertices"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Information Screen indices"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    Ok(Some(InformationScreen {
        _front_texture: front_texture,
        _back_texture: back_texture,
        bind_group,
        pipeline,
        vertex_buffer,
        index_buffer,
        index_count: indices.len() as u32,
        front_path,
        back_path,
    }))
}

fn load_board_plane(path: &Path, dimensions: (u32, u32)) -> Result<BoardPlane> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let image_specific_calibration = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| parent.join(format!("{stem}-calibration.json")));
    let calibration_path = image_specific_calibration
        .filter(|candidate| candidate.is_file())
        .unwrap_or_else(|| parent.join("board-calibration.json"));
    if !calibration_path.is_file() {
        return board_plane_from_bounds(
            dimensions,
            PixelBounds {
                left: 0,
                top: 0,
                right: dimensions.0,
                bottom: dimensions.1,
            },
        );
    }
    let bytes = std::fs::read(&calibration_path).with_context(|| {
        format!(
            "failed to read board calibration {}",
            calibration_path.display()
        )
    })?;
    let calibration: BoardCalibration = serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "board calibration {} is not valid JSON",
            calibration_path.display()
        )
    })?;
    board_plane_from_bounds(dimensions, calibration.playable_bounds_px)
        .with_context(|| format!("invalid board calibration {}", calibration_path.display()))
}

fn board_plane_from_bounds(dimensions: (u32, u32), bounds: PixelBounds) -> Result<BoardPlane> {
    let (width, height) = dimensions;
    if width == 0
        || height == 0
        || bounds.left >= bounds.right
        || bounds.top >= bounds.bottom
        || bounds.right > width
        || bounds.bottom > height
    {
        return Err(anyhow!(
            "playable pixel bounds [{}, {}, {}, {}] do not fit image {}x{}",
            bounds.left,
            bounds.top,
            bounds.right,
            bounds.bottom,
            width,
            height
        ));
    }

    let u0 = bounds.left as f32 / width as f32;
    let u1 = bounds.right as f32 / width as f32;
    let v0 = bounds.top as f32 / height as f32;
    let v1 = bounds.bottom as f32 / height as f32;
    let plane_width = BOARD_WIDTH as f32 / (u1 - u0);
    let plane_height = BOARD_HEIGHT as f32 / (v1 - v0);
    let left = -(BOARD_WIDTH as f32) * 0.5 - u0 * plane_width;
    let top = -(BOARD_HEIGHT as f32) * 0.5 - v0 * plane_height;
    Ok(BoardPlane {
        left,
        right: left + plane_width,
        top,
        bottom: top + plane_height,
    })
}

const SPRITE_SLOTS: &[(SpriteKey, &[&str])] = &[
    (
        SpriteKey::Figure(FigureKind::Hero(HeroKind::Barbarian)),
        &["figures/barbarian.png"],
    ),
    (
        SpriteKey::Figure(FigureKind::Hero(HeroKind::Dwarf)),
        &["figures/dwarf.png"],
    ),
    (
        SpriteKey::Figure(FigureKind::Hero(HeroKind::Elf)),
        &["figures/elf.png"],
    ),
    (
        SpriteKey::Figure(FigureKind::Hero(HeroKind::Wizard)),
        &["figures/wizard.png"],
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Goblin)),
        &["figures/goblin.png"],
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Orc)),
        &["figures/orc.png"],
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Fimir)),
        &["figures/fimir.png", "figures/abomination.png"],
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Skeleton)),
        &["figures/skeleton.png"],
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Zombie)),
        &["figures/zombie.png"],
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Mummy)),
        &["figures/mummy.png"],
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::ChaosWarrior)),
        &["figures/chaos-warrior.png", "figures/dread-warrior.png"],
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Gargoyle)),
        &["figures/gargoyle.png"],
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::ChaosSorcerer)),
        &["figures/chaos-sorcerer.png", "figures/dread-sorcerer.png"],
    ),
    (SpriteKey::Prop(PropKind::Stairs), &["props/stairs.png"]),
    (SpriteKey::Prop(PropKind::Table), &["props/table.png"]),
    (SpriteKey::Prop(PropKind::Chest), &["props/chest.png"]),
    (SpriteKey::Prop(PropKind::Bookcase), &["props/bookcase.png"]),
    (SpriteKey::Prop(PropKind::Throne), &["props/throne.png"]),
    (
        SpriteKey::Prop(PropKind::WeaponRack),
        &["props/weapon-rack.png"],
    ),
    (
        SpriteKey::Prop(PropKind::AlchemistsBench),
        &["props/alchemists-bench.png"],
    ),
    (SpriteKey::Prop(PropKind::Tomb), &["props/tomb.png"]),
    (
        SpriteKey::Prop(PropKind::SorcerersTable),
        &["props/sorcerers-table.png"],
    ),
    (
        SpriteKey::Prop(PropKind::TortureRack),
        &["props/torture-rack.png"],
    ),
    (
        SpriteKey::Prop(PropKind::Fireplace),
        &["props/fireplace.png"],
    ),
    (SpriteKey::Prop(PropKind::Cupboard), &["props/cupboard.png"]),
];

fn local_art_root() -> PathBuf {
    std::env::var_os("HEROQUEST_ART_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let original_us = PathBuf::from("assets/local/editions/original-us");
            if original_us.is_dir() {
                original_us
            } else {
                PathBuf::from("assets/local")
            }
        })
}

const PIECE_MODEL_SLOTS: &[(SpriteKey, &str)] = &[
    (
        SpriteKey::Figure(FigureKind::Hero(HeroKind::Barbarian)),
        "figures/barbarian.glb",
    ),
    (
        SpriteKey::Figure(FigureKind::Hero(HeroKind::Dwarf)),
        "figures/dwarf.glb",
    ),
    (
        SpriteKey::Figure(FigureKind::Hero(HeroKind::Elf)),
        "figures/elf.glb",
    ),
    (
        SpriteKey::Figure(FigureKind::Hero(HeroKind::Wizard)),
        "figures/wizard.glb",
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Goblin)),
        "figures/goblin-sword.glb",
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Goblin)),
        "figures/goblin-axe.glb",
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Goblin)),
        "figures/goblin-scimitar.glb",
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Orc)),
        "figures/orc-sword.glb",
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Orc)),
        "figures/orc-notched-sword.glb",
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Orc)),
        "figures/orc-staff.glb",
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Orc)),
        "figures/orc-flail.glb",
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Orc)),
        "figures/orc-cleaver.glb",
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Fimir)),
        "figures/fimir.glb",
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Skeleton)),
        "figures/skeleton.glb",
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Zombie)),
        "figures/zombie.glb",
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Mummy)),
        "figures/mummy.glb",
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::ChaosWarrior)),
        "figures/chaos-warrior.glb",
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::Gargoyle)),
        "figures/gargoyle.glb",
    ),
    (
        SpriteKey::Figure(FigureKind::Monster(MonsterKind::ChaosSorcerer)),
        "figures/chaos-warlock.glb",
    ),
    (SpriteKey::Prop(PropKind::Table), "furniture/table.glb"),
    (
        SpriteKey::Prop(PropKind::Chest),
        "furniture/treasure-chest.glb",
    ),
    (
        SpriteKey::Prop(PropKind::Bookcase),
        "furniture/bookcase.glb",
    ),
    (SpriteKey::Prop(PropKind::Throne), "furniture/throne.glb"),
    (
        SpriteKey::Prop(PropKind::WeaponRack),
        "furniture/weapons-rack.glb",
    ),
    (
        SpriteKey::Prop(PropKind::AlchemistsBench),
        "furniture/alchemists-bench.glb",
    ),
    (SpriteKey::Prop(PropKind::Tomb), "furniture/tomb.glb"),
    (
        SpriteKey::Prop(PropKind::SorcerersTable),
        "furniture/sorcerers-table.glb",
    ),
    (
        SpriteKey::Prop(PropKind::TortureRack),
        "furniture/torture-rack.glb",
    ),
    (
        SpriteKey::Prop(PropKind::Fireplace),
        "furniture/fireplace.glb",
    ),
    (
        SpriteKey::Prop(PropKind::Cupboard),
        "furniture/cupboard.glb",
    ),
    (SpriteKey::Die(DieKind::Movement), "dice/movement.glb"),
    (SpriteKey::Die(DieKind::Combat), "dice/combat.glb"),
    (SpriteKey::DoorOpen, "doors/open.glb"),
    (SpriteKey::DoorClosed, "doors/closed.glb"),
    (SpriteKey::SecretDoor, "markers/secret-door.glb"),
    (SpriteKey::Trap(TrapKind::Pit), "markers/pit-trap.glb"),
    (
        SpriteKey::Trap(TrapKind::FallingBlock),
        "markers/falling-block-trap.glb",
    ),
    (SpriteKey::BlockedSquare, "markers/blocked-square-1x1.glb"),
    (SpriteKey::BlockedDouble, "markers/blocked-square-1x2.glb"),
    (SpriteKey::DamageSkull, "markers/skull.glb"),
    (SpriteKey::FurnitureRat, "dressing/rat.glb"),
    (SpriteKey::FurnitureSkull, "dressing/skull.glb"),
];

fn piece_model_target_bounds(key: SpriteKey) -> (f32, f32) {
    match key {
        SpriteKey::Figure(_) => (0.80, 1.72),
        SpriteKey::Prop(kind) => match kind {
            PropKind::Stairs => (1.92, 0.26),
            PropKind::Table => (2.10, 1.05),
            PropKind::Chest => (1.05, 0.78),
            PropKind::Bookcase => (1.35, 1.72),
            PropKind::Throne => (1.18, 1.55),
            PropKind::WeaponRack => (1.32, 1.52),
            PropKind::AlchemistsBench => (1.72, 1.18),
            PropKind::Tomb => (1.82, 0.90),
            PropKind::SorcerersTable => (1.72, 1.18),
            PropKind::TortureRack => (1.72, 0.98),
            PropKind::Fireplace => (1.75, 1.70),
            PropKind::Cupboard => (1.38, 1.62),
            PropKind::StarOfWest => (0.54, 0.12),
        },
        SpriteKey::Die(_) => (DIE_HALF_EXTENT * 2.0, DIE_HALF_EXTENT * 2.0),
        SpriteKey::DoorOpen | SpriteKey::DoorClosed => (0.96, 1.42),
        SpriteKey::SecretDoor => (0.92, 0.32),
        SpriteKey::Trap(TrapKind::Pit)
        | SpriteKey::Trap(TrapKind::FallingBlock)
        | SpriteKey::BlockedSquare => (0.92, 0.13),
        SpriteKey::BlockedDouble => (1.84, 0.13),
        SpriteKey::Trap(TrapKind::Spear) => (0.92, 0.13),
        SpriteKey::DamageSkull => (0.22, 0.045),
        SpriteKey::FurnitureRat => (0.33, 0.20),
        SpriteKey::FurnitureSkull => (0.19, 0.22),
    }
}

fn load_piece_models(device: &wgpu::Device) -> Result<PieceModels> {
    let root = std::env::var_os("HEROQUEST_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| local_art_root().join("models"));
    let mut assets = Vec::new();
    for &(key, relative_path) in PIECE_MODEL_SLOTS {
        let path = root.join(relative_path);
        if !path.is_file() {
            return Err(anyhow!(
                "required original-US piece model is missing: {} (run tools/audit-local-models.sh)",
                path.display()
            ));
        }
        let (document, buffers, _) = gltf::import(&path)
            .with_context(|| format!("failed to import piece model {}", path.display()))?;
        let mut cpu_primitives: Vec<(Vec<Vertex>, Vec<u32>)> = Vec::new();

        fn visit_piece_node(
            node: gltf::Node<'_>,
            parent_transform: Mat4,
            buffers: &[gltf::buffer::Data],
            output: &mut Vec<(Vec<Vertex>, Vec<u32>)>,
        ) -> Result<()> {
            let local_transform = Mat4::from_cols_array_2d(&node.transform().matrix());
            let world_transform = parent_transform * local_transform;
            if let Some(mesh) = node.mesh() {
                for primitive in mesh.primitives() {
                    if primitive.mode() != gltf::mesh::Mode::Triangles {
                        return Err(anyhow!("piece model primitive is not a triangle list"));
                    }
                    let reader =
                        primitive.reader(|buffer| Some(buffers[buffer.index()].0.as_slice()));
                    let positions = reader
                        .read_positions()
                        .ok_or_else(|| anyhow!("piece model primitive has no positions"))?
                        .collect::<Vec<_>>();
                    let normals = reader
                        .read_normals()
                        .map(|values| values.collect::<Vec<_>>())
                        .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);
                    let vertices = positions
                        .into_iter()
                        .zip(normals)
                        .map(|(position, normal)| {
                            let position =
                                world_transform.transform_point3(Vec3::from_array(position));
                            let normal = world_transform
                                .transform_vector3(Vec3::from_array(normal))
                                .normalize_or_zero();
                            Vertex {
                                position: position.to_array(),
                                normal: normal.to_array(),
                            }
                        })
                        .collect::<Vec<_>>();
                    let indices = reader
                        .read_indices()
                        .map(|values| values.into_u32().collect::<Vec<_>>())
                        .unwrap_or_else(|| (0..vertices.len() as u32).collect());
                    output.push((vertices, indices));
                }
            }
            for child in node.children() {
                visit_piece_node(child, world_transform, buffers, output)?;
            }
            Ok(())
        }

        let scene = document
            .default_scene()
            .or_else(|| document.scenes().next())
            .ok_or_else(|| anyhow!("piece model has no scene"))?;
        for node in scene.nodes() {
            visit_piece_node(node, Mat4::IDENTITY, &buffers, &mut cpu_primitives)?;
        }
        if cpu_primitives.is_empty() {
            return Err(anyhow!(
                "piece model {} contains no renderable primitives",
                path.display()
            ));
        }

        let mut minimum = Vec3::splat(f32::INFINITY);
        let mut maximum = Vec3::splat(f32::NEG_INFINITY);
        for (vertices, _) in &cpu_primitives {
            for vertex in vertices {
                let position = Vec3::from_array(vertex.position);
                minimum = minimum.min(position);
                maximum = maximum.max(position);
            }
        }
        let dimensions = maximum - minimum;
        let horizontal = dimensions.x.max(dimensions.z);
        if !horizontal.is_finite() || horizontal <= f32::EPSILON {
            return Err(anyhow!("piece model {} has invalid bounds", path.display()));
        }
        let (target_horizontal, target_height) = piece_model_target_bounds(key);
        let scale =
            (target_horizontal / horizontal).min(target_height / dimensions.y.max(f32::EPSILON));
        let origin = Vec3::new(
            (minimum.x + maximum.x) * 0.5,
            minimum.y,
            (minimum.z + maximum.z) * 0.5,
        );
        for (vertices, _) in &mut cpu_primitives {
            for vertex in vertices {
                vertex.position = ((Vec3::from_array(vertex.position) - origin) * scale).to_array();
            }
        }

        let primitives = cpu_primitives
            .into_iter()
            .map(|(vertices, indices)| PieceModelPrimitive {
                vertex_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("classic piece model vertices"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
                index_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("classic piece model indices"),
                    contents: bytemuck::cast_slice(&indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
                index_count: indices.len() as u32,
            })
            .collect();
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("classic piece model instances"),
            size: (MAX_SPRITE_INSTANCES * std::mem::size_of::<InstanceRaw>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        assets.push(PieceModelAsset {
            key,
            variant_index: 0,
            variant_count: 1,
            primitives,
            instance_buffer,
            path,
        });
    }
    for asset_index in 0..assets.len() {
        let key = assets[asset_index].key;
        let matching = assets
            .iter()
            .enumerate()
            .filter_map(|(index, asset)| (asset.key == key).then_some(index))
            .collect::<Vec<_>>();
        assets[asset_index].variant_index = matching
            .iter()
            .position(|&index| index == asset_index)
            .unwrap_or(0);
        assets[asset_index].variant_count = matching.len().max(1);
    }
    if assets.len() != PIECE_MODEL_SLOTS.len() {
        return Err(anyhow!(
            "loaded {} of {} required original-US piece models",
            assets.len(),
            PIECE_MODEL_SLOTS.len()
        ));
    }
    Ok(PieceModels { assets })
}

fn load_sprite_art(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    camera_layout: &wgpu::BindGroupLayout,
    output_format: wgpu::TextureFormat,
) -> Result<Option<SpriteArt>> {
    let root = local_art_root();
    let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("local art texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("local art sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });
    let mut assets = Vec::new();
    for &(key, candidates) in SPRITE_SLOTS {
        let path = candidates
            .iter()
            .map(|candidate| root.join(candidate))
            .find(|candidate| candidate.is_file());
        let Some(path) = path else {
            continue;
        };
        let rgba = image::open(&path)
            .with_context(|| format!("failed to decode local art {}", path.display()))?
            .into_rgba8();
        let dimensions = rgba.dimensions();
        ensure_texture_fits(device, dimensions, &path)?;
        let texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("local figure or furniture art"),
                size: wgpu::Extent3d {
                    width: dimensions.0,
                    height: dimensions.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            rgba.as_raw(),
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("local art texture bind group"),
            layout: &texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        assets.push(SpriteAsset {
            key,
            _texture: texture,
            bind_group,
            path,
        });
    }
    if assets.is_empty() {
        return Ok(None);
    }

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("local art sprite shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("sprite.wgsl"))),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("local art sprite pipeline layout"),
        bind_group_layouts: &[Some(camera_layout), Some(&texture_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("local art sprite pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_sprite"),
            buffers: &[
                Some(BoardVertex::layout()),
                Some(SpriteInstanceRaw::layout()),
            ],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_sprite"),
            targets: &[Some(wgpu::ColorTargetState {
                format: output_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    let vertices = [
        BoardVertex {
            position: [-0.5, 0.0, 0.0],
            uv: [0.0, 1.0],
        },
        BoardVertex {
            position: [0.5, 0.0, 0.0],
            uv: [1.0, 1.0],
        },
        BoardVertex {
            position: [0.5, 1.0, 0.0],
            uv: [1.0, 0.0],
        },
        BoardVertex {
            position: [-0.5, 1.0, 0.0],
            uv: [0.0, 0.0],
        },
    ];
    let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("local art sprite vertices"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("local art sprite indices"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("local art sprite instances"),
        size: (MAX_SPRITE_INSTANCES * std::mem::size_of::<SpriteInstanceRaw>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    Ok(Some(SpriteArt {
        pipeline,
        vertex_buffer,
        index_buffer,
        instance_buffer,
        index_count: indices.len() as u32,
        assets,
    }))
}

fn ensure_texture_fits(device: &wgpu::Device, dimensions: (u32, u32), path: &Path) -> Result<()> {
    let limit = device.limits().max_texture_dimension_2d;
    if dimensions.0 > limit || dimensions.1 > limit {
        return Err(anyhow!(
            "local art {} is {}x{}, exceeding this GPU's {limit}px texture limit",
            path.display(),
            dimensions.0,
            dimensions.1
        ));
    }
    Ok(())
}

fn prop_sprite_size(kind: PropKind) -> (f32, f32) {
    match kind {
        PropKind::Stairs => (1.92, 0.18),
        PropKind::Table => (1.20, 0.92),
        PropKind::Chest => (0.92, 0.72),
        PropKind::Bookcase => (1.12, 1.45),
        PropKind::Throne => (1.10, 1.38),
        PropKind::WeaponRack => (1.10, 1.35),
        PropKind::AlchemistsBench => (1.32, 1.05),
        PropKind::Tomb => (1.42, 0.82),
        PropKind::SorcerersTable => (1.34, 1.05),
        PropKind::TortureRack => (1.32, 0.88),
        PropKind::Fireplace => (1.45, 1.42),
        PropKind::Cupboard => (1.18, 1.38),
        PropKind::StarOfWest => (0.48, 0.10),
    }
}

fn cube_mesh() -> (Vec<Vertex>, Vec<u16>) {
    let faces = [
        (
            [0.0, 0.0, 1.0],
            [
                [-1.0, -1.0, 1.0],
                [1.0, -1.0, 1.0],
                [1.0, 1.0, 1.0],
                [-1.0, 1.0, 1.0],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [1.0, -1.0, -1.0],
                [-1.0, -1.0, -1.0],
                [-1.0, 1.0, -1.0],
                [1.0, 1.0, -1.0],
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            [
                [1.0, -1.0, 1.0],
                [1.0, -1.0, -1.0],
                [1.0, 1.0, -1.0],
                [1.0, 1.0, 1.0],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [-1.0, -1.0, -1.0],
                [-1.0, -1.0, 1.0],
                [-1.0, 1.0, 1.0],
                [-1.0, 1.0, -1.0],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [-1.0, 1.0, 1.0],
                [1.0, 1.0, 1.0],
                [1.0, 1.0, -1.0],
                [-1.0, 1.0, -1.0],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [-1.0, -1.0, -1.0],
                [1.0, -1.0, -1.0],
                [1.0, -1.0, 1.0],
                [-1.0, -1.0, 1.0],
            ],
        ),
    ];
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (face_index, (normal, positions)) in faces.into_iter().enumerate() {
        let base = face_index as u16 * 4;
        vertices.extend(
            positions
                .into_iter()
                .map(|position| Vertex { position, normal }),
        );
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (vertices, indices)
}

fn create_surface(
    instance: &wgpu::Instance,
    window: &Window,
) -> std::result::Result<wgpu::Surface<'static>, String> {
    let raw_display_handle = window
        .display_handle()
        .map_err(|error| error.to_string())?
        .as_raw();
    let raw_window_handle = window
        .window_handle()
        .map_err(|error| error.to_string())?
        .as_raw();
    // SAFETY: `main` declares the SDL window before the renderer, so Rust drops
    // the renderer and its surface first. SDL keeps both raw handles valid for
    // the complete lifetime of the window.
    unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(raw_display_handle),
            raw_window_handle,
        })
    }
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use glam::Vec3Swizzles;
    use image::GenericImageView;

    use super::*;
    use crate::dice::DiceTray;

    #[test]
    fn living_multi_body_monsters_show_one_skull_counter_per_body_lost() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_fire_mage().unwrap(),
            0x534b_554c_4c,
        )
        .unwrap();
        let balur = game
            .units
            .iter()
            .find(|unit| unit.name == "Balur")
            .unwrap()
            .id;
        assert_eq!(monster_damage_marker_count(game.unit(balur).unwrap()), 0);
        game.units
            .iter_mut()
            .find(|unit| unit.id == balur)
            .unwrap()
            .body = 1;
        assert_eq!(monster_damage_marker_count(game.unit(balur).unwrap()), 2);
        game.units
            .iter_mut()
            .find(|unit| unit.id == balur)
            .unwrap()
            .alive = false;
        assert_eq!(monster_damage_marker_count(game.unit(balur).unwrap()), 0);
    }

    #[test]
    fn visible_overflow_monsters_use_their_logical_classic_sculpt() {
        let mut game =
            Game::from_quest(QuestDefinition::original_us_trial().unwrap(), 0x4f56_4552).unwrap();
        let monster = game
            .units
            .iter_mut()
            .find(|unit| matches!(unit.figure, FigureKind::Monster(_)))
            .unwrap();
        let logical = monster.figure;
        monster.physical_figure = None;
        assert_eq!(rendered_figure(monster), logical);
        monster.physical_figure = Some(FigureKind::Monster(MonsterKind::Goblin));
        assert_eq!(
            rendered_figure(monster),
            FigureKind::Monster(MonsterKind::Goblin)
        );
    }

    #[test]
    fn real_model_slots_cover_every_figure_furniture_door_and_marker_type() {
        let keys: HashSet<_> = PIECE_MODEL_SLOTS.iter().map(|(key, _)| *key).collect();
        assert_eq!(
            keys.iter()
                .filter(|key| matches!(key, SpriteKey::Figure(_)))
                .count(),
            13
        );
        assert_eq!(
            keys.iter()
                .filter(|key| matches!(key, SpriteKey::Prop(_)))
                .count(),
            11
        );
        for key in [
            SpriteKey::DoorOpen,
            SpriteKey::DoorClosed,
            SpriteKey::SecretDoor,
            SpriteKey::Trap(TrapKind::Pit),
            SpriteKey::Trap(TrapKind::FallingBlock),
            SpriteKey::BlockedSquare,
            SpriteKey::BlockedDouble,
            SpriteKey::DamageSkull,
            SpriteKey::FurnitureRat,
            SpriteKey::FurnitureSkull,
            SpriteKey::Die(DieKind::Movement),
            SpriteKey::Die(DieKind::Combat),
        ] {
            assert!(keys.contains(&key), "missing model slot for {key:?}");
        }
        assert_eq!(keys.len(), 36);
        assert!(!keys.contains(&SpriteKey::Prop(PropKind::Stairs)));
        assert!(!keys.contains(&SpriteKey::Prop(PropKind::StarOfWest)));
        assert_eq!(
            PIECE_MODEL_SLOTS
                .iter()
                .filter(|(key, _)| {
                    *key == SpriteKey::Figure(FigureKind::Monster(MonsterKind::Goblin))
                })
                .count(),
            3
        );
        assert_eq!(
            PIECE_MODEL_SLOTS
                .iter()
                .filter(|(key, _)| {
                    *key == SpriteKey::Figure(FigureKind::Monster(MonsterKind::Orc))
                })
                .count(),
            5
        );

        let model_root = local_art_root().join("models");
        let mut paths = HashSet::new();
        for &(_, relative_path) in PIECE_MODEL_SLOTS {
            assert!(
                paths.insert(relative_path),
                "duplicate GLB slot {relative_path}"
            );
            let path = model_root.join(relative_path);
            let metadata = std::fs::metadata(&path)
                .unwrap_or_else(|error| panic!("required classic GLB {}: {error}", path.display()));
            assert!(
                metadata.len() > 1_000,
                "required classic GLB {} is empty or a placeholder",
                path.display()
            );
        }
        assert_eq!(paths.len(), PIECE_MODEL_SLOTS.len());
    }

    #[test]
    fn dropped_nonphysical_quest_object_uses_a_gold_pulse_not_a_fake_model() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_barak_tor().unwrap(),
            0x5354_4152,
        )
        .unwrap();
        let item_index = game
            .quest_items
            .iter()
            .position(|item| item.id == "Star of the West")
            .unwrap();
        assert!(visible_nonphysical_quest_item_positions(&game).is_empty());
        let prop_index = game.quest_items[item_index].prop_index;
        let holder = game.quest_items[item_index].holder.take().unwrap();
        let drop_pos = game.unit(holder).unwrap().pos;
        game.units
            .iter_mut()
            .find(|unit| unit.id == holder)
            .unwrap()
            .carried_quest_item = None;
        game.props[prop_index].carried_by = None;
        game.props[prop_index].pos = drop_pos;
        game.cells[Game::cell_index(drop_pos)].revealed = true;

        assert_eq!(visible_nonphysical_quest_item_positions(&game), [drop_pos]);
        game.quest_items[item_index].delivered = true;
        assert!(visible_nonphysical_quest_item_positions(&game).is_empty());
    }

    #[test]
    fn removable_furniture_dressing_is_finite_and_uses_visible_classic_mounts() {
        let mut game = Game::demo(0x4452_4553_5349_4e47).unwrap();
        game.props = vec![
            crate::game::Prop {
                kind: PropKind::Bookcase,
                pos: Pos::new(2, 2),
                rotation_quarters: 0,
                visible: true,
                carried_by: None,
            },
            crate::game::Prop {
                kind: PropKind::Bookcase,
                pos: Pos::new(6, 2),
                rotation_quarters: 1,
                visible: true,
                carried_by: None,
            },
            crate::game::Prop {
                kind: PropKind::Cupboard,
                pos: Pos::new(10, 2),
                rotation_quarters: 0,
                visible: true,
                carried_by: None,
            },
            crate::game::Prop {
                kind: PropKind::Fireplace,
                pos: Pos::new(14, 2),
                rotation_quarters: 1,
                visible: true,
                carried_by: None,
            },
            crate::game::Prop {
                kind: PropKind::Fireplace,
                pos: Pos::new(18, 2),
                rotation_quarters: 0,
                visible: true,
                carried_by: None,
            },
            crate::game::Prop {
                kind: PropKind::Table,
                pos: Pos::new(2, 7),
                rotation_quarters: 0,
                visible: true,
                carried_by: None,
            },
        ];
        for prop in &game.props {
            game.cells[Game::cell_index(prop.pos)].revealed = true;
        }

        let rats = furniture_dressing_placements(&game, SpriteKey::FurnitureRat);
        let skulls = furniture_dressing_placements(&game, SpriteKey::FurnitureSkull);
        assert_eq!(rats.len(), 4);
        assert_eq!(skulls.len(), 4);
        assert!(rats.iter().all(|placement| placement.position.y > 1.5));
        assert!(skulls.iter().all(|placement| placement.position.y > 1.5));
        assert_ne!(rats[0].position, skulls[0].position);
    }

    #[test]
    fn combat_die_decals_match_the_original_three_two_one_face_distribution() {
        let faces = (1..=6).map(combat_decal_kind).collect::<Vec<_>>();
        assert_eq!(
            faces
                .iter()
                .filter(|&&kind| kind == DiceDecalKind::Skull)
                .count(),
            3
        );
        assert_eq!(
            faces
                .iter()
                .filter(|&&kind| kind == DiceDecalKind::WhiteShield)
                .count(),
            2
        );
        assert_eq!(
            faces
                .iter()
                .filter(|&&kind| kind == DiceDecalKind::BlackShield)
                .count(),
            1
        );
    }

    #[test]
    fn movement_die_uses_twenty_one_round_decal_pips_over_the_real_mesh() {
        assert_eq!(movement_pips(1).len(), 1);
        assert_eq!(movement_pips(2).len(), 2);
        assert_eq!(movement_pips(3).len(), 3);
        assert_eq!(movement_pips(4).len(), 4);
        assert_eq!(movement_pips(5).len(), 5);
        assert_eq!(movement_pips(6).len(), 6);
        assert_eq!(
            (1..=6).map(|face| movement_pips(face).len()).sum::<usize>(),
            21
        );

        assert!(
            PIECE_MODEL_SLOTS.contains(&(SpriteKey::Die(DieKind::Movement), "dice/movement.glb"))
        );
        assert!(PIECE_MODEL_SLOTS.contains(&(SpriteKey::Die(DieKind::Combat), "dice/combat.glb")));
    }

    #[test]
    fn die_mesh_and_decals_share_the_same_rotating_body_center() {
        let center = Vec3::new(3.2, 7.4, -1.8);
        let rotation = Quat::from_euler(glam::EulerRot::YXZ, 0.73, -0.46, 1.08);
        let model = die_model_transform(center, rotation);

        let transformed_center = model.transform_point3(Vec3::Y * DIE_HALF_EXTENT);
        assert!((transformed_center - center).length() < 0.000_01);

        let transformed_top = model.transform_point3(Vec3::Y * (DIE_HALF_EXTENT * 2.0));
        let expected_top = center + rotation * (Vec3::Y * DIE_HALF_EXTENT);
        assert!((transformed_top - expected_top).length() < 0.000_01);

        let decal = combat_decal_instance(center, rotation, 1, 1.0);
        let decal_model = Mat4::from_cols_array_2d(&decal.model);
        let decal_center = decal_model.transform_point3(Vec3::ZERO);
        assert!((decal_center - expected_top).length() < 0.02);
        assert!(decal_center.distance(center) > DIE_HALF_EXTENT);
    }

    #[test]
    fn stairwell_is_a_flat_two_by_two_cardboard_cutout() {
        let surface = component_prop_decal(PropKind::Stairs, Pos::new(1, 1), 0).unwrap();
        let center = surface.transform_point3(Vec3::ZERO);
        assert!((center - Vec3::new(-11.0, 0.082, -7.5)).length() < 0.000_01);
        let x_extent = surface.transform_vector3(Vec3::X).length();
        let y_extent = surface.transform_vector3(Vec3::Y).length();
        assert!((x_extent - 1.92).abs() < 0.000_01);
        assert!((y_extent - 1.92).abs() < 0.000_01);

        let path = local_art_root().join("components/markers/stairs.png");
        let scan = image::open(&path)
            .unwrap_or_else(|error| panic!("failed to open {}: {error}", path.display()));
        assert_eq!(scan.dimensions(), (565, 570));
    }

    #[test]
    fn door_art_has_one_thin_layered_cardboard_insert() {
        let center = Vec3::new(2.0, 0.0, -3.0);
        let layers = component_door_layers(center, 0.0);
        assert_eq!(layers.len(), CARDBOARD_LAYER_COUNT);
        let front = layers[CARDBOARD_LAYER_COUNT - 1];
        let back = layers[0];
        let front_center = front.transform_point3(Vec3::ZERO);
        let back_center = back.transform_point3(Vec3::ZERO);
        assert!((front_center.y - 0.72).abs() < 0.000_01);
        assert!((back_center.y - 0.72).abs() < 0.000_01);
        assert!(
            (front_center.distance(back_center) - CARDBOARD_HALF_THICKNESS * 2.0).abs() < 0.000_01
        );
        assert!(CARDBOARD_HALF_THICKNESS * 2.0 < 0.06);

        let path = local_art_root().join("components/doors/open.png");
        let scan = image::open(&path)
            .unwrap_or_else(|error| panic!("failed to open {}: {error}", path.display()))
            .to_rgba8();
        let (width, height) = scan.dimensions();
        let opaque_fraction =
            scan.pixels().filter(|pixel| pixel[3] >= 240).count() as f32 / (width * height) as f32;
        assert!(
            (0.68..0.82).contains(&opaque_fraction),
            "open-door alpha mask retained only {:.1}% of the printed arch",
            opaque_fraction * 100.0
        );
        assert_eq!(scan.get_pixel(0, 0)[3], 0);
        assert_eq!(scan.get_pixel(width / 2, height / 2)[3], 0);
        assert!(scan.get_pixel(width / 5, height / 2)[3] >= 240);
        assert!(scan.get_pixel(width / 2, 0)[3] >= 240);
    }

    #[test]
    fn furniture_cardboard_faces_have_the_same_subtle_depth() {
        let surface = component_prop_decal(PropKind::Bookcase, Pos::new(2, 2), 0).unwrap();
        let layers = component_cardboard_layers(surface);
        let front = layers[CARDBOARD_LAYER_COUNT - 1].transform_point3(Vec3::ZERO);
        let back = layers[0].transform_point3(Vec3::ZERO);
        assert!((front.distance(back) - CARDBOARD_HALF_THICKNESS * 2.0).abs() < 0.000_01);
    }

    #[test]
    fn every_original_us_die_decal_has_visible_transparent_art() {
        for name in [
            "skull.png",
            "white-shield.png",
            "black-shield.png",
            "movement-pip.png",
        ] {
            let path = local_art_root().join("dice").join(name);
            let rgba = image::open(&path)
                .unwrap_or_else(|error| panic!("failed to open {}: {error}", path.display()))
                .into_rgba8();
            assert_eq!((rgba.width(), rgba.height()), (512, 512));
            let visible = rgba.pixels().filter(|pixel| pixel[3] > 16).count();
            let transparent = rgba.pixels().filter(|pixel| pixel[3] < 16).count();
            assert!(
                visible > 8_000,
                "{} has no useful decal art",
                path.display()
            );
            assert!(
                transparent > 40_000,
                "{} is missing its transparent field",
                path.display()
            );
        }
    }

    #[test]
    fn four_player_stations_surround_the_lower_and_side_edges() {
        let (left, _) = player_station_layout(0);
        let (lower_left, _) = player_station_layout(1);
        let (lower_right, _) = player_station_layout(2);
        let (right, _) = player_station_layout(3);
        assert!(left.x < -16.0 && left.z < 0.0);
        assert!(lower_left.x < 0.0 && lower_left.z < -10.0);
        assert!(lower_right.x > 0.0 && lower_right.z < -10.0);
        assert!(right.x > 16.0 && right.z < 0.0);
        assert!((player_station_card_angle(1) - std::f32::consts::PI).abs() < 0.001);
        assert!((player_station_card_angle(2) - std::f32::consts::PI).abs() < 0.001);
    }

    #[test]
    fn side_station_mats_clear_the_board_and_stay_on_the_enlarged_table() {
        let (left, _) = player_station_layout(0);
        let (right, _) = player_station_layout(3);
        let board_half_width = BOARD_WIDTH as f32 * 0.5;
        let station_world_half_width = 2.70_f32;
        let table_half_width = 18.5_f32 * 1.35;

        assert!(left.x + station_world_half_width < -board_half_width);
        assert!(right.x - station_world_half_width > board_half_width);
        assert!(left.x - station_world_half_width > -table_half_width);
        assert!(right.x + station_world_half_width < table_half_width);
    }

    #[test]
    fn enlarged_table_contains_armory_and_zargon_book_footprints() {
        let table_half_depth = 14.0_f32 * 1.55;
        let armory_outer_edge = 17.45 + 3.25;
        let quest_book_outer_edge = 18.25 + 2.30;

        assert!(armory_outer_edge < table_half_depth);
        assert!(quest_book_outer_edge < table_half_depth);
    }

    #[test]
    fn quest_book_is_turned_to_zargons_board_orientation() {
        assert!((ZARGON_QUEST_BOOK_ANGLE - std::f32::consts::PI).abs() < f32::EPSILON);
    }

    #[test]
    fn piece_movement_follows_a_visible_hop_arc() {
        let state = AnimatedPiece {
            grid_pos: Pos::new(2, 1),
            move_from: Vec3::ZERO,
            move_to: Vec3::X,
            move_started: 10.0,
            move_duration: 0.4,
            facing: 0.0,
            attack_started: None,
            attack_target: Vec3::ZERO,
            hit_started: None,
            death_started: None,
            last_body: 1,
            was_alive: true,
        };
        let midpoint = sample_piece_translation(&state, 10.2);
        assert!((midpoint.x - 0.5).abs() < 0.001);
        assert!(midpoint.y > 0.3);
    }

    #[test]
    fn shared_pit_figures_are_lowered_and_fanned_apart() {
        let mut game = Game::demo(0x5049_5450_4f53_45).unwrap();
        let first = game.hero_order[0];
        let second = game.hero_order[1];
        let pit = Pos::new(10, 10);
        for unit in &mut game.units {
            unit.alive = unit.id == first || unit.id == second;
        }
        for &id in &[first, second] {
            let unit = game.units.iter_mut().find(|unit| unit.id == id).unwrap();
            unit.pos = pit;
            unit.in_pit = true;
        }
        let first_world = unit_board_world(&game, game.unit(first).unwrap());
        let second_world = unit_board_world(&game, game.unit(second).unwrap());
        let center = board_world(pit);

        assert!(first_world.y < center.y && second_world.y < center.y);
        assert!((first_world - second_world).length() > 0.4);
        assert!(
            (((first_world + second_world) * 0.5) - (center - Vec3::Y * 0.12)).length() < 0.001
        );
    }

    #[test]
    fn calibrated_board_plane_maps_playable_grid_to_board_world() {
        let dimensions = (7_170, 5_688);
        let bounds = PixelBounds {
            left: 80,
            top: 64,
            right: 7_082,
            bottom: 5_122,
        };
        let plane = board_plane_from_bounds(dimensions, bounds).unwrap();
        let u0 = bounds.left as f32 / dimensions.0 as f32;
        let u1 = bounds.right as f32 / dimensions.0 as f32;
        let v0 = bounds.top as f32 / dimensions.1 as f32;
        let v1 = bounds.bottom as f32 / dimensions.1 as f32;
        let playable_left = plane.left + u0 * (plane.right - plane.left);
        let playable_right = plane.left + u1 * (plane.right - plane.left);
        let playable_top = plane.top + v0 * (plane.bottom - plane.top);
        let playable_bottom = plane.top + v1 * (plane.bottom - plane.top);

        assert!((playable_left + BOARD_WIDTH as f32 * 0.5).abs() < 0.001);
        assert!((playable_right - BOARD_WIDTH as f32 * 0.5).abs() < 0.001);
        assert!((playable_top + BOARD_HEIGHT as f32 * 0.5).abs() < 0.001);
        assert!((playable_bottom - BOARD_HEIGHT as f32 * 0.5).abs() < 0.001);
    }

    #[test]
    fn furniture_models_are_centered_over_their_rotated_board_footprints() {
        let anchor = Pos::new(6, 5);
        let origin = board_world(anchor);
        assert_eq!(
            prop_world_position(PropKind::Table, anchor, 0, false),
            origin + Vec3::new(1.0, 0.0, 0.5)
        );
        assert_eq!(
            prop_world_position(PropKind::Table, anchor, 1, false),
            origin + Vec3::new(0.5, 0.0, 1.0)
        );
        assert_eq!(
            prop_world_position(PropKind::WeaponRack, anchor, 1, false),
            origin + Vec3::new(0.0, 0.0, 1.0)
        );
        assert_eq!(
            prop_world_position(PropKind::Table, anchor, 1, true),
            origin
        );
    }

    #[test]
    fn board_calibration_rejects_bounds_outside_the_scan() {
        let error = board_plane_from_bounds(
            (100, 80),
            PixelBounds {
                left: 0,
                top: 0,
                right: 101,
                bottom: 80,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("do not fit image 100x80"));
    }

    #[test]
    fn camera_can_orbit_beyond_the_old_quarter_view() {
        let mut camera = Camera::default();
        camera.orbit(-400.0, 0.0);

        assert!(camera.yaw > 2.4);
        assert!((-std::f32::consts::PI..std::f32::consts::PI).contains(&camera.yaw));
    }

    #[test]
    fn camera_reaches_near_table_level_and_near_overhead() {
        let mut camera = Camera::default();
        camera.orbit(0.0, -10_000.0);
        assert!((camera.pitch - 0.03).abs() < f32::EPSILON);

        camera.orbit(0.0, 10_000.0);
        assert!((camera.pitch - 1.53).abs() < f32::EPSILON);
    }

    #[test]
    fn camera_relative_controls_always_point_along_the_visible_screen_axes() {
        let mut camera = Camera::default();
        camera.target = Vec3::ZERO;
        camera.distance = 28.0;
        camera.pitch = 0.62;
        for yaw in [-2.4_f32, -0.7, 0.8, 2.5] {
            camera.yaw = yaw;
            let matrix = camera.matrix(16.0 / 9.0);
            let center = matrix.project_point3(camera.target);
            for (intent, screen_axis) in [
                (Direction::North, glam::Vec2::Y),
                (Direction::East, glam::Vec2::X),
                (Direction::South, -glam::Vec2::Y),
                (Direction::West, -glam::Vec2::X),
            ] {
                let direction = camera.screen_relative_direction(intent, 16.0 / 9.0);
                let projected =
                    matrix.project_point3(camera.target + direction_world_delta(direction));
                let delta = glam::Vec2::new(projected.x - center.x, projected.y - center.y)
                    .normalize_or_zero();
                assert!(
                    delta.dot(screen_axis) > 0.65,
                    "yaw {yaw}, intent {intent:?}"
                );
            }
        }
    }

    #[test]
    fn pointer_ray_round_trips_a_projected_board_square() {
        let target = Pos::new(12, 9);
        let mut camera = Camera::default();
        camera.target = board_world(target);
        camera.distance = 24.0;
        camera.pitch = 0.65;
        camera.yaw = -0.9;
        let width = 1440_u32;
        let height = 900_u32;
        let aspect = width as f32 / height as f32;
        let projected = camera
            .matrix(aspect)
            .project_point3(board_world(target) + Vec3::Y * 0.06);
        let x = (projected.x + 1.0) * 0.5 * width as f32;
        let y = (1.0 - projected.y) * 0.5 * height as f32;
        assert_eq!(
            board_pos_at_screen(&camera, aspect, x, y, width, height),
            Some(target)
        );
    }

    #[test]
    fn spell_target_hit_test_selects_the_projected_figure_body() {
        let mut game = Game::from_quest(QuestDefinition::original_us_trial().unwrap(), 23).unwrap();
        let startup = StartupFlow::default();
        game.apply_hero_setup(
            &startup.heroes,
            &startup.wizard_spells(),
            startup.elf_spells,
        );
        game.phase = GamePhase::HeroTurn { order_index: 3 };
        game.cells.iter_mut().for_each(|cell| cell.revealed = true);
        game.doors.iter_mut().for_each(|door| door.open = true);
        let wizard = game.active_hero_id().unwrap();
        let wizard_world = unit_board_world(&game, game.unit(wizard).unwrap());

        let mut camera = Camera::default();
        camera.target = wizard_world;
        camera.distance = 12.0;
        camera.pitch = 0.66;
        camera.yaw = 0.60;
        let width = 1600_u32;
        let height = 900_u32;
        let aspect = width as f32 / height as f32;
        // Aim high on the miniature. A flat-board ray at this pixel lands
        // behind its base, which is the failure this object hit test avoids.
        let projected = camera
            .matrix(aspect)
            .project_point3(wizard_world + Vec3::Y * 1.15);
        let x = (projected.x + 1.0) * 0.5 * width as f32;
        let y = (1.0 - projected.y) * 0.5 * height as f32;

        assert_eq!(
            hero_spell_target_at_screen(
                &camera,
                &game,
                HeroSpell::SwiftWind,
                aspect,
                x,
                y,
                width,
                height,
            ),
            Some(HeroSpellTarget::Unit(wizard))
        );
    }

    #[test]
    fn spell_target_hit_test_selects_a_projected_door() {
        let mut game = Game::from_quest(QuestDefinition::original_us_trial().unwrap(), 31).unwrap();
        let startup = StartupFlow::default();
        game.apply_hero_setup(
            &startup.heroes,
            &startup.wizard_spells(),
            startup.elf_spells,
        );
        game.phase = GamePhase::HeroTurn { order_index: 3 };
        game.cells.iter_mut().for_each(|cell| cell.revealed = true);
        let door_index = game
            .valid_hero_spell_targets(HeroSpell::Genie)
            .into_iter()
            .find_map(|target| match target {
                HeroSpellTarget::Door(index) => Some(index),
                HeroSpellTarget::Unit(_) => None,
            })
            .expect("the revealed Trial board has a legal closed door");
        let door = &game.doors[door_index];
        let door_world = (board_world(door.a) + board_world(door.b)) * 0.5;

        let mut camera = Camera::default();
        camera.target = door_world;
        camera.distance = 14.0;
        camera.pitch = 0.52;
        camera.yaw = -0.75;
        let width = 1600_u32;
        let height = 900_u32;
        let aspect = width as f32 / height as f32;
        let projected = camera
            .matrix(aspect)
            .project_point3(door_world + Vec3::Y * 0.92);
        let x = (projected.x + 1.0) * 0.5 * width as f32;
        let y = (1.0 - projected.y) * 0.5 * height as f32;

        assert_eq!(
            hero_spell_target_at_screen(
                &camera,
                &game,
                HeroSpell::Genie,
                aspect,
                x,
                y,
                width,
                height,
            ),
            Some(HeroSpellTarget::Door(door_index))
        );
    }

    #[test]
    fn pointer_ray_round_trips_every_neighbor_with_scaled_window_coordinates() {
        let center = Pos::new(12, 9);
        let mut camera = Camera::default();
        camera.target = board_world(center);
        camera.distance = 18.0;
        camera.pitch = 0.82;
        camera.yaw = 0.63;
        let surface_width = 2160_u32;
        let surface_height = 1350_u32;
        let input_width = 1440_u32;
        let input_height = 900_u32;
        let aspect = surface_width as f32 / surface_height as f32;

        for target in [
            center,
            Pos::new(12, 8),
            Pos::new(13, 9),
            Pos::new(12, 10),
            Pos::new(11, 9),
        ] {
            let projected = camera
                .matrix(aspect)
                .project_point3(board_world(target) + Vec3::Y * 0.06);
            let x = (projected.x + 1.0) * 0.5 * input_width as f32;
            let y = (1.0 - projected.y) * 0.5 * input_height as f32;
            assert_eq!(
                board_pos_at_screen(&camera, aspect, x, y, input_width, input_height),
                Some(target),
                "scaled pointer missed {target:?}"
            );
        }
    }

    #[test]
    fn pointer_ray_hits_the_drawn_physical_tabletop_surfaces() {
        let width = 1440_u32;
        let height = 900_u32;
        let aspect = width as f32 / height as f32;
        let (barbarian_center, barbarian_angle) = player_station_layout(0);
        let barbarian_card =
            station_point(barbarian_center, barbarian_angle, -1.65, -0.20) + Vec3::Y * 0.060;
        let (elf_center, elf_angle) = player_station_layout(2);
        let elf_spell = station_point(elf_center, elf_angle, 0.0, -2.03) + Vec3::Y * 0.087;
        let elf_discard = station_point(elf_center, elf_angle, 1.75, -2.03) + Vec3::Y * 0.095;
        let (wizard_center, wizard_angle) = player_station_layout(3);
        let wizard_discard =
            station_point(wizard_center, wizard_angle, 2.90, -2.05) + Vec3::Y * 0.095;
        let cases = [
            (
                barbarian_card,
                TabletopHitTarget::Player(TabletopSurface::HeroCard(HeroKind::Barbarian)),
            ),
            (elf_spell, TabletopHitTarget::ElfSpell(1)),
            (elf_discard, TabletopHitTarget::ElfDiscard),
            (wizard_discard, TabletopHitTarget::WizardDiscard),
            (
                Vec3::new(-9.8, 0.181, 17.55),
                TabletopHitTarget::Zargon(TabletopSurface::ZargonDeck(ZargonDeckKind::Artifact)),
            ),
            (
                Vec3::new(8.67, 0.097, 17.25),
                TabletopHitTarget::Zargon(TabletopSurface::MonsterCard(MonsterKind::Fimir)),
            ),
            (
                Vec3::new(0.0, 0.107, 18.25),
                TabletopHitTarget::Zargon(TabletopSurface::QuestBook),
            ),
            (
                Vec3::new(-7.6, 0.103, 19.65),
                TabletopHitTarget::ChaosDiscard,
            ),
        ];

        for (world, expected) in cases {
            let mut camera = Camera::default();
            camera.target = Vec3::new(world.x, 0.0, world.z);
            camera.distance = 18.0;
            camera.pitch = 0.82;
            camera.yaw = 0.63;
            let projected = camera.matrix(aspect).project_point3(world);
            let x = (projected.x + 1.0) * 0.5 * width as f32;
            let y = (1.0 - projected.y) * 0.5 * height as f32;
            assert_eq!(
                tabletop_hit_target_at_screen(&camera, aspect, x, y, width, height),
                Some(expected),
                "missed physical tabletop surface at {world:?}"
            );
        }
    }

    #[test]
    fn destination_marker_is_a_full_translucent_pulsating_square() {
        let mut dim = Vec::new();
        let mut bright = Vec::new();
        add_square_highlight(&mut dim, Pos::new(12, 9), [1.0, 0.42, 0.015], 0.0);
        add_square_highlight(
            &mut bright,
            Pos::new(12, 9),
            [1.0, 0.42, 0.015],
            std::f32::consts::PI / (2.0 * 4.6),
        );
        assert_eq!(dim.len(), 1);
        assert_eq!(bright.len(), 1);
        assert!(bright[0].color[0] > dim[0].color[0]);
        assert!(bright[0].color[3] > dim[0].color[3]);
        assert!(dim[0].color[3] > 0.0 && bright[0].color[3] < 1.0);
        let tile_center = board_world(Pos::new(12, 9));
        let model = Mat4::from_cols_array_2d(&dim[0].model);
        assert!(
            model
                .transform_point3(Vec3::ZERO)
                .xz()
                .distance(tile_center.xz())
                < 0.001
        );
        assert!((model.transform_vector3(Vec3::X).length() - 0.455).abs() < 0.001);
        assert!((model.transform_vector3(Vec3::Z).length() - 0.455).abs() < 0.001);
    }

    #[test]
    fn reachable_tile_under_cursor_switches_from_orange_to_pulsing_blue() {
        let mut game = Game::demo(0x484f_5645_52).unwrap();
        game.apply_movement_roll(&[6, 6]).unwrap();
        let target = game.active_move_destinations()[0];
        let target_world = board_world(target);
        let color_at_target = |instances: &[InstanceRaw]| {
            instances
                .iter()
                .find(|instance| {
                    let position =
                        Mat4::from_cols_array_2d(&instance.model).transform_point3(Vec3::ZERO);
                    (position.x - target_world.x).abs() < 0.001
                        && (position.z - target_world.z).abs() < 0.001
                })
                .unwrap()
                .color
        };

        let ordinary = build_highlights(&game, &[], None, 0.4);
        let hovered = build_highlights(&game, &[], Some(target), 0.4);
        assert!(color_at_target(&ordinary)[0] > color_at_target(&ordinary)[2]);
        assert!(color_at_target(&hovered)[2] > color_at_target(&hovered)[0]);
        assert!(color_at_target(&hovered)[3] < 1.0);
    }

    #[test]
    fn active_hero_halo_is_a_pulsing_ring_below_the_figure() {
        let target = Pos::new(8, 6);
        let mut dim = Vec::new();
        let mut bright = Vec::new();
        add_active_hero_halo(&mut dim, target, 0.0);
        add_active_hero_halo(&mut bright, target, std::f32::consts::PI / (2.0 * 3.8));
        assert_eq!(dim.len(), 24);
        assert_eq!(bright.len(), 24);
        assert!(bright[0].color[3] > dim[0].color[3]);
        let center = board_world(target);
        assert!(dim.iter().all(|instance| {
            let model = Mat4::from_cols_array_2d(&instance.model);
            let position = model.transform_point3(Vec3::ZERO);
            let radius = position.xz().distance(center.xz());
            let top = position.y + model.transform_vector3(Vec3::Y).length();
            (0.42..0.47).contains(&radius) && top < 0.05
        }));

        let mut movement = Vec::new();
        add_square_highlight(&mut movement, target, [1.0, 0.42, 0.015], 0.0);
        let movement_model = Mat4::from_cols_array_2d(&movement[0].model);
        let movement_center = movement_model.transform_point3(Vec3::ZERO);
        let movement_bottom =
            movement_center.y - movement_model.transform_vector3(Vec3::Y).length();
        let halo_top = dim
            .iter()
            .map(|instance| {
                let model = Mat4::from_cols_array_2d(&instance.model);
                model.transform_point3(Vec3::ZERO).y + model.transform_vector3(Vec3::Y).length()
            })
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(halo_top + 0.015 < movement_bottom);
    }

    #[test]
    fn current_monster_attacker_keeps_facing_its_exact_hero_target() {
        let mut game = Game::demo(0x4641_4345).unwrap();
        let monster = game
            .units
            .iter()
            .find(|unit| unit.faction == Faction::Monster)
            .unwrap()
            .id;
        let defender = game.hero_order[2];
        game.last_combat_visual = Some(crate::game::CombatVisualEvent {
            sequence: 1,
            attacker: monster,
            defender,
            damage: 0,
            defender_died: false,
        });
        let mut animations = PieceAnimations::default();
        animations.poses(&game, 1.0);
        let poses = animations.poses(&game, 1.1);
        let attacker_pose = poses[&monster];
        let from = unit_board_world(&game, game.unit(monster).unwrap());
        let to = unit_board_world(&game, game.unit(defender).unwrap());
        let expected = Vec3::new(to.x - from.x, 0.0, to.z - from.z).normalize();
        let actual = (attacker_pose.rotation * Vec3::Z).normalize();
        assert!(actual.dot(expected) > 0.999);
    }

    #[test]
    fn furniture_scan_faces_hug_their_models_instead_of_floating() {
        let table = component_prop_decal(PropKind::Table, Pos::new(2, 2), 0).unwrap();
        assert!((table.transform_point3(Vec3::ZERO).y - 0.74).abs() < 0.001);
        let bookcase = component_prop_decal(PropKind::Bookcase, Pos::new(2, 2), 0).unwrap();
        assert!((bookcase.transform_point3(Vec3::ZERO).y - 0.45).abs() < 0.001);
        let model_forward = prop_model_local_rotation(PropKind::Bookcase) * Vec3::Z;
        assert!(model_forward.dot(Vec3::X) > 0.999);
    }

    #[test]
    fn action_camera_converges_smoothly_without_overshooting() {
        let desired = Vec3::new(12.0, 0.38, -7.0);
        let first = smooth_camera_target(Vec3::ZERO, desired, 1.0 / 60.0);
        let second = smooth_camera_target(first, desired, 1.0 / 60.0);
        assert!(first.length() > 0.0);
        assert!(second.distance(desired) < first.distance(desired));
        assert!(second.x <= desired.x && second.z >= desired.z);
    }

    #[test]
    fn dice_focus_zooms_toward_the_rollers_tabletop() {
        let game = Game::demo(17).unwrap();
        let mut camera = Camera::default();
        let original_distance = camera.distance;
        let center = player_station_layout(0).0 + Vec3::Y * 0.08;
        let original_target_distance = camera.target.distance(center);
        let mut director = ActionCameraDirector::new();
        director.focus_dice(center);
        director.last_update = Instant::now() - Duration::from_millis(100);
        director.update(&mut camera, &game, &HashMap::new());
        assert!(camera.distance < original_distance);
        assert!(camera.target.distance(center) < original_target_distance);
    }

    #[test]
    fn dice_rolls_are_relocated_to_the_rolling_hero_and_zargons_open_table() {
        let game = Game::demo(17).unwrap();
        let hero = game.hero_order[0];
        let hero_roll = dice_roll_transform(&game, Some(hero));
        assert_eq!(
            hero_roll.center,
            player_station_layout(0).0 + Vec3::Y * 0.08
        );
        assert_eq!(hero_roll.station, Some(0));
        assert_ne!(hero_roll.center, ZARGON_DICE_ROLL_CENTER);

        let zargon_roll = dice_roll_transform(&game, None);
        assert_eq!(zargon_roll.center, ZARGON_DICE_ROLL_CENTER);
        assert_eq!(zargon_roll.station, None);
    }

    #[test]
    fn hero_roll_begins_with_the_exact_dice_visible_on_that_heroes_rack() {
        let game = Game::demo(17).unwrap();
        let hero = game.hero_order[0];
        let transform = dice_roll_transform(&game, Some(hero));
        let local_poses = DiceTray::movement(0x4f57_4e45_44).poses();
        let world_poses = local_poses
            .iter()
            .copied()
            .map(|pose| transform.world_pose(pose))
            .collect::<Vec<_>>();

        for (die_index, world_pose) in world_poses.iter().enumerate() {
            let (rack_center, rack_rotation, rack_kind, rack_scale) =
                player_station_die_pose(0, die_index);
            assert_eq!(world_pose.kind, rack_kind);
            assert!(world_pose.translation.distance(rack_center) < 0.0001);
            assert!(world_pose.rotation.dot(rack_rotation).abs() > 0.9999);
            assert_eq!(rack_scale, 1.0);
        }
    }

    #[test]
    fn a_roll_removes_only_its_owned_dice_from_the_active_station_rack() {
        let movement = DiceTray::movement(17).poses();
        assert!(rack_die_is_rolling(0, 0, Some(0), &movement));
        assert!(rack_die_is_rolling(0, 1, Some(0), &movement));
        assert!(
            (2..PLAYER_DICE_RACK.len()).all(|index| !rack_die_is_rolling(
                0,
                index,
                Some(0),
                &movement
            ))
        );
        assert!(
            (0..PLAYER_DICE_RACK.len()).all(|index| !rack_die_is_rolling(
                1,
                index,
                Some(0),
                &movement
            ))
        );

        let combat = DiceTray::combat(23, 3).poses();
        assert!((0..2).all(|index| !rack_die_is_rolling(0, index, Some(0), &combat)));
        assert!((2..5).all(|index| rack_die_is_rolling(0, index, Some(0), &combat)));
        assert!(
            (5..PLAYER_DICE_RACK.len()).all(|index| !rack_die_is_rolling(
                0,
                index,
                Some(0),
                &combat
            ))
        );

        let visible = visible_die_poses(&movement, Some(0));
        assert_eq!(visible.len(), 4 * PLAYER_DICE_RACK.len());
        assert_eq!(
            visible
                .iter()
                .filter(|die| die.kind == DieKind::Movement)
                .count(),
            8
        );
    }

    #[test]
    fn hud_wrap_keeps_osd_copy_inside_its_measured_line_width() {
        let wrapped = hud_wrap_text(
            "[WASD] MOVE [O] OPEN DOOR [F] ATTACK [T] SEARCH TREASURE [E] END TURN",
            24,
            4,
        );
        let lines = wrapped.lines().collect::<Vec<_>>();
        assert!(lines.len() <= 4);
        assert!(lines.iter().all(|line| line.len() <= 24));
    }

    #[test]
    fn bundled_medieval_font_is_proportional_antialiased_and_readable() {
        let size = medieval_pixel_size(2);
        assert!(medieval_text_width("IIII", size) < medieval_text_width("WWWW", size));
        assert!(medieval_text_width("HEROQUEST | THE LOST WIZARD", size) < 900.0);
        assert!(startup_font_pixel_size(2) >= 17.0);
        assert!(startup_font_pixel_size(3) > startup_font_pixel_size(2));

        let mut canvas = image::RgbaImage::new(360, 64);
        draw_medieval_text(
            &mut canvas,
            "HeroQuest - Choose an action",
            8,
            8,
            size,
            20,
            image::Rgba([242, 211, 139, 255]),
        );
        let visible = canvas.pixels().filter(|pixel| pixel[3] > 0).count();
        let antialiased = canvas
            .pixels()
            .filter(|pixel| (1..=254).contains(&pixel[3]))
            .count();
        assert!(visible > 500);
        assert!(antialiased > 250);
    }

    #[test]
    fn hud_action_buttons_have_nonoverlapping_pointer_hit_rectangles() {
        let rects = hud_action_button_rects(8);
        assert_eq!(rects.len(), 8);
        for (index, &(x, y, width, height)) in rects.iter().enumerate() {
            assert!(width >= 100 && height >= 27);
            assert!(x + width <= HUD_TEXTURE_WIDTH);
            assert!(y + height <= HUD_TEXTURE_HEIGHT);
            assert!(rects.iter().enumerate().all(
                |(other_index, &(other_x, other_y, other_width, other_height))| {
                    index == other_index
                        || x + width <= other_x
                        || other_x + other_width <= x
                        || y + height <= other_y
                        || other_y + other_height <= y
                }
            ));
        }
    }

    #[test]
    fn hud_panels_and_modal_are_rasterized_inside_the_screen_texture() {
        let overlay = GameOverlay {
            heading: "HEROQUEST | THE LOST WIZARD".to_owned(),
            status: "Barbarian | Body 8/8 | Move 0".to_owned(),
            message: "Choose the next legal action.".to_owned(),
            actions: vec!["[R] ROLL MOVEMENT".to_owned(), "[E] END TURN".to_owned()],
            dialog: Some(OverlayDialog {
                title: "JUMP A TRAP".to_owned(),
                body: "Choose a safe direction.".to_owned(),
                hint: "Press WASD or an arrow key.".to_owned(),
            }),
        };
        let canvas = draw_hud_overlay(&overlay);
        assert_eq!(canvas.dimensions(), (HUD_TEXTURE_WIDTH, HUD_TEXTURE_HEIGHT));
        assert_eq!(canvas.get_pixel(1023, 0).0[3], 0);
        assert!(canvas.get_pixel(997, 17).0[3] > 0);
        assert!(canvas.get_pixel(197, 157).0[3] > 0);
        if let Some(path) = std::env::var_os("HEROQUEST_UI_PREVIEW") {
            canvas.save(path).unwrap();
        }
    }
}
