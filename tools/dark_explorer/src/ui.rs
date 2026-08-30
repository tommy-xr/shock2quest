//! Windowed asset browser: family/directory tree + search on the left,
//! per-asset preview (image/audio/text/hex) on the right.

use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::rc::Rc;

use eframe::egui;
use engine::assets::asset_paths::{AbstractAssetPath, AssetEntry};
use engine::audio::{AudioClip, AudioContext, AudioHandle};
use engine::texture_format::{self, PixelFormat};

use crate::explorer;

const IMAGE_EXTENSIONS: &[&str] = &["pcx", "tga", "png", "gif", "jpg", "jpeg", "dds"];
const TEXT_EXTENSIONS: &[&str] = &["str", "mtl", "txt", "ini", "cfg", "json"];
/// Cap on rows the flattened search view renders per frame.
const SEARCH_RESULT_CAP: usize = 500;

pub fn run(screenshot: Option<PathBuf>, select: Option<String>, search: Option<String>) {
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([1150.0, 760.0])
        .with_title("dark_explorer");
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let result = eframe::run_native(
        "dark_explorer",
        options,
        Box::new(move |_cc| Ok(Box::new(ExplorerApp::new(screenshot, select, search)))),
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
        width: u32,
        height: u32,
        texture: Option<egui::TextureHandle>,
    },
    Audio {
        bytes: Vec<u8>,
        duration: Option<std::time::Duration>,
    },
    Text(String),
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

pub struct ExplorerApp {
    family_names: Vec<&'static str>,
    families: BTreeMap<String, LoadedFamily>,
    search: String,
    selected: Option<(String, String)>,
    preview: Option<Preview>,
    audio: Option<AudioContext<(), String>>,
    audio_error: Option<String>,
    screenshot: Option<PathBuf>,
    frames_rendered: u32,
    /// One-shot: scroll the tree to the selected row on the next frame.
    scroll_to_selected: bool,
}

impl ExplorerApp {
    fn new(
        screenshot: Option<PathBuf>,
        select: Option<String>,
        search: Option<String>,
    ) -> ExplorerApp {
        let mut app = ExplorerApp {
            family_names: explorer::family_names(),
            families: BTreeMap::new(),
            search: search.unwrap_or_default(),
            selected: None,
            preview: None,
            audio: None,
            audio_error: None,
            screenshot,
            frames_rendered: 0,
            scroll_to_selected: false,
        };
        if let Some(select) = select {
            match select.split_once('/') {
                Some((family, key)) if app.family_names.contains(&family) => {
                    app.select(family.to_string(), key.to_string());
                }
                // `data` keys have no '/', so a bare family/key split fails there.
                _ if select.starts_with("data/") || app.family_names.contains(&select.as_str()) => {
                    eprintln!("--select wants <family>/<key>, got '{select}'");
                }
                _ => eprintln!("--select: unknown family in '{select}'"),
            }
        }
        app
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
        self.scroll_to_selected = true;
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
    let kind = decode_preview(key, bytes);
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

fn decode_preview(key: &str, bytes: Vec<u8>) -> PreviewKind {
    let ext = extension(key).to_ascii_lowercase();
    // Dark parsers panic on malformed input, so every decode runs under
    // catch_unwind and falls back to the hex view instead of crashing.
    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        let decoded = panic::catch_unwind(AssertUnwindSafe(|| {
            texture_format::extension_to_format(ext.clone()).map(|format| format.load(&bytes))
        }));
        return match decoded {
            Ok(Some(raw)) => {
                let size = [raw.width as usize, raw.height as usize];
                let color_image = match raw.format {
                    PixelFormat::RGB => egui::ColorImage::from_rgb(size, &raw.bytes),
                    PixelFormat::RGBA => egui::ColorImage::from_rgba_unmultiplied(size, &raw.bytes),
                };
                PreviewKind::Image {
                    color_image,
                    width: raw.width,
                    height: raw.height,
                    texture: None,
                }
            }
            Ok(None) => raw_fallback(&bytes, format!("no decoder for .{ext}")),
            Err(_) => raw_fallback(&bytes, format!(".{ext} decode panicked")),
        };
    }
    if ext == "wav" {
        let duration = panic::catch_unwind(AssertUnwindSafe(|| {
            AudioClip::from_bytes(bytes.clone()).total_duration()
        }));
        return match duration {
            Ok(duration) => PreviewKind::Audio { bytes, duration },
            Err(_) => raw_fallback(&bytes, "wav decode panicked".to_string()),
        };
    }
    if TEXT_EXTENSIONS.contains(&ext.as_str()) {
        return PreviewKind::Text(String::from_utf8_lossy(&bytes).into_owned());
    }
    raw_fallback(&bytes, format!("cannot render .{ext} files"))
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut clicked: Option<(String, String)> = None;

        egui::Panel::left(egui::Id::new("asset_tree"))
            .resizable(true)
            .default_size(380.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
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
            });

        egui::CentralPanel::default_margins().show(ui, |ui| {
            self.show_preview(ui);
        });

        self.scroll_to_selected = false;
        if let Some((family, key)) = clicked {
            self.select(family, key);
        }

        self.drive_screenshot(&ctx);
    }
}

impl ExplorerApp {
    fn show_tree(&mut self, ui: &mut egui::Ui, clicked: &mut Option<(String, String)>) {
        let selected = self.selected.clone();
        let scroll_to_selected = self.scroll_to_selected;
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
        let mut shown = 0;
        let mut total = 0;
        for family in self.family_names.clone() {
            let loaded = self.family(family);
            // Borrow-friendly copy of the matches; the row set is capped small.
            let matches: Vec<String> = loaded
                .entries
                .iter()
                .filter(|e| e.key.contains(&needle))
                .map(|e| e.key.clone())
                .collect();
            for key in matches {
                total += 1;
                if shown >= SEARCH_RESULT_CAP {
                    continue;
                }
                shown += 1;
                let label = format!("{family}/{key}");
                let is_selected = selected
                    .as_ref()
                    .is_some_and(|(f, k)| f == family && *k == key);
                if ui.selectable_label(is_selected, label).clicked() {
                    *clicked = Some((family.to_string(), key));
                }
            }
        }
        if total > shown {
            ui.label(format!(
                "... {} more matches (narrow the search)",
                total - shown
            ));
        }
        if total == 0 {
            ui.label("no matches");
        }
    }

    fn show_preview(&mut self, ui: &mut egui::Ui) {
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
                width,
                height,
                texture,
            } => {
                ui.label(format!("{width} x {height}"));
                let texture = texture.get_or_insert_with(|| {
                    ui.ctx().load_texture(
                        preview.key.clone(),
                        color_image.clone(),
                        egui::TextureOptions::NEAREST,
                    )
                });
                // Scale small game textures up to a readable size.
                let max_dim = (*width).max(*height) as f32;
                let scale = (256.0 / max_dim).clamp(1.0, 8.0);
                let display = egui::Vec2::new(*width as f32 * scale, *height as f32 * scale);
                egui::ScrollArea::both().show(ui, |ui| {
                    ui.image((texture.id(), display));
                });
            }
            PreviewKind::Audio { bytes, duration } => {
                if let Some(duration) = duration {
                    ui.label(format!("Duration: {:.2}s", duration.as_secs_f32()));
                }
                if ui.button("▶ Play").clicked() {
                    play_wav(&mut self.audio, &mut self.audio_error, bytes.clone());
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
        if self.frames_rendered == 3 {
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
    audio_error: &mut Option<String>,
    bytes: Vec<u8>,
) {
    *audio_error = None;
    // The output device is opened on first play so `--screenshot` runs never
    // touch audio; AudioContext::new panics without an output device.
    let played = panic::catch_unwind(AssertUnwindSafe(|| {
        let context = match audio {
            Some(context) => context,
            None => audio.insert(AudioContext::new()),
        };
        engine::audio::play_audio(
            context,
            AudioHandle::new(),
            None,
            Rc::new(AudioClip::from_bytes(bytes)),
        );
    }));
    if played.is_err() {
        *audio_error = Some("playback failed (no audio device or bad wav)".to_string());
    }
}
