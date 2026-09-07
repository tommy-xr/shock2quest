//! Local authoring of prepared grips. The preview and runtime share geometry,
//! glove poses, transforms, and fingerprint validation.
use crate::model_preview::{ModelPreview, PreviewScene};
use cgmath::{Deg, Euler, InnerSpace, Quaternion, Rad};
use eframe::egui;
use shock2vr::{
    Handedness,
    scenes::debug_interactions::INTERACTION_FIXTURES,
    vr_grip::{BakedGripEntry, GripHints, GripLibrary, SOLVER_REVISION},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::PathBuf,
};

pub(crate) const POSE_PRESETS: [(&str, [f32; 5]); 4] = [
    ("Open", [0.0; 5]),
    ("Point", [0.85, 0.0, 0.9, 0.9, 0.9]),
    ("Closed", [1.0; 5]),
    ("Ball", [0.35, 0.25, 0.3, 0.35, 0.4]),
];

pub fn default_library_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/astra-vr-grips.json")
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
        if library.version != 1 || library.solver_revision != SOLVER_REVISION {
            return Err("Unsupported grip resource revision; rebake with this build first".into());
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
        if std::fs::read(&self.path).map_err(|e| e.to_string())? != self.saved_bytes {
            return Err(
                "Resource changed on disk. Use Save As to keep your drafts, or reopen to load it."
                    .into(),
            );
        }
        self.write(self.path.clone(), false)
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

pub struct GripEditor {
    support_editor: crate::support_grip_editor::SupportEditor,
    support_mode: bool,
    document: Result<GripDocument, String>,
    hints: Result<BTreeMap<String, GripHints>, String>,
    model: String,
    hand: String,
    search: String,
    view: String,
    camera_pending: bool,
    validated: Option<(String, String)>,
    hashes: Option<(String, String)>,
    stale: bool,
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
            .is_some_and(|m| shock2vr::vr_weapon_grip::supports_model(m))
        {
            default_library_path().with_file_name("astra-vr-weapon-grips.json")
        } else {
            default_library_path()
        };
        Self {
            support_editor: crate::support_grip_editor::SupportEditor::new(support_path),
            support_mode,
            document: GripDocument::load(path.unwrap_or(default_path)),
            hints,
            model: model.unwrap_or_else(|| "mug".into()),
            hand,
            search: String::new(),
            view,
            camera_pending: true,
            validated: None,
            hashes: None,
            stale: false,
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
        self.model = std::path::Path::new(key)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if shock2vr::vr_weapon_grip::supports_model(&self.model) {
            if let (Ok(doc), Ok(defaults)) = (
                &mut self.document,
                GripDocument::load(
                    default_library_path().with_file_name("astra-vr-weapon-grips.json"),
                ),
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
            match preview.grip_inputs(&key, hand) {
                Ok((surface, rig, hash, guide)) => inputs.push((name, surface, rig, hash, guide)),
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
                .map(|(hand, surface, rig, surface_hash, guide)| {
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
                    let grip = resolved.ok_or_else(|| {
                        format!("No valid {hand} fit for {model}; existing drafts kept")
                    })?;
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
            && (primary_dirty || self.support_editor.dirty() || busy)
            && ctx.input(|i| i.viewport().close_requested())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        if self.confirm_close {
            egui::Modal::new(egui::Id::new("unsaved_grips")).show(ctx, |ui| {
                ui.heading("Unsaved grip edits");
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
                        .add_enabled(!self.stale && !busy, egui::Button::new("Save and close"))
                        .clicked()
                    {
                        let primary_saved = match &mut self.document {
                            Ok(doc) if doc.dirty() => doc.save(),
                            _ => Ok(()),
                        };
                        match primary_saved.and_then(|_| self.support_editor.save()) {
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
        ui.heading("VR Grips");
        ui.label("Grip overrides · shared glove rig");
        let can_switch = !self.is_busy() && self.document.as_ref().is_ok_and(|doc| !doc.dirty());
        let mut switch = None;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(can_switch, egui::Button::new("Pickups"))
                .clicked()
            {
                switch = Some(("astra-vr-grips.json", "mug"));
            }
            if ui
                .add_enabled(can_switch, egui::Button::new("Weapons"))
                .clicked()
            {
                switch = Some(("astra-vr-weapon-grips.json", "atek_h"));
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
        let models: BTreeSet<_> = doc.library.entries.iter().map(|e| &e.model).collect();
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
                if ui.selectable_label(self.model == *model, text).clicked() {
                    self.model = model.clone();
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
        let mut request_fit = false;
        let doc = self.document.as_mut().unwrap();
        let hints = self
            .hints
            .as_ref()
            .unwrap()
            .get(&self.model)
            .cloned()
            .unwrap_or_default();
        ui.heading(format!("{} — grip override", self.model));
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.support_mode, false, "Primary grip");
            ui.selectable_value(&mut self.support_mode, true, "Support grip");
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
        let entry = &doc.library.entries[index];
        self.stale = self.hashes.as_ref().is_none_or(|(surface, rig)| {
            entry.surface_hash != *surface
                || entry.kinematics_hash != *rig
                || entry.hints_hash != hints.fingerprint()
        });
        if self.stale {
            ui.colored_label(
                egui::Color32::YELLOW,
                "Inputs changed: replace with an automatic fit before editing or saving this pose.",
            );
        }
        if self.support_mode {
            if doc.dirty() {
                ui.label("Primary grip also has unsaved edits. Primary grip → Save all edits saves both resources.");
            }
            let entry = &doc.library.entries[index];
            self.support_editor.show(
                ui,
                &self.model,
                hand,
                entry.grip.item_scale,
                !self.stale && !busy,
            );
            let scene = PreviewScene::Grip(
                hand,
                entry.grip.clone(),
                self.support_editor.profile(&self.model).cloned(),
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
                    (doc.dirty() || self.support_editor.dirty()) && !self.stale && !busy,
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
                        .with_file_name("astra-vr-grips.custom.json")
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
        ui.add_enabled_ui(!self.stale && !busy, |ui| {
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
                for (name, value) in ["Thumb", "Index", "Middle", "Ring", "Pinky"]
                    .into_iter()
                    .zip(&mut entry.grip.curls)
                {
                    tweak_slider(&mut columns[2], name, value, 0.0..=1.0, 0.02, 2);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Rotation nudge step");
                ui.add(egui::Slider::new(&mut self.rotation_step, 0.1..=15.0).suffix("°"));
                ui.separator();
                ui.label("Uniform item scale");
                tweak_slider(ui, "×", &mut entry.grip.item_scale, 0.1..=3.0, 0.02, 2);
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Pose presets");
                for (name, amounts) in POSE_PRESETS {
                    if ui.button(name).clicked() {
                        entry.grip.curls = amounts;
                    }
                }
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
        let scene = PreviewScene::Grip(
            hand,
            entry.grip.clone(),
            self.support_editor.profile(&self.model).cloned(),
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
mod tests {
    use super::*;
    fn document() -> (tempfile::TempDir, GripDocument) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("grips.json");
        std::fs::write(&path, include_bytes!("../../../assets/astra-vr-grips.json")).unwrap();
        let doc = GripDocument::load(path).unwrap();
        (dir, doc)
    }

    #[test]
    fn saved_manual_pose_round_trips_through_runtime_lookup_without_changing_other_hands() {
        let (_dir, mut doc) = document();
        let untouched = serde_json::to_vec(&doc.library.entries[1..]).unwrap();
        let entry = &mut doc.library.entries[0];
        entry.authored = true;
        entry.grip.offset.y += 0.01;
        entry.grip.curls[0] = 0.3;
        let expected = entry.grip.clone();
        assert!(doc.dirty());
        doc.save().unwrap();
        assert!(!doc.dirty());
        let loaded = GripDocument::load(doc.path.clone()).unwrap();
        let entry = &loaded.library.entries[0];
        assert!(entry.authored);
        assert_eq!(
            loaded.library.lookup(
                &entry.model,
                &entry.hand,
                &entry.surface_hash,
                &entry.kinematics_hash,
                &entry.hints_hash
            ),
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
