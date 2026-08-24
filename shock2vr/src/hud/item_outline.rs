use cgmath::{Matrix4, Vector2, point2, vec2, vec3};
use collision::{Aabb2, Aabb3};
use dark::{
    importers::{FONT_IMPORTER, TEXTURE_IMPORTER},
    properties::{PropHUDSelect, PropHitPoints, PropObjName, PropStackCount, PropTemplateId},
};
use engine::{assets::asset_cache::AssetCache, scene::SceneObject, texture::TextureOptions};
use shipyard::{EntityId, Get, View, World};

use crate::physics::PhysicsWorld;

/// Whether the highlight overlay (corner brackets + rollover name) may be
/// drawn for `entity_id`.
///
/// The original gates the overlay on an opt-in "HUD Selectable?" boolean
/// (`P$HUDSelect`) rather than on frobbability: the shipped data marks the
/// families the player is meant to be able to pick out of the scene (weapons,
/// creatures, loot) `true`, and marks fixed set dressing that is nevertheless
/// frobbable - the `Tech` family of consoles, force-field emitters and
/// speakers - explicitly `false`. An object with no `P$HUDSelect` at all
/// (inheritance-resolved) is *not* highlighted.
///
/// This deliberately says nothing about whether the object can be *frobbed*:
/// frob eligibility stays `P$FrobInfo`-based, so a console with no
/// `P$HUDSelect` still uses/hacks normally, it just draws no brackets.
pub(crate) fn is_hud_selectable(world: &World, entity_id: EntityId) -> bool {
    world
        .borrow::<View<PropHUDSelect>>()
        .map(|v| v.get(entity_id).map(|p| p.0).unwrap_or(false))
        .unwrap_or(false)
}

fn format_stack_aware_item_name(item_name: &str, stack_count: Option<i32>) -> String {
    match stack_count {
        Some(count) => item_name.replace("%d", &count.to_string()),
        None => item_name.to_owned(),
    }
}

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

    if maybe_prop_obj_short_name.is_err() {
        return vec![];
    }

    let prop_obj_short_name = maybe_prop_obj_short_name.unwrap();

    if prop_obj_short_name.0.is_empty() {
        return vec![];
    }

    let stack_count = world
        .borrow::<View<PropStackCount>>()
        .unwrap()
        .get(entity_id)
        .map(|stack| stack.0)
        .ok();
    let item_name = format_stack_aware_item_name(&prop_obj_short_name.0, stack_count);

    let aabb = maybe_bbox.unwrap();
    let font = asset_cache.get(&FONT_IMPORTER, "mainfont.fon");
    let extents = project_aabb3(&aabb, view, projection, screen_size);

    let v_prop_hitpoints = world.borrow::<View<PropHitPoints>>().unwrap();
    let maybe_hitpoints = v_prop_hitpoints
        .get(entity_id)
        .map(|hp| hp.hit_points.to_string())
        .unwrap_or("?".to_string());

    let text_content = if debug_show_ids {
        let template_id = world
            .borrow::<View<PropTemplateId>>()
            .unwrap()
            .get(entity_id)
            .map(|prop| prop.template_id.to_string())
            .unwrap_or_else(|_| "runtime".to_owned());
        format!(
            "{} | {} (Tem {}| Ent {})",
            item_name,
            &maybe_hitpoints,
            template_id,
            entity_id.inner(),
        )
    } else {
        format!("{} | {}", item_name, &maybe_hitpoints,)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flat_player_controller::is_frobbable;
    use dark::properties::{FrobFlag, PropFrobInfo};

    fn frob_info() -> PropFrobInfo {
        PropFrobInfo {
            world_action: FrobFlag::SCRIPT,
            inventory_action: FrobFlag::empty(),
            tool_action: FrobFlag::empty(),
        }
    }

    /// The false positive this gate removes, and the invariant that makes it
    /// safe: an object can be frobbable and still not highlightable. A `Tech`
    /// console keeps working when you use it, it just stops drawing brackets.
    #[test]
    fn frobbable_without_hud_select_is_not_highlighted() {
        let mut world = World::new();
        let id = world.add_entity(frob_info());

        assert!(is_frobbable(&world, id), "frob eligibility is unchanged");
        assert!(!is_hud_selectable(&world, id));
    }

    #[test]
    fn hud_select_true_is_highlighted() {
        let mut world = World::new();
        let id = world.add_entity(frob_info());
        world.add_component(id, PropHUDSelect(true));
        assert!(is_hud_selectable(&world, id));
    }

    /// The shipped `Tech` family sets `P$HUDSelect` explicitly false; an
    /// explicit false is as unhighlightable as an absent property, and is
    /// likewise still frobbable.
    #[test]
    fn hud_select_false_is_not_highlighted_but_stays_frobbable() {
        let mut world = World::new();
        let id = world.add_entity(frob_info());
        world.add_component(id, PropHUDSelect(false));

        assert!(is_frobbable(&world, id), "frob eligibility is unchanged");
        assert!(!is_hud_selectable(&world, id));
    }

    /// Plain world geometry - no frob info, no `P$HUDSelect` - is neither.
    #[test]
    fn world_geometry_is_neither_frobbable_nor_highlighted() {
        let mut world = World::new();
        let id = world.add_entity(PropHitPoints { hit_points: 1 });

        assert!(!is_frobbable(&world, id));
        assert!(!is_hud_selectable(&world, id));
    }

    #[test]
    fn stack_count_replaces_object_name_decimal_placeholder() {
        assert_eq!(
            format_stack_aware_item_name(r#"Nanites: "%d nanites.""#, Some(250)),
            r#"Nanites: "250 nanites.""#,
        );
    }

    #[test]
    fn object_name_without_decimal_placeholder_is_unchanged() {
        assert_eq!(
            format_stack_aware_item_name("Maintenance Tool", Some(12)),
            "Maintenance Tool",
        );
    }

    #[test]
    fn decimal_placeholder_is_preserved_without_a_stack_count() {
        assert_eq!(
            format_stack_aware_item_name(r#"Nanites: "%d nanites.""#, None),
            r#"Nanites: "%d nanites.""#,
        );
    }
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

    let options = TextureOptions {
        wrap: false,
        ..Default::default()
    };

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
