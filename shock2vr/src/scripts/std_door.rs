use cgmath::{InnerSpace, Vector3, Zero};
use dark::properties::{Link, Links, PropKeypadCode, PropTranslatingDoor};
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, IntoIter, View, World};
use tracing::trace;

use crate::{physics::PhysicsWorld, time::Time};

use super::{
    Effect, Message, MessagePayload, Script,
    script_util::{is_entity_locked, play_environmental_sound},
};

/// Whether a numeric keypad owns this door through an authored SwitchLink.
///
/// Retail keypad gates put `PropLocked` and the code on the controller, not on
/// the linked door. The keypad's successful `TurnOn` is therefore the only
/// player path that may open a closed target; treating the target as an
/// ordinary unlocked door lets a direct Frob bypass the code entirely.
fn has_incoming_keypad_switch(world: &World, door: EntityId) -> bool {
    let links = world.borrow::<View<Links>>().unwrap();
    let keypad_codes = world.borrow::<View<PropKeypadCode>>().unwrap();

    (&links, &keypad_codes).iter().any(|(links, _code)| {
        links.to_links.iter().any(|link| {
            matches!(link.link, Link::SwitchLink)
                && link.to_entity_id.is_some_and(|target| target.0 == door)
        })
    })
}

pub struct StdDoor {
    audio_handle: AudioHandle,
    current_position: Vector3<f32>,
    desired_position: Vector3<f32>,
    is_moving: bool,
}

impl StdDoor {
    pub fn new() -> StdDoor {
        StdDoor {
            audio_handle: AudioHandle::new(),
            current_position: Vector3::zero(),
            desired_position: Vector3::zero(),
            is_moving: false,
        }
    }

    fn target_is_open(&self, trans_door: &PropTranslatingDoor) -> bool {
        (self.desired_position - trans_door.base_open_location).magnitude2()
            < (self.desired_position - trans_door.base_closed_location).magnitude2()
    }

    /// Door motion is persisted through the effect pipeline (applied by
    /// `mission_core`) rather than written here - scripts stay pure.
    fn persist_motion(entity_id: EntityId, state: i32, position: Vector3<f32>) -> Effect {
        Effect::SetTranslatingDoorState {
            entity_id,
            state,
            base_location: position,
        }
    }

    fn open(
        &mut self,
        entity_id: EntityId,
        world: &World,
        trans_door: &PropTranslatingDoor,
    ) -> Effect {
        if self.target_is_open(trans_door) {
            return Effect::NoEffect;
        }

        self.desired_position = trans_door.base_open_location;
        self.is_moving = true;
        Effect::Combined {
            effects: vec![
                Self::persist_motion(entity_id, 3, self.current_position),
                play_environmental_sound(
                    world,
                    entity_id,
                    "statechange",
                    vec![("openstate", "opening"), ("oldopenstate", "closed")],
                    self.audio_handle.clone(),
                ),
            ],
        }
    }

    fn close(
        &mut self,
        entity_id: EntityId,
        world: &World,
        trans_door: &PropTranslatingDoor,
    ) -> Effect {
        self.desired_position = trans_door.base_closed_location;
        self.is_moving = true;
        Effect::Combined {
            effects: vec![
                Self::persist_motion(entity_id, 2, self.current_position),
                play_environmental_sound(
                    world,
                    entity_id,
                    "statechange",
                    vec![("openstate", "closing"), ("oldopenstate", "open")],
                    self.audio_handle.clone(),
                ),
            ],
        }
    }
}
impl Script for StdDoor {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let trans_door = {
            let doors = world.borrow::<View<PropTranslatingDoor>>().unwrap();
            doors.get(entity_id).ok().cloned()
        };
        if let Some(trans_door) = trans_door {
            // Respect the authored door state - a door saved open must start
            // open. Snapping everything to base_closed_location shut doors the
            // level designer left open.
            let initial = trans_door.initial_location();
            self.current_position = initial;
            match trans_door.state {
                2 => {
                    self.desired_position = trans_door.base_closed_location;
                    self.is_moving = true;
                }
                3 => {
                    self.desired_position = trans_door.base_open_location;
                    self.is_moving = true;
                }
                _ => {
                    self.desired_position = initial;
                    self.is_moving = false;
                }
            }

            Effect::SetPosition {
                entity_id,
                position: initial,
            }
        } else {
            Effect::NoEffect
        }
    }
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        //println!("Updating door: {:?}", entity_id);
        let _lerpval = (f32::cos(time.total.as_secs_f32()) + 1.0) * 0.5;

        let trans_door = {
            let doors = world.borrow::<View<PropTranslatingDoor>>().unwrap();
            doors.get(entity_id).ok().cloned()
        };
        if let Some(trans_door) = trans_door {
            let dir = self.desired_position - self.current_position;
            let step = time.elapsed.as_secs_f32() * trans_door.speed;
            // Complete once within one frame-step of the target - stepping by
            // a fixed amount can otherwise overshoot and oscillate around a
            // small distance threshold forever, so the door never finishes.
            if dir.magnitude() > step.max(0.001) {
                let normalized = dir.normalize();

                trace!(
                    "desired: {:?} current: {:?} dir: {:?}",
                    self.desired_position, self.current_position, normalized
                );

                self.current_position += normalized * step;
                Effect::Combined {
                    effects: vec![
                        Effect::SetPosition {
                            entity_id,
                            position: self.current_position,
                        },
                        Self::persist_motion(
                            entity_id,
                            if self.target_is_open(&trans_door) {
                                3
                            } else {
                                2
                            },
                            self.current_position,
                        ),
                    ],
                }
            } else if self.is_moving {
                self.is_moving = false;
                // Announce which endpoint we reached to this entity's other
                // scripts (the Dark engine's DoorOpen/DoorClose messages);
                // e.g. CS9_DoorReporter forwards these to the cutscene master.
                let reached_open =
                    (self.desired_position - trans_door.base_open_location).magnitude2() < 0.001;
                self.current_position = self.desired_position;
                let state_signal = Effect::Send {
                    msg: Message {
                        to: entity_id,
                        payload: MessagePayload::Signal {
                            name: if reached_open {
                                "DoorOpen".to_string()
                            } else {
                                "DoorClose".to_string()
                            },
                        },
                    },
                };
                Effect::Combined {
                    effects: vec![
                        Effect::SetPosition {
                            entity_id,
                            position: self.desired_position,
                        },
                        Self::persist_motion(
                            entity_id,
                            if reached_open { 1 } else { 0 },
                            self.desired_position,
                        ),
                        play_environmental_sound(
                            world,
                            entity_id,
                            "statechange",
                            vec![("openstate", "closed"), ("oldopenstate", "closing")],
                            self.audio_handle.clone(),
                        ),
                        state_signal,
                    ],
                }
            } else {
                Effect::NoEffect
            }
        } else {
            Effect::NoEffect
        }
        // Effect::SetPosition {
        //     entity_id,
        //     position,
        // }
    }

    fn handle_message(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        let trans_door = {
            let doors = world.borrow::<View<PropTranslatingDoor>>().unwrap();
            doors.get(entity_id).ok().cloned()
        };

        if let Some(trans_door) = trans_door {
            // A permanently open doorway has no collider and no travel, so it
            // can't be frobbed and none of the retail ones are switch-linked -
            // but keep the invariant explicit: nothing may "close" an opening
            // that has nowhere to move to (it would only replay door sounds).
            if trans_door.is_permanently_open() {
                return Effect::NoEffect;
            }
            match msg {
                MessagePayload::Frob => {
                    // The original StdDoor toggles on player FrobWorldEnd. A
                    // locked, closed door rejects that player-driven open,
                    // while scripted TurnOn below deliberately bypasses locks.
                    if self.target_is_open(&trans_door) {
                        self.close(entity_id, world, &trans_door)
                    } else if is_entity_locked(world, entity_id)
                        || has_incoming_keypad_switch(world, entity_id)
                    {
                        // SS2's existing locked-control feedback; the player
                        // still gets a response without changing door state.
                        Effect::PlaySound {
                            handle: AudioHandle::new(),
                            source: Some(entity_id),
                            name: "hackfail".to_owned(),
                            spatial: false,
                        }
                    } else {
                        self.open(entity_id, world, &trans_door)
                    }
                }
                MessagePayload::TurnOn { from: _ } => {
                    // Idempotent: if we're already headed open, ignore repeat
                    // opens (e.g. several AIs converging on the same door) so
                    // the opening sound isn't replayed each frame.
                    self.open(entity_id, world, &trans_door)
                }
                MessagePayload::TurnOff { from: _ } => {
                    //self.current_position = trans_door.base_closed_location;
                    self.close(entity_id, world, &trans_door)
                }
                _ => Effect::NoEffect,
            }
        } else {
            Effect::NoEffect
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use cgmath::{Matrix4, vec3};
    use dark::properties::{
        KeyCard, Link, Links, PropClassTag, PropKeyDst, PropKeypadCode, PropLocked, ToLink,
        WrappedEntityId,
    };

    use crate::{quest_info::QuestInfo, runtime_props::RuntimePropTransform};

    use super::*;

    fn key_card() -> KeyCard {
        KeyCard {
            is_master: false,
            region_id: 2,
            lock_id: 7,
        }
    }

    fn test_world(locked: bool, has_key: bool) -> (World, EntityId) {
        let mut world = World::new();
        let closed = vec3(1.0, 2.0, 3.0);
        let open = vec3(1.0, 4.0, 3.0);
        let entity_id = world.add_entity((
            PropTranslatingDoor {
                door_type: 1,
                closed: 0.0,
                open: 2.0,
                speed: 4.0,
                axis: 1,
                state: 0,
                base_closed_location: closed,
                base_open_location: open,
                base_location: closed,
            },
            PropLocked(locked),
            PropKeyDst(key_card()),
            PropClassTag::from_string("doortype scidoor"),
            RuntimePropTransform(Matrix4::from_translation(closed)),
        ));
        let mut quest = QuestInfo::new();
        if has_key {
            quest.add_key_card(key_card());
        }
        world.add_unique(quest);
        (world, entity_id)
    }

    fn initialized_door(entity_id: EntityId, world: &World) -> StdDoor {
        let mut door = StdDoor::new();
        door.initialize(entity_id, world);
        door
    }

    fn add_keypad_controller(world: &mut World, door: EntityId) -> EntityId {
        world.add_entity((
            PropKeypadCode(15061),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 1091,
                    to_entity_id: Some(WrappedEntityId(door)),
                    link: Link::SwitchLink,
                }],
            },
        ))
    }

    /// A doorway authored permanently open: no travel, state open (#602).
    fn permanently_open_world() -> (World, EntityId) {
        let mut world = World::new();
        let at = vec3(18.0, -0.4, 41.8);
        let entity_id = world.add_entity((
            PropTranslatingDoor {
                door_type: 1,
                closed: 0.0,
                open: 0.0,
                speed: 0.0,
                axis: 0,
                state: 1,
                base_closed_location: at,
                base_open_location: at,
                base_location: at,
            },
            PropClassTag::from_string("doortype scidoor"),
            RuntimePropTransform(Matrix4::from_translation(at)),
        ));
        world.add_unique(QuestInfo::new());
        (world, entity_id)
    }

    #[test]
    fn a_permanently_open_door_ignores_frob_and_turn_off() {
        let (world, entity_id) = permanently_open_world();
        let physics = PhysicsWorld::new();
        let mut door = initialized_door(entity_id, &world);

        for msg in [
            MessagePayload::Frob,
            MessagePayload::TurnOff { from: entity_id },
            MessagePayload::TurnOn { from: entity_id },
        ] {
            let effect = door.handle_message(entity_id, &world, &physics, &msg);
            assert!(matches!(effect, Effect::NoEffect));
            assert!(!door.is_moving);
        }
    }

    #[test]
    fn frob_opens_an_unlocked_closed_door() {
        let (world, entity_id) = test_world(false, false);
        let physics = PhysicsWorld::new();
        let mut door = initialized_door(entity_id, &world);

        door.handle_message(entity_id, &world, &physics, &MessagePayload::Frob);

        assert_eq!(door.desired_position, vec3(1.0, 4.0, 3.0));
        assert!(door.is_moving);
    }

    #[test]
    fn frob_does_not_open_a_closed_door_controlled_by_a_keypad() {
        let (mut world, entity_id) = test_world(false, false);
        let keypad = add_keypad_controller(&mut world, entity_id);
        let physics = PhysicsWorld::new();
        let mut door = initialized_door(entity_id, &world);

        door.handle_message(entity_id, &world, &physics, &MessagePayload::Frob);

        assert_eq!(door.desired_position, vec3(1.0, 2.0, 3.0));
        assert!(!door.is_moving);

        door.handle_message(
            entity_id,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: keypad },
        );

        assert_eq!(door.desired_position, vec3(1.0, 4.0, 3.0));
        assert!(door.is_moving);
    }

    #[test]
    fn frob_toggles_an_opening_door_back_toward_closed() {
        let (world, entity_id) = test_world(false, false);
        let physics = PhysicsWorld::new();
        let mut door = initialized_door(entity_id, &world);

        door.handle_message(entity_id, &world, &physics, &MessagePayload::Frob);
        door.handle_message(entity_id, &world, &physics, &MessagePayload::Frob);

        assert_eq!(door.desired_position, vec3(1.0, 2.0, 3.0));
        assert!(door.is_moving);
    }

    #[test]
    fn frob_does_not_open_a_locked_closed_door_without_its_key() {
        let (world, entity_id) = test_world(true, false);
        let physics = PhysicsWorld::new();
        let mut door = initialized_door(entity_id, &world);

        door.handle_message(entity_id, &world, &physics, &MessagePayload::Frob);

        assert_eq!(door.desired_position, vec3(1.0, 2.0, 3.0));
        assert!(!door.is_moving);
    }

    #[test]
    fn frob_opens_a_locked_door_when_the_player_has_its_key() {
        let (world, entity_id) = test_world(true, true);
        let physics = PhysicsWorld::new();
        let mut door = initialized_door(entity_id, &world);

        door.handle_message(entity_id, &world, &physics, &MessagePayload::Frob);

        assert_eq!(door.desired_position, vec3(1.0, 4.0, 3.0));
        assert!(door.is_moving);
    }

    #[test]
    fn scripted_turn_on_bypasses_a_player_lock() {
        let (world, entity_id) = test_world(true, false);
        let physics = PhysicsWorld::new();
        let mut door = initialized_door(entity_id, &world);

        door.handle_message(
            entity_id,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: entity_id },
        );

        assert_eq!(door.desired_position, vec3(1.0, 4.0, 3.0));
        assert!(door.is_moving);
    }

    fn step(seconds: f32) -> Time {
        Time {
            elapsed: Duration::from_secs_f32(seconds),
            total: Duration::from_secs_f32(seconds),
        }
    }

    /// Stand-in for `mission_core`'s `Effect::SetTranslatingDoorState` handler -
    /// the script only emits the effect, the world write happens here.
    fn apply(world: &World, effect: Effect) {
        for effect in Effect::flatten(vec![effect]) {
            if let Effect::SetTranslatingDoorState {
                entity_id,
                state,
                base_location,
            } = effect
            {
                let mut doors = world
                    .borrow::<shipyard::ViewMut<PropTranslatingDoor>>()
                    .unwrap();
                if let Ok(door) = (&mut doors).get(entity_id) {
                    door.state = state;
                    door.base_location = base_location;
                }
            }
        }
    }

    #[test]
    fn settled_open_endpoint_is_persisted_in_door_property() {
        let (world, entity_id) = test_world(false, false);
        let physics = PhysicsWorld::new();
        let mut door = initialized_door(entity_id, &world);

        let effect = door.handle_message(entity_id, &world, &physics, &MessagePayload::Frob);
        apply(&world, effect);
        let effect = door.update(entity_id, &world, &physics, &step(1.0));
        apply(&world, effect);

        let doors = world.borrow::<View<PropTranslatingDoor>>().unwrap();
        let saved = doors.get(entity_id).unwrap();
        assert_eq!(saved.state, 1);
        assert_eq!(saved.base_location, saved.base_open_location);
    }

    #[test]
    fn frob_emits_the_opening_state_as_an_effect() {
        let (world, entity_id) = test_world(false, false);
        let physics = PhysicsWorld::new();
        let mut door = initialized_door(entity_id, &world);

        let effect = door.handle_message(entity_id, &world, &physics, &MessagePayload::Frob);

        let persisted = Effect::flatten(vec![effect])
            .into_iter()
            .find_map(|effect| match effect {
                Effect::SetTranslatingDoorState {
                    entity_id: id,
                    state,
                    base_location,
                } if id == entity_id => Some((state, base_location)),
                _ => None,
            });
        assert_eq!(persisted, Some((3, vec3(1.0, 2.0, 3.0))));
    }

    #[test]
    fn mid_opening_state_resumes_from_saved_location() {
        let (world, entity_id) = test_world(false, false);
        {
            let mut doors = world
                .borrow::<shipyard::ViewMut<PropTranslatingDoor>>()
                .unwrap();
            let saved = (&mut doors).get(entity_id).unwrap();
            saved.state = 3;
            saved.base_location = vec3(1.0, 3.0, 3.0);
        }

        let door = initialized_door(entity_id, &world);

        assert_eq!(door.current_position, vec3(1.0, 3.0, 3.0));
        assert_eq!(door.desired_position, vec3(1.0, 4.0, 3.0));
        assert!(door.is_moving);
    }

    #[test]
    fn settled_closed_endpoint_is_persisted_symmetrically() {
        let (world, entity_id) = test_world(false, false);
        {
            let mut doors = world
                .borrow::<shipyard::ViewMut<PropTranslatingDoor>>()
                .unwrap();
            let saved = (&mut doors).get(entity_id).unwrap();
            saved.state = 1;
            saved.base_location = saved.base_open_location;
        }
        let physics = PhysicsWorld::new();
        let mut door = initialized_door(entity_id, &world);

        let effect = door.handle_message(entity_id, &world, &physics, &MessagePayload::Frob);
        apply(&world, effect);
        let effect = door.update(entity_id, &world, &physics, &step(1.0));
        apply(&world, effect);

        let doors = world.borrow::<View<PropTranslatingDoor>>().unwrap();
        let saved = doors.get(entity_id).unwrap();
        assert_eq!(saved.state, 0);
        assert_eq!(saved.base_location, saved.base_closed_location);
    }
}
