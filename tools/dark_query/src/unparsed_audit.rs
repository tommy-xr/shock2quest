//! Chunks with no registered parser, tallied across the gamesys and every
//! installed mission - the worklist for new property/link support.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use dark::ss2_entity_info::SystemShock2EntityInfo;
use serde_json::json;

#[derive(Default)]
struct Tally {
    entries: usize,
    /// Distinct record sizes in bytes (properties and link data only).
    sizes: BTreeSet<u32>,
    sources: BTreeSet<String>,
}

#[derive(Default)]
struct Audit {
    properties: BTreeMap<String, Tally>,
    links: BTreeMap<String, Tally>,
    link_data: BTreeMap<String, Tally>,
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
    ];
    if as_json {
        let mut out = serde_json::Map::new();
        for (name, chunks) in sections {
            let rows: Vec<_> = chunks
                .iter()
                .map(|(chunk, t)| {
                    json!({
                        "chunk": chunk,
                        "entries": t.entries,
                        "sizes": t.sizes,
                        "sources": t.sources,
                    })
                })
                .collect();
            out.insert(name.to_string(), json!(rows));
        }
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    for (name, chunks) in sections {
        println!("== unparsed {name}: {} chunks", chunks.len());
        let mut rows: Vec<_> = chunks.iter().collect();
        rows.sort_by(|a, b| b.1.entries.cmp(&a.1.entries).then(a.0.cmp(b.0)));
        for (chunk, t) in rows {
            let sizes: Vec<String> = t.sizes.iter().map(u32::to_string).collect();
            let in_gamesys = t.sources.contains("shock2.gam");
            let missions = t.sources.len() - usize::from(in_gamesys);
            println!(
                "{chunk:<14} {:>6} entries  sizes [{}]  {}{missions} missions",
                t.entries,
                sizes.join(","),
                if in_gamesys { "gam + " } else { "" },
            );
        }
    }
    Ok(())
}
