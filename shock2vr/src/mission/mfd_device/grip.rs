//! Live tricorder placement, independent of the ordinary access-card fit.
use crate::ui::WorldPanel;
use cgmath::{Deg, InnerSpace, Quaternion, Rotation, Rotation3, Vector3, vec3};

#[derive(Clone, Copy, Debug)]
pub(super) enum Edge {
    Bottom,
    Left,
    Top,
    Right,
}

pub(super) struct Tuning {
    pub edge: Edge,
    pub offset: Vector3<f32>,
    pub rotation: Quaternion<f32>,
}

pub(super) fn tuning(hand: usize) -> Tuning {
    use crate::dev_params::*;
    let keys = if hand == 0 {
        [
            VR_MFD_LEFT_GRIP_EDGE,
            VR_MFD_LEFT_GRIP_X,
            VR_MFD_LEFT_GRIP_Y,
            VR_MFD_LEFT_GRIP_Z,
            VR_MFD_LEFT_GRIP_PITCH,
            VR_MFD_LEFT_GRIP_YAW,
            VR_MFD_LEFT_GRIP_ROLL,
        ]
    } else {
        [
            VR_MFD_RIGHT_GRIP_EDGE,
            VR_MFD_RIGHT_GRIP_X,
            VR_MFD_RIGHT_GRIP_Y,
            VR_MFD_RIGHT_GRIP_Z,
            VR_MFD_RIGHT_GRIP_PITCH,
            VR_MFD_RIGHT_GRIP_YAW,
            VR_MFD_RIGHT_GRIP_ROLL,
        ]
    };
    let [edge, x, y, z, pitch, yaw, roll] = keys.map(get);
    Tuning {
        edge: match edge.round() as u8 {
            1 => Edge::Left,
            2 => Edge::Top,
            3 => Edge::Right,
            _ => Edge::Bottom,
        },
        offset: vec3(x, y, z) / crate::METERS_PER_WORLD_UNIT,
        rotation: Quaternion::from_angle_z(Deg(roll))
            * Quaternion::from_angle_y(Deg(yaw))
            * Quaternion::from_angle_x(Deg(pitch)),
    }
}

/// Move the selected edge to the authored pinch point. Rotation adjustments
/// pivot around that contact, and translations are in this controller's axes.
pub(super) fn place(
    base: WorldPanel,
    contact: Vector3<f32>,
    hand_rotation: Quaternion<f32>,
    tuning: &Tuning,
    margin: f32,
) -> WorldPanel {
    let local = base
        .rotation
        .conjugate()
        .rotate_vector(contact - base.center);
    let pixel = base.size.x / super::SIZE.x;
    let clearance = (-local.y - base.size.y * 0.5).max(margin);
    let (anchor, degrees) = match tuning.edge {
        Edge::Bottom => (local, 0.0),
        Edge::Left => (vec3(-base.size.x * 0.5 - clearance, 0.0, local.z), 90.0),
        Edge::Top => (
            vec3(-32.0 * pixel, base.size.y * 0.5 + clearance, local.z),
            180.0,
        ),
        Edge::Right => (vec3(70.0 * pixel + clearance, 36.0 * pixel, local.z), -90.0),
    };
    let rotation = (hand_rotation
        * tuning.rotation
        * hand_rotation.conjugate()
        * base.rotation
        * Quaternion::from_angle_z(Deg(degrees)))
    .normalize();
    WorldPanel {
        center: contact + hand_rotation.rotate_vector(tuning.offset)
            - rotation.rotate_vector(anchor),
        rotation,
        size: base.size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::vec2;
    #[test]
    fn every_edge_stays_at_the_same_hand_contact_with_local_adjustments() {
        for mirror in [-1.0, 1.0] {
            let base = WorldPanel {
                center: vec3(1.0, 2.0, 3.0),
                rotation: Quaternion::from_angle_y(Deg(25.0)),
                size: vec2(2.68, 3.76),
            };
            let hand = Quaternion::from_angle_x(Deg(30.0));
            let local = vec3(mirror * 0.6, -2.18, 0.05);
            let contact = base.center + base.rotation.rotate_vector(local);
            for edge in [Edge::Bottom, Edge::Left, Edge::Top, Edge::Right] {
                let tuning = Tuning {
                    edge,
                    offset: vec3(0.01, 0.02, -0.03),
                    rotation: Quaternion::from_angle_y(Deg(15.0)),
                };
                let placed = place(base, contact, hand, &tuning, 0.4);
                let anchor = match edge {
                    Edge::Bottom => local,
                    Edge::Left => vec3(-1.74, 0.0, 0.05),
                    Edge::Top => vec3(-0.32, 2.28, 0.05),
                    Edge::Right => vec3(1.1, 0.36, 0.05),
                };
                let actual = placed.center + placed.rotation.rotate_vector(anchor);
                assert!(
                    (actual - contact - hand.rotate_vector(tuning.offset)).magnitude() < 0.00001,
                    "{edge:?}"
                );
                assert!((placed.rotation.magnitude() - 1.0).abs() < 0.00001);
                assert_eq!(placed.size, base.size);
            }
            let unchanged = place(
                base,
                contact,
                hand,
                &Tuning {
                    edge: Edge::Bottom,
                    offset: vec3(0.0, 0.0, 0.0),
                    rotation: Quaternion::from_angle_x(Deg(0.0)),
                },
                0.4,
            );
            assert!((unchanged.center - base.center).magnitude() < 0.00001);
        }
    }
}
