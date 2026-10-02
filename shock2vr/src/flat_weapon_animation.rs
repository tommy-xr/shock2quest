//! Timeline driver for flat weapons. Model parts still use the shared LGMD
//! scalar-parameter interface, which a VR gesture can drive independently.
use cgmath::{Deg, Quaternion, Rotation3, Vector3, vec3};
use dark::{SCALE_FACTOR, properties::PropPlayerGun, weapon_animation::WeaponAnimations};
use engine::assets::{asset_cache::AssetCache, text_importer::TEXT_IMPORTER};
use shipyard::{Component, EntityId, Get, View, World};

use crate::{runtime_props::RuntimePropReloading, scripts::Effect};

#[derive(Component, Clone, Copy, serde::Serialize)]
pub(crate) struct FlatWeaponPose {
    pub translation: Vector3<f32>,
    pub rotation: Quaternion<f32>,
    pub delayed_ejection: bool,
    /// LGMD parameters, consumed by the first-person mesh (which differs from
    /// the entity's world model). VR can drive the same scalar interface.
    pub parameters: [(i32, f32); 3],
    pub clip: Option<&'static str>,
    pub frame: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{InnerSpace, Rotation};

    fn fixture() -> (FlatWeaponAnimator, World, EntityId) {
        let mut world = World::new();
        let entity = world.add_entity((PropPlayerGun {
            flags: 0,
            hand_model: "sg_h".into(),
            icon_file: String::new(),
            model_offset: vec3(0.0, 0.0, 0.0),
            fire_offset: vec3(0.0, 0.0, 0.0),
            heading: 0,
            reload_pitch: 0,
            reload_rate: 0,
            gun_type: 0,
        },));
        let clips = dark::weapon_animation::parse(
            r#"
            g_weaponAnimations["Shotgun"]["shoot"] <- ND.PointRigAnimation("shoot", 10, 10, {
                "joint1": [ { "frame": 2, "pos": [-0.3, 0, 0], "events": ["eject"] } ]
            });
        "#,
        );
        let mut animator = FlatWeaponAnimator {
            clips,
            weapon: None,
            playback: None,
            transition: None,
            ejections: Vec::new(),
        };
        animator.advance(&mut world, Some(entity), 0.0);
        (animator, world, entity)
    }

    fn ejections(effects: &[Effect]) -> usize {
        effects
            .iter()
            .filter(|effect| matches!(effect, Effect::EjectWeaponCasings { .. }))
            .count()
    }

    #[test]
    fn restarting_a_clip_does_not_lose_an_earlier_shots_ejection() {
        let (mut animator, mut world, entity) = fixture();
        animator.fired(&world, entity);
        assert_eq!(
            ejections(&animator.advance(&mut world, Some(entity), 0.1)),
            0
        );
        animator.fired(&world, entity);
        assert_eq!(
            ejections(&animator.advance(&mut world, Some(entity), 0.11)),
            1
        );
        assert_eq!(
            ejections(&animator.advance(&mut world, Some(entity), 0.11)),
            1
        );
        assert_eq!(
            ejections(&animator.advance(&mut world, Some(entity), 1.0)),
            0
        );
    }

    #[test]
    fn holstering_cancels_pending_ejection_and_clears_the_pose() {
        let (mut animator, mut world, entity) = fixture();
        animator.fired(&world, entity);
        assert_eq!(ejections(&animator.advance(&mut world, None, 1.0)), 0);
        assert!(
            !world
                .borrow::<View<FlatWeaponPose>>()
                .unwrap()
                .contains(entity)
        );
        animator.fired(&world, entity);
        assert_eq!(
            ejections(&animator.advance(&mut world, Some(entity), 1.0)),
            0
        );
    }

    #[test]
    fn reload_uses_gameplay_progress_without_losing_the_shots_ejection() {
        let (mut animator, mut world, entity) = fixture();
        let reload = dark::weapon_animation::parse(
            r#"
            g_weaponAnimations["Shotgun"]["reload"] <- ND.PointRigAnimation("reload", 30, 100, {
                "joint1": [ { "frame": 0, "pos": [0,0,0] }, { "frame": 100, "pos": [-1,0,0] } ]
            });
        "#,
        );
        animator
            .clips
            .by_category
            .get_mut("Shotgun")
            .unwrap()
            .insert(
                "reload".into(),
                reload.get("Shotgun", "reload").unwrap().clone(),
            );
        world.add_component(
            entity,
            RuntimePropReloading {
                elapsed: 1.0,
                down: 0.5,
                hold: 1.0,
                up: 0.5,
                peak_deg: 45.0,
            },
        );
        animator.fired(&world, entity);
        assert_eq!(
            ejections(&animator.advance(&mut world, Some(entity), 0.1)),
            0
        );
        {
            let poses = world.borrow::<View<FlatWeaponPose>>().unwrap();
            let pose = poses.get(entity).unwrap();
            assert_eq!(pose.clip, Some("reload"));
            assert!((pose.frame - 50.0).abs() < 1e-4);
            assert!((pose.parameters[0].1 + 0.75).abs() < 1e-4);
        }
        assert_eq!(
            ejections(&animator.advance(&mut world, Some(entity), 0.11)),
            1
        );
        assert_eq!(
            ejections(&animator.advance(&mut world, Some(entity), 1.0)),
            0
        );
        world.remove::<(RuntimePropReloading,)>(entity);
        animator.advance(&mut world, Some(entity), 0.0);
        let poses = world.borrow::<View<FlatWeaponPose>>().unwrap();
        let pose = poses.get(entity).unwrap();
        assert_eq!(pose.clip, None);
        assert!(pose.parameters.iter().all(|(_, value)| *value == 0.0));
    }

    #[test]
    fn firing_during_equip_blends_from_the_current_pose_without_delaying_ejection() {
        let (mut animator, mut world, entity) = fixture();
        let mut raise = animator.clips.get("Shotgun", "shoot").unwrap().clone();
        raise.tracks.push(dark::weapon_animation::Track {
            joint: "gunPoint".into(),
            keys: vec![dark::weapon_animation::Keyframe {
                frame: 0.0,
                pos: vec3(0.0, 0.0, -1.5),
                rot: vec3(0.0, 30.0, 0.0),
                events: Vec::new(),
            }],
        });
        animator
            .clips
            .by_category
            .get_mut("Shotgun")
            .unwrap()
            .insert("raise".into(), raise);
        animator.playback = Some(Playback {
            clip: "raise",
            elapsed: 0.0,
        });
        animator.advance(&mut world, Some(entity), 0.0);
        let before = *world
            .borrow::<View<FlatWeaponPose>>()
            .unwrap()
            .get(entity)
            .unwrap();
        animator.fired(&world, entity);
        animator.advance(&mut world, Some(entity), 0.0);
        {
            let poses = world.borrow::<View<FlatWeaponPose>>().unwrap();
            let after = poses.get(entity).unwrap();
            assert_eq!(after.clip, Some("shoot"));
            assert_eq!(after.translation, before.translation);
            assert!((after.rotation - before.rotation).magnitude() < 1e-5);
        }
        assert_eq!(
            ejections(&animator.advance(&mut world, Some(entity), 0.21)),
            1
        );
        assert_eq!(
            world
                .borrow::<View<FlatWeaponPose>>()
                .unwrap()
                .get(entity)
                .unwrap()
                .translation,
            vec3(0.0, 0.0, 0.0)
        );
        assert!(animator.transition.is_none());
    }

    #[test]
    fn point_rig_angles_are_heading_pitch_bank() {
        for (angles, axis, expected) in [
            (vec3(90.0, 0.0, 0.0), Vector3::unit_x(), -Vector3::unit_z()),
            (vec3(0.0, 90.0, 0.0), Vector3::unit_x(), Vector3::unit_y()),
            (vec3(0.0, 0.0, 90.0), Vector3::unit_y(), -Vector3::unit_z()),
        ] {
            let (mut animator, mut world, entity) = fixture();
            animator
                .clips
                .by_category
                .get_mut("Shotgun")
                .unwrap()
                .get_mut("shoot")
                .unwrap()
                .tracks
                .push(dark::weapon_animation::Track {
                    joint: "gunPoint".into(),
                    keys: vec![dark::weapon_animation::Keyframe {
                        frame: 0.0,
                        pos: vec3(0.0, 0.0, 0.0),
                        rot: angles,
                        events: Vec::new(),
                    }],
                });
            animator.fired(&world, entity);
            animator.advance(&mut world, Some(entity), 0.0);
            let poses = world.borrow::<View<FlatWeaponPose>>().unwrap();
            assert!(
                (poses.get(entity).unwrap().rotation.rotate_vector(axis) - expected).magnitude()
                    < 1e-5
            );
        }
    }
}

struct Playback {
    clip: &'static str,
    elapsed: f32,
}

pub(crate) struct FlatWeaponAnimator {
    clips: WeaponAnimations,
    weapon: Option<(EntityId, &'static str)>,
    playback: Option<Playback>,
    /// Briefly blend out of an interrupted draw, without delaying gameplay.
    transition: Option<(FlatWeaponPose, f32)>,
    // Independent of playback restart/reload: each accepted shot owns its
    // ejection, even when another gesture interrupts the visible pump stroke.
    ejections: Vec<f32>,
}

impl FlatWeaponAnimator {
    pub fn load(assets: &mut AssetCache, enabled: bool) -> Self {
        let clips = if enabled {
            assets
                .get_opt(&TEXT_IMPORTER, "sq_scripts/animations_weapons.nut")
                .map(|source| dark::weapon_animation::parse(&source))
                .unwrap_or_default()
        } else {
            WeaponAnimations::default()
        };
        Self {
            clips,
            weapon: None,
            playback: None,
            transition: None,
            ejections: Vec::new(),
        }
    }

    fn ejection_delay(&self, category: &str) -> Option<f32> {
        let clip = self.clips.get(category, "shoot")?;
        clip.tracks
            .iter()
            .flat_map(|t| &t.keys)
            .filter(|key| key.events.iter().any(|e| e == "eject"))
            .map(|key| key.frame / clip.fps)
            .filter(|delay| delay.is_finite() && *delay > 0.0)
            .min_by(f32::total_cmp)
    }

    /// Called only for an accepted shot, never a trigger edge or a dry fire.
    pub fn fired(&mut self, world: &World, entity: EntityId) {
        if let Some((weapon, category)) = self.weapon {
            if weapon == entity {
                if self.playback.as_ref().is_some_and(|p| p.clip == "raise") {
                    self.transition = world
                        .borrow::<View<FlatWeaponPose>>()
                        .ok()
                        .and_then(|poses| poses.get(entity).ok().copied())
                        .map(|pose| (pose, 0.0));
                }
                self.playback = Some(Playback {
                    clip: "shoot",
                    elapsed: 0.0,
                });
                if let Some(delay) = self.ejection_delay(category) {
                    self.ejections.push(delay);
                }
            }
        }
    }

    pub fn advance(
        &mut self,
        world: &mut World,
        wielded: Option<EntityId>,
        dt: f32,
    ) -> Vec<Effect> {
        let selected = wielded.and_then(|entity| {
            let guns = world.borrow::<View<PropPlayerGun>>().ok()?;
            let gun = guns.get(entity).ok()?;
            let category = match gun.hand_model.to_ascii_lowercase().as_str() {
                "sg_h" => "Shotgun",
                "atek_h" => "Pistol",
                _ => return None,
            };
            self.clips.get(category, "shoot")?;
            Some((entity, category))
        });
        let mut effects = Vec::new();
        if selected != self.weapon {
            if let Some((entity, _)) = self.weapon {
                world.remove::<(FlatWeaponPose,)>(entity);
            }
            self.weapon = selected;
            self.playback = selected.and_then(|(_, category)| {
                self.clips.get(category, "raise").map(|_| Playback {
                    clip: "raise",
                    elapsed: 0.0,
                })
            });
            self.ejections.clear();
            self.transition = None;
        }
        let Some((entity, category)) = self.weapon else {
            return effects;
        };
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let reload = world
            .borrow::<View<RuntimePropReloading>>()
            .ok()
            .and_then(|reloads| reloads.get(entity).ok().copied());
        if let Some((reload, clip)) = reload.zip(self.clips.get(category, "reload")) {
            // Gameplay owns the duration and ammo transfer. Retiming the art
            // keeps the fire gate and visible reload on the same clock.
            self.playback = Some(Playback {
                clip: "reload",
                elapsed: reload.progress() * clip.duration_seconds(),
            });
        } else if self.playback.as_ref().is_some_and(|p| p.clip == "reload") {
            self.playback = None;
        }
        self.ejections.retain_mut(|remaining| {
            *remaining -= dt;
            if *remaining <= 0.0 {
                effects.push(Effect::EjectWeaponCasings { entity_id: entity });
                false
            } else {
                true
            }
        });
        if let Some(playback) = &mut self.playback {
            if playback.clip != "reload" {
                playback.elapsed += dt;
            }
        }
        let sample = self.playback.as_ref().and_then(|playback| {
            let clip = self.clips.get(category, playback.clip)?;
            Some((clip, playback.elapsed * clip.fps))
        });
        let (pos, rot) = sample
            .and_then(|(clip, frame)| clip.sample("gunPoint", frame))
            .unwrap_or((vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0)));
        let rot = if self.playback.as_ref().is_some_and(|p| p.clip == "shoot")
            && crate::weapon_recoil::still_hand_active(world)
        {
            vec3(0.0, 0.0, 0.0)
        } else {
            rot
        };
        let parameters = std::array::from_fn(|index| {
            let value = sample
                .and_then(|(clip, frame)| clip.sample(&format!("joint{}", index + 1), frame))
                .map(|(pos, _)| pos.x)
                .unwrap_or(0.0);
            (index as i32, value)
        });
        // PointRig's rotation tuple is (heading, pitch, bank), not XYZ Euler
        // axes. Dark model axes (x,y,z) become (-x,z,y), so heading rotates
        // around +Y, pitch around +Z, and bank around -X in model space.
        let mut pose = FlatWeaponPose {
            translation: vec3(-pos.x, pos.z, pos.y) / SCALE_FACTOR,
            rotation: Quaternion::from_angle_y(Deg(rot.x))
                * Quaternion::from_angle_z(Deg(rot.y))
                * Quaternion::from_angle_x(Deg(-rot.z)),
            delayed_ejection: self.ejection_delay(category).is_some(),
            parameters,
            clip: self.playback.as_ref().map(|playback| playback.clip),
            frame: sample.map(|(_, frame)| frame).unwrap_or(0.0),
        };
        if let Some((from, elapsed)) = &mut self.transition {
            *elapsed += dt;
            let t = (*elapsed / 0.12).clamp(0.0, 1.0);
            let t = t * (2.0 - t);
            pose.translation = from.translation + (pose.translation - from.translation) * t;
            pose.rotation = from.rotation.slerp(pose.rotation, t);
            for ((_, value), (_, old)) in pose.parameters.iter_mut().zip(from.parameters) {
                *value = old + (*value - old) * t;
            }
            if t >= 1.0 {
                self.transition = None;
            }
        }
        world.add_component(entity, pose);
        if sample.is_some_and(|(clip, frame)| frame >= clip.length) {
            self.playback = None;
        }
        effects
    }
}
