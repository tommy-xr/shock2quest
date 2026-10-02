//! Manual shotgun action. The support hand owns a moving contact, never the gun.
use dark::properties::{InternalPropPumpState, PumpPhase};
use shipyard::{Component, EntityId, Get, View, World};

use crate::{scripts::Effect, vr_support::SupportMotion};

#[derive(Component)]
pub(crate) struct PumpMotion;

pub(crate) fn state(world: &World, entity: EntityId) -> InternalPropPumpState {
    world
        .borrow::<View<InternalPropPumpState>>()
        .ok()
        .and_then(|v| v.get(entity).ok().copied())
        .unwrap_or_default()
}

pub(crate) fn enabled(world: &World, entity: EntityId) -> bool {
    crate::mission::mission_core::presentation_is_vr(world)
        && world
            .borrow::<View<PumpMotion>>()
            .is_ok_and(|v| v.get(entity).is_ok())
}

pub(crate) fn blocks_fire(world: &World, entity: EntityId) -> bool {
    let state = state(world, entity);
    enabled(world, entity) && (state.phase != PumpPhase::Ready || state.fraction > 0.1)
}

pub(crate) fn fired(world: &mut World, entity: EntityId) {
    if enabled(world, entity) {
        let mut state = state(world, entity);
        state.phase = PumpPhase::Spent;
        world.add_component(entity, state);
    } else if !crate::mission::mission_core::presentation_is_vr(world) {
        // Flat mode cycles the action through its authored clip. Do not carry
        // an old VR latch back into a later VR session after firing in flat.
        world.remove::<InternalPropPumpState>(entity);
    }
}

fn advance(state: &mut InternalPropPumpState, fraction: f32) -> bool {
    if !fraction.is_finite() {
        return false;
    }
    state.fraction = fraction.clamp(0.0, 1.0);
    if state.phase == PumpPhase::Spent && state.fraction >= 0.9 {
        state.phase = PumpPhase::Ejected;
        return true;
    }
    if state.phase == PumpPhase::Ejected && state.fraction <= 0.1 {
        state.phase = PumpPhase::Ready;
    }
    false
}

pub(crate) fn move_pump(
    world: &mut World,
    entity: EntityId,
    motion: SupportMotion,
    fraction: f32,
) -> Vec<Effect> {
    let mut state = state(world, entity);
    let eject = advance(&mut state, fraction);
    world.add_component(entity, (state, PumpMotion));
    let mut effects = vec![Effect::SetObjectParameters {
        entity_id: entity,
        parameters: vec![(motion.parameter, motion.value(state.fraction))],
    }];
    if eject {
        effects.push(Effect::EjectWeaponCasings { entity_id: entity });
    }
    effects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spent_shell_needs_both_strokes_and_ejects_only_once() {
        let mut state = InternalPropPumpState {
            phase: PumpPhase::Spent,
            fraction: 0.0,
        };
        for fraction in [0.5, 0.0, 0.89] {
            assert!(!advance(&mut state, fraction));
            assert_eq!(state.phase, PumpPhase::Spent);
        }
        assert!(advance(&mut state, 0.95));
        for fraction in [1.0, 0.94, 1.0, 0.2] {
            assert!(!advance(&mut state, fraction));
            assert_eq!(state.phase, PumpPhase::Ejected);
        }
        assert!(!advance(&mut state, 0.05));
        assert_eq!(state.phase, PumpPhase::Ready);
        assert!(
            !advance(&mut state, 1.0),
            "an unfired cycle must not mint a casing"
        );
        assert!(!advance(&mut state, f32::NAN));
        assert_eq!(state.fraction, 1.0);
    }
}
