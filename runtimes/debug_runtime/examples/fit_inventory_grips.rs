//! Fill missing pickup poses using the Explorer/game auto-fitter at the requested scale.
//! Run from the repository root; the hidden GL context loads the production glove rig.
use std::{collections::HashMap, path::PathBuf};

use anyhow::{Context, Result, ensure};
use clap::Parser;
use dark::importers::GRIP_SURFACE_IMPORTER;
use engine::assets::asset_cache::AssetCache;
use glfw::Context as _;
use shock2vr::{
    GloveRenderer, Handedness,
    vr_grip::{
        BakedGripEntry, GripHints, GripLibrary, GripSurface, SOLVER_REVISION, surface_fingerprint,
    },
};

#[derive(Parser)]
struct Args {
    /// Comma-separated pickup model names (not first-person weapon meshes)
    #[arg(long, value_delimiter = ',', required = true)]
    models: Vec<String>,
    /// Explicitly replace existing poses for these models (both hands)
    #[arg(long, value_delimiter = ',')]
    refit: Vec<String>,
    #[arg(long, default_value_t = 0.55)]
    scale: f32,
    #[arg(long, default_value = "assets/vr-grips.json")]
    library: PathBuf,
    /// Separate output; the source library is never overwritten
    #[arg(long)]
    output: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(
        args.scale.is_finite() && args.scale > 0.0 && args.scale <= 10.0,
        "Invalid scale"
    );
    ensure!(
        !args.output.exists(),
        "Output already exists; choose a new output path"
    );
    let source = std::fs::read(&args.library)?;
    let mut library: GripLibrary = serde_json::from_slice(&source)?;
    ensure!(library.version == 1, "Unsupported grip library version");
    let hints: HashMap<String, GripHints> =
        serde_json::from_slice(&std::fs::read("assets/astra-vr-grip-hints.json")?)?;

    let mut glfw = glfw::init(glfw::fail_on_errors)?;
    glfw.window_hint(glfw::WindowHint::ContextVersion(4, 1));
    glfw.window_hint(glfw::WindowHint::OpenGlProfile(
        glfw::OpenGlProfileHint::Core,
    ));
    #[cfg(target_os = "macos")]
    glfw.window_hint(glfw::WindowHint::OpenGlForwardCompat(true));
    glfw.window_hint(glfw::WindowHint::Visible(false));
    let (mut window, _events) = glfw
        .create_window(64, 64, "Grip fitting", glfw::WindowMode::Windowed)
        .context("GL context unavailable")?;
    window.make_current();
    gl::load_with(|symbol| window.get_proc_address(symbol) as *const _);
    let storage =
        engine::file_system::storage::init(Box::new(engine::file_system::DefaultFileSystem {
            root_path: Box::new(std::path::Path::new("./assets/")),
        }));
    let mut cache = AssetCache::new(
        shock2vr::paths::data_root().to_string_lossy().into_owned(),
        shock2vr::game_asset_mounts(storage),
    );
    let mut glove = GloveRenderer::new(&mut cache).context("Production glove unavailable")?;
    let rigs = [
        ("left", glove.grip_kinematics(Handedness::Left)),
        ("right", glove.grip_kinematics(Handedness::Right)),
    ];
    let mut failures = Vec::new();
    for model in &args.models {
        let model = model
            .to_ascii_lowercase()
            .trim_end_matches(".bin")
            .to_owned();
        ensure!(
            !shock2vr::vr_weapon_grip::supports_model(&model),
            "Use the weapon fitter for {model}"
        );
        let refit = args.refit.iter().any(|name| name == &model);
        if !refit
            && rigs
                .iter()
                .all(|(hand, _)| library.lookup(&model, hand).is_some())
        {
            continue;
        }
        let Some(triangles) = cache.get_opt(&GRIP_SURFACE_IMPORTER, &format!("{model}.bin")) else {
            failures.push(format!("{model}: missing geometry"));
            continue;
        };
        let scaled: Vec<_> = triangles
            .iter()
            .map(|t| t.map(|p| p * args.scale))
            .collect();
        let Some(surface) = GripSurface::new(&scaled) else {
            failures.push(format!("{model}: unusable surface"));
            continue;
        };
        let mut hint = hints.get(&model).cloned().unwrap_or_default();
        hint.anchor = hint.anchor.map(|p| p.map(|v| v * args.scale));
        hint.anchor_region = hint
            .anchor_region
            .map(|region| region.map(|p| p.map(|v| v * args.scale)));
        for (hand, rig) in &rigs {
            if !refit && library.lookup(&model, hand).is_some() {
                continue;
            }
            let Some(grip) = surface.resolve(rig, &hint) else {
                failures.push(format!("{model}/{hand}: no valid auto-fit"));
                continue;
            };
            let grip = grip.with_item_scale(args.scale);
            ensure!(grip.is_valid(), "Invalid resolved grip for {model}/{hand}");
            println!(
                "{model}/{hand}: scale={} contacts={} score={:.3}",
                grip.item_scale,
                grip.contacts.iter().flatten().count(),
                grip.score
            );
            let entry = BakedGripEntry {
                model: model.clone(),
                hand: hand.to_string(),
                // Like Explorer's explicit Auto-fit, protect the requested scale
                // and result from unrelated bulk rebakes.
                authored: true,
                surface_hash: surface_fingerprint(&triangles),
                kinematics_hash: rig.fingerprint(),
                hints_hash: hint.fingerprint(),
                grip,
            };
            if let Some(previous) = library
                .entries
                .iter_mut()
                .find(|e| e.model == model && e.hand == *hand)
            {
                *previous = entry;
            } else {
                library.entries.push(entry);
            }
        }
    }
    // Keep successful results available for inspection even when some models need attention.
    library.solver_revision = SOLVER_REVISION;
    ensure!(
        std::fs::read(&args.library)? == source,
        "Source changed during fitting"
    );
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args.output)?;
    use std::io::Write;
    output.write_all(serde_json::to_string_pretty(&library)?.as_bytes())?;
    output.write_all(b"\n")?;
    ensure!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}
