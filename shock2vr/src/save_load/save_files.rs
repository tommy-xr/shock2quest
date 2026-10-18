//! Discovery of saved games on disk.
//!
//! Named saves live in `<save_root>/saves/<name>.sav` (see
//! [`crate::save_file_path`]). Screens that offer a reload - the load-game
//! screen, and the game-over screen after a terminal death - need to know what
//! is actually available without hardcoding a file name, so they ask here.

use std::{
    fs,
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::paths;

/// A saved game found on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveFile {
    /// The bare save name (file stem), as shown to the player.
    pub name: String,
    pub path: PathBuf,
    /// Write time, used to order "most recent first".
    pub modified: SystemTime,
    pub metadata: Option<super::SaveMetadata>,
    /// False for an empty slot shown in the save and load pickers.
    pub occupied: bool,
}

/// The directory holding named saves.
pub fn save_directory() -> PathBuf {
    paths::save_root().join("saves")
}

/// Every save on disk, most recent first. Empty when nothing has been saved
/// yet (or the directory does not exist).
pub fn all_saves() -> Vec<SaveFile> {
    all_saves_in(&save_directory())
}

/// The most recently written save, or `None` when nothing has been saved yet.
pub fn latest_save() -> Option<SaveFile> {
    latest_save_in(&save_directory())
}

/// Moves the `.sav` files older Quest builds kept in the app-private `legacy`
/// directory into `target`, one at a time, so an interrupted migration resumes
/// with what is left. A target save is never overwritten: a clashing legacy
/// save (e.g. `slot01.sav` beside an older shared-storage `slot01.sav`) lands
/// as `slot01-legacy.sav` and is listed with the imported saves; if that is
/// taken too it stays where it is. The directories sit on different
/// filesystems, so each file is copied (flushed, write time kept for the load
/// order) under a temporary name, renamed into place, then removed from
/// `legacy`. Returns each `(from, to)` moved.
pub fn migrate_legacy_saves(legacy: &Path, target: &Path) -> io::Result<Vec<(PathBuf, PathBuf)>> {
    let mut moved = Vec::new();
    for save in all_saves_in(legacy) {
        let Some(to) = [
            format!("{}.sav", save.name),
            format!("{}-legacy.sav", save.name),
        ]
        .into_iter()
        .map(|name| target.join(name))
        .find(|to| !to.exists()) else {
            continue;
        };
        fs::create_dir_all(target)?;
        let staged = to.with_extension("migrating");
        fs::copy(&save.path, &staged)?;
        let file = fs::File::options().write(true).open(&staged)?;
        // Best effort: a filesystem that refuses it only costs load-list order.
        let _ = file.set_modified(save.modified);
        file.sync_all()?;
        fs::rename(&staged, &to)?;
        fs::remove_file(&save.path)?;
        moved.push((save.path, to));
    }
    Ok(moved)
}

/// [`all_saves`] against an explicit directory (missing directory -> empty).
fn all_saves_in(directory: &Path) -> Vec<SaveFile> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut saves: Vec<SaveFile> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.extension().is_some_and(|ext| ext == "sav") {
                return None;
            }
            let modified = entry.metadata().ok()?.modified().ok()?;
            let metadata = fs::File::open(&path).ok().and_then(read_metadata);
            Some(SaveFile {
                metadata,
                occupied: true,
                name: path.file_stem()?.to_string_lossy().into_owned(),
                path,
                modified,
            })
        })
        .collect();
    // Most recent first. Ties (same-second writes on coarse filesystems)
    // resolve by name so the order is deterministic rather than
    // directory-order dependent. Both keys descend, which keeps
    // `latest_save` - now the head of this list - picking the same save it
    // did when it was a `max_by` over the same ordering.
    saves.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| b.name.cmp(&a.name))
    });
    saves
}

fn read_metadata(reader: impl Read) -> Option<super::SaveMetadata> {
    use serde::Deserialize;

    // SaveData writes metadata first. Bound discovery even for damaged saves;
    // the remaining world snapshot can span megabytes and is decoded only on load.
    const PREFIX: &[u8] = b"{\"metadata\":";
    let mut reader = BufReader::new(reader.take(4096));
    let mut prefix = [0; PREFIX.len()];
    reader.read_exact(&mut prefix).ok()?;
    if prefix != PREFIX {
        return None;
    }
    // Direct deserialization stops at this object instead of requiring file EOF.
    super::SaveMetadata::deserialize(&mut serde_json::Deserializer::from_reader(reader)).ok()
}

/// [`latest_save`] against an explicit directory (missing directory -> `None`).
fn latest_save_in(directory: &Path) -> Option<SaveFile> {
    all_saves_in(directory).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;
    use std::time::Duration;

    fn write_save(dir: &Path, name: &str) {
        fs::write(dir.join(name), b"save").unwrap();
    }

    #[test]
    fn discovery_reads_metadata_without_decoding_the_world_snapshot() {
        let dir = TempDir::new("save-files-metadata");
        fs::write(
            dir.path().join("slot01.sav"),
            br#"{"metadata":{"mission":"medsci1.mis","played":{"secs":3723,"nanos":0}},"global_data":not decoded"#,
        )
        .unwrap();

        let saves = all_saves_in(dir.path());
        assert_eq!(saves.len(), 1);
        assert_eq!(
            saves[0].metadata,
            Some(super::super::SaveMetadata {
                mission: "medsci1.mis".to_owned(),
                played: Duration::from_secs(3723),
                ..Default::default()
            })
        );
    }

    #[test]
    fn missing_or_malformed_metadata_keeps_save_available() {
        let dir = TempDir::new("save-files-legacy");
        fs::write(dir.path().join("legacy.sav"), br#"{"global_data":{}}"#).unwrap();
        fs::write(dir.path().join("damaged.sav"), br#"{"metadata":broken"#).unwrap();

        let saves = all_saves_in(dir.path());
        assert_eq!(saves.len(), 2);
        assert!(
            saves
                .iter()
                .all(|save| save.occupied && save.metadata.is_none())
        );
    }

    #[test]
    fn metadata_discovery_is_bounded() {
        let json = format!(
            "{{\"metadata\":{{\"mission\":\"{}\",\"played\":{{\"secs\":0,\"nanos\":0}}}}}}",
            "x".repeat(4096)
        );
        assert_eq!(read_metadata(json.as_bytes()), None);
    }

    #[test]
    fn legacy_saves_move_without_overwriting_target_saves() {
        let legacy = TempDir::new("save-migrate-legacy");
        let target = TempDir::new("save-migrate-target");
        let target_saves = target.path().join("saves");
        write_save(legacy.path(), "slot01.sav");
        write_save(legacy.path(), "quicksave.sav");
        write_save(legacy.path(), "notes.txt");
        fs::create_dir(&target_saves).unwrap();
        fs::write(target_saves.join("slot01.sav"), b"shared").unwrap();

        let mut moved = migrate_legacy_saves(legacy.path(), &target_saves).unwrap();
        moved.sort();
        assert_eq!(
            moved,
            [
                (
                    legacy.path().join("quicksave.sav"),
                    target_saves.join("quicksave.sav")
                ),
                (
                    legacy.path().join("slot01.sav"),
                    target_saves.join("slot01-legacy.sav")
                ),
            ]
        );
        assert_eq!(
            fs::read(target_saves.join("slot01.sav")).unwrap(),
            b"shared"
        );
        assert_eq!(
            fs::read(target_saves.join("slot01-legacy.sav")).unwrap(),
            b"save"
        );
        assert!(all_saves_in(legacy.path()).is_empty());
        assert!(legacy.path().join("notes.txt").exists());
        assert_eq!(all_saves_in(&target_saves).len(), 3);
        // Nothing left to move on the next launch.
        assert!(
            migrate_legacy_saves(legacy.path(), &target_saves)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_save_with_no_free_name_stays_in_place() {
        let legacy = TempDir::new("save-migrate-full");
        let target = TempDir::new("save-migrate-full-target");
        write_save(legacy.path(), "slot01.sav");
        write_save(target.path(), "slot01.sav");
        write_save(target.path(), "slot01-legacy.sav");
        assert!(
            migrate_legacy_saves(legacy.path(), target.path())
                .unwrap()
                .is_empty()
        );
        assert!(legacy.path().join("slot01.sav").exists());
    }

    #[test]
    fn migration_keeps_write_times_and_the_latest_save() {
        let legacy = TempDir::new("save-migrate-mtime");
        let target = TempDir::new("save-migrate-mtime-target");
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        for (name, age) in [("slot01.sav", 0), ("slot02.sav", 60)] {
            write_save(legacy.path(), name);
            fs::File::options()
                .write(true)
                .open(legacy.path().join(name))
                .unwrap()
                .set_modified(base - Duration::from_secs(age))
                .unwrap();
        }
        migrate_legacy_saves(legacy.path(), target.path()).unwrap();
        let saves = all_saves_in(target.path());
        assert_eq!(saves[0].name, "slot01");
        assert_eq!(saves[0].modified, base);
        assert_eq!(saves[1].modified, base - Duration::from_secs(60));
    }

    #[test]
    fn missing_legacy_directory_moves_nothing() {
        let target = TempDir::new("save-migrate-none-target");
        let missing = target.path().join("missing");
        assert!(
            migrate_legacy_saves(&missing, target.path())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn missing_directory_has_no_latest_save() {
        assert_eq!(
            latest_save_in(Path::new("/nonexistent/shock2vr/saves")),
            None
        );
    }

    #[test]
    fn empty_directory_has_no_latest_save() {
        let dir = TempDir::new("save-files-empty");
        assert_eq!(latest_save_in(dir.path()), None);
    }

    #[test]
    fn non_save_files_are_ignored() {
        let dir = TempDir::new("save-files-filter");
        write_save(dir.path(), "notes.txt");
        assert_eq!(latest_save_in(dir.path()), None);
    }

    #[test]
    fn all_saves_lists_every_save_most_recent_first() {
        let dir = TempDir::new("save-files-listing");
        write_save(dir.path(), "old.sav");
        std::thread::sleep(Duration::from_millis(20));
        write_save(dir.path(), "new.sav");
        write_save(dir.path(), "notes.txt");

        let names: Vec<String> = all_saves_in(dir.path())
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, vec!["new".to_owned(), "old".to_owned()]);
    }

    #[test]
    fn all_saves_is_empty_without_a_directory() {
        assert!(all_saves_in(Path::new("/nonexistent/shock2vr/saves")).is_empty());
    }

    #[test]
    fn same_timestamp_saves_order_by_name_descending() {
        // Ties must resolve the same way they did when `latest_save` was a
        // `max_by` over (modified, name): the greater name wins, so it heads
        // the list and stays what `latest_save` returns.
        let dir = TempDir::new("save-files-ties");
        write_save(dir.path(), "alpha.sav");
        write_save(dir.path(), "beta.sav");
        let saves = all_saves_in(dir.path());
        // Only meaningful when the filesystem really gave them equal mtimes.
        if saves[0].modified == saves[1].modified {
            assert_eq!(saves[0].name, "beta");
            assert_eq!(latest_save_in(dir.path()).unwrap().name, "beta");
        }
    }

    #[test]
    fn the_most_recently_written_save_wins() {
        let dir = TempDir::new("save-files-recency");
        write_save(dir.path(), "old.sav");
        // Sleep past the filesystem's mtime granularity before the newer write.
        std::thread::sleep(Duration::from_millis(20));
        write_save(dir.path(), "new.sav");

        let latest = latest_save_in(dir.path()).expect("a save should be found");
        assert_eq!(latest.name, "new");
        assert_eq!(latest.path, dir.path().join("new.sav"));
    }
}
