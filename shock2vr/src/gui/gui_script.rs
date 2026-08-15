use cgmath::{Point2, point2, vec2};
use shipyard::{EntityId, World};

use crate::{
    gui::{Gui, GuiComponent, GuiHandle, GuiInputInfo},
    physics::PhysicsWorld,
    scripts::{Effect, MessagePayload, Script},
    time::Time,
    ui::UiCanvas,
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
    last_input_info_by_hand: [Option<GuiInputInfo>; 2],
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
            last_input_info_by_hand: [None, None],
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
        let config = self.gui.get_config_for(entity_id, world, &self.state);
        let components = {
            let cursor = &self.last_cursor;
            self.gui
                .get_components(cursor, entity_id, world, &self.state)
        };
        self.last_cursor = None;
        let mut canvas = UiCanvas::from_elements(config.screen_size_in_pixels, components);
        let size = vec2(16.0, 16.0);
        canvas.push(GuiComponent::Image {
            alpha: 0.5,
            position: vec2(self.cursor.x, self.cursor.y),
            size,
            texture: "cursor.pcx".to_owned(),
            kind: crate::ui::ImageKind::Ui,
        });
        let render_components = canvas
            .into_elements()
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
            // An offered item is a deposit by default (it goes into the
            // container), but a panel may claim it as a tool instead - see
            // `Gui::on_provide_for_consumption`.
            MessagePayload::ProvideForConsumption { entity } => self
                .gui
                .on_provide_for_consumption(entity_id, world, *entity)
                .unwrap_or(Effect::DropEntityInfo {
                    parent_entity_id: entity_id,
                    dropped_entity_id: *entity,
                }),
            // Frobbing a GUI-bearing entity opens its presentation's panel
            // slot (flat MFD or VR world quad), preserving the original's
            // frob-script -> overlay flow. The `on_frob` side effects (e.g. an
            // audio log recording + playing its clip) fire in both
            // presentations. A gui can veto the
            // open (a content-less log disc) and/or ask for fresh per-open
            // state (the reader's scroll position).
            // Opened without a frob (the audio-log reader): give the gui the
            // same fresh per-open state a frob-open would have.
            MessagePayload::PanelOpened => {
                self.gui.prepare_state_on_frob(&mut self.state);
                Effect::NoEffect
            }
            MessagePayload::Frob => {
                let frob_effect = self.gui.on_frob(entity_id, world);
                if !self.gui.opens_on_frob(entity_id, world) {
                    return frob_effect;
                }
                // The per-open reset arrives via `PanelOpened`, dispatched by
                // the `OpenPanel` handler, so both open paths share one hook.
                Effect::combine(vec![Effect::OpenPanel { entity: entity_id }, frob_effect])
            }
            MessagePayload::GUIHover {
                held_entity_id,
                screen_coordinates,
                is_triggered,
                is_grabbing,
                hand,
            } => {
                let config = self.gui.get_config_for(entity_id, world, &self.state);
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
                let canvas = UiCanvas::from_elements(
                    self.gui
                        .get_config_for(entity_id, world, &self.state)
                        .screen_size_in_pixels,
                    components,
                );
                self.last_cursor = cursor_obj;

                // Is the UI going to generate an event, based on the input state?
                let hand_index = match hand {
                    crate::vr_config::Handedness::Left => 0,
                    crate::vr_config::Handedness::Right => 1,
                };
                let mut maybe_output_event = None;
                if let Some(last) = &self.last_input_info_by_hand[hand_index] {
                    for c in canvas.elements() {
                        let maybe_event = c.get_event(last, &current_input_info);
                        if maybe_event.is_some() {
                            maybe_output_event = maybe_event;
                        }
                    }
                }

                self.cursor = cursor;
                self.last_input_info_by_hand[hand_index] = Some(current_input_info);

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

#[cfg(test)]
mod tests {
    use cgmath::{Vector2, Vector3, point2, vec2};

    use super::*;
    use crate::{gui, scripts::Script, vr_config::Handedness};

    struct CountingGui;

    impl Gui<usize, ()> for CountingGui {
        fn get_components(
            &self,
            _cursor: &Option<GuiCursor>,
            _entity_id: EntityId,
            _world: &World,
            _state: &usize,
        ) -> Vec<GuiComponent<()>> {
            vec![
                gui::button(())
                    .with_position(vec2(0.0, 0.0))
                    .with_size(vec2(100.0, 100.0)),
            ]
        }

        fn get_config(&self) -> crate::gui::GuiConfig {
            crate::gui::GuiConfig {
                world_offset: Vector3::new(0.0, 0.0, 0.0),
                screen_size_in_pixels: Vector2::new(100.0, 100.0),
            }
        }

        fn handle_msg(
            &self,
            _entity_id: EntityId,
            _world: &World,
            state: &usize,
            _msg: &(),
        ) -> (usize, Effect) {
            (state + 1, Effect::NoEffect)
        }
    }

    fn hover(hand: Handedness, pressed: bool) -> MessagePayload {
        MessagePayload::GUIHover {
            held_entity_id: None,
            screen_coordinates: point2(0.5, 0.5),
            is_triggered: pressed,
            is_grabbing: false,
            hand,
        }
    }

    fn send(
        script: &mut GuiScript<usize, ()>,
        entity: EntityId,
        world: &World,
        hand: Handedness,
        pressed: bool,
    ) {
        script.handle_message(entity, world, &PhysicsWorld::new(), &hover(hand, pressed));
    }

    #[test]
    fn each_vr_hand_tracks_its_own_trigger_edge_over_one_shared_panel() {
        let mut world = World::new();
        let entity = world.add_entity(());
        let mut script = GuiScript::new(Box::new(CountingGui));

        // VR dispatches the idle left hand before the active right hand each
        // frame. One held physical trigger must remain one panel activation.
        for pressed in [false, true, true, true] {
            send(&mut script, entity, &world, Handedness::Left, false);
            send(&mut script, entity, &world, Handedness::Right, pressed);
        }
        assert_eq!(script.state, 1);

        // Release and press again: one new physical edge is one new action.
        send(&mut script, entity, &world, Handedness::Left, false);
        send(&mut script, entity, &world, Handedness::Right, false);
        send(&mut script, entity, &world, Handedness::Left, false);
        send(&mut script, entity, &world, Handedness::Right, true);
        assert_eq!(script.state, 2);
    }

    #[test]
    fn one_hand_panel_press_still_fires_once_until_release() {
        let mut world = World::new();
        let entity = world.add_entity(());
        let mut script = GuiScript::new(Box::new(CountingGui));

        for pressed in [false, true, true, true, false, true] {
            send(&mut script, entity, &world, Handedness::Right, pressed);
        }

        assert_eq!(script.state, 2);
    }
}
