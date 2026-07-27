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
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::OnceLock;

use engine::assets::asset_paths::{AbstractAssetPath, AssetPath, ReadableAndSeekable};

use crate::zip_asset_path::ZipAssetPath;

/// The single archive a 25th Anniversary Edition install ships its data in.
const BASE_ARCHIVE: &str = "sshock2.kpf";

/// Where that archive keeps the gamesys and the missions.
const ARCHIVE_DATA_PREFIX: &str = "data/";

/// Whether `data_root` is a 25th Anniversary Edition install rather than a
/// classic one. The remaster ships its data inside `sshock2.kpf`; a classic
/// install has loose `.crf` archives instead.
pub fn is_25th_anniversary_install(data_root: &Path) -> bool {
    data_root.join(BASE_ARCHIVE).exists()
}

/// The archive mounts that hold the raw data files, highest priority first.
///
/// Empty for a classic install, where those files are loose at the data root and
/// the folder mount finds them.
pub fn data_file_mounts(data_root: &Path) -> Vec<Box<dyn AbstractAssetPath>> {
    if !is_25th_anniversary_install(data_root) {
        return Vec::new();
    }

    let archive = data_root.join(BASE_ARCHIVE).to_string_lossy().into_owned();
    vec![
        // `motiondb.bin` moved from the data root to `res/mschema/`, and the
        // missions + gamesys live under `data/`.
        ZipAssetPath::with_prefix(archive.clone(), "data/res/mschema/"),
        ZipAssetPath::with_prefix(archive, ARCHIVE_DATA_PREFIX),
    ]
}

/// Every mission file available in `data_root`, lowercased and sorted.
///
/// A classic install has them loose at the data root; a 25AE install has them
/// inside `sshock2.kpf` under `data/`, so a caller that only reads the directory
/// sees *no* missions at all there - an empty list rather than an error, which
/// is how a "run against every mission" tool silently ends up running against
/// nothing.
pub fn mission_names(data_root: &Path) -> Vec<String> {
    let mut names: BTreeSet<String> = loose_mission_names(data_root);
    names.extend(archived_mission_names(data_root));
    names.into_iter().collect()
}

fn loose_mission_names(data_root: &Path) -> BTreeSet<String> {
    let Ok(entries) = std::fs::read_dir(data_root) else {
        return BTreeSet::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase())
        .filter(|name| name.ends_with(".mis"))
        .collect()
}

/// The `.mis` entries directly under the archive's `data/` folder. Empty for a
/// classic install, and for an archive that cannot be opened - the caller
/// reports "no missions" either way, and the mount layer gives the real error
/// when something then tries to *read* a mission.
fn archived_mission_names(data_root: &Path) -> BTreeSet<String> {
    if !is_25th_anniversary_install(data_root) {
        return BTreeSet::new();
    }
    let Ok(file) = std::fs::File::open(data_root.join(BASE_ARCHIVE)) else {
        return BTreeSet::new();
    };
    let Ok(archive) = zip::ZipArchive::new(std::io::BufReader::new(file)) else {
        return BTreeSet::new();
    };
    archive
        .file_names()
        .filter_map(|name| {
            let lower = name.to_ascii_lowercase();
            let relative = lower.strip_prefix(ARCHIVE_DATA_PREFIX)?;
            // Only the missions themselves, not anything nested deeper.
            (relative.ends_with(".mis") && !relative.contains('/')).then(|| relative.to_owned())
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
/// from the resolved data root, in whichever layout is present.
///
/// The mounts are built once per process because indexing a 25AE archive is not
/// free and callers like `cargo bn path bench --all` read every mission through
/// here.
pub fn open_data_file(name: &str) -> Option<Box<dyn ReadableAndSeekable>> {
    static PATHS: OnceLock<Box<dyn AbstractAssetPath>> = OnceLock::new();
    let data_root = crate::paths::data_root();
    PATHS
        .get_or_init(|| asset_paths(data_root))
        .get_reader(
            data_root.to_string_lossy().into_owned(),
            name.to_ascii_lowercase(),
        )
        .map(RefCell::into_inner)
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::path::PathBuf;

    use super::*;

    /// A scratch directory that removes itself when the test ends.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let path = std::env::temp_dir().join(format!(
                "shock2vr-data-files-{}-{}",
                std::process::id(),
                name
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Write a stand-in for the 25AE archive holding `entries`.
    fn write_archive(root: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(root.join(BASE_ARCHIVE)).unwrap();
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

    #[test]
    fn classic_install_lists_loose_missions() {
        let dir = TempDir::new("classic-missions");
        std::fs::write(dir.path().join("earth.mis"), b"mission").unwrap();
        std::fs::write(dir.path().join("medsci1.mis"), b"mission").unwrap();
        std::fs::write(dir.path().join("shock2.gam"), b"gamesys").unwrap();

        assert_eq!(
            mission_names(dir.path()),
            vec!["earth.mis".to_owned(), "medsci1.mis".to_owned()]
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
