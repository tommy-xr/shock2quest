use std::collections::HashMap;

use dark::{
    EnvSoundQuery,
    properties::{Link, Links, PropClassTag, PropPosition},
};
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, IntoIter, IntoWithId, View, World};

use crate::physics::PhysicsWorld;
use crate::time::Time;

use super::{Effect, Message, MessagePayload, Script};

/// How long (seconds) a charged item must stay out of contact before holding
/// it to the station charges it again. Also absorbs single-frame raycast
/// jitter (and re-fired Collided edges from a held item resting against the
/// station) so a wobbling hand doesn't re-trigger the charge.
const CONTACT_RELEASE_SECONDS: f32 = 0.5;

/// How long (seconds) the station's attached `RechargeFX` particle group
/// plays after a charge. The mission data authors the group as a continuous
/// emitter with no duration of its own, so the station pulses it: on at each
/// charge, off when this expires (roughly the activate sound's length).
const FX_PULSE_SECONDS: f32 = 2.0;

pub struct EnergyStation {
    // VR hover/contact latch: each item currently pressed to the station,
    // keyed per item (both hands can press one item each) with the seconds
    // since its contact was last seen. Hover arrives every frame the hand ray
    // touches the station, so without the latch the charge + sound replay per
    // frame. Transient proximity state - deliberately not saved: after a load
    // the worst case is one extra charge of an already-full item.
    contacts: HashMap<EntityId, f32>,
    /// Seconds left on the current RechargeFX pulse; <= 0 means the FX is off.
    /// Transient like `contacts` - a pulse lost to a save/load is cosmetic.
    fx_seconds_left: f32,
    /// The mission data authors the FX group `is_active: true`, so it would
    /// otherwise glow forever; the first update turns it off. False again
    /// after a load, which re-normalizes the restored state.
    fx_normalized: bool,
}
impl EnergyStation {
    pub fn new() -> EnergyStation {
        EnergyStation {
            contacts: HashMap::new(),
            fx_seconds_left: 0.0,
            fx_normalized: false,
        }
    }

    /// Start (or extend) the charge FX pulse on the station's attached
    /// particle group.
    fn begin_fx_pulse(&mut self, world: &World, entity_id: EntityId) -> Effect {
        self.fx_seconds_left = FX_PULSE_SECONDS;
        self.fx_normalized = true;
        set_attached_fx_active(world, entity_id, true)
    }

    /// Charge `with` once per continuous interaction: first contact charges,
    /// repeated contact only renews the latch until the item leaves. A fresh
    /// item arriving while another contact is still fresh charges *quietly* -
    /// the activate sound already played for this interaction (this covers
    /// the charged cell that replaces a dead one mid-hold, and a second held
    /// item pressed alongside the first).
    fn contact_recharge(&mut self, world: &World, entity_id: EntityId, with: &EntityId) -> Effect {
        let other_contact_fresh = self.contacts.keys().any(|item| item != with);
        let renewed = self.contacts.insert(*with, 0.0).is_some();
        if renewed {
            Effect::NoEffect
        } else if other_contact_fresh {
            Effect::combine(vec![
                recharge_message(with),
                self.begin_fx_pulse(world, entity_id),
            ])
        } else {
            Effect::combine(vec![
                recharge_message(with),
                activate_sound(world, entity_id),
                self.begin_fx_pulse(world, entity_id),
            ])
        }
    }
}

impl Script for EnergyStation {
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let dt = time.elapsed.as_secs_f32();
        if !self.contacts.is_empty() {
            self.contacts.retain(|_, age| {
                *age += dt;
                *age <= CONTACT_RELEASE_SECONDS
            });
        }
        // The authored FX group ships active - switch it off until a charge
        // pulses it (and after a load, re-normalize the restored state).
        if !self.fx_normalized {
            self.fx_normalized = true;
            if self.fx_seconds_left <= 0.0 {
                return set_attached_fx_active(world, entity_id, false);
            }
        }
        // Expire the charge pulse.
        if self.fx_seconds_left > 0.0 {
            self.fx_seconds_left -= dt;
            if self.fx_seconds_left <= 0.0 {
                return set_attached_fx_active(world, entity_id, false);
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
            MessagePayload::Frob => Effect::combine(vec![
                do_recharge_all(world, entity_id),
                self.begin_fx_pulse(world, entity_id),
            ]),
            _ => Effect::NoEffect,
        }
    }
}

fn do_recharge_all(world: &World, entity_id: EntityId) -> Effect {
    let mut effects: Vec<Effect> = super::script_util::player_carried_items(world)
        .into_iter()
        .map(|item| recharge_message(&item))
        .collect();
    effects.push(activate_sound(world, entity_id));
    Effect::combine(effects)
}

/// Toggle the particle groups attached to this station. The mission data
/// authors one `RechargeFX` entity per station, linked BY the FX entity via a
/// concrete `ParticleAttachement` link (FX -> station), so the FX is found by
/// scanning incoming links.
fn set_attached_fx_active(world: &World, station_id: EntityId, active: bool) -> Effect {
    let v_links = world.borrow::<View<Links>>().unwrap();
    let effects: Vec<Effect> = v_links
        .iter()
        .with_id()
        .filter(|(_, links)| {
            links.to_links.iter().any(|l| {
                matches!(l.link, Link::ParticleAttachement(_))
                    && l.to_entity_id.map(|e| e.0) == Some(station_id)
            })
        })
        .map(|(id, _)| Effect::SetParticleActive {
            entity_id: id,
            active,
        })
        .collect();
    Effect::combine(effects)
}

fn recharge_message(to: &EntityId) -> Effect {
    Effect::Send {
        msg: Message {
            to: *to,
            payload: MessagePayload::Recharge,
        },
    }
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use cgmath::{Quaternion, vec3};
    use dark::properties::{
        Link, Links, ParticleAttachOptions, PropPosition, ToLink, WrappedEntityId,
    };
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

    fn tick(script: &mut EnergyStation, station: EntityId, world: &World, seconds: f32) -> Effect {
        let time = Time {
            elapsed: Duration::from_secs_f32(seconds),
            total: Duration::ZERO,
        };
        script.update(station, world, &PhysicsWorld::new(), &time)
    }

    /// Add a mission-style RechargeFX entity: a concrete ParticleAttachement
    /// link from the FX entity to its station.
    fn add_fx(world: &mut World, station: EntityId) -> EntityId {
        world.add_entity(Links {
            to_links: vec![ToLink {
                to_template_id: 0,
                to_entity_id: Some(WrappedEntityId(station)),
                link: Link::ParticleAttachement(ParticleAttachOptions {
                    attach_type: 0,
                    vhot: 0,
                    joint: 0,
                    submodel: 0,
                }),
            }],
        })
    }

    /// Collect (entity, active) from every SetParticleActive in the tree.
    fn fx_toggles(effect: Effect) -> Vec<(EntityId, bool)> {
        Effect::flatten(vec![effect])
            .into_iter()
            .filter_map(|e| match e {
                Effect::SetParticleActive { entity_id, active } => Some((entity_id, active)),
                _ => None,
            })
            .collect()
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
    fn alternating_items_each_charge_once_with_one_sound() {
        // Two hands holding two items at the station: Hover alternates targets
        // every frame. Each item charges once; the sound plays once for the
        // whole interaction (the second item joins an already-audible charge).
        let (mut world, station, item_a) = make_world();
        let item_b = world.add_entity(());
        let mut script = EnergyStation::new();
        let physics = PhysicsWorld::new();

        let mut totals = (0, 0);
        for _ in 0..60 {
            for item in [item_a, item_b] {
                let (r, s) =
                    charge_events(script.handle_message(station, &world, &physics, &hover(item)));
                totals.0 += r;
                totals.1 += s;
            }
            tick(&mut script, station, &world, 1.0 / 60.0);
        }

        assert_eq!(totals, (2, 1), "each item charges once, one sound total");
    }

    #[test]
    fn replacement_item_charges_quietly_mid_hold() {
        // Recharging a dead power cell replaces it with a NEW entity still in
        // the hand; that fresh id must not replay the activate sound.
        let (mut world, station, dead_cell) = make_world();
        let charged_cell = world.add_entity(());
        let mut script = EnergyStation::new();
        let physics = PhysicsWorld::new();

        let first =
            charge_events(script.handle_message(station, &world, &physics, &hover(dead_cell)));
        assert_eq!(first, (1, 1));

        // Next frame the hand holds the replacement entity.
        tick(&mut script, station, &world, 1.0 / 60.0);
        let second =
            charge_events(script.handle_message(station, &world, &physics, &hover(charged_cell)));
        assert_eq!(second, (1, 0), "recharge sent, sound not replayed");
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

    #[test]
    fn charge_pulses_attached_fx_once_per_interaction() {
        let (mut world, station, item) = make_world();
        let fx = add_fx(&mut world, station);
        let mut script = EnergyStation::new();
        let physics = PhysicsWorld::new();

        // First contact turns the FX on...
        let first = fx_toggles(script.handle_message(station, &world, &physics, &hover(item)));
        assert_eq!(first, vec![(fx, true)]);

        // ...latched repeat contacts don't re-emit it...
        tick(&mut script, station, &world, 1.0 / 60.0);
        let repeat = fx_toggles(script.handle_message(station, &world, &physics, &hover(item)));
        assert_eq!(repeat, vec![], "latched contact must not re-pulse the FX");

        // ...and the pulse expires back to off.
        let mut offs = Vec::new();
        for _ in 0..180 {
            offs.extend(fx_toggles(tick(&mut script, station, &world, 1.0 / 60.0)));
        }
        assert_eq!(offs, vec![(fx, false)], "pulse turns off exactly once");
    }

    #[test]
    fn frob_pulses_attached_fx() {
        let (mut world, station, _item) = make_world();
        let fx = add_fx(&mut world, station);
        let mut script = EnergyStation::new();
        let physics = PhysicsWorld::new();

        let toggles =
            fx_toggles(script.handle_message(station, &world, &physics, &MessagePayload::Frob));
        assert_eq!(toggles, vec![(fx, true)]);
    }

    #[test]
    fn first_update_turns_authored_fx_off() {
        // The mission data authors the FX group active; the station normalizes
        // it off before any charge.
        let (mut world, station, _item) = make_world();
        let fx = add_fx(&mut world, station);
        let mut script = EnergyStation::new();

        let first = fx_toggles(tick(&mut script, station, &world, 1.0 / 60.0));
        assert_eq!(first, vec![(fx, false)]);
        let second = fx_toggles(tick(&mut script, station, &world, 1.0 / 60.0));
        assert_eq!(second, vec![], "normalization happens once");
    }
}
