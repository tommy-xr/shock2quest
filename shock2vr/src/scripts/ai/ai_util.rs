use std::f32::consts::PI;

use cgmath::{
    Deg, EuclideanSpace, InnerSpace, Matrix4, Point3, Quaternion, Rad, Rotation3, SquareMatrix,
    Transform, Vector3, point3, vec3, vec4,
};
use dark::{EnvSoundQuery, SCALE_FACTOR, properties::*};
use engine::audio::AudioHandle;
use rand::{Rng, thread_rng};
use shipyard::{EntityId, Get, IntoIter, IntoWithId, UniqueView, View, World};

use crate::{
    creature,
    mission::{PlayerInfo, entity_creator::CreateEntityOptions},
    physics::{InternalCollisionGroups, PhysicsWorld},
    runtime_props::{RuntimePropJointTransforms, RuntimePropTransform},
    scripts::{Effect, script_util::get_first_link_with_template_and_data},
    util,
};

///
/// random_binomial
///
/// Returns a random number between -1 and 1, where values around 0 are more likely
pub fn random_binomial() -> f32 {
    let mut rng = thread_rng();
    let a = rng.gen_range(0.0..1.0);
    let b = rng.gen_range(0.0..1.0);
    a - b
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

pub fn current_yaw(entity_id: shipyard::EntityId, world: &shipyard::World) -> Deg<f32> {
    let (point, forward) = get_position_and_forward(world, entity_id);
    let position = point.to_vec();
    yaw_between_vectors(position, position + forward)
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

pub(crate) fn does_entity_have_hitboxes(world: &World, entity_id: EntityId) -> bool {
    let v_creature_prop = world.borrow::<View<PropCreature>>().unwrap();

    // If the entity has a creature prop, we use hitboxes for damage
    v_creature_prop.contains(entity_id)
}

pub fn draw_debug_facing_line(world: &World, entity_id: EntityId) -> Effect {
    let xform = world
        .borrow::<View<RuntimePropTransform>>()
        .unwrap()
        .get(entity_id)
        .unwrap()
        .0;

    let position = util::get_position_from_matrix(&xform);
    let forward = xform.transform_vector(vec3(0.0, 0.0, 1.0)).normalize();

    Effect::DrawDebugLines {
        lines: vec![(
            position + vec3(0.0, 0.5, 0.0),
            position + forward + vec3(0.0, 0.5, 0.0),
            vec4(1.0, 0.0, 0.0, 1.0),
        )],
    }
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
    let position =
        root_transform
            .0
            .transform_point(point3(right_offset, up_offset, forward_offset))
            + forward;

    if maybe_ranged_weapon_entity_id.is_none() {
        // Let's create the proxy entity...
        Effect::CreateEntity {
            template_id: ranged_weapon,
            position,
            orientation: rotation,
            root_transform: root_transform.0,
            options: CreateEntityOptions::default(),
        }
    } else {
        let rot_matrix: Matrix4<f32> = rotation.into();
        let transformed_forward = root_transform.0.transform_vector(forward);
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

            fire_effects.push(Effect::CreateEntity {
                // Testing
                // template_id: -1415, // rocket turret
                // template_id: -1414, // laser turret
                template_id: projectile_template_id,
                position: point3(0.0, 0.0, 0.0) + forward,
                orientation: Quaternion::from_angle_y(Deg(90.0)),
                root_transform: root_transform.0 * rot_matrix,
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
                root_transform: root_transform.0 * rot_matrix,
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
pub fn fire_ranged_projectile(world: &World, entity_id: EntityId) -> Effect {
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
        let _up = vec3(0.0, 1.0, 0.0);

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
        //let transform = root_transform.0 * joint_transform;

        //let orientation = Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), Rad(PI / 2.0));
        let position = joint_transform.transform_point(point3(0.0, 0.0, 0.0));

        //let rotation = Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), Deg(90.0));
        // TODO: This rotation is needed for some monsters? Like the droids?
        //let _rot_matrix: Matrix4<f32> = Matrix4::from(rotation);

        // panic!("creating entity: {:?}", projectile_id);
        Effect::CreateEntity {
            template_id: projectile_id,
            position: position + forward * 1.0,
            // position: vec3(13.11, 0.382, 16.601),
            // orientation: rotation,
            // Not sure why, but it seems like the orientation of the AI models is off by 90 degrees for the bin models...
            // so we have to corect, otherwise we get sideways lasers
            // orientation: Quaternion::from_angle_y(Deg(180.0)),
            // orientation: Quaternion {
            //     s: 1.0,
            //     v: vec3(0.0, 0.0, 0.0),
            // },
            orientation: Quaternion::from_angle_y(Deg(90.0)),
            // root_transform: transform * rot_matrix,
            root_transform: transform,
            options: CreateEntityOptions::default(),
        }
    } else {
        Effect::NoEffect
    }
}

pub fn is_killed(entity_id: EntityId, world: &World) -> bool {
    let v_prop_hit_points = world.borrow::<View<PropHitPoints>>().unwrap();

    let maybe_prop_hit_points = v_prop_hit_points.get(entity_id);
    if maybe_prop_hit_points.is_err() {
        return false;
    }

    maybe_prop_hit_points.unwrap().hit_points <= 0
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

pub fn is_player_visible(from_entity: EntityId, world: &World, physics: &PhysicsWorld) -> bool {
    let u_player = world.borrow::<UniqueView<PlayerInfo>>().unwrap();
    let v_current_pos = world.borrow::<View<PropPosition>>().unwrap();

    if let Ok(ent_pos) = v_current_pos.get(from_entity) {
        let start_point = point3(0.0, 0.0, 0.0) + ent_pos.position;
        let end_point = point3(0.0, 0.0, 0.0) + u_player.pos;
        let direction = (end_point - start_point).normalize();
        let distance = (end_point - start_point).magnitude();
        let result = physics.ray_cast2(
            start_point,
            direction,
            distance,
            InternalCollisionGroups::WORLD,
            Some(from_entity),
            true,
        );

        // If we didn't hit anything - player visible!
        // Currently, the ray cast doesn't intersect player...
        // TODO: Check for entities, but pass-through transparent ones (ie, glass/windows)
        return result.is_none();
    };

    false
}
