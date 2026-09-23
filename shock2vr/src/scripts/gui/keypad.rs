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

#[derive(Clone, Debug)]
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

#[cfg(test)]
fn effective_hack_values(world: &World, diff: PropHackDiff, security_computer: bool) -> (i32, i32) {
    effective_hrm_values(world, diff, HrmContext::Hack { security_computer })
}

#[derive(Clone, Copy)]
pub(crate) enum HrmContext {
    Hack { security_computer: bool },
    Modify,
    Repair,
}

fn effective_hrm_values(world: &World, diff: PropHackDiff, context: HrmContext) -> (i32, i32) {
    let (skill, stat) = world
        .borrow::<UniqueView<QuestInfo>>()
        .ok()
        .map(|quest| {
            let stats = quest.player_stats();
            let base = stats.skill_level(match context {
                HrmContext::Hack { .. } => Skill::Hack,
                HrmContext::Modify => Skill::Modify,
                HrmContext::Repair => Skill::Repair,
            });
            // Retail grants two effective levels only at security computers;
            // it does not train an unskilled player or change the saved sheet.
            let bonus = if matches!(
                context,
                HrmContext::Hack {
                    security_computer: true
                }
            ) && base > 0
                && stats.has_os_trait(super::traits::TRAIT_SECURITY_EXPERT)
            {
                2
            } else {
                0
            };
            (
                base + bonus
                    + if base > 0 && crate::implants::active(world, 7) {
                        1
                    } else {
                        0
                    },
                stats.cyber_affinity,
            )
        })
        .unwrap_or((0, 0));
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

fn draw_number(num: u32) -> Vec<GuiComponent<KeyPadMsg>> {
    vec![GuiComponent::Text {
        position: vec2(14.0, 14.0),
        size: vec2(139.0, 25.0),
        text: num.to_string(),
        font: "keyfonta.fon".to_owned(),
        font_size: 0.0,
        h: crate::ui::HAlign::Left,
        v: crate::ui::VAlign::Top,
        alpha: 1.0,
        fit_to_rect: false,
    }]
}

pub(crate) fn draw_hack_board<TMsg, F>(
    state: &HackState,
    diff: PropHackDiff,
    wrap: F,
) -> Vec<GuiComponent<TMsg>>
where
    TMsg: Clone,
    F: Fn(KeyPadMsg) -> TMsg + Copy,
{
    let mut components = vec![
        gui::image("hack.pcx")
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
            // The unlit node outline is already part of HACK.PCX. This
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
        HackPhase::Won => Some("winh.pcx"),
        HackPhase::Lost => Some("loseh.pcx"),
        HackPhase::Unwinnable => Some("failh.pcx"),
        HackPhase::InsufficientNanites => Some("payh.pcx"),
        HackPhase::Unpaid | HackPhase::Playing => None,
    };
    if let Some(texture) = result_texture {
        components.push(
            gui::image(texture)
                .with_position(vec2(13.0, 42.0))
                .with_size(vec2(142.0, 134.0)),
        );
    }

    // HACK.PCX already supplies the cyan `COST:` label. Retail draws only the
    // dynamic numeric value in the 48px slot beginning at x=128, after the
    // result overlay so WINH/LOSEH/PAYH cannot obscure it.
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
    security_computer: bool,
    outcomes: HackOutcomeEffects,
) -> (HackState, Effect) {
    handle_hrm_msg(
        entity_id,
        world,
        state,
        msg,
        diff,
        HrmContext::Hack { security_computer },
        outcomes,
    )
}

pub(crate) fn handle_hrm_msg(
    entity_id: EntityId,
    world: &World,
    state: &HackState,
    msg: &KeyPadMsg,
    diff: PropHackDiff,
    context: HrmContext,
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
            let (_, mine_count) = effective_hrm_values(world, diff, context);
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
            let roll = outcome_roll(&mut new_state.rng_state);
            tracing::debug!(
                entity = entity_id.inner(),
                roll,
                rng_state = new_state.rng_state,
                "HRM rng outcome"
            );
            let (chance, _) = effective_hrm_values(world, diff, context);
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
                    ]),
                );
            } else {
                new_state.nodes[index] = HackNode::Burned;
                if !board_has_potential_path(&new_state.nodes) {
                    new_state.phase = HackPhase::Unwinnable;
                }
            }

            (
                new_state,
                Effect::PlaySound {
                    handle: AudioHandle::new(),
                    source: Some(entity_id),
                    name: "hacking".to_owned(),
                    spatial: false,
                },
            )
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
            return draw_hack_board(&_state.hack, hack_diff, |msg| msg);
        }

        // Retail shkkeypd.cpp draws the complete keypad2 artwork and puts
        // invisible hit regions over it. The old key?0 images obscure the
        // remaster's higher-resolution digits. The eleven keysel overlays
        // are pixel-identical; assign one per button in layout order.
        let mut components = vec![
            gui::image("keypad2.pcx")
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(188.0, 296.0))
                .with_alpha(1.0),
        ];
        for (index, digit) in [1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 10].into_iter().enumerate() {
            let rect = crate::ui::Rect::new(
                15.0 + (index % 3) as f32 * 47.0,
                43.0 + (index / 3) as f32 * 61.0,
                44.0,
                59.0,
            );
            let hovered = _cursor
                .as_ref()
                .is_some_and(|cursor| rect.contains(vec2(cursor.position.x, cursor.position.y)));
            let (msg, label) = if digit == 10 {
                (KeyPadMsg::Clear, "clear".to_owned())
            } else {
                (KeyPadMsg::ButtonPressed(digit), digit.to_string())
            };
            components.push(
                gui::button(msg)
                    .with_rect(rect)
                    .with_image(&format!("keysel{index}.png"))
                    .with_label(&label)
                    // Selection tint over the authored key, shared by flat
                    // mouse and VR ray pointing. The PNG supplies translucency;
                    // clicks retain bkeypad audio.
                    .with_alpha(if hovered { 1.0 } else { 0.0 }),
            );
        }

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
                false,
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
    #[test]
    fn security_expert_is_a_contextual_bonus_for_trained_hackers() {
        for (owned, security, base, expected) in [
            (false, true, 1, (35, 9)),
            (true, false, 1, (35, 9)),
            (true, true, 0, (25, 9)),
            (true, true, 1, (55, 9)),
            (true, true, 6, (85, 9)),
        ] {
            let mut world = World::new();
            let entity = world.add_entity(());
            let mut quests = QuestInfo::new();
            quests.player_stats_mut().skills.hack = base;
            quests.player_stats_mut().cyber_affinity = 1;
            if owned {
                quests.player_stats_mut().add_os_trait(10);
            }
            world.add_unique(quests);
            world.add_unique(GlobalHrmParams(Some(dark::gamesys::HrmParams {
                // Retail shock2.gam HRM: skill affects chance, stat affects both.
                skill_critical_bonus: 0,
                skill_success_bonus: 10,
                stat_critical_bonus: 1,
                stat_success_bonus: 5,
                stat_break_chance: [0.0; 8],
            })));
            let diff = PropHackDiff {
                success_chance: 20,
                critical_chance: 10,
                cost: 3.0,
            };
            assert_eq!(effective_hack_values(&world, diff, security), expected);
            // The same roll loses at base Hack 1 (35%) and succeeds at the
            // security computer with the upgrade (55%). Exercise the actual
            // node-message path, not only the numerical resolver.
            let seed = (1..1000)
                .find(|seed| outcome_roll(&mut seed.clone()) == 40)
                .unwrap();
            let state = HackState {
                phase: HackPhase::Playing,
                rng_state: seed,
                ..HackState::default()
            };
            let (after, _) = handle_hack_msg(
                entity,
                &world,
                &state,
                &KeyPadMsg::PlayNode { x: 2, y: 0 },
                diff,
                security,
                HackOutcomeEffects {
                    success: keypad_hack_success,
                    critical_failure: keypad_hack_critical_failure,
                },
            );
            assert_eq!(
                after.nodes[board_index(2, 0)],
                if expected.0 > 40 {
                    HackNode::Lit
                } else {
                    HackNode::Burned
                }
            );
            assert_eq!(
                world
                    .borrow::<UniqueView<QuestInfo>>()
                    .unwrap()
                    .player_stats()
                    .skills
                    .hack,
                base
            );
        }
    }

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
            false,
            HackOutcomeEffects {
                success: keypad_hack_success,
                critical_failure: keypad_hack_critical_failure,
            },
        );

        assert_eq!(after.phase, HackPhase::Lost);
        assert!(matches!(effect, Effect::NoEffect));
    }
}
