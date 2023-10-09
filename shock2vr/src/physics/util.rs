use cgmath::{vec3, Matrix4, Point3, Quaternion, Vector3};
use rapier3d::{na::UnitQuaternion, prelude::*};

pub fn nvec_to_cgmath(vec: Vector<Real>) -> Vector3<f32> {
    Vector3 {
        x: vec.x,
        y: vec.y,
        z: vec.z,
    }
}

pub fn nvec_to_cgmath_point(vec: Vector<Real>) -> Point3<f32> {
    Point3 {
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
pub fn mat_to_nmat(matrix: Matrix4<f32>) -> nalgebra::Isometry3<f32> {
    // Extract rotational component as a cgmath Matrix3
    //let rot = matrix.truncate();
    // Extract rotation component from Matrix4
    let rot_matrix = cgmath::Matrix3::new(
        matrix.x.x, matrix.x.y, matrix.x.z, matrix.y.x, matrix.y.y, matrix.y.z, matrix.z.x,
        matrix.z.y, matrix.z.z,
    );

    // Convert to cgmath Quaternion
    let rot_quat: Quaternion<f32> = rot_matrix.into();
    // Convert the rotational component to a nalgebra Quaternion
    let na_quat = UnitQuaternion::new_normalize(nalgebra::Quaternion::new(
        rot_quat.s,
        rot_quat.v.x,
        rot_quat.v.y,
        rot_quat.v.z,
    ));

    // Extract the translational component
    let trans = matrix.w.truncate();
    let na_trans = vec_to_nvec(Vector3::new(trans.x, trans.y, trans.z));

    nalgebra::Isometry3::from_parts(na_trans.into(), na_quat)
}
