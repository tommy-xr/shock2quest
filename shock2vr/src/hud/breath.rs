//! One oxygen layout, shared by the screen, cyber interface and wrist.
use crate::{
    swimming::WaterStatus,
    ui::{HAlign, Rect, UiCanvas, VAlign},
};
use cgmath::{Vector2, vec2};

pub const SIZE: Vector2<f32> = vec2(128.0, 28.0);
pub const SCREEN_ORIGIN: Vector2<f32> = vec2(10.0, 310.0);

pub fn emit(canvas: &mut UiCanvas, origin: Vector2<f32>, water: WaterStatus) {
    if !water.visible() {
        return;
    }
    let at = |x, y, w, h| Rect::new(origin.x + x, origin.y + y, w, h);
    canvas.fill(at(0.0, 0.0, SIZE.x, SIZE.y), [9, 24, 29]);
    let label = if water.remaining_seconds <= 0.0 {
        "NO AIR".to_owned()
    } else {
        format!("O2  {}s", water.remaining_seconds.ceil() as u32)
    };
    canvas.text_native(
        at(4.0, 1.0, 120.0, 16.0),
        &label,
        "mainfont.fon",
        HAlign::Center,
        VAlign::Middle,
    );
    canvas.fill(at(4.0, 20.0, 120.0, 4.0), [30, 52, 57]);
    let fraction =
        (water.remaining_seconds / water.maximum_seconds.max(1.0)).clamp(0.0, 1.0) as f32;
    let color = if water.remaining_seconds <= 30.0 {
        [255, 156, 42]
    } else {
        [112, 213, 230]
    };
    canvas.fill(at(4.0, 20.0, 120.0 * fraction, 4.0), color);
}

pub fn wrist_canvas(water: WaterStatus) -> UiCanvas {
    let mut canvas = UiCanvas::new(SIZE);
    emit(&mut canvas, vec2(0.0, 0.0), water);
    canvas
}
