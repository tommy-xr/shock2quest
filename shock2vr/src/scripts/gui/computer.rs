use std::collections::HashMap;

use cgmath::{Vector2, Vector3};
use dark::{
    properties::{
        Link, Links, ObjectState, PropHackDiff, PropHackText, PropTemplateId, ToLink,
        WrappedEntityId,
    },
    ss2_entity_info::SystemShock2EntityInfo,
};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View, World};

use crate::{
    gui::{Gui, GuiComponent, GuiConfig, GuiCursor},
    scripts::{Effect, Message, MessagePayload, script_util::*},
};

use super::keypad::{
    HackOutcomeEffects, HackPhase, HackState, KeyPadMsg, draw_hack_panel, hack_diff,
    handle_hack_msg, object_state,
};

/// Xerxes: "Security system offline."
const SECURITY_HACKED_SCHEMA: &str = "xer01";

/// Older saves made before `P$HackText` / `L$HackingLi` were parsed cannot
/// contain those components. Restore them from the same authored object data
/// used for a fresh mission, so such a save retains Computer instructions and
/// the genuine downstream hacking circuit.
pub(crate) fn restore_authored_computer_data(
    world: &mut World,
    entity_info: &SystemShock2EntityInfo,
    template_to_entity_id: &HashMap<i32, WrappedEntityId>,
) {
    let computers = {
        let templates = world.borrow::<View<PropTemplateId>>().unwrap();
        let diffs = world.borrow::<View<PropHackDiff>>().unwrap();
        templates
            .iter()
            .with_id()
            .filter(|(entity_id, _)| diffs.get(*entity_id).is_ok())
            .map(|(entity_id, template)| (entity_id, template.template_id))
            .collect::<Vec<_>>()
    };

    for (entity_id, template_id) in computers {
        let has_text = world
            .borrow::<View<PropHackText>>()
            .unwrap()
            .get(entity_id)
            .is_ok();
        if !has_text
            && let Some(text) = hydrate_template_component::<PropHackText>(template_id, entity_info)
        {
            world.add_component(entity_id, text);
        }

        let has_hacking_link = world
            .borrow::<View<Links>>()
            .unwrap()
            .get(entity_id)
            .is_ok_and(|links| links.to_links.iter().any(|link| link.link == Link::Hacking));
        if has_hacking_link {
            continue;
        }

        let mut ancestors = dark::ss2_entity_info::get_ancestors(
            dark::ss2_entity_info::get_hierarchy(entity_info),
            &template_id,
        );
        ancestors.push(template_id);
        let authored_hacking_links = ancestors
            .into_iter()
            .filter_map(|ancestor| entity_info.template_to_links.get(&ancestor))
            .flat_map(|links| &links.to_links)
            .filter(|link| link.link == Link::Hacking)
            .map(|link| ToLink {
                to_template_id: link.to_template_id,
                to_entity_id: template_to_entity_id.get(&link.to_template_id).copied(),
                link: Link::Hacking,
            })
            .collect::<Vec<_>>();
        if authored_hacking_links.is_empty() {
            continue;
        }
        let mut links = world
            .borrow::<View<Links>>()
            .unwrap()
            .get(entity_id)
            .cloned()
            .unwrap_or_else(|_| Links::empty());
        links.to_links.extend(authored_hacking_links);
        world.add_component(entity_id, links);
    }
}

#[derive(Default)]
pub struct ComputerGui {
    pub security: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ComputerState {
    hack: HackState,
}

#[derive(Clone)]
pub enum ComputerMsg {
    Hack(KeyPadMsg),
}

fn can_hack(world: &World, entity_id: EntityId) -> bool {
    !matches!(
        object_state(world, entity_id),
        ObjectState::Broken | ObjectState::Destroyed | ObjectState::Hacked
    ) && hack_diff(world, entity_id).is_some()
}

fn security_hack_success(entity_id: EntityId, _world: &World) -> Effect {
    Effect::combine(vec![
        Effect::ClearSecurityAlarm { from: entity_id },
        announce(entity_id, SECURITY_HACKED_SCHEMA),
    ])
}

fn computer_hack_success(entity_id: EntityId, world: &World) -> Effect {
    let mut effects = get_all_links_of_type(world, entity_id, Link::Hacking)
        .into_iter()
        .map(|to| Effect::Send {
            msg: Message {
                to,
                payload: MessagePayload::TurnOn { from: entity_id },
            },
        })
        .collect::<Vec<_>>();

    let hacked_replacement =
        get_first_link_with_template_and_data(world, entity_id, |link| match link {
            Link::Corpse(_) => Some(()),
            _ => None,
        })
        .map(|(template_id, ())| template_id);
    effects.push(match hacked_replacement {
        Some(template_id) => Effect::ReplaceEntity {
            entity_id,
            template_id,
        },
        None => Effect::SetObjectState {
            entity_id,
            state: ObjectState::Hacked,
        },
    });
    Effect::combine(effects)
}

fn computer_hack_critical_failure(entity_id: EntityId, _world: &World) -> Effect {
    Effect::SetObjectState {
        entity_id,
        state: ObjectState::Broken,
    }
}

impl Gui<ComputerState, ComputerMsg> for ComputerGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        entity_id: EntityId,
        world: &World,
        state: &ComputerState,
    ) -> Vec<GuiComponent<ComputerMsg>> {
        let Some(diff) = hack_diff(world, entity_id) else {
            return Vec::new();
        };
        draw_hack_panel(
            world,
            entity_id,
            &state.hack,
            diff,
            self.security,
            ComputerMsg::Hack,
        )
    }

    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: Vector3::new(0.0, 0.0, -1.0),
            screen_size_in_pixels: Vector2::new(188.0, 296.0),
        }
    }

    fn handle_msg(
        &self,
        entity_id: EntityId,
        world: &World,
        state: &ComputerState,
        msg: &ComputerMsg,
    ) -> (ComputerState, Effect) {
        let Some(diff) = hack_diff(world, entity_id) else {
            return (state.clone(), Effect::NoEffect);
        };
        let ComputerMsg::Hack(msg) = msg;
        let (hack, effect) = handle_hack_msg(
            entity_id,
            world,
            &state.hack,
            msg,
            diff,
            self.security,
            HackOutcomeEffects {
                success: if self.security {
                    security_hack_success
                } else {
                    computer_hack_success
                },
                critical_failure: computer_hack_critical_failure,
            },
        );
        (ComputerState { hack }, effect)
    }

    fn opens_on_frob(&self, entity_id: EntityId, world: &World) -> bool {
        can_hack(world, entity_id)
    }

    fn prepare_state_on_frob(&self, state: &mut ComputerState) {
        if state.hack.phase != HackPhase::Playing {
            *state = ComputerState::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use dark::properties::{CorpseOptions, Links, ToLink, WrappedEntityId};
    use shipyard::World;

    use super::*;
    use dark::properties::PropObjState;

    fn flatten(effect: &Effect) -> Vec<&Effect> {
        match effect {
            Effect::Combined { effects } => effects.iter().flat_map(flatten).collect(),
            other => vec![other],
        }
    }

    #[test]
    fn success_activates_hacking_links_and_uses_authored_replacement() {
        let mut world = World::new();
        let router = world.add_entity(());
        let computer = world.add_entity(Links {
            to_links: vec![
                ToLink {
                    to_template_id: 312,
                    to_entity_id: Some(WrappedEntityId(router)),
                    link: Link::Hacking,
                },
                ToLink {
                    to_template_id: 1269,
                    to_entity_id: None,
                    link: Link::Corpse(
                        serde_json::from_str::<CorpseOptions>(r#"{"propagate_scale":false}"#)
                            .unwrap(),
                    ),
                },
            ],
        });

        let effect = computer_hack_success(computer, &world);
        let effects = flatten(&effect);
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::Send {
                msg: Message {
                    to,
                    payload: MessagePayload::TurnOn { from },
                },
            } if *to == router && *from == computer
        )));
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::ReplaceEntity {
                entity_id,
                template_id: 1269,
            } if *entity_id == computer
        )));
    }

    #[test]
    fn success_without_corpse_persists_hacked_state() {
        let mut world = World::new();
        let computer = world.add_entity(Links::empty());
        let effects = computer_hack_success(computer, &world);
        assert!(flatten(&effects).iter().any(|effect| matches!(
            effect,
            Effect::SetObjectState {
                entity_id,
                state: ObjectState::Hacked,
            } if *entity_id == computer
        )));
    }

    #[test]
    fn critical_failure_persists_broken_state() {
        let mut world = World::new();
        let computer = world.add_entity(());
        assert!(matches!(
            computer_hack_critical_failure(computer, &world),
            Effect::SetObjectState {
                entity_id,
                state: ObjectState::Broken,
            } if entity_id == computer
        ));
    }

    #[test]
    fn broken_and_hacked_computers_do_not_reopen() {
        let gui = ComputerGui::default();
        for state in [ObjectState::Broken, ObjectState::Hacked] {
            let mut world = World::new();
            let computer = world.add_entity((
                PropHackDiff {
                    success_chance: 20,
                    critical_chance: 10,
                    cost: 3.0,
                },
                PropObjState(state),
            ));
            assert!(!gui.opens_on_frob(computer, &world));
        }
    }

    #[test]
    fn reopening_preserves_only_an_in_progress_hack() {
        let gui = ComputerGui::default();
        let mut state = ComputerState {
            hack: HackState {
                phase: HackPhase::Playing,
                rng_state: 42,
                ..HackState::default()
            },
        };
        gui.prepare_state_on_frob(&mut state);
        assert_eq!(state.hack.phase, HackPhase::Playing);
        assert_eq!(state.hack.rng_state, 42);

        state.hack.phase = HackPhase::Won;
        gui.prepare_state_on_frob(&mut state);
        assert_eq!(state.hack.phase, HackPhase::Unpaid);
    }

    #[test]
    fn a_hacked_security_computer_announces_security_offline() {
        let mut world = World::new();
        let computer = world.add_entity(());
        assert_eq!(
            announced(&security_hack_success(computer, &world)),
            ["xer01"]
        );
    }
}
