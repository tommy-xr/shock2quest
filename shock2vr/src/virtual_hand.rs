// Helper to convert the input context to a form more useful for gameplay / interacting with the world

use cgmath::{point3, vec3, Matrix4, Quaternion, Rotation, Vector3, Zero};
use dark::properties::{FrobFlag, PropFrobInfo, PropModelName};
use engine::scene::SceneObject;
use engine::script_log;

use rapier3d::prelude::RigidBodyHandle;
use shipyard::{EntityId, Get, View, World};
use tracing::{self, trace};

use crate::{
    input_context::Hand,
    physics::{InternalCollisionGroups, PhysicsWorld, RayCastResult},
    scripts::{Message, MessagePayload},
    util::{self, point3_to_vec3},
    vr_config::{self, Handedness},
};

const HAND_OFFSET: Vector3<f32> = vec3(0.0, 0.0, 0.0);

#[derive(Clone)]
pub struct VirtualHand {
    position: Vector3<f32>,
    rotation: Quaternion<f32>,
    trigger_value: f32,
    squeeze_value: f32,
    raytrace_hit: Option<RayCastResult>,

    // Keep track of last frobbed entity so frobbing is 'semi-auto'
    last_frobbed_entity: Option<EntityId>,

    hand_state: HandState,

    handedness: Handedness,
}

pub enum VirtualHandEffect {
    OutMessage {
        message: Message,
    },
    // Keeping to allow for debugging
    #[allow(dead_code)]
    ApplyForce {
        entity_id: EntityId,
        force: Vector3<f32>,
        torque: Vector3<f32>,
    },
    SetPositionRotation {
        entity_id: EntityId,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
        scale: Vector3<f32>,
    },
    // Keeping to allow for debugging
    #[allow(dead_code)]
    SpawnEntity {
        template_id: i32,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    },
    HoldItem {
        entity_id: EntityId,
    },
    DropItem {
        entity_id: EntityId,
    },
}

// All the details we need for the item we are grabbing..
#[derive(Copy, Clone)]
pub enum HandState {
    Empty, // Hand is not holding anything

    Grabbing {
        // Identifiers for world / physics
        entity_id: EntityId,
        // rigid_body_handle: RigidBodyHandle,
    },
}

impl VirtualHand {
    pub fn new(handedness: Handedness) -> VirtualHand {
        VirtualHand {
            position: Vector3::zero(),
            rotation: Quaternion {
                v: Vector3::zero(),
                s: 1.0,
            },
            trigger_value: 0.0,
            squeeze_value: 0.0,
            raytrace_hit: None,
            last_frobbed_entity: None,
            hand_state: HandState::Empty,
            handedness,
        }
    }
    pub fn destroy_entity(&self, entity_to_destroy_id: EntityId) -> VirtualHand {
        match self.hand_state {
            // Nothing to do here!
            HandState::Empty => self.clone(),
            HandState::Grabbing { entity_id } => {
                if entity_id == entity_to_destroy_id {
                    VirtualHand {
                        hand_state: HandState::Empty,
                        ..self.clone()
                    }
                } else {
                    self.clone()
                }
            }
        }
    }

    pub fn get_held_entity(&self) -> Option<EntityId> {
        match self.hand_state {
            HandState::Empty => None,
            HandState::Grabbing { entity_id, .. } => Some(entity_id),
        }
    }

    pub fn get_raytraced_entity(&self) -> Option<EntityId> {
        match &self.raytrace_hit {
            None => None,
            Some(hit) => hit.maybe_entity_id,
        }
    }

    pub fn is_holding(&self, entity_id: EntityId) -> bool {
        self.get_held_entity() == Some(entity_id)
    }

    pub fn get_position(&self) -> Vector3<f32> {
        self.position
    }

    pub fn get_rotation(&self) -> Quaternion<f32> {
        self.rotation
    }

    pub fn grab_entity(
        &self,
        _world: &World,
        entity_id: EntityId,
        // entity_rigid_body: RigidBodyHandle,
    ) -> VirtualHand {
        if self.get_held_entity().is_some() {
            // Already holding something!
            return self.clone();
        }

        VirtualHand {
            hand_state: HandState::Grabbing { entity_id },
            ..self.clone()
        }
    }

    pub fn replace_entity(
        &self,
        old_entity_id: EntityId,
        new_entity_id: EntityId,
        _new_rigid_body: RigidBodyHandle,
    ) -> VirtualHand {
        match self.hand_state {
            HandState::Empty => self.clone(),
            HandState::Grabbing { entity_id } => {
                if entity_id == old_entity_id {
                    VirtualHand {
                        hand_state: HandState::Grabbing {
                            entity_id: new_entity_id,
                        },
                        ..self.clone()
                    }
                } else {
                    self.clone()
                }
            }
        }
    }

    pub fn update(
        prev: &VirtualHand,
        physics: &PhysicsWorld,
        world: &World,
        pawn_pos: Vector3<f32>,
        pawn_rot: Quaternion<f32>,
        input_hand: &Hand,
    ) -> (VirtualHand, Vec<VirtualHandEffect>) {
        let handedness = prev.handedness;
        let hand_position = pawn_pos + HAND_OFFSET + pawn_rot.rotate_vector(input_hand.position);
        let hand_rotation = pawn_rot * input_hand.rotation;

        // Also do a raycast to provide the 'Hover' effect
        let ray_start = point3(hand_position.x, hand_position.y, hand_position.z);
        let forward = hand_rotation.rotate_vector(vec3(0.0, 0.0, -1.0));
        let result = physics.ray_cast(
            ray_start,
            forward,
            // TODO: Two raycasts...
            // One for hit damage:
            //InternalCollisionGroups::HITBOX
            InternalCollisionGroups::ENTITY
                | InternalCollisionGroups::SELECTABLE
                | InternalCollisionGroups::WORLD
                | InternalCollisionGroups::UI
                | InternalCollisionGroups::RAYCAST,
            //InternalCollisionGroups::all(),
            // One for frobbing:
            //CollisionGroups::WORLD | CollisionGroups::SELECTABLE,
        );

        let result = result.map(|r| resolve_hit_proxy_entity(world, r));

        let (hand, mut effs) = match prev.hand_state {
            HandState::Grabbing { entity_id } => {
                // See what we're hitting
                let mut msgs = Vec::new();

                // If we're holding onto something, but not grabbing, we can drop it
                if input_hand.squeeze_value < 0.5 {
                    let mut msgs = vec![VirtualHandEffect::DropItem { entity_id }];

                    let result_copy = result.clone();
                    if let Some(ray_cast_result) = result_copy {
                        if let Some(hit_entity_id) = ray_cast_result.maybe_entity_id {
                            msgs.push(VirtualHandEffect::OutMessage {
                                message: Message {
                                    to: hit_entity_id,
                                    payload: MessagePayload::ProvideForConsumption {
                                        entity: entity_id,
                                    },
                                },
                            });
                        }
                    };

                    let updated_hand = VirtualHand {
                        position: hand_position,
                        rotation: hand_rotation,
                        trigger_value: input_hand.trigger_value,
                        squeeze_value: input_hand.squeeze_value,
                        raytrace_hit: None,
                        last_frobbed_entity: None,
                        hand_state: HandState::Empty,
                        handedness,
                    };
                    (updated_hand, msgs)
                } else {
                    let next_hand_state = prev.hand_state;
                    // let position = &physics.get_position(rigid_body_handle).unwrap();
                    // let velocity = physics.get_velocity(rigid_body_handle).unwrap();
                    // let hold_position = hand_position + offset_position;
                    // let dir = hold_position - position;

                    let vr_offsets = get_held_position_orientation(entity_id, world, handedness);

                    let v_model_name = world.borrow::<View<PropModelName>>().unwrap();
                    let _maybe_model_name = v_model_name.get(entity_id);

                    if prev.trigger_value < 0.5 && input_hand.trigger_value > 0.5 {
                        msgs.push(VirtualHandEffect::OutMessage {
                            message: Message {
                                to: entity_id,
                                payload: MessagePayload::TriggerPull,
                            },
                        });
                    }

                    if prev.trigger_value > 0.5 && input_hand.trigger_value < 0.5 {
                        script_log!(DEBUG, "Hand releasing trigger");
                        msgs.push(VirtualHandEffect::OutMessage {
                            message: Message {
                                to: entity_id,
                                payload: MessagePayload::TriggerRelease,
                            },
                        });
                    }

                    msgs.push(VirtualHandEffect::SetPositionRotation {
                        entity_id,
                        position: hand_position + vr_offsets.offset,
                        rotation: hand_rotation * vr_offsets.rotation,
                        scale: vec3(1.0, 1.0, 1.0), //vr_offsets.scale,
                    });

                    let updated_hand = VirtualHand {
                        position: hand_position,
                        rotation: hand_rotation,
                        trigger_value: input_hand.trigger_value,
                        squeeze_value: input_hand.squeeze_value,
                        raytrace_hit: None,
                        last_frobbed_entity: None,
                        hand_state: next_hand_state,
                        handedness,
                    };
                    (updated_hand, msgs)
                }
            }
            HandState::Empty => handle_empty_hand_state(
                handedness,
                hand_position,
                hand_rotation,
                prev.last_frobbed_entity,
                world,
                physics,
                input_hand,
            ),
        };

        match result {
            Some(RayCastResult {
                hit_point,
                hit_normal: _,
                maybe_entity_id: Some(to_entity_id),
                maybe_rigid_body_handle: _,
                is_sensor: _,
            }) => effs.push(VirtualHandEffect::OutMessage {
                message: Message {
                    to: to_entity_id,
                    payload: MessagePayload::Hover {
                        held_entity_id: hand.get_held_entity(),
                        world_position: point3_to_vec3(hit_point),
                        is_triggered: input_hand.trigger_value > 0.5,
                        is_grabbing: input_hand.squeeze_value > 0.5,
                        hand: hand.handedness,
                    },
                },
            }),
            _ => (),
        };

        (hand, effs)
    }

    pub fn render(&self) -> Vec<SceneObject> {
        let mut scene_objects = Vec::new();

        // Show hand object
        let hand_material = engine::scene::color_material::create(vec3(0.0, 1.0, 0.0));
        let transform = Matrix4::from_translation(self.position)
            * Matrix4::from(self.rotation)
            * Matrix4::from_scale(0.05);
        let mut hand_obj = SceneObject::new(hand_material, Box::new(engine::scene::cube::create()));
        hand_obj.set_transform(transform);
        scene_objects.push(hand_obj);

        let hit_color = self.color_from_state();

        // Show ray trace hit
        if let Some(rayhit) = &self.raytrace_hit {
            let cm = engine::scene::color_material::create(hit_color);
            let transform = Matrix4::from_translation(vec3(
                rayhit.hit_point.x,
                rayhit.hit_point.y,
                rayhit.hit_point.z,
            )) * Matrix4::from_scale(0.05);
            let mut new_obj = SceneObject::new(cm, Box::new(engine::scene::cube::create()));
            new_obj.set_transform(transform);
            scene_objects.push(new_obj);
        }

        scene_objects
    }

    fn color_from_state(&self) -> Vector3<f32> {
        if self.trigger_value < 0.1 && self.squeeze_value < 0.1 {
            vec3(1.0, 1.0, 1.0)
        } else {
            let r = self.trigger_value;
            let b = self.squeeze_value;
            let g = 0.0;
            vec3(r, g, b)
        }
    }
}

fn handle_empty_hand_state(
    handedness: Handedness,
    hand_position: Vector3<f32>,
    hand_rotation: Quaternion<f32>,
    frobbed_entity: Option<EntityId>,
    world: &World,
    physics: &PhysicsWorld,
    input_hand: &Hand,
) -> (VirtualHand, Vec<VirtualHandEffect>) {
    let ray_start = point3(hand_position.x, hand_position.y, hand_position.z);
    let forward = hand_rotation.rotate_vector(vec3(0.0, 0.0, -1.0));
    let result = physics.ray_cast(
        ray_start,
        forward,
        // TODO: Two raycasts...
        // One for hit damage,
        //InternalCollisionGroups::HITBOX
        InternalCollisionGroups::ENTITY
            | InternalCollisionGroups::SELECTABLE
            | InternalCollisionGroups::WORLD
            | InternalCollisionGroups::UI
            | InternalCollisionGroups::RAYCAST,
        // One for frobbing:
        //CollisionGroups::WORLD | CollisionGroups::SELECTABLE,
    );
    let result = result.map(|r| resolve_hit_proxy_entity(world, r));
    trace!("ray cast result: {:?}", &result);
    let mut msgs = Vec::new();
    let mut last_frobbed_entity = frobbed_entity;
    let mut next_hand_state = HandState::Empty;
    if input_hand.trigger_value > 0.5 || input_hand.a_value > 0.5 {
        if let Some(RayCastResult {
            hit_point: _,
            hit_normal: _,
            maybe_entity_id: Some(entity),
            maybe_rigid_body_handle: _,
            is_sensor: _,
        }) = result
        {
            if last_frobbed_entity.is_none() {
                msgs.push(VirtualHandEffect::OutMessage {
                    message: Message {
                        to: entity,
                        payload: {
                            if input_hand.trigger_value > 0.5 {
                                MessagePayload::Frob
                            } else {
                                MessagePayload::Slay
                                //MessagePayload::Damage { amount: 1.0 }
                            }
                        },
                    },
                });
                last_frobbed_entity = Some(entity);

                // Also, frob any items that may be nearby...
            }
        }
    } else {
        last_frobbed_entity = None
    }

    if input_hand.squeeze_value > 0.5 {
        if let Some(RayCastResult {
            hit_point: _,
            hit_normal: _,
            maybe_entity_id: Some(entity_id),
            maybe_rigid_body_handle: Some(rigid_body_handle),
            is_sensor: _,
        }) = result
        {
            if can_grab_item(world, entity_id) {
                let position = &physics.get_position(rigid_body_handle).unwrap();
                let _dir = hand_position - position;
                msgs.push(VirtualHandEffect::HoldItem { entity_id });

                next_hand_state = HandState::Grabbing { entity_id };
            }
        }
    }

    let updated_hand = VirtualHand {
        position: hand_position,
        rotation: hand_rotation,
        trigger_value: input_hand.trigger_value,
        squeeze_value: input_hand.squeeze_value,
        raytrace_hit: result,
        last_frobbed_entity,
        hand_state: next_hand_state,
        handedness,
    };
    (updated_hand, msgs)
}

///
/// get_held_position_orientation
///
/// Helper function to figure out the right way to 'hold' an item.
/// We use world models for our item, and the default orientation/position doesn't always make sense for how to hold it
/// (otherwise, we might end up holding the wrench sideways, or the shotgun backwards!)
fn get_held_position_orientation(
    entity_id: EntityId,
    world: &World,
    handedness: Handedness,
) -> vr_config::VRHandModelPerHandAdjustments {
    vr_config::get_vr_hand_model_adjustments_from_entity(entity_id, world, handedness)
}

fn can_grab_item(world: &World, entity_id: EntityId) -> bool {
    let v_prop_frobinfo = world.borrow::<View<PropFrobInfo>>().unwrap();

    if let Ok(frob_info) = v_prop_frobinfo.get(entity_id) {
        if frob_info.world_action.contains(FrobFlag::MOVE)
            || frob_info.world_action.contains(FrobFlag::USE_AMMO)
        {
            return true;
        }
    }

    false
}

///
/// In the case where we hit an entity that is 'proxied' (like, a hitbox that points to a parent),
/// resolve to the parent entity.
///
fn resolve_hit_proxy_entity(world: &World, ray_cast_result: RayCastResult) -> RayCastResult {
    let maybe_new_entity_id = ray_cast_result
        .maybe_entity_id
        .map(|entity_id| util::resolve_proxy_entity(world, entity_id));

    RayCastResult {
        maybe_entity_id: maybe_new_entity_id,
        ..ray_cast_result
    }
}
