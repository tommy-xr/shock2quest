//! The `show_position` debug readout: the player's world coordinates on the
//! shared [`UiCanvas`], so both presentations lay it out once. Flat draws the
//! canvas in screen space over the HUD; VR presents the same canvas on a
//! head-anchored panel of its own.

use cgmath::{Vector2, Vector3, vec2};

use crate::mission::flat_ui_host::CANVAS_SIZE;
use crate::ui::{HAlign, PanelPlacement, Rect, UiCanvas, VAlign, WorldPanel};

/// Full-width so the readout is centered on the canvas; the row sits in the
/// gap between the psi-overload meter and the flat HUD's bottom panels.
const READOUT: Rect = Rect::new(0.0, 388.0, CANVAS_SIZE.x, 18.0);

/// Give the readout its own compact mounting dimensions, independent of the
/// tunable frontend panel distance. System UI hides this readout while open.
const PANEL_DISTANCE: f32 = 1.2;
const PANEL_SIZE: Vector2<f32> = vec2(1.2, 0.9);

/// The readout's text. Two decimals: a world unit is large enough that one
/// hides movement worth reporting.
fn format_position(pos: Vector3<f32>) -> String {
    format!("X: {:.2}   Y: {:.2}   Z: {:.2}", pos.x, pos.y, pos.z)
}

/// Build the position readout as a resolution-independent canvas. Pure (no
/// asset/GL access), so it is unit-testable.
pub(crate) fn build_debug_overlay_canvas(pos: Vector3<f32>) -> UiCanvas {
    let mut canvas = UiCanvas::new(CANVAS_SIZE);
    canvas.text_native(
        READOUT,
        &format_position(pos),
        "mainfont.fon",
        HAlign::Center,
        VAlign::Middle,
    );
    canvas
}

/// The panel the readout hangs on, for a placement from the anchor. Same yaw
/// and gravity alignment as any frontend panel, at this readout's own distance.
pub(crate) fn readout_panel(placement: PanelPlacement) -> WorldPanel {
    WorldPanel {
        center: placement.head_position + placement.forward * PANEL_DISTANCE,
        rotation: crate::util::get_rotation_from_forward_vector(-placement.forward),
        size: PANEL_SIZE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::UiElement;
    use cgmath::{InnerSpace, Quaternion, Rotation3, vec3};

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
        // The flat bio/ammo panels start at `BIO_ORIGIN.y`, and VR reads the same
        // canvas - so the row must clear them and sit below the canvas center.
        assert!(READOUT.y + READOUT.h <= crate::hud::readouts::BIO_ORIGIN.y);
        assert!(READOUT.center().y > CANVAS_SIZE.y / 2.0);
    }

    #[test]
    fn the_panel_uses_its_own_distance_and_faces_the_head() {
        let head = vec3(1.0, 1.7, -2.0);
        let placement =
            PanelPlacement::from_head(head, Quaternion::from_angle_y(cgmath::Deg(37.0)));
        let panel = readout_panel(placement);
        let distance = (panel.center - head).magnitude();
        assert!((distance - PANEL_DISTANCE).abs() < 1e-4);
        // Facing the head, and gravity-aligned (the placement's forward is).
        let to_head = (head - panel.center).normalize();
        assert!((panel.normal() - to_head).magnitude() < 1e-4);
        assert!((panel.center.y - head.y).abs() < 1e-4);
    }
}
