//! Discovery of saved games on disk.
//!
//! Named saves live in `<save_root>/saves/<name>.sav` (see
//! [`crate::save_file_path`]). Screens that offer a reload - the load-game
//! screen, and the game-over screen after a terminal death - need to know what
//! is actually available without hardcoding a file name, so they ask here.

use std::{
    fs,
    io::{BufReader, Read},
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
