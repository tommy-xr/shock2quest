use cgmath::{Point3, vec3};
use dark::properties::{Link, Links, PropPosition};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View, World};

use crate::{mission::entity_creator::CreateEntityOptions, physics::PhysicsWorld};

use super::{Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError};

const STATE_KEY: &str = "shock2vr.many_brain";

/// Retail ManyBrain counts incoming SwitchLinks and adds Invulnerable until
/// each defender's TriggerDestroy sends TurnOn. Collision messages are already
/// delivered to live scripts by this engine, so no native subscription is needed.
pub struct ManyBrain {
    defenders: u32,
}

impl ManyBrain {
    pub fn new() -> Self {
        Self { defenders: 0 }
    }

    fn defender_sources(world: &World, entity: EntityId) -> Vec<EntityId> {
        let Ok(links) = world.borrow::<View<Links>>() else {
            return Vec::new();
        };
        (&links)
            .iter()
            .with_id()
            .flat_map(|(source, links)| {
                links.to_links.iter().filter_map(move |link| {
                    (matches!(link.link, Link::SwitchLink)
                        && link.to_entity_id.is_some_and(|target| target.0 == entity))
                    .then_some(source)
                })
            })
            .collect()
    }

    fn protection(entity_id: EntityId, add: bool) -> Effect {
        Effect::SetMetaProperty {
            entity_id,
            name: "Invulnerable".into(),
            add,
        }
    }
}

impl Script for ManyBrain {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        self.defenders = Self::defender_sources(world, entity_id).len() as u32;
        if self.defenders > 0 {
            Self::protection(entity_id, true)
        } else {
            Effect::NoEffect
        }
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { .. } if self.defenders > 0 => {
                self.defenders -= 1;
                if self.defenders == 0 {
                    Self::protection(entity_id, false)
                } else {
                    Effect::NoEffect
                }
            }
            MessagePayload::Collided { .. } if self.defenders > 0 => {
                let Ok(positions) = world.borrow::<View<PropPosition>>() else {
                    return Effect::NoEffect;
                };
                // This is a brief flash on the surviving defenders, not a
                // permanent bubble around the brain. The authored ball shield
                // brings its ballhaze attachment and 500 ms deletion tweq.
                Effect::combine(
                    Self::defender_sources(world, entity_id)
                        .into_iter()
                        .filter_map(|source| {
                            let position = positions.get(source).ok()?;
                            Some(Effect::CreateEntityByTemplateName {
                                source_entity_id: entity_id,
                                template_name: "ball shield".into(),
                                position: Point3::new(
                                    position.position.x,
                                    position.position.y,
                                    position.position.z,
                                ),
                                orientation: position.rotation,
                                initial_velocity: vec3(0.0, 0.0, 0.0),
                                options: CreateEntityOptions {
                                    attach_to: Some(source),
                                    ..CreateEntityOptions::default()
                                },
                            })
                        })
                        .collect(),
                )
            }
            _ => Effect::NoEffect,
        }
    }

    fn script_state_key(&self) -> Option<&'static str> {
        Some(STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(1, &self.defenders, STATE_KEY)
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        self.defenders = state.decode(1, STATE_KEY)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::Quaternion;
    use dark::properties::{ToLink, WrappedEntityId};
    use std::collections::HashMap;

    fn fixture(count: u32) -> (World, EntityId, Vec<EntityId>) {
        let mut world = World::new();
        let brain = world.add_entity(());
        let defenders = (0..count)
            .map(|index| {
                world.add_entity((
                    PropPosition {
                        position: vec3(index as f32, 2.0, 3.0),
                        rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                        cell: 0,
                    },
                    Links {
                        to_links: vec![ToLink {
                            to_template_id: 99,
                            to_entity_id: Some(WrappedEntityId(brain)),
                            link: Link::SwitchLink,
                        }],
                    },
                ))
            })
            .collect();
        (world, brain, defenders)
    }

    #[test]
    fn only_the_last_defender_removes_protection() {
        let (world, brain, defenders) = fixture(3);
        let physics = PhysicsWorld::new();
        let mut script = ManyBrain::new();
        assert!(
            matches!(script.initialize(brain, &world), Effect::SetMetaProperty { add: true, name, .. } if name == "Invulnerable")
        );
        for (index, from) in defenders.into_iter().enumerate() {
            let effect =
                script.handle_message(brain, &world, &physics, &MessagePayload::TurnOn { from });
            if index == 2 {
                assert!(
                    matches!(effect, Effect::SetMetaProperty { add: false, name, .. } if name == "Invulnerable")
                );
            } else {
                assert!(matches!(effect, Effect::NoEffect));
            }
        }
        assert!(matches!(
            script.handle_message(
                brain,
                &world,
                &physics,
                &MessagePayload::TurnOn { from: brain }
            ),
            Effect::NoEffect
        ));
    }

    #[test]
    fn protected_impact_flashes_only_remaining_linked_defenders() {
        let (mut world, brain, defenders) = fixture(3);
        let physics = PhysicsWorld::new();
        let mut script = ManyBrain::new();
        script.initialize(brain, &world);
        script.handle_message(
            brain,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: defenders[0] },
        );
        world.delete_entity(defenders[0]);
        let effects = Effect::flatten(vec![script.handle_message(
            brain,
            &world,
            &physics,
            &MessagePayload::Collided {
                with: brain,
                contact: None,
            },
        )]);
        assert_eq!(effects.len(), 2);
        let mut attached = effects
            .into_iter()
            .map(|effect| match effect {
                Effect::CreateEntityByTemplateName {
                    template_name,
                    options,
                    ..
                } => {
                    assert_eq!(template_name, "ball shield");
                    assert!(
                        !options.transient_fx,
                        "the authored 500 ms tweq owns the flash lifetime"
                    );
                    options.attach_to.unwrap()
                }
                other => panic!("unexpected shield effect {other:?}"),
            })
            .collect::<Vec<_>>();
        attached.sort();
        let mut expected = defenders[1..].to_vec();
        expected.sort();
        assert_eq!(attached, expected);
    }

    #[test]
    fn zero_through_three_defenders_round_trip_without_recounting() {
        let (world, brain, _) = fixture(3);
        let physics = PhysicsWorld::new();
        let mapping = HashMap::new();
        let context = ScriptRestoreContext::new(&mapping);
        for remaining in 0..=3 {
            let original = ManyBrain {
                defenders: remaining,
            };
            let mut restored = ManyBrain::new();
            restored
                .restore_state(&original.save_state().unwrap(), &context)
                .unwrap();
            assert!(matches!(
                restored.initialize_after_hydration(brain, &world, true),
                Effect::NoEffect
            ));
            assert_eq!(
                restored.defenders, remaining,
                "hydration cannot recount the still-present fixture links"
            );
            let effects = Effect::flatten(vec![restored.handle_message(
                brain,
                &world,
                &physics,
                &MessagePayload::Collided {
                    with: brain,
                    contact: None,
                },
            )]);
            assert_eq!(effects.len(), if remaining > 0 { 3 } else { 0 });
        }
        let (empty_world, unprotected, _) = fixture(0);
        assert!(matches!(
            ManyBrain::new().initialize(unprotected, &empty_world),
            Effect::NoEffect
        ));
    }
}
