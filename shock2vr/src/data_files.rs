//! Resolving the raw game-data files - the gamesys (`shock2.gam`), the missions
//! (`*.mis`) and `motiondb.bin` - in whichever install layout is present.
//!
//! A classic install keeps them loose at the data root. A 25th Anniversary
//! Edition install has none of them on disk: everything lives inside
//! `sshock2.kpf`, with the gamesys and missions under `data/` and `motiondb.bin`
//! under `data/res/mschema/`.
//!
//! The renderer's mount list (`Game::init`) already handles both layouts; this
//! module owns the data-file part of it so that CLI tools with no renderer (and
//! no bundle storage) resolve exactly the same files the game does, instead of
//! `File::open`ing the data root and failing on a 25AE install.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

use engine::assets::asset_paths::{AbstractAssetPath, AssetPath, ReadableAndSeekable};

use crate::zip_asset_path::ZipAssetPath;

/// Where that archive keeps the gamesys and the missions.
const ARCHIVE_DATA_PREFIX: &str = "data/";

/// Whether `data_root` is a 25th Anniversary Edition install rather than a
/// classic one. The remaster ships its data inside `sshock2.kpf`; a classic
/// install has loose `.crf` archives instead.
pub fn is_25th_anniversary_install(data_root: &Path) -> bool {
    crate::install::is_anniversary(data_root)
}

/// The archive mounts that hold the raw data files, highest priority first.
///
/// Empty for a classic install, where those files are loose at the data root and
/// the folder mount finds them.
pub fn data_file_mounts(data_root: &Path) -> Vec<Box<dyn AbstractAssetPath>> {
    if !is_25th_anniversary_install(data_root) {
        return Vec::new();
    }

    let archive = data_root
        .join(crate::install::ANNIVERSARY_SENTINEL)
        .to_string_lossy()
        .into_owned();
    vec![
        // `motiondb.bin` moved from the data root to `res/mschema/`, and the
        // missions + gamesys live under `data/`.
        ZipAssetPath::with_prefix(archive.clone(), "data/res/mschema/"),
        ZipAssetPath::with_prefix(archive, ARCHIVE_DATA_PREFIX),
    ]
}

/// Every mission file available in `data_root`, sorted, with the spelling that
/// opens it: a loose file keeps its on-disk case (which is what a case-sensitive
/// filesystem needs), an archived one its lowercased archive name.
///
/// A classic install has them loose at the data root; a 25AE install has them
/// inside `sshock2.kpf` under `data/`, so a caller that only reads the directory
/// sees *no* missions at all there - an empty list rather than an error, which
/// is how a "run against every mission" tool silently ends up running against
/// nothing.
pub fn mission_names(data_root: &Path) -> Vec<String> {
    // Keyed by lowercased name so the same mission present in both layouts is
    // listed once, with the loose spelling winning.
    let mut names: BTreeMap<String, String> = archived_mission_names(data_root);
    names.extend(loose_mission_names(data_root));
    names.into_values().collect()
}

fn loose_mission_names(data_root: &Path) -> BTreeMap<String, String> {
    let Ok(entries) = std::fs::read_dir(data_root) else {
        return BTreeMap::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.to_ascii_lowercase().ends_with(".mis"))
        .map(|name| (name.to_ascii_lowercase(), name))
        .collect()
}

/// The `.mis` entries directly under the archive's `data/` folder. Empty for a
/// classic install, and for an archive that cannot be opened - the caller
/// reports "no missions" either way, and the mount layer gives the real error
/// when something then tries to *read* a mission.
fn archived_mission_names(data_root: &Path) -> BTreeMap<String, String> {
    if !is_25th_anniversary_install(data_root) {
        return BTreeMap::new();
    }
    let Ok(file) = std::fs::File::open(data_root.join(crate::install::ANNIVERSARY_SENTINEL)) else {
        return BTreeMap::new();
    };
    let Ok(archive) = zip::ZipArchive::new(std::io::BufReader::new(file)) else {
        return BTreeMap::new();
    };
    archive
        .file_names()
        .filter_map(|name| {
            let lower = name.to_ascii_lowercase();
            let relative = lower.strip_prefix(ARCHIVE_DATA_PREFIX)?;
            // Only the missions themselves, not anything nested deeper.
            (relative.ends_with(".mis") && !relative.contains('/'))
                .then(|| (relative.to_owned(), relative.to_owned()))
        })
        .collect()
}

/// An asset-path layer that resolves only the raw data files - no textures,
/// sounds or bundle storage, so a CLI tool can build it without a renderer.
///
/// Pass `data_root` as the `base_path` argument of `exists`/`get_reader`: that
/// is what the loose-file mount resolves names against.
pub fn asset_paths(data_root: &Path) -> Box<dyn AbstractAssetPath> {
    let mut mounts = data_file_mounts(data_root);
    mounts.push(AssetPath::folder(String::new()));
    AssetPath::combine(mounts)
}

/// Open a raw data file by name (`shock2.gam`, `medsci1.mis`, `motiondb.bin`)
/// from the resolved data root, in whichever layout is present. `None` means
/// "no mount has that name" - the cause of a filesystem error is not reported.
///
/// The mounts are built once per process because indexing a 25AE archive is not
/// free and callers like `cargo bn path bench --all` read every mission through
/// here.
pub fn open_data_file(name: &str) -> Option<Box<dyn ReadableAndSeekable>> {
    static PATHS: OnceLock<Box<dyn AbstractAssetPath>> = OnceLock::new();
    let data_root = crate::paths::data_root();
    let paths = PATHS.get_or_init(|| asset_paths(data_root));
    let base_path = data_root.to_string_lossy().into_owned();
    // Archive keys are lowercased, but the loose-file mount resolves a name
    // byte-for-byte, so an on-disk `Earth.mis` only opens under its own
    // spelling on a case-sensitive filesystem.
    paths
        .get_reader(base_path.clone(), name.to_ascii_lowercase())
        .or_else(|| paths.get_reader(base_path, name.to_owned()))
        .map(RefCell::into_inner)
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use super::*;
    use crate::test_support::TempDir;

    /// Write a stand-in for the 25AE archive holding `entries`.
    fn write_archive(root: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(root.join(crate::install::ANNIVERSARY_SENTINEL)).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, contents) in entries {
            writer
                .start_file(*name, zip::write::FileOptions::default())
                .unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap();
    }

    fn read(root: &Path, name: &str) -> Option<Vec<u8>> {
        let paths = asset_paths(root);
        let reader = paths.get_reader(root.to_string_lossy().into_owned(), name.to_owned())?;
        let mut bytes = Vec::new();
        reader.borrow_mut().read_to_end(&mut bytes).unwrap();
        Some(bytes)
    }

    #[test]
    fn classic_install_reads_loose_data_files() {
        let dir = TempDir::new("classic");
        std::fs::write(dir.path().join("shock2.gam"), b"loose gamesys").unwrap();

        assert!(!is_25th_anniversary_install(dir.path()));
        assert_eq!(
            read(dir.path(), "shock2.gam").as_deref(),
            Some(&b"loose gamesys"[..])
        );
    }

    /// The 25AE install has no loose files at all - the gamesys, the missions and
    /// `motiondb.bin` have to come out of `sshock2.kpf`.
    #[test]
    fn anniversary_install_reads_data_files_out_of_the_archive() {
        let dir = TempDir::new("25th");
        write_archive(
            dir.path(),
            &[
                ("data/shock2.gam", b"archived gamesys"),
                ("data/command1.mis", b"archived mission"),
                ("data/res/mschema/motiondb.bin", b"archived motiondb"),
            ],
        );

        assert!(is_25th_anniversary_install(dir.path()));
        assert_eq!(
            read(dir.path(), "shock2.gam").as_deref(),
            Some(&b"archived gamesys"[..])
        );
        assert_eq!(
            read(dir.path(), "command1.mis").as_deref(),
            Some(&b"archived mission"[..])
        );
        assert_eq!(
            read(dir.path(), "motiondb.bin").as_deref(),
            Some(&b"archived motiondb"[..])
        );
    }

    /// A loose mission keeps its on-disk spelling: that is the only name that
    /// opens it on a case-sensitive filesystem.
    #[test]
    fn classic_install_lists_loose_missions() {
        let dir = TempDir::new("classic-missions");
        std::fs::write(dir.path().join("Earth.MIS"), b"mission").unwrap();
        std::fs::write(dir.path().join("medsci1.mis"), b"mission").unwrap();
        std::fs::write(dir.path().join("shock2.gam"), b"gamesys").unwrap();

        assert_eq!(
            mission_names(dir.path()),
            vec!["Earth.MIS".to_owned(), "medsci1.mis".to_owned()]
        );
    }

    /// A 25AE install has no loose `.mis` at all, so reading the directory finds
    /// nothing - the missions have to be enumerated out of the archive.
    #[test]
    fn anniversary_install_lists_missions_from_the_archive() {
        let dir = TempDir::new("25th-missions");
        write_archive(
            dir.path(),
            &[
                ("data/medsci1.mis", b"mission"),
                ("data/earth.mis", b"mission"),
                ("data/shock2.gam", b"gamesys"),
                // Not a mission at the data root - must not be listed.
                ("data/res/mschema/motiondb.bin", b"motiondb"),
                ("data/saves/quick.mis", b"nested"),
            ],
        );

        assert_eq!(
            mission_names(dir.path()),
            vec!["earth.mis".to_owned(), "medsci1.mis".to_owned()]
        );
    }

    #[test]
    fn a_missing_data_file_resolves_to_nothing_rather_than_panicking() {
        let dir = TempDir::new("missing");
        write_archive(dir.path(), &[("data/shock2.gam", b"archived gamesys")]);

        assert!(!asset_paths(dir.path()).exists(
            dir.path().to_string_lossy().into_owned(),
            "nope.mis".to_owned()
        ));
        assert_eq!(read(dir.path(), "nope.mis"), None);
    }
}
