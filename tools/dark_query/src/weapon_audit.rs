//! Reproducible authored-data inventory; runtime verification is recorded separately.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use dark::{
    properties::*,
    ss2_entity_info::{self, SystemShock2EntityInfo},
};
use serde_json::{Value, json};
use shipyard::{EntityId, Get, View, World};

pub fn run(mission: Option<&str>) -> Result<()> {
    let info = crate::data_loader::load_entity_data(mission)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&inventory(&info, mission))?
    );
    Ok(())
}

fn inventory(info: &SystemShock2EntityInfo, mission: Option<&str>) -> Value {
    let mut world = World::new();
    let ids: BTreeMap<_, _> = info
        .initialize_world_with_entities(&mut world, Default::default(), |id| id < 0)
        .into_iter()
        .collect();
    let gun = world.borrow::<View<PropPlayerGun>>().unwrap();
    let limb = world.borrow::<View<PropLimbModel>>().unwrap();
    let weapons: Vec<i32> = ids
        .iter()
        .filter_map(|(id, entity)| {
            (gun.get(*entity).is_ok() || limb.get(*entity).is_ok()).then_some(*id)
        })
        .collect();
    let mut referenced = BTreeSet::new();
    let mut rows = Vec::new();
    for template in weapons {
        let links = inherited_links(info, template);
        for link in &links {
            if matches!(link.link, Link::Projectile(_) | Link::GunFlash(_)) {
                referenced.insert(link.to_template_id);
            }
        }
        rows.push(describe(info, &world, &ids, template));
    }
    let projectiles: Vec<Value> = referenced
        .into_iter()
        .map(|id| describe(info, &world, &ids, id))
        .collect();
    json!({"source": {"gamesys": "shock2.gam", "mission": mission},
        "note": "Authored data, not a claim of runtime parity. All three setting slots are retained; the UI exposes two.",
        "weapons": rows, "projectiles_and_flashes": projectiles})
}

fn inherited_links(info: &SystemShock2EntityInfo, template: i32) -> Vec<ToTemplateLink> {
    let mut ancestors =
        ss2_entity_info::get_ancestors(ss2_entity_info::get_hierarchy(info), &template);
    ancestors.push(template);
    ancestors
        .into_iter()
        .filter_map(|id| info.template_to_links.get(&id))
        .flat_map(|links| links.to_links.iter().cloned())
        .collect()
}

fn describe(
    info: &SystemShock2EntityInfo,
    world: &World,
    ids: &BTreeMap<i32, EntityId>,
    template: i32,
) -> Value {
    let Some(entity) = ids.get(&template).copied() else {
        return json!({"template": template});
    };
    let mut properties = serde_json::Map::new();
    macro_rules! property {
        ($($ty:ty),* $(,)?) => { $(
            if let Ok(value) = world.borrow::<View<$ty>>().unwrap().get(entity) {
                properties.insert(stringify!($ty).into(), serde_json::to_value(value).unwrap());
            }
        )* };
    }
    property!(
        PropSymName,
        PropModelName,
        PropPlayerGun,
        PropLimbModel,
        PropBaseGunDesc,
        PropGunState,
        PropBaseWeaponDesc,
        PropGunKick,
        PropClassTag,
        PropScripts,
        PropPhysInitialVelocity,
        PropPhysAttr,
        PropCollisionType,
        PropRenderType,
        PropGunSettingHeader1,
        PropGunSettingHeader2
    );
    let names = world.borrow::<View<PropSymName>>().unwrap();
    let links: Vec<Value> = inherited_links(info, template)
        .into_iter()
        .map(|link| {
            let name = ids
                .get(&link.to_template_id)
                .and_then(|id| names.get(*id).ok())
                .map(|n| n.0.clone());
            json!({"target": link.to_template_id, "name": name, "link": link.link})
        })
        .collect();
    let mut ancestors =
        ss2_entity_info::get_ancestors(ss2_entity_info::get_hierarchy(info), &template);
    ancestors.push(template);
    let mut unparsed = BTreeMap::new();
    for (name, entries) in &info.unparsed_properties {
        let matches: Vec<Value> = entries
            .iter()
            .filter(|p| ancestors.contains(&p.entity_id))
            .map(|p| json!({"owner": p.entity_id, "bytes": p.byte_len}))
            .collect();
        if !matches.is_empty() {
            unparsed.insert(name, matches);
        }
    }
    json!({"template": template, "properties": properties, "links": links, "unparsed_properties": unparsed})
}
