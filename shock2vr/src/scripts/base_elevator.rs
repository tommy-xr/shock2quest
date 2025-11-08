use std::{collections::HashSet, ops::Rem};

use cgmath::{InnerSpace, Vector3, Zero};
use dark::properties::{Link, PropPosition, PropTemplateId, TPathData};
use engine::audio::AudioHandle;
use shipyard::{EntityId, Get, View, World};
use tracing::{info, trace};

use crate::{physics::PhysicsWorld, time::Time};

use super::{
    Effect, MessagePayload, Script,
    script_util::{get_first_link_of_type, get_first_link_with_data},
};

pub struct BaseElevator {
    path_offset: Vector3<f32>,
    current_index: u32,
    current_position: Vector3<f32>,
    desired_position: Vector3<f32>,
    speed: f32,
    is_moving: bool,
    path: Vec<(PropPosition, Option<TPathData>)>,
    is_dontstop_elevator: bool, // Flag for if the elevator came from a 'DontStopElevator' script, where it should keep moving when it hits boundaries.
}

impl BaseElevator {
    pub fn new() -> BaseElevator {
        BaseElevator {
            path_offset: Vector3::zero(),
            current_position: Vector3::zero(),
            current_index: 0,
            desired_position: Vector3::zero(),
            is_moving: false,
            speed: 10.0,
            path: Vec::new(),
            is_dontstop_elevator: false,
        }
    }

    pub fn continuous() -> BaseElevator {
        let mut elevator = BaseElevator::new();
        elevator.is_dontstop_elevator = true;
        elevator
    }

    fn move_to_next_target(&mut self, world: &World) {
        let _v_template = world.borrow::<View<PropTemplateId>>().unwrap();
        info!("BaseElevator: Got turn on... paths are: {:?}", self.path);
        let _v_position = world.borrow::<View<PropPosition>>().unwrap();

        let next_position_idx = (self.current_index + 1).rem(self.path.len() as u32);
        self.current_index = next_position_idx;

        let (next_position, next_data) = &self.path[next_position_idx as usize];
        self.desired_position = next_position.position;

        // If we have path data available, use it to set the speed
        if let Some(path_data) = next_data {
            self.speed = path_data.speed
        }

        info!(
            "BaseElevator: Moving to index {} position {:?} with speed {}",
            self.current_index, self.desired_position, self.speed
        );
        //self.target_entity = next_dest_entity;
        self.is_moving = true;
    }
}
impl Script for BaseElevator {
    fn initialize(&mut self, entity_id: EntityId, world: &World) -> Effect {
        let v_position = world.borrow::<View<PropPosition>>().unwrap();
        let initial_path = get_first_link_of_type(world, entity_id, Link::TPathInit);

        let position = v_position.get(entity_id).unwrap();
        self.current_position = position.position;
        self.desired_position = self.current_position;

        // Figure out where the next link goes... if there is no TPathInit, revert back to entity
        let mut target_entity = entity_id;
        if let Some(init_entity_id) = initial_path {
            if let Ok(initial_position) = v_position.get(init_entity_id) {
                self.path_offset = self.current_position - initial_position.position;
            }
            target_entity = init_entity_id;
        }

        // Create path
        let path = get_elevator_path(target_entity, world);

        info!(
            "[BaseElevator] initialized with entity {:?} at offset {:?} with path {:?}",
            target_entity, self.path_offset, &path
        );

        // Save the path
        self.path = path;

        Effect::NoEffect
    }
    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        time: &Time,
    ) -> Effect {
        let dir = self.desired_position - self.current_position;
        if dir.magnitude2() > 0.001 {
            let normalized = dir.normalize();

            trace!(
                "desired: {:?} current: {:?} dir: {:?}",
                self.desired_position, self.current_position, normalized
            );

            self.is_moving = true;
            self.current_position += normalized * time.elapsed.as_secs_f32() * self.speed;
            Effect::SetPosition {
                entity_id,
                position: self.current_position,
            }
        } else if self.is_moving {
            if self.is_dontstop_elevator {
                self.move_to_next_target(world);
            } else {
                self.is_moving = false;
            }
            Effect::NoEffect
        } else {
            Effect::NoEffect
        }
    }

    fn handle_message(
        &mut self,
        _entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        msg: &MessagePayload,
    ) -> Effect {
        match msg {
            MessagePayload::TurnOn { from: _ } => {
                if !self.is_moving {
                    self.move_to_next_target(world);
                    Effect::PlaySound {
                        handle: AudioHandle::new(),
                        name: "Devices/DOOR1OP".to_owned(),
                    }
                } else {
                    Effect::NoEffect
                }
            }
            // MessagePayload::TurnOff => {
            //     //self.current_position = trans_door.base_closed_location;
            //     self.desired_position = trans_door.base_closed_location;
            //     self.is_moving = true;
            //     Effect::PlaySound {
            //         handle: AudioHandle::new(),
            //         name: "Devices/DOOR1CL".to_owned(),
            //     }
            // }
            _ => Effect::NoEffect,
        }
    }
}

///
/// get_elevator_path
///
/// Return a list of Vec<PropPositions>, representing each stop on a TPath link
///
fn get_elevator_path(
    target_entity: EntityId,
    world: &World,
) -> Vec<(PropPosition, Option<TPathData>)> {
    let v_position = world.borrow::<View<PropPosition>>().unwrap();
    let _v_template_id = world.borrow::<View<PropTemplateId>>().unwrap();
    let mut next_path = Some((target_entity, None));

    let mut path = Vec::new();

    // Keep track of previous items visited to break any circular references
    let mut visited = HashSet::new();

    while next_path.is_some() {
        let (next_entity_id, path_data) = next_path.unwrap();
        if visited.contains(&next_entity_id) {
            // Already seen this node, so time to quit
            break;
        }

        visited.insert(next_entity_id);

        let maybe_next_position = v_position.get(next_entity_id);

        if let Ok(next_position) = maybe_next_position {
            path.push((next_position.clone(), path_data))
        }

        next_path = get_first_link_with_data(world, next_entity_id, |link| match link {
            Link::TPath(data) => Some(Some(*data)), // hack...
            _ => None,
        });
    }
    path
}
