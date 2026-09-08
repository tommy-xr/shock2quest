use cgmath::{InnerSpace, Vector3, Zero};
use dark::properties::{
    Link, Links, PropDoorTimer, PropKeypadCode, PropRotatingDoor, PropTranslatingDoor,
};
use engine::audio::AudioHandle;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, IntoIter, View, World};
use tracing::trace;

use crate::{physics::PhysicsWorld, time::Time};

use super::{
    Effect, Message, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError,
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

const SCRIPT_STATE_KEY: &str = "std_door";

#[derive(Debug, Deserialize, Serialize)]
struct StdDoorState {
    current_position: Vector3<f32>,
    desired_position: Vector3<f32>,
    is_moving: bool,
    rotation_progress: f32,
    rotation_target_open: bool,
    auto_close_remaining: Option<f32>,
}

pub struct StdDoor {
    audio_handle: AudioHandle,
    current_position: Vector3<f32>,
    desired_position: Vector3<f32>,
    is_moving: bool,
    rotation_progress: f32,
    rotation_target_open: bool,
    auto_close_remaining: Option<f32>,
}

impl StdDoor {
    pub fn new() -> StdDoor {
        StdDoor {
            audio_handle: AudioHandle::new(),
            current_position: Vector3::zero(),
            desired_position: Vector3::zero(),
            is_moving: false,
            rotation_progress: 0.0,
            rotation_target_open: false,
            auto_close_remaining: None,
        }
    }

    fn target_is_open(&self, trans_door: &PropTranslatingDoor) -> bool {
        (self.desired_position - trans_door.base_open_location).magnitude2()
            < (self.desired_position - trans_door.base_closed_location).magnitude2()
    }

    fn automatic_close_delay(world: &World, entity_id: EntityId) -> Option<f32> {
        world
            .borrow::<View<PropDoorTimer>>()
            .ok()?
            .get(entity_id)
            .ok()
            .map(|timer| timer.0 as f32)
            .filter(|seconds| *seconds > 0.0)
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

    fn persist_rotation(entity_id: EntityId, state: i32, progress: f32) -> Effect {
        Effect::SetRotatingDoorState {
            entity_id,
            state,
            progress,
        }
    }

    fn open_rotation(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _rot_door: &PropRotatingDoor,
    ) -> Effect {
        if self.rotation_target_open {
            return Effect::NoEffect;
        }

        self.rotation_target_open = true;
        self.is_moving = true;
        self.auto_close_remaining = None;
        Effect::Combined {
            effects: vec![
                Self::persist_rotation(entity_id, 3, self.rotation_progress),
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

    fn close_rotation(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _rot_door: &PropRotatingDoor,
    ) -> Effect {
        if !self.rotation_target_open {
            return Effect::NoEffect;
        }

        self.rotation_target_open = false;
        self.is_moving = true;
        self.auto_close_remaining = None;
        Effect::Combined {
            effects: vec![
                Self::persist_rotation(entity_id, 2, self.rotation_progress),
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
        let rot_door = {
            let doors = world.borrow::<View<PropRotatingDoor>>().unwrap();
            doors.get(entity_id).ok().cloned()
        };
        if let Some(rot_door) = rot_door {
            // Dark's GetDoorProperty chooses RotDoor before TransDoor. That
            // precedence matters because rotating doors commonly inherit a
            // zero-filled translating property from StdDoor (#861).
            self.rotation_progress = match rot_door.state {
                0 => 0.0,
                1 => 1.0,
                _ => rot_door.progress.clamp(0.0, 1.0),
            };
            self.rotation_target_open = match rot_door.state {
                1 | 3 => true,
                0 | 2 => false,
                _ => self.rotation_progress >= 0.5,
            };
            self.is_moving = matches!(rot_door.state, 2 | 3);
            self.auto_close_remaining = if rot_door.state == 1 {
                Self::automatic_close_delay(world, entity_id)
            } else {
                None
            };
            let (position, rotation) = rot_door.pose_at_progress(self.rotation_progress);
            self.current_position = position;
            self.desired_position = if self.rotation_target_open {
                rot_door.base_open_location
            } else {
                rot_door.base_closed_location
            };

            return Effect::SetPositionRotation {
                entity_id,
                position,
                rotation,
            };
        }

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
        let rot_door = {
            let doors = world.borrow::<View<PropRotatingDoor>>().unwrap();
            doors.get(entity_id).ok().cloned()
        };
        if let Some(rot_door) = rot_door {
            self.rotation_progress = rot_door.progress.clamp(0.0, 1.0);
            self.rotation_target_open = matches!(rot_door.state, 1 | 3);
            self.is_moving = matches!(rot_door.state, 2 | 3);
            if !self.is_moving {
                if rot_door.state == 1
                    && let Some(remaining) = self.auto_close_remaining
                {
                    let remaining = remaining - time.elapsed.as_secs_f32();
                    if remaining <= 0.0 {
                        self.auto_close_remaining = None;
                        return self.close_rotation(entity_id, world, &rot_door);
                    }
                    self.auto_close_remaining = Some(remaining);
                }
                return Effect::NoEffect;
            }

            let target = if self.rotation_target_open { 1.0 } else { 0.0 };
            let remaining = (target - self.rotation_progress).abs();
            let travel_radians = rot_door.travel_degrees().to_radians().abs();
            let step = if travel_radians > f32::EPSILON {
                time.elapsed.as_secs_f32() * rot_door.speed.abs() / travel_radians
            } else {
                0.0
            };
            if step <= f32::EPSILON {
                return Effect::NoEffect;
            }

            let reached_endpoint = remaining <= step.max(0.0001);
            if reached_endpoint {
                self.rotation_progress = target;
                self.is_moving = false;
                self.auto_close_remaining = if self.rotation_target_open {
                    Self::automatic_close_delay(world, entity_id)
                } else {
                    None
                };
            } else {
                self.rotation_progress += (target - self.rotation_progress).signum() * step;
            }

            let (position, rotation) = rot_door.pose_at_progress(self.rotation_progress);
            self.current_position = position;
            let state = if reached_endpoint {
                if self.rotation_target_open { 1 } else { 0 }
            } else if self.rotation_target_open {
                3
            } else {
                2
            };
            let mut effects = vec![
                Effect::SetPositionRotation {
                    entity_id,
                    position,
                    rotation,
                },
                Self::persist_rotation(entity_id, state, self.rotation_progress),
            ];
            if reached_endpoint {
                let reached_open = self.rotation_target_open;
                effects.push(play_environmental_sound(
                    world,
                    entity_id,
                    "statechange",
                    if reached_open {
                        vec![("openstate", "open"), ("oldopenstate", "opening")]
                    } else {
                        vec![("openstate", "closed"), ("oldopenstate", "closing")]
                    },
                    self.audio_handle.clone(),
                ));
                effects.push(Effect::Send {
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
                });
            }
            return Effect::Combined { effects };
        }

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
        let rot_door = {
            let doors = world.borrow::<View<PropRotatingDoor>>().unwrap();
            doors.get(entity_id).ok().cloned()
        };
        if let Some(rot_door) = rot_door {
            self.rotation_progress = rot_door.progress.clamp(0.0, 1.0);
            self.rotation_target_open = matches!(rot_door.state, 1 | 3);
            self.is_moving = matches!(rot_door.state, 2 | 3);
            if !rot_door.has_travel() {
                return Effect::NoEffect;
            }
            return match msg {
                MessagePayload::Frob => {
                    if self.rotation_target_open {
                        self.close_rotation(entity_id, world, &rot_door)
                    } else if is_entity_locked(world, entity_id) {
                        Effect::PlaySound {
                            handle: AudioHandle::new(),
                            source: Some(entity_id),
                            name: "hackfail".to_owned(),
                            spatial: false,
                        }
                    } else {
                        self.open_rotation(entity_id, world, &rot_door)
                    }
                }
                MessagePayload::TurnOn { from: _ } => {
                    self.open_rotation(entity_id, world, &rot_door)
                }
                MessagePayload::TurnOff { from: _ } => {
                    self.close_rotation(entity_id, world, &rot_door)
                }
                _ => Effect::NoEffect,
            };
        }

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

    fn script_state_key(&self) -> Option<&'static str> {
        Some(SCRIPT_STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(
            1,
            &StdDoorState {
                current_position: self.current_position,
                desired_position: self.desired_position,
                is_moving: self.is_moving,
                rotation_progress: self.rotation_progress,
                rotation_target_open: self.rotation_target_open,
                auto_close_remaining: self.auto_close_remaining,
            },
            SCRIPT_STATE_KEY,
        )
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        let restored: StdDoorState = state.decode(1, SCRIPT_STATE_KEY)?;
        self.current_position = restored.current_position;
        self.desired_position = restored.desired_position;
        self.is_moving = restored.is_moving;
        self.rotation_progress = restored.rotation_progress;
        self.rotation_target_open = restored.rotation_target_open;
        self.auto_close_remaining = restored.auto_close_remaining;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, time::Duration};

    use cgmath::{Deg, Matrix4, Quaternion, Rotation3, vec3};
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

    fn rotating_world() -> (World, EntityId) {
        let mut world = World::new();
        let closed = vec3(5.0, 6.0, 7.0);
        let open = vec3(5.0, 7.0, 8.0);
        let identity = Quaternion::new(1.0, 0.0, 0.0, 0.0);
        let entity_id = world.add_entity((
            PropRotatingDoor {
                door_type: 0,
                closed: 0.0,
                open: 90.0,
                speed: 1.0,
                axis: 0,
                state: 0,
                clockwise: false,
                base_closed_location: closed,
                base_open_location: open,
                base_location: closed,
                base_rotation: identity,
                base_closed_rotation: identity,
                base_open_rotation: Quaternion::from_angle_x(Deg(-90.0)),
                progress: 0.0,
            },
            // Real RotDoors inherit this unusable zero property. The rotating
            // component must win exactly as Dark's GetDoorProperty did.
            PropTranslatingDoor {
                door_type: 1,
                closed: 0.0,
                open: 0.0,
                speed: 0.0,
                axis: 0,
                state: 0,
                base_closed_location: Vector3::zero(),
                base_open_location: Vector3::zero(),
                base_location: Vector3::zero(),
            },
            PropClassTag::from_string("doortype hatch"),
            RuntimePropTransform(Matrix4::from_translation(closed)),
        ));
        world.add_unique(QuestInfo::new());
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

    /// Stand-in for mission_core's persistent door-state effect handlers - the
    /// script only emits effects, and the world writes happen here.
    fn apply(world: &World, effect: Effect) {
        for effect in Effect::flatten(vec![effect]) {
            match effect {
                Effect::SetTranslatingDoorState {
                    entity_id,
                    state,
                    base_location,
                } => {
                    let mut doors = world
                        .borrow::<shipyard::ViewMut<PropTranslatingDoor>>()
                        .unwrap();
                    if let Ok(door) = (&mut doors).get(entity_id) {
                        door.state = state;
                        door.base_location = base_location;
                    }
                }
                Effect::SetRotatingDoorState {
                    entity_id,
                    state,
                    progress,
                } => {
                    let mut doors = world
                        .borrow::<shipyard::ViewMut<PropRotatingDoor>>()
                        .unwrap();
                    if let Ok(door) = (&mut doors).get(entity_id) {
                        door.state = state;
                        door.progress = progress;
                    }
                }
                _ => {}
            }
        }
    }

    #[test]
    fn rotating_door_initialization_ignores_inherited_zero_translation() {
        let (world, entity_id) = rotating_world();
        let mut door = StdDoor::new();

        let effect = door.initialize(entity_id, &world);

        match effect {
            Effect::SetPositionRotation {
                entity_id: id,
                position,
                rotation,
            } => {
                assert_eq!(id, entity_id);
                assert_eq!(position, vec3(5.0, 6.0, 7.0));
                assert!(rotation.s > 0.999);
            }
            unexpected => panic!("expected rotating pose, got {unexpected:?}"),
        }
    }

    #[test]
    fn rotating_door_open_and_closed_endpoints_are_persisted() {
        let (world, entity_id) = rotating_world();
        let physics = PhysicsWorld::new();
        let mut door = initialized_door(entity_id, &world);

        let effect = door.handle_message(
            entity_id,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: entity_id },
        );
        apply(&world, effect);
        let effect = door.update(entity_id, &world, &physics, &step(2.0));
        apply(&world, effect);
        {
            let doors = world.borrow::<View<PropRotatingDoor>>().unwrap();
            let saved = doors.get(entity_id).unwrap();
            assert_eq!(saved.state, 1);
            assert_eq!(saved.progress, 1.0);
        }

        let effect = door.handle_message(
            entity_id,
            &world,
            &physics,
            &MessagePayload::TurnOff { from: entity_id },
        );
        apply(&world, effect);
        let effect = door.update(entity_id, &world, &physics, &step(2.0));
        apply(&world, effect);
        let doors = world.borrow::<View<PropRotatingDoor>>().unwrap();
        let saved = doors.get(entity_id).unwrap();
        assert_eq!(saved.state, 0);
        assert_eq!(saved.progress, 0.0);
    }

    #[test]
    fn rotating_door_closes_after_its_authored_timer() {
        let (mut world, entity_id) = rotating_world();
        world.add_component(entity_id, PropDoorTimer(1));
        let physics = PhysicsWorld::new();
        let mut door = initialized_door(entity_id, &world);

        let effect = door.handle_message(
            entity_id,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: entity_id },
        );
        apply(&world, effect);
        let effect = door.update(entity_id, &world, &physics, &step(2.0));
        apply(&world, effect);
        let effect = door.update(entity_id, &world, &physics, &step(1.1));
        apply(&world, effect);

        {
            let doors = world.borrow::<View<PropRotatingDoor>>().unwrap();
            assert_eq!(doors.get(entity_id).unwrap().state, 2);
        }

        let effect = door.update(entity_id, &world, &physics, &step(2.0));
        apply(&world, effect);
        let doors = world.borrow::<View<PropRotatingDoor>>().unwrap();
        assert_eq!(doors.get(entity_id).unwrap().state, 0);
        assert_eq!(doors.get(entity_id).unwrap().progress, 0.0);
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
    fn translating_door_private_motion_survives_script_state_round_trip() {
        let (world, entity_id) = test_world(false, false);
        let physics = PhysicsWorld::new();
        let mut before = initialized_door(entity_id, &world);
        before.handle_message(entity_id, &world, &physics, &MessagePayload::Frob);

        let state = before.save_state().unwrap();
        let mut after = StdDoor::new();
        let entity_map = HashMap::new();
        let context = ScriptRestoreContext::new(&entity_map);
        after.restore_state(&state, &context).unwrap();

        assert_eq!(after.current_position, before.current_position);
        assert_eq!(after.desired_position, before.desired_position);
        assert_eq!(after.is_moving, before.is_moving);
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
