//! Retail `sq_scripts/object_plants.nut`'s ND_PlantAnimated visual driver.
//! Shape geometry lives separately from collision; nothing here changes ECS
//! positions, physics, or the shared base mesh. Commented-out Hanging/Tall Plant
//! registrations deliberately do not participate.
use crate::mission::mission_core::{GlobalEntityMetadata, GlobalTemplateHierarchy};
use cgmath::Vector3;
use dark::properties::{PropPosition, PropTemplateId};
use engine::scene::SceneObject;
use shipyard::{EntityId, Get, UniqueView, View, World};

const TARGETS: [&str; 8] = [
    "Ivy",
    "Plant #1",
    "Plant #2",
    "Wide Plant",
    "Egg Depositor",
    "Tall Plant 2",
    "U-Shaped Nerve",
    "Straight Nerve",
];

pub fn weights(seconds: f64, position: Vector3<f32>) -> [f32; 3] {
    // Squirrel SimTime is milliseconds, and its x/y are Dark's horizontal
    // coordinates. Undo read_vec3's (-x,z,y) and world-unit conversion.
    let value = seconds * 1000.0 + f64::from((-position.x + position.z) * dark::SCALE_FACTOR);
    [4000.0, 600.0, 1000.0].map(|period| ((value / period).sin() * 0.5) as f32)
}

pub fn apply(world: &World, entity: EntityId, objects: &mut [SceneObject]) {
    if !objects.iter().any(|o| o.geometry.has_morph()) {
        return;
    }
    let (Ok(templates), Ok(positions), Ok(metadata), Ok(hierarchy), Ok(time)) = (
        world.borrow::<View<PropTemplateId>>(),
        world.borrow::<View<PropPosition>>(),
        world.borrow::<UniqueView<GlobalEntityMetadata>>(),
        world.borrow::<UniqueView<GlobalTemplateHierarchy>>(),
        world.borrow::<UniqueView<crate::time::Time>>(),
    ) else {
        return;
    };
    let (Ok(template), Ok(position)) = (templates.get(entity), positions.get(entity)) else {
        return;
    };
    if !TARGETS.iter().any(|name| {
        metadata
            .0
            .get(&name.to_ascii_lowercase())
            .is_some_and(|target| {
                hierarchy.is_or_descends_from(template.template_id, target.template_id)
            })
    }) {
        return;
    }
    let weights = weights(time.total.as_secs_f64(), position.position);
    for object in objects {
        object.morph_weights = weights.to_vec();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_time_and_horizontal_phase_are_per_instance() {
        assert_eq!(weights(0.0, Vector3::new(0.0, 0.0, 0.0)), [0.0; 3]);
        let first = weights(1.0, Vector3::new(0.0, 0.0, 0.0));
        assert!((first[0] - 0.5 * 0.25f32.sin()).abs() < 1e-6);
        assert_ne!(first, weights(1.0, Vector3::new(-5.0, 0.0, 8.0)));
        assert_eq!(first, weights(1.0, Vector3::new(0.0, 99.0, 0.0)));
        assert!(!TARGETS.contains(&"Tall Plant"));
        assert!(!TARGETS.contains(&"Hanging Plant"));
    }
}
