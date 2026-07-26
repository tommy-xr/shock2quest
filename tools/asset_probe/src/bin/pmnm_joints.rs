//! Spike tool: try to map a `PMNM` chunk's joint pivots onto the `.cal` skeleton
//! that the motion system animates.
//!
//! A `PMNM` joint record is only a position — no parent, no name — and a chunk's
//! joint count differs from the original mesh's, so the correspondence has to be
//! recovered. If the pivots are the skeleton's rest-pose joint positions, then
//! nearest-joint matching should be near-exact and injective, and the residuals
//! tell us so. That is the hypothesis this tool tests.
//!
//!   pmnm_joints <mesh.bin> <skeleton.cal>

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};

use cgmath::{Matrix4, Vector3, vec3};

fn joint_world_position(m: &Matrix4<f32>) -> Vector3<f32> {
    vec3(m.w.x, m.w.y, m.w.z)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mesh_path = args
        .next()
        .expect("usage: pmnm_joints <mesh.bin> <skel.cal>");
    let cal_path = args
        .next()
        .expect("usage: pmnm_joints <mesh.bin> <skel.cal>");

    let mut buf = Vec::new();
    File::open(&mesh_path)
        .expect("mesh")
        .read_to_end(&mut buf)
        .expect("read mesh");

    let base = match dark::ss2_bin_pmnm::find_chunk(&buf, 0) {
        Some(b) => b,
        None => {
            println!("{mesh_path}: no PMNM chunk");
            return;
        }
    };
    let pmnm = match dark::ss2_bin_pmnm::read(&buf, base) {
        Some(m) => m,
        None => {
            println!("{mesh_path}: PMNM chunk failed to parse");
            return;
        }
    };

    let cal = dark::ss2_cal_loader::read(&mut BufReader::new(File::open(&cal_path).expect("cal")));
    let skeleton = dark::ss2_skeleton::create(cal);

    // Rest-pose world position of every joint the skeleton knows about.
    let mut skel: Vec<(u32, Vector3<f32>)> = skeleton
        .bones()
        .iter()
        .map(|b| {
            (
                b.joint_id,
                joint_world_position(&skeleton.global_transform(&b.joint_id)),
            )
        })
        .collect();
    skel.sort_by_key(|(id, _)| *id);
    skel.dedup_by_key(|(id, _)| *id);

    println!("mesh     : {mesh_path}");
    println!("cal      : {cal_path}");
    println!(
        "PMNM     : {} pivots, {} verts, {} tris",
        pmnm.joint_pivots.len(),
        pmnm.vertices.len(),
        pmnm.triangle_count()
    );
    println!("skeleton : {} joints", skel.len());

    // Which PMNM pivot indices the mesh actually skins to.
    let mut used: HashMap<u8, usize> = HashMap::new();
    for v in &pmnm.vertices {
        for (i, w) in v.bone_indices.iter().zip(v.bone_weights.iter()) {
            if *w > 0 {
                *used.entry(*i).or_default() += 1;
            }
        }
    }
    let mut used_idx: Vec<_> = used.keys().copied().collect();
    used_idx.sort();
    println!(
        "pivots referenced by weights: {} of {} -> {used_idx:?}",
        used_idx.len(),
        pmnm.joint_pivots.len()
    );

    // Hypothesis: pivot index i IS skeleton joint id i, and the skeleton's rest
    // pose carries an extra 90-degree yaw (see `ss2_skeleton::create`, which puts
    // `from_angle_y(Deg(90))` on the root torso bone) that the PMNM pivots do not.
    // Under a +90 yaw about Y, (x, y, z) -> (-z, y, x).
    {
        let by_id: HashMap<u32, Vector3<f32>> = skel.iter().copied().collect();
        let mut res = Vec::new();
        let mut missing = 0;
        for (i, p) in pmnm.joint_pivots.iter().enumerate() {
            match by_id.get(&(i as u32)) {
                Some(sp) => {
                    let rotated = vec3(-sp.z, sp.y, sp.x);
                    let d = (rotated - p).map(|c| c * c);
                    res.push((d.x + d.y + d.z).sqrt());
                }
                None => missing += 1,
            }
        }
        res.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "\nIDENTITY mapping (pivot i = joint i) with a -90 deg yaw applied to the skeleton:"
        );
        if res.is_empty() {
            println!("  no overlapping joint ids");
        } else {
            println!(
                "  residuals over {} pivots ({} had no matching joint id): min={:.5} median={:.5} mean={:.5} max={:.5}",
                res.len(),
                missing,
                res[0],
                res[res.len() / 2],
                res.iter().sum::<f32>() / res.len() as f32,
                res[res.len() - 1]
            );
            // Per-pivot, so an outlier can be weighed against whether any vertex
            // actually skins to that joint - an unused pivot being off is cosmetic.
            for (i, p) in pmnm.joint_pivots.iter().enumerate() {
                if let Some(sp) = by_id.get(&(i as u32)) {
                    let rotated = vec3(-sp.z, sp.y, sp.x);
                    let d = (rotated - p).map(|c| c * c);
                    let dist = (d.x + d.y + d.z).sqrt();
                    if dist > 0.001 {
                        println!(
                            "    pivot {i:>2}: residual {dist:.4}   vertices skinned to it: {}",
                            used.get(&(i as u8)).copied().unwrap_or(0)
                        );
                    }
                }
            }
            println!(
                "  verdict: {}",
                if res[res.len() - 1] < 0.02 {
                    "MATCH - identity index mapping plus a 90 deg yaw explains the pivots"
                } else {
                    "still not exact"
                }
            );
        }
    }

    println!("\nnearest-joint match per PMNM pivot (residual = distance in world units):");
    println!(
        "{:>5}  {:>26}  {:>7}  {:>26}  {:>9}  {:>6}",
        "pivot", "pmnm position", "joint", "skeleton position", "residual", "verts"
    );
    let mut assigned: HashMap<u32, usize> = HashMap::new();
    let mut residuals = Vec::new();
    for (i, p) in pmnm.joint_pivots.iter().enumerate() {
        let best = skel
            .iter()
            .map(|(id, sp)| {
                let d = (sp - p).map(|c| c * c);
                (*id, *sp, (d.x + d.y + d.z).sqrt())
            })
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap());
        match best {
            Some((id, sp, dist)) => {
                *assigned.entry(id).or_default() += 1;
                residuals.push(dist);
                let n = used.get(&(i as u8)).copied().unwrap_or(0);
                println!(
                    "{:>5}  {:>26}  {:>7}  {:>26}  {:>9.4}  {:>6}",
                    i,
                    format!("({:.3}, {:.3}, {:.3})", p.x, p.y, p.z),
                    id,
                    format!("({:.3}, {:.3}, {:.3})", sp.x, sp.y, sp.z),
                    dist,
                    n
                );
            }
            None => println!("{i:>5}  (no skeleton joints)"),
        }
    }

    if !residuals.is_empty() {
        let mut sorted = residuals.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let sum: f32 = residuals.iter().sum();
        println!(
            "\nresiduals: min={:.4} median={:.4} mean={:.4} max={:.4}",
            sorted[0],
            sorted[sorted.len() / 2],
            sum / residuals.len() as f32,
            sorted[sorted.len() - 1]
        );
        let collisions: Vec<_> = assigned.iter().filter(|(_, n)| **n > 1).collect();
        println!(
            "injective: {}  ({} skeleton joints claimed by more than one pivot)",
            collisions.is_empty(),
            collisions.len()
        );
        println!(
            "verdict: {}",
            if sorted[sorted.len() - 1] < 0.05 && collisions.is_empty() {
                "pivots ARE the skeleton rest positions - mapping recovered"
            } else {
                "NOT a clean match - pivots are not simply the rest-pose joint positions"
            }
        );
    }
}
