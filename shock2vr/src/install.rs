//! What game data is actually present, decided in one place before anything
//! mounts it.
//!
//! Until this existed, "which install is this?" was answered implicitly and in
//! pieces: [`crate::paths::data_root`] searched for sentinel files,
//! [`crate::data_files::is_25th_anniversary_install`] checked for one archive,
//! and `Game::init` built one of two mount lists and then panicked on a missing
//! `shock2.gam`. Nothing reported the answer, which made a wrongly-provisioned
//! device indistinguishable from a broken build - on Quest the panic is just a
//! return to the Horizon shell.
//!
//! This module answers the question once, as a pure function of the filesystem,
//! so it can be logged at startup, unit-tested headlessly, and (later) shown to
//! the player on a screen that has no assets to draw itself with.

use std::path::{Path, PathBuf};

/// The archive a 25th Anniversary install keeps its data in. Its presence is
/// what distinguishes the two layouts.
pub(crate) const ANNIVERSARY_SENTINEL: &str = "sshock2.kpf";

/// Whether `data_root` holds a 25th Anniversary install.
///
/// The single predicate: `data_files` mounts from it, `Game::init` picks a
/// mount list from it, and [`probe`] classifies from it, so the three cannot
/// disagree. `is_file` rather than `exists` so a directory or dangling link
/// named `sshock2.kpf` is not mistaken for the archive.
pub(crate) fn is_anniversary(data_root: &Path) -> bool {
    data_root.join(ANNIVERSARY_SENTINEL).is_file()
}

/// Files that mark a directory as a *pre-remaster* game-data root. That install
/// has the gamesys and `.crf` archives loose; the remaster has none of them.
pub(crate) const LEGACY_SENTINELS: &[&str] =
    &["shock2.gam", "res/obj.crf", "res/mesh.crf", "motiondb.bin"];

/// Which install layout a data root holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    /// A 25th Anniversary install - what the game targets.
    Anniversary,
    /// A pre-remaster install. Still resolves, but is missing the upgraded art
    /// the VR hands and weapons are built against.
    Legacy,
    /// No recognizable game data.
    Missing,
}

/// What a data root holds, and what it is missing.
#[derive(Debug)]
pub struct InstallStatus {
    pub data_root: PathBuf,
    pub kind: InstallKind,
    /// Data files found, in probe order.
    pub found: Vec<&'static str>,
    /// Mod archives the remaster should have but doesn't. Always empty unless
    /// [`InstallKind::Anniversary`].
    ///
    /// Worth reporting on its own because it is the one provisioning mistake
    /// that is otherwise *silent*: copying `sshock2.kpf` without `mods/` boots
    /// perfectly and renders the original 1999 art, so it looks like a working
    /// install and reads as "the upgrade did nothing".
    pub missing_mods: Vec<&'static str>,
}

impl InstallStatus {
    /// Whether there is anything to load at all.
    pub fn has_data(&self) -> bool {
        self.kind != InstallKind::Missing
    }

    /// One line for the startup log - the first thing to look at when a device
    /// renders the wrong art or fails to boot.
    pub fn summary(&self) -> String {
        // `data_root` is often relative (`../../Data`, including the fallback
        // used when nothing was found), and a relative path cannot be acted on
        // without knowing the cwd - which in the fallback case is the thing
        // that went wrong.
        let absolute = std::fs::canonicalize(&self.data_root).unwrap_or_else(|_| {
            std::env::current_dir()
                .unwrap_or_default()
                .join(&self.data_root)
        });
        let root = absolute.display();
        match self.kind {
            InstallKind::Anniversary if self.missing_mods.is_empty() => {
                format!(
                    "install: 25th Anniversary at {root} ({})",
                    self.found.join(", ")
                )
            }
            InstallKind::Anniversary => format!(
                "install: 25th Anniversary at {root} ({}) - missing mod layers: {}; \
                 art from those layers will not load",
                self.found.join(", "),
                self.missing_mods.join(", ")
            ),
            InstallKind::Legacy => format!(
                "install: pre-remaster at {root} ({}) - no {ANNIVERSARY_SENTINEL}, \
                 so the upgraded art is unavailable",
                self.found.join(", ")
            ),
            InstallKind::Missing => format!(
                "install: NO GAME DATA at {root} - looked for {ANNIVERSARY_SENTINEL}, {}",
                LEGACY_SENTINELS.join(", ")
            ),
        }
    }
}

/// Probe `data_root` for game data. Pure: touches only the filesystem, mounts
/// nothing, and never panics on a missing or unreadable root.
pub fn probe(data_root: &Path) -> InstallStatus {
    let mut found = Vec::new();
    let mut missing_mods = Vec::new();

    let kind = if is_anniversary(data_root) {
        found.push(ANNIVERSARY_SENTINEL);
        for archive in crate::MOD_ARCHIVES {
            if data_root.join(archive).exists() {
                found.push(*archive);
            } else {
                missing_mods.push(*archive);
            }
        }
        InstallKind::Anniversary
    } else {
        for sentinel in LEGACY_SENTINELS {
            if data_root.join(sentinel).exists() {
                found.push(*sentinel);
            }
        }
        if found.is_empty() {
            InstallKind::Missing
        } else {
            InstallKind::Legacy
        }
    };

    InstallStatus {
        data_root: data_root.to_path_buf(),
        kind,
        found,
        missing_mods,
    }
}

/// Probe the data root the game will actually load from.
pub fn probe_data_root() -> InstallStatus {
    probe(crate::paths::data_root())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn touch(root: &Path, relative: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }

    #[test]
    fn a_root_with_the_base_archive_and_every_mod_is_a_complete_remaster() {
        let dir = TempDir::new("install-complete");
        touch(dir.path(), ANNIVERSARY_SENTINEL);
        for archive in crate::MOD_ARCHIVES {
            touch(dir.path(), archive);
        }

        let status = probe(dir.path());
        assert_eq!(status.kind, InstallKind::Anniversary);
        assert!(status.missing_mods.is_empty());
        assert!(status.has_data());
        assert_eq!(status.found.len(), 1 + crate::MOD_ARCHIVES.len());
    }

    /// The silent provisioning failure: the game boots and renders, just with
    /// the original art, so nothing else in the system would report it.
    #[test]
    fn a_remaster_without_its_mods_is_reported_as_incomplete() {
        let dir = TempDir::new("install-no-mods");
        touch(dir.path(), ANNIVERSARY_SENTINEL);

        let status = probe(dir.path());
        assert_eq!(status.kind, InstallKind::Anniversary);
        assert_eq!(status.missing_mods.len(), crate::MOD_ARCHIVES.len());
        assert!(status.summary().contains("missing mod layers"));
    }

    #[test]
    fn a_root_with_only_legacy_sentinels_is_a_pre_remaster_install() {
        let dir = TempDir::new("install-legacy");
        touch(dir.path(), "shock2.gam");
        touch(dir.path(), "res/obj.crf");

        let status = probe(dir.path());
        assert_eq!(status.kind, InstallKind::Legacy);
        assert_eq!(status.found, vec!["shock2.gam", "res/obj.crf"]);
        assert!(status.has_data());
    }

    /// The base archive wins outright: `Game::init` mounts the KPF list and
    /// never touches the `.crf`s, so a root holding both is a remaster.
    #[test]
    fn the_base_archive_wins_over_leftover_legacy_files() {
        let dir = TempDir::new("install-both");
        touch(dir.path(), ANNIVERSARY_SENTINEL);
        touch(dir.path(), "shock2.gam");
        touch(dir.path(), "res/obj.crf");

        assert_eq!(probe(dir.path()).kind, InstallKind::Anniversary);
    }

    #[test]
    fn an_empty_root_has_no_data_and_says_where_it_looked() {
        let dir = TempDir::new("install-empty");

        let status = probe(dir.path());
        assert_eq!(status.kind, InstallKind::Missing);
        assert!(!status.has_data());
        assert!(status.found.is_empty());
        let summary = status.summary();
        assert!(summary.contains("NO GAME DATA"));
        assert!(summary.contains(ANNIVERSARY_SENTINEL));
        assert!(summary.contains("shock2.gam"));
    }

    /// A path that does not exist at all must probe, not panic - it is the
    /// first-run state on a device where nothing has been copied yet.
    #[test]
    fn a_nonexistent_root_probes_as_missing() {
        let status = probe(Path::new("/definitely/not/a/data/root"));
        assert_eq!(status.kind, InstallKind::Missing);
        assert!(status.summary().contains("/definitely/not/a/data/root"));
    }
}
