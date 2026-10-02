//! Authored item poses are both the holster allowlist and its rendering source.
use cgmath::{Deg, Matrix4, Vector3};
use dark::properties::{InternalPropOriginalModelName, PropModelName};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Get, View, World};

pub const RESOURCE: &str = "vr-holsters.json";
pub const SHELL_SCALE: f32 = 0.8;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HolsterPose {
    pub position_m: [f32; 3],
    pub rotation_degrees: [f32; 3],
    /// Length of the model's longest bounding-box edge, before rotation.
    pub length_m: f32,
}

impl Default for HolsterPose {
    fn default() -> Self {
        Self {
            position_m: [0.0, 0.04, 0.0],
            rotation_degrees: [-90.0, 0.0, 0.0],
            length_m: 0.24,
        }
    }
}

impl HolsterPose {
    pub fn is_valid(&self) -> bool {
        self.position_m
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 1.0)
            && self
                .rotation_degrees
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 180.0)
            && self.length_m.is_finite()
            && (0.02..=1.2).contains(&self.length_m)
    }

    /// Shared by the editor and gameplay; bounds are in model/world units.
    pub fn model_transform(&self, min: Vector3<f32>, max: Vector3<f32>) -> Matrix4<f32> {
        let size = max - min;
        let scale = self.length_m
            / crate::METERS_PER_WORLD_UNIT
            / size.x.max(size.y).max(size.z).max(0.0001);
        Matrix4::from_translation(Vector3::from(self.position_m) / crate::METERS_PER_WORLD_UNIT)
            * Matrix4::from_angle_x(Deg(self.rotation_degrees[0]))
            * Matrix4::from_angle_y(Deg(self.rotation_degrees[1]))
            * Matrix4::from_angle_z(Deg(self.rotation_degrees[2]))
            * Matrix4::from_scale(scale)
            * Matrix4::from_translation(-(min + max) * 0.5)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HolsterEntry {
    /// World model, stable when the held model switches on Hold/Drop.
    pub model: String,
    pub label: String,
    /// Lets the hand-grip editor open the same item's holster definition.
    pub held_model: Option<String>,
    pub pose: HolsterPose,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HolsterLibrary {
    pub entries: Vec<HolsterEntry>,
}

pub fn model_key(name: &str) -> String {
    name.trim()
        .to_ascii_lowercase()
        .trim_end_matches(".bin")
        .to_owned()
}

/// Item definitions name BIN models; GLB is only used for the wearable shell.
pub fn is_bin_model_name(name: &str) -> bool {
    let key = model_key(name);
    !key.is_empty()
        && !key.contains(['/', '\\'])
        && name
            .trim()
            .rsplit_once('.')
            .is_none_or(|(_, extension)| extension.eq_ignore_ascii_case("bin"))
}

impl HolsterLibrary {
    pub fn parse(text: &str) -> Result<Self, String> {
        let library: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let mut keys = std::collections::HashSet::new();
        for entry in &library.entries {
            if entry.label.trim().is_empty() || !entry.pose.is_valid() {
                return Err("Holster entries require a label and a finite, bounded pose".into());
            }
            for name in std::iter::once(&entry.model).chain(entry.held_model.iter()) {
                let key = model_key(name);
                if !is_bin_model_name(name) || !keys.insert(key) {
                    return Err(format!("Invalid or duplicate holster model: {name}"));
                }
            }
        }
        Ok(library)
    }

    pub fn get(&self, model: &str) -> Option<&HolsterEntry> {
        let key = model_key(model);
        self.entries.iter().find(|entry| {
            model_key(&entry.model) == key
                || entry
                    .held_model
                    .as_ref()
                    .is_some_and(|name| model_key(name) == key)
        })
    }

    pub fn for_entity(&self, world: &World, entity: EntityId) -> Option<&HolsterEntry> {
        let original = world
            .borrow::<View<InternalPropOriginalModelName>>()
            .ok()
            .and_then(|names| names.get(entity).ok().map(|name| name.0.clone()));
        let model = original.or_else(|| {
            world
                .borrow::<View<PropModelName>>()
                .ok()
                .and_then(|names| names.get(entity).ok().map(|name| name.0.clone()))
        })?;
        self.get(&model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{EuclideanSpace, InnerSpace, Point3, Transform, vec3};

    #[test]
    fn definitions_alone_control_eligibility_across_model_switches() {
        let mut library =
            HolsterLibrary::parse(include_str!("../../assets/vr-holsters.json")).unwrap();
        let mut world = World::new();
        assert!(library.get("ar15_h").is_none());
        for entry in &library.entries {
            let item = world.add_entity((InternalPropOriginalModelName(entry.model.clone()),));
            assert!(
                library.for_entity(&world, item).is_some(),
                "{}",
                entry.label
            );
        }
        let pistol = world.add_entity((
            InternalPropOriginalModelName("ATEK_W.BIN".into()),
            PropModelName("atek_h".into()),
        ));
        assert!(library.for_entity(&world, pistol).is_some());
        world.add_component(pistol, PropModelName("atek_w".into()));
        assert!(library.for_entity(&world, pistol).is_some());
        library.entries.retain(|entry| entry.model != "atek_w");
        assert!(library.for_entity(&world, pistol).is_none());
        let mug = world.add_entity((PropModelName("mug".into()),));
        assert!(library.for_entity(&world, mug).is_none());
        library.entries.push(HolsterEntry {
            model: "mug".into(),
            held_model: None,
            label: "Mug".into(),
            pose: HolsterPose::default(),
        });
        assert!(library.for_entity(&world, mug).is_some());
    }

    #[test]
    fn authored_pose_round_trips_and_places_model_center_in_metres() {
        let library = HolsterLibrary::parse(include_str!("../../assets/vr-holsters.json")).unwrap();
        assert_eq!(
            HolsterLibrary::parse(&serde_json::to_string(&library).unwrap()).unwrap(),
            library
        );
        let pose = &library.get("atek_h").unwrap().pose;
        let min = vec3(-2.0, -1.0, 0.0);
        let max = vec3(4.0, 3.0, 1.0);
        let transform = pose.model_transform(min, max);
        let center = transform.transform_point(Point3::from_vec((min + max) * 0.5));
        assert!((center.x - pose.position_m[0] / crate::METERS_PER_WORLD_UNIT).abs() < 0.0001);
        assert!((center.y - pose.position_m[1] / crate::METERS_PER_WORLD_UNIT).abs() < 0.0001);
        assert!((center.z - pose.position_m[2] / crate::METERS_PER_WORLD_UNIT).abs() < 0.0001);
        let length = transform.transform_vector(vec3(6.0, 0.0, 0.0)).magnitude();
        assert!((length - pose.length_m / crate::METERS_PER_WORLD_UNIT).abs() < 0.0001);
        assert!(transform.transform_vector(Vector3::unit_y()).z < 0.0);
        let mut invalid = library.clone();
        invalid.entries[0].pose.length_m = 0.0;
        assert!(HolsterLibrary::parse(&serde_json::to_string(&invalid).unwrap()).is_err());
        invalid = library.clone();
        invalid.entries.push(invalid.entries[0].clone());
        assert!(HolsterLibrary::parse(&serde_json::to_string(&invalid).unwrap()).is_err());
    }

    #[test]
    fn item_definitions_only_accept_bin_models() {
        assert!(is_bin_model_name("atek_w"));
        assert!(is_bin_model_name("ATEK_W.BIN"));
        for model in [
            "holster.glb",
            "item.obj",
            "item.gltf",
            "../atek_w.bin",
            ".bin",
        ] {
            assert!(!is_bin_model_name(model), "{model}");
            let mut library =
                HolsterLibrary::parse(include_str!("../../assets/vr-holsters.json")).unwrap();
            library.entries[0].model = model.into();
            assert!(HolsterLibrary::parse(&serde_json::to_string(&library).unwrap()).is_err());
        }
    }
}
