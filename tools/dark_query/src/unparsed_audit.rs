//! Chunks with no registered parser, script names the port does not
//! implement and receptron effects it does not handle, tallied across the
//! gamesys and every installed mission - the worklist for parity work.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use dark::properties::{Link, PropPsiPower, PropScripts, ReceptronEffect};
use dark::ss2_entity_info::SystemShock2EntityInfo;
use serde_json::json;
use shipyard::{Get, IntoIter, IntoWithId, View, World};
use shock2vr::ScriptSupport;

#[derive(Default)]
struct Tally {
    entries: usize,
    /// Distinct record sizes in bytes (properties and link data only).
    sizes: BTreeSet<u32>,
    sources: BTreeSet<String>,
    /// How the registry resolves the name (scripts only).
    support: Option<ScriptSupport>,
}

#[derive(Default)]
struct Audit {
    properties: BTreeMap<String, Tally>,
    links: BTreeMap<String, Tally>,
    link_data: BTreeMap<String, Tally>,
    /// Authored script names (lowercased, as the registry matches them)
    /// whose registry entry is a stub, a no-op or missing.
    scripts: BTreeMap<String, Tally>,
    /// Receptron effect names (`L$Receptron` data) parsed as `Unhandled`.
    reactions: BTreeMap<String, Tally>,
}

impl Audit {
    fn collect(&mut self, info: &SystemShock2EntityInfo, source: &str) {
        // Every mission carries every chunk, mostly empty; only count records.
        for (chunk, entries) in info
            .unparsed_properties
            .iter()
            .filter(|(_, e)| !e.is_empty())
        {
            let tally = self.properties.entry(chunk.clone()).or_default();
            tally.entries += entries.len();
            tally.sizes.extend(entries.iter().map(|e| e.byte_len));
            tally.sources.insert(source.to_string());
        }
        for (chunk, links) in info.unparsed_links.iter().filter(|(_, l)| !l.is_empty()) {
            let tally = self.links.entry(chunk.clone()).or_default();
            tally.entries += links.len();
            tally.sources.insert(source.to_string());
        }
        for (chunk, entries) in info
            .unparsed_link_data
            .iter()
            .filter(|(_, e)| !e.is_empty())
        {
            let tally = self.link_data.entry(chunk.clone()).or_default();
            tally.entries += entries.len();
            tally.sizes.extend(entries.iter().map(|e| e.byte_len));
            tally.sources.insert(source.to_string());
        }

        // Only each record's own properties: inheritance would count a script
        // once per descendant instead of once per authoring.
        let mut world = World::new();
        for props in info.entity_to_properties.values() {
            let entity = world.add_entity(());
            for prop in props {
                prop.initialize(&mut world, entity);
            }
        }
        // Psi power scripts are dispatched by power id, not the registry.
        let psi_powers = world.borrow::<View<PropPsiPower>>().unwrap();
        let scripts = world.borrow::<View<PropScripts>>().unwrap();
        for (entity, prop) in scripts.iter().with_id() {
            if psi_powers.get(entity).is_ok() {
                continue;
            }
            for name in prop.scripts.iter().filter(|n| !n.is_empty()) {
                let name = name.to_ascii_lowercase();
                let support = shock2vr::script_support(&name);
                if support == ScriptSupport::Implemented {
                    continue;
                }
                let tally = self.scripts.entry(name).or_default();
                tally.support = Some(support);
                tally.entries += 1;
                tally.sources.insert(source.to_string());
            }
        }

        let receptrons = info.template_to_links.values().flat_map(|l| &l.to_links);
        for link in receptrons {
            if let Link::Receptron(options) = &link.link
                && let ReceptronEffect::Unhandled(name) = &options.effect
            {
                let tally = self.reactions.entry(name.clone()).or_default();
                tally.entries += 1;
                tally.sources.insert(source.to_string());
            }
        }
    }
}

pub fn run(as_json: bool) -> Result<()> {
    let mut audit = Audit::default();
    audit.collect(&crate::data_loader::load_gamesys_only()?, "shock2.gam");
    let missions = shock2vr::data_files::mission_names(&shock2vr::paths::data_root());
    for mission in &missions {
        audit.collect(&crate::data_loader::load_mission_only(mission)?, mission);
    }

    let sections = [
        ("properties", &audit.properties),
        ("links", &audit.links),
        ("link_data", &audit.link_data),
        ("scripts", &audit.scripts),
        ("reactions", &audit.reactions),
    ];
    if as_json {
        let mut out = serde_json::Map::new();
        for (section, rows) in sections {
            let rows: Vec<_> = rows
                .iter()
                .map(|(name, t)| {
                    json!({
                        "chunk": name,
                        "entries": t.entries,
                        "sizes": t.sizes,
                        "sources": t.sources,
                        "support": t.support.map(|s| format!("{s:?}")),
                    })
                })
                .collect();
            out.insert(section.to_string(), json!(rows));
        }
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    for (section, rows) in sections {
        println!("== unparsed {section}: {}", rows.len());
        let mut rows: Vec<_> = rows.iter().collect();
        rows.sort_by(|a, b| b.1.entries.cmp(&a.1.entries).then(a.0.cmp(b.0)));
        for (name, t) in rows {
            let sizes: Vec<String> = t.sizes.iter().map(u32::to_string).collect();
            let in_gamesys = t.sources.contains("shock2.gam");
            let missions = t.sources.len() - usize::from(in_gamesys);
            let detail = match t.support {
                Some(support) => format!("{support:?}"),
                None if sizes.is_empty() => String::new(),
                None => format!("sizes [{}]", sizes.join(",")),
            };
            println!(
                "{name:<22} {:>6} entries  {detail:<14} {}{missions} missions",
                t.entries,
                if in_gamesys { "gam + " } else { "" },
            );
        }
    }
    Ok(())
}
