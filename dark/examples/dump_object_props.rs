//! Dump each mission object's effective door properties as this crate
//! resolves them (gamesys merged, ancestors applied), one JSON object per
//! line, for differential checks of other implementations. Positions are in
//! feet (the loader's scale undone) but keep the loader's axes: Dark's
//! (x, y, z) is stored as (-x, z, y).
//!
//!   cargo run -p dark --example dump_object_props -- shock2.gam medsci1.mis > props.jsonl

use std::{collections::HashMap, fs, io::Cursor};

use dark::{
    SCALE_FACTOR,
    properties::{
        Link, PropFrobInfo, PropLocked, PropRotatingDoor, PropScripts, PropTranslatingDoor,
    },
    ss2_chunk_file_reader,
    ss2_entity_info::{self, get_ancestors, get_hierarchy},
};
use serde_json::json;
use shipyard::{Component, Get, View, World};

fn feet(v: cgmath::Vector3<f32>) -> [f32; 3] {
    [v.x * SCALE_FACTOR, v.y * SCALE_FACTOR, v.z * SCALE_FACTOR]
}

fn component<C: Component + Clone + Send + Sync>(
    world: &World,
    e: shipyard::EntityId,
) -> Option<C> {
    world.borrow::<View<C>>().ok()?.get(e).ok().cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (properties, links, links_with_data) = dark::properties::get();
    let mut gam = Cursor::new(fs::read(&args[1]).unwrap());
    let gamesys = dark::gamesys::read(&mut gam, &links, &links_with_data, &properties);
    let mut mis = Cursor::new(fs::read(&args[2]).unwrap());
    let toc = ss2_chunk_file_reader::read_table_of_contents(&mut mis);
    let mission = ss2_entity_info::new(&toc, &links, &links_with_data, &properties, &mut mis);
    let merged = ss2_entity_info::merge_with_gamesys(&mission, &gamesys);
    let hierarchy = get_hierarchy(&merged);

    let mut ids: Vec<i32> = mission.entity_to_properties.keys().copied().collect();
    ids.sort();
    for id in ids {
        let mut world = World::new();
        let e = world.add_entity(());
        let mut ancestors = get_ancestors(hierarchy, &id);
        ancestors.push(id);
        for a in &ancestors {
            for p in merged.entity_to_properties.get(a).into_iter().flatten() {
                p.initialize(&mut world, e);
            }
        }
        let trans = component::<PropTranslatingDoor>(&world, e).map(|d| {
            json!({ "closed": d.closed, "open": d.open, "speed": d.speed * SCALE_FACTOR, "axis": d.axis,
                    "state": d.state, "baseClosed": feet(d.base_closed_location),
                    "baseOpen": feet(d.base_open_location), "base": feet(d.base_location) })
        });
        let rot = component::<PropRotatingDoor>(&world, e).map(|d| {
            json!({ "closed": d.closed, "open": d.open, "speed": d.speed, "axis": d.axis, "state": d.state,
                    "clockwise": d.clockwise, "baseClosed": feet(d.base_closed_location),
                    "baseOpen": feet(d.base_open_location), "base": feet(d.base_location) })
        });
        let locked = component::<PropLocked>(&world, e).map(|l| l.0);
        let frob = component::<PropFrobInfo>(&world, e).map(|f| {
            json!({ "world": f.world_action.bits(), "inventory": f.inventory_action.bits(), "tool": f.tool_action.bits() })
        });
        let scripts = component::<PropScripts>(&world, e).map(|s| s.scripts);
        let switch: Vec<i32> = merged
            .template_to_links
            .get(&id)
            .map(|l| {
                l.to_links
                    .iter()
                    .filter(|t| matches!(t.link, Link::SwitchLink))
                    .map(|t| t.to_template_id)
                    .collect()
            })
            .unwrap_or_default();
        let row: HashMap<&str, serde_json::Value> = HashMap::from([
            ("id", json!(id)),
            // Application order: later entries override earlier ones.
            ("ancestors", json!(ancestors)),
            ("transDoor", json!(trans)),
            ("rotDoor", json!(rot)),
            ("locked", json!(locked)),
            ("frobInfo", json!(frob)),
            ("scripts", json!(scripts)),
            ("switchLink", json!(switch)),
        ]);
        println!("{}", serde_json::to_string(&row).unwrap());
    }
}
