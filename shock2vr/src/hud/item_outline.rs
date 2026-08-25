use std::collections::HashMap;

use cgmath::{Matrix4, Vector2, point2, vec2, vec3};
use collision::{Aabb2, Aabb3};
use dark::{
    importers::{FONT_IMPORTER, STRINGS_IMPORTER, TEXTURE_IMPORTER},
    properties::{
        ObjectNameType, PropGunState, PropHUDSelect, PropHitPoints, PropLog, PropObjName,
        PropObjectNameType, PropStackCount, PropTemplateId,
    },
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

const LOG_UNSET: u32 = 33;

fn resolve_localized_property_string(raw: &str, strings: &HashMap<String, String>) -> String {
    let (key, fallback) = match raw.split_once(':') {
        Some((key, remainder)) => {
            let remainder = remainder.trim();
            let fallback = match remainder.strip_prefix('"') {
                Some(quoted) => quoted.split_once('"').map_or("", |(value, _)| value),
                None => remainder,
            };
            (key.trim(), fallback)
        }
        None => (raw.trim(), ""),
    };

    strings
        .get(&key.to_ascii_lowercase())
        .map(String::as_str)
        .unwrap_or(fallback)
        .to_owned()
}

fn format_stack_aware_item_name(item_name: &str, stack_count: Option<i32>) -> String {
    match stack_count {
        Some(count) => item_name.replace("%d", &count.to_string()),
        None => item_name.to_owned(),
    }
}

fn format_typed_item_name(
    item_name: &str,
    name_type: ObjectNameType,
    stack_count: Option<i32>,
    log_title: Option<&str>,
    weapon_condition: Option<&str>,
) -> String {
    match name_type {
        ObjectNameType::StackCount => format_stack_aware_item_name(item_name, stack_count),
        ObjectNameType::LogTitle => log_title
            .map(|title| item_name.replace("%s", title))
            .unwrap_or_else(|| item_name.to_owned()),
        ObjectNameType::Weapon => weapon_condition
            .map(|condition| item_name.replace("%s", condition))
            .unwrap_or_else(|| item_name.to_owned()),
        ObjectNameType::Normal | ObjectNameType::Unknown(_) => item_name.to_owned(),
    }
}

fn format_hover_label(
    item_name: &str,
    hit_points: Option<i32>,
    debug_identity: Option<(&str, u64)>,
) -> String {
    match (hit_points, debug_identity) {
        (Some(hit_points), Some((template_id, entity_id))) => {
            format!("{item_name} | {hit_points} (Tem {template_id}| Ent {entity_id})")
        }
        (None, Some((template_id, entity_id))) => {
            format!("{item_name} (Tem {template_id}| Ent {entity_id})")
        }
        (Some(hit_points), None) => format!("{item_name} | {hit_points}"),
        (None, None) => item_name.to_owned(),
    }
}

fn format_inline_localized_text(text: &str) -> String {
    text.replace("\\n", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn localized_log_title(
    asset_cache: &mut AssetCache,
    world: &World,
    entity_id: EntityId,
) -> Option<String> {
    let (deck, log) = {
        let logs = world.borrow::<View<PropLog>>().ok()?;
        let log = logs.get(entity_id).ok()?;
        (log.deck, log.log)
    };
    if deck == 0 || log == 0 || log == LOG_UNSET {
        return None;
    }

    let strings = asset_cache.get_opt(&STRINGS_IMPORTER, &format!("level{deck:02}.str"))?;
    strings
        .get(&format!("logname{log}"))
        .map(|name| format_inline_localized_text(name))
}

fn weapon_condition_key(condition: f32) -> String {
    // Looking Glass's GunGetConditionString truncates the 0..100 condition,
    // divides it into ten buckets, and fetches GunCondVal1..10.
    let bucket = ((condition as i32) / 10).clamp(0, 9) + 1;
    format!("guncondval{bucket}")
}

fn localized_weapon_condition(
    asset_cache: &mut AssetCache,
    world: &World,
    entity_id: EntityId,
) -> Option<String> {
    let condition = {
        let gun_states = world.borrow::<View<PropGunState>>().ok()?;
        gun_states.get(entity_id).ok()?.condition
    };
    asset_cache
        .get_opt(&STRINGS_IMPORTER, "weapon.str")?
        .get(&weapon_condition_key(condition))
        .cloned()
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

    let v_prop_obj_name = world.borrow::<View<PropObjName>>().unwrap();
    let maybe_prop_obj_name = v_prop_obj_name.get(entity_id);

    if maybe_prop_obj_name.is_err() {
        return vec![];
    }

    let prop_obj_name = maybe_prop_obj_name.unwrap();

    if prop_obj_name.0.is_empty() {
        return vec![];
    }

    let object_name_strings = asset_cache.get(&STRINGS_IMPORTER, "objname.str");
    let localized_name = resolve_localized_property_string(&prop_obj_name.0, &object_name_strings);
    if localized_name.is_empty() {
        return vec![];
    }

    let name_type = world
        .borrow::<View<PropObjectNameType>>()
        .unwrap()
        .get(entity_id)
        .map(|name_type| name_type.0)
        .unwrap_or_default();
    let stack_count = world
        .borrow::<View<PropStackCount>>()
        .unwrap()
        .get(entity_id)
        .map(|stack| stack.0)
        .ok();
    let log_title = (name_type == ObjectNameType::LogTitle)
        .then(|| localized_log_title(asset_cache, world, entity_id))
        .flatten();
    let weapon_condition = (name_type == ObjectNameType::Weapon)
        .then(|| localized_weapon_condition(asset_cache, world, entity_id))
        .flatten();
    let item_name = format_typed_item_name(
        &localized_name,
        name_type,
        stack_count,
        log_title.as_deref(),
        weapon_condition.as_deref(),
    );

    let aabb = maybe_bbox.unwrap();
    let font = asset_cache.get(&FONT_IMPORTER, "mainfont.fon");
    let extents = project_aabb3(&aabb, view, projection, screen_size);

    let v_prop_hitpoints = world.borrow::<View<PropHitPoints>>().unwrap();
    let hit_points = v_prop_hitpoints.get(entity_id).map(|hp| hp.hit_points).ok();
    let template_id = debug_show_ids.then(|| {
        world
            .borrow::<View<PropTemplateId>>()
            .unwrap()
            .get(entity_id)
            .map(|prop| prop.template_id.to_string())
            .unwrap_or_else(|_| "runtime".to_owned())
    });
    let debug_identity = template_id
        .as_deref()
        .map(|template_id| (template_id, entity_id.inner()));
    let text_content = format_hover_label(&item_name, hit_points, debug_identity);

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
    use dark::properties::{FrobFlag, ObjectNameType, PropFrobInfo};

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
    fn object_name_resource_reference_uses_localized_value() {
        let strings = HashMap::from([(
            "elevator_button".to_string(),
            "Localized elevator button".to_string(),
        )]);

        assert_eq!(
            resolve_localized_property_string(
                r#"Elevator_Button: "A two-state button.""#,
                &strings,
            ),
            "Localized elevator button",
        );
    }

    #[test]
    fn object_name_resource_reference_uses_embedded_fallback() {
        assert_eq!(
            resolve_localized_property_string(r#"HumanCorpses: "A corpse.""#, &HashMap::new(),),
            "A corpse.",
        );
    }

    #[test]
    fn object_name_resource_key_without_fallback_uses_localized_value() {
        let strings = HashMap::from([("basketball".to_string(), "A basketball.".to_string())]);

        assert_eq!(
            resolve_localized_property_string("Basketball", &strings),
            "A basketball.",
        );
    }

    #[test]
    fn hover_label_omits_unknown_hit_points() {
        assert_eq!(format_hover_label("A corpse.", None, None), "A corpse.");
    }

    #[test]
    fn hover_label_keeps_meaningful_hit_points_and_debug_identity() {
        assert_eq!(
            format_hover_label("A turret.", Some(12), Some(("-1778", 42))),
            "A turret. | 12 (Tem -1778| Ent 42)",
        );
    }

    #[test]
    fn hover_label_falls_back_to_runtime_identity_without_a_template() {
        assert_eq!(
            format_hover_label("A corpse.", None, Some(("runtime", 7))),
            "A corpse. (Tem runtime| Ent 7)",
        );
    }

    #[test]
    fn localized_stack_name_replaces_decimal_placeholder() {
        let strings =
            HashMap::from([("nanites".to_string(), "%d translated nanites.".to_string())]);
        let localized = resolve_localized_property_string(r#"Nanites: "%d nanites.""#, &strings);

        assert_eq!(
            format_typed_item_name(
                &localized,
                ObjectNameType::StackCount,
                Some(250),
                None,
                None,
            ),
            "250 translated nanites.",
        );
    }

    #[test]
    fn log_and_weapon_name_types_replace_string_placeholder() {
        assert_eq!(
            format_typed_item_name(
                "An audio log: %s.",
                ObjectNameType::LogTitle,
                None,
                Some("SANGER"),
                None,
            ),
            "An audio log: SANGER.",
        );
        assert_eq!(
            format_typed_item_name(
                "A pistol. (%s)",
                ObjectNameType::Weapon,
                None,
                None,
                Some("Perfect: 10"),
            ),
            "A pistol. (Perfect: 10)",
        );
    }

    #[test]
    fn authored_log_line_breaks_become_single_line_hover_text() {
        assert_eq!(
            format_inline_localized_text("SANGER 10.JUL.14\\nre: Locking Eng. Control\\n"),
            "SANGER 10.JUL.14 re: Locking Eng. Control",
        );
    }

    #[test]
    fn weapon_condition_uses_original_ten_point_buckets() {
        assert_eq!(weapon_condition_key(-1.0), "guncondval1");
        assert_eq!(weapon_condition_key(0.0), "guncondval1");
        assert_eq!(weapon_condition_key(9.9), "guncondval1");
        assert_eq!(weapon_condition_key(10.0), "guncondval2");
        assert_eq!(weapon_condition_key(50.0), "guncondval6");
        assert_eq!(weapon_condition_key(100.0), "guncondval10");
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
