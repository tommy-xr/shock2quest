use cgmath::{Matrix4, Vector2, point2, vec2, vec3};
use collision::{Aabb2, Aabb3};
use dark::{
    importers::{FONT_IMPORTER, TEXTURE_IMPORTER},
    properties::{PropHitPoints, PropObjName, PropTemplateId},
};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject, texture::TextureOptions};
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

pub fn draw_item_name(
    asset_cache: &mut AssetCache,
    physics: &PhysicsWorld,
    entity_id: EntityId,
    world: &World,
    //aabb: collision::Aabb3<f32>,
    view: Matrix4<f32>,
    projection: Matrix4<f32>,
    screen_size: Vector2<f32>,
    debug_show_ids: bool,
) -> Vec<SceneObject> {
    let maybe_bbox = physics.get_aabb2(entity_id);

    if maybe_bbox.is_none() {
        return vec![];
    }

    let v_prop_obj_short_name = world.borrow::<View<PropObjName>>().unwrap();
    let maybe_prop_obj_short_name = v_prop_obj_short_name.get(entity_id);
    let v_prop_template_id = world.borrow::<View<PropTemplateId>>().unwrap();
    let prop_template_id = v_prop_template_id.get(entity_id).unwrap();

    if maybe_prop_obj_short_name.is_err() {
        return vec![];
    }

    let prop_obj_short_name = maybe_prop_obj_short_name.unwrap();

    if prop_obj_short_name.0.is_empty() {
        return vec![];
    }

    let aabb = maybe_bbox.unwrap();
    let font = asset_cache.get(&FONT_IMPORTER, "mainfont.fon");
    let extents = project_aabb3(&aabb, view, projection, screen_size);

    let v_prop_hitpoints = world.borrow::<View<PropHitPoints>>().unwrap();
    let maybe_hitpoints = v_prop_hitpoints
        .get(entity_id)
        .map(|hp| hp.hit_points.to_string())
        .unwrap_or("?".to_string());

    let text_content = if debug_show_ids {
        format!(
            "{} | {} (Tem {}| Ent {})",
            prop_obj_short_name.0,
            &maybe_hitpoints,
            prop_template_id.template_id,
            entity_id.inner(),
        )
    } else {
        format!("{} | {}", prop_obj_short_name.0, &maybe_hitpoints,)
    };

    let text_obj_0_0 = SceneObject::screen_space_text(
        &text_content,
        font.clone(),
        10.0,
        0.5,
        extents.min.x,
        extents.min.y - 10.0,
    );

    vec![text_obj_0_0]
}

pub fn draw_item_outline(
    asset_cache: &mut AssetCache,
    physics: &PhysicsWorld,
    entity_id: EntityId,
    //aabb: collision::Aabb3<f32>,
    view: Matrix4<f32>,
    projection: Matrix4<f32>,
    screen_size: Vector2<f32>,
) -> Vec<SceneObject> {
    let maybe_bbox = physics.get_aabb2(entity_id);

    if maybe_bbox.is_none() {
        return vec![];
    }

    let options = TextureOptions { wrap: false };

    let aabb = maybe_bbox.unwrap();
    let top_left_brack = asset_cache.get_ext(&TEXTURE_IMPORTER, "BRACK0.PCX", &options);
    let top_right_brack = asset_cache.get_ext(&TEXTURE_IMPORTER, "BRACK1.PCX", &options);
    let bottom_right_brack = asset_cache.get_ext(&TEXTURE_IMPORTER, "BRACK2.PCX", &options);
    let bottom_left_brack = asset_cache.get_ext(&TEXTURE_IMPORTER, "BRACK3.PCX", &options);

    let size = vec2(8.0, 8.0);
    let extents = project_aabb3(&aabb, view, projection, screen_size);
    let top_left_brack_obj =
        SceneObject::screen_space_quad(top_left_brack, vec2(extents.min.x, extents.min.y), size);
    let top_right_brack_obj =
        SceneObject::screen_space_quad(top_right_brack, vec2(extents.max.x, extents.min.y), size);
    let bottom_left_brack_obj =
        SceneObject::screen_space_quad(bottom_left_brack, vec2(extents.min.x, extents.max.y), size);
    let bottom_right_brack_obj = SceneObject::screen_space_quad(
        bottom_right_brack,
        vec2(extents.max.x, extents.max.y),
        size,
    );
    vec![
        top_left_brack_obj,
        bottom_left_brack_obj,
        bottom_right_brack_obj,
        top_right_brack_obj,
    ]
}

pub fn project_aabb3(
    aabb: &Aabb3<f32>,
    view: Matrix4<f32>,
    projection: Matrix4<f32>,
    screen_size: Vector2<f32>,
) -> Aabb2<f32> {
    let all_corners = vec![
        vec3(aabb.min.x, aabb.min.y, aabb.min.z),
        vec3(aabb.min.x, aabb.min.y, aabb.max.z),
        vec3(aabb.min.x, aabb.max.y, aabb.min.z),
        vec3(aabb.min.x, aabb.max.y, aabb.max.z),
        vec3(aabb.max.x, aabb.min.y, aabb.min.z),
        vec3(aabb.max.x, aabb.min.y, aabb.max.z),
        vec3(aabb.max.x, aabb.max.y, aabb.min.z),
        vec3(aabb.max.x, aabb.max.y, aabb.max.z),
    ];

    let mapped_corners: Vec<Vector2<f32>> = all_corners
        .into_iter()
        .map(|v| engine::util::project(view, projection, v, screen_size.x, screen_size.y))
        .collect();

    let mut min_x = mapped_corners[0].x;
    let mut min_y = mapped_corners[0].y;
    let mut max_x = mapped_corners[0].x;
    let mut max_y = mapped_corners[0].y;

    for v in mapped_corners {
        if v.x < min_x {
            min_x = v.x
        }

        if v.y < min_y {
            min_y = v.y;
        }

        if v.x > max_x {
            max_x = v.x;
        }

        if v.y > max_y {
            max_y = v.y;
        }
    }

    Aabb2 {
        min: point2(min_x, min_y),
        max: point2(max_x, max_y),
    }
}
