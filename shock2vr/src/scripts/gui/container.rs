use cgmath::{Vector2, Vector3, vec2};
use dark::properties::{Link, PropInventoryDimensions, PropObjIcon};

use shipyard::{EntityId, Get, View, World};

use crate::{
    gui::{Gui, GuiComponent, GuiConfig, GuiCursor},
    inventory::Inventory,
    scripts::{Message, script_util},
};

use crate::gui;

use crate::scripts::{Effect, MessagePayload};

pub struct ContainerGui {
    background_image: String,
    width: f32,
    height: f32,
    inv_offset_x: f32,
    inv_offset_y: f32,
    num_slots_x: usize,
    num_slots_y: usize,
}

impl ContainerGui {
    pub fn loot_container() -> ContainerGui {
        ContainerGui {
            background_image: "contain.pcx".to_owned(),
            width: 188.0,
            height: 296.0,
            inv_offset_x: 15.0,
            inv_offset_y: 160.0,
            num_slots_x: 4,
            num_slots_y: 4,
        }
    }

    pub fn inv_container() -> ContainerGui {
        ContainerGui {
            background_image: "invback.pcx".to_owned(),
            width: 635.0,
            height: 120.0,
            inv_offset_x: 4.0,
            inv_offset_y: 18.0,
            num_slots_x: 15,
            num_slots_y: 3,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ContainerGuiState {}

#[derive(Clone)]
pub enum ContainerGuiMsg {
    GrabbedWithLeftHand(EntityId),
    GrabbedWithRightHand(EntityId),
    Frob(EntityId),
}

impl Gui<ContainerGuiState, ContainerGuiMsg> for ContainerGui {
    fn get_components(
        &self,
        maybe_cursor: &Option<GuiCursor>,
        entity_id: EntityId,
        world: &World,
        _state: &ContainerGuiState,
    ) -> Vec<GuiComponent<ContainerGuiMsg>> {
        let mut components: Vec<GuiComponent<ContainerGuiMsg>> = vec![
            gui::image(self.background_image.as_str())
                .with_position(vec2(0.0, 0.0))
                .with_size(vec2(self.width, self.height)),
        ];

        let mut contained_entities =
            script_util::get_all_links_with_data(world, entity_id, |link| match link {
                Link::Contains(ordinal) => Some(*ordinal),
                _ => None,
            });

        contained_entities.sort_by(|a, b| a.1.cmp(&b.1));

        let mut inventory = Inventory::new(self.num_slots_x, self.num_slots_y);

        let v_inv_dims = world.borrow::<View<PropInventoryDimensions>>().unwrap();
        for entity in contained_entities {
            let inv_dims = v_inv_dims
                .get(entity.0)
                .map(|dims| (dims.width, dims.height))
                .unwrap_or((1, 1));
            inventory.insert_first_available(entity.0, inv_dims.0 as usize, inv_dims.1 as usize);
        }

        let slot_pixel_width = 35.0;
        let slot_pixel_height = 32.0;
        let initial_offset_y = self.inv_offset_y;
        let initial_offset_x = self.inv_offset_x;

        let v_obj_icon = world.borrow::<View<PropObjIcon>>().unwrap();
        for contained_entity_info in inventory.all_items() {
            let ent = contained_entity_info.entity;
            let maybe_obj_icon = v_obj_icon.get(ent);

            if maybe_obj_icon.is_err() {
                continue;
            }

            let inv_dims = (contained_entity_info.width, contained_entity_info.height);
            let position_x = slot_pixel_width * contained_entity_info.x as f32;
            let position_y = slot_pixel_height * contained_entity_info.y as f32;

            let obj_icon = &maybe_obj_icon.unwrap().0;

            // TODO: Fix this issue:
            // thread 'main' panicked at 'called `Result::unwrap()` on an `Err` value: Custom { kind: InvalidInput, error: "pcx::Reader::next_row_paletted called on non-paletted image" }', engine/src/texture_format.rs:81:45
            if obj_icon.contains("upgrade") {
                continue;
            }

            components.push(
                gui::grabbable(
                    ContainerGuiMsg::GrabbedWithLeftHand(ent),
                    ContainerGuiMsg::GrabbedWithRightHand(ent),
                )
                .with_onclick(ContainerGuiMsg::Frob(ent))
                .with_image(&format!("{}.pcx", obj_icon))
                .with_position(vec2(
                    initial_offset_x + position_x,
                    initial_offset_y + position_y,
                ))
                .with_size(vec2(
                    slot_pixel_width * inv_dims.0 as f32,
                    slot_pixel_height * inv_dims.1 as f32,
                )),
            )
        }

        if let Some(cursor) = maybe_cursor {
            if let Some(ent) = cursor.held_entity_id {
                let maybe_obj_icon = v_obj_icon.get(ent);

                if let Ok(obj_icon) = maybe_obj_icon {
                    let inv_dims = v_inv_dims
                        .get(ent)
                        .map(|dims| (dims.width, dims.height))
                        .unwrap_or((1, 1));
                    components.push(
                        gui::image(&format!("{}.pcx", obj_icon.0))
                            .with_position(vec2(cursor.position.x, cursor.position.y))
                            .with_size(vec2(
                                slot_pixel_width * inv_dims.0 as f32,
                                slot_pixel_height * inv_dims.1 as f32,
                            )),
                    );
                }
            }
        }

        components
    }

    fn get_config(&self) -> GuiConfig {
        GuiConfig {
            world_offset: Vector3::new(0.0, 1.0, 0.0),
            screen_size_in_pixels: Vector2::new(self.width, self.height),
        }
    }

    fn handle_msg(
        &self,
        _entity_id: EntityId,
        _world: &World,
        state: &ContainerGuiState,
        msg: &ContainerGuiMsg,
    ) -> (ContainerGuiState, Effect) {
        match msg {
            ContainerGuiMsg::GrabbedWithLeftHand(ent) => (
                state.clone(),
                Effect::GrabEntity {
                    entity_id: *ent,
                    hand: crate::vr_config::Handedness::Left,
                    // TODO: Set this up to break link
                    current_parent_id: None,
                },
            ),
            ContainerGuiMsg::GrabbedWithRightHand(ent) => (
                state.clone(),
                Effect::GrabEntity {
                    entity_id: *ent,
                    hand: crate::vr_config::Handedness::Right,
                    // TODO: Set this up to break link
                    current_parent_id: None,
                },
            ),
            ContainerGuiMsg::Frob(ent) => (
                state.clone(),
                Effect::Send {
                    msg: Message {
                        payload: MessagePayload::Frob,
                        to: *ent,
                    },
                },
            ),
        }
        //(state.clone(), Effect::NoEffect)
    }
}
