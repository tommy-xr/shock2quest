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

use std::path::Path;

use engine::assets::asset_paths::{AbstractAssetPath, AssetPath};

use crate::zip_asset_path::ZipAssetPath;

/// The single archive a 25th Anniversary Edition install ships its data in.
const BASE_ARCHIVE: &str = "sshock2.kpf";

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
        ZipAssetPath::with_prefix(archive, "data/"),
    ]
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
