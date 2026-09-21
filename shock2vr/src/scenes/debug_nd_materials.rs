//! Walkable material/effects gallery. Bursts replay every three seconds so
//! both presentations can inspect the same samples without aiming a weapon.
//! Uses installed remaster art with independently authored preview tuning;
//! these samples do not replace campaign impact routing or material passes.

use std::{rc::Rc, time::Duration};

use cgmath::{Deg, Matrix4, Quaternion, Rotation3, Vector3, vec3};
use dark::importers::{FONT_IMPORTER, MODELS_IMPORTER, TEXTURE_IMPORTER};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::{ParticleSystem, SceneObject},
    texture::{TextureOptions, TextureTrait},
};
use shipyard::EntityId;

use crate::{
    GameOptions,
    game_scene::GameScene,
    input_context::InputContext,
    mission::{GlobalContext, SpawnLocation, mission_core::MissionCore},
    time::Time,
};

use super::debug_common::{
    DebugSceneBuildOptions, DebugSceneBuilder, DebugSceneHooks, boxes_to_geometry,
};

const PERIOD: f32 = 3.0;
const LABELS: [&str; 6] = [
    "METAL",
    "PLASTICRETE",
    "GLASS",
    "BLOOD",
    "GOO",
    "ELECTRICAL",
];

fn station(index: usize) -> Vector3<f32> {
    vec3(-4.0, 1.65, (2.5 - index as f32) * 2.1)
}

struct Exhibit {
    position: Vector3<f32>,
    recipe: ParticleSystem,
    live: ParticleSystem,
}

impl Exhibit {
    fn new(position: Vector3<f32>, recipe: ParticleSystem) -> Self {
        Self {
            position,
            live: recipe.clone(),
            recipe,
        }
    }
}

struct MaterialHooks {
    exhibits: Vec<Exhibit>,
    elapsed: f32,
}

impl DebugSceneHooks for MaterialHooks {
    fn before_update(
        &mut self,
        _core: &mut MissionCore,
        time: &Time,
        _input: &InputContext,
        _assets: &mut AssetCache,
        _options: &GameOptions,
    ) {
        self.elapsed += time.elapsed.as_secs_f32();
        if self.elapsed >= PERIOD {
            self.elapsed %= PERIOD;
            for exhibit in &mut self.exhibits {
                exhibit.live = exhibit.recipe.clone();
            }
        }
        for exhibit in &mut self.exhibits {
            exhibit
                .live
                .update(time.elapsed, Matrix4::from_translation(exhibit.position));
        }
    }

    fn after_render(
        &mut self,
        _core: &mut MissionCore,
        objects: &mut Vec<SceneObject>,
        _camera_position: &mut Vector3<f32>,
        _camera_rotation: &mut Quaternion<f32>,
        _assets: &mut AssetCache,
        _options: &GameOptions,
    ) {
        for exhibit in &self.exhibits {
            objects.extend(exhibit.live.render());
        }
    }
}

/// Particle sequences use zero-based, two-digit suffixes without the model
/// animation loader's underscore. Limit the probe and fall back to the base
/// sprite for single-frame art. All loads go through the normal asset cache.
fn particle_frames(assets: &mut AssetCache, name: &str) -> Vec<Rc<dyn TextureTrait>> {
    let options = TextureOptions {
        wrap: false,
        ..Default::default()
    };
    let mut frames: Vec<Rc<dyn TextureTrait>> = Vec::new();
    for frame in 0..64 {
        // Bitmap mounts register family-qualified basenames, not the full
        // archive path (the txt16 directory is already part of that mount).
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
            tracing::warn!("debug_nd_materials: missing particle art {name}; using glow disk");
        }
    }
    tracing::info!("debug_nd_materials: {name}: {} sprite frames", frames.len());
    frames
}

fn burst(
    frames: &[Rc<dyn TextureTrait>],
    count: usize,
    size: f32,
    lifetime: f32,
) -> ParticleSystem {
    ParticleSystem::new()
        .with_one_shot(true)
        .with_num_particles(count)
        .with_particle_size(size, size * 1.2)
        .with_lifetime(lifetime, lifetime)
        .with_launch_bounding_box(vec3(0.0, -0.035, -0.035), vec3(0.02, 0.035, 0.035))
        .with_velocity(vec3(0.3, -0.3, -0.3), vec3(0.7, 0.3, 0.3))
        .with_fade_time(lifetime * 0.6)
        .with_alpha(0.85)
        .with_sprite_animation(frames.to_vec(), Duration::from_millis(75), false)
}

pub fn create_debug_nd_materials_scene(
    global_context: &GlobalContext,
    game_options: &GameOptions,
    asset_cache: &mut AssetCache,
    audio_context: &mut AudioContext<EntityId, String>,
) -> Box<dyn GameScene> {
    let mut boxes = vec![(
        vec3(0.12, 0.14, 0.17),
        vec3(0.0, -0.5, 0.0),
        vec3(24.0, 1.0, 24.0),
    )];
    for i in 0..LABELS.len() {
        let p = station(i);
        let color = if i == 1 {
            vec3(0.36, 0.32, 0.25)
        } else {
            vec3(0.19, 0.23, 0.28)
        };
        boxes.push((color, vec3(p.x - 0.15, 1.55, p.z), vec3(0.2, 2.7, 1.9)));
    }
    let (objects, collider) = boxes_to_geometry(&boxes);
    let mut builder = DebugSceneBuilder::new("debug_nd_materials")
        .with_spawn_location(SpawnLocation::PositionRotation(
            vec3(7.5, 2.0, 0.0),
            Quaternion::from_angle_y(Deg(0.0)),
        ))
        .with_physics_geometry(collider);
    for object in objects {
        builder = builder.add_scene_object(object);
    }

    let font = asset_cache.get(&FONT_IMPORTER, "mainfont.fon");
    for (i, text) in LABELS.iter().enumerate() {
        let p = station(i);
        let mut label = SceneObject::world_space_text(text, font.clone(), 0.0);
        label.set_transform(
            Matrix4::from_translation(vec3(p.x + 0.02, 2.65, p.z))
                * Matrix4::from_angle_y(Deg(90.0))
                * Matrix4::from_nonuniform_scale(
                    0.14 * engine::measure_text_width(&**font, text, 1.0),
                    0.14,
                    1.0,
                )
                * Matrix4::from_angle_x(Deg(180.0)),
        );
        builder = builder.add_scene_object(label);
    }

    // Static inspection samples below the impact. Preserve model geometry and
    // local transforms; only position/scale the whole object on the panel. The
    // existing model importer supplies the base material (extra shine passes
    // are intentionally not approximated here).
    for (index, models) in [
        (0, vec!["ND-mtlhit0"]),
        (
            1,
            vec!["ND-pcrhit0", "ND-pcrhit1", "ND-pcrhit2", "ND-pcrhit3"],
        ),
    ] {
        for (sample, name) in models.iter().enumerate() {
            if let Some(model) = asset_cache.get_opt(&MODELS_IMPORTER, &format!("{name}.bin")) {
                let p = station(index);
                let offset = (sample as f32 - (models.len() - 1) as f32 / 2.0) * 0.38;
                let transform = Matrix4::from_translation(vec3(p.x - 0.04, 0.75, p.z + offset))
                    * Matrix4::from_angle_y(Deg(180.0))
                    * Matrix4::from_scale(1.8);
                for mut object in model.clone_scene_objects() {
                    object.set_transform(transform * object.get_transform());
                    object.set_depth_bias(true);
                    builder = builder.add_scene_object(object);
                }
            } else {
                tracing::warn!("debug_nd_materials: missing decal model {name}");
            }
        }
    }

    let smoke = particle_frames(asset_cache, "NDsmk");
    let ember = particle_frames(asset_cache, "ND-ember");
    let chips = particle_frames(asset_cache, "NDdbr");
    let glass = particle_frames(asset_cache, "NDgsa");
    let blood = particle_frames(asset_cache, "ND-bsp");
    let droplets = particle_frames(asset_cache, "NDbld");
    let arcs = particle_frames(asset_cache, "NDarc");
    let core = particle_frames(asset_cache, "NDsbll");
    let mut exhibits = Vec::new();

    // Metal: compact fast sparks plus a faint puff.
    exhibits.push(Exhibit::new(
        station(0),
        burst(&ember, 10, 0.13, 0.65)
            .with_velocity(vec3(0.5, -0.5, -1.0), vec3(1.6, 1.2, 1.0))
            .with_acceleration(vec3(0.0, -2.8, 0.0))
            .with_size_velocity(-0.16)
            .with_color(vec3(1.0, 0.7, 0.3)),
    ));
    let dust = burst(&smoke, 5, 0.22, 1.5)
        .with_size_velocity(0.28)
        .with_fade_in_time(0.12)
        .with_alpha(0.3)
        .with_color(vec3(0.7, 0.65, 0.55));
    exhibits.push(Exhibit::new(station(0), dust.clone().with_alpha(0.12)));
    exhibits.push(Exhibit::new(station(1), dust));
    exhibits.push(Exhibit::new(
        station(1),
        burst(&chips, 9, 0.10, 0.9)
            .with_velocity(vec3(0.4, 0.2, -0.7), vec3(1.0, 1.1, 0.7))
            .with_acceleration(vec3(0.0, -3.0, 0.0)),
    ));
    exhibits.push(Exhibit::new(
        station(2),
        burst(&glass, 12, 0.15, 1.0)
            .with_velocity(vec3(0.3, 0.2, -0.8), vec3(1.2, 1.3, 0.8))
            .with_acceleration(vec3(0.0, -3.0, 0.0)),
    ));
    for (index, color) in [(3, vec3(0.65, 0.08, 0.06)), (4, vec3(0.28, 0.7, 0.13))] {
        exhibits.push(Exhibit::new(
            station(index),
            burst(&blood, 3, 0.40, 0.8)
                .with_color(color)
                .with_size_velocity(0.18),
        ));
        exhibits.push(Exhibit::new(
            station(index),
            burst(&droplets, 8, 0.10, 0.95)
                .with_color(color)
                .with_acceleration(vec3(0.0, -2.5, 0.0)),
        ));
    }
    let electrical_position = station(5) + vec3(0.6, 0.0, 0.0);
    let stationary = burst(&core, 1, 0.45, 2.5)
        .with_velocity(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0))
        .with_launch_bounding_box(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0))
        .with_sprite_animation(core, Duration::from_millis(90), true)
        .with_color(vec3(0.4, 0.65, 1.0))
        .with_fade_in_time(0.15);
    exhibits.push(Exhibit::new(electrical_position, stationary));
    exhibits.push(Exhibit::new(
        electrical_position,
        burst(&arcs, 4, 0.5, 2.3)
            .with_sprite_animation(arcs.clone(), Duration::from_millis(65), true)
            .with_velocity(vec3(-0.06, -0.06, -0.06), vec3(0.06, 0.06, 0.06))
            .with_color(vec3(0.4, 0.65, 1.0)),
    ));
    exhibits.push(Exhibit::new(
        electrical_position,
        burst(&[], 1, 1.1, 2.5)
            .with_velocity(vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0))
            .with_color(vec3(0.15, 0.3, 0.8))
            .with_alpha(0.2)
            .with_fade_in_time(0.15),
    ));

    builder.build_with_hooks(
        DebugSceneBuildOptions {
            global_context,
            game_options,
            asset_cache,
            audio_context,
        },
        MaterialHooks {
            exhibits,
            elapsed: 0.0,
        },
    )
}
