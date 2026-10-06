//! A single pixel layout for flat, VR HUD, and cyber-interface hazard readouts.
use crate::{
    scripts::radiation::{ActiveRadiation, RadiationRooms},
    ui::{HAlign, Rect, UiCanvas, VAlign},
};
use cgmath::{Vector2, vec2};
use shipyard::{UniqueView, World};

#[cfg(test)]
const SIZE: Vector2<f32> = vec2(128.0, 66.0);
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

pub fn emit(canvas: &mut UiCanvas, origin: Vector2<f32>, state: &HazardReadout) {
    if !state.active() {
        return;
    }
    let at = |x, y, w, h| Rect::new(origin.x + x, origin.y + y, w, h);
    // shkHazrd: toxin at (10,345), radiation at (10,379), icon spacing 22.
    let pips = state.toxin.ceil().clamp(0.0, 5.0) as usize;
    let radiation_visible = state.exposed || state.radiation > 0.0;
    let toxin_y = if radiation_visible { 0.0 } else { 34.0 };
    let overflow = state.toxin > 5.0;
    for i in 0..pips {
        canvas.image(at(i as f32 * 22.0, toxin_y, 25.0, 32.0), "poisicon.pcx");
    }
    if overflow {
        canvas.text_native(
            at(113.0, toxin_y, 15.0, 32.0),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UiElement;

    #[test]
    fn clear_hazards_hide_and_exposure_alone_shows_radiation() {
        let mut canvas = UiCanvas::new(SIZE);
        emit(&mut canvas, SCREEN_ORIGIN, &HazardReadout::default());
        assert_eq!(canvas.element_count(), 0);
        emit(
            &mut canvas,
            SCREEN_ORIGIN,
            &HazardReadout {
                exposed: true,
                ..Default::default()
            },
        );
        assert_eq!(canvas.element_count(), 3);
        assert_eq!(
            canvas.elements()[0].rect(),
            Rect::new(10.0, 379.0, 128.0, 32.0)
        );
        assert!(
            matches!(&canvas.elements()[1], UiElement::Image { texture, .. } if texture == "radicon.pcx")
        );
    }

    #[test]
    fn active_hazards_stack_from_the_bottom_on_the_shared_canvas() {
        for (toxin, radiation, y) in [
            (1.0, 0.0, 34.0),
            (3.0, 0.0, 34.0),
            (3.0, 18.0, 0.0),
            (7.0, 18.0, 0.0),
        ] {
            let state = HazardReadout {
                toxin,
                radiation,
                exposed: false,
            };
            let mut flat = UiCanvas::new(SIZE);
            emit(&mut flat, vec2(0.0, 0.0), &state);
            assert_eq!(flat.elements()[0].rect(), Rect::new(0.0, y, 25.0, 32.0));
            let flat_pips = toxin.ceil().min(5.0) as usize;
            assert_eq!(
                flat.element_count(),
                flat_pips + usize::from(toxin > 5.0) + if radiation > 0.0 { 3 } else { 0 }
            );
            if radiation > 0.0 {
                assert_eq!(
                    flat.elements()[flat.element_count() - 3].rect(),
                    Rect::new(0.0, 34.0, 128.0, 32.0)
                );
            }
        }
    }
}
