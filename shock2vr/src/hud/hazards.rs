//! A single pixel layout for flat, cyber-interface, and wrist hazard readouts.
use crate::{
    scripts::radiation::{ActiveRadiation, RadiationRooms},
    ui::{HAlign, Rect, UiCanvas, VAlign},
};
use cgmath::{Vector2, vec2};
use shipyard::{UniqueView, World};

pub const SIZE: Vector2<f32> = vec2(128.0, 66.0);
pub const SCREEN_ORIGIN: Vector2<f32> = vec2(10.0, 345.0);
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HazardReadout {
    pub radiation: f32,
    pub toxin: f32,
    pub exposed: bool,
}
impl HazardReadout {
    pub fn from_world(world: &World) -> Self {
        world
            .borrow::<UniqueView<ActiveRadiation>>()
            .map(|state| Self {
                radiation: state.level(),
                toxin: state.toxin_level(),
                exposed: state.is_exposed()
                    || world
                        .borrow::<UniqueView<RadiationRooms>>()
                        .is_ok_and(|r| !r.0.is_empty()),
            })
            .unwrap_or_default()
    }
    pub fn active(&self) -> bool {
        self.exposed || self.radiation > 0.0 || self.toxin > 0.0
    }
}

pub fn emit(canvas: &mut UiCanvas, origin: Vector2<f32>, state: &HazardReadout, always: bool) {
    if !always && !state.active() {
        return;
    }
    let at = |x, y, w, h| Rect::new(origin.x + x, origin.y + y, w, h);
    // shkHazrd: toxin at (10,345), radiation at (10,379), icon spacing 22.
    let pips = state.toxin.ceil().clamp(0.0, 5.0) as usize;
    if pips == 0 && always {
        canvas.text_native(
            at(0.0, 0.0, 128.0, 32.0),
            "TOX 0",
            "mainfont.fon",
            HAlign::Left,
            VAlign::Middle,
        );
    }
    for i in 0..pips {
        canvas.image(at(i as f32 * 22.0, 0.0, 25.0, 32.0), "poisicon.pcx");
    }
    if state.toxin > 5.0 {
        canvas.text_native(
            at(113.0, 0.0, 15.0, 32.0),
            "+",
            "mainfont.fon",
            HAlign::Center,
            VAlign::Middle,
        );
    }
    canvas.image(at(0.0, 34.0, 128.0, 32.0), "radback.pcx");
    canvas.image(
        at(0.0, 34.0, 32.0, 32.0),
        if state.exposed {
            "radicon.pcx"
        } else {
            "radgray.pcx"
        },
    );
    canvas.bar(
        at(32.0, 34.0, 96.0, 32.0),
        "radmeter.pcx",
        (state.radiation / 35.0).clamp(0.0, 1.0),
    );
}

pub fn wrist_canvas(state: &HazardReadout) -> UiCanvas {
    let mut canvas = UiCanvas::new(SIZE);
    emit(&mut canvas, vec2(0.0, 0.0), state, false);
    canvas
}
