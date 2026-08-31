//! Raw archive access for the Archives tab: the full contents of every
//! KPF/CRF on disk, read straight from the zip rather than through the game's
//! mount stack (so entries the game never mounts are visible too).
//!
//! Not `ZipAssetPath`: that mounts one prefix-scoped family and panics on an
//! archive it cannot read, where this browses whole archives and reports a
//! failure as a label.

use std::path::{Path, PathBuf};

/// Every `.kpf`/`.crf` under the data root, sorted: the top level, `mods/`
/// (25AE layers) and `res/` (classic families) are the only places the game
/// keeps archives.
pub fn discover(data_root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for dir in ["", "mods", "res"] {
        let path = if dir.is_empty() {
            data_root.to_path_buf()
        } else {
            data_root.join(dir)
        };
        let Ok(read_dir) = std::fs::read_dir(&path) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if ext == "kpf" || ext == "crf" {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The file names in the archive's central directory (original case, sorted) -
/// the listing only, so even a multi-GB archive lists in milliseconds.
pub fn list_entries(path: &Path) -> Result<Vec<String>, String> {
    let file = std::fs::File::open(path).map_err(|err| err.to_string())?;
    let mut archive =
        zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|err| err.to_string())?;
    let mut entries = Vec::with_capacity(archive.len());
    // One malformed entry must not cost the whole listing. `enclosed_name`
    // normalizes the path (and drops anything escaping the archive), matching
    // how the mount index keys entry names.
    for index in 0..archive.len() {
        let Ok(file) = archive.by_index(index) else {
            continue;
        };
        if file.is_dir() {
            continue;
        }
        let Some(name) = file.enclosed_name() else {
            continue;
        };
        entries.push(name.to_string_lossy().into_owned());
    }
    entries.sort();
    Ok(entries)
}

pub fn read_entry(path: &Path, name: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|err| err.to_string())?;
    let mut archive =
        zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|err| err.to_string())?;
    let mut entry = archive.by_name(name).map_err(|err| err.to_string())?;
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .map_err(|err| err.to_string())?;
    Ok(bytes)
}
