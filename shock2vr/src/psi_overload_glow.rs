//! The amp heats in the overload zone, in both presentations: its device
//! parts glow its own psi teal on entering the zone and shift to red as the
//! charge nears burnout; an overload cast flares that colour and cools over
//! the meter's result flash. The baked hand that shares the model stays skin.
use cgmath::{Vector3, Vector4, VectorSpace, vec3};
use engine::scene::Glow;

use crate::psi::OVERLOAD_ZONE_START;
use crate::runtime_props::{PsiChargePhase, RuntimePropPsiCharge};

pub const SECS: f32 = crate::psi::CHARGE_RESULT_FLASH_SECS;
const ATTACK_SECS: f32 = 0.05;
/// The hue of the amp's status strip, saturated so it still reads on the
/// dark metal: entering the zone looks like the amp's own energy building.
const TEAL: Vector3<f32> = vec3(0.1, 0.8, 1.0);
const RED: Vector3<f32> = vec3(1.0, 0.25, 0.04);
/// Held glow across the zone, below the release flare's 1.0.
const HOLD: [f32; 2] = [0.75, 0.85];

/// The 25AE amp atlas keeps all skin (hand, fingers, forearm) left of this u;
/// the cable, plug and ball segments lie to its right.
const AMP_DEVICE_MIN_U: f32 = 0.68;

/// How far through the overload zone a charge is (0..1), `None` below it.
fn zone_progress(fraction: f32) -> Option<f32> {
    (fraction >= OVERLOAD_ZONE_START)
        .then(|| ((fraction - OVERLOAD_ZONE_START) / (1.0 - OVERLOAD_ZONE_START)).min(1.0))
}

/// The amp's heat for its meter state; `flash_age` is seconds since an
/// overload release. A burnout or a charge below the zone has none.
pub(crate) fn color(
    charge: Option<&RuntimePropPsiCharge>,
    flash_age: Option<f32>,
) -> Option<Vector3<f32>> {
    let charge = charge?;
    let progress = zone_progress(charge.fraction)?;
    let hold = HOLD[0] + (HOLD[1] - HOLD[0]) * progress;
    let intensity = match charge.phase {
        PsiChargePhase::Charging => hold,
        PsiChargePhase::Overloaded => {
            let age = flash_age.filter(|age| (0.0..SECS).contains(age))?;
            if age < ATTACK_SECS {
                hold + (1.0 - hold) * age / ATTACK_SECS
            } else {
                let cooled = (age - ATTACK_SECS) / (SECS - ATTACK_SECS);
                (1.0 - cooled) * (1.0 - cooled)
            }
        }
        PsiChargePhase::Burnout => return None,
    };
    Some(TEAL.lerp(RED, progress) * intensity)
}

/// The glow for one of the amp's materials, `None` for art with no known
/// device region (the classic amp, whose atlas mixes in the hand).
pub(crate) fn glow(color: Vector3<f32>, material: Option<&str>) -> Option<Glow> {
    match material?.to_ascii_lowercase().as_str() {
        "nd-amp_h.psd" => Some(Glow {
            color,
            uv_rect: Vector4::new(
                AMP_DEVICE_MIN_U,
                -Glow::UNBOUNDED,
                Glow::UNBOUNDED,
                Glow::UNBOUNDED,
            ),
        }),
        // The status strip is all device; already bright, it flashes white-hot.
        "nd-amp_il.psd" => Some(Glow::everywhere(color)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn charge(fraction: f32, phase: PsiChargePhase) -> RuntimePropPsiCharge {
        RuntimePropPsiCharge { fraction, phase }
    }

    fn charging(fraction: f32) -> Option<Vector3<f32>> {
        color(Some(&charge(fraction, PsiChargePhase::Charging)), None)
    }

    fn flash(fraction: f32, age: f32) -> Option<Vector3<f32>> {
        color(
            Some(&charge(fraction, PsiChargePhase::Overloaded)),
            Some(age),
        )
    }

    #[test]
    fn heat_starts_teal_in_the_zone_and_turns_red_toward_burnout() {
        assert_eq!(charging(0.5), None);
        let entering = charging(OVERLOAD_ZONE_START).unwrap();
        assert!(entering.z > entering.x, "teal: {entering:?}");
        let full = charging(1.0).unwrap();
        assert!(full.x > 4.0 * full.z, "red: {full:?}");
        assert!(full.x > entering.x);
    }

    #[test]
    fn a_release_flares_its_colour_then_cools() {
        let held = charging(0.9).unwrap();
        let peak = flash(0.9, ATTACK_SECS).unwrap();
        assert!(peak.x > held.x);
        assert!(flash(0.9, 0.5).unwrap().x < flash(0.9, 0.3).unwrap().x);
        assert_eq!(flash(0.9, SECS), None);
        assert_eq!(flash(0.9, -0.1), None);
        assert_eq!(
            color(Some(&charge(1.0, PsiChargePhase::Burnout)), Some(0.1)),
            None
        );
        assert_eq!(color(None, Some(0.1)), None);
    }

    #[test]
    fn only_the_device_heats() {
        let c = vec3(1.0, 0.0, 0.0);
        assert_eq!(
            glow(c, Some("ND-amp_h.psd")).unwrap().uv_rect.x,
            AMP_DEVICE_MIN_U
        );
        assert_eq!(
            glow(c, Some("ND-amp_il.psd")).unwrap().uv_rect.x,
            -Glow::UNBOUNDED
        );
        assert_eq!(glow(c, Some("amp_h.pcx")), None);
        assert_eq!(glow(c, None), None);
    }
}
