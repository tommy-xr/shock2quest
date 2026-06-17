//! hitbox_analyzer
//!
//! Analyzes per-joint limb collision fit for System Shock 2 creature meshes
//! (LGMM `.bin`) and recommends tighter collider sizes for the ragdoll.
//!
//! The ragdoll's limb colliders live in each joint's *local* frame (the body is
//! oriented by the joint), so the box that actually matters is the bounding box
//! of the joint's skinned vertices expressed in **joint-local space**, not model
//! space. This tool loads the mesh + its `.cal` skeleton (bind pose), transforms
//! each joint's vertices by the inverse of that joint's bind world transform, and
//! reports:
//!   - the current model-space (axis-aligned) AABB the ragdoll uses today,
//!   - the recommended joint-local AABB (a bone-aligned box) + its center offset,
//!   - the "inflation" (model vol / local vol) = how much the axis-aligned box
//!     over-sizes vs a bone-aligned one,
//!   - a recommended capsule (axis / radius / half-height) from the local box.
//!
//! Per the Dark engine reference there is no authored per-joint collision data
//! (whole-body collision is a 1-2 sphere column; damage is a mesh raycast), so
//! these mesh-derived shapes are the source of truth.

use std::fs::File;
use std::io::{BufReader, Seek, SeekFrom};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cgmath::{Matrix4, Point3, SquareMatrix};
use clap::Parser;
use dark::ss2_bin_ai_loader::{self, SystemShock2AIMesh};
use dark::ss2_bin_header::{self, BinFileType};
use dark::{ss2_cal_loader, ss2_skeleton};

#[derive(Parser, Debug)]
#[command(
    name = "hitbox_analyzer",
    about = "Analyze per-joint limb collision fit for SS2 creature meshes"
)]
struct Args {
    /// A single mesh `.bin` file, or a directory to scan for creature meshes.
    #[arg(default_value = "Data/res/mesh")]
    path: String,

    /// Only report joints whose inflation (model AABB vol / joint-local AABB vol)
    /// is at least this ratio. Default 0 = show all; > ~1.3 means a bone-aligned
    /// box/capsule would be noticeably tighter than the current axis-aligned box.
    #[arg(long, default_value_t = 0.0)]
    min_inflation: f32,
}

struct JointFit {
    joint_id: u32,
    vert_count: usize,
    model_dim: [f32; 3],
    local: Option<LocalFit>,
}

/// Joint-local (bone-aligned) bound + recommended capsule.
struct LocalFit {
    dim: [f32; 3],
    center: [f32; 3],
    inflation: f32,
    capsule_axis: usize,
    capsule_radius: f32,
    capsule_half_height: f32,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let path = Path::new(&args.path);

    let files: Vec<PathBuf> = if path.is_dir() {
        let mut out = Vec::new();
        for pat in ["*.bin", "*.BIN"] {
            for entry in glob::glob(&path.join(pat).to_string_lossy())?.flatten() {
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
            Ok(false) => {}
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

fn analyze_file(file: &Path, min_inflation: f32) -> Result<bool> {
    let mesh = match load_creature_mesh(file)? {
        Some(mesh) => mesh,
        None => return Ok(false),
    };
    let world = load_world_transforms(file);

    let joint_verts = mesh.joint_vertex_positions();
    let mut fits: Vec<JointFit> = joint_verts
        .into_iter()
        .filter(|(_, verts)| !verts.is_empty())
        .map(|(joint_id, verts)| joint_fit(joint_id, &verts, world.as_ref()))
        .collect();

    // Worst (most inflated) first; jointless fits (no skeleton) sort last.
    fits.sort_by(|a, b| {
        let ia = a.local.as_ref().map(|l| l.inflation).unwrap_or(0.0);
        let ib = b.local.as_ref().map(|l| l.inflation).unwrap_or(0.0);
        ib.total_cmp(&ia)
    });

    let name = file.file_name().unwrap_or_default().to_string_lossy();
    let skel = if world.is_some() {
        ""
    } else {
        "  (no .cal - model-space only)"
    };
    println!("\n=== {name} ==={skel}");
    println!(
        "{:<11} {:>5}  {:<18} {:<18} {:<18} {:>5}  {}",
        "joint",
        "verts",
        "model dim",
        "local dim (rec)",
        "local center",
        "infl",
        "capsule (axis r h)"
    );
    for f in &fits {
        if let Some(l) = &f.local {
            if l.inflation < min_inflation {
                continue;
            }
            println!(
                "{:<11} {:>5}  {:<18} {:<18} {:<18} {:>4.2}x  {} r={:.3} h={:.3}",
                format!("{} {}", f.joint_id, joint_name(f.joint_id)),
                f.vert_count,
                fmt3(f.model_dim),
                fmt3(l.dim),
                fmt3(l.center),
                l.inflation,
                axis_name(l.capsule_axis),
                l.capsule_radius,
                l.capsule_half_height,
            );
        } else {
            println!(
                "{:<11} {:>5}  {:<18} {:<18} {:<18} {:>5}",
                format!("{} {}", f.joint_id, joint_name(f.joint_id)),
                f.vert_count,
                fmt3(f.model_dim),
                "-",
                "-",
                "-",
            );
        }
    }
    Ok(true)
}

fn joint_fit(joint_id: u32, verts: &[Point3<f32>], world: Option<&[Matrix4<f32>; 40]>) -> JointFit {
    let (model_dim, model_vol) = aabb(verts.iter().map(|v| [v.x, v.y, v.z]));

    let local = world.and_then(|wt| {
        let m = wt.get(joint_id as usize)?;
        let inv = m.invert()?;
        let local_pts: Vec<[f32; 3]> = verts
            .iter()
            .map(|v| {
                let h = inv * v.to_homogeneous();
                [h.x, h.y, h.z]
            })
            .collect();
        let (dim, vol) = aabb(local_pts.iter().copied());
        let center = aabb_center(local_pts.iter().copied());
        let inflation = if vol > 1e-9 { model_vol / vol } else { 1.0 };
        let (capsule_axis, capsule_radius, capsule_half_height) = capsule_from_dim(dim);
        Some(LocalFit {
            dim,
            center,
            inflation,
            capsule_axis,
            capsule_radius,
            capsule_half_height,
        })
    });

    JointFit {
        joint_id,
        vert_count: verts.len(),
        model_dim,
        local,
    }
}

/// Recommend a capsule from box dims: longest axis is the capsule axis, radius
/// encloses the larger cross dimension, cylinder half-height is the remainder.
fn capsule_from_dim(dim: [f32; 3]) -> (usize, f32, f32) {
    let axis = (0..3).max_by(|&a, &b| dim[a].total_cmp(&dim[b])).unwrap();
    let radius = 0.5
        * (0..3)
            .filter(|&k| k != axis)
            .map(|k| dim[k])
            .fold(0.0f32, f32::max);
    let half_height = (dim[axis] * 0.5 - radius).max(0.0);
    (axis, radius, half_height)
}

fn aabb(pts: impl Iterator<Item = [f32; 3]>) -> ([f32; 3], f32) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    let mut any = false;
    for p in pts {
        any = true;
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    if !any {
        return ([0.0; 3], 0.0);
    }
    let dim = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    (dim, dim[0] * dim[1] * dim[2])
}

fn aabb_center(pts: impl Iterator<Item = [f32; 3]>) -> [f32; 3] {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in pts {
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ]
}

fn load_creature_mesh(file: &Path) -> Result<Option<SystemShock2AIMesh>> {
    let f = File::open(file).with_context(|| format!("open {}", file.display()))?;
    let mut reader = BufReader::new(f);
    reader.seek(SeekFrom::Start(0))?;

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

/// Load the bind-pose per-joint world transforms from the mesh's `.cal` skeleton
/// (same stem). Returns None if no `.cal` is found / it fails to parse.
fn load_world_transforms(mesh_path: &Path) -> Option<[Matrix4<f32>; 40]> {
    for ext in ["cal", "CAL"] {
        let cal = mesh_path.with_extension(ext);
        if !cal.is_file() {
            continue;
        }
        let Ok(f) = File::open(&cal) else { continue };
        let mut reader = BufReader::new(f);
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            let cal = ss2_cal_loader::read(&mut reader);
            ss2_skeleton::create(cal).world_transforms()
        }));
        if let Ok(wt) = result {
            return Some(wt);
        }
    }
    None
}

fn fmt3(v: [f32; 3]) -> String {
    format!("{:.2},{:.2},{:.2}", v[0], v[1], v[2])
}

fn axis_name(axis: usize) -> &'static str {
    match axis {
        0 => "X",
        1 => "Y",
        2 => "Z",
        _ => "?",
    }
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
