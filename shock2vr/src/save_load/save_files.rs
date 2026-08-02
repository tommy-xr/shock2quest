//! Discovery of saved games on disk.
//!
//! Named saves live in `<data_root>/saves/<name>.sav` (see
//! [`crate::save_file_path`]). Screens that offer a reload - today the
//! game-over screen after a terminal death - need to know what is actually
//! available without hardcoding a file name, so they ask here.

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

/// The most recently written save, or `None` when nothing has been saved yet.
pub fn latest_save() -> Option<SaveFile> {
    latest_save_in(&save_directory())
}

/// [`latest_save`] against an explicit directory (missing directory -> `None`).
pub fn latest_save_in(directory: &Path) -> Option<SaveFile> {
    fs::read_dir(directory)
        .ok()?
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
        // Ties (same-second writes on coarse filesystems) resolve by name so
        // the choice is deterministic rather than directory-order dependent.
        .max_by(|a, b| {
            a.modified
                .cmp(&b.modified)
                .then_with(|| a.name.cmp(&b.name))
        })
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
