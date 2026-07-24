use cgmath::{Vector2, Vector3, vec2, vec3};
use dark::properties::PropReplicatorContents;
use engine::audio::AudioHandle;
use num_traits::ToPrimitive;

use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    gui::{Gui, GuiComponent, GuiConfig, GuiCursor},
    mission::GlobalEntityMetadata,
    quest_info::QuestInfo,
    util::{get_position_from_transform, get_rotation_from_transform},
};

use crate::gui;

use crate::scripts::{Effect, script_util::*};

use super::traits::TRAIT_REPLICATOR_EXPERT;

pub struct ReplicatorGui;

#[derive(Clone, Debug, Default)]
pub struct ReplicatorState {
    message: Option<String>,
}

#[derive(Clone)]
pub enum ReplicatorMsg {
    SelectItem(usize),
}

impl Gui<ReplicatorState, ReplicatorMsg> for ReplicatorGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        entity_id: EntityId,
        world: &World,
        state: &ReplicatorState,
    ) -> Vec<GuiComponent<ReplicatorMsg>> {
        let button_height = 60.0;
        let initial_padding_y = 10.0;
        let button_width = 188.0;
        let button_padding = 4.0;

        let v_prop_replicator = world.borrow::<View<PropReplicatorContents>>().unwrap();
        let replicator_contents = v_prop_replicator.get(entity_id).unwrap();

        let entity_metadata = world.borrow::<UniqueView<GlobalEntityMetadata>>().unwrap();

        let replicator_icon = |icon: &str, position: f32| GuiComponent::Image {
            position: vec2(
                10.0,
                5.0 + initial_padding_y + (button_height + button_padding) * position,
            ),
            size: vec2(30.0, button_height - 10.0),
            texture: icon.to_owned(),
            alpha: 0.5,
        };

        let mut components: Vec<GuiComponent<ReplicatorMsg>> = vec![
            gui::image("replic.pcx")
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(188.0, 296.0)),
        ];
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
                        0.0,
                        initial_padding_y + (button_height + button_padding) * float_i,
                    ))
                    .with_size(vec2(button_width, button_height))
                    .with_image("key0.pcx")
                    .with_label(&format!("buy:{obj_name}")),
            );

            components.push(replicator_icon(obj_icon, float_i));

            if let Some(short_name) = metadata.obj_short_name.as_ref() {
                components.push(gui::text(short_name).with_position(vec2(
                    50.0,
                    button_height / 2.0 + (button_height + button_padding) * float_i,
                )));
            }

            // Retail draws the three-digit price beneath the item icon.
            components.push(
                gui::text(&format!("{cost:03}"))
                    .with_position(vec2(
                        19.0,
                        47.0 + (button_height + button_padding) * float_i,
                    ))
                    .with_size(vec2(34.0, 12.0)),
            );
        }

        // REPLIC.PCX supplies "YOUR NANITES:"; retail places a four-digit
        // live balance at (104, 274) beside it.
        components.push(
            gui::text(&format!("{:04}", player_nanite_total(world)))
                .with_position(vec2(104.0, 274.0))
                .with_size(vec2(40.0, 14.0)),
        );
        if let Some(message) = &state.message {
            components.push(
                gui::text(message)
                    .with_position(vec2(10.0, 248.0))
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

    fn resets_state_on_frob(&self) -> bool {
        true
    }

    fn handle_msg(
        &self,
        entity_id: EntityId,
        world: &World,
        _state: &ReplicatorState,
        msg: &ReplicatorMsg,
    ) -> (ReplicatorState, Effect) {
        match msg {
            ReplicatorMsg::SelectItem(slot) => {
                let contents = world
                    .borrow::<View<PropReplicatorContents>>()
                    .ok()
                    .and_then(|view| view.get(entity_id).ok().cloned());
                let Some(contents) = contents else {
                    return (
                        ReplicatorState {
                            message: Some("Replicator offline".to_owned()),
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
                        },
                        Effect::PlaySound {
                            handle: AudioHandle::new(),
                            name: "repfail".to_owned(),
                        },
                    );
                }
                if spend_player_nanites(world, cost).is_none() {
                    return (
                        ReplicatorState {
                            message: Some("Insufficient nanites".to_owned()),
                        },
                        Effect::PlaySound {
                            handle: AudioHandle::new(),
                            name: "repfail".to_owned(),
                        },
                    );
                }
                let Some(link) =
                    get_first_link_of_type(world, entity_id, dark::properties::Link::Replicator)
                else {
                    return (
                        ReplicatorState {
                            message: Some("Replicator offline".to_owned()),
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

                (ReplicatorState { message: None }, purchase)
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
    use super::*;
    use crate::mission::PlayerInfo;
    use crate::runtime_props::RuntimePropTransform;
    use cgmath::{Matrix4, Quaternion};
    use dark::properties::{
        Link, Links, PropObjIcon, PropPosition, PropStackCount, ToLink, WrappedEntityId,
    };

    fn creates_template(effect: &Effect) -> bool {
        match effect {
            Effect::CreateEntityByTemplateName { .. } | Effect::ReplicatorPurchase { .. } => true,
            Effect::Combined { effects } => effects.iter().any(creates_template),
            _ => false,
        }
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
            &ReplicatorState { message: None },
            &ReplicatorMsg::SelectItem(0),
        );

        assert!(
            !creates_template(&effect),
            "an unaffordable replicator selection must not mint an item: {effect:?}"
        );

        let (_state, zero_cost_effect) = gui.handle_msg(
            replicator,
            &world,
            &ReplicatorState { message: None },
            &ReplicatorMsg::SelectItem(1),
        );
        assert!(
            !creates_template(&zero_cost_effect),
            "a zero-cost replicator slot must not mint a free item: {zero_cost_effect:?}"
        );
        assert!(
            gui.resets_state_on_frob(),
            "reopening the panel should clear transient refusal text"
        );
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
            &ReplicatorState { message: None },
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
            &ReplicatorState { message: None },
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
