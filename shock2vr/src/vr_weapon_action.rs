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
    Stasis,
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
        "sfg_h" => Some(ActionProfile::Stasis),
        _ => None,
    }
}

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct WeaponAction {
    elapsed: Option<f32>,
    reloading: bool,
}

impl WeaponAction {
    fn parameters(&mut self, dt: f32, empty: bool, profile: ActionProfile) -> Vec<(i32, f32)> {
        match profile {
            ActionProfile::Slide(profile) => vec![(0, self.slide_value(dt, empty, profile))],
            ActionProfile::Stasis => {
                let Some(elapsed) = &mut self.elapsed else {
                    return vec![(0, 0.0), (1, 0.0), (2, 0.0)];
                };
                if dt.is_finite() && dt > 0.0 {
                    *elapsed += dt;
                }
                let frame = *elapsed * 30.0;
                // The shipped shoot declares 30 frames but the cylinder's
                // return key is at 35. Finish the mechanical return explicitly.
                let rotation = -30.0 * (1.0 - phase(frame, 30.0, 35.0));
                let cylinder = if self.reloading {
                    -0.8 + 0.9 * phase(frame, 10.0, 11.0) - 0.1 * phase(frame, 11.0, 25.0)
                } else {
                    -0.8 * phase(frame, 14.0, 15.0) + 0.9 * phase(frame, 20.0, 21.0)
                        - 0.1 * phase(frame, 21.0, 25.0)
                };
                let slide = if self.reloading {
                    0.0
                } else {
                    -0.2 * phase(frame, 0.0, 1.0) + 0.2 * phase(frame, 1.0, 8.0)
                };
                if frame >= 35.0 {
                    self.elapsed = None;
                    return vec![(0, 0.0), (1, 0.0), (2, 0.0)];
                }
                vec![(0, rotation), (1, cylinder), (2, slide)]
            }
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
        ActionProfile::Stasis => &[(0, 1), (1, 2), (2, 2)],
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

// Nightdive uses quadratic ease-out between its scalar joint keys.
fn phase(frame: f32, start: f32, end: f32) -> f32 {
    let t = ((frame - start) / (end - start)).clamp(0.0, 1.0);
    t * (2.0 - t)
}

/// Called only after ammunition was actually transferred into the weapon.
pub(crate) fn reloaded(world: &mut World, entity: EntityId) {
    if matches!(enabled_profile(world, entity), Some(ActionProfile::Stasis)) {
        world.add_component(
            entity,
            WeaponAction {
                elapsed: Some(0.0),
                reloading: true,
            },
        );
    }
}

/// Called only for an accepted shot, never a dry fire or a cooldown refusal.
pub(crate) fn fired(world: &mut World, entity: EntityId) {
    if enabled_profile(world, entity).is_some() {
        world.add_component(
            entity,
            WeaponAction {
                elapsed: Some(0.0),
                ..Default::default()
            },
        );
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
    fn stasis_has_a_supported_mechanical_profile() {
        let stasis = profile("SFG_H").expect("stasis profile");
        let mut action = WeaponAction {
            elapsed: Some(0.0),
            ..Default::default()
        };
        let kick = action.parameters(1.0 / 30.0, false, stasis);
        assert_eq!(kick, vec![(0, -30.0), (1, 0.0), (2, -0.2)]);
        assert_eq!(action.parameters(0.0, false, stasis), kick);
        let opened = action.parameters(14.0 / 30.0, false, stasis);
        assert!((opened[1].1 + 0.8).abs() < 1e-5);
        assert_eq!(opened[2].1, 0.0);
        let beyond_declared_clip = action.parameters(17.0 / 30.0, false, stasis);
        assert!(beyond_declared_clip[0].1 < 0.0 && beyond_declared_clip[0].1 > -30.0);
        assert_eq!(
            action.parameters(1.0, true, stasis),
            vec![(0, 0.0), (1, 0.0), (2, 0.0)]
        );
        action = WeaponAction {
            elapsed: Some(0.0),
            reloading: true,
        };
        assert_eq!(
            action.parameters(0.0, false, stasis),
            vec![(0, -30.0), (1, -0.8), (2, 0.0)]
        );
        assert_eq!(
            action.parameters(2.0, false, stasis),
            vec![(0, 0.0), (1, 0.0), (2, 0.0)]
        );
    }

    #[test]
    fn fusion_has_a_supported_mechanical_profile() {
        let fusion = profile("FSN_H").expect("fusion profile");
        let mut action = WeaponAction {
            elapsed: Some(0.0),
            ..Default::default()
        };
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
        let mut slide = WeaponAction {
            elapsed: Some(0.0),
            ..Default::default()
        };
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
        let mut slide = WeaponAction {
            elapsed: Some(0.0),
            ..Default::default()
        };
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
        let mut slide = WeaponAction {
            elapsed: Some(0.0),
            ..Default::default()
        };
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
