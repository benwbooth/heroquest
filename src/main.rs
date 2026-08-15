use std::{
    collections::VecDeque,
    mem::MaybeUninit,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use heroquest::audio::TabletopDiceAudio;
use heroquest::campaign::Campaign;
use heroquest::cards::{Artifact, ChaosSpell, HeroSpell};
use heroquest::dice::{DiceTray, DieResult};
use heroquest::equipment::{Armor, ArmoryItem, ORIGINAL_US_ARMORY, Weapon};
use heroquest::game::{
    AttackPlan, DisarmPlan, Game, GamePhase, HealingPotionUse, HeroDeathChoice, HeroSpellCast,
    HeroSpellDiceKind, HeroSpellRoll, HeroSpellTarget, JumpPlan, PendingTrapRoll, PotionKind,
    ZargonStep,
};
use heroquest::input::{TouchZoom, pinch_scale_to_zoom_delta};
use heroquest::model::{CombatFace, Direction, FigureKind, HeroKind, Pos, monster_stats};
use heroquest::quest::QuestDefinition;
use heroquest::renderer::{GameOverlay, OverlayDialog, Renderer, TabletopSurface, ZargonDeckKind};
use heroquest::startup::{StartupFlow, StartupStage};
use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::{Keycode, Mod};
use sdl3::mouse::{Cursor, MouseButton, SystemCursor};

enum DicePurpose {
    Movement,
    AttackOffense(AttackPlan),
    AttackDefense(AttackPlan),
    HeroSpellRed(HeroSpellRoll),
    HeroSpellCombatOffense(HeroSpellRoll),
    HeroSpellCombatDefense(HeroSpellRoll),
    Disarm(DisarmPlan),
    Jump(JumpPlan),
    Trap(PendingTrapRoll),
    CollapsingCeiling { hero: u32 },
    Teleport { subject: u32 },
    HealingPotion { hero: u32 },
    ChaosSpellRoll { target: u32, spell: ChaosSpell },
}

struct PlannedMovement {
    mover: u32,
    destination: Pos,
    steps: VecDeque<Direction>,
    next_step_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpellUi {
    ChooseSpell { index: usize },
    ChooseTarget { spell: HeroSpell, index: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AttackUi {
    index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DoorUi {
    index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PotionChoice {
    Healing,
    HeroicBrew,
    Strength,
    Defense,
    Petrification,
}

impl PotionChoice {
    const fn from_kind(kind: PotionKind) -> Self {
        match kind {
            PotionKind::Healing => Self::Healing,
            PotionKind::HeroicBrew => Self::HeroicBrew,
            PotionKind::Strength => Self::Strength,
            PotionKind::Defense => Self::Defense,
            PotionKind::Petrification => Self::Petrification,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Healing => "Potion of Healing",
            Self::HeroicBrew => "Heroic Brew",
            Self::Strength => "Potion of Strength",
            Self::Defense => "Potion of Defense",
            Self::Petrification => "Mysterious Purple Potion",
        }
    }

    const fn rules(self) -> &'static str {
        match self {
            Self::Healing => "Roll one red die and restore that many lost Body Points.",
            Self::HeroicBrew => "Make two attacks with the next attack action this turn.",
            Self::Strength => "Add two combat dice to the next attack.",
            Self::Defense => "Add two combat dice to the next defense.",
            Self::Petrification => "Become invulnerable stone and miss five turns.",
        }
    }

    const fn kind(self) -> PotionKind {
        match self {
            Self::Healing => PotionKind::Healing,
            Self::HeroicBrew => PotionKind::HeroicBrew,
            Self::Strength => PotionKind::Strength,
            Self::Defense => PotionKind::Defense,
            Self::Petrification => PotionKind::Petrification,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PotionUi {
    index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArtifactAction {
    ElixirOfLife,
    RingOfReturn,
    DeclareSpellRing,
}

impl ArtifactAction {
    const fn artifact(self) -> Artifact {
        match self {
            Self::ElixirOfLife => Artifact::ElixirOfLife,
            Self::RingOfReturn => Artifact::RingOfReturn,
            Self::DeclareSpellRing => Artifact::SpellRing,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArtifactUi {
    ChooseArtifact { index: usize },
    ChooseElixirTarget { index: usize },
    ChooseSpellRingSpell { index: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShareUi {
    Potion { potion: PotionKind, index: usize },
    Gold { index: usize, amount: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TabletopUi {
    surface: TabletopSurface,
    page: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SheetNameEdit {
    hero: HeroKind,
    draft: String,
}

impl TabletopUi {
    const fn new(surface: TabletopSurface) -> Self {
        Self { surface, page: 0 }
    }

    const fn page_count(self) -> usize {
        match self.surface {
            TabletopSurface::CharacterSheet(_) => 5,
            TabletopSurface::Armory => 2,
            _ => 1,
        }
    }
}

enum AppEvent {
    Sdl(Event),
    PinchBegin { window_id: u32 },
    PinchUpdate { window_id: u32, scale: f32 },
    PinchEnd { window_id: u32 },
}

struct DiceAnimation {
    tray: DiceTray,
    purpose: DicePurpose,
    focus_started: Instant,
    last_tick: Instant,
    physics_accumulator: Duration,
    rolling_started: bool,
    result_applied_at: Option<Instant>,
}

impl DiceAnimation {
    const CAMERA_LEAD_IN: Duration = Duration::from_millis(350);
    const PICKUP_DURATION: Duration = Duration::from_millis(460);
    const PHYSICS_STEP: Duration = Duration::from_nanos(8_333_333);
    const RESULT_LINGER: Duration = Duration::from_millis(900);

    fn new(tray: DiceTray, purpose: DicePurpose) -> Self {
        let now = Instant::now();
        Self {
            tray,
            purpose,
            focus_started: now,
            last_tick: now,
            physics_accumulator: Duration::ZERO,
            rolling_started: false,
            result_applied_at: None,
        }
    }

    fn poses(&self) -> Vec<heroquest::dice::DiePose> {
        // Before Rapier receives its first step these are the exact rack poses
        // at the active Hero's station. Keeping them visible through the
        // camera lead-in makes it clear that the player picked up their own
        // dice; no dice pop into existence when the throw begins.
        self.tray.poses()
    }

    /// Hold on the owned rack dice, visibly carry those same rigid bodies over
    /// the tabletop, release them, then run Rapier at its fixed 120 Hz rate.
    fn advance_physics(&mut self, now: Instant) {
        if self.tray.is_finished() || self.result_applied_at.is_some() {
            self.last_tick = now;
            return;
        }
        if !self.rolling_started {
            let since_focus = now.duration_since(self.focus_started);
            if since_focus < Self::CAMERA_LEAD_IN {
                self.last_tick = now;
                return;
            }
            let pickup_elapsed = since_focus - Self::CAMERA_LEAD_IN;
            let pickup_progress =
                pickup_elapsed.as_secs_f32() / Self::PICKUP_DURATION.as_secs_f32();
            self.tray.set_pickup_progress(pickup_progress);
            if pickup_elapsed < Self::PICKUP_DURATION {
                self.last_tick = now;
                return;
            }
            self.tray.release_drop();
            self.rolling_started = true;
            self.last_tick = now;
        }

        let elapsed = now
            .duration_since(self.last_tick)
            .min(Duration::from_millis(100));
        self.last_tick = now;
        self.physics_accumulator += elapsed;
        let mut steps = 0;
        while self.physics_accumulator >= Self::PHYSICS_STEP && steps < 16 {
            self.tray.step();
            self.physics_accumulator -= Self::PHYSICS_STEP;
            steps += 1;
        }
    }
}

fn dice_click_locked(animation: Option<&DiceAnimation>) -> bool {
    animation.is_some_and(|animation| animation.result_applied_at.is_none())
}

fn overlay_action_key(action: &str) -> Option<Keycode> {
    for (token, key) in [
        ("[R]", Keycode::R),
        ("[E]", Keycode::E),
        ("[F]", Keycode::F),
        ("[O]", Keycode::O),
        ("[T]", Keycode::T),
        ("[C]", Keycode::C),
        ("[I]", Keycode::I),
        ("[Y]", Keycode::Y),
        ("[Q]", Keycode::Q),
        ("[N]", Keycode::N),
        ("[B]", Keycode::B),
        ("[K]", Keycode::K),
        ("[G]", Keycode::G),
        ("[V]", Keycode::V),
        ("[U]", Keycode::U),
        ("[L]", Keycode::L),
        ("[P]", Keycode::P),
        ("[X]", Keycode::X),
        ("[J]", Keycode::J),
        ("[M]", Keycode::M),
        ("[D]", Keycode::D),
        ("[H]", Keycode::H),
        ("[W]", Keycode::W),
        ("[A]", Keycode::A),
        ("[S]", Keycode::S),
    ] {
        if action.contains(token) {
            return Some(key);
        }
    }
    if action.contains("[LEFT") {
        Some(Keycode::Left)
    } else if action.contains("[RIGHT") {
        Some(Keycode::Right)
    } else if action.contains("[UP") {
        Some(Keycode::Up)
    } else if action.contains("[DOWN") {
        Some(Keycode::Down)
    } else if action.contains("[ENTER") {
        Some(Keycode::Return)
    } else if action.contains("[BACKSPACE") {
        Some(Keycode::Backspace)
    } else if action.contains("[ESC") {
        Some(Keycode::Escape)
    } else {
        None
    }
}

fn commit_character_sheet_name(
    game: &mut Game,
    campaign: &mut Campaign,
    startup: &mut StartupFlow,
    campaign_path: &Path,
    campaign_enabled: bool,
    hero: HeroKind,
    draft: &str,
) -> Result<String> {
    let mut updated_campaign = campaign.clone();
    updated_campaign.rename_hero(hero, draft)?;
    if campaign_enabled {
        updated_campaign.save(campaign_path)?;
    }
    let name = updated_campaign
        .heroes
        .iter()
        .find(|sheet| sheet.hero == hero)
        .map(|sheet| sheet.name.clone())
        .unwrap_or_else(|| draft.trim().to_owned());
    *campaign = updated_campaign;
    if let Some(unit) = game
        .units
        .iter_mut()
        .find(|unit| unit.figure == FigureKind::Hero(hero))
    {
        unit.name.clone_from(&name);
    }
    if let Some(setup) = startup.heroes.iter_mut().find(|setup| setup.hero == hero) {
        setup.hero_name.clone_from(&name);
    }
    Ok(name)
}

fn main() -> Result<()> {
    env_logger::init();
    let quest_path = quest_path_from_args()?;
    let seed = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as u64;
    let mut next_seed = seed;
    let quest_override = quest_path.map(QuestDefinition::from_path).transpose()?;
    let campaign_path = original_us_campaign_path();
    let campaign_enabled = quest_override.is_none();
    let mut campaign = if campaign_enabled {
        Campaign::load_or_new(&campaign_path)?
    } else {
        Campaign::default()
    };
    let quest_definition = quest_override.clone().map_or_else(
        || QuestDefinition::original_us_game_system(campaign.next_quest_index()),
        Ok,
    )?;
    let mut game = Game::from_quest(quest_definition.clone(), seed)?;
    let mut startup = StartupFlow::default();
    if campaign_enabled {
        startup.selected_quest = campaign.next_quest_index();
        campaign.apply_names_to_setup(&mut startup);
        campaign.apply_to_game(&mut game);
    } else {
        startup.quest_ceiling = QuestDefinition::IMPLEMENTED_ORIGINAL_US_QUESTS - 1;
    }
    if let Some(stage) = std::env::var_os("HEROQUEST_START_STAGE") {
        startup.stage = match stage.to_string_lossy().as_ref() {
            "box" => StartupStage::Box,
            "armory" => StartupStage::Armory,
            "quest" => StartupStage::QuestSelection,
            "setup" => StartupStage::PlayerSetup,
            "wizard" => StartupStage::WizardSpellChoice,
            "elf" => StartupStage::ElfSpellChoice,
            "ready" => StartupStage::Ready,
            "board" => StartupStage::Playing,
            other => anyhow::bail!(
                "invalid HEROQUEST_START_STAGE={other:?}; expected box, armory, quest, setup, wizard, elf, ready, or board"
            ),
        };
        if startup.stage == StartupStage::Playing {
            game.apply_hero_setup(
                &startup.heroes,
                &startup.wizard_spells(),
                startup.elf_spells,
            );
        }
    }
    let startup_clock = Instant::now();

    let sdl = sdl3::init()?;
    let video = sdl.video()?;
    let dice_audio = match TabletopDiceAudio::new(&sdl) {
        Ok(audio) => Some(audio),
        Err(error) => {
            log::warn!("tabletop impact audio unavailable; continuing silently: {error}");
            None
        }
    };
    let mut window = video
        .window("HeroQuest 3D board", 1440, 900)
        .position_centered()
        .resizable()
        .hidden()
        .metal_view()
        .build()
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    window.set_title("HeroQuest - Original US Game System")?;
    let mut renderer = Renderer::new(&window)?;
    if startup.stage == StartupStage::Playing {
        renderer.focus_active_hero(&game);
    }
    video.text_input().start(&window);
    // Retain SDL's event-pump guard, but poll the raw queue so SDL 3.4 pinch
    // scale events are not discarded by the current high-level Rust wrapper.
    let _event_pump = sdl.event_pump()?;
    let mut dragging = false;
    let mut pointer_down: Option<(f32, f32)> = None;
    let mut pointer_board_pos = None;
    let mut native_pinch_active = false;
    let mut touch_zoom = TouchZoom::default();
    let mut dice_animation: Option<DiceAnimation> = None;
    let mut pending_attack_faces: Option<(AttackPlan, Vec<CombatFace>)> = None;
    let mut pending_hero_spell_attack_faces: Option<(HeroSpellRoll, Vec<CombatFace>)> = None;
    let mut zargon_next_step_at = Instant::now();
    let mut choosing_jump_direction = false;
    let mut spell_ui: Option<SpellUi> = None;
    let mut attack_ui: Option<AttackUi> = None;
    let mut door_ui: Option<DoorUi> = None;
    let mut potion_ui: Option<PotionUi> = None;
    let mut artifact_ui: Option<ArtifactUi> = None;
    let mut tabletop_ui: Option<TabletopUi> = None;
    let mut sheet_name_edit: Option<SheetNameEdit> = None;
    let mut defense_potion_prompt: Option<AttackPlan> = None;
    let mut share_ui: Option<ShareUi> = None;
    let mut planned_movement: Option<PlannedMovement> = None;
    let mut synthetic_key_events = VecDeque::<Keycode>::new();
    let mut possession_ui_index = 0usize;
    let mut retreat_ui = false;
    let mut campaign_committed_for_quest = false;
    let arrow_cursor = Cursor::from_system(SystemCursor::Arrow)?;
    let hand_cursor = Cursor::from_system(SystemCursor::Hand)?;
    let mut cursor_is_hand = false;

    if !window.show() {
        log::warn!("SDL could not show the initialized HeroQuest window");
    }
    window.raise();
    'running: loop {
        if startup.stage == StartupStage::Playing
            && game.phase == GamePhase::Won
            && campaign_enabled
            && !campaign_committed_for_quest
        {
            campaign_committed_for_quest = true;
            match campaign
                .record_success(startup.selected_quest, &game)
                .and_then(|()| campaign.save(&campaign_path))
            {
                Ok(()) => {
                    campaign.apply_names_to_setup(&mut startup);
                    game.notify(format!(
                        "Quest {} was recorded on the surviving character sheets. The campaign has been saved.",
                        startup.selected_quest + 1
                    ));
                }
                Err(error) => game.notify(format!(
                    "The quest is complete, but the campaign sheet could not be saved: {error}"
                )),
            }
        }
        if startup.stage == StartupStage::Playing
            && dice_animation.is_none()
            && let Some(plan) = game.take_pending_forced_attack()
        {
            renderer.focus_combat(plan.attacker, plan.defender);
            next_seed = next_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let attacker = game
                .unit(plan.attacker)
                .map(|unit| unit.name.clone())
                .unwrap_or_else(|| "The guardian".to_owned());
            let defender = game
                .unit(plan.defender)
                .map(|unit| unit.name.clone())
                .unwrap_or_else(|| "the searching Hero".to_owned());
            dice_animation = Some(DiceAnimation::new(
                DiceTray::combat(next_seed, plan.attack_dice),
                DicePurpose::AttackOffense(plan),
            ));
            renderer.focus_dice(&game, Some(plan.attacker));
            game.notify(format!(
                "{attacker}'s immediate attack strikes {defender}; physical combat dice are rolling…"
            ));
        }
        if startup.stage == StartupStage::Playing
            && dice_animation.is_none()
            && let Some(pending) = game.pending_trap_roll()
        {
            next_seed = next_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
            dice_animation = Some(DiceAnimation::new(
                DiceTray::combat(next_seed, pending.dice_count()),
                DicePurpose::Trap(pending),
            ));
            renderer.focus_dice(&game, Some(pending.hero));
            let name = game
                .unit(pending.hero)
                .map(|unit| unit.name.as_str())
                .unwrap_or("The Hero");
            game.notify(format!(
                "{name}'s trap roll is in the air: {} physical combat {}…",
                pending.dice_count(),
                if pending.dice_count() == 1 {
                    "die"
                } else {
                    "dice"
                }
            ));
        }
        if startup.stage == StartupStage::Playing
            && dice_animation.is_none()
            && let Some(hero) = game.pending_collapsing_ceiling_subject()
        {
            next_seed = next_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
            dice_animation = Some(DiceAnimation::new(
                DiceTray::movement_count(next_seed, 1),
                DicePurpose::CollapsingCeiling { hero },
            ));
            renderer.focus_dice(&game, Some(hero));
            let name = game
                .unit(hero)
                .map(|unit| unit.name.as_str())
                .unwrap_or("The Hero");
            game.notify(format!(
                "{name} entered an unstable ceiling square; one physical red die is rolling…"
            ));
        }
        if startup.stage == StartupStage::Playing
            && dice_animation.is_none()
            && let Some(subject) = game.pending_teleport_subject()
        {
            next_seed = next_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
            dice_animation = Some(DiceAnimation::new(
                DiceTray::movement(next_seed),
                DicePurpose::Teleport { subject },
            ));
            renderer.focus_dice(&game, Some(subject));
            let name = game
                .unit(subject)
                .map(|unit| unit.name.as_str())
                .unwrap_or("The displaced figure");
            game.notify(format!(
                "{name} is rolling two physical red dice for Ollar's magical doors…"
            ));
        }
        if startup.stage == StartupStage::Playing
            && dice_animation.is_none()
            && let Some((hero, spell, count)) = game.pending_hero_spell_resistance()
        {
            next_seed = next_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
            dice_animation = Some(DiceAnimation::new(
                DiceTray::movement_count(next_seed, count),
                DicePurpose::ChaosSpellRoll {
                    target: hero,
                    spell,
                },
            ));
            renderer.focus_dice(&game, Some(hero));
            game.notify(format!(
                "Rolling {count} physical Mind dice to break {spell:?}…"
            ));
        }
        while let Some(app_event) = synthetic_key_events
            .pop_front()
            .map(|keycode| {
                AppEvent::Sdl(Event::KeyDown {
                    timestamp: 0,
                    window_id: window.id(),
                    keycode: Some(keycode),
                    scancode: None,
                    keymod: Mod::empty(),
                    repeat: false,
                    which: 0,
                    raw: 0,
                })
            })
            .or_else(poll_app_event)
        {
            let event = match app_event {
                AppEvent::PinchBegin { window_id } if window_id == window.id() => {
                    native_pinch_active = true;
                    touch_zoom.clear();
                    continue;
                }
                AppEvent::PinchUpdate { window_id, scale } if window_id == window.id() => {
                    if let Some(delta) = pinch_scale_to_zoom_delta(scale) {
                        renderer.camera.zoom(delta);
                    }
                    continue;
                }
                AppEvent::PinchEnd { window_id } if window_id == window.id() => {
                    native_pinch_active = false;
                    continue;
                }
                AppEvent::PinchBegin { .. }
                | AppEvent::PinchUpdate { .. }
                | AppEvent::PinchEnd { .. } => continue,
                AppEvent::Sdl(event) => event,
            };
            match event {
                Event::Quit { .. } => break 'running,
                Event::Window {
                    window_id,
                    win_event:
                        WindowEvent::PixelSizeChanged(width, height)
                        | WindowEvent::Resized(width, height),
                    ..
                } if window_id == window.id() => renderer.resize(width as u32, height as u32),
                Event::TextInput { text, .. } if startup.stage == StartupStage::Playing => {
                    if let Some(edit) = &mut sheet_name_edit {
                        for character in text.chars().filter(|character| !character.is_control()) {
                            if edit.draft.chars().count() < 24 {
                                edit.draft.push(character);
                            }
                        }
                    }
                }
                Event::TextInput { text, .. } if startup.stage == StartupStage::PlayerSetup => {
                    let name = &mut startup.heroes[startup.active_hero].hero_name;
                    for character in text.chars().filter(|character| !character.is_control()) {
                        if name.chars().count() < 24 {
                            name.push(character);
                        }
                    }
                }
                Event::MouseMotion { x, y, .. } if startup.stage != StartupStage::Playing => {
                    let (width, height) = window.size();
                    startup.update_pointer(x, y, width, height);
                    let wants_hand = startup.hovered.is_some();
                    if wants_hand != cursor_is_hand {
                        if wants_hand {
                            hand_cursor.set();
                        } else {
                            arrow_cursor.set();
                        }
                        cursor_is_hand = wants_hand;
                    }
                }
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } if startup.stage != StartupStage::Playing => {
                    let (width, height) = window.size();
                    let clicked = startup.click_pointer(x, y, width, height);
                    if clicked == Some(heroquest::startup::StartupHotspot::ArmoryPurchase) {
                        purchase_selected_armory_item(&mut campaign, &mut startup, &campaign_path);
                    }
                    if startup.stage == StartupStage::Playing {
                        arrow_cursor.set();
                        cursor_is_hand = false;
                        next_seed = next_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
                        let selected_quest = quest_override.clone().map_or_else(
                            || QuestDefinition::original_us_game_system(startup.selected_quest),
                            Ok,
                        )?;
                        game = Game::from_quest(selected_quest, next_seed)?;
                        if campaign_enabled {
                            campaign.apply_to_game(&mut game);
                        }
                        game.apply_hero_setup(
                            &startup.heroes,
                            &startup.wizard_spells(),
                            startup.elf_spells,
                        );
                        game.notify(format!(
                            "Original-US setup complete. The computer assumes Zargon; selected campaign quest: {}.",
                            startup.quest_title()
                        ));
                        renderer.camera.reset();
                        renderer.set_selection_highlights(Vec::new());
                        renderer.set_hovered_move_target(None);
                        spell_ui = None;
                        attack_ui = None;
                        door_ui = None;
                        potion_ui = None;
                        artifact_ui = None;
                        tabletop_ui = None;
                        sheet_name_edit = None;
                        defense_potion_prompt = None;
                        share_ui = None;
                        planned_movement = None;
                        retreat_ui = false;
                        campaign_committed_for_quest = false;
                        renderer.focus_active_hero(&game);
                    }
                }
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } if startup.stage == StartupStage::Playing => {
                    pointer_down = Some((x, y));
                    let (width, height) = window.size();
                    pointer_board_pos = renderer.board_pos_at_screen(x, y, width, height);
                    dragging = false;
                }
                Event::MouseButtonUp {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } if startup.stage == StartupStage::Playing => {
                    let was_dragging = dragging;
                    dragging = false;
                    pointer_down = None;
                    let pressed_board_pos = pointer_board_pos.take();
                    let (width, height) = window.size();
                    if !was_dragging
                        && let Some(action) = renderer.hud_action_at_screen(x, y, width, height)
                    {
                        if let Some(key) = overlay_action_key(&action) {
                            synthetic_key_events.push_back(key);
                        }
                        arrow_cursor.set();
                        cursor_is_hand = false;
                        continue;
                    }
                    if !was_dragging
                        && !dice_click_locked(dice_animation.as_ref())
                        && !choosing_jump_direction
                        && !retreat_ui
                        && game.pending_hero_death.is_none()
                        && game.pending_possession_pickup.is_none()
                        && potion_ui.is_none()
                        && artifact_ui.is_none()
                        && attack_ui.is_none()
                        && spell_ui.is_none()
                        && door_ui.is_none()
                        && defense_potion_prompt.is_none()
                        && share_ui.is_none()
                        && planned_movement.is_none()
                        && game.phase != GamePhase::ZargonTurn
                        && let Some(surface) =
                            renderer.tabletop_surface_at_screen(&game, x, y, width, height)
                    {
                        tabletop_ui = Some(TabletopUi::new(surface));
                        renderer.set_selection_highlights(Vec::new());
                        arrow_cursor.set();
                        cursor_is_hand = false;
                        continue;
                    }
                    if !was_dragging
                        && !dice_click_locked(dice_animation.as_ref())
                        && !choosing_jump_direction
                        && !retreat_ui
                        && game.pending_hero_death.is_none()
                        && game.pending_possession_pickup.is_none()
                        && potion_ui.is_none()
                        && artifact_ui.is_none()
                        && defense_potion_prompt.is_none()
                        && share_ui.is_none()
                        && tabletop_ui.is_none()
                        && planned_movement.is_none()
                        && matches!(
                            game.phase,
                            GamePhase::HeroTurn { .. } | GamePhase::AllyTurn { .. }
                        )
                    {
                        let pointed_spell_target = match spell_ui {
                            Some(SpellUi::ChooseTarget { spell, .. }) => renderer
                                .hero_spell_target_at_screen(&game, spell, x, y, width, height),
                            _ => None,
                        };
                        let pointed_spell_pos =
                            pointed_spell_target.and_then(|target| match target {
                                HeroSpellTarget::Unit(id) => game.unit(id).map(|unit| unit.pos),
                                HeroSpellTarget::Door(index) => {
                                    game.doors.get(index).map(|door| door.a)
                                }
                            });
                        if let Some(pos) = pressed_board_pos
                            .or_else(|| renderer.board_pos_at_screen(x, y, width, height))
                            .or(pointed_spell_pos)
                        {
                            if let Some(selection) = attack_ui {
                                let options = game.active_attack_options().unwrap_or_default();
                                let preferred_source = options
                                    .get(selection.index % options.len().max(1))
                                    .map(|plan| plan.source);
                                let plan = options
                                    .iter()
                                    .copied()
                                    .find(|plan| {
                                        preferred_source == Some(plan.source)
                                            && game
                                                .unit(plan.defender)
                                                .is_some_and(|unit| unit.pos == pos)
                                    })
                                    .or_else(|| {
                                        options.iter().copied().find(|plan| {
                                            game.unit(plan.defender)
                                                .is_some_and(|unit| unit.pos == pos)
                                        })
                                    });
                                if let Some(plan) = plan {
                                    begin_selected_attack(
                                        &mut game,
                                        &mut renderer,
                                        plan,
                                        &mut dice_animation,
                                        &mut next_seed,
                                    );
                                    attack_ui = None;
                                    arrow_cursor.set();
                                    cursor_is_hand = false;
                                }
                            } else if let Some(SpellUi::ChooseTarget { spell, .. }) = spell_ui {
                                let target = pointed_spell_target
                                    .or_else(|| spell_target_at_board_pos(&game, spell, pos));
                                if let Some(target) = target
                                    && cast_selected_hero_spell(
                                        &mut game,
                                        &mut renderer,
                                        spell,
                                        target,
                                        &mut dice_animation,
                                        &mut next_seed,
                                    )
                                {
                                    spell_ui = None;
                                    arrow_cursor.set();
                                    cursor_is_hand = false;
                                }
                            } else if door_ui.is_some() {
                                if let Some(door_index) = adjacent_door_at_board_pos(&game, pos) {
                                    open_selected_door(&mut game, &mut renderer, door_index);
                                    door_ui = None;
                                    arrow_cursor.set();
                                    cursor_is_hand = false;
                                }
                            } else if spell_ui.is_none() && attack_ui.is_none() && door_ui.is_none()
                            {
                                let moved = if let Some(door_index) =
                                    adjacent_door_at_board_pos(&game, pos)
                                {
                                    if open_selected_door(&mut game, &mut renderer, door_index)
                                        && game.active_move_destinations().contains(&pos)
                                    {
                                        queue_planned_movement(
                                            &mut game,
                                            pos,
                                            &mut planned_movement,
                                        )
                                    } else if let Some(blocker) = game
                                        .units
                                        .iter()
                                        .find(|unit| unit.alive && !unit.escaped && unit.pos == pos)
                                    {
                                        game.notify(format!(
                                            "The door is open, but {} blocks the doorway. Attack before moving through.",
                                            blocker.name
                                        ));
                                        false
                                    } else {
                                        game.notify(
                                            "The door is open. Roll movement dice or choose an orange square to pass through.",
                                        );
                                        false
                                    }
                                } else {
                                    queue_planned_movement(&mut game, pos, &mut planned_movement)
                                };
                                if moved {
                                    arrow_cursor.set();
                                    cursor_is_hand = false;
                                }
                            }
                        }
                    }
                }
                Event::MouseMotion {
                    x, y, xrel, yrel, ..
                } if startup.stage == StartupStage::Playing && pointer_down.is_some() => {
                    let (start_x, start_y) = pointer_down.expect("guarded pointer press exists");
                    if !dragging && (x - start_x).hypot(y - start_y) >= 6.0 {
                        dragging = true;
                        pointer_board_pos = None;
                        if cursor_is_hand {
                            arrow_cursor.set();
                            cursor_is_hand = false;
                        }
                    }
                    if dragging {
                        renderer.set_hovered_move_target(None);
                        renderer.camera.orbit(xrel, yrel);
                    }
                }
                Event::MouseMotion { x, y, .. }
                    if startup.stage == StartupStage::Playing && pointer_down.is_none() =>
                {
                    let (width, height) = window.size();
                    let board_pos = renderer.board_pos_at_screen(x, y, width, height);
                    let pointed_spell_target = match spell_ui {
                        Some(SpellUi::ChooseTarget { spell, .. }) => {
                            renderer.hero_spell_target_at_screen(&game, spell, x, y, width, height)
                        }
                        _ => None,
                    };
                    let hovered_move_target = board_pos.filter(|target| {
                        !dice_click_locked(dice_animation.as_ref())
                            && !choosing_jump_direction
                            && !retreat_ui
                            && game.pending_hero_death.is_none()
                            && game.pending_possession_pickup.is_none()
                            && potion_ui.is_none()
                            && artifact_ui.is_none()
                            && attack_ui.is_none()
                            && spell_ui.is_none()
                            && door_ui.is_none()
                            && tabletop_ui.is_none()
                            && defense_potion_prompt.is_none()
                            && share_ui.is_none()
                            && planned_movement.is_none()
                            && game.active_move_destinations().contains(target)
                    });
                    renderer.set_hovered_move_target(hovered_move_target);
                    let tabletop_surface =
                        renderer.tabletop_surface_at_screen(&game, x, y, width, height);
                    let action_button = renderer
                        .hud_action_at_screen(x, y, width, height)
                        .and_then(|action| overlay_action_key(&action));
                    let wants_hand = action_button.is_some()
                        || pointed_spell_target.is_some()
                        || (tabletop_surface.is_some()
                            && !dice_click_locked(dice_animation.as_ref())
                            && !choosing_jump_direction
                            && !retreat_ui
                            && game.pending_hero_death.is_none()
                            && game.pending_possession_pickup.is_none()
                            && potion_ui.is_none()
                            && artifact_ui.is_none()
                            && attack_ui.is_none()
                            && spell_ui.is_none()
                            && door_ui.is_none()
                            && defense_potion_prompt.is_none()
                            && share_ui.is_none()
                            && planned_movement.is_none()
                            && game.phase != GamePhase::ZargonTurn)
                        || (!dice_click_locked(dice_animation.as_ref())
                            && !choosing_jump_direction
                            && !retreat_ui
                            && game.pending_hero_death.is_none()
                            && game.pending_possession_pickup.is_none()
                            && potion_ui.is_none()
                            && artifact_ui.is_none()
                            && tabletop_ui.is_none()
                            && defense_potion_prompt.is_none()
                            && share_ui.is_none()
                            && board_pos.is_some_and(|target| {
                                if attack_ui.is_some() {
                                    game.active_attack_options().is_ok_and(|options| {
                                        options.into_iter().any(|plan| {
                                            game.unit(plan.defender)
                                                .is_some_and(|unit| unit.pos == target)
                                        })
                                    })
                                } else if door_ui.is_some() {
                                    adjacent_door_at_board_pos(&game, target).is_some()
                                } else {
                                    match spell_ui {
                                        Some(SpellUi::ChooseTarget { spell, .. }) => {
                                            spell_target_at_board_pos(&game, spell, target)
                                                .is_some()
                                        }
                                        Some(SpellUi::ChooseSpell { .. }) => false,
                                        None => {
                                            game.active_move_destinations().contains(&target)
                                                || adjacent_door_at_board_pos(&game, target)
                                                    .is_some()
                                        }
                                    }
                                }
                            }));
                    if wants_hand != cursor_is_hand {
                        if wants_hand {
                            hand_cursor.set();
                        } else {
                            arrow_cursor.set();
                        }
                        cursor_is_hand = wants_hand;
                    }
                }
                Event::MouseWheel { .. } if startup.stage != StartupStage::Playing => {}
                Event::MouseWheel { y, .. } => renderer.camera.zoom(y),
                Event::FingerDown {
                    touch_id,
                    finger_id,
                    x,
                    y,
                    window_id,
                    ..
                } if window_id == window.id() && !native_pinch_active => {
                    touch_zoom.finger_down(touch_id, finger_id, x, y);
                }
                Event::FingerMotion {
                    touch_id,
                    finger_id,
                    x,
                    y,
                    window_id,
                    ..
                } if window_id == window.id() && !native_pinch_active => {
                    if let Some(delta) = touch_zoom.finger_motion(touch_id, finger_id, x, y) {
                        renderer.camera.zoom(delta);
                    }
                }
                Event::FingerUp {
                    touch_id,
                    finger_id,
                    ..
                } => touch_zoom.finger_up(touch_id, finger_id),
                Event::KeyDown {
                    keycode: Some(key),
                    repeat: false,
                    ..
                } => {
                    if startup.stage != StartupStage::Playing {
                        match key {
                            Keycode::Escape if startup.stage == StartupStage::Box => break 'running,
                            Keycode::Escape => startup.back(),
                            Keycode::Return => startup.advance(),
                            Keycode::P if startup.stage == StartupStage::Armory => {
                                purchase_selected_armory_item(
                                    &mut campaign,
                                    &mut startup,
                                    &campaign_path,
                                );
                            }
                            Keycode::Tab if startup.stage == StartupStage::Armory => {
                                startup.armory_hero = (startup.armory_hero + 1) % 4;
                            }
                            Keycode::Left => {
                                if startup.stage == StartupStage::PlayerSetup {
                                    startup.cycle_active_hero_player();
                                } else {
                                    startup.previous();
                                }
                            }
                            Keycode::Right => {
                                if startup.stage == StartupStage::PlayerSetup {
                                    startup.cycle_active_hero_player();
                                } else {
                                    startup.next();
                                }
                            }
                            Keycode::Up => startup.previous(),
                            Keycode::Down => startup.next(),
                            Keycode::F3 if startup.stage == StartupStage::PlayerSetup => {
                                startup.add_player()
                            }
                            Keycode::F2 if startup.stage == StartupStage::PlayerSetup => {
                                startup.remove_player()
                            }
                            Keycode::Q if startup.stage == StartupStage::PlayerSetup => {
                                startup.move_active_hero_earlier()
                            }
                            Keycode::E if startup.stage == StartupStage::PlayerSetup => {
                                startup.move_active_hero_later()
                            }
                            Keycode::Backspace if startup.stage == StartupStage::PlayerSetup => {
                                startup.heroes[startup.active_hero].hero_name.pop();
                            }
                            _ => {}
                        }
                        if startup.stage == StartupStage::Playing {
                            next_seed = next_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
                            let selected_quest = quest_override.clone().map_or_else(
                                || QuestDefinition::original_us_game_system(startup.selected_quest),
                                Ok,
                            )?;
                            game = Game::from_quest(selected_quest, next_seed)?;
                            if campaign_enabled {
                                campaign.apply_to_game(&mut game);
                            }
                            game.apply_hero_setup(
                                &startup.heroes,
                                &startup.wizard_spells(),
                                startup.elf_spells,
                            );
                            game.notify(format!(
                                "Original-US setup complete. The computer assumes Zargon; selected campaign quest: {}.",
                                startup.quest_title()
                            ));
                            renderer.camera.reset();
                            renderer.set_selection_highlights(Vec::new());
                            spell_ui = None;
                            attack_ui = None;
                            door_ui = None;
                            potion_ui = None;
                            artifact_ui = None;
                            tabletop_ui = None;
                            sheet_name_edit = None;
                            defense_potion_prompt = None;
                            share_ui = None;
                            planned_movement = None;
                            retreat_ui = false;
                            campaign_committed_for_quest = false;
                            renderer.focus_active_hero(&game);
                            zargon_next_step_at = Instant::now();
                        }
                        continue;
                    }
                    if let Some(edit) = sheet_name_edit.clone() {
                        match key {
                            Keycode::Escape => {
                                sheet_name_edit = None;
                                game.notify("Hero name edit cancelled.");
                            }
                            Keycode::Backspace => {
                                if let Some(edit) = &mut sheet_name_edit {
                                    edit.draft.pop();
                                }
                            }
                            Keycode::Return => match commit_character_sheet_name(
                                &mut game,
                                &mut campaign,
                                &mut startup,
                                &campaign_path,
                                campaign_enabled,
                                edit.hero,
                                &edit.draft,
                            ) {
                                Ok(name) => {
                                    sheet_name_edit = None;
                                    game.notify(format!("Character-sheet name saved as {name}."));
                                }
                                Err(error) => game.notify(error.to_string()),
                            },
                            _ => game.notify(
                                "Type a Hero name, then press Enter to save or Esc to cancel.",
                            ),
                        }
                        continue;
                    }
                    if let Some(selection) = tabletop_ui {
                        match key {
                            Keycode::Escape => {
                                tabletop_ui = None;
                                game.notify("Tabletop reference closed.");
                            }
                            Keycode::Left | Keycode::Up if selection.page_count() > 1 => {
                                tabletop_ui = Some(TabletopUi {
                                    page: (selection.page + selection.page_count() - 1)
                                        % selection.page_count(),
                                    ..selection
                                });
                            }
                            Keycode::Right | Keycode::Down if selection.page_count() > 1 => {
                                tabletop_ui = Some(TabletopUi {
                                    page: (selection.page + 1) % selection.page_count(),
                                    ..selection
                                });
                            }
                            Keycode::N
                                if matches!(
                                    selection.surface,
                                    TabletopSurface::CharacterSheet(_)
                                ) && selection.page % 5 == 0 =>
                            {
                                let TabletopSurface::CharacterSheet(hero) = selection.surface
                                else {
                                    unreachable!()
                                };
                                if let Some(unit) = game
                                    .units
                                    .iter()
                                    .find(|unit| unit.figure == FigureKind::Hero(hero))
                                {
                                    sheet_name_edit = Some(SheetNameEdit {
                                        hero,
                                        draft: unit.name.clone(),
                                    });
                                    game.notify(
                                        "Editing the name field on this physical character sheet.",
                                    );
                                }
                            }
                            Keycode::Return | Keycode::M => {
                                if let TabletopSurface::HeroSpellCard { hero, spell } =
                                    selection.surface
                                {
                                    let active_matches = game
                                        .active_hero()
                                        .is_some_and(|unit| unit.figure == FigureKind::Hero(hero));
                                    let castable =
                                        game.active_castable_hero_spells().contains(&spell);
                                    let targets = game.valid_hero_spell_targets(spell);
                                    if active_matches && castable {
                                        if let Some(&target) = targets.first() {
                                            tabletop_ui = None;
                                            spell_ui =
                                                Some(SpellUi::ChooseTarget { spell, index: 0 });
                                            renderer.set_selection_highlights(
                                                spell_target_positions(&game, spell),
                                            );
                                            focus_spell_target(&mut renderer, &game, target);
                                            game.notify(format!(
                                                "Choose a legal target for the physical {} card.",
                                                spell.name()
                                            ));
                                        } else {
                                            game.notify(format!(
                                                "{} currently has no legal target.",
                                                spell.name()
                                            ));
                                        }
                                    } else {
                                        game.notify(
                                            "Only the active Hero may cast one of that Hero's unused physical spell cards.",
                                        );
                                    }
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }
                    if game.phase == GamePhase::Won && key == Keycode::C && campaign_enabled {
                        startup.stage = StartupStage::Armory;
                        startup.selected_quest = campaign.next_quest_index();
                        campaign.apply_names_to_setup(&mut startup);
                        startup.armory_message =
                            "Choose a Hero and an item from the original-US Armory.".to_owned();
                        startup.armory_revision = startup.armory_revision.wrapping_add(1);
                        renderer.set_selection_highlights(Vec::new());
                        arrow_cursor.set();
                        cursor_is_hand = false;
                        continue;
                    }
                    if matches!(game.phase, GamePhase::Retreated | GamePhase::Lost)
                        && key == Keycode::C
                        && campaign_enabled
                    {
                        startup.stage = StartupStage::QuestSelection;
                        startup.selected_quest = campaign.next_quest_index();
                        campaign.apply_names_to_setup(&mut startup);
                        renderer.set_selection_highlights(Vec::new());
                        arrow_cursor.set();
                        cursor_is_hand = false;
                        continue;
                    }
                    if retreat_ui {
                        match key {
                            Keycode::Return | Keycode::N => {
                                match game.voluntarily_retreat() {
                                    Ok(()) => {
                                        retreat_ui = false;
                                        renderer.set_selection_highlights(Vec::new());
                                    }
                                    Err(error) => game.notify(error.to_string()),
                                }
                            }
                            Keycode::Escape => {
                                retreat_ui = false;
                                game.notify("Voluntary retreat cancelled.");
                            }
                            _ => game.notify(
                                "Press Enter to end this quest without completion or Esc to continue playing.",
                            ),
                        }
                        continue;
                    }
                    if let Some(pending) = game.pending_hero_death {
                        if pending.potion_roll_pending || dice_animation.is_some() {
                            game.notify("Wait for the death-saving red die to settle.");
                            continue;
                        }
                        let choice = match key {
                            Keycode::C => Some(HeroDeathChoice::HealingPotion),
                            Keycode::H => Some(HeroDeathChoice::HealBody),
                            Keycode::W => Some(HeroDeathChoice::WaterOfHealing),
                            Keycode::E | Keycode::Backspace => Some(HeroDeathChoice::AcceptDeath),
                            _ => None,
                        };
                        if let Some(choice) = choice {
                            match game.choose_pending_hero_death(choice) {
                                Ok(Some(HealingPotionUse::RollRedDie { hero })) => {
                                    next_seed = next_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
                                    dice_animation = Some(DiceAnimation::new(
                                        DiceTray::movement_count(next_seed, 1),
                                        DicePurpose::HealingPotion { hero },
                                    ));
                                    renderer.focus_dice(&game, Some(hero));
                                }
                                Ok(Some(HealingPotionUse::Restored { hero, .. })) => {
                                    renderer.focus_unit(hero);
                                }
                                Ok(None) => {
                                    if let Some(pickup) = &game.pending_possession_pickup {
                                        possession_ui_index = 0;
                                        renderer.focus_unit(pickup.dead_hero);
                                    } else {
                                        renderer.focus_active_hero(&game);
                                    }
                                }
                                Err(error) => game.notify(error.to_string()),
                            }
                        }
                        continue;
                    }
                    if let Some(pickup) = game.pending_possession_pickup.clone() {
                        if pickup.eligible_heroes.is_empty() {
                            game.notify("No Hero can receive the fallen Hero's possessions.");
                            continue;
                        }
                        match key {
                            Keycode::Left | Keycode::Up => {
                                possession_ui_index =
                                    (possession_ui_index + pickup.eligible_heroes.len() - 1)
                                        % pickup.eligible_heroes.len();
                                renderer.focus_unit(pickup.eligible_heroes[possession_ui_index]);
                            }
                            Keycode::Right | Keycode::Down => {
                                possession_ui_index =
                                    (possession_ui_index + 1) % pickup.eligible_heroes.len();
                                renderer.focus_unit(pickup.eligible_heroes[possession_ui_index]);
                            }
                            Keycode::Return => {
                                let recipient = pickup.eligible_heroes
                                    [possession_ui_index % pickup.eligible_heroes.len()];
                                match game.choose_possession_recipient(recipient) {
                                    Ok(()) => renderer.focus_unit(recipient),
                                    Err(error) => game.notify(error.to_string()),
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }
                    if let Some(plan) = defense_potion_prompt {
                        match key {
                            Keycode::D => match game.drink_potion_of_defense_for(plan.defender) {
                                Ok(()) => {
                                    let refreshed = game
                                        .refresh_attack_defense_dice(plan)
                                        .unwrap_or(plan);
                                    begin_defense_dice(
                                        &mut game,
                                        &mut renderer,
                                        refreshed,
                                        &mut pending_attack_faces,
                                        &mut dice_animation,
                                        &mut next_seed,
                                    );
                                    defense_potion_prompt = None;
                                }
                                Err(error) => game.notify(error.to_string()),
                            },
                            Keycode::Return | Keycode::E => {
                                begin_defense_dice(
                                    &mut game,
                                    &mut renderer,
                                    plan,
                                    &mut pending_attack_faces,
                                    &mut dice_animation,
                                    &mut next_seed,
                                );
                                defense_potion_prompt = None;
                            }
                            _ => game.notify(
                                "Press D to drink the Potion of Defense, or Enter to roll normally.",
                            ),
                        }
                        continue;
                    }
                    if let Some(selection) = share_ui {
                        let recipients = game.living_other_heroes();
                        if recipients.is_empty() {
                            share_ui = None;
                            game.notify("There is no other living Hero to receive a gift.");
                            continue;
                        }
                        match selection {
                            ShareUi::Potion { potion, index } => match key {
                                Keycode::Escape => {
                                    share_ui = None;
                                    game.notify("Potion gift cancelled.");
                                }
                                Keycode::Left | Keycode::Up => {
                                    share_ui = Some(ShareUi::Potion {
                                        potion,
                                        index: (index + recipients.len() - 1) % recipients.len(),
                                    });
                                }
                                Keycode::Right | Keycode::Down => {
                                    share_ui = Some(ShareUi::Potion {
                                        potion,
                                        index: (index + 1) % recipients.len(),
                                    });
                                }
                                Keycode::Return => {
                                    let recipient = recipients[index % recipients.len()];
                                    match game.give_active_potion(recipient, potion) {
                                        Ok(()) => {
                                            share_ui = None;
                                            renderer.focus_unit(recipient);
                                        }
                                        Err(error) => game.notify(error.to_string()),
                                    }
                                }
                                _ => {}
                            },
                            ShareUi::Gold { index, amount } => {
                                let available =
                                    game.active_hero().map_or(0, |hero| hero.inventory.gold);
                                let amount = amount.clamp(1, available.max(1));
                                match key {
                                    Keycode::Escape => {
                                        share_ui = None;
                                        game.notify("Gold sharing cancelled.");
                                    }
                                    Keycode::Left => {
                                        share_ui = Some(ShareUi::Gold {
                                            index: (index + recipients.len() - 1)
                                                % recipients.len(),
                                            amount,
                                        });
                                    }
                                    Keycode::Right => {
                                        share_ui = Some(ShareUi::Gold {
                                            index: (index + 1) % recipients.len(),
                                            amount,
                                        });
                                    }
                                    Keycode::Up => {
                                        share_ui = Some(ShareUi::Gold {
                                            index,
                                            amount: amount.saturating_add(10).min(available),
                                        });
                                    }
                                    Keycode::Down => {
                                        share_ui = Some(ShareUi::Gold {
                                            index,
                                            amount: amount.saturating_sub(10).max(1),
                                        });
                                    }
                                    Keycode::W => {
                                        share_ui = Some(ShareUi::Gold {
                                            index,
                                            amount: amount.saturating_add(1).min(available),
                                        });
                                    }
                                    Keycode::S => {
                                        share_ui = Some(ShareUi::Gold {
                                            index,
                                            amount: amount.saturating_sub(1).max(1),
                                        });
                                    }
                                    Keycode::G => {
                                        share_ui = Some(ShareUi::Gold {
                                            index,
                                            amount: available,
                                        });
                                    }
                                    Keycode::Return => {
                                        let recipient = recipients[index % recipients.len()];
                                        match game.give_active_gold(recipient, amount) {
                                            Ok(()) => {
                                                share_ui = None;
                                                renderer.focus_unit(recipient);
                                            }
                                            Err(error) => game.notify(error.to_string()),
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        continue;
                    }
                    if let Some(selection) = potion_ui {
                        let choices = available_active_potions(&game);
                        if choices.is_empty() {
                            potion_ui = None;
                            game.notify("The active Hero has no currently usable potion.");
                            continue;
                        }
                        match key {
                            Keycode::Escape => {
                                potion_ui = None;
                                game.notify("Potion selection cancelled.");
                            }
                            Keycode::Left | Keycode::Up => {
                                potion_ui = Some(PotionUi {
                                    index: (selection.index + choices.len() - 1) % choices.len(),
                                });
                            }
                            Keycode::Right | Keycode::Down => {
                                potion_ui = Some(PotionUi {
                                    index: (selection.index + 1) % choices.len(),
                                });
                            }
                            Keycode::Return | Keycode::I => {
                                let choice = choices[selection.index % choices.len()];
                                if activate_selected_potion(
                                    &mut game,
                                    &mut renderer,
                                    choice,
                                    &mut dice_animation,
                                    &mut next_seed,
                                ) {
                                    potion_ui = None;
                                }
                            }
                            Keycode::G if !game.living_other_heroes().is_empty() => {
                                let choice = choices[selection.index % choices.len()];
                                share_ui = Some(ShareUi::Potion {
                                    potion: choice.kind(),
                                    index: 0,
                                });
                                potion_ui = None;
                            }
                            _ => {}
                        }
                        continue;
                    }
                    if let Some(selection) = artifact_ui {
                        match selection {
                            ArtifactUi::ChooseArtifact { index } => {
                                let choices = available_active_artifacts(&game);
                                if choices.is_empty() {
                                    artifact_ui = None;
                                    game.notify(
                                        "The active Hero has no currently usable artifact.",
                                    );
                                    continue;
                                }
                                match key {
                                    Keycode::Escape => {
                                        artifact_ui = None;
                                        game.notify("Artifact selection cancelled.");
                                    }
                                    Keycode::Left | Keycode::Up => {
                                        artifact_ui = Some(ArtifactUi::ChooseArtifact {
                                            index: (index + choices.len() - 1) % choices.len(),
                                        });
                                    }
                                    Keycode::Right | Keycode::Down => {
                                        artifact_ui = Some(ArtifactUi::ChooseArtifact {
                                            index: (index + 1) % choices.len(),
                                        });
                                    }
                                    Keycode::Return | Keycode::Y => {
                                        match choices[index % choices.len()] {
                                            ArtifactAction::ElixirOfLife => {
                                                let targets = game.elixir_of_life_targets();
                                                if let Some(&target) = targets.first() {
                                                    artifact_ui =
                                                        Some(ArtifactUi::ChooseElixirTarget {
                                                            index: 0,
                                                        });
                                                    renderer.focus_unit(target);
                                                }
                                            }
                                            ArtifactAction::RingOfReturn => {
                                                match game.use_ring_of_return() {
                                                    Ok(_) => {
                                                        artifact_ui = None;
                                                        renderer.focus_active_hero(&game);
                                                    }
                                                    Err(error) => game.notify(error.to_string()),
                                                }
                                            }
                                            ArtifactAction::DeclareSpellRing => {
                                                if game.spell_ring_storable_spells().is_empty() {
                                                    game.notify(
                                                        "No unused spell can be stored in the Spell Ring.",
                                                    );
                                                } else {
                                                    artifact_ui =
                                                        Some(ArtifactUi::ChooseSpellRingSpell {
                                                            index: 0,
                                                        });
                                                }
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            ArtifactUi::ChooseElixirTarget { index } => {
                                let targets = game.elixir_of_life_targets();
                                if targets.is_empty() {
                                    artifact_ui = None;
                                    game.notify("There is no dead Hero for the Elixir to revive.");
                                    continue;
                                }
                                match key {
                                    Keycode::Escape => {
                                        artifact_ui = None;
                                        renderer.focus_active_hero(&game);
                                    }
                                    Keycode::Backspace => {
                                        artifact_ui = Some(ArtifactUi::ChooseArtifact { index: 0 });
                                        renderer.focus_active_hero(&game);
                                    }
                                    Keycode::Left | Keycode::Up => {
                                        let index = (index + targets.len() - 1) % targets.len();
                                        artifact_ui =
                                            Some(ArtifactUi::ChooseElixirTarget { index });
                                        renderer.focus_unit(targets[index]);
                                    }
                                    Keycode::Right | Keycode::Down => {
                                        let index = (index + 1) % targets.len();
                                        artifact_ui =
                                            Some(ArtifactUi::ChooseElixirTarget { index });
                                        renderer.focus_unit(targets[index]);
                                    }
                                    Keycode::Return | Keycode::Y => {
                                        let target = targets[index % targets.len()];
                                        match game.use_elixir_of_life(target) {
                                            Ok(_) => {
                                                artifact_ui = None;
                                                renderer.focus_unit(target);
                                            }
                                            Err(error) => game.notify(error.to_string()),
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            ArtifactUi::ChooseSpellRingSpell { index } => {
                                let spells = game.spell_ring_storable_spells();
                                if spells.is_empty() {
                                    artifact_ui = None;
                                    game.notify("No unused spell can be stored in the Spell Ring.");
                                    continue;
                                }
                                match key {
                                    Keycode::Escape => artifact_ui = None,
                                    Keycode::Backspace => {
                                        artifact_ui = Some(ArtifactUi::ChooseArtifact { index: 0 });
                                    }
                                    Keycode::Left | Keycode::Up => {
                                        artifact_ui = Some(ArtifactUi::ChooseSpellRingSpell {
                                            index: (index + spells.len() - 1) % spells.len(),
                                        });
                                    }
                                    Keycode::Right | Keycode::Down => {
                                        artifact_ui = Some(ArtifactUi::ChooseSpellRingSpell {
                                            index: (index + 1) % spells.len(),
                                        });
                                    }
                                    Keycode::Return | Keycode::Y => {
                                        let spell = spells[index % spells.len()];
                                        match game.store_active_spell_in_ring(spell) {
                                            Ok(()) => artifact_ui = None,
                                            Err(error) => game.notify(error.to_string()),
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        continue;
                    }
                    if let Some(selection) = attack_ui {
                        let options = game.active_attack_options().unwrap_or_default();
                        if options.is_empty() {
                            attack_ui = None;
                            renderer.set_selection_highlights(Vec::new());
                            game.notify("No legal attack remains.");
                            continue;
                        }
                        match key {
                            Keycode::Escape => {
                                attack_ui = None;
                                renderer.set_selection_highlights(Vec::new());
                                renderer.focus_active_hero(&game);
                                game.notify("Attack cancelled.");
                            }
                            Keycode::Left | Keycode::Up => {
                                let index = (selection.index + options.len() - 1) % options.len();
                                attack_ui = Some(AttackUi { index });
                                let plan = options[index];
                                renderer.focus_combat(plan.attacker, plan.defender);
                            }
                            Keycode::Right | Keycode::Down => {
                                let index = (selection.index + 1) % options.len();
                                attack_ui = Some(AttackUi { index });
                                let plan = options[index];
                                renderer.focus_combat(plan.attacker, plan.defender);
                            }
                            Keycode::Return | Keycode::F => {
                                let plan = options[selection.index % options.len()];
                                begin_selected_attack(
                                    &mut game,
                                    &mut renderer,
                                    plan,
                                    &mut dice_animation,
                                    &mut next_seed,
                                );
                                attack_ui = None;
                            }
                            _ => {}
                        }
                        continue;
                    }
                    if let Some(selection) = spell_ui {
                        match selection {
                            SpellUi::ChooseSpell { index } => {
                                let spells = game.active_castable_hero_spells();
                                if spells.is_empty() {
                                    spell_ui = None;
                                    renderer.set_selection_highlights(Vec::new());
                                    game.notify("The active Hero has no unused spell cards.");
                                    continue;
                                }
                                match key {
                                    Keycode::Escape => {
                                        spell_ui = None;
                                        renderer.set_selection_highlights(Vec::new());
                                        renderer.focus_active_hero(&game);
                                        game.notify("Spell casting cancelled.");
                                    }
                                    Keycode::Left | Keycode::Up => {
                                        spell_ui = Some(SpellUi::ChooseSpell {
                                            index: (index + spells.len() - 1) % spells.len(),
                                        });
                                    }
                                    Keycode::Right | Keycode::Down => {
                                        spell_ui = Some(SpellUi::ChooseSpell {
                                            index: (index + 1) % spells.len(),
                                        });
                                    }
                                    Keycode::Return | Keycode::M => {
                                        let spell = spells[index % spells.len()];
                                        let targets = game.valid_hero_spell_targets(spell);
                                        if let Some(&target) = targets.first() {
                                            spell_ui =
                                                Some(SpellUi::ChooseTarget { spell, index: 0 });
                                            renderer.set_selection_highlights(
                                                spell_target_positions(&game, spell),
                                            );
                                            focus_spell_target(&mut renderer, &game, target);
                                        } else {
                                            game.notify(format!(
                                                "{} currently has no legal target.",
                                                spell.name()
                                            ));
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            SpellUi::ChooseTarget { spell, index } => {
                                let targets = game.valid_hero_spell_targets(spell);
                                if targets.is_empty() {
                                    spell_ui = Some(SpellUi::ChooseSpell { index: 0 });
                                    renderer.set_selection_highlights(Vec::new());
                                    game.notify(format!(
                                        "{} no longer has a legal target.",
                                        spell.name()
                                    ));
                                    continue;
                                }
                                match key {
                                    Keycode::Escape => {
                                        spell_ui = None;
                                        renderer.set_selection_highlights(Vec::new());
                                        renderer.focus_active_hero(&game);
                                        game.notify("Spell casting cancelled.");
                                    }
                                    Keycode::Backspace => {
                                        let spell_index = game
                                            .active_castable_hero_spells()
                                            .iter()
                                            .position(|&card| card == spell)
                                            .unwrap_or(0);
                                        spell_ui =
                                            Some(SpellUi::ChooseSpell { index: spell_index });
                                        renderer.set_selection_highlights(Vec::new());
                                    }
                                    Keycode::Left | Keycode::Up => {
                                        let index = (index + targets.len() - 1) % targets.len();
                                        spell_ui = Some(SpellUi::ChooseTarget { spell, index });
                                        focus_spell_target(&mut renderer, &game, targets[index]);
                                    }
                                    Keycode::Right | Keycode::Down => {
                                        let index = (index + 1) % targets.len();
                                        spell_ui = Some(SpellUi::ChooseTarget { spell, index });
                                        focus_spell_target(&mut renderer, &game, targets[index]);
                                    }
                                    Keycode::Return | Keycode::M => {
                                        let target = targets[index % targets.len()];
                                        if cast_selected_hero_spell(
                                            &mut game,
                                            &mut renderer,
                                            spell,
                                            target,
                                            &mut dice_animation,
                                            &mut next_seed,
                                        ) {
                                            spell_ui = None;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        continue;
                    }
                    if let Some(selection) = door_ui {
                        let choices = game.adjacent_closed_door_indices().unwrap_or_default();
                        if choices.is_empty() {
                            door_ui = None;
                            renderer.set_selection_highlights(Vec::new());
                            game.notify("No adjacent closed door remains.");
                            continue;
                        }
                        match key {
                            Keycode::Escape => {
                                door_ui = None;
                                renderer.set_selection_highlights(Vec::new());
                                renderer.focus_active_hero(&game);
                                game.notify("Door selection cancelled.");
                            }
                            Keycode::Left | Keycode::Up => {
                                let index = (selection.index + choices.len() - 1) % choices.len();
                                door_ui = Some(DoorUi { index });
                                focus_door(&mut renderer, &game, choices[index]);
                            }
                            Keycode::Right | Keycode::Down => {
                                let index = (selection.index + 1) % choices.len();
                                door_ui = Some(DoorUi { index });
                                focus_door(&mut renderer, &game, choices[index]);
                            }
                            Keycode::Return | Keycode::O => {
                                let door_index = choices[selection.index % choices.len()];
                                open_selected_door(&mut game, &mut renderer, door_index);
                                door_ui = None;
                            }
                            _ => {}
                        }
                        continue;
                    }
                    if planned_movement.is_some() {
                        if key == Keycode::Escape {
                            planned_movement = None;
                            renderer.focus_active_hero(&game);
                            game.notify("Planned movement cancelled.");
                        } else {
                            game.notify(
                                "The Hero is following the selected route; press Esc to stop.",
                            );
                        }
                        continue;
                    }
                    if key == Keycode::Escape {
                        break 'running;
                    }
                    if key == Keycode::H {
                        renderer.camera.reset();
                        renderer.focus_active_hero(&game);
                        continue;
                    }
                    if dice_animation.is_some() {
                        game.notify("Wait for the physical dice to settle.");
                        continue;
                    }
                    if game.pending_collapsing_ceiling_subject().is_some() {
                        game.notify("Wait for the collapsing-ceiling red die to settle.");
                        continue;
                    }
                    if game.phase == GamePhase::ZargonTurn {
                        game.notify("Zargon is moving the monsters; wait for the physical dice.");
                        continue;
                    }
                    if choosing_jump_direction {
                        let screen_direction = match key {
                            Keycode::Up | Keycode::W => Some(Direction::North),
                            Keycode::Right | Keycode::D => Some(Direction::East),
                            Keycode::Down | Keycode::S => Some(Direction::South),
                            Keycode::Left | Keycode::A => Some(Direction::West),
                            _ => None,
                        };
                        choosing_jump_direction = false;
                        if let Some(screen_direction) = screen_direction {
                            let direction = renderer.camera_relative_direction(screen_direction);
                            match game.active_jump_plan(direction) {
                                Ok(plan) => {
                                    next_seed = next_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
                                    dice_animation = Some(DiceAnimation::new(
                                        DiceTray::combat(next_seed, 1),
                                        DicePurpose::Jump(plan),
                                    ));
                                    renderer.focus_dice(&game, Some(plan.hero));
                                    game.notify(
                                        "One combat die thrown at the Hero's station for the jump…",
                                    );
                                }
                                Err(error) => game.notify(error.to_string()),
                            }
                        } else {
                            game.notify("Jump cancelled; press J and then a movement direction.");
                        }
                        continue;
                    }
                    match key {
                        Keycode::R => {
                            let roller = game.active_mover_id();
                            next_seed = next_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
                            dice_animation = Some(DiceAnimation::new(
                                DiceTray::movement_count(
                                    next_seed,
                                    game.active_movement_dice_count(),
                                ),
                                DicePurpose::Movement,
                            ));
                            renderer.focus_dice(&game, roller);
                            game.notify("Movement dice thrown on the active player's tabletop…");
                        }
                        Keycode::F => match game.active_attack_options() {
                            Ok(options) => {
                                let plan = options[0];
                                attack_ui = Some(AttackUi { index: 0 });
                                renderer.set_selection_highlights(attack_target_positions(&game));
                                renderer.focus_combat(plan.attacker, plan.defender);
                                game.notify(
                                    "Choose the target and weapon, then press Enter or click a cyan target square.",
                                );
                            }
                            Err(error) => game.notify(error.to_string()),
                        },
                        Keycode::O => match game.adjacent_closed_door_indices() {
                            Ok(choices) => {
                                door_ui = Some(DoorUi { index: 0 });
                                renderer.set_selection_highlights(adjacent_door_positions(&game));
                                focus_door(&mut renderer, &game, choices[0]);
                                game.notify(
                                    "Choose the adjacent door, then press Enter or click its cyan far-side square.",
                                );
                            }
                            Err(error) => game.notify(error.to_string()),
                        },
                        Keycode::T => {
                            if let Err(error) = game.search_treasure() {
                                game.notify(error.to_string());
                            }
                        }
                        Keycode::C => match game.begin_active_healing_potion() {
                            Ok(HealingPotionUse::Restored { .. }) => {}
                            Ok(HealingPotionUse::RollRedDie { hero }) => {
                                next_seed = next_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
                                dice_animation = Some(DiceAnimation::new(
                                    DiceTray::movement_count(next_seed, 1),
                                    DicePurpose::HealingPotion { hero },
                                ));
                                renderer.focus_dice(&game, Some(hero));
                            }
                            Err(error) => game.notify(error.to_string()),
                        },
                        Keycode::I => {
                            let choices = available_active_potions(&game);
                            if choices.is_empty() {
                                game.notify("The active Hero has no currently usable potion.");
                            } else {
                                potion_ui = Some(PotionUi { index: 0 });
                                game.notify(
                                    "Choose a potion with the arrow keys, then press Enter.",
                                );
                            }
                        }
                        Keycode::Y => {
                            let choices = available_active_artifacts(&game);
                            if choices.is_empty() {
                                game.notify("The active Hero has no currently usable artifact.");
                            } else {
                                artifact_ui = Some(ArtifactUi::ChooseArtifact { index: 0 });
                                game.notify(
                                    "Choose an artifact with the arrow keys, then press Enter.",
                                );
                            }
                        }
                        Keycode::Q => {
                            let gold = game.active_hero().map_or(0, |hero| hero.inventory.gold);
                            if gold == 0 {
                                game.notify("The active Hero has no gold to share.");
                            } else if game.living_other_heroes().is_empty() {
                                game.notify("There is no other living Hero to receive gold.");
                            } else {
                                share_ui = Some(ShareUi::Gold {
                                    index: 0,
                                    amount: gold.min(10).max(1),
                                });
                            }
                        }
                        Keycode::N => {
                            if game.can_voluntarily_retreat() {
                                retreat_ui = true;
                            } else {
                                game.notify(
                                    "Every surviving Hero must return to the stairway before ending the quest.",
                                );
                            }
                        }
                        Keycode::B => {
                            if let Err(error) = game.use_petrification_potion() {
                                game.notify(error.to_string());
                            }
                        }
                        Keycode::K => {
                            if game.can_take_fools_gold() {
                                if let Err(error) = game.take_fools_gold() {
                                    game.notify(error.to_string());
                                }
                            } else if let Err(error) = game.take_quest_item() {
                                game.notify(error.to_string());
                            }
                        }
                        Keycode::G => {
                            if game
                                .active_hero()
                                .is_some_and(|hero| hero.inventory.fools_gold > 0)
                            {
                                if let Err(error) = game.drop_fools_gold() {
                                    game.notify(error.to_string());
                                }
                            } else if let Err(error) = game.drop_quest_item() {
                                game.notify(error.to_string());
                            }
                        }
                        Keycode::V => {
                            if let Err(error) = game.transfer_quest_item_to_adjacent_hero() {
                                game.notify(error.to_string());
                            }
                        }
                        Keycode::U => {
                            if let Err(error) = game.use_ring_of_return() {
                                game.notify(error.to_string());
                            }
                        }
                        Keycode::L => {
                            if let Err(error) = game.search_secret_doors() {
                                game.notify(error.to_string());
                            }
                        }
                        Keycode::P => {
                            if let Err(error) = game.search_traps() {
                                game.notify(error.to_string());
                            }
                        }
                        Keycode::X => match game.active_disarm_plan() {
                            Ok(plan) => {
                                next_seed = next_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
                                dice_animation = Some(DiceAnimation::new(
                                    DiceTray::combat(next_seed, 1),
                                    DicePurpose::Disarm(plan),
                                ));
                                renderer.focus_dice(&game, Some(plan.hero));
                                game.notify("One combat die thrown at the Hero's station to disarm the trap…");
                            }
                            Err(error) => game.notify(error.to_string()),
                        },
                        Keycode::J => {
                            choosing_jump_direction = true;
                            game.notify("Choose a direction to jump across a trap.");
                        }
                        Keycode::M => {
                            if game.active_castable_hero_spells().is_empty() {
                                game.notify(
                                    "The active Hero has no spell card that can be cast now.",
                                );
                            } else {
                                spell_ui = Some(SpellUi::ChooseSpell { index: 0 });
                                renderer.set_selection_highlights(Vec::new());
                                game.notify(
                                    "Choose a spell card with the arrow keys, then press Enter.",
                                );
                            }
                        }
                        Keycode::E | Keycode::Return => {
                            if let Err(error) = game.end_hero_turn() {
                                game.notify(error.to_string());
                            } else {
                                renderer.focus_active_hero(&game);
                                if game.phase == GamePhase::ZargonTurn {
                                    zargon_next_step_at =
                                        Instant::now() + Duration::from_millis(250);
                                }
                            }
                        }
                        Keycode::Up | Keycode::W => {
                            let direction = renderer.camera_relative_direction(Direction::North);
                            move_hero(&mut game, &mut renderer, direction)
                        }
                        Keycode::Right | Keycode::D => {
                            let direction = renderer.camera_relative_direction(Direction::East);
                            move_hero(&mut game, &mut renderer, direction)
                        }
                        Keycode::Down | Keycode::S => {
                            let direction = renderer.camera_relative_direction(Direction::South);
                            move_hero(&mut game, &mut renderer, direction)
                        }
                        Keycode::Left | Keycode::A => {
                            let direction = renderer.camera_relative_direction(Direction::West);
                            move_hero(&mut game, &mut renderer, direction)
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        if startup.stage == StartupStage::Playing
            && dice_animation.is_none()
            && planned_movement
                .as_ref()
                .is_some_and(|route| Instant::now() >= route.next_step_at)
        {
            let mut route = planned_movement.take().expect("guarded route exists");
            let route_is_current = game.active_mover_id() == Some(route.mover)
                && matches!(
                    game.phase,
                    GamePhase::HeroTurn { .. } | GamePhase::AllyTurn { .. }
                );
            if !route_is_current {
                game.notify("The selected movement route was interrupted.");
            } else if let Some(direction) = route.steps.pop_front() {
                match game.move_active(direction) {
                    Ok(_) => renderer.focus_unit(route.mover),
                    Err(error) => {
                        route.steps.clear();
                        game.notify(format!("The selected route stopped: {error}"));
                    }
                }
                route.next_step_at = Instant::now() + Duration::from_millis(330);
                let interrupted = game.active_mover_id() != Some(route.mover)
                    || game.hero_turn.movement_left == 0
                    || game.pending_trap_roll().is_some()
                    || game.pending_falling_block.is_some()
                    || game.pending_collapsing_ceiling_subject().is_some()
                    || game.pending_teleport_subject().is_some()
                    || game.pending_hero_death.is_some();
                if !route.steps.is_empty() && !interrupted {
                    planned_movement = Some(route);
                } else if route.steps.is_empty() && !interrupted {
                    game.notify(format!(
                        "Destination {},{} reached; {} movement point{} remain.",
                        route.destination.x + 1,
                        route.destination.y + 1,
                        game.hero_turn.movement_left,
                        if game.hero_turn.movement_left == 1 {
                            ""
                        } else {
                            "s"
                        }
                    ));
                }
            }
        }

        if startup.stage == StartupStage::Playing
            && let Some(animation) = &mut dice_animation
        {
            let now = Instant::now();
            animation.advance_physics(now);
            for strength in animation.tray.drain_impacts() {
                if let Some(audio) = &dice_audio {
                    audio.play_impact(strength);
                }
            }
            if animation.tray.is_finished() && animation.result_applied_at.is_none() {
                let results = animation
                    .tray
                    .results()
                    .expect("finished dice have results");
                match animation.purpose {
                    DicePurpose::Movement => {
                        let values: Vec<_> = results
                            .iter()
                            .filter_map(|result| match result {
                                DieResult::Movement(face) => Some(*face),
                                _ => None,
                            })
                            .collect();
                        if let Err(error) = game.apply_movement_roll(&values) {
                            game.notify(error.to_string());
                        }
                    }
                    DicePurpose::AttackOffense(plan) => {
                        let faces: Vec<CombatFace> = results
                            .iter()
                            .filter_map(|result| match result {
                                DieResult::Combat(face) => Some(*face),
                                _ => None,
                            })
                            .collect();
                        if plan.defend_dice == 0 {
                            if let Err(error) = game.resolve_attack(plan, &faces, &[]) {
                                game.notify(error.to_string());
                            }
                        } else {
                            pending_attack_faces = Some((plan, faces));
                            let defender = game
                                .unit(plan.defender)
                                .map(|unit| unit.name.as_str())
                                .unwrap_or("The defender");
                            game.notify(format!(
                                "The attack dice settled. {defender} now rolls their own defend dice."
                            ));
                        }
                    }
                    DicePurpose::AttackDefense(plan) => {
                        let defend_faces: Vec<CombatFace> = results
                            .iter()
                            .filter_map(|result| match result {
                                DieResult::Combat(face) => Some(*face),
                                DieResult::Movement(_) => None,
                            })
                            .collect();
                        match pending_attack_faces.take() {
                            Some((pending_plan, attack_faces)) if pending_plan == plan => {
                                if let Err(error) =
                                    game.resolve_attack(plan, &attack_faces, &defend_faces)
                                {
                                    game.notify(error.to_string());
                                }
                            }
                            _ => {
                                game.notify("The staged combat roll no longer matches this attack.")
                            }
                        }
                    }
                    DicePurpose::HeroSpellRed(roll) => {
                        let values = results
                            .iter()
                            .filter_map(|result| match result {
                                DieResult::Movement(face) => Some(*face),
                                DieResult::Combat(_) => None,
                            })
                            .collect::<Vec<_>>();
                        if let Err(error) = game.resolve_hero_spell_red_roll(roll, &values) {
                            game.notify(error.to_string());
                        }
                    }
                    DicePurpose::HeroSpellCombatOffense(roll) => {
                        let faces = results
                            .iter()
                            .filter_map(|result| match result {
                                DieResult::Combat(face) => Some(*face),
                                DieResult::Movement(_) => None,
                            })
                            .collect::<Vec<_>>();
                        if let HeroSpellDiceKind::Combat { defend_dice, .. } = roll.kind {
                            if defend_dice == 0 {
                                if let Err(error) =
                                    game.resolve_hero_spell_combat_roll(roll, &faces, &[])
                                {
                                    game.notify(error.to_string());
                                }
                            } else {
                                pending_hero_spell_attack_faces = Some((roll, faces));
                                game.notify(
                                    "The Genie's attack settled; Zargon now rolls the Monster's defend dice.",
                                );
                            }
                        } else {
                            game.notify("The Hero-spell combat roll has the wrong dice type.");
                        }
                    }
                    DicePurpose::HeroSpellCombatDefense(roll) => {
                        let defend_faces = results
                            .iter()
                            .filter_map(|result| match result {
                                DieResult::Combat(face) => Some(*face),
                                DieResult::Movement(_) => None,
                            })
                            .collect::<Vec<_>>();
                        match pending_hero_spell_attack_faces.take() {
                            Some((pending_roll, attack_faces)) if pending_roll == roll => {
                                if let Err(error) = game.resolve_hero_spell_combat_roll(
                                    roll,
                                    &attack_faces,
                                    &defend_faces,
                                ) {
                                    game.notify(error.to_string());
                                }
                            }
                            _ => game.notify(
                                "The staged Genie defend roll no longer matches the spell.",
                            ),
                        }
                    }
                    DicePurpose::Disarm(plan) => {
                        let face = results.iter().find_map(|result| match result {
                            DieResult::Combat(face) => Some(*face),
                            DieResult::Movement(_) => None,
                        });
                        if let Some(face) = face
                            && let Err(error) = game.resolve_disarm(plan, face)
                        {
                            game.notify(error.to_string());
                        }
                    }
                    DicePurpose::Jump(plan) => {
                        let face = results.iter().find_map(|result| match result {
                            DieResult::Combat(face) => Some(*face),
                            DieResult::Movement(_) => None,
                        });
                        if let Some(face) = face
                            && let Err(error) = game.resolve_jump(plan, face)
                        {
                            game.notify(error.to_string());
                        }
                    }
                    DicePurpose::Trap(pending) => {
                        let faces = results
                            .iter()
                            .filter_map(|result| match result {
                                DieResult::Combat(face) => Some(*face),
                                DieResult::Movement(_) => None,
                            })
                            .collect::<Vec<_>>();
                        if let Err(error) = game.resolve_trap_roll(pending, &faces) {
                            game.notify(error.to_string());
                        }
                    }
                    DicePurpose::CollapsingCeiling { hero } => {
                        let face = results.iter().find_map(|result| match result {
                            DieResult::Movement(face) => Some(*face),
                            DieResult::Combat(_) => None,
                        });
                        if let Some(face) = face
                            && let Err(error) = game.resolve_collapsing_ceiling_roll(hero, face)
                        {
                            game.notify(error.to_string());
                        }
                    }
                    DicePurpose::Teleport { subject } => {
                        let values: Vec<_> = results
                            .iter()
                            .filter_map(|result| match result {
                                DieResult::Movement(face) => Some(*face),
                                DieResult::Combat(_) => None,
                            })
                            .collect();
                        match game.resolve_teleport_roll(&values) {
                            Ok(_) => renderer.focus_unit(subject),
                            Err(error) => game.notify(error.to_string()),
                        }
                    }
                    DicePurpose::HealingPotion { hero } => {
                        let face = results.iter().find_map(|result| match result {
                            DieResult::Movement(face) => Some(*face),
                            DieResult::Combat(_) => None,
                        });
                        if let Some(face) = face {
                            match game.resolve_healing_potion_roll(hero, face) {
                                Ok(_) => renderer.focus_unit(hero),
                                Err(error) => game.notify(error.to_string()),
                            }
                        }
                    }
                    DicePurpose::ChaosSpellRoll { target, spell } => {
                        let values: Vec<_> = results
                            .iter()
                            .filter_map(|result| match result {
                                DieResult::Movement(face) => Some(*face),
                                DieResult::Combat(_) => None,
                            })
                            .collect();
                        if let Err(error) =
                            game.resolve_chaos_spell_resistance(target, spell, &values)
                        {
                            game.notify(error.to_string());
                        }
                    }
                }
                animation.result_applied_at = Some(now);
            } else if animation
                .result_applied_at
                .is_some_and(|applied| now.duration_since(applied) >= DiceAnimation::RESULT_LINGER)
            {
                let defense_plan = match animation.purpose {
                    DicePurpose::AttackOffense(plan)
                        if plan.defend_dice > 0
                            && pending_attack_faces
                                .as_ref()
                                .is_some_and(|(pending, _)| *pending == plan) =>
                    {
                        Some(plan)
                    }
                    _ => None,
                };
                let hero_spell_defense = match animation.purpose {
                    DicePurpose::HeroSpellCombatOffense(roll)
                        if pending_hero_spell_attack_faces
                            .as_ref()
                            .is_some_and(|(pending, _)| *pending == roll) =>
                    {
                        match roll.kind {
                            HeroSpellDiceKind::Combat { defend_dice, .. } if defend_dice > 0 => {
                                Some((roll, defend_dice))
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };
                let summoned_by = match animation.purpose {
                    DicePurpose::ChaosSpellRoll {
                        target,
                        spell: ChaosSpell::SummonOrcs | ChaosSpell::SummonUndead,
                    } => Some(target),
                    _ => None,
                };
                dice_animation = None;
                renderer.finish_dice_roll();
                if let Some(plan) = defense_plan {
                    let may_drink_defense = game.unit(plan.defender).is_some_and(|defender| {
                        defender.faction == heroquest::model::Faction::Hero
                            && defender.inventory.potion_of_defense > 0
                    });
                    if may_drink_defense {
                        defense_potion_prompt = Some(plan);
                        renderer.focus_unit(plan.defender);
                        game.notify(
                            "Defense interrupt: drink a Potion of Defense before throwing the Hero's dice, or roll normally.",
                        );
                    } else {
                        begin_defense_dice(
                            &mut game,
                            &mut renderer,
                            plan,
                            &mut pending_attack_faces,
                            &mut dice_animation,
                            &mut next_seed,
                        );
                    }
                } else if let Some((roll, defend_dice)) = hero_spell_defense {
                    next_seed = next_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
                    dice_animation = Some(DiceAnimation::new(
                        DiceTray::combat(next_seed, defend_dice),
                        DicePurpose::HeroSpellCombatDefense(roll),
                    ));
                    renderer.focus_dice(&game, Some(roll.target));
                } else if let Some(caster) = summoned_by {
                    renderer.focus_unit(caster);
                } else {
                    renderer.focus_active_hero(&game);
                }
            }
        }

        if startup.stage == StartupStage::Playing
            && dice_animation.is_none()
            && game.pending_hero_death.is_none()
            && game.pending_possession_pickup.is_none()
            && defense_potion_prompt.is_none()
            && game.phase == GamePhase::ZargonTurn
            && Instant::now() >= zargon_next_step_at
        {
            match game.advance_zargon_turn() {
                Ok(ZargonStep::Moved { unit, .. }) => {
                    renderer.focus_unit(unit);
                    zargon_next_step_at = Instant::now() + Duration::from_millis(620);
                }
                Ok(ZargonStep::Attack(plan)) => {
                    renderer.focus_combat(plan.attacker, plan.defender);
                    next_seed = next_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
                    let attacker = game
                        .unit(plan.attacker)
                        .map(|unit| unit.name.as_str())
                        .unwrap_or("Monster");
                    let defender = game
                        .unit(plan.defender)
                        .map(|unit| unit.name.as_str())
                        .unwrap_or("Hero");
                    game.notify(format!(
                        "{attacker} attacks {defender}; Zargon's physical combat dice are rolling…"
                    ));
                    dice_animation = Some(DiceAnimation::new(
                        DiceTray::combat(next_seed, plan.attack_dice),
                        DicePurpose::AttackOffense(plan),
                    ));
                    renderer.focus_dice(&game, Some(plan.attacker));
                }
                Ok(ZargonStep::Cast {
                    caster,
                    target,
                    spell,
                    resistance_dice,
                }) => {
                    if spell != ChaosSpell::Escape {
                        renderer.focus_combat(caster, target);
                    }
                    if resistance_dice > 0 {
                        next_seed = next_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
                        dice_animation = Some(DiceAnimation::new(
                            DiceTray::movement_count(next_seed, resistance_dice),
                            DicePurpose::ChaosSpellRoll { target, spell },
                        ));
                        renderer.focus_dice(&game, Some(target));
                        let roll_message = match spell {
                            ChaosSpell::BallOfFlame => {
                                "Ball of Flame strikes; two physical red save dice are rolling…"
                                    .to_owned()
                            }
                            ChaosSpell::Firestorm => {
                                "Firestorm engulfs this figure; two physical red save dice are rolling…"
                                    .to_owned()
                            }
                            ChaosSpell::SummonOrcs => {
                                "Summon Orcs: one physical red die determines 4, 5, or 6 Orcs…"
                                    .to_owned()
                            }
                            ChaosSpell::SummonUndead => {
                                "Summon Undead: one physical red die determines the exact Skeleton, Zombie, and Mummy group…"
                                    .to_owned()
                            }
                            _ => format!(
                                "{spell:?} takes hold; {resistance_dice} physical Mind dice are rolling…"
                            ),
                        };
                        game.notify(roll_message);
                    } else {
                        game.notify(format!("Zargon resolved {spell:?}."));
                        zargon_next_step_at = Instant::now() + Duration::from_millis(900);
                    }
                }
                Ok(ZargonStep::HeroSpellRoll(roll)) => {
                    next_seed = next_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
                    match roll.kind {
                        HeroSpellDiceKind::Red => {
                            dice_animation = Some(DiceAnimation::new(
                                DiceTray::movement_count(next_seed, roll.dice_count),
                                DicePurpose::HeroSpellRed(roll),
                            ));
                            renderer.focus_dice(&game, Some(roll.target));
                        }
                        HeroSpellDiceKind::Combat { attack_dice, .. } => {
                            dice_animation = Some(DiceAnimation::new(
                                DiceTray::combat(next_seed, attack_dice),
                                DicePurpose::HeroSpellCombatOffense(roll),
                            ));
                            renderer.focus_dice(&game, Some(roll.caster));
                        }
                    }
                }
                Ok(ZargonStep::Finished) => {
                    renderer.focus_active_hero(&game);
                    zargon_next_step_at = Instant::now();
                }
                Err(error) => {
                    game.notify(format!("Computer game master error: {error}"));
                    zargon_next_step_at = Instant::now() + Duration::from_secs(1);
                }
            }
        }

        if startup.stage == StartupStage::Playing {
            window.set_title(&format!("HeroQuest - {}", game.title))?;
            let poses = dice_animation
                .as_ref()
                .map(DiceAnimation::poses)
                .unwrap_or_default();
            let mut overlay = game_overlay_for_campaign(
                &game,
                dice_animation.as_ref(),
                choosing_jump_direction,
                spell_ui,
                attack_ui,
                door_ui,
                possession_ui_index,
                potion_ui,
                defense_potion_prompt,
                share_ui,
                artifact_ui,
                retreat_ui,
                campaign_enabled,
            );
            if let Some(edit) = &sheet_name_edit {
                apply_sheet_name_edit_overlay(&mut overlay, edit);
            } else if let Some(selection) = tabletop_ui {
                apply_tabletop_overlay(&mut overlay, &game, selection.surface, selection.page);
            }
            renderer.render(&game, &poses, &overlay)?;
        } else {
            window.set_title("HeroQuest - Original US Game System")?;
            renderer.render_startup(&startup, &campaign, startup_clock.elapsed().as_secs_f32())?;
        }
        std::thread::sleep(Duration::from_millis(8));
    }
    Ok(())
}

fn poll_app_event() -> Option<AppEvent> {
    use sdl3::sys::events::{SDL_Event, SDL_PollEvent};

    let mut raw = MaybeUninit::<SDL_Event>::uninit();
    // SAFETY: SDL_PollEvent initializes the complete SDL_Event union whenever
    // it returns true. This function runs on the video-initializing thread.
    if !unsafe { SDL_PollEvent(raw.as_mut_ptr()) } {
        return None;
    }
    // SAFETY: established by the successful SDL_PollEvent call above.
    let raw = unsafe { raw.assume_init() };
    Some(decode_app_event(raw))
}

fn decode_app_event(raw: sdl3::sys::events::SDL_Event) -> AppEvent {
    use sdl3::sys::events::{SDL_EVENT_PINCH_BEGIN, SDL_EVENT_PINCH_END, SDL_EVENT_PINCH_UPDATE};

    // SAFETY: `type` is the common first field of every SDL_Event union member.
    let event_type = unsafe { raw.r#type };
    if event_type == SDL_EVENT_PINCH_BEGIN {
        // SAFETY: SDL marks this union as a pinch event.
        let pinch = unsafe { raw.pinch };
        AppEvent::PinchBegin {
            window_id: pinch.windowID.0,
        }
    } else if event_type == SDL_EVENT_PINCH_UPDATE {
        // SAFETY: SDL marks this union as a pinch event.
        let pinch = unsafe { raw.pinch };
        AppEvent::PinchUpdate {
            window_id: pinch.windowID.0,
            scale: pinch.scale,
        }
    } else if event_type == SDL_EVENT_PINCH_END {
        // SAFETY: SDL marks this union as a pinch event.
        let pinch = unsafe { raw.pinch };
        AppEvent::PinchEnd {
            window_id: pinch.windowID.0,
        }
    } else {
        AppEvent::Sdl(Event::from_ll(raw))
    }
}

fn quest_path_from_args() -> Result<Option<PathBuf>> {
    let mut args = std::env::args_os().skip(1);
    let mut quest = None;
    while let Some(argument) = args.next() {
        if argument == "--quest" {
            anyhow::ensure!(quest.is_none(), "--quest may only be supplied once");
            quest = Some(
                args.next()
                    .map(PathBuf::from)
                    .ok_or_else(|| anyhow::anyhow!("--quest requires a JSON path"))?,
            );
        } else {
            anyhow::bail!(
                "unknown argument {:?}; usage: heroquest [--quest PATH]",
                argument
            );
        }
    }
    Ok(quest)
}

fn original_us_campaign_path() -> PathBuf {
    if let Some(path) = std::env::var_os("HEROQUEST_CAMPAIGN_FILE") {
        return PathBuf::from(path);
    }
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(data_home).join("heroquest/original-us-campaign.json");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local/share/heroquest/original-us-campaign.json");
    }
    PathBuf::from("original-us-campaign.json")
}

fn purchase_selected_armory_item(
    campaign: &mut Campaign,
    startup: &mut StartupFlow,
    campaign_path: &std::path::Path,
) {
    let hero = heroquest::startup::HERO_ORDER[startup.armory_hero % 4];
    let listing = ORIGINAL_US_ARMORY[startup.armory_item % ORIGINAL_US_ARMORY.len()];
    let item_name = heroquest::campaign::armory_item_name(listing.item);
    let sheet = campaign.heroes.iter().find(|sheet| sheet.hero == hero);
    let owned_body_armor = match listing.item {
        ArmoryItem::Armor(armor @ (Armor::ChainMail | Armor::PlateMail)) => sheet
            .is_some_and(|sheet| sheet.inventory.armor.contains(&armor))
            .then_some(armor),
        _ => None,
    };
    let owned_weapon_to_equip = match listing.item {
        ArmoryItem::Weapon(weapon) => sheet
            .filter(|sheet| sheet.inventory.weapons.contains(&weapon))
            .filter(|sheet| {
                weapon != Weapon::Dagger || sheet.inventory.equipped_weapon != Some(weapon)
            })
            .map(|_| weapon),
        _ => None,
    };
    let result = if let Some(weapon) = owned_weapon_to_equip {
        campaign.equip_weapon(hero, weapon).map(|()| {
            format!(
                "{} equipped {item_name} without spending gold.",
                hero.name()
            )
        })
    } else if let Some(armor) = owned_body_armor {
        campaign.equip_body_armor(hero, armor).map(|()| {
            format!(
                "{} equipped {item_name} without spending gold.",
                hero.name()
            )
        })
    } else {
        campaign.purchase(hero, listing.item).map(|()| {
            format!(
                "{} purchased {item_name} for {} gold.",
                hero.name(),
                listing.gold
            )
        })
    };
    startup.armory_message = match result {
        Ok(message) => match campaign.save(campaign_path) {
            Ok(()) => format!("{message} The character sheet was saved."),
            Err(error) => format!("{message} The campaign file could not be saved: {error}"),
        },
        Err(error) => error.to_string(),
    };
    startup.armory_revision = startup.armory_revision.wrapping_add(1);
}

fn move_hero(game: &mut Game, renderer: &mut Renderer, direction: Direction) {
    let hero = game.active_mover_id();
    match game.move_active(direction) {
        Ok(_) => {
            if let Some(hero) = hero {
                renderer.focus_unit(hero);
            }
        }
        Err(error) => game.notify(error.to_string()),
    }
}

fn queue_planned_movement(
    game: &mut Game,
    destination: Pos,
    planned_movement: &mut Option<PlannedMovement>,
) -> bool {
    let Some(steps) = game.active_move_path_to(destination) else {
        if let Some(blocker) = game
            .units
            .iter()
            .find(|unit| unit.alive && !unit.escaped && unit.pos == destination)
        {
            let instruction = if blocker.faction == heroquest::model::Faction::Monster {
                "Attack it before moving through."
            } else {
                "Select a lit square beyond that Hero to pass through."
            };
            game.notify(format!(
                "{} occupies that square. {instruction}",
                blocker.name
            ));
        } else {
            game.notify("That square is not reachable with the remaining movement roll.");
        }
        return false;
    };
    let Some(mover) = game.active_mover_id() else {
        return false;
    };
    let name = game
        .unit(mover)
        .map(|unit| unit.name.as_str())
        .unwrap_or("The Hero");
    game.notify(format!(
        "{name} is following an A* route of {} square{} to {},{}.",
        steps.len(),
        if steps.len() == 1 { "" } else { "s" },
        destination.x + 1,
        destination.y + 1
    ));
    *planned_movement = Some(PlannedMovement {
        mover,
        destination,
        steps: steps.into(),
        next_step_at: Instant::now(),
    });
    true
}

fn attack_target_positions(game: &Game) -> Vec<heroquest::model::Pos> {
    let mut positions = game
        .active_attack_options()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|plan| game.unit(plan.defender).map(|unit| unit.pos))
        .collect::<Vec<_>>();
    positions.sort_by_key(|pos| (pos.y, pos.x));
    positions.dedup();
    positions
}

fn adjacent_door_positions(game: &Game) -> Vec<heroquest::model::Pos> {
    let Some(hero_pos) = game.active_hero().map(|hero| hero.pos) else {
        return Vec::new();
    };
    game.adjacent_closed_door_indices()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|index| game.doors.get(index)?.other_side(hero_pos))
        .collect()
}

fn adjacent_door_at_board_pos(game: &Game, pos: heroquest::model::Pos) -> Option<usize> {
    let hero_pos = game.active_hero()?.pos;
    game.adjacent_closed_door_indices()
        .ok()?
        .into_iter()
        .find(|&index| {
            game.doors
                .get(index)
                .and_then(|door| door.other_side(hero_pos))
                == Some(pos)
        })
}

fn focus_door(renderer: &mut Renderer, game: &Game, door_index: usize) {
    let Some(hero_pos) = game.active_hero().map(|hero| hero.pos) else {
        return;
    };
    if let Some(pos) = game
        .doors
        .get(door_index)
        .and_then(|door| door.other_side(hero_pos))
    {
        renderer.focus_board_pos(pos);
    }
}

fn open_selected_door(game: &mut Game, renderer: &mut Renderer, door_index: usize) -> bool {
    renderer.set_selection_highlights(Vec::new());
    match game.open_selected_adjacent_door(door_index) {
        Ok((a, b)) => {
            let hero_pos = game.active_hero().map(|hero| hero.pos);
            renderer.focus_board_pos(if hero_pos == Some(a) { b } else { a });
            true
        }
        Err(error) => {
            game.notify(error.to_string());
            renderer.focus_active_hero(game);
            false
        }
    }
}

fn available_active_potions(game: &Game) -> Vec<PotionChoice> {
    game.active_owned_potion_kinds()
        .into_iter()
        .map(PotionChoice::from_kind)
        .collect()
}

fn available_active_artifacts(game: &Game) -> Vec<ArtifactAction> {
    let mut choices = Vec::new();
    if !game.elixir_of_life_targets().is_empty() {
        choices.push(ArtifactAction::ElixirOfLife);
    }
    if game.can_use_ring_of_return() {
        choices.push(ArtifactAction::RingOfReturn);
    }
    if !game.spell_ring_storable_spells().is_empty() {
        choices.push(ArtifactAction::DeclareSpellRing);
    }
    choices
}

fn activate_selected_potion(
    game: &mut Game,
    renderer: &mut Renderer,
    choice: PotionChoice,
    dice_animation: &mut Option<DiceAnimation>,
    next_seed: &mut u64,
) -> bool {
    let result = match choice {
        PotionChoice::Healing => match game.begin_active_healing_potion() {
            Ok(HealingPotionUse::Restored { hero, .. }) => {
                renderer.focus_unit(hero);
                Ok(())
            }
            Ok(HealingPotionUse::RollRedDie { hero }) => {
                *next_seed = next_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
                *dice_animation = Some(DiceAnimation::new(
                    DiceTray::movement_count(*next_seed, 1),
                    DicePurpose::HealingPotion { hero },
                ));
                renderer.focus_dice(game, Some(hero));
                Ok(())
            }
            Err(error) => Err(error),
        },
        PotionChoice::HeroicBrew => game.drink_heroic_brew(),
        PotionChoice::Strength => game.drink_potion_of_strength(),
        PotionChoice::Defense => game.drink_active_potion_of_defense(),
        PotionChoice::Petrification => game.use_petrification_potion(),
    };
    match result {
        Ok(()) => true,
        Err(error) => {
            game.notify(error.to_string());
            false
        }
    }
}

fn begin_defense_dice(
    game: &mut Game,
    renderer: &mut Renderer,
    plan: AttackPlan,
    pending_attack_faces: &mut Option<(AttackPlan, Vec<CombatFace>)>,
    dice_animation: &mut Option<DiceAnimation>,
    next_seed: &mut u64,
) {
    let Some((_, attack_faces)) = pending_attack_faces.take() else {
        game.notify("The staged attack no longer has offense dice.");
        return;
    };
    *pending_attack_faces = Some((plan, attack_faces));
    *next_seed = next_seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    *dice_animation = Some(DiceAnimation::new(
        DiceTray::combat(*next_seed, plan.defend_dice),
        DicePurpose::AttackDefense(plan),
    ));
    renderer.focus_dice(game, Some(plan.defender));
}

fn begin_selected_attack(
    game: &mut Game,
    renderer: &mut Renderer,
    plan: AttackPlan,
    dice_animation: &mut Option<DiceAnimation>,
    next_seed: &mut u64,
) {
    renderer.set_selection_highlights(Vec::new());
    renderer.focus_combat(plan.attacker, plan.defender);
    *next_seed = (*next_seed).wrapping_add(0x9e37_79b9_7f4a_7c15);
    *dice_animation = Some(DiceAnimation::new(
        DiceTray::combat(*next_seed, plan.attack_dice),
        DicePurpose::AttackOffense(plan),
    ));
    renderer.focus_dice(game, Some(plan.attacker));
    let attacker = game
        .unit(plan.attacker)
        .map(|unit| unit.name.as_str())
        .unwrap_or("The Hero");
    let defender = game
        .unit(plan.defender)
        .map(|unit| unit.name.as_str())
        .unwrap_or("the target");
    game.notify(format!(
        "{attacker} attacks {defender} with {}; the Hero's own combat dice are rolling.",
        plan.source.name()
    ));
}

fn spell_target_positions(game: &Game, spell: HeroSpell) -> Vec<heroquest::model::Pos> {
    let mut positions = Vec::new();
    for target in game.valid_hero_spell_targets(spell) {
        match target {
            HeroSpellTarget::Unit(id) => {
                if let Some(unit) = game.unit(id)
                    && !positions.contains(&unit.pos)
                {
                    positions.push(unit.pos);
                }
            }
            HeroSpellTarget::Door(index) => {
                if let Some(door) = game.doors.get(index) {
                    for pos in [door.a, door.b] {
                        if !positions.contains(&pos) {
                            positions.push(pos);
                        }
                    }
                }
            }
        }
    }
    positions
}

fn spell_target_at_board_pos(
    game: &Game,
    spell: HeroSpell,
    pos: heroquest::model::Pos,
) -> Option<HeroSpellTarget> {
    game.valid_hero_spell_targets(spell)
        .into_iter()
        .find(|target| match *target {
            HeroSpellTarget::Unit(id) => game.unit(id).is_some_and(|unit| unit.pos == pos),
            HeroSpellTarget::Door(index) => game
                .doors
                .get(index)
                .is_some_and(|door| door.a == pos || door.b == pos),
        })
}

fn spell_target_label(game: &Game, target: HeroSpellTarget) -> String {
    match target {
        HeroSpellTarget::Unit(id) => game
            .unit(id)
            .map(|unit| unit.name.clone())
            .unwrap_or_else(|| "Missing figure".to_owned()),
        HeroSpellTarget::Door(index) => game.doors.get(index).map_or_else(
            || "Missing door".to_owned(),
            |door| {
                format!(
                    "Door {},{} - {},{}",
                    door.a.x + 1,
                    door.a.y + 1,
                    door.b.x + 1,
                    door.b.y + 1
                )
            },
        ),
    }
}

fn focus_spell_target(renderer: &mut Renderer, game: &Game, target: HeroSpellTarget) {
    match target {
        HeroSpellTarget::Unit(id) => renderer.focus_unit(id),
        HeroSpellTarget::Door(index) => {
            if let Some(door) = game.doors.get(index) {
                renderer.focus_board_pos(door.a);
            }
        }
    }
}

fn cast_selected_hero_spell(
    game: &mut Game,
    renderer: &mut Renderer,
    spell: HeroSpell,
    target: HeroSpellTarget,
    dice_animation: &mut Option<DiceAnimation>,
    next_seed: &mut u64,
) -> bool {
    match game.cast_active_hero_spell(spell, target) {
        Ok(HeroSpellCast::Resolved) => {
            renderer.set_selection_highlights(Vec::new());
            focus_spell_target(renderer, game, target);
            true
        }
        Ok(HeroSpellCast::Roll(roll)) => {
            *next_seed = (*next_seed).wrapping_add(0x9e37_79b9_7f4a_7c15);
            match roll.kind {
                HeroSpellDiceKind::Red => {
                    *dice_animation = Some(DiceAnimation::new(
                        DiceTray::movement_count(*next_seed, roll.dice_count),
                        DicePurpose::HeroSpellRed(roll),
                    ));
                    renderer.focus_dice(game, Some(roll.target));
                }
                HeroSpellDiceKind::Combat { attack_dice, .. } => {
                    *dice_animation = Some(DiceAnimation::new(
                        DiceTray::combat(*next_seed, attack_dice),
                        DicePurpose::HeroSpellCombatOffense(roll),
                    ));
                    renderer.focus_dice(game, Some(roll.caster));
                }
            }
            renderer.set_selection_highlights(Vec::new());
            true
        }
        Err(error) => {
            game.notify(error.to_string());
            false
        }
    }
}

fn join_or_none(items: impl IntoIterator<Item = String>) -> String {
    let items = items.into_iter().collect::<Vec<_>>();
    if items.is_empty() {
        "None".to_owned()
    } else {
        items.join(", ")
    }
}

fn hero_condition_summary(hero: &heroquest::game::Unit) -> String {
    let mut conditions = Vec::new();
    if !hero.alive {
        conditions.push("Fallen".to_owned());
    }
    if hero.sleeping {
        conditions.push("Asleep".to_owned());
    }
    if hero.clouded {
        conditions.push("Cloud of Chaos".to_owned());
    }
    if hero.petrified_turns > 0 {
        conditions.push(format!("Stone: {} turns", hero.petrified_turns));
    }
    if hero.in_pit {
        conditions.push("In a pit".to_owned());
    }
    if hero.fearful {
        conditions.push("Afraid".to_owned());
    }
    if hero.sleeping || hero.clouded || hero.petrified_turns > 0 {
        conditions.push("Cannot act normally".to_owned());
    }
    join_or_none(conditions)
}

fn character_sheet_page(hero: &heroquest::game::Unit, page: usize) -> (String, String) {
    let inventory = &hero.inventory;
    match page % 3 {
        0 => (
            format!(
                "Name: {}. Hero: {}. Body: {}/{}. Mind: {}. Gold: {}. Status: {}.",
                hero.name,
                match hero.figure {
                    FigureKind::Hero(kind) => kind.name(),
                    FigureKind::Monster(_) => "Unknown",
                },
                hero.body.max(0),
                hero.stats.body,
                hero.effective_mind(),
                inventory.gold,
                hero_condition_summary(hero),
            ),
            "This is the live record, including damage, Mind, treasure, and conditions.".to_owned(),
        ),
        1 => {
            let weapons = join_or_none(inventory.weapons.iter().map(|weapon| {
                if inventory.equipped_weapon == Some(*weapon) {
                    format!("{} (equipped)", weapon.name())
                } else {
                    weapon.name().to_owned()
                }
            }));
            let armor = join_or_none(inventory.armor.iter().map(|armor| {
                let equipped = matches!(armor, Armor::Helmet | Armor::Shield)
                    || inventory.equipped_body_armor == Some(*armor);
                let name = heroquest::campaign::armory_item_name(ArmoryItem::Armor(*armor));
                if equipped {
                    format!("{name} (equipped)")
                } else {
                    name.to_owned()
                }
            }));
            (
                format!(
                    "Weapons: {weapons}. Armor: {armor}. Tool Kits: {}. Current defense: {} dice.",
                    inventory.tool_kits,
                    hero.effective_defense_dice(),
                ),
                "Equipped gear, carried equipment, and derived defense update immediately."
                    .to_owned(),
            )
        }
        _ => {
            let artifacts = join_or_none(
                inventory
                    .artifacts
                    .iter()
                    .map(|artifact| artifact.name().to_owned()),
            );
            let potions = join_or_none(
                [
                    ("Healing", inventory.potion_of_healing),
                    ("Heroic Brew", inventory.heroic_brew),
                    ("Strength", inventory.potion_of_strength),
                    ("Defense", inventory.potion_of_defense),
                    ("Purple", inventory.petrification_potion),
                ]
                .into_iter()
                .filter(|(_, count)| *count > 0)
                .map(|(name, count)| format!("{name} x{count}")),
            );
            (
                format!(
                    "Artifacts: {artifacts}. Potions: {potions}. Spell cards in hand: {}; discarded: {}. Quest item: {}.",
                    hero.hero_spells.len(),
                    hero.discarded_hero_spells.len(),
                    hero.carried_quest_item
                        .map_or("None".to_owned(), |index| format!("Token {}", index + 1)),
                ),
                "Owned cards and consumables leave this sheet when used, lost, or transferred."
                    .to_owned(),
            )
        }
    }
}

fn apply_sheet_name_edit_overlay(overlay: &mut GameOverlay, edit: &SheetNameEdit) {
    overlay.actions = vec![
        "[ENTER] SAVE NAME".to_owned(),
        "[BACKSPACE] DELETE".to_owned(),
        "[ESC] CANCEL".to_owned(),
    ];
    overlay.dialog = Some(OverlayDialog {
        title: format!("{} CHARACTER SHEET - EDIT NAME", edit.hero.name().to_uppercase()),
        body: format!("Name: {}_", edit.draft),
        hint: "Type up to 24 characters. Body, Mind, gold, equipment, and cards are updated by the rules engine."
            .to_owned(),
    });
}

fn apply_tabletop_overlay(
    overlay: &mut GameOverlay,
    game: &Game,
    surface: TabletopSurface,
    page: usize,
) {
    let mut actions = vec!["[ESC] CLOSE".to_owned()];
    let dialog = match surface {
        TabletopSurface::HeroCard(kind) => {
            let hero = game
                .units
                .iter()
                .find(|unit| unit.figure == FigureKind::Hero(kind));
            let body = hero.map_or_else(
                || "That Hero is not present in this quest.".to_owned(),
                |hero| {
                    format!(
                        "{}: move with 2 red dice; attack {} dice; defend {} dice; Body {}/{}; Mind {}.",
                        hero.name,
                        hero.stats.attack,
                        hero.stats.defend,
                        hero.body.max(0),
                        hero.stats.body,
                        hero.effective_mind(),
                    )
                },
            );
            OverlayDialog {
                title: format!("{} CHARACTER CARD", kind.name().to_uppercase()),
                body,
                hint: "The printed card gives the Hero's starting dice and point values."
                    .to_owned(),
            }
        }
        TabletopSurface::CharacterSheet(kind) => {
            actions.insert(0, "[LEFT] PREVIOUS PAGE".to_owned());
            actions.insert(1, "[RIGHT] NEXT PAGE".to_owned());
            if page % 5 == 0 {
                actions.insert(2, "[N] EDIT HERO NAME".to_owned());
            }
            let hero = game
                .units
                .iter()
                .find(|unit| unit.figure == FigureKind::Hero(kind));
            let (body, hint) = if page % 5 == 3 {
                (
                    game.blurb.clone(),
                    match game.phase {
                        GamePhase::Won => "Objective complete; this Quest may now be recorded.",
                        GamePhase::Retreated => "The party left before completing this objective.",
                        GamePhase::Lost => "The party fell before completing this objective.",
                        _ => "This is the public Quest objective; secret map notes remain hidden.",
                    }
                    .to_owned(),
                )
            } else if page % 5 == 4 {
                let entries = game.log.iter().rev().take(5).cloned().collect::<Vec<_>>();
                (
                    if entries.is_empty() {
                        "No events have been recorded yet.".to_owned()
                    } else {
                        entries.into_iter().rev().collect::<Vec<_>>().join(" | ")
                    },
                    "The five most recent public events are copied from the shared adventure log."
                        .to_owned(),
                )
            } else {
                hero.map_or_else(
                    || (
                        "That Hero is not present in this quest.".to_owned(),
                        "Close this record to return to the table.".to_owned(),
                    ),
                    |hero| character_sheet_page(hero, page),
                )
            };
            OverlayDialog {
                title: format!(
                    "{} CHARACTER SHEET  {}/5",
                    kind.name().to_uppercase(),
                    page % 5 + 1
                ),
                body,
                hint,
            }
        }
        TabletopSurface::ActionReference(kind) => OverlayDialog {
            title: format!("{} TURN REFERENCE", kind.name().to_uppercase()),
            body: "On a Hero turn: move and perform one action. Actions are attack, cast a spell, search for treasure, or search for traps and secret doors. Open doors while moving."
                .to_owned(),
            hint: "The main OSD shows only actions that are legal in the current board state."
                .to_owned(),
        },
        TabletopSurface::Armory => {
            actions.insert(0, "[LEFT] PREVIOUS PAGE".to_owned());
            actions.insert(1, "[RIGHT] NEXT PAGE".to_owned());
            let first = (page % 2) * 6;
            let body = ORIGINAL_US_ARMORY[first..first + 6]
                .iter()
                .map(|listing| {
                    format!(
                        "{} - {} gold",
                        heroquest::campaign::armory_item_name(listing.item),
                        listing.gold
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            OverlayDialog {
                title: format!("ORIGINAL-US ARMORY  {}/2", page % 2 + 1),
                body,
                hint: "Purchase and equip items between completed campaign quests."
                    .to_owned(),
            }
        }
        TabletopSurface::HeroSpellCard { hero, spell } => {
            let active_matches = game
                .active_hero()
                .is_some_and(|unit| unit.figure == FigureKind::Hero(hero));
            let castable = game.active_castable_hero_spells().contains(&spell);
            let has_target = !game.valid_hero_spell_targets(spell).is_empty();
            if active_matches && castable && has_target {
                actions.insert(0, "[ENTER] CAST THIS SPELL".to_owned());
            }
            OverlayDialog {
                title: format!(
                    "{} SPELL CARD - {}",
                    hero.name().to_uppercase(),
                    spell.name().to_uppercase()
                ),
                body: spell.rules_summary().to_owned(),
                hint: if active_matches && castable && has_target {
                    "This physical card is in hand and has a legal target now.".to_owned()
                } else if active_matches && castable {
                    "This card remains in hand, but currently has no legal target.".to_owned()
                } else {
                    "This card belongs to another Hero or cannot be cast at this point in the turn."
                        .to_owned()
                },
            }
        }
        TabletopSurface::HeroSpellDiscard { hero, spell, count } => OverlayDialog {
            title: format!(
                "{} SPELL DISCARD - {}",
                hero.name().to_uppercase(),
                spell.name().to_uppercase()
            ),
            body: format!(
                "{count} used spell card{} in this face-up pile. Top card: {}. {}",
                if count == 1 { "" } else { "s" },
                spell.name(),
                spell.rules_summary()
            ),
            hint: "This card has been consumed for the quest and cannot be cast again."
                .to_owned(),
        },
        TabletopSurface::ZargonDeck(deck) => match deck {
            ZargonDeckKind::Treasure => OverlayDialog {
                title: "TREASURE CARD DECK".to_owned(),
                body: format!(
                    "{} physical cards remain. A legal treasure search shuffles, draws, resolves, and returns or discards the card according to its printed rule.",
                    game.treasure_deck.remaining()
                ),
                hint: "The deck cannot be peeked at or drawn outside a legal search action."
                    .to_owned(),
            },
            ZargonDeckKind::Artifact => OverlayDialog {
                title: "ZARGON ONLY - ARTIFACT CARDS".to_owned(),
                body: "These card backs conceal special treasures until a Quest note awards one. Their faces are hidden from the Heroes."
                    .to_owned(),
                hint: "Hidden information remains protected while the computer plays Zargon."
                    .to_owned(),
            },
            ZargonDeckKind::ChaosSpell => OverlayDialog {
                title: "ZARGON ONLY - CHAOS SPELLS".to_owned(),
                body: "These spell faces and Zargon's remaining hand are secret. The computer reveals a spell only when its caster uses it."
                    .to_owned(),
                hint: "No hidden spell name or order is exposed to the Hero players."
                    .to_owned(),
            },
            ZargonDeckKind::Monster => OverlayDialog {
                title: "MONSTER CARD STACK".to_owned(),
                body: "The public Monster reference cards are laid face-up beside this stack. Select a face-up card to inspect its printed values."
                    .to_owned(),
                hint: "Monster movement, attack, defense, Body, and Mind are public rules."
                    .to_owned(),
            },
        },
        TabletopSurface::ChaosSpellDiscard { spell, count } => OverlayDialog {
            title: format!("CHAOS SPELL DISCARD - {}", spell.name().to_uppercase()),
            body: format!(
                "{count} revealed Chaos Spell card{} in the shared face-up pile. Top card: {}. {}",
                if count == 1 { "" } else { "s" },
                spell.name(),
                spell.rules_summary()
            ),
            hint: "Used Chaos cards are public; uncast cards and their owners remain concealed."
                .to_owned(),
        },
        TabletopSurface::MonsterCard(kind) => {
            let stats = monster_stats(kind);
            OverlayDialog {
                title: format!("{} MONSTER CARD", kind.name().to_uppercase()),
                body: format!(
                    "Movement {} squares; attack {} combat dice; defend {} combat dice; Body Points {}; Mind Points {}.",
                    stats.movement, stats.attack, stats.defend, stats.body, stats.mind
                ),
                hint: "This is a public face-up reference card; Quest-note exceptions still apply."
                    .to_owned(),
            }
        }
        TabletopSurface::QuestBook => OverlayDialog {
            title: "ZARGON ONLY - QUEST BOOK".to_owned(),
            body: "The map, room contents, secret doors, traps, wandering Monster, and Quest notes are concealed behind Zargon's screen and revealed only when the rules require it."
                .to_owned(),
            hint: "The computer reads this page without exposing unrevealed Quest information."
                .to_owned(),
        },
    };
    overlay.actions = actions;
    overlay.dialog = Some(dialog);
}

fn game_overlay_for_campaign(
    game: &Game,
    dice_animation: Option<&DiceAnimation>,
    choosing_jump_direction: bool,
    spell_ui: Option<SpellUi>,
    attack_ui: Option<AttackUi>,
    door_ui: Option<DoorUi>,
    possession_ui_index: usize,
    potion_ui: Option<PotionUi>,
    defense_potion_prompt: Option<AttackPlan>,
    share_ui: Option<ShareUi>,
    artifact_ui: Option<ArtifactUi>,
    retreat_ui: bool,
    campaign_enabled: bool,
) -> GameOverlay {
    let heading = format!("HEROQUEST  |  {}", game.title.to_uppercase());
    let message = game
        .log
        .back()
        .cloned()
        .unwrap_or_else(|| "The quest begins.".to_owned());
    let mut actions = Vec::new();
    let mut dialog = None;

    let status = match game.phase {
        GamePhase::HeroTurn { .. } => {
            let hero = game.active_hero().expect("active Hero exists");
            let condition = if hero.clouded {
                "  |  PARALYZED"
            } else if hero.sleeping {
                "  |  ASLEEP"
            } else {
                ""
            };
            format!(
                "{}  |  Body {}/{}  |  Move {}{}",
                hero.name, hero.body, hero.stats.body, game.hero_turn.movement_left, condition
            )
        }
        GamePhase::AllyTurn { ally, .. } => {
            let ally = game.unit(ally).expect("active ally exists");
            format!(
                "Escort: {}  |  Body {}/{}  |  Move {}",
                ally.name, ally.body, ally.stats.body, game.hero_turn.movement_left
            )
        }
        GamePhase::ZargonTurn => "Zargon's turn - the camera follows the action".to_owned(),
        GamePhase::Won => "QUEST COMPLETE".to_owned(),
        GamePhase::Retreated => "QUEST ENDED WITHOUT COMPLETION".to_owned(),
        GamePhase::Lost => "THE HEROES HAVE FALLEN".to_owned(),
    };

    if dice_animation.is_some() {
        // The focused physical dice are the status display. Keep the action
        // strip empty rather than drawing a button-shaped label that cannot be clicked.
    } else if retreat_ui {
        actions.push("[ENTER] END QUEST WITHOUT REWARD".to_owned());
        actions.push("[ESC] CANCEL".to_owned());
        dialog = Some(OverlayDialog {
            title: "END QUEST EARLY?".to_owned(),
            body: "Every surviving Hero is on the stairway. You may end this unfinished quest now, but its objective will remain incomplete and no final quest reward will be awarded."
                .to_owned(),
            hint: "Press Enter to retreat, or Esc to keep playing.".to_owned(),
        });
    } else if let Some(pending) = game.pending_hero_death {
        let name = game
            .unit(pending.hero)
            .map(|hero| hero.name.as_str())
            .unwrap_or("The Hero");
        let choices = game.pending_hero_death_choices();
        let mut body = format!("{name} has reached zero Body Points.\n\n");
        if choices.contains(&HeroDeathChoice::HealingPotion) {
            body.push_str("[C] Drink a Potion of Healing and roll one red die.\n");
        }
        if choices.contains(&HeroDeathChoice::HealBody) {
            body.push_str("[H] Cast the unused Heal Body card on yourself.\n");
        }
        if choices.contains(&HeroDeathChoice::WaterOfHealing) {
            body.push_str("[W] Cast the unused Water of Healing card on yourself.\n");
        }
        body.push_str("[E] Accept death and remove this Hero from the quest.");
        if choices.contains(&HeroDeathChoice::HealingPotion) {
            actions.push("[C] HEALING POTION".to_owned());
        }
        if choices.contains(&HeroDeathChoice::HealBody) {
            actions.push("[H] CAST HEAL BODY".to_owned());
        }
        if choices.contains(&HeroDeathChoice::WaterOfHealing) {
            actions.push("[W] CAST WATER OF HEALING".to_owned());
        }
        actions.push("[E] ACCEPT DEATH".to_owned());
        dialog = Some(OverlayDialog {
            title: "ZERO BODY POINTS".to_owned(),
            body,
            hint: "This decision interrupts all movement and Zargon actions.".to_owned(),
        });
    } else if let Some(pickup) = &game.pending_possession_pickup {
        let recipient = pickup
            .eligible_heroes
            .get(possession_ui_index % pickup.eligible_heroes.len().max(1))
            .and_then(|&hero| game.unit(hero));
        let fallen = game
            .unit(pickup.dead_hero)
            .map(|hero| hero.name.as_str())
            .unwrap_or("The fallen Hero");
        actions.push("[LEFT] PREVIOUS HERO".to_owned());
        actions.push("[RIGHT] NEXT HERO".to_owned());
        actions.push("[ENTER] PICK UP POSSESSIONS".to_owned());
        dialog = Some(OverlayDialog {
            title: "FALLEN HERO'S POSSESSIONS".to_owned(),
            body: format!(
                "{} is present in the same room or corridor and may take {}'s weapons, armor, artifacts, gold, and potions.",
                recipient.map_or("A Hero", |hero| hero.name.as_str()),
                fallen
            ),
            hint: "Choose the Hero who will carry everything, then press Enter.".to_owned(),
        });
    } else if let Some(plan) = defense_potion_prompt {
        let defender = game
            .unit(plan.defender)
            .map(|unit| unit.name.as_str())
            .unwrap_or("The Hero");
        actions.push("[D] DRINK POTION".to_owned());
        actions.push("[ENTER] ROLL NORMAL DEFENSE".to_owned());
        dialog = Some(OverlayDialog {
            title: "DEFENSE INTERRUPT".to_owned(),
            body: format!(
                "The attack dice have settled. {defender} may drink one Potion of Defense now to add two combat dice before throwing the physical defense dice."
            ),
            hint: format!(
                "Current defense: {} dice. Potion defense: {} dice.",
                plan.defend_dice,
                plan.defend_dice.saturating_add(2)
            ),
        });
    } else if let Some(selection) = share_ui {
        let recipients = game.living_other_heroes();
        match selection {
            ShareUi::Potion { potion, index } => {
                if let Some(recipient) = recipients
                    .get(index % recipients.len().max(1))
                    .and_then(|&hero| game.unit(hero))
                {
                    actions.push("[LEFT] PREVIOUS HERO".to_owned());
                    actions.push("[RIGHT] NEXT HERO".to_owned());
                    actions.push("[ENTER] GIVE POTION".to_owned());
                    actions.push("[ESC] CANCEL".to_owned());
                    dialog = Some(OverlayDialog {
                        title: "GIVE A POTION".to_owned(),
                        body: format!("Give {} to {}?", potion.name(), recipient.name),
                        hint: "Potion cards may be transferred only during the giver's Hero turn."
                            .to_owned(),
                    });
                }
            }
            ShareUi::Gold { index, amount } => {
                if let Some(recipient) = recipients
                    .get(index % recipients.len().max(1))
                    .and_then(|&hero| game.unit(hero))
                {
                    actions.push("[LEFT] PREVIOUS HERO".to_owned());
                    actions.push("[RIGHT] NEXT HERO".to_owned());
                    actions.push("[UP] ADD 10 GOLD".to_owned());
                    actions.push("[DOWN] REMOVE 10 GOLD".to_owned());
                    actions.push("[ENTER] GIVE GOLD".to_owned());
                    actions.push("[G] SELECT ALL GOLD".to_owned());
                    actions.push("[ESC] CANCEL".to_owned());
                    dialog = Some(OverlayDialog {
                        title: "SHARE GOLD".to_owned(),
                        body: format!("Give {amount} gold coins to {}?", recipient.name),
                        hint: "Press G for all available gold, or Esc to cancel.".to_owned(),
                    });
                }
            }
        }
    } else if let Some(selection) = artifact_ui {
        match selection {
            ArtifactUi::ChooseArtifact { index } => {
                let choices = available_active_artifacts(game);
                if let Some(&choice) = choices.get(index % choices.len().max(1)) {
                    let artifact = choice.artifact();
                    actions.push("[LEFT] PREVIOUS ARTIFACT".to_owned());
                    actions.push("[RIGHT] NEXT ARTIFACT".to_owned());
                    actions.push("[ENTER] CHOOSE ARTIFACT".to_owned());
                    actions.push("[ESC] CANCEL".to_owned());
                    dialog = Some(OverlayDialog {
                        title: format!(
                            "ARTIFACT  {}/{}  -  {}",
                            index % choices.len() + 1,
                            choices.len(),
                            artifact.name().to_uppercase()
                        ),
                        body: artifact.rules_summary().to_owned(),
                        hint: match choice {
                            ArtifactAction::ElixirOfLife => {
                                "Choose the fallen Hero who will return to life.".to_owned()
                            }
                            ArtifactAction::RingOfReturn => {
                                "Using the ring consumes it immediately.".to_owned()
                            }
                            ArtifactAction::DeclareSpellRing => {
                                "The declared spell remains in the ring for exactly two casts."
                                    .to_owned()
                            }
                        },
                    });
                }
            }
            ArtifactUi::ChooseElixirTarget { index } => {
                let targets = game.elixir_of_life_targets();
                if let Some(&target) = targets.get(index % targets.len().max(1))
                    && let Some(hero) = game.unit(target)
                {
                    actions.push("[LEFT] PREVIOUS HERO".to_owned());
                    actions.push("[RIGHT] NEXT HERO".to_owned());
                    actions.push("[ENTER] REVIVE HERO".to_owned());
                    actions.push("[BACKSPACE] ARTIFACT LIST".to_owned());
                    dialog = Some(OverlayDialog {
                        title: "ELIXIR OF LIFE".to_owned(),
                        body: format!(
                            "Revive {} with all {} Body Points and full Mind Points?",
                            hero.name, hero.stats.body
                        ),
                        hint: "The Elixir is discarded after this one use.".to_owned(),
                    });
                }
            }
            ArtifactUi::ChooseSpellRingSpell { index } => {
                let spells = game.spell_ring_storable_spells();
                if let Some(&spell) = spells.get(index % spells.len().max(1)) {
                    actions.push("[LEFT] PREVIOUS SPELL".to_owned());
                    actions.push("[RIGHT] NEXT SPELL".to_owned());
                    actions.push("[ENTER] STORE SPELL".to_owned());
                    actions.push("[BACKSPACE] ARTIFACT LIST".to_owned());
                    dialog = Some(OverlayDialog {
                        title: format!(
                            "SPELL RING  {}/{}  -  {}",
                            index % spells.len() + 1,
                            spells.len(),
                            spell.name().to_uppercase()
                        ),
                        body: spell.rules_summary().to_owned(),
                        hint: "Declare now; this spell may be cast twice during the Quest."
                            .to_owned(),
                    });
                }
            }
        }
    } else if let Some(selection) = potion_ui {
        let choices = available_active_potions(game);
        if let Some(&choice) = choices.get(selection.index % choices.len().max(1)) {
            actions.push("[LEFT] PREVIOUS POTION".to_owned());
            actions.push("[RIGHT] NEXT POTION".to_owned());
            actions.push("[ENTER] DRINK POTION".to_owned());
            actions.push("[G] GIVE POTION".to_owned());
            actions.push("[ESC] CANCEL".to_owned());
            dialog = Some(OverlayDialog {
                title: format!(
                    "POTION  {}/{}  -  {}",
                    selection.index % choices.len() + 1,
                    choices.len(),
                    choice.name().to_uppercase()
                ),
                body: choice.rules().to_owned(),
                hint: "Drinking a potion is free; reopen this inventory to combine another potion."
                    .to_owned(),
            });
        }
    } else if let Some(pending) = game.pending_trap_roll() {
        let name = game
            .unit(pending.hero)
            .map(|hero| hero.name.as_str())
            .unwrap_or("The Hero");
        dialog = Some(OverlayDialog {
            title: "TRAP!".to_owned(),
            body: format!(
                "{name} has sprung a trap. {} physical combat {} must settle before play continues.",
                pending.dice_count(),
                if pending.dice_count() == 1 {
                    "die"
                } else {
                    "dice"
                }
            ),
            hint: "The camera is moving to the Hero's dice on the table.".to_owned(),
        });
    } else if game.pending_falling_block.is_some() {
        actions.push("[W] UP".to_owned());
        actions.push("[D] RIGHT".to_owned());
        actions.push("[S] DOWN".to_owned());
        actions.push("[A] LEFT".to_owned());
        dialog = Some(OverlayDialog {
            title: "FALLING BLOCK!".to_owned(),
            body:
                "Choose the open square ahead of or behind the Hero before the passage is sealed."
                    .to_owned(),
            hint: "Press the direction of the safe square.".to_owned(),
        });
    } else if let Some(selection) = attack_ui {
        let options = game.active_attack_options().unwrap_or_default();
        if let Some(plan) = options.get(selection.index % options.len().max(1)) {
            let target = game
                .unit(plan.defender)
                .map(|unit| unit.name.as_str())
                .unwrap_or("Missing target");
            actions.push("[LEFT] PREVIOUS ATTACK".to_owned());
            actions.push("[RIGHT] NEXT ATTACK".to_owned());
            actions.push("[ENTER] ATTACK".to_owned());
            actions.push("[ESC] CANCEL".to_owned());
            dialog = Some(OverlayDialog {
                title: format!(
                    "ATTACK  {}/{}  -  {}",
                    selection.index % options.len() + 1,
                    options.len(),
                    plan.source.name().to_uppercase()
                ),
                body: format!(
                    "Target: {target}\nWeapon: {}\nAttack dice: {}    Defend dice: {}",
                    plan.source.name(),
                    plan.attack_dice,
                    plan.defend_dice
                ),
                hint: "Cyan outlines are legal targets. Arrow through every legal target/weapon combination."
                    .to_owned(),
            });
        }
    } else if let Some(selection) = spell_ui {
        match selection {
            SpellUi::ChooseSpell { index } => {
                let spells = game.active_castable_hero_spells();
                if let Some(&spell) = spells.get(index % spells.len().max(1)) {
                    actions.push("[LEFT] PREVIOUS SPELL".to_owned());
                    actions.push("[RIGHT] NEXT SPELL".to_owned());
                    actions.push("[ENTER] CHOOSE SPELL".to_owned());
                    actions.push("[ESC] CANCEL".to_owned());
                    dialog = Some(OverlayDialog {
                        title: format!(
                            "CAST A SPELL  {}/{}  -  {}",
                            index % spells.len() + 1,
                            spells.len(),
                            spell.name().to_uppercase()
                        ),
                        body: spell.rules_summary().to_owned(),
                        hint: "Choose this card, then select one of its highlighted legal targets."
                            .to_owned(),
                    });
                }
            }
            SpellUi::ChooseTarget { spell, index } => {
                let targets = game.valid_hero_spell_targets(spell);
                if let Some(&target) = targets.get(index % targets.len().max(1)) {
                    actions.push("[LEFT] PREVIOUS TARGET".to_owned());
                    actions.push("[RIGHT] NEXT TARGET".to_owned());
                    actions.push("[ENTER] CAST SPELL".to_owned());
                    actions.push("[BACKSPACE] SPELL LIST".to_owned());
                    actions.push("[ESC] CANCEL".to_owned());
                    dialog = Some(OverlayDialog {
                        title: spell.name().to_uppercase(),
                        body: format!(
                            "Target {}/{}: {}\n\n{}",
                            index % targets.len() + 1,
                            targets.len(),
                            spell_target_label(game, target),
                            spell.rules_summary()
                        ),
                        hint: "Legal targets pulse cyan. Click the figure, door, or highlighted tile; press Enter for the focused target."
                            .to_owned(),
                    });
                }
            }
        }
    } else if let Some(selection) = door_ui {
        let choices = game.adjacent_closed_door_indices().unwrap_or_default();
        if let Some(&door_index) = choices.get(selection.index % choices.len().max(1))
            && let Some(door) = game.doors.get(door_index)
        {
            actions.push("[LEFT] PREVIOUS DOOR".to_owned());
            actions.push("[RIGHT] NEXT DOOR".to_owned());
            actions.push("[ENTER] OPEN DOOR".to_owned());
            actions.push("[ESC] CANCEL".to_owned());
            dialog = Some(OverlayDialog {
                title: format!(
                    "OPEN A DOOR  {}/{}",
                    selection.index % choices.len() + 1,
                    choices.len()
                ),
                body: format!(
                    "Door between squares {},{} and {},{}.",
                    door.a.x + 1,
                    door.a.y + 1,
                    door.b.x + 1,
                    door.b.y + 1
                ),
                hint: "Cyan outlines mark each adjacent door's far-side square.".to_owned(),
            });
        }
    } else if choosing_jump_direction {
        actions.push("[W] UP".to_owned());
        actions.push("[D] RIGHT".to_owned());
        actions.push("[S] DOWN".to_owned());
        actions.push("[A] LEFT".to_owned());
        dialog = Some(OverlayDialog {
            title: "JUMP A TRAP".to_owned(),
            body: "Choose a direction containing a revealed trap and an empty landing square beyond it."
                .to_owned(),
            hint: "Press a direction now; any other key cancels.".to_owned(),
        });
    } else {
        match game.phase {
            GamePhase::HeroTurn { .. } | GamePhase::AllyTurn { .. } => {
                append_allowed_actions(game, &mut actions);
            }
            GamePhase::ZargonTurn => {}
            GamePhase::Won => {
                if campaign_enabled {
                    actions.push("[C] VISIT ARMORY / CONTINUE CAMPAIGN".to_owned());
                }
                actions.push("[ESC] LEAVE TABLE".to_owned());
                dialog = Some(OverlayDialog {
                    title: "QUEST COMPLETE".to_owned(),
                    body: "The surviving Heroes have fulfilled the quest objective. Their names, treasure, equipment, artifacts, potions, and completed quest number are now preserved on the campaign sheets."
                        .to_owned(),
                    hint: "Visit the Armory before beginning the next unlocked quest.".to_owned(),
                });
            }
            GamePhase::Retreated => {
                if campaign_enabled {
                    actions.push("[C] REPLAY UNFINISHED QUEST".to_owned());
                }
                actions.push("[ESC] LEAVE TABLE".to_owned());
                dialog = Some(OverlayDialog {
                    title: "QUEST ENDED EARLY".to_owned(),
                    body: "The surviving Heroes left by the stairway before completing the objective. No final reward or campaign completion was recorded."
                        .to_owned(),
                    hint: if campaign_enabled {
                        "Replay the unfinished quest when the party is ready.".to_owned()
                    } else {
                        "Press Esc to leave the table.".to_owned()
                    },
                });
            }
            GamePhase::Lost => {
                if campaign_enabled {
                    actions.push("[C] REPLAY QUEST".to_owned());
                }
                actions.push("[ESC] LEAVE TABLE".to_owned());
                dialog = Some(OverlayDialog {
                    title: "QUEST FAILED".to_owned(),
                    body: "No Hero remains able to continue the quest.".to_owned(),
                    hint: if campaign_enabled {
                        "Replay the unfinished quest when the party is ready.".to_owned()
                    } else {
                        "Press Esc to leave the table.".to_owned()
                    },
                });
            }
        }
    }

    GameOverlay {
        heading,
        status,
        message,
        actions,
        dialog,
    }
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn game_overlay(
    game: &Game,
    dice_animation: Option<&DiceAnimation>,
    choosing_jump_direction: bool,
    spell_ui: Option<SpellUi>,
    attack_ui: Option<AttackUi>,
    door_ui: Option<DoorUi>,
    possession_ui_index: usize,
    potion_ui: Option<PotionUi>,
    defense_potion_prompt: Option<AttackPlan>,
    share_ui: Option<ShareUi>,
    artifact_ui: Option<ArtifactUi>,
) -> GameOverlay {
    game_overlay_for_campaign(
        game,
        dice_animation,
        choosing_jump_direction,
        spell_ui,
        attack_ui,
        door_ui,
        possession_ui_index,
        potion_ui,
        defense_potion_prompt,
        share_ui,
        artifact_ui,
        false,
        true,
    )
}

fn append_allowed_actions(game: &Game, actions: &mut Vec<String>) {
    if game.hero_turn.movement_roll.is_none() {
        if game.hero_turn.orcs_bane_follow_up && game.active_attack_plan().is_ok() {
            actions.push("[F] ORC'S BANE SECOND ATTACK".to_owned());
            if game.can_voluntarily_retreat() {
                actions.push("[N] END QUEST AT STAIRWAY".to_owned());
            }
            actions.push("[E] FORGO SECOND ATTACK / END TURN".to_owned());
            return;
        }
        if game.hero_turn.heroic_brew_follow_up && game.active_attack_plan().is_ok() {
            actions.push("[F] HEROIC BREW SECOND ATTACK".to_owned());
            if game.can_voluntarily_retreat() {
                actions.push("[N] END QUEST AT STAIRWAY".to_owned());
            }
            actions.push("[E] FORGO SECOND ATTACK / END TURN".to_owned());
            return;
        }
        if !available_active_artifacts(game).is_empty() {
            actions.push("[Y] USE ARTIFACT".to_owned());
        }
        if active_hero_can_cast_spell(game) {
            actions.push("[M] CAST SPELL".to_owned());
        }
        if game.active_attack_plan().is_ok() {
            actions.push("[F] ATTACK".to_owned());
        }
        if !available_active_potions(game).is_empty() {
            actions.push("[I] POTION INVENTORY".to_owned());
        }
        if game
            .active_hero()
            .is_some_and(|hero| hero.inventory.gold > 0)
            && !game.living_other_heroes().is_empty()
        {
            actions.push("[Q] SHARE GOLD".to_owned());
        }
        if game.can_voluntarily_retreat() {
            actions.push("[N] END QUEST AT STAIRWAY".to_owned());
        }
        actions.push("[R] ROLL MOVEMENT".to_owned());
        actions.push("[E] END TURN".to_owned());
        return;
    }

    // Legal destinations themselves are the movement controls. Do not draw a
    // button-shaped MOVE hint that cannot choose a destination when clicked.
    if matches!(game.phase, GamePhase::HeroTurn { .. }) {
        if active_hero_has_closed_door(game) {
            actions.push("[O] OPEN DOOR".to_owned());
        }
        if game.active_attack_plan().is_ok() {
            actions.push("[F] ATTACK".to_owned());
        }
        if active_hero_can_cast_spell(game) {
            actions.push("[M] CAST SPELL".to_owned());
        }
        if Direction::ALL
            .into_iter()
            .any(|direction| game.active_jump_plan(direction).is_ok())
        {
            actions.push("[J] JUMP TRAP".to_owned());
        }
        if game.active_disarm_plan().is_ok() {
            actions.push("[X] DISARM TRAP".to_owned());
        }
        if !available_active_artifacts(game).is_empty() {
            actions.push("[Y] USE ARTIFACT".to_owned());
        }
        append_hero_interactions(game, actions);
        if !available_active_potions(game).is_empty() {
            actions.push("[I] POTION INVENTORY".to_owned());
        }
        if game
            .active_hero()
            .is_some_and(|hero| hero.inventory.gold > 0)
            && !game.living_other_heroes().is_empty()
        {
            actions.push("[Q] SHARE GOLD".to_owned());
        }
        if game.can_voluntarily_retreat() {
            actions.push("[N] END QUEST AT STAIRWAY".to_owned());
        }
    }
    actions.push("[E] END TURN".to_owned());
}

fn active_hero_can_cast_spell(game: &Game) -> bool {
    game.active_castable_hero_spells()
        .into_iter()
        .any(|spell| !game.valid_hero_spell_targets(spell).is_empty())
}

fn active_hero_has_closed_door(game: &Game) -> bool {
    let Some(hero) = game.active_hero() else {
        return false;
    };
    game.doors
        .iter()
        .any(|door| !door.open && (!door.secret || door.discovered) && door.touches(hero.pos))
}

fn append_hero_interactions(game: &Game, actions: &mut Vec<String>) {
    let Some(hero) = game.active_hero() else {
        return;
    };
    if hero.sleeping || hero.clouded || hero.petrified_turns > 0 {
        return;
    }
    let visible_monster = game.units.iter().any(|unit| {
        unit.alive
            && unit.faction == heroquest::model::Faction::Monster
            && !unit.dormant
            && game.is_visible(unit)
            && game.can_see(hero.pos, unit.pos)
    });
    if !game.hero_turn.action_used && !visible_monster {
        if hero.in_pit || game.cell(hero.pos).is_some_and(|cell| cell.region > 0) {
            actions.push("[T] SEARCH TREASURE".to_owned());
        }
        actions.push("[L] SEARCH SECRET DOORS".to_owned());
        actions.push("[P] SEARCH TRAPS".to_owned());
    }
    if hero.inventory.potion_of_healing > 0 && hero.body < hero.stats.body as i16 {
        actions.push("[C] DRINK HEALING POTION".to_owned());
    }
    if hero.inventory.petrification_potion > 0 && !game.hero_turn.action_used {
        actions.push("[B] DRINK PURPLE POTION".to_owned());
    }
    if game.can_take_fools_gold() {
        actions.push("[K] TAKE 5,000 GOLD FROM THE MINE".to_owned());
    }
    if hero.inventory.fools_gold > 0 {
        actions.push("[G] PUT DOWN THE MINE GOLD".to_owned());
    }
    if game.can_use_ring_of_return() {
        actions.push("[U] USE RING OF RETURN".to_owned());
    }
    if hero.carried_quest_item.is_some() {
        actions.push("[G] DROP QUEST ITEM".to_owned());
        let can_pass = game.units.iter().any(|unit| {
            unit.id != hero.id
                && unit.alive
                && !unit.escaped
                && matches!(unit.figure, heroquest::model::FigureKind::Hero(_))
                && unit.carried_quest_item.is_none()
                && hero.pos.is_adjacent(unit.pos)
                && game.boundary_is_open(hero.pos, unit.pos)
        });
        if can_pass {
            actions.push("[V] PASS QUEST ITEM".to_owned());
        }
    } else {
        let can_take = game.quest_items.iter().any(|item| {
            if item.holder.is_some() || item.delivered {
                return false;
            }
            let prop = &game.props[item.prop_index];
            prop.visible
                && (prop.pos == hero.pos
                    || (hero.pos.is_adjacent(prop.pos)
                        && game.boundary_is_open(hero.pos, prop.pos)))
        });
        if can_take {
            actions.push("[K] TAKE QUEST ITEM".to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AppEvent, ArtifactUi, AttackUi, DiceAnimation, DicePurpose, DoorUi, PotionUi, ShareUi,
        SheetNameEdit, SpellUi, adjacent_door_at_board_pos, apply_sheet_name_edit_overlay,
        apply_tabletop_overlay, commit_character_sheet_name, decode_app_event, dice_click_locked,
        game_overlay, game_overlay_for_campaign, overlay_action_key, queue_planned_movement,
        spell_target_at_board_pos,
    };
    use heroquest::campaign::Campaign;
    use heroquest::cards::{Artifact, ChaosSpell, HeroSpell};
    use heroquest::dice::DiceTray;
    use heroquest::equipment::Weapon;
    use heroquest::game::{
        AttackPlan, AttackSource, Game, GamePhase, HeroSpellTarget, PendingHeroDeath, PotionKind,
    };
    use heroquest::model::{Direction, Faction, FigureKind, HeroKind, MonsterKind, Pos};
    use heroquest::quest::QuestDefinition;
    use heroquest::renderer::{TabletopSurface, ZargonDeckKind};
    use heroquest::startup::StartupFlow;
    use sdl3::keyboard::Keycode;
    use sdl3::sys::{
        events::{SDL_EVENT_PINCH_UPDATE, SDL_Event, SDL_PinchFingerEvent},
        video::SDL_WindowID,
    };
    use std::{
        path::Path,
        time::{Duration, Instant},
    };

    #[test]
    fn native_sdl_pinch_scale_survives_raw_event_decoding() {
        let raw = SDL_Event {
            pinch: SDL_PinchFingerEvent {
                r#type: SDL_EVENT_PINCH_UPDATE,
                scale: 1.125,
                windowID: SDL_WindowID(42),
                ..Default::default()
            },
        };
        match decode_app_event(raw) {
            AppEvent::PinchUpdate { window_id, scale } => {
                assert_eq!(window_id, 42);
                assert_eq!(scale, 1.125);
            }
            _ => panic!("pinch update decoded as the wrong event type"),
        }
    }

    #[test]
    fn dice_are_visibly_picked_up_then_released_above_the_heroes_tabletop() {
        let mut animation = DiceAnimation::new(DiceTray::movement(7), DicePurpose::Movement);
        let started = animation.focus_started;
        let rack_poses = animation.poses();
        assert_eq!(rack_poses.len(), 2);
        animation
            .advance_physics(started + DiceAnimation::CAMERA_LEAD_IN - Duration::from_millis(1));
        assert!(!animation.rolling_started);
        let waiting_poses = animation.poses();
        assert_eq!(waiting_poses.len(), rack_poses.len());
        assert!(
            waiting_poses
                .iter()
                .zip(&rack_poses)
                .all(|(waiting, rack)| {
                    waiting.kind == rack.kind
                        && waiting.translation.distance(rack.translation) < f32::EPSILON
                        && waiting.rotation.dot(rack.rotation).abs() > 0.9999
                })
        );

        animation.advance_physics(
            started + DiceAnimation::CAMERA_LEAD_IN + DiceAnimation::PICKUP_DURATION / 2,
        );
        assert!(!animation.rolling_started);
        let carried_poses = animation.poses();
        assert!(
            carried_poses
                .iter()
                .zip(&rack_poses)
                .all(|(carried, rack)| carried.translation.y > rack.translation.y + 1.0)
        );

        animation.advance_physics(
            started + DiceAnimation::CAMERA_LEAD_IN + DiceAnimation::PICKUP_DURATION,
        );
        assert!(animation.rolling_started);
        assert_eq!(animation.poses().len(), 2);
    }

    #[test]
    fn settled_visible_dice_do_not_discard_a_highlighted_movement_click() {
        let mut animation = DiceAnimation::new(DiceTray::movement(11), DicePurpose::Movement);
        assert!(dice_click_locked(Some(&animation)));
        animation.result_applied_at = Some(Instant::now());
        assert!(!dice_click_locked(Some(&animation)));
        assert!(!dice_click_locked(None));
    }

    #[test]
    fn osd_action_buttons_dispatch_the_same_keys_as_the_keyboard() {
        assert_eq!(overlay_action_key("[R] ROLL MOVEMENT"), Some(Keycode::R));
        assert_eq!(overlay_action_key("[E] END TURN"), Some(Keycode::E));
        assert_eq!(
            overlay_action_key("[ENTER] ROLL NORMAL DEFENSE"),
            Some(Keycode::Return)
        );
        assert_eq!(
            overlay_action_key("[LEFT] PREVIOUS HERO"),
            Some(Keycode::Left)
        );
        assert_eq!(overlay_action_key("PLEASE WAIT"), None);
    }

    #[test]
    fn adjacent_enemy_exposes_attack_before_the_movement_roll() {
        let mut game = Game::demo(0x5052_4552_4f4c_4c).unwrap();
        let hero_pos = game.active_hero().unwrap().pos;
        let target = Direction::ALL
            .into_iter()
            .filter_map(|direction| hero_pos.offset(direction))
            .find(|&pos| game.boundary_is_open(hero_pos, pos))
            .unwrap();
        let monster = game
            .units
            .iter_mut()
            .find(|unit| unit.faction == Faction::Monster)
            .unwrap();
        monster.pos = target;
        monster.alive = true;
        monster.dormant = false;
        monster.hidden_until_activated = false;
        game.cells[Game::cell_index(target)].revealed = true;
        assert!(game.hero_turn.movement_roll.is_none());

        let overlay = game_overlay(
            &game, None, false, None, None, None, 0, None, None, None, None,
        );
        assert!(overlay.actions.iter().any(|action| action == "[F] ATTACK"));
        assert!(
            overlay
                .actions
                .iter()
                .any(|action| action == "[R] ROLL MOVEMENT")
        );
    }

    #[test]
    fn every_bracketed_osd_control_literal_has_a_click_dispatch_binding() {
        let source = include_str!("main.rs");
        let mut controls = 0;
        let mut quoted = source.split('"');
        while let Some(_) = quoted.next() {
            let Some(literal) = quoted.next() else {
                break;
            };
            if literal.starts_with('[') && literal.contains(']') {
                controls += 1;
                assert!(
                    overlay_action_key(literal).is_some(),
                    "OSD control looks clickable but has no dispatch binding: {literal}"
                );
            }
        }
        assert!(controls > 50);
    }

    #[test]
    fn clicking_a_distant_highlight_queues_the_entire_astar_route() {
        let mut game = Game::from_quest(
            QuestDefinition::original_us_return_to_barak_tor().unwrap(),
            0x434c_4943_4b,
        )
        .unwrap();
        game.apply_movement_roll(&[3, 4]).unwrap();
        let destination = Pos::new(4, 17);
        let mut planned = None;

        assert!(queue_planned_movement(&mut game, destination, &mut planned));
        let planned = planned.expect("click creates a planned route");
        assert_eq!(planned.destination, destination);
        assert_eq!(planned.steps.len(), 6);
    }

    #[test]
    fn spell_osd_names_the_scanned_card_and_clickable_legal_target() {
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
        let wizard_pos = game.unit(wizard).unwrap().pos;

        assert_eq!(
            spell_target_at_board_pos(&game, HeroSpell::SwiftWind, wizard_pos),
            Some(HeroSpellTarget::Unit(wizard))
        );
        let card_overlay = game_overlay(
            &game,
            None,
            false,
            Some(SpellUi::ChooseSpell { index: 0 }),
            None,
            None,
            0,
            None,
            None,
            None,
            None,
        );
        assert!(
            card_overlay
                .dialog
                .as_ref()
                .is_some_and(|dialog| dialog.title.contains("GENIE"))
        );

        let target_overlay = game_overlay(
            &game,
            None,
            false,
            Some(SpellUi::ChooseTarget {
                spell: HeroSpell::SwiftWind,
                index: 0,
            }),
            None,
            None,
            0,
            None,
            None,
            None,
            None,
        );
        assert!(target_overlay.dialog.as_ref().is_some_and(|dialog| {
            dialog.title == "SWIFT WIND"
                && dialog
                    .hint
                    .contains("Click the figure, door, or highlighted tile")
        }));
    }

    #[test]
    fn attack_osd_names_the_chosen_target_weapon_and_physical_dice() {
        let mut game = Game::from_quest(QuestDefinition::original_us_trial().unwrap(), 29).unwrap();
        game.cells.iter_mut().for_each(|cell| cell.revealed = true);
        game.doors.iter_mut().for_each(|door| door.open = true);
        let hero = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .pos = Pos::new(10, 10);
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .inventory
            .weapons
            .push(Weapon::Crossbow);
        let target = game
            .units
            .iter()
            .find(|unit| unit.faction == Faction::Monster)
            .unwrap()
            .id;
        game.units
            .iter_mut()
            .filter(|unit| unit.faction == Faction::Monster && unit.id != target)
            .for_each(|unit| unit.alive = false);
        let target_unit = game
            .units
            .iter_mut()
            .find(|unit| unit.id == target)
            .unwrap();
        target_unit.pos = Pos::new(12, 10);
        target_unit.dormant = false;
        target_unit.hidden_until_activated = false;
        target_unit.physical_figure = Some(target_unit.figure);
        let target_name = target_unit.name.clone();

        let overlay = game_overlay(
            &game,
            None,
            false,
            None,
            Some(AttackUi { index: 0 }),
            None,
            0,
            None,
            None,
            None,
            None,
        );
        assert!(overlay.dialog.as_ref().is_some_and(|dialog| {
            dialog.title.contains("CROSSBOW")
                && dialog.body.contains("Attack dice: 3")
                && dialog.body.contains(&target_name)
        }));
    }

    #[test]
    fn door_osd_and_click_target_choose_the_far_side_of_the_exact_door() {
        let mut game = Game::demo(29).unwrap();
        game.apply_movement_roll(&[6, 6]).unwrap();
        game.move_active(heroquest::model::Direction::East).unwrap();
        let choices = game.adjacent_closed_door_indices().unwrap();
        let door_index = choices[0];
        let hero_pos = game.active_hero().unwrap().pos;
        let target = game.doors[door_index].other_side(hero_pos).unwrap();

        assert_eq!(adjacent_door_at_board_pos(&game, target), Some(door_index));
        let overlay = game_overlay(
            &game,
            None,
            false,
            None,
            None,
            Some(DoorUi { index: 0 }),
            0,
            None,
            None,
            None,
            None,
        );
        assert!(overlay.dialog.as_ref().is_some_and(|dialog| {
            dialog.title.contains("OPEN A DOOR") && dialog.hint.contains("far-side square")
        }));
    }

    #[test]
    fn zero_body_osd_interrupts_play_with_the_exact_available_rescue_choices() {
        let mut game = Game::demo(31).unwrap();
        let hero = game.active_hero_id().unwrap();
        let unit = game.units.iter_mut().find(|unit| unit.id == hero).unwrap();
        unit.body = 0;
        unit.inventory.potion_of_healing = 1;
        unit.inventory.healing_potion_strengths.push(0);
        game.pending_hero_death = Some(PendingHeroDeath {
            hero,
            potion_roll_pending: false,
        });

        let overlay = game_overlay(
            &game, None, false, None, None, None, 0, None, None, None, None,
        );
        assert!(overlay.dialog.as_ref().is_some_and(|dialog| {
            dialog.title == "ZERO BODY POINTS"
                && dialog.body.contains("[C] Drink a Potion of Healing")
                && dialog.body.contains("[E] Accept death")
                && dialog.hint.contains("interrupts all movement")
        }));
    }

    #[test]
    fn potion_inventory_and_defense_interrupt_are_context_sensitive_modals() {
        let mut game = Game::demo(32).unwrap();
        let hero = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .find(|unit| unit.id == hero)
            .unwrap()
            .inventory
            .potion_of_defense = 1;
        let potion_overlay = game_overlay(
            &game,
            None,
            false,
            None,
            None,
            None,
            0,
            Some(PotionUi { index: 0 }),
            None,
            None,
            None,
        );
        assert!(potion_overlay.dialog.as_ref().is_some_and(|dialog| {
            dialog.title.contains("POTION OF DEFENSE")
                && dialog.body.contains("next defense")
                && dialog.hint.contains("combine another potion")
        }));

        let monster = game
            .units
            .iter()
            .find(|unit| unit.faction == Faction::Monster)
            .unwrap()
            .id;
        let plan = AttackPlan {
            attacker: monster,
            defender: hero,
            source: AttackSource::Natural,
            attack_dice: 3,
            defend_dice: 2,
        };
        let interrupt = game_overlay(
            &game,
            None,
            false,
            None,
            None,
            None,
            0,
            None,
            Some(plan),
            None,
            None,
        );
        assert!(interrupt.dialog.as_ref().is_some_and(|dialog| {
            dialog.title == "DEFENSE INTERRUPT"
                && dialog
                    .body
                    .contains("before throwing the physical defense dice")
                && dialog.hint.contains("Potion defense: 4 dice")
        }));
    }

    #[test]
    fn artifact_osd_explains_the_card_and_names_the_selected_fallen_hero() {
        let mut game = Game::demo(0x4152_5449_4641_4354).unwrap();
        let owner = game.active_hero_id().unwrap();
        let fallen = game.hero_order[1];
        game.units
            .iter_mut()
            .find(|unit| unit.id == owner)
            .unwrap()
            .inventory
            .artifacts
            .push(Artifact::ElixirOfLife);
        let fallen_unit = game
            .units
            .iter_mut()
            .find(|unit| unit.id == fallen)
            .unwrap();
        fallen_unit.alive = false;
        fallen_unit.body = 0;
        let fallen_name = fallen_unit.name.clone();

        let card = game_overlay(
            &game,
            None,
            false,
            None,
            None,
            None,
            0,
            None,
            None,
            None,
            Some(ArtifactUi::ChooseArtifact { index: 0 }),
        );
        assert!(card.dialog.as_ref().is_some_and(|dialog| {
            dialog.title.contains("ELIXIR OF LIFE")
                && dialog.body.contains("full Body and Mind Points")
        }));

        let target = game_overlay(
            &game,
            None,
            false,
            None,
            None,
            None,
            0,
            None,
            None,
            None,
            Some(ArtifactUi::ChooseElixirTarget { index: 0 }),
        );
        assert!(target.dialog.as_ref().is_some_and(|dialog| {
            dialog.title == "ELIXIR OF LIFE"
                && dialog.body.contains(&fallen_name)
                && dialog.hint.contains("discarded")
        }));
    }

    #[test]
    fn gift_modals_name_the_exact_potion_recipient_and_gold_amount() {
        let mut game = Game::demo(33).unwrap();
        let giver = game.active_hero_id().unwrap();
        game.units
            .iter_mut()
            .find(|unit| unit.id == giver)
            .unwrap()
            .inventory
            .gold = 250;
        let recipient = game.living_other_heroes()[0];
        let recipient_name = game.unit(recipient).unwrap().name.clone();

        let potion = game_overlay(
            &game,
            None,
            false,
            None,
            None,
            None,
            0,
            None,
            None,
            Some(ShareUi::Potion {
                potion: PotionKind::Defense,
                index: 0,
            }),
            None,
        );
        assert!(potion.dialog.as_ref().is_some_and(|dialog| {
            dialog.title == "GIVE A POTION"
                && dialog.body.contains("Potion of Defense")
                && dialog.body.contains(&recipient_name)
                && dialog.hint.contains("only during the giver's Hero turn")
        }));

        let gold = game_overlay(
            &game,
            None,
            false,
            None,
            None,
            None,
            0,
            None,
            None,
            Some(ShareUi::Gold {
                index: 0,
                amount: 125,
            }),
            None,
        );
        assert!(gold.dialog.as_ref().is_some_and(|dialog| {
            dialog.title == "SHARE GOLD"
                && dialog.body.contains("125 gold coins")
                && dialog.body.contains(&recipient_name)
        }));
    }

    #[test]
    fn physical_character_sheet_pages_show_live_state_and_owned_cards() {
        let mut game = Game::demo(0x5348_4545_54).unwrap();
        let barbarian = game
            .units
            .iter_mut()
            .find(|unit| unit.figure == FigureKind::Hero(HeroKind::Barbarian))
            .unwrap();
        barbarian.body = 5;
        barbarian.inventory.gold = 135;
        barbarian.inventory.artifacts.push(Artifact::TalismanOfLore);
        barbarian.inventory.potion_of_healing = 2;

        let mut overlay = game_overlay(
            &game, None, false, None, None, None, 0, None, None, None, None,
        );
        apply_tabletop_overlay(
            &mut overlay,
            &game,
            TabletopSurface::CharacterSheet(HeroKind::Barbarian),
            0,
        );
        assert!(overlay.dialog.as_ref().is_some_and(|dialog| {
            dialog.title.contains("1/5")
                && dialog.body.contains("Body: 5/8")
                && dialog.body.contains("Mind: 3")
                && dialog.body.contains("Gold: 135")
        }));
        assert!(
            overlay
                .actions
                .iter()
                .any(|action| action.contains("NEXT PAGE"))
        );
        assert!(
            overlay
                .actions
                .iter()
                .any(|action| action == "[N] EDIT HERO NAME")
        );

        apply_tabletop_overlay(
            &mut overlay,
            &game,
            TabletopSurface::CharacterSheet(HeroKind::Barbarian),
            2,
        );
        assert!(overlay.dialog.as_ref().is_some_and(|dialog| {
            dialog.title.contains("3/5")
                && dialog.body.contains("Talisman of Lore")
                && dialog.body.contains("Healing x2")
        }));

        apply_tabletop_overlay(
            &mut overlay,
            &game,
            TabletopSurface::CharacterSheet(HeroKind::Barbarian),
            3,
        );
        assert!(overlay.dialog.as_ref().is_some_and(|dialog| {
            dialog.title.contains("4/5")
                && dialog.body == game.blurb
                && dialog.hint.contains("secret map notes remain hidden")
        }));

        apply_tabletop_overlay(
            &mut overlay,
            &game,
            TabletopSurface::CharacterSheet(HeroKind::Barbarian),
            4,
        );
        assert!(overlay.dialog.as_ref().is_some_and(|dialog| {
            dialog.title.contains("5/5") && dialog.hint.contains("public events")
        }));
    }

    #[test]
    fn physical_character_sheet_name_editor_updates_all_live_records() {
        let mut game = Game::demo(0x4e41_4d45).unwrap();
        let mut campaign = Campaign::default();
        let mut startup = StartupFlow::default();
        let saved = commit_character_sheet_name(
            &mut game,
            &mut campaign,
            &mut startup,
            Path::new("unused-when-campaign-is-disabled.json"),
            false,
            HeroKind::Barbarian,
            "  Conan  ",
        )
        .unwrap();
        assert_eq!(saved, "Conan");
        assert_eq!(campaign.heroes[0].name, "Conan");
        assert_eq!(startup.heroes[0].hero_name, "Conan");
        assert!(game.units.iter().any(|unit| {
            unit.figure == FigureKind::Hero(HeroKind::Barbarian) && unit.name == "Conan"
        }));

        let mut overlay = game_overlay(
            &game, None, false, None, None, None, 0, None, None, None, None,
        );
        apply_sheet_name_edit_overlay(
            &mut overlay,
            &SheetNameEdit {
                hero: HeroKind::Barbarian,
                draft: "Conan".to_owned(),
            },
        );
        assert!(overlay.dialog.as_ref().is_some_and(|dialog| {
            dialog.title.contains("EDIT NAME") && dialog.body.contains("Conan_")
        }));
        assert_eq!(
            overlay.actions,
            ["[ENTER] SAVE NAME", "[BACKSPACE] DELETE", "[ESC] CANCEL"]
        );
    }

    #[test]
    fn tabletop_cards_enforce_hidden_information_and_cast_from_the_physical_spell() {
        let mut game = Game::demo(0x4341_5244).unwrap();
        let setup = StartupFlow::default();
        game.apply_hero_setup(&setup.heroes, &setup.wizard_spells(), setup.elf_spells);
        let wizard_order = game
            .hero_order
            .iter()
            .position(|&id| {
                game.unit(id)
                    .is_some_and(|unit| unit.figure == FigureKind::Hero(HeroKind::Wizard))
            })
            .unwrap();
        game.phase = GamePhase::HeroTurn {
            order_index: wizard_order,
        };

        let mut overlay = game_overlay(
            &game, None, false, None, None, None, 0, None, None, None, None,
        );
        apply_tabletop_overlay(
            &mut overlay,
            &game,
            TabletopSurface::HeroSpellCard {
                hero: HeroKind::Wizard,
                spell: HeroSpell::Genie,
            },
            0,
        );
        assert!(
            overlay
                .actions
                .iter()
                .any(|action| action.contains("CAST THIS SPELL"))
        );

        apply_tabletop_overlay(
            &mut overlay,
            &game,
            TabletopSurface::ZargonDeck(ZargonDeckKind::ChaosSpell),
            0,
        );
        assert!(overlay.dialog.as_ref().is_some_and(|dialog| {
            dialog.title.contains("ZARGON ONLY")
                && dialog.body.contains("secret")
                && !dialog.body.contains("Ball of Flame")
        }));

        apply_tabletop_overlay(
            &mut overlay,
            &game,
            TabletopSurface::MonsterCard(MonsterKind::Gargoyle),
            0,
        );
        assert!(overlay.dialog.as_ref().is_some_and(|dialog| {
            dialog.title.contains("GARGOYLE")
                && dialog.body.contains("attack 4")
                && dialog.body.contains("Body Points 3")
        }));
    }

    #[test]
    fn physical_spell_discards_are_face_up_public_and_never_castable() {
        let mut game = Game::demo(0x4449_5343_4152_44).unwrap();
        let wizard = game
            .units
            .iter_mut()
            .find(|unit| unit.figure == FigureKind::Hero(HeroKind::Wizard))
            .unwrap();
        wizard.discarded_hero_spells = vec![HeroSpell::Genie, HeroSpell::RockSkin];
        game.discarded_chaos_spells = vec![ChaosSpell::Fear, ChaosSpell::Tempest];
        let mut overlay = game_overlay(
            &game, None, false, None, None, None, 0, None, None, None, None,
        );

        apply_tabletop_overlay(
            &mut overlay,
            &game,
            TabletopSurface::HeroSpellDiscard {
                hero: HeroKind::Wizard,
                spell: HeroSpell::RockSkin,
                count: 2,
            },
            0,
        );
        assert!(overlay.dialog.as_ref().is_some_and(|dialog| {
            dialog.title.contains("ROCK SKIN")
                && dialog.body.contains("2 used spell cards")
                && dialog.hint.contains("cannot be cast again")
        }));
        assert!(
            overlay
                .actions
                .iter()
                .all(|action| !action.contains("CAST"))
        );

        apply_tabletop_overlay(
            &mut overlay,
            &game,
            TabletopSurface::ChaosSpellDiscard {
                spell: ChaosSpell::Tempest,
                count: 2,
            },
            0,
        );
        assert!(overlay.dialog.as_ref().is_some_and(|dialog| {
            dialog.title.contains("TEMPEST")
                && dialog.body.contains("2 revealed Chaos Spell cards")
                && dialog.hint.contains("uncast cards")
        }));
    }

    #[test]
    fn voluntary_retreat_has_a_confirmation_and_replay_flow_without_armory() {
        let mut game = Game::demo(0x5245_5452_4541_54).unwrap();
        let stair = game.stairs[0];
        for hero in game.hero_order.clone() {
            game.units
                .iter_mut()
                .find(|unit| unit.id == hero)
                .unwrap()
                .pos = stair;
        }

        let confirm = game_overlay_for_campaign(
            &game, None, false, None, None, None, 0, None, None, None, None, true, true,
        );
        assert_eq!(
            confirm.dialog.as_ref().map(|dialog| dialog.title.as_str()),
            Some("END QUEST EARLY?")
        );
        assert!(
            confirm
                .actions
                .iter()
                .any(|action| action.starts_with("[ENTER]"))
        );
        assert!(
            confirm
                .dialog
                .as_ref()
                .is_some_and(|dialog| dialog.body.contains("no final quest reward"))
        );

        game.voluntarily_retreat().unwrap();
        let ended = game_overlay_for_campaign(
            &game, None, false, None, None, None, 0, None, None, None, None, false, true,
        );
        assert!(ended.actions.iter().any(|action| action.contains("REPLAY")));
        assert!(!ended.actions.iter().any(|action| action.contains("ARMORY")));
        assert!(
            ended
                .dialog
                .as_ref()
                .is_some_and(|dialog| dialog.body.contains("No final reward"))
        );

        game.phase = GamePhase::Won;
        let won = game_overlay_for_campaign(
            &game, None, false, None, None, None, 0, None, None, None, None, false, true,
        );
        let continue_action = won
            .actions
            .iter()
            .find(|action| action.contains("VISIT ARMORY"))
            .expect("a completed campaign quest must expose the in-game handoff");
        assert_eq!(overlay_action_key(continue_action), Some(Keycode::C));
        assert!(won.dialog.as_ref().is_some_and(|dialog| {
            dialog.title == "QUEST COMPLETE" && dialog.hint.contains("next unlocked quest")
        }));
    }
}
