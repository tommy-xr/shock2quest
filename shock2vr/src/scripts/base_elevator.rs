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
        let step = time.elapsed.as_secs_f32() * self.speed;
        // Complete once within one frame-step of the target - stepping by
        // a fixed amount can otherwise overshoot and oscillate around a
        // small distance threshold forever, so the elevator never finishes
        // and can never be dispatched again.
        if dir.magnitude() > step.max(0.001) {
            let normalized = dir.normalize();

            trace!(
                "desired: {:?} current: {:?} dir: {:?}",
                self.desired_position, self.current_position, normalized
            );

            self.is_moving = true;
            self.current_position += normalized * step;
            Effect::SetPosition {
                entity_id,
                position: self.current_position,
            }
        } else if self.is_moving {
            // Land exactly on the node rather than a step short of it.
            self.current_position = self.desired_position;
            // Captured before move_to_next_target() retargets desired_position.
            let position = self.current_position;
            if self.is_dontstop_elevator {
                self.move_to_next_target(world);
            } else {
                self.is_moving = false;
            }
            Effect::SetPosition {
                entity_id,
                position,
            }
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use cgmath::{Quaternion, vec3};
    use dark::properties::{Links, ToLink, WrappedEntityId};

    use super::*;

    fn frame() -> Time {
        Time {
            elapsed: Duration::from_secs_f32(1.0 / 60.0),
            total: Duration::from_secs_f32(1.0 / 60.0),
        }
    }

    fn position_at(x: f32) -> PropPosition {
        PropPosition {
            position: Vector3::new(x, 0.0, 0.0),
            cell: 0,
            rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
        }
    }

    /// A two-stop elevator - like a tram between its terminals: the entity sits
    /// on the first node, which TPaths to the second at `speed`.
    fn two_stop_world(start_x: f32, end_x: f32, speed: f32) -> (World, EntityId) {
        let mut world = World::new();
        let end_node = world.add_entity((position_at(end_x), Links::empty()));
        let start_node = world.add_entity((
            position_at(start_x),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(end_node)),
                    link: Link::TPath(TPathData { speed }),
                }],
            },
        ));
        let elevator = world.add_entity((
            position_at(start_x),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(start_node)),
                    link: Link::TPathInit,
                }],
            },
        ));
        (world, elevator)
    }

    /// Steps until the elevator stops, returning the effect of the frame it
    /// arrived on.
    fn run_until_stopped(
        elevator: &mut BaseElevator,
        entity_id: EntityId,
        world: &World,
        physics: &PhysicsWorld,
        max_frames: u32,
    ) -> Effect {
        for _ in 0..max_frames {
            let effect = elevator.update(entity_id, world, physics, &frame());
            if !elevator.is_moving {
                return effect;
            }
        }
        panic!(
            "elevator never stopped: at {:?}, target {:?}",
            elevator.current_position, elevator.desired_position
        );
    }

    /// The command1 tram (speed 12): 193.100 wu at 0.2 wu/frame is 965.5 steps,
    /// so the residual lands on the worst-case midpoint of a step (#653).
    #[test]
    fn a_fast_elevator_arrives_at_its_node_and_can_be_dispatched_again() {
        let (world, entity_id) = two_stop_world(-377.53342, -184.43343, 12.0);
        let physics = PhysicsWorld::new();
        let mut elevator = BaseElevator::new();
        elevator.initialize(entity_id, &world);

        let turn_on = MessagePayload::TurnOn { from: entity_id };
        let effect = elevator.handle_message(entity_id, &world, &physics, &turn_on);
        assert!(matches!(effect, Effect::PlaySound { .. }));
        assert!(elevator.is_moving);

        let arrival = run_until_stopped(&mut elevator, entity_id, &world, &physics, 2000);

        assert_eq!(elevator.current_position, vec3(-184.43343, 0.0, 0.0));
        assert!(matches!(
            arrival,
            Effect::SetPosition { position, .. } if position == vec3(-184.43343, 0.0, 0.0)
        ));

        // The user-visible symptom: a jammed elevator can never be recalled.
        let effect = elevator.handle_message(entity_id, &world, &physics, &turn_on);
        assert!(matches!(effect, Effect::PlaySound { .. }));
        assert!(elevator.is_moving);
        assert_eq!(elevator.desired_position, vec3(-377.53342, 0.0, 0.0));
    }

    #[test]
    fn a_slow_elevator_still_arrives_exactly_on_its_node() {
        let (world, entity_id) = two_stop_world(0.0, 4.0, 2.4);
        let physics = PhysicsWorld::new();
        let mut elevator = BaseElevator::new();
        elevator.initialize(entity_id, &world);

        elevator.handle_message(
            entity_id,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: entity_id },
        );
        run_until_stopped(&mut elevator, entity_id, &world, &physics, 2000);

        assert_eq!(elevator.current_position, vec3(4.0, 0.0, 0.0));
    }

    #[test]
    fn a_continuous_elevator_retargets_the_next_node_on_arrival() {
        let (world, entity_id) = two_stop_world(-377.53342, -184.43343, 12.0);
        let physics = PhysicsWorld::new();
        let mut elevator = BaseElevator::continuous();
        elevator.initialize(entity_id, &world);

        elevator.handle_message(
            entity_id,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: entity_id },
        );

        let mut arrival = None;
        for _ in 0..2000 {
            let effect = elevator.update(entity_id, &world, &physics, &frame());
            if elevator.desired_position == vec3(-377.53342, 0.0, 0.0) {
                arrival = Some(effect);
                break;
            }
        }
        // It reports the node it reached, not the one it just retargeted.
        assert!(matches!(
            arrival,
            Some(Effect::SetPosition { position, .. }) if position == vec3(-184.43343, 0.0, 0.0)
        ));
        assert!(elevator.is_moving);
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
