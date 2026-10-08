//! Transient VR rapier activation. Inventory/hand ownership remains authoritative;
//! this only follows it with blade animation, an attached hum, and a draw pulse.
use std::collections::HashMap;

use cgmath::{InnerSpace, Matrix3, Matrix4, SquareMatrix, Vector3, vec3};
use dark::properties::PropLimbModel;
use engine::{audio::AudioHandle, scene::SceneObject};
use shipyard::{Component, EntityId, Get, View, World};

use crate::{Handedness, haptics::HapticPulse, scripts::Effect};

const EXTEND_SECONDS: f32 = 0.18;
const RETRACT_SECONDS: f32 = 0.14;
// patch_ext's ES_loop override is -2500 millibels. Keep the continuous near-ear
// source at that quiet level, fading with the blade instead of retriggering it.
const HUM_GAIN: f32 = 0.056234;

#[derive(Component, Clone, Copy)]
pub(crate) struct Blade(pub f32);

#[derive(Default)]
struct Activation {
    amount: f32,
    held: bool,
    hum: Option<AudioHandle>,
}

impl Activation {
    fn advance(&mut self, held: bool, dt: f32) -> bool {
        let drawn = held && !self.held;
        self.held = held;
        self.amount = if held {
            (self.amount + dt / EXTEND_SECONDS).min(1.0)
        } else {
            (self.amount - dt / RETRACT_SECONDS).max(0.0)
        };
        drawn
    }
}

#[derive(Default)]
pub(crate) struct RapierFeedback(HashMap<EntityId, Activation>);

pub(crate) fn is_rapier(world: &World, entity: EntityId) -> bool {
    world.borrow::<View<PropLimbModel>>().is_ok_and(|v| {
        v.get(entity)
            .is_ok_and(|m| m.0.eq_ignore_ascii_case("rapier_h"))
    })
}

pub(crate) fn amount(world: &World, entity: EntityId) -> f32 {
    world
        .borrow::<View<Blade>>()
        .ok()
        .and_then(|v| v.get(entity).ok().map(|b| b.0))
        .unwrap_or(0.0)
}

/// Until the visible blade has extended, its full-length contact body must not
/// deal damage or advertise a damaging swing. Other melee weapons are unchanged.
pub(crate) fn ready(world: &World, entity: EntityId) -> bool {
    !is_rapier(world, entity) || amount(world, entity) >= 1.0
}

impl RapierFeedback {
    pub fn update(
        &mut self,
        world: &mut World,
        hands: [Option<EntityId>; 2],
        alive: bool,
        dt: f32,
    ) -> Vec<Effect> {
        if dt <= 0.0 || !dt.is_finite() {
            return vec![];
        }
        let mut effects = vec![];
        let held = hands.map(|e| e.filter(|e| alive && is_rapier(world, *e)));
        for entity in held.into_iter().flatten() {
            self.0.entry(entity).or_default();
        }
        self.0.retain(|entity, state| {
            if !world
                .borrow::<shipyard::EntitiesView>()
                .unwrap()
                .is_alive(*entity)
            {
                if let Some(handle) = state.hum.take() {
                    effects.push(Effect::StopSound { handle });
                }
                return false;
            }
            let hand = held.iter().position(|e| *e == Some(*entity));
            if state.advance(hand.is_some(), dt) {
                effects.push(Effect::HandHaptic {
                    hand: if hand == Some(0) {
                        Handedness::Left
                    } else {
                        Handedness::Right
                    },
                    pulse: HapticPulse {
                        amplitude: 0.3,
                        duration_ms: 45,
                    },
                });
            }
            if state.held && state.hum.is_none() {
                let handle = AudioHandle::new();
                effects.push(Effect::PlaySpatialLoopingSound {
                    handle: handle.clone(),
                    name: "ES_loop".to_owned(),
                    source: *entity,
                    gain: HUM_GAIN * state.amount,
                });
                state.hum = Some(handle);
            }
            if let Some(handle) = &state.hum {
                effects.push(Effect::SetSoundGain {
                    handle: handle.clone(),
                    gain: HUM_GAIN * state.amount,
                });
            }
            if state.amount == 0.0 {
                if let Some(handle) = state.hum.take() {
                    effects.push(Effect::StopSound { handle });
                }
            }
            world.add_component(*entity, Blade(state.amount));
            state.held || state.amount > 0.0
        });
        effects
    }

    pub fn take_handles(&mut self) -> Vec<AudioHandle> {
        self.0.drain().filter_map(|(_, s)| s.hum).collect()
    }

    pub fn diagnostics(&self) -> serde_json::Value {
        serde_json::Value::Array(
            self.0
                .iter()
                .map(|(id, state)| {
                    serde_json::json!({
                        "entity_id": id.inner(), "extension": state.amount, "held": state.held,
                        "hum_handle": state.hum.as_ref().map(AudioHandle::id),
                    })
                })
                .collect(),
        )
    }
}

/// Authored Nightdive blade attachment, in loader coordinates (already /2.5).
/// The held PMNM is in bind space; the world LGMD blade grows along +Y.
/// These endpoints were measured from ND-rapier_b's root/tip rings, not the
/// weapon AABB (which includes the hilt/arm and would move the grip).
fn blade_transform(amount: f32, held: bool) -> Matrix4<f32> {
    let (base, axis) = if held {
        (
            vec3(-0.203623, 0.520707, -1.164806),
            vec3(-0.162133, 0.164634, -0.919503).normalize(),
        )
    } else {
        (vec3(0.0, -0.418516, -0.03825), Vector3::unit_y())
    };
    let outer = Matrix3::from_cols(axis * axis.x, axis * axis.y, axis * axis.z);
    let scale = Matrix4::from(Matrix3::identity() + outer * (amount.clamp(0.001, 1.0) - 1.0));
    Matrix4::from_translation(base) * scale * Matrix4::from_translation(-base)
}

pub(crate) fn animate(objects: &mut Vec<SceneObject>, extension: f32, held_model: bool) {
    let blade = |o: &SceneObject| {
        o.material_name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case("ND-rapier_b.psd"))
    };
    if extension <= 0.0 {
        objects.retain(|o| !blade(o));
        return;
    }
    if extension >= 1.0 {
        return;
    }
    let transform = blade_transform(extension, held_model);
    for object in objects.iter_mut().filter(|o| blade(o)) {
        if held_model {
            // Deform in PMNM bind space, before its existing pose/bind palette.
            for joint in &mut object.skinning_data {
                *joint = *joint * transform;
            }
        } else {
            object.local_transform = object.local_transform * transform;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Transform, point3};

    #[test]
    fn draw_reverses_retraction_without_reset_or_extra_draw_pulses() {
        let mut s = Activation::default();
        assert!(s.advance(true, 0.09));
        assert!((s.amount - 0.5).abs() < 0.001);
        assert!(!s.advance(true, 0.09));
        assert_eq!(s.amount, 1.0);
        assert!(!s.advance(false, 0.07));
        assert!((s.amount - 0.5).abs() < 0.001);
        assert!(s.advance(true, 0.09));
        assert_eq!(s.amount, 1.0);
        s.advance(false, 1.0);
        assert_eq!(s.amount, 0.0);
    }

    #[test]
    fn ownership_changes_do_not_duplicate_hums_and_deleted_sources_stop() {
        let mut world = World::new();
        let entity = world.add_entity((PropLimbModel("rapier_h".into()),));
        let mut feedback = RapierFeedback::default();
        let first = feedback.update(&mut world, [Some(entity), None], true, 0.1);
        assert!(
            first
                .iter()
                .any(|e| matches!(e, Effect::PlaySpatialLoopingSound { .. }))
        );
        assert!(!ready(&world, entity));
        let swap = feedback.update(&mut world, [None, Some(entity)], true, 0.1);
        assert!(ready(&world, entity));
        assert!(!swap.iter().any(|e| matches!(
            e,
            Effect::PlaySpatialLoopingSound { .. } | Effect::HandHaptic { .. }
        )));
        world.delete_entity(entity);
        let deleted = feedback.update(&mut world, [None, None], true, 0.1);
        assert!(matches!(deleted.as_slice(), [Effect::StopSound { .. }]));
        assert!(feedback.take_handles().is_empty());
    }

    #[test]
    fn extension_keeps_the_emitter_fixed_and_shortens_only_the_blade_axis() {
        let base = point3(0.0, -0.418516, -0.03825);
        let transform = blade_transform(0.5, false);
        assert!((transform.transform_point(base) - base).magnitude() < 1e-6);
        let tip = base + vec3(0.02, 1.0, 0.01);
        assert!(
            (transform.transform_point(tip) - (base + vec3(0.02, 0.5, 0.01))).magnitude() < 1e-6
        );
    }
}
