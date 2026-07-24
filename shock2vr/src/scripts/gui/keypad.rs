use cgmath::{Vector2, Vector3, vec2};
use dark::properties::{PropHackDiff, PropKeypadCode, PropTemplateId};
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
enum HackPhase {
    #[default]
    Unpaid,
    Playing,
    Won,
    Lost,
    Unwinnable,
    InsufficientNanites,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum HackNode {
    #[default]
    Free,
    Lit,
    Burned,
    Mine,
    Empty,
}

#[derive(Clone, Debug)]
struct HackState {
    phase: HackPhase,
    nodes: [HackNode; BOARD_WIDTH * BOARD_HEIGHT],
    rng_state: u64,
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

const fn board_index(x: usize, y: usize) -> usize {
    y * BOARD_WIDTH + x
}

const fn base_hack_board() -> [HackNode; BOARD_WIDTH * BOARD_HEIGHT] {
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

fn hack_cost(diff: PropHackDiff) -> i32 {
    (diff.cost as i32).max(1)
}

fn outcome_roll(rng_state: &mut u64) -> i32 {
    next_random(rng_state, 100) as i32
}

fn roll_succeeds(roll: i32, chance: i32) -> bool {
    roll < chance
}

fn effective_hack_values(world: &World, diff: PropHackDiff) -> (i32, i32) {
    let (skill, stat) = world
        .borrow::<UniqueView<QuestInfo>>()
        .ok()
        .map(|quest| {
            let stats = quest.player_stats();
            (stats.skill_level(Skill::Hack), stats.cyber_affinity)
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
        });
        x += numeral_width + padding;
    }
    ret
}

fn draw_hack_board(state: &HackState, diff: PropHackDiff) -> Vec<GuiComponent<KeyPadMsg>> {
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
                gui::button(KeyPadMsg::PlayNode { x, y })
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
            gui::button(KeyPadMsg::StartHack)
                .with_position(vec2(157.0, 232.0))
                .with_size(vec2(20.0, 56.0))
                .with_image(normal)
                .with_hover(ButtonHoverBehavior::Texture(hover.to_owned()))
                .with_label(label),
        );
    }
    components
}

fn handle_hack_msg(
    entity_id: EntityId,
    world: &World,
    state: &KeyPadState,
    msg: &KeyPadMsg,
    diff: PropHackDiff,
) -> (KeyPadState, Effect) {
    let mut new_state = state.clone();
    match msg {
        KeyPadMsg::StartHack
            if !matches!(new_state.hack.phase, HackPhase::Won | HackPhase::Lost) =>
        {
            let cost = hack_cost(diff);
            let Some(payment) = spend_player_nanites(world, cost) else {
                new_state.hack.phase = HackPhase::InsufficientNanites;
                return (
                    new_state,
                    Effect::PlaySound {
                        handle: AudioHandle::new(),
                        name: "login".to_owned(),
                    },
                );
            };
            let (_, mine_count) = effective_hack_values(world, diff);
            let mut rng_state = hack_seed(world, entity_id);
            tracing::debug!(entity = entity_id.inner(), rng_state, "HRM rng seed");
            let nodes = board_with_mines(mine_count, &mut rng_state);
            tracing::debug!(
                entity = entity_id.inner(),
                rng_state,
                "HRM rng initialized after mine placement"
            );
            new_state.hack = HackState {
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
                        name: "start_hack".to_owned(),
                    },
                ]),
            )
        }
        KeyPadMsg::PlayNode { x, y }
            if *x < BOARD_WIDTH
                && *y < BOARD_HEIGHT
                && new_state.hack.phase == HackPhase::Playing =>
        {
            let index = board_index(*x, *y);
            let node = new_state.hack.nodes[index];
            if !matches!(node, HackNode::Free | HackNode::Mine) {
                return (
                    new_state,
                    Effect::PlaySound {
                        handle: AudioHandle::new(),
                        name: "login".to_owned(),
                    },
                );
            }

            let was_mine = node == HackNode::Mine;
            let roll = outcome_roll(&mut new_state.hack.rng_state);
            tracing::debug!(
                entity = entity_id.inner(),
                roll,
                rng_state = new_state.hack.rng_state,
                "HRM rng outcome"
            );
            let (chance, _) = effective_hack_values(world, diff);
            if roll_succeeds(roll, chance) {
                new_state.hack.nodes[index] = HackNode::Lit;
                if has_connected_three(&new_state.hack.nodes) {
                    new_state.hack.phase = HackPhase::Won;
                    return (
                        new_state,
                        Effect::combine(vec![
                            send_to_all_switch_links_and_self(
                                world,
                                entity_id,
                                MessagePayload::TurnOn { from: entity_id },
                            ),
                            Effect::PlaySound {
                                handle: AudioHandle::new(),
                                name: "hack_success".to_owned(),
                            },
                        ]),
                    );
                }
            } else if was_mine {
                new_state.hack.phase = HackPhase::Lost;
                return (
                    new_state,
                    Effect::PlaySound {
                        handle: AudioHandle::new(),
                        name: "hack_critical".to_owned(),
                    },
                );
            } else {
                new_state.hack.nodes[index] = HackNode::Burned;
                if !board_has_potential_path(&new_state.hack.nodes) {
                    new_state.hack.phase = HackPhase::Unwinnable;
                }
            }

            (
                new_state,
                Effect::PlaySound {
                    handle: AudioHandle::new(),
                    name: "hacking".to_owned(),
                },
            )
        }
        _ => (new_state, Effect::NoEffect),
    }
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
            return draw_hack_board(&_state.hack, hack_diff);
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
            return handle_hack_msg(entity_id, world, state, msg, hack_diff);
        }

        let v_prop_keypad_code = world.borrow::<View<PropKeypadCode>>().unwrap();
        let maybe_keypad_code = v_prop_keypad_code.get(entity_id);

        let press_effect = Effect::PlaySound {
            handle: AudioHandle::new(),
            name: "bkeypad".to_owned(),
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
                        name: "hacksucc".to_owned(),
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

    #[test]
    fn critical_loss_is_terminal_for_the_live_panel_state() {
        let mut world = World::new();
        let entity = world.add_entity(());
        let state = KeyPadState {
            hack: HackState {
                phase: HackPhase::Lost,
                ..HackState::default()
            },
            ..KeyPadState::default()
        };
        let diff = PropHackDiff {
            success_chance: 50,
            critical_chance: 1,
            cost: 3.0,
        };

        let (after, effect) = handle_hack_msg(entity, &world, &state, &KeyPadMsg::StartHack, diff);

        assert_eq!(after.hack.phase, HackPhase::Lost);
        assert!(matches!(effect, Effect::NoEffect));
    }
}
