//! Read-only landing queries for releasing every climbing hold, and the
//! assisted-release glide (see [`crate::climb_release`]).

use super::*;
use crate::climb_release::{self, GazeRejection, Landing};

/// How long a thrown arc is followed (60 Hz frames), and how far below the
/// body a floor is looked for, before a landing counts as unpredictable.
const NATURAL_ARC_FRAMES: usize = 240;
const NATURAL_MAX_DROP: f32 = 40.0;
/// Snap and hysteresis searches run for at most this many aimed points a frame.
const GAZE_SNAPPED_RAYS: usize = 3;
/// Support checks settle the body for many frames; one per query.
const GAZE_SUPPORT_CHECKS: usize = 1;
/// How far past a held floor's hold a landing on it is tried, nearest first:
/// the hand vault's own insets.
const HELD_FLOOR_INSETS: [f32; 2] = [0.4, 0.8];
/// Fraction of the body radius the footprint probes stand out from the centre.
const FOOTPRINT_PROBE: f32 = 0.8;

/// One frame's gaze verdict: the assisted landing or why there is none, and
/// the aimed surface point behind it.
pub(crate) struct GazeLanding {
    pub landing: Result<Landing, GazeRejection>,
    pub candidate: Option<Vector3<f32>>,
}

impl PhysicsWorld {
    /// The resting body centre on a floor point, for the stance the player
    /// lands in. A slope lifts the rounded bottom clear of the uphill side.
    fn landing_on(floor: Vector<Real>, normal: Vector<Real>, standing: bool) -> Landing {
        let radius = PLAYER_STANDING_RADIUS / SCALE_FACTOR
            * crate::world_scale::WorldScale::active().personal();
        let lift = player_center_above_floor(!standing) + radius * (1.0 / normal.y - 1.0);
        Landing {
            center: nvec_to_cgmath(floor + Vector::y() * lift),
            floor_y: floor.y,
            normal: nvec_to_cgmath(normal),
        }
    }

    /// The glide an assisted release takes from `start` to `landing`: tucked,
    /// up to the higher of the two, across, then down onto it. A landing below
    /// the start ends the glide above it and leaves the descent to an ordinary
    /// fall, so a long drop keeps its consequences. Every leg is swept and the
    /// body must fit with its footprint on the floor; see also
    /// [`Self::landing_is_supported`].
    fn assisted_route(
        &self,
        player: &PlayerHandle,
        start: Vector<Real>,
        landing: Landing,
    ) -> Result<ClimbTopOut, GazeRejection> {
        let filter = player_movement_filter(player.character_handle);
        let not_climbable =
            |_handle: ColliderHandle, collider: &Collider| collider_is_not_climbable(collider);
        let queries = self.player_movement_queries(self.narrow_phase.query_dispatcher(), filter);
        // The hand-climb pass already moves through climbables; so does the glide.
        let route_queries = queries.with_filter(filter.predicate(&not_climbable));
        let tuck = crouched_player_capsule();
        let stand_on_completion = !player.tracking_is_crouched();
        let final_shape = if stand_on_completion {
            standing_player_capsule()
        } else {
            crouched_player_capsule()
        };
        let target = vec_to_nvec(landing.center);
        if !capsule_pose_is_clear(&queries, target, &final_shape) {
            return Err(GazeRejection::NoFit);
        }
        // The whole footprint stands on the landing, not just a rim the
        // rounded bottom balances on: probe the floor around the body.
        let reach = final_shape.radius * FOOTPRINT_PROBE;
        let footprint = [(reach, 0.0), (-reach, 0.0), (0.0, reach), (0.0, -reach)];
        if !footprint.iter().all(|(x, z)| {
            let ray = Ray::new(point![target.x + x, target.y, target.z + z], -Vector::y());
            queries
                .cast_ray(
                    &ray,
                    target.y - landing.floor_y + climb_release::GAZE_SNAP_MAX_STEP,
                    true,
                )
                .is_some_and(|(_, toi)| {
                    (ray.point_at(toi).y - landing.floor_y).abs()
                        <= climb_release::GAZE_SNAP_MAX_STEP
                })
        }) {
            return Err(GazeRejection::Unsupported);
        }
        let cross_y = start.y.max(target.y);
        let raised = vector![start.x, cross_y, start.z];
        let crossed = vector![target.x, cross_y, target.z];
        if shape_intersects(&route_queries, start, &tuck)
            || !shape_sweep_is_clear(&route_queries, start, raised, &tuck)
            || !shape_sweep_is_clear(&route_queries, raised, crossed, &tuck)
        {
            return Err(GazeRejection::Blocked);
        }
        // Falling the rest: the body expands above the landing and drops
        // straight onto it (stopping just short of the floor contact).
        let falls = crossed.y > target.y + PLAYER_MOVE_ARRIVAL_EPSILON;
        if falls
            && (!capsule_pose_is_clear(&queries, crossed, &final_shape)
                || !shape_sweep_is_clear(
                    &queries,
                    crossed,
                    target + Vector::y() * PLAYER_MOVE_ARRIVAL_EPSILON,
                    &final_shape,
                ))
        {
            return Err(GazeRejection::Blocked);
        }
        let end = if falls { crossed } else { target };
        Ok(ClimbTopOut {
            collide_terrain: true,
            stand_on_completion,
            waypoints: [start, raised, crossed, end, end, end, end],
            next_waypoint: 0,
            save_pose: start,
            reversing: false,
            is_crouched: true,
        })
    }

    /// The landing body settles there under ordinary gravity. Costly (many
    /// controller steps), so it runs after every cheaper check has passed.
    fn landing_is_supported(&self, player: &PlayerHandle, landing: Landing) -> bool {
        let queries = self.player_movement_queries(
            self.narrow_phase.query_dispatcher(),
            player_movement_filter(player.character_handle),
        );
        let shape = if player.tracking_is_crouched() {
            crouched_player_capsule()
        } else {
            standing_player_capsule()
        };
        shape_has_stable_support(
            &player.controller,
            &queries,
            &shape,
            vec_to_nvec(landing.center),
            self.integration_parameters.dt,
        )
    }

    /// The assisted landing the head is aimed at from `eye` along `rays`
    /// (centre first; see [`climb_release::gaze_rays`]), for a body with feet
    /// at `feet_y`. Only the first surface a ray meets counts, so nothing is
    /// chosen through an obstruction. `held` (last frame's landing) is kept
    /// while the aim stays on it and its route stays clear.
    pub(crate) fn gaze_landing(
        &self,
        player: &PlayerHandle,
        eye: Vector3<f32>,
        rays: &[Vector3<f32>],
        feet_y: f32,
        held: Option<Landing>,
    ) -> GazeLanding {
        let start = vec_to_nvec(self.get_player_next_translation(player));
        let body = nvec_to_cgmath(start);
        let standing = !player.tracking_is_crouched();
        let filter = player_movement_filter(player.character_handle);
        let not_climbable =
            |_handle: ColliderHandle, collider: &Collider| collider_is_not_climbable(collider);
        let all_queries =
            self.player_movement_queries(self.narrow_phase.query_dispatcher(), filter);
        // Ladders are mostly air between rungs: see past their boxes.
        let queries = all_queries.with_filter(filter.predicate(&not_climbable));
        let origin = Point::from(vec_to_nvec(eye));

        let mut rejection = None;
        let mut candidate = None;
        let mut snapped = 0;
        let mut support_checks = 0;
        for direction in rays {
            let ray = Ray::new(origin, vec_to_nvec(*direction));
            let Some((_, hit)) =
                queries.cast_ray_and_get_normal(&ray, climb_release::GAZE_MAX_RAY, true)
            else {
                rejection.get_or_insert(GazeRejection::NoHit);
                continue;
            };
            let aimed = ray.point_at(hit.time_of_impact).coords;
            candidate.get_or_insert(nvec_to_cgmath(aimed));
            if !is_walkable_normal(hit.normal.y) {
                rejection.get_or_insert(GazeRejection::NotWalkable);
                continue;
            }
            // A held landing must still be in range: climbing down while
            // staring at a deck must not turn it into a climb up.
            if let Some(held) = held.filter(|held| {
                climb_release::keeps_held(*held, nvec_to_cgmath(aimed))
                    && climb_release::range_rejection(
                        body,
                        feet_y,
                        vec3(held.center.x, held.floor_y, held.center.z),
                    )
                    .is_none()
            }) {
                match self.assisted_route(player, start, held) {
                    Ok(_) => {
                        return GazeLanding {
                            landing: Ok(held),
                            candidate: Some(nvec_to_cgmath(aimed)),
                        };
                    }
                    Err(reason) => {
                        rejection.get_or_insert(reason);
                    }
                }
            }
            if let Some(reason) =
                climb_release::range_rejection(body, feet_y, nvec_to_cgmath(aimed))
            {
                rejection.get_or_insert(reason);
                continue;
            }
            if snapped == GAZE_SNAPPED_RAYS {
                break;
            }
            snapped += 1;
            for offset in climb_release::snap_offsets() {
                let at = aimed + vec_to_nvec(offset);
                let above = Vector::y() * 0.1;
                // Snap along the aimed surface only, never through a wall.
                if offset.x != 0.0 || offset.z != 0.0 {
                    let across =
                        Ray::new(Point::from(aimed + above), vec_to_nvec(offset).normalize());
                    if queries
                        .cast_ray(&across, offset.magnitude(), true)
                        .is_some()
                    {
                        continue;
                    }
                }
                let down = Ray::new(Point::from(at + Vector::y() * 0.5), -Vector::y());
                let Some((_, floor_hit)) = queries.cast_ray_and_get_normal(&down, 1.0, true) else {
                    continue;
                };
                let floor = down.point_at(floor_hit.time_of_impact).coords;
                if !is_walkable_normal(floor_hit.normal.y)
                    || (floor.y - aimed.y).abs() > climb_release::GAZE_SNAP_MAX_STEP
                {
                    continue;
                }
                let landing = Self::landing_on(floor, floor_hit.normal, standing);
                let unsnapped = offset.x == 0.0 && offset.z == 0.0;
                if let Some(reason) =
                    climb_release::range_rejection(body, feet_y, nvec_to_cgmath(floor))
                {
                    if unsnapped {
                        rejection.get_or_insert(reason);
                    }
                    continue;
                }
                let verdict = self.assisted_route(player, start, landing).and_then(|_| {
                    if support_checks == GAZE_SUPPORT_CHECKS {
                        return Err(GazeRejection::Unsupported);
                    }
                    support_checks += 1;
                    if self.landing_is_supported(player, landing) {
                        Ok(())
                    } else {
                        Err(GazeRejection::Unsupported)
                    }
                });
                match verdict {
                    Ok(_) => {
                        return GazeLanding {
                            landing: Ok(landing),
                            candidate: Some(nvec_to_cgmath(aimed)),
                        };
                    }
                    Err(reason) if unsnapped => {
                        rejection.get_or_insert(reason);
                    }
                    Err(_) => {}
                }
            }
        }
        GazeLanding {
            landing: Err(rejection.unwrap_or(GazeRejection::NoHit)),
            candidate,
        }
    }

    /// The landing on a walkable floor a hand holds: past the hand, away from
    /// the body, at the first of the vault's insets the body fits. Held, so
    /// reachable however high: no gaze range applies. `previous` (last
    /// frame's landing) is kept while it is still on this floor near the hand
    /// and its glide is clear, skipping the costly support settle.
    pub(crate) fn held_floor_landing(
        &self,
        player: &PlayerHandle,
        grip: &ClimbGrip,
        previous: Option<Landing>,
    ) -> Option<Landing> {
        if !is_walkable_normal(grip.normal.y) {
            return None;
        }
        let start = vec_to_nvec(self.get_player_next_translation(player));
        let held = vec_to_nvec(grip.point);
        let toward = held - start;
        let horizontal = vector![toward.x, 0.0, toward.z];
        if horizontal.norm() < 1e-4 {
            return None;
        }
        let direction = horizontal.normalize();
        let on_this_floor = |landing: &Landing| {
            (landing.floor_y - grip.point.y).abs() <= climb_release::GAZE_SNAP_MAX_STEP
                && vector![landing.center.x - held.x, 0.0, landing.center.z - held.z].norm()
                    <= HELD_FLOOR_INSETS[HELD_FLOOR_INSETS.len() - 1] + 0.1
        };
        if let Some(previous) = previous.filter(on_this_floor) {
            if self.assisted_route(player, start, previous).is_ok() {
                return Some(previous);
            }
        }
        let filter = player_movement_filter(player.character_handle);
        let not_climbable =
            |_handle: ColliderHandle, collider: &Collider| collider_is_not_climbable(collider);
        let all_queries =
            self.player_movement_queries(self.narrow_phase.query_dispatcher(), filter);
        let queries = all_queries.with_filter(filter.predicate(&not_climbable));
        let standing = !player.tracking_is_crouched();
        HELD_FLOOR_INSETS.iter().find_map(|inset| {
            let ray = Ray::new(
                Point::from(held + direction * *inset + Vector::y() * 0.3),
                -Vector::y(),
            );
            let (_, hit) = queries.cast_ray_and_get_normal(&ray, 0.6, true)?;
            let floor = ray.point_at(hit.time_of_impact).coords;
            let landing = Self::landing_on(floor, hit.normal, standing);
            (is_walkable_normal(hit.normal.y)
                && on_this_floor(&landing)
                && self.assisted_route(player, start, landing).is_ok()
                && self.landing_is_supported(player, landing))
            .then_some(landing)
        })
    }

    /// Where letting go now lands: straight down, or along the arc of
    /// `velocity` (the throw the release would give). `None` when no walkable
    /// floor is reached within a long fall, or the arc ends on a slope too
    /// steep to stand on: no landing can be promised.
    pub(crate) fn natural_landing(
        &self,
        player: &PlayerHandle,
        velocity: Option<Vector3<f32>>,
    ) -> Option<Landing> {
        let body = &self.rigid_body_set[player.character_handle];
        let shape = self.collider_set[body.colliders()[0]].shape();
        let filter = player_movement_filter(player.character_handle);
        let not_climbable =
            |_handle: ColliderHandle, collider: &Collider| collider_is_not_climbable(collider);
        let all_queries =
            self.player_movement_queries(self.narrow_phase.query_dispatcher(), filter);
        // Hanging beside a ladder: falling past it is not landing on it.
        let queries = all_queries.with_filter(filter.predicate(&not_climbable));
        let dt = self.integration_parameters.dt;
        let start = vec_to_nvec(self.get_player_next_translation(player));
        let mut position = start;
        let mut across = velocity.map_or_else(Vector::zeros, |v| vector![v.x, 0.0, v.z]);
        let mut rise = velocity.map(|v| v.y);
        let cast = |from: Vector<Real>, delta: Vector<Real>| {
            let distance = delta.norm();
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
                .map(|(_, hit)| {
                    (
                        from + delta / distance * hit.time_of_impact,
                        hit.normal1.into_inner(),
                    )
                })
        };
        // The body's lowest point already sits on the slope: no slope lift.
        let landed = |at: Vector<Real>, normal: Vector<Real>| {
            let floor_y = shape
                .compute_aabb(&Isometry::translation(at.x, at.y, at.z))
                .mins
                .y;
            Landing {
                normal: nvec_to_cgmath(normal),
                ..Self::landing_on(vector![at.x, floor_y, at.z], Vector::y(), true)
            }
        };
        for _ in 0..NATURAL_ARC_FRAMES {
            // Falling straight from here on: one cast finds the floor.
            if across.norm() < 1e-4 && rise.is_none_or(|v| v <= 0.0) {
                let depth = position.y - (start.y - NATURAL_MAX_DROP);
                let (at, normal) = cast(position, -Vector::y() * depth.max(1e-3))?;
                return is_walkable_normal(normal.y).then(|| landed(at, normal));
            }
            // An arc integrates like a jump.
            let mut v = rise.unwrap_or(0.0);
            let delta = across * dt + Vector::y() * v * dt;
            match cast(position, delta) {
                Some((at, normal)) => {
                    if delta.y < 0.0 && is_walkable_normal(normal.y) {
                        return Some(landed(at, normal));
                    }
                    // Like the controller: the drift slides along a wall and a
                    // ceiling ends the rise. Step off the face so the next
                    // cast does not start in contact with it.
                    position = at + normal * 1e-3;
                    let wall = vector![normal.x, 0.0, normal.z];
                    if wall.norm() > 1e-4 {
                        let wall = wall.normalize();
                        across -= wall * across.dot(&wall).min(0.0);
                    }
                    if normal.y < 0.0 {
                        v = v.min(0.0);
                    }
                }
                None => position += delta,
            }
            rise = Some(
                (v - PLAYER_JUMP_GRAVITY / SCALE_FACTOR
                    * dt
                    * self.world_time_scale.max(SLOW_TIME_JUMP_GRAVITY_FLOOR))
                .max(-PLAYER_MAX_FALL_SPEED / SCALE_FACTOR),
            );
        }
        None
    }

    /// Commit an assisted release: revalidate the glide to `landing` from the
    /// body's current pose and start it. `false` leaves the body untouched;
    /// the release is then an ordinary one.
    pub fn plan_assisted_release(&mut self, landing: Landing, player: &mut PlayerHandle) -> bool {
        if player.top_out.is_some() {
            return false;
        }
        let start = vec_to_nvec(self.get_player_next_translation(player));
        let Ok(route) = self.assisted_route(player, start, landing) else {
            return false;
        };
        if !self.landing_is_supported(player, landing) {
            return false;
        }
        self.start_hand_route(route, player);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEET_ABOVE_CENTER: f32 = 1.24;

    fn identity() -> Quaternion<f32> {
        Quaternion::new(1.0, 0.0, 0.0, 0.0)
    }

    /// A floor slab (top y=0) and a platform (top y=4) whose near edge is at
    /// x=1, with the player hanging at x=0 with feet at y=4.
    fn fixture(extra: &[(Vector3<f32>, Vector3<f32>)]) -> (PhysicsWorld, PlayerHandle) {
        let mut world = PhysicsWorld::new();
        let mut boxes = vec![
            (vec3(0.0, -0.5, 0.0), vec3(40.0, 1.0, 40.0)),
            (vec3(3.0, 2.0, 0.0), vec3(4.0, 4.0, 4.0)),
        ];
        boxes.extend_from_slice(extra);
        for (index, (center, size)) in boxes.into_iter().enumerate() {
            world.add_kinematic(
                EntityId::from_inner(100 + index as u64).unwrap(),
                center,
                identity(),
                vec3(0.0, 0.0, 0.0),
                size,
                CollisionGroup::entity(),
                false,
            );
        }
        let mut player = world.create_player(
            vec3(0.0, 4.0 + player_center_above_floor(false), 0.0),
            EntityId::from_inner(1).unwrap(),
        );
        world.update(vec3(0.0, 0.0, 0.0), &mut player);
        world.set_player_translation(vec3(0.0, 4.0 + FEET_ABOVE_CENTER, 0.0), &mut player);
        world.update(vec3(0.0, 0.0, 0.0), &mut player);
        // Hang where we were put: the queries read the queued pose.
        world.set_player_translation(vec3(0.0, 4.0 + FEET_ABOVE_CENTER, 0.0), &mut player);
        (world, player)
    }

    fn eye() -> Vector3<f32> {
        vec3(0.0, 4.0 + 1.24 + 1.0, 0.0)
    }

    fn aim(
        world: &PhysicsWorld,
        player: &PlayerHandle,
        at: Vector3<f32>,
        held: Option<Landing>,
    ) -> GazeLanding {
        let rays = climb_release::gaze_rays(at - eye());
        world.gaze_landing(player, eye(), &rays, 4.0, held)
    }

    #[test]
    fn the_aimed_platform_gives_an_inset_landing_the_body_fits() {
        let (world, player) = fixture(&[]);
        // Aimed right at the edge: the snap pulls the landing inward.
        let gaze = aim(&world, &player, vec3(1.05, 4.0, 0.0), None);
        let landing = gaze.landing.expect("platform");
        assert!((landing.floor_y - 4.0).abs() < 0.01);
        assert!(landing.center.x >= 1.0 + 0.48, "{landing:?}");
        assert!(landing.center.x - 1.05 <= 0.61);
    }

    #[test]
    fn far_high_and_obstructed_targets_are_rejected_with_a_reason() {
        // Too far: a deck point 3 units away.
        let (world, player) = fixture(&[]);
        let gaze = aim(&world, &player, vec3(3.5, 4.0, 0.0), None);
        assert_eq!(gaze.landing.err(), Some(GazeRejection::TooFar));

        // Too high: a step 1.5 above the feet.
        let (world, player) = fixture(&[(vec3(1.8, 4.75, 0.0), vec3(1.0, 1.5, 4.0))]);
        let gaze = aim(&world, &player, vec3(1.8, 5.5, 0.0), None);
        assert_eq!(gaze.landing.err(), Some(GazeRejection::TooHigh));

        // Low ceiling: a slab 1.2 over the platform leaves no headroom.
        let (world, player) = fixture(&[(vec3(3.0, 5.45, 0.0), vec3(4.0, 0.5, 4.0))]);
        let gaze = aim(&world, &player, vec3(2.0, 4.0, 0.0), None);
        assert_eq!(gaze.landing.err(), Some(GazeRejection::NoFit));

        // A post between body and landing blocks the glide (seen over it).
        let (world, player) = fixture(&[(vec3(0.75, 4.5, 0.0), vec3(0.2, 1.0, 4.0))]);
        let gaze = aim(&world, &player, vec3(1.7, 4.0, 0.0), None);
        assert_eq!(gaze.landing.err(), Some(GazeRejection::Blocked));
    }

    #[test]
    fn nothing_is_chosen_through_the_first_obstruction() {
        // A wall between eye and platform: the ray stops on the wall.
        let (world, player) = fixture(&[(vec3(0.8, 7.0, 0.0), vec3(0.2, 6.0, 4.0))]);
        let gaze = aim(&world, &player, vec3(1.7, 4.0, 0.0), None);
        assert_eq!(gaze.landing.err(), Some(GazeRejection::NotWalkable));
    }

    #[test]
    fn the_held_landing_survives_jitter_until_its_route_is_blocked() {
        let (world, player) = fixture(&[]);
        let first = aim(&world, &player, vec3(1.7, 4.0, 0.0), None)
            .landing
            .unwrap();
        let jittered = aim(&world, &player, vec3(1.9, 4.0, 0.2), Some(first));
        assert_eq!(jittered.landing, Ok(first), "same landing through jitter");

        let (world, player) = fixture(&[(vec3(0.75, 4.5, 0.0), vec3(0.2, 1.0, 4.0))]);
        let blocked = aim(&world, &player, vec3(1.9, 4.0, 0.2), Some(first));
        assert!(blocked.landing.is_err());
    }

    fn ledge(point: Vector3<f32>) -> ClimbGrip {
        ClimbGrip {
            kind: ClimbGripKind::Ledge,
            entity_id: None,
            point,
            normal: Vector3::unit_y(),
        }
    }

    #[test]
    fn a_held_floor_lands_on_it_past_the_hand_however_high_it_is() {
        // A deck 2 above the feet: beyond gaze reach, but held.
        let (mut world, mut player) = fixture(&[(vec3(3.0, 5.0, 0.0), vec3(4.0, 2.0, 4.0))]);
        let landing = world
            .held_floor_landing(&player, &ledge(vec3(1.1, 6.0, 0.0)), None)
            .expect("the held deck");
        assert!((landing.floor_y - 6.0).abs() < 0.01, "{landing:?}");
        assert!(landing.center.x > 1.1 + 0.3, "past the hand: {landing:?}");
        assert!(world.plan_assisted_release(landing, &mut player));
    }

    #[test]
    fn a_held_floor_that_cannot_take_the_body_or_is_not_a_floor_gives_nothing() {
        // A 0.3-wide sill: the hand holds it, the body does not fit on it.
        let (world, player) = fixture(&[(vec3(0.75, 5.0, 0.0), vec3(0.3, 2.0, 4.0))]);
        assert_eq!(
            world.held_floor_landing(&player, &ledge(vec3(0.75, 6.0, 0.0)), None),
            None
        );
        let wall = ClimbGrip {
            kind: ClimbGripKind::Surface,
            normal: vec3(-1.0, 0.0, 0.0),
            ..ledge(vec3(1.0, 5.0, 0.0))
        };
        let (world, player) = fixture(&[]);
        assert_eq!(world.held_floor_landing(&player, &wall, None), None);
    }

    #[test]
    fn a_held_landing_out_of_range_from_lower_feet_is_dropped() {
        let (world, player) = fixture(&[]);
        let first = aim(&world, &player, vec3(1.7, 4.0, 0.0), None)
            .landing
            .unwrap();
        // The same aim, but the feet are now 1.5 below the deck: too high.
        let rays = climb_release::gaze_rays(vec3(1.7, 4.0, 0.0) - eye());
        let lower = world.gaze_landing(&player, eye(), &rays, 2.5, Some(first));
        assert_eq!(lower.landing, Err(GazeRejection::TooHigh));
    }

    #[test]
    fn a_throw_into_a_wall_slides_along_it_like_the_controller() {
        // A wall just ahead in +x, above the platform's edge.
        let (world, player) = fixture(&[(vec3(1.0, 7.0, 0.0), vec3(0.2, 6.0, 20.0))]);
        let thrown = world
            .natural_landing(&player, Some(vec3(2.0, 1.0, 2.0)))
            .expect("floor");
        assert!(thrown.floor_y.abs() < 0.05, "{thrown:?}");
        assert!(thrown.center.z > 1.2, "kept sliding: {thrown:?}");
    }

    #[test]
    fn a_plain_release_drops_straight_down_and_a_throw_follows_its_arc() {
        let (world, player) = fixture(&[]);
        let drop = world.natural_landing(&player, None).expect("floor below");
        assert!(drop.floor_y.abs() < 0.05, "{drop:?}");
        assert!(drop.center.x.abs() < 0.01);
        // Thrown toward the platform, up and across: lands on it.
        let thrown = world
            .natural_landing(&player, Some(vec3(4.0, 4.0, 0.0)))
            .expect("platform");
        assert!((thrown.floor_y - 4.0).abs() < 0.05, "{thrown:?}");
        assert!(thrown.center.x > 1.0);
    }

    #[test]
    fn a_committed_glide_arrives_on_the_shown_landing_and_a_lower_one_by_falling() {
        for (aimed, expected_floor) in [(vec3(2.0, 4.0, 0.0), 4.0), (vec3(-1.5, 0.0, 0.0), 0.0)] {
            let (mut world, mut player) = fixture(&[]);
            let landing = aim(&world, &player, aimed, None).landing.expect("valid");
            assert!(world.plan_assisted_release(landing, &mut player));
            for _ in 0..240 {
                world.update(vec3(0.0, 0.0, 0.0), &mut player);
            }
            let at = world.get_player_translation(&player);
            assert!(!player.is_topping_out());
            assert!(
                (vec3(at.x, 0.0, at.z) - vec3(landing.center.x, 0.0, landing.center.z)).magnitude()
                    < 0.05,
                "arrived at {at:?}, shown {landing:?}"
            );
            assert!(
                (at.y - (expected_floor + player_center_above_floor(false))).abs() < 0.1,
                "{at:?}"
            );
        }
    }

    #[test]
    fn a_glide_newly_obstructed_before_commit_is_refused() {
        let (mut world, mut player) = fixture(&[]);
        let landing = aim(&world, &player, vec3(2.0, 4.0, 0.0), None)
            .landing
            .unwrap();
        world.add_kinematic(
            EntityId::from_inner(999).unwrap(),
            vec3(0.75, 4.5, 0.0),
            identity(),
            vec3(0.0, 0.0, 0.0),
            vec3(0.2, 1.0, 4.0),
            CollisionGroup::entity(),
            false,
        );
        world.update(vec3(0.0, 0.0, 0.0), &mut player);
        world.set_player_translation(vec3(0.0, 4.0 + FEET_ABOVE_CENTER, 0.0), &mut player);
        assert!(!world.plan_assisted_release(landing, &mut player));
        assert!(!player.is_topping_out());
    }
}
