//! hitbox_analyzer
//!
//! Analyzes per-joint limb collision fit for System Shock 2 creature meshes
//! (LGMM `.bin`). For each skeleton joint it compares the axis-aligned bounding
//! box (AABB) of the joint's skinned vertices - which is what the ragdoll
//! currently uses for limb colliders - against a tight *oriented* bound (OBB,
//! via PCA of the vertex cloud). The ratio (AABB volume / OBB volume) is the
//! "inflation": how much bigger the axis-aligned box is than a bone-aligned one.
//!
//! Per the Dark engine reference, there is no authored per-joint collision data
//! (whole-body collision is a 1-2 sphere column; damage is a mesh raycast), so
//! mesh-derived shapes are the source of truth. This tool quantifies where
//! axis-aligned boxes inflate badly (diagonal limbs) and would benefit from
//! oriented boxes / capsules, and reports recommended tight sizes.

use std::fs::File;
use std::io::{BufReader, Seek, SeekFrom};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cgmath::Point3;
use clap::Parser;
use dark::ss2_bin_ai_loader::{self, SystemShock2AIMesh};
use dark::ss2_bin_header::{self, BinFileType};
use nalgebra::{Matrix3, Vector3};

#[derive(Parser, Debug)]
#[command(
    name = "hitbox_analyzer",
    about = "Analyze per-joint limb collision fit for SS2 creature meshes"
)]
struct Args {
    /// A single mesh `.bin` file, or a directory to scan for creature meshes.
    /// Defaults to the standard mesh folder.
    #[arg(default_value = "Data/res/mesh")]
    path: String,

    /// Only report joints whose inflation is at least this ratio (default 0 =
    /// show all). Inflation is AABB volume / oriented-bound volume; > ~1.3 means
    /// an oriented box/capsule would be noticeably tighter than the current AABB.
    #[arg(long, default_value_t = 0.0)]
    min_inflation: f32,
}

/// Tight AABB and oriented (PCA) bound for one joint's vertex cloud.
struct JointFit {
    joint_id: u32,
    vert_count: usize,
    aabb_dim: [f32; 3],
    aabb_volume: f32,
    obb_extents: [f32; 3], // sorted descending
    obb_volume: f32,
    inflation: f32,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let path = Path::new(&args.path);

    let files: Vec<PathBuf> = if path.is_dir() {
        let mut out = Vec::new();
        for pat in ["*.bin", "*.BIN"] {
            let glob_pat = path.join(pat);
            for entry in glob::glob(&glob_pat.to_string_lossy())?.flatten() {
                out.push(entry);
            }
        }
        out.sort();
        out.dedup();
        out
    } else if path.is_file() {
        vec![path.to_path_buf()]
    } else {
        anyhow::bail!(
            "path not found: {} (run from the repo root, or pass a mesh/dir)",
            args.path
        );
    };

    if files.is_empty() {
        anyhow::bail!("no .bin files found at {}", args.path);
    }

    let mut creature_count = 0;
    for file in &files {
        match analyze_file(file, args.min_inflation) {
            Ok(true) => creature_count += 1,
            Ok(false) => {} // not a creature mesh; skip silently
            Err(e) => eprintln!("  [skip] {}: {e:#}", file.display()),
        }
    }

    println!(
        "\nScanned {} file(s); {} creature mesh(es) analyzed.",
        files.len(),
        creature_count
    );
    Ok(())
}

/// Returns Ok(true) if the file was an analyzed creature mesh, Ok(false) if it
/// was a non-mesh (object) `.bin` that was skipped.
fn analyze_file(file: &Path, min_inflation: f32) -> Result<bool> {
    let mesh = match load_creature_mesh(file)? {
        Some(mesh) => mesh,
        None => return Ok(false),
    };

    let joint_verts = mesh.joint_vertex_positions();
    let captured: usize = joint_verts.values().map(|v| v.len()).sum();
    eprintln!(
        "  [stat] {}: mesh_verts={}, joints(seg)={}, joint_map={}, captured_verts={}, joint_ids={}",
        file.file_name().unwrap_or_default().to_string_lossy(),
        mesh.vertices.len(),
        mesh.joints.len(),
        mesh.joint_map.len(),
        captured,
        joint_verts.len(),
    );
    let mut fits: Vec<JointFit> = joint_verts
        .into_iter()
        .filter(|(_, verts)| !verts.is_empty())
        .map(|(joint_id, verts)| joint_fit(joint_id, &verts))
        .collect();

    // Worst (most inflated) first.
    fits.sort_by(|a, b| b.inflation.total_cmp(&a.inflation));

    let name = file.file_name().unwrap_or_default().to_string_lossy();
    println!("\n=== {name} ===");
    println!(
        "{:<10} {:>6}  {:<22} {:>8}   {:<22} {:>8}   {:>6}",
        "joint", "verts", "aabb dim (x,y,z)", "aabb vol", "obb ext (l,m,s)", "obb vol", "inflate"
    );
    for f in &fits {
        if f.inflation < min_inflation {
            continue;
        }
        println!(
            "{:<10} {:>6}  {:<22} {:>8.4}   {:<22} {:>8.4}   {:>5.2}x",
            format!("{} {}", f.joint_id, joint_name(f.joint_id)),
            f.vert_count,
            fmt3(f.aabb_dim),
            f.aabb_volume,
            fmt3(f.obb_extents),
            f.obb_volume,
            f.inflation,
        );
    }
    Ok(true)
}

fn load_creature_mesh(file: &Path) -> Result<Option<SystemShock2AIMesh>> {
    let f = File::open(file).with_context(|| format!("open {}", file.display()))?;
    let mut reader = BufReader::new(f);
    reader.seek(SeekFrom::Start(0))?;

    // Parsing can panic on malformed/unsupported variants; isolate per file so a
    // directory scan keeps going.
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let common = ss2_bin_header::read(&mut reader);
        match common.bin_type {
            BinFileType::Mesh => Some(ss2_bin_ai_loader::read(&mut reader, &common)),
            BinFileType::Obj => None,
        }
    }));

    match result {
        Ok(mesh) => Ok(mesh),
        Err(_) => anyhow::bail!("failed to parse (panic)"),
    }
}

fn joint_fit(joint_id: u32, verts: &[Point3<f32>]) -> JointFit {
    let pts: Vec<[f32; 3]> = verts.iter().map(|v| [v.x, v.y, v.z]).collect();

    // AABB
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in &pts {
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    let aabb_dim = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    let aabb_volume = aabb_dim[0] * aabb_dim[1] * aabb_dim[2];

    let obb_extents = oriented_extents(&pts);
    let obb_volume = obb_extents[0] * obb_extents[1] * obb_extents[2];
    let inflation = if obb_volume > 1e-9 {
        aabb_volume / obb_volume
    } else {
        1.0
    };

    JointFit {
        joint_id,
        vert_count: pts.len(),
        aabb_dim,
        aabb_volume,
        obb_extents,
        obb_volume,
        inflation,
    }
}

/// Full extents (max-min) of the vertex cloud along its three principal axes
/// (PCA), sorted descending. This is the tight oriented bound a bone-aligned
/// collider could achieve.
fn oriented_extents(pts: &[[f32; 3]]) -> [f32; 3] {
    let n = pts.len();
    if n == 0 {
        return [0.0; 3];
    }
    let mut c = [0.0f32; 3];
    for p in pts {
        for k in 0..3 {
            c[k] += p[k];
        }
    }
    for k in 0..3 {
        c[k] /= n as f32;
    }

    let mut cov = Matrix3::<f32>::zeros();
    for p in pts {
        let d = Vector3::new(p[0] - c[0], p[1] - c[1], p[2] - c[2]);
        cov += d * d.transpose();
    }
    cov /= n as f32;

    let axes = cov.symmetric_eigen().eigenvectors;

    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in pts {
        for k in 0..3 {
            let axis = axes.column(k);
            let proj = (p[0] - c[0]) * axis[0] + (p[1] - c[1]) * axis[1] + (p[2] - c[2]) * axis[2];
            min[k] = min[k].min(proj);
            max[k] = max[k].max(proj);
        }
    }
    let mut ext = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    ext.sort_by(|a, b| b.total_cmp(a));
    ext
}

fn fmt3(v: [f32; 3]) -> String {
    format!("{:.3},{:.3},{:.3}", v[0], v[1], v[2])
}

/// Humanoid skeleton joint names (from HUMANOID_HIT_BOXES in shock2vr's
/// creature_definitions). Other ids fall back to a generic label.
fn joint_name(id: u32) -> &'static str {
    match id {
        2 => "LToe",
        3 => "RToe",
        4 => "LKnee",
        5 => "RKnee",
        6 => "LThigh",
        7 => "RThigh",
        8 => "Neck",
        9 => "Head",
        10 => "LShoulder",
        11 => "RShoulder",
        12 => "LElbow",
        13 => "RElbow",
        14 => "LWeap",
        15 => "RWeap",
        18 => "Abdomen",
        _ => "-",
    }
}
