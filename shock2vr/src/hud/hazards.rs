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
    emit_layout(canvas, origin, state, always, false);
}

// The wrist groups active hazards above the bio bracelet; flat/cyber retain
// their authored positions. This is the single named layout conversion at
// the wrist boundary: both renderers still consume resolved canvas pixels.
fn emit_layout(
    canvas: &mut UiCanvas,
    origin: Vector2<f32>,
    state: &HazardReadout,
    always: bool,
    wrist: bool,
) {
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
    let radiation_visible = !wrist || state.exposed || state.radiation > 0.0;
    let toxin_y = if wrist && !radiation_visible {
        34.0
    } else {
        0.0
    };
    let overflow = state.toxin > 5.0;
    let toxin_width = if overflow {
        128.0
    } else {
        pips.saturating_sub(1) as f32 * 22.0 + 25.0
    };
    let toxin_x = if wrist {
        (128.0 - toxin_width) * 0.5
    } else {
        0.0
    };
    if wrist && pips > 0 {
        // Reuse only the original meter's edge pixels, not its RADIATED label.
        for (dest, source) in [
            (
                at(0.0, toxin_y, 128.0, 1.0),
                Rect::new(32.0, 0.0, 96.0, 1.0),
            ),
            (
                at(0.0, toxin_y + 31.0, 128.0, 1.0),
                Rect::new(32.0, 31.0, 96.0, 1.0),
            ),
            (at(0.0, toxin_y, 1.0, 32.0), Rect::new(32.0, 0.0, 1.0, 32.0)),
            (
                at(127.0, toxin_y, 1.0, 32.0),
                Rect::new(32.0, 0.0, 1.0, 32.0),
            ),
        ] {
            canvas.cropped_image(dest, "radback.pcx", source, vec2(128.0, 32.0));
        }
    }
    for i in 0..pips {
        canvas.image(
            at(toxin_x + i as f32 * 22.0, toxin_y, 25.0, 32.0),
            "poisicon.pcx",
        );
    }
    if overflow {
        canvas.text_native(
            at(toxin_x + 113.0, toxin_y, 15.0, 32.0),
            "+",
            "mainfont.fon",
            HAlign::Center,
            VAlign::Middle,
        );
    }
    if !radiation_visible {
        return;
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
    emit_layout(&mut canvas, vec2(0.0, 0.0), state, false, true);
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UiElement;

    #[test]
    fn wrist_conversion_centers_active_toxins_and_keeps_flat_authored() {
        for (toxin, radiation, x, y) in [
            (1.0, 0.0, 51.5, 34.0),
            (3.0, 0.0, 29.5, 34.0),
            (3.0, 18.0, 29.5, 0.0),
            (7.0, 18.0, 0.0, 0.0),
        ] {
            let state = HazardReadout {
                toxin,
                radiation,
                exposed: false,
            };
            let wrist = wrist_canvas(&state);
            let first_pip = wrist
                .elements()
                .iter()
                .find(|e| {
                    matches!(e,
                        UiElement::Image { texture, .. } if texture == "poisicon.pcx"
                    )
                })
                .unwrap();
            assert_eq!(first_pip.rect(), Rect::new(x, y, 25.0, 32.0));
            assert_eq!(
                wrist
                    .elements()
                    .iter()
                    .any(|e| matches!(e, UiElement::Bar { .. })),
                radiation > 0.0
            );
            let mut flat = UiCanvas::new(SIZE);
            emit(&mut flat, vec2(0.0, 0.0), &state, false);
            assert_eq!(flat.elements()[0].rect(), Rect::new(0.0, 0.0, 25.0, 32.0));
            let flat_pips = toxin.ceil().min(5.0) as usize;
            assert_eq!(
                flat.element_count(),
                flat_pips + usize::from(toxin > 5.0) + 3
            );
            assert_eq!(
                flat.elements()[flat.element_count() - 3].rect(),
                Rect::new(0.0, 34.0, 128.0, 32.0)
            );
        }
        assert_eq!(wrist_canvas(&HazardReadout::default()).element_count(), 0);
        let radiation_only = wrist_canvas(&HazardReadout {
            radiation: 18.0,
            ..Default::default()
        });
        assert_eq!(radiation_only.element_count(), 3);
    }
}
