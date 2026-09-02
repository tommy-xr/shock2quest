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
/// Probe heights, relative to the body origin (roughly body center). A
/// railing is a thin slab whose top sits just BELOW an upright creature's
/// center, so a single center-height ray sails over it - the wedge this
/// exists to prevent. Two heights also discriminate: a low obstacle (a
/// stair riser, a kerb) blocks only the knee ray, and a bias is only taken
/// when BOTH heights are blocked, so climbable steps don't push AIs
/// sideways off staircases the route chose.
const WHISKER_KNEE_HEIGHT: f32 = -3.0 / SCALE_FACTOR;
const WHISKER_CHEST_HEIGHT: f32 = -1.0 / SCALE_FACTOR;
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

/// Static-geometry whiskers, sampled on an interval and blended into path
/// following as a bias on the aim point. Unlike `CollisionAvoidanceSteering`
/// this never takes the heading over - it runs *while* a route is being
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

    fn probe(&self, world: &World, physics: &PhysicsWorld, entity_id: EntityId) -> Vec<WhiskerHit> {
        let rotation = get_rotation_from_transform(world, entity_id);
        let knee =
            get_position_from_transform(world, entity_id, vec3(0.0, WHISKER_KNEE_HEIGHT, 0.0));
        let chest =
            get_position_from_transform(world, entity_id, vec3(0.0, WHISKER_CHEST_HEIGHT, 0.0));

        let mut hits = Vec::new();
        for spread in [Deg(0.0), WHISKER_SPREAD, -WHISKER_SPREAD] {
            let forward = (rotation * Quaternion::from_angle_y(spread))
                .rotate_vector(vec3(0.0, 0.0, 1.0))
                .normalize();
            let cast = |from| {
                physics
                    .ray_cast2_as_actor(
                        from,
                        forward,
                        WHISKER_DISTANCE,
                        InternalCollisionGroups::ALL_COLLIDABLE,
                        Some(entity_id),
                        true,
                    )
                    // A door the AI can open is not an obstacle to bend around
                    .filter(|hit| {
                        !hit.maybe_entity_id
                            .map(|id| ai_util::is_entity_door(world, id))
                            .unwrap_or(false)
                    })
            };
            // Both heights must be blocked - see WHISKER_KNEE_HEIGHT
            let (Some(low), Some(high)) = (cast(knee), cast(chest)) else {
                continue;
            };
            let nearest =
                if (low.hit_point - knee).magnitude() < (high.hit_point - chest).magnitude() {
                    (low, knee)
                } else {
                    (high, chest)
                };
            hits.push(WhiskerHit {
                normal: nearest.0.hit_normal,
                distance: (nearest.0.hit_point - nearest.1).magnitude(),
            });
        }
        hits
    }
}

/// Sum the blocked whiskers into a horizontal push away from the geometry:
/// each hit contributes its surface normal, weighted by how close it is, and
/// the total is capped at WHISKER_MAX_OFFSET. Floors and ceilings (normals
/// without a horizontal component) contribute nothing - the route walks on
/// them.
pub fn whisker_bias(hits: &[WhiskerHit], max_distance: f32) -> Vector3<f32> {
    let mut bias = vec3(0.0, 0.0, 0.0);
    for hit in hits {
        let horizontal = vec3(hit.normal.x, 0.0, hit.normal.z);
        let length = horizontal.magnitude();
        if length < 0.5 {
            continue;
        }
        let weight = ((max_distance - hit.distance) / max_distance).clamp(0.0, 1.0);
        bias += horizontal / length * weight * WHISKER_MAX_OFFSET;
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

    fn hit(normal: Vector3<f32>, distance: f32) -> WhiskerHit {
        WhiskerHit { normal, distance }
    }

    #[test]
    fn clear_whiskers_do_not_bend_the_route() {
        assert_eq!(whisker_bias(&[], 1.0), vec3(0.0, 0.0, 0.0));
    }

    /// A wall dead ahead pushes back along its normal, proportionally to how
    /// close it is.
    #[test]
    fn a_near_wall_pushes_away_harder_than_a_far_one() {
        let near = whisker_bias(&[hit(vec3(-1.0, 0.0, 0.0), 0.25)], 1.0);
        let far = whisker_bias(&[hit(vec3(-1.0, 0.0, 0.0), 0.75)], 1.0);
        assert!(near.x < far.x && far.x < 0.0, "near {near:?} far {far:?}");
        assert_eq!(near.z, 0.0);
    }

    /// Floors and ceilings are not obstacles.
    #[test]
    fn horizontal_surfaces_are_ignored() {
        assert_eq!(
            whisker_bias(&[hit(vec3(0.0, 1.0, 0.0), 0.1)], 1.0),
            vec3(0.0, 0.0, 0.0)
        );
    }

    /// However many whiskers are blocked, the bias stays a bias: it can
    /// never bend the aim point further than the cap.
    #[test]
    fn the_bias_is_capped() {
        let boxed_in = whisker_bias(
            &[
                hit(vec3(-1.0, 0.0, 0.0), 0.0),
                hit(vec3(-0.7, 0.0, -0.7), 0.0),
                hit(vec3(0.0, 0.0, -1.0), 0.0),
            ],
            1.0,
        );
        assert!(boxed_in.magnitude() <= WHISKER_MAX_OFFSET + 1e-5);
        assert!(boxed_in.magnitude() > 0.0);
    }
}
