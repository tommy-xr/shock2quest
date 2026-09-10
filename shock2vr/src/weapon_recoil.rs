//! Authored gun kick with a VR spring response. Camera jolt is deliberately
//! excluded: the tracked head is never moved. Citadel's mass=1, stiffness=40,
//! damping=14 response is integrated analytically, without frame-rate drift.
use cgmath::{InnerSpace, Vector3, vec3};
use dark::properties::{
    GunKickSetting, Link, Links, PropGunKick, PropGunState, PropImplantDesc, PropModelName,
    PropPlayerGun,
};
use rand::Rng;
use shipyard::{EntityId, Get, UniqueView, View, World};

#[derive(Clone, Copy, Debug)]
pub struct RecoilImpulse {
    pub pitch: f32,
    pub heading: f32,
    pub back: f32,
    pub pitch_limit: f32,
    pub back_limit: f32,
    pub heading_limit: f32,
    pub angular_rate: f32,
    pub back_rate: f32,
    pub forward: Vector3<f32>,
}

/// Intentional VR control tuning, separate from Dark's Agility modifier.
/// Caps and spring rates remain fixed: changing Strength/support affects
/// future impulses, never the pose or recovery of an already moving spring.
pub fn vr_impulses(
    impulse: RecoilImpulse,
    one_hand: RecoilImpulse,
    strength: i32,
    supported: bool,
) -> (RecoilImpulse, Option<RecoilImpulse>) {
    let above_minimum = (strength.clamp(1, 6) - 1) as f32;
    let scaled = |impulse: RecoilImpulse, scale| RecoilImpulse {
        pitch: impulse.pitch * scale,
        heading: impulse.heading * scale,
        back: impulse.back * scale,
        ..impulse
    };
    (
        scaled(impulse, 1.0 / (1.0 + 0.1 * above_minimum)),
        (!supported).then(|| scaled(one_hand, 1.0 / (1.0 + 0.3 * above_minimum))),
    )
}

/// Original CalcKickAngle: Agility, Still Hand, then the aiming implant.
fn kick_angle<R: Rng + ?Sized>(
    degrees: f32,
    flags: u32,
    agility: i32,
    still_hand: bool,
    aiming: bool,
    rng: &mut R,
) -> f32 {
    if still_hand {
        return 0.0;
    }
    let mut turns = (degrees * 65536.0 / 360.0) as u16;
    turns = (turns as f32 * (6 - agility.clamp(1, 6)) as f32 / 5.0) as u16;
    if aiming {
        turns = (turns as f32 * 0.8) as u16;
    }
    let direction = match flags & 3 {
        1 => 1.0,
        2 => -1.0,
        3 => {
            if rng.gen_bool(0.5) {
                1.0
            } else {
                -1.0
            }
        }
        _ => return 0.0,
    };
    turns as f32 * (360.0 / 65536.0) * direction * rng.gen_range(0.5..=1.0)
}

fn aiming_implant(world: &World) -> bool {
    let Ok(player) = world.borrow::<UniqueView<crate::mission::PlayerInfo>>() else {
        return false;
    };
    let Ok((links, implants)) = world.borrow::<(View<Links>, View<PropImplantDesc>)>() else {
        return false;
    };
    // Dark GetEquip: PDOLLBASE 1000 + Special/Special2 (3/4). Merely
    // carrying an implant in a backpack cell does not activate it.
    [player.entity_id, player.inventory_entity_id]
        .into_iter()
        .any(|owner| {
            links.get(owner).is_ok_and(|links| {
                links.to_links.iter().any(|link| {
                    matches!(link.link, Link::Contains(1003 | 1004))
                        && link
                            .to_entity_id
                            .is_some_and(|id| implants.get(id.0).is_ok_and(|p| p.0 == 6))
                })
            })
        })
}

/// Called only after the shared firing gate succeeds, once per shell (not pellet).
pub fn shot_impulse(world: &World, gun: EntityId) -> Option<(RecoilImpulse, RecoilImpulse)> {
    // Keep the flat/nonphysical firing path free of recoil RNG draws.
    crate::mission::mission_core::held_item_collision_group(world, gun)?;
    let (kicks, states, guns) = world
        .borrow::<(View<PropGunKick>, View<PropGunState>, View<PropPlayerGun>)>()
        .ok()?;
    let setting = states.get(gun).map(|s| s.setting).unwrap_or(0);
    let kick = kicks.get(gun).ok()?.setting(setting);
    let flags = guns.get(gun).ok()?.flags;
    let agility = world
        .borrow::<UniqueView<crate::quest_info::QuestInfo>>()
        .map(|q| q.player_stats().agility)
        .unwrap_or(1);
    // Shipped Still Hand template, verified against gamesys (power id 2).
    let still_hand = world
        .borrow::<UniqueView<crate::psi::ActivePsiPowers>>()
        .is_ok_and(|powers| powers.is_active(-1107));
    let mut rng = rand::thread_rng();
    let aiming = aiming_implant(world);
    let authored = authored_impulse(
        kick,
        flags,
        agility,
        still_hand,
        aiming,
        crate::weapon_muzzle::resolve(world, gun).axis,
        &mut rng,
    );
    let model = world.borrow::<View<PropModelName>>().ok()?;
    let extra = one_hand_impulse(
        authored,
        handling_profile(&model.get(gun).ok()?.0, setting),
        agility,
        still_hand,
        aiming,
        &mut rng,
    );
    Some((authored, extra))
}

struct HandlingProfile {
    pitch: f32,
    yaw: f32,
    rate: f32,
    sag: f32,
}

fn handling_profile(model: &str, setting: i32) -> Option<HandlingProfile> {
    let model = model.to_ascii_lowercase();
    let (pitch, yaw, rate, sag) = match (model.trim_end_matches(".bin"), setting) {
        ("atek_h", _) => (2.0, 0.75, 2.0, 1.0),
        ("ar15_h", _) => (8.0, 2.0, 1.0, 8.0),
        // Shotgun modes preserve their heavy backward kick, but add explicit
        // angular handling independent of Agility's vertical suppression.
        ("sg_h", 1) => (24.0, 4.5, 0.5, 0.0),
        ("sg_h", _) => (12.0, 3.0, 0.5, 0.0),
        _ => return None,
    };
    Some(HandlingProfile {
        pitch,
        yaw,
        rate,
        sag,
    })
}

/// Deliberate VR tuning: forward-heavy long guns need more angular correction
/// one-handed. This replaces the generic extra angular kick for these models;
/// authored two-hand kick and the existing extra backward kick are preserved.
fn one_hand_impulse<R: Rng + ?Sized>(
    authored: RecoilImpulse,
    profile: Option<HandlingProfile>,
    agility: i32,
    still_hand: bool,
    aiming: bool,
    rng: &mut R,
) -> RecoilImpulse {
    let Some(HandlingProfile {
        pitch, yaw, rate, ..
    }) = profile
    else {
        return authored;
    };
    RecoilImpulse {
        // Strength scales this later. Agility affects horizontal stability,
        // without making a heavy gun effortless vertically at Agility 6.
        pitch: kick_angle(pitch, 1, 1, still_hand, aiming, rng),
        heading: kick_angle(yaw, 3, agility, still_hand, aiming, rng),
        pitch_limit: pitch * 2.0,
        heading_limit: yaw * 2.0,
        angular_rate: rate,
        ..authored
    }
}

fn authored_impulse<R: Rng + ?Sized>(
    kick: &GunKickSetting,
    flags: u32,
    agility: i32,
    still_hand: bool,
    aiming: bool,
    forward: Vector3<f32>,
    rng: &mut R,
) -> RecoilImpulse {
    RecoilImpulse {
        pitch: kick_angle(
            kick.kick_pitch_degrees,
            flags,
            agility,
            still_hand,
            aiming,
            rng,
        ),
        heading: kick_angle(
            kick.kick_heading_degrees,
            flags >> 2,
            agility,
            still_hand,
            aiming,
            rng,
        ),
        back: kick.kick_back / dark::SCALE_FACTOR,
        pitch_limit: kick.kick_pitch_max_degrees.abs(),
        back_limit: kick.kick_back_max.abs() / dark::SCALE_FACTOR,
        heading_limit: f32::MAX,
        angular_rate: recovery_rate(
            kick.kick_angular_return_rate_degrees,
            kick.kick_pitch_max_degrees,
        ),
        back_rate: recovery_rate(kick.kick_back_return_rate, kick.kick_back_max),
        forward,
    }
}

// The original uses linear return. VR maps its return/limit ratio onto the
// spring's time scale, bounded so even authored zero-return settings recover.
fn recovery_rate(return_rate: f32, limit: f32) -> f32 {
    if !return_rate.is_finite() || !limit.is_finite() || limit.abs() < 1e-6 {
        return 1.0;
    }
    (return_rate.abs() / limit.abs()).clamp(0.25, 4.0)
}

#[derive(Clone, Copy, Debug, Default)]
struct Spring {
    position: f32,
    velocity: f32,
}

impl Spring {
    fn kick(&mut self, amount: f32, rate: f32) {
        // Normalize the isolated spring peak to the authored kick magnitude.
        // Unit impulse response is (exp(-4t)-exp(-10t))/6.
        let peak_time = (2.5_f32).ln() / 6.0;
        let peak = ((-4.0 * peak_time).exp() - (-10.0 * peak_time).exp()) / 6.0;
        self.velocity += amount * rate / peak;
    }
    fn step_toward(&mut self, dt: f32, target: f32) {
        if dt <= 0.0 || !dt.is_finite() {
            return;
        }
        // Integrate error around a moving equilibrium with the same analytic
        // spring as firing recoil. Preserve displacement and velocity on change.
        self.position -= target;
        self.step(dt, 1.0, f32::MAX);
        self.position += target;
    }
    fn step(&mut self, dt: f32, rate: f32, limit: f32) {
        if dt <= 0.0 || !dt.is_finite() {
            return;
        }
        let a = (self.velocity + 10.0 * rate * self.position) / (6.0 * rate);
        let b = self.position - a;
        let slow = a * (-4.0 * rate * dt).exp();
        let fast = b * (-10.0 * rate * dt).exp();
        self.position = slow + fast;
        self.velocity = -4.0 * rate * slow - 10.0 * rate * fast;
        if self.position.abs() > limit {
            self.position = self.position.clamp(-limit, limit);
            self.velocity = 0.0;
        }
    }
}

/// A continuously updated weight bias. The anchor is in scaled model space.
#[derive(Clone, Copy, Debug)]
pub struct GunWeightTarget {
    pub anchor: Vector3<f32>,
    pub forward: Vector3<f32>,
    pub degrees: f32,
}

pub fn gun_weight_target(
    world: &World,
    gun: EntityId,
    anchor: Vector3<f32>,
    strength: i32,
    supported: bool,
) -> Option<GunWeightTarget> {
    crate::mission::mission_core::held_item_collision_group(world, gun)?;
    let models = world.borrow::<View<PropModelName>>().ok()?;
    let profile = handling_profile(&models.get(gun).ok()?.0, 0)?;
    Some(GunWeightTarget {
        anchor,
        forward: crate::weapon_muzzle::resolve(world, gun).axis,
        degrees: if supported {
            0.0
        } else {
            profile.sag * (6 - strength.clamp(1, 6)) as f32 / 5.0
        },
    })
}

/// Spring the world-space angular bias, so rolled wrists still sag downward
/// and pointing vertically smoothly removes the lever arm. This is a bounded
/// handling approximation, not a calibrated mass/centre-of-mass simulation.
#[derive(Clone, Copy, Debug, Default)]
pub struct GunWeightState {
    rotation: [Spring; 3],
}
impl GunWeightState {
    pub fn step(
        &mut self,
        dt: f32,
        forward: Vector3<f32>,
        degrees: f32,
    ) -> cgmath::Quaternion<f32> {
        use cgmath::{Deg, Quaternion, Rotation3};
        if ![forward.x, forward.y, forward.z, degrees]
            .into_iter()
            .all(f32::is_finite)
            || forward.magnitude2() < 1e-8
        {
            return Quaternion::new(1.0, 0.0, 0.0, 0.0);
        }
        let target = forward.normalize().cross(-Vector3::unit_y()) * degrees.clamp(0.0, 8.0);
        let target = [target.x, target.y, target.z];
        for (spring, target) in self.rotation.iter_mut().zip(target) {
            spring.step_toward(dt, target);
        }
        let angle = vec3(
            self.rotation[0].position,
            self.rotation[1].position,
            self.rotation[2].position,
        );
        let magnitude = angle.magnitude();
        if magnitude < 1e-8 {
            Quaternion::new(1.0, 0.0, 0.0, 0.0)
        } else {
            Quaternion::from_axis_angle(angle / magnitude, Deg(magnitude.min(8.0)))
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RecoilState {
    pitch: Spring,
    heading: Spring,
    back: Spring,
    impulse: Option<RecoilImpulse>,
}
impl RecoilState {
    pub fn kick(&mut self, impulse: RecoilImpulse) {
        // Malformed authored data must not introduce NaNs into a rigid body.
        if ![
            impulse.pitch,
            impulse.heading,
            impulse.back,
            impulse.pitch_limit,
            impulse.back_limit,
            impulse.heading_limit,
            impulse.angular_rate,
            impulse.back_rate,
            impulse.forward.x,
            impulse.forward.y,
            impulse.forward.z,
        ]
        .into_iter()
        .all(f32::is_finite)
            || impulse.angular_rate <= 0.0
            || impulse.back_rate <= 0.0
            || impulse.pitch_limit < 0.0
            || impulse.back_limit < 0.0
            || impulse.heading_limit < 0.0
            || Vector3::unit_y().cross(impulse.forward).magnitude2() < 1e-8
        {
            return;
        }
        self.pitch.kick(impulse.pitch, impulse.angular_rate);
        self.heading.kick(impulse.heading, impulse.angular_rate);
        self.back.kick(impulse.back, impulse.back_rate);
        self.impulse = Some(impulse);
    }
    /// Translation and local rotation axes/angles, in the held model's frame.
    pub fn step(&mut self, dt: f32) -> (Vector3<f32>, cgmath::Quaternion<f32>) {
        use cgmath::{Deg, Quaternion, Rotation3};
        let Some(i) = self.impulse else {
            return (vec3(0.0, 0.0, 0.0), Quaternion::new(1.0, 0.0, 0.0, 0.0));
        };
        self.pitch.step(dt, i.angular_rate, i.pitch_limit);
        self.heading.step(dt, i.angular_rate, i.heading_limit);
        self.back.step(dt, i.back_rate, i.back_limit);
        let forward = i.forward.normalize();
        let right = Vector3::unit_y().cross(forward).normalize();
        (
            forward * self.back.position,
            Quaternion::from_axis_angle(Vector3::unit_y(), Deg(self.heading.position))
                * Quaternion::from_axis_angle(right, Deg(-self.pitch.position)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};
    #[test]
    fn weight_follows_world_gravity_and_smoothly_releases_without_frame_drift() {
        use cgmath::Rotation;
        for forward in [-Vector3::unit_x(), Vector3::unit_x(), -Vector3::unit_z()] {
            let mut slow = GunWeightState::default();
            let mut fast = GunWeightState::default();
            for _ in 0..120 {
                slow.step(1.0 / 60.0, forward, 8.0);
            }
            for _ in 0..240 {
                fast.step(1.0 / 120.0, forward, 8.0);
            }
            let before = slow.step(0.0, forward, 8.0).rotate_vector(forward);
            let other = fast.step(0.0, forward, 8.0).rotate_vector(forward);
            assert!((before - other).magnitude() < 1e-5);
            assert!((before.y + 8.0_f32.to_radians().sin()).abs() < 0.001);
            // Stat/support changes change the target, not the current pose.
            assert!(
                (slow.step(0.0, forward, 0.0).rotate_vector(forward) - before).magnitude() < 1e-6
            );
            let first = slow.step(1.0 / 60.0, forward, 0.0).rotate_vector(forward);
            assert!((first - before).magnitude() < 0.001);
            for _ in 0..240 {
                slow.step(1.0 / 60.0, forward, 0.0);
            }
            assert!(
                (slow.step(0.0, forward, 0.0).rotate_vector(forward) - forward).magnitude() < 1e-5
            );
        }
        for forward in [Vector3::unit_y(), -Vector3::unit_y()] {
            let mut state = GunWeightState::default();
            assert_eq!(
                state.step(1.0, forward, 8.0).rotate_vector(forward),
                forward
            );
        }
        let mut state = GunWeightState::default();
        for i in 0..1000 {
            let forward = vec3((i as f32).sin(), 0.0, (i as f32).cos());
            let q = state.step(1.0 / 60.0, forward, 8.0);
            assert!(q.s.is_finite() && q.v.magnitude() <= (4.0_f32.to_radians()).sin() + 1e-6);
        }
    }

    #[test]
    fn one_hand_profiles_separate_vertical_load_from_agility_and_preserve_baseline() {
        let authored = RecoilImpulse {
            pitch: 3.0,
            heading: 0.0,
            back: -0.1,
            pitch_limit: 4.0,
            heading_limit: f32::MAX,
            back_limit: 0.2,
            angular_rate: 2.0,
            back_rate: 1.0,
            forward: -Vector3::unit_x(),
        };
        let extra = |model, agility, still| {
            one_hand_impulse(
                authored,
                handling_profile(model, 0),
                agility,
                still,
                false,
                &mut StdRng::seed_from_u64(7),
            )
        };
        let shotgun = |setting, agility, still, aiming| {
            one_hand_impulse(
                authored,
                handling_profile("sg_h", setting),
                agility,
                still,
                aiming,
                &mut StdRng::seed_from_u64(7),
            )
        };
        let normal = shotgun(0, 1, false, false);
        let triple = shotgun(1, 1, false, false);
        assert!((triple.pitch / normal.pitch - 2.0).abs() < 0.01);
        assert!((triple.heading / normal.heading - 1.5).abs() < 0.01);
        for setting in [0, 1] {
            let low = shotgun(setting, 1, false, false);
            let agile = shotgun(setting, 6, false, false);
            assert_eq!(low.pitch, agile.pitch);
            assert_eq!(agile.heading, 0.0);
            assert!(low.heading.abs() > 0.0);
            assert_eq!(low.back, authored.back);
            assert_eq!(low.back_limit, authored.back_limit);
            assert_eq!(low.back_rate, authored.back_rate);
            let still = shotgun(setting, 1, true, false);
            assert_eq!((still.pitch, still.heading), (0.0, 0.0));
            assert!(shotgun(setting, 1, false, true).pitch < low.pitch);
            let (_, stronger) = vr_impulses(authored, low, 6, false);
            assert!(stronger.unwrap().pitch < low.pitch);
            let (supported, extra) = vr_impulses(authored, low, 1, true);
            assert!(extra.is_none());
            assert_eq!(supported.pitch, authored.pitch);
            assert_eq!(supported.back, authored.back);
            let mut spring = RecoilState::default();
            for _ in 0..300 {
                spring.kick(low);
                spring.step(1.0 / 60.0);
                assert!(spring.pitch.position.abs() <= low.pitch_limit);
                assert!(spring.heading.position.abs() <= low.heading_limit);
            }
        }
        let pistol = extra("atek_h", 1, false);
        let ar = extra("ar15_h", 1, false);
        assert!(ar.pitch > pistol.pitch * 3.9);
        assert!(ar.heading.abs() > pistol.heading.abs() * 2.6);
        assert!(ar.angular_rate < pistol.angular_rate);
        assert_eq!(ar.back, authored.back);
        let agile = extra("ar15_h", 6, false);
        assert_eq!(agile.pitch, ar.pitch);
        assert_eq!(agile.heading, 0.0);
        let middle = extra("ar15_h", 3, false);
        assert!(middle.heading.abs() < ar.heading.abs());
        let still = extra("ar15_h", 1, true);
        assert_eq!((still.pitch, still.heading), (0.0, 0.0));
        assert_eq!(still.back, authored.back);
        let (base, penalty) = vr_impulses(authored, ar, 1, true);
        assert_eq!((base.pitch, base.heading, base.back), (3.0, 0.0, -0.1));
        assert!(penalty.is_none());
        let low = vr_impulses(authored, ar, 1, false).1.unwrap();
        let high = vr_impulses(authored, ar, 6, false).1.unwrap();
        assert!(high.pitch < low.pitch && high.heading.abs() < low.heading.abs());
        assert_eq!(extra("fsn_h", 1, false).pitch, authored.pitch);
        let mut state = RecoilState::default();
        for _ in 0..200 {
            state.kick(ar);
            state.step(1.0 / 60.0);
            assert!(state.heading.position.abs() <= ar.heading_limit);
            assert!(state.pitch.position.abs() <= ar.pitch_limit);
        }
    }

    #[test]
    fn strength_reduces_both_new_impulses_without_changing_recovery_or_caps() {
        let authored = RecoilImpulse {
            pitch: 7.0,
            heading: 1.0,
            back: -0.1,
            pitch_limit: 10.0,
            back_limit: 0.2,
            heading_limit: f32::MAX,
            angular_rate: 1.0,
            back_rate: 2.0,
            forward: -Vector3::unit_x(),
        };
        let mut previous = (f32::MAX, f32::MAX);
        for strength in [1, 3, 6] {
            let (base, extra) = vr_impulses(authored, authored, strength, false);
            let extra = extra.unwrap();
            assert!(base.pitch < previous.0 && extra.pitch < previous.1);
            previous = (base.pitch, extra.pitch);
            assert_eq!(base.pitch_limit, authored.pitch_limit);
            assert_eq!(extra.back_limit, authored.back_limit);
            assert_eq!(base.angular_rate, authored.angular_rate);
            assert_eq!(extra.back_rate, authored.back_rate);
            let (supported, penalty) = vr_impulses(authored, authored, strength, true);
            assert!(penalty.is_none());
            assert_eq!(supported.pitch, base.pitch);
            assert_eq!(supported.back, base.back);
        }
        assert_eq!(
            vr_impulses(authored, authored, 1, true).0.pitch,
            authored.pitch
        );
        assert_eq!(
            vr_impulses(authored, authored, -2, true).0.pitch,
            authored.pitch
        );
        assert_eq!(
            vr_impulses(authored, authored, 99, true).0.pitch,
            vr_impulses(authored, authored, 6, true).0.pitch
        );
    }

    #[test]
    fn acquiring_support_preserves_the_previous_one_hand_kick() {
        let authored = RecoilImpulse {
            pitch: 4.0,
            heading: 0.0,
            back: -0.1,
            pitch_limit: 10.0,
            back_limit: 0.2,
            heading_limit: f32::MAX,
            angular_rate: 1.0,
            back_rate: 1.0,
            forward: -Vector3::unit_x(),
        };
        let mut penalty = RecoilState::default();
        penalty.kick(vr_impulses(authored, authored, 1, false).1.unwrap());
        penalty.step(0.1);
        let mut uninterrupted = penalty;
        // A supported follow-up shot has no new penalty; the old spring remains.
        if let Some(extra) = vr_impulses(authored, authored, 6, true).1 {
            penalty.kick(extra);
        }
        assert_eq!(penalty.step(0.1), uninterrupted.step(0.1));
        assert!(penalty.pitch.position > 0.0);
        // Losing support adds velocity, without resetting accumulated displacement.
        let before = penalty.step(0.0);
        penalty.kick(vr_impulses(authored, authored, 6, false).1.unwrap());
        assert_eq!(penalty.step(0.0), before);
        penalty.step(5.0);
        assert!(penalty.pitch.position.abs() < 1e-6);
    }

    #[test]
    fn recoil_raises_and_backs_away_along_each_model_barrel() {
        use cgmath::Rotation;
        for forward in [-Vector3::unit_x(), Vector3::unit_x(), -Vector3::unit_z()] {
            let mut state = RecoilState::default();
            state.kick(RecoilImpulse {
                pitch: 7.0,
                heading: 0.0,
                back: -0.1,
                pitch_limit: 10.0,
                back_limit: 0.2,
                heading_limit: f32::MAX,
                angular_rate: 1.0,
                back_rate: 1.0,
                forward,
            });
            let (offset, rotation) = state.step(0.15);
            assert!(offset.dot(forward) < -0.09);
            assert!(rotation.rotate_vector(forward).y > 0.1);
            let (offset, rotation) = state.step(5.0);
            assert!(offset.magnitude() < 1e-6);
            assert!((rotation.rotate_vector(forward) - forward).magnitude() < 1e-6);
        }
    }

    #[test]
    fn invalid_impulse_cannot_poison_physics_pose() {
        let mut state = RecoilState::default();
        state.kick(RecoilImpulse {
            pitch: f32::NAN,
            heading: 0.0,
            back: -0.1,
            pitch_limit: 10.0,
            back_limit: 0.2,
            heading_limit: f32::MAX,
            angular_rate: 1.0,
            back_rate: 1.0,
            forward: Vector3::unit_y(),
        });
        assert!(state.impulse.is_none());
        assert_eq!(state.step(0.1).0, vec3(0.0, 0.0, 0.0));
    }

    #[test]
    fn aiming_implant_requires_an_equipped_special_slot() {
        use crate::mission::PlayerInfo;
        use dark::properties::{ToLink, WrappedEntityId};
        let mut world = World::new();
        let implant = world.add_entity(PropImplantDesc(6));
        let owner = world.add_entity(Links::empty());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 0.0),
            rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
            entity_id: owner,
            inventory_entity_id: owner,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
        });
        for (slot, expected) in [(0, false), (1002, false), (1003, true), (1004, true)] {
            world.add_component(
                owner,
                Links {
                    to_links: vec![ToLink {
                        to_template_id: 0,
                        to_entity_id: Some(WrappedEntityId(implant)),
                        link: Link::Contains(slot),
                    }],
                },
            );
            assert_eq!(aiming_implant(&world), expected);
        }
        world.add_component(implant, PropImplantDesc(5));
        assert!(!aiming_implant(&world));
    }

    #[test]
    fn original_angular_modifiers_and_direction_flags() {
        for (agility, still, aiming, max) in [
            (1, false, false, 10.0),
            (3, false, false, 6.0),
            (1, false, true, 8.0),
            (6, false, false, 0.0),
            (1, true, false, 0.0),
        ] {
            let mut rng = StdRng::seed_from_u64(9);
            for _ in 0..50 {
                let angle = kick_angle(10.0, 1, agility, still, aiming, &mut rng);
                assert!(angle >= max * 0.5 - 0.02 && angle <= max + 0.02);
                assert!(kick_angle(10.0, 2, agility, still, aiming, &mut rng) <= 0.0);
                assert_eq!(kick_angle(10.0, 0, agility, still, aiming, &mut rng), 0.0);
            }
        }
    }
    #[test]
    fn spring_has_authored_peak_and_independent_timestep() {
        let mut a = Spring::default();
        a.kick(7.0, 1.0);
        let mut b = a;
        let mut peak = 0.0_f32;
        for _ in 0..120 {
            a.step(1.0 / 120.0, 1.0, 100.0);
            peak = peak.max(a.position);
        }
        for _ in 0..60 {
            b.step(1.0 / 60.0, 1.0, 100.0);
        }
        assert!((peak - 7.0).abs() < 0.02);
        assert!((a.position - b.position).abs() < 1e-4);
        assert!((a.velocity - b.velocity).abs() < 1e-4);
        a.step(10.0, 1.0, 100.0);
        assert!(a.position.abs() < 1e-10);
    }
    #[test]
    fn repeated_impulses_respect_authored_ceiling_and_paused_time() {
        let mut s = Spring::default();
        for _ in 0..200 {
            s.kick(10.0, 1.0);
            s.step(1.0 / 60.0, 1.0, 12.0);
            assert!(s.position <= 12.0);
        }
        let before = s;
        s.step(0.0, 1.0, 12.0);
        assert_eq!(before.position, s.position);
        assert_eq!(before.velocity, s.velocity);
    }
}
