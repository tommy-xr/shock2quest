use cgmath::{Vector2, Vector3, vec2, vec3};
use dark::properties::{ObjectState, PropReplicatorContents, PropReplicatorHackedContents};
use engine::audio::AudioHandle;
use num_traits::ToPrimitive;

use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    gui::{ButtonHoverBehavior, Gui, GuiComponent, GuiConfig, GuiCursor},
    mission::GlobalEntityMetadata,
    quest_info::QuestInfo,
    util::{get_position_from_transform, get_rotation_from_transform},
};

use crate::gui;

use crate::scripts::{Effect, script_util::*};

use super::{
    keypad::{
        HackOutcomeEffects, HackPhase, HackState, KeyPadMsg, draw_hack_board, hack_diff,
        handle_hack_msg, object_state,
    },
    traits::TRAIT_REPLICATOR_EXPERT,
};

pub struct ReplicatorGui;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ReplicatorPanel {
    #[default]
    Inventory,
    Hacking,
}

#[derive(Clone, Debug, Default)]
pub struct ReplicatorState {
    message: Option<String>,
    panel: ReplicatorPanel,
    hack: HackState,
}

#[derive(Clone)]
pub enum ReplicatorMsg {
    SelectItem(usize),
    OpenHack,
    Hack(KeyPadMsg),
}

#[derive(Clone)]
struct ReplicatorInventory {
    costs: [i32; 6],
    object_names: [String; 6],
}

fn active_inventory(world: &World, entity_id: EntityId) -> Option<ReplicatorInventory> {
    match object_state(world, entity_id) {
        ObjectState::Broken | ObjectState::Destroyed => None,
        ObjectState::Hacked => world
            .borrow::<View<PropReplicatorHackedContents>>()
            .ok()
            .and_then(|contents| {
                contents
                    .get(entity_id)
                    .ok()
                    .map(|contents| ReplicatorInventory {
                        costs: contents.costs,
                        object_names: contents.object_names.clone(),
                    })
            }),
        _ => world
            .borrow::<View<PropReplicatorContents>>()
            .ok()
            .and_then(|contents| {
                contents
                    .get(entity_id)
                    .ok()
                    .map(|contents| ReplicatorInventory {
                        costs: contents.costs,
                        object_names: contents.object_names.clone(),
                    })
            }),
    }
}

fn can_hack(world: &World, entity_id: EntityId) -> bool {
    !matches!(
        object_state(world, entity_id),
        ObjectState::Broken | ObjectState::Destroyed | ObjectState::Hacked
    ) && hack_diff(world, entity_id).is_some()
        && world
            .borrow::<View<PropReplicatorHackedContents>>()
            .ok()
            .is_some_and(|contents| contents.get(entity_id).is_ok())
}

fn replicator_hack_success(entity_id: EntityId, _world: &World) -> Effect {
    Effect::SetObjectState {
        entity_id,
        state: ObjectState::Hacked,
    }
}

fn replicator_hack_critical_failure(entity_id: EntityId, _world: &World) -> Effect {
    Effect::SetObjectState {
        entity_id,
        state: ObjectState::Broken,
    }
}

impl Gui<ReplicatorState, ReplicatorMsg> for ReplicatorGui {
    fn on_provide_for_consumption(
        &self,
        _entity_id: EntityId,
        _world: &World,
        _provided_entity_id: EntityId,
    ) -> Option<Effect> {
        // A replicator is a dispenser, not a container or tool receptor. In
        // particular, output spawned at its authored hopper marker can touch
        // RepBase immediately; refusing that ToolConsumable offer leaves the
        // item physical for the player to pick up instead of hiding it behind
        // a runtime Contains link.
        Some(Effect::NoEffect)
    }

    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        entity_id: EntityId,
        world: &World,
        state: &ReplicatorState,
    ) -> Vec<GuiComponent<ReplicatorMsg>> {
        if state.panel == ReplicatorPanel::Hacking {
            if let Some(diff) = hack_diff(world, entity_id) {
                return draw_hack_board(&state.hack, diff, ReplicatorMsg::Hack);
            }
        }

        let button_height = 60.0;
        let initial_padding_y = 10.0;
        let button_width = 188.0;
        let button_padding = 4.0;
        let replicator_contents = active_inventory(world, entity_id);
        let current_state = object_state(world, entity_id);
        let has_sidecar = can_hack(world, entity_id) || current_state == ObjectState::Broken;
        let main_x = if has_sidecar { 73.0 } else { 0.0 };

        let replicator_icon = |icon: &str, position: f32| GuiComponent::Image {
            position: vec2(
                main_x + 10.0,
                5.0 + initial_padding_y + (button_height + button_padding) * position,
            ),
            size: vec2(30.0, button_height - 10.0),
            texture: icon.to_owned(),
            alpha: 0.5,
            transparent_index_0: false,
        };

        let mut components: Vec<GuiComponent<ReplicatorMsg>> = vec![
            gui::image("replic.pcx")
                .with_position(vec2(main_x, 0.0))
                .with_size(vec2(188.0, 296.0)),
        ];
        if let Some(replicator_contents) = replicator_contents {
            let entity_metadata = world.borrow::<UniqueView<GlobalEntityMetadata>>().unwrap();
            for (i, (obj_name, authored_cost)) in replicator_contents
                .object_names
                .iter()
                .zip(replicator_contents.costs)
                .enumerate()
            {
                let float_i = i.to_f32().unwrap();

                if obj_name.is_empty() || authored_cost <= 0 {
                    continue;
                }
                let cost = effective_replicator_cost(world, authored_cost);
                if cost <= 0 {
                    continue;
                }

                let metadata = entity_metadata.0.get(obj_name).unwrap();
                let obj_icon = metadata.obj_icon.as_ref().unwrap();

                components.push(
                    gui::button(ReplicatorMsg::SelectItem(i))
                        .with_position(vec2(
                            main_x,
                            initial_padding_y + (button_height + button_padding) * float_i,
                        ))
                        .with_size(vec2(button_width, button_height))
                        .with_image("key0.pcx")
                        .with_label(&format!("buy:{obj_name}")),
                );

                components.push(replicator_icon(obj_icon, float_i));

                if let Some(short_name) = metadata.obj_short_name.as_ref() {
                    components.push(gui::text(short_name).with_position(vec2(
                        main_x + 50.0,
                        button_height / 2.0 + (button_height + button_padding) * float_i,
                    )));
                }

                // Retail draws the three-digit price beneath the item icon.
                components.push(
                    gui::text(&format!("{cost:03}"))
                        .with_position(vec2(
                            main_x + 19.0,
                            47.0 + (button_height + button_padding) * float_i,
                        ))
                        .with_size(vec2(34.0, 12.0)),
                );
            }
        }

        if can_hack(world, entity_id) {
            // Retail raises this 73x194 companion beside the MFD, with its
            // 52x74 PLUGH button at sidecar-local (16,114).
            components.push(
                gui::image("plughack.pcx")
                    .with_position(vec2(0.0, 96.0))
                    .with_size(vec2(73.0, 194.0)),
            );
            components.push(
                gui::button(ReplicatorMsg::OpenHack)
                    .with_position(vec2(16.0, 210.0))
                    .with_size(vec2(52.0, 74.0))
                    .with_image("plugh0.pcx")
                    .with_hover(ButtonHoverBehavior::Texture("plugh1.pcx".to_owned()))
                    .with_label("hack-replicator"),
            );
        } else if current_state == ObjectState::Broken {
            // Original raises the repair plug for a broken replicator. Repair
            // is not implemented yet, so expose the authored status art but
            // deliberately no clickable repair/hack/purchase path.
            components.push(
                gui::image("plugrep.pcx")
                    .with_position(vec2(0.0, 96.0))
                    .with_size(vec2(73.0, 194.0)),
            );
        }

        // REPLIC.PCX supplies "YOUR NANITES:"; retail places a four-digit
        // live balance at (104, 274) beside it.
        components.push(
            gui::text(&format!("{:04}", player_nanite_total(world)))
                .with_position(vec2(main_x + 104.0, 274.0))
                .with_size(vec2(40.0, 14.0)),
        );
        let message = if current_state == ObjectState::Broken {
            Some("Replicator broken; repair required")
        } else if active_inventory(world, entity_id).is_none() {
            Some("Replicator offline")
        } else {
            state.message.as_deref()
        };
        if let Some(message) = message {
            components.push(
                gui::text(message)
                    .with_position(vec2(main_x + 10.0, 248.0))
                    .with_size(vec2(168.0, 14.0)),
            );
        }

        components
    }

    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: Vector3::new(0.0, 0.0, -1.0),
            screen_size_in_pixels: Vector2::new(188.0, 296.0),
        }
    }

    fn get_config_for(
        &self,
        entity_id: EntityId,
        world: &World,
        state: &ReplicatorState,
    ) -> GuiConfig {
        let has_sidecar = state.panel == ReplicatorPanel::Inventory
            && (can_hack(world, entity_id)
                || object_state(world, entity_id) == ObjectState::Broken);
        GuiConfig {
            world_offset: Vector3::new(0.0, 0.0, -1.0),
            screen_size_in_pixels: Vector2::new(if has_sidecar { 261.0 } else { 188.0 }, 296.0),
        }
    }

    fn prepare_state_on_frob(&self, state: &mut ReplicatorState) {
        if state.panel != ReplicatorPanel::Hacking || state.hack.phase != HackPhase::Playing {
            *state = ReplicatorState::default();
        }
    }

    fn handle_msg(
        &self,
        entity_id: EntityId,
        world: &World,
        state: &ReplicatorState,
        msg: &ReplicatorMsg,
    ) -> (ReplicatorState, Effect) {
        match msg {
            ReplicatorMsg::SelectItem(slot) => {
                let Some(contents) = active_inventory(world, entity_id) else {
                    return (
                        ReplicatorState {
                            message: Some("Replicator offline".to_owned()),
                            ..state.clone()
                        },
                        Effect::NoEffect,
                    );
                };
                let Some((item, authored_cost)) = contents
                    .object_names
                    .get(*slot)
                    .zip(contents.costs.get(*slot))
                    .filter(|(item, cost)| !item.is_empty() && **cost > 0)
                else {
                    return (
                        ReplicatorState {
                            message: Some("Item unavailable".to_owned()),
                            ..state.clone()
                        },
                        Effect::NoEffect,
                    );
                };
                let cost = effective_replicator_cost(world, *authored_cost);
                // Retail treats a post-discount cost of zero as a failed
                // selection, not a free vend (`ShockReplicate`: cost != 0).
                if cost <= 0 {
                    return (
                        ReplicatorState {
                            message: Some("Item unavailable".to_owned()),
                            ..state.clone()
                        },
                        Effect::PlaySound {
                            handle: AudioHandle::new(),
                            source: Some(entity_id),
                            name: "repfail".to_owned(),
                            spatial: false,
                        },
                    );
                }
                if spend_player_nanites(world, cost).is_none() {
                    return (
                        ReplicatorState {
                            message: Some("Insufficient nanites".to_owned()),
                            ..state.clone()
                        },
                        Effect::PlaySound {
                            handle: AudioHandle::new(),
                            source: Some(entity_id),
                            name: "repfail".to_owned(),
                            spatial: false,
                        },
                    );
                }
                let Some(link) =
                    get_first_link_of_type(world, entity_id, dark::properties::Link::Replicator)
                else {
                    return (
                        ReplicatorState {
                            message: Some("Replicator offline".to_owned()),
                            ..state.clone()
                        },
                        Effect::NoEffect,
                    );
                };

                let purchase = Effect::ReplicatorPurchase {
                    cost,
                    template_name: item.clone(),
                    position: get_position_from_transform(world, link, vec3(0.0, 0.0, 0.0)),
                    orientation: get_rotation_from_transform(world, link),
                };

                (
                    ReplicatorState {
                        message: None,
                        ..state.clone()
                    },
                    purchase,
                )
            }
            ReplicatorMsg::OpenHack => {
                if can_hack(world, entity_id) {
                    (
                        ReplicatorState {
                            message: None,
                            panel: ReplicatorPanel::Hacking,
                            hack: HackState::default(),
                        },
                        Effect::NoEffect,
                    )
                } else {
                    (state.clone(), Effect::NoEffect)
                }
            }
            ReplicatorMsg::Hack(hack_msg) => {
                let Some(diff) = hack_diff(world, entity_id) else {
                    return (state.clone(), Effect::NoEffect);
                };
                if state.panel != ReplicatorPanel::Hacking
                    || matches!(
                        object_state(world, entity_id),
                        ObjectState::Broken | ObjectState::Destroyed
                    )
                {
                    return (state.clone(), Effect::NoEffect);
                }
                let (hack, effect) = handle_hack_msg(
                    entity_id,
                    world,
                    &state.hack,
                    hack_msg,
                    diff,
                    HackOutcomeEffects {
                        success: replicator_hack_success,
                        critical_failure: replicator_hack_critical_failure,
                    },
                );
                (
                    ReplicatorState {
                        hack,
                        ..state.clone()
                    },
                    effect,
                )
            }
        }
    }
}

/// Retail's Replicator Expert trait applies an integer 20% discount. Resolve
/// this from the persistent character sheet for both rendering and purchase
/// handling so the quoted and charged prices cannot diverge.
fn effective_replicator_cost(world: &World, authored_cost: i32) -> i32 {
    let has_expert = world
        .borrow::<UniqueView<QuestInfo>>()
        .map(|quests| quests.player_stats().has_os_trait(TRAIT_REPLICATOR_EXPERT))
        .unwrap_or(false);
    if has_expert {
        ((i64::from(authored_cost) * 8) / 10) as i32
    } else {
        authored_cost
    }
}

#[cfg(test)]
mod tests {
    use super::super::keypad::{HackNode, HackPhase, base_hack_board, board_index};
    use super::*;
    use crate::gui::GuiScript;
    use crate::mission::PlayerInfo;
    use crate::physics::PhysicsWorld;
    use crate::runtime_props::RuntimePropTransform;
    use crate::scripts::{MessagePayload, Script};
    use cgmath::{Matrix4, Quaternion};
    use dark::properties::{
        Link, Links, PropObjIcon, PropPosition, PropStackCount, ToLink, WrappedEntityId,
    };
    use dark::properties::{PropHackDiff, PropObjState};

    fn creates_template(effect: &Effect) -> bool {
        match effect {
            Effect::CreateEntityByTemplateName { .. } | Effect::ReplicatorPurchase { .. } => true,
            Effect::Combined { effects } => effects.iter().any(creates_template),
            _ => false,
        }
    }

    #[test]
    fn replicator_refuses_an_offered_item_instead_of_containing_it() {
        let mut world = World::new();
        let replicator = world.add_entity(());
        let resonator = world.add_entity(());
        let mut script = GuiScript::new(Box::new(ReplicatorGui));

        let effect = script.handle_message(
            replicator,
            &world,
            &PhysicsWorld::new(),
            &MessagePayload::ProvideForConsumption { entity: resonator },
        );

        assert!(
            matches!(effect, Effect::NoEffect),
            "a replicator is a dispenser, not a container; got {effect:?}"
        );
    }

    #[test]
    fn hack_action_requires_an_authored_hacked_inventory() {
        let mut world = World::new();
        let replicator = world.add_entity((
            PropHackDiff {
                success_chance: 50,
                critical_chance: 0,
                cost: 3.0,
            },
            PropReplicatorContents {
                costs: [3, 0, 0, 0, 0, 0],
                object_names: [
                    "chips".to_owned(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                ],
            },
        ));

        assert!(
            !can_hack(&world, replicator),
            "success must not turn a usable machine into an offline Hacked state"
        );
        world.add_component(
            replicator,
            PropReplicatorHackedContents {
                costs: [70, 0, 0, 0, 0, 0],
                object_names: [
                    "small he clip".to_owned(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                ],
            },
        );
        assert!(can_hack(&world, replicator));
    }

    #[test]
    fn critical_hack_loss_persistently_breaks_and_disables_the_replicator() {
        let mut world = World::new();
        let replicator = world.add_entity((
            PropHackDiff {
                success_chance: 0,
                critical_chance: 0,
                cost: 3.0,
            },
            PropReplicatorContents {
                costs: [3, 0, 0, 0, 0, 0],
                object_names: [
                    "chips".to_owned(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                ],
            },
            PropReplicatorHackedContents {
                costs: [70, 0, 0, 0, 0, 0],
                object_names: [
                    "small he clip".to_owned(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                ],
            },
        ));
        let mut nodes = base_hack_board();
        nodes[board_index(2, 0)] = HackNode::Mine;
        let state = ReplicatorState {
            panel: ReplicatorPanel::Hacking,
            hack: HackState {
                phase: HackPhase::Playing,
                nodes,
                rng_state: 1,
            },
            ..ReplicatorState::default()
        };

        let (after, effect) = ReplicatorGui.handle_msg(
            replicator,
            &world,
            &state,
            &ReplicatorMsg::Hack(KeyPadMsg::PlayNode { x: 2, y: 0 }),
        );
        assert_eq!(after.hack.phase, HackPhase::Lost);

        let mut applied = false;
        for effect in Effect::flatten(vec![effect]) {
            if let Effect::SetObjectState { entity_id, state } = effect {
                assert_eq!(entity_id, replicator);
                assert_eq!(state, ObjectState::Broken);
                world.add_component(entity_id, PropObjState(state));
                applied = true;
            }
        }
        assert!(applied, "critical loss must emit persistent Broken state");
        assert_eq!(object_state(&world, replicator), ObjectState::Broken);
        assert!(active_inventory(&world, replicator).is_none());
        assert!(!can_hack(&world, replicator));

        let reopened =
            ReplicatorGui.get_components(&None, replicator, &world, &ReplicatorState::default());
        assert!(
            reopened.iter().any(|component| matches!(
                component,
                GuiComponent::Image { texture, .. } if texture == "plugrep.pcx"
            )),
            "broken state should expose the authored repair status sidecar"
        );
        assert!(
            reopened
                .iter()
                .all(|component| !matches!(component, GuiComponent::Button { .. })),
            "broken state must disable every purchase and hack action"
        );
    }

    #[test]
    fn unaffordable_or_zero_cost_selection_does_not_create_an_item() {
        let mut world = World::new();
        let output = world.add_entity((
            RuntimePropTransform(Matrix4::from_translation(vec3(1.0, 2.0, 3.0))),
            PropPosition {
                position: vec3(1.0, 2.0, 3.0),
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                cell: 0,
            },
        ));
        let replicator = world.add_entity((
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(output)),
                    link: Link::Replicator,
                }],
            },
            PropReplicatorContents {
                costs: [3, 0, 0, 0, 0, 0],
                object_names: [
                    "Food Snack".to_owned(),
                    "Free Snack".to_owned(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                ],
            },
        ));

        let gui = ReplicatorGui;
        let (_state, effect) = gui.handle_msg(
            replicator,
            &world,
            &ReplicatorState::default(),
            &ReplicatorMsg::SelectItem(0),
        );

        assert!(
            !creates_template(&effect),
            "an unaffordable replicator selection must not mint an item: {effect:?}"
        );

        let (_state, zero_cost_effect) = gui.handle_msg(
            replicator,
            &world,
            &ReplicatorState::default(),
            &ReplicatorMsg::SelectItem(1),
        );
        assert!(
            !creates_template(&zero_cost_effect),
            "a zero-cost replicator slot must not mint a free item: {zero_cost_effect:?}"
        );
        let mut refused_state = ReplicatorState {
            message: Some("Insufficient nanites".to_owned()),
            ..ReplicatorState::default()
        };
        gui.prepare_state_on_frob(&mut refused_state);
        assert_eq!(refused_state.message, None);
    }

    #[test]
    fn reopening_preserves_only_a_paid_in_progress_hack() {
        let gui = ReplicatorGui;
        let mut playing = ReplicatorState {
            panel: ReplicatorPanel::Hacking,
            hack: HackState {
                phase: HackPhase::Playing,
                rng_state: 42,
                ..HackState::default()
            },
            ..ReplicatorState::default()
        };
        gui.prepare_state_on_frob(&mut playing);
        assert_eq!(playing.panel, ReplicatorPanel::Hacking);
        assert_eq!(playing.hack.phase, HackPhase::Playing);
        assert_eq!(playing.hack.rng_state, 42);

        playing.hack.phase = HackPhase::Won;
        gui.prepare_state_on_frob(&mut playing);
        assert_eq!(playing.panel, ReplicatorPanel::Inventory);
        assert_eq!(playing.hack.phase, HackPhase::Unpaid);
    }

    #[test]
    fn affordable_selection_debits_authored_cost_before_creating_item() {
        let mut world = World::new();
        let nanites = world.add_entity((
            PropObjIcon("nan_ic".to_owned()),
            PropStackCount(10),
            Links::empty(),
        ));
        let inventory = world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 0,
                to_entity_id: Some(WrappedEntityId(nanites)),
                link: Link::Contains(0),
            }],
        });
        let player = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });
        let output = world.add_entity((
            RuntimePropTransform(Matrix4::from_translation(vec3(1.0, 2.0, 3.0))),
            PropPosition {
                position: vec3(1.0, 2.0, 3.0),
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                cell: 0,
            },
        ));
        let replicator = world.add_entity((
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(output)),
                    link: Link::Replicator,
                }],
            },
            PropReplicatorContents {
                costs: [3, 1, 0, 0, 0, 0],
                object_names: [
                    "Food Snack".to_owned(),
                    "Almost Free Snack".to_owned(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                ],
            },
        ));

        let (_state, effect) = ReplicatorGui.handle_msg(
            replicator,
            &world,
            &ReplicatorState::default(),
            &ReplicatorMsg::SelectItem(0),
        );
        assert!(
            matches!(
                effect,
                Effect::ReplicatorPurchase {
                    cost: 3,
                    ref template_name,
                    ..
                } if template_name == "Food Snack"
            ),
            "an affordable click should defer one authoritative purchase: {effect:?}"
        );
        assert_eq!(
            player_nanite_total(&world),
            10,
            "GUI pre-validation must not mutate before effect handling"
        );

        let mut quests = QuestInfo::new();
        assert!(
            quests
                .player_stats_mut()
                .add_os_trait(TRAIT_REPLICATOR_EXPERT)
        );
        world.add_unique(quests);
        let (_state, discounted_to_zero) = ReplicatorGui.handle_msg(
            replicator,
            &world,
            &ReplicatorState::default(),
            &ReplicatorMsg::SelectItem(1),
        );
        assert!(
            !creates_template(&discounted_to_zero),
            "Replicator Expert must not turn an authored cost of 1 into a free vend"
        );
    }

    #[test]
    fn replicator_expert_applies_retail_integer_discount() {
        let world = World::new();
        assert_eq!(effective_replicator_cost(&world, 41), 41);

        let mut quests = QuestInfo::new();
        assert!(
            quests
                .player_stats_mut()
                .add_os_trait(TRAIT_REPLICATOR_EXPERT)
        );
        world.add_unique(quests);

        assert_eq!(effective_replicator_cost(&world, 3), 2);
        assert_eq!(effective_replicator_cost(&world, 41), 32);
        assert_eq!(
            effective_replicator_cost(&world, 1),
            0,
            "the caller must refuse this retail zero-cost result rather than vend for free"
        );
        assert_eq!(
            effective_replicator_cost(&world, i32::MAX),
            1_717_986_917,
            "the retail integer discount must not saturate before division"
        );
    }
}
