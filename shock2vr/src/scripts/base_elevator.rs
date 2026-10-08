use std::{collections::HashSet, ops::Rem};

use cgmath::{InnerSpace, Vector3, Zero};
use dark::properties::{Link, PropPosition, PropTemplateId, TPathData};
use engine::audio::AudioHandle;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, View, World};
use tracing::{info, trace};

use crate::{physics::PhysicsWorld, time::Time};

use super::{
    Effect, MessagePayload, Script, ScriptRestoreContext, ScriptState, ScriptStateError,
    script_util::{get_first_link_of_type, get_first_link_with_data},
};

/// Native elevator reroutes treat a platform within this distance as already
/// occupying a station. Use the same tolerance when reconstructing script
/// state from a save-restored moving-terrain position.
const ELEVATOR_STATION_TOLERANCE: f32 = 0.1;

const SCRIPT_STATE_KEY: &str = "shock2vr.base_elevator";

/// Runtime waypoint IDs are rebuilt from the restored TPath links. Only the
/// dispatch state belongs in the script envelope.
#[derive(Serialize, Deserialize)]
struct ElevatorState {
    path_offset: Vector3<f32>,
    current_index: u32,
    current_position: Vector3<f32>,
    desired_position: Vector3<f32>,
    speed: f32,
    is_moving: bool,
}

pub struct BaseElevator {
    path_offset: Vector3<f32>,
    current_index: u32,
    current_position: Vector3<f32>,
    desired_position: Vector3<f32>,
    speed: f32,
    is_moving: bool,
    path: Vec<(EntityId, PropPosition, Option<TPathData>)>,
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
        self.move_to_target(next_position_idx);
    }

    fn move_to_target(&mut self, target_index: u32) {
        self.current_index = target_index;

        let (_, next_position, next_data) = &self.path[target_index as usize];
        self.desired_position = next_position.position;

        // Zero is the sparse-record default; nonpositive speeds keep the current speed.
        if let Some(path_data) = next_data {
            if path_data.speed > 0.0 {
                self.speed = path_data.speed;
            }
        }

        info!(
            "BaseElevator: Moving to index {} position {:?} with speed {}",
            self.current_index, self.desired_position, self.speed
        );
        self.is_moving = true;
    }

    fn reroute_to(&mut self, target_waypoint: EntityId) -> bool {
        let Some(target_index) = self
            .path
            .iter()
            .position(|(waypoint, _, _)| *waypoint == target_waypoint)
            .map(|index| index as u32)
        else {
            return false;
        };

        // The native script ignores a request for the station the platform is
        // already at (within 0.1 world units), and a repeated request for the
        // waypoint it is already approaching does not restart it.
        let target_position = self.path[target_index as usize].1.position;
        if (self.current_position - target_position).magnitude() <= 0.1
            || (self.is_moving && self.current_index == target_index)
        {
            return false;
        }

        self.move_to_target(target_index);
        true
    }

    fn restored_station_index(&self) -> Option<u32> {
        let tolerance_squared = ELEVATOR_STATION_TOLERANCE * ELEVATOR_STATION_TOLERANCE;
        let mut nearest: Option<(u32, f32)> = None;

        for (index, (_, station, _)) in self.path.iter().enumerate() {
            let distance_squared = (self.current_position - station.position).magnitude2();
            if distance_squared > tolerance_squared {
                continue;
            }

            // Strictly closer replaces the candidate. Equal-distance ties
            // retain the earlier (lower) path index for deterministic routing.
            if nearest
                .map(|(_, nearest_distance)| distance_squared < nearest_distance)
                .unwrap_or(true)
            {
                nearest = Some((index as u32, distance_squared));
            }
        }

        nearest.map(|(index, _)| index)
    }
}
impl Script for BaseElevator {
    fn script_state_key(&self) -> Option<&'static str> {
        Some(SCRIPT_STATE_KEY)
    }

    fn save_state(&self) -> Result<ScriptState, ScriptStateError> {
        ScriptState::encode(
            1,
            &ElevatorState {
                path_offset: self.path_offset,
                current_index: self.current_index,
                current_position: self.current_position,
                desired_position: self.desired_position,
                speed: self.speed,
                is_moving: self.is_moving,
            },
            SCRIPT_STATE_KEY,
        )
    }

    fn restore_state(
        &mut self,
        state: &ScriptState,
        _context: &ScriptRestoreContext<'_>,
    ) -> Result<(), ScriptStateError> {
        let saved: ElevatorState = state.decode(1, SCRIPT_STATE_KEY)?;
        self.path_offset = saved.path_offset;
        self.current_index = saved.current_index;
        self.current_position = saved.current_position;
        self.desired_position = saved.desired_position;
        self.speed = saved.speed;
        self.is_moving = saved.is_moving;
        Ok(())
    }

    fn initialize_after_hydration(
        &mut self,
        entity_id: EntityId,
        world: &World,
        hydrated: bool,
    ) -> Effect {
        if !hydrated {
            return self.initialize(entity_id, world);
        }
        // Initialization normally resets the destination to the platform's
        // position. A loaded platform must continue its saved leg instead.
        // Resolve path handles anew so no pre-save runtime entity ID survives.
        let first = get_first_link_of_type(world, entity_id, Link::TPathInit).unwrap_or(entity_id);
        self.path = get_elevator_path(first, world);
        Effect::NoEffect
    }

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
        if let Some(restored_index) = self.restored_station_index() {
            self.current_index = restored_index;
        }

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
        entity_id: EntityId,
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
                        source: Some(entity_id),
                        name: "Devices/DOOR1OP".to_owned(),
                        spatial: false,
                    }
                } else {
                    Effect::NoEffect
                }
            }
            MessagePayload::RerouteElevator { target_waypoint } => {
                if self.reroute_to(*target_waypoint) {
                    Effect::PlaySound {
                        handle: AudioHandle::new(),
                        source: Some(entity_id),
                        name: "Devices/DOOR1OP".to_owned(),
                        spatial: false,
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
                    link: Link::TPath(TPathData {
                        speed,
                        ..Default::default()
                    }),
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

    fn four_stop_world(elevator_x: f32) -> (World, EntityId, [EntityId; 4]) {
        let mut world = World::new();
        let fourth = world.add_entity((position_at(30.0), Links::empty()));
        let third = world.add_entity((
            position_at(20.0),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(fourth)),
                    link: Link::TPath(TPathData {
                        speed: 4.0,
                        ..Default::default()
                    }),
                }],
            },
        ));
        let second = world.add_entity((
            position_at(10.0),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(third)),
                    link: Link::TPath(TPathData {
                        speed: 3.0,
                        ..Default::default()
                    }),
                }],
            },
        ));
        let first = world.add_entity((
            position_at(0.0),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(second)),
                    link: Link::TPath(TPathData {
                        speed: 2.0,
                        ..Default::default()
                    }),
                }],
            },
        ));
        let elevator = world.add_entity((
            position_at(elevator_x),
            Links {
                to_links: vec![ToLink {
                    to_template_id: 0,
                    to_entity_id: Some(WrappedEntityId(first)),
                    link: Link::TPathInit,
                }],
            },
        ));
        (world, elevator, [first, second, third, fourth])
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

    #[test]
    fn parked_and_moving_elevators_round_trip_their_dispatch_state() {
        for moving in [false, true] {
            for continuous in [false, true] {
                let (world, entity_id, nodes) = four_stop_world(0.0);
                let physics = PhysicsWorld::new();
                let mut before = if continuous {
                    BaseElevator::continuous()
                } else {
                    BaseElevator::new()
                };
                before.initialize(entity_id, &world);
                before.reroute_to(nodes[2]);
                if moving {
                    for _ in 0..37 {
                        before.update(entity_id, &world, &physics, &frame());
                    }
                } else {
                    // Park at the noninitial station, including the continuous
                    // variant's just-arrived state before dispatching again.
                    before.current_position = before.desired_position;
                    before.is_moving = false;
                }
                let state = before
                    .save_state()
                    .expect("elevator must persist dispatch state");
                let mut after = if continuous {
                    BaseElevator::continuous()
                } else {
                    BaseElevator::new()
                };
                after
                    .restore_state(
                        &state,
                        &super::super::ScriptRestoreContext::new(&std::collections::HashMap::new()),
                    )
                    .unwrap();
                after.initialize_after_hydration(entity_id, &world, true);
                assert_eq!(after.current_index, before.current_index);
                assert_eq!(after.current_position, before.current_position);
                assert_eq!(after.desired_position, before.desired_position);
                assert_eq!(after.speed, before.speed);
                assert_eq!(after.is_moving, before.is_moving);
                if !moving {
                    before.move_to_next_target(&world);
                    after.move_to_next_target(&world);
                    assert_eq!(after.current_index, 3);
                }
                // Follow through arrival and (for DontStopElevator) the next
                // leg. Restore must rebuild the runtime waypoint references.
                for _ in 0..600 {
                    let expected = before.update(entity_id, &world, &physics, &frame());
                    let actual = after.update(entity_id, &world, &physics, &frame());
                    assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
                    assert_eq!(after.current_index, before.current_index);
                }
            }
        }
    }

    #[test]
    fn missing_or_nonpositive_path_speed_keeps_the_previous_speed() {
        for speed in [0.0, -1.0] {
            let (world, entity_id) = two_stop_world(0.0, 5.0, speed);
            let physics = PhysicsWorld::new();
            let mut elevator = BaseElevator::new();
            elevator.initialize(entity_id, &world);
            elevator.speed = 3.0;
            elevator.handle_message(
                entity_id,
                &world,
                &physics,
                &MessagePayload::TurnOn { from: entity_id },
            );
            assert_eq!(elevator.speed, 3.0);
            run_until_stopped(&mut elevator, entity_id, &world, &physics, 200);
            assert_eq!(elevator.current_position, vec3(5.0, 0.0, 0.0));
        }
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
    fn reroute_targets_the_requested_station_without_visiting_intermediate_nodes() {
        let (world, entity_id, nodes) = four_stop_world(0.0);
        let physics = PhysicsWorld::new();
        let mut elevator = BaseElevator::new();
        elevator.initialize(entity_id, &world);

        let effect = elevator.handle_message(
            entity_id,
            &world,
            &physics,
            &MessagePayload::RerouteElevator {
                target_waypoint: nodes[3],
            },
        );

        assert!(matches!(effect, Effect::PlaySound { .. }));
        assert!(elevator.is_moving);
        assert_eq!(elevator.current_index, 3);
        assert_eq!(elevator.desired_position, vec3(30.0, 0.0, 0.0));
        assert_eq!(elevator.speed, 4.0);
    }

    /// Save/load restores the moving terrain's position but constructs a new
    /// script. The next ordinary button press must advance from the station
    /// the platform visibly occupies, not from path index zero.
    #[test]
    fn initialized_elevator_advances_from_its_restored_station() {
        let (world, entity_id, _) = four_stop_world(20.0);
        let physics = PhysicsWorld::new();
        let mut elevator = BaseElevator::new();
        elevator.initialize(entity_id, &world);

        elevator.handle_message(
            entity_id,
            &world,
            &physics,
            &MessagePayload::TurnOn { from: entity_id },
        );

        assert_eq!(elevator.current_index, 3);
        assert_eq!(elevator.desired_position, vec3(30.0, 0.0, 0.0));
    }

    #[test]
    fn restored_station_match_has_bounded_deterministic_tolerance() {
        let (world, entity_id, _) = four_stop_world(0.0);
        let mut elevator = BaseElevator::new();
        elevator.initialize(entity_id, &world);

        elevator.current_position = vec3(20.09, 0.0, 0.0);
        assert_eq!(elevator.restored_station_index(), Some(2));
        elevator.current_position = vec3(20.11, 0.0, 0.0);
        assert_eq!(elevator.restored_station_index(), None);

        elevator.path[1].1.position = vec3(19.95, 0.0, 0.0);
        elevator.path[2].1.position = vec3(20.05, 0.0, 0.0);
        elevator.current_position = vec3(20.0, 0.0, 0.0);
        assert_eq!(
            elevator.restored_station_index(),
            Some(1),
            "an equal-distance tie should keep the lower path index"
        );
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
) -> Vec<(EntityId, PropPosition, Option<TPathData>)> {
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
            path.push((next_entity_id, next_position.clone(), path_data))
        }

        next_path = get_first_link_with_data(world, next_entity_id, |link| match link {
            Link::TPath(data) => Some(Some(*data)), // hack...
            _ => None,
        });
    }
    path
}
