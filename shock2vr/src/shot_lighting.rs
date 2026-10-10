//! Transient shot lights derived from live visual entities, not duplicate timers.
use cgmath::{EuclideanSpace, InnerSpace, Point3, Transform, Vector3, Vector4};
use dark::properties::{PropRenderType, PropTemplateId, RenderType};
use engine::scene::light::PointLight;
use shipyard::{Get, IntoIter, IntoWithId, View, World};

use crate::{
    runtime_props::{RuntimePropAttachment, RuntimePropTransform, RuntimePropViewmodelToWorld},
    weapon_modification::{self, Kind},
};

/// Keep at least two of the renderer's six slots available for object-local
/// room lights. Existing carried lights have priority; shots use at most two.
pub(crate) fn budget(carried_count: usize) -> usize {
    4usize.saturating_sub(carried_count).min(2)
}

pub(crate) fn lights(
    world: &World,
    observer: Vector3<f32>,
    limit: usize,
    lookup: impl Fn(&str) -> Option<i32>,
) -> Vec<PointLight> {
    if limit == 0 {
        return Vec::new();
    }
    let flash = lookup("assault flash");
    let lasers = [lookup("laser shot"), lookup("big laser shot")];
    let (templates, transforms, attachments, mappings, render_types) = world
        .borrow::<(
            View<PropTemplateId>,
            View<RuntimePropTransform>,
            View<RuntimePropAttachment>,
            View<RuntimePropViewmodelToWorld>,
            View<PropRenderType>,
        )>()
        .unwrap();
    let mut candidates = Vec::new();
    for (entity, (template, transform)) in (&templates, &transforms).iter().with_id() {
        // LaserShot deliberately hides the bolt for its first 50 ms. Its
        // light follows that same reveal, and disappears with the entity.
        if render_types
            .get(entity)
            .is_ok_and(|render| matches!(render.0, RenderType::NoRender | RenderType::EditorOnly))
        {
            continue;
        }
        let mut position = transform.0.transform_point(Point3::new(0.0, 0.0, 0.0));
        let (priority, color_intensity, range) = if flash == Some(template.template_id) {
            let Ok(attachment) = attachments.get(entity) else {
                continue;
            };
            if weapon_modification::kind(world, attachment.parent) != Some(Kind::Pistol) {
                continue;
            }
            // The flash shares the flat weapon's special projection. Map its
            // rendered position into the world once so it lights nearby walls.
            if let Ok(mapping) = mappings.get(attachment.parent) {
                position = mapping.0.transform_point(position);
            }
            (0, Vector4::new(1.0, 0.65, 0.25, 2.0), 3.0)
        } else if lasers.contains(&Some(template.template_id)) {
            (1, Vector4::new(0.25, 0.6, 1.0, 1.2), 2.5)
        } else {
            continue;
        };
        let position = position.to_vec();
        let distance = (position - observer).magnitude2();
        if !distance.is_finite() {
            continue;
        }
        candidates.push((
            priority,
            distance,
            entity,
            PointLight {
                position,
                color_intensity,
                range,
            },
        ));
    }
    // A nearby shot should not lose its light to an older distant bolt. Stable
    // tie-breaking also prevents equally distant lights from alternating slots.
    candidates.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.total_cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });
    candidates
        .into_iter()
        .take(limit.min(2))
        .map(|(_, _, _, light)| light)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Matrix4, SquareMatrix, vec3};
    use dark::properties::PropScripts;
    use shipyard::EntityId;

    fn lookup(name: &str) -> Option<i32> {
        match name {
            "assault flash" => Some(-10),
            "laser shot" => Some(-20),
            "big laser shot" => Some(-21),
            _ => None,
        }
    }

    fn spawn(world: &mut World, template: i32, position: Vector3<f32>) -> EntityId {
        world.add_entity((
            PropTemplateId {
                template_id: template,
            },
            RuntimePropTransform(Matrix4::from_translation(position)),
        ))
    }

    #[test]
    fn pistol_light_requires_live_flash_and_uses_flat_projection_mapping() {
        let mut world = World::new();
        let gun = world.add_entity((
            PropScripts {
                scripts: vec!["PistolModify".into()],
                inherits: true,
            },
            RuntimePropViewmodelToWorld(Matrix4::from_translation(vec3(0.5, 0.0, 0.0))),
        ));
        // Dry firing or a silencer creates no flash, hence no light.
        assert!(lights(&world, vec3(0.0, 0.0, 0.0), 2, lookup).is_empty());
        let flash = spawn(&mut world, -10, vec3(1.0, 2.0, 3.0));
        world.add_component(
            flash,
            RuntimePropAttachment {
                parent: gun,
                local_transform: Matrix4::identity(),
            },
        );
        let active = lights(&world, vec3(0.0, 0.0, 0.0), 2, lookup);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].position, vec3(1.5, 2.0, 3.0));
        world.delete_entity(flash);
        assert!(lights(&world, vec3(0.0, 0.0, 0.0), 2, lookup).is_empty());
    }

    #[test]
    fn laser_light_follows_reveal_motion_and_destruction() {
        let mut world = World::new();
        let bolt = spawn(&mut world, -20, vec3(1.0, 0.0, 0.0));
        world.add_component(bolt, PropRenderType(RenderType::NoRender));
        assert!(lights(&world, vec3(0.0, 0.0, 0.0), 2, lookup).is_empty());
        world.add_component(bolt, PropRenderType(RenderType::FullBright));
        assert_eq!(
            lights(&world, vec3(0.0, 0.0, 0.0), 2, lookup)[0].position.x,
            1.0
        );
        world.add_component(
            bolt,
            RuntimePropTransform(Matrix4::from_translation(vec3(4.0, 0.0, 0.0))),
        );
        assert_eq!(
            lights(&world, vec3(0.0, 0.0, 0.0), 2, lookup)[0].position.x,
            4.0
        );
        world.delete_entity(bolt);
        assert!(lights(&world, vec3(0.0, 0.0, 0.0), 2, lookup).is_empty());
    }

    #[test]
    fn light_budget_prioritizes_flash_then_nearest_bolt_and_ignores_other_templates() {
        let mut world = World::new();
        let gun = world.add_entity((PropScripts {
            scripts: vec!["PistolModify".into()],
            inherits: true,
        },));
        let flash = spawn(&mut world, -10, vec3(5.0, 0.0, 0.0));
        world.add_component(
            flash,
            RuntimePropAttachment {
                parent: gun,
                local_transform: Matrix4::identity(),
            },
        );
        spawn(&mut world, -21, vec3(3.0, 0.0, 0.0));
        spawn(&mut world, -20, vec3(1.0, 0.0, 0.0));
        spawn(&mut world, -99, vec3(0.1, 0.0, 0.0));
        let active = lights(&world, vec3(0.0, 0.0, 0.0), 6, lookup);
        assert_eq!(
            active.iter().map(|l| l.position.x).collect::<Vec<_>>(),
            vec![5.0, 1.0]
        );
        assert_eq!(
            (0..=6).map(budget).collect::<Vec<_>>(),
            vec![2, 2, 2, 1, 0, 0, 0]
        );
        assert!(lights(&world, vec3(0.0, 0.0, 0.0), 0, lookup).is_empty());
    }
}
