//! Helpers shared by unit tests across the crate.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use dark::properties::{
    FrobFlag, KeyCard, PropFrobInfo, PropKeySrc, PropLog, PropScripts, PropStackCount,
    PropTemplateId,
};
use shipyard::{EntityId, UniqueView, World};

/// One of the four categories the game *collects* rather than carries. Spawned
/// the way the shipped data authors them, so a test exercises the same identity
/// `scripts::script_util::is_always_collected` reads at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectedKind {
    /// `PropKeySrc` - gains a derived `internal_keycard` script.
    KeyCard,
    /// A nanite pile - identified by descent from the pile templates.
    NanitePile,
    /// An `ExpCookie` cyber-module pile.
    CyberModule,
    /// A `LogDiscScript` disc with a readable `(deck, log)`.
    AudioLog,
}

impl CollectedKind {
    /// Every category, for the matrix tests that must cover all four.
    pub const ALL: [CollectedKind; 4] = [
        CollectedKind::KeyCard,
        CollectedKind::NanitePile,
        CollectedKind::CyberModule,
        CollectedKind::AudioLog,
    ];
}

/// Spawn one always-collected pickup of `kind` into `world`.
///
/// Every kind carries the frob metadata its archetype does, because the
/// acquisition paths branch on it: keycards and piles inherit `Goodies`'
/// grabbable `MOVE` (which is exactly why they must be recognized by category,
/// not by metadata), while modules and discs override it to `SCRIPT`.
pub fn spawn_collected(world: &mut World, kind: CollectedKind) -> EntityId {
    let goodies_frob = PropFrobInfo {
        world_action: FrobFlag::MOVE,
        inventory_action: FrobFlag::SCRIPT,
        tool_action: FrobFlag::empty(),
    };
    let script_frob = PropFrobInfo {
        world_action: FrobFlag::SCRIPT,
        inventory_action: FrobFlag::SCRIPT,
        tool_action: FrobFlag::empty(),
    };
    let scripts = |name: &str| PropScripts {
        scripts: vec![name.to_owned()],
        inherits: true,
    };
    match kind {
        CollectedKind::KeyCard => world.add_entity((
            goodies_frob,
            PropKeySrc(KeyCard {
                is_master: false,
                region_id: 128,
                lock_id: 0,
            }),
        )),
        CollectedKind::NanitePile => {
            register_nanite_hierarchy(world);
            world.add_entity((
                goodies_frob,
                PropTemplateId {
                    template_id: NANITE_PILE_TEMPLATE_ID,
                },
                PropStackCount(10),
            ))
        }
        CollectedKind::CyberModule => {
            world.add_entity((script_frob, scripts("ExpCookie"), PropStackCount(10)))
        }
        CollectedKind::AudioLog => world.add_entity((
            script_frob,
            scripts("LogDiscScript"),
            PropLog {
                deck: 2,
                email: 0,
                log: 5,
                note: 0,
                video: 0,
            },
        )),
    }
}

/// Ordinary grabbable loot - the control every matrix test needs, to show the
/// always-collected rule did not swallow the whole inventory.
pub fn spawn_ordinary_loot(world: &mut World) -> EntityId {
    world.add_entity(PropFrobInfo {
        world_action: FrobFlag::MOVE,
        inventory_action: FrobFlag::empty(),
        tool_action: FrobFlag::empty(),
    })
}

/// One of the base pile templates the predicate under test owns, taken from it
/// rather than restated - a fixture pinned to a stale id would silently stop
/// testing a pile.
const NANITE_PILE_TEMPLATE_ID: i32 = crate::scripts::script_util::NANITE_PILE_TEMPLATE_IDS[0];

/// Nanite piles are recognized through the runtime template hierarchy, so a
/// test world needs one. Registered once per world; adding the unique twice
/// would clobber another fixture's hierarchy.
fn register_nanite_hierarchy(world: &mut World) {
    if world
        .borrow::<UniqueView<crate::mission::mission_core::GlobalTemplateHierarchy>>()
        .is_ok()
    {
        return;
    }
    let mut hierarchy = HashMap::new();
    hierarchy.insert(NANITE_PILE_TEMPLATE_ID, Vec::new());
    world.add_unique(crate::mission::mission_core::GlobalTemplateHierarchy(
        hierarchy,
    ));
}

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

/// Write a zip archive at `path` holding `entries`, for tests that mount one.
pub fn write_archive(path: &Path, entries: &[(&str, &[u8])]) {
    use std::io::Write;
    let file = std::fs::File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    for (name, contents) in entries {
        writer
            .start_file(*name, zip::write::FileOptions::default())
            .unwrap();
        writer.write_all(contents).unwrap();
    }
    writer.finish().unwrap();
}
