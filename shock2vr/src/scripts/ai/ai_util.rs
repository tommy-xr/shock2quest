use std::f32::consts::PI;

use cgmath::{
    point3, vec3, vec4, Deg, EuclideanSpace, InnerSpace, Matrix4, Point3,
    Quaternion, Rad, Rotation3, SquareMatrix, Transform, Vector3,
};
use dark::{properties::*};
use rand::{thread_rng, Rng};
use shipyard::{EntityId, Get, View, World};

use crate::{
    creature,
    runtime_props::{RuntimePropJointTransforms, RuntimePropTransform},
    scripts::{
        script_util::get_first_link_with_template_and_data, Effect,
    },
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
    // Project vectors onto the XZ plane
    // TRY 1: Quat to Euler
    // let diff = (b - a).normalize();
    // let quat = util::get_rotation_from_forward_vector(diff);
    // //let mat3 = quat.into();
    // let euler: Euler<Rad<f32>> = Euler::from(quat);

    // TRY 2: Quat to Euler
    // let diff = (b - a).normalize();
    // let before_quat = util::get_rotation_from_forward_vector(diff);
    // let quat = vec4(
    //     before_quat.v.x,
    //     before_quat.v.y,
    //     before_quat.v.z,
    //     before_quat.s,
    // );
    // let sinr_cosp = 2.0 * (quat.w * quat.x + quat.y * quat.z);
    // let cosr_cosp = 1.0 - 2.0 * (quat.x * quat.x + quat.y * quat.y);
    // let roll = Rad(sinr_cosp.atan2(cosr_cosp));

    // let sinp = 2.0 * (quat.w * quat.y - quat.z * quat.x);
    // let pitch = if sinp.abs() >= 1.0 {
    //     Rad(std::f32::consts::PI / 2.0 * sinp.signum())
    // } else {
    //     Rad(sinp.asin())
    // };

    // let siny_cosp = 2.0 * (quat.w * quat.z + quat.x * quat.y);
    // let cosy_cosp = 1.0 - 2.0 * (quat.y * quat.y + quat.z * quat.z);
    // let yaw = Rad(siny_cosp.atan2(cosy_cosp));

    // //(roll, pitch, yaw);
    // yaw.into()

    // Deg(euler.y.0 * 180.0 / PI)

    // // FROM:

    // // // Calculate the cosine of the angle
    // let a_proj = vec3(a.x, 0.0, a.z);
    // let b_proj = vec3(b.x, 0.0, b.z);
    // //let cos_theta = a_proj.dot(b_proj) / (a_proj.magnitude() * b_proj.magnitude());

    // let dot = a_proj.x * b_proj.x + a_proj.z * b_proj.z;
    // let det = a_proj.x * b_proj.z - a_proj.z * b_proj.x;

    // let angle = dot.atan2(det);
    // Rad(angle).into()

    // Another try
    let ang = -(b.z - a.z).atan2(b.x - a.x) + PI / 2.0;
    Rad(ang).into()

    // // Use atan2 to get the angle in radians taking into account the full 360 degrees
    // let angle = sin_theta.atan2(cos_theta);

    // // // Return the angle (in radians)
    // Deg(angle * 180.0 / PI)
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
        let forward = vec3(0.0, 0.0, -1.0);
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

        let transform = root_transform.0 * joint_transform;

        //let orientation = Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), Rad(PI / 2.0));
        let _position = joint_transform.transform_point(point3(0.0, 0.0, 0.0));

        let rotation = Quaternion::from_axis_angle(vec3(0.0, 1.0, 0.0), Deg(90.0));
        // TODO: This rotation is needed for some monsters? Like the droids?
        let _rot_matrix: Matrix4<f32> = Matrix4::from(rotation);

        // panic!("creating entity: {:?}", projectile_id);
        Effect::CreateEntity {
            template_id: projectile_id,
            position: forward * 0.75,
            // position: vec3(13.11, 0.382, 16.601),
            // orientation: rotation,
            orientation: Quaternion {
                v: vec3(0.0, 0.0, 0.0),
                s: 1.0,
            },
            velocity: vec3(0.0, 0.0, 0.0),
            // root_transform: transform * rot_matrix,
            root_transform: transform,
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
