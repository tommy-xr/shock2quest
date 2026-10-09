//! Opt-in, data-driven workloads for repeatable renderer measurements.
//! Setup uses ordinary mission effects; it never writes a save or changes defaults.
use cgmath::{Matrix4, Quaternion, SquareMatrix, Vector3, point3};
use serde::{Deserialize, Serialize};
use shipyard::EntityId;

use crate::game_scene::DebugEntityMessage;
use crate::mission::entity_creator::CreateEntityOptions;
use crate::scripts::Effect;
use crate::{Game, dev_params, free_camera};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkScene {
    pub name: String,
    pub mission: String,
    pub player_position: [f32; 3],
    pub camera_eye: [f32; 3],
    pub camera_look_at: [f32; 3],
    pub subject_model: String,
    /// Additional models in a mixed crowd; counts are validated separately.
    #[serde(default)]
    pub additional_subjects: Vec<BenchmarkSubject>,
    #[serde(default)]
    pub upgraded_terrain: bool,
    #[serde(default)]
    pub terrain_wetness: f32,
    /// Quest-only fixed foveation. Fixtures override the device's opt-in file.
    #[serde(default = "default_ffr")]
    pub ffr: String,
    #[serde(default)]
    pub profile_cpu: bool,
    #[serde(default)]
    pub fixed_simulation: bool,
    #[serde(default)]
    pub spotlight: Option<BenchmarkSpotlight>,
    pub expected_subject_meshes: usize,
    #[serde(default)]
    pub subject_templates: Vec<i32>,
    #[serde(default)]
    pub object_lighting: bool,
    #[serde(default)]
    pub lights_on: bool,
    #[serde(default)]
    pub light_templates: Vec<i32>,
    #[serde(default)]
    pub remove_templates: Vec<i32>,
    #[serde(default)]
    pub spawns: Vec<BenchmarkSpawn>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkSubject {
    pub model: String,
    pub expected_meshes: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkSpotlight {
    pub position: [f32; 3],
    pub look_at: [f32; 3],
    pub intensity: f32,
    pub range: f32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkSpawn {
    pub template_id: i32,
    /// Static egg props still count as meshes but have no skeletal animation.
    #[serde(default = "default_animated")]
    pub animated: bool,
    pub position: [f32; 3],
}

fn default_animated() -> bool {
    true
}

fn default_ffr() -> String {
    "off".into()
}

pub struct BenchmarkRun {
    pub scene: BenchmarkScene,
    subjects: Vec<EntityId>,
    subject_entities: std::collections::HashSet<u64>,
    lamps: Vec<EntityId>,
    setup_remaining: std::time::Duration,
}

impl BenchmarkScene {
    pub fn parse(json: &str) -> Result<Self, String> {
        let scene: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if !matches!(scene.ffr.as_str(), "off" | "low" | "medium" | "high") {
            return Err("benchmark ffr must be off, low, medium or high".into());
        }
        if scene.name.is_empty()
            || !scene
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err("benchmark name must contain only letters, digits or hyphens".into());
        }
        if !scene.mission.ends_with(".mis") || scene.mission.contains(['/', '\\']) {
            return Err("benchmark mission must be a data-relative .mis name".into());
        }
        if scene.subject_model.is_empty()
            || scene.expected_subject_meshes == 0
            || scene.spawns.len() > 64
            || (scene.subject_templates.is_empty() && scene.spawns.is_empty())
        {
            return Err("benchmark needs identified subjects and at most 64 spawns".into());
        }
        for position in std::iter::once(&scene.player_position)
            .chain([&scene.camera_eye, &scene.camera_look_at])
            .chain(scene.spawns.iter().map(|s| &s.position))
        {
            if !position.iter().all(|x| x.is_finite()) {
                return Err("non-finite benchmark position".into());
            }
        }
        if free_camera::look_at_rotation(scene.camera_eye.into(), scene.camera_look_at.into())
            .is_none()
        {
            return Err("benchmark camera must have a nonzero look direction".into());
        }
        if !scene.terrain_wetness.is_finite()
            || !(0.0..=4.0).contains(&scene.terrain_wetness)
            || (scene.terrain_wetness > 0.0 && !scene.upgraded_terrain)
        {
            return Err("wetness requires upgraded terrain and a strength in 0..=4".into());
        }
        let mut models = std::collections::HashSet::from([scene.subject_model.to_lowercase()]);
        for subject in &scene.additional_subjects {
            if subject.model.is_empty()
                || subject.expected_meshes == 0
                || !models.insert(subject.model.to_lowercase())
            {
                return Err(
                    "benchmark subject models must be unique and have positive mesh counts".into(),
                );
            }
        }
        if let Some(light) = &scene.spotlight {
            if !light
                .position
                .iter()
                .chain(&light.look_at)
                .all(|x| x.is_finite())
                || free_camera::look_at_rotation(light.position.into(), light.look_at.into())
                    .is_none()
                || !light.intensity.is_finite()
                || !(0.0..=10.0).contains(&light.intensity)
                || !light.range.is_finite()
                || !(0.1..=100.0).contains(&light.range)
            {
                return Err("invalid benchmark spotlight".into());
            }
        }
        Ok(scene)
    }

    /// Apply before asset mounting so classic and upgraded runs use the same APK.
    pub fn configure(&self, features: &mut std::collections::HashSet<String>) {
        if self.fixed_simulation {
            features.insert("fixed_simulation".into());
        } else {
            features.remove("fixed_simulation");
        }
        if self.profile_cpu {
            features.insert("profile_cpu".into());
        } else {
            features.remove("profile_cpu");
        }
        if self.upgraded_terrain {
            features.insert("upgraded_terrain".into());
            features.remove("no_upgraded_terrain");
        } else {
            features.remove("upgraded_terrain");
            features.insert("no_upgraded_terrain".into());
        }
        dev_params::set(
            dev_params::OBJECT_LIGHTING,
            if self.object_lighting { 1.0 } else { 0.0 },
        );
        dev_params::set(dev_params::TERRAIN_WETNESS, self.terrain_wetness);
    }

    pub fn spotlight(&self) -> Option<engine::scene::light::SpotLight> {
        self.spotlight.as_ref().map(|light| {
            let position: Vector3<f32> = light.position.into();
            let target: Vector3<f32> = light.look_at.into();
            let mut spot = engine::scene::light::SpotLight::new(
                position,
                target - position,
                Vector3::new(1.0, 1.0, 1.0),
                light.intensity,
            );
            spot.range = light.range;
            spot
        })
    }

    pub fn apply(self, game: &mut Game) -> Result<BenchmarkRun, String> {
        if game.options.mission != self.mission {
            return Err("benchmark mission mismatch".into());
        }
        let debug = game.debug_scene().ok_or("mission has no debug interface")?;
        let entities = debug.list_entities(None, None);
        // Validate every authored handle before making any changes. Runtime IDs
        // are discovered on each launch, never serialized into a fixture.
        let resolve = |template| -> Result<EntityId, String> {
            let matches: Vec<_> = entities
                .iter()
                .filter(|e| e.template_id == template)
                .collect();
            if matches.len() != 1 {
                return Err(format!(
                    "benchmark template {template}: expected one entity, got {}",
                    matches.len()
                ));
            }
            debug
                .resolve_entity_id(matches[0].id)
                .ok_or_else(|| format!("missing entity for {template}"))
        };
        let removed = self
            .remove_templates
            .iter()
            .map(|t| resolve(*t))
            .collect::<Result<Vec<_>, _>>()?;
        let lamps = self
            .light_templates
            .iter()
            .map(|t| resolve(*t))
            .collect::<Result<Vec<_>, _>>()?;
        let authored_subjects = self
            .subject_templates
            .iter()
            .map(|t| resolve(*t))
            .collect::<Result<Vec<_>, _>>()?;
        game.debug_scene_mut()
            .unwrap()
            .teleport_player(self.player_position.into())?;
        game.apply_scene_effects(
            removed
                .into_iter()
                .map(|entity_id| Effect::DestroyEntity { entity_id })
                .collect(),
        );
        game.apply_scene_effects(
            self.spawns
                .iter()
                .enumerate()
                .map(|(index, spawn)| Effect::CreateEntity {
                    template_id: spawn.template_id,
                    position: point3(spawn.position[0], spawn.position[1], spawn.position[2]),
                    orientation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
                    root_transform: Matrix4::identity(),
                    options: CreateEntityOptions {
                        force_visible: true,
                        name_override: Some(format!("benchmark-{}-{index}", self.name)),
                        ..Default::default()
                    },
                })
                .collect(),
        );
        let debug = game.debug_scene().unwrap();
        let prefix = format!("benchmark-{}-", self.name);
        let spawned = debug
            .list_entities(None, Some(&prefix))
            .iter()
            .filter_map(|e| debug.resolve_entity_id(e.id))
            .collect::<Vec<_>>();
        if spawned.len() != self.spawns.len() {
            return Err("benchmark spawn count mismatch".into());
        }
        let subject_entities = spawned
            .iter()
            .chain(&authored_subjects)
            .map(|id| id.inner())
            .collect();
        let named = debug.list_entities(None, Some(&prefix));
        let subjects = self
            .spawns
            .iter()
            .enumerate()
            .filter(|(_, spawn)| spawn.animated)
            .map(|(index, _)| {
                let name = format!("{prefix}{index}");
                named
                    .iter()
                    .find(|entity| entity.name == name)
                    .and_then(|entity| debug.resolve_entity_id(entity.id))
                    .ok_or_else(|| format!("missing animated benchmark subject {name}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        dev_params::set(dev_params::FREE_CAMERA, 1.0);
        dev_params::set(dev_params::FREE_CAMERA_CULL_FROM_CAMERA, 1.0);
        Ok(BenchmarkRun {
            scene: self,
            subjects,
            subject_entities,
            lamps,
            setup_remaining: std::time::Duration::from_secs(2),
        })
    }
}

impl BenchmarkRun {
    /// Placement can trigger authored switches on subsequent updates. Let that
    /// startup chain settle, then establish the fixture's lamp state once.
    /// The runner verifies the state stays correct throughout measurement.
    pub fn advance_setup(&mut self, game: &mut Game, elapsed: std::time::Duration) {
        if self.setup_remaining.is_zero() {
            return;
        }
        self.setup_remaining = self.setup_remaining.saturating_sub(elapsed);
        if !self.setup_remaining.is_zero() {
            return;
        }
        for &entity in &self.lamps {
            let message = if self.scene.lights_on {
                DebugEntityMessage::TurnOn
            } else {
                DebugEntityMessage::TurnOff
            };
            assert!(
                game.debug_scene_mut()
                    .unwrap()
                    .send_entity_message(entity, message),
                "benchmark light message rejected"
            );
        }
    }

    /// Compensate the current tracked head pose, retaining each eye's actual
    /// stereo offset. Only an explicitly loaded benchmark locks the camera.
    pub fn place_camera(
        &self,
        game: &mut Game,
        head_offset: Vector3<f32>,
        head_rotation: Quaternion<f32>,
    ) {
        let eye = self.scene.camera_eye.into();
        let rotation =
            free_camera::look_at_rotation(eye, self.scene.camera_look_at.into()).unwrap();
        game.place_free_camera(free_camera::pose_for_eye(
            (eye, rotation),
            head_offset,
            head_rotation,
        ));
    }

    /// Once-per-second workload evidence, outside the measured render stages.
    pub fn observation(
        &self,
        game: &Game,
        scene: &[engine::scene::SceneObject],
    ) -> serde_json::Value {
        let meshes: Vec<_> = scene
            .iter()
            .filter(|o| {
                o.debug_tag().is_some_and(|tag| {
                    tag.entity_id
                        .is_some_and(|id| self.subject_entities.contains(&id))
                        && tag
                            .model
                            .as_ref()
                            .is_some_and(|m| m.eq_ignore_ascii_case(&self.scene.subject_model))
                })
            })
            .collect();
        let additional_subjects: Vec<_> =
            self.scene
                .additional_subjects
                .iter()
                .map(|subject| {
                    let matching: Vec<_> = scene
                        .iter()
                        .filter(|object| {
                            object.debug_tag().is_some_and(|tag| {
                                tag.entity_id
                                    .is_some_and(|id| self.subject_entities.contains(&id))
                                    && tag.model.as_ref().is_some_and(|model| {
                                        model.eq_ignore_ascii_case(&subject.model)
                                    })
                            })
                        })
                        .collect();
                    serde_json::json!({"model":subject.model,"meshes":matching.len(),
                "lit_meshes":matching.iter().filter(|object| object.lights().is_some()).count()})
                })
                .collect();
        let animations: Vec<_> = self.subjects.iter().filter_map(|id| game.debug_scene()?.animation_state(*id))
            .map(|a| serde_json::json!({"entity":a.entity_id,"clip":a.clip,"frame":a.frame,"position":a.position})).collect();
        let lamp_intensities: Vec<_> = self
            .lamps
            .iter()
            .map(|id| {
                game.debug_scene()
                    .and_then(|debug| debug.entity_detail(*id))
                    .and_then(|detail| {
                        detail
                            .properties
                            .into_iter()
                            .find(|p| p.name == "AnimLightIntensity")
                    })
                    .and_then(|property| property.value.parse::<f32>().ok())
            })
            .collect();
        serde_json::json!({"name":self.scene.name,"object_lighting":dev_params::get_bool(dev_params::OBJECT_LIGHTING),
            "upgraded_terrain":game.options.upgraded_terrain_enabled(),
            "terrain_wetness":dev_params::get(dev_params::TERRAIN_WETNESS),"additional_subjects":additional_subjects,
            "subject_meshes":meshes.len(),"expected_subject_meshes":self.scene.expected_subject_meshes,
            "lit_subject_meshes":meshes.iter().filter(|o| o.lights().is_some()).count(),
            "scene_objects":scene.len(),"animations":animations,"lamp_intensities":lamp_intensities,"setup_complete":self.setup_remaining.is_zero()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_camera_and_unknown_fields_are_validated_before_setup() {
        let value = serde_json::json!({"name":"court","mission":"rec1.mis","player_position":[0,0,0],
            "camera_eye":[0,1,0],"camera_look_at":[0,1,-1],"subject_model":"grunt_p","expected_subject_meshes":12,"subject_templates":[466]});
        assert!(BenchmarkScene::parse(&value.to_string()).is_ok());
        let mut invalid = value.clone();
        invalid["camera_look_at"] = invalid["camera_eye"].clone();
        assert!(BenchmarkScene::parse(&invalid.to_string()).is_err());
        let mut invalid = value.clone();
        invalid["mission"] = "../rec1.mis".into();
        assert!(BenchmarkScene::parse(&invalid.to_string()).is_err());
        let mut invalid = value;
        invalid["typo"] = true.into();
        assert!(BenchmarkScene::parse(&invalid.to_string()).is_err());
    }

    #[test]
    fn mixed_workload_rejects_invalid_modes_models_and_lights() {
        let value: serde_json::Value = serde_json::from_str(include_str!(
            "../../benchmarks/scenes/many-brain-mixed-crowd.json"
        ))
        .unwrap();
        for (field, invalid) in [
            ("ffr", serde_json::json!("dynamic")),
            ("terrain_wetness", serde_json::json!(1.5)),
            ("terrain_wetness", serde_json::json!(-1)),
            (
                "additional_subjects",
                serde_json::json!([{"model":"RUMBLER","expected_meshes":1}]),
            ),
            (
                "spotlight",
                serde_json::json!({"position":[0,0,0],"look_at":[0,0,0],"intensity":1,"range":10}),
            ),
        ] {
            let mut invalid_scene = value.clone();
            invalid_scene[field] = invalid;
            assert!(BenchmarkScene::parse(&invalid_scene.to_string()).is_err());
        }
        let scene = BenchmarkScene::parse(&value.to_string()).unwrap();
        assert_eq!(scene.ffr, "off");
        assert_eq!(
            scene.spawns.iter().filter(|spawn| spawn.animated).count(),
            6
        );
    }

    #[test]
    fn benchmark_can_force_both_terrain_modes_despite_the_default() {
        let mut scene = BenchmarkScene::parse(include_str!(
            "../../benchmarks/scenes/many-brain-mixed-crowd.json"
        ))
        .unwrap();
        let mut options = crate::GameOptions::default();
        for enabled in [false, true, false] {
            scene.upgraded_terrain = enabled;
            scene.configure(&mut options.experimental_features);
            assert_eq!(options.upgraded_terrain_enabled(), enabled);
        }
    }

    #[test]
    fn checked_in_fixtures_have_explicit_subjects() {
        for json in [
            include_str!("../../benchmarks/scenes/many-brain-mixed-crowd.json"),
            include_str!("../../benchmarks/scenes/rec1-court-lit.json"),
            include_str!("../../benchmarks/scenes/rec1-court-dark.json"),
            include_str!("../../benchmarks/scenes/rec1-six-hybrids.json"),
        ] {
            BenchmarkScene::parse(json).unwrap();
        }
    }
}
