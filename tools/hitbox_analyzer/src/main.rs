//! hitbox_analyzer
//!
//! Analyzes per-joint limb collision fit for System Shock 2 creature meshes
//! (LGMM `.bin`) — the data the ragdoll uses to build limb colliders.
//!
//! Mesh verts are stored in *joint-local* space (each joint's verts cluster
//! around that joint's origin; confirmed by comparing vertex-AABB centers to
//! joint world positions). So each joint's collider is the joint-local AABB of
//! the verts weighted to it, placed at the joint. That's geometrically fine, but
//! it can leave the *bone segments between joints* uncovered when few verts are
//! weighted to a mid-limb joint (elbow, knee).
//!
//! This tool reports, per creature:
//!   - each joint's box dims (joint-local AABB), and
//!   - **bone-segment coverage**: for every skeleton bone (parent→child), what
//!     fraction of the segment is inside the union of the joint boxes. Low
//!     coverage = the limb is not enclosed (the "small cubes at joints" problem).

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Seek, SeekFrom};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cgmath::{InnerSpace, Matrix4, SquareMatrix, Vector3, Vector4};
use clap::Parser;
use dark::hit_box::{self, HitBoxShape};
use dark::motion::JointId;
use dark::ss2_bin_ai_loader::{self, SystemShock2AIMesh};
use dark::ss2_bin_header::{self, BinFileType};
use dark::ss2_cal_loader;
use dark::ss2_skeleton::{self, Skeleton};

#[derive(Parser, Debug)]
#[command(
    name = "hitbox_analyzer",
    about = "Analyze per-joint limb collision fit / coverage for SS2 creature meshes"
)]
struct Args {
    /// A single mesh `.bin` file, or a directory to scan for creature meshes.
    #[arg(default_value = "Data/res/mesh")]
    path: String,

    /// Samples per bone segment for the coverage test.
    #[arg(long, default_value_t = 12)]
    samples: usize,
}

/// A joint's collider box, in joint-local space (min/max), with its bind world
/// transform for placement.
struct JointBox {
    min: [f32; 3],
    max: [f32; 3],
    world: Matrix4<f32>,
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
        anyhow::bail!("path not found: {}", args.path);
    };
    if files.is_empty() {
        anyhow::bail!("no .bin files found at {}", args.path);
    }

    let mut creature_count = 0;
    for file in &files {
        match analyze_file(file, args.samples) {
            Ok(true) => creature_count += 1,
            Ok(false) => {}
            Err(e) => eprintln!("  [skip] {}: {e:#}", file.display()),
        }
    }
    println!(
        "\nScanned {} file(s); {} creature(s).",
        files.len(),
        creature_count
    );
    Ok(())
}

fn analyze_file(file: &Path, samples: usize) -> Result<bool> {
    let mesh = match load_creature_mesh(file)? {
        Some(mesh) => mesh,
        None => return Ok(false),
    };
    let skeleton = match load_skeleton(file) {
        Some(s) => s,
        None => {
            eprintln!("  [skip] {}: no .cal skeleton", file.display());
            return Ok(false);
        }
    };
    let world = skeleton.world_transforms();

    // Per-joint joint-local AABB box (collider), keyed by joint id.
    let mut boxes: HashMap<u32, JointBox> = HashMap::new();
    for (joint_id, verts) in mesh.joint_vertex_positions() {
        if verts.is_empty() || (joint_id as usize) >= world.len() {
            continue;
        }
        let (min, max) = aabb_min_max(verts.iter().map(|v| [v.x, v.y, v.z]));
        boxes.insert(
            joint_id,
            JointBox {
                min,
                max,
                world: world[joint_id as usize],
            },
        );
    }

    // Fitted shapes (the shared source of truth: capsule-toward-child / box),
    // paired with each joint's world transform for placement.
    let fitted: HashMap<u32, (HitBoxShape, Matrix4<f32>)> =
        hit_box::fit_hit_box_shapes(&mesh, &skeleton)
            .into_iter()
            .filter(|(j, _)| (*j as usize) < world.len())
            .map(|(j, s)| (j, (s, world[j as usize])))
            .collect();

    let name = file.file_name().unwrap_or_default().to_string_lossy();
    println!("\n=== {name} ===");
    let mut shape_rows: Vec<(u32, String)> = fitted
        .iter()
        .map(|(j, (s, _))| {
            let desc = match s {
                HitBoxShape::Capsule { a, b, radius } => format!(
                    "capsule a=({:.2},{:.2},{:.2}) b=({:.2},{:.2},{:.2}) r={:.3} len={:.2}",
                    a.x,
                    a.y,
                    a.z,
                    b.x,
                    b.y,
                    b.z,
                    radius,
                    (b - a).magnitude()
                ),
                HitBoxShape::Cuboid {
                    half_extents,
                    center,
                } => format!(
                    "cuboid half=({:.2},{:.2},{:.2}) center=({:.2},{:.2},{:.2})",
                    half_extents.x, half_extents.y, half_extents.z, center.x, center.y, center.z
                ),
            };
            (*j, format!("{} {}", joint_label(*j), desc))
        })
        .collect();
    shape_rows.sort_by_key(|(j, _)| *j);
    println!("  fitted shapes:");
    for (_, d) in &shape_rows {
        println!("    {d}");
    }

    // Per-bone segment coverage, comparing the legacy per-joint AABB to the
    // fitted shapes.
    let (mut aabb_tot, mut aabb_cov) = (0usize, 0usize);
    let (mut fit_tot, mut fit_cov) = (0usize, 0usize);
    let mut rows: Vec<(String, f32, f32, f32)> = Vec::new(); // (label, aabb_cov, fit_cov, length)
    for bone in skeleton.bones() {
        let Some(parent) = bone.parent_id else {
            continue;
        };
        let (cw, pw) = (joint_pos(&world, bone.joint_id), joint_pos(&world, parent));
        let len = dist(pw, cw);
        if len < 1e-4 {
            continue;
        }
        let (mut a_b, mut f_b) = (0usize, 0usize);
        for i in 0..=samples {
            let t = i as f32 / samples as f32;
            let p = lerp(pw, cw, t);
            if point_covered_aabb(p, &boxes) {
                a_b += 1;
                aabb_cov += 1;
            }
            if point_covered_shapes(p, &fitted) {
                f_b += 1;
                fit_cov += 1;
            }
            aabb_tot += 1;
            fit_tot += 1;
        }
        let n = (samples + 1) as f32;
        rows.push((
            format!("{}->{}", joint_label(parent), joint_label(bone.joint_id)),
            a_b as f32 / n,
            f_b as f32 / n,
            len,
        ));
    }

    rows.sort_by(|a, b| a.2.total_cmp(&b.2));
    println!("  bone segment coverage    aabb -> fitted   (worst-fitted first):");
    for (label, a, f, len) in &rows {
        println!(
            "    {:<22} {:>4.0}% -> {:>4.0}%   (len {:.2})",
            label,
            a * 100.0,
            f * 100.0,
            len
        );
    }
    let pct = |c: usize, t: usize| {
        if t > 0 {
            c as f32 / t as f32 * 100.0
        } else {
            0.0
        }
    };
    println!(
        "  OVERALL bone coverage: aabb {:.0}% -> fitted {:.0}%  ({} joints)",
        pct(aabb_cov, aabb_tot),
        pct(fit_cov, fit_tot),
        fitted.len()
    );

    // Vertex (surface) coverage: fraction of the mesh's skinned verts inside the
    // union of shapes. Unlike bone-segment coverage this IS sensitive to radius,
    // so it's the guard when tightening capsule radii.
    let (mut v_tot, mut v_in) = (0usize, 0usize);
    for (joint_id, verts) in mesh.joint_vertex_positions() {
        if (joint_id as usize) >= world.len() {
            continue;
        }
        let jw = world[joint_id as usize];
        for v in &verts {
            let w = jw * Vector4::new(v.x, v.y, v.z, 1.0);
            if point_covered_shapes([w.x, w.y, w.z], &fitted) {
                v_in += 1;
            }
            v_tot += 1;
        }
    }
    println!("  vertex (surface) coverage: {:.0}%", pct(v_in, v_tot));

    // Overlap: fraction of each shape's volume that lies inside a NON-adjacent
    // shape (adjacent/jointed pairs are meant to meet at the joint).
    let mut adjacency: HashSet<(u32, u32)> = HashSet::new();
    for bone in skeleton.bones() {
        if let Some(p) = bone.parent_id {
            adjacency.insert((p.min(bone.joint_id), p.max(bone.joint_id)));
        }
    }
    let (mut ov_total, mut ov_hit) = (0usize, 0usize);
    let mut ov_rows: Vec<(String, f32)> = Vec::new();
    for (jid, (shape, world)) in &fitted {
        let samples = sample_shape_points(shape);
        if samples.is_empty() {
            continue;
        }
        let mut overlapped = 0usize;
        for s in &samples {
            let w = world * Vector4::new(s.x, s.y, s.z, 1.0);
            let wp = [w.x, w.y, w.z];
            let in_other = fitted.iter().any(|(ojid, (osh, ow))| {
                if ojid == jid {
                    return false;
                }
                let key = (*jid.min(ojid), *jid.max(ojid));
                if adjacency.contains(&key) {
                    return false;
                }
                point_in_shape_world(wp, osh, ow)
            });
            if in_other {
                overlapped += 1;
            }
        }
        ov_total += samples.len();
        ov_hit += overlapped;
        ov_rows.push((joint_label(*jid), overlapped as f32 / samples.len() as f32));
    }
    ov_rows.sort_by(|a, b| b.1.total_cmp(&a.1));
    println!("  shape overlap into non-adjacent shapes (worst first):");
    for (label, f) in ov_rows.iter().take(8) {
        println!("    {:<22} {:>5.0}%", label, f * 100.0);
    }
    println!("  OVERALL shape overlap: {:.0}%", pct(ov_hit, ov_total));
    Ok(true)
}

/// True if world point `p` is inside any fitted joint shape (tested in the
/// joint's local frame).
fn point_covered_shapes(p: [f32; 3], shapes: &HashMap<u32, (HitBoxShape, Matrix4<f32>)>) -> bool {
    shapes
        .values()
        .any(|(shape, world)| point_in_shape_world(p, shape, world))
}

/// True if world point `p` is inside `shape` placed at `world`.
fn point_in_shape_world(p: [f32; 3], shape: &HitBoxShape, world: &Matrix4<f32>) -> bool {
    let Some(inv) = world.invert() else {
        return false;
    };
    let h = inv * Vector4::new(p[0], p[1], p[2], 1.0);
    shape_contains_local(shape, Vector3::new(h.x, h.y, h.z))
}

fn shape_contains_local(shape: &HitBoxShape, l: Vector3<f32>) -> bool {
    match shape {
        HitBoxShape::Cuboid {
            half_extents,
            center,
        } => {
            let d = l - center;
            d.x.abs() <= half_extents.x
                && d.y.abs() <= half_extents.y
                && d.z.abs() <= half_extents.z
        }
        HitBoxShape::Capsule { a, b, radius } => point_segment_dist(l, *a, *b) <= *radius,
    }
}

/// Sample points inside `shape` (joint-local), for overlap estimation.
fn sample_shape_points(shape: &HitBoxShape) -> Vec<Vector3<f32>> {
    let (min, max) = match shape {
        HitBoxShape::Cuboid {
            half_extents,
            center,
        } => (center - half_extents, center + half_extents),
        HitBoxShape::Capsule { a, b, radius } => {
            let r = Vector3::new(*radius, *radius, *radius);
            (
                Vector3::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z)) - r,
                Vector3::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z)) + r,
            )
        }
    };
    let n = 5;
    let mut pts = Vec::new();
    for i in 0..n {
        for j in 0..n {
            for k in 0..n {
                let t = |idx: usize| (idx as f32 + 0.5) / n as f32;
                let p = Vector3::new(
                    min.x + (max.x - min.x) * t(i),
                    min.y + (max.y - min.y) * t(j),
                    min.z + (max.z - min.z) * t(k),
                );
                if shape_contains_local(shape, p) {
                    pts.push(p);
                }
            }
        }
    }
    pts
}

fn point_segment_dist(p: Vector3<f32>, a: Vector3<f32>, b: Vector3<f32>) -> f32 {
    let ab = b - a;
    let len2 = ab.magnitude2();
    if len2 < 1e-9 {
        return (p - a).magnitude();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (p - (a + ab * t)).magnitude()
}

/// True if world point `p` lies inside any legacy per-joint AABB (in that joint's
/// local frame).
fn point_covered_aabb(p: [f32; 3], boxes: &HashMap<u32, JointBox>) -> bool {
    for b in boxes.values() {
        let Some(inv) = b.world.invert() else {
            continue;
        };
        let h = inv * Vector4::new(p[0], p[1], p[2], 1.0);
        let l = [h.x, h.y, h.z];
        if (0..3).all(|k| l[k] >= b.min[k] && l[k] <= b.max[k]) {
            return true;
        }
    }
    false
}

fn joint_pos(world: &[Matrix4<f32>; 40], joint: JointId) -> [f32; 3] {
    let m = world[joint as usize];
    [m.w.x, m.w.y, m.w.z]
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

fn aabb_min_max(pts: impl Iterator<Item = [f32; 3]>) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in pts {
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    (min, max)
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

fn load_skeleton(mesh_path: &Path) -> Option<Skeleton> {
    for ext in ["cal", "CAL"] {
        let cal = mesh_path.with_extension(ext);
        if !cal.is_file() {
            continue;
        }
        let Ok(f) = File::open(&cal) else { continue };
        let mut reader = BufReader::new(f);
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            ss2_skeleton::create(ss2_cal_loader::read(&mut reader))
        }));
        if let Ok(s) = result {
            return Some(s);
        }
    }
    None
}

fn joint_label(id: JointId) -> String {
    let name = match id {
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
        _ => "",
    };
    if name.is_empty() {
        format!("j{id}")
    } else {
        format!("{id}:{name}")
    }
}
