mod debug_render_pipeline;
mod physics_events;
pub(crate) mod util;

use collision::Aabb3;
use engine::profile;
use std::collections::{HashMap, HashSet};
use util::*;

use bitflags::bitflags;
use cgmath::{InnerSpace, Point3, Quaternion, Vector3, point3, vec3};
use dark::{SCALE_FACTOR, mission::SystemShock2Level};
use engine::scene::SceneObject;
use rapier3d::{
    control::{
        CharacterCollision, CharacterLength, EffectiveCharacterMovement,
        KinematicCharacterController,
    },
    na,
    na::UnitQuaternion,
    prelude::*,
};
use shipyard::EntityId;

use crate::game_scene::PlayerSavePoseError;

use physics_events::*;

use self::debug_render_pipeline::DebugRenderer;

/// Original standing player collision profile (SS2 ft): six feet tall and
/// 2.4 feet wide. Dark represents it as a vertical stack of spheres; a capsule
/// is the continuous equivalent and preserves the rounded traversal behavior.
const PLAYER_STANDING_HEIGHT: f32 = 6.0;
const PLAYER_STANDING_RADIUS: f32 = 1.2;

/// The standing collider's radius in world units, for callers that must place
/// something outside the player's own body (the camera sits on the capsule
/// axis, so "in front of the eye" is only outside the collider beyond this).
pub const PLAYER_STANDING_RADIUS_WORLD: f32 = PLAYER_STANDING_RADIUS / SCALE_FACTOR;

/// Height of the player's head sphere above the body origin (SS2 ft),
/// `(PLAYER_HEIGHT / 2) - PLAYER_RADIUS` in the original game's collision
/// profile. The original's first-person camera is anchored to this submodel,
/// so deriving it here keeps the camera tied to the collision profile rather
/// than floating free of it.
pub const PLAYER_HEAD_POS: f32 = PLAYER_STANDING_HEIGHT / 2.0 - PLAYER_STANDING_RADIUS;

/// Eye offset above the head sphere center (SS2 ft). The original game does
/// not put the camera at the head sphere center: after locating the head it
/// raises the viewpoint by this default eye offset (configurable as
/// "eyeloc"; our data ships no override). Head sphere (1.8) + this (0.8) puts
/// the standing eye 2.6 ft above the body center - 5.6 ft above the floor -
/// and still 0.4 ft below the capsule crown, so the camera stays inside the
/// collider by construction. See [`crate::PLAYER_EYE_HEIGHT`].
pub const PLAYER_EYE_OFFSET: f32 = 0.8;

/// Crouched capsule height (SS2 ft). The original engine's crouched COLLISION
/// profile (a stack of two 1.2 ft spheres, body-bottom -1.8 to head-top +1.0
/// around the object origin) is ~2.8 ft tall - its taller "crouch height" is
/// only the camera. The shorter collision profile leaves authored low routes
/// traversable while keeping the player's feet planted during the transition.
const PLAYER_CROUCH_HEIGHT: f32 = 2.8;
/// Keep the crouched footprint added in #513 unchanged at 1.6 feet wide. It
/// clears the authentic MedSci low passage while the wider standing profile
/// does not.
const PLAYER_CROUCH_RADIUS: f32 = 0.8;

/// Widening search used to rescue an obstructed authored placement (a respawn
/// marker whose capsule does not fit). The step is one standing radius, so two
/// rings clear a full capsule-diameter overlap, and the search reaches
/// `STEPS * STEP` = 6 SS2 ft - one player height - before giving up and using
/// the authored pose unchanged.
const PLAYER_PLACEMENT_SEARCH_STEP: f32 = PLAYER_STANDING_RADIUS / SCALE_FACTOR;
const PLAYER_PLACEMENT_SEARCH_STEPS: u32 = 5;
const PLAYER_PLACEMENT_SEARCH_DIRECTIONS: u32 = 8;
/// How far below a substitute pose's feet ground may be before it counts as
/// mid-air or a void. One player height: enough for the ordinary "the pose is
/// a little above its pad" case, short of dropping the player down a shaft.
const PLAYER_PLACEMENT_MAX_DROP: f32 = PLAYER_STANDING_HEIGHT / SCALE_FACTOR;

/// Margin (SS2 ft) for the stand-up headroom test capsule: its radius is
/// shrunk by this and its pose lifted by it, keeping the test top exactly at
/// the standing crown while floating the test bottom off the floor. Without
/// it, grazing contacts (the floor rest gap, walls the crouched capsule
/// already touches) would falsely refuse standing; anything the lateral
/// shrink lets through is well inside the controller's contact offset and
/// resolves over the next frames.
const PLAYER_STAND_TEST_MARGIN: f32 = 0.05;

/// How far (world units) the collider CENTER sits below its standing height
/// while crouched (the feet stay planted while the capsule shrinks). Save
/// code uses this to store a standing-equivalent center so a game saved
/// while crouched doesn't reload a standing capsule embedded in the floor.
pub fn player_crouch_center_shift() -> f32 {
    (PLAYER_STANDING_HEIGHT - PLAYER_CROUCH_HEIGHT) / 2.0 / SCALE_FACTOR
}

/// Clearance (SS2 ft) kept between a capped eye and the capsule crown, so the
/// camera stays strictly inside the collider rather than sitting exactly on
/// its surface where a coplanar ceiling could still catch it.
const PLAYER_EYE_CAP_MARGIN: f32 = 0.2;

/// Highest the eye may sit (world units) above the collider CENTER for a
/// stance: the capsule crown less [`PLAYER_EYE_CAP_MARGIN`]. VR runtimes clamp
/// tracked eye poses to this - a tracked head is a real body's head, not the
/// game capsule's, and a physically crouched adult's eye is well above the
/// short crouched capsule's crown. Uncapped, such a player sees over and
/// through geometry the capsule itself clears. Crouched, the cap IS the flat
/// runtime's [`crate::PLAYER_CROUCH_EYE_HEIGHT`] (2.6 ft above the feet), so
/// VR and flat can never disagree about the crouched eye line; standing it is
/// the crown less [`PLAYER_EYE_CAP_MARGIN`] (2.8 ft above the center), which
/// only an unusually tall player ever reaches, so tracking stays 1:1 in
/// practice.
pub fn player_eye_cap_above_center(crouched: bool) -> f32 {
    if crouched {
        crate::PLAYER_CROUCH_EYE_HEIGHT / SCALE_FACTOR
    } else {
        (PLAYER_STANDING_HEIGHT / 2.0 - PLAYER_EYE_CAP_MARGIN) / SCALE_FACTOR
    }
}

/// Height (world units) of the collider CENTER above the surface the player
/// stands on, per stance: half the capsule height plus the resting gap the
/// character controller maintains. VR runtimes subtract this from the body
/// position to get the feet/floor anchor for floor-relative tracked poses.
pub fn player_center_above_floor(crouched: bool) -> f32 {
    let height = if crouched {
        PLAYER_CROUCH_HEIGHT
    } else {
        PLAYER_STANDING_HEIGHT
    };
    (height / 2.0 + PLAYER_CONTACT_OFFSET + PLAYER_REST_LIFT) / SCALE_FACTOR
}

/// The player's standing collision capsule, in world units.
fn standing_player_capsule() -> Capsule {
    Capsule::new_y(
        (PLAYER_STANDING_HEIGHT / 2.0 - PLAYER_STANDING_RADIUS) / SCALE_FACTOR,
        PLAYER_STANDING_RADIUS / SCALE_FACTOR,
    )
}

/// [`standing_player_capsule`] as a collider shape.
fn standing_player_shared_shape() -> SharedShape {
    SharedShape::new(standing_player_capsule())
}

/// The player's crouched collision capsule, in world units.
fn crouched_player_capsule() -> Capsule {
    Capsule::new_y(
        (PLAYER_CROUCH_HEIGHT / 2.0 - PLAYER_CROUCH_RADIUS) / SCALE_FACTOR,
        PLAYER_CROUCH_RADIUS / SCALE_FACTOR,
    )
}

/// The player's crouched collision capsule, in world units.
fn crouched_player_shared_shape() -> SharedShape {
    SharedShape::new(crouched_player_capsule())
}

/// Gap (SS2 ft) the character controller keeps between the player collider
/// and world geometry (`KinematicCharacterController::offset`).
const PLAYER_CONTACT_OFFSET: f32 = 0.1;

/// Extra height (SS2 ft) the player is lifted each grounded frame, keeping the
/// resting gap a hair above `PLAYER_CONTACT_OFFSET`. Load-bearing: movement
/// casts use `PLAYER_CONTACT_OFFSET` as their target distance, so a capsule
/// resting at exactly that gap starts every cast already "in contact" with
/// its own floor. On a yaw-ROTATED support (e.g. the earth.mis tram floor
/// slab) the rotated top-face normal carries ~1e-6 float error, which makes
/// even purely tangential walking read as "approaching" - every solver
/// iteration then re-hits the same contact at toi=0, applies zero
/// translation, and the player freezes in place. (Axis-aligned floors yield
/// an exact (0,1,0) normal, where tangential motion reports no hit - which is
/// why flat test floors work without this.) The next frame's gravity pass
/// consumes the lift again, so the rest height is stable. Regression-tested
/// by `player_walks_on_rotated_platform`.
const PLAYER_REST_LIFT: f32 = 0.01;

/// Half-life for gravity-induced horizontal displacement after a slope hands
/// the player onto walkable flat ground. Dark's dynamic player coasts through
/// short flat seams instead of losing all tangent motion instantly; damping
/// makes that carry finite for the kinematic controller.
const PLAYER_SLOPE_FLAT_DAMPING_HALF_LIFE: f32 = 0.25;

/// Below half a millimeter of per-frame horizontal displacement, slope carry
/// is at rest. This also absorbs the tiny normal jitter emitted at trimesh
/// triangle edges after the actual slide has ended.
const PLAYER_SLOPE_DISPLACEMENT_EPSILON_SQUARED: f32 = 2.5e-7;

/// Treat contacts shallower than about five degrees as flat. Triangle-edge
/// shape casts can return slightly tilted normals even on a planar landing;
/// accepting those as real slopes would continually regenerate tiny motion.
const PLAYER_SLOPE_MIN_HORIZONTAL_NORMAL_SQUARED: f32 = 0.0076;

/// A floor contact must have a meaningful upward normal. This still covers
/// authored chute faces up to about 75 degrees while excluding near-vertical
/// walls whose shape-cast normal has tiny upward numerical noise.
const PLAYER_SLOPE_MIN_FLOOR_NORMAL: f32 = 0.25;

fn slope_ground_probe_distance(shape: &dyn Shape) -> Real {
    let contact_margin = (PLAYER_CONTACT_OFFSET + PLAYER_REST_LIFT) / SCALE_FACTOR;
    if let Some(capsule) = shape.as_capsule() {
        // For a plane whose upward-normal component is n, the vertical
        // center-to-plane distance at contact is:
        //
        //   segment_half_height + (radius + contact_margin) / n
        //
        // Size the probe for the steepest face we classify as a floor. This
        // matters most for crouching: simply doubling its short vertical AABB
        // does not reach the plane under the capsule's rounded side.
        capsule.half_height() + (capsule.radius + contact_margin) / PLAYER_SLOPE_MIN_FLOOR_NORMAL
    } else {
        // Player locomotion uses capsules, but keep a conservative fallback
        // for any future shape passed through this shared movement primitive.
        2.0 * shape.compute_local_aabb().half_extents().y + contact_margin
    }
}

/// Maximum ledge height (SS2 ft) the player steps up automatically, and how
/// far below their feet the ground is snapped to when walking down. 2 ft is
/// the original engine's step-probe height, so stairs climb and descend
/// without a jump.
const PLAYER_STEP_HEIGHT: f32 = 2.0;

/// Ordinary jump launch speed and downward acceleration, in SS2 feet per
/// second(/squared). These produce a short, fixed-height hop while preserving
/// the existing terminal fall speed. The authored shodan.mis final barrier is
/// deliberately taller than the 2 ft stair probe and needs this separate
/// player action to clear.
const PLAYER_JUMP_SPEED: f32 = 28.0;
const PLAYER_JUMP_GRAVITY: f32 = 40.0;
const PLAYER_MAX_FALL_SPEED: f32 = 30.0;
/// Maximum forward search for a jump-through landing. This is deliberately a
/// short body-scale transition, not a general wall bypass.
const PLAYER_JUMP_MANTLE_FORWARD: f32 = 8.0;
const PLAYER_JUMP_MANTLE_PROBE_STEP: f32 = 0.5;
/// Maximum authored drop a sparse-body jump transition may recover, in SS2
/// feet. SHODAN's stacked final-descent cells put the lower side ring about
/// 34 feet below the upper floor; keeping this below one room-scale span makes
/// the exception local instead of a general search for arbitrary floors.
const PLAYER_JUMP_MANTLE_MAX_DROP: f32 = 40.0;
/// Lowest upward normal the player treats as walkable. SHODAN's side ring is
/// an authored 45-degree tread (normal y ~= 0.707); the small margin prevents
/// imported float normals from landing just beyond Rapier's exact pi/4
/// climb/slide boundary.
const PLAYER_MIN_WALKABLE_NORMAL: f32 = 0.7;

/// How far below the player's feet (world units) a surface still counts as the
/// thing they are *standing on* for support-motion transfer (see
/// [`PlayerSupport`]). Comfortably clears the controller's contact offset plus
/// the resting lift the player settles at (~0.044 wu) while staying far short
/// of a step (0.8 wu), so a platform carries the player only while they are
/// genuinely on it - never while they are airborne above it.
const SUPPORT_PROBE_DISTANCE: f32 = 0.1;

/// How horizontal a probed surface must be to count as support: the same
/// walkable-ground threshold the mantle probe uses (~44 degrees, just inside
/// the controller's default 45-degree slide angle). A surface the player would
/// slide down is not one they ride, and a wall never is.
const SUPPORT_MIN_GROUND_NORMAL: f32 = CLIMB_TOP_OUT_MIN_GROUND_NORMAL;

fn player_character_controller() -> KinematicCharacterController {
    let mut controller = KinematicCharacterController::default();
    controller.offset = CharacterLength::Absolute(PLAYER_CONTACT_OFFSET / SCALE_FACTOR);
    // Walking down stairs stays grounded (snapped onto the next tread)
    // instead of chaining micro-falls. Deliberate tradeoff: this also
    // absorbs any intended drop up to the step height (the player glues
    // to <= 2 ft ledges rather than falling) - revisit when gravity
    // becomes an integrated velocity. Stepping UP is handled by an
    // explicit probe in `move_player` (see `try_step_up`) - Rapier's
    // built-in autostep needs a wall-classified contact, but a capsule
    // touching a step edge above its bottom-sphere center reads as a
    // ceiling and never triggers it.
    controller.snap_to_ground = Some(CharacterLength::Absolute(PLAYER_STEP_HEIGHT / SCALE_FACTOR));
    let max_walkable_angle = PLAYER_MIN_WALKABLE_NORMAL.acos();
    controller.max_slope_climb_angle = max_walkable_angle;
    controller.min_slope_slide_angle = max_walkable_angle;
    controller
}

/// Horizontal reach (world units) for ladder detection: the player grips a
/// climbable surface when their collider, inflated by this much radially,
/// overlaps it. Standing flush against a ladder leaves a small gap between the
/// collider and the rungs, so the un-inflated shapes never intersect.
const CLIMB_REACH: f32 = 1.0 / SCALE_FACTOR;

/// Climb speed as a fraction of walk speed (the into-ladder input component is
/// redirected to vertical movement at this scale).
const CLIMB_SPEED_SCALE: f32 = 0.6;

/// How much of the desired horizontal movement must point INTO the climbable
/// surface before the player grips it: 0.5 is a 60 degree cone around the
/// contact normal. Below it the push is mostly along the surface, which is
/// someone walking PAST a wall-mounted ladder, not climbing it - they keep
/// walking (and keep gravity). Since a grip now consumes the whole horizontal
/// input (see [`climb_redirect`]), without this floor a ladder set into a
/// corridor wall would act as flypaper: a grazing input would zero the player's
/// forward progress and creep them up the wall instead.
const MIN_CLIMB_GRIP_FRACTION: f32 = 0.5;

/// How much of the requested climb the cast must actually achieve for the frame
/// to count as climbing rather than walking (see [`step_player_movement`]).
/// Anything at or below this is a redirect wedged into solid geometry, not a
/// climb.
const CLIMB_MIN_PROGRESS_FRACTION: f32 = 0.1;

/// Dark probes one head diameter beyond the cleared lip, then advances the
/// compressed player another body diameter.
const CLIMB_TOP_OUT_PROBE_FORWARD: f32 = 2.0 * CLIMB_TOP_OUT_RADIUS;
const CLIMB_TOP_OUT_ADVANCE: f32 = 2.0 * PLAYER_STANDING_RADIUS / SCALE_FACTOR;

/// Dark's mantle probe goes 3.5 units up, then seven down.
const CLIMB_TOP_OUT_UP: f32 = 3.5 / SCALE_FACTOR;
const CLIMB_TOP_OUT_MAX_DROP: f32 = 7.0 / SCALE_FACTOR;
const CLIMB_TOP_OUT_RADIUS: f32 = PLAYER_STANDING_RADIUS / SCALE_FACTOR;
const CLIMB_TOP_OUT_FINAL_DROP: f32 = 4.0 / SCALE_FACTOR;
const CLIMB_TOP_OUT_MIN_GROUND_NORMAL: f32 = 0.72;
const CLIMB_TOP_OUT_RECOVERY_RETREAT: f32 = PLAYER_CONTACT_OFFSET / SCALE_FACTOR;
const CLIMB_TOP_OUT_MAX_RECOVERY_RETREAT: f32 = 4.0 / SCALE_FACTOR;

/// Begin probing only near the highest climbable in the current ladder column.
/// The expanded column query sees the next segment in stacked ladders, avoiding
/// a false "top" at every rung seam. Include one scripted substep and the
/// controller gap: rick1's vertical cast stops with the capsule crown just over
/// one half-height below the authored top, before the nominal crown-only
/// threshold can ever become true.
const CLIMB_TOP_OUT_TOP_REACH: f32 = PLAYER_STANDING_HEIGHT / 2.0 / SCALE_FACTOR
    + CLIMB_TOP_OUT_SUBSTEP
    + PLAYER_CONTACT_OFFSET / SCALE_FACTOR;
const CLIMB_TOP_OUT_COLUMN_LOOKAHEAD: f32 =
    CLIMB_TOP_OUT_UP + PLAYER_STANDING_HEIGHT / SCALE_FACTOR;

/// Dark's jump-through phase moves at ten SS2 feet per second. The physics
/// runtime advances at a fixed 60 Hz.
const CLIMB_TOP_OUT_SUBSTEP: f32 = 10.0 / SCALE_FACTOR / 60.0;

/// Half-Life-style flat ladder movement: convert the into-ladder component of
/// the desired movement into a vertical climb. `toward_ladder` is the
/// horizontal unit normal from the player toward the climbable surface (from
/// the contact query - NOT the collider-center direction, which tilts away from
/// the surface whenever the player is off-center and so mis-measures both the
/// grip test and the climb speed). Returns `None` when the player is not
/// pushing mostly toward the ladder (no grip - normal walking/gravity
/// applies). Looking down (a downward-pitched desired movement) descends
/// instead of ascending, so the same input walks down a shaft ladder; the sign
/// flip at the pitch threshold matches classic ladder feel.
///
/// **A grip climbs straight up the face: the horizontal movement is dropped
/// entirely, not just its into-ladder component.** The into-ladder part has to
/// go because the character controller's slope limiting treats a vertical
/// climbable face as an unclimbable slope and cancels the ascent when the
/// player also pushes into it (empirically: ascent drops from ~7u to ~0.3u over
/// 240 frames). Passing the remaining ALONG-face part through is what broke
/// issue #596: shipped ladders are ~0.9 world units wide, so a few degrees of
/// heading error slides the player off the side at walk speed (10 wu/s x sin
/// theta) in a fraction of the ~0.5 s the 6 wu/s climb needs - and once the
/// capsule is past the ladder's edge the contact normal turns diagonal, so even
/// a dead-on push resolves into a sideways shove that carries the player
/// further off (a positive feedback loop: measured as a stall after ~1.4 wu
/// with the player shoved ~1.1 wu sideways, on stacked-rung AND single-collider
/// ladders alike). Climbing straight up removes both: the redirect can no
/// longer steer the player off the surface it is gripping. Stepping off at the
/// top is unaffected - the grip drops once the capsule clears the ladder's top
/// and normal walking carries the player onto the landing.
///
/// The vertical component of `desired` (head pitch / debug fly channel) is
/// dropped so climb speed depends only on the into-ladder push.
fn climb_redirect(desired: Vector<Real>, toward_ladder: Vector<Real>) -> Option<Vector<Real>> {
    let desired_h = vector![desired.x, 0.0, desired.z];
    let speed = desired_h.norm();
    if speed <= 1e-6 {
        return None;
    }
    let into = desired_h.dot(&toward_ladder);
    if into < MIN_CLIMB_GRIP_FRACTION * speed {
        return None;
    }
    let vertical = if desired.y < -0.25 * into {
        -into
    } else {
        into
    };
    Some(Vector::y() * vertical * CLIMB_SPEED_SCALE)
}

/// Original mantling follows projected player-facing, not a strafe direction.
/// Require the grip input to contain a forward component so a sideways brush
/// against a ladder cannot start a mantle.
fn climb_top_out_direction(desired: Vector<Real>, facing: Vector<Real>) -> Option<Vector<Real>> {
    let desired = vector![desired.x, 0.0, desired.z].try_normalize(1.0e-6)?;
    let facing = vector![facing.x, 0.0, facing.z].try_normalize(1.0e-6)?;
    (desired.dot(&facing) >= MIN_CLIMB_GRIP_FRACTION).then_some(facing)
}

/// How much (SS2 ft) the step probe's clearance sweeps shrink the player
/// capsule's RADIUS. The player comes to rest `PLAYER_CONTACT_OFFSET` from
/// whatever it touches, so a full-width sweep travelling PARALLEL to a nearby
/// surface - straight up the riser face it stands against, or forward past the
/// wall beside it - reports that surface as a grazing hit at toi ~ 0 and the
/// probe aborts on an obstruction that isn't in its way. That is what rejected
/// an otherwise legal 1.5 ft step onto the station.mis service-hall ledge and
/// blocked the route out of the Station training level (issue #499).
///
/// Only the radius shrinks - the capsule's HEIGHT is preserved, so these stay
/// genuine swept tests and a shelf crossed part-way up the lift still blocks.
/// Half the resting gap is the margin: it clears a player sitting at the
/// nominal offset, and tolerates up to `PROBE_SKIN` of penetration beyond it
/// before a graze can register again. Widening it further would start hiding
/// real geometry alongside the player, so it is deliberately small.
const PROBE_SKIN: f32 = PLAYER_CONTACT_OFFSET / 2.0;

/// How much of the input must survive projection onto a blocking wall before
/// the step probe follows the slide. `dir` is a unit vector, so this is the
/// tangential fraction: 0.2 rejects an input within ~11 degrees of head-on
/// into the wall, where the sideways component is incidental rather than
/// intended. Without the floor, normalizing a near-zero tangent would turn
/// "walked straight into a wall" into a full `forward`-sized sideways hop onto
/// whatever happens to sit beside the player.
///
/// Keep this WELL BELOW 0.28: the station.mis repro that motivated the
/// deflection (input `normalize(-1.38, 0, 0.4)` against the x-facing west
/// wall) has a tangential fraction of ~0.278, so raising the threshold to
/// there or beyond silently reinstates issue #499.
///
/// Note the resulting step can still deviate a long way from the raw input
/// (at the threshold, ~78 degrees) - that is inherent to sliding along a wall,
/// and matches what the character controller's own movement does with the same
/// input; the guard is against deflecting on noise, not against large angles.
const MIN_SLIDE_FRACTION: f32 = 0.2;

/// Step-up probe (the original engine's stair-climbing approach: probe up,
/// forward, then down from the blocked position). Called when the player's
/// horizontal movement was mostly blocked; returns the extra translation that
/// hops the player onto a stair-sized ledge ahead, or `None` when there is no
/// steppable ledge (a full wall, no headroom, or a too-tall/too-steep step).
///
/// This exists because rapier's built-in autostep never fires against a step
/// with a capsule: the step's top edge contacts the bottom sphere above its
/// center, which parry classifies as a ceiling-ish hit, not a wall.
///
/// `pos` is the collider pose after the blocked move; `desired` the movement
/// input for the frame; `applied` the translation the move actually achieved.
/// The probe (all casts collision-checked, so the result is a valid pose):
/// 1. headroom: find lift candidates up to `PLAYER_STEP_HEIGHT`;
/// 2. clearance: at a viable height it must fit forward far enough to plant the
///    capsule axis past the riser face (radius + contact gaps) - otherwise
///    the overhanging capsule gets pulled back down by snap-to-ground;
/// 3. tread: dropping back down must land on a walkable (mostly-horizontal)
///    surface above the starting feet - the landing defines the step height.
fn try_step_up(
    queries: &QueryPipeline,
    shape: &dyn Shape,
    pos: &Isometry<Real>,
    desired: Vector<Real>,
    applied: Vector<Real>,
) -> Option<Vector<Real>> {
    let desired_h = vector![desired.x, 0.0, desired.z];
    let desired_norm = desired_h.norm();
    if desired_norm < 1.0e-6 {
        return None;
    }
    let dir = desired_h / desired_norm;
    // Not blocked: the move achieved most of the desired horizontal distance.
    if applied.dot(&dir) > 0.5 * desired_norm {
        return None;
    }

    let step_height = PLAYER_STEP_HEIGHT / SCALE_FACTOR;
    let contact_offset = PLAYER_CONTACT_OFFSET / SCALE_FACTOR;
    // Far enough forward that the capsule axis (its lowest point) stands on
    // the tread: the capsule radius, the gap to the riser (which can exceed
    // the contact offset when the walk stalled early), and margin on top.
    let capsule_radius = shape
        .as_capsule()
        .map_or(PLAYER_STANDING_RADIUS / SCALE_FACTOR, |capsule| {
            capsule.radius
        });
    let forward = capsule_radius + 4.0 * contact_offset;
    // Full-width cast, used for the LANDING sweep: the tread height it
    // measures depends on the real capsule bottom, and its `target_distance`
    // is the contact offset so the player comes to rest at the normal gap
    // above the tread.
    let cast = |from: &Isometry<Real>, dir: Vector<Real>, max_dist: f32, target: f32| {
        queries.cast_shape(
            from,
            &dir,
            shape,
            rapier3d::parry::query::ShapeCastOptions {
                max_time_of_impact: max_dist,
                target_distance: target,
                stop_at_penetration: false,
                compute_impact_geometry_on_penetration: true,
            },
        )
    };
    // Clearance sweeps (up, forward) use a slightly narrower capsule so that
    // surfaces the player is merely resting against don't read as
    // obstructions - see `PROBE_SKIN` for why, and why only the radius shrinks.
    let narrowed = shape.as_capsule().map(|c| {
        Capsule::new(
            c.segment.a,
            c.segment.b,
            c.radius - PROBE_SKIN / SCALE_FACTOR,
        )
    });
    let narrow_shape: &dyn Shape = narrowed.as_ref().map_or(shape, |c| c as &dyn Shape);
    let cast_narrow = |from: &Isometry<Real>, dir: Vector<Real>, max_dist: f32| {
        queries.cast_shape(
            from,
            &dir,
            narrow_shape,
            rapier3d::parry::query::ShapeCastOptions {
                max_time_of_impact: max_dist,
                target_distance: 0.0,
                stop_at_penetration: false,
                compute_impact_geometry_on_penetration: true,
            },
        )
    };

    // 1) Measure headroom directly above. Requiring the entire maximum lift
    // rejects smaller legal steps under a ceiling: earth.mis's
    // tram-to-boardwalk lip needs only ~0.7 SS2 ft, but has just under the full
    // two-foot probe distance above the six-foot player. Stay one controller
    // contact offset below a hit because the clearance cast uses the narrowed
    // shape; the final full-width overlap remains the authoritative fit check.
    let max_lift = cast_narrow(pos, Vector::y(), step_height)
        .map(|(_, hit)| (hit.time_of_impact - contact_offset).max(0.0))
        .unwrap_or(step_height);
    if max_lift < 0.05 / SCALE_FACTOR {
        return None;
    }

    let try_lift = |lift_height: f32| -> Option<Vector<Real>> {
        let lifted = Translation::from(Vector::y() * lift_height) * pos;
        // 2) Forward clearance at the lifted height, and 3) drop onto the
        // tread. `land` assumes the forward path for `d` is already clear.
        let land = |d: Vector<Real>| -> Option<Vector<Real>> {
            let planted = Translation::from(d * forward) * lifted;
            let (_, hit) = cast(&planted, -Vector::y(), lift_height, contact_offset)?;
            let lift = lift_height - hit.time_of_impact;
            // Too small to matter (the rounded capsule bottom slides over it
            // anyway), or a downward/steep landing normal (not a tread). The
            // threshold sits just above cos(45 deg) so the probe can't hop up
            // slopes the controller's slope limit (default 45 deg) refuses to
            // walk.
            if lift < 0.05 / SCALE_FACTOR || hit.normal1.y < 0.72 {
                return None;
            }
            // The clearance sweeps above ran NARROWED, which is what lets them
            // ignore surfaces the player is merely resting against - but it
            // also means they would miss a surface that the real, full-width
            // capsule would clip by up to `PROBE_SKIN` at the landing pose.
            // Check the final pose at full width so the hop still lands the
            // player in a genuinely valid pose, preserving the invariant that
            // every applied translation comes from a collision-checked query
            // (see `step_player_movement`).
            let step = Vector::y() * lift + d * forward;
            if queries
                .intersect_shape(Translation::from(step) * pos, shape)
                .next()
                .is_some()
            {
                return None;
            }
            Some(step)
        };

        // Straight ahead: if nothing blocks the lifted path, the landing
        // decides.
        let Some((_, wall)) = cast_narrow(&lifted, dir, forward) else {
            return land(dir);
        };
        // Blocked by a WALL: the player walking into it would slide along it,
        // so probe the slide direction too - mirroring what the character
        // controller's own movement does. Without this, a diagonal push into
        // a corner whose forward face is a wall but whose side is a steppable
        // ledge never steps at all: on the station.mis service hall, pushing
        // northwest wedges the player against the west wall instead of
        // climbing the ledge to the north (issue #499). Only a
        // mostly-vertical face deflects; a steep slope or tread is the straight
        // path's business, handled above.
        if wall.normal1.y.abs() > 0.72 {
            return None;
        }
        let n_h = vector![wall.normal1.x, 0.0, wall.normal1.z];
        let n_norm = n_h.norm();
        if n_norm < 1.0e-3 {
            return None;
        }
        // Project the input onto the wall face (n must be unit for the
        // rejection to be correct). `dir` is a unit vector, so `slide_norm` is
        // exactly the tangential fraction of the input - require a real
        // sideways intent before hopping sideways.
        let n = n_h / n_norm;
        let slide = dir - n * dir.dot(&n);
        let slide_norm = slide.norm();
        if slide_norm < MIN_SLIDE_FRACTION {
            return None;
        }
        let slide_dir = slide / slide_norm;
        if cast_narrow(&lifted, slide_dir, forward).is_some() {
            return None;
        }
        land(slide_dir)
    };

    // Start with a small half-foot probe and increase only as needed. That
    // avoids scraping a nearby ceiling for a short step while reaching the
    // original two-foot step limit in at most four attempts.
    let increment = 0.5 / SCALE_FACTOR;
    let mut lift_height = increment.min(max_lift);
    loop {
        if let Some(step) = try_lift(lift_height) {
            return Some(step);
        }
        if lift_height >= max_lift - 1.0e-5 {
            break;
        }
        lift_height = (lift_height + increment).min(max_lift);
    }
    None
}

/// One frame of gripped ladder movement: the vertical redirect from
/// [`climb_redirect`], plus the query pipeline it is cast against.
///
/// The climb cast gets its OWN pipeline because it must ignore **climbable
/// colliders themselves**. A grip slides the player vertically along the face it
/// holds, so the ladder standing in that path is not an obstacle - but to the
/// character controller it is ordinary solid geometry, and a capsule that grazes
/// it (which is exactly where a gripped player sits) has its vertical cast
/// stopped dead by the horizontal cap of whatever rung it overlaps. Measured on
/// the hydro2 Sector-C stack (eleven 0.8-spaced `Rick Ladder` rungs at
/// `(59.6, -2.0..6.0, 22.2)`): descending from the top, the climb cast returned
/// **zero** translation at y 3.27 and the player hung there for as long as the
/// input was held. The walk and gravity passes are untouched, so a ladder stays
/// solid to anyone who is not climbing it, and the climb still collides with
/// real geometry (the floor at the shaft's foot, a ceiling above) - which is
/// what keeps the `CLIMB_MIN_PROGRESS_FRACTION` walk fallback working.
///
/// Two things this deliberately does not do: the exclusion covers EVERY
/// climbable, not just the gripped one - stacked rungs are separate entities, so
/// there is no single collider or body to scope the exclusion to, and scoping it
/// to the rung that supplied the grip would reinstate the bug on the rung above
/// or below it. (A second, unrelated ladder in the same vertical path would also
/// be passed through; the shipped maps have no such case.)
struct ClimbPass<'a> {
    movement: Vector<Real>,
    top_out: Option<(Vector<Real>, Real)>,
    validation_queries: QueryPipeline<'a>,
    probe_queries: QueryPipeline<'a>,
    scripted_queries: QueryPipeline<'a>,
}

#[derive(Clone, Copy)]
struct ClimbTopOut {
    waypoints: [Vector<Real>; 7],
    next_waypoint: usize,
    save_pose: Vector<Real>,
    reversing: bool,
    is_crouched: bool,
}

struct PlayerMovement {
    movement: EffectiveCharacterMovement,
    /// `movement.translation` with the moving-platform carry removed, i.e.
    /// what the player did under their own power. Only the ordinary walk pass
    /// folds a carry in at all, so the scripted mantle and ladder branches
    /// report their translation unchanged - subtracting a carry there would
    /// fabricate motion the player never made.
    self_translation: Vector<Real>,
    /// This frame was a ladder climb or a scripted mantle rather than walking
    /// on a floor. The player is on a surface they are holding, so they are
    /// neither striding nor free-falling however far they travel.
    is_climbing: bool,
    top_out: Option<ClimbTopOut>,
    /// Horizontal part of a gravity-induced slope slide, carried into the
    /// next gravity pass so a seam does not erase the player's momentum.
    slope_displacement: Vector<Real>,
    /// Live actor contacts from the player's intentional horizontal walk.
    /// These are resolved against the dynamic bodies after the immutable
    /// character-controller query is finished.
    actor_collisions: Vec<CharacterCollision>,
}

/// The moving-terrain body the player is standing on, and where it was the
/// last time we looked. Its per-frame displacement is handed to the player as
/// a movement pass, so a rider travels *exactly* with a tram, a lift or an
/// elevator instead of sliding around on its deck. This replaces the
/// controller's own contact-based transfer - see
/// [`PhysicsWorld::player_movement_queries`] for what that got wrong.
///
/// Deliberately limited to **kinematic** bodies: those are the script-driven
/// platforms (`BaseElevator`, doors), whose motion is authored and exact.
/// Dynamic bodies are excluded - the player's own weight is not simulated
/// against them, so carrying off a jittering ragdoll limb or a settling crate
/// would inject that noise straight into the camera.
///
/// Translation only. Every moving platform in the shipped data translates:
/// `BaseElevator` emits `Effect::SetPosition` and never a rotation, and no
/// authored elevator path turns. Orbiting a rotating support would also have
/// to turn the player's *facing*, which lives outside physics - so rotational
/// carry is deliberately deferred rather than half-implemented.
#[derive(Clone, Copy)]
struct PlayerSupport {
    body: RigidBodyHandle,
    translation: Vector<Real>,
}

/// Collision filter shared by every player movement cast: collide with the
/// collidable groups as the player, ignore the player's own body and all
/// sensors.
fn player_movement_filter(character_handle: RigidBodyHandle) -> QueryFilter<'static> {
    QueryFilter::new()
        .groups(InteractionGroups::new(
            InternalCollisionGroups::PLAYER.bits.into(),
            InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            Default::default(),
        ))
        .exclude_rigid_body(character_handle)
        .exclude_sensors()
}

/// The kinematic body the player is standing on at `pos`, if any: a short
/// downward shape cast that has to land on a near-horizontal surface.
///
/// Probing for the *surface underfoot* - rather than taking any contact - is
/// what keeps a wall or a door sliding past the player from dragging them
/// along with it.
///
/// The cast only sees kinematic-bodied colliders, because it is looking for
/// something that can move, and asking for the nearest collider of any kind
/// would let an immobile one mask it: a tram deck parked flush with its station
/// floor is exactly that geometry, and a rider would lose their support to the
/// platform underneath at the moment of departure.
fn detect_support(
    queries: &QueryPipeline,
    bodies: &RigidBodySet,
    shape: &dyn Shape,
    pos: &Isometry<Real>,
) -> Option<PlayerSupport> {
    let is_kinematic = |_handle: ColliderHandle, collider: &Collider| {
        collider
            .parent()
            .and_then(|parent| bodies.get(parent))
            .is_some_and(RigidBody::is_kinematic)
    };
    let (handle, hit) = queries
        .with_filter(queries.filter.predicate(&is_kinematic))
        .cast_shape(
            pos,
            &-Vector::y(),
            shape,
            rapier3d::parry::query::ShapeCastOptions {
                max_time_of_impact: SUPPORT_PROBE_DISTANCE,
                target_distance: 0.0,
                stop_at_penetration: false,
                compute_impact_geometry_on_penetration: true,
            },
        )?;
    // `normal1` is the normal on the world collider (the pipeline is shape 1
    // of the cast), the same one the controller's own floor/wall test reads.
    if hit.normal1.y < SUPPORT_MIN_GROUND_NORMAL {
        return None;
    }
    let body = queries.colliders.get(handle)?.parent()?;
    Some(PlayerSupport {
        body,
        translation: *bodies.get(body)?.translation(),
    })
}

/// Distance along a horizontal ray at which it exits a climbable AABB expanded
/// by the player's required clearance. `None` means the ray never crosses the
/// expanded box.
///
/// This is a slab intersection, not a support-point projection. A diagonal
/// route clears a wide-X/thin-Z ladder as soon as it leaves the thin Z slab; it
/// does not need to pass the AABB's far corner.
fn climbable_aabb_exit_distance(
    origin: Vector<Real>,
    direction: Vector<Real>,
    aabb: &rapier3d::parry::bounding_volume::Aabb,
    clearance: Real,
) -> Option<Real> {
    let mut enter: Real = 0.0;
    let mut exit: Real = Real::INFINITY;
    for (origin, direction, min, max) in [
        (origin.x, direction.x, aabb.mins.x, aabb.maxs.x),
        (origin.z, direction.z, aabb.mins.z, aabb.maxs.z),
    ] {
        let min = min - clearance;
        let max = max + clearance;
        if direction.abs() <= 1.0e-6 {
            if origin < min || origin > max {
                return None;
            }
            continue;
        }
        let t0 = (min - origin) / direction;
        let t1 = (max - origin) / direction;
        enter = enter.max(t0.min(t1));
        exit = exit.min(t0.max(t1));
    }
    (exit >= enter && exit >= 0.0).then_some(exit.max(0.0))
}

fn scripted_character_movement(translation: Vector<Real>) -> EffectiveCharacterMovement {
    EffectiveCharacterMovement {
        translation,
        grounded: false,
        is_sliding_down_slope: false,
    }
}

/// Query filter for testing a player pose against everything that blocks the
/// player, excluding the player's own body.
fn player_pose_filter<'a>(character_handle: RigidBodyHandle) -> QueryFilter<'a> {
    QueryFilter::new()
        .groups(InteractionGroups::new(
            InternalCollisionGroups::PLAYER.bits.into(),
            InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            Default::default(),
        ))
        .exclude_rigid_body(character_handle)
        .exclude_sensors()
}

fn shape_intersects(queries: &QueryPipeline, position: Vector<Real>, shape: &dyn Shape) -> bool {
    queries
        .intersect_shape(
            Isometry::translation(position.x, position.y, position.z),
            shape,
        )
        .next()
        .is_some()
}

/// Whether an expanded player shape is genuinely resting on walkable
/// all-collider geometry at `position`.
///
/// A point ray can find a finite tread beside the capsule while the capsule
/// itself remains non-overlapping in unsupported air. Run a short sequence of
/// the same zero-input walk/gravity passes ordinary locomotion uses, accepting
/// only endpoints that remain grounded within the contact-sized rest band.
fn shape_has_stable_support(
    controller: &KinematicCharacterController,
    queries: &QueryPipeline,
    shape: &dyn Shape,
    position: Vector<Real>,
    dt: Real,
) -> bool {
    const SETTLE_FRAMES: usize = 96;

    let mut settled = position;
    let settle_limit = PLAYER_CONTACT_OFFSET / SCALE_FACTOR;
    for _ in 0..SETTLE_FRAMES {
        let pose = Isometry::translation(settled.x, settled.y, settled.z);
        let support = step_player_movement(
            controller,
            queries,
            shape,
            &pose,
            Vector::zeros(),
            Vector::zeros(),
            dt,
            -0.5 / SCALE_FACTOR,
            None,
            None,
            None,
        )
        .movement;
        if !support.grounded {
            return false;
        }
        settled += support.translation;
        let displacement = settled - position;
        let lateral = vector![displacement.x, 0.0, displacement.z].norm();
        if lateral > settle_limit || displacement.y.abs() > settle_limit {
            return false;
        }
    }
    true
}

fn slide_toward(
    controller: &KinematicCharacterController,
    queries: &QueryPipeline,
    shape: &dyn Shape,
    from: Vector<Real>,
    to: Vector<Real>,
    dt: Real,
) -> Option<EffectiveCharacterMovement> {
    let remaining = to - from;
    let distance = remaining.norm();
    if distance <= PLAYER_MOVE_ARRIVAL_EPSILON {
        return Some(scripted_character_movement(Vector::zeros()));
    }
    let requested = if distance > CLIMB_TOP_OUT_SUBSTEP {
        remaining / distance * CLIMB_TOP_OUT_SUBSTEP
    } else {
        remaining
    };
    let position = Isometry::translation(from.x, from.y, from.z);
    let movement = controller.move_shape(dt, queries, shape, &position, requested, |_c| ());
    let distance_after = (to - (from + movement.translation)).norm();
    (distance_after + 1.0e-5 < distance).then_some(movement)
}

fn shape_sweep_is_clear(
    queries: &QueryPipeline,
    from: Vector<Real>,
    to: Vector<Real>,
    shape: &dyn Shape,
) -> bool {
    let delta = to - from;
    let distance = delta.norm();
    if distance <= 1.0e-6 {
        return !shape_intersects(queries, to, shape);
    }
    queries
        .cast_shape(
            &Isometry::translation(from.x, from.y, from.z),
            &(delta / distance),
            shape,
            rapier3d::parry::query::ShapeCastOptions {
                max_time_of_impact: distance,
                target_distance: 0.0,
                stop_at_penetration: false,
                compute_impact_geometry_on_penetration: true,
            },
        )
        .is_none()
}

fn ray_segment_is_clear(queries: &QueryPipeline, from: Vector<Real>, to: Vector<Real>) -> bool {
    // The compressed sphere intentionally models Dark's sparse body and may
    // overlap immutable terrain around the ladder lip. Its centerline still
    // has to pass every authored point probe unobstructed: this prevents the
    // exception from becoming permission to cross an unrelated wall.
    let delta = to - from;
    let distance = delta.norm();
    distance <= 1.0e-6
        || queries
            .cast_ray(
                &Ray::new(Point::from(from), delta / distance),
                distance,
                true,
            )
            .is_none()
}

fn shape_route_exits_only_initial_obstruction(
    queries: &QueryPipeline,
    waypoints: &[Vector<Real>],
    shape: &dyn Shape,
    initial_obstruction_limit: Real,
) -> bool {
    // Dark's sparse body may meet the local lip before reaching the ladder
    // column's geometry-derived exit. Permit one continuous overlap in that
    // bounded interval, but require the route to exit it and reject any later
    // re-entry/new solid. Contact-offset sampling is finer than shipped wall
    // thicknesses and keeps the exception spatially bound even when level
    // terrain is one monolithic parentless collider.
    let Some(first) = waypoints.first().copied() else {
        return false;
    };
    let mut obstruction_seen = shape_intersects(queries, first, shape);
    let mut cleared_after_obstruction = false;
    let mut route_distance = 0.0;
    for segment in waypoints.windows(2) {
        let from = segment[0];
        let to = segment[1];
        let delta = to - from;
        let distance = delta.norm();
        let steps = (distance / CLIMB_TOP_OUT_RECOVERY_RETREAT).ceil().max(1.0) as usize;
        for step in 1..=steps {
            let sample = from + delta * (step as Real / steps as Real);
            let blocked = shape_intersects(queries, sample, shape);
            let sample_distance = route_distance + distance * step as Real / steps as Real;
            if blocked {
                if cleared_after_obstruction || sample_distance > initial_obstruction_limit {
                    return false;
                }
                obstruction_seen = true;
            } else if obstruction_seen {
                cleared_after_obstruction = true;
            }
        }
        route_distance += distance;
    }
    !obstruction_seen || cleared_after_obstruction
}

fn climb_top_out_recovery_start(
    queries: &QueryPipeline,
    head: Vector<Real>,
    direction: Vector<Real>,
    cross_y: Real,
    compressed: &Ball,
) -> Option<Vector<Real>> {
    let recovery_steps =
        (CLIMB_TOP_OUT_MAX_RECOVERY_RETREAT / CLIMB_TOP_OUT_RECOVERY_RETREAT) as usize;
    (0..=recovery_steps).find_map(|step| {
        let candidate = head - direction * (step as Real * CLIMB_TOP_OUT_RECOVERY_RETREAT);
        let raised_candidate = vector![candidate.x, cross_y, candidate.z];
        shape_sweep_is_clear(queries, candidate, raised_candidate, compressed).then_some(candidate)
    })
}

/// Plan the jump-through/mantle transition that finishes an upward climb.
///
/// The single-capsule equivalent compresses at the physical head-sphere center,
/// crosses by the greater of the geometry-derived ladder exit and Dark's fixed
/// one-head-diameter probe, then advances another body diameter. Dark's sparse
/// five-sphere body rises before compression; our continuous capsule cannot
/// straddle the same lip, so early compression is the least-deviating mapping.
/// Scripted substeps ignore climbables and parentless immutable level terrain,
/// matching Dark's jump-through transition. Parented entity colliders remain
/// live blockers throughout, and the final standing pose is validated against
/// every collider, including level terrain.
fn plan_climb_top_out(
    controller: &KinematicCharacterController,
    validation_queries: &QueryPipeline,
    probe_queries: &QueryPipeline,
    scripted_queries: &QueryPipeline,
    pos: &Isometry<Real>,
    direction: Vector<Real>,
    minimum_clear_forward: Real,
    dt: Real,
) -> Option<PlayerMovement> {
    let direction_h = vector![direction.x, 0.0, direction.z];
    let direction_norm = direction_h.norm();
    if direction_norm <= 1.0e-6 {
        return None;
    }
    let direction = direction_h / direction_norm;
    let head_offset = (PLAYER_STANDING_HEIGHT / 2.0 - PLAYER_STANDING_RADIUS) / SCALE_FACTOR;
    let head = pos.translation.vector + Vector::y() * head_offset;
    let raised = head + Vector::y() * CLIMB_TOP_OUT_UP;
    let probe_ahead = raised + direction * CLIMB_TOP_OUT_PROBE_FORWARD;
    let route_forward = minimum_clear_forward.max(CLIMB_TOP_OUT_PROBE_FORWARD);
    let route_ahead = raised + direction * route_forward;
    let standing = standing_player_capsule();
    let compressed = Ball::new(CLIMB_TOP_OUT_RADIUS);
    let up_ray = Ray::new(Point::from(head), Vector::y());
    // BreakClimb's jump-through remains valid when the mantle search meets a
    // lip after the compressed head has room to rise. Do not conflate that
    // authored fallback with a low ceiling: it must clear at least one full
    // head radius before the first upward obstruction. Rick1's merged lip is
    // hit after 0.559 wu; a ceiling close enough to pin the sphere is rejected.
    let up_obstruction = probe_queries
        .cast_ray(&up_ray, CLIMB_TOP_OUT_UP, true)
        .map(|(_, time_of_impact)| (time_of_impact, head.y + time_of_impact));
    if up_obstruction.is_some_and(|(time_of_impact, _)| time_of_impact < CLIMB_TOP_OUT_RADIUS) {
        return None;
    }
    if !ray_segment_is_clear(probe_queries, raised, probe_ahead) {
        return None;
    }
    let down_ray = Ray::new(Point::from(probe_ahead), -Vector::y());
    let mantle_floor = probe_queries
        .cast_ray_and_get_normal(&down_ray, CLIMB_TOP_OUT_MAX_DROP, true)
        .filter(|(_, landing)| landing.normal.y > CLIMB_TOP_OUT_MIN_GROUND_NORMAL)
        .map(|(_, landing)| probe_ahead - Vector::y() * landing.time_of_impact);
    if up_obstruction.is_some_and(|(_, obstruction_y)| {
        // A lip can have a thin underside above its adjacent landing. Treat
        // surfaces within one compressed radius plus the solver gap on both
        // faces as the same landing edge; a genuinely separate low ceiling
        // remains farther above the probed floor.
        mantle_floor.is_none_or(|floor| {
            floor.y + CLIMB_TOP_OUT_RADIUS + 2.0 * PLAYER_CONTACT_OFFSET / SCALE_FACTOR + 1.0e-4
                < obstruction_y
        })
    }) {
        return None;
    }

    // Dark's mantle target puts the physical head sphere just over the lip:
    // lip + 1.02 * radius + one authored unit. If CheckMantle cannot find that
    // lip, preserve BreakClimb's already-successful jump-through rather than
    // turning the failed mantle probe into a failed jump. A continuous sphere
    // cannot pass a thin rail the way Dark's sparse body can, so try only the
    // smallest lift clear of parented entity blockers up to the probe endpoint.
    let minimum_cross_y = mantle_floor
        .map(|floor| {
            head.y
                .max(floor.y + (1.02 * PLAYER_STANDING_RADIUS + 1.0) / SCALE_FACTOR)
        })
        .unwrap_or(head.y);
    // Dark's rise target is the authored 3.5-foot probe endpoint (or the
    // mantle floor-derived target when higher). Only that one route is
    // simulated; the bounded recovery search below already selects its nearest
    // viable retreat and avoids multiplying hundreds of controller casts per
    // candidate.
    let cross_y = minimum_cross_y.max(raised.y);
    // A full sphere can sit under an overhanging lip even when the head
    // point above it is clear. Dark's recovery searches backward/up within
    // four authored feet. Use the same bound and choose the first
    // contact-offset step whose full-radius vertical sweep is clear.
    let rise_start =
        climb_top_out_recovery_start(scripted_queries, head, direction, cross_y, &compressed)?;
    let cross_start = vector![rise_start.x, cross_y, rise_start.z];
    let probe_cross_end = vector![probe_ahead.x, cross_y, probe_ahead.z];
    let cross_end = vector![route_ahead.x, cross_y, route_ahead.z];
    let final_sphere = cross_end + direction * CLIMB_TOP_OUT_ADVANCE;
    // Parry treats balls and boxes wholly inside a closed trimesh as
    // intersecting its volume. A tiny non-degenerate capsule uses the same
    // triangle-surface query as the player while remaining point-scale.
    let route_probe = Capsule::new_y(PROBE_SKIN / SCALE_FACTOR, PROBE_SKIN / SCALE_FACTOR);
    if !shape_sweep_is_clear(probe_queries, head, rise_start, &compressed) {
        return None;
    }
    // The continuous capsule must compress before rising through the local
    // lip that Dark's sparse body can straddle. Validate the recovered
    // vertical centerline against all terrain: it may cross that same lip
    // surface, but never an offset ceiling the original upward probe did
    // not see, and it must leave the one overlap before crossing forward.
    let recovery_up_ray = Ray::new(Point::from(rise_start), Vector::y());
    let recovery_up_obstruction = probe_queries
        .cast_ray(&recovery_up_ray, cross_y - rise_start.y, true)
        .map(|(_, time_of_impact)| (time_of_impact, rise_start.y + time_of_impact));
    if recovery_up_obstruction.is_some_and(|(_, obstruction_y)| {
        up_obstruction.is_none_or(|(_, original_y)| {
            (obstruction_y - original_y).abs()
                > CLIMB_TOP_OUT_RECOVERY_RETREAT + PROBE_SKIN / SCALE_FACTOR
        })
    }) {
        return None;
    }
    let recovery_obstruction_limit = recovery_up_obstruction
        .map(|(time_of_impact, _)| time_of_impact + CLIMB_TOP_OUT_RECOVERY_RETREAT)
        .unwrap_or(0.0);
    if !shape_route_exits_only_initial_obstruction(
        probe_queries,
        &[rise_start, cross_start],
        &route_probe,
        recovery_obstruction_limit,
    ) {
        return None;
    }
    // Validate Dark's sparse-body corridor with a point-scale probe: it may
    // remain inside the one local lip component while crossing, but must
    // exit by the end and may never enter a second obstruction.
    // Recovery may retreat the rise behind `raised`. Validate that entire
    // offset approach before permitting the one bounded lip overlap below;
    // otherwise the scripted parentless-terrain exception could cross an
    // unrelated wall between the recovered rise and Dark's probe endpoint.
    if shape_intersects(probe_queries, cross_start, &route_probe)
        || !shape_sweep_is_clear(probe_queries, cross_start, probe_cross_end, &route_probe)
    {
        return None;
    }
    if mantle_floor.is_none() {
        let support_ray = Ray::new(Point::from(final_sphere), -Vector::y());
        let supported = validation_queries
            .cast_ray_and_get_normal(&support_ray, 2.0 * CLIMB_TOP_OUT_MAX_DROP, true)
            .is_some_and(|(_, ground)| ground.normal.y > CLIMB_TOP_OUT_MIN_GROUND_NORMAL);
        if !supported {
            return None;
        }
    }
    let final_down_ray = Ray::new(Point::from(final_sphere), -Vector::y());
    let final_standing = validation_queries
        .cast_ray_and_get_normal(&final_down_ray, CLIMB_TOP_OUT_FINAL_DROP, true)
        .filter(|(_, ground)| ground.normal.y > CLIMB_TOP_OUT_MIN_GROUND_NORMAL)
        .map(|(_, ground)| {
            let floor = final_sphere - Vector::y() * ground.time_of_impact;
            floor
                + Vector::y()
                    * (PLAYER_STANDING_HEIGHT / 2.0 / SCALE_FACTOR
                        + PLAYER_CONTACT_OFFSET / SCALE_FACTOR
                        + PLAYER_REST_LIFT / SCALE_FACTOR)
        })
        // Some ladder tops open over a drop. Expand at a pose validated
        // against every collider, then let ordinary gravity find the lower
        // deck.
        .unwrap_or(final_sphere - Vector::y() * head_offset);
    if shape_intersects(validation_queries, final_standing, &standing) {
        return None;
    }
    let initial_obstruction_limit =
        (cross_end - probe_cross_end).norm() + CLIMB_TOP_OUT_RECOVERY_RETREAT;
    if !shape_route_exits_only_initial_obstruction(
        probe_queries,
        &[probe_cross_end, cross_end, final_sphere, final_standing],
        &route_probe,
        initial_obstruction_limit,
    ) {
        return None;
    }

    // Preserve Dark's ordered rise-then-cross states. Scripted casts keep
    // parented entity blockers live but deliberately permit passage through
    // parentless immutable level terrain, the narrow Dark jump-through
    // exception. Final standing fit still uses `validation_queries`.
    let waypoints = [
        head,
        rise_start,
        cross_start,
        probe_cross_end,
        cross_end,
        final_sphere,
        final_standing,
    ];
    let mut simulated = pos.translation.vector;
    let mut first_movement = None;
    for waypoint in waypoints {
        for _ in 0..512 {
            if (waypoint - simulated).norm() <= PLAYER_MOVE_ARRIVAL_EPSILON {
                break;
            }
            let Some(movement) = slide_toward(
                controller,
                scripted_queries,
                &compressed,
                simulated,
                waypoint,
                dt,
            ) else {
                return None;
            };
            first_movement.get_or_insert(movement.translation);
            simulated += movement.translation;
        }
        if (waypoint - simulated).norm() > PLAYER_MOVE_ARRIVAL_EPSILON {
            return None;
        }
    }
    let first_step = first_movement?;

    Some(PlayerMovement {
        movement: scripted_character_movement(first_step),
        self_translation: first_step,
        is_climbing: true,
        top_out: Some(ClimbTopOut {
            waypoints,
            next_waypoint: 0,
            save_pose: pos.translation.vector,
            reversing: false,
            is_crouched: false,
        }),
        slope_displacement: Vector::zeros(),
        actor_collisions: Vec::new(),
    })
}

/// Plan Dark's ordinary jump-through/mantle for a grounded player pressing
/// into a non-climbable low obstacle.
///
/// Dark's sparse sphere-stack can jump through the local terrain lip after a
/// point probe finds clearance above it. Our continuous capsule cannot occupy
/// that intermediate pose, so this uses the same temporary head sphere and
/// parentless-terrain exception as ladder `BreakClimb`. The transition is
/// tightly bounded: the current frame must either be blocked by a low lip or
/// find a walkable landing beyond the stair limit, the first body-scale
/// standing pose must fit against *all* colliders, and every scripted substep
/// still collides with parented entities. Same-height and lower landings
/// require a genuinely clear all-world point probe above the lip, so
/// full-height walls remain solid. An elevated landing may cross the local
/// platform lip with Dark's parentless-terrain jump-through probe, but its
/// initial vertical rise must remain clear against all world geometry. That
/// is what permits authored stacked corridors such as shodan's log platforms
/// without treating a room ceiling directly overhead as an exterior landing.
fn plan_jump_mantle(
    controller: &KinematicCharacterController,
    validation_queries: &QueryPipeline,
    scripted_queries: &QueryPipeline,
    shape: &dyn Shape,
    pos: &Isometry<Real>,
    desired: Vector<Real>,
    dt: Real,
    is_crouched: bool,
) -> Option<PlayerMovement> {
    let desired_h = vector![desired.x, 0.0, desired.z];
    let desired_distance = desired_h.norm();
    if desired_distance <= 1.0e-6 {
        return None;
    }
    let direction = desired_h / desired_distance;
    let walk = controller.move_shape(dt, validation_queries, shape, pos, desired_h, |_c| ());
    let walk_blocked = walk.translation.dot(&direction) <= 0.25 * desired_distance;

    let final_shape = if is_crouched {
        crouched_player_capsule()
    } else {
        standing_player_capsule()
    };
    let (body_height, body_radius) = if is_crouched {
        (PLAYER_CROUCH_HEIGHT, PLAYER_CROUCH_RADIUS)
    } else {
        (PLAYER_STANDING_HEIGHT, PLAYER_STANDING_RADIUS)
    };
    let half_height = body_height / 2.0 / SCALE_FACTOR;
    let current_feet_y = pos.translation.vector.y - half_height;
    let head_offset = (body_height / 2.0 - body_radius) / SCALE_FACTOR;
    let head = pos.translation.vector + Vector::y() * head_offset;
    let max_rise =
        (PLAYER_JUMP_SPEED * PLAYER_JUMP_SPEED) / (2.0 * PLAYER_JUMP_GRAVITY * SCALE_FACTOR);
    let minimum_forward = (2.0 * body_radius + 2.0 * PLAYER_CONTACT_OFFSET) / SCALE_FACTOR;
    let mut transition = None;
    let mut lower_transition = None;
    let mut rise_ss2 = PLAYER_JUMP_MANTLE_PROBE_STEP;
    while rise_ss2 <= max_rise * SCALE_FACTOR + 1.0e-4 && transition.is_none() {
        let raised = head + Vector::y() * (rise_ss2 / SCALE_FACTOR);
        let vertical_probe_clear = ray_segment_is_clear(validation_queries, head, raised);
        let mut forward_ss2 = minimum_forward * SCALE_FACTOR;
        while forward_ss2 <= PLAYER_JUMP_MANTLE_FORWARD + 1.0e-4 {
            let forward = forward_ss2 / SCALE_FACTOR;
            let raised_forward = raised + direction * forward;
            // Dark's sparse player spheres can pass the lip/underside of
            // immutable level terrain during the jump-through transition.
            // An elevated landing may therefore use the same parented-only
            // probe as the scripted movement itself. Same-height crossings
            // below additionally require the all-world probe to be clear, so
            // a full-height wall with open space behind it is never a mantle.
            let scripted_probe_clear =
                ray_segment_is_clear(scripted_queries, raised, raised_forward);
            let validation_probe_clear =
                ray_segment_is_clear(validation_queries, raised, raised_forward);
            if scripted_probe_clear {
                // A platform above the current floor is a genuine mantle even
                // when its underside has no blocking vertical face.
                let down_ray = Ray::new(Point::from(raised_forward), -Vector::y());
                let nearest_floor = validation_queries
                    .cast_ray_and_get_normal(&down_ray, 2.0 * max_rise, true)
                    .map(|(_, ground)| (ground.normal.y, raised_forward.y - ground.time_of_impact));
                let elevated_floor = vertical_probe_clear
                    .then_some(nearest_floor)
                    .flatten()
                    .filter(|(normal_y, _)| *normal_y > CLIMB_TOP_OUT_MIN_GROUND_NORMAL)
                    .map(|(_, floor_y)| floor_y)
                    .filter(|floor_y| {
                        let rise = *floor_y - current_feet_y;
                        rise > (PLAYER_STEP_HEIGHT + PLAYER_CONTACT_OFFSET) / SCALE_FACTOR
                            // The compressed body models Dark's discontinuous
                            // sphere stack crossing a terrain lip; it does not
                            // add vertical reach. Require the player's feet to
                            // be able to reach the landing within the ordinary
                            // ballistic apex. Otherwise an open lift shaft can
                            // turn the room ceiling ahead into an "elevated
                            // floor" and script the player onto its exterior
                            // roof (#744).
                            && rise <= max_rise
                    });
                // A same-height floor visible above the lower candidate is the
                // stacked-terrain signature: the continuous capsule cannot
                // descend without first clearing that overhang. An ordinary
                // open ledge has no current-height floor at the forward probe
                // and remains a normal ballistic fall.
                let overhanging_current_floor = nearest_floor.is_some_and(|(normal_y, floor_y)| {
                    normal_y > PLAYER_MIN_WALKABLE_NORMAL
                        && (floor_y - current_feet_y).abs()
                            <= (PLAYER_STEP_HEIGHT + PLAYER_CONTACT_OFFSET) / SCALE_FACTOR
                });
                let lower_probe_start = vector![
                    raised_forward.x,
                    current_feet_y - (PLAYER_STEP_HEIGHT + PLAYER_CONTACT_OFFSET) / SCALE_FACTOR,
                    raised_forward.z
                ];
                let lower_ray = Ray::new(Point::from(lower_probe_start), -Vector::y());
                let lower_floor = validation_probe_clear
                    .then(|| {
                        validation_queries
                            .cast_ray_and_get_normal(
                                &lower_ray,
                                PLAYER_JUMP_MANTLE_MAX_DROP / SCALE_FACTOR,
                                true,
                            )
                            .filter(|(_, ground)| ground.normal.y > PLAYER_MIN_WALKABLE_NORMAL)
                            .map(|(_, ground)| {
                                (
                                    lower_probe_start - Vector::y() * ground.time_of_impact,
                                    ground.normal,
                                )
                            })
                            .filter(|(floor_point, _)| {
                                current_feet_y - floor_point.y
                                    > (PLAYER_STEP_HEIGHT + PLAYER_CONTACT_OFFSET) / SCALE_FACTOR
                            })
                    })
                    .flatten();
                let elevated_standing = elevated_floor.map(|floor_y| {
                    vector![
                        raised_forward.x,
                        floor_y
                            + half_height
                            + (PLAYER_CONTACT_OFFSET + PLAYER_REST_LIFT) / SCALE_FACTOR,
                        raised_forward.z
                    ]
                });
                let lower_standing = lower_floor
                    .map(|(floor_point, floor_normal)| {
                        // A vertical ray identifies the exact sloped tread.
                        // Offset the capsule along that surface normal rather
                        // than straight up: on SHODAN's 45-degree side ring, a
                        // vertical half-height offset embeds the bottom sphere
                        // into the upslope half of the triangle.
                        let segment_half = (body_height / 2.0 - body_radius) / SCALE_FACTOR;
                        floor_point
                            + floor_normal * ((body_radius + PLAYER_CONTACT_OFFSET) / SCALE_FACTOR)
                            + Vector::y() * (segment_half + PLAYER_REST_LIFT / SCALE_FACTOR)
                    })
                    // A normal open ledge needs no compatibility transition:
                    // the ordinary ballistic capsule can fall beside it. Use
                    // the sparse body only where the upper-floor ray proves
                    // authored stacked terrain overhangs the lower landing,
                    // and only while already crouched for that low route.
                    .filter(|_| is_crouched && overhanging_current_floor)
                    // Restore the continuous capsule an additional radius
                    // beyond the minimum crossing distance. The first point
                    // sample on a finite downhill tread can support the bottom
                    // sphere while leaving its center on the boundary; one
                    // body-radius of interior clearance selects a durable
                    // landing without widening the bounded forward search.
                    .filter(|_| forward >= minimum_forward + body_radius / SCALE_FACTOR)
                    .filter(|final_standing| {
                        // A non-overlapping capsule can still be suspended
                        // beside the finite tread that produced the point-ray
                        // hit. Prove the expanded body is actually supported:
                        // repeated ordinary zero-input gravity passes must keep
                        // it grounded without meaningful drift. Unsupported
                        // edge candidates are skipped so the bounded search
                        // can reach the real side-ring tread.
                        shape_has_stable_support(
                            controller,
                            validation_queries,
                            &final_shape,
                            *final_standing,
                            dt,
                        )
                    });
                // A blocked low lip first crosses to the same-height standing
                // pose beyond it. That destination may itself open over a
                // drop, as SHODAN's initial final-descent barrier does; do not
                // skip that normal crossing in favor of a deeper landing.
                let same_height_standing = (walk_blocked && validation_probe_clear)
                    .then(|| pos.translation.vector + direction * forward);
                if let Some(final_standing) = elevated_standing.or(same_height_standing) {
                    if !shape_intersects(validation_queries, final_standing, &final_shape) {
                        transition = Some((raised, raised_forward, final_standing));
                        break;
                    }
                }
                // A lower landing is a fallback for stacked geometry, never
                // competition for an elevated/same-height mantle farther
                // along the bounded search. Remember the first valid one and
                // use it only if the preferred search is exhausted.
                if lower_transition.is_none() {
                    if let Some(final_standing) = lower_standing {
                        if !shape_intersects(validation_queries, final_standing, &final_shape) {
                            lower_transition = Some((raised, raised_forward, final_standing));
                        }
                    }
                }
            }
            forward_ss2 += PLAYER_JUMP_MANTLE_PROBE_STEP;
        }
        rise_ss2 += PLAYER_JUMP_MANTLE_PROBE_STEP;
    }
    let (raised, raised_forward, final_standing) = transition.or(lower_transition)?;
    let final_head = final_standing + Vector::y() * head_offset;
    let waypoints = [
        head,
        raised,
        raised_forward,
        raised_forward,
        final_head,
        final_head,
        final_standing,
    ];

    // Preflight the exact fixed-timestep route against every parented entity
    // blocker. Parentless level terrain is the one Dark jump-through
    // exception; the destination itself was validated against all colliders.
    let compressed = Ball::new(body_radius / SCALE_FACTOR);
    let mut simulated = pos.translation.vector;
    let mut first_movement = None;
    for waypoint in waypoints {
        for _ in 0..512 {
            if (waypoint - simulated).norm() <= PLAYER_MOVE_ARRIVAL_EPSILON {
                break;
            }
            let movement = slide_toward(
                controller,
                scripted_queries,
                &compressed,
                simulated,
                waypoint,
                dt,
            )?;
            first_movement.get_or_insert(movement.translation);
            simulated += movement.translation;
        }
        if (waypoint - simulated).norm() > PLAYER_MOVE_ARRIVAL_EPSILON {
            return None;
        }
    }

    let first_movement = first_movement?;
    Some(PlayerMovement {
        movement: scripted_character_movement(first_movement),
        self_translation: first_movement,
        is_climbing: true,
        top_out: Some(ClimbTopOut {
            waypoints,
            next_waypoint: 0,
            save_pose: pos.translation.vector,
            reversing: false,
            is_crouched,
        }),
        slope_displacement: Vector::zeros(),
        actor_collisions: Vec::new(),
    })
}

/// Advance a compressed ladder or ordinary-jump top-out by one fixed-timestep
/// step, checking every substep against current parented entity geometry.
/// Parentless immutable level terrain is the narrow Dark jump-through
/// exception. A newly-blocked route returns to its last valid standing pose
/// before the capsule is expanded, and final standing fit is checked against
/// every collider.
fn advance_climb_top_out(
    controller: &KinematicCharacterController,
    validation_queries: &QueryPipeline,
    scripted_queries: &QueryPipeline,
    pos: &Isometry<Real>,
    mut top_out: ClimbTopOut,
    dt: Real,
) -> (EffectiveCharacterMovement, Option<ClimbTopOut>) {
    let final_shape = if top_out.is_crouched {
        crouched_player_capsule()
    } else {
        standing_player_capsule()
    };
    let target = loop {
        let target = if top_out.reversing {
            match top_out.next_waypoint.checked_sub(1) {
                Some(index) => top_out.waypoints[index],
                None => top_out.save_pose,
            }
        } else if let Some(target) = top_out.waypoints.get(top_out.next_waypoint) {
            *target
        } else if shape_intersects(validation_queries, pos.translation.vector, &final_shape) {
            top_out.reversing = true;
            continue;
        } else {
            return (scripted_character_movement(Vector::zeros()), None);
        };

        if (target - pos.translation.vector).norm() > PLAYER_MOVE_ARRIVAL_EPSILON {
            break target;
        }
        if top_out.reversing {
            if top_out.next_waypoint == 0 {
                if shape_intersects(validation_queries, top_out.save_pose, &final_shape) {
                    return (scripted_character_movement(Vector::zeros()), Some(top_out));
                }
                return (scripted_character_movement(Vector::zeros()), None);
            }
            top_out.next_waypoint -= 1;
        } else {
            top_out.next_waypoint += 1;
        }
    };
    let compressed_radius = if top_out.is_crouched {
        PLAYER_CROUCH_RADIUS / SCALE_FACTOR
    } else {
        CLIMB_TOP_OUT_RADIUS
    };
    let compressed = Ball::new(compressed_radius);
    // A live entity can move into the compressed sphere between frames. The
    // forward route must stop, but refusing every cast from an overlapping
    // pose would pin the recovery forever. During reversal only, let Rapier's
    // character controller compute a collision-checked depenetrating step
    // toward the preceding validated waypoint.
    let movement = (!shape_intersects(scripted_queries, pos.translation.vector, &compressed)
        || top_out.reversing)
        .then(|| {
            slide_toward(
                controller,
                scripted_queries,
                &compressed,
                pos.translation.vector,
                target,
                dt,
            )
        })
        .flatten();
    let Some(movement) = movement else {
        if !top_out.reversing {
            top_out.reversing = true;
        }
        return (scripted_character_movement(Vector::zeros()), Some(top_out));
    };
    (movement, Some(top_out))
}

/// Downward translation (world units) applied to the player capsule per
/// movement frame, honoring the body's gravity scale.
fn player_gravity_step(character_body: &RigidBody) -> Real {
    -0.5 / SCALE_FACTOR * character_body.gravity_scale()
}

/// One frame of player locomotion: the character controller's walk pass, the
/// gravity pass (with ground snapping and the resting lift), and the stair
/// step-up probe. Shared by real player movement ([`PhysicsWorld::move_player`])
/// and the validated debug move ([`PhysicsWorld::move_player_validated`]) so
/// both traverse exactly the same geometry.
///
/// Ordinary walk, gravity, climb, and stair translations use all-collider
/// shape casts. The scripted ladder top-out is the one narrow exception: its
/// compressed-body casts ignore climbables and parentless immutable level
/// terrain while retaining parented entity blockers, then validate the final
/// standing pose against every collider.
///
/// `climb` is the ladder redirect vector when the player grips a climbable
/// surface, paired with the query pipeline the climb cast runs against (see
/// [`ClimbPass`]): it replaces both the walk input and the gravity pass - unless
/// it achieves nothing, in which case the frame walks normally (see
/// `CLIMB_MIN_PROGRESS_FRACTION`).
///
/// `carry` is how far the moving-terrain body underfoot travelled this frame
/// (see [`PlayerSupport`]); it is applied first, so the player's own input and
/// gravity are resolved from where the platform has taken them. A player
/// gripping a ladder is holding the ladder, not riding the floor, so the climb
/// branch above skips it.
///
/// `airborne_vertical` is one frame of an ordinary jump arc. While present it
/// replaces the legacy constant gravity pass and disables ground snapping and
/// stair probes; all collision casts and wall/ceiling rejection remain live.
fn step_player_movement(
    controller: &KinematicCharacterController,
    queries: &QueryPipeline,
    shape: &dyn Shape,
    pos: &Isometry<Real>,
    desired: Vector<Real>,
    carry: Vector<Real>,
    dt: Real,
    gravity: Real,
    slope_displacement: Option<Vector<Real>>,
    airborne_vertical: Option<Real>,
    climb: Option<ClimbPass<'_>>,
) -> PlayerMovement {
    // Ladder: the climb vector replaces both the walk and the gravity pass.
    // Only if it actually moves the player, though - a climb consumes the
    // horizontal input, so a grip whose redirect is cast into solid geometry
    // (the floor the player is standing on when the redirect points DOWN, a
    // ceiling above them when it points up) would otherwise pin them in place
    // with nothing left to walk out with.
    //
    // Progress is measured as SIGNED VERTICAL travel, not the raw translation
    // length: the redirect is purely vertical, so any horizontal component in
    // the result is the character controller sliding off something in the way -
    // sideways travel is the blocked case, not a climb.
    if let Some(ClimbPass {
        movement: climb,
        top_out,
        validation_queries,
        probe_queries: climb_queries,
        scripted_queries,
    }) = climb
    {
        if climb.y > 0.0 {
            if let Some(top_out) = top_out.and_then(|(direction, minimum_clear_forward)| {
                plan_climb_top_out(
                    controller,
                    &validation_queries,
                    &climb_queries,
                    &scripted_queries,
                    pos,
                    direction,
                    minimum_clear_forward,
                    dt,
                )
            }) {
                return top_out;
            }
        }
        let mvt = controller.move_shape(dt, &climb_queries, shape, pos, climb, |_c| ());
        let climbed = mvt.translation.y * climb.y.signum();
        if climbed > CLIMB_MIN_PROGRESS_FRACTION * climb.y.abs() {
            return PlayerMovement {
                self_translation: mvt.translation,
                movement: mvt,
                is_climbing: true,
                top_out: None,
                slope_displacement: Vector::zeros(),
                actor_collisions: Vec::new(),
            };
        }
    }
    // Support motion first: the platform underfoot took the player with it
    // before they got a say. Cast like any other movement rather than
    // teleported, so a platform driving the player into geometry slides them
    // along it instead of pushing them through it. Everything below then
    // resolves the player's own movement from where the platform left them.
    let carried = if carry != Vector::zeros() {
        controller
            .move_shape(dt, queries, shape, pos, carry, |_c| ())
            .translation
    } else {
        Vector::zeros()
    };
    let pos = &(Translation::from(carried) * pos);
    // Walk and gravity run as separate passes - NOT the old up-bump hack
    // (there is no artificial upward movement): a combined walk+gravity cast
    // points into the floor the player rests on, which degenerates into
    // zero-progress resting contacts (see `PLAYER_REST_LIFT`).
    let mut walk_collisions = Vec::new();
    let mut mvt = controller.move_shape(dt, queries, shape, pos, desired, |collision| {
        walk_collisions.push(collision);
    });
    walk_collisions.retain(|collision| {
        queries
            .colliders
            .get(collision.handle)
            .is_some_and(|collider| {
                collider
                    .collision_groups()
                    .memberships
                    .intersects(InternalCollisionGroups::ACTOR.bits.into())
            })
    });
    let after_walk = Translation::from(mvt.translation) * pos;
    let gravity_step = Vector::y() * gravity;
    // A live jump owns vertical movement until it lands. Disable the
    // stair-sized ground snap for that pass so the initial ascent is not
    // immediately glued back to the floor; collision, sliding, ceiling
    // rejection, and slope classification remain the same controller path.
    // Slope carry resumes from rest after landing instead of adding horizontal
    // fall momentum to an independently integrated jump arc.
    let (fall, next_slope_displacement) = if let Some(vertical) = airborne_vertical {
        let mut airborne_controller = *controller;
        airborne_controller.snap_to_ground = None;
        (
            airborne_controller.move_shape(
                dt,
                queries,
                shape,
                &after_walk,
                Vector::y() * vertical,
                |_collision| (),
            ),
            Vector::zeros(),
        )
    } else if let Some(slope_displacement) = slope_displacement {
        // Dark's dynamic player preserves velocity when a fall is redirected
        // by terrain. Our kinematic controller has no velocity of its own, so
        // retain only the horizontal part of a slope slide and feed it into
        // the next gravity cast. This is load-bearing at chained slope seams:
        // a steep face can start a slide, and that momentum must carry onto a
        // shallower, otherwise walkable face instead of treating the seam as
        // a fresh rest.
        //
        // Keep the request at the existing fixed gravity-step magnitude. The
        // retained component changes direction, not fall speed, preserving
        // the established movement rate until gravity becomes fully
        // integrated.
        //
        // This state models momentum redirected by ordinary downward gravity.
        // A zero/negative-gravity room transition must not inject the old
        // downhill direction into its first upward frame.
        let carried_slope_displacement = if gravity < 0.0 {
            slope_displacement
        } else {
            Vector::zeros()
        };
        let mut fall_input = gravity_step + carried_slope_displacement;
        let gravity_distance = gravity.abs();
        if gravity_distance == 0.0 {
            fall_input = Vector::zeros();
        } else if carried_slope_displacement.norm_squared()
            > PLAYER_SLOPE_DISPLACEMENT_EPSILON_SQUARED
        {
            let fall_distance = fall_input.norm();
            if fall_distance > gravity_distance {
                fall_input *= gravity_distance / fall_distance;
            }
        }
        let mut touched_floor = false;
        let fall =
            controller.move_shape(dt, queries, shape, &after_walk, fall_input, |collision| {
                let up_dot = controller.up.dot(&collision.hit.normal1);
                touched_floor |= up_dot > PLAYER_SLOPE_MIN_FLOOR_NORMAL;
            });
        // A capsule cast that begins inside the controller's target distance
        // can report an edge-directed impact normal even on a planar trimesh.
        // Probe straight down from the resolved center to classify the actual
        // surface underfoot, otherwise an initial flat-floor contact can
        // manufacture sideways momentum.
        let resolved_pos = Translation::from(fall.translation) * after_walk;
        let ground_ray = Ray::new(
            Point::from(resolved_pos.translation.vector),
            -*controller.up.as_ref(),
        );
        let ground_probe_distance = slope_ground_probe_distance(shape);
        let touched_sloped_floor = touched_floor
            && fall.is_sliding_down_slope
            && queries
                .cast_ray_and_get_normal(&ground_ray, ground_probe_distance, true)
                .is_some_and(|(_, hit)| {
                    let up_dot = controller.up.dot(&hit.normal);
                    let horizontal_normal = hit.normal - *controller.up.as_ref() * up_dot;
                    up_dot > PLAYER_SLOPE_MIN_FLOOR_NORMAL
                        && horizontal_normal.norm_squared()
                            > PLAYER_SLOPE_MIN_HORIZONTAL_NORMAL_SQUARED
                });
        // Rapier's flag means the slope handler took its permissive branch, not
        // necessarily that the contact itself was a slope. In particular,
        // carried horizontal input against a flat floor sets it too. Preserve
        // the full tangent on an actual slope (and in the air), but damp it on
        // flat support so short authored seams are crossed without making the
        // player coast forever after landing.
        let horizontal_fall = vector![fall.translation.x, 0.0, fall.translation.z];
        let had_slope_displacement =
            carried_slope_displacement.norm_squared() > PLAYER_SLOPE_DISPLACEMENT_EPSILON_SQUARED;
        let mut next_slope_displacement = if fall.is_sliding_down_slope && touched_sloped_floor {
            horizontal_fall
        } else if had_slope_displacement && (touched_floor || fall.grounded) {
            let damping = 2.0_f32.powf(-dt / PLAYER_SLOPE_FLAT_DAMPING_HALF_LIFE);
            horizontal_fall * damping
        } else if had_slope_displacement {
            // A gap between faces is still part of the fall. With no floor
            // contact there is no friction to remove horizontal motion.
            horizontal_fall
        } else {
            Vector::zeros()
        };
        if next_slope_displacement.norm_squared() <= PLAYER_SLOPE_DISPLACEMENT_EPSILON_SQUARED {
            next_slope_displacement = Vector::zeros();
        }
        (fall, next_slope_displacement)
    } else {
        // Collision-valid debug walks deliberately have no motion history:
        // preserve their established exact gravity/step behavior.
        (
            controller.move_shape(
                dt,
                queries,
                shape,
                &after_walk,
                gravity_step,
                |_collision| (),
            ),
            Vector::zeros(),
        )
    };
    mvt.translation += fall.translation;
    mvt.grounded = fall.grounded;
    if mvt.grounded {
        mvt.translation += Vector::y() * (PLAYER_REST_LIFT / SCALE_FACTOR);
    }
    // Stairs: if grounded walking was blocked, probe for a step and hop onto
    // it. (Grounded-only: an airborne player pressed against a wall must not
    // ratchet up ledges.)
    if airborne_vertical.is_none() && mvt.grounded {
        if let Some(step) = try_step_up(
            queries,
            shape,
            &(Translation::from(mvt.translation) * pos),
            desired,
            mvt.translation,
        ) {
            mvt.translation += step;
        }
    }
    // The caller applies one translation from the ORIGINAL pose, so fold the
    // platform's contribution back in only now that every pass above (which
    // measures from the carried pose) is done.
    mvt.translation += carried;
    PlayerMovement {
        self_translation: mvt.translation - carried,
        movement: mvt,
        is_climbing: false,
        top_out: None,
        slope_displacement: next_slope_displacement,
        actor_collisions: walk_collisions,
    }
}

/// Maximum distance (world units) a single validated player move may advance.
/// A request for a farther target is clamped to this, so an automated tester
/// navigates in short, collision-checked hops instead of one long teleport.
pub const MAX_PLAYER_MOVE_DISTANCE: f32 = 5.0;

/// Distance (world units) a single substep of a validated move attempts: the
/// player's per-frame walk displacement (25 SS2 ft/s at the 60 Hz fixed step -
/// see the walk vector built in `mission_core`). A validated move is walked as
/// a sequence of these rather than cast in one go, because the stair probe hops
/// one ledge per call; matching the real per-frame distance also makes gravity
/// (applied once per substep, as it is once per frame) fall at the real rate.
const PLAYER_MOVE_SUBSTEP: f32 = 25.0 / SCALE_FACTOR / 60.0;

/// Fraction of a substep's attempted distance that still counts as progress.
/// Below it the player is genuinely stopped (a wall, a closed door) and the
/// move ends; above it, walking / slope handling / the step-up probe is still
/// carrying the player toward the target.
const PLAYER_MOVE_PROGRESS_FRACTION: f32 = 0.25;

/// Consecutive low-progress substeps tolerated before a validated move is
/// considered blocked. Real locomotion keeps stepping while the capsule
/// scrapes around a corner; a single sub-threshold frame is therefore not
/// evidence of a wall. Three frames cover the shipped corner contacts while
/// keeping a truly stopped move short.
const PLAYER_MOVE_STALL_SUBSTEPS: usize = 3;

/// How close to the requested distance a validated move must get to count as
/// having arrived (world units). Also the tolerance for reporting `blocked`,
/// and what keeps the loop from grinding on an ever-shrinking remainder (the
/// final attempt shrinks to whatever is left, so its minimum progress shrinks
/// with it).
const PLAYER_MOVE_ARRIVAL_EPSILON: f32 = 0.02;

bitflags! {
    pub struct InternalCollisionGroups: u32 {
        const WORLD = 1 << 0; // 1
        const ENTITY = 1 << 1; // 2
        const SELECTABLE = 1 << 2; // 2
        const PLAYER = 1 << 3;
        const UI = 1 << 4;
        const HITBOX = 1 << 5;
        const RAYCAST = 1 << 6;
        // Marker membership for climbable surfaces (PropPhysAttr.climbable != 0,
        // e.g. ladders). Nothing filters on it for collision - it exists so the
        // player movement code can query "am I touching a ladder?" cheaply.
        const CLIMBABLE = 1 << 7;
        // Living creature capsules need a distinct membership from ordinary
        // physical entities: interaction-only/model-bounds stand-ins can then
        // let characters pass while remaining solid to projectiles and props.
        const ACTOR = 1 << 8;
        // A melee weapon in the player's hand. Its own membership, because a
        // creature's hitboxes answer to it and to nothing else: they are
        // damage volumes, and generating contacts against every prop that
        // brushes a limb is both meaningless and expensive.
        const HELD_MELEE = 1 << 9;
        /// Every physical ECS object, including living creature actors. Use
        /// this for entity queries/filters; use `ENTITY` or `ACTOR` for an
        /// individual collider's membership.
        const ENTITIES = Self::ENTITY.bits | Self::ACTOR.bits;
        const CHARACTERS = Self::PLAYER.bits | Self::ACTOR.bits;
        const ALL_COLLIDABLE = Self::WORLD.bits | Self::ENTITIES.bits | Self::PLAYER.bits | Self::SELECTABLE.bits;
        const ALL = Self::ALL_COLLIDABLE.bits | Self::UI.bits | Self::HITBOX.bits | Self::RAYCAST.bits;
    }
}

pub struct DynamicPhysicsOptions {
    pub gravity_scale: f32,
    /// Coefficient of restitution (bounciness). Already calibrated from Dark's
    /// authored `elasticity` at the call site; see `entity_creator`. The default
    /// reproduces the value dynamic bodies used before per-object attributes
    /// were threaded through.
    pub restitution: f32,
    /// Coefficient of friction, from Dark's authored `friction`. The default
    /// reproduces Rapier's default friction (what dynamic bodies used before).
    pub friction: f32,
}

impl Default for DynamicPhysicsOptions {
    fn default() -> DynamicPhysicsOptions {
        DynamicPhysicsOptions {
            gravity_scale: 1.0,
            restitution: 0.7,
            friction: 0.5,
        }
    }
}

#[derive(Clone, Copy)]
pub struct CollisionGroup {
    collision: InteractionGroups,
    solver: InteractionGroups,
}

impl CollisionGroup {
    fn solid(collision: InteractionGroups) -> CollisionGroup {
        CollisionGroup {
            collision,
            // Ordinary contacts solve exactly when they collide. Keeping the
            // two filters together by default makes the one deliberate split
            // in `held_melee` explicit and prevents stale all-groups solver
            // state when a collider changes roles.
            solver: collision,
        }
    }

    /// A creature's per-joint damage proxy. Raycasts find it (that is how a
    /// shot picks a limb), and a held melee weapon *contacts* it - so a swing
    /// lands on the arm it visually struck rather than on the capsule around
    /// the creature.
    ///
    /// It solves against nothing: these are damage volumes, and a limb that
    /// shoved the weapon out of the swing (or the creature off its feet) would
    /// be a physics body, which the actor capsule already is.
    pub fn hitbox() -> CollisionGroup {
        // `solid` mirrors these into the solver groups, which resolves to
        // nothing anyway: no group filters on `HITBOX`, and `held_melee`'s
        // solver filter deliberately excludes it - so a limb never shoves the
        // weapon that struck it.
        Self::solid(InteractionGroups {
            memberships: (InternalCollisionGroups::HITBOX.bits
                | InternalCollisionGroups::RAYCAST.bits)
                .into(),
            filter: (InternalCollisionGroups::RAYCAST.bits
                | InternalCollisionGroups::HELD_MELEE.bits)
                .into(),
            test_mode: Default::default(),
        })
    }

    pub fn ui() -> CollisionGroup {
        Self::solid(InteractionGroups {
            memberships: InternalCollisionGroups::UI.bits.into(),
            filter: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            test_mode: Default::default(),
        })
    }

    pub fn entity() -> CollisionGroup {
        Self::solid(InteractionGroups {
            memberships: InternalCollisionGroups::ENTITY.bits.into(),
            filter: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            test_mode: Default::default(),
        })
    }

    /// A player-held VR melee weapon remains a physical contact shape so an
    /// actual controller swing can meet authored world/actor collision while
    /// ignoring the player's own capsule. Damage is still owned by the
    /// weapon's trigger-gated script; this group only filters physical contact.
    pub fn held_melee() -> CollisionGroup {
        let collision = InteractionGroups {
            memberships: (InternalCollisionGroups::ENTITY.bits
                | InternalCollisionGroups::HELD_MELEE.bits)
                .into(),
            filter: (InternalCollisionGroups::WORLD.bits
                | InternalCollisionGroups::ENTITIES.bits
                | InternalCollisionGroups::SELECTABLE.bits
                // A creature's own hitboxes, so a swing lands on the limb it
                // struck. The capsule contact is still generated and is what
                // a creature with no hitboxes is hit on.
                | InternalCollisionGroups::HITBOX.bits)
                .into(),
            test_mode: Default::default(),
        };
        let solver = InteractionGroups {
            memberships: InternalCollisionGroups::ENTITY.bits.into(),
            // Contact generation still includes ACTOR through `collision`,
            // so HeldMeleeWeapon receives CollisionStarted and owns the
            // authored damage. Only Rapier's physical impulse is suppressed:
            // a motor-driven weapon must not launch a living dynamic capsule.
            // World and loose-prop response remains solid.
            filter: (InternalCollisionGroups::WORLD.bits
                | InternalCollisionGroups::ENTITY.bits
                | InternalCollisionGroups::SELECTABLE.bits)
                .into(),
            test_mode: Default::default(),
        };
        CollisionGroup { collision, solver }
    }

    /// Collision behavior for a living creature capsule. It collides exactly
    /// like an ordinary physical entity, but its distinct membership lets
    /// interaction-only fixtures and unsimulated movable debris opt out of
    /// blocking characters without also becoming transparent to physical
    /// projectiles and movable props.
    pub fn actor() -> CollisionGroup {
        Self::solid(InteractionGroups {
            memberships: InternalCollisionGroups::ACTOR.bits.into(),
            filter: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            test_mode: Default::default(),
        })
    }

    /// Collision behavior for a retained non-ragdoll creature corpse. Keep the
    /// body supported by authored world geometry and in selectable raycasts,
    /// but do not leave its old creature capsule solid to players, living AIs,
    /// moving terrain, or loose props.
    pub fn corpse() -> CollisionGroup {
        Self::solid(InteractionGroups {
            memberships: InternalCollisionGroups::SELECTABLE.bits.into(),
            filter: InternalCollisionGroups::WORLD.bits.into(),
            test_mode: Default::default(),
        })
    }

    /// Same collision behavior as `entity()`, plus the `CLIMBABLE` marker
    /// membership so player movement can detect ladder contact (see
    /// `PropPhysAttr.climbable`).
    pub fn climbable_entity() -> CollisionGroup {
        Self::solid(InteractionGroups {
            memberships: (InternalCollisionGroups::ENTITY.bits
                | InternalCollisionGroups::CLIMBABLE.bits)
                .into(),
            filter: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            test_mode: Default::default(),
        })
    }

    pub fn selectable() -> CollisionGroup {
        Self::solid(InteractionGroups {
            memberships: InternalCollisionGroups::SELECTABLE.bits.into(),
            filter: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            test_mode: Default::default(),
        })
    }

    #[cfg(test)]
    pub(crate) fn world_for_test() -> CollisionGroup {
        Self::solid(InteractionGroups {
            memberships: InternalCollisionGroups::WORLD.bits.into(),
            filter: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            test_mode: Default::default(),
        })
    }

    /// Keep this collider visible to generic interaction/projectile rays while
    /// removing it from every physical contact pair. Used for render-model
    /// frob bounds that accompany a separate authored physics collider.
    fn interaction_only(self) -> CollisionGroup {
        Self::solid(InteractionGroups {
            memberships: self.collision.memberships,
            filter: InternalCollisionGroups::RAYCAST.bits.into(),
            test_mode: self.collision.test_mode,
        })
    }

    /// The same membership as this group, with living characters dropped from
    /// its filter so neither the player nor creature capsules collide with it.
    ///
    /// Dark only instantiates a physics model for an object that carries a
    /// `PhysType` (`phprop.cpp`'s PhysType listener is what creates the
    /// instance `PhysDims`). An object with no `PhysType` anywhere in its
    /// inheritance chain - a wall console, a card slot, a button, the
    /// Resurrection Station's own casing - is therefore never solid in
    /// retail; its physical presence is the brushwork behind it. This engine
    /// still needs a collider there so the object stays frobbable and
    /// raycastable, so keep the collider and only take characters out of it.
    pub fn non_solid_to_characters(self) -> CollisionGroup {
        let filter = self.collision.filter.bits() & !InternalCollisionGroups::CHARACTERS.bits;
        Self::solid(InteractionGroups {
            memberships: self.collision.memberships,
            filter: filter.into(),
            test_mode: self.collision.test_mode,
        })
    }

    /// Collision group for ragdoll limb bodies. Members are `SELECTABLE` (so
    /// they remain raycast/selectable) and collide with `WORLD` geometry *and*
    /// each other (`SELECTABLE`), so limbs don't pass through the torso/head.
    ///
    /// Limb-vs-limb collision between *directly jointed* bodies is disabled at the
    /// joint level (`contacts_enabled(false)`), not here - otherwise adjacent
    /// bodies that spawn slightly overlapping get violently ejected (the original
    /// ragdoll "explosion"). The filter still excludes the leftover creature
    /// capsule (`ACTOR`) and per-joint hitboxes (`HITBOX`).
    pub fn ragdoll() -> CollisionGroup {
        Self::solid(InteractionGroups {
            memberships: InternalCollisionGroups::SELECTABLE.bits.into(),
            filter: (InternalCollisionGroups::WORLD.bits
                | InternalCollisionGroups::SELECTABLE.bits)
                .into(),
            test_mode: Default::default(),
        })
    }

    /// Ragdoll limbs that collide with the world but NOT with each other (nor
    /// other selectables). Used for death-handoff rigs spawned in a crumpled,
    /// limb-overlapping pose: the many simultaneous deep limb-limb contacts
    /// there can drive the articulated (multibody) solve to non-finite
    /// positions in a single step. Floor contact is what matters for a lying
    /// corpse; limb self-collision is cosmetic in that pose.
    pub fn ragdoll_no_self() -> CollisionGroup {
        Self::corpse()
    }
}

#[derive(Clone, Debug)]
pub struct RayCastResult {
    pub hit_point: Point3<f32>,
    pub hit_normal: Vector3<f32>,
    pub maybe_entity_id: Option<EntityId>,
    pub maybe_rigid_body_handle: Option<RigidBodyHandle>,
    pub is_sensor: bool,
    // TODO:
    // entity_id
}

/// Result of a bounded, collision-validated player move (see
/// [`PhysicsWorld::move_player_validated`]).
#[derive(Clone, Debug)]
pub struct MoveResult {
    /// Whether the player's position changed (a blocked move can still settle
    /// the player under gravity).
    pub moved: bool,
    /// Whether the player failed to cover the requested distance - stopped by
    /// geometry (a wall or a closed door; stairs and ramps are walked over).
    pub blocked: bool,
    /// The player's new world position after the move.
    pub new_position: Vector3<f32>,
    /// How much closer the player got to the bounded horizontal destination
    /// (world units); the vertical result of gravity and stair steps is not
    /// counted.
    pub distance_moved: f32,
    /// The distance the move was allowed to attempt this call: `min(horizontal
    /// target distance, MAX_PLAYER_MOVE_DISTANCE)`.
    pub requested_distance: f32,
}

#[derive(Clone, Debug)]
pub enum CollisionEvent {
    BeginIntersect {
        sensor_id: EntityId,
        entity_id: EntityId,
    },
    EndIntersect {
        sensor_id: EntityId,
        entity_id: EntityId,
    },
    CollisionStarted {
        entity1_id: EntityId,
        entity2_id: EntityId,
        contact: Option<CollisionContact>,
    },
}

/// World-space geometry for a newly-started physical contact.
#[derive(Clone, Copy, Debug)]
pub struct CollisionContact {
    /// Midpoint between the two touching surfaces.
    pub point: Vector3<f32>,
    /// Unit normal pointing from `entity1_id` toward `entity2_id`.
    pub normal: Vector3<f32>,
}

/// Predicate selecting *only* sensor colliders - the volumes the player's
/// ENTER/EXIT tracking is built from. Shared by the per-frame poll in
/// [`PhysicsWorld::move_player`] and the swept poll in
/// [`PhysicsWorld::move_player_validated`] so both observe the same set.
fn is_sensor_collider(_handle: ColliderHandle, collider: &Collider) -> bool {
    collider.is_sensor()
}

/// Entity ids of every sensor volume overlapping `shape` at `pos`. Callers pass
/// a sensor-filtered pipeline so the broad phase does the narrowing; the
/// `is_sensor` check keeps the result correct for any pipeline.
fn sensors_overlapping(
    queries: &QueryPipeline,
    pos: &Isometry<Real>,
    shape: &dyn Shape,
) -> HashSet<EntityId> {
    queries
        .intersect_shape(*pos, shape)
        .filter(|(_handle, collider)| collider.is_sensor())
        .filter_map(|(_handle, collider)| EntityId::from_inner(collider.user_data as u64))
        .collect()
}

/// ENTER/EXIT events for a change in the set of sensors containing the player.
/// EXIT events come first, matching the order the per-frame diff has always
/// emitted them in.
fn sensor_transition_events(
    previous: &HashSet<EntityId>,
    current: &HashSet<EntityId>,
    player_id: EntityId,
) -> Vec<CollisionEvent> {
    let mut events = Vec::new();
    for expired in previous.difference(current) {
        events.push(CollisionEvent::EndIntersect {
            sensor_id: *expired,
            entity_id: player_id,
        });
    }
    for entered in current.difference(previous) {
        events.push(CollisionEvent::BeginIntersect {
            sensor_id: *entered,
            entity_id: player_id,
        });
    }
    events
}

#[derive(Debug)]
pub enum PhysicsShape {
    Capsule { height: f32, radius: f32 },
    Cuboid(Vector3<f32>),
    Sphere(f32),
}

pub struct PlayerHandle {
    // Player
    controller: KinematicCharacterController,
    character_handle: RigidBodyHandle,
    // Whether the collider is currently the crouched capsule. Mutated only by
    // `PhysicsWorld::set_player_crouch`, which keeps the shape and this flag
    // in sync.
    is_crouched: bool,
    // Live-validated waypoints for an in-progress ladder top-out. This is
    // transient locomotion state: direct relocation and crouching cancel it,
    // while save/load uses the last valid standing pose stored with it.
    top_out: Option<ClimbTopOut>,
    // Moving terrain the player was last seen standing on, and where it was
    // then. Purely derived per-frame state (re-probed every move), so nothing
    // needs to save or restore it.
    support: Option<PlayerSupport>,
    // Horizontal displacement produced by the previous frame's gravity/slope
    // collision. Carried only while gravity keeps sliding the player; flat
    // support, climbing and direct relocation clear it.
    slope_displacement: Vector<Real>,
    // Ground contact reported by the previous character-controller frame.
    // Unlike `support`, this includes immutable level terrain.
    is_grounded: bool,
    // A live ordinary jump's vertical velocity (world units / second).
    // `None` means the legacy constant-gravity walk/fall path is active.
    jump_velocity: Option<Real>,
    // Held-button edge state: one press launches at most one jump.
    jump_was_pressed: bool,
    // The player's OWN translation from the last movement frame - this
    // frame's total travel minus the moving-support carry - so a rider
    // standing still on an elevator reports zero. Purely derived per-frame
    // state, recomputed by every `move_player`, so nothing saves it.
    self_translation: Vector3<f32>,
    // Whether that frame was a ladder climb or a scripted mantle rather than
    // ordinary walking. Also purely derived per-frame state.
    is_climbing: bool,
}

/// The physical-hand seam for one held melee weapon. `target` is an invisible
/// controller-driven kinematic body carrying the pose the hand asked for; the
/// visible dynamic weapon is driven onto it each step by velocity. Keeping the
/// target out of `entity_id_to_body` makes the weapon remain the entity's one
/// authoritative rendered/contact body.
#[derive(Clone, Copy)]
struct HeldMeleeDrive {
    target: RigidBodyHandle,
    /// The loose/restored world body is seated at the first tracked pose once;
    /// later hand poses move only `target` so world contact stays physical.
    seated: bool,
}

#[derive(Clone, Copy, Debug)]
struct KinematicAttachment {
    parent: RigidBodyHandle,
    /// Dark's PhysAttach offset is world-space translation, not a transform
    /// composed with the parent's rotation.
    offset: Vector<Real>,
}

#[derive(Clone, Copy, Debug)]
struct LiveCreatureSweepRecovery {
    support_translation: Vector<Real>,
    had_moving_side_contact: bool,
    horizontal_sweep_direction: Vector<Real>,
    moving_body: RigidBodyHandle,
}

impl PlayerHandle {
    /// Whether the player collider is currently the crouched capsule. This is
    /// the *actual* state (stand-up can be refused for lack of headroom), not
    /// the requested input.
    pub fn is_crouched(&self) -> bool {
        self.is_crouched
    }

    /// Ground contact reported by the last movement frame. Unlike a support
    /// probe this includes immutable level terrain, so it is what "standing on
    /// the deck" means.
    pub fn is_grounded(&self) -> bool {
        self.is_grounded
    }

    /// How far the player moved themselves on the last movement frame, with
    /// any moving-platform carry removed (see [`PlayerSupport`]). This is the
    /// displacement footstep pacing accumulates: a player carried by an
    /// elevator, or relocated by a teleport (which never runs a movement
    /// frame), contributes nothing.
    pub fn self_translation(&self) -> Vector3<f32> {
        self.self_translation
    }

    /// Whether the last movement frame was a ladder climb or a scripted
    /// mantle. Such a player is holding a surface: they are neither striding
    /// along a floor nor falling, however far they travel vertically.
    pub fn is_climbing(&self) -> bool {
        self.is_climbing
    }
}

/// Clearance the held-melee sweep stops short of a surface by, so a weapon
/// resting against one does not re-report a zero-distance hit every frame.
const HELD_MELEE_SKIN: f32 = 0.01;

pub struct PhysicsWorld {
    gravity: Vector<Real>,
    integration_parameters: IntegrationParameters,
    physics_pipeline: PhysicsPipeline,
    island_manager: IslandManager,
    broad_phase: DefaultBroadPhase,
    /// Whether the pipeline has stepped at least once. Rapier builds the
    /// broad-phase BVH inside `PhysicsPipeline::step`, so until this is true
    /// every spatial query against this world matches nothing at all.
    has_stepped: bool,
    narrow_phase: NarrowPhase,
    impulse_joint_set: ImpulseJointSet,
    multibody_joint_set: MultibodyJointSet,
    ccd_solver: CCDSolver,
    collider_set: ColliderSet,
    rigid_body_set: RigidBodySet,

    /// Always empty. Substituted for the real body set in the query pipelines
    /// the *player's* movement casts against - see
    /// [`PhysicsWorld::player_movement_queries`].
    no_bodies: RigidBodySet,

    rigid_bodies_with_forces: Vec<RigidBodyHandle>,

    entity_id_to_body: HashMap<EntityId, RigidBodyHandle>,

    // Short-lived recovery state created only while a living, gravity-driven
    // creature is touching the side of horizontally-moving kinematic terrain.
    // Ordinary supported movement, falling, and knockback allocate no entry.
    live_creature_sweep_recovery: HashMap<EntityId, LiveCreatureSweepRecovery>,

    // Horizontal velocity transferred by this frame's kinematic-player shove.
    // Creature animation publishes authored root velocity after player physics
    // runs, so it consumes and adds this delta instead of overwriting it.
    pending_player_push_velocity: HashMap<EntityId, Vector3<f32>>,

    // Dark PhysAttach links rigidly drive a kinematic child's translation
    // from its parent's next translation plus an authored world-space offset.
    // Keyed by child because Dark permits at most one physical parent.
    kinematic_attachments: HashMap<RigidBodyHandle, KinematicAttachment>,

    // Dynamic held-melee bodies driven toward invisible controller targets by
    // Rapier joint motors. Keyed by the weapon body handle so the normal
    // set_position_rotation path can redirect hand poses to the target.
    held_melee_drives: HashMap<RigidBodyHandle, HeldMeleeDrive>,

    // TODO:
    // physics_hooks: Box<dyn PhysicsHooks>,
    // event_handler: Box<dyn EventHandler>,

    // Debug
    debug_pipeline: DebugRenderPipeline,

    // Sensor Intersection List
    player_sensor_intersections: HashSet<EntityId>,

    // Sensor ENTER/EXIT edges observed along a swept `move_player_validated`
    // hop, in traversal order. The per-frame poll in `move_player` only samples
    // the pose at the start of each frame, so a hop that crosses a volume
    // between two frames would otherwise leave no trace; these are queued here
    // and drained by the next frame's poll ahead of its own diff.
    //
    // A hop covers up to a walking second in one call, so a volume crossed
    // whole reports its ENTER and EXIT in the same drained batch (walking
    // spreads the pair over the frames it spends inside). That is the same
    // compression the hop already applies to the player's position. Queued
    // edges are only delivered by a stepped frame, so moving while the sim is
    // paused accumulates them until it resumes.
    pending_player_sensor_events: Vec<CollisionEvent>,

    // Entities already reported by report_nonfinite_rigid_body_state, so a
    // body fed bad state every frame (e.g. NaN animation joints driving a
    // kinematic hitbox) is reported once instead of every frame.
    reported_nonfinite_entities: HashSet<Option<EntityId>>,

    // Collision Events
    events: PhysicsEvents,
}

/// Clamp a collider's full size to finite, positive, bounded values. Some Dark
/// objects (notably certain trigger/`Ecology` objects) resolve to a non-finite
/// (e.g. infinite) collider dimension, producing a collider with a NaN/infinite
/// AABB. The old SAP broad-phase tolerated that; rapier's BVH broad-phase panics
/// on it (parry binned build, "index out of bounds"). Sanitize at the source and
/// log the offender so the bad data is traceable.
fn sanitize_collider_size(entity_id: EntityId, context: &str, size: Vector3<f32>) -> Vector3<f32> {
    const MIN_SIZE: f32 = 0.01;
    const MAX_SIZE: f32 = 1.0e4;
    let clamp = |v: f32| -> f32 {
        if v.is_finite() && v > 0.0 {
            // Clamp up as well as down: a tiny-but-positive size (e.g. medsci1's
            // "Lift 1 Walls" wall segment) yields a point-like AABB, and parry's
            // debug-build ray-AABB test overflows on those (FeatureId::Face(0 - 1))
            // - any AI vision/ground probe crossing it panics the game thread.
            v.clamp(MIN_SIZE, MAX_SIZE)
        } else {
            MIN_SIZE
        }
    };
    let out = Vector3::new(clamp(size.x), clamp(size.y), clamp(size.z));
    if out != size {
        tracing::warn!(
            "[physics] {} entity {:?}: invalid collider size {:?} -> clamped to {:?}",
            context,
            entity_id,
            size,
            out
        );
    }
    out
}

/// Scalar variant of [`sanitize_collider_size`] for ball colliders. A zero
/// radius (e.g. medsci1's "Lift 1 Walls": a dynamic SPHERE with radius 0)
/// yields a point AABB, and rapier 0.31's BVH broad-phase build panics on
/// zero/non-finite AABBs (parry `bvh_binned_build` "index out of bounds") -
/// the crash is nondeterministic because it depends on the bin layout around
/// the degenerate AABB.
fn sanitize_collider_radius(entity_id: EntityId, context: &str, radius: f32) -> f32 {
    const MIN_RADIUS: f32 = 0.005;
    const MAX_RADIUS: f32 = 5.0e3;
    let out = if radius.is_finite() && radius > 0.0 {
        radius.clamp(MIN_RADIUS, MAX_RADIUS)
    } else {
        MIN_RADIUS
    };
    if out != radius {
        tracing::warn!(
            "[physics] {} entity {:?}: invalid collider radius {} -> clamped to {}",
            context,
            entity_id,
            radius,
            out
        );
    }
    out
}

impl PhysicsWorld {
    pub fn add_level_geometry(&mut self, entity_id: EntityId, level: &SystemShock2Level) {
        /* Create the ground. */
        //let collider = ColliderBuilder::cuboid(100.0, 0.1, 100.0).build();

        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for geo in &level.all_geometry {
            let verts = &geo.verts;

            let mut idx = 0;
            let len = verts.len();

            while idx < len {
                let dest_idx = vertices.len() as u32;

                vertices.push(vec_to_npoint(verts[idx].position));
                vertices.push(vec_to_npoint(verts[idx + 1].position));
                vertices.push(vec_to_npoint(verts[idx + 2].position));

                indices.push([dest_idx, dest_idx + 1, dest_idx + 2]);

                idx += 3;
            }
        }

        let mut collider = ColliderBuilder::trimesh(vertices, indices)
            .expect("level geometry trimesh")
            .build();
        collider.user_data = entity_id.inner() as u128;
        collider.set_collision_groups(InteractionGroups {
            memberships: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            filter: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            test_mode: Default::default(),
        });
        self.collider_set.insert(collider);
    }

    pub fn add_collider(&mut self, entity_id: EntityId, mut collider: Collider) {
        collider.user_data = entity_id.inner() as u128;
        collider.set_collision_groups(InteractionGroups {
            memberships: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            filter: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            test_mode: Default::default(),
        });
        self.collider_set.insert(collider);
    }

    pub fn set_position_rotation2(
        &mut self,
        entity_id: EntityId,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) {
        if let Some(handle) = self.entity_id_to_body.get(&entity_id) {
            self.set_position_rotation(*handle, position, rotation);
        }
    }

    pub fn set_position_rotation(
        &mut self,
        handle: RigidBodyHandle,
        position: Vector3<f32>,
        rotation: Quaternion<f32>,
    ) {
        let nquat = nalgebra::geometry::Quaternion::new(
            rotation.s,
            rotation.v.x,
            rotation.v.y,
            rotation.v.z,
        );
        let nquat_unit = UnitQuaternion::from_quaternion(nquat);
        let mut xform = Isometry::identity();
        xform.append_rotation_mut(&nquat_unit);
        xform.translation = Translation {
            vector: vec_to_nvec(position),
        };

        if let Some(drive) = self.held_melee_drives.get(&handle).copied() {
            let mut just_seated = false;
            if !drive.seated {
                if let Some(weapon) = self.rigid_body_set.get_mut(handle) {
                    weapon.set_position(xform, true);
                    weapon.set_linvel(Vector::zeros(), true);
                    weapon.set_angvel(Vector::zeros(), true);
                }
                if let Some(drive) = self.held_melee_drives.get_mut(&handle) {
                    drive.seated = true;
                }
                just_seated = true;
            }
            if let Some(target) = self.rigid_body_set.get_mut(drive.target) {
                if just_seated {
                    target.set_position(xform, true);
                }
                target.set_next_kinematic_position(xform);
            }
        } else if let Some(rigid_body) = self.rigid_body_set.get_mut(handle) {
            if rigid_body.is_kinematic() {
                rigid_body.set_next_kinematic_position(xform);
            } else {
                rigid_body.set_position(xform, true);
                rigid_body.reset_torques(true);
                rigid_body.reset_forces(true);
            }
        }
    }

    pub fn set_translation(&mut self, handle: RigidBodyHandle, position: Vector3<f32>) {
        let maybe_rigid_body = self.rigid_body_set.get_mut(handle);

        if let Some(rigid_body) = maybe_rigid_body {
            rigid_body.set_next_kinematic_translation(vec_to_nvec(position));
        }
    }

    pub fn set_rotation(&mut self, handle: RigidBodyHandle, quat: Quaternion<f32>) {
        let maybe_rigid_body = self.rigid_body_set.get_mut(handle);

        if let Some(rigid_body) = maybe_rigid_body {
            if rigid_body.is_kinematic() {
                rigid_body.set_next_kinematic_rotation(quat_to_nquat(quat));
            } else {
                rigid_body.set_rotation(quat_to_nquat(quat), true);
            }
        }
    }

    pub fn set_rotation2(&mut self, entity_id: EntityId, quat: Quaternion<f32>) {
        if let Some(handle) = self.entity_id_to_body.get(&entity_id) {
            let maybe_rigid_body = self.rigid_body_set.get_mut(*handle);

            if let Some(rigid_body) = maybe_rigid_body {
                rigid_body.set_rotation(quat_to_nquat(quat), true);
            }
        }
    }

    pub fn set_gravity(&mut self, entity_id: EntityId, percent: f32) {
        if let Some(handle) = self.entity_id_to_body.get(&entity_id) {
            let maybe_rigid_body = self.rigid_body_set.get_mut(*handle);

            if let Some(rigid_body) = maybe_rigid_body {
                rigid_body.set_gravity_scale(percent, true);
            }
        }
    }

    pub fn clear_forces(&mut self) {
        for rigid_body_handle in &self.rigid_bodies_with_forces {
            let rigid_body = &mut self.rigid_body_set[*rigid_body_handle];
            rigid_body.reset_forces(true);
            rigid_body.reset_torques(true);
        }

        self.rigid_bodies_with_forces = Vec::new();
    }

    pub fn apply_force(&mut self, handle: RigidBodyHandle, force: Vector3<f32>) {
        let maybe_rigid_body = self.rigid_body_set.get_mut(handle);

        if let Some(rigid_body) = maybe_rigid_body {
            rigid_body.add_force(vec_to_nvec(force), true);
            self.rigid_bodies_with_forces.push(handle);
        }
    }

    pub fn apply_impulse(&mut self, handle: RigidBodyHandle, impulse: Vector3<f32>) {
        if let Some(rigid_body) = self.rigid_body_set.get_mut(handle) {
            rigid_body.apply_impulse(vec_to_nvec(impulse), true);
        }
    }

    /// Shove every dynamic body within `radius` of `center` directly away
    /// from it, adding `speed * (1 - d/radius)` to its velocity (explosion
    /// blasts). Mass-independent, like the game's other impulse-as-speed
    /// launches (flinderize).
    pub fn apply_radial_impulse(&mut self, center: Vector3<f32>, radius: f32, speed: f32) {
        for (_handle, body) in self.rigid_body_set.iter_mut() {
            if !body.is_dynamic() {
                continue;
            }
            let translation = body.translation();
            let offset = vec3(translation.x, translation.y, translation.z) - center;
            let distance = offset.magnitude();
            if distance >= radius {
                continue;
            }
            // A body at the exact center has no outward direction; toss it up.
            let direction = if distance > 1e-3 {
                offset / distance
            } else {
                vec3(0.0, 1.0, 0.0)
            };
            let delta = direction * speed * (1.0 - distance / radius);
            let new_velocity = body.linvel() + vec_to_nvec(delta);
            body.set_linvel(new_velocity, true);
        }
    }

    pub fn remove_rigid_body_handle(&mut self, handle: RigidBodyHandle) {
        self.rigid_body_set.remove(
            handle,
            &mut self.island_manager,
            &mut self.collider_set,
            &mut self.impulse_joint_set,
            &mut self.multibody_joint_set,
            true,
        );
    }

    /// Replace the collision groups on every collider owned by an entity's
    /// primary body. This preserves the body's pose, material, sleep state,
    /// and world contacts while changing which actors it can obstruct.
    pub fn set_collision_group(&mut self, entity_id: EntityId, group: CollisionGroup) {
        let Some(handle) = self.entity_id_to_body.get(&entity_id).copied() else {
            return;
        };
        let Some(body) = self.rigid_body_set.get(handle) else {
            return;
        };
        let collider_handles = body.colliders().to_vec();
        for collider_handle in collider_handles {
            if let Some(collider) = self.collider_set.get_mut(collider_handle) {
                collider.set_collision_groups(group.collision);
                collider.set_solver_groups(group.solver);
            }
        }
    }

    /// Turn an existing loose-prop body into a swept kinematic contact shape
    /// while a melee weapon is held in VR.
    ///
    /// The hand pose drives an invisible kinematic target; each step the
    /// visible weapon is *shape-cast* from where it is toward that target and
    /// stopped at the first world surface in the way (see
    /// [`Self::drive_held_melee`]). Kinematic, not dynamic: a dynamic weapon
    /// is subject to contact impulses, and a contact against a body whose
    /// origin sits out on the weapon head torques it - which in a headset
    /// read as the weapon spinning out of the player's hand the moment it
    /// touched anything. A swept kinematic body cannot be spun by the solver,
    /// cannot tunnel, and still reports every contact.
    pub fn set_held_melee(&mut self, entity_id: EntityId) {
        let Some(handle) = self.entity_id_to_body.get(&entity_id).copied() else {
            return;
        };
        let (pose, collider_handles) = {
            let Some(body) = self.rigid_body_set.get_mut(handle) else {
                return;
            };
            body.set_body_type(RigidBodyType::KinematicPositionBased, true);
            body.set_linvel(Vector::zeros(), true);
            body.set_angvel(Vector::zeros(), true);
            body.enable_ccd(true);
            (*body.position(), body.colliders().to_vec())
        };

        if !self.held_melee_drives.contains_key(&handle) {
            let target = self.rigid_body_set.insert(
                RigidBodyBuilder::kinematic_position_based()
                    .pose(pose)
                    .build(),
            );
            self.held_melee_drives.insert(
                handle,
                HeldMeleeDrive {
                    target,
                    seated: false,
                },
            );
        }

        for collider_handle in collider_handles {
            if let Some(collider) = self.collider_set.get_mut(collider_handle) {
                // A kinematic body generates no contacts against fixed world
                // or other kinematics under Rapier's defaults, and the whole
                // point of this body is its contacts.
                collider.set_active_collision_types(
                    ActiveCollisionTypes::default()
                        | ActiveCollisionTypes::KINEMATIC_KINEMATIC
                        | ActiveCollisionTypes::KINEMATIC_FIXED,
                );
            }
        }
        self.set_collision_group(entity_id, CollisionGroup::held_melee());
    }

    /// Replace a held melee body's inherited loose-pickup box with the local
    /// bounds of the weapon geometry rendered in the hand.
    ///
    /// The rigid-body target stays controller-driven at the authored weapon
    /// joint; the collider's parent-relative center covers the rest of the
    /// handle/blade without moving the rendered model or the motor target.
    pub fn fit_held_melee_cuboid(
        &mut self,
        entity_id: EntityId,
        size: Vector3<f32>,
        center: Vector3<f32>,
    ) {
        let size = sanitize_collider_size(entity_id, "fit_held_melee_cuboid", size);
        let Some(handle) = self.entity_id_to_body.get(&entity_id).copied() else {
            return;
        };
        let Some(body) = self.rigid_body_set.get(handle) else {
            return;
        };
        if !self.held_melee_drives.contains_key(&handle) {
            return;
        }

        let collider_handles = body.colliders().to_vec();
        let shape = SharedShape::cuboid(size.x / 2.0, size.y / 2.0, size.z / 2.0);
        let center = vec_to_nvec(center);
        for collider_handle in collider_handles {
            if let Some(collider) = self.collider_set.get_mut(collider_handle) {
                collider.set_shape(shape.clone());
                collider.set_translation_wrt_parent(center);
            }
        }
    }

    /// Resize every cuboid collider attached to a kinematic body. GUI panels
    /// use this when their authored pixel geometry changes while the proxy
    /// entity remains live.
    pub fn resize_kinematic_cuboid(
        &mut self,
        handle: RigidBodyHandle,
        entity_id: EntityId,
        size: Vector3<f32>,
    ) {
        let size = sanitize_collider_size(entity_id, "resize_kinematic_cuboid", size);
        let Some(body) = self.rigid_body_set.get(handle) else {
            return;
        };
        let collider_handles = body.colliders().to_vec();
        let shape = SharedShape::cuboid(size.x / 2.0, size.y / 2.0, size.z / 2.0);
        for collider_handle in collider_handles {
            if let Some(collider) = self.collider_set.get_mut(collider_handle) {
                collider.set_shape(shape.clone());
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn cuboid_full_size(&self, handle: RigidBodyHandle) -> Option<Vector3<f32>> {
        let body = self.rigid_body_set.get(handle)?;
        let collider = self.collider_set.get(*body.colliders().first()?)?;
        let cuboid = collider.shape().as_cuboid()?;
        Some(vec3(
            cuboid.half_extents.x * 2.0,
            cuboid.half_extents.y * 2.0,
            cuboid.half_extents.z * 2.0,
        ))
    }

    #[cfg(test)]
    pub(crate) fn capsule_dimensions(&self, handle: RigidBodyHandle) -> Option<(f32, f32)> {
        let body = self.rigid_body_set.get(handle)?;
        let collider = self.collider_set.get(*body.colliders().first()?)?;
        let capsule = collider.shape().as_capsule()?;
        Some((capsule.radius, capsule.half_height() * 2.0))
    }

    #[cfg(test)]
    pub(crate) fn sphere_radius(&self, handle: RigidBodyHandle) -> Option<f32> {
        let body = self.rigid_body_set.get(handle)?;
        let collider = self.collider_set.get(*body.colliders().first()?)?;
        collider.shape().as_ball().map(|ball| ball.radius)
    }

    #[cfg(test)]
    pub(crate) fn collider_local_translation(
        &self,
        handle: RigidBodyHandle,
    ) -> Option<Vector3<f32>> {
        let body = self.rigid_body_set.get(handle)?;
        let collider = self.collider_set.get(*body.colliders().first()?)?;
        let translation = collider.position_wrt_parent()?.translation.vector;
        Some(vec3(translation.x, translation.y, translation.z))
    }

    #[cfg(test)]
    pub(crate) fn collider_count(&self, handle: RigidBodyHandle) -> usize {
        self.rigid_body_set
            .get(handle)
            .map_or(0, |body| body.colliders().len())
    }

    #[cfg(test)]
    pub(crate) fn secondary_cuboid_full_size(
        &self,
        handle: RigidBodyHandle,
    ) -> Option<Vector3<f32>> {
        let body = self.rigid_body_set.get(handle)?;
        body.colliders().iter().skip(1).find_map(|handle| {
            let cuboid = self.collider_set.get(*handle)?.shape().as_cuboid()?;
            Some(vec3(
                cuboid.half_extents.x * 2.0,
                cuboid.half_extents.y * 2.0,
                cuboid.half_extents.z * 2.0,
            ))
        })
    }

    /// Whether any collider on a body is solid to the given character
    /// membership. Mirrors the player/actor movement queries: disabled bodies,
    /// disabled colliders, sensors, and non-collidable memberships never stop a
    /// character however their filters read.
    fn collider_blocks_character(
        &self,
        handle: RigidBodyHandle,
        character: InternalCollisionGroups,
    ) -> bool {
        let Some(body) = self.rigid_body_set.get(handle) else {
            return false;
        };
        if !body.is_enabled() {
            return false;
        }
        body.colliders().iter().any(|collider_handle| {
            self.collider_set.get(*collider_handle).is_some_and(|c| {
                let groups = c.collision_groups();
                c.is_enabled()
                    && !c.is_sensor()
                    && groups.filter.bits() & character.bits != 0
                    && groups.memberships.bits() & InternalCollisionGroups::ALL_COLLIDABLE.bits != 0
            })
        })
    }

    pub(crate) fn collider_blocks_player(&self, handle: RigidBodyHandle) -> bool {
        self.collider_blocks_character(handle, InternalCollisionGroups::PLAYER)
    }

    pub(crate) fn collider_blocks_actor(&self, handle: RigidBodyHandle) -> bool {
        self.collider_blocks_character(handle, InternalCollisionGroups::ACTOR)
    }

    pub fn remove_impulse_joint(&mut self, handle: ImpulseJointHandle) {
        self.impulse_joint_set.remove(handle, true);
    }

    pub fn apply_torque(&mut self, handle: RigidBodyHandle, force: Vector3<f32>) {
        let maybe_rigid_body = self.rigid_body_set.get_mut(handle);

        if let Some(rigid_body) = maybe_rigid_body {
            rigid_body.add_torque(vec_to_nvec(force), true);
            self.rigid_bodies_with_forces.push(handle);
        }
    }

    pub fn set_player_translation(
        &mut self,
        position: Vector3<f32>,
        player_handle: &mut PlayerHandle,
    ) {
        let previous =
            nvec_to_cgmath(*self.rigid_body_set[player_handle.character_handle].translation());
        self.translate_held_melee_for_player_relocation(position - previous);
        player_handle.top_out = None;
        player_handle.slope_displacement = Vector::zeros();
        player_handle.is_grounded = false;
        player_handle.jump_velocity = None;
        let collider_handle = self.rigid_body_set[player_handle.character_handle].colliders()[0];
        let shape = if player_handle.is_crouched {
            crouched_player_shared_shape()
        } else {
            standing_player_shared_shape()
        };
        self.collider_set[collider_handle].set_shape(shape);
        let character_body = self
            .rigid_body_set
            .get_mut(player_handle.character_handle)
            .unwrap();
        character_body.enable_ccd(true);
        character_body.set_translation(vec_to_nvec(position), true);
        // Whatever the player was riding, they are not standing on it here.
        // Re-probe now rather than just forgetting: a stale support would carry
        // them by a platform's next step from clear across the level, and an
        // absent one would drop the first frame of a ride they land in the
        // middle of (a teleport, or a save restored onto a moving deck).
        self.refresh_player_support(player_handle);
    }

    /// Carry the complete physical-hand pair through a discontinuous player
    /// relocation. The controller's next local pose cannot communicate that a
    /// debug/scripted teleport moved the whole tracked stage; without this the
    /// invisible target appears at the destination while the dynamic weapon
    /// tries to motor across the entire level through every wall in between.
    /// Ordinary hand motion remains motor-driven because it never calls the
    /// player relocation seam.
    fn translate_held_melee_for_player_relocation(&mut self, delta: Vector3<f32>) {
        if delta.magnitude2() <= f32::EPSILON {
            return;
        }
        let delta = vec_to_nvec(delta);
        let pairs = self
            .held_melee_drives
            .iter()
            .map(|(weapon, drive)| (*weapon, drive.target))
            .collect::<Vec<_>>();
        for (weapon, target) in pairs {
            if let Some(body) = self.rigid_body_set.get_mut(weapon) {
                let mut pose = *body.position();
                pose.translation.vector += delta;
                body.set_position(pose, true);
            }
            if let Some(body) = self.rigid_body_set.get_mut(target) {
                let mut pose = *body.position();
                pose.translation.vector += delta;
                body.set_position(pose, true);
                body.set_next_kinematic_position(pose);
            }
        }
    }

    /// Place the player at an authored pose, stepping aside only when their
    /// capsule does not fit there.
    ///
    /// A respawn marker is an authored coordinate, so it is used verbatim
    /// whenever it is free - this never second-guesses level data. It exists
    /// because a marker that *is* obstructed leaves the player permanently
    /// immobile: the character controller has no depenetration pass, so every
    /// cast from inside a collider returns a zero-length move in every
    /// direction and no input can recover (#801). Returns the position
    /// actually used.
    ///
    /// Only QBR reconstruction routes through here. Scripted teleports and the
    /// debug teleport still write the body directly: those are triggered, and
    /// a player who walks into a bad trap can at least reload - reconstruction
    /// is the relocation they cannot decline.
    pub fn set_player_translation_unobstructed(
        &mut self,
        position: Vector3<f32>,
        player_handle: &mut PlayerHandle,
    ) -> Vector3<f32> {
        let placed = match self.nearest_unobstructed_player_pose(position, player_handle) {
            Some(placed) => placed,
            None => {
                // Nothing within a player height of the marker fits. The
                // authored pose is still the best answer available - the
                // alternatives (leaving the player dead, or inventing a
                // coordinate) are worse - but it means the level data and the
                // engine disagree somewhere, so say so loudly.
                tracing::warn!(
                    "no unobstructed standing pose within {} wu of authored placement {:?}; using it as authored",
                    PLAYER_PLACEMENT_SEARCH_STEP * PLAYER_PLACEMENT_SEARCH_STEPS as f32,
                    position
                );
                position
            }
        };
        self.set_player_translation(placed, player_handle);
        placed
    }

    /// Whether the player's *current* capsule - crouched or standing, since
    /// nothing uncrouches them on the way to a respawn - fits at `position`
    /// without overlapping blocking geometry (their own body excluded).
    fn player_pose_is_clear(&self, position: Vector3<f32>, player_handle: &PlayerHandle) -> bool {
        let capsule = if player_handle.is_crouched {
            crouched_player_capsule()
        } else {
            standing_player_capsule()
        };
        self.pose_is_clear(position, player_handle, &capsule)
    }

    /// Whether the STANDING capsule fits at `position`, whatever the player is
    /// doing right now. Used where the pose has to be valid for an upright
    /// player later (the climb top-out's saved pose), and by the tests.
    fn standing_player_pose_is_clear(
        &self,
        position: Vector3<f32>,
        player_handle: &PlayerHandle,
    ) -> bool {
        self.pose_is_clear(position, player_handle, &standing_player_capsule())
    }

    fn pose_is_clear(
        &self,
        position: Vector3<f32>,
        player_handle: &PlayerHandle,
        shape: &dyn Shape,
    ) -> bool {
        let queries = self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.rigid_body_set,
            &self.collider_set,
            player_pose_filter(player_handle.character_handle),
        );
        !shape_intersects(&queries, vec_to_nvec(position), shape)
    }

    /// Whether a *substitute* pose is somewhere the player can actually be
    /// left: the capsule fits AND there is ground within one player height
    /// below it. The clearance test alone is happy in mid-air and over a
    /// void, which would trade one stranding for another.
    ///
    /// There is deliberately no "is the route there clear" test: the only
    /// geometry between an obstructed marker and any escape from it is the
    /// obstruction itself, so such a test would reject every candidate it was
    /// meant to vet. The bounded search radius is what keeps a substitute
    /// near the marker instead.
    fn placement_candidate_is_usable(
        &self,
        position: Vector3<f32>,
        player_handle: &PlayerHandle,
    ) -> bool {
        if !self.player_pose_is_clear(position, player_handle) {
            return false;
        }
        let queries = self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.rigid_body_set,
            &self.collider_set,
            player_pose_filter(player_handle.character_handle),
        );
        let down = Ray::new(Point::from(vec_to_nvec(position)), -Vector::y());
        let to_feet = if player_handle.is_crouched {
            PLAYER_CROUCH_HEIGHT / 2.0 / SCALE_FACTOR
        } else {
            PLAYER_STANDING_HEIGHT / 2.0 / SCALE_FACTOR
        };
        queries
            .cast_ray(&down, to_feet + PLAYER_PLACEMENT_MAX_DROP, true)
            .is_some()
    }

    /// The authored pose if the standing capsule fits there, otherwise the
    /// closest usable pose on a widening deterministic search around it, or
    /// `None` when nothing within the search fits.
    ///
    /// The authored pose only has to be *clear* - a marker over an authored
    /// drop is level data, not a mistake. A pose this code invents has to
    /// stand up to more than that; see `placement_candidate_is_usable`.
    fn nearest_unobstructed_player_pose(
        &self,
        position: Vector3<f32>,
        player_handle: &PlayerHandle,
    ) -> Option<Vector3<f32>> {
        if self.player_pose_is_clear(position, player_handle) {
            return Some(position);
        }
        // A single small lift covers the common near-miss - a marker sunk into
        // its own pad - and is deliberately not widened: a taller lift would
        // perch the player on the roof of whatever they were placed inside.
        // Everything past that slides out horizontally, nearest ring first.
        let lift = position + vec3(0.0, PLAYER_PLACEMENT_SEARCH_STEP, 0.0);
        if self.placement_candidate_is_usable(lift, player_handle) {
            return Some(lift);
        }
        for step in 1..=PLAYER_PLACEMENT_SEARCH_STEPS {
            let distance = PLAYER_PLACEMENT_SEARCH_STEP * step as f32;
            for compass in 0..PLAYER_PLACEMENT_SEARCH_DIRECTIONS {
                let angle = std::f32::consts::TAU * compass as f32
                    / PLAYER_PLACEMENT_SEARCH_DIRECTIONS as f32;
                let slide = position + vec3(angle.cos() * distance, 0.0, angle.sin() * distance);
                if self.placement_candidate_is_usable(slide, player_handle) {
                    return Some(slide);
                }
            }
        }
        None
    }

    /// The character body's current translation, without stepping the
    /// simulation. Used while time is frozen (debug-runtime pause) so
    /// teleports - which write the physics body directly - are still
    /// reflected in `PlayerInfo`/introspection before the next real step.
    pub fn get_player_translation(&self, player_handle: &PlayerHandle) -> Vector3<f32> {
        let character_body = self
            .rigid_body_set
            .get(player_handle.character_handle)
            .unwrap();
        nvec_to_cgmath(*character_body.translation())
    }

    fn top_out_save_pose_is_clear(
        &self,
        player_handle: &PlayerHandle,
        top_out: ClimbTopOut,
    ) -> bool {
        self.standing_player_pose_is_clear(nvec_to_cgmath(top_out.save_pose), player_handle)
    }

    /// Whether the live pose has nearby walkable ground to settle onto. The
    /// last movement frame's `is_grounded` flag is the fast path, but a fresh
    /// mission spawn can still be a fraction above its authored floor while
    /// the controller settles. Accept ground within one standing body height;
    /// that short fall is faithfully restorable, unlike a pose over a void.
    fn player_save_pose_has_support(&self, player_handle: &PlayerHandle) -> bool {
        if player_handle.is_grounded {
            return true;
        }
        let position = vec_to_nvec(self.get_player_translation(player_handle));
        let half_height = if player_handle.is_crouched {
            PLAYER_CROUCH_HEIGHT / 2.0 / SCALE_FACTOR
        } else {
            PLAYER_STANDING_HEIGHT / 2.0 / SCALE_FACTOR
        };
        let queries = self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.rigid_body_set,
            &self.collider_set,
            player_pose_filter(player_handle.character_handle),
        );
        let down = Ray::new(Point::from(position), -Vector::y());
        queries
            .cast_ray_and_get_normal(&down, half_height + PLAYER_PLACEMENT_MAX_DROP, true)
            .is_some_and(|(_, hit)| hit.normal.y >= SUPPORT_MIN_GROUND_NORMAL)
    }

    /// Position safe to serialize for the player. A compressed mantle can
    /// occupy places where the standing capsule does not fit, so persist its
    /// last valid standing start instead of an in-flight waypoint. A live
    /// blocker can move into that cached pose after planning; in that case
    /// there is temporarily no safe standing transform to save. Likewise,
    /// defer while a slope carries transient motion that the position-only
    /// save format cannot represent.
    pub fn get_player_save_translation(
        &self,
        player_handle: &PlayerHandle,
    ) -> Result<Vector3<f32>, PlayerSavePoseError> {
        // This short-lived displacement is the kinematic equivalent of
        // in-flight velocity. Loading only a transform in the middle of a
        // chained slope would erase it and can strand the player on an
        // otherwise walkable face, so defer saving until the carry settles.
        if player_handle.slope_displacement.norm_squared()
            > PLAYER_SLOPE_DISPLACEMENT_EPSILON_SQUARED
        {
            return Err(PlayerSavePoseError::TransientSlopeMotion);
        }
        match player_handle.top_out {
            Some(top_out) if self.top_out_save_pose_is_clear(player_handle, top_out) => {
                Ok(nvec_to_cgmath(top_out.save_pose))
            }
            Some(_) => Err(PlayerSavePoseError::BlockedRecoveryPose),
            None if !self.player_save_pose_has_support(player_handle) => {
                Err(PlayerSavePoseError::UnsupportedPose)
            }
            None => Ok(self.get_player_translation(player_handle)),
        }
    }

    /// Move the player toward `target`, but bounded and collision-validated.
    ///
    /// This *walks*: the horizontal displacement toward `target` is clamped to
    /// [`MAX_PLAYER_MOVE_DISTANCE`] and covered in [`PLAYER_MOVE_SUBSTEP`]-sized
    /// increments through [`step_player_movement`] - the same character
    /// controller, gravity/ground-snapping and stair step-up path real player
    /// movement uses, fed the same horizontal walk vector. So it traverses
    /// whatever the player can actually walk over (stairs, ramps, small ledges)
    /// instead of reporting the first riser as a wall (issue #559), and the
    /// vertical result is whatever walking produces - the requested `y` only
    /// picks a direction to walk in, it is never moved along.
    ///
    /// Safety is unchanged: every applied translation comes from the
    /// controller's shape-cast solver or the step probe's collision-checked
    /// casts, so - unlike `set_player_translation` - this can never move the
    /// player through a wall or out of bounds. Collision sliding may alter the
    /// route, but no result outside the requested horizontal radius is applied.
    ///
    /// `blocked` means the player did not cover the requested distance: either
    /// [`PLAYER_MOVE_STALL_SUBSTEPS`] consecutive substeps made less than
    /// [`PLAYER_MOVE_PROGRESS_FRACTION`] of their attempts (a wall, a closed
    /// door), the next solver result would exceed the bounded radius, or the
    /// move ran out of substeps still short of the target. A purely vertical
    /// request has nothing to walk toward and is a no-op.
    ///
    /// Ladders are not climbed (the move never grips a climbable surface), so
    /// issuing one on a ladder lets the player fall - exactly as walking
    /// without pushing into the ladder does.
    pub fn move_player_validated(
        &mut self,
        target: Vector3<f32>,
        player_handle: &mut PlayerHandle,
    ) -> MoveResult {
        // The endpoint promises standing-player collision safety. A top-out is
        // temporarily a compressed ball, so validating from its live pose and
        // then cancelling the state would expand a capsule at a ball-only
        // location. Rewind to the route's last valid standing pose first; the
        // ordinary validated walk below then snapshots and casts the capsule.
        let rewound_top_out = player_handle.top_out.is_some();
        if let Some(top_out) = player_handle.top_out {
            if !self.top_out_save_pose_is_clear(player_handle, top_out) {
                let current = self.get_player_translation(player_handle);
                let delta = target - current;
                let requested_distance = vec3(delta.x, 0.0, delta.z).magnitude();
                return MoveResult {
                    moved: false,
                    blocked: true,
                    new_position: current,
                    distance_moved: 0.0,
                    requested_distance: if requested_distance.is_finite() {
                        requested_distance.min(MAX_PLAYER_MOVE_DISTANCE)
                    } else {
                        0.0
                    },
                };
            }
            self.set_player_translation(nvec_to_cgmath(top_out.save_pose), player_handle);
        }
        let current = self.get_player_translation(player_handle);
        let delta = target - current;
        // Walking is horizontal: the vertical component of the request is
        // dropped, exactly like the walk vector the game feeds `move_player`.
        // Gravity, ground snapping and the stair probe decide `y`.
        let delta_h = vec3(delta.x, 0.0, delta.z);
        let dist = delta_h.magnitude();

        // Degenerate request (zero, purely vertical, or non-finite): nothing to
        // walk toward. Guard before computing `requested_distance` so a NaN
        // target doesn't report a bogus clamp value (`NaN.min(5.0) == 5.0`).
        if !dist.is_finite() || dist == 0.0 {
            return MoveResult {
                moved: rewound_top_out,
                blocked: false,
                new_position: current,
                distance_moved: 0.0,
                requested_distance: 0.0,
            };
        }

        // Distance we are allowed to attempt this call.
        let requested_distance = dist.min(MAX_PLAYER_MOVE_DISTANCE);

        // Snapshot the character shape + pose (cheap Arc clone) before building
        // the query pipeline, which borrows the body/collider sets. Mirrors the
        // pattern in `move_player`.
        //
        // Cast from the *body* pose, not the collider's cached pose: a kinematic
        // body's collider position is only re-synced during a physics step, so
        // after a prior `set_player_translation` (below) with no step in between
        // - e.g. two `move_player_validated` calls back to back - the collider
        // pose is stale and would restart the cast from the old spot, letting
        // the player tunnel through walls. The body's own `position()` updates
        // immediately. The collider is parented at the body origin (identity
        // local transform), so the body pose is the collider's true world pose.
        let character_body = &self.rigid_body_set[player_handle.character_handle];
        let character_collider = &self.collider_set[character_body.colliders()[0]];
        let character_shape = character_collider.shared_shape().clone();
        let mut pos = *character_body.position();
        let gravity = player_gravity_step(character_body);
        let player_id = EntityId::from_inner(character_body.user_data as u64);

        let filter = player_movement_filter(player_handle.character_handle);

        let dt = self.integration_parameters.dt;
        // The bounded destination (which may be short of a farther target).
        // Keep feeding the controller the same heading real thumbstick
        // locomotion receives, but measure progress against this destination:
        // collision sliding can alter the route, so summing only the original
        // heading's projection overshoots the target and oscillates around it.
        let start = pos.translation.vector;
        let walk_dir = vector![delta_h.x / dist, 0.0, delta_h.z / dist];
        let destination = start + walk_dir * requested_distance;
        let mut stalled_substeps = 0;
        let mut grounded = player_handle.is_grounded;
        // Sensor volumes the player occupies, carried across the sweep. The hop
        // is committed as one jump, so without polling per substep any volume
        // entered and left between the endpoints would never be observed (#654).
        // Substeps are the same size real walking covers in one frame, so this
        // samples the swept path at exactly the fidelity thumbstick walking does.
        let mut occupied_sensors = self.player_sensor_intersections.clone();
        let mut swept_sensor_events = Vec::new();
        {
            let queries =
                self.player_movement_queries(self.narrow_phase.query_dispatcher(), filter);
            // Sensors are excluded from `filter` (they must never block the
            // move); observing them is a separate query over the same pipeline.
            let sensor_queries =
                queries.with_filter(QueryFilter::new().predicate(&is_sensor_collider));
            let observe_sensors =
                |pos: &Isometry<Real>,
                 occupied: &mut HashSet<EntityId>,
                 events: &mut Vec<CollisionEvent>| {
                    if let Some(player_id) = player_id {
                        let current =
                            sensors_overlapping(&sensor_queries, pos, character_shape.as_ref());
                        events.extend(sensor_transition_events(occupied, &current, player_id));
                        *occupied = current;
                    }
                };

            // Sample the starting pose first: the cached occupancy was taken at
            // the last stepped frame, and a direct relocation since then (a
            // teleport with no frame in between) can have moved the player into
            // or out of a volume the cache has not seen yet.
            observe_sensors(&pos, &mut occupied_sensors, &mut swept_sensor_events);

            // Walk toward the target one substep at a time. The iteration bound
            // is the worst case a *progressing* move can need (every substep
            // scraping by at the progress fraction, plus a few for the
            // shrinking final attempt); running it out leaves `distance_moved`
            // short of the request, which is reported as `blocked` below rather
            // than passing for success.
            let max_substeps = (requested_distance
                / (PLAYER_MOVE_SUBSTEP * PLAYER_MOVE_PROGRESS_FRACTION))
                .ceil() as usize
                + 8;
            for _ in 0..max_substeps {
                let to_destination = vector![
                    destination.x - pos.translation.vector.x,
                    0.0,
                    destination.z - pos.translation.vector.z
                ];
                let remaining = to_destination.norm();
                if remaining <= PLAYER_MOVE_ARRIVAL_EPSILON {
                    break;
                }
                let attempt = remaining.min(PLAYER_MOVE_SUBSTEP);
                let mvt = step_player_movement(
                    &player_handle.controller,
                    &queries,
                    character_shape.as_ref(),
                    &pos,
                    walk_dir * attempt,
                    // A validated hop happens between frames, so no platform
                    // has moved since the last one; the support is refreshed
                    // at the end of the hop instead.
                    Vector::zeros(),
                    dt,
                    gravity,
                    None,
                    None,
                    // No ladder redirect: a validated move walks, it doesn't climb.
                    None,
                )
                .movement;
                grounded = mvt.grounded;

                let candidate = Translation::from(mvt.translation) * pos;
                let from_start = vector![
                    candidate.translation.vector.x - start.x,
                    0.0,
                    candidate.translation.vector.z - start.z
                ];
                // A collision slide may alter the route, but this API promises
                // a bounded hop. Never commit a solver result outside the
                // requested horizontal radius.
                if from_start.norm() > requested_distance + PLAYER_MOVE_ARRIVAL_EPSILON {
                    break;
                }
                let after = vector![
                    destination.x - candidate.translation.vector.x,
                    0.0,
                    destination.z - candidate.translation.vector.z
                ]
                .norm();
                let progress = remaining - after;
                pos = candidate;
                observe_sensors(&pos, &mut occupied_sensors, &mut swept_sensor_events);

                if progress < attempt * PLAYER_MOVE_PROGRESS_FRACTION {
                    stalled_substeps += 1;
                    if stalled_substeps >= PLAYER_MOVE_STALL_SUBSTEPS {
                        break;
                    }
                } else {
                    stalled_substeps = 0;
                }
            }
        }

        // Hand the traversed edges to the next frame's poll, and leave it the
        // occupancy at the final pose so it does not re-fire what was already
        // reported here (or miss an EXIT for something left mid-hop). Both are
        // unchanged when there is no player entity to attribute edges to.
        self.player_sensor_intersections = occupied_sensors;
        self.pending_player_sensor_events
            .append(&mut swept_sensor_events);

        let new_position = nvec_to_cgmath(pos.translation.vector);

        // `moved` tells callers to mirror the body's final position into
        // `PlayerInfo`. A top-out rewind counts even if the subsequent walk is
        // degenerate or blocked; otherwise the physics body and ECS pose would
        // disagree. A blocked walk can also settle the player under gravity
        // without advancing toward the target.
        let walked = new_position != current;
        let moved = rewound_top_out || walked;
        // `set_player_translation` re-probes the support, so a hop that walked
        // the player onto (or off) moving terrain is accounted for.
        if walked {
            self.set_player_translation(new_position, player_handle);
        }
        player_handle.is_grounded = grounded;

        let remaining = vector![
            destination.x - pos.translation.vector.x,
            0.0,
            destination.z - pos.translation.vector.z
        ]
        .norm();
        let distance_moved = (requested_distance - remaining).clamp(0.0, requested_distance);

        MoveResult {
            moved,
            blocked: remaining > PLAYER_MOVE_ARRIVAL_EPSILON,
            new_position,
            distance_moved,
            requested_distance,
        }
    }

    pub fn get_aabb2(&self, entity_id: EntityId) -> Option<Aabb3<f32>> {
        if let Some(handle) = self.entity_id_to_body.get(&entity_id) {
            let maybe_rigid_body = self.rigid_body_set.get(*handle);
            maybe_rigid_body.and_then(|rigid_body| {
                let collider_handle = rigid_body.colliders().first()?;
                let character_collider = self.collider_set.get(*collider_handle)?;
                let aabb = character_collider.compute_aabb();
                Some(Aabb3 {
                    min: point3(aabb.mins.x, aabb.mins.y, aabb.mins.z),
                    max: point3(aabb.maxs.x, aabb.maxs.y, aabb.maxs.z),
                })
            })
        } else {
            None
        }
    }

    pub fn get_position(&self, handle: RigidBodyHandle) -> Option<Vector3<f32>> {
        let maybe_rigid_body = self.rigid_body_set.get(handle);

        maybe_rigid_body.map(|rigid_body| nvec_to_cgmath(*rigid_body.translation()))
    }

    pub fn get_velocity(&self, entity_id: EntityId) -> Option<Vector3<f32>> {
        if let Some(handle) = self.entity_id_to_body.get(&entity_id) {
            let maybe_rigid_body = self.rigid_body_set.get(*handle);

            maybe_rigid_body.map(|rigid_body| {
                nvec_to_cgmath(rigid_body.velocity_at_point(rigid_body.center_of_mass()))
            })
        } else {
            None
        }
    }

    /// Velocity of `entity_id`'s body at a world-space point on it.
    ///
    /// Differs from [`Self::get_velocity`] by including the `omega x r` term:
    /// the head of a weapon swung about the wrist moves fast while its centre
    /// of mass barely moves, so a guard reading centre-of-mass velocity
    /// under-reads exactly the gesture it is meant to measure.
    pub fn velocity_at_point(
        &self,
        entity_id: EntityId,
        point: Vector3<f32>,
    ) -> Option<Vector3<f32>> {
        let handle = self.entity_id_to_body.get(&entity_id)?;
        let rigid_body = self.rigid_body_set.get(*handle)?;
        Some(nvec_to_cgmath(
            rigid_body.velocity_at_point(&Point::from(vec_to_nvec(point))),
        ))
    }

    pub fn set_velocity(&mut self, entity_id: EntityId, velocity: Vector3<f32>) {
        if let Some(handle) = self.entity_id_to_body.get(&entity_id) {
            let maybe_rigid_body = self.rigid_body_set.get_mut(*handle);

            if let Some(rigid_body) = maybe_rigid_body {
                rigid_body.set_linvel(vec_to_nvec(velocity), true);
            }
        }
    }

    /// Consume velocity transferred by the kinematic player's most recent
    /// walk contact with this live actor. Creature animation root motion is
    /// published after the physics update, so its normal velocity write adds
    /// this delta rather than erasing the shove before Rapier can integrate it.
    pub fn take_player_push_velocity(&mut self, entity_id: EntityId) -> Vector3<f32> {
        self.pending_player_push_velocity
            .remove(&entity_id)
            .unwrap_or_else(|| vec3(0.0, 0.0, 0.0))
    }

    pub fn get_rotation(&self, handle: RigidBodyHandle) -> Option<Quaternion<f32>> {
        let maybe_rigid_body = self.rigid_body_set.get(handle);

        maybe_rigid_body.map(|rigid_body| nquat_to_quat(*rigid_body.rotation()))
    }

    pub fn get_rotation2(&self, entity_id: EntityId) -> Option<Quaternion<f32>> {
        if let Some(handle) = self.entity_id_to_body.get(&entity_id) {
            let maybe_rigid_body = self.rigid_body_set.get(*handle);

            maybe_rigid_body.map(|rigid_body| nquat_to_quat(*rigid_body.rotation()))
        } else {
            None
        }
    }

    pub fn add_dynamic(
        &mut self,
        entity_id: EntityId,
        pos: Vector3<f32>,
        facing: Quaternion<f32>,
        offset: Vector3<f32>,
        shape: PhysicsShape,
        collision_group: CollisionGroup,
        is_sensor: bool,
        opts: DynamicPhysicsOptions,
    ) -> RigidBodyHandle {
        let nquat =
            nalgebra::geometry::Quaternion::new(facing.s, facing.v.x, facing.v.y, facing.v.z);
        let nquat_unit = UnitQuaternion::from_quaternion(nquat);

        let mut test = Isometry::identity();
        test.append_rotation_mut(&nquat_unit);
        test.translation = Translation {
            vector: vec_to_nvec(pos),
        };

        let mut rigid_body = RigidBodyBuilder::dynamic()
            // TODO: How can we make this more reliable? Seems to slow down the projectile randomly...
            // .ccd_enabled(true)
            .pose(test)
            .build();
        rigid_body.user_data = entity_id.inner() as u128;
        rigid_body.set_gravity_scale(opts.gravity_scale, false);
        //rigid_body.set_additional_mass(5.0, false);
        let handle = &self.rigid_body_set.insert(rigid_body);
        let mut collider = match shape {
            PhysicsShape::Capsule { height, radius } => {
                assert!(height > 0.0 && radius > 0.0);
                ColliderBuilder::capsule_y(height / 2.0, radius)
                    //.rotation(vector!(angles.0, angles.1, angles.2))
                    //.rotation(vector!(facing.z, facing.x, facing.y))
                    .translation(vec_to_nvec(offset))
                    //.position(test)
                    .restitution(opts.restitution)
                    .friction(opts.friction)
                    .build()
            }
            PhysicsShape::Cuboid(size) => {
                let size = sanitize_collider_size(entity_id, "add_dynamic", size);
                ColliderBuilder::cuboid(size.x / 2.0, size.y / 2.0, size.z / 2.0)
                    //.rotation(vector!(angles.0, angles.1, angles.2))
                    //.rotation(vector!(facing.z, facing.x, facing.y))
                    .translation(vec_to_nvec(offset))
                    //.position(test)
                    .restitution(opts.restitution)
                    .friction(opts.friction)
                    .build()
            }
            PhysicsShape::Sphere(size) => {
                let radius = sanitize_collider_radius(entity_id, "add_dynamic", size);
                ColliderBuilder::ball(radius)
                    //.rotation(vector!(angles.0, angles.1, angles.2))
                    //.rotation(vector!(facing.z, facing.x, facing.y))
                    .translation(vec_to_nvec(offset))
                    //.position(test)
                    .restitution(opts.restitution)
                    .friction(opts.friction)
                    .build()
            }
        };

        self.entity_id_to_body.insert(entity_id, *handle);
        collider.set_density(0.1);
        collider.set_enabled(true);
        collider.set_sensor(is_sensor);
        collider.set_collision_groups(collision_group.collision);
        collider.set_solver_groups(collision_group.solver);
        collider
            .set_active_events(ActiveEvents::COLLISION_EVENTS | ActiveEvents::CONTACT_FORCE_EVENTS);
        collider.user_data = entity_id.inner() as u128;
        self.collider_set
            .insert_with_parent(collider, *handle, &mut self.rigid_body_set);
        *handle
    }

    pub fn add_kinematic(
        &mut self,
        entity_id: EntityId,
        pos: Vector3<f32>,
        facing: Quaternion<f32>,
        offset: Vector3<f32>,
        size: Vector3<f32>,
        collision_groups: CollisionGroup,
        is_sensor: bool,
    ) -> RigidBodyHandle {
        self.add_kinematic_shape(
            entity_id,
            pos,
            facing,
            offset,
            PhysicsShape::Cuboid(size),
            collision_groups,
            is_sensor,
        )
    }

    /// Create a kinematic body with an explicit collider shape. `add_kinematic`
    /// is the cuboid special case; authored sphere/capsule geometry (notably
    /// immobile frobbable props) reaches physics through here so the exact
    /// authored shape survives instead of being replaced by a bounding box.
    pub fn add_kinematic_shape(
        &mut self,
        entity_id: EntityId,
        pos: Vector3<f32>,
        facing: Quaternion<f32>,
        offset: Vector3<f32>,
        shape: PhysicsShape,
        collision_groups: CollisionGroup,
        is_sensor: bool,
    ) -> RigidBodyHandle {
        let shape = match shape {
            PhysicsShape::Capsule { height, radius } => {
                assert!(height > 0.0 && radius > 0.0);
                SharedShape::capsule_y(height / 2.0, radius)
            }
            PhysicsShape::Cuboid(size) => {
                let size = sanitize_collider_size(entity_id, "add_kinematic", size);
                SharedShape::cuboid(size.x / 2.0, size.y / 2.0, size.z / 2.0)
            }
            PhysicsShape::Sphere(radius) => {
                let radius = sanitize_collider_radius(entity_id, "add_kinematic", radius);
                SharedShape::ball(radius)
            }
        };
        self.add_kinematic_shared_shape(
            entity_id,
            pos,
            facing,
            shape,
            offset,
            collision_groups,
            is_sensor,
        )
    }

    /// Create a kinematic body carrying an arbitrary shape, rather than the
    /// axis-aligned cuboid `add_kinematic` passes. The creature hitbox proxies
    /// use it so their collider is the *fitted* per-joint shape (capsule along
    /// the bone / box) shared with the ragdoll, which covers the body far
    /// better than a per-joint AABB.
    pub fn add_kinematic_shared_shape(
        &mut self,
        entity_id: EntityId,
        pos: Vector3<f32>,
        facing: Quaternion<f32>,
        shape: SharedShape,
        offset: Vector3<f32>,
        collision_groups: CollisionGroup,
        is_sensor: bool,
    ) -> RigidBodyHandle {
        let handle = self.add_kinematic_anchor(entity_id, pos, facing);
        let mut collider = ColliderBuilder::new(shape)
            .translation(vec_to_nvec(offset))
            .restitution(0.7)
            .build();

        collider.set_enabled(true);
        collider.set_sensor(is_sensor);
        collider.user_data = entity_id.inner() as u128;
        collider.set_collision_groups(collision_groups.collision);
        collider.set_solver_groups(collision_groups.solver);

        self.collider_set
            .insert_with_parent(collider, handle, &mut self.rigid_body_set);
        handle
    }

    /// Create a kinematic transform anchor without collision geometry.
    ///
    /// Zero-volume PhysAttach objects (notably `Lift 1 Walls`) still own a
    /// render transform that follows moving terrain, but contribute no shape
    /// to collision queries. A bare body preserves that transform flow without
    /// turning the marker into a tiny solid collider.
    pub fn add_kinematic_anchor(
        &mut self,
        entity_id: EntityId,
        pos: Vector3<f32>,
        facing: Quaternion<f32>,
    ) -> RigidBodyHandle {
        let nquat =
            nalgebra::geometry::Quaternion::new(facing.s, facing.v.x, facing.v.y, facing.v.z);
        let nquat_unit = UnitQuaternion::from_quaternion(nquat);
        // let angles = nquat_unit.euler_angles();

        // let r0 = 33.75f32.to_radians();
        // let r1 = 90f32.to_radians();
        // let r2 = 0.0;

        // r0-r1-r2
        // r0-r2-r1
        //let quat = UnitQuaternion::from_euler_angles(facing.y, facing.x, facing.z);

        let mut test = Isometry::identity();
        test.append_rotation_mut(&nquat_unit);
        test.translation = Translation {
            vector: vec_to_nvec(pos),
        };

        let mut rigid_body = RigidBodyBuilder::kinematic_position_based()
            .pose(test)
            .build();
        rigid_body.user_data = entity_id.inner() as u128;
        let handle = self.rigid_body_set.insert(rigid_body);
        self.entity_id_to_body.insert(entity_id, handle);
        handle
    }

    /// Attach a massless, contact-free model-bounds cuboid used only by frob
    /// and projectile rays. The body's first collider remains its authoritative
    /// physical shape, so movement, support, mass, and debug solidity reports
    /// continue to describe authored physics rather than selection geometry.
    pub fn add_interaction_cuboid(
        &mut self,
        handle: RigidBodyHandle,
        entity_id: EntityId,
        offset: Vector3<f32>,
        size: Vector3<f32>,
        collision_groups: CollisionGroup,
    ) -> bool {
        if self.rigid_body_set.get(handle).is_none() {
            return false;
        }
        let size = sanitize_collider_size(entity_id, "add_interaction_cuboid", size);
        let mut collider = ColliderBuilder::cuboid(size.x / 2.0, size.y / 2.0, size.z / 2.0)
            .translation(vec_to_nvec(offset))
            .density(0.0)
            .build();
        collider.set_enabled(true);
        collider.set_sensor(false);
        collider.user_data = entity_id.inner() as u128;
        let interaction = collision_groups.interaction_only();
        collider.set_collision_groups(interaction.collision);
        collider.set_solver_groups(interaction.solver);
        self.collider_set
            .insert_with_parent(collider, handle, &mut self.rigid_body_set);
        true
    }

    /// Attach one kinematic entity to another using Dark's PhysAttach
    /// translation semantics (`child = parent + offset`). Returns false when
    /// either entity has no kinematic body.
    pub fn attach_kinematic(
        &mut self,
        child_entity: EntityId,
        parent_entity: EntityId,
        offset: Vector3<f32>,
    ) -> bool {
        let (Some(&child), Some(&parent)) = (
            self.entity_id_to_body.get(&child_entity),
            self.entity_id_to_body.get(&parent_entity),
        ) else {
            return false;
        };
        if !self.rigid_body_set[child].is_kinematic() || !self.rigid_body_set[parent].is_kinematic()
        {
            return false;
        }

        self.kinematic_attachments.insert(
            child,
            KinematicAttachment {
                parent,
                offset: vec_to_nvec(offset),
            },
        );
        true
    }

    fn attachment_target_translation(
        &self,
        child: RigidBodyHandle,
        visiting: &mut HashSet<RigidBodyHandle>,
    ) -> Option<Vector<Real>> {
        let attachment = self.kinematic_attachments.get(&child)?;
        if !visiting.insert(child) {
            tracing::warn!(
                "[physics] cyclic kinematic attachment involving body {:?}; leaving it in place",
                child
            );
            return None;
        }

        let parent_translation = if self.kinematic_attachments.contains_key(&attachment.parent) {
            self.attachment_target_translation(attachment.parent, visiting)?
        } else {
            self.rigid_body_set
                .get(attachment.parent)?
                .next_position()
                .translation
                .vector
        };
        visiting.remove(&child);
        Some(parent_translation + attachment.offset)
    }

    fn update_kinematic_attachments(&mut self) {
        let targets = self
            .kinematic_attachments
            .keys()
            .copied()
            .filter_map(|child| {
                self.attachment_target_translation(child, &mut HashSet::new())
                    .map(|target| (child, target))
            })
            .collect::<Vec<_>>();

        for (child, target) in targets {
            if let Some(body) = self.rigid_body_set.get_mut(child) {
                body.set_next_kinematic_translation(target);
            }
        }
    }

    /// Move each held melee weapon toward its tracked-hand target, stopping it
    /// at the first world surface in the way.
    ///
    /// The weapon is a *kinematic* body swept with a shape cast, which is the
    /// third drive this has had and the first that behaves. A six-axis joint
    /// motor trailed the hand badly and its orientation error kept growing
    /// after the hand stopped. Driving a *dynamic* body by velocity fixed the
    /// tracking, but left the weapon subject to contact impulses: the body's
    /// origin sits out on the weapon head, so any contact torqued it and the
    /// weapon spun out of the player's hand the moment it touched a bench.
    ///
    /// A swept kinematic body has neither failure. The solver applies no
    /// impulses to it, so nothing can spin it; the sweep is what makes it
    /// stop at a wall rather than pass through one; and its contacts are still
    /// generated, which is what the damage rule reads.
    ///
    /// Orientation always takes the hand's exactly - it is never swept.
    /// Rotational sweeps are expensive and ill-defined against thin geometry,
    /// and the failure they would prevent (turning the blade into a wall) is
    /// far milder than the one taking the hand's rotation prevents (a weapon
    /// whose angle is not the angle of the hand holding it).
    ///
    /// Only WORLD geometry blocks. Actors deliberately do not: a swing has to
    /// travel *into* a creature to damage it, and loose props are better
    /// swept through than treated as walls.
    fn drive_held_melee(&mut self) {
        let max_step = crate::dev_params::get(crate::dev_params::MELEE_MAX_SPEED)
            * self.integration_parameters.dt;
        let pairs = self
            .held_melee_drives
            .iter()
            .map(|(weapon, drive)| (*weapon, drive.target))
            .collect::<Vec<_>>();
        for (weapon, target) in pairs {
            // `next_position`, not `position`: the hand pose arrives through
            // `set_next_kinematic_position`, which Rapier only commits during
            // the step. Reading the committed pose would aim at where the hand
            // was last frame, a permanent one-frame lag no gain removes.
            let Some(desired) = self
                .rigid_body_set
                .get(target)
                .map(|body| *body.next_position())
            else {
                continue;
            };
            let Some(body) = self.rigid_body_set.get(weapon) else {
                continue;
            };
            let current = *body.position();
            let delta = desired.translation.vector - current.translation.vector;
            let distance = delta.norm();

            // Cap how far one step may carry the weapon, and sweep the capped
            // distance - never skip the sweep for a long jump. Skipping it is
            // how a fast hand walks the weapon through a wall, and "fast" is
            // not a rare case in a test or a hitched frame. A genuine
            // teleport does not need the bypass either: it carries both motor
            // endpoints together (`translate_held_melee_for_player_relocation`),
            // so the error the drive sees is already near zero.
            let step = distance.min(max_step);
            let allowed = if distance <= 1.0e-6 {
                0.0
            } else {
                let direction = delta / distance;
                step * self.held_melee_sweep_fraction(weapon, &current, direction, step)
            };

            let mut next = desired;
            next.translation.vector = if distance <= 1.0e-6 {
                desired.translation.vector
            } else {
                current.translation.vector + (delta / distance) * allowed
            };
            if let Some(body) = self.rigid_body_set.get_mut(weapon) {
                body.set_next_kinematic_position(next);
            }
        }
    }

    /// How far along `direction * distance` this weapon's colliders may travel
    /// before one of them meets world geometry, as a fraction in `0..=1`.
    fn held_melee_sweep_fraction(
        &self,
        weapon: RigidBodyHandle,
        current: &Isometry<Real>,
        direction: Vector<Real>,
        distance: Real,
    ) -> Real {
        let Some(body) = self.rigid_body_set.get(weapon) else {
            return 1.0;
        };
        let filter = QueryFilter::new()
            .groups(InteractionGroups::new(
                InternalCollisionGroups::ENTITY.bits.into(),
                InternalCollisionGroups::WORLD.bits.into(),
                Default::default(),
            ))
            .exclude_rigid_body(weapon)
            .exclude_sensors();
        let dispatcher = self.narrow_phase.query_dispatcher();
        let queries = self.broad_phase.as_query_pipeline(
            dispatcher,
            &self.rigid_body_set,
            &self.collider_set,
            filter,
        );

        let mut nearest = distance;
        for collider_handle in body.colliders() {
            let Some(collider) = self.collider_set.get(*collider_handle) else {
                continue;
            };
            // Sweep from the collider's own world pose: a fitted melee volume
            // is offset from the body origin, so sweeping the body's origin
            // would probe empty space beside the weapon.
            let shape_pose = current * collider.position_wrt_parent().copied().unwrap_or_default();
            if let Some((_, hit)) = queries.cast_shape(
                &shape_pose,
                &direction,
                collider.shape(),
                rapier3d::parry::query::ShapeCastOptions {
                    max_time_of_impact: distance,
                    // Leave a hair of clearance so a weapon resting on a
                    // surface does not re-report a zero-distance hit forever.
                    target_distance: HELD_MELEE_SKIN,
                    // An already-penetrating start must not pin the weapon in
                    // place; let it keep sweeping out.
                    stop_at_penetration: false,
                    compute_impact_geometry_on_penetration: true,
                },
            ) {
                nearest = nearest.min(hit.time_of_impact);
            }
        }
        if distance <= 0.0 {
            1.0
        } else {
            (nearest / distance).clamp(0.0, 1.0)
        }
    }

    pub fn remove(&mut self, entity_id: EntityId) {
        let entity_as_int = entity_id.inner() as u128;
        let mut bodies_to_remove = Vec::new();
        for (handle, body) in self.rigid_body_set.iter() {
            if body.user_data == entity_as_int {
                bodies_to_remove.push(handle);
            }
        }
        self.kinematic_attachments.retain(|child, attachment| {
            !bodies_to_remove.contains(child) && !bodies_to_remove.contains(&attachment.parent)
        });
        let drives_to_remove = bodies_to_remove
            .iter()
            .filter_map(|handle| self.held_melee_drives.remove(handle))
            .collect::<Vec<_>>();
        for drive in drives_to_remove {
            self.rigid_body_set.remove(
                drive.target,
                &mut self.island_manager,
                &mut self.collider_set,
                &mut self.impulse_joint_set,
                &mut self.multibody_joint_set,
                true,
            );
        }
        for handle in bodies_to_remove {
            self.rigid_body_set.remove(
                handle,
                &mut self.island_manager,
                &mut self.collider_set,
                &mut self.impulse_joint_set,
                &mut self.multibody_joint_set,
                true,
            );
        }
        self.entity_id_to_body.remove(&entity_id);
        self.live_creature_sweep_recovery.remove(&entity_id);
        self.pending_player_push_velocity.remove(&entity_id);
    }

    pub fn create_player(
        &mut self,
        start_pos: Vector3<f32>,
        player_entity: EntityId,
    ) -> PlayerHandle {
        let mut rigid_body = RigidBodyBuilder::kinematic_position_based()
            .translation(vec_to_nvec(start_pos))
            .ccd_enabled(true)
            .build();

        let player_entity_user_data = player_entity.inner() as u128;
        rigid_body.user_data = player_entity_user_data;
        let character_handle = self.rigid_body_set.insert(rigid_body);
        // Dark's standing player is six SS2 feet tall and 2.4 feet wide. A
        // capsule is the continuous equivalent of its vertical sphere stack.
        let mut collider = ColliderBuilder::capsule_y(
            (PLAYER_STANDING_HEIGHT / 2.0 - PLAYER_STANDING_RADIUS) / SCALE_FACTOR,
            PLAYER_STANDING_RADIUS / SCALE_FACTOR,
        );
        collider = collider.collision_groups(InteractionGroups::new(
            InternalCollisionGroups::PLAYER.bits.into(),
            InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            Default::default(),
        ));
        collider = collider.user_data(player_entity_user_data);

        self.collider_set
            .insert_with_parent(collider, character_handle, &mut self.rigid_body_set);

        let controller = player_character_controller();
        self.entity_id_to_body
            .insert(player_entity, character_handle);

        PlayerHandle {
            controller,
            character_handle,
            is_crouched: false,
            top_out: None,
            support: None,
            slope_displacement: Vector::zeros(),
            is_grounded: false,
            jump_velocity: None,
            jump_was_pressed: false,
            self_translation: Vector3::new(0.0, 0.0, 0.0),
            is_climbing: false,
        }
    }

    /// Set the player's crouch state, resizing the capsule with the feet
    /// planted (the body translation is the collider center, so half the
    /// height difference is added/removed from it). Standing up is refused
    /// while there is not enough headroom for the standing capsule; the
    /// returned bool is the *resulting* crouch state.
    pub fn set_player_crouch(
        &mut self,
        want_crouch: bool,
        player_handle: &mut PlayerHandle,
    ) -> bool {
        // Dark disables ordinary player motion while its mantle sequence is
        // active. In particular, do not resize the temporary head sphere in
        // response to a crouch edge part-way across a lip.
        if player_handle.top_out.is_some() {
            return player_handle.is_crouched;
        }
        if want_crouch == player_handle.is_crouched {
            return player_handle.is_crouched;
        }

        let character_handle = player_handle.character_handle;
        let collider_handle = self.rigid_body_set[character_handle].colliders()[0];
        // Feet-planted center shift between the two capsule sizes.
        let center_shift = (PLAYER_STANDING_HEIGHT - PLAYER_CROUCH_HEIGHT) / 2.0 / SCALE_FACTOR;

        if want_crouch {
            self.collider_set[collider_handle].set_shape(crouched_player_shared_shape());
            let body = &mut self.rigid_body_set[character_handle];
            let mut translation = *body.translation();
            translation.y -= center_shift;
            body.set_translation(translation, true);
            player_handle.is_crouched = true;
        } else {
            // Headroom check: intersect a test capsule at the feet-planted
            // standing pose against the same groups the movement casts use.
            // The test capsule keeps the full segment but shrinks the radius
            // by the margin and is lifted by the margin, so its TOP sits
            // exactly at the standing crown (a ceiling lower than standing
            // height always blocks) while its BOTTOM floats 2x the margin
            // above the standing feet (the floor/steps the player rests on
            // never falsely block).
            let standing_pos = Translation::from(
                Vector::y() * (center_shift + PLAYER_STAND_TEST_MARGIN / SCALE_FACTOR),
            ) * self.rigid_body_set[character_handle].position();
            let test_shape = Capsule::new_y(
                (PLAYER_STANDING_HEIGHT / 2.0 - PLAYER_STANDING_RADIUS) / SCALE_FACTOR,
                (PLAYER_STANDING_RADIUS - PLAYER_STAND_TEST_MARGIN) / SCALE_FACTOR,
            );
            let filter = QueryFilter::new()
                .groups(InteractionGroups::new(
                    InternalCollisionGroups::PLAYER.bits.into(),
                    InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
                    Default::default(),
                ))
                .exclude_rigid_body(character_handle)
                .exclude_sensors();
            let dispatcher = self.narrow_phase.query_dispatcher();
            let queries = self.broad_phase.as_query_pipeline(
                dispatcher,
                &self.rigid_body_set,
                &self.collider_set,
                filter,
            );
            let overlaps_final_pose = queries
                .intersect_shape(standing_pos, &test_shape)
                .next()
                .is_some();
            // Level ceilings are authored as one-sided triangles facing down.
            // A static overlap against their underside can miss them, even
            // though an upward movement cast correctly blocks. Sweep a sphere
            // from the standing capsule's bottom axis endpoint to its top
            // endpoint: the swept sphere is exactly the capsule volume and
            // approaches those ceiling faces from below. Keep the final-pose
            // overlap above as well, because a cast configured to leave an
            // initial penetration may not report a prop already intersecting
            // the bottom sphere.
            let bottom_sphere_pos =
                Translation::from(Vector::y() * test_shape.segment.a.y) * standing_pos;
            let axis_length = (test_shape.segment.b - test_shape.segment.a).norm();
            let crown_sweep = Ball::new(test_shape.radius);
            let blocked_by_crown_sweep = queries
                .cast_shape(
                    &bottom_sphere_pos,
                    &Vector::y(),
                    &crown_sweep,
                    rapier3d::parry::query::ShapeCastOptions {
                        max_time_of_impact: axis_length,
                        target_distance: 0.0,
                        stop_at_penetration: false,
                        compute_impact_geometry_on_penetration: true,
                    },
                )
                .is_some();
            // Both probes above are spatial queries, and Rapier only builds the
            // broad-phase BVH inside `PhysicsPipeline::step`. On a world that
            // has never been stepped they therefore match nothing and report
            // "clear" everywhere, which is not an answer - it is the absence of
            // one. Refuse to expand into geometry we cannot see yet: the very
            // next frame has a populated broad phase and re-decides normally.
            //
            // This is exactly the load frame. `Game::load_from_file` rebuilds
            // the world, re-applies the saved crouch, and then runs a frame
            // whose crouch input is released - which used to stand the player
            // up inside the ceiling they had saved under (hydro2's "Low Head
            // Room" ledge, issue #773). A standing capsule embedded in level
            // geometry is unrecoverable: the character controller resolves zero
            // movement in every direction for the rest of the session.
            let blocked = !self.has_stepped || overlaps_final_pose || blocked_by_crown_sweep;

            if !blocked {
                self.collider_set[collider_handle].set_shape(standing_player_shared_shape());
                let body = &mut self.rigid_body_set[character_handle];
                let mut translation = *body.translation();
                translation.y += center_shift;
                body.set_translation(translation, true);
                player_handle.is_crouched = false;
            }
        }

        player_handle.is_crouched
    }

    pub fn new() -> PhysicsWorld {
        let rigid_body_set = RigidBodySet::new();
        let collider_set = ColliderSet::new();

        /* Create other structures necessary for the simulation. */
        let gravity = vector![0.0, -9.81, 0.0];
        let integration_parameters = IntegrationParameters {
            max_ccd_substeps: 1,
            ..IntegrationParameters::default()
        };
        let physics_pipeline = PhysicsPipeline::new();
        let island_manager = IslandManager::new();
        let broad_phase = DefaultBroadPhase::new();
        let narrow_phase = NarrowPhase::new();
        let impulse_joint_set = ImpulseJointSet::new();
        let multibody_joint_set = MultibodyJointSet::new();
        let ccd_solver = CCDSolver::new();

        let debug_pipeline = DebugRenderPipeline::new(
            DebugRenderStyle::default(),
            DebugRenderMode::default()
                | DebugRenderMode::COLLIDER_AABBS
                | DebugRenderMode::COLLIDER_SHAPES
                | DebugRenderMode::CONTACTS,
        );

        PhysicsWorld {
            gravity,
            integration_parameters,
            collider_set,
            physics_pipeline,
            island_manager,
            broad_phase,
            has_stepped: false,
            narrow_phase,
            impulse_joint_set,
            multibody_joint_set,
            ccd_solver,
            rigid_body_set,
            no_bodies: RigidBodySet::new(),
            rigid_bodies_with_forces: Vec::new(),
            // TODO:
            // physics_hooks: Box::new(physics_hooks),
            // event_handler: Box::new(event_handler),
            entity_id_to_body: HashMap::new(),
            live_creature_sweep_recovery: HashMap::new(),
            pending_player_push_velocity: HashMap::new(),
            kinematic_attachments: HashMap::new(),
            held_melee_drives: HashMap::new(),

            debug_pipeline,

            player_sensor_intersections: HashSet::new(),
            pending_player_sensor_events: Vec::new(),

            reported_nonfinite_entities: HashSet::new(),

            events: PhysicsEvents::new(),
        }
    }

    /// A line wireframe of every collider on `entity_id`'s primary body, read
    /// straight out of Rapier at each collider's own live world isometry.
    ///
    /// Deliberately *not* a recomputation of what the wield intended: a
    /// recomputation would agree with the renderer by construction and so
    /// could never show the case worth seeing, which is the simulated volume
    /// sitting somewhere other than the drawn weapon. Reuses `dark::hit_box`'s
    /// existing builders rather than growing a second debug-draw path.
    pub fn debug_entity_collider_lines(
        &self,
        entity_id: EntityId,
        color: Vector3<f32>,
    ) -> Vec<SceneObject> {
        let Some(handle) = self.entity_id_to_body.get(&entity_id) else {
            return Vec::new();
        };
        let Some(body) = self.rigid_body_set.get(*handle) else {
            return Vec::new();
        };
        let mut objects = Vec::new();
        for collider_handle in body.colliders() {
            let Some(collider) = self.collider_set.get(*collider_handle) else {
                continue;
            };
            let shape = match collider.shape().as_typed_shape() {
                TypedShape::Cuboid(cuboid) => dark::hit_box::HitBoxShape::Cuboid {
                    half_extents: nvec_to_cgmath(cuboid.half_extents),
                    center: Vector3::new(0.0, 0.0, 0.0),
                },
                TypedShape::Ball(ball) => dark::hit_box::HitBoxShape::Cuboid {
                    half_extents: Vector3::new(ball.radius, ball.radius, ball.radius),
                    center: Vector3::new(0.0, 0.0, 0.0),
                },
                TypedShape::Capsule(capsule) => dark::hit_box::HitBoxShape::Capsule {
                    a: nvec_to_cgmath(capsule.segment.a.coords),
                    b: nvec_to_cgmath(capsule.segment.b.coords),
                    radius: capsule.radius,
                },
                // Compounds and meshes have no single primitive to draw; a
                // held melee weapon never has one, and silently drawing an
                // approximation would be worse than drawing nothing.
                _ => continue,
            };
            let isometry = collider.position();
            let rotation = isometry.rotation;
            let world: cgmath::Matrix4<f32> =
                cgmath::Matrix4::from_translation(nvec_to_cgmath(isometry.translation.vector))
                    * cgmath::Matrix4::from(Quaternion::new(
                        rotation.w, rotation.i, rotation.j, rotation.k,
                    ));
            objects.extend(dark::hit_box::draw_debug_hit_box_shapes(
                &std::collections::HashMap::from([(0u32, shape)]),
                &[world],
                color,
            ));
        }
        objects
    }

    pub fn debug_render(&mut self) -> Vec<SceneObject> {
        let mut debug_renderer = DebugRenderer::new();

        self.debug_pipeline.render(
            &mut debug_renderer,
            &self.rigid_body_set,
            &self.collider_set,
            &self.impulse_joint_set,
            &self.multibody_joint_set,
            &self.narrow_phase,
        );

        // Joint-anchor overlay: each joint constrains a point on the parent
        // body (anchor1, cyan cross) to coincide with a point on the child
        // body (anchor2, magenta cross). A line connects them - a satisfied joint
        // has overlapping crosses and an invisible line; a sagging/separated joint
        // (e.g. the hips) shows a visible gap. Impulse and multibody joints both
        // draw (multibody anchors should always coincide - translation is not a
        // DOF there).
        let impulse_anchors = self
            .impulse_joint_set
            .iter()
            .filter_map(|(_h, joint)| {
                let b1 = self.rigid_body_set.get(joint.body1)?;
                let b2 = self.rigid_body_set.get(joint.body2)?;
                Some((
                    b1.position() * joint.data.local_frame1,
                    b2.position() * joint.data.local_frame2,
                ))
            })
            .collect::<Vec<_>>();
        let multibody_anchors = self
            .multibody_joint_anchor_pairs()
            .into_iter()
            .map(|(_, _, a1, a2)| (a1, a2));
        for (a1, a2) in impulse_anchors.into_iter().chain(multibody_anchors) {
            let p1 = Vector3::new(a1.translation.x, a1.translation.y, a1.translation.z);
            let p2 = Vector3::new(a2.translation.x, a2.translation.y, a2.translation.z);
            debug_renderer.add_cross(p1, 0.12, Vector3::new(0.0, 0.6, 1.0)); // anchor1 blue
            debug_renderer.add_cross(p2, 0.12, Vector3::new(1.0, 0.0, 1.0)); // anchor2 magenta
            debug_renderer.add_line(p1, p2, Vector3::new(1.0, 1.0, 0.0)); // gap (yellow)
        }

        debug_renderer.render()
    }

    /// Report (once per entity, ERROR level) any rigid body whose state went
    /// non-finite before it reaches the broad-phase. Detection only - no
    /// state is mutated.
    ///
    /// A NaN velocity integrates into a NaN pose, which puts a NaN collider
    /// AABB into the broad-phase BVH. parry's binned rebuild
    /// (`bvh_binned_build.rs`) bins leaves by their AABB centers, and a NaN
    /// center silently truncates the computed centroid range (NaN drops the
    /// accumulated min/max), so valid leaves land outside it and the bin
    /// index goes out of bounds - possibly many frames later, whenever the
    /// incremental optimizer happens to rebuild the poisoned subtree
    /// (issue #506; upstream: dimforge/rapier#961, still unfixed as of parry
    /// 0.29). The creation-time size sanitizers above can't catch this: the
    /// state goes bad at *runtime* (e.g. NaN animation joints driving a
    /// kinematic hitbox, #508). This report names the exact entity and the
    /// offending field(s) at the first bad frame, so the parry crash that
    /// follows a few frames later is fully attributed; the fix belongs at
    /// the producer's source, not here.
    fn report_nonfinite_rigid_body_state(&mut self) {
        fn finite_pose(pose: &Isometry<Real>) -> bool {
            pose.translation.vector.iter().all(|c| c.is_finite())
                && pose.rotation.coords.iter().all(|c| c.is_finite())
        }

        for (_handle, body) in self.rigid_body_set.iter() {
            if !body.is_enabled() {
                continue;
            }

            let linvel_bad = !body.linvel().iter().all(|c| c.is_finite());
            let angvel_bad = !body.angvel().iter().all(|c| c.is_finite());
            let pose_bad = !finite_pose(body.position());
            let next_pose_bad = body.is_kinematic() && !finite_pose(body.next_position());

            if !(linvel_bad || angvel_bad || pose_bad || next_pose_bad) {
                continue;
            }

            let entity_id = body
                .colliders()
                .first()
                .and_then(|c| self.collider_set.get(*c))
                .and_then(|c| EntityId::from_inner(c.user_data as u64));
            // A body fed bad state every frame (e.g. NaN animation joints
            // driving a kinematic hitbox) is only reported once.
            if !self.reported_nonfinite_entities.insert(entity_id) {
                continue;
            }

            let mut bad_fields = Vec::new();
            if linvel_bad {
                bad_fields.push(format!("linvel {:?}", body.linvel()));
            }
            if angvel_bad {
                bad_fields.push(format!("angvel {:?}", body.angvel()));
            }
            if pose_bad {
                bad_fields.push(format!("pose {:?}", body.position()));
            }
            if next_pose_bad {
                bad_fields.push(format!("next kinematic pose {:?}", body.next_position()));
            }
            tracing::error!(
                "[physics] entity {:?} ({:?} body at {:?}): non-finite rigid-body state: {} - this poisons the broad-phase BVH and will panic parry's binned rebuild within a few frames (#506; reported once per entity)",
                entity_id,
                body.body_type(),
                body.translation(),
                bad_fields.join(", "),
            );
        }
    }

    pub fn update(
        &mut self,
        desired_movement: Vector3<f32>,
        player_handle: &mut PlayerHandle,
    ) -> (Vector3<f32>, Vec<CollisionEvent>) {
        self.update_with_facing(desired_movement, desired_movement, player_handle)
    }

    /// Update player movement with an explicit world-space facing vector.
    /// Production input supplies camera/player facing for Dark-style mantling;
    /// the legacy wrapper above preserves direction-driven physics tests.
    pub fn update_with_facing(
        &mut self,
        desired_movement: Vector3<f32>,
        facing: Vector3<f32>,
        player_handle: &mut PlayerHandle,
    ) -> (Vector3<f32>, Vec<CollisionEvent>) {
        self.update_with_facing_and_jump(desired_movement, facing, false, player_handle)
    }

    /// Update player movement with facing and a held ordinary-jump button.
    ///
    /// The button is edge-triggered inside [`PlayerHandle`], starts only from
    /// controller-confirmed ground, and then follows a collision-cast ballistic
    /// arc until landing. Keeping the request here (rather than as a debug
    /// relocation/effect) means desktop, VR, and automated playtests all drive
    /// the same production character controller.
    pub fn update_with_facing_and_jump(
        &mut self,
        desired_movement: Vector3<f32>,
        facing: Vector3<f32>,
        jump_pressed: bool,
        player_handle: &mut PlayerHandle,
    ) -> (Vector3<f32>, Vec<CollisionEvent>) {
        // Queue every PhysAttach child at its parent's same next-frame target
        // before Rapier derives kinematic velocities. Moving-terrain assemblies
        // (tram floor + walls/buttons) therefore advance as one physical body,
        // matching Dark's source->destination attachment flow.
        self.update_kinematic_attachments();
        self.drive_held_melee();

        // Attribute any non-finite body state to its entity before the step
        // consumes it (see report_nonfinite_rigid_body_state) - by the time
        // parry panics, the culprit is already named in the log.
        self.report_nonfinite_rigid_body_state();

        /* Run the game loop, stepping the simulation once per frame. */
        profile!(scope: "physics", level: TRACE, "physics.step", {
            self.physics_pipeline.step(
                &self.gravity,
                &self.integration_parameters,
                &mut self.island_manager,
                &mut self.broad_phase,
                &mut self.narrow_phase,
                &mut self.rigid_body_set,
                &mut self.collider_set,
                &mut self.impulse_joint_set,
                &mut self.multibody_joint_set,
                &mut self.ccd_solver,
                &(),
                &self.events,
            )
        });
        self.has_stepped = true;

        // Update character controller
        let desired_movement = vec_to_nvec(desired_movement);
        let facing = vec_to_nvec(facing);
        let (mut collision_events, character_body) =
            { self.move_player(desired_movement, facing, jump_pressed, player_handle) };
        let translation = nvec_to_cgmath(*character_body.translation());

        let mut additional_collision_events = { self.events.get_and_clear_events() };

        collision_events.append(&mut additional_collision_events);

        // Output result
        (translation, collision_events)
    }

    /// Re-probe the moving terrain the player is standing on (see
    /// [`PlayerSupport`]). Needed after a relocation that did not go through a
    /// movement frame, so the next frame carries them from where they now are.
    fn refresh_player_support(&mut self, player_handle: &mut PlayerHandle) {
        let character_body = &self.rigid_body_set[player_handle.character_handle];
        let character_pos = *character_body.position();
        let character_shape = self.collider_set[character_body.colliders()[0]]
            .shared_shape()
            .clone();
        let filter = player_movement_filter(player_handle.character_handle);
        let dispatcher = self.narrow_phase.query_dispatcher();
        let queries = self.player_movement_queries(dispatcher, filter);
        player_handle.support = detect_support(
            &queries,
            &self.rigid_body_set,
            character_shape.as_ref(),
            &character_pos,
        );
    }

    /// The query pipeline every player movement cast runs against: the real
    /// broad phase and colliders, but **no rigid bodies**.
    ///
    /// Rapier's character controller has its own moving-platform support: for
    /// every *kinematic* collider the capsule touches, it adds that body's
    /// `velocity_at_point * dt` to the movement
    /// (`detect_grounded_status_and_apply_friction`). That is contact-based,
    /// not support-based - a wall or a door sliding *past* the player drags
    /// them along with it (measured at 10.6 wu in one second, see
    /// `player_is_not_dragged_by_a_wall_they_are_only_brushing`) - and it is
    /// lossy where it does apply, leaking ~10% of the platform's travel per
    /// second so a rider slides off the back of a long ride. We replace it
    /// with explicit support tracking ([`PlayerSupport`]), which is exact and
    /// only ever transfers motion from the surface underfoot.
    ///
    /// Emptying `bodies` is what switches the built-in off. The controller
    /// reads it in exactly two places: that friction lookup, and an autostep
    /// guard unreachable while `CharacterAutostep::include_dynamic_bodies` is
    /// true (rapier's default, which we keep). Our filters never consult it
    /// either - `exclude_rigid_body` compares collider parent handles
    /// directly, and only the `EXCLUDE_FIXED/KINEMATIC/DYNAMIC` flags would,
    /// which we never set. So collision itself is completely unaffected.
    fn player_movement_queries<'a>(
        &'a self,
        dispatcher: &'a dyn rapier3d::parry::query::QueryDispatcher,
        filter: QueryFilter<'a>,
    ) -> QueryPipeline<'a> {
        let mut queries = self.broad_phase.as_query_pipeline(
            dispatcher,
            &self.rigid_body_set,
            &self.collider_set,
            filter,
        );
        queries.bodies = &self.no_bodies;
        queries
    }

    fn move_player(
        &mut self,
        desired_movement: Vector<Real>,
        facing: Vector<Real>,
        jump_pressed: bool,
        player_handle: &mut PlayerHandle,
    ) -> (Vec<CollisionEvent>, &RigidBody) {
        let jump_edge = jump_pressed && !player_handle.jump_was_pressed;
        player_handle.jump_was_pressed = jump_pressed;
        let launch_jump = jump_edge && player_handle.is_grounded && player_handle.top_out.is_none();
        if launch_jump {
            player_handle.jump_velocity = Some(PLAYER_JUMP_SPEED / SCALE_FACTOR);
            player_handle.is_grounded = false;
            // A jumping player has left their moving support. Its carry is
            // already represented by the first frame's body pose; do not keep
            // transferring later platform motion through the air.
            player_handle.support = None;
        }

        let character_body = &self.rigid_body_set[player_handle.character_handle];
        let original_position = *character_body.position();
        let character_user_data = character_body.user_data;
        let character_collider = &self.collider_set[character_body.colliders()[0]];

        // In rapier 0.31 the `QueryPipeline` is a transient view built from the
        // broad-phase BVH, and it borrows the body/collider sets. Snapshot the
        // character shape (cheap Arc clone) and position up front so those
        // borrows are released before we build the query pipeline below.
        let character_shape = character_collider.shared_shape().clone();
        let character_pos = *character_collider.position();

        let gravity = player_gravity_step(&self.rigid_body_set[player_handle.character_handle]);

        let movement_filter = player_movement_filter(player_handle.character_handle);
        let dispatcher = self.narrow_phase.query_dispatcher();

        // Flat climbing: when the player overlaps a climbable surface (ladder)
        // and pushes toward it, redirect that input to vertical movement and
        // suppress the gravity pass for this frame (see `climb_redirect`).
        let climb_movement = if player_handle.jump_velocity.is_some() {
            None
        } else {
            let climb_filter = QueryFilter::new()
                .groups(InteractionGroups::new(
                    InternalCollisionGroups::PLAYER.bits.into(),
                    InternalCollisionGroups::CLIMBABLE.bits.into(),
                    Default::default(),
                ))
                .exclude_rigid_body(player_handle.character_handle)
                .exclude_sensors();
            let queries = self.broad_phase.as_query_pipeline(
                dispatcher,
                &self.rigid_body_set,
                &self.collider_set,
                climb_filter,
            );
            // Broad-phase candidate query: the capsule's bounds inflated by
            // CLIMB_REACH on x/z only (a cuboid, so the reach stays horizontal
            // - inflating the capsule radius would also extend the caps
            // vertically and grip ladders from above/below their ends).
            character_shape.as_capsule().and_then(|capsule| {
                let half_height = capsule.half_height() + capsule.radius;
                let inflated = Cuboid::new(vector![
                    capsule.radius + CLIMB_REACH,
                    half_height,
                    capsule.radius + CLIMB_REACH
                ]);
                let column_probe = Cuboid::new(vector![
                    capsule.radius + CLIMB_REACH,
                    half_height + CLIMB_TOP_OUT_COLUMN_LOOKAHEAD,
                    capsule.radius + CLIMB_REACH
                ]);
                let column_top = queries
                    .intersect_shape(character_pos, &column_probe)
                    .map(|(_, collider)| collider.compute_aabb().maxs.y)
                    .fold(f32::NEG_INFINITY, f32::max);
                let character_top = character_pos.translation.vector.y + half_height;
                let near_column_top = column_top - character_top <= CLIMB_TOP_OUT_TOP_REACH;
                // Grip the closest climbable within reach, by contact distance,
                // and take the contact's *face normal* as the climb direction.
                // (The collider-center direction is wrong when the player is
                // off-center: it tilts away from the face, which under-reads
                // the into-ladder push the grip test and climb speed use.)
                let mut nearest: Option<(f32, Vector<Real>)> = None;
                for (_handle, collider) in queries.intersect_shape(character_pos, &inflated) {
                    let contact = rapier3d::parry::query::contact(
                        &character_pos,
                        character_shape.as_ref(),
                        collider.position(),
                        collider.shape(),
                        CLIMB_REACH,
                    );
                    if let Ok(Some(contact)) = contact {
                        // normal1 points from the player toward the climbable.
                        let toward_h = vector![contact.normal1.x, 0.0, contact.normal1.z];
                        let toward_norm = toward_h.norm();
                        // A mostly-vertical normal means the player is on top of
                        // (or under) the surface - that's standing, not climbing.
                        if toward_norm > 0.5 && nearest.is_none_or(|(d, _)| contact.dist < d) {
                            let toward = toward_h / toward_norm;
                            nearest = Some((contact.dist, toward));
                        }
                    }
                }
                nearest.and_then(|(_, toward)| {
                    climb_redirect(desired_movement, toward).map(|movement| {
                        let top_out = if near_column_top && !player_handle.is_crouched {
                            climb_top_out_direction(desired_movement, facing).map(|direction| {
                                // Stay compressed until projected facing has
                                // exited every horizontally-expanded climbable
                                // AABB in this ladder column.
                                let capsule_clearance =
                                    capsule.radius + PLAYER_CONTACT_OFFSET / SCALE_FACTOR;
                                let minimum_clear_forward = queries
                                    .intersect_shape(character_pos, &column_probe)
                                    .filter_map(|(_, collider)| {
                                        climbable_aabb_exit_distance(
                                            character_pos.translation.vector,
                                            direction,
                                            &collider.compute_aabb(),
                                            capsule_clearance,
                                        )
                                    })
                                    .fold(0.0, f32::max);
                                (direction, minimum_clear_forward)
                            })
                        } else {
                            None
                        };
                        (movement, top_out)
                    })
                })
            })
        };

        // The climb cast collides with everything the walk does EXCEPT the
        // climbable surfaces themselves - see `ClimbPass`. Membership is checked
        // by predicate rather than by group filter because a ladder is also an
        // `ENTITY`, so masking the CLIMBABLE bit out of the group filter would
        // not exclude it.
        let not_climbable = |_handle: ColliderHandle, collider: &Collider| {
            !collider
                .collision_groups()
                .memberships
                .intersects(InternalCollisionGroups::CLIMBABLE.bits.into())
        };
        let parented_non_climbable = |handle: ColliderHandle, collider: &Collider| {
            collider.parent().is_some() && not_climbable(handle, collider)
        };
        let climb_pass_filter = movement_filter.predicate(&not_climbable);
        // Dark's scripted jump-through may cross immutable world terrain.
        // Structural parentage is the boundary: every parented entity collider
        // stays live, while only parentless terrain and climbables are omitted.
        let scripted_top_out_filter = movement_filter.predicate(&parented_non_climbable);

        // How far the moving terrain the player is standing on travelled since
        // the last time we saw them on it. The physics step above has already
        // advanced it to this frame's pose, so this is exactly the platform's
        // displacement for this frame. A support that has been removed simply
        // stops carrying.
        let carry = player_handle
            .support
            .and_then(|support| {
                self.rigid_body_set
                    .get(support.body)
                    .map(|body| body.translation() - support.translation)
            })
            .unwrap_or_else(Vector::zeros);

        let player_movement = profile!(scope: "physics", level: TRACE, "physics.move_player", {
            let queries = self.player_movement_queries(dispatcher, movement_filter);
            if let Some(top_out) = player_handle.top_out {
                let (movement, top_out) = advance_climb_top_out(
                    &player_handle.controller,
                    &queries,
                    &queries.with_filter(scripted_top_out_filter),
                    &character_pos,
                    top_out,
                    self.integration_parameters.dt,
                );
                PlayerMovement {
                    self_translation: movement.translation,
                    movement,
                    is_climbing: true,
                    top_out,
                    slope_displacement: Vector::zeros(),
                    actor_collisions: Vec::new(),
                }
            } else {
                // A compressed mantle restores the same standing/crouched
                // capsule it started with, so a player deliberately crouched
                // for a low stacked route never expands under its ceiling.
                let jump_mantle = launch_jump
                    .then(|| {
                        plan_jump_mantle(
                            &player_handle.controller,
                            &queries,
                            &queries.with_filter(scripted_top_out_filter),
                            character_shape.as_ref(),
                            &character_pos,
                            desired_movement,
                            self.integration_parameters.dt,
                            player_handle.is_crouched,
                        )
                    })
                    .flatten();
                if let Some(jump_mantle) = jump_mantle {
                    jump_mantle
                } else {
                    let airborne_vertical = player_handle
                        .jump_velocity
                        .map(|velocity| velocity * self.integration_parameters.dt);
                    step_player_movement(
                        &player_handle.controller,
                        &queries,
                        character_shape.as_ref(),
                        &character_pos,
                        desired_movement,
                        carry,
                        self.integration_parameters.dt,
                        gravity,
                        Some(player_handle.slope_displacement),
                        airborne_vertical,
                        climb_movement.map(|(movement, top_out)| ClimbPass {
                            movement,
                            top_out,
                            validation_queries: queries,
                            probe_queries: queries.with_filter(climb_pass_filter),
                            scripted_queries: queries.with_filter(scripted_top_out_filter),
                        }),
                    )
                }
            }
        });
        let was_top_out = player_handle.top_out.is_some();
        // Recorded by whichever branch produced the movement, because only the
        // walk pass folds the platform carry in (see `PlayerMovement`).
        let self_translation = player_movement.self_translation;
        player_handle.self_translation = nvec_to_cgmath(self_translation);
        player_handle.is_climbing = player_movement.is_climbing;
        player_handle.top_out = player_movement.top_out;
        player_handle.slope_displacement = player_movement.slope_displacement;
        let is_top_out = player_handle.top_out.is_some();
        let collider_handle = self.rigid_body_set[player_handle.character_handle].colliders()[0];
        if !was_top_out && is_top_out {
            let compressed_radius = if player_handle
                .top_out
                .is_some_and(|top_out| top_out.is_crouched)
            {
                PLAYER_CROUCH_RADIUS / SCALE_FACTOR
            } else {
                CLIMB_TOP_OUT_RADIUS
            };
            self.collider_set[collider_handle].set_shape(SharedShape::ball(compressed_radius));
        } else if was_top_out && !is_top_out {
            let restored = if player_handle.is_crouched {
                crouched_player_shared_shape()
            } else {
                standing_player_shared_shape()
            };
            self.collider_set[collider_handle].set_shape(restored);
        }
        self.rigid_body_set[player_handle.character_handle].enable_ccd(!is_top_out);
        let scripted_top_out_frame = was_top_out || is_top_out;
        let mvt = player_movement.movement;
        let actor_collisions = player_movement.actor_collisions;

        // Dark's dynamic character bodies shove one another at close range.
        // Our player is kinematic, so Rapier's ordinary contact solver cannot
        // transfer its requested motion to a live creature: two lightweight
        // actors touching opposite sides of the capsule leave every cast at
        // zero and pin the player permanently. Apply Rapier's character-body
        // impulse approximation for the intentional walk contacts only. The
        // actor-only query keeps floors, doors, corpses, hitboxes, and loose
        // props unchanged; gravity, platform carry, and scripted mantles never
        // generate a shove.
        if !actor_collisions.is_empty() {
            let character_mass = self.rigid_body_set[player_handle.character_handle].mass();
            let actor_bodies = actor_collisions
                .iter()
                .filter_map(|collision| self.collider_set[collision.handle].parent())
                .collect::<HashSet<_>>();
            let before = actor_bodies
                .iter()
                .filter_map(|handle| {
                    self.rigid_body_set.get(*handle).map(|body| {
                        (
                            *handle,
                            EntityId::from_inner(body.user_data as u64),
                            *body.linvel(),
                        )
                    })
                })
                .collect::<Vec<_>>();
            let actor_filter = QueryFilter::new()
                .groups(InteractionGroups::new(
                    InternalCollisionGroups::PLAYER.bits.into(),
                    InternalCollisionGroups::ACTOR.bits.into(),
                    Default::default(),
                ))
                .exclude_rigid_body(player_handle.character_handle)
                .exclude_sensors();
            let mut queries = self.broad_phase.as_query_pipeline_mut(
                dispatcher,
                &mut self.rigid_body_set,
                &mut self.collider_set,
                actor_filter,
            );
            player_handle.controller.solve_character_collision_impulses(
                self.integration_parameters.dt,
                &mut queries,
                character_shape.as_ref(),
                character_mass,
                &actor_collisions,
            );
            drop(queries);
            for (handle, maybe_entity_id, velocity_before) in before {
                let Some(entity_id) = maybe_entity_id else {
                    continue;
                };
                let Some(body) = self.rigid_body_set.get(handle) else {
                    continue;
                };
                let velocity_delta = *body.linvel() - velocity_before;
                let delta = vec3(velocity_delta.x, 0.0, velocity_delta.z);
                if delta.magnitude2() > 0.0 {
                    self.pending_player_push_velocity
                        .entry(entity_id)
                        .and_modify(|pending| *pending += delta)
                        .or_insert(delta);
                }
            }
        }

        if is_top_out {
            player_handle.jump_velocity = None;
            player_handle.is_grounded = false;
        } else if let Some(mut velocity) = player_handle.jump_velocity {
            let requested_vertical = velocity * self.integration_parameters.dt;
            let applied_vertical = self_translation.y;
            if mvt.grounded && requested_vertical <= 0.0 {
                player_handle.jump_velocity = None;
                player_handle.is_grounded = true;
            } else {
                // A ceiling (or other overhead collision) consumes the upward
                // cast. Cancel the rise immediately, then let the next frame's
                // downward half of the same arc settle normally.
                if requested_vertical > 0.0 && applied_vertical < requested_vertical * 0.5 {
                    velocity = 0.0;
                }
                velocity -= PLAYER_JUMP_GRAVITY / SCALE_FACTOR * self.integration_parameters.dt;
                player_handle.jump_velocity =
                    Some(velocity.max(-PLAYER_MAX_FALL_SPEED / SCALE_FACTOR));
                player_handle.is_grounded = false;
            }
        } else {
            player_handle.is_grounded = mvt.grounded;
        }

        // Edges already observed along a swept `move_player_validated` hop come
        // first: they happened before this frame's pose. They also leave
        // `player_sensor_intersections` at the hop's final occupancy, so the
        // diff below sees no change for anything they already reported.
        let mut collision_events = std::mem::take(&mut self.pending_player_sensor_events);
        let current_sensor_intersections = profile!(scope: "physics", level: TRACE, "physics.intersections_with_shape", {
            // Only consider sensor colliders for player/sensor intersections.
            let sensor_filter = QueryFilter::new().predicate(&is_sensor_collider);
            let queries = self.broad_phase.as_query_pipeline(
                dispatcher,
                &self.rigid_body_set,
                &self.collider_set,
                sensor_filter,
            );
            sensors_overlapping(&queries, &original_position, character_shape.as_ref())
        });

        let player_id = EntityId::from_inner(character_user_data as u64).unwrap();

        collision_events.extend(sensor_transition_events(
            &self.player_sensor_intersections,
            &current_sensor_intersections,
            player_id,
        ));

        self.player_sensor_intersections = current_sensor_intersections;

        // Re-probe the surface underfoot at the pose the player just reached,
        // so next frame carries them by however far it moves. A scripted
        // mantle is running on a temporary compressed shape and is not
        // standing on anything - it rides nothing.
        player_handle.support = (!scripted_top_out_frame)
            .then(|| {
                let queries = self.player_movement_queries(dispatcher, movement_filter);
                detect_support(
                    &queries,
                    &self.rigid_body_set,
                    character_shape.as_ref(),
                    &(Translation::from(mvt.translation) * character_pos),
                )
            })
            .flatten();

        let character_body = &mut self.rigid_body_set[player_handle.character_handle];
        let pos = character_body.position();
        let target = pos.translation.vector + mvt.translation;
        if scripted_top_out_frame {
            // Dark's mantle states directly drive their preflighted target
            // locations. Rapier's kinematic next-position path still clips the
            // compressed sphere against the lip this special movement is
            // crossing; apply the small scripted substep immediately, as the
            // collision-validated debug move does for its already-cast poses.
            character_body.set_translation(target, true);
            character_body.set_next_kinematic_translation(target);
        } else {
            character_body.set_next_kinematic_translation(target);
        }
        (collision_events, character_body)
    }

    pub fn ray_cast2(
        &self,
        start_point: Point3<f32>,
        direction: Vector3<f32>,
        max_toi: f32,
        collision_groups: InternalCollisionGroups,
        entity_to_ignore: Option<EntityId>,
        ignore_sensors: bool,
    ) -> Option<RayCastResult> {
        self.ray_cast2_with_memberships(
            InternalCollisionGroups::ALL,
            start_point,
            direction,
            max_toi,
            collision_groups,
            entity_to_ignore,
            ignore_sensors,
            None,
        )
    }

    /// Raycast using a living actor's collision membership. Unlike a generic
    /// interaction/visibility ray (whose membership is `ALL`), this respects
    /// colliders that explicitly opt out of blocking creature movement.
    pub fn ray_cast2_as_actor(
        &self,
        start_point: Point3<f32>,
        direction: Vector3<f32>,
        max_toi: f32,
        collision_groups: InternalCollisionGroups,
        entity_to_ignore: Option<EntityId>,
        ignore_sensors: bool,
    ) -> Option<RayCastResult> {
        self.ray_cast2_with_memberships(
            InternalCollisionGroups::ACTOR,
            start_point,
            direction,
            max_toi,
            collision_groups,
            entity_to_ignore,
            ignore_sensors,
            None,
        )
    }

    /// Raycast with living-actor membership while allowing the caller to
    /// reject individual entity colliders. The filter is consulted only for
    /// colliders that carry an entity id, so ownerless colliders - world
    /// geometry among them - are retained without being offered to it; an
    /// entity-owned collider is subject to the filter whatever it represents.
    /// This is the visibility-query path:
    /// actor membership naturally skips interaction-only bounds that do not
    /// block creatures, while the entity filter can pass through authored
    /// transparent objects without changing projectile or selection rays.
    pub fn ray_cast2_as_actor_with_entity_filter(
        &self,
        start_point: Point3<f32>,
        direction: Vector3<f32>,
        max_toi: f32,
        collision_groups: InternalCollisionGroups,
        entity_to_ignore: Option<EntityId>,
        ignore_sensors: bool,
        entity_filter: &dyn Fn(EntityId) -> bool,
    ) -> Option<RayCastResult> {
        self.ray_cast2_with_memberships(
            InternalCollisionGroups::ACTOR,
            start_point,
            direction,
            max_toi,
            collision_groups,
            entity_to_ignore,
            ignore_sensors,
            Some(entity_filter),
        )
    }

    /// Generic interaction raycast with a per-entity rejection predicate.
    /// Ownerless world geometry is always retained. This is intentionally the
    /// same all-membership query as [`Self::ray_cast2`]; callers use it only
    /// when a semantic relationship makes one entity transparent to the ray.
    pub fn ray_cast2_with_entity_filter(
        &self,
        start_point: Point3<f32>,
        direction: Vector3<f32>,
        max_toi: f32,
        collision_groups: InternalCollisionGroups,
        entity_to_ignore: Option<EntityId>,
        ignore_sensors: bool,
        entity_filter: &dyn Fn(EntityId) -> bool,
    ) -> Option<RayCastResult> {
        self.ray_cast2_with_memberships(
            InternalCollisionGroups::ALL,
            start_point,
            direction,
            max_toi,
            collision_groups,
            entity_to_ignore,
            ignore_sensors,
            Some(entity_filter),
        )
    }

    fn ray_cast2_with_memberships(
        &self,
        query_memberships: InternalCollisionGroups,
        start_point: Point3<f32>,
        direction: Vector3<f32>,
        max_toi: f32,
        collision_groups: InternalCollisionGroups,
        entity_to_ignore: Option<EntityId>,
        ignore_sensors: bool,
        entity_filter: Option<&dyn Fn(EntityId) -> bool>,
    ) -> Option<RayCastResult> {
        // Guard against degenerate rays. A zero-length direction normalizes to
        // NaN, and a NaN/zero ray direction sends parry's `clip_aabb_line` down
        // its `near_side == 0` path (see parry3d clip_aabb_line.rs): when the
        // ray origin also lies inside a collider AABB it returns face index 0,
        // and parry's feature math then computes `0u32 - 1`, which panics with
        // "attempt to subtract with overflow" in debug builds (issue #405) and
        // silently returns garbage hits in release. Reject such rays here - the
        // single choke point every raycast funnels through - rather than let a
        // bad caller crash the whole runtime.
        // Reject only truly non-normalizable input: an exactly-zero or
        // non-finite direction (both normalize to NaN) or a non-finite origin. A
        // tiny-but-nonzero direction still normalizes cleanly, so - unlike a
        // `< EPSILON` bound - this never rejects legitimate short rays such as
        // `ray_cast3`'s `end - start`.
        let norm_sq = direction.magnitude2();
        if !(start_point.x.is_finite() && start_point.y.is_finite() && start_point.z.is_finite())
            || !norm_sq.is_finite()
            || norm_sq == 0.0
        {
            tracing::debug!(
                "ray_cast2: rejecting degenerate ray (origin={:?}, direction={:?})",
                start_point,
                direction
            );
            return None;
        }
        let direction = direction / norm_sq.sqrt();
        let ray = Ray::new(
            point![start_point.x, start_point.y, start_point.z],
            vector![direction.x, direction.y, direction.z],
        );
        // TODO: Take end point instead
        let solid = true;
        let mut filter = QueryFilter::default();

        if ignore_sensors {
            filter = filter.exclude_sensors()
        };

        let binding = |_collider_handle: ColliderHandle, collider: &Collider| {
            let data = collider.user_data;
            let maybe_entity_id = EntityId::from_inner(data as u64);
            if maybe_entity_id == entity_to_ignore {
                return false;
            }
            if let (Some(entity_id), Some(entity_filter)) = (maybe_entity_id, entity_filter)
                && !entity_filter(entity_id)
            {
                return false;
            }
            // A degenerate collider (zero-extent / non-finite AABB, e.g.
            // medsci1's point-sized "Lift 1 Walls") poisons parry's ray-AABB
            // clip the same way a degenerate ray does: face index 0 ->
            // `0u32 - 1` overflow panic in debug builds, NaN normals in
            // release. Skip such colliders - a point can't meaningfully block
            // a ray, and letting one through crashes AI vision/ground probes.
            // (`GET /v1/physics/colliders/validate` audits the data root
            // cause.)
            let aabb = collider.compute_aabb();
            aabb.mins.iter().all(|v| v.is_finite())
                && aabb.extents().iter().all(|e| e.is_finite() && *e > 1.0e-5)
        };
        filter = filter.predicate(&binding);

        filter = filter.groups(InteractionGroups::new(
            query_memberships.bits.into(),
            collision_groups.bits.into(),
            Default::default(),
        ));

        let queries = self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.rigid_body_set,
            &self.collider_set,
            filter,
        );

        if let Some((handle, intersection)) = queries.cast_ray_and_get_normal(&ray, max_toi, solid)
        {
            // This is similar to `QueryPipeline::cast_ray` illustrated above except
            // that it also returns the normal of the collider shape at the hit point.
            let hit_point = ray.point_at(intersection.time_of_impact);
            let hit_normal = intersection.normal;
            let collider = self.collider_set.get(handle).unwrap();
            let maybe_rigid_body_handle = collider.parent();
            let data = collider.user_data;

            let maybe_entity_id = EntityId::from_inner(data as u64);

            // let rigid_body_handle = self.collider_set.get(handle).unwrap().parent().unwrap();
            // let rigid_body = self.rigid_body_set.get(rigid_body_handle).unwrap();

            // println!(
            //     "Collider {:?} hit at point {} with normal {} collider_data: {}",
            //     handle, hit_point, hit_normal, data
            // );

            Some(RayCastResult {
                hit_point: npoint_to_cgmath(hit_point),
                hit_normal: nvec_to_cgmath(hit_normal),
                maybe_entity_id,
                maybe_rigid_body_handle,
                is_sensor: collider.is_sensor(),
            })
        } else {
            None
        }
    }

    pub fn ray_cast(
        &self,
        start_point: Point3<f32>,
        direction: Vector3<f32>,
        collision_groups: InternalCollisionGroups,
    ) -> Option<RayCastResult> {
        self.ray_cast2(start_point, direction, 100.0, collision_groups, None, true)
    }

    /// Neutralize only the horizontal velocity transfer where moving
    /// kinematic terrain sweeps through the side of a living creature, with
    /// one validated recovery frame if that contact already removed support.
    ///
    /// The active side contact is essential: unsupported creatures otherwise
    /// fall normally, and vertical lifts or horizontal platforms contacted
    /// underfoot still carry their riders. Callers pass only currently living
    /// creatures, so dead capsules/ragdolls retain their independent physics
    /// lifecycle.
    pub fn recover_live_creatures_swept_off_support(&mut self, living_creatures: &[EntityId]) {
        self.live_creature_sweep_recovery
            .retain(|entity_id, _| living_creatures.contains(entity_id));

        for entity_id in living_creatures {
            let Some(handle) = self.entity_id_to_body.get(entity_id).copied() else {
                continue;
            };
            let Some(body) = self.rigid_body_set.get(handle) else {
                continue;
            };
            if !body.is_dynamic() || body.gravity_scale() <= 0.0 {
                self.live_creature_sweep_recovery.remove(entity_id);
                continue;
            }

            let position = nvec_to_cgmath(*body.translation());
            let colliders = body.colliders();
            let lowest_collider = body
                .colliders()
                .iter()
                .filter_map(|collider| self.collider_set.get(*collider))
                .map(|collider| collider.compute_aabb().mins.y)
                .min_by(f32::total_cmp);
            let Some(lowest_collider) = lowest_collider else {
                continue;
            };

            let horizontal_sweep = colliders.iter().find_map(|creature_collider| {
                self.narrow_phase
                    .contact_pairs_with(*creature_collider)
                    .filter(|pair| pair.has_any_active_contact)
                    .find_map(|pair| {
                        let other_collider = if pair.collider1 == *creature_collider {
                            pair.collider2
                        } else {
                            pair.collider1
                        };
                        let Some(other_body) = self.collider_set[other_collider]
                            .parent()
                            .and_then(|handle| self.rigid_body_set.get(handle))
                        else {
                            return None;
                        };
                        let horizontal_motion = other_body.linvel().x * other_body.linvel().x
                            + other_body.linvel().z * other_body.linvel().z;
                        let is_horizontal_kinematic_side_contact = other_body.body_type()
                            == RigidBodyType::KinematicPositionBased
                            && horizontal_motion > 1.0e-6
                            && pair.manifolds.iter().any(|manifold| {
                                !manifold.data.solver_contacts.is_empty()
                                    && manifold.data.normal.y.abs() < 0.5
                            });
                        is_horizontal_kinematic_side_contact.then(|| {
                            (
                                vector![other_body.linvel().x, 0.0, other_body.linvel().z]
                                    / horizontal_motion.sqrt(),
                                self.collider_set[other_collider].parent().unwrap(),
                            )
                        })
                    })
            });
            let swept_by_horizontal_kinematic = horizontal_sweep.is_some();
            let recovery = self.live_creature_sweep_recovery.get(entity_id).copied();
            // Most creatures never touch moving terrain. Avoid a per-creature
            // support ray at 60 Hz until a moving side-contact starts a
            // recovery episode (or for its one-frame contact-loss grace).
            if !swept_by_horizontal_kinematic && recovery.is_none() {
                continue;
            }

            // Reach from the body origin through its lowest collider point,
            // plus one world unit for stairs, slopes, and contact separation.
            let probe_distance = (position.y - lowest_collider + 1.0).max(1.0);
            let moving_body = horizontal_sweep
                .map(|(_, moving_body)| moving_body)
                .or_else(|| recovery.map(|state| state.moving_body))
                .unwrap();
            let support_velocity = self.walkable_support_velocity_excluding_body(
                *entity_id,
                position,
                probe_distance,
                moving_body,
            );
            let supported = support_velocity.is_some();

            if supported {
                if swept_by_horizontal_kinematic {
                    let support_translation = recovery
                        .map(|state| {
                            state.support_translation
                                + support_velocity.unwrap() * self.integration_parameters.dt
                        })
                        .unwrap_or_else(|| vec_to_nvec(position));
                    let sweep_direction = horizontal_sweep.unwrap().0;
                    if let Some(body) = self.rigid_body_set.get_mut(handle) {
                        body.set_translation(support_translation, true);
                        let mut velocity = *body.linvel();
                        let inherited_speed = velocity.dot(&sweep_direction);
                        if inherited_speed > 0.0 {
                            velocity -= sweep_direction * inherited_speed;
                            body.set_linvel(velocity, true);
                        }
                    }
                    self.live_creature_sweep_recovery.insert(
                        *entity_id,
                        LiveCreatureSweepRecovery {
                            support_translation,
                            had_moving_side_contact: true,
                            horizontal_sweep_direction: sweep_direction,
                            moving_body,
                        },
                    );
                } else {
                    // The creature is still safe, but Rapier leaves it with
                    // the mover's horizontal normal velocity after contact
                    // ends. Remove only that forward component; AI locomotion
                    // can continue on the following frame and tangential or
                    // opposing motion is preserved.
                    if let Some(recovery) = recovery {
                        if let Some(body) = self.rigid_body_set.get_mut(handle) {
                            let mut velocity = *body.linvel();
                            let inherited_speed =
                                velocity.dot(&recovery.horizontal_sweep_direction);
                            if inherited_speed > 0.0 {
                                velocity -= recovery.horizontal_sweep_direction * inherited_speed;
                                body.set_linvel(velocity, true);
                            }
                        }
                    }
                    self.live_creature_sweep_recovery.remove(entity_id);
                }
            } else if let Some(recovery) = recovery {
                if swept_by_horizontal_kinematic || recovery.had_moving_side_contact {
                    let anchor_position = nvec_to_cgmath(recovery.support_translation);
                    let anchor_is_still_supported = self
                        .walkable_support_velocity_excluding_body(
                            *entity_id,
                            anchor_position,
                            probe_distance,
                            moving_body,
                        )
                        .is_some();

                    if anchor_is_still_supported {
                        if let Some(body) = self.rigid_body_set.get_mut(handle) {
                            body.set_translation(recovery.support_translation, true);
                            body.set_linvel(Vector::zeros(), true);
                            body.set_angvel(Vector::zeros(), true);
                        }
                        self.live_creature_sweep_recovery.insert(
                            *entity_id,
                            LiveCreatureSweepRecovery {
                                had_moving_side_contact: swept_by_horizontal_kinematic,
                                horizontal_sweep_direction: horizontal_sweep
                                    .map(|(direction, _)| direction)
                                    .unwrap_or(recovery.horizontal_sweep_direction),
                                moving_body,
                                ..recovery
                            },
                        );
                    } else {
                        self.live_creature_sweep_recovery.remove(entity_id);
                    }
                } else {
                    self.live_creature_sweep_recovery.remove(entity_id);
                }
            }
        }
    }

    fn walkable_support_velocity_excluding_body(
        &self,
        entity_to_ignore: EntityId,
        position: Vector3<f32>,
        probe_distance: f32,
        body_to_ignore: RigidBodyHandle,
    ) -> Option<Vector<Real>> {
        let ray = Ray::new(
            point![position.x, position.y, position.z],
            -Vector::y_axis().into_inner(),
        );
        let predicate = |_handle: ColliderHandle, collider: &Collider| {
            if collider.is_sensor()
                || collider.parent() == Some(body_to_ignore)
                || EntityId::from_inner(collider.user_data as u64) == Some(entity_to_ignore)
            {
                return false;
            }
            let aabb = collider.compute_aabb();
            aabb.mins.iter().all(|v| v.is_finite())
                && aabb.extents().iter().all(|e| e.is_finite() && *e > 1.0e-5)
        };
        let filter = QueryFilter::default()
            .exclude_sensors()
            .groups(InteractionGroups::new(
                InternalCollisionGroups::ACTOR.bits.into(),
                InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
                Default::default(),
            ))
            .predicate(&predicate);
        let queries = self.broad_phase.as_query_pipeline(
            self.narrow_phase.query_dispatcher(),
            &self.rigid_body_set,
            &self.collider_set,
            filter,
        );
        let (collider, hit) = queries.cast_ray_and_get_normal(&ray, probe_distance, true)?;
        (hit.normal.y >= 0.4).then(|| {
            self.collider_set[collider]
                .parent()
                .and_then(|handle| self.rigid_body_set.get(handle))
                .map(|body| *body.linvel())
                .unwrap_or_else(Vector::zeros)
        })
    }

    pub fn ray_cast3(
        &self,
        start_point: Point3<f32>,
        end_point: Point3<f32>,
        collision_groups: InternalCollisionGroups,
        entity_to_ignore: Option<EntityId>,
        ignore_sensors: bool,
    ) -> Option<RayCastResult> {
        let direction = end_point - start_point;
        self.ray_cast2(
            start_point,
            direction,
            direction.magnitude(),
            collision_groups,
            entity_to_ignore,
            ignore_sensors,
        )
    }

    pub(crate) fn set_enabled_rotations(
        &mut self,
        entity_id: EntityId,
        arg_1: bool,
        arg_2: bool,
        arg_3: bool,
    ) {
        if let Some(handle) = self.entity_id_to_body.get(&entity_id) {
            let maybe_rigid_body = self.rigid_body_set.get_mut(*handle);

            if let Some(rigid_body) = maybe_rigid_body {
                rigid_body.set_enabled_rotations(arg_1, arg_2, arg_3, true);
            }
        }
    }

    // ============================================================================
    // Ragdoll Physics Utilities
    // ============================================================================

    /// Create a dynamic rigid body without requiring an EntityId
    /// Returns the handle for use in ragdoll systems
    pub fn create_dynamic_body(
        &mut self,
        isometry: Isometry<Real>,
        user_tag: Option<EntityId>,
    ) -> RigidBodyHandle {
        let mut rigid_body = RigidBodyBuilder::dynamic().pose(isometry).build();

        // Set user data if provided
        if let Some(entity_id) = user_tag {
            rigid_body.user_data = entity_id.inner() as u128;
        }

        self.rigid_body_set.insert(rigid_body)
    }

    /// Set linear and angular damping on a rigid body. Ragdoll limbs need
    /// non-zero damping (especially angular) so that a limb not in contact with
    /// the world bleeds off momentum and comes to rest, instead of spinning or
    /// flailing indefinitely.
    pub fn set_body_damping(&mut self, handle: RigidBodyHandle, linear: f32, angular: f32) {
        if let Some(body) = self.rigid_body_set.get_mut(handle) {
            body.set_linear_damping(linear);
            body.set_angular_damping(angular);
        }
    }

    /// Put a dynamic body to sleep at its current transform. Contact or an
    /// explicit impulse can still wake it.
    pub fn sleep_body(&mut self, handle: RigidBodyHandle) {
        if let Some(body) = self.rigid_body_set.get_mut(handle) {
            body.sleep();
        }
    }

    /// Raise a body's angular sleep threshold so residual solver noise (e.g. a
    /// ragdoll extremity buzzing against the floor) still counts as "at rest".
    /// One awake body keeps its whole jointed island awake, so without this a
    /// settled ragdoll never sleeps. Sleeping bodies auto-wake on contact or
    /// applied force, so the corpse stays interactive. (The linear threshold is
    /// left at rapier's default - the residual buzz is angular.)
    pub fn set_body_angular_sleep_threshold(&mut self, handle: RigidBodyHandle, angular: f32) {
        if let Some(body) = self.rigid_body_set.get_mut(handle) {
            body.activation_mut().angular_threshold = angular;
        }
    }

    /// Clamp ragdoll body velocities to finite, bounded magnitudes. The spawn
    /// transient of a ragdoll can (rarely, and nondeterministically across
    /// processes) enter a runaway feedback loop - a contact spike begets a
    /// bigger spike next frame until a position goes non-finite and the parry
    /// BVH broad-phase panics on the NaN AABB. Clamping once per frame breaks
    /// the cascade while leaving normal collapse dynamics (peaks well below
    /// the cap) untouched.
    ///
    /// Multibody-linked bodies clamp the owning multibody's generalized (DOF)
    /// velocities - writing a link body's linvel/angvel would be overwritten
    /// by the reduced-coordinate readback. Each multibody is clamped once no
    /// matter how many of its links appear in `handles`. Free bodies clamp
    /// linvel/angvel directly. Returns the number of corrected values.
    pub fn sanitize_ragdoll_bodies(&mut self, handles: &[RigidBodyHandle], max_speed: f32) -> u32 {
        let mut corrected = 0u32;
        let mut seen_multibodies = Vec::new();
        for handle in handles {
            if let Some(link) = self.multibody_joint_set.rigid_body_link(*handle) {
                let index = link.multibody;
                if seen_multibodies.contains(&index) {
                    continue;
                }
                seen_multibodies.push(index);
                if let Some(multibody) = self.multibody_joint_set.get_multibody_mut(index) {
                    for v in multibody.generalized_velocity_mut().iter_mut() {
                        if !v.is_finite() {
                            *v = 0.0;
                            corrected += 1;
                        } else if v.abs() > max_speed {
                            *v = v.clamp(-max_speed, max_speed);
                            corrected += 1;
                        }
                    }
                }
            } else if let Some(body) = self.rigid_body_set.get_mut(*handle) {
                let lin = *body.linvel();
                let ang = *body.angvel();
                if !lin.iter().all(|v| v.is_finite()) || lin.norm() > max_speed {
                    let new_lin = if lin.iter().all(|v| v.is_finite()) {
                        lin * (max_speed / lin.norm())
                    } else {
                        na::zero()
                    };
                    body.set_linvel(new_lin, false);
                    corrected += 1;
                }
                if !ang.iter().all(|v| v.is_finite()) || ang.norm() > max_speed {
                    let new_ang = if ang.iter().all(|v| v.is_finite()) {
                        ang * (max_speed / ang.norm())
                    } else {
                        na::zero()
                    };
                    body.set_angvel(new_ang, false);
                    corrected += 1;
                }
            }
        }
        corrected
    }

    /// Apply a world-space impulse to a dynamic body by its debug `body_id`
    /// (the rigid body handle index, as reported by [`debug_list_bodies`]),
    /// waking it even for a zero impulse (rapier skips a zero `apply_impulse`
    /// entirely, so the wake is explicit - `{"impulse":[0,0,0]}` is a pure
    /// wake). Debug/testing hook - e.g. poke a sleeping ragdoll to verify
    /// wake-on-impulse. Matches by handle index like the other debug-endpoint
    /// lookups (the generation isn't exposed over HTTP), so callers must use
    /// ids from a fresh body listing. Returns false if no dynamic body matches.
    pub fn apply_body_impulse(&mut self, body_id: u32, impulse: Vector3<f32>) -> bool {
        let handle = self
            .rigid_body_set
            .iter()
            .find(|(handle, _)| handle.into_raw_parts().0 == body_id)
            .map(|(handle, _)| handle);
        let Some(handle) = handle else {
            return false;
        };
        self.apply_impulse_to_handle(handle, impulse)
    }

    /// Seed a multibody's free-root velocity with a world-space linear
    /// velocity (rapier free-joint DOF layout: linear xyz at generalized
    /// indices 0..3, angular at 3..6 - see `MultibodyJoint::integrate`).
    /// Body-level `set_linvel` is clobbered by the reduced-coordinate
    /// readback, so inherited motion (e.g. a dying creature's root-motion
    /// velocity carrying into its ragdoll) must be written into the
    /// generalized coordinates. `body` may be any link of the multibody.
    /// Returns false for non-multibody bodies.
    pub fn set_multibody_root_linvel(
        &mut self,
        body: RigidBodyHandle,
        linvel: Vector3<f32>,
    ) -> bool {
        // Sanitize the seed: it gets a full physics step before the ragdoll's
        // per-frame velocity clamp sees it, so a non-finite or runaway value
        // (e.g. a capsule mid-knockback) must not reach the solver verbatim.
        const MAX_SEED_SPEED: f32 = 30.0;
        if !(linvel.x.is_finite() && linvel.y.is_finite() && linvel.z.is_finite()) {
            return false;
        }
        let norm = linvel.magnitude();
        let linvel = if norm > MAX_SEED_SPEED {
            linvel * (MAX_SEED_SPEED / norm)
        } else {
            linvel
        };
        let Some(link) = self.multibody_joint_set.rigid_body_link(body) else {
            // Not articulated (e.g. a one-bone skeleton spawns a lone free
            // body): a direct write works there.
            if let Some(rigid_body) = self.rigid_body_set.get_mut(body) {
                rigid_body.set_linvel(vec_to_nvec(linvel), true);
                return true;
            }
            return false;
        };
        let index = link.multibody;
        if let Some(multibody) = self.multibody_joint_set.get_multibody_mut(index) {
            let mut generalized = multibody.generalized_velocity_mut();
            if generalized.len() >= 3 {
                generalized[0] = linvel.x;
                generalized[1] = linvel.y;
                generalized[2] = linvel.z;
                return true;
            }
        }
        false
    }

    /// Multibody-aware impulse on a body handle, waking it even for a zero
    /// impulse. A multibody link ignores direct velocity writes: the reduced-
    /// coordinate solver recomputes every link body's velocity from the joint
    /// velocities each step (`Multibody` forward kinematics), so
    /// `apply_impulse` is silently overwritten. Its forward *dynamics* does
    /// read the per-body user-force accumulator, so convert the impulse to a
    /// force over one physics step - `clear_forces` (called right after each
    /// step) makes it impulsive. Free dynamic bodies get a plain impulse.
    pub fn apply_impulse_to_handle(
        &mut self,
        handle: RigidBodyHandle,
        impulse: Vector3<f32>,
    ) -> bool {
        let is_multibody_link = self.multibody_joint_set.rigid_body_link(handle).is_some();
        let dt = self.integration_parameters.dt;
        if let Some(body) = self.rigid_body_set.get_mut(handle) {
            if body.body_type() == RigidBodyType::Dynamic {
                body.wake_up(true);
                if is_multibody_link {
                    body.add_force(vec_to_nvec(impulse) / dt, true);
                    self.rigid_bodies_with_forces.push(handle);
                } else {
                    body.apply_impulse(vec_to_nvec(impulse), true);
                }
                return true;
            }
        }
        false
    }

    /// Create a static (fixed) rigid body without requiring an EntityId
    /// Returns the handle for use in ragdoll systems
    pub fn create_static_body(
        &mut self,
        isometry: Isometry<Real>,
        user_tag: Option<EntityId>,
    ) -> RigidBodyHandle {
        let mut rigid_body = RigidBodyBuilder::fixed().pose(isometry).build();

        if let Some(entity_id) = user_tag {
            rigid_body.user_data = entity_id.inner() as u128;
        }

        self.rigid_body_set.insert(rigid_body)
    }

    /// Attach a collider to an existing rigid body
    pub fn attach_collider(
        &mut self,
        handle: RigidBodyHandle,
        shape: SharedShape,
        density: f32,
        collision_group: CollisionGroup,
    ) {
        self.attach_collider_with_offset(
            handle,
            shape,
            Vector3::new(0.0, 0.0, 0.0),
            density,
            collision_group,
        );
    }

    /// Attach a collider to a body with a local-space translation offset. Used by
    /// the ragdoll rig so a limb's collider can sit at its hitbox center rather
    /// than at the joint origin.
    pub fn attach_collider_with_offset(
        &mut self,
        handle: RigidBodyHandle,
        shape: SharedShape,
        offset: Vector3<f32>,
        density: f32,
        collision_group: CollisionGroup,
    ) {
        let collider = ColliderBuilder::new(shape)
            .density(density)
            .translation(vec_to_nvec(offset))
            .collision_groups(collision_group.collision)
            .solver_groups(collision_group.solver)
            .active_events(ActiveEvents::COLLISION_EVENTS | ActiveEvents::CONTACT_FORCE_EVENTS)
            .build();

        self.collider_set
            .insert_with_parent(collider, handle, &mut self.rigid_body_set);
    }

    /// Create an impulse joint between two rigid bodies
    pub fn create_impulse_joint(
        &mut self,
        parent: RigidBodyHandle,
        child: RigidBodyHandle,
        joint_params: GenericJoint,
    ) -> ImpulseJointHandle {
        self.impulse_joint_set
            .insert(parent, child, joint_params, true)
    }

    /// Create a reduced-coordinate (multibody) joint between `parent` and `child`.
    /// Unlike an impulse joint, the constraint is structural - translation is not a
    /// DOF, so the bodies cannot separate, and no spring energy is injected. The
    /// child must currently be the root of its own multibody (each rigid body has at
    /// most one parent link); returns `None` if that invariant is violated (e.g. a
    /// duplicate edge / cycle). Removing either body (via `remove_rigid_body_handle`)
    /// also removes the joint.
    pub fn create_multibody_joint(
        &mut self,
        parent: RigidBodyHandle,
        child: RigidBodyHandle,
        joint_params: GenericJoint,
    ) -> Option<MultibodyJointHandle> {
        self.multibody_joint_set
            .insert(parent, child, joint_params, true)
    }

    /// Get the transform (position and rotation) of a rigid body by handle
    pub fn get_body_transform(&self, handle: RigidBodyHandle) -> Option<Isometry<Real>> {
        self.rigid_body_set.get(handle).map(|body| *body.position())
    }

    /// Linear and angular velocity of a rigid body by handle.
    pub fn body_velocities(&self, handle: RigidBodyHandle) -> Option<(Vector3<f32>, Vector3<f32>)> {
        self.rigid_body_set.get(handle).map(|body| {
            let l = body.linvel();
            let a = body.angvel();
            (Vector3::new(l.x, l.y, l.z), Vector3::new(a.x, a.y, a.z))
        })
    }

    /// World-space AABB enclosing all of a body's colliders (min, max).
    pub fn body_world_aabb(&self, handle: RigidBodyHandle) -> Option<(Vector3<f32>, Vector3<f32>)> {
        let body = self.rigid_body_set.get(handle)?;
        let mut min: Option<Vector3<f32>> = None;
        let mut max: Option<Vector3<f32>> = None;
        for collider_handle in body.colliders() {
            if let Some(collider) = self.collider_set.get(*collider_handle) {
                let aabb = collider.compute_aabb();
                let lo = Vector3::new(aabb.mins.x, aabb.mins.y, aabb.mins.z);
                let hi = Vector3::new(aabb.maxs.x, aabb.maxs.y, aabb.maxs.z);
                min = Some(match min {
                    Some(m) => Vector3::new(m.x.min(lo.x), m.y.min(lo.y), m.z.min(lo.z)),
                    None => lo,
                });
                max = Some(match max {
                    Some(m) => Vector3::new(m.x.max(hi.x), m.y.max(hi.y), m.z.max(hi.z)),
                    None => hi,
                });
            }
        }
        Some((min?, max?))
    }

    /// Scan every collider's world AABB for values that break physics queries:
    /// NaN/infinite bounds, degenerate (zero/negative) extents, or bounds far
    /// outside any plausible level. A single bad AABB can make raycasts return
    /// garbage or (in debug builds) panic inside parry3d, so this is the
    /// first-line check when a level misbehaves. Cheap enough to call on demand
    /// or per frame while diagnosing.
    pub fn audit_colliders(&self) -> Vec<ColliderIssue> {
        const EXTREME: f32 = 1.0e5;
        let mut issues = Vec::new();
        for (_handle, collider) in self.collider_set.iter() {
            let aabb = collider.compute_aabb();
            let mn = aabb.mins;
            let mx = aabb.maxs;
            let finite = mn.x.is_finite()
                && mn.y.is_finite()
                && mn.z.is_finite()
                && mx.x.is_finite()
                && mx.y.is_finite()
                && mx.z.is_finite();

            let kind = if !finite {
                Some(ColliderIssueKind::NonFinite)
            } else if mx.x <= mn.x || mx.y <= mn.y || mx.z <= mn.z {
                Some(ColliderIssueKind::Degenerate)
            } else if [mn.x, mn.y, mn.z, mx.x, mx.y, mx.z]
                .iter()
                .any(|c| c.abs() > EXTREME)
            {
                Some(ColliderIssueKind::Extreme)
            } else {
                None
            };

            if let Some(kind) = kind {
                let entity_id =
                    EntityId::from_inner(collider.user_data as u64).map(|id| id.inner() as i32);
                issues.push(ColliderIssue {
                    entity_id,
                    kind,
                    aabb_min: [mn.x, mn.y, mn.z],
                    aabb_max: [mx.x, mx.y, mx.z],
                    is_sensor: collider.is_sensor(),
                });
            }
        }
        issues
    }

    /// Enumerate every rigid body in the simulation for debug tooling.
    ///
    /// This iterates the raw Rapier `RigidBodySet` rather than the
    /// `entity_id_to_body` map, so it surfaces *all* bodies - including the
    /// many bodies that share a single `EntityId` (e.g. ragdoll limbs) and
    /// bodies with no entity at all.
    pub fn debug_list_bodies(&self) -> Vec<DebugBodyInfo> {
        self.rigid_body_set
            .iter()
            .map(|(handle, body)| self.debug_body_info(handle, body))
            .collect()
    }

    /// Look up a single body's debug info by its `body_id` (the rigid body
    /// handle index, as reported by [`debug_list_bodies`]).
    pub fn debug_body_detail(&self, body_id: u32) -> Option<DebugBodyInfo> {
        self.rigid_body_set
            .iter()
            .find(|(handle, _)| handle.into_raw_parts().0 == body_id)
            .map(|(handle, body)| self.debug_body_info(handle, body))
    }

    /// Enumerate every impulse joint with its anchor separation and applied
    /// impulse, for ragdoll diagnostics. A healthy ball joint at rest has
    /// `separation ≈ 0` and a small impulse; a persistent separation/impulse
    /// means the constraint can't be satisfied (the rig fights itself).
    pub fn debug_list_joints(&self) -> Vec<DebugJointInfo> {
        let impulse = self
            .impulse_joint_set
            .iter()
            .filter_map(|(_handle, joint)| {
                let b1 = self.rigid_body_set.get(joint.body1)?;
                let b2 = self.rigid_body_set.get(joint.body2)?;
                let a1 = b1.position() * joint.data.local_frame1;
                let a2 = b2.position() * joint.data.local_frame2;
                let separation = (a1.translation.vector - a2.translation.vector).norm();
                // impulses: first 3 components are linear (translation), last 3 angular.
                let imp = joint.impulses;
                let linear_impulse = (imp[0] * imp[0] + imp[1] * imp[1] + imp[2] * imp[2]).sqrt();
                let angular_impulse = if imp.len() >= 6 {
                    (imp[3] * imp[3] + imp[4] * imp[4] + imp[5] * imp[5]).sqrt()
                } else {
                    0.0
                };
                Some(DebugJointInfo {
                    body1_id: joint.body1.into_raw_parts().0,
                    body2_id: joint.body2.into_raw_parts().0,
                    joint_type: "impulse",
                    anchor1: [a1.translation.x, a1.translation.y, a1.translation.z],
                    anchor2: [a2.translation.x, a2.translation.y, a2.translation.z],
                    separation,
                    linear_impulse,
                    angular_impulse,
                })
            });
        let multibody = self
            .multibody_joint_anchor_pairs()
            .into_iter()
            .map(|(b1, b2, a1, a2)| DebugJointInfo {
                body1_id: b1.into_raw_parts().0,
                body2_id: b2.into_raw_parts().0,
                joint_type: "multibody",
                anchor1: [a1.translation.x, a1.translation.y, a1.translation.z],
                anchor2: [a2.translation.x, a2.translation.y, a2.translation.z],
                separation: (a1.translation.vector - a2.translation.vector).norm(),
                linear_impulse: 0.0,
                angular_impulse: 0.0,
            });
        impulse.chain(multibody).collect()
    }

    /// `(parent body, child body, world anchor on parent, world anchor on child)`
    /// for every multibody joint. Translation is structurally not a DOF for
    /// these, so the anchors should always coincide.
    fn multibody_joint_anchor_pairs(
        &self,
    ) -> Vec<(
        RigidBodyHandle,
        RigidBodyHandle,
        Isometry<Real>,
        Isometry<Real>,
    )> {
        self.multibody_joint_set
            .iter()
            .filter_map(|(_handle, _link_id, multibody, link)| {
                let parent = multibody.link(link.parent_id()?)?;
                let b1_handle = parent.rigid_body_handle();
                let b2_handle = link.rigid_body_handle();
                let b1 = self.rigid_body_set.get(b1_handle)?;
                let b2 = self.rigid_body_set.get(b2_handle)?;
                Some((
                    b1_handle,
                    b2_handle,
                    b1.position() * link.joint.data.local_frame1,
                    b2.position() * link.joint.data.local_frame2,
                ))
            })
            .collect()
    }

    fn debug_body_info(&self, handle: RigidBodyHandle, body: &RigidBody) -> DebugBodyInfo {
        let (index, generation) = handle.into_raw_parts();

        let entity_id = if body.user_data != 0 {
            Some(body.user_data as i32)
        } else {
            None
        };

        let body_type = match body.body_type() {
            RigidBodyType::Dynamic => "dynamic",
            RigidBodyType::Fixed => "static",
            RigidBodyType::KinematicPositionBased | RigidBodyType::KinematicVelocityBased => {
                "kinematic"
            }
        };

        let translation = body.translation();
        let rotation = body.rotation();
        let linvel = body.linvel();
        let angvel = body.angvel();
        let com = body.center_of_mass();

        // Pull shape/group/sensor data from the body's first collider, if any.
        let mut collision_groups = Vec::new();
        let mut is_sensor = false;
        if let Some(collider_handle) = body.colliders().first() {
            if let Some(collider) = self.collider_set.get(*collider_handle) {
                is_sensor = collider.is_sensor();
                collision_groups =
                    collision_group_names(collider.collision_groups().memberships.bits());
            }
        }

        DebugBodyInfo {
            body_id: index,
            generation,
            entity_id,
            body_type,
            position: [translation.x, translation.y, translation.z],
            rotation: [rotation.i, rotation.j, rotation.k, rotation.w],
            linear_velocity: [linvel.x, linvel.y, linvel.z],
            angular_velocity: [angvel.x, angvel.y, angvel.z],
            mass: body.mass(),
            center_of_mass: [com.x, com.y, com.z],
            gravity_scale: body.gravity_scale(),
            linear_damping: body.linear_damping(),
            angular_damping: body.angular_damping(),
            collision_groups,
            blocks_player: self.collider_blocks_player(handle),
            blocks_actor: self.collider_blocks_actor(handle),
            is_sensor,
            is_enabled: body.is_enabled(),
            is_sleeping: body.is_sleeping(),
        }
    }
}

/// Rapier-free description of a joint, for ragdoll diagnostics.
#[derive(Debug, Clone)]
pub struct DebugJointInfo {
    pub body1_id: u32,
    pub body2_id: u32,
    /// `"impulse"` or `"multibody"` - which joint set this came from.
    pub joint_type: &'static str,
    /// World anchor on each body (should coincide for a satisfied ball joint).
    pub anchor1: [f32; 3],
    pub anchor2: [f32; 3],
    /// Distance between the two anchors - the translation-constraint violation.
    /// Structurally ~0 for multibody joints (translation is not a DOF there);
    /// a persistent gap on a multibody joint means the frame setup is wrong.
    pub separation: f32,
    /// Magnitude of the linear (translation) constraint impulse this step.
    /// Rapier does not expose applied impulses for multibody links, so 0 there.
    pub linear_impulse: f32,
    /// Magnitude of the angular (limit) constraint impulse this step.
    pub angular_impulse: f32,
}

/// A malformed collider found by [`PhysicsWorld::audit_colliders`]. Bad
/// collider AABBs feed garbage into the physics queries (parry3d's ray-AABB
/// math can even integer-overflow on a degenerate box in debug builds), so
/// this is a data-hygiene check surfaced for any level on demand.
#[derive(Debug, Clone)]
pub struct ColliderIssue {
    /// Owning entity (from `Collider::user_data`), if any.
    pub entity_id: Option<i32>,
    /// What's wrong with the collider's world AABB.
    pub kind: ColliderIssueKind,
    /// The offending world-space AABB (min, max) - may contain NaN/inf.
    pub aabb_min: [f32; 3],
    pub aabb_max: [f32; 3],
    pub is_sensor: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColliderIssueKind {
    /// An AABB bound is NaN or infinite - poisons every query it touches.
    NonFinite,
    /// Zero (or negative) extent on some axis - a flat/degenerate box.
    Degenerate,
    /// Bounds far outside any plausible level extent (|coord| > 1e5) -
    /// usually an entity that fell out of the world or a bad transform.
    Extreme,
}

impl ColliderIssueKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ColliderIssueKind::NonFinite => "non_finite",
            ColliderIssueKind::Degenerate => "degenerate",
            ColliderIssueKind::Extreme => "extreme",
        }
    }
}

/// Rapier-free description of a rigid body, for debug tooling / HTTP introspection.
#[derive(Debug, Clone)]
pub struct DebugBodyInfo {
    /// Stable-within-session id: the rigid body handle's index.
    pub body_id: u32,
    /// Handle generation - distinguishes a reused index across removals.
    pub generation: u32,
    /// Owning entity (from `RigidBody::user_data`). Non-unique: many bodies
    /// (e.g. ragdoll limbs) can report the same entity.
    pub entity_id: Option<i32>,
    pub body_type: &'static str,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub linear_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub mass: f32,
    pub center_of_mass: [f32; 3],
    pub gravity_scale: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub collision_groups: Vec<String>,
    /// Whether this body stops the player capsule. `collision_groups` reports
    /// membership only, so a body that keeps its `entity` membership while
    /// dropping `PLAYER` from its filter looks identical there.
    pub blocks_player: bool,
    /// Whether this body stops a living creature capsule. Like
    /// `blocks_player`, this comes from the filter rather than membership.
    pub blocks_actor: bool,
    pub is_sensor: bool,
    pub is_enabled: bool,
    pub is_sleeping: bool,
}

/// Decode an `InteractionGroups` membership bitmask into human-readable names.
fn collision_group_names(bits: u32) -> Vec<String> {
    let mut names = Vec::new();
    let candidates = [
        (InternalCollisionGroups::WORLD, "world"),
        (InternalCollisionGroups::ENTITY, "entity"),
        (InternalCollisionGroups::SELECTABLE, "selectable"),
        (InternalCollisionGroups::PLAYER, "player"),
        (InternalCollisionGroups::UI, "ui"),
        (InternalCollisionGroups::HITBOX, "hitbox"),
        (InternalCollisionGroups::RAYCAST, "raycast"),
        (InternalCollisionGroups::CLIMBABLE, "climbable"),
        (InternalCollisionGroups::ACTOR, "actor"),
    ];
    for (group, name) in candidates {
        if bits & group.bits != 0 {
            names.push(name.to_string());
        }
    }
    names
}

#[cfg(test)]
mod held_melee_drive;

#[cfg(test)]
mod tests {
    //! Verify that per-object `DynamicPhysicsOptions` actually drive the Rapier
    //! simulation. These are deterministic, headless physics tests (fixed 1/60
    //! step, no mission/asset load) so an agent can confirm the plumbing.
    use super::*;
    use cgmath::{Quaternion, vec3};

    fn identity_quat() -> Quaternion<f32> {
        Quaternion::new(1.0, 0.0, 0.0, 0.0)
    }

    #[test]
    fn held_melee_is_swept_kinematic_contact_without_blocking_player() {
        let mut world = PhysicsWorld::new();
        let weapon = EntityId::from_inner(1).unwrap();
        world.add_dynamic(
            weapon,
            vec3(0.0, 0.0, 0.0),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(0.1, 0.5, 0.1)),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions::default(),
        );

        world.set_held_melee(weapon);

        let body = world
            .debug_list_bodies()
            .into_iter()
            .find(|body| body.entity_id == Some(weapon.inner() as i32))
            .unwrap();
        assert_eq!(
            body.body_type, "kinematic",
            "the held weapon must be kinematic, so no contact impulse can spin it out of the hand"
        );
        assert!(!body.blocks_player, "the weapon must ignore its owner");
        assert!(
            body.blocks_actor,
            "the swept weapon must retain actor contact detection"
        );
        assert_eq!(body.linear_velocity, [0.0; 3]);
        assert_eq!(body.angular_velocity, [0.0; 3]);

        let body_handle = world.entity_id_to_body[&weapon];
        let collider_handle = world.rigid_body_set[body_handle].colliders()[0];
        let collider = &world.collider_set[collider_handle];
        let actor = CollisionGroup::actor();
        assert!(
            collider.collision_groups().test(actor.collision),
            "held melee must still generate actor contacts"
        );
        assert!(
            !collider.solver_groups().test(actor.solver),
            "held melee must not solve impulses against living actors"
        );
    }

    /// In free space the weapon reaches the pose the hand asked for. An
    /// earlier revision drove this with a joint motor and asserted the
    /// opposite - that the weapon must *trail* its target - which is what a
    /// headset reported as heavy lag. Being stopped by geometry is a separate
    /// assertion; not being stopped by nothing is this one.
    #[test]
    fn held_melee_weapon_reaches_its_controller_target() {
        let (mut world, mut player) = world_with_floor();
        let weapon = EntityId::from_inner(2).unwrap();
        let handle = world.add_dynamic(
            weapon,
            vec3(-2.0, 1.0, 0.0),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(0.2, 0.4, 0.2)),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions::default(),
        );
        world.set_held_melee(weapon);
        world.set_position_rotation2(weapon, vec3(-2.0, 1.0, 0.0), identity_quat());
        step(&mut world, &mut player, 1);
        world.set_position_rotation2(weapon, vec3(0.0, 1.0, 0.0), identity_quat());

        step(&mut world, &mut player, 3);

        let weapon_x = world.get_position(handle).unwrap().x;
        assert!(
            (weapon_x - 0.0).abs() < 0.02,
            "the weapon should arrive on the tracked hand: x={weapon_x}"
        );
        let target = world.held_melee_drives[&handle].target;
        assert!((world.get_position(target).unwrap().x - 0.0).abs() < 1.0e-4);
    }

    /// A restored or newly wielded loose body may still carry a world pose
    /// unrelated to the tracked hand. Its first held pose establishes the
    /// physical-hand pair; only subsequent motion is spring-driven.
    #[test]
    fn held_melee_first_tracked_pose_seats_both_motor_endpoints() {
        let mut world = PhysicsWorld::new();
        let weapon = EntityId::from_inner(2).unwrap();
        let handle = world.add_dynamic(
            weapon,
            vec3(-40.0, 8.0, 20.0),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(0.2, 0.4, 0.2)),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions::default(),
        );
        world.set_held_melee(weapon);

        let tracked_pose = vec3(3.0, 1.0, -4.0);
        world.set_position_rotation2(weapon, tracked_pose, identity_quat());

        let drive = world.held_melee_drives[&handle];
        assert!(drive.seated);
        assert!((world.get_position(handle).unwrap() - tracked_pose).magnitude() < 1.0e-4);
        assert!((world.get_position(drive.target).unwrap() - tracked_pose).magnitude() < 1.0e-4);
        assert!(world.get_velocity(weapon).unwrap().magnitude() < 1.0e-4);
    }

    /// World solver contact must win over the hand motor, then releasing that
    /// obstruction must let the weapon return to the tracked pose.
    #[test]
    fn held_melee_weapon_stops_at_world_geometry_then_springs_back() {
        let (mut world, mut player) = world_with_floor();
        world.add_collider(
            EntityId::from_inner(2).unwrap(),
            ColliderBuilder::cuboid(0.05, 1.0, 1.0)
                .translation(vector![0.0, 1.0, 0.0])
                .build(),
        );
        let weapon = EntityId::from_inner(3).unwrap();
        let handle = world.add_dynamic(
            weapon,
            vec3(-1.0, 1.0, 0.0),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(0.4, 0.4, 0.4)),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions::default(),
        );
        world.set_held_melee(weapon);
        world.set_position_rotation2(weapon, vec3(-1.0, 1.0, 0.0), identity_quat());
        step(&mut world, &mut player, 1);
        world.set_position_rotation2(weapon, vec3(1.0, 1.0, 0.0), identity_quat());

        step(&mut world, &mut player, 120);

        let blocked_x = world.get_position(handle).unwrap().x;
        assert!(
            blocked_x < -0.20,
            "the dynamic weapon crossed the fixed wall instead of stopping: x={blocked_x}"
        );

        world.set_position_rotation2(weapon, vec3(-1.0, 1.0, 0.0), identity_quat());
        step(&mut world, &mut player, 60);
        let returned_x = world.get_position(handle).unwrap().x;
        assert!(
            returned_x < -0.8,
            "the weapon did not spring back after the target cleared the wall: x={returned_x}"
        );
    }

    /// A player teleport moves the tracked stage, not the hand relative to the
    /// player. Carry both motor endpoints by the same delta so the weapon does
    /// not attempt to traverse the intervening level geometry.
    #[test]
    fn player_relocation_carries_the_held_melee_body_and_target_together() {
        let (mut world, mut player) = world_with_floor();
        let weapon = EntityId::from_inner(2).unwrap();
        let weapon_handle = world.add_dynamic(
            weapon,
            vec3(-2.0, 1.0, 0.0),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(0.2, 0.4, 0.2)),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions::default(),
        );
        world.set_held_melee(weapon);
        let target_handle = world.held_melee_drives[&weapon_handle].target;
        let weapon_before = world.get_position(weapon_handle).unwrap();
        let target_before = world.get_position(target_handle).unwrap();
        let player_before = world.get_player_translation(&player);
        let delta = vec3(80.0, 4.0, -60.0);

        world.set_player_translation(player_before + delta, &mut player);

        assert!(
            (world.get_position(weapon_handle).unwrap() - weapon_before - delta).magnitude()
                < 1.0e-4
        );
        assert!(
            (world.get_position(target_handle).unwrap() - target_before - delta).magnitude()
                < 1.0e-4
        );
    }

    /// A held melee body begins life as the loose pickup's model-bounds box.
    /// Once the rendered first-person weapon has been posed, its fitted bounds
    /// must replace both that oversized shape and its pickup-space center.
    #[test]
    fn held_melee_cuboid_fits_the_rendered_weapon_bounds() {
        let mut world = PhysicsWorld::new();
        let weapon = EntityId::from_inner(2).unwrap();
        let handle = world.add_dynamic(
            weapon,
            vec3(0.0, 0.0, 0.0),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(1.83, 0.39, 0.20)),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions::default(),
        );
        world.set_held_melee(weapon);

        let rendered_size = vec3(0.24, 1.02, 0.18);
        let rendered_center = vec3(0.0, -0.51, 0.01);
        world.fit_held_melee_cuboid(weapon, rendered_size, rendered_center);

        assert_eq!(world.cuboid_full_size(handle), Some(rendered_size));
        let collider = &world.collider_set[world.rigid_body_set[handle].colliders()[0]];
        assert_eq!(
            collider.position_wrt_parent().unwrap().translation.vector,
            vec_to_nvec(rendered_center)
        );
        assert_eq!(
            world.rigid_body_set[handle].body_type(),
            RigidBodyType::KinematicPositionBased
        );
    }

    /// A controller pose can move a held weapon much farther than ordinary
    /// simulation motion in one frame. That contact must still report a
    /// `CollisionStarted` for trigger-gated damage, but it must not transfer
    /// the kinematic velocity into a living actor's dynamic capsule (#984).
    ///
    /// Negative-first: with collision groups also controlling the solver, the
    /// controller sweep below launches the actor at 18 world units/s.
    #[test]
    fn held_melee_contacts_live_actor_without_solver_launch() {
        let (mut world, mut player) = world_with_floor();
        let weapon = EntityId::from_inner(2).unwrap();
        let actor = EntityId::from_inner(3).unwrap();

        let actor_handle = world.add_dynamic(
            actor,
            vec3(0.0, 1.0, 0.0),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Capsule {
                height: 0.8,
                radius: 0.4,
            },
            CollisionGroup::actor(),
            false,
            DynamicPhysicsOptions {
                gravity_scale: 0.0,
                restitution: 0.0,
                friction: 0.0,
            },
        );
        world.set_enabled_rotations(actor, false, false, false);
        world.add_dynamic(
            weapon,
            vec3(-2.0, 1.0, 0.0),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(1.5, 0.3, 0.3)),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions {
                gravity_scale: 0.0,
                restitution: 0.0,
                friction: 0.0,
            },
        );
        world.set_held_melee(weapon);

        // Seat the just-held body at its first tracked pose, establish the
        // broad phase, then put only the hand target beyond the actor. The
        // visible weapon must cross through simulation, report contact, and
        // still leave actor solver response disabled.
        world.set_position_rotation2(weapon, vec3(-2.0, 1.0, 0.0), identity_quat());
        step(&mut world, &mut player, 1);
        world.set_position_rotation2(weapon, vec3(0.0, 1.0, 0.0), identity_quat());
        let mut events = Vec::new();
        for _ in 0..30 {
            let (_, mut frame_events) = world.update(vec3(0.0, 0.0, 0.0), &mut player);
            events.append(&mut frame_events);
        }

        assert!(
            events.iter().any(|event| matches!(
                event,
                CollisionEvent::CollisionStarted {
                    entity1_id,
                    entity2_id,
                    ..
                } if (*entity1_id == weapon && *entity2_id == actor)
                    || (*entity1_id == actor && *entity2_id == weapon)
            )),
            "held melee must keep reporting authentic actor contact"
        );
        let weapon_contact = events.iter().find_map(|event| match event {
            CollisionEvent::CollisionStarted {
                entity1_id,
                entity2_id,
                contact: Some(contact),
            } if *entity1_id == weapon && *entity2_id == actor => Some(*contact),
            CollisionEvent::CollisionStarted {
                entity1_id,
                entity2_id,
                contact: Some(contact),
            } if *entity1_id == actor && *entity2_id == weapon => Some(CollisionContact {
                point: contact.point,
                normal: -contact.normal,
            }),
            _ => None,
        });
        let weapon_contact = weapon_contact.expect("actor contact should carry its manifold");
        assert!(
            weapon_contact.point.x.is_finite()
                && weapon_contact.point.y.is_finite()
                && weapon_contact.point.z.is_finite(),
            "contact point should be finite: {weapon_contact:?}"
        );
        assert!(
            (weapon_contact.normal.magnitude() - 1.0).abs() < 1.0e-4
                && weapon_contact.normal.x > 0.5,
            "contact normal should point from the left-side weapon toward the actor: {weapon_contact:?}"
        );
        let actor_velocity = world.get_velocity(actor).unwrap();
        let actor_position = world.get_position(actor_handle).unwrap();
        assert!(
            actor_velocity.magnitude() < 1.0 && actor_position.x.abs() < 0.1,
            "held melee solver launched the actor to {actor_position:?} at {actor_velocity:?}"
        );
    }

    /// Held-only solver filtering must not leak into the ordinary loose-prop
    /// body recreated by DropItem. Store/destroy removes the body entirely;
    /// a later world instantiation must likewise start with authored solid
    /// groups instead of inheriting the old held collider's filter.
    #[test]
    fn recreated_melee_body_restores_ordinary_actor_solver_contact() {
        let mut world = PhysicsWorld::new();
        let weapon = EntityId::from_inner(4).unwrap();
        let actor = CollisionGroup::actor();
        world.add_dynamic(
            weapon,
            vec3(0.0, 1.0, 0.0),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(1.5, 0.3, 0.3)),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions::default(),
        );
        world.set_held_melee(weapon);
        let held_handle = world.entity_id_to_body[&weapon];
        let target_handle = world.held_melee_drives[&held_handle].target;
        let held_collider = &world.collider_set[world.rigid_body_set[held_handle].colliders()[0]];
        assert!(!held_collider.solver_groups().test(actor.solver));

        world.remove(weapon);
        assert!(
            !world.entity_id_to_body.contains_key(&weapon),
            "store/destroy must remove the held physics body"
        );
        assert!(
            world.rigid_body_set.get(target_handle).is_none(),
            "store/destroy must remove the invisible hand target too"
        );

        let loose_handle = world.add_dynamic(
            weapon,
            vec3(0.0, 1.0, 0.0),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(1.5, 0.3, 0.3)),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions::default(),
        );
        let loose_collider = &world.collider_set[world.rigid_body_set[loose_handle].colliders()[0]];
        assert!(
            loose_collider.solver_groups().test(actor.solver),
            "a dropped/recreated loose prop must restore ordinary actor response"
        );
    }

    /// A collider deliberately made non-solid to characters must not remain an
    /// immovable obstacle to the live creature that follows the player through
    /// it, while ordinary physical objects and interaction rays still hit it.
    ///
    /// Negative-first: before the actor group split, live creatures shared
    /// `ENTITY` with ordinary props, so #805/#810's player-only filter still
    /// collided here.
    #[test]
    fn non_solid_character_obstacles_do_not_block_live_creatures() {
        let obstacle = CollisionGroup::entity().non_solid_to_characters();
        let live_creature = CollisionGroup::actor();
        let physical_entity = CollisionGroup::entity();
        let generic_entity_ray = InteractionGroups::new(
            InternalCollisionGroups::ALL.bits.into(),
            InternalCollisionGroups::ENTITIES.bits.into(),
            Default::default(),
        );

        assert!(
            !obstacle.collision.test(live_creature.collision),
            "a player-passable collider must not stop a pursuing creature"
        );
        assert!(
            physical_entity.collision.test(live_creature.collision),
            "ordinary props must remain solid to creatures"
        );
        assert!(
            obstacle.collision.test(physical_entity.collision),
            "slow physical projectiles and movable props must still hit the obstacle"
        );
        assert!(
            obstacle.collision.test(generic_entity_ray),
            "selection/projectile rays must still hit the obstacle"
        );
    }

    /// A pair of lightweight live actors can settle against opposite sides of
    /// the kinematic player capsule. Walking must transfer enough motion to
    /// the dynamic actors to open an escape route instead of leaving every
    /// character-controller cast at zero progress (#819).
    ///
    /// Negative-first: without character collision impulses, both actor
    /// capsules remain fixed against the player and the requested walk makes
    /// only 0.014 world units of progress across the full second.
    #[test]
    fn player_pushes_out_between_live_creatures() {
        let (mut world, mut player) = world_with_floor();
        world.set_player_translation(vec3(0.0, 1.2, 0.0), &mut player);

        let actors = [1010, 1011].map(|entity| EntityId::from_inner(entity).unwrap());
        for (entity_id, x, z) in [(actors[0], 0.362, -1.016), (actors[1], 0.082, 1.076)] {
            world.add_dynamic(
                entity_id,
                vec3(x, 0.84, z),
                identity_quat(),
                vec3(0.0, 0.0, 0.0),
                PhysicsShape::Capsule {
                    height: 0.6,
                    radius: 0.6,
                },
                CollisionGroup::actor(),
                false,
                DynamicPhysicsOptions {
                    gravity_scale: 0.0,
                    restitution: 0.0,
                    friction: 0.0,
                },
            );
            world.set_enabled_rotations(entity_id, false, false, false);
        }

        let start = world.get_player_translation(&player);
        for _ in 0..60 {
            world.update(vec3(1.0 / 6.0, 0.0, 0.0), &mut player);
            // Mission animation publishes root motion after physics. Mimic
            // that overwrite and prove the player shove is carried into it.
            for actor in actors {
                let push = world.take_player_push_velocity(actor);
                let vertical = world.get_velocity(actor).unwrap().y;
                world.set_velocity(actor, vec3(push.x, vertical, push.z));
            }
        }
        let end = world.get_player_translation(&player);

        assert!(
            end.x - start.x > 1.0,
            "player remained pinned between live creatures: {start:?} -> {end:?}"
        );
    }

    #[test]
    fn corpse_body_stays_grounded_and_selectable_without_blocking_characters() {
        let mut world = PhysicsWorld::new();
        let corpse_id = EntityId::from_inner(1).unwrap();
        let handle = world.add_dynamic(
            corpse_id,
            vec3(0.0, 1.0, 0.0),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Capsule {
                height: 1.0,
                radius: 0.5,
            },
            CollisionGroup::actor(),
            false,
            DynamicPhysicsOptions::default(),
        );

        world.set_collision_group(corpse_id, CollisionGroup::corpse());

        let collider = &world.collider_set[world.rigid_body_set[handle].colliders()[0]];
        let corpse = collider.collision_groups();
        let player = InteractionGroups::new(
            InternalCollisionGroups::PLAYER.bits.into(),
            InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            Default::default(),
        );
        let live_creature = CollisionGroup::actor().collision;
        let terrain = InteractionGroups::new(
            InternalCollisionGroups::WORLD.bits.into(),
            InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            Default::default(),
        );
        let selectable_query = InteractionGroups::new(
            InternalCollisionGroups::ALL.bits.into(),
            InternalCollisionGroups::SELECTABLE.bits.into(),
            Default::default(),
        );

        assert!(
            !corpse.test(player),
            "corpse capsule must not block the player"
        );
        assert!(
            !corpse.test(live_creature),
            "corpse capsule must not block living creatures"
        );
        assert!(
            corpse.test(terrain),
            "corpse must remain supported by terrain"
        );
        assert!(
            corpse.test(selectable_query),
            "corpse must remain selectable for looting"
        );
        assert!(!collider.is_sensor(), "world support uses a solid collider");
    }

    #[test]
    fn interaction_only_bounds_are_raycastable_without_physical_contacts() {
        let bounds = CollisionGroup::selectable().interaction_only();
        let generic_ray = InteractionGroups::new(
            InternalCollisionGroups::ALL.bits.into(),
            InternalCollisionGroups::SELECTABLE.bits.into(),
            Default::default(),
        );

        assert!(bounds.collision.test(generic_ray));
        assert!(!bounds.collision.test(CollisionGroup::entity().collision));
        assert!(!bounds.collision.test(CollisionGroup::actor().collision));
        assert!(!bounds.collision.test(InteractionGroups::new(
            InternalCollisionGroups::PLAYER.bits.into(),
            InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            Default::default(),
        )));
    }

    /// AI movement probes must use the actor membership, not a generic ray:
    /// the latter intentionally keeps seeing interaction-only geometry so
    /// frobbing, shooting, and line-of-sight behavior do not change.
    #[test]
    fn actor_movement_ray_ignores_character_passable_obstacles() {
        let mut world = PhysicsWorld::new();
        world.add_kinematic(
            EntityId::from_inner(42).unwrap(),
            vec3(0.0, 2.0, 0.0),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            vec3(2.0, 2.0, 2.0),
            CollisionGroup::entity().non_solid_to_characters(),
            false,
        );
        let mut player =
            world.create_player(vec3(100.0, 100.0, 100.0), EntityId::from_inner(43).unwrap());
        world.update(vec3(0.0, 0.0, 0.0), &mut player);

        let origin = point3(-3.0, 2.0, 0.0);
        let direction = Vector3::unit_x();
        let generic_hit = world.ray_cast2(
            origin,
            direction,
            10.0,
            InternalCollisionGroups::ENTITIES,
            None,
            true,
        );
        let actor_hit = world.ray_cast2_as_actor(
            origin,
            direction,
            10.0,
            InternalCollisionGroups::ENTITIES,
            None,
            true,
        );

        assert!(
            generic_hit.is_some(),
            "generic interaction rays must still hit the obstacle"
        );
        assert!(
            actor_hit.is_none(),
            "AI movement probes must pass through the obstacle"
        );
    }

    /// A world with a large static floor whose top surface is at `y = 0`, plus a
    /// throwaway player far away (so it never interacts with the test bodies but
    /// satisfies `update`'s signature).
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

    fn step(world: &mut PhysicsWorld, player: &mut PlayerHandle, frames: usize) {
        for _ in 0..frames {
            world.update(Vector3::new(0.0, 0.0, 0.0), player);
        }
    }

    fn add_ramp(world: &mut PhysicsWorld, start: (f32, f32), end: (f32, f32), half_width: f32) {
        let (x0, y0) = start;
        let (x1, y1) = end;
        let vertices = vec![
            point![x0, y0, -half_width],
            point![x0, y0, half_width],
            point![x1, y1, -half_width],
            point![x1, y1, half_width],
        ];
        let indices = vec![[0, 3, 2], [0, 1, 3]];
        let mut collider = ColliderBuilder::trimesh(vertices, indices)
            .expect("valid ramp mesh")
            .build();
        collider.set_collision_groups(InteractionGroups {
            memberships: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            filter: InternalCollisionGroups::ALL_COLLIDABLE.bits.into(),
            test_mode: Default::default(),
        });
        world.collider_set.insert(collider);
    }

    /// A room's level geometry: a floor quad at `floor_y` and a ceiling quad at
    /// `ceiling_y`, in ONE `ALL_COLLIDABLE` trimesh, exactly as
    /// [`PhysicsWorld::add_level_geometry`] builds `WorldRep`.
    fn add_level_room(world: &mut PhysicsWorld, floor_y: f32, ceiling_y: f32, half: f32) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for y in [floor_y, ceiling_y] {
            let base = vertices.len() as u32;
            vertices.push(point![-half, y, -half]);
            vertices.push(point![-half, y, half]);
            vertices.push(point![half, y, -half]);
            vertices.push(point![half, y, half]);
            indices.push([base, base + 1, base + 3]);
            indices.push([base, base + 3, base + 2]);
        }
        let collider = ColliderBuilder::trimesh(vertices, indices)
            .expect("valid room mesh")
            .build();
        world.add_collider(EntityId::from_inner(2199).unwrap(), collider);
    }

    /// The standing eye is the head sphere center plus the original game's eye
    /// offset, which stays inside the collider by construction - a camera above
    /// the crown would be outside every room whose ceiling the body clears.
    #[test]
    fn standing_eye_is_the_head_sphere_plus_eye_offset_inside_the_collider() {
        let eye = crate::player_eye_height_for(false);
        assert_eq!(
            eye,
            PLAYER_HEAD_POS + PLAYER_EYE_OFFSET,
            "the standing eye must be the head sphere plus the eye offset"
        );
        let crown = PLAYER_STANDING_HEIGHT / 2.0;
        assert!(
            eye < crown,
            "the standing eye ({eye} ft) must sit below the collider crown ({crown} ft)"
        );
    }

    /// The crosshair ray must reach a frobbable object in a room the player can
    /// stand in. hydro2's Hydro Card B corpse (#795) lies under a seven-foot
    /// ceiling: the six-foot body clears it, but an eye above the crown put the
    /// ray origin *above* the ceiling, so every aim point hit that ceiling from
    /// outside the room instead of the container - no highlight, no frob.
    ///
    /// Negative-first: with the previous `PLAYER_EYE_HEIGHT` of 4.0 ft this
    /// resolves to the one-sided ceiling rather than the container.
    #[test]
    fn standing_crosshair_reaches_a_container_under_a_seven_foot_ceiling() {
        // hydro2's corpse alcove: a flat floor with a one-sided level ceiling
        // seven feet above it, which the six-foot standing body clears.
        const ROOM_HEIGHT: f32 = 7.0 / SCALE_FACTOR;
        let mut world = PhysicsWorld::new();
        add_level_room(&mut world, 0.0, ROOM_HEIGHT, 20.0);

        // A frobbable container resting on the floor, two units ahead.
        let container = EntityId::from_inner(2201).unwrap();
        let container_pos = vec3(2.0, 0.2, 0.0);
        world.add_kinematic(
            container,
            container_pos,
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(0.8, 0.4, 0.8),
            CollisionGroup::selectable(),
            false,
        );

        let mut player =
            world.create_player(vec3(0.0, 2.0, 0.0), EntityId::from_inner(2200).unwrap());
        step(&mut world, &mut player, 120);
        let body = world.get_player_translation(&player);
        assert!(
            (body.y - PLAYER_STANDING_HEIGHT / 2.0 / SCALE_FACTOR).abs() < 0.2,
            "the standing player should fit in a seven-foot room; got {body:?}"
        );

        // Fixture precondition: this room must actually be one the old 4.0 ft
        // eye escaped, or the test would pass on the buggy build too.
        assert!(
            body.y + 4.0 / SCALE_FACTOR > ROOM_HEIGHT,
            "the pre-fix eye must be above this ceiling for the repro to bite"
        );

        let eye = point3(
            body.x,
            body.y + crate::player_eye_height_for(false) / SCALE_FACTOR,
            body.z,
        );
        // The exact mask the flat controller's crosshair raycast uses.
        let direction =
            (point3(container_pos.x, container_pos.y, container_pos.z) - eye).normalize();
        let hit = world
            .ray_cast(
                eye,
                direction,
                InternalCollisionGroups::ENTITY
                    | InternalCollisionGroups::SELECTABLE
                    | InternalCollisionGroups::WORLD
                    | InternalCollisionGroups::UI
                    | InternalCollisionGroups::RAYCAST,
            )
            .expect("the crosshair ray should hit something");
        assert_eq!(
            hit.maybe_entity_id,
            Some(container),
            "the crosshair should select the container, not the ceiling; hit {:?} at {:?}",
            hit.maybe_entity_id,
            hit.hit_point
        );
    }

    /// Falling onto a steep face carries the resulting downslope momentum
    /// across a shallower face instead of treating the seam as a fresh rest.
    ///
    /// These are the two authored angles at the start of shodan.mis: 56.31°
    /// slides under the controller's default 45° threshold, then hands off to
    /// a 38.66° face. Negative-first: without retained slope momentum the
    /// pure vertical gravity pass stops at that seam.
    #[test]
    fn fall_momentum_crosses_a_steep_to_shallow_ramp_seam() {
        let mut world = PhysicsWorld::new();
        // tan(56.31°) = 1.5; tan(38.66°) = 0.8.
        add_ramp(&mut world, (-2.0, 7.0), (2.0, 1.0), 3.0);
        add_ramp(&mut world, (2.0, 1.0), (7.0, -3.0), 3.0);
        add_ramp(&mut world, (7.0, -3.0), (30.0, -3.0), 3.0);
        let mut player =
            world.create_player(vec3(0.0, 9.0, 0.0), EntityId::from_inner(2100).unwrap());

        step(&mut world, &mut player, 180);
        let end = world.get_player_translation(&player);
        assert!(
            end.x > 2.5,
            "fall momentum should carry the player across the shallow handoff; got {end:?}"
        );

        // Rapier also marks its permissive slope-handler branch as "sliding"
        // when carried horizontal input meets a flat floor. That flag alone
        // would re-inject the displacement forever, so verify the player
        // settles once the chained slopes hand off to the flat landing.
        step(&mut world, &mut player, 240);
        let settled = world.get_player_translation(&player);
        step(&mut world, &mut player, 120);
        let after = world.get_player_translation(&player);
        let drift = after - settled;
        assert!(
            drift.x.abs() < 0.05 && drift.y.abs() < 0.05 && drift.z.abs() < 0.05,
            "slope displacement must settle on flat support; moved from {settled:?} to {after:?}"
        );
        assert!(
            player.slope_displacement.norm_squared() <= PLAYER_SLOPE_DISPLACEMENT_EPSILON_SQUARED,
            "flat support must clear retained displacement; got {:?}",
            player.slope_displacement
        );
    }

    /// A moderate ramp remains ordinary walkable ground when the player did
    /// not arrive with downslope momentum.
    #[test]
    fn player_at_rest_does_not_slide_down_a_moderate_ramp() {
        let mut world = PhysicsWorld::new();
        add_ramp(&mut world, (0.0, 4.0), (5.0, 0.0), 3.0);
        let start_x = 2.5;
        let surface_y = 2.0;
        let mut player = world.create_player(
            vec3(start_x, surface_y + PLAYER_HALF_HEIGHT + 0.2, 0.0),
            EntityId::from_inner(2101).unwrap(),
        );

        step(&mut world, &mut player, 120);
        let end = world.get_player_translation(&player);
        assert!(
            (end.x - start_x).abs() < 0.05,
            "a player placed at rest on a 38.66° ramp should not drift; got {end:?}"
        );
    }

    /// The ground-normal probe must reach the plane below the rounded side of
    /// both player capsules near the steepest angle accepted as a floor.
    #[test]
    fn standing_and_crouched_falls_retain_momentum_from_a_near_threshold_slope() {
        let run = |crouched: bool| {
            let mut world = PhysicsWorld::new();
            // tan(75°) = 3.732; its upward normal is 0.259, just above the
            // 0.25 floor threshold. It hands off to the same 38.66° moderate
            // face used by the Shodan regression, then a flat landing.
            add_ramp(&mut world, (-1.0, 7.464), (1.0, 0.0), 3.0);
            add_ramp(&mut world, (1.0, 0.0), (6.0, -4.0), 3.0);
            add_ramp(&mut world, (6.0, -4.0), (30.0, -4.0), 3.0);
            // Drop over the interior of the finite face. The restored
            // 2.4-foot standing width reaches around the old x=-0.75 start
            // and contacts the ramp's upper endpoint, whose edge normal does
            // not exercise the planar near-threshold ground probe this
            // regression is about.
            let mut player = world.create_player(
                vec3(0.0, 10.0, 0.0),
                EntityId::from_inner(if crouched { 2104 } else { 2105 }).unwrap(),
            );
            if crouched {
                assert!(world.set_player_crouch(true, &mut player));
            }

            step(&mut world, &mut player, 240);
            world.get_player_translation(&player)
        };

        for (label, end) in [("standing", run(false)), ("crouched", run(true))] {
            assert!(
                end.x > 2.0,
                "{label} capsule should carry the near-threshold slide across the shallow handoff; got {end:?}"
            );
        }
    }

    #[test]
    fn player_save_waits_for_transient_slope_displacement() {
        let (mut world, mut player) = world_with_floor();
        world.set_player_translation(vec3(0.0, 2.0, 0.0), &mut player);
        step(&mut world, &mut player, 30);
        assert!(
            world.get_player_save_translation(&player).is_ok(),
            "a stationary player should be saveable"
        );

        player.slope_displacement = vector![0.1, 0.0, 0.0];
        assert_eq!(
            world.get_player_save_translation(&player),
            Err(PlayerSavePoseError::TransientSlopeMotion),
            "a transform-only save must not erase active slope carry"
        );

        let current = world.get_player_translation(&player);
        world.set_player_translation(current + vec3(1.0, 0.0, 0.0), &mut player);
        step(&mut world, &mut player, 2);
        assert!(
            world.get_player_save_translation(&player).is_ok(),
            "an explicit relocation clears the transient carry"
        );
    }

    #[test]
    fn player_save_rejects_an_unsupported_void_pose() {
        let mut world = PhysicsWorld::new();
        let mut player =
            world.create_player(vec3(0.0, -100.0, 0.0), EntityId::from_inner(2106).unwrap());

        step(&mut world, &mut player, 3);

        assert_eq!(
            world.get_player_save_translation(&player),
            Err(PlayerSavePoseError::UnsupportedPose),
            "a falling player with no walkable support must not brick a save slot"
        );
    }

    #[test]
    fn reversed_gravity_clears_downhill_displacement_without_carrying_it() {
        let mut world = PhysicsWorld::new();
        let mut player =
            world.create_player(vec3(0.0, 3.0, 0.0), EntityId::from_inner(2103).unwrap());
        world.rigid_body_set[player.character_handle].set_gravity_scale(-1.0, true);
        player.slope_displacement = vector![0.1, 0.0, 0.0];
        let start = world.get_player_translation(&player);

        step(&mut world, &mut player, 2);
        let end = world.get_player_translation(&player);
        assert!(
            (end.x - start.x).abs() < 1.0e-4 && (end.z - start.z).abs() < 1.0e-4,
            "upward gravity must not inherit downhill displacement: {start:?} -> {end:?}"
        );
        assert_eq!(
            player.slope_displacement,
            Vector::zeros(),
            "upward gravity must clear downhill carry immediately"
        );
    }

    fn walk_player_toward(
        world: &mut PhysicsWorld,
        player: &mut PlayerHandle,
        target: Vector3<f32>,
        max_frames: usize,
    ) {
        for _ in 0..max_frames {
            let current = world.get_player_translation(player);
            let delta = vec3(target.x - current.x, 0.0, target.z - current.z);
            if delta.magnitude() < 0.05 {
                break;
            }
            world.update(delta.normalize() * (25.0 / 60.0 / SCALE_FACTOR), player);
        }
    }

    fn player_capsule_dimensions_ss2(world: &PhysicsWorld, player: &PlayerHandle) -> (f32, f32) {
        let body = &world.rigid_body_set[player.character_handle];
        let collider = &world.collider_set[body.colliders()[0]];
        let capsule = collider
            .shape()
            .as_capsule()
            .expect("player collider should be a capsule");
        (
            2.0 * (capsule.half_height() + capsule.radius) * SCALE_FACTOR,
            2.0 * capsule.radius * SCALE_FACTOR,
        )
    }

    fn assert_player_capsule_dimensions(
        world: &PhysicsWorld,
        player: &PlayerHandle,
        expected_height: f32,
        expected_width: f32,
    ) {
        let (height, width) = player_capsule_dimensions_ss2(world, player);
        assert!(
            (height - expected_height).abs() < 1.0e-5 && (width - expected_width).abs() < 1.0e-5,
            "expected {expected_height} x {expected_width} SS2 feet, got {height} x {width}"
        );
    }

    /// Standing restores Dark's original 6 x 2.4-foot body, while crouch and
    /// direct relocation retain the 2.8 x 1.6-foot profile added in #513.
    #[test]
    fn player_capsule_uses_original_standing_and_existing_crouched_footprints() {
        let mut world = PhysicsWorld::new();
        let mut player = world.create_player(vec3(0.0, 0.0, 0.0), EntityId::from_inner(1).unwrap());

        assert_player_capsule_dimensions(&world, &player, 6.0, 2.4);
        assert!(
            (player_crouch_center_shift() - 0.64).abs() < 1.0e-6,
            "save/load normalization must use the new feet-planted center shift"
        );

        assert!(world.set_player_crouch(true, &mut player));
        assert_player_capsule_dimensions(&world, &player, 2.8, 1.6);
        world.set_player_translation(vec3(1.0, 2.0, 3.0), &mut player);
        assert_player_capsule_dimensions(&world, &player, 2.8, 1.6);

        // Standing up runs a headroom query, which only has an answer once the
        // pipeline has stepped and built its broad phase.
        step(&mut world, &mut player, 1);
        assert!(!world.set_player_crouch(false, &mut player));
        assert_player_capsule_dimensions(&world, &player, 6.0, 2.4);
    }

    /// The scene-wide query pipeline the climb/top-out helpers take.
    fn query_pipeline<'a>(world: &'a PhysicsWorld, filter: QueryFilter<'a>) -> QueryPipeline<'a> {
        world.broad_phase.as_query_pipeline(
            world.narrow_phase.query_dispatcher(),
            &world.rigid_body_set,
            &world.collider_set,
            filter,
        )
    }

    /// A point probe can see a narrow tread while the crouched capsule
    /// endpoint chosen beside it is entirely unsupported. Non-overlap alone
    /// accepted that endpoint and let the expanded body fall after top-out.
    #[test]
    fn jump_drop_support_rejects_pose_beside_finite_tread() {
        let mut world = PhysicsWorld::new();
        world.add_collider(
            EntityId::from_inner(1).unwrap(),
            ColliderBuilder::cuboid(0.05, 0.05, 0.05)
                .translation(vector![0.0, -0.05, 0.0])
                .build(),
        );
        world.add_collider(
            EntityId::from_inner(2).unwrap(),
            ColliderBuilder::cuboid(1.0, 0.05, 1.0)
                .translation(vector![2.0, -0.05, 0.0])
                .build(),
        );
        let mut player =
            world.create_player(vec3(100.0, 100.0, 100.0), EntityId::from_inner(3).unwrap());
        world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);

        let queries = query_pipeline(&world, QueryFilter::default());
        let shape = crouched_player_capsule();
        let center_y = PLAYER_CROUCH_HEIGHT / 2.0 / SCALE_FACTOR
            + (PLAYER_CONTACT_OFFSET + PLAYER_REST_LIFT) / SCALE_FACTOR;
        let unsupported = vector![0.5, center_y, 0.0];
        let supported = vector![2.0, center_y, 0.0];
        let controller = player_character_controller();

        assert!(
            queries
                .cast_ray(
                    &Ray::new(vector![0.0, center_y, 0.0].into(), -Vector::y()),
                    PLAYER_STEP_HEIGHT / SCALE_FACTOR,
                    true,
                )
                .is_some(),
            "the point probe must find the nearby finite tread"
        );
        assert!(
            !shape_intersects(&queries, unsupported, &shape),
            "the old non-overlap predicate must accept this unsupported endpoint"
        );
        assert!(
            !shape_has_stable_support(&controller, &queries, &shape, unsupported, 1.0 / 60.0),
            "an overlap-free pose beside the tread must not count as a landing"
        );
        assert!(
            shape_has_stable_support(&controller, &queries, &shape, supported, 1.0 / 60.0),
            "a crouched capsule centered over a broad tread must remain a valid landing"
        );
    }

    /// Imported 45-degree treads carry tiny normal error around pi/4. The
    /// player's walkable-normal margin must keep those authored surfaces from
    /// turning into an automatic slide after a validated jump landing.
    #[test]
    fn jump_drop_support_accepts_authored_45_degree_tread() {
        let mut world = PhysicsWorld::new();
        let angle = std::f32::consts::FRAC_PI_4;
        world.add_collider(
            EntityId::from_inner(1).unwrap(),
            ColliderBuilder::cuboid(2.0, 0.05, 1.0)
                .rotation(vector![0.0, 0.0, angle])
                .build(),
        );
        let mut player =
            world.create_player(vec3(100.0, 100.0, 100.0), EntityId::from_inner(2).unwrap());
        world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);

        let queries = query_pipeline(&world, QueryFilter::default());
        let shape = crouched_player_capsule();
        let normal = vector![-angle.sin(), angle.cos(), 0.0];
        let floor_point = normal * 0.05;
        let segment_half = (PLAYER_CROUCH_HEIGHT / 2.0 - PLAYER_CROUCH_RADIUS) / SCALE_FACTOR;
        let standing = floor_point
            + normal * ((PLAYER_CROUCH_RADIUS + PLAYER_CONTACT_OFFSET) / SCALE_FACTOR)
            + Vector::y() * (segment_half + PLAYER_REST_LIFT / SCALE_FACTOR);

        assert!(
            !shape_intersects(&queries, standing, &shape),
            "the capsule should begin at a collision-valid rest offset"
        );
        assert!(
            shape_has_stable_support(
                &player_character_controller(),
                &queries,
                &shape,
                standing,
                1.0 / 60.0,
            ),
            "an authored 45-degree tread must remain stable through the landing preflight"
        );
    }

    /// Plan the standard rejection-test mantle: from the origin, facing +X,
    /// with the scripted casts seeing only parented (entity) colliders.
    fn plan_top_out_from_origin(world: &PhysicsWorld) -> Option<PlayerMovement> {
        let validation_queries = query_pipeline(world, QueryFilter::default());
        let parented_only = QueryFilter::default()
            .predicate(&|_handle: ColliderHandle, collider: &Collider| collider.parent().is_some());
        let scripted_queries = validation_queries.with_filter(parented_only);
        plan_climb_top_out(
            &KinematicCharacterController::default(),
            &validation_queries,
            &validation_queries,
            &scripted_queries,
            &Isometry::translation(0.0, 0.0, 0.0),
            Vector::x(),
            CLIMB_TOP_OUT_PROBE_FORWARD,
            1.0 / 60.0,
        )
    }

    /// Half the player capsule's height in world units - the body translation
    /// sits this far above the surface the player stands on.
    const PLAYER_HALF_HEIGHT: f32 = PLAYER_STANDING_HEIGHT / 2.0 / SCALE_FACTOR;

    /// One frame of the tram's authored pace: `command1`'s car covers ~12.2
    /// world units per second, i.e. ~0.2 per 60 Hz frame.
    const PLATFORM_STEP: f32 = 0.2;

    /// A world containing a single kinematic platform whose top surface is at
    /// `y = 0`, with the player standing (settled) on it. This is exactly what
    /// `entity_creator` builds for moving terrain: a kinematic position-based
    /// body with a cuboid collider, driven by `set_translation`.
    fn world_with_kinematic_platform() -> (PhysicsWorld, PlayerHandle, RigidBodyHandle) {
        let mut world = PhysicsWorld::new();
        let platform = world.add_kinematic(
            EntityId::from_inner(2000).unwrap(),
            vec3(0.0, -0.5, 0.0),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            vec3(20.0, 1.0, 20.0),
            CollisionGroup::entity(),
            false,
        );
        let mut player = world.create_player(
            vec3(0.0, PLAYER_HALF_HEIGHT + 0.1, 0.0),
            EntityId::from_inner(2001).unwrap(),
        );
        // Settle onto the deck before anything moves.
        step(&mut world, &mut player, 30);
        (world, player, platform)
    }

    /// A player standing on horizontally moving terrain rides it *exactly*.
    /// The controller's own contact-based transfer leaks about a tenth of the
    /// platform's travel per second, which walks a rider off the back of a
    /// command1-length tram ride; support-based transfer does not.
    #[test]
    fn player_is_carried_by_a_horizontally_moving_platform() {
        let (mut world, mut player, platform) = world_with_kinematic_platform();
        let start = world.get_player_translation(&player);

        for frame in 1..=60 {
            world.set_translation(platform, vec3(PLATFORM_STEP * frame as f32, -0.5, 0.0));
            step(&mut world, &mut player, 1);
        }
        // One more (stationary) frame: a movement frame writes the player's
        // *next* kinematic position, so the last one only lands at the next
        // step.
        step(&mut world, &mut player, 1);

        let end = world.get_player_translation(&player);
        let platform_travel = PLATFORM_STEP * 60.0;
        // The property that matters is that the player does not slide on the
        // deck: a rider who keeps only 90% of the platform's travel (which is
        // what rapier's own contact-based transfer leaks - see
        // `player_movement_queries`) is off the back of a tram car within a
        // couple of seconds.
        assert!(
            (end.x - start.x - platform_travel).abs() < 0.02,
            "player should ride the platform all {platform_travel} units without \
             sliding on it: moved {} (from {start:?} to {end:?})",
            end.x - start.x,
        );
        assert!(
            (end.y - start.y).abs() < 0.1,
            "player should stay on the deck: y {} -> {}",
            start.y,
            end.y,
        );
    }

    /// Carry comes from the surface the player is *standing on*: a player in
    /// the air above a moving platform keeps their own trajectory until they
    /// land on it.
    #[test]
    fn player_is_not_carried_while_airborne_above_a_platform() {
        let (mut world, mut player, platform) = world_with_kinematic_platform();
        // Well clear of the deck - far enough that a full second of falling
        // (0.2 wu per frame) does not reach it.
        world.set_player_translation(vec3(0.0, PLAYER_HALF_HEIGHT + 15.0, 0.0), &mut player);
        step(&mut world, &mut player, 1);
        let start = world.get_player_translation(&player);

        for frame in 1..=60 {
            world.set_translation(platform, vec3(PLATFORM_STEP * frame as f32, -0.5, 0.0));
            step(&mut world, &mut player, 1);
        }

        let end = world.get_player_translation(&player);
        assert!(
            (end.x - start.x).abs() < 0.02,
            "an airborne player must not be towed by the platform below them: \
             moved {} in x",
            end.x - start.x,
        );
        assert!(
            end.y < start.y - 1.0,
            "the airborne player should still be falling: y {} -> {}",
            start.y,
            end.y,
        );
    }

    /// Vertical carry (grav lifts) must keep working: the player rises with the
    /// deck rather than being left in the air or pushed through it.
    #[test]
    fn player_is_carried_by_a_vertically_moving_platform() {
        let (mut world, mut player, platform) = world_with_kinematic_platform();
        let start = world.get_player_translation(&player);

        for frame in 1..=60 {
            world.set_translation(
                platform,
                vec3(0.0, -0.5 + PLATFORM_STEP * frame as f32, 0.0),
            );
            step(&mut world, &mut player, 1);
        }
        // See the horizontal case: the last frame's movement lands next step.
        step(&mut world, &mut player, 1);

        let end = world.get_player_translation(&player);
        let platform_travel = PLATFORM_STEP * 60.0;
        assert!(
            (end.y - start.y - platform_travel).abs() < 0.02,
            "player should ride the lift all {platform_travel} units up: moved {} (from {start:?} to {end:?})",
            end.y - start.y,
        );
    }

    /// Carry comes from the surface underfoot, not from any moving thing the
    /// player touches: a kinematic wall sliding along its own length past a
    /// player standing on static ground must not drag them with it.
    #[test]
    fn player_is_not_dragged_by_a_wall_they_are_only_brushing() {
        let (mut world, mut player) = world_with_floor();
        // The shared floor's top surface is at y = 0.
        world.set_player_translation(vec3(0.0, PLAYER_HALF_HEIGHT + 0.1, 0.0), &mut player);
        // A tall thin wall running along x, just grazing the player's side
        // (its face sits one contact offset away from the capsule).
        let wall_z =
            PLAYER_STANDING_RADIUS / SCALE_FACTOR + 0.1 + PLAYER_CONTACT_OFFSET / SCALE_FACTOR;
        let wall = world.add_kinematic(
            EntityId::from_inner(2002).unwrap(),
            vec3(0.0, 1.0, wall_z),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            vec3(40.0, 4.0, 0.2),
            CollisionGroup::entity(),
            false,
        );
        step(&mut world, &mut player, 30);
        let start = world.get_player_translation(&player);

        for frame in 1..=60 {
            world.set_translation(wall, vec3(PLATFORM_STEP * frame as f32, 1.0, wall_z));
            step(&mut world, &mut player, 1);
        }

        let end = world.get_player_translation(&player);
        assert!(
            (end.x - start.x).abs() < 0.02,
            "player must not be dragged along by a wall they only brush: moved {} in x",
            end.x - start.x,
        );
    }

    fn live_creature_test_world(
        first_id: u64,
        floor_size: f32,
    ) -> (PhysicsWorld, PlayerHandle, EntityId, RigidBodyHandle) {
        let mut world = PhysicsWorld::new();
        world.add_kinematic(
            EntityId::from_inner(first_id).unwrap(),
            vec3(0.0, -0.5, 0.0),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            vec3(floor_size, 1.0, floor_size),
            CollisionGroup::entity(),
            false,
        );
        let creature_id = EntityId::from_inner(first_id + 1).unwrap();
        let creature = world.add_dynamic(
            creature_id,
            vec3(0.0, 1.0, 0.0),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Capsule {
                height: 1.0,
                radius: 0.5,
            },
            CollisionGroup::actor(),
            false,
            DynamicPhysicsOptions::default(),
        );
        world.set_enabled_rotations(creature_id, false, false, false);
        let player = world.create_player(
            vec3(0.0, 10.0, 20.0),
            EntityId::from_inner(first_id + 2).unwrap(),
        );
        (world, player, creature_id, creature)
    }

    fn step_creature_test(
        world: &mut PhysicsWorld,
        player: &mut PlayerHandle,
        living_creatures: &[EntityId],
        frames: usize,
    ) {
        for _ in 0..frames {
            world.update(vec3(0.0, 0.0, 0.0), player);
            world.recover_live_creatures_swept_off_support(living_creatures);
        }
    }

    fn drive_creature_horizontally(
        world: &mut PhysicsWorld,
        player: &mut PlayerHandle,
        creature_id: EntityId,
        frames: usize,
    ) {
        for _ in 0..frames {
            let y_velocity = world.get_velocity(creature_id).unwrap().y;
            world.set_velocity(creature_id, vec3(1.0, y_velocity, 0.0));
            step_creature_test(world, player, &[creature_id], 1);
        }
    }

    fn add_sweeping_wall(world: &mut PhysicsWorld, entity_id: u64) -> RigidBodyHandle {
        world.add_kinematic(
            EntityId::from_inner(entity_id).unwrap(),
            vec3(-2.8, 1.0, 0.0),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            vec3(0.8, 3.0, 3.8),
            CollisionGroup::entity(),
            false,
        )
    }

    fn sweep_wall_across_creature(
        world: &mut PhysicsWorld,
        player: &mut PlayerHandle,
        wall: RigidBodyHandle,
        living_creatures: &[EntityId],
    ) {
        for frame in 0..90 {
            world.set_translation(wall, vec3(-2.8 + frame as f32 * 0.07, 1.0, 0.0));
            step_creature_test(world, player, living_creatures, 1);
        }
        step_creature_test(world, player, living_creatures, 60);
    }

    /// A kinematic hazard crossing a finite deck must not sweep a living
    /// gravity-driven creature over the edge. This is SHODAN's Spike02 /
    /// Red Assassin 649 geometry in miniature.
    #[test]
    fn moving_kinematic_keeps_live_creature_on_its_last_support() {
        let (mut world, mut player, creature_id, creature) = live_creature_test_world(2100, 4.0);
        let wall = add_sweeping_wall(&mut world, 2103);
        step_creature_test(&mut world, &mut player, &[creature_id], 30);
        sweep_wall_across_creature(&mut world, &mut player, wall, &[creature_id]);

        let end = world.get_position(creature).unwrap();
        assert!(
            end.y > 0.5 && end.x < 2.5,
            "moving terrain swept the live creature off its supported deck: {end:?}"
        );
    }

    /// A support anchor is not a general anti-fall mechanism. With no moving
    /// kinematic side-contact, a living creature can walk over an edge and
    /// continues falling under gravity.
    #[test]
    fn live_creature_can_intentionally_leave_support_and_fall() {
        let (mut world, mut player, creature_id, creature) = live_creature_test_world(2110, 4.0);
        step_creature_test(&mut world, &mut player, &[creature_id], 30);
        world.set_velocity(creature_id, vec3(20.0, 0.0, 0.0));
        step_creature_test(&mut world, &mut player, &[creature_id], 120);

        let end = world.get_position(creature).unwrap();
        assert!(
            end.y < -2.0,
            "ordinary unsupported motion must keep falling instead of snapping to its anchor: {end:?}"
        );
    }

    /// Repeated AI-style horizontal velocity on a broad static floor must not
    /// be mistaken for a kinematic sweep or introduce anchor jitter.
    #[test]
    fn live_creature_locomotion_advances_normally_on_static_support() {
        let (mut world, mut player, creature_id, creature) = live_creature_test_world(2120, 40.0);
        drive_creature_horizontally(&mut world, &mut player, creature_id, 120);

        let end = world.get_position(creature).unwrap();
        assert!(
            end.x > 1.5 && end.y > 0.5,
            "live locomotion should advance smoothly across static support: {end:?}"
        );
    }

    /// A living creature driven by the same repeated velocity updates as the
    /// AI must cross an unsimulated debris/frob stand-in while that collider
    /// remains present for physical props and interaction rays.
    #[test]
    fn live_creature_locomotion_crosses_character_passable_obstacle() {
        let (mut world, mut player, creature_id, creature) = live_creature_test_world(2125, 40.0);
        world.add_kinematic(
            EntityId::from_inner(2128).unwrap(),
            vec3(1.0, 1.0, 0.0),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            vec3(0.4, 3.0, 3.0),
            CollisionGroup::entity().non_solid_to_characters(),
            false,
        );

        drive_creature_horizontally(&mut world, &mut player, creature_id, 180);

        let end = world.get_position(creature).unwrap();
        assert!(
            end.x > 2.0 && end.y > 0.5,
            "live creature should cross the character-passable obstacle: {end:?}"
        );
    }

    /// Dead creatures are deliberately omitted by the mission caller. Any
    /// stale living anchor is pruned, so corpse physics remains independent.
    #[test]
    fn moving_kinematic_does_not_restore_dead_creature_support() {
        let (mut world, mut player, creature_id, creature) = live_creature_test_world(2130, 4.0);
        let wall = add_sweeping_wall(&mut world, 2133);
        step_creature_test(&mut world, &mut player, &[creature_id], 30);
        sweep_wall_across_creature(&mut world, &mut player, wall, &[]);

        let end = world.get_position(creature).unwrap();
        assert!(
            end.y < 0.5 || end.x > 2.5,
            "dead creature physics must not be restored to a stale living anchor: {end:?}"
        );
    }

    /// The seven-waypoint route the reversal tests drive by hand.
    fn test_waypoints() -> [Vector<Real>; 7] {
        [
            vector![0.0, 0.0, 0.0],
            vector![0.0, 0.0, 0.0],
            vector![2.0, 0.0, 0.0],
            vector![2.5, 0.0, 0.0],
            vector![3.0, 0.0, 0.0],
            vector![4.0, 0.0, 0.0],
            vector![4.0, -0.64, 0.0],
        ]
    }

    #[test]
    fn sleeping_dynamic_body_stays_dynamic_and_wakes_on_impulse() {
        let mut world = PhysicsWorld::new();
        let handle = world.add_dynamic(
            EntityId::from_inner(1).unwrap(),
            vec3(0.0, 1.0, 0.0),
            identity_quat(),
            vec3(0.0, 0.0, 0.0),
            PhysicsShape::Capsule {
                height: 1.0,
                radius: 0.5,
            },
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions::default(),
        );

        world.sleep_body(handle);
        let sleeping = world.debug_body_info(handle, &world.rigid_body_set[handle]);
        assert_eq!(sleeping.body_type, "dynamic");
        assert!(sleeping.is_sleeping);

        world.apply_impulse(handle, vec3(1.0, 0.0, 0.0));
        let woken = world.debug_body_info(handle, &world.rigid_body_set[handle]);
        assert_eq!(woken.body_type, "dynamic");
        assert!(!woken.is_sleeping);
    }

    /// A higher-elasticity object must rebound higher than a low-elasticity one
    /// dropped from the same height. (Negative-first: before `add_dynamic`
    /// honored `opts.restitution`, both used the hardcoded 0.7 and reached the
    /// same height, so this assertion failed.)
    #[test]
    fn higher_restitution_bounces_higher() {
        let (mut world, mut player) = world_with_floor();

        let low = world.add_dynamic(
            EntityId::from_inner(1).unwrap(),
            vec3(-5.0, 5.0, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            PhysicsShape::Sphere(0.5),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions {
                restitution: 0.1,
                ..Default::default()
            },
        );
        let high = world.add_dynamic(
            EntityId::from_inner(2).unwrap(),
            vec3(5.0, 5.0, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            PhysicsShape::Sphere(0.5),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions {
                restitution: 0.95,
                ..Default::default()
            },
        );

        // Both balls fall identically (~58 frames to first impact from y=5).
        // Measure the rebound apex *after* that first contact, so the shared
        // initial drop height doesn't mask the difference.
        const SETTLE_FRAMES: usize = 70;
        let mut low_peak = f32::MIN;
        let mut high_peak = f32::MIN;
        for frame in 0..300 {
            step(&mut world, &mut player, 1);
            if frame >= SETTLE_FRAMES {
                low_peak = low_peak.max(world.get_position(low).unwrap().y);
                high_peak = high_peak.max(world.get_position(high).unwrap().y);
            }
        }
        assert!(
            high_peak > low_peak + 0.1,
            "high-restitution apex {high_peak} should exceed low-restitution apex {low_peak}"
        );
    }

    /// A degenerate ray (zero-length or non-finite direction) whose origin sits
    /// inside a collider AABB must return `None`, not panic. (Negative-first:
    /// without the guard in `ray_cast2`, parry's `clip_aabb_line` returns face
    /// index 0 for this case and the feature math computes `0u32 - 1`, panicking
    /// with "attempt to subtract with overflow" in debug builds - issue #405.)
    #[test]
    fn degenerate_ray_does_not_panic() {
        let (mut world, mut player) = world_with_floor();
        // A dynamic *cuboid* the query pipeline will actually see (static bodies
        // don't enter the query BVH until they move). The shape must be a cuboid:
        // parry's face-index overflow is in the ray-vs-box path; a ball uses a
        // different ray cast and never hits it. AABB ~ [-1, 1, -1] .. [1, 3, 1].
        world.add_dynamic(
            EntityId::from_inner(42).unwrap(),
            vec3(0.0, 2.0, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            PhysicsShape::Cuboid(vec3(1.0, 1.0, 1.0)),
            CollisionGroup::entity(),
            false,
            DynamicPhysicsOptions::default(),
        );
        step(&mut world, &mut player, 1);

        // Origin inside the cuboid's AABB - the exact condition that makes
        // parry's `clip_aabb_line` return face index 0 for a degenerate ray.
        let origin = point3(0.0, 2.0, 0.0);

        let zero_dir = world.ray_cast2(
            origin,
            Vector3::new(0.0, 0.0, 0.0),
            100.0,
            InternalCollisionGroups::ALL_COLLIDABLE,
            None,
            false,
        );
        assert!(zero_dir.is_none(), "zero-direction ray should return None");

        let nan_dir = world.ray_cast2(
            origin,
            Vector3::new(f32::NAN, 0.0, 0.0),
            100.0,
            InternalCollisionGroups::ALL_COLLIDABLE,
            None,
            false,
        );
        assert!(nan_dir.is_none(), "NaN-direction ray should return None");

        // Regression guard: a valid ray must still hit (the check must reject
        // only degenerate rays, not good ones).
        let valid = world.ray_cast2(
            point3(0.0, 5.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
            100.0,
            InternalCollisionGroups::ALL_COLLIDABLE,
            None,
            false,
        );
        assert!(valid.is_some(), "valid downward ray should hit the cuboid");
    }

    /// A higher-friction box, given the same initial horizontal velocity on the
    /// floor, must travel less far than a low-friction one. (Negative-first:
    /// before `add_dynamic` honored `opts.friction`, both used Rapier's default
    /// and traveled the same distance.)
    #[test]
    fn higher_friction_slides_less() {
        let (mut world, mut player) = world_with_floor();

        let make_box = |world: &mut PhysicsWorld, id: u64, z: f32, friction: f32| {
            let entity = EntityId::from_inner(id).unwrap();
            let handle = world.add_dynamic(
                entity,
                vec3(0.0, 0.5, z),
                identity_quat(),
                Vector3::new(0.0, 0.0, 0.0),
                PhysicsShape::Cuboid(vec3(1.0, 1.0, 1.0)),
                CollisionGroup::entity(),
                false,
                DynamicPhysicsOptions {
                    friction,
                    ..Default::default()
                },
            );
            // Slide, don't tumble: isolate sliding friction from rolling.
            world.set_enabled_rotations(entity, false, false, false);
            world.set_velocity(entity, vec3(10.0, 0.0, 0.0));
            handle
        };

        let slippery = make_box(&mut world, 1, -5.0, 0.0);
        let grippy = make_box(&mut world, 2, 5.0, 1.0);

        step(&mut world, &mut player, 120);

        let slippery_x = world.get_position(slippery).unwrap().x;
        let grippy_x = world.get_position(grippy).unwrap().x;
        assert!(
            slippery_x > grippy_x + 1.0,
            "low-friction box ({slippery_x}) should out-slide high-friction box ({grippy_x})"
        );
    }

    // --- Flat ladder climbing (`climb_redirect` + CLIMBABLE detection) ---

    /// Pushing toward the ladder redirects the input to an ascent.
    #[test]
    fn climb_redirect_ascends_when_pushing_into_ladder() {
        let toward = vector![1.0, 0.0, 0.0];
        let climb = climb_redirect(vector![0.1, 0.0, 0.0], toward).expect("should grip");
        assert!(climb.y > 0.0, "expected upward redirect, got {climb:?}");
    }

    /// Pushing away from (or parallel to) the ladder does not grip.
    #[test]
    fn climb_redirect_ignores_push_away_or_parallel() {
        let toward = vector![1.0, 0.0, 0.0];
        assert!(climb_redirect(vector![-0.1, 0.0, 0.0], toward).is_none());
        assert!(climb_redirect(vector![0.0, 0.0, 0.1], toward).is_none());
        assert!(climb_redirect(vector![0.0, 0.0, 0.0], toward).is_none());
    }

    /// A downward-pitched push (looking down) descends instead of ascending.
    #[test]
    fn climb_redirect_descends_when_looking_down() {
        let toward = vector![1.0, 0.0, 0.0];
        let climb = climb_redirect(vector![0.1, -0.1, 0.0], toward).expect("should grip");
        assert!(climb.y < 0.0, "expected downward redirect, got {climb:?}");
    }

    #[test]
    fn climb_top_out_uses_facing_and_rejects_sideways_input() {
        let facing = vector![0.0, 0.0, 1.0];
        assert!(
            climb_top_out_direction(vector![1.0, 0.0, 0.0], facing).is_none(),
            "pure strafe must not start a facing-directed mantle"
        );
        assert_eq!(
            climb_top_out_direction(vector![0.1, 0.0, 0.1], facing),
            Some(facing)
        );
    }

    #[test]
    fn climb_top_out_recovery_uses_minimum_bounded_clearance() {
        let mut world = PhysicsWorld::new();
        let mut player =
            world.create_player(vec3(100.0, 100.0, 100.0), EntityId::from_inner(1).unwrap());
        world.add_kinematic(
            EntityId::from_inner(2).unwrap(),
            vec3(0.0, 0.5, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(0.8, 0.2, 4.0),
            CollisionGroup::entity(),
            false,
        );
        world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);

        let queries = query_pipeline(&world, QueryFilter::default());
        let head = vector![0.0, 0.0, 0.0];
        let direction = Vector::x();
        let cross_y = 1.4;
        let compressed = Ball::new(CLIMB_TOP_OUT_RADIUS);
        assert!(
            !shape_sweep_is_clear(&queries, head, vector![0.0, cross_y, 0.0], &compressed),
            "zero retreat must remain blocked by the overhang"
        );
        let one_offset = head - direction * CLIMB_TOP_OUT_RECOVERY_RETREAT;
        assert!(
            !shape_sweep_is_clear(
                &queries,
                one_offset,
                vector![one_offset.x, cross_y, one_offset.z],
                &compressed,
            ),
            "one contact offset must not be mistaken for full overhang clearance"
        );

        let recovered =
            climb_top_out_recovery_start(&queries, head, direction, cross_y, &compressed)
                .expect("the authored recovery bound should reach a clear vertical sweep");
        let retreat = (head - recovered).norm();
        assert!(retreat > CLIMB_TOP_OUT_RECOVERY_RETREAT);
        assert!(retreat <= CLIMB_TOP_OUT_MAX_RECOVERY_RETREAT);
        assert!(shape_sweep_is_clear(
            &queries,
            recovered,
            vector![recovered.x, cross_y, recovered.z],
            &compressed,
        ));
        let previous = recovered + direction * CLIMB_TOP_OUT_RECOVERY_RETREAT;
        assert!(
            !shape_sweep_is_clear(
                &queries,
                previous,
                vector![previous.x, cross_y, previous.z],
                &compressed,
            ),
            "the recovery search must return the first clear contact-offset step"
        );
    }

    #[test]
    fn climb_top_out_rejects_an_initial_obstruction_that_outlasts_its_bound() {
        let mut world = PhysicsWorld::new();
        world.add_collider(
            EntityId::from_inner(1).unwrap(),
            ColliderBuilder::cuboid(0.5, 0.2, 2.0)
                .translation(vector![0.7, 0.0, 0.0])
                .build(),
        );
        let mut player =
            world.create_player(vec3(100.0, 100.0, 100.0), EntityId::from_inner(2).unwrap());
        world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
        let queries = query_pipeline(&world, QueryFilter::default());
        let route_probe = Capsule::new_y(PROBE_SKIN / SCALE_FACTOR, PROBE_SKIN / SCALE_FACTOR);

        assert!(
            !shape_route_exits_only_initial_obstruction(
                &queries,
                &[vector![0.0, 0.0, 0.0], vector![2.0, 0.0, 0.0]],
                &route_probe,
                0.4,
            ),
            "an obstruction entered inside the allowance must still clear before the bound ends"
        );
    }

    #[test]
    fn climb_top_out_rejects_parentless_terrain_on_recovered_approach() {
        let mut world = PhysicsWorld::new();
        world.add_collider(
            EntityId::from_inner(1).unwrap(),
            ColliderBuilder::cuboid(10.0, 0.05, 10.0)
                .translation(vector![0.0, -0.55, 0.0])
                .build(),
        );
        // This parented overhang misses the center ray but forces the full
        // compressed sphere to retreat in -X before rising.
        world.add_kinematic(
            EntityId::from_inner(2).unwrap(),
            vec3(0.35, 1.4, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(0.2, 0.4, 4.0),
            CollisionGroup::entity(),
            false,
        );
        // The recovered rise starts inside this unrelated parentless wall and
        // moves out toward Dark's probe endpoint. A shape cast configured to
        // ignore initial penetration does not report that escape, while the
        // original unretreated point ray at x >= 0 does not see the wall.
        world.add_collider(
            EntityId::from_inner(3).unwrap(),
            ColliderBuilder::cuboid(0.03, 0.2, 2.0)
                .translation(vector![-0.08, 2.04, 0.0])
                .build(),
        );
        let mut player =
            world.create_player(vec3(100.0, 100.0, 100.0), EntityId::from_inner(4).unwrap());
        world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);

        let movement = plan_top_out_from_origin(&world);

        let planned_route = movement
            .as_ref()
            .and_then(|movement| movement.top_out)
            .map(|top_out| top_out.waypoints);
        assert!(
            movement.is_none(),
            "the recovered approach must not cross unrelated parentless terrain; planned {planned_route:?}"
        );
    }

    #[test]
    fn climb_top_out_rejects_an_obstructed_clearance_probe() {
        let mut world = PhysicsWorld::new();
        let floor = ColliderBuilder::cuboid(10.0, 0.05, 10.0)
            .translation(vector![0.0, -0.55, 0.0])
            .build();
        world.add_collider(EntityId::from_inner(1).unwrap(), floor);
        let ceiling = ColliderBuilder::cuboid(0.05, 0.05, 1.0)
            .translation(vector![0.0, 1.0, 0.0])
            .build();
        world.add_collider(EntityId::from_inner(2).unwrap(), ceiling);
        let mut player =
            world.create_player(vec3(100.0, 100.0, 100.0), EntityId::from_inner(3).unwrap());
        world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);

        let movement = plan_top_out_from_origin(&world);

        assert!(
            movement.is_none(),
            "a blocked upward clearance probe must not become permission to phase through terrain"
        );
    }

    #[test]
    fn climb_top_out_rejects_parentless_terrain_beyond_the_lip_probe() {
        let mut world = PhysicsWorld::new();
        world.add_collider(
            EntityId::from_inner(1).unwrap(),
            ColliderBuilder::cuboid(10.0, 0.05, 10.0)
                .translation(vector![0.0, -0.55, 0.0])
                .build(),
        );
        // The Dark point probe ends at x=0.64. This wall sits beyond that
        // bounded lip region but before the otherwise-clear final standing
        // pose. A blanket parentless-terrain exemption phases through it.
        world.add_collider(
            EntityId::from_inner(2).unwrap(),
            ColliderBuilder::cuboid(0.05, 2.0, 2.0)
                .translation(vector![1.0, 1.0, 0.0])
                .build(),
        );
        let mut player =
            world.create_player(vec3(100.0, 100.0, 100.0), EntityId::from_inner(3).unwrap());
        world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);

        let movement = plan_top_out_from_origin(&world);

        assert!(
            movement.is_none(),
            "the lip exception must not permit crossing unrelated parentless terrain"
        );
    }

    #[test]
    fn climb_top_out_reverses_when_a_parented_blocker_appears() {
        let mut world = PhysicsWorld::new();
        let mut player =
            world.create_player(vec3(100.0, 100.0, 100.0), EntityId::from_inner(1).unwrap());
        world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
        let controller = KinematicCharacterController::default();
        let mut pos = Isometry::translation(0.0, 0.0, 0.0);
        let top_out = ClimbTopOut {
            waypoints: test_waypoints(),
            next_waypoint: 2,
            save_pose: vector![-1.0, 0.0, 0.0],
            reversing: false,
            is_crouched: false,
        };
        let first = {
            let queries = query_pipeline(&world, QueryFilter::default());
            advance_climb_top_out(&controller, &queries, &queries, &pos, top_out, 1.0 / 60.0)
        };
        assert!(first.0.translation.x > 0.0);
        pos.translation.vector += first.0.translation;
        let blocker_x =
            pos.translation.vector.x + CLIMB_TOP_OUT_RADIUS + 0.5 * CLIMB_TOP_OUT_SUBSTEP;
        world.add_kinematic(
            EntityId::from_inner(2).unwrap(),
            vec3(blocker_x, 0.0, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(0.02, 2.0, 2.0),
            CollisionGroup::entity(),
            false,
        );
        world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
        let queries = query_pipeline(&world, QueryFilter::default());
        let mut active = first.1.expect("route remains active");
        let mut reversing = None;
        for _ in 0..32 {
            let (movement, next) =
                advance_climb_top_out(&controller, &queries, &queries, &pos, active, 1.0 / 60.0);
            pos.translation.vector += movement.translation;
            active = next.expect("blocked route should remain recoverable");
            if active.reversing {
                reversing = Some(active);
                break;
            }
        }
        let reversing = reversing.expect("the live blocker must reverse the route");
        assert!(reversing.reversing);
        player.top_out = Some(reversing);
        assert_eq!(
            world.get_player_save_translation(&player),
            Ok(vec3(-1.0, 0.0, 0.0)),
            "an in-flight save must serialize the last standing pose"
        );
    }

    #[test]
    fn climb_top_out_reverses_out_of_an_overlapping_parented_blocker() {
        let mut world = PhysicsWorld::new();
        let controller = KinematicCharacterController::default();
        let mut pos = Isometry::translation(0.0, 0.0, 0.0);
        let mut active = ClimbTopOut {
            waypoints: test_waypoints(),
            next_waypoint: 2,
            save_pose: vector![-1.0, 0.0, 0.0],
            reversing: false,
            is_crouched: false,
        };
        world.add_kinematic(
            EntityId::from_inner(2).unwrap(),
            vec3(0.1, 0.0, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(0.02, 2.0, 2.0),
            CollisionGroup::entity(),
            false,
        );
        let mut player =
            world.create_player(vec3(100.0, 100.0, 100.0), EntityId::from_inner(3).unwrap());
        world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
        let start = pos.translation.vector;

        for _ in 0..32 {
            let queries = query_pipeline(&world, QueryFilter::default());
            let (movement, next) =
                advance_climb_top_out(&controller, &queries, &queries, &pos, active, 1.0 / 60.0);
            pos.translation.vector += movement.translation;
            let Some(next) = next else {
                break;
            };
            active = next;
        }

        assert!(
            pos.translation.vector.x < start.x - 0.01,
            "a live entity overlapping the compressed player must not permanently pin reversal"
        );
    }

    #[test]
    fn crouched_top_out_finishes_without_expanding_into_low_headroom() {
        let mut world = PhysicsWorld::new();
        world.add_kinematic(
            EntityId::from_inner(2).unwrap(),
            vec3(0.0, 0.75, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(4.0, 0.1, 4.0),
            CollisionGroup::entity(),
            false,
        );
        let queries = query_pipeline(&world, QueryFilter::default());
        let controller = KinematicCharacterController::default();
        let pos = Isometry::identity();
        let completed = ClimbTopOut {
            waypoints: test_waypoints(),
            next_waypoint: test_waypoints().len(),
            save_pose: Vector::zeros(),
            reversing: false,
            is_crouched: true,
        };

        let (movement, active) =
            advance_climb_top_out(&controller, &queries, &queries, &pos, completed, 1.0 / 60.0);

        assert_eq!(movement.translation, Vector::zeros());
        assert!(
            active.is_none(),
            "the crouched final capsule fits below headroom that blocks standing"
        );
    }

    #[test]
    fn validated_move_refuses_a_top_out_whose_safe_pose_became_blocked() {
        let mut world = PhysicsWorld::new();
        let mut player = world.create_player(vec3(3.0, 0.5, 0.0), EntityId::from_inner(1).unwrap());
        world.add_kinematic(
            EntityId::from_inner(2).unwrap(),
            vec3(0.0, 0.5, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(1.0, 3.0, 3.0),
            CollisionGroup::entity(),
            false,
        );
        world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
        let compressed_pose = vector![3.0, 0.5, 0.0];
        let save_pose = vector![0.0, 0.5, 0.0];
        world.set_player_translation(nvec_to_cgmath(compressed_pose), &mut player);
        let collider_handle = world.rigid_body_set[player.character_handle].colliders()[0];
        world.collider_set[collider_handle].set_shape(SharedShape::ball(CLIMB_TOP_OUT_RADIUS));
        world.rigid_body_set[player.character_handle].enable_ccd(false);
        player.top_out = Some(ClimbTopOut {
            waypoints: [compressed_pose; 7],
            next_waypoint: 3,
            save_pose,
            reversing: true,
            is_crouched: false,
        });
        assert_eq!(
            world.get_player_save_translation(&player),
            Err(PlayerSavePoseError::BlockedRecoveryPose),
            "a blocker occupying the cached standing pose must defer saving"
        );

        let result = world.move_player_validated(vec3(f32::NAN, 0.5, 0.0), &mut player);

        assert!(!result.moved, "an unsafe rewind must not move the body");
        assert!(result.blocked, "an unsafe rewind must report blocked");
        assert!(
            player.top_out.is_some(),
            "the compressed recovery must remain active until a standing pose is clear"
        );
        assert_eq!(
            world.get_player_translation(&player),
            nvec_to_cgmath(compressed_pose),
            "the body must remain at its live compressed pose"
        );
    }

    /// A player standing at the base of a climbable wall who pushes into it
    /// climbs it; against an identical NON-climbable wall the same input leaves
    /// the player on the floor. (Negative-first: without the CLIMBABLE
    /// detection + redirect in `move_player`, both cases stay at floor height
    /// and the ascent assertion fails.)
    ///
    /// Also guards against lateral drift: the player starts OFF-CENTER on the
    /// wall (z = 0.5 on a 2-wide wall). With a collider-center-based climb
    /// direction the residual lateral term steers the player sideways along
    /// the wall while climbing; the contact face normal keeps the climb
    /// straight (with center-delta the ascent itself also collapses here).
    #[test]
    fn player_climbs_climbable_wall_but_not_plain_wall() {
        let run = |group: CollisionGroup| -> (f32, f32) {
            let mut world = PhysicsWorld::new();
            // Floor top at y=0, built the way the game builds level geometry
            // (a parentless collider): a fixed-BODY floor never enters the
            // query BVH the character controller moves against, so the player
            // would fall straight through it.
            let floor_verts = vec![
                point![-100.0, 0.0, -100.0],
                point![100.0, 0.0, -100.0],
                point![100.0, 0.0, 100.0],
                point![-100.0, 0.0, 100.0],
            ];
            let floor_tris = vec![[0u32, 1, 2], [0, 2, 3]];
            world.add_collider(
                EntityId::from_inner(1000).unwrap(),
                ColliderBuilder::trimesh(floor_verts, floor_tris)
                    .expect("floor trimesh")
                    .build(),
            );
            // Off-center on the wall's z-extent (see doc comment). The scene
            // sits at x=-6 so the floor's triangle seam (the x=z diagonal)
            // stays away from the walk path - crossing the seam produces a
            // lateral slide artifact unrelated to climbing.
            let mut player =
                world.create_player(vec3(-6.0, 1.0, 0.5), EntityId::from_inner(2000).unwrap());
            // A tall thin "ladder" wall just +x of the player, feet on the floor.
            world.add_kinematic(
                EntityId::from_inner(2001).unwrap(),
                vec3(-5.0, 5.0, 0.0),
                identity_quat(),
                Vector3::new(0.0, 0.0, 0.0),
                vec3(0.2, 10.0, 2.0),
                group,
                false,
            );
            // Let the player settle onto the floor, then push into the wall.
            for _ in 0..30 {
                world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
            }
            let start = world.get_player_translation(&player);
            for _ in 0..240 {
                world.update(Vector3::new(0.05, 0.0, 0.0), &mut player);
            }
            let end = world.get_player_translation(&player);
            (end.y - start.y, end.z - start.z)
        };

        let (climbable_ascent, climbable_drift) = run(CollisionGroup::climbable_entity());
        let (plain_ascent, _) = run(CollisionGroup::entity());

        assert!(
            climbable_ascent > 2.0,
            "pushing into a climbable wall should ascend it, rose {climbable_ascent}"
        );
        assert!(
            climbable_drift.abs() < 0.1,
            "climbing straight up must not drift sideways, drifted {climbable_drift}"
        );
        assert!(
            plain_ascent.abs() < 0.5,
            "a plain wall must not be climbable, rose {plain_ascent}"
        );
    }

    /// A push that only grazes the ladder (mostly along its face) is walking
    /// past it, not climbing it, so it must not grip - otherwise a ladder set
    /// into a corridor wall becomes flypaper, since a grip consumes the whole
    /// horizontal input.
    #[test]
    fn climb_redirect_ignores_a_grazing_push() {
        let toward = vector![1.0, 0.0, 0.0];
        // ~15 degrees into the face: mostly walking along it.
        assert!(climb_redirect(vector![0.026, 0.0, 0.097], toward).is_none());
        // ~70 degrees into the face: climbing.
        assert!(climb_redirect(vector![0.094, 0.0, 0.034], toward).is_some());
    }

    /// Issue #596: real ladders are built as a STACK of ~0.8 wu rung colliders
    /// only ~0.9 wu wide (eng1's Engine Core shaft, medsci1's cryo bay), and a
    /// player rarely faces one dead-on. Pushing into such a ladder a few
    /// degrees off perpendicular must still climb it.
    ///
    /// Negative-first: while `climb_redirect` passed the along-face component
    /// of the push through, that residue slid the player off the 0.9 wu-wide
    /// ladder at walk speed - the ascent stalled after ~1 wu and the player was
    /// shoved sideways clear of the rungs, ending back on the floor (before the
    /// fix this test finished at y +0.00, 2.40 wu off the ladder's center),
    /// which is exactly the live symptom.
    #[test]
    fn player_climbs_a_stacked_rung_ladder_when_pushing_slightly_off_angle() {
        let mut world = PhysicsWorld::new();
        // Floor top at y=0 (parentless trimesh, like level geometry).
        let floor_verts = vec![
            point![-100.0, 0.0, -100.0],
            point![100.0, 0.0, -100.0],
            point![100.0, 0.0, 100.0],
            point![-100.0, 0.0, 100.0],
        ];
        let floor_tris = vec![[0u32, 1, 2], [0, 2, 3]];
        world.add_collider(
            EntityId::from_inner(1000).unwrap(),
            ColliderBuilder::trimesh(floor_verts, floor_tris)
                .expect("floor trimesh")
                .build(),
        );
        // Ten stacked rungs, each the size of a shipped `rickladd` collider
        // (0.9 wide x 0.8 tall x 0.1 deep), contiguous from y=0 to y=8. Keep
        // the top above this test's short ascent so it continues to isolate
        // rung-to-rung climbing rather than exercising the separate top-out.
        for rung in 0..10 {
            world.add_kinematic(
                EntityId::from_inner(2001 + rung).unwrap(),
                vec3(-5.0, 0.4 + 0.8 * rung as f32, 0.0),
                identity_quat(),
                Vector3::new(0.0, 0.0, 0.0),
                vec3(0.1, 0.8, 0.9),
                CollisionGroup::climbable_entity(),
                false,
            );
        }
        let mut player =
            world.create_player(vec3(-6.0, 1.0, 0.0), EntityId::from_inner(3000).unwrap());
        for _ in 0..30 {
            world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
        }
        let start = world.get_player_translation(&player);
        // A frame of walking (25 SS2 ft/s at 60 Hz), aimed 20 degrees off the
        // ladder's face normal.
        let walk = 25.0 / SCALE_FACTOR / 60.0;
        let desired = Vector3::new(
            walk * 20f32.to_radians().cos(),
            0.0,
            walk * 20f32.to_radians().sin(),
        );
        for _ in 0..40 {
            world.update(desired, &mut player);
        }
        let end = world.get_player_translation(&player);
        let ascent = end.y - start.y;
        let drift = end.z - start.z;

        assert!(
            ascent > 3.0,
            "an off-angle push should still climb the rung stack, rose {ascent} (ended {end:?})"
        );
        assert!(
            drift.abs() < 0.45,
            "the climb must not slide the player off the 0.9-wide ladder, drifted {drift}"
        );
    }

    /// A diagonal route through a wide-X/thin-Z ladder clears at the first
    /// expanded AABB face (+Z), not at a projected far corner or the unrelated
    /// +X face.
    #[test]
    fn diagonal_top_out_clearance_exits_rectangular_ladders_thin_side() {
        let aabb = rapier3d::parry::bounding_volume::Aabb::new(
            point![-0.45, -10.0, -0.05],
            point![0.45, 30.0, 0.05],
        );
        let origin = vector![-0.755, 0.0, -0.392];
        let direction = vector![0.592, 0.0, 0.806];
        let clearance =
            PLAYER_STANDING_RADIUS / SCALE_FACTOR + PLAYER_CONTACT_OFFSET / SCALE_FACTOR;
        let exit = climbable_aabb_exit_distance(origin, direction, &aabb, clearance)
            .expect("the diagonal route crosses the expanded ladder");

        let z_exit = (aabb.maxs.z + clearance - origin.z) / direction.z;
        let x_exit = (aabb.maxs.x + clearance - origin.x) / direction.x;
        assert!(
            (exit - z_exit).abs() < 1.0e-5,
            "the route should clear at the thin +Z face: exit={exit}, z_exit={z_exit}"
        );
        assert!(
            exit < 0.5 * x_exit,
            "clearance must not be projected to the wide +X face: exit={exit}, x_exit={x_exit}"
        );
    }

    /// Issue #626: the actual top-out transition must carry a standing player
    /// through the ladder collider and leave them supported above the landing.
    ///
    /// Negative-first: the vertical-only climb redirect reaches standing height
    /// beside the landing, then its normal-walk fallback collides with the
    /// ladder. Before top-out handling the player remains on the shaft side at
    /// x ~= -5.4 indefinitely.
    #[test]
    fn player_tops_out_from_a_ladder_onto_an_upper_landing() {
        let quad = |x0: f32, x1: f32, y: f32| {
            let verts = vec![
                point![x0, y, -100.0],
                point![x1, y, -100.0],
                point![x1, y, 100.0],
                point![x0, y, 100.0],
            ];
            ColliderBuilder::trimesh(verts, vec![[0u32, 1, 2], [0, 2, 3]]).expect("trimesh")
        };

        let mut world = PhysicsWorld::new();
        // Shaft floor and an upper landing whose lip catches the side of the
        // standing capsule. The player's center remains on the shaft side, so
        // the head-point mantle probe is clear above the lip.
        world.add_collider(
            EntityId::from_inner(1000).unwrap(),
            quad(-100.0, 100.0, 0.0).build(),
        );
        world.add_collider(
            EntityId::from_inner(1001).unwrap(),
            quad(-5.2, 100.0, 6.4).build(),
        );
        // A thin climbable wall at the landing lip, continuing above the deck
        // like rick1's authored ladder. Its face toward the shaft is x=-5.1.
        world.add_kinematic(
            EntityId::from_inner(2001).unwrap(),
            vec3(-5.0, 3.2, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(0.2, 6.4, 2.0),
            CollisionGroup::climbable_entity(),
            false,
        );

        let mut player =
            world.create_player(vec3(-6.0, 1.0, 0.0), EntityId::from_inner(3000).unwrap());
        for _ in 0..30 {
            world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
        }

        let walk = 25.0 / SCALE_FACTOR / 60.0;
        for _ in 0..180 {
            world.update(Vector3::new(walk, 0.0, 0.0), &mut player);
        }
        let end = world.get_player_translation(&player);

        assert!(
            end.x > -4.5,
            "holding forward at the top must transfer the player through the ladder onto the landing, ended {end:?}"
        );
        assert!(
            end.y > 6.8,
            "the top-out must leave the player supported on the upper landing, ended {end:?}"
        );
    }

    /// Issue #603: a gripped ladder must be climbable DOWN, not just up. The
    /// player stands against the rung stack (at the resting gap a real approach
    /// leaves - `PLAYER_CONTACT_OFFSET`) and looks down: a downward-pitched
    /// push, still into the face, must ride the ladder back to the floor at the
    /// climb rate.
    ///
    /// Negative-first: while the climb cast collided with the ladder itself,
    /// the horizontal cap of the rung the capsule overlapped stopped the
    /// descent dead - before the fix this test covered 0.45 wu in 200 frames
    /// (y 6.50 -> 6.05) instead of reaching the floor. Live on hydro2's
    /// Sector-C stack the same block pinned the player at y 5.39, with the climb
    /// cast returning exactly zero translation for as long as "look down +
    /// push" was held (and, going the other way, topped the ascent out at
    /// y 5.2 instead of clearing the ladder).
    #[test]
    fn player_descends_a_stacked_rung_ladder() {
        let mut world = PhysicsWorld::new();
        let floor_verts = vec![
            point![-100.0, 0.0, -100.0],
            point![100.0, 0.0, -100.0],
            point![100.0, 0.0, 100.0],
            point![-100.0, 0.0, 100.0],
        ];
        let floor_tris = vec![[0u32, 1, 2], [0, 2, 3]];
        world.add_collider(
            EntityId::from_inner(1000).unwrap(),
            ColliderBuilder::trimesh(floor_verts, floor_tris)
                .expect("floor trimesh")
                .build(),
        );
        // The same shipped-size rung stack as the ascent test: 0.9 x 0.8 x 0.1,
        // contiguous from y=0 to y=8. Its climbable face is at x = -5.05.
        for rung in 0..10 {
            world.add_kinematic(
                EntityId::from_inner(2001 + rung).unwrap(),
                vec3(-5.0, 0.4 + 0.8 * rung as f32, 0.0),
                identity_quat(),
                Vector3::new(0.0, 0.0, 0.0),
                vec3(0.1, 0.8, 0.9),
                CollisionGroup::climbable_entity(),
                false,
            );
        }
        // A gripped player six rungs up, flush against the face at the gap the
        // walk pass leaves before the grip takes over (capsule radius + resting
        // contact offset). That is where a climbing player actually sits
        // (measured live on hydro2: 0.04 wu of clearance) and it is what puts
        // the rungs' horizontal caps in the path of the descent cast.
        let stand_x = -5.05 - (PLAYER_STANDING_RADIUS + PLAYER_CONTACT_OFFSET) / SCALE_FACTOR;
        let mut player =
            world.create_player(vec3(stand_x, 6.5, 0.0), EntityId::from_inner(3000).unwrap());

        // Looking down 60 degrees while still pushing into the face.
        let walk = 25.0 / SCALE_FACTOR / 60.0;
        let descend = Vector3::new(walk * 0.5, -walk * 0.866, 0.0);
        for _ in 0..40 {
            world.update(descend, &mut player);
        }
        let after = world.get_player_translation(&player);
        // 40 frames at the climb rate (CLIMB_SPEED_SCALE * walk * cos 60) covers
        // 2 wu; a free fall covers 8 and would already be on the floor.
        assert!(
            after.y < 6.2,
            "looking down and pushing into a ladder must descend it, stuck at {after:?}"
        );
        assert!(
            after.y > 4.0,
            "the descent must ride the ladder at the climb rate, not fall - reached {after:?} in 40 frames"
        );

        for _ in 0..160 {
            world.update(descend, &mut player);
        }
        let end = world.get_player_translation(&player);
        assert!(
            end.y < PLAYER_HALF_HEIGHT + 0.1,
            "the descent must carry the player all the way to the floor, ended {end:?}"
        );
    }

    /// Issue #603 / `projects/climbing.md`: entering a descent **from the top
    /// lip**. The player stands on the landing whose floor is flush with the
    /// ladder's top (eng1's Engine Core shaft: landing feet at y -12.80, top
    /// rung capped at -12.80), steps off into the shaft, and - still looking
    /// down - pushes back toward the ladder. The grip must catch them within a
    /// step's worth of falling and carry them down at the climb rate.
    ///
    /// Negative-first: while the climb cast collided with the ladder itself the
    /// grip caught but could not move the player down at all - the rung their
    /// capsule overlapped blocked the descent - so the push back toward the
    /// ladder just shoved them onto the landing again (before the fix this test
    /// was back at x -1.66, y +7.40, i.e. up on the landing).
    #[test]
    fn player_enters_a_descent_from_the_top_lip() {
        let quad = |x0: f32, x1: f32, y: f32| {
            let verts = vec![
                point![x0, y, -100.0],
                point![x1, y, -100.0],
                point![x1, y, 100.0],
                point![x0, y, 100.0],
            ];
            ColliderBuilder::trimesh(verts, vec![[0u32, 1, 2], [0, 2, 3]]).expect("trimesh")
        };

        let mut world = PhysicsWorld::new();
        // Shaft floor, and the landing above it - its lip flush with the
        // ladder's near face, so the shaft is everything at x < -4.95.
        world.add_collider(
            EntityId::from_inner(1000).unwrap(),
            quad(-100.0, 100.0, 0.0).build(),
        );
        world.add_collider(
            EntityId::from_inner(1001).unwrap(),
            quad(-4.95, 100.0, 6.4).build(),
        );
        // Rung stack from the shaft floor up to the landing (top rung capped at
        // y = 6.4). Its climbable face points into the shaft, at x = -5.05.
        for rung in 0..8 {
            world.add_kinematic(
                EntityId::from_inner(2001 + rung).unwrap(),
                vec3(-5.0, 0.4 + 0.8 * rung as f32, 0.0),
                identity_quat(),
                Vector3::new(0.0, 0.0, 0.0),
                vec3(0.1, 0.8, 0.9),
                CollisionGroup::climbable_entity(),
                false,
            );
        }

        let mut player =
            world.create_player(vec3(-3.0, 7.5, 0.0), EntityId::from_inner(3000).unwrap());
        for _ in 0..60 {
            world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
        }
        let landing = world.get_player_translation(&player);
        assert!(
            landing.y > 6.0,
            "setup: the player should be standing on the landing, at {landing:?}"
        );

        // Walk off the lip, looking down. Nothing to grip yet - the ladder top
        // is level with the floor being walked off.
        let walk = 25.0 / SCALE_FACTOR / 60.0;
        let off_the_lip = Vector3::new(-walk * 0.5, -walk * 0.866, 0.0);
        for _ in 0..30 {
            world.update(off_the_lip, &mut player);
            if world.get_player_translation(&player).x < -5.2 {
                break;
            }
        }
        let lip = world.get_player_translation(&player);
        assert!(
            lip.x < -5.05,
            "setup: the player should have stepped past the ladder into the shaft, at {lip:?}"
        );

        // Still looking down, push BACK toward the ladder: the natural "back
        // down the ladder" input. The grip must catch and take over from the
        // fall.
        let onto_the_ladder = Vector3::new(walk * 0.5, -walk * 0.866, 0.0);
        for _ in 0..40 {
            world.update(onto_the_ladder, &mut player);
        }
        let gripped = world.get_player_translation(&player);
        assert!(
            gripped.x < -5.05,
            "the push back toward the ladder must grip it, not shove the player onto the landing again - at {gripped:?}"
        );
        assert!(
            gripped.y > 3.0,
            "the grip must catch the fall near the top of the shaft, dropped to {gripped:?} in 40 frames \
             (a free fall covers the whole 6 wu shaft in 30)"
        );

        for _ in 0..120 {
            world.update(onto_the_ladder, &mut player);
        }
        let end = world.get_player_translation(&player);
        assert!(
            end.y < PLAYER_HALF_HEIGHT + 0.1,
            "the descent must carry the player to the shaft floor, ended {end:?}"
        );
    }

    /// A grip whose climb cannot move the player must not pin them in place.
    /// Standing at a ladder's foot while looking down redirects the push into a
    /// DESCENT, which is cast straight into the floor; since a grip also
    /// consumes the horizontal input and suppresses gravity, the player would
    /// be frozen with nothing to walk out with. The frame must fall back to
    /// normal walking, so pushing 30 degrees across the ladder still carries
    /// the player along it.
    #[test]
    fn a_climb_that_cannot_move_the_player_walks_instead() {
        let mut world = PhysicsWorld::new();
        let floor_verts = vec![
            point![-100.0, 0.0, -100.0],
            point![100.0, 0.0, -100.0],
            point![100.0, 0.0, 100.0],
            point![-100.0, 0.0, 100.0],
        ];
        let floor_tris = vec![[0u32, 1, 2], [0, 2, 3]];
        world.add_collider(
            EntityId::from_inner(1000).unwrap(),
            ColliderBuilder::trimesh(floor_verts, floor_tris)
                .expect("floor trimesh")
                .build(),
        );
        // A wide climbable wall at x=-5 (wide, so this is about the descent
        // cast hitting the floor - not about running out of ladder).
        world.add_kinematic(
            EntityId::from_inner(2001).unwrap(),
            vec3(-5.0, 5.0, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(0.2, 10.0, 20.0),
            CollisionGroup::climbable_entity(),
            false,
        );
        let mut player =
            world.create_player(vec3(-6.0, 1.0, 0.0), EntityId::from_inner(3000).unwrap());
        for _ in 0..30 {
            world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
        }
        let start = world.get_player_translation(&player);
        // Pushing 30 degrees across the wall (still inside the grip cone) while
        // pitched down - a downward redirect, cast into the floor.
        let walk = 25.0 / SCALE_FACTOR / 60.0;
        let desired = Vector3::new(
            walk * 30f32.to_radians().cos(),
            -walk,
            walk * 30f32.to_radians().sin(),
        );
        for _ in 0..60 {
            world.update(desired, &mut player);
        }
        let end = world.get_player_translation(&player);

        assert!(
            (end.z - start.z) > 0.5,
            "a climb that cannot move the player must fall back to walking, moved {} along the wall (ended {end:?})",
            end.z - start.z
        );
    }

    /// Walking into a stair-sized ledge steps up onto it; a too-tall ledge
    /// blocks. The step limit is 2 SS2 ft (0.8 wu), the original engine's
    /// step-probe height - a 1.5 ft riser climbs, a 3 ft ledge doesn't.
    /// (Negative-first: the old two-pass up-bump stepped at most
    /// `MOVEMENT_STEP_SIZE * dt` = 0.33 wu per frame, so the 0.6 wu riser
    /// failed before native autostep.)
    #[test]
    fn player_steps_up_stairs_but_not_tall_ledges() {
        // Height gained after walking +x into a `step_height`-tall platform.
        let run = |step_height: f32| -> f32 {
            let mut world = PhysicsWorld::new();
            // Floor top at y=0 (parentless trimesh, like level geometry).
            let floor_verts = vec![
                point![-100.0, 0.0, -100.0],
                point![100.0, 0.0, -100.0],
                point![100.0, 0.0, 100.0],
                point![-100.0, 0.0, 100.0],
            ];
            let floor_tris = vec![[0u32, 1, 2], [0, 2, 3]];
            world.add_collider(
                EntityId::from_inner(1000).unwrap(),
                ColliderBuilder::trimesh(floor_verts, floor_tris)
                    .expect("floor trimesh")
                    .build(),
            );
            let mut player = world.create_player(
                vec3(-6.0, PLAYER_HALF_HEIGHT + 0.1, 0.0),
                EntityId::from_inner(2000).unwrap(),
            );
            // A platform ahead of the player whose top sits at `step_height`.
            world.add_kinematic(
                EntityId::from_inner(2001).unwrap(),
                vec3(-3.0, step_height / 2.0, 0.0),
                identity_quat(),
                Vector3::new(0.0, 0.0, 0.0),
                vec3(4.0, step_height, 4.0),
                CollisionGroup::entity(),
                false,
            );
            // Settle onto the floor, then walk into the step; report the
            // highest point reached (a successful step-up crosses the platform
            // and walks off the far side, so the END height is floor level
            // either way).
            for _ in 0..30 {
                world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
            }
            let start = world.get_player_translation(&player);
            let mut max_y = start.y;
            for _ in 0..240 {
                world.update(Vector3::new(0.05, 0.0, 0.0), &mut player);
                max_y = max_y.max(world.get_player_translation(&player).y);
            }
            max_y - start.y
        };

        let riser = run(0.6); // 1.5 SS2 ft - a typical stair riser
        let ledge = run(1.2); // 3.0 SS2 ft - over the 2 ft step limit

        assert!(
            riser > 0.5,
            "a 1.5 ft riser should be stepped up, rose {riser}"
        );
        assert!(
            ledge < 0.1,
            "a 3 ft ledge must not be auto-stepped, rose {ledge}"
        );
    }

    /// A short legal step must not require the entire two-foot probe height of
    /// overhead clearance. Earth’s intro tram has this arrangement at the
    /// boardwalk threshold: the original six-foot body fits after stepping,
    /// but a full-height preliminary lift brushes the tram ceiling.
    #[test]
    fn player_steps_under_a_ceiling_with_less_than_maximum_probe_headroom() {
        let mut world = PhysicsWorld::new();
        let floor_verts = vec![
            point![-100.0, 0.0, -100.0],
            point![100.0, 0.0, -100.0],
            point![100.0, 0.0, 100.0],
            point![-100.0, 0.0, 100.0],
        ];
        world.add_collider(
            EntityId::from_inner(1000).unwrap(),
            ColliderBuilder::trimesh(floor_verts, vec![[0u32, 1, 2], [0, 2, 3]])
                .expect("floor trimesh")
                .build(),
        );
        let mut player = world.create_player(
            vec3(-3.0, PLAYER_HALF_HEIGHT + 0.1, 0.0),
            EntityId::from_inner(2000).unwrap(),
        );
        // A 0.75 SS2 ft threshold followed by a ceiling that allows about 1.9
        // ft of upward probe from the floor: enough for the real step, less
        // than the controller's full two-foot maximum.
        world.add_kinematic(
            EntityId::from_inner(2001).unwrap(),
            vec3(1.0, 0.15, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(4.0, 0.3, 4.0),
            CollisionGroup::entity(),
            false,
        );
        world.add_kinematic(
            EntityId::from_inner(2002).unwrap(),
            vec3(0.0, 3.3, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(10.0, 0.2, 4.0),
            CollisionGroup::entity(),
            false,
        );

        for _ in 0..30 {
            world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
        }
        let start = world.get_player_translation(&player);
        let mut max_y = start.y;
        for _ in 0..160 {
            world.update(Vector3::new(0.05, 0.0, 0.0), &mut player);
            max_y = max_y.max(world.get_player_translation(&player).y);
        }
        assert!(
            max_y - start.y > 0.25,
            "the player should step onto the threshold under the ceiling, rose {}",
            max_y - start.y
        );
    }

    /// Ordinary jump is a separate, grounded input: it clears a low obstacle
    /// taller than the stair probe without changing automatic step height.
    /// Negative-first: the walking control remains stopped at the 5 ft wall,
    /// and before jump locomotion both runs ended at that same face.
    #[test]
    fn grounded_jump_clears_a_non_climbable_low_obstacle() {
        let run = |jump: bool| -> (Vector3<f32>, f32) {
            let mut world = PhysicsWorld::new();
            world.add_kinematic(
                EntityId::from_inner(1000).unwrap(),
                vec3(0.0, -0.5, 0.0),
                identity_quat(),
                Vector3::new(0.0, 0.0, 0.0),
                vec3(20.0, 1.0, 20.0),
                CollisionGroup::entity(),
                false,
            );
            // 2 world units == 5 SS2 ft: taller than the 2 ft stair probe but
            // comfortably inside the ordinary jump arc.
            world.add_kinematic(
                EntityId::from_inner(1001).unwrap(),
                vec3(0.0, 1.0, 0.0),
                identity_quat(),
                Vector3::new(0.0, 0.0, 0.0),
                vec3(0.4, 2.0, 4.0),
                CollisionGroup::entity(),
                false,
            );
            let mut player = world.create_player(
                vec3(-2.0, PLAYER_HALF_HEIGHT + 0.1, 0.0),
                EntityId::from_inner(2000).unwrap(),
            );
            step(&mut world, &mut player, 30);
            let start = world.get_player_translation(&player);
            let mut max_y = start.y;
            for frame in 0..120 {
                world.update_with_facing_and_jump(
                    vec3(0.1, 0.0, 0.0),
                    Vector3::unit_x(),
                    jump && frame == 0,
                    &mut player,
                );
                max_y = max_y.max(world.get_player_translation(&player).y);
            }
            (world.get_player_translation(&player), max_y - start.y)
        };

        let (walked, walked_rise) = run(false);
        assert!(
            walked.x < -0.3 && walked_rise < 0.1,
            "walking must stay blocked by the over-step-height wall, ended {walked:?}, rose {walked_rise}"
        );

        let (jumped, jumped_rise) = run(true);
        assert!(
            jumped.x > 1.0 && jumped_rise > 2.0,
            "jump should arc over the low wall, ended {jumped:?}, rose {jumped_rise}"
        );
    }

    /// The authored Delacroix-log route in shodan.mis uses a platform whose
    /// walkable top is above the stair probe but still inside the player's
    /// ordinary jump/head reach. Unlike the final-descent lip, its underside
    /// leaves forward walking unobstructed, so the elevated landing probe must
    /// recognize the top rather than waiting for a blocked walk cast.
    #[test]
    fn grounded_jump_mantles_an_elevated_world_platform() {
        let mut world = PhysicsWorld::new();
        world.add_collider(
            EntityId::from_inner(1000).unwrap(),
            ColliderBuilder::cuboid(10.0, 0.5, 10.0)
                .translation(vector![0.0, -0.5, 0.0])
                .build(),
        );
        world.add_collider(
            EntityId::from_inner(1001).unwrap(),
            ColliderBuilder::cuboid(3.0, 1.8, 3.0)
                .translation(vector![1.5, 1.8, 0.0])
                .build(),
        );
        let mut player = world.create_player(
            vec3(-2.0, PLAYER_HALF_HEIGHT + 0.1, 0.0),
            EntityId::from_inner(2000).unwrap(),
        );
        step(&mut world, &mut player, 30);

        for frame in 0..120 {
            world.update_with_facing_and_jump(
                vec3(0.1, 0.0, 0.0),
                Vector3::unit_x(),
                frame == 0,
                &mut player,
            );
        }
        let end = world.get_player_translation(&player);
        assert!(
            end.x > 0.0 && end.y > 4.4,
            "jump should mantle onto the elevated platform, ended {end:?}"
        );
    }

    /// The compressed compatibility body may cross a local terrain lip, but it
    /// must not add vertical reach beyond the ordinary ballistic jump. A roof
    /// above that apex is not a mantle landing even when its top is visible to
    /// the elevated-floor probe (#744).
    #[test]
    fn jump_mantle_rejects_a_platform_above_the_ballistic_apex() {
        let mut world = PhysicsWorld::new();
        world.add_collider(
            EntityId::from_inner(1000).unwrap(),
            ColliderBuilder::cuboid(10.0, 0.5, 10.0)
                .translation(vector![0.0, -0.5, 0.0])
                .build(),
        );
        // Top at y=4.5: 4.4 world units above the player's resting feet,
        // beyond the 3.92-world-unit / 9.8-SS2-foot ballistic apex.
        world.add_collider(
            EntityId::from_inner(1001).unwrap(),
            ColliderBuilder::cuboid(10.0, 2.25, 3.0)
                .translation(vector![8.5, 2.25, 0.0])
                .build(),
        );
        let mut player = world.create_player(
            vec3(-2.0, PLAYER_HALF_HEIGHT + 0.1, 0.0),
            EntityId::from_inner(2000).unwrap(),
        );
        step(&mut world, &mut player, 30);

        for frame in 0..180 {
            world.update_with_facing_and_jump(
                vec3(0.1, 0.0, 0.0),
                Vector3::unit_x(),
                frame == 0,
                &mut player,
            );
        }
        let end = world.get_player_translation(&player);
        assert!(
            end.y < 4.5,
            "an above-apex platform must not become a scripted mantle landing, ended {end:?}"
        );
    }

    /// A parented body can block a corridor without making the parentless room
    /// ceiling above it a valid mantle destination. This is the deterministic
    /// equivalent of the campaign's retained-body choke: the initial rise must
    /// remain collision-checked against world geometry before the narrow lip
    /// exception is allowed for the later horizontal crossing (#744).
    #[test]
    fn dynamic_obstacle_does_not_turn_the_world_ceiling_into_a_mantle() {
        let mut world = PhysicsWorld::new();
        world.add_collider(
            EntityId::from_inner(1000).unwrap(),
            ColliderBuilder::cuboid(10.0, 0.5, 10.0)
                .translation(vector![0.0, -0.5, 0.0])
                .build(),
        );
        // Parentless room slab: underside y=3.6, exterior roof y=3.8. The roof
        // is within the ballistic apex, so the vertical-clearance invariant is
        // what distinguishes it from a genuine elevated platform ahead.
        world.add_collider(
            EntityId::from_inner(1001).unwrap(),
            ColliderBuilder::cuboid(10.0, 0.1, 10.0)
                .translation(vector![0.0, 3.7, 0.0])
                .build(),
        );
        world.add_kinematic(
            EntityId::from_inner(1002).unwrap(),
            vec3(0.0, 1.0, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(0.8, 2.0, 3.0),
            CollisionGroup::entity(),
            false,
        );
        let mut player = world.create_player(
            vec3(-2.0, PLAYER_HALF_HEIGHT + 0.1, 0.0),
            EntityId::from_inner(2000).unwrap(),
        );
        step(&mut world, &mut player, 30);

        for frame in 0..180 {
            world.update_with_facing_and_jump(
                vec3(0.1, 0.0, 0.0),
                Vector3::unit_x(),
                frame == 0,
                &mut player,
            );
        }
        let end = world.get_player_translation(&player);
        assert!(
            end.y < 3.6,
            "an obstacle below a world ceiling must not script the player onto its exterior, ended {end:?}"
        );
    }

    /// Jump remains collision-cast locomotion: it may clear a low barrier but
    /// cannot climb or tunnel through a full-height wall.
    #[test]
    fn jump_still_rejects_full_height_walls() {
        let run = |parented: bool| {
            let mut world = PhysicsWorld::new();
            world.add_kinematic(
                EntityId::from_inner(1000).unwrap(),
                vec3(0.0, -0.5, 0.0),
                identity_quat(),
                Vector3::new(0.0, 0.0, 0.0),
                vec3(20.0, 1.0, 20.0),
                CollisionGroup::entity(),
                false,
            );
            let wall = ColliderBuilder::cuboid(0.2, 5.0, 2.0)
                .translation(vector![0.0, 5.0, 0.0])
                .build();
            if parented {
                world.add_kinematic(
                    EntityId::from_inner(1001).unwrap(),
                    vec3(0.0, 5.0, 0.0),
                    identity_quat(),
                    Vector3::new(0.0, 0.0, 0.0),
                    vec3(0.4, 10.0, 4.0),
                    CollisionGroup::entity(),
                    false,
                );
            } else {
                world.add_collider(EntityId::from_inner(1001).unwrap(), wall);
            }
            let mut player = world.create_player(
                vec3(-2.0, PLAYER_HALF_HEIGHT + 0.1, 0.0),
                EntityId::from_inner(2000).unwrap(),
            );
            step(&mut world, &mut player, 30);
            for frame in 0..180 {
                world.update_with_facing_and_jump(
                    vec3(0.1, 0.0, 0.0),
                    Vector3::unit_x(),
                    frame == 0,
                    &mut player,
                );
            }
            world.get_player_translation(&player)
        };

        for (kind, end) in [
            ("parented entity", run(true)),
            ("world terrain", run(false)),
        ] {
            assert!(
                end.x < -0.3,
                "jump must remain blocked by a full-height {kind} wall, ended {end:?}"
            );
        }
    }

    /// A held jump button is one impulse, not repeated upward thrust, and a
    /// real low ceiling clips that impulse through the ordinary shape cast.
    #[test]
    fn held_jump_does_not_repeat_and_ceiling_blocks_rise() {
        let run = |ceiling: bool| -> f32 {
            let mut world = PhysicsWorld::new();
            world.add_kinematic(
                EntityId::from_inner(1000).unwrap(),
                vec3(0.0, -0.5, 0.0),
                identity_quat(),
                Vector3::new(0.0, 0.0, 0.0),
                vec3(20.0, 1.0, 20.0),
                CollisionGroup::entity(),
                false,
            );
            if ceiling {
                world.add_kinematic(
                    EntityId::from_inner(1001).unwrap(),
                    vec3(0.0, 2.8, 0.0),
                    identity_quat(),
                    Vector3::new(0.0, 0.0, 0.0),
                    vec3(6.0, 0.4, 6.0),
                    CollisionGroup::entity(),
                    false,
                );
            }
            let mut player = world.create_player(
                vec3(0.0, PLAYER_HALF_HEIGHT + 0.1, 0.0),
                EntityId::from_inner(2000).unwrap(),
            );
            step(&mut world, &mut player, 30);
            let start = world.get_player_translation(&player);
            let mut max_y = start.y;
            for _ in 0..240 {
                world.update_with_facing_and_jump(
                    Vector3::new(0.0, 0.0, 0.0),
                    Vector3::unit_x(),
                    true,
                    &mut player,
                );
                max_y = max_y.max(world.get_player_translation(&player).y);
            }
            max_y - start.y
        };

        let open_rise = run(false);
        assert!(
            open_rise > 2.0 && open_rise < 5.0,
            "one held press should produce one bounded hop, rose {open_rise}"
        );
        let capped_rise = run(true);
        assert!(
            capped_rise < 0.15,
            "a low ceiling must clip the jump cast, rose {capped_rise}"
        );
    }

    /// Releasing and pressing again while airborne must not reset the vertical
    /// velocity to a fresh launch. This covers both rapid tapping and a held
    /// controller that bounces around its threshold during one arc.
    #[test]
    fn airborne_jump_edges_do_not_relaunch() {
        let mut world = PhysicsWorld::new();
        world.add_kinematic(
            EntityId::from_inner(1000).unwrap(),
            vec3(0.0, -0.5, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(20.0, 1.0, 20.0),
            CollisionGroup::entity(),
            false,
        );
        let mut player = world.create_player(
            vec3(0.0, PLAYER_HALF_HEIGHT + 0.1, 0.0),
            EntityId::from_inner(2000).unwrap(),
        );
        step(&mut world, &mut player, 30);

        world.update_with_facing_and_jump(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::unit_x(),
            true,
            &mut player,
        );
        for _ in 0..5 {
            world.update_with_facing_and_jump(
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::unit_x(),
                false,
                &mut player,
            );
        }
        let before_second_edge = player
            .jump_velocity
            .expect("the first jump should still be airborne");
        world.update_with_facing_and_jump(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::unit_x(),
            true,
            &mut player,
        );
        let after_second_edge = player
            .jump_velocity
            .expect("an ignored airborne edge should leave the arc active");
        assert!(
            after_second_edge < before_second_edge,
            "an airborne press must keep consuming gravity, not relaunch ({before_second_edge} -> {after_second_edge})"
        );
    }

    /// A crouched player may hop without ever expanding to the standing
    /// profile. An ordinary blocked hop remains ballistic, keeps the crouched
    /// collider, and can return to the standing capsule once grounded in open
    /// headroom.
    #[test]
    fn crouched_jump_preserves_collider_state_and_can_stand_after_landing() {
        let mut world = PhysicsWorld::new();
        world.add_kinematic(
            EntityId::from_inner(1000).unwrap(),
            vec3(0.0, -0.5, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(20.0, 1.0, 20.0),
            CollisionGroup::entity(),
            false,
        );
        world.add_kinematic(
            EntityId::from_inner(1001).unwrap(),
            vec3(0.0, 1.0, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(0.4, 2.0, 4.0),
            CollisionGroup::entity(),
            false,
        );
        let mut player = world.create_player(
            vec3(-2.0, PLAYER_HALF_HEIGHT + 0.1, 0.0),
            EntityId::from_inner(2000).unwrap(),
        );
        step(&mut world, &mut player, 30);
        assert!(world.set_player_crouch(true, &mut player));

        world.update_with_facing_and_jump(
            vec3(0.1, 0.0, 0.0),
            Vector3::unit_x(),
            true,
            &mut player,
        );
        assert!(
            player.top_out.is_none() && player.is_crouched(),
            "this blocked crouched jump should stay ballistic and keep the crouched collider"
        );
        let collider_handle = world.rigid_body_set[player.character_handle].colliders()[0];
        let crouched = world.collider_set[collider_handle]
            .shape()
            .as_capsule()
            .expect("player collider should remain a capsule");
        assert!(
            (2.0 * (crouched.half_height() + crouched.radius)
                - PLAYER_CROUCH_HEIGHT / SCALE_FACTOR)
                .abs()
                < 1.0e-4,
            "jump must not expand the crouched capsule"
        );

        for _ in 0..240 {
            world.update_with_facing_and_jump(
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::unit_x(),
                false,
                &mut player,
            );
        }
        assert!(
            !world.set_player_crouch(false, &mut player),
            "a landed crouched player in open headroom should stand normally"
        );
    }

    /// Build a floor whose top face is flush with the bottom of a walk-in
    /// fixture's model-bounds box - hydro2's Resurrection Station casing sits
    /// on the alcove floor exactly like this - and return the world plus the
    /// y a standing capsule rests at on that floor.
    fn frob_box_fixture(solid: bool) -> (PhysicsWorld, f32) {
        let mut world = PhysicsWorld::new();
        // Floor: 40x40, 1 thick, centred on the origin -> top face at y = 0.5.
        world.add_kinematic(
            EntityId::from_inner(1001).unwrap(),
            vec3(0.0, 0.0, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(40.0, 1.0, 40.0),
            CollisionGroup::entity(),
            false,
        );
        // A wall, standing on the floor with its +x face at x = 0.5 - the
        // alcove's authored step, which the hydro2 wedge also grazed.
        world.add_kinematic(
            EntityId::from_inner(1002).unwrap(),
            vec3(-0.5, 2.5, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(2.0, 4.0, 20.0),
            CollisionGroup::entity(),
            false,
        );
        // The casing, at the Resurrection Station's real model-bounds extents,
        // standing on the floor: x in [0.9, 3.1], y in [0.5, 4.5], z in
        // [-1.25, 1.25]. That leaves a 0.4-wide slot against the wall - half a
        // capsule diameter - so a pose inside it is pinned between two
        // opposing faces with no direction left to resolve into.
        let casing = CollisionGroup::entity();
        world.add_kinematic(
            EntityId::from_inner(1003).unwrap(),
            vec3(2.0, 2.5, 0.0),
            Quaternion::new(1.0, 0.0, 0.0, 0.0),
            vec3(0.0, 0.0, 0.0),
            vec3(2.2, 4.0, 2.5),
            if solid {
                casing
            } else {
                casing.non_solid_to_characters()
            },
            false,
        );
        (world, 1.7)
    }

    /// Furthest the player gets from `start` over `HEADINGS` compass
    /// directions of ordinary locomotion input - the same probe the issue used
    /// against the live level.
    fn best_walk_from(solid: bool, start: Vector3<f32>) -> f32 {
        const HEADINGS: usize = 8;
        let mut best: f32 = 0.0;
        for heading in 0..HEADINGS {
            let angle = std::f32::consts::TAU * heading as f32 / HEADINGS as f32;
            let (mut world, _) = frob_box_fixture(solid);
            let mut player = world.create_player(start, EntityId::from_inner(2000).unwrap());
            for _ in 0..30 {
                world.update(vec3(0.0, 0.0, 0.0), &mut player);
            }
            let from = world.get_player_translation(&player);
            let step = vec3(angle.cos() * 0.05, 0.0, angle.sin() * 0.05);
            for _ in 0..120 {
                world.update(step, &mut player);
            }
            let to = world.get_player_translation(&player);
            best = best.max(((to.x - from.x).powi(2) + (to.z - from.z).powi(2)).sqrt());
        }
        best
    }

    /// A walk-in fixture's model-bounds box - built only so the object can be
    /// frobbed, for an object Dark never gives a physics model - must not be
    /// solid to the player. While it was, a capsule overlapping the corner of
    /// two of its faces had no way out: the character controller has no
    /// depenetration pass, so every cast returns toi=0 and the resolved
    /// translation is exactly zero in every direction (hydro2's Resurrection
    /// Station alcove, #801).
    ///
    /// Negative-first: the `solid` half is the bug and still freezes; only the
    /// `non_solid_to_characters` half changes behavior.
    #[test]
    fn a_non_solid_frob_box_never_wedges_the_player() {
        let (_, stand_y) = frob_box_fixture(true);
        // Poses inside the slot, the way the reported wedge sat between the
        // casing and the alcove's step.
        let starts = [
            vec3(0.75, stand_y, 0.0),
            vec3(0.85, stand_y, 0.0),
            vec3(0.85, stand_y, 0.6),
        ];

        let worst_when_solid = starts
            .iter()
            .map(|start| best_walk_from(true, *start))
            .fold(f32::INFINITY, f32::min);
        assert!(
            worst_when_solid < 0.25,
            "at least one slot pose should still be pinned by a solid frob box, best walk over all of them was {worst_when_solid}"
        );

        for start in starts {
            let walked = best_walk_from(false, start);
            assert!(
                walked > 1.0,
                "a non-solid frob box must leave {start:?} walkable, moved {walked}"
            );
        }
    }

    /// An authored placement (a QBR marker, a teleport trap) is used verbatim
    /// when the standing capsule fits, and only steps aside when it does not.
    /// An obstructed placement is unrecoverable otherwise - see the wedge
    /// above - so reconstruction would end the run outright (#801).
    #[test]
    fn an_obstructed_authored_placement_steps_aside() {
        let (mut world, stand_y) = frob_box_fixture(true);
        let mut player = world.create_player(
            vec3(-8.0, stand_y, 0.0),
            EntityId::from_inner(2000).unwrap(),
        );
        for _ in 0..10 {
            world.update(vec3(0.0, 0.0, 0.0), &mut player);
        }

        let free = vec3(-4.0, stand_y, 0.0);
        assert_eq!(
            world.set_player_translation_unobstructed(free, &mut player),
            free,
            "a free authored placement must be used exactly, never nudged"
        );

        let obstructed = vec3(2.0, stand_y, 0.0);
        assert!(
            !world.standing_player_pose_is_clear(obstructed, &player),
            "the fixture interior should be obstructed for the standing capsule"
        );
        let placed = world.set_player_translation_unobstructed(obstructed, &mut player);
        assert_ne!(placed, obstructed);
        assert!(
            world.standing_player_pose_is_clear(placed, &player),
            "reconstruction must land somewhere the standing capsule fits, got {placed:?}"
        );

        // The casing is still solid in this fixture, so escape is measured the
        // way the issue measured it: the best of eight compass headings.
        let mut walked: f32 = 0.0;
        for heading in 0..8 {
            let angle = std::f32::consts::TAU * heading as f32 / 8.0;
            world.set_player_translation(placed, &mut player);
            for _ in 0..30 {
                world.update(vec3(0.0, 0.0, 0.0), &mut player);
            }
            let from = world.get_player_translation(&player);
            let step = vec3(angle.cos() * 0.05, 0.0, angle.sin() * 0.05);
            for _ in 0..120 {
                world.update(step, &mut player);
            }
            let to = world.get_player_translation(&player);
            walked = walked.max(((to.x - from.x).powi(2) + (to.z - from.z).powi(2)).sqrt());
        }
        assert!(
            walked > 1.0,
            "a reconstructed player must be able to walk away, moved {walked}"
        );
    }

    /// Walking along the top of a yaw-ROTATED kinematic cuboid (e.g. the
    /// earth.mis tram floor slab) must make progress. The rotated top-face
    /// normal carries ~1e-6 float error, so tangential movement casts read as
    /// "approaching" and re-hit the resting contact at toi=0 every solver
    /// iteration and the player freezes in place after a frame or two.
    /// (Negative-first: fails without `PLAYER_REST_LIFT`; an AXIS-ALIGNED
    /// slab passes either way because its exact (0,1,0) normal reports no hit
    /// for tangential motion.)
    #[test]
    fn player_walks_on_rotated_platform() {
        use cgmath::Rotation3;
        let mut world = PhysicsWorld::new();
        // A tram-slab-like platform, yawed 90 degrees: authored 4 wide x 10
        // long, so after rotation its long axis lies along world x.
        world.add_kinematic(
            EntityId::from_inner(1001).unwrap(),
            vec3(0.0, 1.0, 0.0),
            Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), cgmath::Deg(90.0)),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(4.0, 0.4, 10.0),
            CollisionGroup::entity(),
            false,
        );
        let mut player =
            world.create_player(vec3(-3.0, 3.0, 0.0), EntityId::from_inner(2000).unwrap());
        // Settle onto the slab, then walk along it.
        for _ in 0..30 {
            world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
        }
        let start = world.get_player_translation(&player);
        for _ in 0..120 {
            world.update(Vector3::new(0.05, 0.0, 0.0), &mut player);
        }
        let end = world.get_player_translation(&player);
        let walked = end.x - start.x;
        assert!(
            walked > 3.0,
            "walking on a rotated platform should progress ~6 units, moved {walked}"
        );
    }

    /// A stationary player supported by a moving kinematic body must inherit
    /// that body's displacement. This is the generic moving-terrain behavior
    /// used by authored lifts and trams: every PhysAttach collision part moves
    /// with the support, and the passenger remains aboard without supplying
    /// locomotion input.
    #[test]
    fn stationary_player_is_carried_by_moving_kinematic_support() {
        use cgmath::Rotation3;

        let run = |attach_front_wall: bool| {
            let mut world = PhysicsWorld::new();
            let support_id = EntityId::from_inner(1001).unwrap();
            let wall_id = EntityId::from_inner(1002).unwrap();
            let support = world.add_kinematic(
                support_id,
                vec3(0.0, 1.0, 0.0),
                Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), cgmath::Deg(90.0)),
                Vector3::new(0.0, 0.0, 0.0),
                vec3(4.0, 0.4, 10.0),
                CollisionGroup::entity(),
                false,
            );
            // A thin front wall crossing the direction of travel, like
            // command1's Tram Front. Left stationary it blocks the passenger
            // at its contact boundary while the floor moves out underneath.
            let wall = world.add_kinematic(
                wall_id,
                vec3(4.0, 2.6, 0.0),
                Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), cgmath::Deg(90.0)),
                Vector3::new(0.0, 0.0, 0.0),
                vec3(3.0, 3.2, 0.4),
                CollisionGroup::entity(),
                false,
            );
            if attach_front_wall {
                assert!(world.attach_kinematic(wall_id, support_id, vec3(4.0, 1.6, 0.0)));
            }
            let mut player =
                world.create_player(vec3(0.0, 3.0, 0.0), EntityId::from_inner(2000).unwrap());

            for _ in 0..30 {
                world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
            }
            let player_start = world.get_player_translation(&player);
            let support_start = world.get_position(support).unwrap();
            let wall_start = world.get_position(wall).unwrap();

            for frame in 1..=60 {
                world.set_translation(support, support_start + vec3(frame as f32 * 0.2, 0.0, 0.0));
                world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
            }

            let player_end = world.get_player_translation(&player);
            let support_end = world.get_position(support).unwrap();
            let wall_end = world.get_position(wall).unwrap();
            (
                player_end.x - player_start.x,
                support_end.x - support_start.x,
                wall_end.x - wall_start.x,
            )
        };

        let (blocked_player, _, stationary_wall) = run(false);
        assert!(
            blocked_player < 4.0 && stationary_wall.abs() < 0.01,
            "without PhysAttach propagation the wall should strand the passenger: player {blocked_player}, wall {stationary_wall}"
        );

        let (player_displacement, support_displacement, wall_displacement) = run(true);
        assert!(
            support_displacement > 11.9,
            "test support should move twelve units, moved {support_displacement}"
        );
        assert!(
            (wall_displacement - support_displacement).abs() < 0.01,
            "attached wall must match support displacement: wall {wall_displacement}, support {support_displacement}"
        );
        assert!(
            player_displacement > 10.0 && (player_displacement - support_displacement).abs() < 1.5,
            "stationary passenger should remain aboard the moving support: player {player_displacement}, support {support_displacement}"
        );
    }

    #[test]
    fn sensor_transitions_report_exits_before_entries() {
        let player_id = EntityId::from_inner(1).unwrap();
        let left = EntityId::from_inner(2).unwrap();
        let entered = EntityId::from_inner(3).unwrap();
        let previous = HashSet::from([left]);
        let current = HashSet::from([entered]);

        let events = sensor_transition_events(&previous, &current, player_id);

        assert_eq!(events.len(), 2, "one exit and one entry: {events:?}");
        assert!(
            matches!(events[0], CollisionEvent::EndIntersect { sensor_id, .. } if sensor_id == left),
            "the exit must be reported first: {events:?}"
        );
        assert!(
            matches!(events[1], CollisionEvent::BeginIntersect { sensor_id, .. } if sensor_id == entered),
            "the entry must follow the exit: {events:?}"
        );
        assert!(
            sensor_transition_events(&previous, &previous, player_id).is_empty(),
            "unchanged occupancy is not an edge"
        );
    }

    /// A validated move (`/v1/player/move`, the automation navigation
    /// primitive) must fire the sensors it crosses, like walking does.
    /// (Negative-first: the hop was committed as one atomic translation and
    /// sensors were only polled once per frame, so a volume entered and left
    /// between two frames fired nothing at all - automated playtests walked
    /// straight through tripwires, issue #654.)
    #[test]
    fn validated_move_fires_sensors_crossed_along_the_hop() {
        let sensor_id = EntityId::from_inner(3000).unwrap();
        // Settle the player at x=-2, hop toward `target_x` through a thin
        // sensor slab standing at x=0, then take one frame; report the sensor
        // events that frame delivered.
        let run = |target_x: f32| -> Vec<CollisionEvent> {
            let mut world = PhysicsWorld::new();
            let floor_verts = vec![
                point![-100.0, 0.0, -100.0],
                point![100.0, 0.0, -100.0],
                point![100.0, 0.0, 100.0],
                point![-100.0, 0.0, 100.0],
            ];
            let floor_tris = vec![[0u32, 1, 2], [0, 2, 3]];
            world.add_collider(
                EntityId::from_inner(1000).unwrap(),
                ColliderBuilder::trimesh(floor_verts, floor_tris)
                    .expect("floor trimesh")
                    .build(),
            );
            world.add_collider(
                sensor_id,
                ColliderBuilder::cuboid(0.15, 2.0, 4.0)
                    .translation(vector![0.0, 1.0, 0.0])
                    .sensor(true)
                    .build(),
            );
            let mut player =
                world.create_player(vec3(-2.0, 1.0, 0.0), EntityId::from_inner(2000).unwrap());
            for _ in 0..30 {
                world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
            }
            let current = world.get_player_translation(&player);
            world.move_player_validated(vec3(target_x, current.y, current.z), &mut player);
            let (_, events) = world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
            // The hop is the only thing that moved: a second idle frame must
            // not replay anything the hop already reported.
            let (_, extra) = world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
            assert!(
                extra.is_empty(),
                "a stationary follow-up frame must not re-fire: {extra:?}"
            );
            events
        };

        // Passing through: enter then exit, in traversal order.
        let crossed = run(2.0);
        assert_eq!(
            crossed.len(),
            2,
            "crossing a sensor must fire enter then exit: {crossed:?}"
        );
        assert!(
            matches!(crossed[0], CollisionEvent::BeginIntersect { sensor_id: id, .. } if id == sensor_id),
            "the crossing must enter first: {crossed:?}"
        );
        assert!(
            matches!(crossed[1], CollisionEvent::EndIntersect { sensor_id: id, .. } if id == sensor_id),
            "the crossing must then exit: {crossed:?}"
        );

        // Ending inside: exactly one enter, and no duplicate from the
        // per-frame poll that used to be the only thing reporting it.
        let ended_inside = run(0.0);
        assert_eq!(
            ended_inside.len(),
            1,
            "a hop ending inside a sensor fires one enter: {ended_inside:?}"
        );
        assert!(
            matches!(ended_inside[0], CollisionEvent::BeginIntersect { sensor_id: id, .. } if id == sensor_id),
            "a hop ending inside a sensor must enter it: {ended_inside:?}"
        );
    }

    /// The cached occupancy is only as fresh as the last stepped frame, so a
    /// teleport with no frame in between can leave the player standing in a
    /// volume the cache has never seen. The sweep therefore samples its own
    /// starting pose before walking; without that, a hop whose first substep
    /// already leaves the volume reports neither the entry nor the exit.
    #[test]
    fn validated_move_reports_a_volume_the_player_was_teleported_into() {
        let sensor_id = EntityId::from_inner(3000).unwrap();
        let mut world = PhysicsWorld::new();
        let floor_verts = vec![
            point![-100.0, 0.0, -100.0],
            point![100.0, 0.0, -100.0],
            point![100.0, 0.0, 100.0],
            point![-100.0, 0.0, 100.0],
        ];
        let floor_tris = vec![[0u32, 1, 2], [0, 2, 3]];
        world.add_collider(
            EntityId::from_inner(1000).unwrap(),
            ColliderBuilder::trimesh(floor_verts, floor_tris)
                .expect("floor trimesh")
                .build(),
        );
        world.add_collider(
            sensor_id,
            ColliderBuilder::cuboid(0.15, 2.0, 4.0)
                .translation(vector![0.0, 1.0, 0.0])
                .sensor(true)
                .build(),
        );
        let mut player =
            world.create_player(vec3(-2.0, 1.0, 0.0), EntityId::from_inner(2000).unwrap());
        for _ in 0..30 {
            world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
        }

        // Land just inside the far edge of the slab (the overlap band reaches
        // 0.47 with the capsule's radius), so the very first walk substep
        // already carries the player out of it.
        let settled = world.get_player_translation(&player);
        world.set_player_translation(vec3(0.4, settled.y, settled.z), &mut player);
        world.move_player_validated(vec3(2.0, settled.y, settled.z), &mut player);
        let (_, events) = world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);

        assert_eq!(
            events.len(),
            2,
            "the teleported-into volume must report entry then exit: {events:?}"
        );
        assert!(
            matches!(events[0], CollisionEvent::BeginIntersect { sensor_id: id, .. } if id == sensor_id),
            "the entry established by the teleport must be reported: {events:?}"
        );
        assert!(
            matches!(events[1], CollisionEvent::EndIntersect { sensor_id: id, .. } if id == sensor_id),
            "walking out of it must then report the exit: {events:?}"
        );
    }

    /// A validated move (`/v1/player/move`, the automation navigation
    /// primitive) must traverse whatever real walking traverses: it steps up
    /// onto a stair-sized riser, while a full-height wall still reports
    /// `blocked` and stops the player in front of it.
    /// (Negative-first: with the old single shape-cast implementation the
    /// riser produced a time-of-impact of ~0, so the call returned
    /// `blocked: true, distance_moved: 0` on walkable ground - issue #559.)
    #[test]
    fn validated_move_steps_up_stairs_but_not_walls() {
        // Walk +x into an obstacle `obstacle_height` tall using validated
        // moves only; report (x advanced, y gained, any call reported blocked).
        let run = |obstacle_height: f32| -> (f32, f32, bool) {
            let mut world = PhysicsWorld::new();
            // Floor top at y=0 (parentless trimesh, like level geometry).
            let floor_verts = vec![
                point![-100.0, 0.0, -100.0],
                point![100.0, 0.0, -100.0],
                point![100.0, 0.0, 100.0],
                point![-100.0, 0.0, 100.0],
            ];
            let floor_tris = vec![[0u32, 1, 2], [0, 2, 3]];
            world.add_collider(
                EntityId::from_inner(1000).unwrap(),
                ColliderBuilder::trimesh(floor_verts, floor_tris)
                    .expect("floor trimesh")
                    .build(),
            );
            let mut player =
                world.create_player(vec3(-6.0, 1.0, 0.0), EntityId::from_inner(2000).unwrap());
            // An obstacle ahead of the player (x from -5 to -1) whose top sits
            // at `obstacle_height`.
            world.add_kinematic(
                EntityId::from_inner(2001).unwrap(),
                vec3(-3.0, obstacle_height / 2.0, 0.0),
                identity_quat(),
                Vector3::new(0.0, 0.0, 0.0),
                vec3(4.0, obstacle_height, 4.0),
                CollisionGroup::entity(),
                false,
            );
            // Settle onto the floor, then advance with validated moves alone -
            // no `update`, so only `move_player_validated` moves the player.
            for _ in 0..30 {
                world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
            }
            let start = world.get_player_translation(&player);
            let mut blocked_any = false;
            let mut max_y = start.y;
            for _ in 0..4 {
                let current = world.get_player_translation(&player);
                // Leave enough horizontal budget for the capsule axis to plant
                // on the tread. A shorter target can end before any valid
                // standing pose on top of the riser and must remain bounded
                // rather than overshoot that target.
                let result =
                    world.move_player_validated(current + vec3(1.5, 0.0, 0.0), &mut player);
                blocked_any |= result.blocked;
                max_y = max_y.max(result.new_position.y);
            }
            let end = world.get_player_translation(&player);
            (end.x - start.x, max_y - start.y, blocked_any)
        };

        // 1.5 SS2 ft (0.6 wu) - a typical stair riser: walkable, so validated
        // moves must climb it and cross the platform.
        let (riser_x, riser_y, riser_blocked) = run(0.6);
        assert!(
            riser_x > 5.0,
            "a validated move should walk over a 1.5 ft riser, advanced {riser_x}"
        );
        assert!(
            riser_y > 0.4,
            "a validated move over a riser should gain its height, rose {riser_y}"
        );
        assert!(
            !riser_blocked,
            "walkable ground must not report blocked (issue #559)"
        );

        // A full-height wall: the safety property - the player stops in front
        // of it (the capsule's own radius is the only advance) and never
        // passes through.
        let (wall_x, _, wall_blocked) = run(10.0);
        assert!(
            wall_blocked,
            "a solid wall must still report blocked, advanced {wall_x}"
        );
        assert!(
            wall_x < 0.8,
            "a validated move must not pass into a wall, advanced {wall_x}"
        );
    }

    /// Sliding along a wall changes the controller's route. Progress projected
    /// only onto the requested heading can therefore consume the whole request
    /// while the net translation travels much farther; the hop must remain
    /// inside its requested horizontal radius (issue #599).
    #[test]
    fn validated_move_stays_bounded_while_sliding_along_a_wall() {
        let mut world = PhysicsWorld::new();
        let floor_verts = vec![
            point![-100.0, 0.0, -100.0],
            point![100.0, 0.0, -100.0],
            point![100.0, 0.0, 100.0],
            point![-100.0, 0.0, 100.0],
        ];
        world.add_collider(
            EntityId::from_inner(1000).unwrap(),
            ColliderBuilder::trimesh(floor_verts, vec![[0u32, 1, 2], [0, 2, 3]])
                .expect("floor trimesh")
                .build(),
        );
        // Near face one capsule radius + controller offset to -x; long in z
        // so the diagonal request can only slide along it, never round an end.
        world.add_kinematic(
            EntityId::from_inner(1001).unwrap(),
            vec3(-1.0, 2.0, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(1.28, 4.0, 100.0),
            CollisionGroup::entity(),
            false,
        );
        let mut player =
            world.create_player(vec3(0.0, 1.0, 0.0), EntityId::from_inner(2000).unwrap());
        for _ in 0..30 {
            world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
        }

        let start = world.get_player_translation(&player);
        let direction = vec3(-0.5, 0.0, -f32::sqrt(3.0) / 2.0);
        let result = world.move_player_validated(start + direction * 3.0, &mut player);
        let translated = result.new_position - start;

        assert!(
            result.blocked,
            "the wall should keep the target unreachable"
        );
        assert!(
            vec3(translated.x, 0.0, translated.z).magnitude()
                <= result.requested_distance + PLAYER_MOVE_ARRIVAL_EPSILON,
            "validated movement must remain inside its requested radius: {result:?}"
        );
    }

    #[test]
    fn validated_move_rewinds_a_compressed_top_out_before_walking() {
        let mut world = PhysicsWorld::new();
        let floor = ColliderBuilder::cuboid(10.0, 0.05, 10.0)
            .translation(vector![0.0, -0.55, 0.0])
            .build();
        world.add_collider(EntityId::from_inner(1000).unwrap(), floor);
        world.add_kinematic(
            EntityId::from_inner(1001).unwrap(),
            vec3(0.5, 1.1, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(2.0, 0.2, 2.0),
            CollisionGroup::entity(),
            false,
        );
        let mut player = world.create_player(
            vec3(100.0, 100.0, 100.0),
            EntityId::from_inner(2000).unwrap(),
        );
        world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);

        let compressed_pose = vector![0.0, 0.5, 0.0];
        let save_pose = vector![-3.0, PLAYER_HALF_HEIGHT - 0.5 + 0.1, 0.0];
        world.set_player_translation(nvec_to_cgmath(compressed_pose), &mut player);
        let collider_handle = world.rigid_body_set[player.character_handle].colliders()[0];
        world.collider_set[collider_handle].set_shape(SharedShape::ball(CLIMB_TOP_OUT_RADIUS));
        world.rigid_body_set[player.character_handle].enable_ccd(false);
        player.top_out = Some(ClimbTopOut {
            waypoints: [compressed_pose; 7],
            next_waypoint: 3,
            save_pose,
            reversing: false,
            is_crouched: false,
        });

        let result = world.move_player_validated(vec3(f32::NAN, 0.5, 0.0), &mut player);
        let standing = standing_player_capsule();
        let queries = query_pipeline(
            &world,
            QueryFilter::default().exclude_rigid_body(player.character_handle),
        );

        assert!(
            player.top_out.is_none(),
            "validated movement must cancel transient top-out state"
        );
        assert!(
            result.moved,
            "rewinding a transient top-out must be observable by callers even when no walk follows"
        );
        assert!(
            !shape_intersects(&queries, vec_to_nvec(result.new_position), &standing,),
            "validated movement must never expand the standing capsule at a ball-only pose"
        );
    }

    /// A player FALLING beside a wall, asked to move into it toward a target
    /// below them, must still report `blocked` and gain no ground: falling is
    /// not progress. (Negative-first: when the move walked along the full 3D
    /// direction to the target and measured progress along it, the gravity
    /// translation projected onto that downward-tilted direction and passed the
    /// progress threshold on its own - a walled player reported
    /// `blocked: false` with several units of "distance moved".)
    #[test]
    fn validated_move_into_wall_while_falling_reports_blocked() {
        let mut world = PhysicsWorld::new();
        // A tall wall spanning x from -5 to -1, over a bottomless drop (no
        // floor), so the player keeps falling for the whole move.
        world.add_kinematic(
            EntityId::from_inner(2001).unwrap(),
            vec3(-3.0, 0.0, 0.0),
            identity_quat(),
            Vector3::new(0.0, 0.0, 0.0),
            vec3(4.0, 200.0, 4.0),
            CollisionGroup::entity(),
            false,
        );
        // The player in mid-air right against the wall's face.
        let mut player =
            world.create_player(vec3(-5.5, 5.0, 0.0), EntityId::from_inner(2000).unwrap());

        // A few frames so the wall collider enters the broad-phase BVH the
        // movement queries run against (colliders are only inserted on a step).
        for _ in 0..5 {
            world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
        }
        let start = world.get_player_translation(&player);
        let mut blocked_all = true;
        let mut reported_distance = 0.0;
        for _ in 0..4 {
            // A steeply DOWNWARD target: the only walkable direction toward it
            // is +x (into the wall), and the drop is what the old 3D progress
            // metric mistook for progress toward the target.
            let result = world.move_player_validated(vec3(3.0, -20.0, 0.0), &mut player);
            blocked_all &= result.blocked;
            reported_distance += result.distance_moved;
        }
        let end = world.get_player_translation(&player);

        assert!(
            blocked_all,
            "moving into a wall while falling must report blocked"
        );
        assert!(
            reported_distance < 0.4,
            "falling must not be reported as distance toward the target, reported {reported_distance}"
        );
        assert!(
            end.x - start.x < 0.4,
            "a falling player must not gain ground against a wall, advanced {}",
            end.x - start.x
        );
        assert!(
            end.y < start.y,
            "the player should still fall while blocked, y went {} -> {}",
            start.y,
            end.y
        );
    }

    /// A low ceiling must still veto a step-up. The issue #499 fix narrows the
    /// capsule used for the headroom sweep so surfaces beside the player stop
    /// masking it; this guards that the sweep still rejects a real ceiling
    /// (its height is deliberately not narrowed).
    #[test]
    fn player_does_not_step_up_under_low_ceiling() {
        // Height gained walking +x into a 0.6 riser, with a ceiling `gap`
        // above the player's head (or none at all).
        let run = |ceiling: Option<f32>| -> f32 {
            let mut world = PhysicsWorld::new();
            let floor_verts = vec![
                point![-100.0, 0.0, -100.0],
                point![100.0, 0.0, -100.0],
                point![100.0, 0.0, 100.0],
                point![-100.0, 0.0, 100.0],
            ];
            let floor_tris = vec![[0u32, 1, 2], [0, 2, 3]];
            world.add_collider(
                EntityId::from_inner(1000).unwrap(),
                ColliderBuilder::trimesh(floor_verts, floor_tris)
                    .expect("floor trimesh")
                    .build(),
            );
            let mut player = world.create_player(
                vec3(-6.0, PLAYER_HALF_HEIGHT + 0.1, 0.0),
                EntityId::from_inner(2000).unwrap(),
            );
            world.add_kinematic(
                EntityId::from_inner(2001).unwrap(),
                vec3(-3.0, 0.3, 0.0),
                identity_quat(),
                Vector3::new(0.0, 0.0, 0.0),
                vec3(4.0, 0.6, 4.0),
                CollisionGroup::entity(),
                false,
            );
            if let Some(y) = ceiling {
                world.add_kinematic(
                    EntityId::from_inner(2002).unwrap(),
                    vec3(-3.0, y, 0.0),
                    identity_quat(),
                    Vector3::new(0.0, 0.0, 0.0),
                    vec3(4.0, 0.4, 4.0),
                    CollisionGroup::entity(),
                    false,
                );
            }
            for _ in 0..30 {
                world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
            }
            let start = world.get_player_translation(&player);
            let mut max_y = start.y;
            for _ in 0..240 {
                world.update(Vector3::new(0.05, 0.0, 0.0), &mut player);
                max_y = max_y.max(world.get_player_translation(&player).y);
            }
            max_y - start.y
        };

        // Sanity: with open sky above, the 0.6 riser is climbed.
        let open = run(None);
        assert!(
            open > 0.5,
            "a 1.5 ft riser should be stepped up, rose {open}"
        );
        // A ceiling slab just above the riser leaves nowhere to stand.
        let capped = run(Some(1.5));
        assert!(
            capped < 0.1,
            "a step under a low ceiling must be refused, rose {capped}"
        );
    }

    // ---- mission-derived regression (issue #499) ----

    /// Load a real mission's level geometry, headlessly and GL-free. Returns
    /// `None` when the game data is not present (the repo ships a placeholder
    /// `Data/`), so this is a no-op on CI but a real check locally.
    fn try_load_level(mission: &str) -> Option<dark::mission::SystemShock2Level> {
        use crate::zip_asset_path::ZipAssetPath;
        use engine::assets::asset_paths::AssetPath;
        use std::fs::File;
        use std::io::BufReader;

        let mission_path = crate::resource_path(mission);
        if !std::path::Path::new(&mission_path).exists() {
            eprintln!("skipping: {mission} not found (no game data)");
            return None;
        }
        let asset_paths = AssetPath::combine(vec![
            AssetPath::folder(crate::resource_path("res/mesh")),
            AssetPath::folder(crate::resource_path("res/obj")),
            ZipAssetPath::new(crate::resource_path("res/obj.crf")),
            ZipAssetPath::new(crate::resource_path("res/bitmap.crf")),
            ZipAssetPath::new(crate::resource_path("res/fam.crf")),
            AssetPath::folder("".to_owned()),
        ]);
        let base_path = crate::paths::data_root().to_string_lossy().into_owned();
        let (properties, links, links_with_data) = dark::properties::get();
        let mut game_reader = BufReader::new(File::open(crate::resource_path("shock2.gam")).ok()?);
        let gamesys = dark::gamesys::read(&mut game_reader, &links, &links_with_data, &properties);
        let mut reader = BufReader::new(File::open(&mission_path).ok()?);
        Some(dark::mission::read(
            asset_paths.as_ref(),
            &base_path,
            &mut reader,
            &gamesys,
            &links,
            &links_with_data,
            &properties,
        ))
    }

    /// The authored MedSci corridor west of keypad 45100 has exactly five SS2
    /// feet between its floor and ceiling. The original six-foot standing
    /// profile must stop at the ceiling face around x=-26.4; only crouching
    /// may enter. This exercises the real mission geometry when game data is
    /// available (the checked-in fixture below covers CI).
    #[test]
    fn standing_player_does_not_enter_medsci_five_foot_ceiling() {
        let Some(level) = try_load_level("medsci1.mis") else {
            return;
        };
        let mut world = PhysicsWorld::new();
        world.add_level_geometry(EntityId::from_inner(1).unwrap(), &level);
        let mut player = world.create_player(
            vec3(-22.294, -0.596, -17.1),
            EntityId::from_inner(2).unwrap(),
        );
        step(&mut world, &mut player, 30);

        for _ in 0..60 {
            world.update(vec3(-10.0 / 60.0, 0.0, 0.0), &mut player);
        }
        let end = world.get_player_translation(&player);
        assert!(
            end.x > -26.0,
            "standing player crossed the five-foot ceiling face: ended {end:?}"
        );
    }

    /// The two ceiling triangles from the real MedSci corridor above. Keeping
    /// the authored coordinates makes the regression run without game data in
    /// CI while the live-mission test verifies the extraction locally.
    const MEDSCI_FIVE_FOOT_CEILING_TRIS: &[[[f32; 3]; 3]] = &[
        [
            [-28.8, 0.4, -15.6],
            [-26.4, 0.4, -15.6],
            [-26.4, 0.4, -17.6],
        ],
        [
            [-28.8, 0.4, -17.6],
            [-28.8, 0.4, -15.6],
            [-26.4, 0.4, -17.6],
        ],
    ];

    fn world_with_medsci_five_foot_ceiling() -> (PhysicsWorld, PlayerHandle) {
        let mut world = PhysicsWorld::new();
        let floor = world.create_static_body(
            Isometry::translation(0.0, -2.6, 0.0),
            EntityId::from_inner(1000),
        );
        world.attach_collider(
            floor,
            SharedShape::cuboid(100.0, 1.0, 100.0),
            1.0,
            CollisionGroup::entity(),
        );

        let mut verts = Vec::new();
        let mut tris = Vec::new();
        for triangle in MEDSCI_FIVE_FOOT_CEILING_TRIS {
            let base = verts.len() as u32;
            verts.extend(
                triangle
                    .iter()
                    .map(|point| point![point[0], point[1], point[2]]),
            );
            tris.push([base, base + 1, base + 2]);
        }
        world.add_collider(
            EntityId::from_inner(1001).unwrap(),
            ColliderBuilder::trimesh(verts, tris)
                .expect("MedSci ceiling trimesh")
                .build(),
        );

        let player = world.create_player(
            vec3(-22.294, -0.596, -17.1),
            EntityId::from_inner(2000).unwrap(),
        );
        (world, player)
    }

    /// The six-foot standing profile stops before MedSci's five-foot ceiling,
    /// while the unchanged 2.8-foot crouched profile enters and cannot expand
    /// there. Negative-first: the temporary 4.8-foot standing capsule reached
    /// x=-31.96 through this clearance.
    #[test]
    fn only_crouched_player_enters_extracted_medsci_five_foot_clearance() {
        let (mut standing_world, mut standing_player) = world_with_medsci_five_foot_ceiling();
        step(&mut standing_world, &mut standing_player, 30);
        for _ in 0..60 {
            standing_world.update(vec3(-10.0 / 60.0, 0.0, 0.0), &mut standing_player);
        }
        let standing_end = standing_world.get_player_translation(&standing_player);
        assert!(
            standing_end.x > -26.0,
            "standing player crossed the five-foot ceiling face: ended {standing_end:?}"
        );

        let (mut crouched_world, mut crouched_player) = world_with_medsci_five_foot_ceiling();
        step(&mut crouched_world, &mut crouched_player, 30);
        assert!(
            crouched_world.set_player_crouch(true, &mut crouched_player),
            "crouch request must shrink the collider"
        );
        for _ in 0..36 {
            crouched_world.update(vec3(-10.0 / 60.0, 0.0, 0.0), &mut crouched_player);
        }
        let crouched_end = crouched_world.get_player_translation(&crouched_player);
        assert!(
            crouched_end.x < -27.0,
            "crouched player should cross the ceiling face: ended {crouched_end:?}"
        );
        assert!(
            crouched_world.set_player_crouch(false, &mut crouched_player),
            "standing must be refused under the five-foot ceiling"
        );
    }

    /// A crawlspace whose 3.875 ft clearance admits the 2.8 ft crouched body
    /// but not the 6 ft standing one, entered at the standing-equivalent
    /// collider centre a save file stores (`GlobalData::position`).
    fn world_with_low_ceiling_crawlspace() -> (PhysicsWorld, PlayerHandle) {
        let mut world = PhysicsWorld::new();
        for (entity, y) in [(1000u64, 0.0), (1001, 1.55)] {
            world.add_collider(
                EntityId::from_inner(entity).unwrap(),
                ColliderBuilder::trimesh(
                    vec![
                        point![-50.0, y, -50.0],
                        point![50.0, y, -50.0],
                        point![50.0, y, 50.0],
                        point![-50.0, y, 50.0],
                    ],
                    vec![[0u32, 1, 2], [0, 2, 3]],
                )
                .expect("crawlspace trimesh")
                .build(),
            );
        }
        let player = world.create_player(
            vec3(0.0, PLAYER_STANDING_HEIGHT / 2.0 / SCALE_FACTOR, 0.0),
            EntityId::from_inner(2000).unwrap(),
        );
        (world, player)
    }

    /// Loading a save taken while crouched must not expand the player into the
    /// ceiling they were crouched under on the very first frame.
    ///
    /// `Game::load_from_file` builds a fresh world, re-applies the saved crouch
    /// (`Mission::restore_saved_crouch`) and then runs a frame whose crouch
    /// input is released, so the stand-up headroom probes run before the world
    /// has ever been stepped - i.e. against a broad phase Rapier has not built.
    ///
    /// Negative-first: those probes used to report "clear" (they match nothing
    /// at all), standing the 6 ft capsule up inside a 3.875 ft clearance. An
    /// embedded capsule is unrecoverable - the character controller resolves
    /// zero movement in every direction - which is the permanently stuck hydro2
    /// "Low Head Room" pose reported in issue #773.
    #[test]
    fn first_frame_after_load_keeps_a_crouch_the_ceiling_requires() {
        let (mut world, mut player) = world_with_low_ceiling_crawlspace();
        // What `Mission::restore_saved_crouch` does at load, on a world the
        // physics pipeline has not stepped yet.
        assert!(
            world.set_player_crouch(true, &mut player),
            "the saved crouch must shrink the collider"
        );
        // Frame 1: the crouch key is not held.
        assert!(
            world.set_player_crouch(false, &mut player),
            "standing must stay refused before the world can answer a query"
        );

        // ...and the player is still free to crawl back out.
        step(&mut world, &mut player, 10);
        let start = world.get_player_translation(&player);
        for _ in 0..60 {
            world.update(vec3(-10.0 / 60.0, 0.0, 0.0), &mut player);
        }
        let end = world.get_player_translation(&player);
        assert!(
            start.x - end.x > 2.0,
            "a loaded crouched player must still move, went {start:?} -> {end:?}"
        );
        assert!(
            player.is_crouched(),
            "the ceiling must still refuse standing once the world is queryable"
        );
    }

    /// The same first-frame load against the real hydro2 geometry and the exact
    /// pose stored in the issue #773 save (`(84.37742, 4.044117, 20.42488)`,
    /// `is_crouched: true`): the "Low Head Room" ledge under the Sector C
    /// window row, where the brush ceiling sits 1.55 wu above the y=2.8 floor.
    /// Skipped when the game data is absent (CI); the synthetic fixture above
    /// covers the same regression there.
    #[test]
    fn hydro2_low_head_room_save_pose_stays_crouched_on_load() {
        let Some(level) = try_load_level("hydro2.mis") else {
            return;
        };
        let mut world = PhysicsWorld::new();
        world.add_level_geometry(EntityId::from_inner(1).unwrap(), &level);
        let mut player = world.create_player(
            vec3(84.37742, 4.044117, 20.42488),
            EntityId::from_inner(2).unwrap(),
        );
        assert!(world.set_player_crouch(true, &mut player));
        assert!(
            world.set_player_crouch(false, &mut player),
            "the hydro2 ledge has no standing headroom, so the load frame must stay crouched"
        );

        step(&mut world, &mut player, 10);
        let start = world.get_player_translation(&player);
        for _ in 0..90 {
            world.update(vec3(-10.0 / 60.0, 0.0, 0.0), &mut player);
        }
        let end = world.get_player_translation(&player);
        assert!(
            start.x - end.x > 2.0,
            "the loaded player must be able to crawl back along the ledge, went {start:?} -> {end:?}"
        );
    }

    /// The station.mis service-hall ledge that blocked the Earth -> Station
    /// campaign (issue #499). The player stands in the pocket below the ledge
    /// and pushes northwest along the authored AIPATH route
    /// `(13.38,6.40) -> (13.38,6.60) -> (12.80,6.70) -> (12.00,6.80)`; the
    /// route crosses a 1.5 ft riser, well inside the 2 ft step height.
    ///
    /// (Negative-first: before the fix the player wedges at `(13.16,-3.80,6.44)`
    /// forever - the exact coordinate reported in the issue - because the
    /// headroom cast reports the riser face it is pressed against as an
    /// immediate hit and the forward cast aims straight into the west wall.)
    ///
    /// This runs against REAL mission geometry on purpose: two earlier
    /// attempts at this bug went green on synthetic cuboid fixtures that did
    /// not reproduce the contact at all.
    #[test]
    fn player_climbs_station_service_hall_ledge() {
        let Some(level) = try_load_level("station.mis") else {
            return;
        };
        let mut world = PhysicsWorld::new();
        world.add_level_geometry(EntityId::from_inner(1).unwrap(), &level);
        let mut player =
            world.create_player(vec3(13.38, -3.8, 6.4), EntityId::from_inner(2).unwrap());
        for _ in 0..30 {
            world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
        }
        let start = world.get_player_translation(&player);
        // Follow every authored AIPATH waypoint around the corner. The wider
        // original body cannot cut diagonally across the inside wall the way
        // the temporary narrow capsule did.
        for target in [
            vec3(13.38, 0.0, 6.6),
            vec3(12.8, 0.0, 6.7),
            vec3(12.0, 0.0, 6.8),
        ] {
            walk_player_toward(&mut world, &mut player, target, 60);
        }
        let end = world.get_player_translation(&player);
        assert!(
            end.y - start.y > 0.5,
            "should climb the ~1.5 ft service-hall riser, rose {} (ended {:?})",
            end.y - start.y,
            end
        );
    }

    /// The station.mis service-hall ledge, as REAL collision triangles lifted
    /// straight out of the shipped mission (123 of them, the geometry within
    /// ~2 units of the blocked pose). Checked in so the regression runs on CI,
    /// where the game data is absent - `player_climbs_station_service_hall_ledge`
    /// covers the same ground against the live mission when data IS present.
    ///
    /// This is a real-geometry fixture on purpose. Two earlier attempts at
    /// issue #499 were rejected because hand-built cuboid fixtures went green
    /// without ever reproducing the contact that actually blocked the player.
    #[rustfmt::skip]
    const STATION_LEDGE_TRIS: &[[[f32; 3]; 3]] = &[
    [[12.8000, 0.8000, 9.6000], [12.8000, 0.8000, 6.4000], [12.8000, -2.4000, 9.6000]],
    [[12.8000, 0.8000, 6.4000], [12.8000, -0.8000, 6.4000], [12.8000, -2.4000, 9.6000]],
    [[12.8000, -0.8000, 6.4000], [12.8000, -2.4000, 6.4000], [12.8000, -2.4000, 9.6000]],
    [[12.8000, -2.4000, 6.4000], [12.8000, -2.4000, 6.6000], [12.8000, -2.4000, 9.6000]],
    [[12.8000, -2.4000, 6.6000], [12.8000, -2.4000, 9.4000], [12.8000, -2.4000, 9.6000]],
    [[12.8000, -4.8000, 9.6000], [12.8000, -2.4000, 9.6000], [12.8000, -4.8000, 9.4000]],
    [[12.8000, -2.4000, 9.6000], [12.8000, -2.4000, 9.4000], [12.8000, -4.8000, 9.4000]],
    [[12.8000, -2.4000, 9.4000], [12.8000, -4.0000, 9.4000], [12.8000, -4.8000, 9.4000]],
    [[12.8000, -4.0000, 9.4000], [12.8000, -4.2000, 9.4000], [12.8000, -4.8000, 9.4000]],
    [[24.0000, -4.8000, 9.4000], [24.0000, -4.8000, 9.6000], [12.8000, -4.8000, 9.4000]],
    [[24.0000, -4.8000, 9.6000], [14.4000, -4.8000, 9.6000], [12.8000, -4.8000, 9.4000]],
    [[14.4000, -4.8000, 9.6000], [12.8000, -4.8000, 9.6000], [12.8000, -4.8000, 9.4000]],
    [[8.0000, -2.4000, 6.6000], [12.4000, -2.4000, 6.6000], [8.0000, -4.2000, 6.6000]],
    [[12.4000, -2.4000, 6.6000], [12.4000, -4.0000, 6.6000], [8.0000, -4.2000, 6.6000]],
    [[12.4000, -4.0000, 6.6000], [12.4000, -4.2000, 6.6000], [8.0000, -4.2000, 6.6000]],
    [[8.0000, -2.4000, 6.6000], [8.0000, -2.4000, 9.4000], [12.4000, -2.4000, 6.6000]],
    [[8.0000, -2.4000, 9.4000], [12.4000, -2.4000, 9.4000], [12.4000, -2.4000, 6.6000]],
    [[12.4000, -2.4000, 9.4000], [12.4000, -2.4000, 9.0000], [12.4000, -2.4000, 6.6000]],
    [[12.4000, -2.4000, 9.0000], [12.4000, -2.4000, 7.0000], [12.4000, -2.4000, 6.6000]],
    [[8.0000, -2.4000, 9.4000], [8.0000, -4.2000, 9.4000], [12.4000, -2.4000, 9.4000]],
    [[8.0000, -4.2000, 9.4000], [12.4000, -4.2000, 9.4000], [12.4000, -2.4000, 9.4000]],
    [[12.4000, -4.2000, 9.4000], [12.4000, -4.0000, 9.4000], [12.4000, -2.4000, 9.4000]],
    [[12.4000, -2.4000, 6.6000], [12.4000, -2.4000, 7.0000], [12.4000, -4.0000, 6.6000]],
    [[12.4000, -2.4000, 7.0000], [12.4000, -4.0000, 7.0000], [12.4000, -4.0000, 6.6000]],
    [[12.4000, -4.0000, 9.4000], [12.4000, -4.0000, 9.0000], [12.4000, -2.4000, 9.4000]],
    [[12.4000, -4.0000, 9.0000], [12.4000, -2.4000, 9.0000], [12.4000, -2.4000, 9.4000]],
    [[12.4000, -4.2000, 9.2000], [8.0000, -4.2000, 9.2000], [12.4000, -4.2000, 6.8000]],
    [[8.0000, -4.2000, 9.2000], [8.0000, -4.2000, 6.8000], [12.4000, -4.2000, 6.8000]],
    [[12.8000, -2.4000, 9.4000], [12.8000, -2.4000, 6.6000], [12.4000, -2.4000, 9.0000]],
    [[12.8000, -2.4000, 6.6000], [12.4000, -2.4000, 7.0000], [12.4000, -2.4000, 9.0000]],
    [[12.4000, -4.0000, 7.0000], [12.4000, -2.4000, 7.0000], [12.8000, -4.0000, 6.6000]],
    [[12.4000, -2.4000, 7.0000], [12.8000, -2.4000, 6.6000], [12.8000, -4.0000, 6.6000]],
    [[12.8000, -2.4000, 6.6000], [12.8000, -2.9000, 6.6000], [12.8000, -4.0000, 6.6000]],
    [[12.8000, -4.0000, 9.4000], [12.8000, -2.4000, 9.4000], [12.4000, -4.0000, 9.0000]],
    [[12.8000, -2.4000, 9.4000], [12.4000, -2.4000, 9.0000], [12.4000, -4.0000, 9.0000]],
    [[12.4000, -4.2000, 6.8000], [24.0000, -4.2000, 6.8000], [12.4000, -4.2000, 9.2000]],
    [[24.0000, -4.2000, 6.8000], [24.0000, -4.2000, 9.2000], [12.4000, -4.2000, 9.2000]],
    [[12.4000, -4.0000, 7.0000], [12.8000, -4.0000, 6.6000], [12.4000, -4.0000, 6.6000]],
    [[12.8000, -4.0000, 6.6000], [12.8000, -4.2000, 6.6000], [12.4000, -4.0000, 6.6000]],
    [[12.8000, -4.2000, 6.6000], [12.4000, -4.2000, 6.6000], [12.4000, -4.0000, 6.6000]],
    [[12.4000, -4.0000, 9.0000], [12.4000, -4.0000, 9.4000], [12.8000, -4.0000, 9.4000]],
    [[12.4000, -4.0000, 9.4000], [12.4000, -4.2000, 9.4000], [12.8000, -4.0000, 9.4000]],
    [[12.4000, -4.2000, 9.4000], [12.8000, -4.2000, 9.4000], [12.8000, -4.0000, 9.4000]],
    [[8.0000, -4.4000, 9.2000], [8.0000, -4.2000, 9.2000], [24.0000, -4.4000, 9.2000]],
    [[8.0000, -4.2000, 9.2000], [12.4000, -4.2000, 9.2000], [24.0000, -4.4000, 9.2000]],
    [[12.4000, -4.2000, 9.2000], [24.0000, -4.2000, 9.2000], [24.0000, -4.4000, 9.2000]],
    [[12.8000, -4.8000, 9.4000], [12.8000, -4.2000, 9.4000], [8.0000, -4.8000, 9.4000]],
    [[12.8000, -4.2000, 9.4000], [12.4000, -4.2000, 9.4000], [8.0000, -4.8000, 9.4000]],
    [[12.4000, -4.2000, 9.4000], [8.0000, -4.2000, 9.4000], [8.0000, -4.8000, 9.4000]],
    [[12.8000, -4.8000, 9.4000], [8.0000, -4.8000, 9.4000], [12.8000, -4.8000, 9.2000]],
    [[8.0000, -4.8000, 9.4000], [8.0000, -4.8000, 9.2000], [12.8000, -4.8000, 9.2000]],
    [[12.8000, -4.8000, 9.4000], [12.8000, -4.8000, 9.2000], [24.0000, -4.8000, 9.4000]],
    [[12.8000, -4.8000, 9.2000], [24.0000, -4.8000, 9.2000], [24.0000, -4.8000, 9.4000]],
    [[19.8000, -4.4000, 9.0000], [19.8000, -4.8000, 9.0000], [13.9000, -4.4000, 9.0000]],
    [[19.8000, -4.8000, 9.0000], [13.9000, -4.8000, 9.0000], [13.9000, -4.4000, 9.0000]],
    [[13.9000, -4.4000, 9.0000], [13.9000, -4.8000, 9.0000], [8.0000, -4.4000, 9.0000]],
    [[13.9000, -4.8000, 9.0000], [12.8000, -4.8000, 9.0000], [8.0000, -4.4000, 9.0000]],
    [[12.8000, -4.8000, 9.0000], [8.0000, -4.8000, 9.0000], [8.0000, -4.4000, 9.0000]],
    [[24.0000, -4.4000, 9.2000], [24.0000, -4.4000, 9.0000], [8.0000, -4.4000, 9.2000]],
    [[24.0000, -4.4000, 9.0000], [19.8000, -4.4000, 9.0000], [8.0000, -4.4000, 9.2000]],
    [[19.8000, -4.4000, 9.0000], [13.9000, -4.4000, 9.0000], [8.0000, -4.4000, 9.2000]],
    [[13.9000, -4.4000, 9.0000], [8.0000, -4.4000, 9.0000], [8.0000, -4.4000, 9.2000]],
    [[8.0000, -4.8000, 9.2000], [8.0000, -4.8000, 9.0000], [12.8000, -4.8000, 9.2000]],
    [[8.0000, -4.8000, 9.0000], [12.8000, -4.8000, 9.0000], [12.8000, -4.8000, 9.2000]],
    [[12.8000, -4.8000, 9.2000], [12.8000, -4.8000, 9.0000], [24.0000, -4.8000, 9.2000]],
    [[12.8000, -4.8000, 9.0000], [13.9000, -4.8000, 9.0000], [24.0000, -4.8000, 9.2000]],
    [[13.9000, -4.8000, 9.0000], [19.8000, -4.8000, 9.0000], [24.0000, -4.8000, 9.2000]],
    [[12.2000, -4.8000, 7.0000], [12.2000, -4.4000, 7.0000], [8.0000, -4.8000, 7.0000]],
    [[12.2000, -4.4000, 7.0000], [8.0000, -4.4000, 7.0000], [8.0000, -4.8000, 7.0000]],
    [[18.1000, -4.4000, 7.0000], [12.2000, -4.4000, 7.0000], [18.1000, -4.8000, 7.0000]],
    [[12.2000, -4.4000, 7.0000], [12.2000, -4.8000, 7.0000], [18.1000, -4.8000, 7.0000]],
    [[12.2000, -4.8000, 7.0000], [12.8000, -4.8000, 7.0000], [18.1000, -4.8000, 7.0000]],
    [[18.1000, -4.8000, 7.0000], [12.8000, -4.8000, 7.0000], [24.0000, -4.8000, 7.0000]],
    [[12.8000, -4.8000, 7.0000], [12.8000, -4.8000, 6.8000], [24.0000, -4.8000, 7.0000]],
    [[12.8000, -4.8000, 6.8000], [24.0000, -4.8000, 6.8000], [24.0000, -4.8000, 7.0000]],
    [[8.0000, -4.4000, 7.0000], [12.2000, -4.4000, 7.0000], [8.0000, -4.4000, 6.8000]],
    [[12.2000, -4.4000, 7.0000], [18.1000, -4.4000, 7.0000], [8.0000, -4.4000, 6.8000]],
    [[18.1000, -4.4000, 7.0000], [24.0000, -4.4000, 7.0000], [8.0000, -4.4000, 6.8000]],
    [[24.0000, -4.4000, 7.0000], [24.0000, -4.4000, 6.8000], [8.0000, -4.4000, 6.8000]],
    [[12.8000, -4.8000, 7.0000], [12.2000, -4.8000, 7.0000], [12.8000, -4.8000, 6.8000]],
    [[12.2000, -4.8000, 7.0000], [8.0000, -4.8000, 7.0000], [12.8000, -4.8000, 6.8000]],
    [[8.0000, -4.8000, 7.0000], [8.0000, -4.8000, 6.8000], [12.8000, -4.8000, 6.8000]],
    [[12.8000, -4.8000, 6.8000], [12.8000, -4.8000, 6.6000], [24.0000, -4.8000, 6.8000]],
    [[12.8000, -4.8000, 6.6000], [22.1000, -4.8000, 6.6000], [24.0000, -4.8000, 6.8000]],
    [[24.0000, -4.4000, 6.8000], [24.0000, -4.2000, 6.8000], [8.0000, -4.4000, 6.8000]],
    [[24.0000, -4.2000, 6.8000], [12.4000, -4.2000, 6.8000], [8.0000, -4.4000, 6.8000]],
    [[12.4000, -4.2000, 6.8000], [8.0000, -4.2000, 6.8000], [8.0000, -4.4000, 6.8000]],
    [[12.8000, -4.8000, 6.8000], [8.0000, -4.8000, 6.8000], [12.8000, -4.8000, 6.6000]],
    [[8.0000, -4.8000, 6.8000], [8.0000, -4.8000, 6.6000], [12.8000, -4.8000, 6.6000]],
    [[8.0000, -4.8000, 6.6000], [8.0000, -4.2000, 6.6000], [12.8000, -4.8000, 6.6000]],
    [[8.0000, -4.2000, 6.6000], [12.4000, -4.2000, 6.6000], [12.8000, -4.8000, 6.6000]],
    [[12.4000, -4.2000, 6.6000], [12.8000, -4.2000, 6.6000], [12.8000, -4.8000, 6.6000]],
    [[12.8000, -2.9000, 6.6000], [12.8000, -2.4000, 6.6000], [12.8000, -2.9000, 6.4000]],
    [[12.8000, -2.4000, 6.6000], [12.8000, -2.4000, 6.4000], [12.8000, -2.9000, 6.4000]],
    [[22.1000, -4.8000, 6.6000], [12.8000, -4.8000, 6.6000], [22.1000, -4.8000, 6.4000]],
    [[12.8000, -4.8000, 6.6000], [12.8000, -4.8000, 6.4000], [22.1000, -4.8000, 6.4000]],
    [[12.8000, -4.8000, 6.4000], [14.6000, -4.8000, 6.4000], [22.1000, -4.8000, 6.4000]],
    [[14.6000, -4.8000, 6.4000], [15.0000, -4.8000, 6.4000], [22.1000, -4.8000, 6.4000]],
    [[15.0000, -4.8000, 6.4000], [19.0000, -4.8000, 6.4000], [22.1000, -4.8000, 6.4000]],
    [[12.8000, -2.9000, 6.4000], [12.8000, -4.4000, 6.4000], [12.8000, -2.9000, 6.6000]],
    [[12.8000, -4.4000, 6.4000], [12.8000, -4.8000, 6.4000], [12.8000, -2.9000, 6.6000]],
    [[12.8000, -4.8000, 6.4000], [12.8000, -4.8000, 6.6000], [12.8000, -2.9000, 6.6000]],
    [[12.8000, -4.8000, 6.6000], [12.8000, -4.2000, 6.6000], [12.8000, -2.9000, 6.6000]],
    [[12.8000, -4.2000, 6.6000], [12.8000, -4.0000, 6.6000], [12.8000, -2.9000, 6.6000]],
    [[11.0000, -4.8000, 6.4000], [7.6000, -4.8000, 6.4000], [11.0000, -4.8000, 3.2000]],
    [[7.6000, -4.8000, 6.4000], [7.6000, -4.8000, 6.2461], [11.0000, -4.8000, 3.2000]],
    [[7.6000, -4.8000, 6.2461], [10.3958, -4.8000, 3.4503], [11.0000, -4.8000, 3.2000]],
    [[7.6000, -4.8000, 6.4000], [11.0000, -4.8000, 6.4000], [7.6000, 1.6000, 6.4000]],
    [[11.0000, -4.8000, 6.4000], [11.0000, -4.4000, 6.4000], [7.6000, 1.6000, 6.4000]],
    [[11.0000, -4.4000, 6.4000], [11.0000, -0.8000, 6.4000], [7.6000, 1.6000, 6.4000]],
    [[11.0000, -4.4000, 6.4000], [12.8000, -4.4000, 6.4000], [11.0000, -0.8000, 6.4000]],
    [[12.8000, -4.4000, 6.4000], [12.8000, -2.9000, 6.4000], [11.0000, -0.8000, 6.4000]],
    [[12.8000, -2.9000, 6.4000], [12.8000, -2.4000, 6.4000], [11.0000, -0.8000, 6.4000]],
    [[12.8000, -2.4000, 6.4000], [12.8000, -0.8000, 6.4000], [11.0000, -0.8000, 6.4000]],
    [[11.0000, -4.8000, 3.2000], [14.6000, -4.8000, 3.2000], [11.0000, -4.8000, 6.4000]],
    [[14.6000, -4.8000, 3.2000], [14.6000, -4.8000, 6.4000], [11.0000, -4.8000, 6.4000]],
    [[14.6000, -4.8000, 6.4000], [12.8000, -4.8000, 6.4000], [11.0000, -4.8000, 6.4000]],
    [[11.0000, -4.4000, 6.4000], [11.0000, -4.8000, 6.4000], [12.8000, -4.4000, 6.4000]],
    [[11.0000, -4.8000, 6.4000], [12.8000, -4.8000, 6.4000], [12.8000, -4.4000, 6.4000]],
    [[15.0000, -4.8000, 6.4000], [14.6000, -4.8000, 6.4000], [15.0000, -4.8000, 3.2000]],
    [[14.6000, -4.8000, 6.4000], [14.6000, -4.8000, 3.2000], [15.0000, -4.8000, 3.2000]],
    [[15.0000, -4.8000, 6.4000], [15.0000, -4.8000, 3.2000], [19.0000, -4.8000, 6.4000]],
    [[15.0000, -4.8000, 3.2000], [18.6000, -4.8000, 3.2000], [19.0000, -4.8000, 6.4000]],
    ];

    /// Walking northwest into the service-hall ledge must climb it, exactly as
    /// on the real mission. (Negative-first: before the fix the player wedges
    /// at the issue's reported `(13.16, -3.80, 6.44)` - the upward headroom
    /// sweep reports the riser face it rests against as an immediate grazing
    /// hit, and the forward cast aims into the west wall instead of the ledge.)
    #[test]
    fn player_climbs_extracted_station_ledge() {
        let mut world = PhysicsWorld::new();
        let mut verts = Vec::new();
        let mut tris = Vec::new();
        for t in STATION_LEDGE_TRIS {
            let base = verts.len() as u32;
            for v in t {
                verts.push(point![v[0], v[1], v[2]]);
            }
            tris.push([base, base + 1, base + 2]);
        }
        world.add_collider(
            EntityId::from_inner(1).unwrap(),
            ColliderBuilder::trimesh(verts, tris)
                .expect("ledge trimesh")
                .build(),
        );
        let mut player =
            world.create_player(vec3(13.38, -3.8, 6.4), EntityId::from_inner(2).unwrap());
        for _ in 0..30 {
            world.update(Vector3::new(0.0, 0.0, 0.0), &mut player);
        }
        let start = world.get_player_translation(&player);
        for target in [
            vec3(13.38, 0.0, 6.6),
            vec3(12.8, 0.0, 6.7),
            vec3(12.0, 0.0, 6.8),
        ] {
            walk_player_toward(&mut world, &mut player, target, 60);
        }
        let end = world.get_player_translation(&player);
        assert!(
            end.y - start.y > 0.5,
            "should climb the ~1.5 ft service-hall riser, rose {} (ended {:?})",
            end.y - start.y,
            end
        );
    }
}
