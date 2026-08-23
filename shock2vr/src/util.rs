use std::{collections::HashMap, rc::Rc};

use cgmath::{InnerSpace, Matrix3, Matrix4, Point3, Quaternion, Transform, Vector3, point3, vec3};

use dark::properties::{PropHasRefs, PropPosition};
use engine::{
    game_log,
    scene::{SceneObject, SceneObjectDebugTag},
};
use shipyard::{EntityId, Get, View, World};
use tracing::warn;

use crate::runtime_props::{RuntimePropProxyEntity, RuntimePropTransform};

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

pub fn debug_entity(world: &World, id: EntityId) -> String {
    let (name, template_id) = entity_ident(world, id);
    let template_id = template_id
        .map(|t| t.to_string())
        .unwrap_or_else(|| "None".to_string());
    format!("{:?} | {} | {}", id, template_id, name)
}

/// Stable-ish identity of an entity for diagnostics: its symbolic name and
/// template id. (Runtime entity ids are reassigned every launch, so the
/// template id is the durable handle.)
pub fn entity_ident(world: &World, id: EntityId) -> (String, Option<i32>) {
    world.run(
        |v_template_id: View<dark::properties::PropTemplateId>,
         v_symname: View<dark::properties::PropSymName>,
         v_objname: View<dark::properties::PropObjName>,
         v_objshortname: View<dark::properties::PropObjShortName>| {
            let template_id = v_template_id.get(id).map(|t| t.template_id).ok();
            let name = v_symname
                .get(id)
                .map(|s| s.0.clone())
                .or_else(|_| v_objname.get(id).map(|o| o.0.clone()))
                .or_else(|_| v_objshortname.get(id).map(|o| o.0.clone()))
                .unwrap_or_else(|_| "Unknown".to_string());
            (name, template_id)
        },
    )
}

pub fn vec3_to_point3(v: Vector3<f32>) -> Point3<f32> {
    point3(v.x, v.y, v.z)
}

pub fn point3_to_vec3(p: Point3<f32>) -> Vector3<f32> {
    vec3(p.x, p.y, p.z)
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

/// The entity's current world position, preferring the runtime transform that
/// follows animation and physics over the authored position property.
pub fn get_entity_position(world: &World, entity_id: EntityId) -> Option<Vector3<f32>> {
    if let Ok(transforms) = world.borrow::<View<RuntimePropTransform>>() {
        if let Ok(xform) = transforms.get(entity_id) {
            return Some(point3_to_vec3(
                xform.0.transform_point(point3(0.0, 0.0, 0.0)),
            ));
        }
    }
    world
        .borrow::<View<PropPosition>>()
        .ok()
        .and_then(|positions| positions.get(entity_id).ok().map(|prop| prop.position))
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

/// Where a tracked head is and what it is looking at, or `None` when the head
/// is not tracked.
///
/// An untracked head arrives as the **ZERO quaternion**, and cgmath's
/// `rotate_vector` silently returns the input *unrotated* for it - so a caller
/// that skips this check gets a plausible-looking forward pointing at world
/// -Z, plus a meaningless position, and hangs its layer somewhere arbitrary.
/// That is VR rule 7 in the `vr-ui-design` skill; it has cost real bugs
/// (#994, #997), so it is encoded once here rather than at each view-locked
/// layer. Callers supply their own fallback for the `None` case, because what
/// stands in for a missing head differs: the pause dim uses its world-locked
/// panel, the hit tint uses the pawn's default eye.
pub fn tracked_gaze(
    head_position: Vector3<f32>,
    head_rotation: Quaternion<f32>,
) -> Option<(Vector3<f32>, Vector3<f32>)> {
    use cgmath::Rotation;
    let rotation = tracked_rotation(head_rotation)?;
    Some((head_position, rotation.rotate_vector(vec3(0.0, 0.0, -1.0))))
}

/// The unit form of a tracked rotation, or `None` when it carries no rotation
/// information (a zero quaternion - an untracked head, or a default-constructed
/// pose). The same rule [`tracked_gaze`] applies, exposed for callers that need
/// the rotation itself rather than a gaze ray: normalizing a zero quaternion
/// yields NaN, and a NaN rotation poisons whatever matrix it reaches.
pub fn tracked_rotation(rotation: Quaternion<f32>) -> Option<Quaternion<f32>> {
    (rotation.magnitude2() >= 1e-6).then(|| rotation.normalize())
}

/// Smoothstep over `t`, clamped to `[0, 1]`: no velocity discontinuity at
/// either end, which is what makes a timed ease read as a move rather than a
/// jump. Shared by the UI panel re-placement ease and the death camera's fall.
pub fn smoothstep(t: f32) -> f32 {
    if !t.is_finite() || t <= 0.0 {
        return 0.0;
    }
    let t = t.min(1.0);
    t * t * (3.0 - 2.0 * t)
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
    use shipyard::ViewMut;

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

    #[test]
    fn entity_position_follows_the_live_runtime_transform() {
        let mut world = World::new();
        let entity = world.add_entity((
            PropPosition {
                position: vec3(1.0, 2.0, 3.0),
                cell: 0,
                rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
            },
            RuntimePropTransform(Matrix4::from_translation(vec3(4.0, 5.0, 6.0))),
        ));

        assert_eq!(
            get_entity_position(&world, entity),
            Some(vec3(4.0, 5.0, 6.0))
        );

        let mut transforms = world.borrow::<ViewMut<RuntimePropTransform>>().unwrap();
        (&mut transforms).get(entity).unwrap().0 = Matrix4::from_translation(vec3(7.0, 8.0, 9.0));
        drop(transforms);

        assert_eq!(
            get_entity_position(&world, entity),
            Some(vec3(7.0, 8.0, 9.0))
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

/// Render-path labels attached to scene objects as [`SceneObjectDebugTag::source`].
///
/// `/v1/scene` reports them, so automation can assert what the renderer was
/// actually handed. One of them is also load-bearing: `Game` drops
/// [`PLAYER_HANDS_SOURCE`] objects while the pause menu is up (issue #1018).
pub mod render_source {
    /// The player's own hand visuals: the VR gloves, their raycast markers and
    /// the forearm HUD panels. Emitted by both the mission interaction
    /// controller and the `debug_hud` scene.
    pub const PLAYER_HANDS: &str = "player_hands";
    /// A frontend panel's pointer: a hand per tracked controller, its aim beam
    /// and the hit dot. This is the *only* pair of hands a frontend screen or
    /// the pause menu shows.
    pub const FRONTEND_POINTER: &str = "frontend_pointer";
    /// The pause menu's comfort dim, behind its panel.
    pub const PAUSE_DIM: &str = "pause_dim";
    /// The VR cyber interface's comfort dim, behind the use-mode panel.
    pub const USE_MODE_DIM: &str = "use_mode_dim";
    /// The VR cyber interface's aim beams and hit dot. Unlike
    /// [`FRONTEND_POINTER`] this draws no hands: the interface is a mode of
    /// play, so the player's real [`PLAYER_HANDS`] are still on screen and a
    /// second static glove would stack on top of them.
    pub const USE_MODE_POINTER: &str = "use_mode_pointer";
    /// The red rim tint shown for a moment after the player takes damage.
    pub const HIT_FEEDBACK: &str = "hit_feedback";
    /// The cyber interface's own rim vignette, eased in/out with its
    /// entry/exit ramp - layered alongside (not merged with) [`HIT_FEEDBACK`],
    /// so a hit still reads while the interface is open.
    pub const USE_MODE_VIGNETTE: &str = "use_mode_vignette";
}

/// A tag that records only which render path produced an object.
pub fn render_source_tag(source: &str) -> Rc<SceneObjectDebugTag> {
    Rc::new(SceneObjectDebugTag {
        source: Some(source.to_owned()),
        ..Default::default()
    })
}

/// Label every object in `objects` with `source`, sharing one tag allocation.
pub fn tag_render_source(objects: &mut [SceneObject], source: &str) {
    let tag = render_source_tag(source);
    for object in objects {
        object.set_debug_tag(Some(tag.clone()));
    }
}
