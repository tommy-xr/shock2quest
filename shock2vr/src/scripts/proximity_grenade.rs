//! Proximity grenades retain an authored sensor as a separate, saved object.
//! Bounce mode arms at physics sleep; the contact spang is already deployed.
//! The sensor's Corpse link supplies the blast, rather than a hardcoded radius
//! or damage value. ScriptParams links persist/remap the mine/sensor pair.
use dark::properties::{Link, PropAI, PropHitPoints};
use shipyard::{EntitiesView, EntityId, IntoIter, IntoWithId, View, World};

use super::{Effect, MessagePayload, Script, script_util};
use crate::{physics::PhysicsWorld, time::Time};

pub(crate) fn linked_peer(world: &World, entity: EntityId) -> Option<EntityId> {
    script_util::get_first_link_of_type(world, entity, Link::ScriptParams)
}

pub struct ProximityGrenade {
    contact: bool,
    detonating: bool,
}

impl ProximityGrenade {
    pub fn new(contact: bool) -> Self {
        Self {
            contact,
            detonating: false,
        }
    }
}

impl Script for ProximityGrenade {
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        _time: &Time,
    ) -> Effect {
        if self.detonating || linked_peer(world, entity_id).is_some() {
            return Effect::NoEffect;
        }
        if self.contact || physics.is_entity_sleeping(entity_id) {
            Effect::Multiple(vec![
                Effect::ArmProximityGrenade { entity_id },
                script_util::change_to_last_model(world, entity_id),
            ])
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
        if self.detonating || !matches!(msg, MessagePayload::Damage { amount, .. } if *amount > 0.0)
        {
            return Effect::NoEffect;
        }
        self.detonating = true;
        match linked_peer(world, entity_id) {
            Some(trigger) => Effect::Multiple(vec![
                Effect::SlayEntity { entity_id: trigger },
                Effect::DestroyEntity { entity_id },
            ]),
            None => Effect::SlayEntity { entity_id },
        }
    }
}

#[derive(Default)]
pub struct ProximityTrigger {
    detonating: bool,
}

impl Script for ProximityTrigger {
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        _time: &Time,
    ) -> Effect {
        if self.detonating {
            return Effect::NoEffect;
        }
        let owner = linked_peer(world, entity_id);
        if owner.is_some_and(|id| !world.borrow::<EntitiesView>().unwrap().is_alive(id)) {
            return Effect::DestroyEntity { entity_id };
        }
        // General sensor enter events currently cover the player only. Query
        // actual collider overlap for AI, including AI already inside at arm
        // time or after a load. Scenery and the player cannot trip this sensor.
        let ai = world.borrow::<View<PropAI>>().unwrap();
        let hp = world.borrow::<View<PropHitPoints>>().unwrap();
        let triggered = (&ai, &hp)
            .iter()
            .with_id()
            .any(|(id, (_, hp))| hp.hit_points > 0 && physics.entities_overlap(entity_id, id));
        if !triggered {
            return Effect::NoEffect;
        }
        self.detonating = true;
        let mut effects = vec![Effect::SlayEntity { entity_id }];
        if let Some(entity_id) = owner {
            effects.push(Effect::DestroyEntity { entity_id });
        }
        Effect::Multiple(effects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::{CollisionGroup, DynamicPhysicsOptions, PhysicsShape};
    use cgmath::{Quaternion, vec3};
    use dark::properties::{Links, ToLink, WrappedEntityId};

    fn link(world: &mut World, from: EntityId, to: EntityId) {
        world.add_component(
            from,
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(to)),
                    link: Link::ScriptParams,
                }],
            },
        );
    }

    fn body(physics: &mut PhysicsWorld, id: EntityId, x: f32, size: f32, sensor: bool) {
        physics.add_kinematic(
            id,
            vec3(x, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(size, size, size),
            CollisionGroup::actor(),
            sensor,
        );
    }

    #[test]
    fn bounce_arms_only_when_the_body_sleeps() {
        let mut world = World::new();
        let mine = world.add_entity(Links { to_links: vec![] });
        let mut physics = PhysicsWorld::new();
        let handle = physics.add_dynamic(
            mine,
            vec3(0.0, 2.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Sphere(0.2),
            CollisionGroup::actor(),
            false,
            DynamicPhysicsOptions::default(),
        );
        let mut script = ProximityGrenade::new(false);
        assert!(matches!(
            script.update(mine, &world, &physics, &Time::default()),
            Effect::NoEffect
        ));
        physics.sleep_body(handle);
        let Effect::Multiple(effects) = script.update(mine, &world, &physics, &Time::default())
        else {
            panic!("sleep must arm");
        };
        assert!(
            effects.iter().any(
                |e| matches!(e, Effect::ArmProximityGrenade {entity_id} if *entity_id == mine)
            )
        );
        let trigger = world.add_entity(());
        link(&mut world, mine, trigger);
        assert!(matches!(
            script.update(mine, &world, &physics, &Time::default()),
            Effect::NoEffect
        ));
        // Arming lives in the persisted Links; a new script instance after load
        // does not duplicate the trigger even though its physics starts awake.
        assert!(matches!(
            ProximityGrenade::new(false).update(mine, &world, &physics, &Time::default()),
            Effect::NoEffect
        ));
    }

    #[test]
    fn contact_mine_arms_without_a_dynamic_body() {
        let mut world = World::new();
        let mine = world.add_entity(Links { to_links: vec![] });
        assert!(matches!(
            ProximityGrenade::new(true).update(
                mine,
                &world,
                &PhysicsWorld::new(),
                &Time::default()
            ),
            Effect::Multiple(_)
        ));
        assert!(matches!(
            ProximityGrenade::new(false).update(
                mine,
                &world,
                &PhysicsWorld::new(),
                &Time::default()
            ),
            Effect::NoEffect
        ));
    }

    #[test]
    fn sensor_requires_live_ai_and_exact_overlap_then_fires_once() {
        for (ai, hp, x, expected) in [
            (false, 10, 0.0, false),
            (true, 0, 0.0, false),
            (true, 10, 5.0, false),
            (true, 10, 2.7, true),
        ] {
            let mut world = World::new();
            let owner = world.add_entity(());
            let trigger = world.add_entity(());
            link(&mut world, trigger, owner);
            let target = world.add_entity(PropHitPoints { hit_points: hp });
            if ai {
                world.add_component(target, PropAI("Grunt".into()));
            }
            let mut physics = PhysicsWorld::new();
            body(&mut physics, trigger, 0.0, 4.8, true);
            body(&mut physics, target, x, 1.0, false);
            let mut script = ProximityTrigger::default();
            let effect = script.update(trigger, &world, &physics, &Time::default());
            assert_eq!(matches!(effect, Effect::Multiple(_)), expected);
            if expected {
                let Effect::Multiple(effects) = effect else {
                    unreachable!()
                };
                assert!(
                    matches!(effects[0],Effect::SlayEntity {entity_id} if entity_id == trigger)
                );
                assert!(
                    matches!(effects[1],Effect::DestroyEntity {entity_id} if entity_id == owner)
                );
                assert!(matches!(
                    script.update(trigger, &world, &physics, &Time::default()),
                    Effect::NoEffect
                ));
            }
        }
    }

    #[test]
    fn overlap_uses_moved_body_pose_before_the_next_physics_step() {
        let mut world = World::new();
        let trigger = world.add_entity(());
        let target = world.add_entity(());
        let mut physics = PhysicsWorld::new();
        body(&mut physics, trigger, 0.0, 4.8, true);
        body(&mut physics, target, 10.0, 1.0, false);
        assert!(!physics.entities_overlap(trigger, target));
        physics.sync_sensor_position_rotation(
            trigger,
            vec3(10.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
        );
        assert!(physics.entities_overlap(trigger, target));
    }

    #[test]
    fn shot_armed_mine_slays_only_its_sensor() {
        let mut world = World::new();
        let mine = world.add_entity(());
        let trigger = world.add_entity(());
        link(&mut world, mine, trigger);
        let mut script = ProximityGrenade::new(false);
        let damage = MessagePayload::Damage {
            amount: 1.0,
            impact: None,
        };
        let Effect::Multiple(effects) =
            script.handle_message(mine, &world, &PhysicsWorld::new(), &damage)
        else {
            panic!("must detonate");
        };
        assert!(matches!(effects[0],Effect::SlayEntity {entity_id} if entity_id == trigger));
        assert!(matches!(effects[1],Effect::DestroyEntity {entity_id} if entity_id == mine));
        assert!(matches!(
            script.handle_message(mine, &world, &PhysicsWorld::new(), &damage),
            Effect::NoEffect
        ));
    }
}
