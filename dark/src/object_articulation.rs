//! LGMD scalar parameters are shared by any number of sub-objects, not bone IDs.
//! See Dark's `md_start_subobj` in tech/libsrc/md/render.c: enter the authored
//! axle frame, then rotate or slide along its X axis. Joint positions use
//! degrees for rotation and Dark feet for translation (not normalized ranges).
use std::{collections::HashMap, ops::Range};

use cgmath::{Deg, Matrix4, Point3, Transform, vec3};

use crate::{SCALE_FACTOR, ss2_bin_obj_loader::Vhot, ss2_skeleton::Skeleton};

#[derive(Clone, Debug)]
pub struct ObjectJoint {
    pub index: u32,
    pub parameter: i32,
    pub motion_type: u8,
    pub vhot_range: Range<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ss2_skeleton::Bone;
    use cgmath::{InnerSpace, SquareMatrix, point3};

    fn rig(remastered: bool) -> ObjectArticulation {
        let (cap, gun) = if remastered { (1, 2) } else { (2, 1) };
        ObjectArticulation {
            skeleton: Skeleton::create_from_bones(vec![
                Bone {
                    joint_id: 0,
                    parent_id: None,
                    local_transform: Matrix4::identity(),
                },
                // Authored axle turns the converted -X slider axis upward.
                Bone {
                    joint_id: cap,
                    parent_id: Some(0),
                    local_transform: Matrix4::from_angle_z(Deg(-90.0)),
                },
                Bone {
                    joint_id: gun,
                    parent_id: Some(cap),
                    local_transform: Matrix4::from_translation(vec3(0.0, 0.0, 1.0)),
                },
            ]),
            joints: vec![
                ObjectJoint {
                    index: 0,
                    parameter: -1,
                    motion_type: 0,
                    vhot_range: 0..0,
                },
                ObjectJoint {
                    index: cap,
                    parameter: 0,
                    motion_type: 2,
                    vhot_range: 0..0,
                },
                ObjectJoint {
                    index: gun,
                    parameter: 1,
                    motion_type: 1,
                    vhot_range: 0..1,
                },
            ],
            vhots: vec![Vhot {
                id: 7,
                point: point3(0.0, 1.0, 0.0),
            }],
        }
    }

    #[test]
    fn reordered_parts_keep_the_same_pose_and_animated_attachment() {
        for angle in [0.0, 45.0, -90.0, 180.0] {
            let parameters = [(0, 2.0), (1, angle)];
            let before = rig(false).vhot_position(7, &parameters).unwrap();
            let after = rig(true).vhot_position(7, &parameters).unwrap();
            assert!((before - after).magnitude() < 1e-5);
        }
        let model = rig(true);
        let closed = model.vhot_position(7, &[(0, 0.0), (1, 0.0)]).unwrap();
        let open = model.vhot_position(7, &[(0, 2.0), (1, 0.0)]).unwrap();
        assert!((open - closed - vec3(0.0, 2.0 / SCALE_FACTOR, 0.0)).magnitude() < 1e-5);
        let aimed = model.vhot_position(7, &[(0, 2.0), (1, 90.0)]).unwrap();
        assert!((aimed - point3(0.0, 2.0 / SCALE_FACTOR, 0.0)).magnitude() < 1e-5);
        assert!(
            model.vhot_position(0, &[]).is_none(),
            "IDs are not array indices"
        );
    }

    #[test]
    fn one_parameter_can_drive_multiple_parts_and_unknown_parameters_are_ignored() {
        let mut model = rig(true);
        model.joints.push(ObjectJoint {
            index: 3,
            parameter: 0,
            motion_type: 2,
            vhot_range: 0..0,
        });
        let transforms = model.joint_transforms(&[(0, 2.0), (55, 99.0)]);
        assert_eq!(transforms.len(), 2);
        assert_eq!(transforms[&1], transforms[&3]);
        assert!(model.joint_transforms(&[(0, f32::NAN)]).is_empty());
    }
}

#[derive(Clone, Debug)]
pub struct ObjectArticulation {
    pub skeleton: Skeleton,
    pub joints: Vec<ObjectJoint>,
    pub vhots: Vec<Vhot>,
}

impl ObjectArticulation {
    pub fn joint_transforms(&self, parameters: &[(i32, f32)]) -> HashMap<u32, Matrix4<f32>> {
        self.joints
            .iter()
            .filter_map(|joint| {
                let value = parameters
                    .iter()
                    .rev()
                    .find(|(id, _)| *id == joint.parameter)?
                    .1;
                if !value.is_finite() {
                    return None;
                }
                // read_vec3 maps Dark (x,y,z) to (-x,z,y). The same basis
                // conversion makes the authored axle -X in renderer space.
                let transform = match joint.motion_type {
                    1 => Matrix4::from_angle_x(Deg(-value)),
                    2 => Matrix4::from_translation(vec3(-value / SCALE_FACTOR, 0.0, 0.0)),
                    _ => return None,
                };
                Some((joint.index, transform))
            })
            .collect()
    }

    pub fn pose(&self, parameters: &[(i32, f32)]) -> [Matrix4<f32>; 40] {
        Skeleton::set_joint_transforms(&self.skeleton, &self.joint_transforms(parameters))
            .get_transforms()
    }

    /// Attachment ownership is a range in the file's vhot table; the attachment
    /// ID itself may be sparse and is never a bone index.
    pub fn vhot_position(&self, id: u32, parameters: &[(i32, f32)]) -> Option<Point3<f32>> {
        let (index, vhot) = self
            .vhots
            .iter()
            .enumerate()
            .find(|(_, vhot)| vhot.id == id)?;
        let owner = self
            .joints
            .iter()
            .find(|joint| joint.vhot_range.contains(&index))?;
        self.pose(parameters)
            .get(owner.index as usize)
            .map(|pose| pose.transform_point(vhot.point))
    }
}
