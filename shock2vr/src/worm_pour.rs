//! Physical worm feeding. The rounds commit on emission; the grubs are only
//! short-lived visuals, never creatures or additional inventory objects.
use cgmath::{
    Deg, EuclideanSpace, InnerSpace, Matrix4, Point3, SquareMatrix, Transform, Vector3, vec3,
};
use dark::{importers::MODELS_IMPORTER, model::Model};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject};
use shipyard::EntityId;
use std::rc::Rc;

const INTERVAL: f32 = 0.32;
const LIFETIME: f32 = 0.65;

pub(crate) fn empty_beaker(model: &str) -> Option<&'static str> {
    match model.to_ascii_lowercase().as_str() {
        "beakew1" => Some("small beaker"),
        "beakew2" => Some("large beaker"),
        _ => None,
    }
}

/// A downward stream can reach any part of the gun's body. Work in its local
/// bounds so turning the gun doesn't enlarge the receiving area. A small
/// margin keeps the gesture forgiving at the edges of the organic silhouette.
pub(crate) fn pour_target(
    beaker: &Model,
    beaker_transform: Matrix4<f32>,
    weapon: &Model,
    weapon_transform: Matrix4<f32>,
) -> Option<(Vector3<f32>, Vector3<f32>)> {
    let bounds = beaker.bounding_box()?;
    let mouth = Point3::new(
        (bounds.min.x + bounds.max.x) * 0.5,
        bounds.max.y,
        (bounds.min.z + bounds.max.z) * 0.5,
    );
    let up = beaker_transform.transform_vector(Vector3::unit_y());
    if up.magnitude2() < 1e-8 || up.normalize().y > -0.2 {
        return None;
    }
    let start = beaker_transform.transform_point(mouth).to_vec();
    let bounds = weapon.bounding_box()?;
    let end = body_hit(
        start,
        weapon_transform,
        bounds.min.to_vec(),
        bounds.max.to_vec(),
    )?;
    Some((start, end))
}

fn body_hit(
    start: Vector3<f32>,
    transform: Matrix4<f32>,
    min: Vector3<f32>,
    max: Vector3<f32>,
) -> Option<Vector3<f32>> {
    let inverse = transform.invert()?;
    let origin = inverse.transform_point(Point3::from_vec(start)).to_vec();
    let direction = inverse.transform_vector(-Vector3::unit_y());
    let mut near: f32 = 0.0;
    let mut far: f32 = 0.6;
    // Held-item scales are uniform. Convert the world-space forgiveness to
    // that model scale without growing the receiving zone when held smaller.
    let margin = inverse
        .transform_vector(Vector3::unit_x() * 0.06)
        .magnitude();
    for axis in 0..3 {
        let lo = min[axis] - margin;
        let hi = max[axis] + margin;
        if direction[axis].abs() < 1e-6 {
            if origin[axis] < lo || origin[axis] > hi {
                return None;
            }
        } else {
            let a = (lo - origin[axis]) / direction[axis];
            let b = (hi - origin[axis]) / direction[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
        }
    }
    if !near.is_finite() || near > far {
        return None;
    }
    // Fade just inside the body, rather than stopping on its forgiving shell.
    Some(start - Vector3::unit_y() * (near + 0.05).min(far))
}

#[derive(Default)]
pub(crate) struct WormPour {
    hands: [PourClock; 2],
    worms: Vec<Worm>,
}

#[derive(Default)]
struct PourClock {
    pair: Option<(EntityId, EntityId)>,
    elapsed: f32,
}

struct Worm {
    weapon: EntityId,
    start: Vector3<f32>,
    target: Vector3<f32>,
    age: f32,
    phase: f32,
}

impl WormPour {
    pub fn advance(&mut self, dt: f32) {
        for worm in &mut self.worms {
            worm.age += dt;
        }
        self.worms.retain(|worm| worm.age < LIFETIME);
    }

    /// Require a sustained pour, and discard accumulated time on withdrawal,
    /// full magazine, changed ownership or lost tracking. No catch-up burst.
    pub fn ready(&mut self, slot: usize, pair: Option<(EntityId, EntityId)>, dt: f32) -> bool {
        let clock = &mut self.hands[slot];
        if clock.pair != pair || pair.is_none() {
            clock.pair = pair;
            clock.elapsed = 0.0;
        }
        if pair.is_none() {
            return false;
        }
        clock.elapsed += dt.min(0.1);
        if clock.elapsed + 1e-6 < INTERVAL {
            return false;
        }
        clock.elapsed = 0.0;
        true
    }

    pub fn emit(
        &mut self,
        weapon: EntityId,
        start: Vector3<f32>,
        target: Vector3<f32>,
        transform: Matrix4<f32>,
    ) {
        if let Some(inverse) = transform.invert() {
            self.worms.push(Worm {
                weapon,
                start,
                target: inverse.transform_point(Point3::from_vec(target)).to_vec(),
                age: 0.0,
                phase: self.worms.len() as f32 * 2.4,
            });
        }
    }

    pub fn render(
        &self,
        assets: &mut AssetCache,
        model_name: &str,
        transform: impl Fn(EntityId) -> Option<Matrix4<f32>>,
    ) -> Vec<SceneObject> {
        if self.worms.is_empty() {
            return vec![];
        }
        let Some(model) = assets.get_opt(&MODELS_IMPORTER, &format!("{model_name}.bin")) else {
            return vec![];
        };
        let Some(bounds) = model.bounding_box() else {
            return vec![];
        };
        let size = bounds.max - bounds.min;
        let scale = 0.11 / size.x.max(size.y).max(size.z).max(0.001);
        let center = (bounds.min.to_vec() + bounds.max.to_vec()) * 0.5;
        let mut objects = Vec::new();
        for worm in &self.worms {
            let Some(host) = transform(worm.weapon) else {
                continue;
            };
            let end = host.transform_point(Point3::from_vec(worm.target)).to_vec();
            let t = (worm.age / 0.42).min(1.0);
            let wiggle = (worm.age * 35.0 + worm.phase).sin();
            let position = worm.start * (1.0 - t)
                + end * t
                + vec3(wiggle * 0.012, 0.04 * (std::f32::consts::PI * t).sin(), 0.0);
            let root = Matrix4::from_translation(position)
                * Matrix4::from_angle_y(Deg(worm.phase * 50.0 + wiggle * 20.0))
                * Matrix4::from_angle_z(Deg(25.0 + wiggle * 18.0))
                * Matrix4::from_nonuniform_scale(scale * (1.0 + wiggle * 0.12), scale, scale)
                * Matrix4::from_translation(-center);
            let fade = ((worm.age - 0.38) / (LIFETIME - 0.38)).clamp(0.0, 1.0);
            for mut object in model.clone_scene_objects() {
                object.set_transform(root);
                object.set_transparency(Some(fade));
                object.set_depth_write(false);
                object.set_debug_tag(Some(Rc::new(engine::scene::SceneObjectDebugTag {
                    entity_id: Some(worm.weapon.inner()),
                    name: None,
                    model: Some(model_name.to_owned()),
                    source: Some("worm_pour".to_owned()),
                })));
                objects.push(object);
            }
        }
        objects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_accepts_both_ends_but_not_a_stream_beside_or_below_it() {
        let min = vec3(-0.5, -0.1, -0.1);
        let max = vec3(0.5, 0.1, 0.1);
        for x in [-0.45, 0.0, 0.45] {
            assert!(body_hit(vec3(x, 0.4, 0.0), Matrix4::identity(), min, max).is_some());
        }
        for start in [
            vec3(0.8, 0.4, 0.0),
            vec3(0.0, -0.4, 0.0),
            vec3(0.0, 1.0, 0.0),
        ] {
            assert!(body_hit(start, Matrix4::identity(), min, max).is_none());
        }
        let rotated = Matrix4::from_angle_y(Deg(90.0));
        assert!(body_hit(vec3(0.0, 0.4, 0.45), rotated, min, max).is_some());
        assert!(body_hit(vec3(0.45, 0.4, 0.0), rotated, min, max).is_none());
    }

    #[test]
    fn stopping_or_changing_hands_discards_partial_pour_time() {
        let mut pour = WormPour::default();
        let mut world = shipyard::World::new();
        let a = world.add_entity(());
        let b = world.add_entity(());
        let pair = Some((a, b));
        for _ in 0..19 {
            assert!(!pour.ready(0, pair, 1.0 / 60.0));
        }
        assert!(pour.ready(0, pair, 1.0 / 60.0));
        assert!(!pour.ready(0, None, 1.0));
        assert!(!pour.ready(0, pair, 1.0 / 60.0));
        assert!(!pour.ready(1, pair, 1.0 / 60.0));
        assert!(!pour.ready(0, Some((b, a)), 1.0 / 60.0));
    }
}
