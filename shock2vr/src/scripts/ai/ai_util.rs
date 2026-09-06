use std::{
    collections::{HashMap, HashSet},
    f32::consts::PI,
};

use cgmath::{
    Deg, EuclideanSpace, InnerSpace, Matrix4, Point3, Quaternion, Rad, Rotation, Rotation3,
    SquareMatrix, Transform, Vector3, point3, vec3, vec4,
};
use dark::{EnvSoundQuery, SCALE_FACTOR, properties::*};
use engine::audio::AudioHandle;
use rand::{Rng, thread_rng};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, UniqueView, View, World};

use crate::{
    creature,
    mission::{GlobalPathfinding, PlayerInfo, entity_creator::CreateEntityOptions},
    pathfinding::MovementHold,
    physics::{InternalCollisionGroups, PhysicsWorld},
    runtime_props::{RuntimePropJointTransforms, RuntimePropProxyEntity, RuntimePropTransform},
    scripts::{
        Effect,
        script_util::{
            get_all_links_of_type, get_first_link_of_type, get_first_link_with_template_and_data,
        },
    },
};

///
/// random_binomial
///
/// Returns a random number between -1 and 1, where values around 0 are more likely
/// Tell every watchdog that this AI is standing still on purpose (or is free
/// to move again). Published by the AI script BEFORE it steers - see
/// `MovementHold`.
pub fn publish_movement_hold(world: &World, entity_id: EntityId, hold: MovementHold) {
    if let Some(service) = world
        .borrow::<UniqueView<GlobalPathfinding>>()
        .ok()
        .and_then(|g| g.0.clone())
    {
        service.record_movement_hold(entity_id.inner(), hold);
    }
}

/// Why this AI is standing still this frame, if it is.
pub fn movement_hold(world: &World, entity_id: EntityId) -> MovementHold {
    world
        .borrow::<UniqueView<GlobalPathfinding>>()
        .ok()
        .and_then(|g| g.0.clone())
        .map(|service| service.movement_hold(entity_id.inner()))
        .unwrap_or_default()
}

pub fn random_binomial() -> f32 {
    let mut rng = thread_rng();
    let a = rng.gen_range(0.0..1.0);
    let b = rng.gen_range(0.0..1.0);
    a - b
}

/// Height to fall back on when the creature has no definition (world units)
const CREATURE_DEFAULT_HEIGHT: f32 = 6.5 / SCALE_FACTOR;

/// The creature's height in world units. Fractions of it (rather than fixed
/// feet) are what let the same reasoning work on a monkey and on a hybrid.
pub fn creature_height(world: &World, entity_id: EntityId) -> f32 {
    world
        .borrow::<View<PropCreature>>()
        .ok()
        .and_then(|v_creature| v_creature.get(entity_id).ok().map(|creature| creature.0))
        .and_then(crate::creature::get_creature_definition)
        .map(|definition| definition.bounding_size.y)
        .unwrap_or(CREATURE_DEFAULT_HEIGHT)
}

pub fn get_position_and_forward(
    world: &shipyard::World,
    entity_id: shipyard::EntityId,
) -> (Point3<f32>, Vector3<f32>) {
    let v_transform = world
        .borrow::<shipyard::View<RuntimePropTransform>>()
        .unwrap();

    let xform = v_transform.get(entity_id).unwrap().0;
    let position = xform.transform_point(point3(0.0, 0.0, 0.0));
    let forward = xform.transform_vector(vec3(0.0, 0.0, 1.0)).normalize();

    (position, forward)
}

/// Whether this AI is flagged to walk a patrol route (P$AI_Patrol = true).
pub fn is_patroller(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<View<PropAIPatrol>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().map(|p| p.0))
        .unwrap_or(false)
}

/// Current patrol destination stored in Dark's runtime-only relation. The
/// Links component is serialized and entity-remapped by the normal save path,
/// so this is also the alertness and save/load resume point.
pub fn current_patrol_point(
    world: &World,
    patroller: EntityId,
) -> Option<(EntityId, Vector3<f32>)> {
    let target = get_first_link_of_type(world, patroller, Link::AICurrentPatrol)?;
    entity_origin(world, target).map(|position| (target, position))
}

/// Whether `point` is a genuine end of chain - it authors no outgoing
/// `AIPatrol` link at all. Distinct from `next_patrol_point` returning None,
/// which also covers a link whose target was never instantiated or has no
/// transform. Only a true dead end may clear the authored `AI_Patrol` flag;
/// an unresolved route is a broken mission, not the end of a patrol.
pub fn is_patrol_dead_end(world: &World, point: EntityId) -> bool {
    // Deliberately inspects the authored links rather than going through
    // `get_first_link_of_type`, which drops links whose target was never
    // instantiated - exactly the case this must NOT report as an end of chain.
    world
        .borrow::<View<Links>>()
        .ok()
        .and_then(|links| {
            links.get(point).ok().map(|links| {
                !links
                    .to_links
                    .iter()
                    .any(|link| link.link == Link::AIPatrol)
            })
        })
        .unwrap_or(true)
}

/// The transform origin of an entity, if it has a runtime transform.
fn entity_origin(world: &World, entity_id: EntityId) -> Option<Vector3<f32>> {
    let v_transform = world.borrow::<View<RuntimePropTransform>>().ok()?;
    let xform = v_transform.get(entity_id).ok()?.0;
    Some(xform.transform_point(point3(0.0, 0.0, 0.0)).to_vec())
}

/// Initial patrol destination chosen the way Dark's `TargetNextPatrolObj`
/// does: find the `AIPatrol` link whose source is nearest to `from`, but head
/// directly to that link's destination. None if the mission has no usable
/// patrol edge. Scans all links, so call it only when beginning a route.
pub fn nearest_patrol_point(world: &World, from: Vector3<f32>) -> Option<(EntityId, Vector3<f32>)> {
    let v_links = world.borrow::<View<Links>>().ok()?;
    let v_transform = world.borrow::<View<RuntimePropTransform>>().ok()?;

    let mut best: Option<(EntityId, Vector3<f32>, f32)> = None;
    for (source, links) in (&v_links).iter().with_id() {
        let Ok(xform) = v_transform.get(source) else {
            continue;
        };
        let source_pos = xform.0.transform_point(point3(0.0, 0.0, 0.0)).to_vec();
        let dist_sq = (source_pos - from).magnitude2();
        for link in links
            .to_links
            .iter()
            .filter(|link| link.link == Link::AIPatrol)
        {
            let Some(target) = link.to_entity_id.map(|id| id.0) else {
                continue;
            };
            let Some(target_pos) = entity_origin(world, target) else {
                continue;
            };
            if best
                .map(|(_, _, best_sq)| dist_sq < best_sq)
                .unwrap_or(true)
            {
                best = Some((target, target_pos, dist_sq));
            }
        }
    }
    best.map(|(id, pos, _)| (id, pos))
}

fn connected_patrol_points(world: &World, start: EntityId) -> Vec<EntityId> {
    let Ok(v_links) = world.borrow::<View<Links>>() else {
        return Vec::new();
    };

    // One pass over the link storage builds both directions of the patrol
    // graph, so the walk below is O(nodes + edges) rather than rescanning
    // every entity's links once per node reached.
    let mut adjacency: HashMap<EntityId, Vec<EntityId>> = HashMap::new();
    for (source, links) in (&v_links).iter().with_id() {
        for target in links
            .to_links
            .iter()
            .filter(|link| link.link == Link::AIPatrol)
            .filter_map(|link| link.to_entity_id.map(|id| id.0))
        {
            adjacency.entry(source).or_default().push(target);
            adjacency.entry(target).or_default().push(source);
        }
    }

    let mut points = vec![start];
    let mut seen = HashSet::from([start]);
    let mut index = 0;
    while index < points.len() {
        let current = points[index];
        index += 1;

        for neighbor in adjacency.get(&current).into_iter().flatten() {
            if seen.insert(*neighbor) {
                points.push(*neighbor);
            }
        }
    }
    points
}

/// Pick the next patrol target. Ordinary routes choose uniformly among the
/// current point's outgoing branches. `AI_PtrlRnd` routes may jump to any
/// other node in the bidirectionally connected graph, matching Dark's random
/// sequence mode.
pub fn next_patrol_point(
    world: &World,
    patroller: EntityId,
    point: EntityId,
) -> Option<(EntityId, Vector3<f32>)> {
    next_patrol_point_with_rng(world, patroller, point, &mut thread_rng())
}

fn next_patrol_point_with_rng<R: Rng + ?Sized>(
    world: &World,
    patroller: EntityId,
    point: EntityId,
    rng: &mut R,
) -> Option<(EntityId, Vector3<f32>)> {
    let random_sequence = world
        .borrow::<View<PropAIPatrolRandom>>()
        .ok()
        .and_then(|random| random.get(patroller).ok().map(|value| value.0))
        .unwrap_or(false);
    let candidates = if random_sequence {
        connected_patrol_points(world, point)
            .into_iter()
            .filter(|candidate| *candidate != point)
            .filter_map(|candidate| entity_origin(world, candidate).map(|pos| (candidate, pos)))
            .collect::<Vec<_>>()
    } else {
        get_all_links_of_type(world, point, Link::AIPatrol)
            .into_iter()
            .filter_map(|candidate| entity_origin(world, candidate).map(|pos| (candidate, pos)))
            .collect::<Vec<_>>()
    };
    (!candidates.is_empty()).then(|| {
        let index = rng.gen_range(0..candidates.len());
        candidates[index]
    })
}

pub fn current_yaw(entity_id: shipyard::EntityId, world: &shipyard::World) -> Deg<f32> {
    let (point, forward) = get_position_and_forward(world, entity_id);
    let position = point.to_vec();
    yaw_between_vectors(position, position + forward)
}

#[cfg(test)]
mod patrol_tests {
    use std::collections::HashSet;

    use super::*;
    use cgmath::Matrix4;
    use rand::{SeedableRng, rngs::StdRng};
    use shipyard::{Get, ViewMut};

    fn point(world: &mut World, position: Vector3<f32>) -> EntityId {
        world.add_entity((
            RuntimePropTransform(Matrix4::from_translation(position)),
            Links::empty(),
        ))
    }

    fn link(world: &World, source: EntityId, dest: EntityId) {
        let mut links = world.borrow::<ViewMut<Links>>().unwrap();
        (&mut links).get(source).unwrap().to_links.push(ToLink {
            to_template_id: 0,
            to_entity_id: Some(WrappedEntityId(dest)),
            link: Link::AIPatrol,
        });
    }

    #[test]
    fn initial_patrol_target_is_destination_of_nearest_source() {
        let mut world = World::new();
        let near_source = point(&mut world, vec3(1.0, 0.0, 0.0));
        let near_dest = point(&mut world, vec3(20.0, 0.0, 0.0));
        let far_source = point(&mut world, vec3(5.0, 0.0, 0.0));
        let far_dest = point(&mut world, vec3(6.0, 0.0, 0.0));
        link(&world, near_source, near_dest);
        link(&world, far_source, far_dest);

        let (target, goal) = nearest_patrol_point(&world, vec3(0.0, 0.0, 0.0)).unwrap();

        assert_eq!(target, near_dest, "distance is measured to the link source");
        assert_eq!(
            goal,
            vec3(20.0, 0.0, 0.0),
            "the link destination is targeted"
        );
    }

    #[test]
    fn ordinary_patrol_can_take_every_outgoing_branch() {
        let mut world = World::new();
        let patroller = world.add_entity(PropAIPatrolRandom(false));
        let source = point(&mut world, vec3(0.0, 0.0, 0.0));
        let left = point(&mut world, vec3(-5.0, 0.0, 0.0));
        let right = point(&mut world, vec3(5.0, 0.0, 0.0));
        link(&world, source, left);
        link(&world, source, right);

        let mut rng = StdRng::seed_from_u64(410);
        let selected: HashSet<_> = (0..256)
            .filter_map(|_| {
                next_patrol_point_with_rng(&world, patroller, source, &mut rng).map(|(id, _)| id)
            })
            .collect();

        assert_eq!(selected, HashSet::from([left, right]));
    }

    #[test]
    fn random_sequence_can_target_any_other_node_in_connected_graph() {
        let mut world = World::new();
        let patroller = world.add_entity(PropAIPatrolRandom(true));
        let current = point(&mut world, vec3(0.0, 0.0, 0.0));
        let outgoing = point(&mut world, vec3(1.0, 0.0, 0.0));
        let descendant = point(&mut world, vec3(2.0, 0.0, 0.0));
        let upstream = point(&mut world, vec3(-1.0, 0.0, 0.0));
        link(&world, current, outgoing);
        link(&world, outgoing, descendant);
        link(&world, upstream, current);

        let mut rng = StdRng::seed_from_u64(410);
        let selected: HashSet<_> = (0..512)
            .filter_map(|_| {
                next_patrol_point_with_rng(&world, patroller, current, &mut rng).map(|(id, _)| id)
            })
            .collect();

        assert_eq!(selected, HashSet::from([outgoing, descendant, upstream]));
        assert!(!selected.contains(&current));
    }
}

pub fn clamp_to_minimal_delta_angle(ang: Deg<f32>) -> Deg<f32> {
    let mut ang = ang;
    while ang.0 > 180.0 {
        ang.0 -= 360.0;
    }
    while ang.0 < -180.0 {
        ang.0 += 360.0;
    }
    ang
}

pub fn yaw_between_vectors(a: Vector3<f32>, b: Vector3<f32>) -> Deg<f32> {
    // Another try
    let ang = -(b.z - a.z).atan2(b.x - a.x) + PI / 2.0;
    Rad(ang).into()
}

pub(crate) fn is_entity_door(world: &shipyard::World, entity_id: shipyard::EntityId) -> bool {
    let v_door_prop = world.borrow::<View<PropTranslatingDoor>>().unwrap();
    //let v_rot_door_prop = world.borrow::<View<PropRotating>>.unwrap();

    v_door_prop.contains(entity_id)
}

/// Fire Ranged Weapon
///
/// Handles firing a projectile through the AIRangedWeapon link, which is a proxy between the main entity link
/// Used primarily by turrets
///
pub fn fire_ranged_weapon(world: &World, entity_id: EntityId, rotation: Quaternion<f32>) -> Effect {
    // First, let's find the link
    let maybe_ranged_weapon = get_first_link_with_template_and_data(world, entity_id, |link| {
        if matches!(link, Link::AIRangedWeapon) {
            Some(())
        } else {
            None
        }
    });

    if maybe_ranged_weapon.is_none() {
        return Effect::NoEffect;
    }

    let ranged_weapon = maybe_ranged_weapon.unwrap().0;

    // Is there an entity aleady created for this link?

    let maybe_ranged_weapon_entity_id = find_first_entity_by_template_id(world, ranged_weapon);

    let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
    let root_transform = v_transform.get(entity_id).unwrap();
    let forward_offset = 3.0 / SCALE_FACTOR;
    let up_offset = 0.5 / SCALE_FACTOR;
    let right_offset = 0.5 / SCALE_FACTOR;
    let forward = vec3(right_offset, up_offset, 1.0 * forward_offset);
    let firing_transform = root_transform.0 * Matrix4::from(rotation);
    let muzzle_transform = firing_transform * Matrix4::from_translation(forward);
    let position = muzzle_transform.transform_point(point3(0.0, 0.0, 0.0));

    if maybe_ranged_weapon_entity_id.is_none() {
        // Let's create the proxy entity...
        Effect::CreateEntity {
            template_id: ranged_weapon,
            position: point3(0.0, 0.0, 0.0) + forward,
            orientation: Quaternion::from_angle_y(Deg(90.0)),
            root_transform: firing_transform,
            options: CreateEntityOptions::default(),
        }
    } else {
        let transformed_forward = firing_transform.transform_vector(forward);
        let debug_effect = Effect::DrawDebugLines {
            lines: vec![(
                position,
                position + transformed_forward * 10.0 + vec3(0.0, -0.25, 0.0),
                vec4(0.0, 1.0, 1.0, 1.0),
            )],
        };
        // We have the ranged weapon id, let's figure out its projectile
        let mut fire_effects = vec![debug_effect];

        let ranged_weapon_entity_id = maybe_ranged_weapon_entity_id.unwrap();
        let maybe_projectile = get_first_link_with_template_and_data(
            world,
            ranged_weapon_entity_id,
            |link| match link {
                Link::Projectile(data) => Some(*data),
                _ => None,
            },
        );

        if let Some((_projectile_id, _options)) = maybe_projectile {
            let (projectile_template_id, _projectile_opts) = maybe_projectile.unwrap();
            let projectile_transform =
                projectile_transform_aimed_at_player(world, position, muzzle_transform);

            fire_effects.push(Effect::CreateEntity {
                // Testing
                // template_id: -1415, // rocket turret
                // template_id: -1414, // laser turret
                template_id: projectile_template_id,
                position: point3(0.0, 0.0, 0.0),
                orientation: Quaternion::from_angle_y(Deg(90.0)),
                root_transform: projectile_transform,
                options: CreateEntityOptions::default(),
            });

            fire_effects.push(play_positional_sound(
                ranged_weapon_entity_id,
                world,
                Some(position.to_vec()),
                vec![("event", "shoot")],
            ));
        }

        let maybe_muzzle_flash = get_first_link_with_template_and_data(
            world,
            ranged_weapon_entity_id,
            |link| match link {
                Link::GunFlash(data) => Some(*data),
                _ => None,
            },
        );

        if let Some((muzzle_flash_template_id, _muzzle_flash_options)) = maybe_muzzle_flash {
            fire_effects.push(Effect::CreateEntity {
                template_id: muzzle_flash_template_id,
                position: point3(0.0, 0.0, 0.0) + forward,
                orientation: Quaternion::from_angle_y(Deg(90.0)),
                root_transform: firing_transform,
                options: CreateEntityOptions::default(),
            })
        }

        Effect::combine(fire_effects)
    }
}

fn find_first_entity_by_template_id(world: &World, ranged_weapon: i32) -> Option<EntityId> {
    let v_template_id = world.borrow::<View<PropTemplateId>>().unwrap();

    for (entity_id, template_id) in v_template_id.iter().with_id() {
        if template_id.template_id == ranged_weapon {
            return Some(entity_id);
        }
    }

    None
}

///
/// Fire Ranged Projectile
///
/// Handles firing a projectile from a ranged weapon, when that weapon is own directly by the creature.
/// Used by most creatures (robots, hybrids, midwives, etc)
///
pub fn fire_ranged_projectile(
    world: &World,
    physics: &PhysicsWorld,
    entity_id: EntityId,
) -> Effect {
    let maybe_projectile =
        get_first_link_with_template_and_data(world, entity_id, |link| match link {
            Link::AIProjectile(data) => Some(*data),
            _ => None,
        });

    let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
    let v_joint_transforms = world.borrow::<View<RuntimePropJointTransforms>>().unwrap();

    let v_creature = world.borrow::<View<PropCreature>>().unwrap();
    if let Some((projectile_id, options)) = maybe_projectile {
        let root_transform = v_transform.get(entity_id).unwrap();
        let forward = vec3(0.0, 0.0, 1.0);

        let creature_type = v_creature.get(entity_id).unwrap();
        let joint_index = creature::get_creature_definition(creature_type.0)
            .and_then(|def| def.get_mapped_joint(options.joint))
            .unwrap_or(0);
        let joint_transform = v_joint_transforms
            .get(entity_id)
            .map(|transform| transform.0.get(joint_index as usize))
            .ok()
            .flatten()
            .copied()
            .unwrap_or(Matrix4::identity());

        let transform = root_transform.0;
        let position = joint_transform.transform_point(point3(0.0, 0.0, 0.0));
        let muzzle_transform = transform * Matrix4::from_translation((position + forward).to_vec());
        let muzzle_position = muzzle_transform.transform_point(point3(0.0, 0.0, 0.0));
        let Some((target_entity, target_position)) = world
            .borrow::<UniqueView<PlayerInfo>>()
            .ok()
            .map(|player| (player.entity_id, player.pos))
        else {
            return Effect::NoEffect;
        };
        if !has_line_of_fire_from(
            entity_id,
            muzzle_position,
            target_entity,
            world,
            physics,
            target_position,
        ) {
            return Effect::NoEffect;
        }
        let projectile_transform =
            projectile_transform_aimed_at_player(world, muzzle_position, muzzle_transform);

        Effect::CreateEntity {
            template_id: projectile_id,
            position: point3(0.0, 0.0, 0.0),
            orientation: Quaternion::from_angle_y(Deg(90.0)),
            root_transform: projectile_transform,
            options: CreateEntityOptions::default(),
        }
    } else {
        Effect::NoEffect
    }
}

fn projectile_transform_aimed_at_player(
    world: &World,
    muzzle_position: Point3<f32>,
    fallback_transform: Matrix4<f32>,
) -> Matrix4<f32> {
    let Ok(player) = world.borrow::<UniqueView<PlayerInfo>>() else {
        return fallback_transform;
    };
    projectile_transform_aimed_at(muzzle_position, player.pos).unwrap_or(fallback_transform)
}

fn projectile_transform_aimed_at(
    muzzle_position: Point3<f32>,
    target_position: Vector3<f32>,
) -> Option<Matrix4<f32>> {
    // Fast hostile projectiles previously inherited only the attacker's yaw.
    // Their high authored joints could therefore send a perfectly horizontal
    // ray over the player's shorter capsule. Preserve the authored muzzle but
    // orient +Z at the collider center so the existing velocity path includes
    // the necessary pitch.
    let to_player = target_position - muzzle_position.to_vec();
    if to_player.magnitude2() <= 1.0e-12 {
        return None;
    }

    Some(
        Matrix4::from_translation(muzzle_position.to_vec())
            * Matrix4::from(crate::util::get_rotation_from_forward_vector(
                to_player.normalize(),
            )),
    )
}

/// Monster FOV half-angle, in degrees (matches `FovDebugConfig::monster()`).
/// Shared by sight checks and the melee connect check so an AI can only hit
/// what it could see.
pub const MONSTER_FOV_HALF_ANGLE: f32 = 60.0;

/// How close an AI must be to melee. The attack behavior uses this to decide
/// whether to keep swinging, and a swing may only connect inside it.
pub const MELEE_ATTACK_RANGE: f32 = 8.0 / SCALE_FACTOR;

///
/// Melee Contact
///
/// A melee swing reaching its contact frame. Dark authors the connect moment
/// on the animation itself - ShockEd's motion editor marks a damage window on
/// the attack clip, which the port surfaces as the `MELEE_CONTACT_START`
/// motion flag - so this runs off the animation, not a timer.
///
/// The blow itself is resolved entirely from the gamesys: the attacker's
/// `L$Weapon` archetype (`Lead Pipe`, `Rumbler Claw`, `Midwife Spike`, ...)
/// carries the `Contact` stim sources that describe what a landing hit does
/// (`WeaponBash` at an authored intensity, plus e.g. `Venom` for arachnids),
/// and the victim's own receptrons turn those into damage. Nothing here
/// invents a damage number.
///
/// The swing connects only if the player is actually within melee range and
/// inside the attacker's field of view - the AI swings at where it believes
/// the target is, but only reality can be hit.
pub fn melee_contact_attack(world: &World, entity_id: EntityId, physics: &PhysicsWorld) -> Effect {
    let Some((player_entity_id, player_pos)) = world
        .borrow::<UniqueView<PlayerInfo>>()
        .ok()
        .map(|player| (player.entity_id, player.pos))
    else {
        return Effect::NoEffect;
    };

    let in_range = {
        let v_current_pos = world.borrow::<View<PropPosition>>().unwrap();
        v_current_pos
            .get(entity_id)
            .ok()
            .map(|pos| {
                (pos.position + creature::sense_offset(world, entity_id) - player_pos).magnitude()
                    < MELEE_ATTACK_RANGE
            })
            .unwrap_or(false)
    };
    if !in_range {
        return Effect::NoEffect;
    }

    if !is_player_visible_in_fov(entity_id, world, physics, Deg(0.0), MONSTER_FOV_HALF_ANGLE) {
        return Effect::NoEffect;
    }

    let Some((weapon_template_id, _)) =
        get_first_link_with_template_and_data(world, entity_id, |link| match link {
            Link::Weapon => Some(()),
            _ => None,
        })
    else {
        return Effect::NoEffect;
    };

    let damage = crate::mission::stim_response::contact_stim_damage(
        world,
        weapon_template_id,
        player_entity_id,
    );
    if damage <= 0.0 {
        return Effect::NoEffect;
    }

    Effect::Send {
        msg: crate::scripts::Message {
            to: player_entity_id,
            payload: crate::scripts::MessagePayload::Damage {
                amount: damage,
                impact: None,
            },
        },
    }
}

/// Where this AI should chase: its last-known target position when it has
/// published awareness (frozen at the point sight broke), falling back to
/// the player's true position for entities without awareness (scripts that
/// don't track it, debug scenes).
pub fn chase_target(world: &World, entity_id: EntityId) -> Option<Vector3<f32>> {
    if let Ok(v_awareness) =
        world.borrow::<View<crate::runtime_props::RuntimePropAITargetAwareness>>()
    {
        if let Ok(awareness) = v_awareness.get(entity_id) {
            return Some(awareness.last_known_pos);
        }
    }
    world
        .borrow::<UniqueView<PlayerInfo>>()
        .ok()
        .map(|player| player.pos)
}

/// Distance from the entity to its chase target (the last-known position
/// when awareness is published, the player's true position otherwise).
/// Combat gates use this so attacks trigger against what the AI KNOWS,
/// consistent with where chase steering faces.
pub fn chase_target_distance(world: &World, entity_id: EntityId) -> Option<f32> {
    let target = chase_target(world, entity_id)?;
    let v_current_pos = world.borrow::<View<PropPosition>>().unwrap();
    let prop_pos = v_current_pos.get(entity_id).ok()?;
    Some((prop_pos.position - target).magnitude())
}

/// Current hit points, if the entity has a health pool
pub fn hit_points(entity_id: EntityId, world: &World) -> Option<i32> {
    let v_prop_hit_points = world.borrow::<View<PropHitPoints>>().unwrap();
    v_prop_hit_points.get(entity_id).ok().map(|p| p.hit_points)
}

/// AI-to-AI repel, the near-field half of `crowd_bias`: how hard one
/// neighbour pushes, ramping from nothing at `none` to a full push at
/// `full`.
pub struct CrowdRepel {
    /// At or inside this distance the push is at full strength
    pub full: f32,
    /// At or beyond this distance there is no push at all
    pub none: f32,
    /// Overall multiplier, so a caller can fade the whole term in and out
    /// smoothly (see the melee suppression in the path-follow strategy)
    pub strength: f32,
}

/// Repel strength for one neighbour: 0 at (or beyond) `none`, 1 at (or
/// inside) `full`, linear in between.
pub fn repel_ramp(distance: f32, full: f32, none: f32) -> f32 {
    if distance >= none {
        0.0
    } else if distance <= full {
        1.0
    } else {
        (none - distance) / (none - full)
    }
}

/// A push counts as head-on when it points back down the travel direction
/// within this cone of exactly anti-parallel. Wider and an ordinary
/// glancing push gets turned sideways; narrower and two bodies converging
/// a few degrees off dead-centre still cancel each other out.
const HEAD_ON_CONE: Deg<f32> = Deg(30.0);

/// The XZ direction of `v`, or None when it has no horizontal extent.
pub(crate) fn normalized_horizontal(v: Vector3<f32>) -> Option<Vector3<f32>> {
    let length = (v.x * v.x + v.z * v.z).sqrt();
    (length > 1e-6).then(|| vec3(v.x / length, 0.0, v.z / length))
}

/// Drop whatever part of `v` points against `direction`, keeping the rest.
/// Used both to keep a bias from pulling a body backwards down its route
/// and to stop a crowd push from bidding against the whiskers.
pub(crate) fn drop_opposing(v: Vector3<f32>, direction: Vector3<f32>) -> Vector3<f32> {
    match normalized_horizontal(direction) {
        Some(direction) => {
            let opposing = (v.x * direction.x + v.z * direction.z).min(0.0);
            v - direction * opposing
        }
        None => v,
    }
}

/// The AI's own side of the direction of travel: the perpendicular that
/// `yield_sideways` yields onto and that `blend_biases` applies the
/// geometry-over-crowd priority along. Both must read the same handedness -
/// the opposite one would zero exactly the sidesteps it has to preserve -
/// so they share this. Length follows `heading`'s; only its direction is used.
pub(crate) fn lateral_axis(heading: Vector3<f32>) -> Vector3<f32> {
    vec3(heading.z, 0.0, -heading.x)
}

/// Turn a head-on crowd push into a sidestep of the same strength.
///
/// A neighbour squarely ahead pushes straight back down the route, and a
/// bias pointing backwards is worth nothing: the aim point keeps only its
/// sideways part (`aim_with_bias`), so the whole push is discarded and two
/// AIs meeting head-on walk into each other. Yielding around is what the
/// original engine's regulator does, so inside `HEAD_ON_CONE` the push is
/// rotated onto the perpendicular instead of being thrown away.
///
/// Apply this to the WHOLE crowd bias, not to one neighbour's share: a
/// neighbour dead ahead and another abreast would otherwise yield onto the
/// same axis and cancel, leaving an AI with two neighbours worse off than
/// with one. Summed first, a push that still has somewhere sideways to go
/// falls outside the cone and is left exactly as it was.
///
/// The side is always the same one relative to the direction of travel,
/// which is what lets two AIs pass without coordinating: facing opposite
/// ways, their own sides are opposite sides of the corridor. Leaning toward
/// whichever side the push already favours reads as more natural and is
/// wrong - two bodies converging a few degrees off dead-centre lean the
/// same way in world space and stay nose to nose.
///
/// `avoid` is the static-geometry bias (the whiskers), which outranks the
/// choice of side: if the AI's own side is a wall, the other one is open by
/// construction, and both AIs of a pair read the same wall the same way.
pub(crate) fn yield_sideways(
    push: Vector3<f32>,
    heading: Vector3<f32>,
    avoid: Vector3<f32>,
) -> Vector3<f32> {
    let magnitude = (push.x * push.x + push.z * push.z).sqrt();
    if magnitude < 1e-6 {
        return push;
    }
    let Some(heading) = normalized_horizontal(heading) else {
        return push;
    };
    let along = push.x * heading.x + push.z * heading.z;
    if along >= 0.0 {
        return push; // not pointing back at all
    }
    // sin of the angle off exactly anti-parallel, ramped so the conversion
    // eases in rather than stepping the aim point by the whole offset
    // budget as a neighbour drifts across the edge of the cone.
    let lateral = push - heading * along;
    let lateral_length = (lateral.x * lateral.x + lateral.z * lateral.z).sqrt();
    let head_on = repel_ramp(
        lateral_length / magnitude,
        0.0,
        Rad::from(HEAD_ON_CONE).0.sin(),
    );
    if head_on <= 0.0 {
        return push;
    }
    let side = lateral_axis(heading);
    let side = if side.x * avoid.x + side.z * avoid.z < 0.0 {
        -side
    } else {
        side
    };
    push + (side * magnitude - push) * head_on
}

/// Horizontal crowd bias: the sum of repulsions from other LIVING
/// creatures near `position` (same floor). Returns a direction-and-
/// magnitude vector in the XZ plane; empty crowd = zero. Callers blend a
/// capped amount of this into their steering target so converging AIs bend
/// around each other instead of pushing capsule-to-capsule into a gridlock
/// (issue #487) - it must BIAS the route, never veto it (see collision
/// avoidance history).
///
/// Two terms over one neighbour walk:
/// - crowd separation, a mild proximity-weighted push over the whole
///   `separation_radius`, which keeps a group's lines from converging; and
/// - `repel` (optional), the near-field push modelled on the original
///   engine's object regulator, which is what actually stops bodies from
///   stacking up in a doorway.
///
/// The player is not a creature body (no `PropCreature`), so nothing here
/// ever pushes an AI off its chase target.
pub fn crowd_bias(
    world: &World,
    entity_id: EntityId,
    position: Vector3<f32>,
    separation_radius: f32,
    repel: Option<CrowdRepel>,
) -> Vector3<f32> {
    let v_creature = world.borrow::<View<PropCreature>>().unwrap();
    let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
    let v_hit_points = world.borrow::<View<PropHitPoints>>().unwrap();
    let reach = repel
        .as_ref()
        .map(|repel| separation_radius.max(repel.none))
        .unwrap_or(separation_radius);
    let mut bias = vec3(0.0, 0.0, 0.0);
    for (other_id, (_, xform)) in (&v_creature, &v_transform).iter().with_id() {
        if other_id == entity_id {
            continue;
        }
        // Corpses don't crowd (they're ragdolls on the floor)
        if v_hit_points
            .get(other_id)
            .map(|hp| hp.hit_points <= 0)
            .unwrap_or(false)
        {
            continue;
        }
        let other = xform.0.transform_point(point3(0.0, 0.0, 0.0));
        if (position.y - other.y).abs() > 2.0 {
            continue; // different floor
        }
        let dx = position.x - other.x;
        let dz = position.z - other.z;
        let distance = (dx * dx + dz * dz).sqrt();
        if distance >= reach || distance < 1e-3 {
            continue;
        }
        // Two ramps with different knees: separation fades in gently from
        // the whole radius, repel bites hard up close.
        let mut weight = repel_ramp(distance, 0.0, separation_radius);
        if let Some(repel) = &repel {
            weight += repel.strength * repel_ramp(distance, repel.full, repel.none);
        }
        if weight <= 0.0 {
            continue;
        }
        bias += vec3(dx / distance, 0.0, dz / distance) * weight;
    }
    bias
}

pub fn is_killed(entity_id: EntityId, world: &World) -> bool {
    let v_prop_hit_points = world.borrow::<View<PropHitPoints>>().unwrap();

    let maybe_prop_hit_points = v_prop_hit_points.get(entity_id);
    if maybe_prop_hit_points.is_err() {
        return false;
    }

    maybe_prop_hit_points.unwrap().hit_points <= 0
}

/// Whether this AI attacks by destroying itself: the `protocol` AI type (the
/// protocol droid), whose gamesys template carries no `L$Weapon` archetype to
/// resolve a blow with, but does link a `Corpse` explosion.
pub fn is_self_destructing(world: &World, entity_id: EntityId) -> bool {
    let v_ai = world.borrow::<View<PropAI>>().unwrap();
    v_ai.get(entity_id)
        .map(|prop_ai| prop_ai.0.eq_ignore_ascii_case("protocol"))
        .unwrap_or(false)
}

/// Check if an entity has a ranged weapon capability
///
/// Returns true if the entity has either an AIRangedWeapon link (used by turrets)
/// or an AIProjectile link (used by most creatures like robots, hybrids, midwives).
pub fn has_ranged_weapon(world: &World, entity_id: EntityId) -> bool {
    // Check for AIRangedWeapon (turret-style)
    let has_ai_ranged = get_first_link_with_template_and_data(world, entity_id, |link| {
        if matches!(link, Link::AIRangedWeapon) {
            Some(())
        } else {
            None
        }
    })
    .is_some();

    if has_ai_ranged {
        return true;
    }

    // Check for AIProjectile (creature-style)
    let has_ai_projectile = get_first_link_with_template_and_data(world, entity_id, |link| {
        if matches!(link, Link::AIProjectile(_)) {
            Some(())
        } else {
            None
        }
    })
    .is_some();

    has_ai_projectile
}

pub fn play_positional_sound(
    producing_entity: EntityId,
    world: &World,
    override_position: Option<Vector3<f32>>,
    tags: Vec<(&str, &str)>,
) -> Effect {
    let v_class_tag = world.borrow::<View<PropClassTag>>().unwrap();
    let mut class_tags = v_class_tag
        .get(producing_entity)
        .map(|p| p.class_tags())
        .unwrap_or(vec![]);

    let pos = match override_position {
        None => {
            let v_pos = world.borrow::<View<PropPosition>>().unwrap();
            v_pos.get(producing_entity).unwrap().position
        }
        Some(pos) => pos,
    };
    let mut query = tags;
    query.append(&mut class_tags);

    Effect::PlayEnvironmentalSound {
        audio_handle: AudioHandle::new(),
        query: EnvSoundQuery::from_tag_values(query),
        position: pos,
    }
}

/// Whether the player is psi-invisible: Photonic Redirection (`Inviso`) is
/// among the active sustained psi powers. An invisible player fails every
/// AI/camera/turret visibility check.
fn is_player_psi_invisible(world: &World) -> bool {
    world
        .borrow::<UniqueView<crate::psi::ActivePsiPowers>>()
        .map(|active| active.is_active(crate::psi::INVISO_TEMPLATE_ID))
        .unwrap_or(false)
}

const SIGHT_COLLISION_GROUPS: InternalCollisionGroups = InternalCollisionGroups::WORLD
    .union(InternalCollisionGroups::ENTITIES)
    .union(InternalCollisionGroups::SELECTABLE);

fn resolve_proxy_entity(world: &World, entity_id: EntityId) -> EntityId {
    world
        .borrow::<View<RuntimePropProxyEntity>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().map(|proxy| proxy.0))
        .unwrap_or(entity_id)
}

/// Whether an entity-backed physical surface should stop an AI sight ray.
///
/// The ray itself uses actor membership, so model-bounds colliders authored
/// only for frobbing/selecting are already excluded by the same collision
/// filter that lets creatures walk through them. Of the genuinely physical
/// surfaces that remain, fully rendered objects occlude while authored alpha
/// (the Windows archetype is 0.45) and non-rendered helpers pass sight. Proxy
/// hitboxes are classified through their visible owner.
///
/// Creatures never occlude, in any state. A packmate's capsule must not hide
/// the player from the AI behind it - a pack closing on the player would blind
/// everything but its front rank, leaving sight stricter than line of fire -
/// and the same has to hold once that packmate dies, or an AI would see
/// through the body standing up and be blinded by it a second later. Corpses
/// and ragdoll limbs are `SELECTABLE` rather than `ACTOR`, so deciding this
/// here rather than by dropping `ACTOR` from the collision mask is what covers
/// every state from one place. The original engine's sight cast tests world
/// geometry plus an explicit list of vision-blocking objects (doors and
/// anything authored to block AI vision); creatures are never in it. Line of
/// FIRE stays stricter and keeps reasoning about allies (see
/// `has_line_of_fire`): a packmate does not hide the player, but it is still a
/// reason not to shoot.
fn entity_occludes_sight(
    world: &World,
    observer: EntityId,
    target: EntityId,
    hit_entity: EntityId,
) -> bool {
    let entity_id = resolve_proxy_entity(world, hit_entity);
    if entity_id == observer || entity_id == target {
        return false;
    }

    let is_creature = world
        .borrow::<View<PropCreature>>()
        .map(|creatures| creatures.contains(entity_id))
        .unwrap_or(false);
    if is_creature {
        return false;
    }

    let is_not_rendered = world
        .borrow::<View<PropRenderType>>()
        .map(|render_types| {
            render_types.get(entity_id).is_ok_and(|render_type| {
                matches!(render_type.0, RenderType::NoRender | RenderType::EditorOnly)
            })
        })
        .unwrap_or(false);
    if is_not_rendered {
        return false;
    }

    world
        .borrow::<View<PropRenderAlpha>>()
        .map(|render_alpha| {
            render_alpha
                .get(entity_id)
                .map(|render_alpha| render_alpha.0 >= 1.0)
                .unwrap_or(true)
        })
        .unwrap_or(true)
}

fn has_clear_sight_between(
    observer: EntityId,
    target: EntityId,
    start_point: Point3<f32>,
    end_point: Point3<f32>,
    world: &World,
    physics: &PhysicsWorld,
) -> bool {
    let to_target = end_point - start_point;
    let distance_squared = to_target.magnitude2();
    if distance_squared <= 1.0e-12 {
        return true;
    }
    let distance = distance_squared.sqrt();
    let occludes = |hit_entity| entity_occludes_sight(world, observer, target, hit_entity);
    physics
        .ray_cast2_as_actor_with_entity_filter(
            start_point,
            to_target / distance,
            distance,
            SIGHT_COLLISION_GROUPS,
            Some(observer),
            true,
            &occludes,
        )
        .is_none()
}

/// Check if the player is visible from an entity (raycast only, no FOV check)
///
/// This is a basic visibility check that only verifies line-of-sight.
/// For FOV-aware visibility, use `is_player_visible_in_fov`.
pub fn is_player_visible(from_entity: EntityId, world: &World, physics: &PhysicsWorld) -> bool {
    if is_player_psi_invisible(world) {
        return false;
    }

    let u_player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
    let v_current_pos = world.borrow::<View<PropPosition>>().unwrap();

    if let Ok(ent_pos) = v_current_pos.get(from_entity) {
        let start_point =
            point3(0.0, 0.0, 0.0) + ent_pos.position + creature::sense_offset(world, from_entity);
        let end_point = point3(0.0, 0.0, 0.0) + u_player.pos;
        return has_clear_sight_between(
            from_entity,
            u_player.entity_id,
            start_point,
            end_point,
            world,
            physics,
        );
    };

    false
}

/// Whether `from_entity` has a clear line of FIRE to `target` (its known
/// aim point - see `chase_target`) - the occlusion gate for standing
/// ranged attacks. This intentionally remains stricter than sight: projectiles
/// collide with transparent glass and small physical props too, so an AI
/// allowed to stop and shoot through them stands rooted firing into the
/// obstacle for as long as its target stays known (issue #481's
/// stand-and-shoot freeze). Living allies block the shot, while an enemy
/// creature or the intended target counts as clear. This preserves
/// pursuit/attack against enemies without letting converged ranged AIs shoot
/// through their own side.
pub fn has_line_of_fire(
    from_entity: EntityId,
    world: &World,
    physics: &PhysicsWorld,
    target: Vector3<f32>,
) -> bool {
    if is_player_psi_invisible(world) {
        return false;
    }

    let v_current_pos = world.borrow::<View<PropPosition>>().unwrap();

    let Ok(ent_pos) = v_current_pos.get(from_entity) else {
        return false;
    };
    let start_point =
        point3(0.0, 0.0, 0.0) + ent_pos.position + creature::sense_offset(world, from_entity);
    let Some(target_entity) = world
        .borrow::<UniqueView<PlayerInfo>>()
        .ok()
        .map(|player| player.entity_id)
    else {
        return false;
    };
    has_line_of_fire_from(
        from_entity,
        start_point,
        target_entity,
        world,
        physics,
        target,
    )
}

fn has_line_of_fire_from(
    from_entity: EntityId,
    start_point: Point3<f32>,
    target_entity: EntityId,
    world: &World,
    physics: &PhysicsWorld,
    target: Vector3<f32>,
) -> bool {
    let end_point = point3(0.0, 0.0, 0.0) + target;
    if (end_point - start_point).magnitude2() <= 1.0e-12 {
        return true;
    }
    let direction = (end_point - start_point).normalize();
    let distance = (end_point - start_point).magnitude();
    let result = physics.ray_cast2(
        start_point,
        direction,
        distance,
        InternalCollisionGroups::WORLD
            | InternalCollisionGroups::ENTITIES
            | InternalCollisionGroups::SELECTABLE,
        Some(from_entity),
        true,
    );
    match result {
        None => true,
        Some(hit) => hit
            .maybe_entity_id
            .map(|id| {
                let id = resolve_proxy_entity(world, id);
                if target_entity == id {
                    return true;
                }

                let is_creature = world
                    .borrow::<View<PropCreature>>()
                    .map(|v| v.contains(id))
                    .unwrap_or(false);
                if !is_creature {
                    return false;
                }
                let is_living = world
                    .borrow::<View<PropHitPoints>>()
                    .ok()
                    .and_then(|v| v.get(id).ok().map(|hp| hp.hit_points > 0))
                    .unwrap_or(true);
                if !is_living {
                    return true;
                }

                ai_team(world, id) != ai_team(world, from_entity)
            })
            .unwrap_or(false),
    }
}

/// Dark defaults ordinary AIs to Bad 1; Good/Neutral/other Bad teams are
/// explicit `P$AI_Team` overrides (including the shipped Good Guy and Charmed
/// metaproperties). Equal teams are allies; a living creature on another team
/// is a valid hostile obstruction and does not suppress the shot.
fn ai_team(world: &World, entity_id: EntityId) -> AITeam {
    world
        .borrow::<View<PropAITeam>>()
        .ok()
        .and_then(|v| v.get(entity_id).ok().map(|team| team.0))
        .unwrap_or(AITeam::Bad1)
}

/// Check if the player is visible from an entity within a field of view
///
/// This combines a raycast check with an FOV cone check based on the entity's
/// current heading and FOV half-angle.
///
/// # Arguments
/// * `from_entity` - The entity doing the looking
/// * `world` - The ECS world
/// * `physics` - Physics world for raycasting
/// * `heading` - Additional rotation offset applied on top of `pose.rotation` (see below)
/// * `fov_half_angle` - Half of the field of view angle in degrees
///
/// # Heading Convention
/// The forward direction is calculated as:
/// ```ignore
/// orientation = pose.rotation * Quaternion::from_angle_y(-heading)
/// forward = orientation.rotate_vector(vec3(0.0, 0.0, 1.0))
/// ```
///
/// Different entity types require different heading values:
/// - **Monsters**: Pass `Deg(0.0)` - rotation is set via `Effect::SetRotation`, so
///   `pose.rotation` already contains the full orientation.
/// - **Cameras**: Pass `Deg(view_angle + 90.0)` - rotation is via joint transforms,
///   not entity rotation. The +90 offset aligns with the joint coordinate system.
/// - **Turrets**: Pass `-current_heading` - similar to cameras but with negated heading
///   due to how the turret joint rotation is calculated.
///
/// # Returns
/// `true` if the player is within the FOV cone AND there's line-of-sight
pub fn is_player_visible_in_fov(
    from_entity: EntityId,
    world: &World,
    physics: &PhysicsWorld,
    heading: Deg<f32>,
    fov_half_angle: f32,
) -> bool {
    if is_player_psi_invisible(world) {
        return false;
    }

    let u_player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
    let v_current_pos = world.borrow::<View<PropPosition>>().unwrap();

    if let Ok(ent_pos) = v_current_pos.get(from_entity) {
        let entity_pos = ent_pos.position;
        let player_pos = u_player.pos;

        // Calculate direction to player
        let to_player = player_pos - entity_pos;
        let to_player_2d = Vector3::new(to_player.x, 0.0, to_player.z);

        if to_player_2d.magnitude2() < 1e-6 {
            // Player is directly above/below - consider visible
            return is_player_visible(from_entity, world, physics);
        }

        let to_player_2d = to_player_2d.normalize();

        // Calculate entity's forward direction combining base rotation and heading offset
        // This matches the debug visualization in ai_debug_util::draw_debug_fov
        let orientation = ent_pos.rotation * Quaternion::from_angle_y(-heading);
        let forward_3d = orientation.rotate_vector(vec3(0.0, 0.0, 1.0));
        let forward = Vector3::new(forward_3d.x, 0.0, forward_3d.z).normalize();

        // Calculate angle between forward and direction to player
        let dot = forward.dot(to_player_2d);
        // Clamp dot product to valid range for acos
        let dot_clamped = dot.clamp(-1.0, 1.0);
        let angle_to_player = dot_clamped.acos().to_degrees();

        // Check if player is within FOV
        if angle_to_player > fov_half_angle {
            return false;
        }

        // Player is in FOV, now check line-of-sight
        let start_point =
            point3(0.0, 0.0, 0.0) + entity_pos + creature::sense_offset(world, from_entity);
        let end_point = point3(0.0, 0.0, 0.0) + player_pos;
        return has_clear_sight_between(
            from_entity,
            u_player.entity_id,
            start_point,
            end_point,
            world,
            physics,
        );
    };

    false
}

#[cfg(test)]
mod separation_tests {
    use super::*;
    use cgmath::Matrix4;
    use shipyard::World;

    /// No static geometry nearby, so the whiskers express no preference
    const NO_WALL: Vector3<f32> = vec3(0.0, 0.0, 0.0);

    fn spawn_creature(world: &mut World, at: Vector3<f32>, hit_points: i32) -> EntityId {
        world.add_entity((
            PropCreature(0),
            RuntimePropTransform(Matrix4::from_translation(at)),
            PropHitPoints { hit_points },
        ))
    }

    #[test]
    fn separation_pushes_away_from_a_close_neighbor() {
        let mut world = World::new();
        let me = spawn_creature(&mut world, vec3(0.0, 0.0, 0.0), 10);
        spawn_creature(&mut world, vec3(1.0, 0.0, 0.0), 10);

        let bias = crowd_bias(&world, me, vec3(0.0, 0.0, 0.0), 2.4, None);
        assert!(
            bias.x < -0.1,
            "neighbor at +x must push toward -x: {bias:?}"
        );
        assert_eq!(bias.y, 0.0, "bias is horizontal only");
        assert!(bias.z.abs() < 1e-6);
    }

    #[test]
    fn separation_ignores_corpses_far_neighbors_and_other_floors() {
        let mut world = World::new();
        let me = spawn_creature(&mut world, vec3(0.0, 0.0, 0.0), 10);
        spawn_creature(&mut world, vec3(1.0, 0.0, 0.0), 0); // corpse
        spawn_creature(&mut world, vec3(10.0, 0.0, 0.0), 10); // out of radius
        spawn_creature(&mut world, vec3(1.0, 5.0, 0.0), 10); // floor above

        let bias = crowd_bias(&world, me, vec3(0.0, 0.0, 0.0), 2.4, None);
        assert!(
            bias.magnitude() < 1e-6,
            "corpses, far and stacked-floor creatures must not repel: {bias:?}"
        );
    }

    #[test]
    fn repel_ramps_from_nothing_at_reach_to_full_up_close() {
        assert_eq!(repel_ramp(4.5, 1.5, 4.5), 0.0, "no push at the reach");
        assert_eq!(repel_ramp(9.0, 1.5, 4.5), 0.0, "none beyond it either");
        assert_eq!(repel_ramp(1.5, 1.5, 4.5), 1.0, "full push at the near end");
        assert_eq!(repel_ramp(0.1, 1.5, 4.5), 1.0, "and no more than full");
        assert!(
            (repel_ramp(3.0, 1.5, 4.5) - 0.5).abs() < 1e-6,
            "linear in between"
        );
    }

    #[test]
    fn repel_outpushes_separation_up_close() {
        let mut world = World::new();
        let me = spawn_creature(&mut world, vec3(0.0, 0.0, 0.0), 10);
        spawn_creature(&mut world, vec3(1.5, 0.0, 0.0), 10);

        let separation_only = crowd_bias(&world, me, vec3(0.0, 0.0, 0.0), 6.0, None);
        let with_repel = crowd_bias(
            &world,
            me,
            vec3(0.0, 0.0, 0.0),
            6.0,
            Some(CrowdRepel {
                full: 1.5,
                none: 4.5,
                strength: 1.0,
            }),
        );
        assert!(
            with_repel.x < separation_only.x - 0.5,
            "a neighbor inside the full-push distance must push much harder \
             than separation alone: {with_repel:?} vs {separation_only:?}"
        );

        // ...and a faded-out repel is exactly the separation bias again
        let faded = crowd_bias(
            &world,
            me,
            vec3(0.0, 0.0, 0.0),
            6.0,
            Some(CrowdRepel {
                full: 1.5,
                none: 4.5,
                strength: 0.0,
            }),
        );
        assert!(
            (faded.x - separation_only.x).abs() < 1e-6,
            "{faded:?} vs {separation_only:?}"
        );
    }

    #[test]
    fn separation_from_two_sides_partially_cancels() {
        let mut world = World::new();
        let me = spawn_creature(&mut world, vec3(0.0, 0.0, 0.0), 10);
        spawn_creature(&mut world, vec3(1.0, 0.0, 0.0), 10);
        spawn_creature(&mut world, vec3(-1.0, 0.0, 0.0), 10);

        let bias = crowd_bias(&world, me, vec3(0.0, 0.0, 0.0), 2.4, None);
        assert!(
            bias.x.abs() < 1e-6,
            "symmetric neighbors cancel on x: {bias:?}"
        );
    }

    /// A neighbour squarely ahead pushes straight backwards, and a
    /// backwards bias is discarded by the aim point - so head-on the crowd
    /// term used to be worth exactly nothing and the two bodies met.
    #[test]
    fn a_head_on_push_becomes_a_sidestep() {
        let stepped = yield_sideways(vec3(0.0, 0.0, -2.0), vec3(0.0, 0.0, 1.0), NO_WALL);
        assert!(
            stepped.x.abs() > 1.9,
            "a neighbour dead ahead must be stepped around, not pushed back \
             through: {stepped:?}"
        );
        assert!(
            stepped.z.abs() < 1e-6,
            "and the sidestep keeps nothing pointing back down the route: {stepped:?}"
        );
    }

    /// Two AIs walking into each other each yield to their own side, which
    /// is the opposite side of the corridor because they face opposite
    /// ways. This must hold when they are NOT exactly anti-parallel too:
    /// picking the side the push happens to lean toward looks natural but
    /// sends both of them the same way in world space (here both headings
    /// lean +x, e.g. two AIs aiming at the same off-centre doorway), which
    /// is the very nose-to-nose case this exists to break.
    #[test]
    fn two_ai_meeting_head_on_yield_to_opposite_sides() {
        // A stands at -z looking north, B stands at +z looking south, so
        // A's push (away from B) is -z and B's is its exact opposite.
        let push_a = vec3(0.0, 0.0, -1.0);
        let opposite = |heading_a: Vector3<f32>, heading_b: Vector3<f32>| {
            let a = yield_sideways(push_a, heading_a, NO_WALL);
            let b = yield_sideways(-push_a, heading_b, NO_WALL);
            assert!(
                a.x * b.x < 0.0,
                "opposed AIs must pass on opposite sides: {a:?} vs {b:?}"
            );
        };
        opposite(vec3(0.0, 0.0, 1.0), vec3(0.0, 0.0, -1.0));
        // ...and five degrees off, both leaning the same way in world x
        let lean = Rad::from(Deg(5.0_f32)).0;
        opposite(
            vec3(lean.sin(), 0.0, lean.cos()),
            vec3(lean.sin(), 0.0, -lean.cos()),
        );
    }

    /// The whiskers outrank the AI's own side: yielding into the wall they
    /// just found would be undone by `blend_biases` and the head-on fix
    /// would do nothing in a doorway, which is where it is needed most.
    #[test]
    fn a_wall_flips_the_side_the_ai_yields_to() {
        let push = vec3(0.0, 0.0, -1.0);
        let heading = vec3(0.0, 0.0, 1.0);
        let open = yield_sideways(push, heading, NO_WALL);
        let wall_on_that_side = yield_sideways(push, heading, -open);
        assert!(
            open.x * wall_on_that_side.x < 0.0,
            "a wall on the AI's own side must send it the other way: \
             {open:?} vs {wall_on_that_side:?}"
        );
    }

    /// A push that is not head-on is left exactly as it was - sideways and
    /// forward pushes already survive the aim point. This is also what
    /// keeps a crowd from cancelling itself: applied to the SUM, a
    /// neighbour dead ahead plus one abreast still has somewhere sideways
    /// to go and is not rotated onto that neighbour's axis.
    #[test]
    fn a_push_off_the_head_on_cone_is_untouched() {
        let heading = vec3(0.0, 0.0, 1.0);
        assert_eq!(
            yield_sideways(vec3(1.0, 0.0, 0.0), heading, NO_WALL),
            vec3(1.0, 0.0, 0.0)
        );
        assert_eq!(
            yield_sideways(vec3(0.0, 0.0, 1.0), heading, NO_WALL),
            vec3(0.0, 0.0, 1.0)
        );
        // one neighbour dead ahead (-z) plus one abreast (-x): 45 degrees
        // off anti-parallel, outside the cone, lateral escape preserved
        let crowd = yield_sideways(vec3(-1.0, 0.0, -1.0), heading, NO_WALL);
        assert_eq!(crowd, vec3(-1.0, 0.0, -1.0));
    }

    /// The conversion eases in across the cone rather than switching: at
    /// the edge the push is untouched, just inside it is barely changed.
    #[test]
    fn yielding_eases_in_across_the_cone() {
        let heading = vec3(0.0, 0.0, 1.0);
        let at_edge = Deg(30.0_f32);
        let just_inside = Deg(29.0_f32);
        let push = |off: Deg<f32>| {
            let off = Rad::from(off).0;
            vec3(off.sin(), 0.0, -off.cos())
        };
        let edge = yield_sideways(push(at_edge), heading, NO_WALL);
        let inside = yield_sideways(push(just_inside), heading, NO_WALL);
        assert_eq!(edge, push(at_edge), "untouched at the edge");
        assert!(
            (inside - push(just_inside)).magnitude() < 0.1,
            "and barely changed just inside it: {inside:?}"
        );
    }
}

#[cfg(test)]
mod projectile_aim_tests {
    use super::*;
    use cgmath::MetricSpace;

    #[test]
    fn hostile_projectiles_pitch_from_the_muzzle_toward_the_player() {
        let muzzle = point3(1.0, 3.0, 5.0);
        let player = vec3(-2.0, 1.0, -7.0);
        let transform =
            projectile_transform_aimed_at(muzzle, player).expect("distinct points define aim");

        let spawned_at = transform.transform_point(point3(0.0, 0.0, 0.0));
        assert!(
            spawned_at.distance(muzzle) < 1.0e-6,
            "aiming must preserve the authored muzzle position",
        );

        let actual_forward = transform.transform_vector(vec3(0.0, 0.0, 1.0)).normalize();
        let expected_forward = (player - muzzle.to_vec()).normalize();
        assert!(
            (actual_forward - expected_forward).magnitude() < 1.0e-6,
            "projectile direction must include pitch toward the player's collider",
        );
    }
}

#[cfg(test)]
mod line_of_fire_tests {
    use super::*;
    use crate::{
        mission::PlayerInfo, physics::CollisionGroup, runtime_props::RuntimePropTransform,
    };

    fn identity_rotation() -> Quaternion<f32> {
        Quaternion::from_sv(1.0, vec3(0.0, 0.0, 0.0))
    }

    fn line_of_fire_through(blocker_team: AITeam, blocker_hit_points: i32) -> bool {
        let mut world = World::new();
        let shooter = world.add_entity((
            PropCreature(0),
            PropAITeam(AITeam::Bad1),
            PropHitPoints { hit_points: 10 },
            PropPosition {
                position: vec3(0.0, 0.0, 0.0),
                cell: 0,
                rotation: identity_rotation(),
            },
            RuntimePropTransform(Matrix4::from_translation(vec3(0.0, 0.0, 0.0))),
        ));
        let blocker = world.add_entity((
            PropCreature(0),
            PropAITeam(blocker_team),
            PropHitPoints {
                hit_points: blocker_hit_points,
            },
            PropPosition {
                position: vec3(0.0, 0.0, 5.0),
                cell: 0,
                rotation: identity_rotation(),
            },
            RuntimePropTransform(Matrix4::from_translation(vec3(0.0, 0.0, 5.0))),
        ));
        let player = world.add_entity(());
        let inventory = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 10.0),
            rotation: identity_rotation(),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });

        let mut physics = PhysicsWorld::new();
        physics.add_kinematic(
            blocker,
            vec3(0.0, 0.0, 5.0),
            identity_rotation(),
            vec3(0.0, 0.0, 0.0),
            vec3(1.0, 2.0, 1.0),
            CollisionGroup::entity(),
            false,
        );
        let mut player_handle = physics.create_player(vec3(50.0, 50.0, 50.0), player);
        physics.update(vec3(0.0, 0.0, 0.0), &mut player_handle);

        has_line_of_fire(shooter, &world, &physics, vec3(0.0, 0.0, 10.0))
    }

    #[test]
    fn living_ally_blocks_line_of_fire() {
        assert!(!line_of_fire_through(AITeam::Bad1, 10));
    }

    #[test]
    fn living_enemy_does_not_block_line_of_fire() {
        assert!(line_of_fire_through(AITeam::Good, 10));
    }

    #[test]
    fn dead_ally_does_not_block_line_of_fire() {
        assert!(line_of_fire_through(AITeam::Bad1, 0));
    }

    #[test]
    fn intended_target_does_not_block_its_own_line_of_fire() {
        let mut world = World::new();
        let shooter = world.add_entity((
            PropCreature(0),
            PropAITeam(AITeam::Bad1),
            PropHitPoints { hit_points: 10 },
            PropPosition {
                position: vec3(0.0, 0.0, 0.0),
                cell: 0,
                rotation: identity_rotation(),
            },
        ));
        let target = world.add_entity(());
        let inventory = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 5.0),
            rotation: identity_rotation(),
            entity_id: target,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });

        let mut physics = PhysicsWorld::new();
        physics.add_kinematic(
            target,
            vec3(0.0, 0.0, 5.0),
            identity_rotation(),
            vec3(0.0, 0.0, 0.0),
            vec3(1.0, 2.0, 1.0),
            CollisionGroup::entity(),
            false,
        );
        let mut player_handle = physics.create_player(vec3(50.0, 50.0, 50.0), target);
        physics.update(vec3(0.0, 0.0, 0.0), &mut player_handle);

        assert!(has_line_of_fire(
            shooter,
            &world,
            &physics,
            vec3(0.0, 0.0, 5.0),
        ));
    }
}

#[cfg(test)]
mod creature_height_tests {
    use super::*;

    /// A creature's authored bounding box is already in world units - the same
    /// units the no-definition fallback is written in, and the units the
    /// physics capsule is built from. Measuring a hybrid at a third of its
    /// height let it walk under a door leaf that had barely left the floor.
    #[test]
    fn a_creature_is_measured_in_the_same_units_as_the_fallback() {
        let mut world = World::new();
        // 0 is the human schema, whose bounding box is the 6.5 feet the
        // fallback also uses.
        let human = world.add_entity((PropCreature(0),));
        assert!(
            (creature_height(&world, human) - CREATURE_DEFAULT_HEIGHT).abs() < 1e-3,
            "human measured {} against a {} fallback",
            creature_height(&world, human),
            CREATURE_DEFAULT_HEIGHT
        );
    }
}

#[cfg(test)]
mod sight_occlusion_tests {
    use super::*;
    use crate::{
        mission::PlayerInfo,
        physics::{CollisionGroup, PlayerHandle},
    };

    struct SightScene {
        world: World,
        physics: PhysicsWorld,
        observer: EntityId,
        blocker: EntityId,
        player_handle: PlayerHandle,
    }

    fn identity_rotation() -> Quaternion<f32> {
        Quaternion::from_sv(1.0, vec3(0.0, 0.0, 0.0))
    }

    fn sight_scene(group: CollisionGroup) -> SightScene {
        let mut world = World::new();
        let observer = world.add_entity(PropPosition {
            position: vec3(0.0, 0.0, 0.0),
            cell: 0,
            rotation: identity_rotation(),
        });
        let blocker = world.add_entity(());
        let player = world.add_entity(());
        let inventory = world.add_entity(());
        world.add_unique(PlayerInfo {
            pos: vec3(0.0, 0.0, 10.0),
            rotation: identity_rotation(),
            entity_id: player,
            left_hand_entity_id: None,
            right_hand_entity_id: None,
            inventory_entity_id: inventory,
        });

        let mut physics = PhysicsWorld::new();
        physics.add_kinematic(
            blocker,
            vec3(0.0, 0.0, 5.0),
            identity_rotation(),
            vec3(0.0, 0.0, 0.0),
            vec3(1.0, 2.0, 1.0),
            group,
            false,
        );
        let mut player_handle = physics.create_player(vec3(50.0, 50.0, 50.0), player);
        physics.update(vec3(0.0, 0.0, 0.0), &mut player_handle);

        SightScene {
            world,
            physics,
            observer,
            blocker,
            player_handle,
        }
    }

    fn player_is_visible(scene: &SightScene) -> bool {
        is_player_visible(scene.observer, &scene.world, &scene.physics)
    }

    fn player_is_in_line_of_fire(scene: &SightScene) -> bool {
        has_line_of_fire(
            scene.observer,
            &scene.world,
            &scene.physics,
            vec3(0.0, 0.0, 10.0),
        )
    }

    #[test]
    fn opaque_solid_entity_occludes_sight() {
        let scene = sight_scene(CollisionGroup::entity());

        assert!(
            !player_is_visible(&scene),
            "a solid entity collider between the observer and player must block sight"
        );
    }

    #[test]
    fn opaque_cover_hands_ranged_attack_back_to_pursuit() {
        let mut scene = sight_scene(CollisionGroup::entity());
        scene.world.add_component(
            scene.observer,
            Links {
                to_links: vec![ToLink {
                    to_template_id: -1,
                    to_entity_id: None,
                    link: Link::AIRangedWeapon,
                }],
            },
        );

        assert!(
            crate::scripts::ai::behavior::attack_behavior_for_distance(
                &scene.world,
                &scene.physics,
                scene.observer,
            )
            .is_none(),
            "closed cover must make the ranged behavior resume pursuit",
        );

        scene.physics.set_position_rotation2(
            scene.blocker,
            vec3(10.0, 0.0, 5.0),
            identity_rotation(),
        );
        scene
            .physics
            .update(vec3(0.0, 0.0, 0.0), &mut scene.player_handle);

        let behavior = crate::scripts::ai::behavior::attack_behavior_for_distance(
            &scene.world,
            &scene.physics,
            scene.observer,
        )
        .expect("open line of fire should select the ranged attack");
        assert_eq!(behavior.borrow().name(), "RangedAttack");
    }

    #[test]
    fn authored_transparency_passes_sight_but_still_blocks_projectiles() {
        let mut scene = sight_scene(CollisionGroup::entity());
        scene
            .world
            .add_component(scene.blocker, PropRenderAlpha(0.45));

        assert!(
            player_is_visible(&scene),
            "the shipped Windows alpha must remain transparent to sight"
        );
        assert!(
            !player_is_in_line_of_fire(&scene),
            "transparent physical cover must keep the projectile gate unchanged"
        );
    }

    #[test]
    fn a_living_creature_does_not_occlude_sight_but_does_block_the_shot() {
        let mut scene = sight_scene(CollisionGroup::actor());
        scene.world.add_component(
            scene.blocker,
            (PropCreature(0), PropHitPoints { hit_points: 10 }),
        );

        assert!(
            player_is_visible(&scene),
            "a packmate standing in the way must not hide the player"
        );
        assert!(
            !player_is_in_line_of_fire(&scene),
            "a living ally in the way must still suppress the shot"
        );
    }

    /// A corpse keeps its creature identity but swaps `ACTOR` membership for
    /// `SELECTABLE`. Today it is already unreachable by an actor-membership
    /// ray, so this passes for two independent reasons - it is here to catch a
    /// future regrouping that puts bodies back in the ray's path and leaves an
    /// AI seeing through a packmate standing up but blinded by its corpse.
    #[test]
    fn a_creature_corpse_does_not_occlude_sight_either() {
        let mut scene = sight_scene(CollisionGroup::corpse());
        scene.world.add_component(
            scene.blocker,
            (PropCreature(0), PropHitPoints { hit_points: 0 }),
        );

        assert!(
            player_is_visible(&scene),
            "a body on the floor must not hide the player any more than it did standing"
        );
    }

    #[test]
    fn interaction_only_selectable_bounds_do_not_occlude_sight() {
        let scene = sight_scene(CollisionGroup::selectable().non_solid_to_characters());

        assert!(
            player_is_visible(&scene),
            "a collider retained only for frobbing must not become visual cover"
        );
        assert!(
            !player_is_in_line_of_fire(&scene),
            "the generic projectile query must remain independent of sight"
        );
    }

    #[test]
    fn transparent_proxy_is_classified_through_its_owner() {
        let mut scene = sight_scene(CollisionGroup::entity());
        let visible_owner = scene.world.add_entity(PropRenderAlpha(0.45));
        scene
            .world
            .add_component(scene.blocker, RuntimePropProxyEntity(visible_owner));

        assert!(
            player_is_visible(&scene),
            "a transparent owner must not become opaque through a physics proxy"
        );
    }

    #[test]
    fn non_rendered_physics_helpers_do_not_occlude_sight() {
        let mut scene = sight_scene(CollisionGroup::entity());
        scene
            .world
            .add_component(scene.blocker, PropRenderType(RenderType::NoRender));

        assert!(player_is_visible(&scene));
    }

    #[test]
    fn a_closed_standard_door_occludes_and_its_open_pose_restores_sight() {
        let mut scene = sight_scene(CollisionGroup::entity());
        scene.world.add_component(
            scene.blocker,
            PropTranslatingDoor {
                door_type: 0,
                closed: 0.0,
                open: 10.0,
                speed: 1.0,
                axis: 2,
                state: 0,
                base_closed_location: vec3(0.0, 0.0, 5.0),
                base_open_location: vec3(10.0, 0.0, 5.0),
                base_location: vec3(0.0, 0.0, 5.0),
            },
        );

        assert!(!player_is_visible(&scene), "the closed door must occlude");

        scene.physics.set_position_rotation2(
            scene.blocker,
            vec3(10.0, 0.0, 5.0),
            identity_rotation(),
        );
        scene
            .physics
            .update(vec3(0.0, 0.0, 0.0), &mut scene.player_handle);

        assert!(
            player_is_visible(&scene),
            "moving the door collider to its open pose must restore sight"
        );
    }
}
