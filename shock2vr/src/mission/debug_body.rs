//! Spectator-only hacker body. No collision, inventory ownership, or save state.
use cgmath::{Deg, InnerSpace, Matrix4, Quaternion, SquareMatrix, Vector3, vec3};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};

#[derive(Default)]
pub(super) struct DebugBody;

impl DebugBody {
    pub fn render(
        &self,
        assets: &mut AssetCache,
        belt_center: Vector3<f32>,
        heading: Matrix4<f32>,
        wrists: [Option<Matrix4<f32>>; 2],
    ) -> Vec<SceneObject> {
        let Some(asset) = assets.get_opt(&dark::importers::VR_BODY_IMPORTER, "player.bin") else {
            return Vec::new();
        };
        let Some(model) = &asset.0 else {
            return Vec::new();
        };
        let Some(skeleton) = model.skeleton() else {
            return Vec::new();
        };
        let rest = skeleton.get_transforms();
        // CAL's standing rig faces +Z. The belt faces -Z. Fit the abdomen to
        // the belt's existing centre; both use the same smoothed body heading.
        // 0.85 fits this authored avatar to the current ~1.7 m standing setup.
        let root = Matrix4::from_translation(belt_center)
            * heading
            * Matrix4::from_angle_y(Deg(180.0))
            * Matrix4::from_scale(0.85)
            * Matrix4::from_translation(-rest[18].w.truncate());
        let inverse = root.invert().expect("nonzero fixed body scale");
        let mut posed = rest;
        for (side, joints) in [[10, 12, 14], [11, 13, 15]].into_iter().enumerate() {
            let [shoulder, elbow, wrist] = joints;
            let start = rest[shoulder].w.truncate();
            let middle = rest[elbow].w.truncate();
            let end = rest[wrist].w.truncate();
            let side_sign = if side == 0 { 1.0 } else { -1.0 };
            let target = wrists[side]
                .map(|w| (inverse * w).w.truncate())
                // Tracking loss relaxes the arm; never chase stale controllers.
                .unwrap_or(start + vec3(side_sign * 0.08, -0.80, 0.10));
            let (bend, reached) = solve_arm(
                start,
                target,
                (middle - start).magnitude(),
                (end - middle).magnitude(),
                vec3(side_sign * 0.65, -1.0, -0.25),
            );
            posed[shoulder] = segment_pose(rest[shoulder], middle - start, bend - start, start);
            posed[elbow] = segment_pose(rest[elbow], end - middle, reached - bend, bend);
            posed[wrist] = posed[elbow];
            posed[wrist].w = reached.extend(1.0);
        }
        let mut objects = model.to_posed_scene_objects(&posed);
        for object in &mut objects {
            object.set_transform(root * object.get_transform());
        }
        crate::util::tag_render_source(&mut objects, "vr_debug_body");
        if let Some(backpack) =
            assets.get_opt(&dark::importers::GLB_MODELS_IMPORTER, "backpack.glb")
        {
            // The asset is in metres, with its contact surface at Z=0 and its
            // front facing -Z. Face it away from the torso, just above the belt.
            let mount = Matrix4::from_translation(belt_center)
                * heading
                * Matrix4::from_translation(vec3(0.0, 0.23, 0.02) / crate::METERS_PER_WORLD_UNIT)
                * Matrix4::from_angle_y(Deg(180.0))
                * Matrix4::from_scale(1.0 / crate::METERS_PER_WORLD_UNIT);
            let mut pack = backpack.clone_scene_objects();
            for object in &mut pack {
                object.set_transform(mount * object.get_transform());
            }
            crate::util::tag_render_source(&mut pack, "vr_debug_backpack");
            objects.extend(pack);
        }
        objects
    }
}

fn segment_pose(
    rest: Matrix4<f32>,
    from: Vector3<f32>,
    to: Vector3<f32>,
    position: Vector3<f32>,
) -> Matrix4<f32> {
    let rotation = Quaternion::from_arc(from.normalize(), to.normalize(), None);
    let mut frame = Matrix4::from(rotation) * rest;
    frame.w = position.extend(1.0);
    frame
}

/// Fixed-length two-bone IK with a body-relative elbow pole. Clamp unreachable
/// targets instead of stretching the mesh; retain a stable bend at full reach.
fn solve_arm(
    start: Vector3<f32>,
    target: Vector3<f32>,
    upper: f32,
    lower: f32,
    pole: Vector3<f32>,
) -> (Vector3<f32>, Vector3<f32>) {
    let delta = target - start;
    let direction = if delta.magnitude2() > 1e-8 {
        delta.normalize()
    } else {
        -Vector3::unit_y()
    };
    let distance = delta
        .magnitude()
        .clamp((upper - lower).abs() + 1e-4, upper + lower - 1e-4);
    let along = (upper * upper - lower * lower + distance * distance) / (2.0 * distance);
    let height = (upper * upper - along * along).max(0.0).sqrt();
    let mut bend = pole - direction * pole.dot(direction);
    if bend.magnitude2() < 1e-6 {
        let fallback = if direction.x.abs() < 0.9 {
            Vector3::unit_x()
        } else {
            Vector3::unit_z()
        };
        bend = fallback - direction * fallback.dot(direction);
    }
    (
        start + direction * along + bend.normalize() * height,
        start + direction * distance,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arms_reach_targets_without_changing_bone_lengths() {
        let shoulder = vec3(-0.2, 1.4, 0.0);
        for target in [vec3(-0.4, 1.2, -0.3), vec3(0.1, 1.5, -0.2)] {
            let (elbow, wrist) = solve_arm(shoulder, target, 0.32, 0.27, vec3(-1.0, -1.0, 0.0));
            assert!((wrist - target).magnitude() < 1e-5);
            assert!(((elbow - shoulder).magnitude() - 0.32).abs() < 1e-5);
            assert!(((wrist - elbow).magnitude() - 0.27).abs() < 1e-5);
        }
    }

    #[test]
    fn unreachable_folded_and_pole_aligned_targets_stay_finite() {
        for target in [
            vec3(0.0, 0.0, 0.0),
            vec3(0.0, -4.0, 0.0),
            vec3(0.0, -0.2, 0.0),
        ] {
            let (elbow, wrist) =
                solve_arm(vec3(0.0, 0.0, 0.0), target, 0.32, 0.27, -Vector3::unit_y());
            assert!(((elbow).magnitude() - 0.32).abs() < 1e-5);
            assert!(((wrist - elbow).magnitude() - 0.27).abs() < 1e-5);
            assert!(wrist.magnitude() <= 0.59);
        }
    }
}
