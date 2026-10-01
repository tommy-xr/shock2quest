//! Cosmetic VR weapon actions: only mechanical parts move; the tracked grip stays put.
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

#[derive(Clone, Copy)]
enum ActionProfile {
    Slide(SlideProfile),
    Fusion,
}

fn profile(model: &str) -> Option<ActionProfile> {
    match model.to_ascii_lowercase().as_str() {
        "atek_h" => Some(ActionProfile::Slide(PISTOL)),
        // Nightdive Assault Rifle/shoot returns joint1 at frame 8. This
        // external charging handle completes its cycle even on the last shot.
        "ar15_h" => Some(ActionProfile::Slide(SlideProfile {
            cycle_time: 8.0 / 30.0,
            locks_empty: false,
        })),
        "fsn_h" => Some(ActionProfile::Fusion),
        _ => None,
    }
}

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct WeaponAction {
    elapsed: Option<f32>,
}

impl WeaponAction {
    fn parameters(&mut self, dt: f32, empty: bool, profile: ActionProfile) -> Vec<(i32, f32)> {
        match profile {
            ActionProfile::Slide(profile) => vec![(0, self.slide_value(dt, empty, profile))],
            ActionProfile::Fusion => {
                // Nightdive's shoot: holder -160, core +160 degrees, both
                // return at frame 29/30 Hz with quadratic ease-out. There is
                // no third (extender) joint in the installed fsn_h model.
                let remaining = if let Some(elapsed) = &mut self.elapsed {
                    if dt.is_finite() && dt > 0.0 {
                        *elapsed += dt;
                    }
                    let t = (*elapsed / (29.0 / 30.0)).clamp(0.0, 1.0);
                    if t >= 1.0 {
                        self.elapsed = None;
                    }
                    (1.0 - t).powi(2)
                } else {
                    0.0
                };
                vec![(0, -160.0 * remaining), (1, 160.0 * remaining)]
            }
        }
    }

    fn slide_value(&mut self, dt: f32, empty: bool, profile: SlideProfile) -> f32 {
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

fn enabled_profile(world: &World, entity: EntityId) -> Option<ActionProfile> {
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
    let rig = &rigs.get(entity).ok()?.0;
    let required: &[(i32, u8)] = match profile {
        ActionProfile::Slide(_) => &[(0, 2)],
        ActionProfile::Fusion => &[(0, 1), (1, 1)],
    };
    required
        .iter()
        .all(|&(parameter, motion)| {
            rig.joints
                .iter()
                .any(|joint| joint.parameter == parameter && joint.motion_type == motion)
        })
        .then_some(profile)
}

/// Called only for an accepted shot, never a dry fire or a cooldown refusal.
pub(crate) fn fired(world: &mut World, entity: EntityId) {
    if enabled_profile(world, entity).is_some() {
        world.add_component(entity, WeaponAction { elapsed: Some(0.0) });
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
            .borrow::<View<WeaponAction>>()
            .ok()
            .and_then(|v| v.get(entity).ok().copied())
            .unwrap_or_default();
        let parameters = slide.parameters(dt, empty, profile);
        world.add_component(entity, slide);
        effects.push(Effect::SetObjectParameters {
            entity_id: entity,
            parameters,
        });
    }
    effects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fusion_has_a_supported_mechanical_profile() {
        let fusion = profile("FSN_H").expect("fusion profile");
        let mut action = WeaponAction { elapsed: Some(0.0) };
        assert_eq!(
            action.parameters(0.0, false, fusion),
            vec![(0, -160.0), (1, 160.0)]
        );
        assert_eq!(
            action.parameters(0.0, false, fusion),
            vec![(0, -160.0), (1, 160.0)],
            "pause holds the pose"
        );
        let halfway = action.parameters(14.5 / 30.0, false, fusion);
        assert!((halfway[0].1 + 40.0).abs() < 1e-4);
        assert!((halfway[1].1 - 40.0).abs() < 1e-4);
        assert_eq!(
            action.parameters(1.0, true, fusion),
            vec![(0, 0.0), (1, 0.0)]
        );
        assert!(action.elapsed.is_none());
    }

    #[test]
    fn rifle_retracts_on_each_shot_and_returns_forward_even_when_empty() {
        let Some(ActionProfile::Slide(rifle)) = profile("AR15_H") else {
            panic!("the AR15 must enable its slide")
        };
        let mut slide = WeaponAction { elapsed: Some(0.0) };
        assert_eq!(slide.slide_value(BACK_TIME, true, rifle), OPEN);
        assert!(
            slide.slide_value(6.0 / 30.0, true, rifle) < -0.001,
            "the AR15 returns at frame 8, later than the pistol"
        );
        assert_eq!(slide.slide_value(1.0 / 30.0, true, rifle), 0.0);
        assert_eq!(slide.slide_value(1.0, true, rifle), 0.0);
        assert_eq!(WeaponAction::default().slide_value(0.0, true, rifle), 0.0);
    }

    #[test]
    fn accepted_shot_cycles_only_the_slide_and_pause_does_not_advance_it() {
        let mut slide = WeaponAction { elapsed: Some(0.0) };
        assert_eq!(slide.slide_value(0.0, false, PISTOL), 0.0);
        assert!((slide.slide_value(BACK_TIME, false, PISTOL) - OPEN).abs() < 1e-6);
        assert_eq!(slide.slide_value(0.0, false, PISTOL), OPEN);
        let returning = slide.slide_value(0.05, false, PISTOL);
        assert!(returning > OPEN && returning < 0.0);
        assert_eq!(slide.slide_value(1.0, false, PISTOL), 0.0);
        assert!(slide.elapsed.is_none());
    }

    #[test]
    fn last_round_locks_back_until_ammo_is_loaded() {
        let mut slide = WeaponAction { elapsed: Some(0.0) };
        assert_eq!(slide.slide_value(0.0, true, PISTOL), 0.0);
        assert_eq!(slide.slide_value(BACK_TIME, true, PISTOL), OPEN);
        assert_eq!(slide.slide_value(1.0, true, PISTOL), OPEN);
        assert_eq!(slide.slide_value(10.0, true, PISTOL), OPEN);
        assert_eq!(slide.slide_value(0.0, false, PISTOL), 0.0);
    }

    #[test]
    fn empty_equip_or_save_restore_needs_no_persisted_animation_state() {
        let mut slide = WeaponAction::default();
        assert_eq!(slide.slide_value(0.0, true, PISTOL), OPEN);
        assert_eq!(slide.slide_value(0.0, false, PISTOL), 0.0);
    }
}
