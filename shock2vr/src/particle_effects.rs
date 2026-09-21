//! Layered visual replacements for authored particle effects. Gameplay still
//! selects the original spang, so collision, damage and creature routing stay
//! in their existing paths. Missing installed art preserves the legacy effect.

use cgmath::{Matrix4, vec3};
use dark::importers::TEXTURE_IMPORTER;
use engine::{
    assets::asset_cache::AssetCache,
    scene::{ParticleSystem, SceneObject},
    texture::{TextureOptions, TextureTrait},
};
use std::{rc::Rc, time::Duration};

pub struct ParticleEffect {
    layers: Vec<ParticleSystem>,
}

impl From<ParticleSystem> for ParticleEffect {
    fn from(system: ParticleSystem) -> Self {
        Self {
            layers: vec![system],
        }
    }
}

impl ParticleEffect {
    pub fn update(&mut self, elapsed: Duration, transform: Matrix4<f32>) {
        for layer in &mut self.layers {
            layer.update(elapsed, transform);
        }
    }

    pub fn is_done(&self) -> bool {
        self.layers.iter().all(ParticleSystem::is_done)
    }

    pub fn render(&self) -> Vec<SceneObject> {
        self.layers
            .iter()
            .flat_map(ParticleSystem::render)
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EnhancedEffect {
    BloodDropletsAndMist,
    BloodSpray,
}

impl EnhancedEffect {
    pub fn for_template(template: i32, lookup: impl Fn(&str) -> Option<i32>) -> Option<Self> {
        [
            ("standard blood spang", Self::BloodDropletsAndMist),
            ("new blood spang", Self::BloodSpray),
        ]
        .into_iter()
        .find_map(|(name, kind)| (lookup(name) == Some(template)).then_some(kind))
    }

    pub fn build(self, assets: &mut AssetCache) -> Option<ParticleEffect> {
        let sprite_names: &[&str] = match self {
            Self::BloodDropletsAndMist => &["NDbld", "NDsmk"],
            Self::BloodSpray => &["ND-bsp"],
        };
        let frames: Vec<_> = sprite_names
            .iter()
            .map(|name| particle_frames(assets, name))
            .collect();
        if frames.iter().any(Vec::is_empty) {
            return None;
        }
        Some(ParticleEffect {
            layers: blood_layers(self, &frames),
        })
    }
}

/// Named bitmap mounts expose qualified basenames, including zero-based frame
/// suffixes. Model animation's underscore convention does not apply here.
pub(crate) fn particle_frames(assets: &mut AssetCache, name: &str) -> Vec<Rc<dyn TextureTrait>> {
    let options = TextureOptions {
        wrap: false,
        ..Default::default()
    };
    let mut frames: Vec<Rc<dyn TextureTrait>> = Vec::new();
    for frame in 0..64 {
        let path = format!("bitmap/{name}{frame:02}.dds");
        let Some(texture) = assets.get_ext_opt(&TEXTURE_IMPORTER, &path, &options) else {
            break;
        };
        frames.push(texture);
    }
    if frames.is_empty() {
        if let Some(texture) =
            assets.get_ext_opt(&TEXTURE_IMPORTER, &format!("bitmap/{name}.dds"), &options)
        {
            frames.push(texture);
        } else {
            tracing::warn!("missing particle art {name}; keeping legacy visuals");
        }
    }
    frames
}

/// The spang orientation places local -X along the impact normal. World-space
/// simulation preserves that launch direction while gravity always stays down.
fn blood_layers(kind: EnhancedEffect, frames: &[Vec<Rc<dyn TextureTrait>>]) -> Vec<ParticleSystem> {
    let base = ParticleSystem::new()
        .with_one_shot(true)
        .with_world_space(true)
        .with_launch_bounding_box(vec3(-0.015, -0.025, -0.025), vec3(0.0, 0.025, 0.025))
        .with_color(vec3(0.65, 0.08, 0.06))
        .with_alpha(0.85);
    match kind {
        EnhancedEffect::BloodDropletsAndMist => vec![
            base.clone()
                .with_num_particles(8)
                .with_lifetime(0.85, 1.05)
                .with_particle_size(0.075, 0.11)
                .with_velocity(vec3(-1.0, -0.2, -0.6), vec3(-0.3, 0.8, 0.6))
                .with_acceleration(vec3(0.0, -2.5, 0.0))
                .with_fade_time(0.3)
                .with_sprite_animation(frames[0].clone(), Duration::from_millis(110), false),
            base.with_num_particles(3)
                .with_lifetime(0.65, 0.8)
                .with_particle_size(0.18, 0.24)
                .with_velocity(vec3(-0.25, -0.12, -0.12), vec3(-0.08, 0.12, 0.12))
                .with_size_velocity(0.3)
                .with_alpha(0.22)
                .with_fade_in_time(0.06)
                .with_fade_time(0.6)
                .with_sprite_animation(frames[1].clone(), Duration::from_millis(60), false),
        ],
        EnhancedEffect::BloodSpray => vec![
            base.with_num_particles(2)
                .with_lifetime(0.45, 0.55)
                .with_particle_size(0.32, 0.4)
                .with_velocity(vec3(-0.45, -0.15, -0.15), vec3(-0.15, 0.15, 0.15))
                .with_size_velocity(0.2)
                .with_fade_time(0.35)
                .with_sprite_animation(frames[0].clone(), Duration::from_millis(40), false),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::SquareMatrix;

    #[test]
    fn replaces_only_named_blood_effects_using_loaded_template_ids() {
        let lookup = |name: &str| match name {
            "standard blood spang" => Some(-10),
            "new blood spang" => Some(-20),
            _ => None,
        };
        assert_eq!(
            EnhancedEffect::for_template(-10, lookup),
            Some(EnhancedEffect::BloodDropletsAndMist)
        );
        assert_eq!(
            EnhancedEffect::for_template(-20, lookup),
            Some(EnhancedEffect::BloodSpray)
        );
        assert_eq!(EnhancedEffect::for_template(-366, lookup), None);
        assert_eq!(EnhancedEffect::for_template(-10, |_| None), None);
    }

    #[test]
    fn blood_parent_outlives_spray_and_all_layers_expire() {
        let empty_frames = vec![vec![], vec![]];
        let mut parent = ParticleEffect {
            layers: blood_layers(EnhancedEffect::BloodDropletsAndMist, &empty_frames),
        };
        let mut spray = ParticleEffect {
            layers: blood_layers(EnhancedEffect::BloodSpray, &empty_frames),
        };
        for effect in [&mut parent, &mut spray] {
            effect.update(Duration::ZERO, Matrix4::identity());
        }
        for effect in [&mut parent, &mut spray] {
            effect.update(Duration::from_millis(600), Matrix4::identity());
        }
        assert!(spray.is_done());
        assert!(!parent.is_done());
        parent.update(Duration::from_secs(1), Matrix4::identity());
        assert!(parent.is_done());
    }
}
