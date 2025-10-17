use std::{collections::HashMap, fmt};

use cgmath::{
    point3, vec2, vec3, InnerSpace, Matrix3, Matrix4, Point2, Point3, Quaternion, Transform,
    Vector2, Vector3,
};

use dark::properties::{Link, Links, PropHasRefs, PropPosition};
use engine::game_log;
use shipyard::{Component, EntityId, Get, IntoIter, IntoWithId, View, World};
use tracing::warn;

use crate::runtime_props::{RuntimePropProxyEntity, RuntimePropTransform};

/// log_property
///
/// Helper function to log a specific property of the world, with extra context
pub fn log_property<T>(world: &World)
where
    T: Component + fmt::Debug + Sync + Send,
{
    world.run(
        |v_template_id: View<dark::properties::PropTemplateId>,
         v_objname: View<dark::properties::PropObjName>,
         v_symname: View<dark::properties::PropSymName>,
         v_property: View<T>| {
            for (id, door) in (&v_property).iter().with_id() {
                let maybe_template_id = v_template_id.get(id);
                let maybe_sym_name = v_symname.get(id);
                let maybe_obj_name = v_objname.get(id);
                game_log!(DEBUG, "Entity {id:?} [{maybe_template_id:?}|{maybe_sym_name:?}|{maybe_obj_name:?}] prop: {door:?}");
            }
        },
    );
}

pub fn log_entities_with_link<F>(world: &World, should_log_link: F)
where
    F: Fn(&Link) -> bool,
{
    world.run(
        |v_template_id: View<dark::properties::PropTemplateId>,
         v_objname: View<dark::properties::PropObjName>,
         v_symname: View<dark::properties::PropSymName>,
         v_links: View<Links>| {
            for (id, links) in (&v_links).iter().with_id() {
                let maybe_template_id = v_template_id.get(id);
                let maybe_sym_name = v_symname.get(id);
                let maybe_obj_name = v_objname.get(id);

                for link in &links.to_links {
                    if should_log_link(&link.link) {
                        game_log!(
                            DEBUG,
                            "Entity {:?} [{:?}|{:?}|{:?}] link: {:?}",
                            id, maybe_template_id, maybe_sym_name, maybe_obj_name, link.link
                        );
                    }
                }
            }
        },
    );
}

pub fn log_entity(world: &World, id: EntityId) {
    world.run(
        |v_template_id: View<dark::properties::PropTemplateId>,
         v_symname: View<dark::properties::PropSymName>,
         v_objname: View<dark::properties::PropObjName>,
         v_objshortname: View<dark::properties::PropObjShortName>,
         v_scripts: View<dark::properties::PropScripts>,
         v_links: View<dark::properties::Links>| {
            let maybe_template_id = v_template_id.get(id);
            let maybe_sym_name = v_symname.get(id);
            let maybe_obj_name = v_objname.get(id);
            let maybe_obj_short_name = v_objshortname.get(id);
            let maybe_links = v_links.get(id);
            let maybe_scripts = v_scripts.get(id);
            game_log!(DEBUG, "Entity {id:?}:\n  template: {maybe_template_id:?}\n  symname: {maybe_sym_name:?}\n  objname: {maybe_obj_name:?}\n  objshortname: {maybe_obj_short_name:?}\n  links: {maybe_links:?}\n  scripts: {maybe_scripts:?}");
        },
    );
}

pub fn vec3_to_point3(v: Vector3<f32>) -> Point3<f32> {
    point3(v.x, v.y, v.z)
}

pub fn point3_to_vec3(p: Point3<f32>) -> Vector3<f32> {
    vec3(p.x, p.y, p.z)
}

pub fn point2_to_vec2(p: Point2<f32>) -> Vector2<f32> {
    vec2(p.x, p.y)
}

fn format_number(num: u32) -> String {
    if num < 10 {
        format!("0{num}")
    } else {
        num.to_string()
    }
}

pub fn get_email_sound_file(deck: u32, email_num: u32) -> String {
    format!("EM{}{}", format_number(deck), format_number(email_num))
}

pub fn get_position_from_transform(
    world: &World,
    entity_id: EntityId,
    offset: Vector3<f32>,
) -> Point3<f32> {
    let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
    let v_prop_position = world.borrow::<View<PropPosition>>().unwrap();

    if let Ok(transform) = v_transform.get(entity_id) {
        let point = vec3_to_point3(offset);
        let xform = transform.0;

        xform.transform_point(point)
    } else if let Ok(position) = v_prop_position.get(entity_id) {
        warn!("no transform for entity: {:?}", entity_id);
        let ret = position.position + (position.rotation * offset);
        point3(ret.x, ret.y, ret.z)
    } else {
        warn!("no transform or position for entity: {:?}", entity_id);
        point3(0.0, 0.0, 0.0)
    }
}

pub fn has_refs(world: &World, entity_id: EntityId) -> bool {
    let v_has_refs = world.borrow::<View<PropHasRefs>>().unwrap();

    let maybe_prop_has_refs = v_has_refs.get(entity_id);

    if maybe_prop_has_refs.is_err() {
        return true;
    }

    let prop_has_refs = maybe_prop_has_refs.unwrap();
    prop_has_refs.0
}

pub fn get_rotation_from_transform(world: &World, entity_id: EntityId) -> Quaternion<f32> {
    if let Ok(position) = world.borrow::<View<PropPosition>>().unwrap().get(entity_id) {
        position.rotation
    } else {
        warn!("no transform or position for entity: {:?}", entity_id);
        Quaternion {
            v: vec3(0.0, 0.0, 0.0),
            s: 1.0,
        }
    }

    // let v_prop_position = world.borrow::<View<PropPosition>>().unwrap();
    // let position = v_prop_position.get(entity_id).unwrap();
    // position.rotation
    //let v_transform = world.borrow::<View<RuntimePropTransform>>().unwrap();
    // let xform = v_transform.get(entity_id).unwrap().0;
    // let forward = xform.transform_vector(vec3(0.0, 0.0, 1.0)).normalize();
    // let up = xform.transform_vector(vec3(0.0, 1.0, 0.0)).normalize();
    // Matrix3::look_to_lh(forward, up).into()
}

pub fn get_position_from_matrix(xform: &Matrix4<f32>) -> Point3<f32> {
    let p = point3(0.0, 0.0, 0.0);

    xform.transform_point(p)
}

pub fn get_rotation_from_matrix(matrix: &Matrix4<f32>) -> Quaternion<f32> {
    let rot_matrix = cgmath::Matrix3::new(
        matrix.x.x, matrix.x.y, matrix.x.z, matrix.y.x, matrix.y.y, matrix.y.z, matrix.z.x,
        matrix.z.y, matrix.z.z,
    );

    rot_matrix.into()
}

pub fn get_rotation_from_forward_vector(forward: Vector3<f32>) -> Quaternion<f32> {
    let mut default_up = Vector3::new(0.0, 1.0, 0.0);

    if forward.dot(default_up).abs() > 0.99 {
        default_up = Vector3::new(1.0, 0.0, 0.0);
    }

    // Calculate the right direction
    let right = default_up.cross(forward).normalize();

    // Recalculate up to ensure it's orthogonal to forward and right
    let up = forward.cross(right);

    // Construct rotation matrix from forward, up, and right
    let rot_matrix = Matrix3::from_cols(right, up, forward);

    // Convert the rotation matrix to a quaternion
    Quaternion::from(rot_matrix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{Deg, Matrix4, Point3, Quaternion, Rotation3, Vector3};

    #[test]
    fn test_get_position_from_matrix() {
        // Given a translation matrix with a translation of (1.0, 2.0, 3.0)
        let xform = Matrix4::from_translation(Vector3::new(1.0, 2.0, 3.0));

        // When we extract the position
        let position = get_position_from_matrix(&xform);

        // Then the extracted position should be (1.0, 2.0, 3.0)
        assert_eq!(position, Point3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn test_get_rotation_from_matrix() {
        // Given a rotation matrix that represents a 90-degree rotation around the Y axis
        let rotation: Matrix4<f32> = Matrix4::from_angle_y(Deg(90.0));
        let xform = rotation;

        // When we extract the rotation
        let extracted_rotation = get_rotation_from_matrix(&xform);

        // Create a quaternion from our known rotation for comparison
        let known_rotation = Quaternion::from_angle_y(Deg(90.0));

        // Then the extracted rotation should be approximately the same as our input rotation
        // We use approximate equality here because of potential floating point precision issues.
        let close_enough = extracted_rotation.dot(known_rotation) > 0.999;
        assert!(
            close_enough,
            "Rotations are not close enough: {:?} vs {:?}",
            extracted_rotation, known_rotation
        );
    }
}

pub fn partition_map<K, V, F>(map: HashMap<K, V>, predicate: F) -> (HashMap<K, V>, HashMap<K, V>)
where
    K: std::hash::Hash + Eq,
    V: Clone,
    F: Fn(&K) -> bool,
{
    let mut true_map = HashMap::new();
    let mut false_map = HashMap::new();

    for (key, value) in map {
        if predicate(&key) {
            true_map.insert(key, value);
        } else {
            false_map.insert(key, value);
        }
    }

    (true_map, false_map)
}

///
/// resolve_proxy_entity
///
/// Given an EntityId, if the entity is a proxy entity (like a hitbox or gui), this will return the
/// parent proxy.
pub fn resolve_proxy_entity(world: &World, maybe_entity_id: EntityId) -> EntityId {
    let v_proxy_entity = world.borrow::<View<RuntimePropProxyEntity>>().unwrap();
    let maybe_prop_proxy_entity = v_proxy_entity.get(maybe_entity_id);

    if let Ok(proxy_entity) = maybe_prop_proxy_entity {
        // Proxy entity, return parent
        proxy_entity.0
    } else {
        // Otherwise, return the current entity id
        maybe_entity_id
    }
}
