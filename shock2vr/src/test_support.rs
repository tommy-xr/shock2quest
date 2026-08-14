//! Helpers shared by unit tests across the crate.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Makes each directory unique regardless of `name`. Without it, uniqueness is
/// a crate-global convention enforced by nothing, and `new` starts with
/// `remove_dir_all` - so two tests that happened to pick the same name would
/// delete each other's fixtures mid-run.
static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

/// A scratch directory that removes itself when the test ends.
///
/// `name` only labels the directory for a human reading `/tmp`; uniqueness
/// comes from the pid and a counter, so no two live `TempDir`s collide.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(name: &str) -> TempDir {
        let path = std::env::temp_dir().join(format!(
            "shock2vr-{}-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed),
            name
        ));
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
