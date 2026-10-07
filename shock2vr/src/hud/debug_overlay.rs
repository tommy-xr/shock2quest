//! The `show_position` debug readout: player and hand-ray world coordinates on the
//! shared [`UiCanvas`], so both presentations lay it out once. Flat draws the
//! canvas in screen space over the HUD; VR presents the same canvas on a
//! head-anchored panel of its own.

use cgmath::{InnerSpace, Matrix4, Quaternion, Rotation, Vector2, Vector3, vec2, vec3};
use engine::scene::{SceneObject, laser_material, quad};
use shipyard::EntityId;

use crate::input_context::InputContext;
use crate::mission::flat_ui_host::CANVAS_SIZE;
use crate::physics::{InternalCollisionGroups, PhysicsWorld, RayCastResult};
use crate::ui::{HAlign, PanelPlacement, Rect, UiCanvas, VAlign, WorldPanel};

/// Full-width so the readout is centered on the canvas; the rows sit in the
/// gap between the psi-overload meter and the flat HUD's bottom panels.
const READOUT: Rect = Rect::new(0.0, 352.0, CANVAS_SIZE.x, 54.0);

/// One sample drives both the labels and the surface spots. Debug rays have
/// unlimited reach without changing the game's grab/use range.
#[derive(Default)]
pub(crate) struct DebugPositionTargets {
    hits: [Option<RayCastResult>; 2],
}

impl DebugPositionTargets {
    pub(crate) fn sample(
        input: &InputContext,
        pawn_position: Vector3<f32>,
        pawn_rotation: Quaternion<f32>,
        held: [Option<EntityId>; 2],
        physics: &PhysicsWorld,
    ) -> Self {
        let hands = [&input.left_hand, &input.right_hand];
        Self {
            hits: std::array::from_fn(|i| {
                let hand = hands[i];
                let pose = crate::vr_support::GripPose {
                    position: crate::virtual_hand::hand_world_position(
                        pawn_position,
                        pawn_rotation,
                        hand.position,
                    ),
                    rotation: pawn_rotation * hand.rotation,
                };
                if input.pose_tracking.is_some_and(|p| !p.head || !p.hands[i]) || !pose.is_tracked()
                {
                    return None;
                }
                physics.ray_cast2(
                    crate::util::vec3_to_point3(pose.position),
                    pose.rotation
                        .normalize()
                        .rotate_vector(vec3(0.0, 0.0, -1.0)),
                    f32::MAX,
                    InternalCollisionGroups::WORLD
                        | InternalCollisionGroups::ENTITIES
                        | InternalCollisionGroups::SELECTABLE
                        | InternalCollisionGroups::RAYCAST,
                    held[i],
                    true,
                )
            }),
        }
    }

    pub(crate) fn positions(&self) -> [Option<Vector3<f32>>; 2] {
        self.hits.each_ref().map(|hit| {
            hit.as_ref()
                .map(|hit| crate::util::point3_to_vec3(hit.hit_point))
        })
    }

    pub(crate) fn render(&self) -> Vec<SceneObject> {
        self.hits
            .iter()
            .flatten()
            .filter_map(|hit| {
                if hit.hit_normal.magnitude2() <= 1e-8 {
                    return None;
                }
                let normal = hit.hit_normal.normalize();
                let mut dot = SceneObject::new(
                    laser_material::create(vec3(1.0, 1.0, 1.0)),
                    Box::new(quad::create()),
                );
                // Surface-aligned and depth-tested; the tiny lift avoids z-fighting
                // without changing the exact hit reported by the coordinate label.
                dot.set_transform(
                    Matrix4::from_translation(
                        crate::util::point3_to_vec3(hit.hit_point) + normal * 0.001,
                    ) * Matrix4::from(Quaternion::from_arc(vec3(0.0, 0.0, 1.0), normal, None))
                        * Matrix4::from_nonuniform_scale(0.11, 0.11, 1.0),
                );
                dot.set_depth_write(false);
                dot.set_backface_culling(None);
                Some(dot)
            })
            .collect()
    }
}

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
pub(crate) fn build_debug_overlay_canvas(
    pos: Vector3<f32>,
    targets: [Option<Vector3<f32>>; 2],
) -> UiCanvas {
    let mut canvas = UiCanvas::new(CANVAS_SIZE);
    for (i, (label, position)) in [
        ("Player", Some(pos)),
        ("Left ray", targets[0]),
        ("Right ray", targets[1]),
    ]
    .into_iter()
    .enumerate()
    {
        let value = position
            .map(format_position)
            .unwrap_or_else(|| "no hit".into());
        canvas.text_native(
            Rect::new(READOUT.x, READOUT.y + i as f32 * 18.0, READOUT.w, 18.0),
            &format!("{label}: {value}"),
            "mainfont.fon",
            HAlign::Center,
            VAlign::Middle,
        );
    }
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

    fn text_of(canvas: &UiCanvas) -> Vec<&str> {
        canvas
            .elements()
            .iter()
            .map(|element| match element {
                UiElement::Text { text, .. } => text.as_str(),
                other => panic!("expected text, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn the_canvas_labels_player_and_each_hand() {
        let canvas = build_debug_overlay_canvas(
            vec3(-32.153, 1.0, 21.4),
            [Some(vec3(8.126, -2.3456, 0.0)), None],
        );
        assert_eq!(canvas.element_count(), 3);
        assert_eq!(
            text_of(&canvas),
            [
                "Player: X: -32.15   Y: 1.00   Z: 21.40",
                "Left ray: X: 8.13   Y: -2.35   Z: 0.00",
                "Right ray: no hit",
            ]
        );
    }

    #[test]
    fn long_rays_ignore_held_items_and_clear_when_untracked_or_missing() {
        use crate::input_context::PoseTracking;
        use crate::physics::CollisionGroup;
        use cgmath::{One, Zero};

        let mut world = shipyard::World::new();
        let mut physics = PhysicsWorld::new();
        let wall = world.add_entity(());
        let held = world.add_entity(());
        for (entity, position, size) in [
            (wall, vec3(0.0, 2.0, -5.0), vec3(0.2, 20.0, 20.0)),
            (held, vec3(8.0, 2.0, -5.0), vec3(0.2, 20.0, 20.0)),
        ] {
            physics.add_kinematic(
                entity,
                position,
                Quaternion::one(),
                Vector3::zero(),
                size,
                CollisionGroup::selectable(),
                false,
            );
        }
        let player = world.add_entity(());
        let mut player_handle = physics.create_player(vec3(100.0, 100.0, 100.0), player);
        physics.update(Vector3::zero(), &mut player_handle);

        let mut input = InputContext::default();
        input.left_hand.position = vec3(-0.2, 0.0, 0.0);
        input.right_hand.position = vec3(0.2, 0.5, 0.0);
        input.left_hand.rotation = Quaternion::one();
        input.right_hand.rotation = Quaternion::one();
        let sample = |input: &InputContext| {
            DebugPositionTargets::sample(
                input,
                vec3(10.0, 2.0, -5.0),
                Quaternion::from_angle_y(cgmath::Deg(90.0)),
                [Some(held); 2],
                &physics,
            )
        };
        let targets = sample(&input);
        for (actual, expected) in targets
            .positions()
            .into_iter()
            .zip([vec3(0.1, 2.0, -4.8), vec3(0.1, 2.5, -5.2)])
        {
            assert!((actual.unwrap() - expected).magnitude() < 1e-4);
        }
        let dots = targets.render();
        assert_eq!(dots.len(), 2);
        for (dot, point) in dots.iter().zip(targets.positions()) {
            assert!((dot.get_transform().w.truncate() - point.unwrap()).magnitude() < 0.002);
        }

        input.pose_tracking = Some(PoseTracking {
            head: true,
            hands: [false, true],
        });
        assert_eq!(sample(&input).positions()[0], None);
        input.right_hand.rotation = Quaternion::from_angle_y(cgmath::Deg(180.0));
        assert_eq!(sample(&input).positions(), [None; 2]);
        assert!(sample(&input).render().is_empty());
        input.pose_tracking = None;
        input.left_hand.rotation = Quaternion::zero();
        assert_eq!(sample(&input).positions(), [None; 2]);
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
