//! The VR comfort dim: a darkening layer between the player and the world
//! while a head-anchored overlay panel is up.
//!
//! Extracted from the pause menu so any overlay presented on a
//! [`FrontendPanelAnchor`](crate::ui::FrontendPanelAnchor)-placed panel (the
//! pause menu, the cyber-interface use mode) dims the world the same way
//! rather than growing its own copy. Flat presentation needs none of this -
//! an opaque backdrop (pause) or the undimmed original overlay (use mode)
//! already answers "what happens to the world behind the UI" there. In VR the
//! panel covers about 50 degrees of a 100-degree field, and the vivid world
//! around it - at a depth the eyes are no longer converging on - is what
//! on-headset testing reported as nauseating (issue #1018). A darkening layer
//! around the player is the standard answer: it says "this space is UI now"
//! without hiding where they are standing.

use cgmath::{InnerSpace, Matrix4, Quaternion, Rotation, Vector3, vec3};
use engine::scene::{RenderLayer, SceneObject, color_material, quad};

use crate::ui::{WorldPanel, frontend_panel_distance};

/// How dark the world goes behind the panel: 0 leaves it untouched, 1 blacks it
/// out completely. **This is the tuning knob** - the value wants a worn check,
/// because how heavy a dim reads in a headset is not something a screenshot
/// settles.
///
/// Live-tunable ([`crate::dev_params::WORLD_DIM_STRENGTH`]); read per frame.
pub fn world_dim_strength() -> f32 {
    crate::dev_params::get(crate::dev_params::WORLD_DIM_STRENGTH)
}

/// What the world is dimmed *toward*. Black rather than a tint: any hue here
/// would grade the whole scene and fight the panel art.
const WORLD_DIM_COLOR: Vector3<f32> = Vector3 {
    x: 0.0,
    y: 0.0,
    z: 0.0,
};

/// How far in front of the eyes the dim hangs, in metres, when the player is
/// standing where they opened the overlay. Behind the panel (which is at
/// [`frontend_panel_distance`]) so the panel's own content occludes it and the
/// canvas stays at full brightness. Room-scale movement can push the panel
/// farther away than this, which is what [`dim_distance`] is for.
pub fn world_dim_min_distance() -> f32 {
    frontend_panel_distance() + 1.0
}

/// Clear air kept between the panel's farthest corner and the dim, in metres.
const WORLD_DIM_PANEL_CLEARANCE: f32 = 1.0;

/// Half-edge of the dim quad, as a multiple of its distance. This is
/// the whole coverage argument: a viewer-locked quad at ratio `r` covers
/// `atan(r)` off-axis in every direction, *independently of where the player is
/// looking and of how wide the headset's field of view is*. 5 covers +/-78.7
/// degrees, against a Quest's ~55 degree half-field - and against the ~0.6
/// degrees of parallax the eyes' 32 mm offset adds at this distance, which is
/// why one viewer-locked quad is safe for both eyes. Because it is a *ratio*,
/// coverage is unchanged when [`dim_distance`] pushes the quad farther out.
pub const WORLD_DIM_EXTENT_RATIO: f32 = 5.0;

/// "No head pose yet": the ZERO quaternion a runtime reports for an untracked
/// head, which [`dim_pose`] resolves against the panel instead.
pub const UNTRACKED_HEAD: (Vector3<f32>, Quaternion<f32>) = (
    Vector3::new(0.0, 0.0, 0.0),
    Quaternion::new(0.0, 0.0, 0.0, 0.0),
);

/// How far out to hang the dim, given where the viewer actually is.
///
/// The panel is world-locked and the player is not: in a room-scale space they
/// can physically step back a metre or more before the anchor's lazy recenter
/// (a sustained deviation, held ~1 s) brings the panel with them. A dim at a
/// fixed distance would then be in *front* of the panel and grey out the
/// canvas. So it is pushed past the panel's farthest corner, with clearance -
/// never nearer than [`world_dim_min_distance`], and never at the panel's own
/// depth.
pub fn dim_distance(head_position: Vector3<f32>, panel: &WorldPanel) -> f32 {
    let right = panel.rotation.rotate_vector(vec3(1.0, 0.0, 0.0)) * panel.size.x * 0.5;
    let up = panel.rotation.rotate_vector(vec3(0.0, 1.0, 0.0)) * panel.size.y * 0.5;
    let farthest_corner = [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)]
        .into_iter()
        .map(|(x, y)| (panel.center + right * x + up * y - head_position).magnitude())
        .fold(0.0_f32, f32::max);
    (farthest_corner + WORLD_DIM_PANEL_CLEARANCE).max(world_dim_min_distance())
}

/// The darkening layer between the player and the world.
///
/// **Locked to the live head pose**, not to the panel: the panel is
/// world-locked and the head is not, so a dim placed once at open time stops
/// covering the view the moment the player looks off-axis. Following the head
/// makes coverage a property of the geometry rather than of where they happen
/// to be looking.
///
/// It is one flat quad and it **does not rely on backface culling**. The
/// previous attempt was a box drawn from inside, which needs the host to agree
/// about which winding faces the viewer; on the Quest it does not, and half the
/// box was culled - the "only parts of the screen were covered" report on
/// #1020. A single double-sided quad rasterizes exactly once per pixel on any
/// host, so it can neither vanish nor double-blend.
///
/// It must be emitted **first inside the owning overlay's clear-depth group**
/// (so it carries the group's depth clear) rather than from `render_per_eye`,
/// which would be the natural home for a view-locked layer: the two VR hosts
/// disagree about where per-eye objects land in the scene list - the debug
/// runtime appends them, `oculus_runtime` prepends them - so from there the
/// dim would either be drawn before the panel (dimming the canvas) or be
/// depth-tested against a world that has not been cleared yet (dimming only
/// the parts of the view with nothing close in front of them). Inside the
/// group, order is the caller's.
///
/// `head_forward` is the *true* gaze direction (not flattened to horizontal),
/// so looking at the floor or the ceiling is covered like any other direction.
/// `source` is the debug provenance tag (a `crate::util::render_source` name).
pub fn world_dim_layer(
    head_position: Vector3<f32>,
    head_forward: Vector3<f32>,
    distance: f32,
    source: &str,
) -> SceneObject {
    world_dim_layer_scaled(head_position, head_forward, distance, 1.0, source)
}

/// Same as [`world_dim_layer`], but [`world_dim_strength`] is scaled by
/// `ramp` (0..1, clamped) first - lets a caller with an entry/exit ramp (the
/// cyber interface's [`crate::ui::entry_ramp`]) fade the dim in and out
/// instead of snapping to full strength the instant the overlay opens. The
/// pause menu has no such ramp and always gets full strength via
/// [`world_dim_layer`].
pub fn world_dim_layer_scaled(
    head_position: Vector3<f32>,
    head_forward: Vector3<f32>,
    distance: f32,
    ramp: f32,
    source: &str,
) -> SceneObject {
    let extent = distance * WORLD_DIM_EXTENT_RATIO;
    let mut object = SceneObject::new(
        color_material::create(WORLD_DIM_COLOR),
        Box::new(quad::create()),
    );
    object.set_transform(
        Matrix4::from_translation(head_position + head_forward * distance)
            // The quad's own +Z faces the viewer when it is turned to look back
            // along the gaze, exactly as the frontend panel is oriented.
            * Matrix4::from(crate::util::get_rotation_from_forward_vector(-head_forward))
            * Matrix4::from_scale(extent * 2.0),
    );
    // The material speaks in transparency (0 = opaque), the strength in "how
    // dark does the world go" - the direction a human tunes in.
    object.set_transparency(Some(1.0 - world_dim_strength() * ramp.clamp(0.0, 1.0)));
    // Translucent, and drawn before the panel: writing depth here would let the
    // dim occlude the canvas that draws over it.
    object.set_depth_write(false);
    // A modal panel the player cannot read is not an overlay: world-locked
    // two metres ahead, it lands inside a wall or a console often enough that
    // depth-testing it against the world is not an option. The renderer clears
    // depth once at this explicit system-layer boundary.
    object.set_render_layer(RenderLayer::SystemOverlay);
    object.set_debug_tag(Some(crate::util::render_source_tag(source)));
    object
}

/// Where to hang the dim: the viewer's position and gaze.
///
/// An untracked head arrives as the ZERO quaternion, which `rotate_vector`
/// silently returns unrotated (and comes with a meaningless position) - so it is
/// treated as "no pose", and the panel stands in for it. The panel hangs
/// [`frontend_panel_distance`] along its own normal from the head that placed
/// it, with the normal pointing back at that head, so it carries a usable
/// viewer pose: where the player was when they opened the overlay.
pub fn dim_pose(
    head_position: Vector3<f32>,
    head_rotation: Quaternion<f32>,
    panel: &WorldPanel,
) -> (Vector3<f32>, Vector3<f32>) {
    crate::util::tracked_gaze(head_position, head_rotation).unwrap_or((
        panel.center + panel.normal() * frontend_panel_distance(),
        -panel.normal(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Negative test: at `ramp = 1.0` the scaled layer must be identical to
    /// the unscaled one the pause menu still uses - only a `ramp < 1.0`
    /// caller (the cyber interface, mid-fade) should ever see it differ.
    #[test]
    fn full_ramp_matches_the_unscaled_layer() {
        let head = vec3(0.0, 1.7, 0.0);
        let forward = vec3(0.0, 0.0, -1.0);
        let full = world_dim_layer(head, forward, 3.0, "test");
        let scaled = world_dim_layer_scaled(head, forward, 3.0, 1.0, "test");
        assert_eq!(
            full.effective_transparency(),
            scaled.effective_transparency()
        );
    }

    /// A ramp partway open must dim the world less than full strength, and a
    /// ramp of zero must not dim it at all (fully transparent).
    #[test]
    fn a_partial_ramp_dims_less_than_full_strength() {
        let head = vec3(0.0, 1.7, 0.0);
        let forward = vec3(0.0, 0.0, -1.0);
        let full = world_dim_layer_scaled(head, forward, 3.0, 1.0, "test")
            .effective_transparency()
            .unwrap();
        let half = world_dim_layer_scaled(head, forward, 3.0, 0.5, "test")
            .effective_transparency()
            .unwrap();
        let none = world_dim_layer_scaled(head, forward, 3.0, 0.0, "test")
            .effective_transparency()
            .unwrap();
        // Transparency is `1 - dim`, so a weaker dim reads as MORE transparent.
        assert!(none > half && half > full);
        assert_eq!(none, 1.0, "a zero ramp must leave the world untouched");
    }

    /// An out-of-range ramp must clamp rather than invert the dim or panic.
    #[test]
    fn ramp_is_clamped_to_0_1() {
        let head = vec3(0.0, 1.7, 0.0);
        let forward = vec3(0.0, 0.0, -1.0);
        let over = world_dim_layer_scaled(head, forward, 3.0, 5.0, "test")
            .effective_transparency()
            .unwrap();
        let full = world_dim_layer_scaled(head, forward, 3.0, 1.0, "test")
            .effective_transparency()
            .unwrap();
        assert_eq!(over, full);

        let under = world_dim_layer_scaled(head, forward, 3.0, -5.0, "test")
            .effective_transparency()
            .unwrap();
        let none = world_dim_layer_scaled(head, forward, 3.0, 0.0, "test")
            .effective_transparency()
            .unwrap();
        assert_eq!(under, none);
    }
}
