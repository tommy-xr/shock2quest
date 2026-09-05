use cgmath::{Deg, InnerSpace, Quaternion, Rotation, Rotation3, Vector3, vec3};
use dark::SCALE_FACTOR;
use shipyard::{EntityId, World};

use crate::{
    physics::{InternalCollisionGroups, PhysicsWorld},
    scripts::ai::ai_util,
    time::Time,
    util::{get_position_from_transform, get_rotation_from_transform},
};

/// How far ahead the whiskers look (3 Dark feet - a body width plus a
/// little, so the bias builds while there is still room to turn)
const WHISKER_DISTANCE: f32 = 3.0 / SCALE_FACTOR;
/// Whiskers to either side of the facing direction
const WHISKER_SPREAD: Deg<f32> = Deg(30.0);
/// Probe heights, as a fraction of the creature's own height below its body
/// origin (colliders are centered on that origin). A railing is a thin slab
/// whose top sits just BELOW an upright creature's center, so a single
/// center-height ray sails over it - the wedge this exists to prevent. Two
/// heights also discriminate: a low obstacle (a stair riser, a kerb) blocks
/// only the knee ray, and a bias is only taken when BOTH heights are
/// blocked, so climbable steps don't push AIs sideways off staircases the
/// route chose. Fractions rather than fixed feet, because a monkey is half
/// a hybrid's height and fixed offsets would put both its probes underground.
const WHISKER_KNEE_FRACTION: f32 = 0.35;
const WHISKER_CHEST_FRACTION: f32 = 0.15;
/// Seconds between probes. Static geometry doesn't move and every
/// path-following AI runs this, so the rays are cast a few times a second
/// and the bias is held in between (the original engine's wall regulator
/// runs on a comparable interval).
const WHISKER_INTERVAL_SECONDS: f32 = 0.2;
/// The bias bends the aim point at most this far (3 Dark feet), the same
/// cap crowd separation uses: whiskers nudge the route around geometry the
/// navigation mesh doesn't model, they never veto it.
pub const WHISKER_MAX_OFFSET: f32 = 3.0 / SCALE_FACTOR;

/// One blocked whisker: the surface normal at the hit and how far away it is
pub struct WhiskerHit {
    pub normal: Vector3<f32>,
    pub distance: f32,
}

/// One whisker: the (horizontal, unit) direction it was cast along, and what
/// it found. Readings are ordered CENTER FIRST, then the side whiskers.
pub struct WhiskerReading {
    pub direction: Vector3<f32>,
    pub hit: Option<WhiskerHit>,
}

/// Static-geometry whiskers, sampled on an interval and blended into path
/// following as a bias on the aim point. Unlike
/// `CollisionAvoidanceSteeringStrategy` - which probes at body center only
/// and answers with a heading of its own, and stays the no-route fallback -
/// this never takes the heading over: it runs *while* a route is being
/// followed, where overriding the route deadlocks AIs against walls the path
/// was about to turn away from (issue #481).
pub struct WhiskerAvoidance {
    bias: Vector3<f32>,
    seconds_since_probe: f32,
}

impl WhiskerAvoidance {
    pub fn new() -> WhiskerAvoidance {
        WhiskerAvoidance {
            // Probe on the first update
            bias: vec3(0.0, 0.0, 0.0),
            seconds_since_probe: f32::MAX,
        }
    }

    /// The current avoidance bias (world units, horizontal), re-probing at
    /// most every WHISKER_INTERVAL_SECONDS.
    pub fn update(
        &mut self,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
        time: &Time,
    ) -> Vector3<f32> {
        self.seconds_since_probe += time.elapsed.as_secs_f32();
        if self.seconds_since_probe < WHISKER_INTERVAL_SECONDS {
            return self.bias;
        }
        self.seconds_since_probe = 0.0;
        self.bias = whisker_bias(&self.probe(world, physics, entity_id), WHISKER_DISTANCE);
        self.bias
    }

    fn probe(
        &self,
        world: &World,
        physics: &PhysicsWorld,
        entity_id: EntityId,
    ) -> Vec<WhiskerReading> {
        let rotation = get_rotation_from_transform(world, entity_id);
        let height = ai_util::creature_height(world, entity_id);
        let knee = get_position_from_transform(
            world,
            entity_id,
            vec3(0.0, -height * WHISKER_KNEE_FRACTION, 0.0),
        );
        let chest = get_position_from_transform(
            world,
            entity_id,
            vec3(0.0, -height * WHISKER_CHEST_FRACTION, 0.0),
        );

        // Center first, then the sides - whisker_bias reads them that way
        let mut readings = Vec::new();
        for spread in [Deg(0.0), WHISKER_SPREAD, -WHISKER_SPREAD] {
            let forward = (rotation * Quaternion::from_angle_y(spread))
                .rotate_vector(vec3(0.0, 0.0, 1.0))
                .normalize();
            // Only static geometry and props: living bodies (and the player)
            // are what crowd separation is for, and biasing away from them
            // here would repel a chasing AI from its own target. A door the
            // AI can open is not an obstacle to bend around either - filtered
            // inside the query, so whatever stands behind it is still seen.
            let not_a_door = |id: EntityId| !ai_util::is_entity_door(world, id);
            let cast = |from| {
                physics.ray_cast2_as_actor_with_entity_filter(
                    from,
                    forward,
                    WHISKER_DISTANCE,
                    InternalCollisionGroups::ALL_COLLIDABLE
                        - InternalCollisionGroups::PLAYER
                        - InternalCollisionGroups::ACTOR,
                    Some(entity_id),
                    true,
                    &not_a_door,
                )
            };
            // Both heights must be blocked - see WHISKER_KNEE_HEIGHT
            let hit = match (cast(knee), cast(chest)) {
                (Some(low), Some(high)) => {
                    let nearest = if (low.hit_point - knee).magnitude()
                        < (high.hit_point - chest).magnitude()
                    {
                        (low, knee)
                    } else {
                        (high, chest)
                    };
                    Some(WhiskerHit {
                        normal: nearest.0.hit_normal,
                        distance: (nearest.0.hit_point - nearest.1).magnitude(),
                    })
                }
                _ => None,
            };
            readings.push(WhiskerReading {
                direction: vec3(forward.x, 0.0, forward.z).normalize(),
                hit,
            });
        }
        readings
    }
}

/// Sum the whiskers into a horizontal push away from the geometry ahead:
/// each blocked whisker contributes its surface normal, weighted by how
/// close it is, and the total is capped at WHISKER_MAX_OFFSET. Floors and
/// ceilings (normals without a horizontal component) contribute nothing -
/// the route walks on them.
///
/// A surface square across the path pushes straight BACK, which is no help
/// to a body that has to get past it, so a blocked center whisker also
/// contributes a sideways push toward whichever side has more room. That is
/// what carries an AI along a wall (or a railing) instead of into it.
pub fn whisker_bias(readings: &[WhiskerReading], max_distance: f32) -> Vector3<f32> {
    let weight = |hit: &WhiskerHit| ((max_distance - hit.distance) / max_distance).clamp(0.0, 1.0);
    let mut bias = vec3(0.0, 0.0, 0.0);
    for reading in readings {
        let Some(hit) = &reading.hit else { continue };
        let horizontal = vec3(hit.normal.x, 0.0, hit.normal.z);
        let length = horizontal.magnitude();
        if length < 0.5 {
            continue;
        }
        bias += horizontal / length * weight(hit) * WHISKER_MAX_OFFSET;
    }

    // Sideways escape, when the way ahead itself is blocked: aim past the
    // obstacle on its roomier side (an unblocked whisker has the full
    // whisker length of room).
    if let Some(center) = readings.first().and_then(|r| r.hit.as_ref()) {
        let clearance =
            |r: &WhiskerReading| r.hit.as_ref().map(|h| h.distance).unwrap_or(max_distance);
        let roomiest = readings[1..]
            .iter()
            .max_by(|a, b| clearance(a).total_cmp(&clearance(b)));
        if let Some(side) = roomiest {
            bias += side.direction * weight(center) * WHISKER_MAX_OFFSET;
        }
    }

    let magnitude = bias.magnitude();
    if magnitude > WHISKER_MAX_OFFSET {
        bias *= WHISKER_MAX_OFFSET / magnitude;
    }
    bias
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear(direction: Vector3<f32>) -> WhiskerReading {
        WhiskerReading {
            direction,
            hit: None,
        }
    }

    fn blocked(direction: Vector3<f32>, normal: Vector3<f32>, distance: f32) -> WhiskerReading {
        WhiskerReading {
            direction,
            hit: Some(WhiskerHit { normal, distance }),
        }
    }

    const AHEAD: Vector3<f32> = Vector3::new(0.0, 0.0, 1.0);
    const RIGHT: Vector3<f32> = Vector3::new(1.0, 0.0, 0.0);
    const LEFT: Vector3<f32> = Vector3::new(-1.0, 0.0, 0.0);

    #[test]
    fn clear_whiskers_do_not_bend_the_route() {
        let readings = [clear(AHEAD), clear(RIGHT), clear(LEFT)];
        assert_eq!(whisker_bias(&readings, 1.0), vec3(0.0, 0.0, 0.0));
    }

    /// A wall to one side pushes away from it, harder the closer it is.
    #[test]
    fn a_near_wall_pushes_away_harder_than_a_far_one() {
        let near = whisker_bias(&[clear(AHEAD), blocked(RIGHT, LEFT, 0.25)], 1.0);
        let far = whisker_bias(&[clear(AHEAD), blocked(RIGHT, LEFT, 0.75)], 1.0);
        assert!(near.x < far.x && far.x < 0.0, "near {near:?} far {far:?}");
        assert_eq!(near.z, 0.0);
    }

    /// A wall square across the path: its normal points straight back, so
    /// without a sideways term the AI would just be pushed into itself. The
    /// bias must carry it toward the side with room (here, the right).
    #[test]
    fn a_wall_across_the_path_steers_to_the_roomier_side() {
        let bias = whisker_bias(
            &[
                blocked(AHEAD, -AHEAD, 0.2),
                clear(RIGHT),
                blocked(LEFT, RIGHT, 0.2),
            ],
            1.0,
        );
        assert!(bias.x > 0.0, "expected a push to the right, got {bias:?}");
    }

    /// Floors and ceilings are not obstacles.
    #[test]
    fn horizontal_surfaces_are_ignored() {
        assert_eq!(
            whisker_bias(
                &[clear(AHEAD), blocked(RIGHT, vec3(0.0, 1.0, 0.0), 0.1)],
                1.0
            ),
            vec3(0.0, 0.0, 0.0)
        );
    }

    /// However many whiskers are blocked, the bias stays a bias: it can
    /// never bend the aim point further than the cap.
    #[test]
    fn the_bias_is_capped() {
        let boxed_in = whisker_bias(
            &[
                blocked(AHEAD, -AHEAD, 0.0),
                blocked(RIGHT, LEFT, 0.0),
                blocked(LEFT, RIGHT, 0.0),
            ],
            1.0,
        );
        assert!(boxed_in.magnitude() <= WHISKER_MAX_OFFSET + 1e-5);
        assert!(boxed_in.magnitude() > 0.0);
    }

    /// A bias needs BOTH rays blocked, so the two must straddle a real
    /// obstacle: roughly a hybrid's knee and its chest, which is where the
    /// fixed offsets these fractions replaced put them. Measuring the creature
    /// at a third of its height once bunched both of them at chest level.
    #[test]
    fn the_probes_straddle_knee_and_chest() {
        let mut world = World::new();
        // 0 is the human schema a hybrid animates on.
        let creature = world.add_entity((dark::properties::PropCreature(0),));
        let hybrid = ai_util::creature_height(&world, creature);
        let knee = hybrid * WHISKER_KNEE_FRACTION;
        let chest = hybrid * WHISKER_CHEST_FRACTION;
        assert!(
            (0.8..1.1).contains(&knee),
            "knee ray {knee} is not about 3 feet below the body origin"
        );
        assert!(
            (0.3..0.5).contains(&chest),
            "chest ray {chest} is not about 1 foot below the body origin"
        );
    }
}
