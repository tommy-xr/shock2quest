//! Local authoring of prepared grips. The preview and runtime share geometry,
//! glove poses, transforms, and pose validation.
use crate::model_preview::{ModelPreview, PreviewScene};
use cgmath::{Deg, Euler, InnerSpace, Matrix4, Quaternion, Rad};
use eframe::egui;
use shock2vr::{
    Handedness,
    scenes::debug_interactions::INTERACTION_FIXTURES,
    vr_grip::{BakedGripEntry, FingerPose, GRIP_RESOURCES, GripHints, GripLibrary, ResolvedGrip},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::PathBuf,
};

pub(crate) const POSE_PRESETS: [(&str, [f32; 5]); 6] = [
    ("Open", [0.0; 5]),
    ("Point", [0.85, 0.0, 0.9, 0.9, 0.9]),
    ("Closed", [1.0; 5]),
    ("Cylindrical", [0.65, 0.65, 0.7, 0.7, 0.7]),
    ("Ball", [0.35, 0.25, 0.3, 0.35, 0.4]),
    ("Thumbs up", [-0.5, 1.0, 1.0, 1.0, 1.0]),
];

/// Shared Rest/pressed authoring for primary and support finger poses.
#[derive(Default)]
pub(crate) struct CurlPoseEditor {
    pressed: bool,
    pub preview: f32,
}
impl CurlPoseEditor {
    /// The finger mode and its curls. A VR snapshot shows its estimated curls
    /// read-only until converted to curls.
    pub fn show_fingers(
        &mut self,
        ui: &mut egui::Ui,
        fingers: &mut FingerPose,
        preview_label: &str,
    ) {
        if let FingerPose::Bones { .. } = fingers {
            ui.label("Mode: VR snapshot");
            if ui
                .button("Convert fingers to curls")
                .on_hover_text("Replace the captured bones with these estimated curls")
                .clicked()
            {
                *fingers = fingers.to_curls();
            }
        } else {
            ui.label("Mode: Curls");
        }
        match fingers {
            FingerPose::Curls {
                curls,
                trigger_curls,
            } => self.show(ui, curls, trigger_curls, preview_label, true),
            FingerPose::Bones {
                bones,
                trigger_bones,
            } => {
                let estimate = shock2vr::vr_grip::bones_to_curls;
                let (mut rest, mut pressed) =
                    (estimate(bones), trigger_bones.as_ref().map(estimate));
                self.show(ui, &mut rest, &mut pressed, preview_label, false);
            }
        }
    }

    /// Rest/pressed curl sliders; `live: false` shows them disabled.
    fn show(
        &mut self,
        ui: &mut egui::Ui,
        rest: &mut [f32; 5],
        pressed: &mut Option<[f32; 5]>,
        preview_label: &str,
        live: bool,
    ) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_value(&mut self.pressed, false, "Rest")
                .clicked()
            {
                self.preview = 0.0;
            }
            if ui
                .selectable_value(&mut self.pressed, true, "Trigger pressed")
                .clicked()
            {
                self.preview = 1.0;
            }
        });
        if self.pressed && live {
            if pressed.is_none() {
                ui.small("Pressed uses Rest until customized.");
                if ui.button("Customize pressed pose").clicked() {
                    *pressed = Some(*rest);
                }
            } else if ui.button("Use Rest for both").clicked() {
                *pressed = None;
            }
        }
        let editable = live && (!self.pressed || pressed.is_some());
        let curls = if self.pressed {
            pressed.as_mut().unwrap_or(rest)
        } else {
            rest
        };
        ui.add_enabled_ui(editable, |ui| {
            for (name, value) in ["Thumb", "Index", "Middle", "Ring", "Pinky"]
                .into_iter()
                .zip(curls.iter_mut())
            {
                let min = if name == "Thumb" {
                    shock2vr::vr_grip::MIN_THUMB_CURL
                } else {
                    0.0
                };
                tweak_slider(ui, name, value, min..=1.0, 0.02, 2);
            }
            ui.small("Negative thumb values extend outward beyond the open pose.");
            ui.horizontal_wrapped(|ui| {
                for (name, values) in POSE_PRESETS {
                    if ui.button(name).clicked() {
                        *curls = values;
                    }
                }
            });
        });
        ui.label(preview_label);
        ui.add(egui::Slider::new(&mut self.preview, 0.0..=1.0));
    }
}

pub fn default_library_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/vr-grips.json")
}

/// Each override replaces the pose the runtime would pick for its model and
/// hand (keeping that entry's provenance), or joins the pickup library. A
/// glove weapon posed for one hand only also replaces the other hand's,
/// mirrored, as play holds it before the overrides are cleared.
fn merge_overrides(libraries: &mut [GripLibrary], overrides: GripLibrary) -> Result<(), String> {
    let posed = |model: &str, hand: &str| {
        overrides
            .entries
            .iter()
            .any(|e| e.model == model && e.hand == hand)
    };
    let mut merges = Vec::new();
    for entry in &overrides.entries {
        merges.push(entry.clone());
        let (other, other_name) = match entry.hand.as_str() {
            "left" => (Handedness::Right, "right"),
            _ => (Handedness::Left, "left"),
        };
        if let Some(mirror) = shock2vr::vr_weapon_grip::named_model_mirror(&entry.model)
            .filter(|_| !posed(&entry.model, other_name))
        {
            let grip = shock2vr::vr_grip::hand_model_mirror(mirror, other)
                .and_then(|mirror| entry.grip.mirrored(mirror))
                .ok_or("Invalid model reflection or grip")?;
            merges.push(BakedGripEntry {
                hand: other_name.to_owned(),
                grip,
                ..entry.clone()
            });
        }
    }
    for entry in merges {
        let existing = libraries.iter_mut().find_map(|library| {
            library
                .entries
                .iter_mut()
                .find(|e| e.model == entry.model && e.hand == entry.hand)
        });
        match existing {
            Some(target) => transfer_grip(&entry.grip, target, None)?,
            None => libraries[0].entries.push(BakedGripEntry {
                authored: true,
                ..entry
            }),
        }
    }
    Ok(())
}

pub fn merge_override_file(path: PathBuf) -> Result<(), String> {
    let dir = default_library_path()
        .parent()
        .ok_or("Missing assets directory")?
        .to_owned();
    merge_override_file_into(path, &dir)
}

/// Merge the overrides file into the grip and support resources in `dir`.
fn merge_override_file_into(path: PathBuf, dir: &std::path::Path) -> Result<(), String> {
    let file = shock2vr::vr_grip::GripOverrides::parse(
        &std::fs::read_to_string(&path).map_err(|e| e.to_string())?,
    )?;
    // A partial merge into the shipped assets would silently drop poses.
    let unread = file.unread.entries.len() + file.unread.support.len();
    if unread > 0 {
        return Err(format!("{unread} overrides this build cannot read"));
    }
    validate(&file.grips)?;
    let (overrides, support) = (file.grips, file.support);
    let count = overrides.entries.len();
    let mut docs = GRIP_RESOURCES
        .iter()
        .map(|name| GripDocument::load(dir.join(name)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut libraries: Vec<_> = docs.iter().map(|doc| doc.library.clone()).collect();
    merge_overrides(&mut libraries, overrides)?;
    // Merged in memory before anything is written, so a bad pose writes
    // nothing.
    let support_doc =
        crate::support_grip_editor::merged_support(dir.join("vr-support-grips.json"), &support)?;
    for (doc, library) in docs.iter_mut().zip(libraries) {
        doc.library = library;
    }
    docs.retain(GripDocument::dirty);
    // Every save refuses an externally edited file: check them all first.
    if let Some(doc) = &support_doc {
        doc.check_unchanged()?;
    }
    for doc in &docs {
        doc.check_unchanged()?;
    }
    if let Some(mut doc) = support_doc {
        doc.save()?;
        println!("updated {}", doc.path.display());
    }
    for doc in &mut docs {
        doc.save()?;
        println!("updated {}", doc.path.display());
    }
    println!(
        "merged {count} grip overrides and {} support poses",
        support.len()
    );
    Ok(())
}

/// A version 1 library of valid grips, one per model and hand.
fn validate(library: &GripLibrary) -> Result<(), String> {
    if library.version != 1 {
        return Err("Unsupported grip resource version".into());
    }
    let mut keys = BTreeSet::new();
    for e in &library.entries {
        if !matches!(e.hand.as_str(), "left" | "right")
            || !e.grip.is_valid()
            || !keys.insert((e.model.clone(), e.hand.clone()))
        {
            return Err("Invalid or duplicate prepared grip entry".into());
        }
    }
    Ok(())
}

pub struct GripDocument {
    pub library: GripLibrary,
    path: PathBuf,
    saved_bytes: Vec<u8>,
    saved: GripLibrary,
}

impl GripDocument {
    pub fn load(path: PathBuf) -> Result<Self, String> {
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(path)
        };
        let saved_bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let library: GripLibrary =
            serde_json::from_slice(&saved_bytes).map_err(|e| e.to_string())?;
        validate(&library)?;
        Ok(Self {
            saved: library.clone(),
            library,
            path,
            saved_bytes,
        })
    }

    pub fn dirty(&self) -> bool {
        serde_json::to_vec(&self.library).unwrap() != serde_json::to_vec(&self.saved).unwrap()
    }

    pub fn save(&mut self) -> Result<(), String> {
        self.check_unchanged()?;
        self.write(self.path.clone(), false)
    }

    /// Save refuses a file edited on disk since it was loaded or saved.
    pub fn check_unchanged(&self) -> Result<(), String> {
        if std::fs::read(&self.path).map_err(|e| e.to_string())? != self.saved_bytes {
            return Err(
                "Resource changed on disk. Use Save As to keep your drafts, or reopen to load it."
                    .into(),
            );
        }
        Ok(())
    }

    /// Save As never replaces an existing file, including the current file.
    /// Future Save commands target the successfully written new path.
    pub fn save_as(&mut self, path: PathBuf) -> Result<(), String> {
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(path)
        };
        self.write(path, true)
    }

    fn write(&mut self, path: PathBuf, new_file: bool) -> Result<(), String> {
        if self.library.entries.iter().any(|e| !e.grip.is_valid()) {
            return Err("Cannot save an invalid pose".into());
        }
        let mut bytes = serde_json::to_vec_pretty(&self.library).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        persist_json(&path, &bytes, new_file)?;
        self.path = path;
        self.saved_bytes = bytes;
        self.saved = self.library.clone();
        Ok(())
    }

    fn restore(&mut self, index: usize) {
        let current = &self.library.entries[index];
        if let Some(saved) = self
            .saved
            .entries
            .iter()
            .find(|e| e.model == current.model && e.hand == current.hand)
        {
            self.library.entries[index] = saved.clone();
        }
    }
}

pub(crate) fn persist_json(
    path: &std::path::Path,
    bytes: &[u8],
    new_file: bool,
) -> Result<(), String> {
    let mut temp =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("Missing parent directory")?)
            .map_err(|e| e.to_string())?;
    temp.write_all(bytes)
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    if new_file {
        temp.persist_noclobber(path)
    } else {
        temp.persist(path)
    }
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Copies authoring values only; destination identity and validation hashes stay intact.
fn transfer_grip(
    source: &ResolvedGrip,
    target: &mut BakedGripEntry,
    mirror: Option<Matrix4<f32>>,
) -> Result<(), String> {
    let mut grip = source.clone();
    if let Some(model_mirror) = mirror {
        grip = grip
            .mirrored(model_mirror)
            .ok_or("Invalid model reflection or grip")?;
    }
    grip.contacts = [None; 5];
    grip.score = 0.0;
    if !grip.is_valid() {
        return Err("Cannot transfer an invalid pose".into());
    }
    target.grip = grip;
    target.authored = true;
    Ok(())
}

fn copy_model_poses(library: &GripLibrary, model: &str) -> Result<[ResolvedGrip; 2], String> {
    let pose = |hand| {
        library.lookup(model, hand).cloned().ok_or_else(|| {
            format!("Prepare a valid {hand} pose for {model} before copying both hands")
        })
    };
    Ok([pose("left")?, pose("right")?])
}

fn paste_model_poses(
    poses: &[ResolvedGrip; 2],
    library: &mut GripLibrary,
    model: &str,
) -> Result<(), String> {
    // Stage both transfers so a missing/invalid second hand cannot leave the
    // first hand modified. Never copy source identity or fitting provenance.
    let mut replacements = Vec::new();
    for (hand, pose) in ["left", "right"].into_iter().zip(poses) {
        let index = library
            .entries
            .iter()
            .position(|e| e.model == model && e.hand == hand)
            .ok_or_else(|| {
                format!("Prepare the {hand} draft for {model} before pasting both hands")
            })?;
        let mut target = library.entries[index].clone();
        transfer_grip(pose, &mut target, None)?;
        replacements.push((index, target));
    }
    for (index, target) in replacements {
        library.entries[index] = target;
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
pub enum VrSetupMode {
    Hands,
    Belt,
    Holster,
}

pub struct GripEditor {
    pub mode: VrSetupMode,
    pub holster_editor: crate::holster_editor::HolsterEditor,
    pub belt_editor: crate::belt_card_editor::BeltCardEditor,
    support_editor: crate::support_grip_editor::SupportEditor,
    support_mode: bool,
    curl_editor: CurlPoseEditor,
    clipboard: Option<(String, [ResolvedGrip; 2])>,
    document: Result<GripDocument, String>,
    hints: Result<BTreeMap<String, GripHints>, String>,
    model: String,
    hand: String,
    search: String,
    view: String,
    camera_pending: bool,
    validated: Option<(String, String)>,
    hashes: Option<(String, String)>,
    message: String,
    family: Option<u8>,
    rotation_edit: Option<(Quaternion<f32>, [f32; 3])>,
    rotation_step: f32,
    save_as_path: Option<String>,
    confirm_close: bool,
    allow_close: bool,
    prepare_missing: bool,
    fitting: Option<std::sync::mpsc::Receiver<Result<Vec<BakedGripEntry>, String>>>,
    fitting_model: String,
}

impl GripEditor {
    pub fn new(
        path: Option<PathBuf>,
        model: Option<String>,
        hand: String,
        view: String,
        support_mode: bool,
        support_path: Option<PathBuf>,
    ) -> Self {
        // Hints are the same authoring inputs used by gameplay and the baker.
        let hints_path = default_library_path().with_file_name("astra-vr-grip-hints.json");
        let hints = std::fs::read(hints_path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()));
        let default_path = if model
            .as_ref()
            .is_some_and(|m| shock2vr::vr_magazine::grip_source(m).is_some())
        {
            default_library_path().with_file_name("vr-magazine-grips.json")
        } else if model
            .as_ref()
            .is_some_and(|m| shock2vr::tricorder::is_model(m))
        {
            default_library_path().with_file_name("vr-tricorder-grips.json")
        } else if model
            .as_ref()
            .is_some_and(|m| shock2vr::vr_weapon_grip::supports_model(m))
        {
            default_library_path().with_file_name("vr-weapon-grips.json")
        } else {
            default_library_path()
        };
        Self {
            mode: VrSetupMode::Hands,
            holster_editor: crate::holster_editor::HolsterEditor::new(None, None),
            belt_editor: crate::belt_card_editor::BeltCardEditor::new(None, true),
            support_editor: crate::support_grip_editor::SupportEditor::new(support_path),
            support_mode,
            curl_editor: CurlPoseEditor::default(),
            clipboard: None,
            document: GripDocument::load(path.unwrap_or(default_path)),
            hints,
            model: model.unwrap_or_else(|| "mug".into()),
            hand,
            search: String::new(),
            view,
            camera_pending: true,
            validated: None,
            hashes: None,
            message: String::new(),
            family: None,
            rotation_edit: None,
            rotation_step: 5.0,
            save_as_path: None,
            confirm_close: false,
            allow_close: false,
            prepare_missing: true,
            fitting: None,
            fitting_model: String::new(),
        }
    }

    pub fn open_model(&mut self, key: &str) {
        self.mode = VrSetupMode::Hands;
        self.model = std::path::Path::new(key)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if shock2vr::vr_weapon_grip::supports_model(&self.model) {
            if let (Ok(doc), Ok(defaults)) = (
                &mut self.document,
                GripDocument::load(default_library_path().with_file_name("vr-weapon-grips.json")),
            ) {
                for entry in defaults
                    .library
                    .entries
                    .into_iter()
                    .filter(|e| e.model == self.model)
                {
                    if !doc
                        .library
                        .entries
                        .iter()
                        .any(|e| e.model == entry.model && e.hand == entry.hand)
                    {
                        doc.library.entries.push(entry);
                    }
                }
            }
        }
        self.prepare_missing = true;
        self.validated = None;
        self.camera_pending = true;
        self.rotation_edit = None;
        self.message.clear();
    }

    pub fn is_busy(&self) -> bool {
        self.fitting.is_some()
    }

    fn start_fit(
        &mut self,
        preview: &mut ModelPreview,
        hands: Vec<String>,
        family: Option<u8>,
        authored: bool,
    ) {
        if self.is_busy() || hands.is_empty() {
            return;
        }
        let Ok(hints) = &self.hints else {
            return;
        };
        let mut hints = hints.get(&self.model).cloned().unwrap_or_default();
        if shock2vr::vr_magazine::grip_source(&self.model).is_some() {
            hints.keep_upright = true;
        }
        let hints_hash = hints.fingerprint();
        if family.is_some() {
            hints.pose_family = family;
        }
        let key = format!("{}.bin", self.model);
        let mut inputs = Vec::new();
        for name in hands {
            let hand = if name == "left" {
                Handedness::Left
            } else {
                Handedness::Right
            };
            let scale = match preview.magazine_grip_scale(&key, hand) {
                Ok(scale) => scale,
                Err(e) => {
                    self.message = e.into();
                    return;
                }
            };
            match preview.grip_inputs(&key, hand) {
                Ok((surface, rig, hash, guide)) => {
                    inputs.push((name, surface, rig, hash, guide, scale))
                }
                Err(e) => {
                    self.message = e;
                    return;
                }
            }
        }
        let model = self.model.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.fitting = Some(rx);
        self.fitting_model = model.clone();
        self.message = format!("Fitting {model}… You can continue inspecting the preview.");
        std::thread::spawn(move || {
            let result = inputs
                .into_iter()
                .map(|(hand, surface, rig, surface_hash, guide, scale)| {
                    let resolved = if let Some((triangles, arms)) = guide {
                        let side = if hand == "left" {
                            Handedness::Left
                        } else {
                            Handedness::Right
                        };
                        shock2vr::vr_weapon_grip::resolve(&model, side, &triangles, &arms, &rig)
                    } else {
                        surface.resolve(&rig, &hints)
                    };
                    let mut grip = resolved.ok_or_else(|| {
                        format!("No valid {hand} fit for {model}; existing drafts kept")
                    })?;
                    if let Some(scale) = scale {
                        grip = grip.with_item_scale(scale);
                    }
                    Ok(BakedGripEntry {
                        model: model.clone(),
                        hand,
                        surface_hash,
                        kinematics_hash: rig.fingerprint(),
                        hints_hash: hints_hash.clone(),
                        grip,
                        authored,
                    })
                })
                .collect::<Result<Vec<_>, String>>();
            let _ = tx.send(result);
        });
    }

    fn poll_fit(&mut self, ctx: &egui::Context) {
        let Some(receiver) = &self.fitting else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
                return;
            }
            Err(_) => Err("Fitting worker stopped; existing drafts kept".into()),
        };
        self.fitting = None;
        match result {
            Ok(entries) => {
                if let Ok(doc) = &mut self.document {
                    for entry in entries {
                        if let Some(old) = doc
                            .library
                            .entries
                            .iter_mut()
                            .find(|e| e.model == entry.model && e.hand == entry.hand)
                        {
                            *old = entry;
                        } else {
                            doc.library.entries.push(entry);
                        }
                    }
                }
                self.validated = None;
                self.rotation_edit = None;
                self.camera_pending = true;
                self.message = format!(
                    "{} drafts ready. Review, then Save or Save As.",
                    self.fitting_model
                );
            }
            Err(e) => self.message = e,
        }
    }

    pub fn error(&self) -> Option<&str> {
        if self.mode == VrSetupMode::Holster {
            return self.holster_editor.error();
        }
        if self.mode == VrSetupMode::Belt {
            return self.belt_editor.error();
        }
        if matches!(self.model.as_str(), "amp_h" | "amp_w") {
            return None;
        }
        match (&self.document, &self.hints) {
            (Err(e), _) | (_, Err(e)) => Some(e),
            (Ok(doc), _)
                if !doc
                    .library
                    .entries
                    .iter()
                    .any(|e| e.model == self.model && e.hand == self.hand) =>
            {
                Some("No prepared grip for the selected model and hand")
            }
            _ => None,
        }
    }

    pub fn guard_close(&mut self, ctx: &egui::Context) {
        self.poll_fit(ctx);
        let busy = self.is_busy();
        let primary_dirty = self.document.as_ref().is_ok_and(|doc| doc.dirty());
        if !self.allow_close
            && (primary_dirty
                || self.support_editor.dirty()
                || self.belt_editor.dirty()
                || self.holster_editor.dirty()
                || busy)
            && ctx.input(|i| i.viewport().close_requested())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        if self.confirm_close {
            egui::Modal::new(egui::Id::new("unsaved_grips")).show(ctx, |ui| {
                ui.heading("Unsaved VR edits");
                ui.label(if busy {
                    "Fitting is still running. Keep editing to wait, or discard and close."
                } else {
                    "Save your drafts before closing?"
                });
                ui.horizontal(|ui| {
                    if ui.button("Keep editing").clicked() {
                        self.confirm_close = false;
                    }
                    if ui
                        .add_enabled(!busy, egui::Button::new("Save and close"))
                        .clicked()
                    {
                        let primary_saved = match &mut self.document {
                            Ok(doc) if doc.dirty() => doc.save(),
                            _ => Ok(()),
                        };
                        match primary_saved
                            .and_then(|_| self.support_editor.save())
                            .and_then(|_| self.belt_editor.save())
                            .and_then(|_| self.holster_editor.save())
                        {
                            Ok(()) => self.allow_close = true,
                            Err(e) => self.message = e,
                        }
                    }
                    if ui.button("Discard and close").clicked() {
                        self.allow_close = true;
                    }
                });
                if !self.message.is_empty() {
                    ui.label(&self.message);
                }
            });
            if self.allow_close {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    pub fn show_list(&mut self, ui: &mut egui::Ui) {
        ui.heading("VR Setup");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.mode, VrSetupMode::Hands, "Hand grips");
            ui.selectable_value(&mut self.mode, VrSetupMode::Belt, "Belt tool");
            ui.selectable_value(&mut self.mode, VrSetupMode::Holster, "Holster");
        });
        if self.mode == VrSetupMode::Belt {
            ui.label("Tricorder / personal access card");
            ui.label("Adjust the resting tool pose against the battle belt and player collision capsule. Save the pose as an asset for gameplay.");
            return;
        }
        if self.mode == VrSetupMode::Holster {
            self.holster_editor.show_list(ui);
            return;
        }
        ui.label("Grip overrides · shared glove rig");
        let can_switch = !self.is_busy() && self.document.as_ref().is_ok_and(|doc| !doc.dirty());
        let mut switch = None;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(can_switch, egui::Button::new("Pickups"))
                .clicked()
            {
                switch = Some(("vr-grips.json", "mug"));
            }
            if ui
                .add_enabled(can_switch, egui::Button::new("Tricorder"))
                .clicked()
            {
                switch = Some(("vr-tricorder-grips.json", "tricorder"));
            }
            if ui
                .add_enabled(can_switch, egui::Button::new("Weapons"))
                .clicked()
            {
                switch = Some(("vr-weapon-grips.json", "atek_h"));
            }
            if ui
                .add_enabled(can_switch, egui::Button::new("Magazines"))
                .clicked()
            {
                switch = Some(("vr-magazine-grips.json", "atek_h_magazine"));
            }
        });
        if let Some((file, model)) = switch {
            match GripDocument::load(default_library_path().with_file_name(file)) {
                Ok(doc) => {
                    self.document = Ok(doc);
                    self.open_model(model);
                }
                Err(error) => {
                    self.message = format!("Could not switch library: {error}");
                }
            }
        }
        ui.text_edit_singleline(&mut self.search);
        let Ok(doc) = &self.document else {
            return;
        };
        let mut models: BTreeSet<_> = doc
            .library
            .entries
            .iter()
            .map(|e| e.model.as_str())
            .collect();
        if doc
            .path
            .file_name()
            .is_some_and(|name| name == "vr-magazine-grips.json")
            || shock2vr::vr_magazine::grip_source(&self.model).is_some()
        {
            models.extend(["atek_h_magazine", "ar15_h_magazine"]);
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for model in models {
                let labels = INTERACTION_FIXTURES
                    .iter()
                    .filter(|f| f.model == model)
                    .map(|f| f.label)
                    .collect::<Vec<_>>()
                    .join(", ");
                let text = format!("{model}  {labels}");
                if !text.to_lowercase().contains(&self.search.to_lowercase()) {
                    continue;
                }
                if ui.selectable_label(self.model == model, text).clicked() {
                    self.model = model.to_owned();
                    self.prepare_missing = true;
                    self.validated = None;
                    self.camera_pending = true;
                    self.message.clear();
                    self.family = None;
                }
            }
        });
        ui.separator();
        ui.label(
            "Primary grips affect every item using this model. Left and right primary hands save independently.",
        );
        ui.label("Weapon grips replace authored hands with the glove. Psi amp retains its integrated forearm.");
        ui.small(format!("Primary grip resource: {}", doc.path.display()));
        if !can_switch {
            ui.small("Save or revert drafts before switching libraries.");
        }
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        frame: &mut eframe::Frame,
        preview: &mut ModelPreview,
    ) {
        if self.mode == VrSetupMode::Belt {
            self.belt_editor.show(ui, frame, preview);
            return;
        }
        if self.mode == VrSetupMode::Holster {
            self.holster_editor.show(ui, frame, preview);
            return;
        }
        if ui.button("Edit holster pose").clicked() {
            self.holster_editor.open_model(&self.model);
            self.mode = VrSetupMode::Holster;
            return;
        }
        if matches!(self.model.as_str(), "amp_h" | "amp_w") {
            ui.heading("Psi amp — integrated forearm reference");
            ui.label("The psi amp retains its authored forearm and does not use a glove override.");
            preview.show(ui, frame, "amp_h.bin", &PreviewScene::VrReference);
            return;
        }
        if let (Err(error), _) | (_, Err(error)) = (&self.document, &self.hints) {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
            return;
        }
        self.poll_fit(ui.ctx());
        if self.prepare_missing && !self.is_busy() {
            self.prepare_missing = false;
            let doc = self.document.as_ref().unwrap();
            let missing = ["left", "right"]
                .into_iter()
                .filter(|hand| {
                    !doc.library
                        .entries
                        .iter()
                        .any(|e| e.model == self.model && e.hand == *hand)
                })
                .map(str::to_string)
                .collect();
            self.start_fit(preview, missing, None, true);
        }
        let busy = self.is_busy();
        if busy {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
        let magazine = shock2vr::vr_magazine::grip_source(&self.model).is_some();
        let mut request_fit = false;
        let doc = self.document.as_mut().unwrap();
        ui.heading(format!("{} — grip override", self.model));
        if shock2vr::tricorder::is_model(&self.model) {
            self.support_mode = false;
            ui.label("Phone grip: support the back and curl fingers around an edge. The green rear lens emits along -Z. Save, then restart the game to load both hand poses.");
        }
        if magazine {
            self.support_mode = false;
            ui.label("Extracted pouch magazine. Adjust each hand's grip, then Save and restart the game. Size follows the weapon held in the other hand.");
        }
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.support_mode, false, "Primary grip");
            if !shock2vr::tricorder::is_model(&self.model) && !magazine {
                ui.selectable_value(&mut self.support_mode, true, "Support grip");
            }
        });
        ui.horizontal(|ui| {
            ui.label("Primary hand");
            for hand in ["left", "right"] {
                if ui
                    .selectable_value(&mut self.hand, hand.to_string(), hand)
                    .changed()
                {
                    self.validated = None;
                    self.camera_pending = true;
                    self.message.clear();
                }
            }
            ui.separator();
            for view in ["front", "back", "top", "oblique", "palm"] {
                if ui
                    .selectable_value(&mut self.view, view.to_string(), view)
                    .clicked()
                {
                    self.camera_pending = true;
                }
            }
        });
        let Some(index) = doc
            .library
            .entries
            .iter()
            .position(|e| e.model == self.model && e.hand == self.hand)
        else {
            if !self.message.is_empty() {
                ui.label(&self.message);
            }
            ui.label("No prepared grip for this hand yet.");
            if !busy && ui.button("Prepare left and right drafts").clicked() {
                self.prepare_missing = true;
            }
            preview.show(
                ui,
                frame,
                &format!("{}.bin", self.model),
                &PreviewScene::Model,
            );
            return;
        };
        let hand = if self.hand == "left" {
            Handedness::Left
        } else {
            Handedness::Right
        };
        let key = format!("{}.bin", self.model);
        if magazine {
            match preview.magazine_grip_scale(&key, hand) {
                Ok(Some(scale)) => doc.library.entries[index].grip.item_scale = scale,
                Ok(None) => unreachable!(),
                Err(error) => {
                    ui.label(error);
                    return;
                }
            }
        }
        let identity = (self.model.clone(), self.hand.clone());
        if self.validated.as_ref() != Some(&identity) {
            self.hashes = match preview.grip_inputs(&key, hand) {
                Ok((_, rig, surface, _)) => Some((surface, rig.fingerprint())),
                Err(e) => {
                    self.message = e;
                    None
                }
            };
            self.validated = Some(identity);
        }
        if self.support_mode {
            if doc.dirty() {
                ui.label("Primary grip also has unsaved edits. Primary grip → Save all edits saves both resources.");
            }
            let entry = &doc.library.entries[index];
            let model_mirror = match preview.grip_model_mirror(&key) {
                Ok(mirror) => mirror,
                Err(error) => {
                    ui.label(error);
                    return;
                }
            };
            self.support_editor.show(
                ui,
                &self.model,
                hand,
                entry.grip.item_scale,
                model_mirror,
                !busy,
            );
            let mut visual_grip = entry.grip.clone();
            visual_grip.fingers = entry.grip.fingers.at(self.curl_editor.preview);
            let scene = PreviewScene::Grip(
                hand,
                visual_grip,
                self.support_editor.preview_profile(&self.model),
            );
            preview.show_grip(
                ui,
                frame,
                &key,
                &scene,
                self.camera_pending.then_some((&self.view, hand)),
            );
            self.camera_pending = false;
            return;
        }
        ui.horizontal(|ui| {
            let label = if doc.dirty() || self.support_editor.dirty() {
                "Save all edits *"
            } else {
                "Saved"
            };
            if ui
                .add_enabled(
                    (doc.dirty() || self.support_editor.dirty()) && !busy,
                    egui::Button::new(label),
                )
                .clicked()
            {
                self.message = match (if doc.dirty() { doc.save() } else { Ok(()) })
                    .and_then(|_| self.support_editor.save())
                {
                    Ok(()) => "Saved. Restart the game runtime to load the resource.".into(),
                    Err(e) => e,
                };
            }
            if ui
                .add_enabled(!busy, egui::Button::new("Save As…"))
                .clicked()
            {
                self.save_as_path = Some(
                    doc.path
                        .with_file_name("vr-grips.custom.json")
                        .display()
                        .to_string(),
                );
            }
            let has_saved = doc
                .saved
                .entries
                .iter()
                .any(|e| e.model == self.model && e.hand == self.hand);
            if ui
                .add_enabled(
                    has_saved && !busy,
                    egui::Button::new("Revert this hand to saved"),
                )
                .clicked()
            {
                doc.restore(index);
            }
            ui.label(if doc.library.entries[index].authored {
                "Manual override"
            } else {
                "Automatic fit"
            });
        });
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Copy all poses"))
                .on_hover_text("Copy left and right hands, including each hand's rest and trigger-pressed pose")
                .clicked() {
                match copy_model_poses(&doc.library, &self.model) {
                    Ok(poses) => {
                        self.clipboard = Some((self.model.clone(), poses));
                        self.message = format!("Copied {}: both hands, including trigger poses", self.model);
                    }
                    Err(error) => self.message = error,
                }
            }
            if ui.add_enabled(!busy && self.clipboard.is_some(), egui::Button::new("Paste all poses"))
                .on_hover_text("Replace both hands and their trigger poses; left stays left and right stays right")
                .clicked() {
                let (model, poses) = self.clipboard.as_ref().unwrap();
                self.message = match paste_model_poses(poses, &mut doc.library, &self.model) {
                    Ok(()) => format!("Pasted both hands and trigger poses from {model}. Review the fit, then save."),
                    Err(error) => error,
                };
            }
            let opposite = if self.hand == "right" { "left" } else { "right" };
            if ui.add_enabled(!busy, egui::Button::new(format!("Mirror to {opposite} hand"))).clicked() {
                let other = if hand == Handedness::Right { Handedness::Left } else { Handedness::Right };
                let result = preview.grip_inputs(&key, other).and_then(|(_, rig, surface, _)| {
                    let mirror = preview.grip_model_mirror(&key)?;
                    let mut target = doc.library.entries[index].clone();
                    target.hand = opposite.into();
                    target.surface_hash = surface;
                    target.kinematics_hash = rig.fingerprint();
                    transfer_grip(&doc.library.entries[index].grip, &mut target, Some(mirror))?;
                    Ok(target)
                });
                match result {
                    Ok(target) => {
                        if let Some(existing) = doc.library.entries.iter_mut().find(|e| e.model == self.model && e.hand == opposite) {
                            *existing = target;
                        } else {
                            doc.library.entries.push(target);
                        }
                        self.message = format!("Mirrored to {opposite} hand. Switch hands to review; Save all edits keeps both.");
                    }
                    Err(e) => self.message = e,
                }
            }
            if let Some((model, _)) = &self.clipboard { ui.small(format!("Copied: {model} · both hands + trigger poses")); }
        });
        egui::CollapsingHeader::new("Automatic fitting").show(ui, |ui| {
            ui.horizontal(|ui| {
                if shock2vr::vr_weapon_grip::supports_model(&self.model) {
                    ui.label("Fit near the authored weapon grip");
                } else {
                    egui::ComboBox::from_id_salt("grip_family")
                        .selected_text(match self.family {
                            Some(0) => "Cylindrical",
                            Some(1) => "Pinch",
                            Some(2) => "Broad grasp",
                            Some(3) => "Trigger",
                            _ => "Model defaults",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.family, None, "Model defaults");
                            for (i, name) in ["Cylindrical", "Pinch", "Broad grasp", "Trigger"]
                                .iter()
                                .enumerate()
                            {
                                ui.selectable_value(&mut self.family, Some(i as u8), *name);
                            }
                        });
                }
                if ui
                    .add_enabled(
                        self.hashes.is_some() && !busy,
                        egui::Button::new("Replace draft with auto fit"),
                    )
                    .clicked()
                {
                    request_fit = true;
                }
            });
        });
        let entry = &mut doc.library.entries[index];
        let before = entry.grip.clone();
        // Keep entered Euler angles stable while dragging across a singularity;
        // only re-project when the pose changes outside these sliders.
        if self
            .rotation_edit
            .as_ref()
            .is_none_or(|(q, _)| *q != entry.grip.rotation)
        {
            let angles: Euler<Rad<f32>> = entry.grip.rotation.into();
            self.rotation_edit = Some((
                entry.grip.rotation,
                [
                    angles.x.0.to_degrees(),
                    angles.y.0.to_degrees(),
                    angles.z.0.to_degrees(),
                ],
            ));
        }
        ui.add_enabled_ui(!busy, |ui| {
            ui.columns(3, |columns| {
                columns[0].strong("Position (cm)");
                for (axis, value) in ["X", "Y", "Z"].into_iter().zip([
                    &mut entry.grip.offset.x,
                    &mut entry.grip.offset.y,
                    &mut entry.grip.offset.z,
                ]) {
                    let mut cm = *value * shock2vr::METERS_PER_WORLD_UNIT * 100.0;
                    if tweak_slider(&mut columns[0], axis, &mut cm, -50.0..=50.0, 0.1, 2) {
                        *value = cm / (shock2vr::METERS_PER_WORLD_UNIT * 100.0);
                    }
                }
                columns[1].strong("Rotation (degrees)");
                let (last, angles) = self.rotation_edit.as_mut().unwrap();
                let mut changed = false;
                for (axis, angle) in ["X", "Y", "Z"].into_iter().zip(angles.iter_mut()) {
                    changed |= tweak_slider(
                        &mut columns[1],
                        axis,
                        angle,
                        -180.0..=180.0,
                        self.rotation_step,
                        1,
                    );
                }
                if changed {
                    entry.grip.rotation = Quaternion::from(Euler::new(
                        Deg(angles[0]),
                        Deg(angles[1]),
                        Deg(angles[2]),
                    ))
                    .normalize();
                    *last = entry.grip.rotation;
                }
                columns[2].strong("Finger curls");
                self.curl_editor.show_fingers(
                    &mut columns[2],
                    &mut entry.grip.fingers,
                    "Primary trigger preview",
                );
            });
            ui.horizontal(|ui| {
                ui.label("Rotation nudge step");
                ui.add(egui::Slider::new(&mut self.rotation_step, 0.1..=15.0).suffix("°"));
                ui.separator();
                ui.label("Uniform item scale");
                ui.add_enabled_ui(!magazine, |ui| {
                    tweak_slider(ui, "×", &mut entry.grip.item_scale, 0.1..=3.0, 0.02, 2);
                });
            });
        });
        if before != entry.grip {
            entry.authored = true;
            // Contact diagnostics describe the old solver pose, not this edit.
            entry.grip.contacts = [None; 5];
            entry.grip.score = 0.0;
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        ui.small("Drag sliders or click values to type. Ball is a cupped starting pose; adjust curls to the item. Unsaved drafts stay when switching models.");
        // Build first, then select the requested camera so the initial model
        // framing cannot overwrite it. show() keeps the same camera on edits.
        let mut visual_grip = entry.grip.clone();
        visual_grip.fingers = entry.grip.fingers.at(self.curl_editor.preview);
        let scene = PreviewScene::Grip(
            hand,
            visual_grip,
            self.support_editor.preview_profile(&self.model),
        );
        preview.show_grip(
            ui,
            frame,
            &key,
            &scene,
            self.camera_pending.then_some((&self.view, hand)),
        );
        self.camera_pending = false;
        if let Some(path) = &mut self.save_as_path {
            let mut close = false;
            egui::Modal::new(egui::Id::new("save_grips_as")).show(ui.ctx(), |ui| {
                ui.heading("Save grips as JSON");
                ui.label("Write a new file. The original stays untouched.");
                ui.add(egui::TextEdit::singleline(path).desired_width(520.0));
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui.button("Save new file").clicked() {
                        self.message = match doc.save_as(PathBuf::from(path.trim())) {
                            Ok(()) => {
                                close = true;
                                format!("Saving to {}", doc.path.display())
                            }
                            Err(e) => format!("Could not save: {e}. Choose a new filename."),
                        };
                    }
                });
                if !self.message.is_empty() {
                    ui.label(&self.message);
                }
            });
            if close {
                self.save_as_path = None;
            }
        }
        if request_fit {
            self.start_fit(
                preview,
                vec![self.hand.clone()],
                self.family,
                self.family.is_some(),
            );
        }
    }
}

/// Each value offers a coarse slider, small nudges, and optional exact entry.
pub(crate) fn tweak_slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    decimals: usize,
) -> bool {
    let mut changed = false;
    ui.push_id(label, |ui| {
        ui.horizontal(|ui| {
            ui.label(label);
            if ui.small_button("−").clicked() {
                *value = (*value - step).max(*range.start());
                changed = true;
            }
            ui.spacing_mut().slider_width = 80.0;
            changed |= ui
                .add(
                    egui::Slider::new(value, range.clone())
                        .clamping(egui::SliderClamping::Edits)
                        .fixed_decimals(decimals),
                )
                .changed();
            if ui.small_button("+").clicked() {
                *value = (*value + step).min(*range.end());
                changed = true;
            }
        })
    });
    changed
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cgmath::Transform;

    /// Draws `show` headlessly and clicks the button labelled `label`;
    /// returns the texts drawn by the frame after the click.
    pub(crate) fn click_button(label: &str, mut show: impl FnMut(&mut egui::Ui)) -> Vec<String> {
        fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Rect)> {
            fn visit(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
                match shape {
                    egui::Shape::Text(text) => out.push((
                        text.galley.text().to_owned(),
                        text.galley.rect.translate(text.pos.to_vec2()),
                    )),
                    egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| visit(s, out)),
                    _ => {}
                }
            }
            let mut out = Vec::new();
            shapes.iter().for_each(|s| visit(&s.shape, &mut out));
            out
        }
        let ctx = egui::Context::default();
        let mut frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 900.0),
                )),
                ..Default::default()
            };
            texts(&ctx.run_ui(input, |ui| show(ui)).shapes)
        };
        let at = frame(Vec::new())
            .into_iter()
            .find(|(text, _)| text == label)
            .unwrap_or_else(|| panic!("{label} not drawn"))
            .1
            .center();
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![egui::Event::PointerMoved(at), button(true)]);
        frame(vec![button(false)]);
        frame(Vec::new())
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    }

    #[test]
    fn converting_a_vr_snapshot_keeps_its_placement_and_saves_curls() {
        let (_dir, mut doc) = document();
        let grip = &mut doc.library.entries[0].grip;
        grip.fingers = FingerPose::Curls {
            curls: [-0.5, 0.3, 0.6, 0.8, 1.0],
            trigger_curls: Some([0.2, 1.0, 0.6, 0.8, 1.0]),
        }
        .to_bones();
        let snapshot = grip.clone();
        let mut editor = CurlPoseEditor::default();
        let drawn = click_button("Convert fingers to curls", |ui| {
            editor.show_fingers(ui, &mut doc.library.entries[0].grip.fingers, "Preview")
        });
        assert!(drawn.iter().any(|t| t == "Mode: Curls"), "{drawn:?}");
        let converted = doc.library.entries[0].grip.clone();
        assert_eq!(converted.fingers, snapshot.fingers.to_curls());
        assert_eq!(
            ResolvedGrip {
                fingers: snapshot.fingers.clone(),
                ..converted.clone()
            },
            snapshot,
            "only the fingers change"
        );
        doc.save().unwrap();
        let restored = GripDocument::load(doc.path.clone()).unwrap();
        assert_eq!(restored.library.entries[0].grip, converted);
    }

    #[test]
    fn overrides_replace_the_entry_the_runtime_picks_or_join_pickups() {
        let (_dir, doc) = document();
        let shipped = doc.library.entries[0].clone();
        let mut weapons = GripLibrary::default();
        weapons.entries.push(BakedGripEntry {
            model: "pistol_h".into(),
            ..shipped.clone()
        });
        let mut libraries = vec![doc.library.clone(), weapons];
        let mut overrides = GripLibrary::default();
        let mut replaced = shipped.clone();
        replaced.surface_hash = "device".into();
        replaced.grip.item_scale = 0.5;
        let mut weapon = replaced.clone();
        weapon.model = "pistol_h".into();
        let mut added = replaced.clone();
        added.model = "new_item".into();
        overrides.entries = vec![replaced, weapon, added];
        merge_overrides(&mut libraries, overrides).unwrap();

        let pickup = &libraries[0].entries[0];
        assert_eq!(pickup.grip.item_scale, 0.5);
        assert_eq!(pickup.surface_hash, shipped.surface_hash, "provenance kept");
        assert!(pickup.authored);
        assert_eq!(libraries[1].entries[0].grip.item_scale, 0.5);
        assert_eq!(libraries[1].entries.len(), 1);
        let new = libraries[0].entries.last().unwrap();
        assert_eq!((new.model.as_str(), new.authored), ("new_item", true));
        assert_eq!(libraries[0].entries.len(), doc.library.entries.len() + 1);
    }

    #[test]
    fn a_headset_trigger_pose_survives_the_merge() {
        let (dir, doc) = document();
        let mut entry = doc.library.entries[0].clone();
        let pressed = FingerPose::curls([0.9; 5]).rest_bones();
        entry.grip.fingers = FingerPose::Bones {
            bones: entry.grip.fingers.rest_bones(),
            trigger_bones: Some(pressed),
        };
        // As the headset writes it: overrides file, then loaded for the merge.
        let path = dir.path().join("vr-grip-overrides.json");
        let mut overrides = shock2vr::vr_grip::GripOverrides::default();
        overrides.grips.entries.push(entry.clone());
        std::fs::write(&path, serde_json::to_vec(&overrides).unwrap()).unwrap();
        let mut libraries = vec![doc.library.clone()];
        merge_overrides(&mut libraries, GripDocument::load(path).unwrap().library).unwrap();
        let merged = libraries[0]
            .entries
            .iter()
            .find(|e| e.model == entry.model && e.hand == entry.hand)
            .unwrap();
        assert_eq!(merged.grip.fingers, entry.grip.fingers);
    }

    /// merge-grips writes the grip and support resources, or with an
    /// override it cannot read, nothing at all.
    #[test]
    fn merging_an_overrides_file_writes_all_or_nothing() {
        let assets = default_library_path().parent().unwrap().to_owned();
        let dir = tempfile::tempdir().unwrap();
        let names = GRIP_RESOURCES
            .iter()
            .copied()
            .chain(["vr-support-grips.json"]);
        for name in names.clone() {
            std::fs::copy(assets.join(name), dir.path().join(name)).unwrap();
        }
        let read_all = || {
            names
                .clone()
                .map(|n| std::fs::read(dir.path().join(n)).unwrap())
                .collect::<Vec<_>>()
        };
        let shipped = read_all();
        let weapons: GripLibrary =
            serde_json::from_str(include_str!("../../../assets/vr-weapon-grips.json")).unwrap();
        let mut entry = weapons.entries[0].clone();
        let scale = entry.grip.item_scale * 1.1;
        entry.grip = entry.grip.with_item_scale(scale);
        let support: BTreeMap<String, shock2vr::vr_support::SupportProfile> =
            serde_json::from_str(include_str!("../../../assets/vr-support-grips.json")).unwrap();
        let (model, profile) = support.iter().next().unwrap();
        let mut overrides = shock2vr::vr_grip::GripOverrides::default();
        overrides.grips.entries.push(entry);
        overrides.support.insert(
            model.clone(),
            shock2vr::vr_support::SupportPose {
                palm_anchor: profile.palm_anchor,
                rotation_degrees: [profile.rotation_degrees[0] + 5.0, 0.0, 0.0],
                fingers: profile.fingers.clone(),
            },
        );
        let mut file = serde_json::to_value(&overrides).unwrap();
        file["entries"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"model": "sg_h", "hand": "left"}));
        let path = dir.path().join("vr-grip-overrides.json");
        std::fs::write(&path, file.to_string()).unwrap();
        assert!(merge_override_file_into(path.clone(), dir.path()).is_err());
        assert_eq!(read_all(), shipped, "nothing written");

        std::fs::write(&path, serde_json::to_vec(&overrides).unwrap()).unwrap();
        merge_override_file_into(path, dir.path()).unwrap();
        let merged = read_all();
        let changed: Vec<_> = names
            .zip(merged.iter().zip(&shipped))
            .filter(|(_, (a, b))| a != b)
            .map(|(name, _)| name)
            .collect();
        assert_eq!(changed, ["vr-weapon-grips.json", "vr-support-grips.json"]);
    }

    #[test]
    fn a_glove_weapon_posed_for_one_hand_merges_mirrored_into_the_other() {
        let (_dir, doc) = document();
        let weapons: GripLibrary =
            serde_json::from_str(include_str!("../../../assets/vr-weapon-grips.json")).unwrap();
        let mut libraries = vec![doc.library.clone(), weapons];
        let entry = |libraries: &[GripLibrary], model: &str, hand: &str| {
            libraries
                .iter()
                .flat_map(|l| &l.entries)
                .find(|e| e.model == model && e.hand == hand)
                .unwrap()
                .clone()
        };
        let moved = |mut e: BakedGripEntry| {
            e.grip.offset.x += 0.05;
            e
        };
        // The shotgun and a mug posed right only; the wrench posed both hands.
        let shotgun = moved(entry(&libraries, "sg_h", "right"));
        let mug_left = entry(&libraries, "mug", "left");
        let wrench_left = moved(entry(&libraries, "wrench_h", "left"));
        let overrides = GripLibrary {
            entries: vec![
                shotgun.clone(),
                moved(entry(&libraries, "mug", "right")),
                moved(entry(&libraries, "wrench_h", "right")),
                wrench_left.clone(),
            ],
            ..Default::default()
        };
        merge_overrides(&mut libraries, overrides).unwrap();
        // The shotgun's left hand takes the right pose mirrored, as play does.
        let mirror = shock2vr::vr_weapon_grip::named_model_mirror("sg_h").unwrap();
        let expected = shotgun.grip.mirrored(mirror).unwrap();
        let left = entry(&libraries, "sg_h", "left");
        assert!(left.authored);
        assert_eq!(
            (left.grip.offset, left.grip.rotation),
            (expected.offset, expected.rotation)
        );
        // A prop is drawn alike in both hands: its left is untouched.
        assert_eq!(entry(&libraries, "mug", "left").grip, mug_left.grip);
        // A left pose of its own wins.
        assert_eq!(
            entry(&libraries, "wrench_h", "left").grip.offset,
            wrench_left.grip.offset
        );
    }

    fn curls_mut(grip: &mut ResolvedGrip) -> (&mut [f32; 5], &mut Option<[f32; 5]>) {
        match &mut grip.fingers {
            FingerPose::Curls {
                curls,
                trigger_curls,
            } => (curls, trigger_curls),
            FingerPose::Bones { .. } => panic!("expected a curl pose"),
        }
    }
    fn document() -> (tempfile::TempDir, GripDocument) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grips.json");
        std::fs::write(&path, include_bytes!("../../../assets/vr-grips.json")).unwrap();
        let doc = GripDocument::load(path).unwrap();
        (dir, doc)
    }

    #[test]
    fn thumbs_up_preset_saves_and_reloads_for_both_trigger_poses() {
        let (_dir, mut doc) = document();
        let thumbs_up = POSE_PRESETS
            .iter()
            .find(|(name, _)| *name == "Thumbs up")
            .unwrap()
            .1;
        *curls_mut(&mut doc.library.entries[0].grip).0 = thumbs_up;
        *curls_mut(&mut doc.library.entries[0].grip).1 = Some([-1.0, 0.8, 1.0, 1.0, 1.0]);
        doc.save().unwrap();
        let restored = GripDocument::load(doc.path.clone()).unwrap();
        assert_eq!(
            restored.library.entries[0].grip,
            doc.library.entries[0].grip
        );
    }

    #[test]
    fn magazine_opens_its_own_grip_library() {
        let editor = GripEditor::new(
            None,
            Some("atek_h_magazine".into()),
            "left".into(),
            "oblique".into(),
            false,
            None,
        );
        assert_eq!(
            editor.document.unwrap().path.file_name().unwrap(),
            "vr-magazine-grips.json"
        );
    }

    #[test]
    fn saved_magazine_pose_reaches_runtime_without_changing_ammo_or_weapon_grips() {
        let (_dir, mut doc) = document();
        let original = doc.library.entries.clone();
        for hand in ["left", "right"] {
            let mut entry = original[0].clone();
            entry.model = "atek_h_magazine".into();
            entry.hand = hand.into();
            entry.grip.offset.x = if hand == "left" { -0.12 } else { 0.15 };
            curls_mut(&mut entry.grip).0[0] = 0.42;
            entry.grip.item_scale = 0.55;
            doc.library.entries.push(entry);
        }
        doc.save().unwrap();
        let loaded = GripDocument::load(doc.path.clone()).unwrap();
        for hand in ["left", "right"] {
            let mut expected = doc.library.lookup("atek_h_magazine", hand).unwrap().clone();
            // A weapon size edit must not make the magazine grow on insertion.
            expected.item_scale = 0.7;
            assert_eq!(
                shock2vr::vr_magazine::prepared_grip(&loaded.library, "atek_h", hand, 0.7),
                Some(expected)
            );
        }
        for entry in original {
            assert_eq!(
                loaded.library.lookup(&entry.model, &entry.hand),
                Some(&entry.grip)
            );
        }
        assert!(
            shock2vr::vr_magazine::prepared_grip(&loaded.library, "ar15_h", "left", 0.55).is_none()
        );
        assert!(
            shock2vr::vr_magazine::prepared_grip(&loaded.library, "atek_h", "left", f32::NAN)
                .is_none()
        );
    }

    #[test]
    fn tricorder_hand_adjustment_saves_without_changing_the_other_hand() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vr-tricorder-grips.json");
        std::fs::write(
            &path,
            include_bytes!("../../../assets/vr-tricorder-grips.json"),
        )
        .unwrap();
        let mut doc = GripDocument::load(path.clone()).unwrap();
        let right = doc.library.lookup("tricorder", "right").unwrap().clone();
        let left = doc
            .library
            .entries
            .iter_mut()
            .find(|e| e.hand == "left")
            .unwrap();
        left.grip.offset.x += 0.025;
        curls_mut(&mut left.grip).0[0] = 0.42;
        let adjusted = left.grip.clone();
        doc.save().unwrap();
        let saved = GripDocument::load(path).unwrap();
        // This is the same lookup API used when the runtime draws the device.
        assert_eq!(saved.library.lookup("tricorder", "left"), Some(&adjusted));
        assert_eq!(saved.library.lookup("tricorder", "right"), Some(&right));
    }

    #[test]
    fn copied_pose_preserves_destination_identity_and_round_trips() {
        let (_dir, mut doc) = document();
        let mut source = doc.library.entries[0].grip.clone();
        source.item_scale = 0.63;
        *curls_mut(&mut source).0 = [0.1, 0.2, 0.3, 0.4, 0.5];
        *curls_mut(&mut source).1 = Some([0.6, 0.7, 0.3, 0.4, 0.5]);
        let identity = doc.library.entries[1].clone();
        transfer_grip(&source, &mut doc.library.entries[1], None).unwrap();
        let target = &doc.library.entries[1];
        assert_eq!(
            (
                &target.model,
                &target.hand,
                &target.surface_hash,
                &target.kinematics_hash,
                &target.hints_hash
            ),
            (
                &identity.model,
                &identity.hand,
                &identity.surface_hash,
                &identity.kinematics_hash,
                &identity.hints_hash
            )
        );
        assert_eq!(target.grip.fingers, source.fingers);
        assert_eq!(target.grip.item_scale, 0.63);
        assert_eq!(target.grip.contacts, [None; 5]);
        assert!(target.authored);
        let before_invalid = doc.library.entries[1].grip.clone();
        assert!(
            transfer_grip(
                &source,
                &mut doc.library.entries[1],
                Some(Matrix4::from_scale(0.0))
            )
            .is_err()
        );
        assert_eq!(doc.library.entries[1].grip, before_invalid);
        doc.save().unwrap();
        let loaded = GripDocument::load(doc.path.clone()).unwrap();
        assert_eq!(loaded.library.entries[1].grip, doc.library.entries[1].grip);
    }

    #[test]
    fn clipboard_copies_both_hands_and_trigger_poses_through_save() {
        let (_dir, mut doc) = document();
        for (index, hand) in ["left", "right"].into_iter().enumerate() {
            let source = doc
                .library
                .entries
                .iter_mut()
                .find(|e| e.model == "mug" && e.hand == hand)
                .unwrap();
            source.grip.offset.x = 0.1 + index as f32 * 0.2;
            source.grip.item_scale = 0.55 + index as f32 * 0.1;
            *curls_mut(&mut source.grip).0 = [0.2 + index as f32 * 0.3; 5];
            *curls_mut(&mut source.grip).1 = (index == 0).then_some([0.8; 5]);
        }
        let poses = copy_model_poses(&doc.library, "mug").unwrap();
        // The clipboard is a snapshot, independent of subsequent source edits.
        doc.library
            .entries
            .iter_mut()
            .find(|e| e.model == "mug")
            .unwrap()
            .grip
            .fingers = FingerPose::curls([0.9; 5]);
        for target in doc
            .library
            .entries
            .iter_mut()
            .filter(|e| e.model == "medpatch")
        {
            *curls_mut(&mut target.grip).1 = Some([0.1; 5]);
        }
        let before = doc.library.entries.clone();
        paste_model_poses(&poses, &mut doc.library, "medpatch").unwrap();
        for (old, actual) in before.iter().zip(&doc.library.entries) {
            let mut expected = old.clone();
            if old.model == "medpatch" {
                let index = if old.hand == "left" { 0 } else { 1 };
                expected.grip = poses[index].clone();
                expected.grip.contacts = [None; 5];
                expected.grip.score = 0.0;
                expected.authored = true;
            }
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
        }
        doc.save().unwrap();
        let loaded = GripDocument::load(doc.path.clone()).unwrap();
        for hand in ["left", "right"] {
            assert_eq!(
                loaded.library.lookup("medpatch", hand),
                doc.library.lookup("medpatch", hand)
            );
        }
    }

    #[test]
    fn clipboard_requires_both_hands_and_pastes_atomically() {
        let (_dir, mut doc) = document();
        let mut poses = copy_model_poses(&doc.library, "mug").unwrap();
        poses[1].item_scale = -1.0;
        let before = serde_json::to_value(&doc.library.entries).unwrap();
        assert!(paste_model_poses(&poses, &mut doc.library, "medpatch").is_err());
        assert_eq!(serde_json::to_value(&doc.library.entries).unwrap(), before);

        let poses = copy_model_poses(&doc.library, "mug").unwrap();
        doc.library
            .entries
            .retain(|e| !(e.model == "medpatch" && e.hand == "right"));
        assert!(copy_model_poses(&doc.library, "medpatch").is_err());
        let before = serde_json::to_value(&doc.library.entries).unwrap();
        assert!(paste_model_poses(&poses, &mut doc.library, "medpatch").is_err());
        assert_eq!(serde_json::to_value(&doc.library.entries).unwrap(), before);
    }

    #[test]
    fn mirrored_pose_reflects_rendered_points_and_round_trips_for_weapon_frames() {
        let (_dir, doc) = document();
        let mut target = doc.library.entries[0].clone();
        let mut source = target.grip.clone();
        source.offset = cgmath::vec3(0.12, -0.08, 0.03);
        source.rotation = Quaternion::from(Euler::new(Deg(23.0), Deg(-37.0), Deg(11.0)));
        source.item_scale = 0.7;
        *curls_mut(&mut source).1 = Some([0.2, 0.8, 0.3, 0.4, 0.5]);
        // Guns reflect Z; posed melee reflects X around its contact origin.
        let contact = cgmath::vec3(0.15, 0.2, -0.1);
        for mirror in [
            Handedness::Left.gun_mirror(),
            Matrix4::from_translation(contact)
                * Handedness::Left.mirror()
                * Matrix4::from_translation(-contact),
        ] {
            transfer_grip(&source, &mut target, Some(mirror)).unwrap();
            assert!(target.grip.is_valid());
            assert_eq!(target.grip.fingers, source.fingers);
            for point in [
                cgmath::Point3::new(0.2, 0.1, 0.4),
                cgmath::Point3::new(-0.1, 0.4, -0.3),
            ] {
                let original = source.offset
                    + source.rotation * (point.to_homogeneous().truncate() * source.item_scale);
                let reflected = target.grip.offset
                    + target.grip.rotation
                        * (mirror.transform_point(point).to_homogeneous().truncate()
                            * target.grip.item_scale);
                assert!((reflected - Handedness::Left.mirror_point(original)).magnitude() < 1e-5);
            }
            let support = shock2vr::vr_support::SupportProfile {
                palm_anchor: [0.13, -0.21, 0.34],
                region: None,
                motion: None,
                rotation_degrees: [15.0, -20.0, 30.0],
                fingers: shock2vr::vr_grip::FingerPose::curls([0.4; 5]),
                grab_radius: 0.07,
                release_distance: 0.12,
                max_swing_degrees: 75.0,
            };
            let mut rig = shock2vr::vr_grip::GripKinematics {
                fingers: std::array::from_fn(|_| Vec::new()),
                palm: cgmath::vec3(0.02, -0.01, -0.08),
                normal: cgmath::Vector3::unit_x(),
            };
            let right = support.glove_pose(
                Handedness::Right,
                shock2vr::vr_support::GripPose {
                    position: source.offset,
                    rotation: source.rotation,
                },
                &source,
                &rig,
                support.anchor_in_frame(Handedness::Right, mirror) * source.item_scale,
            );
            rig.palm = Handedness::Left.mirror_point(rig.palm);
            let left = support.glove_pose(
                Handedness::Left,
                shock2vr::vr_support::GripPose {
                    position: target.grip.offset,
                    rotation: target.grip.rotation,
                },
                &target.grip,
                &rig,
                support.anchor_in_frame(Handedness::Left, mirror) * target.grip.item_scale,
            );
            assert!(
                (left.position - Handedness::Left.mirror_point(right.position)).magnitude() < 1e-5,
                "support glove must use the gun/offset-melee model reflection too"
            );
            let mirrored = target.grip.clone();
            transfer_grip(&mirrored, &mut target, Some(mirror)).unwrap();
            assert!((target.grip.offset - source.offset).magnitude() < 1e-5);
            assert!((target.grip.rotation.dot(source.rotation).abs() - 1.0).abs() < 1e-5);
            assert_eq!(target.grip.fingers, source.fingers);
            assert!(
                (cgmath::Vector3::from(target.grip.anchor) - cgmath::Vector3::from(source.anchor))
                    .magnitude()
                    < 1e-5
            );
        }
    }

    #[test]
    fn saved_manual_pose_round_trips_through_runtime_lookup_without_changing_other_hands() {
        let (_dir, mut doc) = document();
        doc.library.solver_revision = shock2vr::vr_grip::SOLVER_REVISION + 1;
        let untouched = serde_json::to_vec(&doc.library.entries[1..]).unwrap();
        let entry = &mut doc.library.entries[0];
        entry.authored = true;
        entry.grip.offset.y += 0.01;
        curls_mut(&mut entry.grip).0[0] = 0.3;
        let expected = entry.grip.clone();
        assert!(doc.dirty());
        doc.save().unwrap();
        assert!(!doc.dirty());
        let loaded = GripDocument::load(doc.path.clone()).unwrap();
        let entry = &loaded.library.entries[0];
        assert!(entry.authored);
        assert_eq!(
            loaded.library.lookup(&entry.model, &entry.hand),
            Some(&expected)
        );
        assert_eq!(
            serde_json::to_vec(&loaded.library.entries[1..]).unwrap(),
            untouched
        );
    }

    #[test]
    fn external_edits_and_invalid_drafts_cannot_overwrite_the_resource() {
        let (_dir, mut doc) = document();
        doc.library.entries[0].grip.rotation.s = f32::NAN;
        assert!(doc.save().is_err());
        assert_eq!(std::fs::read(&doc.path).unwrap(), doc.saved_bytes);
        doc.restore(0);
        assert!(!doc.dirty());
        let external = b"external edit";
        std::fs::write(&doc.path, external).unwrap();
        assert!(doc.save().unwrap_err().contains("changed on disk"));
        assert_eq!(std::fs::read(&doc.path).unwrap(), external);
    }
    #[test]
    fn save_as_preserves_original_and_refuses_to_replace_an_existing_file() {
        let (dir, mut doc) = document();
        let original = std::fs::read(&doc.path).unwrap();
        let original_path = doc.path.clone();
        doc.library.entries[0].grip.item_scale = 0.8;
        doc.library.entries[0].authored = true;
        assert!(doc.save_as(original_path.clone()).is_err());
        assert_eq!(doc.path, original_path);
        let copy = dir.path().join("my-overrides.json");
        doc.save_as(copy.clone()).unwrap();
        assert_eq!(std::fs::read(&original_path).unwrap(), original);
        let loaded = GripDocument::load(copy.clone()).unwrap();
        assert_eq!(loaded.library.entries[0].grip.item_scale, 0.8);
        doc.library.entries[0].grip.item_scale = 0.9;
        doc.save().unwrap();
        assert_eq!(
            GripDocument::load(copy).unwrap().library.entries[0]
                .grip
                .item_scale,
            0.9
        );
        assert_eq!(std::fs::read(&original_path).unwrap(), original);
    }

    #[test]
    fn added_entries_can_be_saved_without_a_saved_baseline_hand() {
        let (_dir, mut doc) = document();
        let mut added = doc.library.entries[0].clone();
        added.model = "new-model".into();
        doc.library.entries.push(added);
        doc.restore(doc.library.entries.len() - 1); // no saved hand to revert
        doc.save().unwrap();
        let loaded = GripDocument::load(doc.path).unwrap();
        assert!(
            loaded
                .library
                .entries
                .iter()
                .any(|e| e.model == "new-model")
        );
    }
}
