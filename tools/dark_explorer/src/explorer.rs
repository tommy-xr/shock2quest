//! Shared mount enumeration used by both the CLI subcommands and the UI.

use std::io::Read;

use engine::assets::asset_paths::{AbstractAssetPath, AssetEntry};

/// The families the tool enumerates, in the game's lookup priority order:
/// every family the game consults, plus the raw data files (gamesys, missions,
/// motiondb) as a pseudo-family, plus `fonts` on a classic install (mounted
/// from `res/fonts.crf` outside the family list there).
pub fn family_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = shock2vr::resource_families().to_vec();
    if !shock2vr::is_25th_anniversary_install() {
        names.push("fonts");
    }
    names.push("data");
    names
}

/// The combined mount stack for one family (or the `data` pseudo-family),
/// with the same archives and precedence the game resolves it through.
/// Building this indexes the archives, so callers that make repeated lookups
/// (the UI) should build it once per family and reuse it.
pub fn family_mounts(family: &str) -> Box<dyn AbstractAssetPath> {
    if family == "data" {
        engine::assets::asset_paths::AssetPath::combine(shock2vr::data_files::data_file_mounts(
            shock2vr::paths::data_root(),
        ))
    } else {
        shock2vr::resource_family_paths(family)
    }
}

pub fn entries_of(family: &str, mounts: &dyn AbstractAssetPath) -> Vec<AssetEntry> {
    let entries = mounts.entries();
    if family == "data" {
        // The data mount spans all of `data/`, which contains the res/
        // families already listed as their own families; keep only the
        // root-level files (gamesys, missions, motiondb).
        return entries
            .into_iter()
            .filter(|entry| !entry.key.contains('/'))
            .collect();
    }
    entries
}

pub fn family_entries(family: &str) -> Vec<AssetEntry> {
    let mounts = family_mounts(family);
    entries_of(family, &*mounts)
}

/// The bytes behind one asset key, read through the family's mount stack (so
/// the winning archive's copy is returned).
pub fn read_asset_bytes(mounts: &dyn AbstractAssetPath, key: &str) -> Option<Vec<u8>> {
    let base_path = shock2vr::paths::data_root().to_string_lossy().into_owned();
    let reader = mounts.get_reader(base_path, key.to_string())?;
    let mut bytes = Vec::new();
    reader.borrow_mut().read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Mounts the game consults that this tool cannot enumerate, so results can be
/// incomplete: a classic install's loose `res/mesh` / `res/obj` folder mounts
/// (which outrank the archives) and its loose data-root files.
pub fn print_coverage_caveat() {
    if !shock2vr::is_25th_anniversary_install() {
        eprintln!(
            "note: classic install - loose res/mesh and res/obj folders (which outrank the \
             archives) and loose data-root files are not enumerated; results may be incomplete"
        );
    }
}

/// Archive path relative to the data root, for compact display.
pub fn short_source(source: &str) -> String {
    let root = shock2vr::paths::data_root().to_string_lossy().into_owned();
    source
        .strip_prefix(&format!("{root}/"))
        .unwrap_or(source)
        .to_string()
}
