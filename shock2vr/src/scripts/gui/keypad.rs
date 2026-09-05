use cgmath::{Vector2, Vector3, vec2};
use dark::properties::{ObjectState, PropHackDiff, PropKeypadCode, PropObjState, PropTemplateId};
use engine::audio::AudioHandle;

use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::gui::{self, ButtonHoverBehavior, Gui, GuiComponent, GuiConfig, GuiCursor};
use crate::{mission::GlobalHrmParams, player_stats::Skill, quest_info::QuestInfo, time::Time};

use crate::scripts::{Effect, MessagePayload, script_util::*};

pub struct KeyPadGui;

#[derive(Clone, Debug, Default)]
pub struct KeyPadState {
    current_value: Option<u32>,
    hack: HackState,
}

#[derive(Clone)]
pub enum KeyPadMsg {
    ButtonPressed(u32),
    Clear,
    StartHack,
    PlayNode { x: usize, y: usize },
}

const BOARD_WIDTH: usize = 5;
const BOARD_HEIGHT: usize = 4;
const BOARD_X: f32 = 16.0;
const BOARD_Y: f32 = 48.0;
const BOARD_DX: f32 = 30.0;
const BOARD_DY: f32 = 36.0;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum HackPhase {
    #[default]
    Unpaid,
    Playing,
    Won,
    Lost,
    Unwinnable,
    InsufficientNanites,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum HackNode {
    #[default]
    Free,
    Lit,
    Burned,
    Mine,
    Empty,
}

#[derive(Clone, Debug)]
pub(crate) struct HackState {
    pub(crate) phase: HackPhase,
    pub(crate) nodes: [HackNode; BOARD_WIDTH * BOARD_HEIGHT],
    pub(crate) rng_state: u64,
}

impl Default for HackState {
    fn default() -> Self {
        Self {
            phase: HackPhase::Unpaid,
            nodes: base_hack_board(),
            rng_state: 0,
        }
    }
}

pub(crate) const fn board_index(x: usize, y: usize) -> usize {
    y * BOARD_WIDTH + x
}

pub(crate) const fn base_hack_board() -> [HackNode; BOARD_WIDTH * BOARD_HEIGHT] {
    use HackNode::{Empty as E, Free as F};
    [
        E, E, F, F, F, //
        F, F, F, E, F, //
        F, E, F, F, F, //
        F, F, F, E, E,
    ]
}

fn next_random(rng_state: &mut u64, upper_exclusive: u32) -> u32 {
    debug_assert!(upper_exclusive > 0);
    let mut value = if *rng_state == 0 {
        0x9e37_79b9_7f4a_7c15
    } else {
        *rng_state
    };
    value ^= value << 13;
    value ^= value >> 7;
    value ^= value << 17;
    *rng_state = value;
    (value % u64::from(upper_exclusive)) as u32
}

fn hack_seed(world: &World, entity_id: EntityId) -> u64 {
    // Simulation time makes ordinary play vary with the moment START is
    // pressed, while fixed-step replay requests produce the same board. Mix in
    // the stable authored object id rather than the run-local Shipyard id.
    let time = world
        .borrow::<UniqueView<Time>>()
        .map(|time| time.total.as_nanos() as u64)
        .unwrap_or_default();
    let stable_id = world
        .borrow::<View<PropTemplateId>>()
        .ok()
        .and_then(|templates| {
            templates
                .get(entity_id)
                .ok()
                .map(|template| template.template_id as u64)
        })
        .unwrap_or_else(|| entity_id.inner());
    mix_hack_seed(time, stable_id)
}

fn mix_hack_seed(time_nanoseconds: u64, stable_id: u64) -> u64 {
    time_nanoseconds ^ stable_id.rotate_left(17) ^ 0x1ac1_d4b1
}

fn board_with_mines(
    mine_count: i32,
    rng_state: &mut u64,
) -> [HackNode; BOARD_WIDTH * BOARD_HEIGHT] {
    let mut nodes = base_hack_board();
    let mut free: Vec<_> = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| (*node == HackNode::Free).then_some(index))
        .collect();
    for _ in 0..mine_count.max(0) as usize {
        if free.is_empty() {
            break;
        }
        let selected = next_random(rng_state, free.len() as u32) as usize;
        nodes[free.remove(selected)] = HackNode::Mine;
    }
    nodes
}

/// An object's persistent `P$ObjState`, defaulting to `Normal`. Every hackable
/// object keys its post-hack behavior off this (the replicator's hacked
/// inventory, the crate's opened lid), and it is a registered Dark property, so
/// the result survives save/load.
pub(crate) fn object_state(world: &World, entity_id: EntityId) -> ObjectState {
    world
        .borrow::<View<PropObjState>>()
        .ok()
        .and_then(|states| states.get(entity_id).ok().map(|state| state.0))
        .unwrap_or(ObjectState::Normal)
}

/// An object's authored `P$HackDiff` - the terms this object is hacked on.
pub(crate) fn hack_diff(world: &World, entity_id: EntityId) -> Option<PropHackDiff> {
    world
        .borrow::<View<PropHackDiff>>()
        .ok()
        .and_then(|diffs| diffs.get(entity_id).ok().copied())
}

fn hack_cost(diff: PropHackDiff) -> i32 {
    (diff.cost as i32).max(1)
}

fn outcome_roll(rng_state: &mut u64) -> i32 {
    next_random(rng_state, 100) as i32
}

fn roll_succeeds(roll: i32, chance: i32) -> bool {
    roll < chance
}

/// Which operation the board is running. One board, one set of odds; the mode
/// only picks the tech skill those odds are computed against and what a win or
/// a critical failure does to the object.
#[derive(Clone, Copy, Debug)]
pub(crate) enum HrmMode {
    Hack,
    Repair,
    Modify,
}

impl HrmMode {
    /// The tech level this mode is played at. Repair and modify count their
    /// installed software on top of the trained skill; the hack mode does not
    /// consult its own software yet. This is the single definition of "how
    /// good is the player at this", so a mode's entry requirement and its
    /// odds can never be judged against different numbers.
    pub(crate) fn player_level(self, world: &World) -> i32 {
        world
            .borrow::<UniqueView<QuestInfo>>()
            .ok()
            .map(|quest| {
                let stats = quest.player_stats();
                match self {
                    HrmMode::Hack => stats.skill_level(Skill::Hack),
                    HrmMode::Repair => stats.skill_level(Skill::Repair) + stats.software.repair,
                    HrmMode::Modify => stats.skill_level(Skill::Modify) + stats.software.modify,
                }
            })
            .unwrap_or(0)
    }

    /// The art this mode's board wears. Retail keeps one board and swaps the
    /// pictures: the backdrop is named after the mode, and the result overlays
    /// share a base name with a per-mode suffix.
    pub(crate) const fn art(self) -> HrmArt {
        match self {
            HrmMode::Hack => HrmArt {
                backdrop: "hack.pcx",
                won: "winh.pcx",
                lost: "loseh.pcx",
                unwinnable: "failh.pcx",
                unpaid: "payh.pcx",
            },
            HrmMode::Repair => HrmArt {
                // Archive-qualified: `repair.pcx` is one of the seven basenames
                // the interface art shares with a model texture, and the model
                // mounts first, so the plain key resolves to the wrong picture.
                backdrop: "iface/repair.pcx",
                won: "winr.pcx",
                lost: "loser.pcx",
                unwinnable: "failr.pcx",
                unpaid: "payr.pcx",
            },
            HrmMode::Modify => HrmArt {
                backdrop: "modify.pcx",
                won: "winm.pcx",
                lost: "losem.pcx",
                unwinnable: "failm.pcx",
                unpaid: "paym.pcx",
            },
        }
    }

    /// Position in the mode-indexed tables, which is also the order the modes
    /// are numbered in the shipped data.
    pub(crate) const fn index(self) -> usize {
        match self {
            HrmMode::Hack => 0,
            HrmMode::Repair => 1,
            HrmMode::Modify => 2,
        }
    }

    /// The HRM.STR key prefix; result keys are `<prefix>result<n>`.
    pub(crate) const fn string_prefix(self) -> &'static str {
        match self {
            HrmMode::Hack => "hack",
            HrmMode::Repair => "repair",
            HrmMode::Modify => "modify",
        }
    }
}

/// The five pictures a board draws: its backdrop, plus the overlay that covers
/// the matrix once the board has stopped being playable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HrmArt {
    pub(crate) backdrop: &'static str,
    pub(crate) won: &'static str,
    pub(crate) lost: &'static str,
    pub(crate) unwinnable: &'static str,
    pub(crate) unpaid: &'static str,
}

/// A finished board's outcome, in the numbering the HRM.STR result keys use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HrmResult {
    /// The board ran out of playable squares: the attempt is over, nothing
    /// happened to the object.
    PlayedOut,
    Won,
    Lost,
}

impl HrmResult {
    pub(crate) const fn index(self) -> usize {
        match self {
            HrmResult::PlayedOut => 0,
            HrmResult::Won => 1,
            HrmResult::Lost => 2,
        }
    }
}

/// The status line a finished board posts, in the mode's own words.
fn result_message(world: &World, mode: HrmMode, result: HrmResult) -> Effect {
    Effect::ShowMessage {
        text: crate::hud::hud_strings(world)
            .hrm_result(mode, result)
            .to_owned(),
    }
}

fn effective_hack_values(world: &World, diff: PropHackDiff, mode: HrmMode) -> (i32, i32) {
    // Test hook: a board of nothing but mines that no node can survive, so the
    // critical-failure branch is reachable headlessly.
    if crate::dev_params::get_bool(crate::dev_params::HRM_FORCE_CRITICAL) {
        return (0, (BOARD_WIDTH * BOARD_HEIGHT) as i32);
    }
    let skill = mode.player_level(world);
    let stat = world
        .borrow::<UniqueView<QuestInfo>>()
        .ok()
        .map(|quest| quest.player_stats().cyber_affinity)
        .unwrap_or(0);
    world
        .borrow::<UniqueView<GlobalHrmParams>>()
        .ok()
        .and_then(|params| params.0.clone())
        .map(|params| {
            (
                params.success_chance(diff.success_chance, skill, stat),
                params.mine_count(diff.critical_chance, skill, stat),
            )
        })
        .unwrap_or((
            diff.success_chance.clamp(0, 85),
            diff.critical_chance.max(0),
        ))
}

fn has_connected_three(nodes: &[HackNode; BOARD_WIDTH * BOARD_HEIGHT]) -> bool {
    (0..BOARD_HEIGHT).any(|y| {
        (0..=BOARD_WIDTH - 3)
            .any(|x| (0..3).all(|offset| nodes[board_index(x + offset, y)] == HackNode::Lit))
    }) || (0..BOARD_WIDTH).any(|x| {
        (0..=BOARD_HEIGHT - 3)
            .any(|y| (0..3).all(|offset| nodes[board_index(x, y + offset)] == HackNode::Lit))
    })
}

fn board_has_potential_path(nodes: &[HackNode; BOARD_WIDTH * BOARD_HEIGHT]) -> bool {
    let can_contribute = |node| matches!(node, HackNode::Lit | HackNode::Free | HackNode::Mine);
    (0..BOARD_HEIGHT).any(|y| {
        (0..=BOARD_WIDTH - 3)
            .any(|x| (0..3).all(|offset| can_contribute(nodes[board_index(x + offset, y)])))
    }) || (0..BOARD_WIDTH).any(|x| {
        (0..=BOARD_HEIGHT - 3)
            .any(|y| (0..3).all(|offset| can_contribute(nodes[board_index(x, y + offset)])))
    })
}

/// Numeric keypad codes take precedence over the inherited tech difficulty.
/// Retail numeric keypads inherit from the same archetype as hack-only panels,
/// so `P$HackDiff` alone does not distinguish the two modes.
fn hack_diff_for_entity(world: &World, entity_id: EntityId) -> Option<PropHackDiff> {
    let has_numeric_code = world
        .borrow::<View<PropKeypadCode>>()
        .is_ok_and(|view| view.get(entity_id).is_ok());
    if has_numeric_code {
        return None;
    }
    world
        .borrow::<View<PropHackDiff>>()
        .ok()
        .and_then(|view| view.get(entity_id).ok().copied())
}

fn get_texture_for_char(char: char) -> String {
    format!("key{}0.pcx", char)
}

fn draw_number(num: u32) -> Vec<GuiComponent<KeyPadMsg>> {
    let num_str = num.to_string();

    let offset_left = 10.0;
    let offset_top = 10.0;
    let padding = 1.5;
    let mut x = 0.0;
    let numeral_width = 22.5;
    let numeral_height = 30.0;

    let reversed_chars: Vec<char> = num_str.chars().collect();

    let mut ret = Vec::new();
    for ch in reversed_chars {
        ret.push(GuiComponent::Image {
            position: vec2(x + offset_left, offset_top),
            size: vec2(numeral_width, numeral_height),
            texture: get_texture_for_char(ch),
            alpha: 0.5,
            kind: crate::ui::ImageKind::Ui,
        });
        x += numeral_width + padding;
    }
    ret
}

pub(crate) fn draw_hack_board<TMsg, F>(
    state: &HackState,
    diff: PropHackDiff,
    mode: HrmMode,
    wrap: F,
) -> Vec<GuiComponent<TMsg>>
where
    TMsg: Clone,
    F: Fn(KeyPadMsg) -> TMsg + Copy,
{
    let art = mode.art();
    let mut components = vec![
        gui::image(art.backdrop)
            .with_position(vec2(0.0, 0.0))
            .with_size(vec2(188.0, 296.0)),
    ];

    // Connected bars sit behind their lit endpoints, like retail DrawBoard.
    for y in 0..BOARD_HEIGHT {
        for x in 0..BOARD_WIDTH {
            if state.nodes[board_index(x, y)] != HackNode::Lit {
                continue;
            }
            let position = vec2(BOARD_X + x as f32 * BOARD_DX, BOARD_Y + y as f32 * BOARD_DY);
            if x + 1 < BOARD_WIDTH && state.nodes[board_index(x + 1, y)] == HackNode::Lit {
                components.push(
                    gui::image("hrmbarh.pcx")
                        .with_position(position)
                        .with_size(vec2(30.0, 16.0)),
                );
            }
            if y + 1 < BOARD_HEIGHT && state.nodes[board_index(x, y + 1)] == HackNode::Lit {
                components.push(
                    gui::image("hrmbarv.pcx")
                        .with_position(position)
                        .with_size(vec2(16.0, 36.0)),
                );
            }
        }
    }

    for y in 0..BOARD_HEIGHT {
        for x in 0..BOARD_WIDTH {
            let node = state.nodes[board_index(x, y)];
            if node == HackNode::Empty {
                continue;
            }
            let position = vec2(BOARD_X + x as f32 * BOARD_DX, BOARD_Y + y as f32 * BOARD_DY);
            let texture = match node {
                HackNode::Lit => Some("hrmon.pcx"),
                HackNode::Burned => Some("hrmburn.pcx"),
                HackNode::Mine => Some("hrmmine.pcx"),
                HackNode::Free | HackNode::Empty => None,
            };
            if let Some(texture) = texture {
                components.push(
                    gui::image(texture)
                        .with_position(position)
                        .with_size(vec2(16.0, 16.0)),
                );
            }
            // The unlit node outline is already part of the backdrop. This
            // zero-alpha button supplies a normal 16x16 hit target without
            // painting a placeholder over that authored art.
            components.push(
                gui::button(wrap(KeyPadMsg::PlayNode { x, y }))
                    .with_position(position)
                    .with_size(vec2(16.0, 16.0))
                    .with_image("hrmpip.pcx")
                    .with_alpha(0.0)
                    .with_label(&format!("node-{x}-{y}")),
            );
        }
    }

    let result_texture = match state.phase {
        HackPhase::Won => Some(art.won),
        HackPhase::Lost => Some(art.lost),
        HackPhase::Unwinnable => Some(art.unwinnable),
        HackPhase::InsufficientNanites => Some(art.unpaid),
        HackPhase::Unpaid | HackPhase::Playing => None,
    };
    if let Some(texture) = result_texture {
        components.push(
            gui::image(texture)
                .with_position(vec2(13.0, 42.0))
                .with_size(vec2(142.0, 134.0)),
        );
    }

    // The backdrop already supplies the cyan `COST:` label. Retail draws only
    // the dynamic numeric value in the 48px slot beginning at x=128, after the
    // result overlay so the win/lose/pay art cannot obscure it.
    components.push(
        gui::text(&hack_cost(diff).to_string())
            .with_position(vec2(147.0, 158.0))
            .with_size(vec2(29.0, 18.0)),
    );

    if !matches!(state.phase, HackPhase::Won | HackPhase::Lost) {
        let (normal, hover, label) =
            if matches!(state.phase, HackPhase::Playing | HackPhase::Unwinnable) {
                ("reset0.pcx", "reset1.pcx", "reset-hack")
            } else {
                ("start0.pcx", "start1.pcx", "start-hack")
            };
        components.push(
            gui::button(wrap(KeyPadMsg::StartHack))
                .with_position(vec2(157.0, 232.0))
                .with_size(vec2(20.0, 56.0))
                .with_image(normal)
                .with_hover(ButtonHoverBehavior::Texture(hover.to_owned()))
                .with_label(label),
        );
    }
    components
}

pub(crate) struct HackOutcomeEffects {
    pub(crate) success: fn(EntityId, &World) -> Effect,
    pub(crate) critical_failure: fn(EntityId, &World) -> Effect,
}

pub(crate) fn handle_hack_msg(
    entity_id: EntityId,
    world: &World,
    state: &HackState,
    msg: &KeyPadMsg,
    diff: PropHackDiff,
    mode: HrmMode,
    outcomes: HackOutcomeEffects,
) -> (HackState, Effect) {
    let mut new_state = state.clone();
    match msg {
        KeyPadMsg::StartHack if !matches!(new_state.phase, HackPhase::Won | HackPhase::Lost) => {
            let cost = hack_cost(diff);
            let Some(payment) = spend_player_nanites(world, cost) else {
                new_state.phase = HackPhase::InsufficientNanites;
                return (
                    new_state,
                    Effect::PlaySound {
                        handle: AudioHandle::new(),
                        source: Some(entity_id),
                        name: "login".to_owned(),
                        spatial: false,
                    },
                );
            };
            let (_, mine_count) = effective_hack_values(world, diff, mode);
            let mut rng_state = hack_seed(world, entity_id);
            tracing::debug!(entity = entity_id.inner(), rng_state, "HRM rng seed");
            let nodes = board_with_mines(mine_count, &mut rng_state);
            tracing::debug!(
                entity = entity_id.inner(),
                rng_state,
                "HRM rng initialized after mine placement"
            );
            new_state = HackState {
                phase: HackPhase::Playing,
                nodes,
                rng_state,
            };
            (
                new_state,
                Effect::combine(vec![
                    payment,
                    Effect::PlaySound {
                        handle: AudioHandle::new(),
                        source: Some(entity_id),
                        name: "start_hack".to_owned(),
                        spatial: false,
                    },
                ]),
            )
        }
        KeyPadMsg::PlayNode { x, y }
            if *x < BOARD_WIDTH && *y < BOARD_HEIGHT && new_state.phase == HackPhase::Playing =>
        {
            let index = board_index(*x, *y);
            let node = new_state.nodes[index];
            if !matches!(node, HackNode::Free | HackNode::Mine) {
                return (
                    new_state,
                    Effect::PlaySound {
                        handle: AudioHandle::new(),
                        source: Some(entity_id),
                        name: "login".to_owned(),
                        spatial: false,
                    },
                );
            }

            let was_mine = node == HackNode::Mine;
            let mut played_out = false;
            let roll = outcome_roll(&mut new_state.rng_state);
            tracing::debug!(
                entity = entity_id.inner(),
                roll,
                rng_state = new_state.rng_state,
                "HRM rng outcome"
            );
            let (chance, _) = effective_hack_values(world, diff, mode);
            if roll_succeeds(roll, chance) {
                new_state.nodes[index] = HackNode::Lit;
                if has_connected_three(&new_state.nodes) {
                    new_state.phase = HackPhase::Won;
                    return (
                        new_state,
                        Effect::combine(vec![
                            (outcomes.success)(entity_id, world),
                            Effect::PlaySound {
                                handle: AudioHandle::new(),
                                source: Some(entity_id),
                                name: "hack_success".to_owned(),
                                spatial: false,
                            },
                            result_message(world, mode, HrmResult::Won),
                        ]),
                    );
                }
            } else if was_mine {
                new_state.phase = HackPhase::Lost;
                return (
                    new_state,
                    Effect::combine(vec![
                        (outcomes.critical_failure)(entity_id, world),
                        Effect::PlaySound {
                            handle: AudioHandle::new(),
                            source: Some(entity_id),
                            name: "hack_critical".to_owned(),
                            spatial: false,
                        },
                        result_message(world, mode, HrmResult::Lost),
                    ]),
                );
            } else {
                new_state.nodes[index] = HackNode::Burned;
                if !board_has_potential_path(&new_state.nodes) {
                    new_state.phase = HackPhase::Unwinnable;
                    played_out = true;
                }
            }

            let mut effects = vec![Effect::PlaySound {
                handle: AudioHandle::new(),
                source: Some(entity_id),
                name: "hacking".to_owned(),
                spatial: false,
            }];
            if played_out {
                effects.push(result_message(world, mode, HrmResult::PlayedOut));
            }
            (new_state, Effect::combine(effects))
        }
        _ => (new_state, Effect::NoEffect),
    }
}

fn keypad_hack_success(entity_id: EntityId, world: &World) -> Effect {
    send_to_all_switch_links_and_self(world, entity_id, MessagePayload::TurnOn { from: entity_id })
}

fn keypad_hack_critical_failure(_entity_id: EntityId, _world: &World) -> Effect {
    Effect::NoEffect
}

impl Gui<KeyPadState, KeyPadMsg> for KeyPadGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        _entity_id: EntityId,
        _world: &World,
        _state: &KeyPadState,
    ) -> Vec<GuiComponent<KeyPadMsg>> {
        let hack_diff = hack_diff_for_entity(_world, _entity_id);
        if let Some(hack_diff) = hack_diff {
            return draw_hack_board(&_state.hack, hack_diff, HrmMode::Hack, |msg| msg);
        }

        let button_width = 45.0;
        let button_height = 60.0;
        let left_margin = 15.0;
        let top_margin = 42.0;
        let padding = 1.5;

        let mut components: Vec<GuiComponent<KeyPadMsg>> = vec![
            gui::image("keypad2.pcx")
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(188.0, 296.0)),
            // First row of buttons
            gui::button(KeyPadMsg::ButtonPressed(1))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 0.0,
                    top_margin,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key10.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key11.pcx".to_owned())),
            gui::button(KeyPadMsg::ButtonPressed(2))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 1.0,
                    top_margin,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key20.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key21.pcx".to_owned())),
            gui::button(KeyPadMsg::ButtonPressed(3))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 2.0,
                    top_margin,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key30.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key31.pcx".to_owned())),
            // Second row of buttons
            gui::button(KeyPadMsg::ButtonPressed(4))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 0.0,
                    top_margin + (button_height + padding) * 1.0,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key40.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key41.pcx".to_owned())),
            gui::button(KeyPadMsg::ButtonPressed(5))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 1.0,
                    top_margin + (button_height + padding) * 1.0,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key50.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key51.pcx".to_owned())),
            gui::button(KeyPadMsg::ButtonPressed(6))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 2.0,
                    top_margin + (button_height + padding) * 1.0,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key60.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key61.pcx".to_owned())),
            // Third row of buttons
            gui::button(KeyPadMsg::ButtonPressed(7))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 0.0,
                    top_margin + (button_height + padding) * 2.0,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key70.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key71.pcx".to_owned())),
            gui::button(KeyPadMsg::ButtonPressed(8))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 1.0,
                    top_margin + (button_height + padding) * 2.0,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key80.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key81.pcx".to_owned())),
            gui::button(KeyPadMsg::ButtonPressed(9))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 2.0,
                    top_margin + (button_height + padding) * 2.0,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key90.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key91.pcx".to_owned())),
            // Fourth row of buttons
            gui::button(KeyPadMsg::ButtonPressed(0))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 0.0,
                    top_margin + (button_height + padding) * 3.0,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key00.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key01.pcx".to_owned())),
            gui::button(KeyPadMsg::Clear)
                .with_position(vec2(
                    left_margin + (button_width + padding) * 1.0,
                    top_margin + (button_height + padding) * 3.0,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("keyn0.pcx")
                .with_hover(ButtonHoverBehavior::Texture("keyn1.pcx".to_owned())),
        ];

        if let Some(v) = _state.current_value {
            components.extend(draw_number(v))
        }
        components
    }

    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: Vector3::new(0.0, 0.0, -0.1),
            screen_size_in_pixels: Vector2::new(188.0, 296.0),
        }
    }

    fn handle_msg(
        &self,
        entity_id: EntityId,
        world: &World,
        state: &KeyPadState,
        msg: &KeyPadMsg,
    ) -> (KeyPadState, crate::Effect) {
        let hack_diff = hack_diff_for_entity(world, entity_id);
        if let Some(hack_diff) = hack_diff {
            let (hack, effect) = handle_hack_msg(
                entity_id,
                world,
                &state.hack,
                msg,
                hack_diff,
                HrmMode::Hack,
                HackOutcomeEffects {
                    success: keypad_hack_success,
                    critical_failure: keypad_hack_critical_failure,
                },
            );
            return (
                KeyPadState {
                    hack,
                    ..state.clone()
                },
                effect,
            );
        }

        let v_prop_keypad_code = world.borrow::<View<PropKeypadCode>>().unwrap();
        let maybe_keypad_code = v_prop_keypad_code.get(entity_id);

        let press_effect = Effect::PlaySound {
            handle: AudioHandle::new(),
            source: Some(entity_id),
            name: "bkeypad".to_owned(),
            spatial: false,
        };

        let new_state = match msg {
            KeyPadMsg::ButtonPressed(n) => {
                let new_value = match state.current_value {
                    Some(current_value) => {
                        // If current value is at least 5 digits, reset
                        // TODO: Is it guaranteed that all keypad codes are 5 digits?
                        if current_value > 9999 {
                            *n
                        } else {
                            current_value * 10 + n
                        }
                    }
                    None => *n,
                };
                KeyPadState {
                    current_value: Some(new_value),
                    ..state.clone()
                }
            }
            KeyPadMsg::Clear => KeyPadState {
                current_value: None,
                ..state.clone()
            },
            KeyPadMsg::StartHack | KeyPadMsg::PlayNode { .. } => state.clone(),
        };

        // Check if the keypad code matches
        let additional_effect = if let Ok(keypad_code) = maybe_keypad_code {
            if let Some(current_value) = new_state.current_value {
                if current_value == keypad_code.0 {
                    let switch_link_effect = send_to_all_switch_links_and_self(
                        world,
                        entity_id,
                        MessagePayload::TurnOn { from: entity_id },
                    );
                    let sound_effect = Effect::PlaySound {
                        handle: AudioHandle::new(),
                        source: Some(entity_id),
                        name: "hacksucc".to_owned(),
                        spatial: false,
                    };
                    Effect::combine(vec![switch_link_effect, sound_effect])
                } else {
                    Effect::NoEffect
                }
            } else {
                Effect::NoEffect
            }
        } else {
            Effect::NoEffect
        };

        (
            new_state,
            Effect::combine(vec![additional_effect, press_effect]),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retail_board_has_authored_holes_and_fourteen_playable_nodes() {
        let board = base_hack_board();
        assert_eq!(
            board.iter().filter(|node| **node == HackNode::Free).count(),
            14
        );
        assert_eq!(board[board_index(0, 0)], HackNode::Empty);
        assert_eq!(board[board_index(3, 1)], HackNode::Empty);
        assert_eq!(board[board_index(4, 3)], HackNode::Empty);
    }

    #[test]
    fn earth_top_row_requires_all_three_connected_nodes() {
        let mut board = base_hack_board();
        board[board_index(2, 0)] = HackNode::Lit;
        board[board_index(3, 0)] = HackNode::Lit;
        assert!(!has_connected_three(&board));

        board[board_index(4, 0)] = HackNode::Lit;
        assert!(has_connected_three(&board));
    }

    #[test]
    fn connected_three_also_recognizes_vertical_paths() {
        let mut board = base_hack_board();
        for y in 1..=3 {
            board[board_index(0, y)] = HackNode::Lit;
        }
        assert!(has_connected_three(&board));
    }

    #[test]
    fn seeded_earth_route_uses_three_genuine_success_rolls() {
        let earth_effective_chance = 55;
        let mut rng_state = 7;
        let board = board_with_mines(1, &mut rng_state);
        let rolls = (0..3)
            .map(|_| outcome_roll(&mut rng_state))
            .collect::<Vec<_>>();

        assert!(
            [(2, 0), (3, 0), (4, 0)]
                .into_iter()
                .all(|(x, y)| board[board_index(x, y)] != HackNode::Mine)
        );
        assert!(
            rolls
                .iter()
                .all(|roll| roll_succeeds(*roll, earth_effective_chance)),
            "seeded route should genuinely pass all three 55% rolls: {rolls:?}"
        );
    }

    #[test]
    fn real_interaction_time_changes_board_randomness() {
        assert_ne!(
            mix_hack_seed(1_000_000_000, 266),
            mix_hack_seed(1_016_666_667, 266)
        );
    }

    #[test]
    fn probability_boundaries_have_exact_outcome_counts() {
        for chance in [0, 55, 85] {
            assert_eq!(
                (0..100).filter(|roll| roll_succeeds(*roll, chance)).count(),
                chance as usize
            );
        }
    }

    #[test]
    fn board_is_unwinnable_when_free_nodes_remain_but_no_triple_can_survive() {
        let mut board = base_hack_board();
        for (x, y) in [(3, 0), (1, 1), (3, 2), (1, 3), (0, 2), (2, 1), (4, 1)] {
            board[board_index(x, y)] = HackNode::Burned;
        }

        assert!(board.contains(&HackNode::Free));
        assert!(!board_has_potential_path(&board));
    }

    #[test]
    fn numeric_keypad_code_takes_precedence_over_inherited_hack_diff() {
        let mut world = World::new();
        let numeric_with_inherited_hack_diff = world.add_entity((
            PropKeypadCode(45100),
            PropHackDiff {
                success_chance: 50,
                critical_chance: 2,
                cost: 3.0,
            },
        ));
        assert!(
            hack_diff_for_entity(&world, numeric_with_inherited_hack_diff).is_none(),
            "an authored numeric code must take precedence over inherited HackDiff"
        );
    }

    /// Every image the board would draw, in draw order.
    fn board_images(state: &HackState, mode: HrmMode) -> Vec<String> {
        let diff = PropHackDiff {
            success_chance: 50,
            critical_chance: 1,
            cost: 3.0,
        };
        draw_hack_board::<KeyPadMsg, _>(state, diff, mode, |msg| msg)
            .into_iter()
            .filter_map(|component| match component {
                crate::ui::UiElement::Image { texture, .. } => Some(texture),
                _ => None,
            })
            .collect()
    }

    fn finished_board(phase: HackPhase) -> HackState {
        HackState {
            phase,
            ..HackState::default()
        }
    }

    #[test]
    fn each_mode_wears_its_own_backdrop_and_result_art() {
        for (mode, backdrop, won, lost, unwinnable, unpaid) in [
            (
                HrmMode::Repair,
                "iface/repair.pcx",
                "winr.pcx",
                "loser.pcx",
                "failr.pcx",
                "payr.pcx",
            ),
            (
                HrmMode::Modify,
                "modify.pcx",
                "winm.pcx",
                "losem.pcx",
                "failm.pcx",
                "paym.pcx",
            ),
        ] {
            let images = board_images(&HackState::default(), mode);
            assert_eq!(images.first().map(String::as_str), Some(backdrop));
            for (phase, expected) in [
                (HackPhase::Won, won),
                (HackPhase::Lost, lost),
                (HackPhase::Unwinnable, unwinnable),
                (HackPhase::InsufficientNanites, unpaid),
            ] {
                let images = board_images(&finished_board(phase), mode);
                assert!(
                    images.iter().any(|texture| texture == expected),
                    "{mode:?} in {phase:?} should draw {expected}, drew {images:?}"
                );
            }
        }
    }

    #[test]
    fn hack_mode_art_is_unchanged() {
        assert_eq!(
            board_images(&HackState::default(), HrmMode::Hack)
                .first()
                .map(String::as_str),
            Some("hack.pcx")
        );
        for (phase, expected) in [
            (HackPhase::Won, "winh.pcx"),
            (HackPhase::Lost, "loseh.pcx"),
            (HackPhase::Unwinnable, "failh.pcx"),
            (HackPhase::InsufficientNanites, "payh.pcx"),
        ] {
            let images = board_images(&finished_board(phase), HrmMode::Hack);
            assert!(
                images.iter().any(|texture| texture == expected),
                "hack in {phase:?} should still draw {expected}, drew {images:?}"
            );
        }
    }

    #[test]
    fn a_finished_board_speaks_in_its_own_modes_words() {
        let strings = crate::hud::HudStrings::default();
        assert_eq!(
            strings.hrm_result(HrmMode::Hack, HrmResult::Won),
            "Hacking successful!"
        );
        assert_eq!(
            strings.hrm_result(HrmMode::Repair, HrmResult::Lost),
            "You have destroyed the item!"
        );
        assert_eq!(
            strings.hrm_result(HrmMode::Modify, HrmResult::PlayedOut),
            "You did not successfully modify the weapon on this attempt."
        );
        // The nine keys must stay distinct: an index slip would silently make
        // one mode speak in another's voice.
        let mut all: Vec<&str> = [HrmMode::Hack, HrmMode::Repair, HrmMode::Modify]
            .iter()
            .flat_map(|mode| {
                [HrmResult::PlayedOut, HrmResult::Won, HrmResult::Lost]
                    .iter()
                    .map(|result| strings.hrm_result(*mode, *result))
                    .collect::<Vec<_>>()
            })
            .collect();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), 9);
    }

    #[test]
    fn a_won_board_posts_its_modes_result_line() {
        let mut world = World::new();
        let entity = world.add_entity(());
        // A board one lit node short of a run of three, so the next successful
        // node wins it.
        let mut nodes = base_hack_board();
        nodes[board_index(2, 0)] = HackNode::Lit;
        nodes[board_index(3, 0)] = HackNode::Lit;
        let state = HackState {
            phase: HackPhase::Playing,
            nodes,
            rng_state: 1,
        };
        let (after, effect) = handle_hack_msg(
            entity,
            &world,
            &state,
            &KeyPadMsg::PlayNode { x: 4, y: 0 },
            // Certain success, so the win is not a coin flip.
            PropHackDiff {
                success_chance: 100,
                critical_chance: 0,
                cost: 0.0,
            },
            HrmMode::Repair,
            HackOutcomeEffects {
                success: keypad_hack_success,
                critical_failure: keypad_hack_critical_failure,
            },
        );
        assert_eq!(after.phase, HackPhase::Won);
        let messages: Vec<String> = Effect::flatten(vec![effect])
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::ShowMessage { text } => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(
            messages,
            vec!["The item has been successfully repaired, and can be used normally."]
        );
    }

    #[test]
    fn critical_loss_is_terminal_for_the_live_panel_state() {
        let mut world = World::new();
        let entity = world.add_entity(());
        let state = HackState {
            phase: HackPhase::Lost,
            ..HackState::default()
        };
        let diff = PropHackDiff {
            success_chance: 50,
            critical_chance: 1,
            cost: 3.0,
        };

        let (after, effect) = handle_hack_msg(
            entity,
            &world,
            &state,
            &KeyPadMsg::StartHack,
            diff,
            HrmMode::Hack,
            HackOutcomeEffects {
                success: keypad_hack_success,
                critical_failure: keypad_hack_critical_failure,
            },
        );

        assert_eq!(after.phase, HackPhase::Lost);
        assert!(matches!(effect, Effect::NoEffect));
    }
}
