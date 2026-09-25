//! Shared physical reference for the in-game tricorder and ss2ex grip authoring.
use cgmath::{EuclideanSpace, Matrix4, Point3, Vector3, point3, vec3};
use engine::scene::{SceneObject, SceneObjectDebugTag, color_material, cube};

pub const WIDTH_M: f32 = 0.14;
pub const CANVAS: [f32; 2] = [268.0, 376.0];
pub const DEPTH_M: f32 = 0.008;
pub fn is_model(key: &str) -> bool {
    key.eq_ignore_ascii_case("tricorder") || key.eq_ignore_ascii_case("tricorder.bin")
}
pub fn width() -> f32 {
    WIDTH_M / crate::METERS_PER_WORLD_UNIT
}
pub fn parts(width: f32) -> [(Vector3<f32>, Vector3<f32>); 2] {
    let p = width / CANVAS[0];
    let d = DEPTH_M / crate::METERS_PER_WORLD_UNIT;
    let gap = 0.001 / crate::METERS_PER_WORLD_UNIT;
    [
        (
            vec3(-32.0 * p, 0.0, -d * 0.5 - gap),
            vec3(204.0 * p, 376.0 * p, d),
        ),
        (
            vec3(0.0, -152.0 * p, -d * 0.5 - gap),
            vec3(width, 72.0 * p, d),
        ),
    ]
}
/// Outward lens face on the back: its outward normal is local -Z.
pub fn lens(width: f32) -> Vector3<f32> {
    vec3(
        -32.0 * width / CANVAS[0],
        (CANVAS[1] * 0.5 - 28.0) * width / CANVAS[0],
        -(DEPTH_M + 0.003) / crate::METERS_PER_WORLD_UNIT,
    )
}
fn box_object(
    center: Vector3<f32>,
    size: Vector3<f32>,
    color: Vector3<f32>,
    source: &str,
) -> SceneObject {
    let mut object = SceneObject::new(color_material::create(color), Box::new(cube::create()));
    object.set_transform(
        Matrix4::from_translation(center) * Matrix4::from_nonuniform_scale(size.x, size.y, size.z),
    );
    object.set_debug_tag(Some(std::rc::Rc::new(SceneObjectDebugTag {
        entity_id: None,
        name: None,
        model: Some("tricorder".into()),
        source: Some(source.into()),
    })));
    object
}
pub fn lens_objects(width: f32) -> Vec<SceneObject> {
    let m = crate::METERS_PER_WORLD_UNIT;
    let center = lens(width);
    vec![
        box_object(
            center + vec3(0.0, 0.0, 0.002 / m),
            vec3(0.011, 0.011, 0.003) / m,
            vec3(0.01, 0.015, 0.02),
            "mfd_scanner_housing",
        ),
        // Unlit emissive art: visible in dark rooms without adding a world light.
        box_object(
            center + vec3(0.0, 0.0, 0.0005 / m),
            vec3(0.005, 0.005, 0.001) / m,
            vec3(0.05, 1.0, 0.7),
            "mfd_scanner_lens",
        ),
    ]
}
pub fn frame_objects(width: f32) -> Vec<SceneObject> {
    parts(width)
        .into_iter()
        .map(|(center, size)| box_object(center, size, vec3(0.035, 0.05, 0.06), "mfd_body"))
        .collect()
}
pub fn triangles() -> Vec<[Point3<f32>; 3]> {
    let faces = [
        [0, 2, 1],
        [1, 2, 3],
        [4, 5, 6],
        [5, 7, 6],
        [0, 1, 4],
        [1, 5, 4],
        [2, 6, 3],
        [3, 6, 7],
        [0, 4, 2],
        [2, 4, 6],
        [1, 3, 5],
        [3, 7, 5],
    ];
    parts(width())
        .into_iter()
        .flat_map(|(c, s)| {
            let corners: [Point3<f32>; 8] = std::array::from_fn(|i| {
                Point3::from_vec(
                    c + vec3(
                        if i & 1 == 0 { -s.x } else { s.x },
                        if i & 2 == 0 { -s.y } else { s.y },
                        if i & 4 == 0 { -s.z } else { s.z },
                    ) * 0.5,
                )
            });
            faces.map(|f| f.map(|i| corners[i]))
        })
        .collect()
}
pub fn model() -> dark::model::Model {
    let w = width();
    let p = w / CANVAS[0];
    let mut objects = frame_objects(w);
    // Screen reference is behind the runtime canvas; the editor needs its facing.
    objects.push(box_object(
        vec3(-32.0 * p, 32.0 * p, 0.0001),
        vec3(188.0 * p, 296.0 * p, 0.0001),
        vec3(0.0, 0.15, 0.12),
        "mfd_screen_reference",
    ));
    objects.extend(lens_objects(w));
    dark::model::Model::from_glb(
        objects,
        collision::Aabb3::new(
            point3(
                -w * 0.5,
                -376.0 * p * 0.5,
                -(DEPTH_M + 0.004) / crate::METERS_PER_WORLD_UNIT,
            ),
            point3(w * 0.5, 376.0 * p * 0.5, 0.001),
        ),
        None,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::InnerSpace;
    #[test]
    fn scanner_is_behind_the_shared_frame_and_surface_has_solid_depth() {
        assert!(lens(width()).z < -DEPTH_M / crate::METERS_PER_WORLD_UNIT);
        assert_eq!(triangles().len(), 24);
        for t in triangles() {
            assert!((t[1] - t[0]).cross(t[2] - t[0]).magnitude2() > 0.0);
        }
    }
}
