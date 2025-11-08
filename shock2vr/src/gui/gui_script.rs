use cgmath::{Point2, point2, vec2};
use shipyard::{EntityId, World};

use crate::{
    gui::{Gui, GuiComponent, GuiHandle, GuiInputInfo},
    physics::PhysicsWorld,
    scripts::{Effect, MessagePayload, Script},
    time::Time,
};

use super::{GUI_PIXEL_TO_WORLD_SIZE, GuiCursor};

pub struct GuiScript<TState, TMsg>
where
    TState: Default,
{
    handle: Option<GuiHandle>,
    cursor: Point2<f32>,
    gui: Box<dyn Gui<TState, TMsg>>,
    state: TState,
    last_input_info: Option<GuiInputInfo>,
    last_cursor: Option<GuiCursor>,
}

impl<TState, TMsg> GuiScript<TState, TMsg>
where
    TState: Default,
    TMsg: Clone,
{
    pub fn new(gui: Box<dyn crate::gui::Gui<TState, TMsg>>) -> GuiScript<TState, TMsg> {
        GuiScript {
            handle: None,
            cursor: point2(0.0, 0.0),
            gui,
            state: TState::default(),
            last_input_info: None,
            last_cursor: None,
        }
    }
}

impl<TState, TMsg> Script for GuiScript<TState, TMsg>
where
    TState: Default,
    TMsg: Clone,
{
    fn initialize(&mut self, _entity_id: EntityId, _world: &World) -> Effect {
        self.handle = Some(GuiHandle::new());
        Effect::NoEffect
    }

    fn update(
        &mut self,
        entity_id: EntityId,
        world: &World,
        _physics: &PhysicsWorld,
        _time: &Time,
    ) -> Effect {
        let config = self.gui.get_config();
        let mut components = {
            let cursor = &self.last_cursor;
            self.gui
                .get_components(cursor, entity_id, world, &self.state)
        };
        self.last_cursor = None;
        let size = vec2(16.0, 16.0);
        components.push(GuiComponent::Image {
            alpha: 0.5,
            position: vec2(self.cursor.x, self.cursor.y),
            size,
            texture: "cursor.pcx".to_owned(),
        });
        let render_components = components
            .into_iter()
            .map(|c| c.to_render_info(config.screen_size_in_pixels, self.cursor))
            .collect();

        Effect::SetUI {
            parent_entity: entity_id,
            handle: self.handle.unwrap(),
            world_offset: config.world_offset,
            world_size: config.screen_size_in_pixels * GUI_PIXEL_TO_WORLD_SIZE,
            components: render_components,
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
            MessagePayload::ProvideForConsumption { entity } => Effect::DropEntityInfo {
                parent_entity_id: entity_id,
                dropped_entity_id: *entity,
            },
            MessagePayload::GUIHover {
                held_entity_id,
                screen_coordinates,
                is_triggered,
                is_grabbing,
                hand,
            } => {
                let config = self.gui.get_config();
                let cursor = point2(
                    screen_coordinates.x * config.screen_size_in_pixels.x,
                    screen_coordinates.y * config.screen_size_in_pixels.y,
                );

                // Check if there is an event

                let current_input_info = GuiInputInfo {
                    cursor_position: cursor,
                    held_entity_id: *held_entity_id,
                    is_pressed: *is_triggered,
                    is_grabbed: *is_grabbing,
                    hand: *hand,
                };

                let cursor_obj = Some(GuiCursor {
                    position: cursor,
                    held_entity_id: *held_entity_id,
                });

                let components =
                    self.gui
                        .get_components(&cursor_obj, entity_id, world, &self.state);
                self.last_cursor = cursor_obj;

                // Is the UI going to generate an event, based on the input state?
                let mut maybe_output_event = None;
                if let Some(last) = &self.last_input_info {
                    for c in components {
                        let maybe_event = c.get_event(last, &current_input_info);
                        if maybe_event.is_some() {
                            maybe_output_event = maybe_event;
                        }
                    }
                }

                self.cursor = cursor;
                self.last_input_info = Some(current_input_info);

                if let Some(output_event) = maybe_output_event {
                    let (state, effect) =
                        self.gui
                            .handle_msg(entity_id, world, &self.state, &output_event);
                    self.state = state;
                    effect
                } else {
                    // Check if there is an event we should consider!
                    Effect::NoEffect
                }
            }
            _ => Effect::NoEffect,
        }
    }
}

pub fn gui_script<TState: Default + 'static, TMsg: Clone + 'static>(
    gui: Box<dyn Gui<TState, TMsg>>,
) -> Box<dyn Script> {
    Box::new(GuiScript::new(gui))
}
