//! Discovery of saved games on disk.
//!
//! Named saves live in `<data_root>/saves/<name>.sav` (see
//! [`crate::save_file_path`]). Screens that offer a reload - the load-game
//! screen, and the game-over screen after a terminal death - need to know what
//! is actually available without hardcoding a file name, so they ask here.

use std::{
    fs,
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
}

/// The directory holding named saves.
pub fn save_directory() -> PathBuf {
    paths::data_root().join("saves")
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
            Some(SaveFile {
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
    saves.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| b.name.cmp(&a.name)));
    saves
}

/// [`latest_save`] against an explicit directory (missing directory -> `None`).
fn latest_save_in(directory: &Path) -> Option<SaveFile> {
    all_saves_in(directory).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shock2vr_save_files_{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_save(dir: &Path, name: &str) {
        fs::write(dir.join(name), b"save").unwrap();
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
        let dir = scratch_dir("empty");
        assert_eq!(latest_save_in(&dir), None);
    }

    #[test]
    fn non_save_files_are_ignored() {
        let dir = scratch_dir("filter");
        write_save(&dir, "notes.txt");
        assert_eq!(latest_save_in(&dir), None);
    }

    #[test]
    fn all_saves_lists_every_save_most_recent_first() {
        let dir = scratch_dir("listing");
        write_save(&dir, "old.sav");
        std::thread::sleep(Duration::from_millis(20));
        write_save(&dir, "new.sav");
        write_save(&dir, "notes.txt");

        let names: Vec<String> = all_saves_in(&dir).into_iter().map(|s| s.name).collect();
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
        let dir = scratch_dir("ties");
        write_save(&dir, "alpha.sav");
        write_save(&dir, "beta.sav");
        let saves = all_saves_in(&dir);
        // Only meaningful when the filesystem really gave them equal mtimes.
        if saves[0].modified == saves[1].modified {
            assert_eq!(saves[0].name, "beta");
            assert_eq!(latest_save_in(&dir).unwrap().name, "beta");
        }
    }

    #[test]
    fn the_most_recently_written_save_wins() {
        let dir = scratch_dir("recency");
        write_save(&dir, "old.sav");
        // Sleep past the filesystem's mtime granularity before the newer write.
        std::thread::sleep(Duration::from_millis(20));
        write_save(&dir, "new.sav");

        let latest = latest_save_in(&dir).expect("a save should be found");
        assert_eq!(latest.name, "new");
        assert_eq!(latest.path, dir.join("new.sav"));
    }
}
