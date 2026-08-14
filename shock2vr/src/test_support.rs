//! Helpers shared by unit tests across the crate.

use std::path::{Path, PathBuf};

/// A scratch directory that removes itself when the test ends.
///
/// `name` must be unique per test: the path is derived from it, so two tests
/// sharing a name would share a directory and race.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(name: &str) -> TempDir {
        let path = std::env::temp_dir().join(format!("shock2vr-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
