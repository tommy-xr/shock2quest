//! rick1's torpedo bay launch FX. Both scripts sit beside `BaseElevator`,
//! which moves the object; they only add the effects on the same `TurnOn`.

use cgmath::{point3, vec3};
use dark::properties::PropPosition;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

use super::{Effect, MessagePayload, Script, script_util::attached_particles};

/// Spawned at a torpedo as it starts moving: an invisible swingbar that
/// carries the "stars" particles and deletes itself after 1.25 s.
const LIFT_FX_TEMPLATE: &str = "Torpedo Lift FX";

/// A loading-bay torpedo: puffs the lift FX when its button sends it off.
pub struct TorpedoLift;

impl Script for TorpedoLift {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::TurnOn { .. }) {
            return Effect::NoEffect;
        }
        let v_pos = world.borrow::<View<PropPosition>>().unwrap();
        let Ok(pose) = v_pos.get(entity_id) else {
            return Effect::NoEffect;
        };
        Effect::CreateEntityByTemplateName {
            source_entity_id: entity_id,
            template_name: LIFT_FX_TEMPLATE.into(),
            position: point3(pose.position.x, pose.position.y, pose.position.z),
            orientation: pose.rotation,
            initial_velocity: vec3(0.0, 0.0, 0.0),
            options: Default::default(),
        }
    }
}

/// The Salvo pod: lights its authored-off jets when launched. The jets are a
/// "Pod Jets" group attached to the pod, with "pod jet blue" groups attached
/// to the jets, so one level of riders-of-riders is switched on too.
pub struct TorpedoHack;

impl Script for TorpedoHack {
    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        if !matches!(msg, MessagePayload::TurnOn { .. }) {
            return Effect::NoEffect;
        }
        let jets = attached_particles(world, entity_id);
        let riders = jets.iter().flat_map(|jet| attached_particles(world, *jet));
        Effect::combine(
            jets.iter()
                .copied()
                .chain(riders)
                .map(|entity_id| Effect::SetParticleActive {
                    entity_id,
                    active: true,
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use cgmath::{Quaternion, vec3};
    use dark::properties::{
        Link, Links, ParticleAttachOptions, PropPosition, ToLink, WrappedEntityId,
    };
    use shipyard::{EntityId, World};

    use crate::physics::PhysicsWorld;

    use super::{Effect, MessagePayload, Script, TorpedoHack, TorpedoLift};

    fn turn_on(from: EntityId) -> MessagePayload {
        MessagePayload::TurnOn { from }
    }

    /// Mission-style particle group riding `host` (link FX -> host).
    fn add_rider(world: &mut World, host: EntityId) -> EntityId {
        world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 0,
                to_entity_id: Some(WrappedEntityId(host)),
                link: Link::ParticleAttachement(ParticleAttachOptions {
                    attach_type: 0,
                    vhot: 0,
                    joint: 0,
                    submodel: 0,
                }),
            }],
        })
    }

    fn activated(effect: Effect) -> Vec<EntityId> {
        Effect::flatten(vec![effect])
            .into_iter()
            .filter_map(|e| match e {
                Effect::SetParticleActive {
                    entity_id,
                    active: true,
                } => Some(entity_id),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn lift_spawns_fx_at_the_torpedo_on_turn_on() {
        let mut world = World::new();
        let torpedo = world.add_entity((PropPosition {
            position: vec3(1.0, 2.0, 3.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            cell: 0,
        },));
        let physics = PhysicsWorld::new();

        let effects = Effect::flatten(vec![TorpedoLift.handle_message(
            torpedo,
            &world,
            &physics,
            &turn_on(torpedo),
        )]);
        assert!(
            matches!(
                effects.as_slice(),
                [Effect::CreateEntityByTemplateName { template_name, position, .. }]
                    if template_name == "Torpedo Lift FX"
                        && *position == cgmath::point3(1.0, 2.0, 3.0)
            ),
            "one lift FX at the torpedo"
        );

        let ignored = TorpedoLift.handle_message(
            torpedo,
            &world,
            &physics,
            &MessagePayload::TurnOff { from: torpedo },
        );
        assert!(Effect::flatten(vec![ignored]).is_empty());
    }

    #[test]
    fn hack_lights_jets_and_their_riders_only() {
        let mut world = World::new();
        let salvo = world.add_entity(());
        let elsewhere = world.add_entity(());
        let jets = add_rider(&mut world, salvo);
        let blue_a = add_rider(&mut world, jets);
        let blue_b = add_rider(&mut world, jets);
        let _unrelated = add_rider(&mut world, elsewhere);
        let physics = PhysicsWorld::new();

        let mut lit =
            activated(TorpedoHack.handle_message(salvo, &world, &physics, &turn_on(salvo)));
        lit.sort();
        let mut expected = vec![jets, blue_a, blue_b];
        expected.sort();
        assert_eq!(lit, expected);

        let ignored = TorpedoHack.handle_message(
            salvo,
            &world,
            &physics,
            &MessagePayload::TurnOff { from: salvo },
        );
        assert!(activated(ignored).is_empty());
    }
}
