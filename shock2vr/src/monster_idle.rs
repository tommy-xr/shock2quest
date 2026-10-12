//! Native equivalent of the unconditional registrations in retail
//! `sq_scripts/visuals_monsteridles.nut`. The AI wrapper owns the saved last
//! value: freeze/death suppress writes, rather than resetting the shape.
use crate::mission::mission_core::{GlobalEntityMetadata, GlobalTemplateHierarchy};
use dark::properties::{AIMode, PropAIMode, PropTemplateId};
use engine::scene::SceneObject;
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, UniqueView, View, World};

const FAMILIES: [&str; 4] = ["Once-Grunts", "Rumbler", "Arachnid", "Overlords"];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MonsterIdle {
    phase: f64,
    weight: f32,
}
impl MonsterIdle {
    pub fn for_entity(world: &World, entity: EntityId) -> Option<Self> {
        let templates = world.borrow::<View<PropTemplateId>>().ok()?;
        let metadata = world.borrow::<UniqueView<GlobalEntityMetadata>>().ok()?;
        let hierarchy = world.borrow::<UniqueView<GlobalTemplateHierarchy>>().ok()?;
        let template = templates.get(entity).ok()?.template_id;
        let eligible = FAMILIES.iter().any(|name| {
            metadata
                .0
                .get(&name.to_ascii_lowercase())
                .is_some_and(|parent| hierarchy.is_or_descends_from(template, parent.template_id))
        });
        // Authored objects use their original Dark object id; runtime-spawned
        // instances use their own id. Saving the phase preserves it on remap.
        let identity = if template > 0 {
            template as u64
        } else {
            entity.inner() & 0xffff_ffff
        };
        eligible.then(|| Self {
            phase: identity as f64 * 0.9,
            weight: 0.0,
        })
    }

    pub fn update(&mut self, seconds: f64, suppressed: bool) {
        if !suppressed {
            // SimTime is milliseconds: sin((SimTime + self * 360) / 400) + .5.
            // Deliberately not clamped: the authored range is [-.5, 1.5].
            self.weight = ((seconds * 2.5 + self.phase).sin() + 0.5) as f32;
        }
    }
    pub fn weight(&self) -> f32 {
        self.weight
    }
    pub fn valid(&self) -> bool {
        self.phase.is_finite() && self.weight.is_finite()
    }
}

pub fn dead_mode(world: &World, entity: EntityId) -> bool {
    world.borrow::<View<PropAIMode>>().is_ok_and(|modes| {
        modes
            .get(entity)
            .is_ok_and(|mode| mode.mode == AIMode::Dead)
    })
}

pub fn apply(objects: &mut [SceneObject], weight: f32) {
    for object in objects {
        if let Some(index) = object
            .geometry
            .morph_names()
            .iter()
            .position(|name| name == "worm")
        {
            object
                .morph_weights
                .resize(object.geometry.morph_names().len(), 0.0);
            object.morph_weights[index] = weight;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn applying_worm_preserves_facial_channels_and_shared_geometry() {
        struct NamedMesh(Vec<String>);
        impl engine::scene::Geometry for NamedMesh {
            fn draw(&self) {}
            fn morph_names(&self) -> &[String] {
                &self.0
            }
        }
        let mut first = SceneObject::new(
            engine::scene::color_material::create(cgmath::vec3(1.0, 1.0, 1.0)),
            Box::new(NamedMesh(vec![
                "MouthShapeH".into(),
                "worm".into(),
                "MouthShapeA".into(),
            ])),
        );
        first.morph_weights = vec![0.7, 0.0, 0.3];
        let mut second = first.duplicate();
        apply(std::slice::from_mut(&mut first), 1.25);
        apply(std::slice::from_mut(&mut second), -0.25);
        assert_eq!(first.morph_weights, vec![0.7, 1.25, 0.3]);
        assert_eq!(second.morph_weights, vec![0.7, -0.25, 0.3]);
        assert!(std::rc::Rc::ptr_eq(&first.geometry, &second.geometry));
    }

    #[test]
    fn authored_phase_range_and_suppression_round_trip() {
        let mut idle = MonsterIdle {
            phase: 0.9,
            weight: 0.0,
        };
        idle.update(1.0, false);
        assert!((idle.weight() - (3.4_f32.sin() + 0.5)).abs() < 1e-6);
        let last = idle.weight();
        idle.update(2.0, true);
        assert_eq!(idle.weight(), last);
        let mut loaded: MonsterIdle =
            serde_json::from_str(&serde_json::to_string(&idle).unwrap()).unwrap();
        loaded.update(3.0, true);
        assert_eq!(loaded.weight(), last);
        loaded.update(3.0, false);
        idle.update(3.0, false);
        assert_eq!(loaded.weight(), idle.weight());
        idle.phase = std::f64::consts::FRAC_PI_2;
        idle.update(0.0, false);
        assert_eq!(idle.weight(), 1.5);
    }
}
