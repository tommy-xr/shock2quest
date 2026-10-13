use cgmath::{Transform, Vector2, point2};
use shipyard::{EntityId, Get, View, World};

use crate::{
    physics::PhysicsWorld,
    runtime_props::RuntimePropTransform,
    scripts::{Effect, Message, MessagePayload, Script},
    time::Time,
    util::vec3_to_point3,
};

use super::GuiPropProxySize;

pub struct ProxyGuiScript {
    parent_entity_id: EntityId,
}

impl ProxyGuiScript {
    pub fn new(parent_entity_id: EntityId) -> ProxyGuiScript {
        ProxyGuiScript { parent_entity_id }
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
                    let world_size = world
                        .borrow::<View<GuiPropProxySize>>()
                        .ok()
                        .and_then(|sizes| sizes.get(entity_id).ok().map(|size| size.0))
                        .unwrap_or(Vector2::new(1.0, 1.0));
                    let inv_transform = transform.0.inverse_transform().unwrap();
                    let local_point =
                        inv_transform.transform_point(vec3_to_point3(*world_position));

                    let mut screen_point = point2(
                        1.0 - (local_point.x + world_size.x / 2.0) / world_size.x,
                        1.0 - (local_point.y + world_size.y / 2.0) / world_size.y,
                    );

                    if let Some(screen) = super::handheld_screen::HandheldScreen::for_entity(
                        world,
                        self.parent_entity_id,
                    ) {
                        let surface = screen.surface();
                        // Margins are outside the content, but still carry
                        // button edges so dragging a held press in cannot click
                        // and releasing outside cannot leave a stale latch.
                        let point = screen
                            .viewport()
                            .to_src_point(cgmath::vec2(
                                screen_point.x * surface.w,
                                screen_point.y * surface.h,
                            ))
                            .unwrap_or(cgmath::vec2(-188.0, -296.0));
                        screen_point = point2(point.x / 188.0, point.y / 296.0);
                    }
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
