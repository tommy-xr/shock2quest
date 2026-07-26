//! `PMNM` — the high-detail mesh the 25th Anniversary Edition appends to its
//! `mesh/*.bin` files.
//!
//! Every one of the Nightdive layer's 66 skinned meshes is two meshes
//! concatenated: a byte-identical copy of the original LGMM (which is what a
//! stock Dark engine reads), followed by a second chunk tagged `PMNM` carrying
//! roughly 7x the triangles and the upgraded `ND-*.psd` material names. Reading
//! only the first chunk — which is what we did before this — renders every
//! creature and the first-person arms at original quality and leaves the
//! upgraded textures unreachable, because the material name that points at them
//! exists only here.
//!
//! The layout below was reverse-engineered and validated against all 66 chunks;
//! see `projects/25th-anniversary-assets.md` for the evidence. Unlike the
//! original LGMM (joint-local positions, per-joint vertex ranges), a `PMNM`
//! vertex is a modern interleaved skinned vertex in **model space** with four
//! bone indices and four weights.
//!
//! Skeleton binding is solved: a joint pivot's *index* is the `.cal` skeleton's
//! joint id, and the pivots are authored without the 90-degree yaw that
//! `ss2_skeleton::create` puts on every torso bone. See
//! `ss2_bin_ai_loader::pmnm_bind_matrices`, and
//! `tools/asset_probe/src/bin/pmnm_joints.rs` for the residual check.

use cgmath::{Vector2, Vector3, vec2, vec3};
use tracing::trace;

use crate::SCALE_FACTOR;

/// A bone index addresses the renderer's skinning palette, so a chunk claiming
/// more joints than it holds cannot be skinned.
const MAX_JOINTS: usize = engine::scene::MAX_SKINNED_JOINTS;

const MAGIC: &[u8; 4] = b"PMNM";
const HEADER_LEN: usize = 60;
const MATERIAL_STRIDE: usize = 56;
const MATERIAL_NAME_LEN: usize = 16;
const JOINT_STRIDE: usize = 12;
const VERTEX_STRIDE: usize = 40;
const INDEX_STRIDE: usize = 2;
/// Optional morph block strides, kept for bounds-checking even though the data
/// itself is not consumed.
const MORPH_TARGET_STRIDE: usize = 16;
const MORPH_DELTA_STRIDE: usize = 32;

#[derive(Debug, Clone)]
pub struct PmnmVertex {
    pub position: Vector3<f32>,
    pub uv: Vector2<f32>,
    pub normal: Vector3<f32>,
    pub bone_indices: [u8; 4],
    /// Weights as authored, in 0..=255; they sum to exactly 255.
    pub bone_weights: [u8; 4],
}

#[derive(Debug, Clone)]
pub struct PmnmMaterial {
    pub name: String,
    /// This material's slice of the index buffer. Across all 66 shipped chunks
    /// the ranges are contiguous, start at 0, and sum to the index count.
    pub index_start: usize,
    pub index_count: usize,
}

#[derive(Debug, Clone)]
pub struct PmnmMesh {
    pub materials: Vec<PmnmMaterial>,
    /// Joint pivots in model space. Position only - no parent, no name.
    pub joint_pivots: Vec<Vector3<f32>>,
    pub vertices: Vec<PmnmVertex>,
    /// Triangle list into `vertices`.
    pub indices: Vec<u16>,
}

fn u32_at(buf: &[u8], offset: usize) -> Option<u32> {
    let b = buf.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn f32_at(buf: &[u8], offset: usize) -> Option<f32> {
    let b = buf.get(offset..offset + 4)?;
    Some(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Read a Dark-space vector and convert it to engine space.
///
/// Dark stores `(x, z, y)` with x pointing the other way, so the conversion
/// negates x and swaps the last two components — matching
/// `ss2_common::read_vec3` / `read_point3`, which every other Dark reader uses.
/// Skipping this leaves a PMNM mesh rotated 90 degrees against its own skeleton.
fn vec3_at(buf: &[u8], offset: usize) -> Option<Vector3<f32>> {
    let neg_x = f32_at(buf, offset)?;
    let z = f32_at(buf, offset + 4)?;
    let y = f32_at(buf, offset + 8)?;
    Some(vec3(-neg_x, y, z))
}

/// Byte offset of the `PMNM` chunk within a `mesh/*.bin`, if one is appended.
///
/// The marker's position relative to the trailing chunk's own `LGMM` magic
/// varies between files, so it is located by scanning rather than computed.
///
/// `from` should be the end of the original LGMM data. Scanning the whole file
/// instead would search the LGMM vertex and index bytes too, where a chance
/// `50 4D 4E 4D` would shadow the real chunk and silently lose the high-detail
/// mesh. Callers that don't know the boundary can pass 0.
pub fn find_chunk(buf: &[u8], from: usize) -> Option<usize> {
    // Never start at 0: a file that somehow *begins* with the marker is not an
    // appended chunk.
    let start = from.max(4).min(buf.len());
    buf.get(start..)?
        .windows(MAGIC.len())
        .position(|w| w == MAGIC)
        .map(|p| p + start)
}

/// Parse the `PMNM` chunk at `base`. Returns `None` for anything that does not
/// match the validated layout, so a malformed or unexpected chunk degrades to
/// "no high-detail mesh" rather than to garbage geometry.
pub fn read(buf: &[u8], base: usize) -> Option<PmnmMesh> {
    if buf.get(base..base + 4)? != MAGIC {
        return None;
    }

    let num_materials = u32_at(buf, base + 8)? as usize;
    let num_joints = u32_at(buf, base + 12)? as usize;
    let num_vertices = u32_at(buf, base + 16)? as usize;
    let num_indices = u32_at(buf, base + 20)? as usize;
    let morph_vertices = u32_at(buf, base + 24)? as usize;
    let morph_targets = u32_at(buf, base + 28)? as usize;

    let mut offsets = [0usize; 7];
    for (i, slot) in offsets.iter_mut().enumerate() {
        *slot = u32_at(buf, base + 32 + i * 4)? as usize;
    }

    // Bound the chunk against the real buffer BEFORE any stride arithmetic.
    // Every count and offset here comes from the file, so without this a header
    // claiming huge counts either overflows the `usize` products below (a panic
    // in debug builds, which would break this function's "returns None on
    // anything malformed" contract) or reaches a `Vec::with_capacity` sized in
    // gigabytes. Anchoring on the last section's end bounds all of them.
    let chunk_end = base
        .checked_add(offsets[6])?
        .checked_add(INDEX_STRIDE.checked_mul(morph_vertices)?)?;
    if chunk_end > buf.len() {
        trace!(
            "PMNM chunk at {base} claims {chunk_end} bytes, buffer is {}",
            buf.len()
        );
        return None;
    }

    // Structural checks, each of which holds for all 66 shipped chunks. A
    // mismatch means we are not looking at the layout we validated.
    // NB: the ascending-offsets test must precede every subtraction below.
    if offsets[0] != HEADER_LEN
        || num_vertices == 0
        || num_indices == 0
        || num_indices % 3 != 0
        || num_joints > MAX_JOINTS
        || offsets.windows(2).any(|w| w[0] > w[1])
        || offsets[1] - offsets[0] != MATERIAL_STRIDE.checked_mul(num_materials)?
        || offsets[2] - offsets[1] != JOINT_STRIDE.checked_mul(num_joints)?
        || offsets[3] - offsets[2] != VERTEX_STRIDE.checked_mul(num_vertices)?
        || offsets[4] - offsets[3] != MORPH_TARGET_STRIDE.checked_mul(morph_targets)?
        || offsets[5] - offsets[4]
            != MORPH_DELTA_STRIDE
                .checked_mul(morph_targets)?
                .checked_mul(morph_vertices)?
        || offsets[6] - offsets[5] != INDEX_STRIDE.checked_mul(num_indices)?
    {
        trace!("PMNM chunk at {base} does not match the expected layout");
        return None;
    }

    let mut materials = Vec::with_capacity(num_materials);
    for i in 0..num_materials {
        let at = base + offsets[0] + i * MATERIAL_STRIDE;
        let raw = buf.get(at..at + MATERIAL_NAME_LEN)?;
        let end = raw.iter().position(|c| *c == 0).unwrap_or(raw.len());
        let index_start = u32_at(buf, at + 48)? as usize;
        let index_count = u32_at(buf, at + 52)? as usize;
        // A range that escapes the index buffer would slice out of bounds later;
        // a partial triangle would leave GL silently dropping the tail.
        if index_count % 3 != 0 || index_start.saturating_add(index_count) > num_indices {
            trace!(
                "PMNM material {i} index range {index_start}+{index_count} exceeds {num_indices}"
            );
            return None;
        }
        materials.push(PmnmMaterial {
            name: String::from_utf8_lossy(&raw[..end]).into_owned(),
            index_start,
            index_count,
        });
    }

    let mut joint_pivots = Vec::with_capacity(num_joints);
    for i in 0..num_joints {
        joint_pivots.push(vec3_at(buf, base + offsets[1] + i * JOINT_STRIDE)? / SCALE_FACTOR);
    }

    let mut vertices = Vec::with_capacity(num_vertices);
    for i in 0..num_vertices {
        let at = base + offsets[2] + i * VERTEX_STRIDE;
        let position = vec3_at(buf, at)? / SCALE_FACTOR;
        let uv = vec2(f32_at(buf, at + 12)?, f32_at(buf, at + 16)?);
        let normal = vec3_at(buf, at + 20)?;
        let idx = buf.get(at + 32..at + 36)?;
        let wts = buf.get(at + 36..at + 40)?;
        // A bone index goes straight to the shader as an index into the skinning
        // palette. Out of range it would silently address the wrong joint (the
        // palette's upper half mirrors the lower) or read out of bounds, so
        // reject it here - the vanilla LGMM path guards the same thing.
        if idx.iter().any(|b| *b as usize >= num_joints) {
            trace!("PMNM vertex {i} references a bone index beyond {num_joints} joints");
            return None;
        }
        vertices.push(PmnmVertex {
            position,
            uv,
            normal,
            bone_indices: [idx[0], idx[1], idx[2], idx[3]],
            bone_weights: [wts[0], wts[1], wts[2], wts[3]],
        });
    }

    let mut indices = Vec::with_capacity(num_indices);
    for i in 0..num_indices {
        let at = base + offsets[5] + i * INDEX_STRIDE;
        let b = buf.get(at..at + 2)?;
        let v = u16::from_le_bytes([b[0], b[1]]);
        // Never hand the renderer an index it would read past the end of.
        if v as usize >= num_vertices {
            trace!("PMNM index {v} out of range for {num_vertices} vertices");
            return None;
        }
        indices.push(v);
    }

    trace!(
        "PMNM: {} tris, {} verts, {} materials, {} joints",
        num_indices / 3,
        num_vertices,
        num_materials,
        num_joints
    );

    Some(PmnmMesh {
        materials,
        joint_pivots,
        vertices,
        indices,
    })
}

impl PmnmMesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Expand the triangle list into skinned vertex runs.
    ///
    /// A pivot index is the `.cal` skeleton's joint id directly - verified across
    /// all 66 shipped chunks, 52 of them to exactly zero residual, by comparing
    /// each pivot against that joint's rest-pose world position (see
    /// `tools/asset_probe/src/bin/pmnm_joints.rs`). So the bone indices need no
    /// remapping; positions are bind-pose model space, which the caller undoes
    /// with the joint's inverse rest transform.
    pub fn to_skinned_vertices(
        &self,
    ) -> Vec<(
        String,
        Vec<engine::scene::VertexPositionTextureSkinnedNormal>,
    )> {
        self.per_material(|v| {
            // Weights are authored as bytes summing to 255.
            let total = v.bone_weights.iter().map(|w| *w as f32).sum::<f32>();
            let scale = if total > 0.0 { 1.0 / total } else { 0.0 };
            engine::scene::VertexPositionTextureSkinnedNormal {
                position: v.position,
                uv: v.uv,
                normal: v.normal,
                bone_indices: v.bone_indices.map(|b| b as u32),
                bone_weights: v.bone_weights.map(|w| w as f32 * scale),
            }
        })
    }

    /// Each material paired with the vertices of **its own slice** of the index
    /// buffer, mapped through `f`.
    ///
    /// The per-material range comes from the material record (`+48` start, `+52`
    /// count); across all 66 shipped chunks those ranges are contiguous, start at
    /// 0, and sum to the index count. Without this a multi-material creature
    /// draws one material's texture over the whole body.
    fn per_material<T>(&self, f: impl Fn(&PmnmVertex) -> T) -> Vec<(String, Vec<T>)> {
        self.materials
            .iter()
            .filter(|m| m.index_count > 0)
            .map(|m| {
                // `read` range-checks every index, so indexing directly is safe -
                // and silently dropping one would shift the whole triangle list.
                let verts = self.indices[m.index_start..m.index_start + m.index_count]
                    .iter()
                    .map(|i| f(&self.vertices[*i as usize]))
                    .collect();
                (m.name.clone(), verts)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal single-material, single-triangle chunk.
    fn synth() -> Vec<u8> {
        synth_with(1, 1, 3, 3, &[(0, 3)])
    }

    /// Build a chunk with the given counts and per-material index ranges.
    fn synth_with(
        num_materials: usize,
        num_joints: usize,
        num_vertices: usize,
        num_indices: usize,
        ranges: &[(u32, u32)],
    ) -> Vec<u8> {
        assert_eq!(ranges.len(), num_materials);
        let offsets = [
            HEADER_LEN,
            HEADER_LEN + MATERIAL_STRIDE * num_materials,
            HEADER_LEN + MATERIAL_STRIDE * num_materials + JOINT_STRIDE * num_joints,
            HEADER_LEN
                + MATERIAL_STRIDE * num_materials
                + JOINT_STRIDE * num_joints
                + VERTEX_STRIDE * num_vertices,
            HEADER_LEN
                + MATERIAL_STRIDE * num_materials
                + JOINT_STRIDE * num_joints
                + VERTEX_STRIDE * num_vertices,
            HEADER_LEN
                + MATERIAL_STRIDE * num_materials
                + JOINT_STRIDE * num_joints
                + VERTEX_STRIDE * num_vertices,
            HEADER_LEN
                + MATERIAL_STRIDE * num_materials
                + JOINT_STRIDE * num_joints
                + VERTEX_STRIDE * num_vertices
                + INDEX_STRIDE * num_indices,
        ];
        let mut b = Vec::new();
        b.extend_from_slice(b"\0\0\0\0"); // so find_chunk's skip-4 still locates it
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&0u32.to_le_bytes());
        for v in [num_materials, num_joints, num_vertices, num_indices, 0, 0] {
            b.extend_from_slice(&(v as u32).to_le_bytes());
        }
        for o in offsets {
            b.extend_from_slice(&(o as u32).to_le_bytes());
        }
        assert_eq!(b.len(), 4 + HEADER_LEN);
        // materials: 16-byte name, then index_start/index_count at +48/+52
        for (m, (start, count)) in ranges.iter().enumerate() {
            let mut name = format!("ND-test{m}.psd").into_bytes();
            name.resize(MATERIAL_NAME_LEN, 0);
            b.extend_from_slice(&name);
            let mut rest = vec![0u8; MATERIAL_STRIDE - MATERIAL_NAME_LEN];
            rest[48 - MATERIAL_NAME_LEN..52 - MATERIAL_NAME_LEN]
                .copy_from_slice(&start.to_le_bytes());
            rest[52 - MATERIAL_NAME_LEN..56 - MATERIAL_NAME_LEN]
                .copy_from_slice(&count.to_le_bytes());
            b.extend_from_slice(&rest);
        }
        // joint pivots
        for _ in 0..num_joints {
            for f in [1.0f32, 2.0, 3.0] {
                b.extend_from_slice(&f.to_le_bytes());
            }
        }
        // vertices
        for i in 0..num_vertices {
            for f in [i as f32, 0.0, 0.0] {
                b.extend_from_slice(&f.to_le_bytes());
            }
            for f in [0.25f32, 0.75] {
                b.extend_from_slice(&f.to_le_bytes());
            }
            for f in [0.0f32, 1.0, 0.0] {
                b.extend_from_slice(&f.to_le_bytes());
            }
            b.extend_from_slice(&[0, 0, 0, 0]);
            b.extend_from_slice(&[255, 0, 0, 0]);
        }
        for i in 0..num_indices {
            b.extend_from_slice(&((i % num_vertices) as u16).to_le_bytes());
        }
        b
    }

    #[test]
    fn finds_and_parses_a_synthetic_chunk() {
        let buf = synth();
        let base = find_chunk(&buf, 0).expect("marker should be found");
        let mesh = read(&buf, base).expect("should parse");
        assert_eq!(mesh.materials.len(), 1);
        assert_eq!(mesh.materials[0].name, "ND-test0.psd");
        assert_eq!(mesh.vertices.len(), 3);
        assert_eq!(mesh.triangle_count(), 1);
        assert_eq!(mesh.joint_pivots.len(), 1);
    }

    #[test]
    fn positions_are_scaled_and_converted_to_engine_axes() {
        let buf = synth();
        let mesh = read(&buf, find_chunk(&buf, 0).unwrap()).unwrap();
        // Vertex 1 is authored at Dark (1, 0, 0); the conversion negates x and
        // swaps the last two components, then scales.
        assert_eq!(
            mesh.vertices[1].position,
            vec3(-1.0, 0.0, 0.0) / SCALE_FACTOR
        );
        // The joint pivot is authored at Dark (1, 2, 3) -> engine (-1, 3, 2).
        assert_eq!(mesh.joint_pivots[0], vec3(-1.0, 3.0, 2.0) / SCALE_FACTOR);
        // Normals take the same axis conversion but are NOT scaled: Dark
        // (0, 1, 0) -> engine (0, 0, 1).
        assert_eq!(mesh.vertices[0].normal, vec3(0.0, 0.0, 1.0));
        // UVs are untouched.
        assert_eq!(mesh.vertices[0].uv, vec2(0.25, 0.75));
    }

    #[test]
    fn weights_and_indices_survive_verbatim() {
        let buf = synth();
        let mesh = read(&buf, find_chunk(&buf, 0).unwrap()).unwrap();
        assert_eq!(mesh.vertices[0].bone_weights, [255, 0, 0, 0]);
        assert_eq!(mesh.vertices[0].bone_indices, [0, 0, 0, 0]);
        assert_eq!(mesh.indices, vec![0, 1, 2]);
    }

    #[test]
    fn rejects_a_layout_that_does_not_match() {
        let mut buf = synth();
        // Claim one more vertex than the section can hold.
        let base = find_chunk(&buf, 0).unwrap();
        let bad = 4u32.to_le_bytes();
        buf[base + 16..base + 20].copy_from_slice(&bad);
        assert!(read(&buf, base).is_none());
    }

    #[test]
    fn rejects_an_out_of_range_index() {
        let mut buf = synth();
        let base = find_chunk(&buf, 0).unwrap();
        let idx_at = base + HEADER_LEN + MATERIAL_STRIDE + JOINT_STRIDE + VERTEX_STRIDE * 3;
        buf[idx_at..idx_at + 2].copy_from_slice(&99u16.to_le_bytes());
        assert!(read(&buf, base).is_none());
    }

    /// Two materials each owning half of a six-index buffer.
    fn synth_two_materials() -> Vec<u8> {
        // 6 indices over 3 vertices: `synth_with` writes them as i % num_vertices,
        // so material 0 owns [0,1,2] and material 1 owns [0,1,2] again - which is
        // not distinguishable. Rewrite material 1's slice to start at vertex 1.
        let mut b = synth_with(2, 1, 3, 6, &[(0, 3), (3, 3)]);
        let idx_at = 4 + HEADER_LEN + MATERIAL_STRIDE * 2 + JOINT_STRIDE + VERTEX_STRIDE * 3;
        for (k, v) in [0u16, 1, 2, 1, 2, 1].iter().enumerate() {
            b[idx_at + k * 2..idx_at + k * 2 + 2].copy_from_slice(&v.to_le_bytes());
        }
        b
    }

    #[test]
    fn each_material_gets_only_its_own_index_range() {
        let buf = synth_two_materials();
        let mesh = read(&buf, find_chunk(&buf, 0).unwrap()).expect("should parse");
        assert_eq!(mesh.materials.len(), 2);
        assert_eq!(
            (mesh.materials[0].index_start, mesh.materials[0].index_count),
            (0, 3)
        );
        assert_eq!(
            (mesh.materials[1].index_start, mesh.materials[1].index_count),
            (3, 3)
        );
        let runs = mesh.to_skinned_vertices();
        assert_eq!(runs.len(), 2, "one run per material");
        assert_eq!(runs[0].1.len(), 3);
        assert_eq!(runs[1].1.len(), 3);
        // The two ranges cover different triangles, so the runs differ.
        assert_ne!(
            runs[0].1[0].position, runs[1].1[0].position,
            "the second material must draw its own slice, not the first's"
        );
    }

    #[test]
    fn rejects_a_material_range_past_the_index_buffer() {
        // Claim 99 indices when only 6 exist.
        let buf = synth_with(2, 1, 3, 6, &[(0, 3), (3, 99)]);
        assert!(read(&buf, find_chunk(&buf, 0).unwrap()).is_none());
    }

    #[test]
    fn absent_marker_yields_none() {
        assert!(find_chunk(&[0u8; 64], 0).is_none());
        assert!(find_chunk(&[], 0).is_none());
    }

    #[test]
    fn skinned_expansion_normalizes_weights_and_keeps_bone_indices() {
        let buf = synth();
        let mesh = read(&buf, find_chunk(&buf, 0).unwrap()).unwrap();
        let runs = mesh.to_skinned_vertices();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].0, "ND-test0.psd");
        assert_eq!(runs[0].1.len(), 3);
        let v = &runs[0].1[0];
        // Authored weights are bytes summing to 255; the renderer wants 0..1.
        assert_eq!(v.bone_weights, [1.0, 0.0, 0.0, 0.0]);
        // A pivot index IS the skeleton joint id, so indices pass through as-is.
        assert_eq!(v.bone_indices, [0, 0, 0, 0]);
    }

    /// A two-bone blend must come out summing to 1.0, not 255.
    #[test]
    fn skinned_expansion_handles_a_blended_vertex() {
        let mut buf = synth();
        let base = find_chunk(&buf, 0).unwrap();
        let v0 = base + HEADER_LEN + MATERIAL_STRIDE + JOINT_STRIDE;
        buf[v0 + 32..v0 + 36].copy_from_slice(&[0, 0, 0, 0]);
        buf[v0 + 36..v0 + 40].copy_from_slice(&[193, 62, 0, 0]);
        let mesh = read(&buf, find_chunk(&buf, 0).unwrap()).unwrap();
        let v = &mesh.to_skinned_vertices()[0].1[0];
        let sum: f32 = v.bone_weights.iter().sum();
        assert!(
            (sum - 1.0).abs() < 1e-6,
            "weights should sum to 1, got {sum}"
        );
        assert!((v.bone_weights[0] - 193.0 / 255.0).abs() < 1e-6);
    }

    /// A header claiming counts far larger than the file must return `None`, not
    /// panic on `usize` overflow or try to allocate gigabytes.
    #[test]
    fn rejects_a_header_whose_counts_exceed_the_buffer() {
        let mut buf = synth();
        let base = find_chunk(&buf, 0).unwrap();
        // num_vertices large enough that `40 * n` alone would size a huge Vec.
        buf[base + 16..base + 20].copy_from_slice(&107_000_000u32.to_le_bytes());
        assert!(read(&buf, base).is_none());

        // Morph counts big enough to overflow `32 * targets * vertices`.
        let mut buf = synth();
        buf[base + 24..base + 28].copy_from_slice(&u32::MAX.to_le_bytes());
        buf[base + 28..base + 32].copy_from_slice(&268_435_446u32.to_le_bytes());
        assert!(read(&buf, base).is_none());
    }

    #[test]
    fn rejects_a_bone_index_beyond_the_joint_count() {
        let mut buf = synth();
        let base = find_chunk(&buf, 0).unwrap();
        let v0 = base + HEADER_LEN + MATERIAL_STRIDE + JOINT_STRIDE;
        // synth has 1 joint, so index 5 is out of range.
        buf[v0 + 32] = 5;
        assert!(read(&buf, base).is_none());
    }
}
