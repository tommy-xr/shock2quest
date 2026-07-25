//! Spike tool: run the real `dark` / `engine` parsers over a directory of game
//! assets and report which ones load. Built to answer "are the 25th Anniversary
//! Edition asset overrides readable by our existing importers?" without booting
//! a game session.
//!
//! Each file is parsed inside `catch_unwind` because the Dark format readers
//! signal malformed input by panicking (`assert!` / `unwrap`), and a probe needs
//! to survive that and keep going.

use std::io::Cursor;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;

use clap::Parser;
use walkdir::WalkDir;

#[derive(Parser)]
#[command(about = "Parse a tree of Dark Engine assets and report load results")]
struct Args {
    /// Directory to walk
    dir: String,
    /// Only probe files whose path contains this substring
    #[arg(long)]
    filter: Option<String>,
    /// Print one line per failing file
    #[arg(long)]
    verbose: bool,
}

#[derive(Default)]
struct Tally {
    ok: usize,
    failed: usize,
    failures: Vec<(String, String)>,
}

impl Tally {
    fn record(&mut self, name: &str, res: Result<(), String>) {
        match res {
            Ok(()) => self.ok += 1,
            Err(e) => {
                self.failed += 1;
                if self.failures.len() < 2000 {
                    self.failures.push((name.to_owned(), e));
                }
            }
        }
    }
}

/// Run `f`, converting a panic into an `Err` with the panic message.
fn guard<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    let prev = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let res = panic::catch_unwind(AssertUnwindSafe(f));
    panic::set_hook(prev);
    res.map_err(|e| {
        e.downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "panic".to_owned())
    })
}

/// Static object model (LGMD) or skinned mesh (LGMM).
fn probe_bin(buf: &[u8]) -> Result<String, String> {
    guard(|| {
        let mut c = Cursor::new(buf);
        let header = dark::ss2_bin_header::read(&mut c);
        let version = header.version;
        match header.bin_type {
            dark::ss2_bin_header::BinFileType::Obj => {
                let m = dark::ss2_bin_obj_loader::read(&mut c, &header);
                // A misaligned read still "succeeds" because nothing bounds-checks
                // the indices, so validate them explicitly: garbage geometry shows
                // up as indices pointing past the vertex/normal/uv arrays.
                let (nv, nn, nu) = (m.vertices.len(), m.normals.len(), m.uvs.len());
                let bad = m
                    .polygons
                    .iter()
                    .filter(|p| {
                        p.vertex_indices.iter().any(|&i| i as usize >= nv)
                            || p.normal_indices.iter().any(|&i| i as usize >= nn)
                            || p.uv_indices.iter().any(|&i| i as usize >= nu)
                    })
                    .count();
                if bad > 0 {
                    panic!(
                        "out-of-range indices in {bad}/{} polys (v{version})",
                        m.polygons.len()
                    );
                }
                format!(
                    "LGMD v{version} ({} polys, {nv} verts) indices ok",
                    m.polygons.len()
                )
            }
            dark::ss2_bin_header::BinFileType::Mesh => {
                dark::ss2_bin_ai_loader::read(&mut c, &header);
                // 25AE meshes append a high-detail PMNM chunk; report it so a
                // parse regression there shows up as a probe failure.
                match dark::ss2_bin_pmnm::find_chunk(buf, 0) {
                    Some(base) => match dark::ss2_bin_pmnm::read(buf, base) {
                        Some(m) => format!(
                            "LGMM v{version} + PMNM ({} tris, {} verts, {} mats, {} joints)",
                            m.triangle_count(),
                            m.vertices.len(),
                            m.materials.len(),
                            m.joint_pivots.len()
                        ),
                        None => panic!("PMNM chunk present at {base} but failed to parse"),
                    },
                    None => format!("LGMM v{version}"),
                }
            }
        }
    })
}

fn probe_texture(name: &str, buf: &[u8]) -> Result<String, String> {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let Some(format) = engine::texture_format::extension_to_format(ext.clone()) else {
        return Err(format!("no importer for .{ext}"));
    };
    guard(|| {
        let t = format.load(buf);
        format!("{}x{}", t.width, t.height)
    })
}

fn probe_motion(buf: &[u8]) -> Result<String, String> {
    guard(|| {
        let mut c = Cursor::new(buf);
        let m = dark::motion::MotionInfo::read(&mut c);
        format!("{m:?}").chars().take(40).collect::<String>()
    })
}

fn main() {
    let args = Args::parse();
    let mut models = Tally::default();
    let mut textures = Tally::default();
    let mut motions = Tally::default();
    let mut skipped: usize = 0;

    // Walk/read errors are reported rather than skipped: silently dropping them
    // would let the probe claim a layer is 100% clean while never having looked
    // at part of it.
    let mut access_errors: Vec<String> = Vec::new();

    for entry in WalkDir::new(&args.dir) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                access_errors.push(format!("walk: {e}"));
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let disp = path
            .strip_prefix(&args.dir)
            .unwrap_or(path)
            .display()
            .to_string();
        if let Some(f) = &args.filter {
            if !disp.contains(f) {
                continue;
            }
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let buf = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                access_errors.push(format!("read {disp}: {e}"));
                continue;
            }
        };

        match ext.as_str() {
            "bin" => {
                // Model .bin files are identified by magic; the 2D widget-rect
                // .bin files share the extension, so skip anything else.
                if buf.len() < 4 || (&buf[0..4] != b"LGMD" && &buf[0..4] != b"LGMM") {
                    skipped += 1;
                    continue;
                }
                let res = probe_bin(&buf);
                if args.verbose {
                    match &res {
                        Ok(d) => println!("  ok   {disp:44} {d}"),
                        Err(e) => println!("  FAIL {disp:44} {e}"),
                    }
                }
                models.record(&disp, res.map(|_| ()));
            }
            "dds" | "png" | "pcx" | "gif" | "tga" | "jpg" | "jpeg" => {
                textures.record(&disp, probe_texture(&disp, &buf).map(|_| ()));
            }
            // Only `.mi` (motion info) is parseable standalone. A `.mc` clip is
            // decoded against an `MpsMotion` entry from motiondb.bin, so it has
            // no meaningful standalone probe - it is covered by the e2e run.
            "mi" => {
                motions.record(&disp, probe_motion(&buf).map(|_| ()));
            }
            _ => skipped += 1,
        }
    }

    for (label, t) in [
        ("models (LGMD/LGMM)", &models),
        ("textures", &textures),
        ("motions", &motions),
    ] {
        let total = t.ok + t.failed;
        if total == 0 {
            continue;
        }
        println!(
            "{label:22} {:5} ok / {:5} failed  ({:.0}% ok)",
            t.ok,
            t.failed,
            100.0 * t.ok as f64 / total as f64
        );
    }
    println!("{:22} {skipped:5} (unhandled extensions)", "skipped");
    if !access_errors.is_empty() {
        println!(
            "{:22} {:5} (files that could not be read - results are INCOMPLETE)",
            "access errors",
            access_errors.len()
        );
        for e in access_errors.iter().take(10) {
            println!("           {e}");
        }
    }

    for (label, t) in [
        ("models", &models),
        ("textures", &textures),
        ("motions", &motions),
    ] {
        if t.failures.is_empty() {
            continue;
        }
        // Group failures by message so the shape of the problem is obvious.
        let mut by_msg: std::collections::BTreeMap<&str, Vec<&String>> = Default::default();
        for (n, m) in &t.failures {
            by_msg.entry(m.as_str()).or_default().push(n);
        }
        println!("\n--- {label} failures by cause ---");
        for (msg, names) in by_msg {
            println!("  [{:4}] {}", names.len(), msg);
            let show = if args.verbose { names.len() } else { 3 };
            for n in names.iter().take(show) {
                println!("           {n}");
            }
        }
    }
}
