use std::rc::Rc;

use cgmath::{Deg, Matrix4, Point2, Vector2, vec2, vec3};
use dark::importers::{FONT_IMPORTER, TEXTURE_IMPORTER};
use engine::{
    assets::asset_cache::AssetCache,
    scene::SceneObject,
    texture::{TextureOptions, TextureTrait},
};
use shipyard::EntityId;

use crate::{
    ui::{HAlign, ImageKind, UiElement, VAlign},
    vr_config::Handedness,
};

pub use crate::ui::ButtonHoverBehavior;

/// Compatibility name for the interaction framework. GUI panels and
/// screen-space canvases now author the same [`UiElement`] description.
pub type GuiComponent<TEvent> = UiElement<TEvent>;

impl<TEvent> GuiComponent<TEvent>
where
    TEvent: Clone,
{
    pub fn with_position(self, new_position: Vector2<f32>) -> GuiComponent<TEvent> {
        match self {
            Self::Image {
                size,
                texture,
                alpha,
                kind,
                ..
            } => Self::Image {
                position: new_position,
                size,
                texture,
                alpha,
                kind,
            },
            Self::Bar {
                size,
                texture,
                fill,
                alpha,
                ..
            } => Self::Bar {
                position: new_position,
                size,
                texture,
                fill,
                alpha,
            },
            Self::Button {
                size,
                texture,
                on_click,
                on_grab,
                hover,
                alpha,
                entity,
                label,
                kind,
                ..
            } => Self::Button {
                position: new_position,
                size,
                texture,
                on_click,
                on_grab,
                hover,
                alpha,
                entity,
                label,
                kind,
            },
            Self::Text {
                size,
                text,
                font,
                font_size,
                h,
                v,
                alpha,
                ..
            } => Self::Text {
                position: new_position,
                size,
                text,
                font,
                font_size,
                h,
                v,
                alpha,
                fit_to_rect: false,
            },
        }
    }

    pub fn with_size(self, new_size: Vector2<f32>) -> GuiComponent<TEvent> {
        match self {
            Self::Image {
                position,
                texture,
                alpha,
                kind,
                ..
            } => Self::Image {
                position,
                size: new_size,
                texture,
                alpha,
                kind,
            },
            Self::Bar {
                position,
                texture,
                fill,
                alpha,
                ..
            } => Self::Bar {
                position,
                size: new_size,
                texture,
                fill,
                alpha,
            },
            Self::Button {
                position,
                texture,
                on_click,
                on_grab,
                hover,
                alpha,
                entity,
                label,
                kind,
                ..
            } => Self::Button {
                position,
                size: new_size,
                texture,
                on_click,
                on_grab,
                hover,
                alpha,
                entity,
                label,
                kind,
            },
            Self::Text {
                position,
                text,
                font,
                font_size,
                h,
                v,
                alpha,
                ..
            } => Self::Text {
                position,
                size: new_size,
                text,
                font,
                font_size,
                h,
                v,
                alpha,
                fit_to_rect: false,
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
                entity,
                label,
                kind,
                ..
            } => GuiComponent::Button {
                kind,
                alpha,
                position,
                size,
                texture,
                on_click: Some(click_event),
                on_grab,
                hover,
                entity,
                label,
            },
            other => other,
        }
    }

    pub fn with_alpha(self, alpha: f32) -> GuiComponent<TEvent> {
        match self {
            Self::Image {
                position,
                size,
                texture,
                kind,
                ..
            } => Self::Image {
                position,
                size,
                texture,
                alpha,
                kind,
            },
            Self::Bar {
                position,
                size,
                texture,
                fill,
                ..
            } => Self::Bar {
                position,
                size,
                texture,
                fill,
                alpha,
            },
            Self::Button {
                position,
                size,
                texture,
                on_click,
                on_grab,
                hover,
                entity,
                label,
                kind,
                ..
            } => Self::Button {
                position,
                size,
                texture,
                on_click,
                on_grab,
                hover,
                alpha,
                entity,
                label,
                kind,
            },
            Self::Text {
                position,
                size,
                text,
                font,
                font_size,
                h,
                v,
                ..
            } => Self::Text {
                position,
                size,
                text,
                font,
                font_size,
                h,
                v,
                alpha,
                fit_to_rect: false,
            },
        }
    }

    pub fn with_hover(self, hover: ButtonHoverBehavior) -> GuiComponent<TEvent> {
        match self {
            Self::Button {
                alpha,
                position,
                size,
                texture,
                on_click,
                on_grab,
                entity,
                label,
                kind,
                ..
            } => Self::Button {
                alpha,
                position,
                size,
                texture,
                on_click,
                on_grab,
                hover,
                entity,
                label,
                kind,
            },
            other => other,
        }
    }

    pub fn with_image(self, image: &str) -> GuiComponent<TEvent> {
        match self {
            Self::Image {
                alpha,
                position,
                size,
                kind,
                ..
            } => Self::Image {
                alpha,
                position,
                size,
                texture: image.to_owned(),
                kind,
            },
            Self::Bar {
                alpha,
                position,
                size,
                fill,
                ..
            } => Self::Bar {
                alpha,
                position,
                size,
                fill,
                texture: image.to_owned(),
            },
            Self::Button {
                alpha,
                position,
                size,
                on_click,
                on_grab,
                hover,
                entity,
                label,
                kind,
                ..
            } => Self::Button {
                alpha,
                position,
                size,
                texture: image.to_owned(),
                on_click,
                on_grab,
                hover,
                entity,
                label,
                kind,
            },
            Self::Text { .. } => self,
        }
    }
    /// Declare this element's art to be Dark object-icon art: keyed on
    /// palette index 0 and blitted at its authored pixel size, centered in
    /// the element's rect (see [`ImageKind`]). The rect still defines layout
    /// and hit-testing.
    pub fn with_object_icon(self) -> GuiComponent<TEvent> {
        match self {
            Self::Image {
                position,
                size,
                texture,
                alpha,
                ..
            } => Self::Image {
                position,
                size,
                texture,
                alpha,
                kind: ImageKind::ObjectIcon,
            },
            Self::Button {
                position,
                size,
                texture,
                on_click,
                on_grab,
                hover,
                alpha,
                entity,
                label,
                ..
            } => Self::Button {
                position,
                size,
                texture,
                on_click,
                on_grab,
                hover,
                alpha,
                entity,
                label,
                kind: ImageKind::ObjectIcon,
            },
            other => other,
        }
    }

    /// Tag a `Button` with the world entity it stands for (no-op for other
    /// component kinds) - see `GuiComponent::Button::entity`.
    pub fn with_entity(self, new_entity: EntityId) -> GuiComponent<TEvent> {
        match self {
            Self::Button {
                alpha,
                position,
                size,
                texture,
                on_click,
                on_grab,
                hover,
                label,
                kind,
                ..
            } => Self::Button {
                alpha,
                position,
                size,
                texture,
                on_click,
                on_grab,
                hover,
                entity: Some(new_entity),
                label,
                kind,
            },
            other => other,
        }
    }

    /// Give a `Button` an explicit semantic label (no-op for other component
    /// kinds) - surfaced by `GET /v1/ui` so clients click the button by meaning
    /// (e.g. an elevator floor name) rather than by art name. See
    /// `GuiComponent::Button::label`.
    pub fn with_label(self, new_label: &str) -> GuiComponent<TEvent> {
        match self {
            Self::Button {
                alpha,
                position,
                size,
                texture,
                on_click,
                on_grab,
                hover,
                entity,
                kind,
                ..
            } => Self::Button {
                alpha,
                position,
                size,
                texture,
                on_click,
                on_grab,
                hover,
                entity,
                label: Some(new_label.to_owned()),
                kind,
            },
            other => other,
        }
    }

    /// Re-tag this component's events into another panel's message type, so an
    /// existing panel's components can be embedded inside a composite panel
    /// (the hackable crate reuses the whole loot panel once the crate is
    /// open). Only buttons carry events; everything else is pure art.
    pub fn map<TOut: Clone>(self, f: impl Fn(TEvent) -> TOut) -> GuiComponent<TOut> {
        match self {
            Self::Image {
                position,
                size,
                texture,
                alpha,
                kind,
            } => GuiComponent::Image {
                position,
                size,
                texture,
                alpha,
                kind,
            },
            Self::Text {
                position,
                size,
                font,
                text,
                font_size,
                h,
                v,
                alpha,
                fit_to_rect,
            } => GuiComponent::Text {
                position,
                size,
                font,
                text,
                font_size,
                h,
                v,
                alpha,
                fit_to_rect,
            },
            Self::Bar {
                position,
                size,
                texture,
                fill,
                alpha,
            } => GuiComponent::Bar {
                position,
                size,
                texture,
                fill,
                alpha,
            },
            Self::Button {
                position,
                size,
                texture,
                on_click,
                on_grab,
                hover,
                alpha,
                entity,
                label,
                kind,
            } => GuiComponent::Button {
                position,
                size,
                texture,
                on_click: on_click.map(&f),
                on_grab: on_grab.map(|(left, right)| (f(left), f(right))),
                hover,
                alpha,
                kind,
                entity,
                label,
            },
        }
    }
}

pub fn image<TMsg: Clone>(texture: &str) -> GuiComponent<TMsg> {
    GuiComponent::Image {
        position: vec2(0.0, 0.0),
        size: vec2(30.0, 30.0),
        texture: texture.to_owned(),
        alpha: 0.5,
        kind: ImageKind::Ui,
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
        entity: None,
        label: None,
        kind: ImageKind::Ui,
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
        entity: None,
        label: None,
        kind: ImageKind::Ui,
    }
}

pub fn text<TMsg: Clone>(text: &str) -> GuiComponent<TMsg> {
    GuiComponent::Text {
        position: vec2(0.0, 0.0),
        size: vec2(30.0, 30.0),
        text: text.to_owned(),
        font: "mainfont.fon".to_owned(),
        font_size: 0.0,
        h: HAlign::Left,
        v: VAlign::Middle,
        alpha: 1.0,
        fit_to_rect: false,
    }
}

/// Normalized, event-free presentation data emitted in `Effect::SetUI`.
///
/// This is compiled render input rather than a second authoring language:
/// panels and ordinary canvases both originate as [`UiElement`]. Keeping the
/// normalized snapshot lets the existing effect boundary remain cloneable and
/// keeps UI scripts pure.
#[derive(Clone, Debug)]
pub enum GuiComponentRenderInfo {
    Image {
        position: Vector2<f32>,
        size: Vector2<f32>,
        texture: String,
        alpha: f32,
        /// Whether the source component reacts to clicks (a `Button`, as
        /// opposed to a plain `Image`). Purely informational - used by the
        /// debug UI introspection (`GET /v1/ui`) to distinguish clickable
        /// elements; the VR quad renderer ignores it.
        interactive: bool,
        /// The world entity the source button stands for (a contained item in
        /// a loot panel), if any. Purely informational - used by `GET /v1/ui`
        /// to label the element with the item's name and entity id. How the
        /// art draws is decided by `kind`, never by this.
        entity: Option<EntityId>,
        /// An explicit semantic label from the source `Button` (e.g. an
        /// elevator floor name), if any. Purely informational - used by
        /// `GET /v1/ui`; takes precedence over entity/art-derived labels.
        label: Option<String>,
        /// The source panel's pixel size. `position`/`size` are normalized by
        /// it, so it is what converts an object icon's authored pixel
        /// dimensions back into the same space (see [`Self::is_object_icon`]).
        panel_size_px: Vector2<f32>,
        /// How the art is keyed and sized, carried from the authoring
        /// component (see [`ImageKind`]).
        kind: ImageKind,
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

    /// Whether this is Dark object-icon art: keyed on palette index 0 and
    /// blitted at its authored pixel size, centered in its slot.
    pub(crate) fn is_object_icon(&self) -> bool {
        matches!(
            self,
            Self::Image {
                kind: ImageKind::ObjectIcon,
                ..
            }
        )
    }

    pub fn render(&self, asset_cache: &mut AssetCache) -> SceneObject {
        let scene_object = match self {
            Self::Image {
                position,
                size,
                texture,
                alpha,
                panel_size_px,
                kind,
                ..
            } => {
                let texture = asset_cache
                    .get_ext(
                        &TEXTURE_IMPORTER,
                        texture,
                        &TextureOptions {
                            transparent_index_0: self.is_object_icon(),
                            ..Default::default()
                        },
                    )
                    .clone();
                // Placement is decided in panel pixels by the same helper the
                // 2D presenters use, then renormalized - this panel's own
                // coordinates are normalized by `panel_size_px`.
                //
                // NOTE: the quad below composes through a 180-degree z
                // rotation, which negates both axes. `drawn_rect` breaks an
                // odd pixel of centering slack toward the top-left, so here
                // that lands toward the bottom-right - the two presentations
                // can differ by one pixel on odd slack.
                let (position_px, size_px) = crate::ui::drawn_rect(
                    vec2(position.x * panel_size_px.x, position.y * panel_size_px.y),
                    vec2(size.x * panel_size_px.x, size.y * panel_size_px.y),
                    vec2(texture.width() as f32, texture.height() as f32),
                    *kind,
                );
                let position = &vec2(
                    position_px.x / panel_size_px.x,
                    position_px.y / panel_size_px.y,
                );
                let size = &vec2(size_px.x / panel_size_px.x, size_px.y / panel_size_px.y);
                let texture = texture as Rc<dyn TextureTrait>;
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
            GuiComponent::Text {
                position,
                size,
                font,
                text,
                alpha,
                ..
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
                kind,
            } => GuiComponentRenderInfo::Image {
                position: vec2(position.x / screen_size.x, position.y / screen_size.y),
                size: vec2(size.x / screen_size.x, size.y / screen_size.y),
                texture: texture.clone(),
                alpha: *alpha,
                interactive: false,
                entity: None,
                label: None,
                panel_size_px: screen_size,
                kind: *kind,
            },
            GuiComponent::Bar {
                position,
                size,
                texture,
                alpha,
                ..
            } => GuiComponentRenderInfo::Image {
                position: vec2(position.x / screen_size.x, position.y / screen_size.y),
                size: vec2(size.x / screen_size.x, size.y / screen_size.y),
                texture: texture.clone(),
                alpha: *alpha,
                interactive: false,
                entity: None,
                label: None,
                panel_size_px: screen_size,
                kind: ImageKind::Ui,
            },
            GuiComponent::Button {
                position,
                size,
                texture,
                hover,
                alpha,
                on_click,
                on_grab,
                entity,
                label,
                kind,
            } => {
                let is_hovered = self
                    .rect()
                    .contains(vec2(screen_space_cursor.x, screen_space_cursor.y));

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
                    interactive: on_click.is_some() || on_grab.is_some(),
                    entity: *entity,
                    label: label.clone(),
                    panel_size_px: screen_size,
                    kind: *kind,
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
            GuiComponent::Text { .. } => None,
            GuiComponent::Image { .. } => None,
            GuiComponent::Bar { .. } => None,
            GuiComponent::Button { on_click, .. } => {
                let is_pressed = !last_input.is_pressed && current_input.is_pressed;
                let is_grabbed = current_input.is_grabbed;

                if is_pressed || is_grabbed {
                    if self.rect().contains(vec2(
                        current_input.cursor_position.x,
                        current_input.cursor_position.y,
                    )) {
                        if is_pressed {
                            on_click.clone()
                        } else if is_grabbed {
                            self.grab_event(current_input.hand).cloned()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gui_buttons_are_canvas_elements() {
        let element = button("clicked")
            .with_position(vec2(10.0, 20.0))
            .with_size(vec2(30.0, 40.0));
        let canvas = crate::ui::UiCanvas::from_elements(vec2(100.0, 100.0), vec![element]);

        assert_eq!(canvas.click_at(vec2(25.0, 40.0)), Some("clicked"));
        assert_eq!(canvas.click_at(vec2(5.0, 5.0)), None);
    }

    /// The declared `ImageKind` survives the trip into render info - the
    /// renderers must not have to re-guess it (they used to infer it from
    /// `entity`, which silently disagreed with the flat path for the icon
    /// riding the cursor: a plain image with no entity).
    #[test]
    fn the_declared_object_icon_kind_reaches_render_info() {
        let entity = EntityId::from_inner(1).unwrap();
        let button = grabbable((), ())
            .with_entity(entity)
            .with_image("passkey.pcx")
            .with_object_icon();
        assert!(
            button
                .to_render_info(vec2(640.0, 480.0), Point2::new(0.5, 0.5))
                .is_object_icon()
        );

        // An entity-less plain image is object-icon art too, when it says so.
        let cursor_item = image::<()>("passkey.pcx").with_object_icon();
        assert!(
            cursor_item
                .to_render_info(vec2(640.0, 480.0), Point2::new(0.5, 0.5))
                .is_object_icon()
        );

        // ...and an entity-tagged button is NOT, unless it says so.
        let plain = button_from_entity(entity);
        assert!(
            !plain
                .to_render_info(vec2(640.0, 480.0), Point2::new(0.5, 0.5))
                .is_object_icon()
        );

        let backdrop =
            image::<()>("invback.pcx").to_render_info(vec2(640.0, 480.0), Point2::new(0.5, 0.5));
        assert!(!backdrop.is_object_icon());
    }

    fn button_from_entity(entity: EntityId) -> GuiComponent<()> {
        grabbable((), ()).with_entity(entity).with_image("key0.pcx")
    }
}
