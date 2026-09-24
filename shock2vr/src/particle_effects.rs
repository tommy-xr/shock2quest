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

#[derive(Default)]
pub struct ParticleEffect {
    layers: Vec<ParticleSystem>,
    replaces_parent_model: bool,
}

impl From<ParticleSystem> for ParticleEffect {
    fn from(system: ParticleSystem) -> Self {
        Self {
            layers: vec![system],
            ..Self::default()
        }
    }
}

impl ParticleEffect {
    pub fn replaces_parent_model(&self) -> bool {
        self.replaces_parent_model
    }

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
    EmpCore { overload: bool },
    EmpArcs { overload: bool },
    EmpLegacyJet,
    EmpExplosion { overload: bool },
    EmpLegacyPulse,
}

impl EnhancedEffect {
    pub fn for_template(template: i32, lookup: impl Fn(&str) -> Option<i32>) -> Option<Self> {
        [
            ("standard blood spang", Self::BloodDropletsAndMist),
            ("new blood spang", Self::BloodSpray),
            ("emp explosion", Self::EmpExplosion { overload: false }),
            ("big emp explosion", Self::EmpExplosion { overload: true }),
        ]
        .into_iter()
        .find_map(|(name, kind)| (lookup(name) == Some(template)).then_some(kind))
    }

    /// Restrict replacements to cosmetic riders of real EMP shots. These
    /// particle archetypes may also appear as standalone level decorations.
    pub fn for_emp_attachment(
        template: i32,
        parent: i32,
        lookup: impl Fn(&str) -> Option<i32>,
    ) -> Option<Self> {
        if lookup("blue pulse") == Some(template)
            && matches!(
                Self::for_template(parent, &lookup),
                Some(Self::EmpExplosion { .. })
            )
        {
            return Some(Self::EmpLegacyPulse);
        }
        let overload = lookup("big emp shot") == Some(parent);
        if !overload && lookup("emp shot") != Some(parent) {
            return None;
        }
        [
            ("emp blue", Self::EmpCore { overload }),
            ("emp2", Self::EmpArcs { overload }),
            ("emp jet up", Self::EmpLegacyJet),
            ("emp jet down", Self::EmpLegacyJet),
            ("emp jet left", Self::EmpLegacyJet),
            ("emp jet right", Self::EmpLegacyJet),
            ("electric sparks", Self::EmpLegacyJet),
        ]
        .into_iter()
        .find_map(|(name, kind)| (lookup(name) == Some(template)).then_some(kind))
    }

    pub fn build(self, assets: &mut AssetCache) -> Option<ParticleEffect> {
        let sprite_names: &[&str] = match self {
            Self::BloodDropletsAndMist => &["NDbld", "NDsmk"],
            Self::BloodSpray => &["ND-bsp"],
            Self::EmpExplosion { .. } | Self::EmpLegacyPulse => &["NDsbll", "NDsmk", "NDsprk"],
            // Every EMP component checks the same complete asset set. If any
            // part is missing, retain the whole legacy projectile presentation
            // rather than hiding its mesh/jets around a partial replacement.
            Self::EmpCore { .. } | Self::EmpArcs { .. } | Self::EmpLegacyJet => {
                &["NDsbll", "NDarc", "NDsprk"]
            }
        };
        let frames: Vec<_> = sprite_names
            .iter()
            .map(|name| particle_frames(assets, name))
            .collect();
        if frames.iter().any(Vec::is_empty) {
            return None;
        }
        Some(ParticleEffect {
            layers: match self {
                Self::BloodDropletsAndMist | Self::BloodSpray => blood_layers(self, &frames),
                Self::EmpExplosion { overload } => emp_explosion_layers(overload, &frames),
                Self::EmpLegacyPulse => vec![],
                _ => emp_layers(self, &frames),
            },
            replaces_parent_model: matches!(self, Self::EmpCore { .. }),
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
        _ => unreachable!("blood recipe called for a non-blood effect"),
    }
}

fn emp_layers(kind: EnhancedEffect, frames: &[Vec<Rc<dyn TextureTrait>>]) -> Vec<ParticleSystem> {
    let scale = match kind {
        EnhancedEffect::EmpCore { overload } | EnhancedEffect::EmpArcs { overload } => {
            if overload {
                1.35
            } else {
                1.0
            }
        }
        EnhancedEffect::EmpLegacyJet => return vec![],
        _ => unreachable!("EMP recipe called for a non-EMP effect"),
    };
    let base = ParticleSystem::new()
        .with_velocity(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0))
        .with_launch_bounding_box(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0))
        .with_color(vec3(0.4, 0.65, 1.0))
        .with_alpha(0.9)
        .with_launch_time(Duration::ZERO);
    match kind {
        EnhancedEffect::EmpCore { .. } => vec![
            base.clone()
                .with_num_particles(1)
                .with_lifetime(0.9, 0.9)
                .with_particle_size(0.30 * scale, 0.30 * scale)
                .with_fade_time(0.0)
                .with_sprite_animation(frames[0].clone(), Duration::from_millis(90), true),
            base.with_num_particles(1)
                .with_lifetime(0.9, 0.9)
                .with_particle_size(0.75 * scale, 0.75 * scale)
                .with_alpha(0.18)
                .with_fade_time(0.0),
        ],
        EnhancedEffect::EmpArcs { .. } => vec![
            base.clone()
                .with_num_particles(3)
                .with_lifetime(0.25, 0.35)
                .with_particle_size(0.45 * scale, 0.55 * scale)
                .with_launch_time(Duration::from_millis(50))
                .with_fade_time(0.12)
                .with_sprite_animation(frames[1].clone(), Duration::from_millis(45), true),
            base.with_num_particles(8)
                .with_lifetime(0.18, 0.3)
                .with_particle_size(0.08 * scale, 0.12 * scale)
                .with_velocity(vec3(-0.15, -0.4, -0.4), vec3(0.15, 0.4, 0.4))
                .with_world_space(true)
                .with_launch_time(Duration::from_millis(30))
                .with_fade_time(0.18)
                .with_sprite_animation(frames[2].clone(), Duration::from_millis(35), true),
        ],
        _ => unreachable!("non-rendering EMP component handled above"),
    }
}

/// A brief expanding burst, followed by a slower cloud. The parent owns the
/// longest layer so its attached legacy pulse can be removed with it.
fn emp_explosion_layers(
    overload: bool,
    frames: &[Vec<Rc<dyn TextureTrait>>],
) -> Vec<ParticleSystem> {
    let scale = if overload { 1.4 } else { 1.0 };
    let base = ParticleSystem::new()
        .with_one_shot(true)
        .with_world_space(true)
        .with_launch_bounding_box(vec3(-0.06, -0.06, -0.06), vec3(0.06, 0.06, 0.06))
        .with_color(vec3(0.35, 0.65, 1.0));
    vec![
        base.clone()
            .with_num_particles(1)
            .with_velocity(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0))
            .with_particle_size(0.2 * scale, 0.2 * scale)
            .with_size_velocity(3.0 * scale)
            .with_lifetime(0.5, 0.5)
            .with_alpha(0.75)
            .with_fade_time(0.4)
            .with_sprite_animation(frames[0].clone(), Duration::from_millis(50), false),
        base.clone()
            .with_num_particles(5)
            .with_velocity(
                vec3(-0.25, -0.25, -0.25) * scale,
                vec3(0.25, 0.25, 0.25) * scale,
            )
            .with_particle_size(0.4 * scale, 0.6 * scale)
            .with_size_velocity(0.65 * scale)
            .with_lifetime(1.1, 1.4)
            .with_alpha(0.22)
            .with_fade_in_time(0.1)
            .with_fade_time(0.9)
            .with_sprite_animation(frames[1].clone(), Duration::from_millis(100), false),
        base.with_num_particles(12)
            .with_velocity(vec3(-1.4, -1.4, -1.4) * scale, vec3(1.4, 1.4, 1.4) * scale)
            .with_particle_size(0.1 * scale, 0.16 * scale)
            .with_lifetime(0.35, 0.65)
            .with_alpha(0.9)
            .with_fade_time(0.35)
            .with_sprite_animation(frames[2].clone(), Duration::from_millis(50), false),
    ]
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
            ..ParticleEffect::default()
        };
        let mut spray = ParticleEffect {
            layers: blood_layers(EnhancedEffect::BloodSpray, &empty_frames),
            ..ParticleEffect::default()
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

    #[test]
    fn emp_replacements_require_an_emp_parent_and_keep_overload_distinct() {
        let lookup = |name: &str| match name {
            "emp shot" => Some(-1),
            "big emp shot" => Some(-2),
            "emp blue" => Some(-3),
            "emp2" => Some(-4),
            "emp jet up" => Some(-5),
            "electric sparks" => Some(-6),
            _ => None,
        };
        assert_eq!(
            EnhancedEffect::for_emp_attachment(-3, -1, lookup),
            Some(EnhancedEffect::EmpCore { overload: false })
        );
        assert_eq!(
            EnhancedEffect::for_emp_attachment(-4, -2, lookup),
            Some(EnhancedEffect::EmpArcs { overload: true })
        );
        assert_eq!(
            EnhancedEffect::for_emp_attachment(-6, -2, lookup),
            Some(EnhancedEffect::EmpLegacyJet)
        );
        assert_eq!(EnhancedEffect::for_emp_attachment(-3, -99, lookup), None);
        assert_eq!(EnhancedEffect::for_emp_attachment(-99, -1, lookup), None);
        assert_eq!(EnhancedEffect::for_template(-3, lookup), None);
    }

    #[test]
    fn emp_layers_keep_emitting_for_the_projectile_lifetime() {
        let frames = vec![vec![], vec![], vec![]];
        for kind in [
            EnhancedEffect::EmpCore { overload: false },
            EnhancedEffect::EmpArcs { overload: true },
        ] {
            let mut effect = ParticleEffect {
                layers: emp_layers(kind, &frames),
                ..ParticleEffect::default()
            };
            for _ in 0..600 {
                effect.update(Duration::from_millis(16), Matrix4::identity());
            }
            assert!(!effect.is_done());
        }
        assert!(emp_layers(EnhancedEffect::EmpLegacyJet, &frames).is_empty());
    }

    #[test]
    fn emp_explosion_selection_and_bounded_lifetime() {
        let lookup = |name: &str| match name {
            "emp explosion" => Some(-10),
            "big emp explosion" => Some(-20),
            "blue pulse" => Some(-30),
            _ => None,
        };
        for (id, overload) in [(-10, false), (-20, true)] {
            assert_eq!(
                EnhancedEffect::for_template(id, lookup),
                Some(EnhancedEffect::EmpExplosion { overload })
            );
            assert_eq!(
                EnhancedEffect::for_emp_attachment(-30, id, lookup),
                Some(EnhancedEffect::EmpLegacyPulse)
            );
            let mut effect = ParticleEffect {
                layers: emp_explosion_layers(overload, &[vec![], vec![], vec![]]),
                ..Default::default()
            };
            effect.update(Duration::ZERO, Matrix4::identity());
            effect.update(Duration::from_millis(700), Matrix4::identity());
            assert!(
                !effect.is_done(),
                "cloud must outlive the initial spark burst"
            );
            effect.update(Duration::from_secs(1), Matrix4::identity());
            assert!(effect.is_done());
        }
        assert_eq!(EnhancedEffect::for_emp_attachment(-30, -99, lookup), None);
        assert_eq!(EnhancedEffect::for_template(-30, lookup), None);
    }

    #[test]
    fn missing_art_keeps_legacy_blood_and_all_emp_components() {
        let mut assets = AssetCache::new(
            String::new(),
            engine::assets::asset_paths::AssetPath::combine(vec![]),
        );
        for kind in [
            EnhancedEffect::BloodDropletsAndMist,
            EnhancedEffect::BloodSpray,
            EnhancedEffect::EmpCore { overload: false },
            EnhancedEffect::EmpArcs { overload: true },
            EnhancedEffect::EmpLegacyJet,
            EnhancedEffect::EmpExplosion { overload: false },
            EnhancedEffect::EmpExplosion { overload: true },
            EnhancedEffect::EmpLegacyPulse,
        ] {
            assert!(kind.build(&mut assets).is_none());
        }
    }
}
