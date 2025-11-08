use cgmath::{Vector2, Vector3, vec2, vec3};
use dark::properties::PropReplicatorContents;
use engine::audio::AudioHandle;
use num_traits::ToPrimitive;

use shipyard::{EntityId, Get, UniqueView, View, World};

use crate::{
    gui::{Gui, GuiComponent, GuiConfig, GuiCursor},
    mission::GlobalEntityMetadata,
    util::{get_position_from_transform, get_rotation_from_transform},
};

use crate::gui;

use crate::scripts::{Effect, script_util::*};

pub struct ReplicatorGui;

#[derive(Clone, Debug, Default)]
pub struct ReplicatorState {}

#[derive(Clone)]
pub enum ReplicatorMsg {
    SelectItem(String),
}

impl Gui<ReplicatorState, ReplicatorMsg> for ReplicatorGui {
    fn get_components(
        &self,
        _cursor: &Option<GuiCursor>,
        entity_id: EntityId,
        world: &World,
        _state: &ReplicatorState,
    ) -> Vec<GuiComponent<ReplicatorMsg>> {
        let button_height = 60.0;
        let initial_padding_y = 10.0;
        let button_width = 188.0;
        let button_padding = 4.0;

        let v_prop_replicator = world.borrow::<View<PropReplicatorContents>>().unwrap();
        let replicator_contents = v_prop_replicator.get(entity_id).unwrap();

        let entity_metadata = world.borrow::<UniqueView<GlobalEntityMetadata>>().unwrap();

        let replicator_icon = |icon: &str, position: f32| GuiComponent::Image {
            position: vec2(
                10.0,
                5.0 + initial_padding_y + (button_height + button_padding) * position,
            ),
            size: vec2(30.0, button_height - 10.0),
            texture: icon.to_owned(),
            alpha: 0.5,
        };

        let mut components: Vec<GuiComponent<ReplicatorMsg>> = vec![
            gui::image("replic.pcx")
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(188.0, 296.0)),
        ];
        for i in 0..6 {
            let float_i = i.to_f32().unwrap();

            if replicator_contents.object_names[i].is_empty() {
                continue;
            }

            let obj_name = replicator_contents.object_names[i].clone();

            let metadata = entity_metadata.0.get(&obj_name).unwrap();
            let obj_icon = metadata.obj_icon.as_ref().unwrap();

            components.push(
                gui::button(ReplicatorMsg::SelectItem(obj_name.clone()))
                    .with_position(vec2(
                        0.0,
                        initial_padding_y + (button_height + button_padding) * float_i,
                    ))
                    .with_size(vec2(button_width, button_height))
                    .with_image("key0.pcx"),
            );

            components.push(replicator_icon(obj_icon, float_i));

            if let Some(short_name) = metadata.obj_short_name.as_ref() {
                components.push(gui::text(short_name).with_position(vec2(
                    50.0,
                    button_height / 2.0 + (button_height + button_padding) * float_i,
                )));
            }
        }

        // components.push(GuiComponent::Text {
        //     position: vec2(10.0, 50.0),
        //     size: vec2(button_width, button_height),
        //     font: "mainfont.fon".to_owned(),
        //     text: "10,50".to_owned(),
        // });

        // components.push(GuiComponent::Text {
        //     position: vec2(120.0, 250.0),
        //     size: vec2(button_width, button_height),
        //     font: "mainfont.fon".to_owned(),
        //     text: "120,250".to_owned(),
        // });

        components
    }

    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: Vector3::new(0.0, 0.0, -1.0),
            screen_size_in_pixels: Vector2::new(188.0, 296.0),
        }
    }

    fn handle_msg(
        &self,
        entity_id: EntityId,
        world: &World,
        state: &ReplicatorState,
        msg: &ReplicatorMsg,
    ) -> (ReplicatorState, Effect) {
        match msg {
            ReplicatorMsg::SelectItem(item) => {
                let maybe_link =
                    get_first_link_of_type(world, entity_id, dark::properties::Link::Replicator);
                let link = maybe_link.unwrap();

                let eff = Effect::CreateEntityByTemplateName {
                    template_name: item.to_owned(),
                    position: get_position_from_transform(world, link, vec3(0.0, 0.0, 0.0)),
                    orientation: get_rotation_from_transform(world, link),
                };

                let sound_eff = Effect::PlaySound {
                    handle: AudioHandle::new(),
                    name: "replic2e".to_owned(),
                };

                (state.clone(), Effect::combine(vec![eff, sound_eff]))
            }
        }
    }
}
