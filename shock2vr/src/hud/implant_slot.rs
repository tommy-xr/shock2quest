//! Native inventory implant well, reused on the wristband.
use crate::ui::{Rect, UiCanvas};
use cgmath::{Vector2, vec2};
use shipyard::World;

pub(crate) const INVENTORY_SIZE: Vector2<f32> = Vector2::new(636.0, 121.0);
pub(crate) fn inventory_well(slot: usize) -> Rect {
    Rect::new(563.0 + slot as f32 * 36.0, 84.0, 34.0, 34.0)
}

pub(crate) fn canvas(world: &World, slot: usize) -> UiCanvas {
    let mut canvas = UiCanvas::new(vec2(34.0, 34.0));
    let rect = Rect::new(0.0, 0.0, 34.0, 34.0);
    // Normalized source coordinates select the same well in legacy and 2x remaster art.
    canvas.cropped_image(rect, "invback.pcx", inventory_well(slot), INVENTORY_SIZE);
    if let Some(entity) = crate::implants::equipped(world)[slot] {
        if let Some(icon) = crate::scripts::gui::inventory_icon(world, entity) {
            canvas.fitted_object_icon(Rect::new(2.0, 2.0, 30.0, 30.0), &format!("{icon}.pcx"));
        }
    } else if slot >= crate::implants::capacity(world) {
        canvas.image(rect, "iface/block.pcx");
    }
    canvas
}
