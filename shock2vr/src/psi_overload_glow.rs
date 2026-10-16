//! An overload cast heats the amp: its device parts flare red-hot and cool
//! over the meter's result flash, in both presentations. The baked hand that
//! shares the model stays skin.
use cgmath::{Vector3, Vector4, vec3};
use engine::scene::Glow;

pub const SECS: f32 = crate::psi::CHARGE_RESULT_FLASH_SECS;
const ATTACK_SECS: f32 = 0.05;
const HEAT: Vector3<f32> = vec3(1.0, 0.25, 0.04);

/// The 25AE amp atlas keeps all skin (hand, fingers, forearm) left of this u;
/// the cable, plug and ball segments lie to its right.
const AMP_DEVICE_MIN_U: f32 = 0.68;

/// The glow for one of the amp's materials `age` seconds after the overload,
/// `None` once it has cooled or for art with no known device region (the
/// classic amp, whose atlas mixes in the hand).
pub(crate) fn glow(age: f32, material: Option<&str>) -> Option<Glow> {
    if !(0.0..SECS).contains(&age) {
        return None;
    }
    let intensity = if age < ATTACK_SECS {
        age / ATTACK_SECS
    } else {
        let cooled = (age - ATTACK_SECS) / (SECS - ATTACK_SECS);
        (1.0 - cooled) * (1.0 - cooled)
    };
    let color = HEAT * intensity;
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

    const AMP: Option<&str> = Some("ND-amp_h.psd");

    fn red(age: f32) -> f32 {
        glow(age, AMP).unwrap().color.x
    }

    #[test]
    fn flares_fast_then_cools() {
        assert!((red(ATTACK_SECS) - HEAT.x).abs() < 1e-6);
        assert!(red(0.01) < red(ATTACK_SECS));
        assert!(red(0.5) < red(0.3) && red(0.3) < red(ATTACK_SECS));
        assert_eq!(glow(SECS, AMP), None);
        assert_eq!(glow(-0.1, AMP), None);
    }

    #[test]
    fn only_the_device_heats() {
        assert_eq!(glow(0.1, AMP).unwrap().uv_rect.x, AMP_DEVICE_MIN_U);
        assert_eq!(
            glow(0.1, Some("ND-amp_il.psd")).unwrap().uv_rect.x,
            -Glow::UNBOUNDED
        );
        assert_eq!(glow(0.1, Some("amp_h.pcx")), None);
        assert_eq!(glow(0.1, None), None);
    }
}
