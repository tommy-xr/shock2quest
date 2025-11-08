use cgmath::{InnerSpace, Vector3, Zero};
use dark::properties::PropTranslatingDoor;
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};
use tracing::trace;

use crate::{physics::PhysicsWorld, time::Time};

use super::{Effect, MessagePayload, Script, script_util::play_environmental_sound};

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
}
impl Script for StdDoor {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let v_trans_door = world.borrow::<View<PropTranslatingDoor>>().unwrap();
        if let Ok(trans_door) = v_trans_door.get(entity_id) {
            self.desired_position = trans_door.base_closed_location;
            self.current_position = trans_door.base_closed_location;

            Effect::SetPosition {
                entity_id,
                position: trans_door.base_closed_location,
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
            if dir.magnitude2() > 0.001 {
                let normalized = dir.normalize();
                //dir *= trans_door.speed;

                trace!(
                    "desired: {:?} current: {:?} dir: {:?}",
                    self.desired_position, self.current_position, normalized
                );

                self.current_position += normalized * time.elapsed.as_secs_f32() * trans_door.speed;
                Effect::SetPosition {
                    entity_id,
                    position: self.current_position,
                }
            } else if self.is_moving {
                self.is_moving = false;
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
                MessagePayload::TurnOn { from: _ } => {
                    //self.current_position = trans_door.base_closed_location;
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
                MessagePayload::TurnOff { from: _ } => {
                    //self.current_position = trans_door.base_closed_location;
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
                _ => Effect::NoEffect,
            }
        } else {
            Effect::NoEffect
        }
    }
}
