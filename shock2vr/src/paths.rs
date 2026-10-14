use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[cfg(not(target_os = "android"))]
use tracing::warn;

#[cfg(target_os = "android")]
pub fn data_root() -> &'static Path {
    Path::new("/sdcard/shock2quest")
}

#[cfg(target_os = "android")]
static ANDROID_SAVE_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Set Android's app-private files directory before constructing the game.
#[cfg(target_os = "android")]
pub fn init_android_save_root(files_directory: &Path) {
    ANDROID_SAVE_ROOT
        .set(files_directory.to_path_buf())
        .expect("Android save root was already initialized");
}

/// Parent of the saves directory; Android keeps saves separate from retail assets.
pub fn save_root() -> &'static Path {
    #[cfg(target_os = "android")]
    {
        ANDROID_SAVE_ROOT
            .get()
            .expect("Initialize Android app-private save root before accessing saves")
            .as_path()
    }
    #[cfg(not(target_os = "android"))]
    {
        data_root()
    }
}

#[cfg(not(target_os = "android"))]
static DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();

#[cfg(not(target_os = "android"))]
const DESKTOP_CANDIDATES: &[&str] = &["./Data", "../Data", "../../Data", "."];

#[cfg(not(target_os = "android"))]
pub fn data_root() -> &'static Path {
    DATA_ROOT.get_or_init(resolve_desktop_data_root).as_path()
}

#[cfg(not(target_os = "android"))]
fn resolve_desktop_data_root() -> PathBuf {
    // Check for DARK_ASSET_PATH environment variable first
    if let Ok(env_path) = std::env::var("DARK_ASSET_PATH") {
        let path = Path::new(&env_path);
        if candidate_has_sentinel(path) {
            return path.to_path_buf();
        } else {
            warn!(
                path = %env_path,
                "DARK_ASSET_PATH set but no sentinel files found; falling back to default search"
            );
        }
    }

    for candidate in DESKTOP_CANDIDATES {
        let path = Path::new(candidate);
        if candidate_has_sentinel(path) {
            return path.to_path_buf();
        }
    }

    warn!(
        candidates = ?DESKTOP_CANDIDATES,
        "Falling back to default Data path; no sentinel files were found"
    );
    PathBuf::from("../../Data")
}

#[cfg(not(target_os = "android"))]
fn candidate_has_sentinel(path: &Path) -> bool {
    // One owner for "what counts as game data": `install` names both the
    // remaster's single archive and the pre-remaster loose files, and probes
    // the same set to report which layout was found.
    crate::install::probe(path).has_data()
}
