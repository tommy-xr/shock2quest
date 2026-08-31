//! Windowed asset browser: family/directory tree + search on the left,
//! per-asset preview (image/audio/text/3D model/hex) on the right.

use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::rc::Rc;

use eframe::egui;
use engine::assets::asset_paths::{AbstractAssetPath, AssetEntry};
use engine::audio::{AudioClip, AudioContext, AudioHandle};
use engine::texture_format::{self, PixelFormat};

use crate::archetypes::{Archetype, ArchetypeDb};
use crate::explorer;
use crate::model_preview::{self, ModelPreview};

const TEXT_EXTENSIONS: &[&str] = &["str", "mtl", "txt", "ini", "cfg", "json"];
/// Cap on rows the flattened search view renders per frame.
const SEARCH_RESULT_CAP: usize = 500;

pub struct UiOptions {
    pub screenshot: Option<PathBuf>,
    pub select: Option<String>,
    pub search: Option<String>,
    /// Start the model preview with the skeleton / hitbox overlay on (so a
    /// `--screenshot` run can capture the debug overlays).
    pub skeletons: bool,
    pub hitboxes: bool,
    /// Open the Archetypes tab with this creature selected (name or
    /// template id), optionally playing `clip`, advanced by `advance` seconds
    /// of simulation time before a `--screenshot` capture.
    pub archetype: Option<String>,
    pub clip: Option<String>,
    pub advance: Option<f32>,
}

pub fn run(options: UiOptions) {
    explorer::print_coverage_caveat();
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([1150.0, 760.0])
        .with_title("dark_explorer");
    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let result = eframe::run_native(
        "dark_explorer",
        native_options,
        Box::new(move |cc| {
            // Point the engine's raw `gl` bindings at eframe's GL context so
            // the model preview can render with the game renderer.
            model_preview::init_raw_gl(cc);
            // No open/close or scroll animation: a `--select` scroll measured
            // against a still-animating CollapsingHeader lands on stale layout,
            // and an animated scroll-to hasn't arrived when `--screenshot`
            // captures a few frames in.
            cc.egui_ctx.all_styles_mut(|style| {
                style.animation_time = 0.0;
                style.scroll_animation = egui::style::ScrollAnimation::none();
            });
            Ok(Box::new(ExplorerApp::new(options)))
        }),
    );
    if let Err(err) = result {
        eprintln!("ui failed: {err}");
        std::process::exit(1);
    }
}

/// Directory level of one family's tree: subdirectories plus files, both
/// keyed/indexed for stable alphabetical rendering.
#[derive(Default)]
struct DirNode {
    dirs: BTreeMap<String, DirNode>,
    /// (file name, index into `LoadedFamily::entries`)
    files: Vec<(String, usize)>,
}

struct LoadedFamily {
    mounts: Box<dyn AbstractAssetPath>,
    /// Every enumerable entry, aliases included, in mount-priority order
    /// (first occurrence of a key is the mount a lookup serves it from).
    all_entries: Vec<AssetEntry>,
    /// De-aliased, key-sorted entries backing the tree.
    entries: Vec<AssetEntry>,
    tree: DirNode,
}

impl LoadedFamily {
    fn load(family: &str) -> LoadedFamily {
        let mounts = explorer::family_mounts(family);
        let all_entries = explorer::entries_of(family, &*mounts);
        // One tree row per key: mount order means the first occurrence is the
        // copy a lookup serves; later same-key entries are shadowed.
        let mut seen = std::collections::HashSet::new();
        let mut entries: Vec<AssetEntry> = all_entries
            .iter()
            .filter(|e| !e.is_alias && seen.insert(e.key.clone()))
            .cloned()
            .collect();
        entries.sort_by(|a, b| a.key.cmp(&b.key));
        let mut tree = DirNode::default();
        for (index, entry) in entries.iter().enumerate() {
            let mut node = &mut tree;
            let mut segments = entry.key.split('/').peekable();
            while let Some(segment) = segments.next() {
                if segments.peek().is_some() {
                    node = node.dirs.entry(segment.to_string()).or_default();
                } else {
                    node.files.push((segment.to_string(), index));
                }
            }
        }
        LoadedFamily {
            mounts,
            all_entries,
            entries,
            tree,
        }
    }
}

enum PreviewKind {
    Image {
        color_image: egui::ColorImage,
        texture: Option<egui::TextureHandle>,
    },
    Audio {
        bytes: Vec<u8>,
        duration: Option<std::time::Duration>,
    },
    Text(String),
    /// A `.bin` 3D model, rendered by `ModelPreview` from `Preview::key`.
    Model,
    /// Undecodable content: the reason plus a hex dump of the leading bytes.
    Raw {
        reason: String,
        hex: String,
    },
}

struct Preview {
    family: String,
    key: String,
    size: usize,
    winner: AssetEntry,
    /// Same key served by lower-priority mounts (shadowed copies).
    shadowed: Vec<AssetEntry>,
    kind: PreviewKind,
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Files,
    Archetypes,
}

pub struct ExplorerApp {
    tab: Tab,
    family_names: Vec<&'static str>,
    families: BTreeMap<String, LoadedFamily>,
    search: String,
    selected: Option<(String, String)>,
    preview: Option<Preview>,
    audio: Option<AudioContext<(), String>>,
    /// The previous Play's handle, stopped before the next one starts.
    audio_handle: Option<AudioHandle>,
    audio_error: Option<String>,
    screenshot: Option<PathBuf>,
    /// Lazily built on the first model selection: constructing it indexes
    /// every family's archives and creates the engine render host.
    model_preview: Option<ModelPreview>,
    /// Initial overlay toggles for the model preview (from the CLI).
    initial_overlays: (bool, bool),
    frames_rendered: u32,
    /// Frames left in which the tree scrolls to the selected row (a couple of
    /// frames, so the scroll re-applies once layout has settled).
    scroll_frames: u8,
    /// Flattened search rows, cached per needle: (needle, capped rows, total).
    search_results: Option<(String, Vec<(String, String)>, usize)>,
    /// Lazily built on first opening the Archetypes tab: parsing the gamesys
    /// and motion database takes a moment. Err = why it failed, shown inline.
    archetype_db: Option<Result<ArchetypeDb, String>>,
    archetype_search: String,
    selected_archetype: Option<i32>,
    selected_clip: Option<String>,
    /// Clip list cached for the selected archetype's template id.
    archetype_clips: Option<(i32, Result<Vec<String>, String>)>,
    /// Simulation seconds to step the scene once it exists (from `--advance`).
    advance_pending: Option<f32>,
}

impl ExplorerApp {
    fn new(options: UiOptions) -> ExplorerApp {
        let mut app = ExplorerApp {
            tab: Tab::Files,
            family_names: explorer::family_names(),
            families: BTreeMap::new(),
            search: options.search.unwrap_or_default(),
            selected: None,
            preview: None,
            audio: None,
            audio_handle: None,
            audio_error: None,
            screenshot: options.screenshot,
            model_preview: None,
            initial_overlays: (options.skeletons, options.hitboxes),
            frames_rendered: 0,
            scroll_frames: 0,
            search_results: None,
            archetype_db: None,
            archetype_search: String::new(),
            selected_archetype: None,
            selected_clip: None,
            archetype_clips: None,
            advance_pending: options.advance,
        };
        if let Some(archetype) = options.archetype {
            // Fail loudly, like --select: a `--screenshot` run that quietly
            // captured an empty preview would still exit 0 otherwise.
            let id = {
                let db = match app.archetype_db().as_ref() {
                    Ok(db) => db,
                    Err(err) => {
                        eprintln!("--archetype: {err}");
                        std::process::exit(2);
                    }
                };
                let id = match db.resolve(&archetype) {
                    Ok(id) => id,
                    Err(err) => {
                        eprintln!("--archetype: {err}");
                        std::process::exit(2);
                    }
                };
                if let Some(clip) = &options.clip {
                    match db.archetypes.get(&id).map(|a| db.clips_for(a)) {
                        Some(Ok(clips)) if clips.iter().any(|c| c == clip) => {}
                        Some(Ok(_)) => {
                            eprintln!("--clip: no clip '{clip}' for that archetype");
                            std::process::exit(2);
                        }
                        Some(Err(err)) => {
                            eprintln!("--clip: cannot list clips: {err}");
                            std::process::exit(2);
                        }
                        None => unreachable!("resolve returned an unknown archetype"),
                    }
                }
                id
            };
            app.tab = Tab::Archetypes;
            app.selected_archetype = Some(id);
            app.selected_clip = options.clip;
        } else if options.clip.is_some() {
            eprintln!("--clip needs --archetype");
            std::process::exit(2);
        }
        if let Some(select) = options.select {
            // Fail loudly on a bad selection: a `--screenshot` run that quietly
            // captured an empty preview would still exit 0 otherwise.
            match select.split_once('/') {
                Some((family, key)) if app.family_names.contains(&family) => {
                    let key = key.to_ascii_lowercase(); // lookup keys are lowercased
                    if !app.family(family).all_entries.iter().any(|e| e.key == key) {
                        eprintln!("--select: no asset '{key}' in family '{family}'");
                        std::process::exit(2);
                    }
                    app.select(family.to_string(), key);
                }
                _ => {
                    eprintln!("--select wants <known family>/<key>, got '{select}'");
                    std::process::exit(2);
                }
            }
        }
        app
    }

    fn archetype_db(&mut self) -> &Result<ArchetypeDb, String> {
        self.archetype_db.get_or_insert_with(ArchetypeDb::load)
    }

    fn family(&mut self, name: &str) -> &LoadedFamily {
        self.families
            .entry(name.to_string())
            .or_insert_with(|| LoadedFamily::load(name))
    }

    fn select(&mut self, family: String, key: String) {
        let loaded = self.family(&family);
        self.preview = Some(build_preview(&family, &key, loaded));
        self.selected = Some((family, key));
        self.scroll_frames = 3;
    }
}

fn build_preview(family: &str, key: &str, loaded: &LoadedFamily) -> Preview {
    // Same-key entries in mount order: first is the copy a lookup gets,
    // the rest are shadowed (the CLI `which` view of this one key).
    let mut same_key = loaded.all_entries.iter().filter(|e| e.key == key);
    let winner = same_key.next().cloned().unwrap_or(AssetEntry {
        key: key.to_string(),
        source: String::new(),
        entry_name: String::new(),
        is_alias: false,
    });
    let shadowed: Vec<AssetEntry> = same_key.cloned().collect();

    let bytes = match explorer::read_asset_bytes(&*loaded.mounts, key) {
        Some(bytes) => bytes,
        None => {
            return Preview {
                family: family.to_string(),
                key: key.to_string(),
                size: 0,
                winner,
                shadowed,
                kind: PreviewKind::Raw {
                    reason: "failed to read asset bytes".to_string(),
                    hex: String::new(),
                },
            };
        }
    };
    let size = bytes.len();
    let kind = decode_preview(family, key, bytes);
    Preview {
        family: family.to_string(),
        key: key.to_string(),
        size,
        winner,
        shadowed,
        kind,
    }
}

fn extension(key: &str) -> &str {
    key.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("")
}

fn decode_preview(family: &str, key: &str, bytes: Vec<u8>) -> PreviewKind {
    let ext = extension(key).to_ascii_lowercase();
    // Dark parsers panic on malformed input, so every decode runs under
    // catch_unwind and falls back to the hex view instead of crashing.
    if texture_format::DECODABLE_EXTENSIONS.contains(&ext.as_str()) {
        // ColorImage construction is inside the guard too: it asserts
        // bytes.len() == w*h*channels, which a bad decode can violate.
        let decoded = quiet_catch(|| {
            texture_format::extension_to_format(ext.clone()).map(|format| {
                let raw = format.load(&bytes);
                let size = [raw.width as usize, raw.height as usize];
                match raw.format {
                    PixelFormat::RGB => egui::ColorImage::from_rgb(size, &raw.bytes),
                    PixelFormat::RGBA => egui::ColorImage::from_rgba_unmultiplied(size, &raw.bytes),
                }
            })
        });
        return match decoded {
            Ok(Some(color_image)) => PreviewKind::Image {
                color_image,
                texture: None,
            },
            Ok(None) => raw_fallback(&bytes, format!("no decoder for .{ext}")),
            Err(msg) => raw_fallback(&bytes, format!(".{ext} decode failed: {msg}")),
        };
    }
    if ext == "wav" {
        let duration = quiet_catch(|| AudioClip::from_bytes(bytes.clone()).total_duration());
        return match duration {
            Ok(duration) => PreviewKind::Audio { bytes, duration },
            Err(msg) => raw_fallback(&bytes, format!("wav decode failed: {msg}")),
        };
    }
    if TEXT_EXTENSIONS.contains(&ext.as_str()) {
        return PreviewKind::Text(String::from_utf8_lossy(&bytes).into_owned());
    }
    // Only the model families: `data` also serves `.bin` files (motiondb)
    // whose parse failure aborts rather than unwinds.
    if ext == "bin" && matches!(family, "obj" | "mesh") {
        return PreviewKind::Model;
    }
    raw_fallback(&bytes, format!("cannot render .{ext} files"))
}

/// Run a Dark parser under `catch_unwind` with the panic hook silenced (the
/// format readers panic on malformed input), mapping a panic to its message.
pub(crate) fn quiet_catch<T>(f: impl FnOnce() -> T) -> Result<T, String> {
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

fn raw_fallback(bytes: &[u8], reason: String) -> PreviewKind {
    PreviewKind::Raw {
        reason,
        hex: hex_dump(bytes, 256),
    }
}

/// Classic offset/hex/ascii dump of the first `limit` bytes.
fn hex_dump(bytes: &[u8], limit: usize) -> String {
    let mut out = String::new();
    for (row, chunk) in bytes[..bytes.len().min(limit)].chunks(16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let ascii: String = chunk
            .iter()
            .map(|&b| {
                if (0x20..0x7f).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        out.push_str(&format!(
            "{:04x}  {:<47}  {}\n",
            row * 16,
            hex.join(" "),
            ascii
        ));
    }
    if bytes.len() > limit {
        out.push_str(&format!("... {} more bytes\n", bytes.len() - limit));
    }
    out
}

impl eframe::App for ExplorerApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut clicked: Option<(String, String)> = None;

        egui::Panel::left(egui::Id::new("asset_tree"))
            .resizable(true)
            .default_size(380.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, Tab::Files, "Files");
                    ui.selectable_value(&mut self.tab, Tab::Archetypes, "Archetypes");
                });
                ui.separator();
                match self.tab {
                    Tab::Files => {
                        ui.horizontal(|ui| {
                            ui.label("Search:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.search)
                                    .hint_text("substring of a key")
                                    .desired_width(f32::INFINITY),
                            );
                        });
                        ui.separator();
                        egui::ScrollArea::both()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if self.search.is_empty() {
                                    self.show_tree(ui, &mut clicked);
                                } else {
                                    self.show_search_results(ui, &mut clicked);
                                }
                            });
                    }
                    Tab::Archetypes => self.show_archetype_panel(ui),
                }
            });

        egui::CentralPanel::default_margins().show(ui, |ui| {
            if self.tab == Tab::Archetypes {
                self.show_archetype_preview(ui, frame);
            } else {
                self.show_preview(ui, frame);
            }
        });

        self.scroll_frames = self.scroll_frames.saturating_sub(1);
        if let Some((family, key)) = clicked {
            self.select(family, key);
        }

        self.drive_screenshot(&ctx);
    }
}

impl ExplorerApp {
    fn show_tree(&mut self, ui: &mut egui::Ui, clicked: &mut Option<(String, String)>) {
        let selected = self.selected.clone();
        let scroll_to_selected = self.scroll_frames > 0;
        for family in self.family_names.clone() {
            let open_family = selected.as_ref().map(|(f, _)| f == family);
            let header =
                egui::CollapsingHeader::new(family).default_open(open_family == Some(true));
            header.show(ui, |ui| {
                let loaded = self.family(family);
                let selected_key = selected
                    .as_ref()
                    .filter(|(f, _)| f == family)
                    .map(|(_, k)| k.clone());
                show_dir(
                    ui,
                    family,
                    "",
                    &loaded.tree,
                    &loaded.entries,
                    selected_key.as_deref(),
                    scroll_to_selected,
                    clicked,
                );
            });
        }
    }

    fn show_search_results(&mut self, ui: &mut egui::Ui, clicked: &mut Option<(String, String)>) {
        let needle = self.search.to_ascii_lowercase();
        let selected = self.selected.clone();
        // Recompute the flattened rows only when the needle changes; a search
        // over every family is too costly to redo per frame.
        if self.search_results.as_ref().map(|(n, _, _)| n.as_str()) != Some(needle.as_str()) {
            let mut rows: Vec<(String, String)> = Vec::new();
            let mut total = 0;
            for family in self.family_names.clone() {
                let loaded = self.family(family);
                for entry in &loaded.entries {
                    if !entry.key.contains(&needle) {
                        continue;
                    }
                    total += 1;
                    if rows.len() < SEARCH_RESULT_CAP {
                        rows.push((family.to_string(), entry.key.clone()));
                    }
                }
            }
            self.search_results = Some((needle.clone(), rows, total));
        }
        let Some((_, rows, total)) = &self.search_results else {
            return;
        };
        for (family, key) in rows {
            let is_selected = selected
                .as_ref()
                .is_some_and(|(f, k)| f == family && k == key);
            if ui
                .selectable_label(is_selected, format!("{family}/{key}"))
                .clicked()
            {
                *clicked = Some((family.clone(), key.clone()));
            }
        }
        if *total > rows.len() {
            ui.label(format!(
                "... {} more matches (narrow the search)",
                total - rows.len()
            ));
        }
        if *total == 0 {
            ui.label("no matches");
        }
    }

    /// Left panel, Archetypes tab: search box + creature template tree (or a
    /// flat filtered list while searching).
    fn show_archetype_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Search:");
            ui.add(
                egui::TextEdit::singleline(&mut self.archetype_search)
                    .hint_text("archetype name")
                    .desired_width(f32::INFINITY),
            );
        });
        ui.separator();
        let db = match self.archetype_db.get_or_insert_with(ArchetypeDb::load) {
            Ok(db) => db,
            Err(err) => {
                ui.label(format!("Cannot load gamesys: {err}"));
                return;
            }
        };
        let selected = self.selected_archetype;
        let mut clicked: Option<i32> = None;
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.archetype_search.is_empty() {
                    let open_path = selected.map(|id| db.ancestors_of(id)).unwrap_or_default();
                    for root in &db.roots {
                        show_archetype_node(ui, db, *root, selected, &open_path, &mut clicked);
                    }
                } else {
                    let needle = self.archetype_search.to_ascii_lowercase();
                    let mut matches: Vec<&Archetype> = db
                        .archetypes
                        .values()
                        .filter(|a| a.name.to_ascii_lowercase().contains(&needle))
                        .collect();
                    matches.sort_by_key(|a| a.name.to_ascii_lowercase());
                    for archetype in &matches {
                        let is_selected = selected == Some(archetype.template_id);
                        if ui.selectable_label(is_selected, &archetype.name).clicked() {
                            clicked = Some(archetype.template_id);
                        }
                    }
                    if matches.is_empty() {
                        ui.label("no matches");
                    }
                }
            });
        if let Some(id) = clicked {
            self.selected_archetype = Some(id);
            self.selected_clip = None;
            self.archetype_clips = None;
        }
    }

    /// Right pane, Archetypes tab: archetype info, clip list, 3D preview.
    fn show_archetype_preview(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let Some(id) = self.selected_archetype else {
            ui.centered_and_justified(|ui| {
                ui.label("Select an archetype to preview it");
            });
            return;
        };
        // The db is loaded whenever a selection exists; guard regardless.
        let archetype = match &self.archetype_db {
            Some(Ok(db)) => match db.archetypes.get(&id) {
                Some(archetype) => archetype.clone(),
                None => {
                    ui.label(format!("no archetype with template id {id}"));
                    return;
                }
            },
            _ => {
                ui.label("archetype data not loaded");
                return;
            }
        };
        if self.archetype_clips.as_ref().map(|(i, _)| *i) != Some(id) {
            let clips = match &self.archetype_db {
                Some(Ok(db)) => db.clips_for(&archetype),
                _ => Err("archetype data not loaded".to_string()),
            };
            self.archetype_clips = Some((id, clips));
        }

        ui.heading(&archetype.name);
        egui::Grid::new("archetype_info")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Template id");
                ui.label(archetype.template_id.to_string());
                ui.end_row();
                ui.label("Model");
                ui.label(archetype.model_key());
                ui.end_row();
                ui.label("Creature type");
                ui.label(archetype.creature_type_name());
                ui.end_row();
                ui.label("Actor type");
                ui.label(archetype.actor_type_name());
                ui.end_row();
            });
        ui.separator();

        egui::Panel::right(egui::Id::new("clip_list"))
            .resizable(true)
            .default_size(220.0)
            .show(ui, |ui| {
                ui.label("Clips (click to play)");
                ui.separator();
                match &self.archetype_clips {
                    Some((_, Ok(clips))) => {
                        if clips.is_empty() {
                            ui.label("(no clips for this actor type)");
                        }
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for clip in clips {
                                    let is_playing = self.selected_clip.as_deref() == Some(clip);
                                    if ui.selectable_label(is_playing, clip).clicked() {
                                        // Click toggles: re-clicking stops it.
                                        self.selected_clip = (!is_playing).then(|| clip.clone());
                                    }
                                }
                            });
                    }
                    Some((_, Err(err))) => {
                        ui.label(format!("Cannot list clips: {err}"));
                    }
                    None => {}
                }
            });

        let host = preview_host(
            &mut self.model_preview,
            self.initial_overlays,
            self.screenshot.is_some(),
        );
        host.show(
            ui,
            frame,
            &archetype.model_key(),
            self.selected_clip.as_deref(),
        );
    }

    fn show_preview(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let Some(preview) = &mut self.preview else {
            ui.centered_and_justified(|ui| {
                ui.label("Select an asset to preview it");
            });
            return;
        };

        ui.heading(&preview.key);
        egui::Grid::new("asset_info").num_columns(2).show(ui, |ui| {
            ui.label("Family");
            ui.label(&preview.family);
            ui.end_row();
            ui.label("Source");
            ui.label(explorer::short_source(&preview.winner.source));
            ui.end_row();
            ui.label("Entry name");
            ui.label(&preview.winner.entry_name);
            ui.end_row();
            ui.label("Size");
            ui.label(format!("{} bytes", preview.size));
            ui.end_row();
            if !preview.shadowed.is_empty() {
                ui.label("Shadowed copies");
                ui.vertical(|ui| {
                    for entry in &preview.shadowed {
                        ui.label(format!(
                            "{} ({})",
                            explorer::short_source(&entry.source),
                            entry.entry_name
                        ));
                    }
                });
                ui.end_row();
            }
        });
        ui.separator();

        match &mut preview.kind {
            PreviewKind::Image {
                color_image,
                texture,
            } => {
                let [width, height] = color_image.size;
                ui.label(format!("{width} x {height}"));
                let texture = texture.get_or_insert_with(|| {
                    ui.ctx().load_texture(
                        preview.key.clone(),
                        color_image.clone(),
                        egui::TextureOptions::NEAREST,
                    )
                });
                // Scale small game textures up to a readable size.
                let max_dim = width.max(height) as f32;
                let scale = (256.0 / max_dim).clamp(1.0, 8.0);
                let display = egui::Vec2::new(width as f32 * scale, height as f32 * scale);
                egui::ScrollArea::both().show(ui, |ui| {
                    ui.image((texture.id(), display));
                });
            }
            PreviewKind::Audio { bytes, duration } => {
                if let Some(duration) = duration {
                    ui.label(format!("Duration: {:.2}s", duration.as_secs_f32()));
                }
                if ui.button("▶ Play").clicked() {
                    play_wav(
                        &mut self.audio,
                        &mut self.audio_handle,
                        &mut self.audio_error,
                        bytes.clone(),
                    );
                }
                if let Some(error) = &self.audio_error {
                    ui.colored_label(egui::Color32::RED, error);
                }
            }
            PreviewKind::Text(text) => {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut text.as_str())
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY),
                        );
                    });
            }
            PreviewKind::Model => {
                let key = preview.key.clone();
                let host = preview_host(
                    &mut self.model_preview,
                    self.initial_overlays,
                    self.screenshot.is_some(),
                );
                host.show(ui, frame, &key, None);
            }
            PreviewKind::Raw { reason, hex } => {
                ui.label(format!("Cannot render this file: {reason}"));
                if !hex.is_empty() {
                    egui::ScrollArea::both()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut hex.as_str())
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY),
                            );
                        });
                }
            }
        }
    }

    /// In `--screenshot` mode: let a few frames render, request a framebuffer
    /// capture, write it as PNG, and exit.
    fn drive_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else {
            return;
        };
        self.frames_rendered += 1;
        ctx.request_repaint();
        // The scene exists after the first frame's show; step it before the
        // capture so `--advance` screenshots a mid-clip pose.
        if self.frames_rendered == 2 && self.advance_pending.is_some() {
            if let Some(preview) = self.model_preview.as_mut() {
                preview.advance(self.advance_pending.take().unwrap());
            }
        }
        if self.frames_rendered == 3 {
            // A selection that failed to load would capture only its error
            // label; fail loudly instead so automation can trust exit 0.
            if let Some(error) = self.model_preview.as_ref().and_then(|p| p.error()) {
                eprintln!("cannot render the selection: {error}");
                std::process::exit(2);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let [width, height] = image.size;
            let result = image::save_buffer(
                &path,
                image.as_raw(),
                width as u32,
                height as u32,
                image::ColorType::Rgba8,
            );
            match result {
                Ok(()) => {
                    println!("wrote {}", path.display());
                    std::process::exit(0);
                }
                Err(err) => {
                    eprintln!("failed to write {}: {err}", path.display());
                    std::process::exit(1);
                }
            }
        }
        if self.frames_rendered > 300 {
            eprintln!("screenshot never arrived after 300 frames");
            std::process::exit(1);
        }
    }
}

/// The lazily-built 3D preview host. CLI overlays apply on first build, and a
/// `--screenshot` run pauses wall-clock animation so `--advance` is the only
/// time source (a deterministic pose per invocation).
fn preview_host(
    model_preview: &mut Option<ModelPreview>,
    (skeletons, hitboxes): (bool, bool),
    paused: bool,
) -> &mut ModelPreview {
    model_preview.get_or_insert_with(|| {
        let mut host = ModelPreview::new();
        host.debug_skeletons = skeletons;
        host.debug_hit_boxes = hitboxes;
        host.paused = paused;
        host
    })
}

/// One node of the archetype tree: a collapsible header where the template has
/// children (with a selectable "(this)" row when it is itself a creature),
/// else a selectable leaf.
fn show_archetype_node(
    ui: &mut egui::Ui,
    db: &ArchetypeDb,
    id: i32,
    selected: Option<i32>,
    open_path: &std::collections::HashSet<i32>,
    clicked: &mut Option<i32>,
) {
    let name = db.name_of(id);
    let is_archetype = db.archetypes.contains_key(&id);
    let is_selected = selected == Some(id);
    match db.children.get(&id) {
        Some(children) if !children.is_empty() => {
            egui::CollapsingHeader::new(&name)
                .id_salt(id)
                .default_open(open_path.contains(&id))
                .show(ui, |ui| {
                    if is_archetype {
                        if ui
                            .selectable_label(is_selected, format!("{name} (this)"))
                            .clicked()
                        {
                            *clicked = Some(id);
                        }
                    }
                    for child in children {
                        show_archetype_node(ui, db, *child, selected, open_path, clicked);
                    }
                });
        }
        _ => {
            // A childless grouping node (a multi-parent template's other
            // ancestor) is not selectable - only archetypes are.
            if !is_archetype {
                ui.label(&name);
            } else if ui.selectable_label(is_selected, &name).clicked() {
                *clicked = Some(id);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn show_dir(
    ui: &mut egui::Ui,
    family: &str,
    path: &str,
    node: &DirNode,
    entries: &[AssetEntry],
    selected_key: Option<&str>,
    scroll_to_selected: bool,
    clicked: &mut Option<(String, String)>,
) {
    for (name, child) in &node.dirs {
        let child_path = if path.is_empty() {
            name.clone()
        } else {
            format!("{path}/{name}")
        };
        let is_ancestor =
            selected_key.is_some_and(|key| key.starts_with(&format!("{child_path}/")));
        egui::CollapsingHeader::new(name)
            .id_salt(&child_path)
            .default_open(is_ancestor)
            .show(ui, |ui| {
                show_dir(
                    ui,
                    family,
                    &child_path,
                    child,
                    entries,
                    selected_key,
                    scroll_to_selected,
                    clicked,
                );
            });
    }
    for (name, index) in &node.files {
        let key = &entries[*index].key;
        let is_selected = selected_key == Some(key.as_str());
        let response = ui.selectable_label(is_selected, name);
        if is_selected && scroll_to_selected {
            response.scroll_to_me(Some(egui::Align::Center));
        }
        if response.clicked() {
            *clicked = Some((family.to_string(), key.clone()));
        }
    }
}

fn play_wav(
    audio: &mut Option<AudioContext<(), String>>,
    audio_handle: &mut Option<AudioHandle>,
    audio_error: &mut Option<String>,
    bytes: Vec<u8>,
) {
    *audio_error = None;
    // The output device is opened on first play so `--screenshot` runs never
    // touch audio; AudioContext::new panics without an output device.
    let played = quiet_catch(|| {
        let context = match audio {
            Some(context) => context,
            None => audio.insert(AudioContext::new()),
        };
        // Stop the previous play: it also reaps its sink entry, which nothing
        // else does here (the UI never runs AudioContext::update).
        if let Some(previous) = audio_handle.take() {
            engine::audio::stop_audio(context, previous);
        }
        let handle = AudioHandle::new();
        *audio_handle = Some(handle.clone());
        engine::audio::play_audio(context, handle, None, Rc::new(AudioClip::from_bytes(bytes)));
    });
    if let Err(msg) = played {
        *audio_error = Some(format!("playback failed: {msg}"));
    }
}
