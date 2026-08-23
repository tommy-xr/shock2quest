//! Headless stability sweep for the held-melee spring drive (#1053).
//!
//! Replacing the pose-teleported kinematic weapon with a *dynamic* body motored
//! toward an invisible kinematic hand target turns held melee into a control
//! problem: tracking error, overshoot, resting jitter, contact penetration, and
//! whether the solver stays bounded. Those are questions a screenshot cannot
//! answer, so this measures them across the tuning grid with nothing but
//! `PhysicsWorld` - no assets, no window, no game session.
//!
//! ```text
//! cargo test -p shock2vr --release --lib spring_sweep -- --ignored --nocapture
//! ```
//!
//! `held_melee_drive_stays_bounded_across_the_tuning_range` is *not* ignored:
//! it is the regression guard that every reachable dev-param setting keeps the
//! weapon finite, penetration bounded, and resting jitter below the visible
//! threshold. The sweep proper prints the table a human tunes from.

use cgmath::{Deg, InnerSpace, Quaternion, Rotation, Rotation3, Vector3, vec3};
use rapier3d::prelude::*;
use shipyard::EntityId;

use super::{
    CollisionGroup, DynamicPhysicsOptions, PhysicsShape, PhysicsWorld, PlayerHandle,
    RigidBodyHandle,
};
use crate::dev_params;

/// A wrench-scale fitted melee volume. The body origin sits on the rendered
/// weapon head and the fitted cuboid extends back over the handle, which is
/// what production builds via `fit_held_melee_cuboid`; the offset, elongated
/// shape is the inertia-awkward case an angular motor has to hold.
const WEAPON_HALF_EXTENTS: Vector3<f32> = Vector3::new(0.045, 0.30, 0.045);
const WEAPON_COLLIDER_CENTER: Vector3<f32> = Vector3::new(0.0, -0.30, 0.0);

/// The swing gesture every scenario shares: a 100 degree arc about the
/// shoulder in 0.3 s, at 0.7 units of reach. Coupled translation *and*
/// rotation, unlike the pure translation the drive's own unit tests use.
const SWING_ARC_DEG: f32 = 100.0;
const SWING_RADIUS: f32 = 0.7;
const SWING_FRAMES: usize = 18;
const HOLD_FRAMES: usize = 60;
const RETREAT_FRAMES: usize = 60;

struct Metrics {
    /// Worst hand-to-weapon distance during the swing: the "the weapon is not
    /// where my hand is" the player actually feels.
    max_free_lag_m: f32,
    /// Distance still remaining after the hand has been still for a second.
    settled_error_m: f32,
    /// Worst angular error over the same window.
    max_free_lag_deg: f32,
    /// How far past the hand the weapon carried once the hand stopped.
    overshoot_m: f32,
    /// RMS third difference of the weapon path over the settle window - the
    /// numeric form of visible buzz, since smooth motion cancels out of it.
    jitter: f32,
    /// Non-finite state, or a position past any sane bound, on any frame.
    diverged: bool,
    /// Deepest the weapon reached past the wall face (wall scenario only).
    penetration_m: f32,
    /// Frames to recover to within 2 cm of the hand after the hand withdraws.
    springback_frames: Option<usize>,
}

struct Sample {
    weapon: Vector3<f32>,
    hand: Vector3<f32>,
    hand_rot: Quaternion<f32>,
    weapon_rot: Quaternion<f32>,
}

fn identity_quat() -> Quaternion<f32> {
    Quaternion::new(1.0, 0.0, 0.0, 0.0)
}

fn world_with_floor() -> (PhysicsWorld, PlayerHandle) {
    let mut world = PhysicsWorld::new();
    let floor = world.create_static_body(
        Isometry::translation(0.0, -1.0, 0.0),
        EntityId::from_inner(1000),
    );
    world.attach_collider(
        floor,
        SharedShape::cuboid(100.0, 1.0, 100.0),
        1.0,
        CollisionGroup::entity(),
    );
    let player = world.create_player(
        vec3(1000.0, 1000.0, 1000.0),
        EntityId::from_inner(1001).unwrap(),
    );
    (world, player)
}

fn spawn_held_wrench(world: &mut PhysicsWorld, at: Vector3<f32>) -> (EntityId, RigidBodyHandle) {
    let weapon = EntityId::from_inner(2).unwrap();
    let handle = world.add_dynamic(
        weapon,
        at,
        identity_quat(),
        vec3(0.0, 0.0, 0.0),
        PhysicsShape::Cuboid(WEAPON_HALF_EXTENTS),
        CollisionGroup::entity(),
        false,
        DynamicPhysicsOptions::default(),
    );
    world.set_held_melee(weapon);
    world.fit_held_melee_cuboid(weapon, WEAPON_HALF_EXTENTS * 2.0, WEAPON_COLLIDER_CENTER);
    (weapon, handle)
}

fn swing_pose(t: f32) -> (Vector3<f32>, Quaternion<f32>) {
    let rot = Quaternion::from_angle_y(Deg(SWING_ARC_DEG * (t - 0.5)));
    (rot.rotate_vector(vec3(0.0, 1.2, SWING_RADIUS)), rot)
}

fn is_sane(p: Vector3<f32>) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.z.is_finite() && p.magnitude() < 1.0e4
}

fn angle_between(a: Quaternion<f32>, b: Quaternion<f32>) -> f32 {
    let d = (a.conjugate() * b).normalize();
    2.0 * d.s.abs().min(1.0).acos().to_degrees()
}

fn jitter_of(samples: &[Sample]) -> f32 {
    if samples.len() < 4 {
        return 0.0;
    }
    let mut acc = 0.0;
    let mut n = 0.0;
    for w in samples.windows(4) {
        let d = w[3].weapon - 3.0 * w[2].weapon + 3.0 * w[1].weapon - w[0].weapon;
        acc += d.magnitude2();
        n += 1.0;
    }
    (acc / n).sqrt()
}

/// `wall_at` puts a fixed pane in the swing's path; `None` runs the gesture
/// unobstructed. `hold_only` keeps the hand still for the whole run.
fn run(stiffness: f32, damping: f32, wall_at: Option<f32>, hold_only: bool) -> Metrics {
    // NOTE: process-global. See the note on
    // `held_melee_drive_stays_bounded_across_the_tuning_range`.
    dev_params::set(dev_params::MELEE_SPRING_STIFFNESS, stiffness);
    dev_params::set(dev_params::MELEE_SPRING_DAMPING, damping);

    let (mut world, mut player) = world_with_floor();
    if let Some(x) = wall_at {
        // Perpendicular to the swing, past its midpoint: the hand's x runs
        // -0.54 -> +0.54 across the arc, so this stops the second half while
        // leaving the rest pose (and the withdrawal) in free space.
        world.add_collider(
            EntityId::from_inner(3).unwrap(),
            ColliderBuilder::cuboid(0.05, 2.0, 2.0)
                .translation(vector![x, 1.2, 0.6])
                .build(),
        );
    }

    let (start, start_rot) = swing_pose(0.0);
    let (weapon, handle) = spawn_held_wrench(&mut world, start);
    for _ in 0..30 {
        world.set_position_rotation2(weapon, start, start_rot);
        world.update(vec3(0.0, 0.0, 0.0), &mut player);
    }

    let mut samples: Vec<Sample> = Vec::new();
    let mut diverged = false;
    let mut penetration = 0.0f32;

    let advance = |world: &mut PhysicsWorld,
                   player: &mut PlayerHandle,
                   hand: Vector3<f32>,
                   hand_rot: Quaternion<f32>,
                   samples: &mut Vec<Sample>,
                   diverged: &mut bool| {
        world.set_position_rotation2(weapon, hand, hand_rot);
        world.update(vec3(0.0, 0.0, 0.0), player);
        let w = world
            .get_position(handle)
            .unwrap_or(vec3(f32::NAN, f32::NAN, f32::NAN));
        if !is_sane(w) {
            *diverged = true;
        }
        samples.push(Sample {
            weapon: w,
            hand,
            hand_rot,
            weapon_rot: world.get_rotation(handle).unwrap_or(identity_quat()),
        });
    };

    for frame in 0..(SWING_FRAMES + HOLD_FRAMES) {
        let t = if hold_only {
            0.0
        } else {
            (frame as f32 / SWING_FRAMES as f32).min(1.0)
        };
        let (hand, hand_rot) = swing_pose(t);
        advance(
            &mut world,
            &mut player,
            hand,
            hand_rot,
            &mut samples,
            &mut diverged,
        );
        if let Some(x) = wall_at {
            let w = samples.last().unwrap().weapon;
            if is_sane(w) {
                penetration = penetration.max(w.x - (x - 0.05));
            }
        }
    }
    let swung = samples.len();

    let (rest, rest_rot) = swing_pose(0.0);
    let mut springback = None;
    for frame in 0..RETREAT_FRAMES {
        advance(
            &mut world,
            &mut player,
            rest,
            rest_rot,
            &mut samples,
            &mut diverged,
        );
        let s = samples.last().unwrap();
        if springback.is_none() && is_sane(s.weapon) && (s.weapon - s.hand).magnitude() < 0.02 {
            springback = Some(frame);
        }
    }

    let free = &samples[..swung];
    let mut max_lag = 0.0f32;
    let mut max_lag_deg = 0.0f32;
    for s in free.iter().filter(|s| is_sane(s.weapon)) {
        max_lag = max_lag.max((s.weapon - s.hand).magnitude());
        max_lag_deg = max_lag_deg.max(angle_between(s.hand_rot, s.weapon_rot));
    }
    let settle = &samples[swung.saturating_sub(30)..swung];
    let settled_error = settle
        .last()
        .map(|s| (s.weapon - s.hand).magnitude())
        .unwrap_or(f32::NAN);
    let overshoot = settle
        .iter()
        .filter(|s| is_sane(s.weapon))
        .map(|s| (s.weapon - s.hand).dot(s.hand.normalize()).max(0.0))
        .fold(0.0f32, f32::max);

    Metrics {
        max_free_lag_m: max_lag,
        settled_error_m: settled_error,
        max_free_lag_deg: max_lag_deg,
        overshoot_m: overshoot,
        jitter: jitter_of(settle),
        diverged,
        penetration_m: penetration,
        springback_frames: springback,
    }
}

/// The grid actually reachable from the Developer screen, honouring the
/// registry's own clamping/step snapping (a value the menu cannot produce is
/// not worth reporting).
fn grid() -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    for k in [225.0f32, 400.0, 900.0, 1600.0, 2500.0, 3000.0] {
        for zeta in [0.5f32, 1.0, 2.0] {
            let applied_k = dev_params::set(dev_params::MELEE_SPRING_STIFFNESS, k);
            let applied_c = dev_params::set(
                dev_params::MELEE_SPRING_DAMPING,
                zeta * 2.0 * applied_k.sqrt(),
            );
            out.push((applied_k, applied_c));
        }
    }
    out.dedup();
    out
}

#[test]
#[ignore = "tuning sweep - prints a table, run explicitly"]
fn spring_sweep_table() {
    println!(
        "\nheld-melee spring sweep @60 Hz - fitted cuboid half-extents {:?}, collider center {:?}",
        WEAPON_HALF_EXTENTS, WEAPON_COLLIDER_CENTER
    );

    println!("\n== free swing (no obstruction) ==");
    println!(
        "{:>7} {:>7} {:>6} {:>9} {:>9} {:>9} {:>10} {:>9}",
        "stiff", "damp", "zeta", "maxlag_m", "settle_m", "maxlag_d", "overshoot", "jitter"
    );
    for (k, c) in grid() {
        let m = run(k, c, None, false);
        println!(
            "{:>7.0} {:>7.1} {:>6.2} {:>9.4} {:>9.4} {:>9.2} {:>10.4} {:>9.6}{}",
            k,
            c,
            c / (2.0 * k.sqrt()),
            m.max_free_lag_m,
            m.settled_error_m,
            m.max_free_lag_deg,
            m.overshoot_m,
            m.jitter,
            if m.diverged { "  DIVERGED" } else { "" }
        );
    }

    println!("\n== swing into a fixed wall, then withdraw ==");
    println!(
        "{:>7} {:>7} {:>6} {:>9} {:>9} {:>9} {:>12}",
        "stiff", "damp", "zeta", "penetr_m", "settle_m", "jitter", "springback_f"
    );
    for (k, c) in grid() {
        let m = run(k, c, Some(0.30), false);
        println!(
            "{:>7.0} {:>7.1} {:>6.2} {:>9.4} {:>9.4} {:>9.6} {:>12}{}",
            k,
            c,
            c / (2.0 * k.sqrt()),
            m.penetration_m,
            m.settled_error_m,
            m.jitter,
            m.springback_frames
                .map(|f| f.to_string())
                .unwrap_or_else(|| ">60".into()),
            if m.diverged { "  DIVERGED" } else { "" }
        );
    }

    println!("\n== stationary hand (resting jitter) ==");
    println!(
        "{:>7} {:>7} {:>6} {:>9} {:>9}",
        "stiff", "damp", "zeta", "settle_m", "jitter"
    );
    for (k, c) in grid() {
        let m = run(k, c, None, true);
        println!(
            "{:>7.0} {:>7.1} {:>6.2} {:>9.4} {:>9.6}{}",
            k,
            c,
            c / (2.0 * k.sqrt()),
            m.settled_error_m,
            m.jitter,
            if m.diverged { "  DIVERGED" } else { "" }
        );
    }
}

/// A dev parameter a player can reach from the Developer screen must not be
/// able to make the weapon leave the world, tunnel through a wall, or buzz in
/// a still hand. Guards the corners of the reachable range rather than the
/// whole grid.
///
/// Ignored, and that is the finding rather than a convenience: the drive reads
/// its stiffness/damping from the process-global [`dev_params`] registry every
/// step, so a test that exercises the range changes the simulation *other*
/// tests in the same process are running. Left un-ignored it makes
/// `held_melee_contacts_live_actor_without_solver_launch` fail intermittently
/// under `cargo test`'s parallel threads while passing in isolation. Held
/// melee cannot have a stability regression test in the ordinary suite until
/// the tuning is owned by `PhysicsWorld` instead of by a global.
#[test]
#[ignore = "mutates the global dev-param registry other physics tests simulate against"]
fn held_melee_drive_stays_bounded_across_the_tuning_range() {
    let bounds = |id| match dev_params::spec(id).kind {
        dev_params::DevParamKind::Float { min, max, .. } => (min, max),
    };
    let (k_min, k_max) = bounds(dev_params::MELEE_SPRING_STIFFNESS);
    let (c_min, c_max) = bounds(dev_params::MELEE_SPRING_DAMPING);

    for (k, c) in [
        (k_min, c_min),
        (k_min, c_max),
        (k_max, c_min),
        (k_max, c_max),
        (900.0, 60.0),
    ] {
        for wall in [None, Some(0.30)] {
            let m = run(k, c, wall, false);
            assert!(
                !m.diverged,
                "stiffness {k} damping {c} wall {wall:?}: the held weapon left the world"
            );
            assert!(
                m.penetration_m < 0.25,
                "stiffness {k} damping {c}: weapon drove {:.3} past the wall face",
                m.penetration_m
            );
        }
        let resting = run(k, c, None, true);
        assert!(
            resting.jitter < 0.01,
            "stiffness {k} damping {c}: a still hand buzzes (jitter {:.5})",
            resting.jitter
        );
    }
}

/// Per-frame trace of one configuration. The aggregate table says *how far*
/// the weapon trails the hand; this says *when*, which is what distinguishes
/// "a spring with a time constant" from "the angular motor is not tracking".
#[test]
#[ignore = "diagnostic trace - run explicitly"]
fn spring_trace_default() {
    for (k, c) in [(900.0f32, 60.0f32), (3000.0, 200.0)] {
        dev_params::set(dev_params::MELEE_SPRING_STIFFNESS, k);
        dev_params::set(dev_params::MELEE_SPRING_DAMPING, c);
        let (mut world, mut player) = world_with_floor();
        let (start, start_rot) = swing_pose(0.0);
        let (weapon, handle) = spawn_held_wrench(&mut world, start);
        for _ in 0..30 {
            world.set_position_rotation2(weapon, start, start_rot);
            world.update(vec3(0.0, 0.0, 0.0), &mut player);
        }
        println!("\n== trace: stiffness {k}, damping {c} ==");
        println!("{:>5} {:>9} {:>9}", "frame", "lag_m", "lag_deg");
        for frame in 0..(SWING_FRAMES + 24) {
            let t = (frame as f32 / SWING_FRAMES as f32).min(1.0);
            let (hand, hand_rot) = swing_pose(t);
            world.set_position_rotation2(weapon, hand, hand_rot);
            world.update(vec3(0.0, 0.0, 0.0), &mut player);
            let w = world.get_position(handle).unwrap();
            let wr = world.get_rotation(handle).unwrap();
            println!(
                "{:>5} {:>9.4} {:>9.2}",
                frame,
                (w - hand).magnitude(),
                angle_between(hand_rot, wr)
            );
        }
    }
}
