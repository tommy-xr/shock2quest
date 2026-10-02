//! Presentation boundary for the amp's psionic projection. Layout stays in HRM.
use crate::{
    Handedness,
    input_context::InputContext,
    ui::{Rect, UiCanvas, WorldPanel},
};
use cgmath::{InnerSpace, Matrix3, Quaternion, Rotation, vec2, vec3};
use shipyard::{EntityId, World};

pub(super) fn panel(
    world: &World,
    amp: EntityId,
    hand: Handedness,
    player: &super::PlayerInfo,
    input: &InputContext,
) -> Option<WorldPanel> {
    let i = crate::vr_config::hand_slot(hand);
    let pose = [&input.left_hand, &input.right_hand][i];
    if !input.pose_tracking.is_none_or(|p| p.head && p.hands[i])
        || pose.rotation.magnitude2() < 1e-6
        || input.head.rotation.magnitude2() < 1e-6
    {
        return None;
    }
    let (ball, radius, _) = crate::psi_amp_readout::sphere(world, amp, hand)?;
    let local_ball = player.rotation.conjugate().rotate_vector(ball - player.pos);
    // Lift the full shared board above the ball. A level, viewer-facing basis
    // keeps small HRM labels upright while the amp is rolled in the hand.
    let size = vec2(
        0.55,
        0.55 * super::mfd_device::SIZE.y / super::mfd_device::SIZE.x,
    );
    let center = local_ball + vec3(0., size.y * 0.5 + radius + 0.04, 0.);
    let basis = crate::psi_carousel::projection_frame(center, input.head.position);
    Some(WorldPanel {
        center,
        size,
        rotation: Quaternion::from(Matrix3::from_cols(
            basis.x.truncate(),
            basis.y.truncate(),
            basis.z.truncate(),
        )),
    })
}

pub(super) fn compose(native: UiCanvas, source: Option<Rect>) -> UiCanvas {
    let layout = super::mfd_device::layout(source);
    let mut canvas = UiCanvas::new(layout.size);
    if let Some(window) = layout.screen {
        let r = window.dst;
        canvas
            .fill(
                Rect::new(r.x - 4., r.y - 4., r.w + 8., r.h + 8.),
                [42, 65, 120],
            )
            .opacity(0.35);
        canvas.project(native, vec![window]);
    }
    canvas
}
