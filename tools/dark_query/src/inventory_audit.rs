//! Inventory model coverage, using the same inherited properties as gameplay.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use dark::{properties::*, ss2_entity_info::SystemShock2EntityInfo};
use serde_json::json;
use shipyard::{Get, View, World};

#[derive(Default)]
struct ItemModel {
    names: BTreeSet<String>,
    sources: BTreeSet<String>,
    objects: BTreeSet<String>,
    held_models: BTreeSet<String>,
}

pub fn run(all_missions: bool) -> Result<()> {
    let mut models = BTreeMap::new();
    collect(
        &crate::data_loader::load_entity_data(None)?,
        "shock2.gam",
        &mut models,
    );
    let missions = if all_missions {
        shock2vr::data_files::mission_names(&shock2vr::paths::data_root())
    } else {
        Vec::new()
    };
    for mission in &missions {
        collect(
            &crate::data_loader::load_entity_data(Some(mission))?,
            mission,
            &mut models,
        );
    }
    let models: BTreeMap<_, _> = models
        .into_iter()
        .map(|(model, entry)| {
            (
                model,
                json!({
                    "names": entry.names, "sources": entry.sources, "objects": entry.objects,
                    "held_models": entry.held_models,
                }),
            )
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
        "criteria": "Inherited world frob MOVE/USE_AMMO, an authored Contains link, or positive inventory dimensions with an inventory action. Contains items default to 1x1 without dimensions. Includes abstract templates and download pickups; dimensions alone do not make scenery carryable. Empty model keys identify items without model metadata.",
            "missions": missions,
            "models": models,
        }))?
    );
    Ok(())
}

fn collect(info: &SystemShock2EntityInfo, source: &str, models: &mut BTreeMap<String, ItemModel>) {
    let contained: BTreeSet<_> = info
        .template_to_links
        .values()
        .flat_map(|links| &links.to_links)
        .filter(|link| matches!(link.link, Link::Contains(_)))
        .map(|link| link.to_template_id)
        .collect();
    let mut world = World::new();
    let ids = info.initialize_world_with_entities(&mut world, Default::default(), |_| true);
    let (dims, frobs, names, meshes, guns, limbs) = world
        .borrow::<(
            View<PropInventoryDimensions>,
            View<PropFrobInfo>,
            View<PropSymName>,
            View<PropModelName>,
            View<PropPlayerGun>,
            View<PropLimbModel>,
        )>()
        .unwrap();
    for (id, entity) in ids {
        let inventory = contained.contains(&id)
            || (dims.get(entity).is_ok_and(|d| d.width > 0 && d.height > 0)
                && frobs
                    .get(entity)
                    .is_ok_and(|f| !f.inventory_action.is_empty()));
        let movable = frobs.get(entity).is_ok_and(|f| {
            f.world_action
                .intersects(FrobFlag::MOVE | FrobFlag::USE_AMMO)
        });
        if !inventory && !movable {
            continue;
        }
        let model = meshes
            .get(entity)
            .map(|m| m.0.to_ascii_lowercase().trim_end_matches(".bin").to_owned())
            .unwrap_or_default();
        let entry = models.entry(model).or_default();
        if let Ok(name) = names.get(entity) {
            entry.names.insert(name.0.clone());
        }
        entry.sources.insert(source.to_owned());
        // Template IDs are stable across missions; concrete object IDs are local.
        entry.objects.insert(if id < 0 {
            id.to_string()
        } else {
            format!("{source}:{id}")
        });
        let held = guns
            .get(entity)
            .map(|g| g.hand_model.as_str())
            .ok()
            .or_else(|| limbs.get(entity).ok().map(|l| l.0.as_str()));
        if let Some(held) = held {
            entry.held_models.insert(held.to_ascii_lowercase());
        }
    }
}
