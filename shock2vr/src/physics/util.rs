use cgmath::{vec3, Point3, Quaternion, Vector3};
use rapier3d::{na::UnitQuaternion, prelude::*};

pub fn nvec_to_cgmath(vec: Vector<Real>) -> Vector3<f32> {
    Vector3 {
        x: vec.x,
        y: vec.y,
        z: vec.z,
    }
}


pub fn npoint_to_cgmath(point: Point<Real>) -> Point3<f32> {
    Point3 {
        x: point.x,
        y: point.y,
        z: point.z,
    }
}
pub fn npoint_to_cgvec(point: Point<Real>) -> Vector3<f32> {
    Vector3 {
        x: point.x,
        y: point.y,
        z: point.z,
    }
}

pub fn vec_to_npoint(vec: Vector3<f32>) -> Point<Real> {
    point![vec.x, vec.y, vec.z]
}

pub fn vec_to_nvec(vec: Vector3<f32>) -> Vector<Real> {
    vector![vec.x, vec.y, vec.z]
}

pub fn nquat_to_quat(quat: nalgebra::UnitQuaternion<f32>) -> cgmath::Quaternion<f32> {
    cgmath::Quaternion {
        v: vec3(quat.i, quat.j, quat.k),

        s: quat.w,
    }
}

pub fn quat_to_nquat(facing: Quaternion<f32>) -> nalgebra::UnitQuaternion<f32> {
    let nquat = nalgebra::geometry::Quaternion::new(facing.s, facing.v.x, facing.v.y, facing.v.z);
    UnitQuaternion::from_quaternion(nquat)
}
