use cgmath::{Vector2, Vector3, vec2};
use dark::properties::PropKeypadCode;
use engine::audio::AudioHandle;

use shipyard::{EntityId, Get, View, World};

use crate::gui::{self, ButtonHoverBehavior, Gui, GuiComponent, GuiConfig, GuiCursor};

use crate::scripts::{Effect, MessagePayload, script_util::*};

pub struct KeyPadGui;

#[derive(Clone, Debug, Default)]
pub struct KeyPadState {
    current_value: Option<u32>,
}

#[derive(Clone)]
pub enum KeyPadMsg {
    ButtonPressed(u32),
    Clear,
}

fn get_texture_for_char(char: char) -> String {
    format!("key{}0.pcx", char)
}

fn draw_number(num: u32) -> Vec<GuiComponent<KeyPadMsg>> {
    let num_str = num.to_string();

    let offset_left = 10.0;
    let offset_top = 10.0;
    let padding = 1.5;
    let mut x = 0.0;
    let numeral_width = 22.5;
    let numeral_height = 30.0;

    let reversed_chars: Vec<char> = num_str.chars().collect();

    let mut ret = Vec::new();
    for ch in reversed_chars {
        ret.push(GuiComponent::Image {
            position: vec2(x + offset_left, offset_top),
            size: vec2(numeral_width, numeral_height),
            texture: get_texture_for_char(ch),
            alpha: 0.5,
        });
        x += numeral_width + padding;
    }
    ret
}

impl Gui<KeyPadState, KeyPadMsg> for KeyPadGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        _entity_id: EntityId,
        _world: &World,
        _state: &KeyPadState,
    ) -> Vec<GuiComponent<KeyPadMsg>> {
        let button_width = 45.0;
        let button_height = 60.0;
        let left_margin = 15.0;
        let top_margin = 42.0;
        let padding = 1.5;

        let mut components: Vec<GuiComponent<KeyPadMsg>> = vec![
            gui::image("keypad2.pcx")
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(188.0, 296.0)),
            // First row of buttons
            gui::button(KeyPadMsg::ButtonPressed(1))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 0.0,
                    top_margin,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key10.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key11.pcx".to_owned())),
            gui::button(KeyPadMsg::ButtonPressed(2))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 1.0,
                    top_margin,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key20.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key21.pcx".to_owned())),
            gui::button(KeyPadMsg::ButtonPressed(3))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 2.0,
                    top_margin,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key30.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key31.pcx".to_owned())),
            // Second row of buttons
            gui::button(KeyPadMsg::ButtonPressed(4))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 0.0,
                    top_margin + (button_height + padding) * 1.0,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key40.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key41.pcx".to_owned())),
            gui::button(KeyPadMsg::ButtonPressed(5))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 1.0,
                    top_margin + (button_height + padding) * 1.0,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key50.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key51.pcx".to_owned())),
            gui::button(KeyPadMsg::ButtonPressed(6))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 2.0,
                    top_margin + (button_height + padding) * 1.0,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key60.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key61.pcx".to_owned())),
            // Third row of buttons
            gui::button(KeyPadMsg::ButtonPressed(7))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 0.0,
                    top_margin + (button_height + padding) * 2.0,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key70.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key71.pcx".to_owned())),
            gui::button(KeyPadMsg::ButtonPressed(8))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 1.0,
                    top_margin + (button_height + padding) * 2.0,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key80.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key81.pcx".to_owned())),
            gui::button(KeyPadMsg::ButtonPressed(9))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 2.0,
                    top_margin + (button_height + padding) * 2.0,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key90.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key91.pcx".to_owned())),
            // Fourth row of buttons
            gui::button(KeyPadMsg::ButtonPressed(0))
                .with_position(vec2(
                    left_margin + (button_width + padding) * 0.0,
                    top_margin + (button_height + padding) * 3.0,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("key00.pcx")
                .with_hover(ButtonHoverBehavior::Texture("key01.pcx".to_owned())),
            gui::button(KeyPadMsg::Clear)
                .with_position(vec2(
                    left_margin + (button_width + padding) * 1.0,
                    top_margin + (button_height + padding) * 3.0,
                ))
                .with_size(vec2(button_width, button_height))
                .with_image("keyn0.pcx")
                .with_hover(ButtonHoverBehavior::Texture("keyn1.pcx".to_owned())),
        ];

        if let Some(v) = _state.current_value {
            components.extend(draw_number(v))
        }
        components
    }

    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: Vector3::new(0.0, 0.0, -0.1),
            screen_size_in_pixels: Vector2::new(188.0, 296.0),
        }
    }

    fn handle_msg(
        &self,
        entity_id: EntityId,
        world: &World,
        state: &KeyPadState,
        msg: &KeyPadMsg,
    ) -> (KeyPadState, crate::Effect) {
        let v_prop_keypad_code = world.borrow::<View<PropKeypadCode>>().unwrap();
        let maybe_keypad_code = v_prop_keypad_code.get(entity_id);

        let press_effect = Effect::PlaySound {
            handle: AudioHandle::new(),
            name: "bkeypad".to_owned(),
        };

        let new_state = match msg {
            KeyPadMsg::ButtonPressed(n) => {
                let new_value = match state.current_value {
                    Some(current_value) => {
                        // If current value is at least 5 digits, reset
                        // TODO: Is it guaranteed that all keypad codes are 5 digits?
                        if current_value > 9999 {
                            *n
                        } else {
                            current_value * 10 + n
                        }
                    }
                    None => *n,
                };
                KeyPadState {
                    current_value: Some(new_value),
                }
            }
            KeyPadMsg::Clear => KeyPadState {
                current_value: None,
            },
        };

        // Check if the keypad code matches
        let additional_effect = if let Ok(keypad_code) = maybe_keypad_code {
            if let Some(current_value) = new_state.current_value {
                if current_value == keypad_code.0 {
                    let switch_link_effect = send_to_all_switch_links_and_self(
                        world,
                        entity_id,
                        MessagePayload::TurnOn { from: entity_id },
                    );
                    let sound_effect = Effect::PlaySound {
                        handle: AudioHandle::new(),
                        name: "hacksucc".to_owned(),
                    };
                    Effect::combine(vec![switch_link_effect, sound_effect])
                } else {
                    Effect::NoEffect
                }
            } else {
                Effect::NoEffect
            }
        } else {
            Effect::NoEffect
        };

        (
            new_state,
            Effect::combine(vec![additional_effect, press_effect]),
        )
    }
}
