//! Headless measurement of the held-melee drive (#1053).
//!
//! A held melee weapon is a *dynamic* body driven onto an invisible
//! kinematic hand target, so how well it tracks - lag, orientation error,
//! resting jitter, contact penetration, recovery after an obstruction - is a
//! control question, and one a screenshot cannot answer. This measures it with
//! nothing but `PhysicsWorld`: no assets, no window, no game session.
//!
//! The tests here are ordinary, un-ignored ones and deliberately do not touch
//! the `dev_params` registry: the drive's only parameters are safety clamps
//! that never bind in these gestures, so the guard needs no global mutation
//! and cannot perturb tests running beside it. `melee_drive_trace` is the
//! ignored, human-facing entry point that prints the per-frame table:
//!
//! ```text
//! cargo test -p shock2vr --release --lib melee_drive_trace -- --ignored --nocapture
//! ```
//!
//! Historical note, because the numbers are the reason the drive changed: with
//! the six-axis generic-joint position motor this replaced, the same free
//! swing below trailed the hand by 0.14 units and its *orientation* error kept
//! growing for thirteen frames after the hand stopped, peaking near 88 degrees
//! and taking over 0.7 s to unwind. Rapier solves per-axis angular motors
//! independently, which is not a shortest-arc servo, and the body's origin
//! sits on the weapon head rather than in the grip, so linear and angular
//! motors fought through the offset. In a headset that read as heavy lag and
//! as a weapon pivoting about its head instead of about the hand.

use cgmath::{Deg, InnerSpace, Quaternion, Rotation, Rotation3, Vector3, vec3};
use rapier3d::prelude::*;
use shipyard::EntityId;

use super::{
    CollisionGroup, DynamicPhysicsOptions, PhysicsShape, PhysicsWorld, PlayerHandle,
    RigidBodyHandle,
};

/// A wrench-scale fitted melee volume. The body origin sits on the rendered
/// weapon head and the fitted cuboid extends back over the handle, which is
/// what production builds via `fit_held_item_cuboid`; the offset, elongated
/// shape is the inertia-awkward case a drive has to hold.
const WEAPON_HALF_EXTENTS: Vector3<f32> = Vector3::new(0.045, 0.30, 0.045);
const WEAPON_COLLIDER_CENTER: Vector3<f32> = Vector3::new(0.0, -0.30, 0.0);

/// The swing gesture every scenario shares: a 100 degree arc about the
/// shoulder in 0.3 s, at 0.7 units of reach. Coupled translation *and*
/// rotation, unlike a pure-translation test.
const SWING_ARC_DEG: f32 = 100.0;
const SWING_RADIUS: f32 = 0.7;
const SWING_FRAMES: usize = 18;
const HOLD_FRAMES: usize = 60;
const RETREAT_FRAMES: usize = 60;

/// Where the obstructing pane sits, perpendicular to the swing and past its
/// midpoint: the hand's x runs -0.54 -> +0.54 across the arc, so this stops
/// the second half while leaving the rest pose in free space.
const WALL_X: f32 = 0.30;

struct Metrics {
    /// Worst hand-to-weapon distance during the swing: the "the weapon is not
    /// where my hand is" the player actually feels.
    max_free_lag_m: f32,
    /// Worst orientation error over the same window.
    max_free_lag_deg: f32,
    /// Distance still remaining after the hand has been still for a second.
    settled_error_m: f32,
    /// RMS third difference of the weapon path over the settle window - the
    /// numeric form of visible buzz, since smooth motion cancels out of it.
    jitter: f32,
    /// Non-finite state, or a position past any sane bound, on any frame.
    diverged: bool,
    /// Deepest the weapon reached past the wall face (wall scenario only).
    penetration_m: f32,
    /// Frames to recover to within 2 cm of the hand after the hand withdraws.
    springback_frames: Option<usize>,
    /// Worst orientation error at any point in the *whole* run, obstruction
    /// included. A weapon that touches the world must not come away pointing
    /// somewhere else - that was reported from a headset as the weapon
    /// spinning out of the hand on contact with a bench.
    max_any_lag_deg: f32,
    /// Whether the weapon body was asleep at the end of the settle window. An
    /// exactly-zero resting jitter would otherwise be indistinguishable from a
    /// slept body, which is a very different claim.
    slept: bool,
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
    world.set_held_item_physical(weapon, CollisionGroup::held_melee());
    world.fit_held_item_cuboid(weapon, WEAPON_HALF_EXTENTS * 2.0, WEAPON_COLLIDER_CENTER);
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

/// `wall` puts a fixed pane across the swing's path; `hold_only` keeps the
/// hand still for the whole run.
fn run(wall: bool, hold_only: bool) -> Metrics {
    let (mut world, mut player) = world_with_floor();
    if wall {
        world.add_collider(
            EntityId::from_inner(3).unwrap(),
            ColliderBuilder::cuboid(0.05, 2.0, 2.0)
                .translation(vector![WALL_X, 1.2, 0.6])
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
        if wall {
            let w = samples.last().unwrap().weapon;
            if is_sane(w) {
                penetration = penetration.max(w.x - (WALL_X - 0.05));
            }
        }
    }
    let swung = samples.len();
    let slept = world
        .rigid_body_set
        .get(handle)
        .is_some_and(|body| body.is_sleeping());

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

    let max_any_lag_deg = samples
        .iter()
        .filter(|s| is_sane(s.weapon))
        .map(|s| angle_between(s.hand_rot, s.weapon_rot))
        .fold(0.0f32, f32::max);

    let free = &samples[..swung];
    let mut max_lag = 0.0f32;
    let mut max_lag_deg = 0.0f32;
    for s in free.iter().filter(|s| is_sane(s.weapon)) {
        max_lag = max_lag.max((s.weapon - s.hand).magnitude());
        max_lag_deg = max_lag_deg.max(angle_between(s.hand_rot, s.weapon_rot));
    }
    let settle = &samples[swung.saturating_sub(30)..swung];

    Metrics {
        max_free_lag_m: max_lag,
        max_free_lag_deg: max_lag_deg,
        settled_error_m: settle
            .last()
            .map(|s| (s.weapon - s.hand).magnitude())
            .unwrap_or(f32::NAN),
        jitter: jitter_of(settle),
        diverged,
        penetration_m: penetration,
        springback_frames: springback,
        max_any_lag_deg,
        slept,
    }
}

/// Free-space tracking is the whole point of driving by velocity: the weapon
/// the player sees must be where their hand is, through a real swing, not
/// trailing it by a spring's time constant. The joint motor this replaced
/// peaked at 0.14 units and 47 degrees mid-swing on this exact gesture.
#[test]
fn held_melee_tracks_the_hand_through_a_swing() {
    let m = run(false, false);
    assert!(!m.diverged, "the held weapon left the world");
    assert!(
        m.max_free_lag_m < 0.02,
        "the weapon trailed the hand by {:.4} units through the swing",
        m.max_free_lag_m
    );
    assert!(
        m.max_free_lag_deg < 5.0,
        "the weapon's orientation trailed the hand by {:.2} degrees",
        m.max_free_lag_deg
    );
    assert!(
        m.settled_error_m < 0.005,
        "the weapon never settled onto the still hand: {:.4}",
        m.settled_error_m
    );
}

/// The other half of the contract: converging on the hand must not cost the
/// weapon its physicality. World geometry still stops it dead, it must not
/// tunnel, and it must recover promptly once the obstruction is clear.
#[test]
fn held_melee_is_still_stopped_by_world_geometry() {
    let m = run(true, false);
    assert!(!m.diverged, "the held weapon left the world");
    assert!(
        m.penetration_m < 0.25,
        "the weapon drove {:.3} units past the wall face",
        m.penetration_m
    );
    let springback = m
        .springback_frames
        .expect("the weapon never returned to the hand after the wall cleared");
    assert!(
        springback < 20,
        "recovering from the obstruction took {springback} frames"
    );
    // The regression this drive exists to prevent. A dynamic weapon took
    // contact impulses through a body origin that sits out on the weapon head,
    // and spun out of the hand on any touch.
    assert!(
        m.max_any_lag_deg < 5.0,
        "hitting the wall turned the weapon {:.1} degrees off the hand",
        m.max_any_lag_deg
    );
}

/// A still hand must hold a still weapon. Buzz here is what reads as an
/// unstable grip in a headset even when nothing is moving.
#[test]
fn held_melee_is_still_in_a_still_hand() {
    let m = run(false, true);
    assert!(!m.diverged, "the held weapon left the world");
    assert!(
        m.jitter < 0.001,
        "a still hand buzzes (jitter {:.6}, slept {})",
        m.jitter,
        m.slept
    );
}

/// What the free-swing damage gate actually reads, for the two gestures it has
/// to tell apart. The 0.5 threshold `debug_melee` shipped with was calibrated
/// against the *spring* drive, where a swing peaked at 1.6 while walking is
/// ~1.8 - overlapping, which is why walking billed free hits. The swept drive
/// tracks the hand, so this prints whether the two have separated.
#[test]
#[ignore = "measurement - run explicitly"]
fn free_swing_speed_separation() {
    // A swing: the hand arcs and the weapon follows it.
    let (mut world, mut player) = world_with_floor();
    let (start, start_rot) = swing_pose(0.0);
    let (weapon, handle) = spawn_held_wrench(&mut world, start);
    for _ in 0..30 {
        world.set_position_rotation2(weapon, start, start_rot);
        world.update(vec3(0.0, 0.0, 0.0), &mut player);
    }
    let mut swing_peak = 0.0f32;
    for frame in 0..SWING_FRAMES {
        let (hand, hand_rot) = swing_pose(frame as f32 / SWING_FRAMES as f32);
        world.set_position_rotation2(weapon, hand, hand_rot);
        world.update(vec3(0.0, 0.0, 0.0), &mut player);
        let head = world.get_position(handle).unwrap();
        let v = world
            .velocity_at_point(weapon, head)
            .unwrap_or(vec3(0.0, 0.0, 0.0));
        swing_peak = swing_peak.max(v.magnitude());
    }

    // Walking: the hand holds still relative to the player, and the whole
    // pair translates. This is the gesture that must NOT bill damage.
    let (mut world, mut player) = world_with_floor();
    let (rest, rest_rot) = swing_pose(0.0);
    let (weapon, handle) = spawn_held_wrench(&mut world, rest);
    for _ in 0..30 {
        world.set_position_rotation2(weapon, rest, rest_rot);
        world.update(vec3(0.0, 0.0, 0.0), &mut player);
    }
    // 1.8 units/s is ordinary walking; step the hand by that per frame.
    let per_frame = 1.8 / 60.0;
    let mut walk_peak = 0.0f32;
    for frame in 0..30 {
        let carried = rest + vec3(per_frame * frame as f32, 0.0, 0.0);
        world.set_position_rotation2(weapon, carried, rest_rot);
        world.update(vec3(0.0, 0.0, 0.0), &mut player);
        let head = world.get_position(handle).unwrap();
        let v = world
            .velocity_at_point(weapon, head)
            .unwrap_or(vec3(0.0, 0.0, 0.0));
        walk_peak = walk_peak.max(v.magnitude());
    }

    println!("\nswing peak: {swing_peak:.3} u/s");
    println!("walk  peak: {walk_peak:.3} u/s");
    println!("separation ratio: {:.1}x", swing_peak / walk_peak.max(1e-6));
}

/// The per-frame table a human reads when the drive feels wrong. Ignored
/// because it prints rather than asserts.
#[test]
#[ignore = "diagnostic trace - run explicitly"]
fn melee_drive_trace() {
    for (label, wall) in [("free swing", false), ("into a wall", true)] {
        println!("\n== {label} ==");
        println!("{:>5} {:>9} {:>9}", "frame", "lag_m", "lag_deg");
        let (mut world, mut player) = world_with_floor();
        if wall {
            world.add_collider(
                EntityId::from_inner(3).unwrap(),
                ColliderBuilder::cuboid(0.05, 2.0, 2.0)
                    .translation(vector![WALL_X, 1.2, 0.6])
                    .build(),
            );
        }
        let (start, start_rot) = swing_pose(0.0);
        let (weapon, handle) = spawn_held_wrench(&mut world, start);
        for _ in 0..30 {
            world.set_position_rotation2(weapon, start, start_rot);
            world.update(vec3(0.0, 0.0, 0.0), &mut player);
        }
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

/// Walking is not swinging. A held weapon rides the player, so a player who
/// simply walks a still hand into a creature moves the weapon exactly as fast
/// as a swing does - and billed the same authored blow for it.
///
/// Negative-first: with the player's own motion left in, the sweep reports the
/// full walking speed here, five times the gate.
#[test]
fn walking_a_weapon_into_a_limb_is_not_a_swing() {
    let (mut world, mut player) = world_with_floor();
    let (weapon, _) = spawn_held_wrench(&mut world, vec3(0.0, 1.2, 0.0));

    let limb = EntityId::from_inner(7).unwrap();
    world.add_kinematic(
        limb,
        vec3(0.0, 1.2, 1.5),
        identity_quat(),
        Vector3::new(0.0, 0.0, 0.0),
        vec3(0.6, 0.6, 0.6),
        CollisionGroup::hitbox(),
        false,
    );

    // The hand holds still in front of the player; the player walks it into
    // the limb at the shipped walking speed (10 units/s at 60 Hz).
    let step = 10.0 / 60.0;
    let mut reported = None;
    for frame in 0..30 {
        let z = step * frame as f32;
        world.set_position_rotation2(weapon, vec3(0.0, 1.2, z), identity_quat());
        let (_, events) = world.update(vec3(0.0, 0.0, step), &mut player);
        for event in events {
            if let super::CollisionEvent::CollisionStarted {
                entity1_id,
                entity2_id,
                contact: Some(contact),
            } = event
            {
                if entity1_id == weapon && entity2_id == limb {
                    reported = Some(contact);
                }
            }
        }
        if reported.is_some() {
            break;
        }
    }

    let contact = reported.expect("the carried weapon should still report the limb it stopped on");
    let gate = crate::dev_params::spec(crate::dev_params::MELEE_FREE_SWING_SPEED).default;
    let speed = contact.closing_speed.unwrap_or(0.0);
    assert!(
        speed < gate,
        "walking must read below the swing gate {gate}, got {speed}"
    );
}

/// A relocation is not travel. The swing gate divides the player's own motion
/// out of a held weapon's, so a teleport that jumped the sampler would read as
/// hundreds of units/s for a frame and bill a free blow on arrival.
///
/// Negative-first: without re-seating the sampler this reads the whole jump
/// divided by one frame.
#[test]
fn relocating_the_player_is_not_player_velocity() {
    let (mut world, mut player) = world_with_floor();
    world.update(vec3(0.0, 0.0, 0.0), &mut player);

    world.set_player_translation(vec3(1050.0, 1000.0, 1000.0), &mut player);
    world.update(vec3(0.0, 0.0, 0.0), &mut player);

    let speed = world.player_velocity().magnitude();
    assert!(
        speed < 1.0,
        "a relocation must not register as player velocity, read {speed}"
    );
}

/// A swing stopped by a limb reports the blow, and reports it pointing the way
/// the weapon was going. The contact's normal drives the killing blow's ragdoll
/// impulse (`rag_doll::apply_killing_blow`), so an inverted one throws the
/// corpse back at the player.
///
/// Negative-first: `cast_shape` reports the normal *on the limb*, i.e. limb ->
/// weapon, which is the opposite of what a `CollisionContact` means.
#[test]
fn a_swing_stopped_by_a_limb_reports_it_pointing_at_the_limb() {
    let (mut world, mut player) = world_with_floor();
    let (weapon, _) = spawn_held_wrench(&mut world, vec3(0.0, 1.2, 0.0));

    // A limb straight ahead of the weapon, in the hitbox group.
    let limb = EntityId::from_inner(7).unwrap();
    world.add_kinematic(
        limb,
        vec3(0.0, 1.2, 1.5),
        identity_quat(),
        Vector3::new(0.0, 0.0, 0.0),
        vec3(0.6, 0.6, 0.6),
        CollisionGroup::hitbox(),
        false,
    );

    // Drive the hand through it, and collect what the sweep reported.
    let mut reported = None;
    for frame in 0..30 {
        let z = 0.1 * frame as f32;
        world.set_position_rotation2(weapon, vec3(0.0, 1.2, z), identity_quat());
        let (_, events) = world.update(vec3(0.0, 0.0, 0.0), &mut player);
        for event in events {
            if let super::CollisionEvent::CollisionStarted {
                entity1_id,
                entity2_id,
                contact: Some(contact),
            } = event
            {
                if entity1_id == weapon && entity2_id == limb {
                    reported = Some(contact);
                }
            }
        }
        if reported.is_some() {
            break;
        }
    }

    let contact = reported.expect("the swing should have reported the limb it stopped on");
    assert!(
        contact.normal.z > 0.5,
        "the normal should point from the weapon toward the limb (+z), got {:?}",
        contact.normal
    );
    assert!(
        contact.closing_speed.unwrap_or(0.0) > 0.0,
        "a swept blow carries the speed the sweep measured, got {:?}",
        contact.closing_speed
    );
}
