use cgmath::{InnerSpace, Vector3, Zero};
use dark::properties::PropTranslatingDoor;
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};
use tracing::trace;

use crate::{physics::PhysicsWorld, time::Time};

use super::{
    Effect, Message, MessagePayload, Script,
    script_util::{is_entity_locked, play_environmental_sound},
};

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
        play_environmental_sound(
            world,
            entity_id,
            "statechange",
            vec![("openstate", "opening"), ("oldopenstate", "closed")],
            self.audio_handle.clone(),
        )
    }

    fn close(
        &mut self,
        entity_id: EntityId,
        world: &World,
        trans_door: &PropTranslatingDoor,
    ) -> Effect {
        self.desired_position = trans_door.base_closed_location;
        self.is_moving = true;
        play_environmental_sound(
            world,
            entity_id,
            "statechange",
            vec![("openstate", "closing"), ("oldopenstate", "open")],
            self.audio_handle.clone(),
        )
    }
}
impl Script for StdDoor {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let v_trans_door = world.borrow::<View<PropTranslatingDoor>>().unwrap();
        if let Ok(trans_door) = v_trans_door.get(entity_id) {
            // Respect the authored door state - a door saved open must start
            // open. Snapping everything to base_closed_location shut doors the
            // level designer left open.
            let initial = trans_door.initial_location();
            self.desired_position = initial;
            self.current_position = initial;

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

        let v_trans_door = world.borrow::<View<PropTranslatingDoor>>().unwrap();
        if let Ok(trans_door) = v_trans_door.get(entity_id) {
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
                Effect::SetPosition {
                    entity_id,
                    position: self.current_position,
                }
            } else if self.is_moving {
                self.is_moving = false;
                // Announce which endpoint we reached to this entity's other
                // scripts (the Dark engine's DoorOpen/DoorClose messages);
                // e.g. CS9_DoorReporter forwards these to the cutscene master.
                let reached_open =
                    (self.desired_position - trans_door.base_open_location).magnitude2() < 0.001;
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
        let v_trans_door = world.borrow::<View<PropTranslatingDoor>>().unwrap();

        if let Ok(trans_door) = v_trans_door.get(entity_id) {
            match msg {
                MessagePayload::Frob => {
                    // The original StdDoor toggles on player FrobWorldEnd. A
                    // locked, closed door rejects that player-driven open,
                    // while scripted TurnOn below deliberately bypasses locks.
                    if self.target_is_open(trans_door) {
                        self.close(entity_id, world, trans_door)
                    } else if is_entity_locked(world, entity_id) {
                        // SS2's existing locked-control feedback; the player
                        // still gets a response without changing door state.
                        Effect::PlaySound {
                            handle: AudioHandle::new(),
                            name: "hackfail".to_owned(),
                        }
                    } else {
                        self.open(entity_id, world, trans_door)
                    }
                }
                MessagePayload::TurnOn { from: _ } => {
                    // Idempotent: if we're already headed open, ignore repeat
                    // opens (e.g. several AIs converging on the same door) so
                    // the opening sound isn't replayed each frame.
                    self.open(entity_id, world, trans_door)
                }
                MessagePayload::TurnOff { from: _ } => {
                    //self.current_position = trans_door.base_closed_location;
                    self.close(entity_id, world, trans_door)
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
    use cgmath::{Matrix4, vec3};
    use dark::properties::{KeyCard, PropClassTag, PropKeyDst, PropLocked};

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
}
