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
        // The mini mesh occupies the upper well; its live charge stays readable below.
        canvas.fill(Rect::new(2.0, 2.0, 30.0, 30.0), [0, 0, 0]);
        canvas.fill(Rect::new(3.0, 28.0, 28.0, 4.0), [70, 80, 80]);
        canvas.fill(Rect::new(4.0, 29.0, 26.0, 2.0), [12, 18, 18]);
        let charge = (crate::implants::energy(world, entity)
            / crate::implants::recharge_capacity(world))
        .clamp(0.0, 1.0);
        if charge > 0.0 {
            canvas.fill(Rect::new(4.0, 29.0, 26.0 * charge, 2.0), [45, 215, 120]);
        }
    } else if crate::implants::socket_locked(world, slot) {
        canvas.image(rect, "iface/block.pcx");
    }
    canvas
}
