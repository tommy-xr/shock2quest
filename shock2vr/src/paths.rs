use std::path::{Path, PathBuf};

#[cfg(not(target_os = "android"))]
use std::sync::OnceLock;

#[cfg(not(target_os = "android"))]
use tracing::warn;

#[cfg(target_os = "android")]
pub fn data_root() -> &'static Path {
    Path::new("/mnt/sdcard/shock2quest")
}

#[cfg(target_os = "android")]
pub fn search_roots() -> &'static [&'static str] {
    &["/mnt/sdcard/shock2quest"]
}

#[cfg(not(target_os = "android"))]
static DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();

#[cfg(not(target_os = "android"))]
const DESKTOP_CANDIDATES: &[&str] = &["./Data", "../Data", "../../Data", "."];

#[cfg(not(target_os = "android"))]
const SENTINELS: &[&str] = &["shock2.gam", "res/obj.crf", "res/mesh.crf", "motiondb.bin"];

#[cfg(not(target_os = "android"))]
pub fn data_root() -> &'static Path {
    DATA_ROOT.get_or_init(resolve_desktop_data_root).as_path()
}

#[cfg(not(target_os = "android"))]
pub fn search_roots() -> &'static [&'static str] {
    DESKTOP_CANDIDATES
}

#[cfg(not(target_os = "android"))]
fn resolve_desktop_data_root() -> PathBuf {
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
    SENTINELS
        .iter()
        .map(|sentinel| path.join(sentinel))
        .any(|probe| probe.exists())
}
