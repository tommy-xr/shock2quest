use std::f32::consts::PI;

use cgmath::{
    point3, vec3, vec4, Deg, EuclideanSpace, Euler, InnerSpace, Matrix, Matrix3, Point3, Rad,
    Transform, Vector3,
};
use dark::properties::{PropCreature, PropTranslatingDoor};
use rand::{thread_rng, Rng};
use shipyard::{EntityId, Get, View, World};

use crate::{runtime_props::RuntimePropTransform, util};

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
    return v_creature_prop.contains(entity_id);
}
