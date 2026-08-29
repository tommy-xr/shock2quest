//! The `show_position` debug readout: the player's world coordinates, drawn on
//! the shared [`UiCanvas`] so flat and VR present the identical layout.
//!
//! It sits on the same 640x480 virtual canvas as the flat HUD, in the gap
//! between the psi-overload meter and the bottom bio/ammo panels - which in VR
//! puts it comfortably below the center of vision.

use cgmath::{Vector3, vec2};

use crate::ui::{HAlign, Rect, UiCanvas, VAlign};

/// The original SS2 HUD canvas, shared with [`super::flat_hud`].
const VIRTUAL_W: f32 = 640.0;
const VIRTUAL_H: f32 = 480.0;

/// Full-width so the readout is centered on the canvas; y clears the
/// bio/ammo panels (which start at 414) and the overload meter above it.
const READOUT: Rect = Rect::new(0.0, 388.0, VIRTUAL_W, 18.0);

/// The readout's text. Two decimals: a world unit is large enough that one
/// hides movement worth reporting.
fn format_position(pos: Vector3<f32>) -> String {
    format!("X: {:.2}   Y: {:.2}   Z: {:.2}", pos.x, pos.y, pos.z)
}

/// Build the position readout as a resolution-independent canvas. Pure (no
/// asset/GL access), so it is unit-testable.
pub(crate) fn build_debug_overlay_canvas(pos: Vector3<f32>) -> UiCanvas {
    let mut canvas = UiCanvas::new(vec2(VIRTUAL_W, VIRTUAL_H));
    canvas.text_native(
        READOUT,
        &format_position(pos),
        "mainfont.fon",
        HAlign::Center,
        VAlign::Middle,
    );
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UiElement;

    fn text_of(canvas: &UiCanvas) -> String {
        match canvas.elements().first().expect("one element") {
            UiElement::Text { text, .. } => text.clone(),
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn the_canvas_is_one_labelled_xyz_readout() {
        let canvas = build_debug_overlay_canvas(Vector3::new(-32.153, 1.0, 21.4));
        assert_eq!(canvas.element_count(), 1);
        assert_eq!(text_of(&canvas), "X: -32.15   Y: 1.00   Z: 21.40");
    }

    #[test]
    fn the_readout_clears_the_hud_panels_and_stays_below_center() {
        // The bio/ammo panels start at y=414, and VR reads the same canvas -
        // so the row must clear them and sit below the canvas center.
        assert!(READOUT.y + READOUT.h <= 414.0);
        assert!(READOUT.center().y > VIRTUAL_H / 2.0);
    }
}
