use glam::{Quat, Vec3};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rapier3d::prelude::*;
use std::sync::mpsc::{Receiver, channel};

use crate::model::CombatFace;

pub const DIE_HALF_EXTENT: f32 = 0.32;
const FIXED_DT: f32 = 1.0 / 120.0;
// The tabletop and camera deliberately exaggerate the dice to keep their
// faces readable. A literal inch-to-metre conversion made an eight-unit lift
// cross the screen in about 0.2 seconds and read as super-Earth gravity. This
// visually calibrated acceleration gives the high throw a half-second fall:
// quick enough to retain weight, slow enough for the eye to follow.
const TABLETOP_GRAVITY: f32 = -62.0;
// Release from roughly eight board inches above the player's tabletop.  The
// camera deliberately frames the complete rack-to-table arc, so this reads as
// a real handful of dice being thrown rather than bodies appearing over the
// landing spot.
const DROP_HEIGHT: f32 = 8.10;
const CONTACT_FORCE_THRESHOLD: f32 = 55.0;
const ROLL_HALF_WIDTH: f32 = 3.08;
const ROLL_HALF_DEPTH: f32 = 2.62;
const WALL_HALF_THICKNESS: f32 = 0.06;
const WALL_HALF_HEIGHT: f32 = 5.50;
const MAX_STEPS: u32 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DieKind {
    Movement,
    Combat,
}

/// The eight physical dice kept at each Hero's station. These coordinates are
/// local to the station mat and are shared with the renderer so a roll starts
/// with the same visible dice instead of creating new bodies in midair.
pub const PLAYER_DICE_RACK: [(f32, f32, DieKind); 8] = [
    (2.52, -1.55, DieKind::Movement),
    (2.52, -0.88, DieKind::Movement),
    (2.52, -0.10, DieKind::Combat),
    (2.52, 0.60, DieKind::Combat),
    (2.52, 1.30, DieKind::Combat),
    (1.82, -0.10, DieKind::Combat),
    (1.82, 0.60, DieKind::Combat),
    (1.82, 1.30, DieKind::Combat),
];
pub const PLAYER_DICE_RACK_LOCAL_Y: f32 = DIE_HALF_EXTENT + 0.015;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DieResult {
    Movement(u8),
    Combat(CombatFace),
}

#[derive(Debug, Clone, Copy)]
pub struct DiePose {
    pub kind: DieKind,
    pub translation: Vec3,
    pub rotation: Quat,
}

struct DieBody {
    kind: DieKind,
    handle: RigidBodyHandle,
    rack_translation: Vector,
    drop_translation: Vector,
    release_linear_velocity: Vector,
    release_angular_velocity: Vector,
}

/// A small, self-contained physical dice rolling surface.
///
/// No result is chosen when a throw starts. Rapier advances rounded cuboids on
/// the active player's station, then the local face whose normal points most
/// strongly upward is read after all bodies settle.
pub struct DiceTray {
    pipeline: PhysicsPipeline,
    gravity: Vector,
    integration: IntegrationParameters,
    islands: IslandManager,
    broad_phase: BroadPhaseBvh,
    narrow_phase: NarrowPhase,
    bodies: RigidBodySet,
    colliders: ColliderSet,
    impulse_joints: ImpulseJointSet,
    multibody_joints: MultibodyJointSet,
    ccd_solver: CCDSolver,
    event_handler: ChannelEventCollector,
    _collision_events: Receiver<CollisionEvent>,
    contact_force_events: Receiver<ContactForceEvent>,
    dice: Vec<DieBody>,
    pending_impacts: Vec<f32>,
    steps: u32,
    stable_steps: u16,
    finished: bool,
    released: bool,
    used_timeout_recovery: bool,
}

impl DiceTray {
    pub fn throw(seed: u64, kinds: &[DieKind]) -> Self {
        let mut tray = Self::empty();
        let mut rng = ChaCha8Rng::seed_from_u64(seed);

        // The renderer relocates this local surface to the acting player's
        // station. The collision floor and tall containment walls are not
        // rendered. They follow the tabletop edge so the dice visibly land on
        // the player's table without ever disappearing into the scenery.
        tray.insert_fixed_cuboid(
            Vec3::new(0.0, -0.22, 0.0),
            Vec3::new(ROLL_HALF_WIDTH, 0.22, ROLL_HALF_DEPTH),
        );
        let wall_y = WALL_HALF_HEIGHT;
        tray.insert_fixed_cuboid(
            Vec3::new(ROLL_HALF_WIDTH - WALL_HALF_THICKNESS, wall_y, 0.0),
            Vec3::new(WALL_HALF_THICKNESS, WALL_HALF_HEIGHT, ROLL_HALF_DEPTH),
        );
        tray.insert_fixed_cuboid(
            Vec3::new(-ROLL_HALF_WIDTH + WALL_HALF_THICKNESS, wall_y, 0.0),
            Vec3::new(WALL_HALF_THICKNESS, WALL_HALF_HEIGHT, ROLL_HALF_DEPTH),
        );
        tray.insert_fixed_cuboid(
            Vec3::new(0.0, wall_y, ROLL_HALF_DEPTH - WALL_HALF_THICKNESS),
            Vec3::new(ROLL_HALF_WIDTH, WALL_HALF_HEIGHT, WALL_HALF_THICKNESS),
        );
        tray.insert_fixed_cuboid(
            Vec3::new(0.0, wall_y, -ROLL_HALF_DEPTH + WALL_HALF_THICKNESS),
            Vec3::new(ROLL_HALF_WIDTH, WALL_HALF_HEIGHT, WALL_HALF_THICKNESS),
        );

        for (index, &kind) in kinds.iter().enumerate() {
            // Lift the actual dice from the rack drawn at the right edge of
            // the active player's mat, then throw them inward. The first pose
            // is deliberately identical to the resting renderer pose. If an
            // effect needs more dice than the physical rack holds, additional
            // dice begin in tidy layers over those same rack slots.
            let rack_indices = PLAYER_DICE_RACK
                .iter()
                .enumerate()
                .filter_map(|(rack_index, &(_, _, rack_kind))| {
                    (rack_kind == kind).then_some(rack_index)
                })
                .collect::<Vec<_>>();
            let rack_slot = index % rack_indices.len();
            let layer = index / rack_indices.len();
            let rack_index = rack_indices[rack_slot];
            let (rack_x, rack_z, _) = PLAYER_DICE_RACK[rack_index];
            let translation = Vector::new(
                rack_x - layer as f32 * 0.16,
                PLAYER_DICE_RACK_LOCAL_Y + layer as f32 * 0.70,
                rack_z,
            );
            let rotation = Vector::new(0.0, rack_index as f32 * 0.31, 0.0);
            // Release at the rack side of the mat and throw inward.  The old
            // layout carried every die slowly to its eventual landing area,
            // which made the pre-release motion look like low gravity.  Four
            // separated lanes and raised rows prevent crowded rolls from
            // spawning intersecting rigid bodies.
            let lane = index % 4;
            let row = index / 4;
            let drop_translation = Vector::new(
                2.02 - row as f32 * 0.74 + rng.random_range(-0.06..0.06),
                DROP_HEIGHT + row as f32 * 0.72,
                -1.20 + lane as f32 * 0.80 + rng.random_range(-0.05..0.05),
            );
            let release_linear_velocity = Vector::new(
                rng.random_range(-9.5..-6.5),
                rng.random_range(-1.2..0.8),
                rng.random_range(-2.2..2.2),
            );
            let release_angular_velocity = Vector::new(
                rng.random_range(-34.0..34.0),
                rng.random_range(-34.0..34.0),
                rng.random_range(-34.0..34.0),
            );
            let body = RigidBodyBuilder::dynamic()
                .translation(translation)
                .rotation(rotation)
                .linear_damping(0.18)
                .angular_damping(0.72)
                .ccd_enabled(true)
                .can_sleep(true)
                .build();
            let handle = tray.bodies.insert(body);
            let collider = ColliderBuilder::round_cuboid(
                DIE_HALF_EXTENT - 0.045,
                DIE_HALF_EXTENT - 0.045,
                DIE_HALF_EXTENT - 0.045,
                0.045,
            )
            .density(1.15)
            .friction(0.88)
            .restitution(0.13)
            .active_events(ActiveEvents::CONTACT_FORCE_EVENTS)
            .contact_force_event_threshold(CONTACT_FORCE_THRESHOLD)
            .build();
            tray.colliders
                .insert_with_parent(collider, handle, &mut tray.bodies);
            tray.dice.push(DieBody {
                kind,
                handle,
                rack_translation: translation,
                drop_translation,
                release_linear_velocity,
                release_angular_velocity,
            });
        }
        tray
    }

    pub fn movement(seed: u64) -> Self {
        Self::throw(seed, &[DieKind::Movement, DieKind::Movement])
    }

    pub fn movement_count(seed: u64, count: u8) -> Self {
        Self::throw(seed, &vec![DieKind::Movement; count as usize])
    }

    pub fn combat(seed: u64, count: u8) -> Self {
        Self::throw(seed, &vec![DieKind::Combat; count as usize])
    }

    fn empty() -> Self {
        let (collision_sender, collision_events) = channel();
        let (contact_force_sender, contact_force_events) = channel();
        let integration = IntegrationParameters {
            dt: FIXED_DT,
            num_solver_iterations: 8,
            max_ccd_substeps: 4,
            ..Default::default()
        };
        Self {
            pipeline: PhysicsPipeline::new(),
            // Scene-calibrated Earth-like fall timing; see TABLETOP_GRAVITY.
            gravity: Vector::new(0.0, TABLETOP_GRAVITY, 0.0),
            integration,
            islands: IslandManager::new(),
            broad_phase: BroadPhaseBvh::new(),
            narrow_phase: NarrowPhase::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            ccd_solver: CCDSolver::new(),
            event_handler: ChannelEventCollector::new(collision_sender, contact_force_sender),
            _collision_events: collision_events,
            contact_force_events,
            dice: Vec::new(),
            pending_impacts: Vec::new(),
            steps: 0,
            stable_steps: 0,
            finished: false,
            released: false,
            used_timeout_recovery: false,
        }
    }

    fn insert_fixed_cuboid(&mut self, center: Vec3, half_extents: Vec3) {
        self.colliders.insert(
            ColliderBuilder::cuboid(half_extents.x, half_extents.y, half_extents.z)
                .translation(Vector::new(center.x, center.y, center.z))
                .friction(0.84)
                .restitution(0.10)
                .build(),
        );
    }

    pub fn step(&mut self) {
        if self.finished || !self.released {
            return;
        }
        self.pipeline.step(
            self.gravity,
            &self.integration,
            &mut self.islands,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.ccd_solver,
            &(),
            &self.event_handler,
        );
        self.steps += 1;
        while let Ok(event) = self.contact_force_events.try_recv() {
            if event.started {
                let strength = impact_strength_from_force(event.total_force_magnitude);
                if strength > 0.0 {
                    self.pending_impacts.push(strength);
                }
            }
        }
        self.recover_escaped_dice();

        let stable = self.dice.iter().all(|die| {
            let body = &self.bodies[die.handle];
            body.is_sleeping()
                || (body.linvel().length_squared() < 0.01 && body.angvel().length_squared() < 0.04)
        });
        self.stable_steps = if stable {
            self.stable_steps.saturating_add(1)
        } else {
            0
        };
        self.finished = self.stable_steps >= 30;
        if !self.finished && self.steps >= MAX_STEPS {
            self.settle_after_timeout();
        }
    }

    /// Kinematically carry the same rigid bodies from their visible rack slots
    /// to the release point. Rapier remains paused during this hand animation;
    /// at progress 1 the bodies are exactly where gravity will take over.
    pub fn set_pickup_progress(&mut self, progress: f32) {
        if self.released || self.finished {
            return;
        }
        let progress = progress.clamp(0.0, 1.0);
        // Lift decisively before carrying inward.  Interpolating the complete
        // diagonal path over the entire pickup duration made the dice appear
        // to float under their own power.
        let lift_progress = (progress / 0.58).clamp(0.0, 1.0);
        let carry_progress = ((progress - 0.42) / 0.58).clamp(0.0, 1.0);
        let lift_eased = lift_progress * lift_progress * (3.0 - 2.0 * lift_progress);
        let carry_eased = carry_progress * carry_progress * (3.0 - 2.0 * carry_progress);
        for die in &self.dice {
            let body = &mut self.bodies[die.handle];
            let mut position = die.rack_translation;
            position.y = die.rack_translation.y
                + (die.drop_translation.y - die.rack_translation.y) * lift_eased;
            position.x = die.rack_translation.x
                + (die.drop_translation.x - die.rack_translation.x) * carry_eased;
            position.z = die.rack_translation.z
                + (die.drop_translation.z - die.rack_translation.z) * carry_eased;
            body.set_translation(position, true);
            body.set_linvel(Vector::ZERO, true);
            body.set_angvel(Vector::ZERO, true);
        }
    }

    /// Release the carried dice above the tabletop. From this point onward,
    /// their motion and upward faces are determined only by Rapier physics.
    pub fn release_drop(&mut self) {
        if self.released || self.finished {
            return;
        }
        self.set_pickup_progress(1.0);
        for die in &self.dice {
            let body = &mut self.bodies[die.handle];
            body.set_linvel(die.release_linear_velocity, true);
            body.set_angvel(die.release_angular_velocity, true);
            body.wake_up(true);
        }
        self.released = true;
    }

    /// CCD should keep every die inside the station lip. This is a final guard
    /// against numerical tunnelling: preserve the physical pose, return the
    /// body to the playable surface, and damp its escaped velocity.
    fn recover_escaped_dice(&mut self) {
        let limit_x = ROLL_HALF_WIDTH - DIE_HALF_EXTENT - WALL_HALF_THICKNESS;
        let limit_z = ROLL_HALF_DEPTH - DIE_HALF_EXTENT - WALL_HALF_THICKNESS;
        for die in &self.dice {
            let body = &mut self.bodies[die.handle];
            let position = body.translation();
            let invalid = !position.x.is_finite()
                || !position.y.is_finite()
                || !position.z.is_finite()
                || position.y < -0.75
                || position.x.abs() > ROLL_HALF_WIDTH + 0.5
                || position.z.abs() > ROLL_HALF_DEPTH + 0.5;
            if !invalid {
                continue;
            }

            let x = if position.x.is_finite() {
                position.x.clamp(-limit_x, limit_x)
            } else {
                0.0
            };
            let z = if position.z.is_finite() {
                position.z.clamp(-limit_z, limit_z)
            } else {
                0.0
            };
            body.set_translation(Vector::new(x, DIE_HALF_EXTENT + 0.05, z), true);
            body.set_linvel(Vector::ZERO, true);
            body.set_angvel(body.angvel() * 0.2, true);
        }
    }

    /// Extremely crowded throws can retain tiny solver jitter. After five
    /// simulated seconds, freeze the current physical poses on the surface so
    /// input can never remain locked waiting for a die.
    fn settle_after_timeout(&mut self) {
        let limit_x = ROLL_HALF_WIDTH - DIE_HALF_EXTENT - WALL_HALF_THICKNESS;
        let limit_z = ROLL_HALF_DEPTH - DIE_HALF_EXTENT - WALL_HALF_THICKNESS;
        for die in &self.dice {
            let body = &mut self.bodies[die.handle];
            let position = body.translation();
            body.set_translation(
                Vector::new(
                    position.x.clamp(-limit_x, limit_x),
                    position.y.max(DIE_HALF_EXTENT),
                    position.z.clamp(-limit_z, limit_z),
                ),
                false,
            );
            body.set_linvel(Vector::ZERO, false);
            body.set_angvel(Vector::ZERO, false);
            body.sleep();
        }
        self.used_timeout_recovery = true;
        self.finished = true;
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Drain new collision-energy events produced by the most recent physics
    /// steps. Values are normalized for audio gain, but originate from
    /// Rapier's measured contact force rather than animation timing.
    pub fn drain_impacts(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.pending_impacts)
    }

    pub fn poses(&self) -> Vec<DiePose> {
        self.dice
            .iter()
            .map(|die| {
                let body = &self.bodies[die.handle];
                let translation = body.translation();
                let rotation = body.rotation();
                DiePose {
                    kind: die.kind,
                    translation: Vec3::new(translation.x, translation.y, translation.z),
                    rotation: Quat::from_xyzw(rotation.x, rotation.y, rotation.z, rotation.w),
                }
            })
            .collect()
    }

    pub fn results(&self) -> Option<Vec<DieResult>> {
        self.finished.then(|| {
            self.dice
                .iter()
                .map(|die| {
                    let face = movement_face_from_rotation(self.bodies[die.handle].rotation());
                    match die.kind {
                        DieKind::Movement => DieResult::Movement(face),
                        DieKind::Combat => DieResult::Combat(combat_face_from_number(face)),
                    }
                })
                .collect()
        })
    }

    pub fn simulate_to_rest(&mut self) -> Vec<DieResult> {
        self.release_drop();
        while !self.is_finished() {
            self.step();
        }
        self.results().expect("a settled tray has results")
    }
}

fn impact_strength_from_force(force: f32) -> f32 {
    if !force.is_finite() || force < CONTACT_FORCE_THRESHOLD {
        return 0.0;
    }
    (force / 1_600.0).sqrt().clamp(0.08, 1.0)
}

fn movement_face_from_rotation(rotation: &Rotation) -> u8 {
    let local_faces = [
        (Vector::Y, 1),
        (Vector::Z, 2),
        (Vector::X, 3),
        (-Vector::X, 4),
        (-Vector::Z, 5),
        (-Vector::Y, 6),
    ];
    local_faces
        .into_iter()
        .max_by(|(a, _), (b, _)| {
            (rotation * *a)
                .y
                .partial_cmp(&(rotation * *b).y)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(_, face)| face)
        .unwrap_or(1)
}

fn combat_face_from_number(face: u8) -> CombatFace {
    match face {
        1..=3 => CombatFace::Skull,
        4..=5 => CombatFace::WhiteShield,
        _ => CombatFace::BlackShield,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upward_local_face_determines_the_result() {
        assert_eq!(movement_face_from_rotation(&Rotation::IDENTITY), 1);
        let upside_down = Rotation::from_rotation_x(std::f32::consts::PI);
        assert_eq!(movement_face_from_rotation(&upside_down), 6);
    }

    #[test]
    fn combat_die_has_three_skulls_two_white_shields_and_one_black_shield() {
        let faces: Vec<_> = (1..=6).map(combat_face_from_number).collect();
        assert_eq!(
            faces
                .iter()
                .filter(|&&face| face == CombatFace::Skull)
                .count(),
            3
        );
        assert_eq!(
            faces
                .iter()
                .filter(|&&face| face == CombatFace::WhiteShield)
                .count(),
            2
        );
        assert_eq!(
            faces
                .iter()
                .filter(|&&face| face == CombatFace::BlackShield)
                .count(),
            1
        );
    }

    #[test]
    fn thrown_dice_settle_and_are_read_from_their_pose() {
        let mut tray = DiceTray::movement(0x4845_524f);
        let results = tray.simulate_to_rest();
        assert_eq!(results.len(), 2);
        assert!(
            results
                .iter()
                .all(|result| matches!(result, DieResult::Movement(1..=6)))
        );
        assert!(tray.poses().iter().all(|pose| pose.translation.y > 0.0));
    }

    #[test]
    fn a_throw_lifts_dice_from_the_player_station_rack_edge() {
        let tray = DiceTray::movement(0x5241_434b);
        let poses = tray.poses();
        assert_eq!(poses.len(), 2);
        for (pose, &(rack_x, rack_z, kind)) in poses.iter().zip(&PLAYER_DICE_RACK[..2]) {
            assert_eq!(pose.kind, kind);
            assert!((pose.translation.x - rack_x).abs() < f32::EPSILON);
            assert!((pose.translation.z - rack_z).abs() < f32::EPSILON);
            assert!((pose.translation.y - PLAYER_DICE_RACK_LOCAL_Y).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn owned_dice_are_carried_above_the_table_before_physics_releases_them() {
        let mut tray = DiceTray::movement(0x4452_4f50);
        let rack = tray.poses();
        tray.set_pickup_progress(0.5);
        let carried = tray.poses();
        assert!(!tray.released);
        assert!(carried.iter().zip(&rack).all(|(carried, rack)| {
            carried.translation.y > rack.translation.y + 1.0
                && carried.translation.x < rack.translation.x
        }));

        tray.release_drop();
        assert!(tray.released);
        assert!(
            tray.poses()
                .iter()
                .all(|pose| pose.translation.y >= DROP_HEIGHT)
        );
        let results = tray.simulate_to_rest();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn gravity_gives_the_high_release_an_earth_like_visual_fall() {
        let free_fall_seconds =
            (2.0 * (DROP_HEIGHT - DIE_HALF_EXTENT) / TABLETOP_GRAVITY.abs()).sqrt();
        assert!(
            (0.49..0.52).contains(&free_fall_seconds),
            "tabletop drop took {free_fall_seconds:.3}s"
        );
    }

    #[test]
    fn release_is_high_and_thrown_firmly_inward_from_the_rack_side() {
        let tray = DiceTray::movement(0x5448_524f_57);
        assert!(tray.dice.iter().all(|die| {
            die.drop_translation.y >= 8.0
                && die.drop_translation.x > 1.0
                && die.release_linear_velocity.x <= -6.5
        }));
    }

    #[test]
    fn rapier_contact_energy_produces_bounded_tabletop_impact_events() {
        let mut tray = DiceTray::movement(0x534f_554e_44);
        tray.release_drop();
        let mut impacts = Vec::new();
        while !tray.is_finished() {
            tray.step();
            impacts.extend(tray.drain_impacts());
        }
        assert!(
            impacts.len() >= 2,
            "each thrown die should make at least one audible contact"
        );
        assert!(
            impacts
                .iter()
                .all(|strength| (0.08..=1.0).contains(strength))
        );
        assert!(impacts.iter().any(|strength| *strength > 0.35));
    }

    #[test]
    fn invalid_or_subthreshold_contact_force_is_silent() {
        assert_eq!(impact_strength_from_force(f32::NAN), 0.0);
        assert_eq!(
            impact_strength_from_force(CONTACT_FORCE_THRESHOLD - 0.01),
            0.0
        );
        assert_eq!(impact_strength_from_force(f32::INFINITY), 0.0);
    }

    #[test]
    fn crowded_tabletop_throws_cannot_fall_off_the_physics_surface() {
        for seed in 0..64 {
            let mut tray = DiceTray::combat(0x4851_0000 + seed, 10);
            let results = tray.simulate_to_rest();
            assert_eq!(results.len(), 10);
            assert!(
                !tray.used_timeout_recovery,
                "seed {seed} needed the settlement watchdog"
            );
            assert!(tray.poses().iter().all(|pose| {
                pose.translation.y > 0.0
                    && pose.translation.x.abs() < ROLL_HALF_WIDTH
                    && pose.translation.z.abs() < ROLL_HALF_DEPTH
            }));
        }
    }

    #[test]
    fn every_roll_has_a_hard_settlement_deadline() {
        let mut tray = DiceTray::movement(0x5345_5454_4c45);
        tray.release_drop();
        for die in &tray.dice {
            let body = &mut tray.bodies[die.handle];
            body.set_linvel(Vector::new(0.0, 0.0, 0.0), true);
            body.set_angvel(Vector::new(100.0, 100.0, 100.0), true);
        }
        let results = tray.simulate_to_rest();
        assert_eq!(results.len(), 2);
        assert!(tray.steps <= MAX_STEPS);
        assert!(tray.poses().iter().all(|pose| {
            pose.translation.y > 0.0
                && pose.translation.x.abs() < ROLL_HALF_WIDTH
                && pose.translation.z.abs() < ROLL_HALF_DEPTH
        }));
    }
}
