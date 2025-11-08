use cgmath::{Vector2, Vector3, vec2};

use shipyard::{EntityId, World};

use crate::gui::{Gui, GuiComponent, GuiConfig, GuiCursor};

use crate::gui;

use crate::scripts::Effect;

pub struct ElevatorGui;

#[derive(Clone, Debug, Default)]
pub struct ElevatorGuiState {}

#[derive(Clone)]
pub enum ElevatorGuiMsg {
    ButtonPressed(String),
}

impl Gui<ElevatorGuiState, ElevatorGuiMsg> for ElevatorGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        _entity_id: EntityId,
        _world: &World,
        _state: &ElevatorGuiState,
    ) -> Vec<GuiComponent<ElevatorGuiMsg>> {
        let button_height = 54.0;
        let initial_padding_y = 6.0;
        let initial_padding_x = 14.0;
        let button_width = 142.0;
        let button_padding = 4.0;

        let stops = vec![
            ("rec1.mis", "elev50.pcx", "elev51.pcx", "5: Recreation"),
            ("ops2.mis", "elev40.pcx", "elev41.pcx", "4: Operations"),
            ("hydro2.mis", "elev30.pcx", "elev31.pcx", "3: Hydropondics"),
            ("medsci1.mis", "elev20.pcx", "elev21.pcx", "2: Med/Sci"),
            ("eng1.mis", "elev10.pcx", "elev11.pcx", "1: Engineering"),
        ];

        let mut components: Vec<GuiComponent<ElevatorGuiMsg>> = vec![
            gui::image("elev.pcx")
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(188.0, 296.0)),
        ];

        for (i, (level, up_texture, _down_texture, label)) in stops.iter().enumerate() {
            let button_y = initial_padding_y + (button_height + button_padding) * i as f32;
            let button_x = initial_padding_x;

            components.push(
                gui::button(ElevatorGuiMsg::ButtonPressed((*level).to_owned()))
                    .with_position(vec2(button_x, button_y))
                    .with_size(vec2(button_width, button_height))
                    .with_image(up_texture), //.with_hover_texture((*down_texture).to_owned()),
            );

            components.push(
                gui::text(label.to_owned())
                    .with_position(vec2(button_x + 60.0, button_y + 30.0))
                    .with_size(vec2(100.0, 20.0))
                    .with_alpha(0.7),
            );
        }

        components
    }

    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: Vector3::new(0.0, 0.0, -0.2),
            screen_size_in_pixels: Vector2::new(188.0, 296.0),
        }
    }

    fn handle_msg(
        &self,
        _entity_id: EntityId,
        _world: &World,
        state: &ElevatorGuiState,
        msg: &ElevatorGuiMsg,
    ) -> (ElevatorGuiState, Effect) {
        match msg {
            ElevatorGuiMsg::ButtonPressed(level) => (
                state.clone(),
                Effect::GlobalEffect(crate::scripts::GlobalEffect::TransitionLevel {
                    level_file: level.clone(),
                    loc: Some(22),
                    entities_to_trigger: vec![],
                }),
            ),
        }
    }
}
