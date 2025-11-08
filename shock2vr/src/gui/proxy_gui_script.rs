use cgmath::{Transform, Vector2, point2};
use shipyard::{EntityId, Get, View, World};

use crate::{
    physics::PhysicsWorld,
    runtime_props::RuntimePropTransform,
    scripts::{Effect, Message, MessagePayload, Script},
    time::Time,
    util::vec3_to_point3,
};

pub struct ProxyGuiScript {
    world_size: Vector2<f32>,
    parent_entity_id: EntityId,
}

impl ProxyGuiScript {
    pub fn new(world_size: Vector2<f32>, parent_entity_id: EntityId) -> ProxyGuiScript {
        ProxyGuiScript {
            world_size,
            parent_entity_id,
        }
    }
}

impl Script for ProxyGuiScript {
    fn initialize(&mut self, _entity_id: EntityId, _world: &World) -> Effect {
        Effect::NoEffect
    }

    fn update(
        &mut self,
        _entity_id: EntityId,
        _world: &World,
        _physics: &PhysicsWorld,
        _time: &Time,
    ) -> Effect {
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
            MessagePayload::ProvideForConsumption { entity } => Effect::Send {
                msg: Message {
                    payload: MessagePayload::ProvideForConsumption { entity: *entity },
                    to: self.parent_entity_id,
                },
            },
            MessagePayload::Hover {
                held_entity_id,
                world_position,
                is_triggered,
                is_grabbing,
                hand,
            } => {
                let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
                let maybe_transform = v_transform.get(entity_id);
                if let Ok(transform) = maybe_transform {
                    let inv_transform = transform.0.inverse_transform().unwrap();
                    let local_point =
                        inv_transform.transform_point(vec3_to_point3(*world_position));

                    let screen_point = point2(
                        1.0 - (local_point.x + self.world_size.x / 2.0) / self.world_size.x,
                        1.0 - (local_point.y + self.world_size.y / 2.0) / self.world_size.y,
                    );

                    Effect::Send {
                        msg: Message {
                            payload: MessagePayload::GUIHover {
                                held_entity_id: *held_entity_id,
                                screen_coordinates: screen_point,
                                is_triggered: *is_triggered,
                                is_grabbing: *is_grabbing,
                                hand: *hand,
                            },
                            to: self.parent_entity_id,
                        },
                    }
                } else {
                    Effect::NoEffect
                }
            }
            _ => Effect::NoEffect,
        }
    }
}
