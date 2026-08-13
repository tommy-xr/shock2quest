//! The SS2 master palette (`res/pal/SHOCKPAL.PCX`).
//!
//! Some Dark data stores colors as 8-bit indices into this game-wide master
//! palette rather than literal RGB - notably particle-group colors
//! (`PropParticleGroup`'s `cr/cg/cb`). The texture path never needed it (PCX
//! textures carry their own embedded palettes), so it isn't loaded elsewhere.
//! Loaded once and cached.

use std::io::Read;
use std::sync::OnceLock;

use cgmath::{Vector3, vec3};
use tracing::warn;

use crate::data_files;

/// Master palette PCX, relative to the data root.
const PALETTE_FILE: &str = "res/pal/SHOCKPAL.PCX";

/// 256 RGB triples. Index 0 is the magenta color key / "unused" sentinel.
pub type MasterPalette = [[u8; 3]; 256];

static MASTER_PALETTE: OnceLock<MasterPalette> = OnceLock::new();

/// The game master palette, loaded once. Falls back to all-white if the file is
/// missing/malformed so palette-indexed colors degrade to white rather than panic.
pub fn master_palette() -> &'static MasterPalette {
    MASTER_PALETTE.get_or_init(load_master_palette)
}

/// Resolve a palette index to a normalized RGB color in `[0, 1]`.
pub fn index_to_rgb(index: u8) -> Vector3<f32> {
    let [r, g, b] = master_palette()[index as usize];
    vec3(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

fn load_master_palette() -> MasterPalette {
    // Resolved through the data-file mounts, not the filesystem: a 25th
    // Anniversary install has no loose files at all - the palette lives inside
    // `sshock2.kpf` under `data/` - and reading the data root directly there
    // silently fell back to white, which turns every palette-indexed particle
    // color (explosion smoke, sparks, steam) white.
    let Some(mut reader) = data_files::open_data_file(PALETTE_FILE) else {
        warn!("[palette] {PALETTE_FILE} not found in the data root - using white");
        return [[255; 3]; 256];
    };
    let mut bytes = Vec::new();
    if let Err(e) = reader.read_to_end(&mut bytes) {
        warn!("[palette] could not read {PALETTE_FILE}: {e} - using white");
        return [[255; 3]; 256];
    }
    parse_pcx_palette(&bytes).unwrap_or_else(|| {
        warn!("[palette] {PALETTE_FILE} has no 256-color palette trailer; using white");
        [[255; 3]; 256]
    })
}

/// A 256-color PCX stores its palette in the trailing 769 bytes: a `0x0C` marker
/// byte followed by 256 RGB triples.
fn parse_pcx_palette(bytes: &[u8]) -> Option<MasterPalette> {
    if bytes.len() < 769 {
        return None;
    }
    let tail = &bytes[bytes.len() - 769..];
    if tail[0] != 0x0C {
        return None;
    }
    let rgb = &tail[1..];
    let mut pal = [[0u8; 3]; 256];
    for (i, entry) in pal.iter_mut().enumerate() {
        *entry = [rgb[i * 3], rgb[i * 3 + 1], rgb[i * 3 + 2]];
    }
    Some(pal)
}
