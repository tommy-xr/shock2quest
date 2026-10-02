//! Cosmetic VR pistol action: only the slide moves; the tracked grip stays put.
use dark::properties::PropGunState;
use shipyard::{Component, EntityId, Get, View, World};

use crate::{
    runtime_props::{RuntimePropGloveWeapon, RuntimePropObjectArticulation},
    scripts::Effect,
};

// Nightdive Pistol/shoot joint1: 0 -> -0.25 at frame 1 -> 0 at frame 7,
// at 30 fps. These are LGMD parameter units, before the model's fitted scale.
const OPEN: f32 = -0.25;
const BACK_TIME: f32 = 1.0 / 30.0;
const CYCLE_TIME: f32 = 7.0 / 30.0;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct PistolSlide {
    elapsed: Option<f32>,
}

impl PistolSlide {
    fn advance(&mut self, dt: f32, empty: bool) -> f32 {
        if let Some(elapsed) = &mut self.elapsed {
            if dt.is_finite() && dt > 0.0 {
                *elapsed += dt;
            }
            if *elapsed < BACK_TIME {
                return OPEN * (*elapsed / BACK_TIME);
            }
            if !empty && *elapsed < CYCLE_TIME {
                return OPEN * (CYCLE_TIME - *elapsed) / (CYCLE_TIME - BACK_TIME);
            }
            self.elapsed = None;
        }
        if empty { OPEN } else { 0.0 }
    }
}

fn enabled(world: &World, entity: EntityId) -> bool {
    crate::mission::mission_core::presentation_is_vr(world)
        && crate::scripts::internal_switch_held_model::get_raw_view_model(world, entity)
            .is_some_and(|name| name.eq_ignore_ascii_case("atek_h"))
        && world
            .borrow::<View<RuntimePropGloveWeapon>>()
            .is_ok_and(|v| v.get(entity).is_ok())
        && world
            .borrow::<View<RuntimePropObjectArticulation>>()
            .is_ok_and(|v| {
                v.get(entity).is_ok_and(|rig| {
                    rig.0
                        .joints
                        .iter()
                        .any(|joint| joint.parameter == 0 && joint.motion_type == 2)
                })
            })
}

/// Called only for an accepted shot, never a dry fire or a cooldown refusal.
pub(crate) fn fired(world: &mut World, entity: EntityId) {
    if enabled(world, entity) {
        world.add_component(entity, PistolSlide { elapsed: Some(0.0) });
    }
}

pub(crate) fn advance(
    world: &mut World,
    held: (Option<EntityId>, Option<EntityId>),
    dt: f32,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    for entity in [held.0, held.1].into_iter().flatten() {
        if !enabled(world, entity) {
            continue;
        }
        let empty = world
            .borrow::<View<PropGunState>>()
            .is_ok_and(|v| v.get(entity).is_ok_and(|gun| gun.ammo <= 0));
        let mut slide = world
            .borrow::<View<PistolSlide>>()
            .ok()
            .and_then(|v| v.get(entity).ok().copied())
            .unwrap_or_default();
        let value = slide.advance(dt, empty);
        world.add_component(entity, slide);
        effects.push(Effect::SetObjectParameters {
            entity_id: entity,
            parameters: vec![(0, value)],
        });
    }
    effects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_shot_cycles_only_the_slide_and_pause_does_not_advance_it() {
        let mut slide = PistolSlide { elapsed: Some(0.0) };
        assert_eq!(slide.advance(0.0, false), 0.0);
        assert!((slide.advance(BACK_TIME, false) - OPEN).abs() < 1e-6);
        assert_eq!(slide.advance(0.0, false), OPEN);
        let returning = slide.advance(0.05, false);
        assert!(returning > OPEN && returning < 0.0);
        assert_eq!(slide.advance(1.0, false), 0.0);
        assert!(slide.elapsed.is_none());
    }

    #[test]
    fn last_round_locks_back_until_ammo_is_loaded() {
        let mut slide = PistolSlide { elapsed: Some(0.0) };
        assert_eq!(slide.advance(0.0, true), 0.0);
        assert_eq!(slide.advance(BACK_TIME, true), OPEN);
        assert_eq!(slide.advance(1.0, true), OPEN);
        assert_eq!(slide.advance(10.0, true), OPEN);
        assert_eq!(slide.advance(0.0, false), 0.0);
    }

    #[test]
    fn empty_equip_or_save_restore_needs_no_persisted_animation_state() {
        let mut slide = PistolSlide::default();
        assert_eq!(slide.advance(0.0, true), OPEN);
        assert_eq!(slide.advance(0.0, false), 0.0);
    }
}
