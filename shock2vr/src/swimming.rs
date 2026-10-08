//! Player air economy and authored water feedback. Breathing uses the head,
//! independently of the body-level test that selects swimming locomotion.
use cgmath::{InnerSpace, Matrix4, Vector3, vec2, vec3};
use engine::{
    audio::AudioHandle,
    scene::{ParticleSystem, SceneObject},
};
use serde::{Deserialize, Serialize};
use shipyard::{EntityId, Unique, UniqueView, UniqueViewMut, World};
use std::time::Duration;

use crate::{quest_info::QuestInfo, scripts::Effect};

/// Stored with the player's quest state, including partial drowning intervals.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Breath {
    used_seconds: f64,
    until_damage: f64,
    warned: bool,
}

pub fn capacity(endurance: i32) -> f64 {
    60.0 + 10.0 * endurance.max(0) as f64
}

impl Breath {
    fn tick(&mut self, dt: f64, submerged: bool, maximum: f64) -> (bool, i32) {
        let mut damage = 0;
        if submerged {
            let exhausted_time = (self.used_seconds + dt - maximum).max(0.0).min(dt);
            self.used_seconds = (self.used_seconds + dt).min(maximum);
            self.until_damage -= exhausted_time;
            while self.until_damage < 0.0 {
                damage += 3;
                self.until_damage += 3.0;
            }
        } else {
            self.used_seconds = (self.used_seconds - dt * 5.0).max(0.0);
            if self.used_seconds == 0.0 {
                self.warned = false;
                self.until_damage = 0.0;
            }
        }
        let warning = submerged && maximum - self.used_seconds <= 30.0 && !self.warned;
        self.warned |= warning;
        (warning, damage)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Unique)]
pub struct WaterStatus {
    pub submerged: bool,
    pub remaining_seconds: f64,
    pub maximum_seconds: f64,
    pub tint: f32,
}
impl WaterStatus {
    pub fn from_world(world: &World) -> Self {
        world
            .borrow::<UniqueView<Self>>()
            .map(|s| *s)
            .unwrap_or_default()
    }
}

#[derive(Default)]
pub(crate) struct WaterFeedback {
    medium: Option<u8>,
    ambience: Option<AudioHandle>,
    distance: f32,
    tint: f32,
    bubbles: Option<ParticleSystem>,
}

pub(crate) struct WaterFrame {
    pub elapsed: Duration,
    pub head: Vector3<f32>,
    pub forward: Vector3<f32>,
    /// Dry / feet / body / head, matching retail's MediaLevel tags.
    pub medium: u8,
    pub travel: Vector3<f32>,
    pub swimming_input: bool,
    pub alive: bool,
    pub player: EntityId,
}

fn water_sound(event: &str, level: u8, direction: Option<&str>, point: Vector3<f32>) -> Effect {
    let mut tags = vec![
        ("event", event),
        ("creaturetype", "player"),
        (
            "medialevel",
            match level {
                1 => "foot",
                2 => "body",
                _ => "head",
            },
        ),
    ];
    if let Some(direction) = direction {
        tags.push(("medtransdir", direction));
    }
    Effect::PlayEnvironmentalSound {
        audio_handle: AudioHandle::new(),
        query: dark::EnvSoundQuery::from_tag_values(tags),
        position: point,
    }
}

impl WaterFeedback {
    pub fn update(&mut self, world: &World, frame: WaterFrame) -> Vec<Effect> {
        if frame.elapsed.is_zero() {
            return Vec::new();
        }
        let mut effects = Vec::new();
        let submerged = frame.medium == 3;
        let previous = self.medium.replace(frame.medium);
        if previous != Some(frame.medium) {
            self.distance = 0.0;
            if submerged {
                effects.push(water_sound("mediatrans", 3, Some("enter"), frame.head));
                let handle = AudioHandle::new();
                effects.push(Effect::PlayLoopingSound {
                    handle: handle.clone(),
                    name: "underwater".into(),
                    source: None,
                });
                self.ambience = Some(handle);
            } else if previous == Some(3) {
                if let Some(handle) = self.ambience.take() {
                    effects.push(Effect::StopSound { handle });
                }
                effects.push(water_sound("mediatrans", 3, Some("exit"), frame.head));
            }
            if let Some(previous) = previous {
                // Retail gates body splashes on crossing at >5 Dark units/s.
                let vertical_speed = frame.travel.y / frame.elapsed.as_secs_f32();
                if previous < 2 && frame.medium >= 2 && vertical_speed < -5.0 / dark::SCALE_FACTOR {
                    effects.push(water_sound("mediatrans", 2, Some("enter"), frame.head));
                } else if previous >= 2
                    && frame.medium < 2
                    && vertical_speed > 5.0 / dark::SCALE_FACTOR
                {
                    effects.push(water_sound("mediatrans", 2, Some("exit"), frame.head));
                }
            }
        }
        if frame.medium > 0 && frame.swimming_input && frame.alive {
            self.distance += frame.travel.magnitude();
            let stride = if frame.medium == 1 { 1.0 } else { 1.8 };
            if self.distance >= stride {
                self.distance %= stride;
                effects.push(water_sound("footstep", frame.medium, None, frame.head));
            }
        }
        let maximum = capacity(crate::implants::effective_stats(world).map_or(1, |s| s.endurance));
        let remaining = {
            let mut quests = world.borrow::<UniqueViewMut<QuestInfo>>().unwrap();
            if frame.alive {
                let (warning, damage) =
                    quests
                        .breath
                        .tick(frame.elapsed.as_secs_f64(), submerged, maximum);
                if warning {
                    effects.push(Effect::ShowMessage {
                        text: "OXYGEN LOW - surface for air".into(),
                    });
                }
                if damage > 0 && !crate::dev_params::get_bool(crate::dev_params::CHEAT) {
                    // The central HP applier supplies hurt audio, hit feedback and death.
                    effects.push(Effect::AdjustHitPoints {
                        entity_id: frame.player,
                        delta: -damage,
                    });
                }
            }
            (maximum - quests.breath.used_seconds).max(0.0)
        };
        let target = if submerged { 0.16 } else { 0.0 };
        self.tint += (target - self.tint) * (frame.elapsed.as_secs_f32() * 5.0).min(1.0);
        *world.borrow::<UniqueViewMut<WaterStatus>>().unwrap() = WaterStatus {
            submerged,
            remaining_seconds: remaining,
            maximum_seconds: maximum,
            tint: self.tint,
        };
        if submerged && frame.alive {
            let bubbles = self.bubbles.get_or_insert_with(bubbles);
            let horizontal = vec3(frame.forward.x, 0.0, frame.forward.z);
            let forward = if horizontal.magnitude2() > 0.0001 {
                horizontal.normalize()
            } else {
                vec3(0.0, 0.0, -1.0)
            };
            let side = forward.cross(vec3(0.0, 1.0, 0.0));
            bubbles.update(
                frame.elapsed,
                Matrix4::from_translation(
                    frame.head + forward * 0.65 + side * 0.32 - vec3(0.0, 0.25, 0.0),
                ),
            );
        } else {
            self.bubbles = None;
        }
        effects
    }

    pub fn take_ambience(&mut self) -> Option<AudioHandle> {
        self.ambience.take()
    }

    pub fn render(&self) -> Vec<SceneObject> {
        self.bubbles
            .as_ref()
            .map_or_else(Vec::new, ParticleSystem::render)
    }
}

fn bubbles() -> ParticleSystem {
    // A translucent rim, not the default luminous particle disk.
    let size = 32;
    let mut bytes = vec![0; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let r = vec2(x as f32 - 15.5, y as f32 - 15.5).magnitude() / 15.5;
            let a = (1.0 - (r - 0.78).abs() / 0.12).clamp(0.0, 1.0);
            let i = (x + y * size) * 4;
            bytes[i..i + 4].copy_from_slice(&[190, 225, 235, (a * 170.0) as u8]);
        }
    }
    let texture = engine::texture::init_from_memory(engine::texture_format::RawTextureData {
        bytes,
        width: size as u32,
        height: size as u32,
        format: engine::texture_format::PixelFormat::RGBA,
    });
    ParticleSystem::new()
        .with_sprite_texture(std::rc::Rc::new(texture))
        .with_world_space(true)
        .with_num_particles(12)
        .with_alpha(0.45)
        .with_particle_size(0.035, 0.075)
        .with_lifetime(0.6, 1.1)
        .with_fade_time(0.35)
        .with_fade_in_time(0.12)
        .with_launch_time(Duration::from_millis(260))
        .with_launch_bounding_box(vec3(-0.08, -0.04, -0.08), vec3(0.08, 0.04, 0.08))
        .with_velocity(vec3(-0.04, 0.35, -0.04), vec3(0.04, 0.55, 0.04))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endurance_capacity_warning_damage_and_recovery() {
        assert_eq!(capacity(1), 70.0);
        assert_eq!(capacity(6), 120.0);
        let mut breath = Breath::default();
        assert_eq!(breath.tick(39.0, true, 70.0), (false, 0));
        assert_eq!(breath.tick(1.0, true, 70.0), (true, 0));
        assert_eq!(breath.tick(30.0, true, 70.0), (false, 0));
        assert_eq!(breath.tick(0.1, true, 70.0), (false, 3));
        assert_eq!(breath.tick(2.0, true, 70.0), (false, 0));
        let saved = serde_json::to_string(&breath).unwrap();
        let mut loaded: Breath = serde_json::from_str(&saved).unwrap();
        assert_eq!(loaded.tick(1.0, true, 70.0), (false, 3));
        assert_eq!(loaded.tick(14.0, false, 70.0), (false, 0));
        assert_eq!(loaded.used_seconds, 0.0);
        assert_eq!(loaded.tick(40.0, true, 70.0), (true, 0));
    }

    #[test]
    fn brief_surface_does_not_restart_the_drowning_hit_interval() {
        let mut breath = Breath::default();
        assert_eq!(breath.tick(70.1, true, 70.0).1, 3);
        breath.tick(0.02, false, 70.0);
        assert_eq!(breath.tick(0.2, true, 70.0).1, 0);
        assert_eq!(breath.tick(3.0, true, 70.0).1, 3);
    }
}
