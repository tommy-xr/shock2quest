//! Weapon-specific appearance for actual ammo entities. Inventory identity and
//! stack accounting stay in the ordinary reload/containment paths.
use dark::{
    importers::{MAGAZINE_MODEL_IMPORTER, MagazineModel},
    properties::InternalPropMagazineModel,
};
use engine::assets::asset_cache::AssetCache;
use shipyard::{EntityId, Get, View, World};

pub(crate) fn removed(world: &World, weapon: EntityId) -> bool {
    world
        .borrow::<View<dark::properties::InternalPropMagazineRemoved>>()
        .ok()
        .and_then(|v| v.get(weapon).ok().map(|p| p.0))
        .unwrap_or(false)
}

pub(crate) fn set_removed(world: &mut World, weapon: EntityId, removed: bool) {
    if crate::scripts::internal_switch_held_model::get_raw_view_model(world, weapon)
        .is_some_and(|name| matches!(name.to_ascii_lowercase().as_str(), "atek_h" | "ar15_h"))
    {
        world.add_component(
            weapon,
            dark::properties::InternalPropMagazineRemoved(removed),
        );
    }
}

/// Rebuild a held weapon only when its saved magazine state and visible mesh
/// disagree. ChangeModel retains the normal handedness and muzzle setup.
pub(crate) fn refresh_weapons(
    world: &World,
    held: (Option<EntityId>, Option<EntityId>),
) -> Vec<crate::scripts::Effect> {
    let Ok(meshes) = world.borrow::<View<crate::runtime_props::RuntimePropGloveWeapon>>() else {
        return vec![];
    };
    [held.0, held.1]
        .into_iter()
        .flatten()
        .filter_map(|weapon| {
            let mesh = meshes.get(weapon).ok()?;
            if mesh.magazine_removed == removed(world, weapon) {
                return None;
            }
            Some(crate::scripts::Effect::ChangeModel {
                entity_id: weapon,
                model_name: crate::scripts::internal_switch_held_model::get_raw_view_model(
                    world, weapon,
                )?,
            })
        })
        .collect()
}
use std::rc::Rc;

pub(crate) fn appearance(world: &World, entity: EntityId) -> Option<InternalPropMagazineModel> {
    world
        .borrow::<View<InternalPropMagazineModel>>()
        .ok()?
        .get(entity)
        .ok()
        .cloned()
}

pub(crate) fn load(assets: &mut AssetCache, source: &str) -> Option<Rc<Option<MagazineModel>>> {
    if !matches!(source, "atek_h" | "ar15_h") {
        return None;
    }
    assets
        .get_opt(&MAGAZINE_MODEL_IMPORTER, &format!("{source}.bin"))
        .filter(|model| model.as_ref().is_some())
}

pub(crate) fn entity_model(
    assets: &mut AssetCache,
    world: &World,
    entity: EntityId,
) -> Option<dark::model::Model> {
    let appearance = appearance(world, entity)?;
    let source = load(assets, &appearance.source)?;
    Some(source.as_ref().as_ref()?.model.clone())
}

pub(crate) fn resolve_grip(
    triangles: &[[cgmath::Point3<f32>; 3]],
    scale: f32,
    rig: &crate::vr_grip::GripKinematics,
) -> Option<crate::vr_grip::ResolvedGrip> {
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let scaled: Vec<_> = triangles.iter().map(|t| t.map(|p| p * scale)).collect();
    let grip = crate::vr_grip::GripSurface::new(&scaled)?.resolve(
        rig,
        &crate::vr_grip::GripHints {
            keep_upright: true,
            ..Default::default()
        },
    )?;
    Some(grip.with_item_scale(scale))
}
