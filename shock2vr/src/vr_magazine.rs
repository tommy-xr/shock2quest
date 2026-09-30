//! Weapon-specific appearance for actual ammo entities. Inventory identity and
//! stack accounting stay in the ordinary reload/containment paths.
use dark::{
    importers::{MAGAZINE_MODEL_IMPORTER, MagazineModel},
    properties::InternalPropMagazineModel,
};
use engine::assets::asset_cache::AssetCache;
use shipyard::{EntityId, Get, View, World};
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
