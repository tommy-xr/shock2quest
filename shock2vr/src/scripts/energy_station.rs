use dark::{
    EnvSoundQuery,
    properties::{PropClassTag, PropPosition},
};
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;
use crate::time::Time;

use super::{Effect, Message, MessagePayload, Script};

/// How long (seconds) a charged item must stay out of contact before holding
/// it to the station charges it again. Also absorbs single-frame raycast
/// jitter so a wobbling hand doesn't re-trigger the charge.
const CONTACT_RELEASE_SECONDS: f32 = 0.5;

pub struct EnergyStation {
    // VR hover/contact latch: the item currently pressed to the station, and
    // how long since its contact was last seen. Hover arrives every frame the
    // hand ray touches the station, so without the latch the charge + sound
    // replay per frame. Transient proximity state - deliberately not saved:
    // after a load the worst case is one extra charge of an already-full item.
    contact_target: Option<EntityId>,
    seconds_since_contact: f32,
}
impl EnergyStation {
    pub fn new() -> EnergyStation {
        EnergyStation {
            contact_target: None,
            seconds_since_contact: 0.0,
        }
    }

    /// Charge `with` once per continuous interaction: first contact charges,
    /// repeated contact only renews the latch until the item leaves.
    fn contact_recharge(&mut self, world: &World, entity_id: EntityId, with: &EntityId) -> Effect {
        let renewed = self.contact_target == Some(*with);
        self.contact_target = Some(*with);
        self.seconds_since_contact = 0.0;
        if renewed {
            Effect::NoEffect
        } else {
            do_recharge(world, entity_id, with)
        }
    }
}

impl Script for EnergyStation {
    fn update(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        if self.contact_target.is_some() {
            self.seconds_since_contact += time.elapsed.as_secs_f32();
            if self.seconds_since_contact > CONTACT_RELEASE_SECONDS {
                self.contact_target = None;
            }
        }
        Effect::NoEffect
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::Hover {
                held_entity_id,
                world_position: _,
                is_triggered: _,
                is_grabbing: _,
                hand: _,
            } => {
                if let Some(with) = held_entity_id {
                    self.contact_recharge(world, entity_id, with)
                } else {
                    Effect::NoEffect
                }
            }
            MessagePayload::Collided { with, .. } => self.contact_recharge(world, entity_id, with),
            // Frobbing the station (flat click, or a VR empty-hand trigger
            // press) sends Recharge to every item the player carries at once.
            // Each item self-handles Recharge, so only responders react - dead
            // power cells replace themselves with charged cells, while energy
            // weapons replenish their internal charge. The VR-only
            // hold-one-item-to-the-station step is Hover/Collided above.
            MessagePayload::Frob => do_recharge_all(world, entity_id),
            _ => Effect::NoEffect,
        }
    }
}

fn do_recharge_all(world: &World, entity_id: EntityId) -> Effect {
    let mut effects: Vec<Effect> = super::script_util::player_carried_items(world)
        .into_iter()
        .map(|item| Effect::Send {
            msg: Message {
                to: item,
                payload: MessagePayload::Recharge,
            },
        })
        .collect();
    effects.push(activate_sound(world, entity_id));
    Effect::combine(effects)
}

fn activate_sound(world: &World, entity_id: EntityId) -> Effect {
    let v_pos = world.borrow::<View<PropPosition>>().unwrap();
    let v_class_tag = world.borrow::<View<PropClassTag>>().unwrap();
    let mut class_tags = v_class_tag
        .get(entity_id)
        .map(|p| p.class_tags())
        .unwrap_or(vec![]);
    let pos = v_pos.get(entity_id).unwrap();
    let mut query = vec![("event", "activate")];
    query.append(&mut class_tags);
    Effect::PlayEnvironmentalSound {
        audio_handle: AudioHandle::new(),
        query: EnvSoundQuery::from_tag_values(query),
        position: pos.position,
    }
}

fn do_recharge(world: &World, entity_id: EntityId, with: &EntityId) -> Effect {
    let recharge_effect = Effect::Send {
        msg: Message {
            to: *with,
            payload: MessagePayload::Recharge,
        },
    };
    Effect::combine(vec![recharge_effect, activate_sound(world, entity_id)])
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use cgmath::{Quaternion, vec3};
    use dark::properties::PropPosition;
    use shipyard::{EntityId, World};

    use crate::{physics::PhysicsWorld, time::Time, vr_config::Handedness};

    use super::{Effect, EnergyStation, MessagePayload, Script};

    fn make_world() -> (World, EntityId, EntityId) {
        let mut world = World::new();
        let station = world.add_entity((PropPosition {
            position: vec3(0.0, 0.0, 0.0),
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            cell: 0,
        },));
        let item = world.add_entity(());
        (world, station, item)
    }

    fn hover(item: EntityId) -> MessagePayload {
        MessagePayload::Hover {
            held_entity_id: Some(item),
            world_position: vec3(0.0, 0.0, 0.0),
            is_triggered: false,
            is_grabbing: true,
            hand: Handedness::Right,
        }
    }

    /// Count (recharge sends, sounds) in one returned effect tree.
    fn charge_events(effect: Effect) -> (usize, usize) {
        let flat = Effect::flatten(vec![effect]);
        let recharges = flat
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Effect::Send {
                        msg: super::Message {
                            payload: MessagePayload::Recharge,
                            ..
                        }
                    }
                )
            })
            .count();
        let sounds = flat
            .iter()
            .filter(|e| matches!(e, Effect::PlayEnvironmentalSound { .. }))
            .count();
        (recharges, sounds)
    }

    fn tick(script: &mut EnergyStation, station: EntityId, world: &World, seconds: f32) {
        let time = Time {
            elapsed: Duration::from_secs_f32(seconds),
            total: Duration::ZERO,
        };
        script.update(station, world, &PhysicsWorld::new(), &time);
    }

    #[test]
    fn continuous_hover_charges_once() {
        let (world, station, item) = make_world();
        let mut script = EnergyStation::new();
        let physics = PhysicsWorld::new();

        // Hover arrives every frame while the held item points at the station.
        let mut totals = (0, 0);
        for _ in 0..120 {
            let (r, s) =
                charge_events(script.handle_message(station, &world, &physics, &hover(item)));
            totals.0 += r;
            totals.1 += s;
            tick(&mut script, station, &world, 1.0 / 60.0);
        }

        assert_eq!(totals, (1, 1), "one charge + one sound per interaction");
    }

    #[test]
    fn collided_spam_charges_once() {
        let (world, station, item) = make_world();
        let mut script = EnergyStation::new();
        let physics = PhysicsWorld::new();
        let collided = MessagePayload::Collided {
            with: item,
            contact: None,
        };

        let first = charge_events(script.handle_message(station, &world, &physics, &collided));
        let second = charge_events(script.handle_message(station, &world, &physics, &collided));
        assert_eq!(first, (1, 1));
        assert_eq!(second, (0, 0));
    }

    #[test]
    fn recharges_again_after_item_leaves() {
        let (world, station, item) = make_world();
        let mut script = EnergyStation::new();
        let physics = PhysicsWorld::new();

        let first = charge_events(script.handle_message(station, &world, &physics, &hover(item)));
        assert_eq!(first, (1, 1));

        // Pull the item away for over the release window, then bring it back.
        tick(&mut script, station, &world, 1.0);
        let again = charge_events(script.handle_message(station, &world, &physics, &hover(item)));
        assert_eq!(again, (1, 1), "latch re-arms once contact lapses");
    }

    #[test]
    fn frob_is_not_latched() {
        let (world, station, _item) = make_world();
        let mut script = EnergyStation::new();
        let physics = PhysicsWorld::new();

        // Frob is already edge-triggered upstream; each deliberate use plays.
        for _ in 0..2 {
            let (_, sounds) = charge_events(script.handle_message(
                station,
                &world,
                &physics,
                &MessagePayload::Frob,
            ));
            assert_eq!(sounds, 1);
        }
    }
}
