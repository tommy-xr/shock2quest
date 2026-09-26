//! Scratch tool: report polygon/vertex counts and open (boundary) edges for an
//! LGMD object `.bin`. A closed solid has every edge shared by exactly two
//! polygons; a one-sided "viewmodel" mesh leaves many edges used only once.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Cursor, Read};

fn main() {
    for path in std::env::args().skip(1) {
        let mut buf = Vec::new();
        BufReader::new(File::open(&path).unwrap())
            .read_to_end(&mut buf)
            .unwrap();
        let mut cursor = Cursor::new(buf);
        let header = dark::ss2_bin_header::read(&mut cursor);
        let mesh = dark::ss2_bin_obj_loader::read(&mut cursor, &header);

        // Weld positions so an edge shared across sub-objects still matches.
        let key = |i: u16| {
            let v = mesh.vertices[i as usize];
            (
                (v.x * 4096.0).round() as i64,
                (v.y * 4096.0).round() as i64,
                (v.z * 4096.0).round() as i64,
            )
        };

        let mut edges: HashMap<(_, _), u32> = HashMap::new();
        let mut tris = 0usize;
        for poly in &mesh.polygons {
            let n = poly.vertex_indices.len();
            if n < 3 {
                continue;
            }
            tris += n - 2;
            for i in 0..n {
                let a = key(poly.vertex_indices[i]);
                let b = key(poly.vertex_indices[(i + 1) % n]);
                let e = if a <= b { (a, b) } else { (b, a) };
                *edges.entry(e).or_insert(0) += 1;
            }
        }
        let open = edges.values().filter(|c| **c == 1).count();
        let bb = mesh.bounding_box;
        println!(
            "  bbox min=({:.3},{:.3},{:.3}) max=({:.3},{:.3},{:.3})",
            bb.min.x, bb.min.y, bb.min.z, bb.max.x, bb.max.y, bb.max.z
        );
        for v in &mesh.vhots {
            println!(
                "  vhot {:?} ({:.3},{:.3},{:.3})",
                v.id, v.point.x, v.point.y, v.point.z
            );
        }
        println!(
            "{path}: verts={} polys={} tris={} edges={} open_edges={} ({:.1}%)",
            mesh.vertices.len(),
            mesh.polygons.len(),
            tris,
            edges.len(),
            open,
            100.0 * open as f32 / edges.len().max(1) as f32,
        );
    }
}
