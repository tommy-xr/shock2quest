//! The blade is an addition to the original amp, never a replacement item.
use cgmath::{Matrix4, Point3, Quaternion, Transform, Vector3, vec3};
use dark::importers::GLOVE_WEAPON_IMPORTER;
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use shipyard::{Component, EntityId, Get, UniqueView, View, World};
pub const POWER: i32 = -1119;
pub const WEAPON: i32 = -2291;
pub const LENGTH: f32 = 1.4;
#[derive(Component)]
pub struct BoundBlade;
pub fn active(world: &World, amp: EntityId) -> bool {
    world
        .borrow::<View<BoundBlade>>()
        .is_ok_and(|v| v.contains(amp))
        && world
            .borrow::<UniqueView<crate::psi::ActivePsiPowers>>()
            .is_ok_and(|p| p.is_active(POWER))
}
pub fn frame(world: &World, amp: EntityId) -> Option<Matrix4<f32>> {
    let transforms = world
        .borrow::<View<crate::runtime_props::RuntimePropTransform>>()
        .ok()?;
    let transform = transforms.get(amp).ok()?.0;
    Some(crate::weapon_muzzle::resolve(world, amp).shot_frame(transform))
}
pub fn segment(world: &World, amp: EntityId) -> Option<(Point3<f32>, Point3<f32>)> {
    let frame = frame(world, amp)?;
    Some((
        frame.transform_point(Point3::new(0.0, 0.0, 0.0)),
        frame.transform_point(Point3::new(0.0, LENGTH, 0.0)),
    ))
}
pub fn render(world: &World, cache: &mut AssetCache, amp: EntityId) -> Vec<SceneObject> {
    if !active(world, amp) {
        return vec![];
    }
    let Some(frame) = frame(world, amp) else {
        return vec![];
    };
    let Some(source) = cache.get_opt(&GLOVE_WEAPON_IMPORTER, "psword_h") else {
        return vec![];
    };
    let Some(source) = source.as_ref().as_ref() else {
        return vec![];
    };
    let mut low = vec3(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut high = -low;
    for p in source.triangles.iter().flatten() {
        for i in 0..3 {
            low[i] = low[i].min(p[i]);
            high[i] = high[i].max(p[i]);
        }
    }
    let size = high - low;
    let axis = (0..3).max_by(|a, b| size[*a].total_cmp(&size[*b])).unwrap();
    if !size[axis].is_finite() || size[axis] <= 0.0 {
        return vec![];
    }
    let mut base = (low + high) * 0.5;
    base[axis] = low[axis];
    let mut direction = Vector3::new(0.0, 0.0, 0.0);
    direction[axis] = 1.0;
    let local = Matrix4::from(Quaternion::from_arc(direction, Vector3::unit_y(), None))
        * Matrix4::from_scale(LENGTH / size[axis])
        * Matrix4::from_translation(-base);
    source
        .model
        .to_scene_objects()
        .iter()
        .map(|object| {
            let mut object = object.clone();
            object.set_transform(frame * local);
            object.material = std::rc::Rc::new(std::cell::RefCell::new(
                engine::scene::color_material::create(vec3(0.12, 0.8, 1.0)),
            ));
            object.set_depth_write(false);
            object.set_transparency(Some(0.15));
            object
        })
        .collect()
}
