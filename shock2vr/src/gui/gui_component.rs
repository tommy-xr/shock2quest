use std::rc::Rc;

use cgmath::{Deg, Matrix4, Point2, Vector2, vec2, vec3};
use dark::importers::{FONT_IMPORTER, TEXTURE_IMPORTER};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject, texture::TextureTrait};
use shipyard::EntityId;

use crate::{inventory::Inventory, vr_config::Handedness};

#[derive(Clone, Debug)]
pub enum ButtonHoverBehavior {
    None,
    Texture(String),
}

#[derive(Clone, Debug)]
pub enum GuiComponent<TEvent>
where
    TEvent: Clone,
{
    Image {
        position: Vector2<f32>,
        size: Vector2<f32>,
        texture: String,
        alpha: f32,
    },
    Button {
        position: Vector2<f32>,
        size: Vector2<f32>,
        texture: String,
        on_click: Option<TEvent>,
        on_grab: Option<(TEvent, TEvent)>,
        hover: ButtonHoverBehavior,
        alpha: f32,
    },
    Text {
        position: Vector2<f32>,
        size: Vector2<f32>,
        font: String,
        text: String,
        alpha: f32,
    },
    Inventory {
        inventory: Inventory,
    },
}

impl<TEvent> GuiComponent<TEvent>
where
    TEvent: Clone,
{
    pub fn with_position(self, new_position: Vector2<f32>) -> GuiComponent<TEvent> {
        match self {
            Self::Image {
                alpha,
                size,
                texture,
                ..
            } => Self::Image {
                alpha,
                position: new_position,
                size,
                texture,
            },
            Self::Inventory { inventory } => Self::Inventory { inventory },
            Self::Button {
                alpha,
                size,
                texture,
                on_click,
                on_grab,
                hover,
                ..
            } => Self::Button {
                alpha,
                position: new_position,
                size,
                texture,
                on_click,
                on_grab,
                hover,
            },
            Self::Text {
                alpha,
                size,
                font,
                text,
                ..
            } => Self::Text {
                size,
                font,
                text,
                position: new_position,
                alpha,
            },
        }
    }

    pub fn with_size(self, new_size: Vector2<f32>) -> GuiComponent<TEvent> {
        match self {
            Self::Inventory { inventory } => Self::Inventory { inventory },
            Self::Image {
                alpha,
                position,
                texture,
                ..
            } => Self::Image {
                alpha,
                position,
                size: new_size,
                texture,
            },
            Self::Button {
                alpha,
                position,
                texture,
                on_click,
                on_grab,
                hover,
                ..
            } => Self::Button {
                alpha,
                position,
                size: new_size,
                texture,
                on_click,
                hover,
                on_grab,
            },
            Self::Text {
                position,
                font,
                text,
                alpha,
                ..
            } => Self::Text {
                position,
                size: new_size,
                font,
                text,
                alpha,
            },
        }
    }

    pub fn with_onclick(self, click_event: TEvent) -> GuiComponent<TEvent> {
        match self {
            Self::Button {
                position,
                size,
                texture,
                on_grab,
                hover,
                alpha,
                ..
            } => GuiComponent::Button {
                alpha,
                position,
                size,
                texture,
                on_click: Some(click_event),
                on_grab,
                hover,
            },
            Self::Image {
                alpha,
                position,
                size,
                texture,
            } => Self::Image {
                alpha,
                position,
                size,
                texture,
            },
            Self::Text {
                position,
                size,
                font,
                text,
                alpha,
            } => Self::Text {
                position,
                size,
                font,
                text,
                alpha,
            },
            Self::Inventory { inventory } => Self::Inventory { inventory },
        }
    }

    pub fn with_alpha(self, alpha: f32) -> GuiComponent<TEvent> {
        match self {
            Self::Image {
                position,
                size,
                texture,
                ..
            } => Self::Image {
                position,
                size,
                texture,
                alpha,
            },

            Self::Inventory { inventory } => Self::Inventory { inventory },
            Self::Button {
                position,
                size,
                texture,
                on_click,
                on_grab,
                hover,
                ..
            } => Self::Button {
                position,
                size,
                texture,
                on_click,
                on_grab,
                hover,
                alpha,
            },
            Self::Text {
                position,
                size,
                font,
                text,
                ..
            } => Self::Text {
                position,
                size,
                font,
                text,
                alpha,
            },
        }
    }

    pub fn with_hover(self, hover: ButtonHoverBehavior) -> GuiComponent<TEvent> {
        match self {
            Self::Image {
                position,
                size,
                texture,
                alpha,
                ..
            } => Self::Image {
                alpha,
                position,
                size,
                texture,
            },
            Self::Inventory { inventory } => Self::Inventory { inventory },
            Self::Button {
                alpha,
                position,
                size,
                texture,
                on_click,
                on_grab,
                ..
            } => Self::Button {
                alpha,
                position,
                size,
                texture,
                on_click,
                on_grab,
                hover,
            },
            Self::Text {
                position,
                size,
                font,
                text,
                alpha,
                ..
            } => Self::Text {
                position,
                size,
                font,
                text,
                alpha,
            },
        }
    }

    pub fn with_image(self, image: &str) -> GuiComponent<TEvent> {
        match self {
            Self::Image {
                alpha,
                position,
                size,
                ..
            } => Self::Image {
                alpha,
                position,
                size,
                texture: image.to_owned(),
            },
            Self::Inventory { inventory } => Self::Inventory { inventory },
            Self::Button {
                alpha,
                position,
                size,
                on_click,
                on_grab,
                hover,
                ..
            } => Self::Button {
                alpha,
                position,
                size,
                texture: image.to_owned(),
                on_click,
                on_grab,
                hover,
            },
            Self::Text { .. } => self,
        }
    }
}

pub fn image<TMsg: Clone>(texture: &str) -> GuiComponent<TMsg> {
    GuiComponent::Image {
        position: vec2(0.0, 0.0),
        size: vec2(30.0, 30.0),
        texture: texture.to_owned(),
        alpha: 0.5,
    }
}

pub fn button<TMsg: Clone>(on_click: TMsg) -> GuiComponent<TMsg> {
    GuiComponent::Button {
        position: vec2(0.0, 0.0),
        size: vec2(30.0, 30.0),
        texture: "key0.pcx".to_owned(),
        on_click: Some(on_click),
        on_grab: None,
        hover: ButtonHoverBehavior::None,
        alpha: 0.5,
    }
}

pub fn grabbable<TMsg: Clone>(on_left_grab: TMsg, on_right_grab: TMsg) -> GuiComponent<TMsg> {
    GuiComponent::Button {
        position: vec2(0.0, 0.0),
        size: vec2(30.0, 30.0),
        texture: "key0.pcx".to_owned(),
        on_click: None,
        on_grab: Some((on_left_grab, on_right_grab)),
        hover: ButtonHoverBehavior::None,
        alpha: 0.5,
    }
}

pub fn text<TMsg: Clone>(text: &str) -> GuiComponent<TMsg> {
    GuiComponent::Text {
        position: vec2(0.0, 0.0),
        size: vec2(30.0, 30.0),
        text: text.to_owned(),
        font: "mainfont.fon".to_owned(),
        alpha: 1.0,
    }
}

#[derive(Clone, Debug)]
pub enum GuiComponentRenderInfo {
    Image {
        position: Vector2<f32>,
        size: Vector2<f32>,
        texture: String,
        alpha: f32,
    },
    Text {
        position: Vector2<f32>,
        size: Vector2<f32>,
        font: String,
        text: String,
        alpha: f32,
    },
}

impl GuiComponentRenderInfo {
    pub fn position(&self) -> Vector2<f32> {
        match self {
            Self::Image { position, .. } => *position,
            Self::Text { position, .. } => *position,
        }
    }

    pub fn size(&self) -> Vector2<f32> {
        match self {
            Self::Image { size, .. } => *size,
            Self::Text { size, .. } => *size,
        }
    }

    pub fn render(&self, asset_cache: &mut AssetCache) -> SceneObject {
        let scene_object = match self {
            Self::Image {
                position,
                size,
                texture,
                alpha,
            } => {
                let texture: Rc<dyn TextureTrait> =
                    asset_cache.get(&TEXTURE_IMPORTER, texture).clone();
                let comp_mat = engine::scene::basic_material::create(texture, 1.0, 1.0 - alpha);
                let mut comp_obj =
                    SceneObject::new(comp_mat, Box::new(engine::scene::quad::create()));
                comp_obj.set_local_transform(
                    Matrix4::from_angle_z(Deg(180.0))
                        * Matrix4::from_translation(vec3(
                            position.x - 0.5 + size.x / 2.0,
                            position.y - 0.5 + size.y / 2.0,
                            0.0,
                        ))
                        * Matrix4::from_nonuniform_scale(size.x, size.y, 1.0),
                );
                comp_obj
            }
            Self::Text {
                position,
                size: _,
                text,
                font,
                alpha,
            } => {
                let font = asset_cache.get(&FONT_IMPORTER, font).clone();

                let mut text =
                    SceneObject::world_space_text(text, font, (1.0 - alpha).max(0.0).min(1.0));
                text.set_local_transform(
                    Matrix4::from_angle_y(Deg(180.0))
                        * Matrix4::from_translation(vec3(position.x - 0.5, position.y - 0.5, 0.01)),
                );
                text
            }
        };

        scene_object
    }
}

#[derive(Clone, Debug)]
pub struct GuiInputInfo {
    #[allow(dead_code)]
    pub(crate) held_entity_id: Option<EntityId>,
    pub(crate) cursor_position: Point2<f32>,
    pub(crate) is_pressed: bool,
    pub(crate) is_grabbed: bool,
    pub(crate) hand: Handedness,
}

fn is_in_bounds(position: &Vector2<f32>, size: &Vector2<f32>, cursor: Point2<f32>) -> bool {
    cursor.x >= position.x
        && cursor.y >= position.y
        && cursor.x <= position.x + size.x
        && cursor.y <= position.y + size.y
}

impl<TEvent> GuiComponent<TEvent>
where
    TEvent: Clone,
{
    pub fn to_render_info(
        &self,
        screen_size: Vector2<f32>,
        screen_space_cursor: Point2<f32>,
    ) -> GuiComponentRenderInfo {
        match self {
            GuiComponent::Inventory { inventory: _ } => todo!("not implemented"),
            GuiComponent::Text {
                position,
                size,
                font,
                text,
                alpha,
            } => GuiComponentRenderInfo::Text {
                position: vec2(position.x / screen_size.x, (-position.y) / screen_size.y),
                size: vec2(size.x / screen_size.x, size.y / screen_size.y),
                font: font.clone(),
                text: text.clone(),
                alpha: *alpha,
            },
            GuiComponent::Image {
                position,
                size,
                texture,
                alpha,
            } => GuiComponentRenderInfo::Image {
                position: vec2(position.x / screen_size.x, position.y / screen_size.y),
                size: vec2(size.x / screen_size.x, size.y / screen_size.y),
                texture: texture.clone(),
                alpha: *alpha,
            },
            GuiComponent::Button {
                position,
                size,
                texture,
                hover,
                alpha,
                ..
            } => {
                let is_hovered = is_in_bounds(position, size, screen_space_cursor);

                let position = vec2(position.x / screen_size.x, position.y / screen_size.y);
                let size = vec2(size.x / screen_size.x, size.y / screen_size.y);

                let texture = if !is_hovered {
                    texture.clone()
                } else {
                    match hover {
                        ButtonHoverBehavior::None => texture.clone(),
                        ButtonHoverBehavior::Texture(hover_texture) => hover_texture.clone(),
                    }
                };

                GuiComponentRenderInfo::Image {
                    position,
                    size,
                    texture,
                    alpha: *alpha,
                }
            }
        }
    }

    pub fn get_event(
        &self,
        last_input: &GuiInputInfo,
        current_input: &GuiInputInfo,
    ) -> Option<TEvent> {
        match self {
            GuiComponent::Inventory { inventory: _ } => todo!("not implemented"),
            GuiComponent::Text { .. } => None,
            GuiComponent::Image { .. } => None,
            GuiComponent::Button {
                on_click,
                on_grab,
                position,
                size,
                ..
            } => {
                let is_pressed = !last_input.is_pressed && current_input.is_pressed;
                let is_grabbed = current_input.is_grabbed;

                if is_pressed || is_grabbed {
                    if is_in_bounds(position, size, current_input.cursor_position) {
                        if is_pressed {
                            on_click.clone()
                        } else if is_grabbed {
                            if let Some((left_grab, right_grab)) = on_grab {
                                if current_input.hand == Handedness::Left {
                                    Some(left_grab.clone())
                                } else {
                                    Some(right_grab.clone())
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
    }
}
