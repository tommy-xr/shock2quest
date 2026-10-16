//! Saves written by every release since v0.0.3 must keep loading. The fixtures
//! are real saves from each tagged build, written by
//! `tools/shock2-sdk/scripts/gen-save-fixtures.ts`.
use super::{EntitySaveData, SaveData};
use dark::properties::Links;
use shipyard::{EntityId, Get, View, World};
use std::{fs::File, io::Read, path::PathBuf};

fn fixture_paths() -> Vec<PathBuf> {
    // check-save-compat.mjs points this at saves written by the base commit.
    let root = std::env::var_os("SHOCK2_SAVE_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/saves"));
    let mut paths: Vec<_> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("{}: {e}", root.display()))
        .map(|release| release.unwrap().path())
        .filter(|release| release.is_dir())
        .flat_map(|release| std::fs::read_dir(release).unwrap())
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.to_string_lossy().ends_with(".sav.gz"))
        .collect();
    paths.sort();
    paths
}

/// Decoding a property or link drops it with only a warning, so a schema break
/// there loads "successfully" with state missing. Count what survives instead.
fn check_restores(label: &str, data: &EntitySaveData, failures: &mut Vec<String>) {
    let mut world = World::new();
    let (_, entity_map) = data.instantiate(&mut world);
    // Values of entities outside `all_entities` (destroyed ones) are never restored.
    let restored_entity = |old: u64| EntityId::from_inner(old).and_then(|old| entity_map.get(&old));
    let (all_properties, _, _) = dark::properties::get::<File>();
    for (name, saved) in &data.properties {
        let Some(prop) = all_properties.iter().find(|p| p.name() == *name) else {
            failures.push(format!(
                "{label}: {name} is no longer a registered property"
            ));
            continue;
        };
        let expected = saved
            .keys()
            .filter(|old| restored_entity(**old).is_some())
            .count();
        let restored = prop.serialize(&world).len();
        if restored < expected {
            failures.push(format!("{label}: {name} restored {restored} of {expected}"));
        }
    }
    let restored_links = world.borrow::<View<Links>>().unwrap();
    for (old, saved) in &data.links {
        let Some(new) = restored_entity(*old) else {
            continue;
        };
        let Some(saved_count) = saved["to_links"].as_array().map(Vec::len) else {
            failures.push(format!("{label}: links of {old} have no to_links list"));
            continue;
        };
        let restored = restored_links
            .get(*new)
            .map_or(0, |links| links.to_links.len());
        if restored < saved_count {
            failures.push(format!(
                "{label}: links of {old} restored {restored} of {saved_count}"
            ));
        }
    }
}

#[test]
fn every_release_save_restores_all_of_its_state() {
    let paths = fixture_paths();
    assert!(!paths.is_empty(), "no save fixtures found");
    let mut failures = vec![];
    for path in &paths {
        let name = path.file_name().unwrap().to_string_lossy();
        let mut json = String::new();
        flate2::read::GzDecoder::new(File::open(path).unwrap())
            .read_to_string(&mut json)
            .unwrap();
        let save = match SaveData::read(&mut json.as_bytes()) {
            Ok(save) => save,
            Err(error) => {
                failures.push(format!("{name}: {error}"));
                continue;
            }
        };
        for (mission, level) in &save.level_data {
            check_restores(&format!("{name} {mission}"), level, &mut failures);
        }
        check_restores(
            &format!("{name} carried items"),
            &save.global_data.held_items.held_entities,
            &mut failures,
        );
    }
    assert!(
        failures.is_empty(),
        "release saves no longer load:\n{}",
        failures.join("\n")
    );
}
