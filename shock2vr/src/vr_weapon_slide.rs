//! Cosmetic VR weapon actions: only the slide moves; the tracked grip stays put.
use dark::properties::PropGunState;
use shipyard::{Component, EntityId, Get, View, World};

use crate::{
    runtime_props::{RuntimePropGloveWeapon, RuntimePropObjectArticulation},
    scripts::Effect,
};

// Nightdive joint1: 0 -> -0.25 at frame 1, at 30 fps.
// LGMD parameter units, before the model's fitted scale.
const OPEN: f32 = -0.25;
const BACK_TIME: f32 = 1.0 / 30.0;
#[derive(Clone, Copy)]
struct SlideProfile {
    cycle_time: f32,
    locks_empty: bool,
}

const PISTOL: SlideProfile = SlideProfile {
    cycle_time: 7.0 / 30.0,
    locks_empty: true,
};

fn profile(model: &str) -> Option<SlideProfile> {
    match model.to_ascii_lowercase().as_str() {
        "atek_h" => Some(PISTOL),
        // Nightdive Assault Rifle/shoot returns joint1 at frame 8. This
        // external charging handle completes its cycle even on the last shot.
        "ar15_h" => Some(SlideProfile {
            cycle_time: 8.0 / 30.0,
            locks_empty: false,
        }),
        _ => None,
    }
}

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct WeaponSlide {
    elapsed: Option<f32>,
}

impl WeaponSlide {
    fn advance(&mut self, dt: f32, empty: bool, profile: SlideProfile) -> f32 {
        let hold_open = empty && profile.locks_empty;
        if let Some(elapsed) = &mut self.elapsed {
            if dt.is_finite() && dt > 0.0 {
                *elapsed += dt;
            }
            if *elapsed < BACK_TIME {
                return OPEN * (*elapsed / BACK_TIME);
            }
            if !hold_open && *elapsed < profile.cycle_time {
                return OPEN * (profile.cycle_time - *elapsed) / (profile.cycle_time - BACK_TIME);
            }
            self.elapsed = None;
        }
        if hold_open { OPEN } else { 0.0 }
    }
}

fn enabled_profile(world: &World, entity: EntityId) -> Option<SlideProfile> {
    if !crate::mission::mission_core::presentation_is_vr(world) {
        return None;
    }
    let profile =
        profile(&crate::scripts::internal_switch_held_model::get_raw_view_model(world, entity)?)?;
    world
        .borrow::<View<RuntimePropGloveWeapon>>()
        .ok()?
        .get(entity)
        .ok()?;
    let rigs = world.borrow::<View<RuntimePropObjectArticulation>>().ok()?;
    rigs.get(entity)
        .ok()?
        .0
        .joints
        .iter()
        .any(|joint| joint.parameter == 0 && joint.motion_type == 2)
        .then_some(profile)
}

/// Called only for an accepted shot, never a dry fire or a cooldown refusal.
pub(crate) fn fired(world: &mut World, entity: EntityId) {
    if enabled_profile(world, entity).is_some() {
        world.add_component(entity, WeaponSlide { elapsed: Some(0.0) });
    }
}

pub(crate) fn advance(
    world: &mut World,
    held: (Option<EntityId>, Option<EntityId>),
    dt: f32,
) -> Vec<Effect> {
    let mut effects = Vec::new();
    for entity in [held.0, held.1].into_iter().flatten() {
        let Some(profile) = enabled_profile(world, entity) else {
            continue;
        };
        let empty = world
            .borrow::<View<PropGunState>>()
            .is_ok_and(|v| v.get(entity).is_ok_and(|gun| gun.ammo <= 0));
        let mut slide = world
            .borrow::<View<WeaponSlide>>()
            .ok()
            .and_then(|v| v.get(entity).ok().copied())
            .unwrap_or_default();
        let value = slide.advance(dt, empty, profile);
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
    fn rifle_retracts_on_each_shot_and_returns_forward_even_when_empty() {
        let rifle = profile("AR15_H").expect("the AR15 must enable its slide");
        let mut slide = WeaponSlide { elapsed: Some(0.0) };
        assert_eq!(slide.advance(BACK_TIME, true, rifle), OPEN);
        assert!(
            slide.advance(6.0 / 30.0, true, rifle) < -0.001,
            "the AR15 returns at frame 8, later than the pistol"
        );
        assert_eq!(slide.advance(1.0 / 30.0, true, rifle), 0.0);
        assert_eq!(slide.advance(1.0, true, rifle), 0.0);
        assert_eq!(WeaponSlide::default().advance(0.0, true, rifle), 0.0);
    }

    #[test]
    fn accepted_shot_cycles_only_the_slide_and_pause_does_not_advance_it() {
        let mut slide = WeaponSlide { elapsed: Some(0.0) };
        assert_eq!(slide.advance(0.0, false, PISTOL), 0.0);
        assert!((slide.advance(BACK_TIME, false, PISTOL) - OPEN).abs() < 1e-6);
        assert_eq!(slide.advance(0.0, false, PISTOL), OPEN);
        let returning = slide.advance(0.05, false, PISTOL);
        assert!(returning > OPEN && returning < 0.0);
        assert_eq!(slide.advance(1.0, false, PISTOL), 0.0);
        assert!(slide.elapsed.is_none());
    }

    #[test]
    fn last_round_locks_back_until_ammo_is_loaded() {
        let mut slide = WeaponSlide { elapsed: Some(0.0) };
        assert_eq!(slide.advance(0.0, true, PISTOL), 0.0);
        assert_eq!(slide.advance(BACK_TIME, true, PISTOL), OPEN);
        assert_eq!(slide.advance(1.0, true, PISTOL), OPEN);
        assert_eq!(slide.advance(10.0, true, PISTOL), OPEN);
        assert_eq!(slide.advance(0.0, false, PISTOL), 0.0);
    }

    #[test]
    fn empty_equip_or_save_restore_needs_no_persisted_animation_state() {
        let mut slide = WeaponSlide::default();
        assert_eq!(slide.advance(0.0, true, PISTOL), OPEN);
        assert_eq!(slide.advance(0.0, false, PISTOL), 0.0);
    }
}
