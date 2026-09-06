//! Local authoring of prepared grips. The preview and runtime share geometry,
//! glove poses, transforms, and fingerprint validation.
use crate::model_preview::{ModelPreview, PreviewScene};
use cgmath::{Deg, InnerSpace, Quaternion, Rotation3, Vector3};
use eframe::egui;
use shock2vr::{
    Handedness,
    scenes::debug_interactions::INTERACTION_FIXTURES,
    vr_grip::{GripHints, GripLibrary, SOLVER_REVISION},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::PathBuf,
};

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
        if self.library.entries.iter().any(|e| !e.grip.is_valid()) {
            return Err("Cannot save an invalid pose".into());
        }
        if std::fs::read(&self.path).map_err(|e| e.to_string())? != self.saved_bytes {
            return Err(
                "Resource changed on disk. Reopen the editor to load it before saving.".into(),
            );
        }
        let mut bytes = serde_json::to_vec_pretty(&self.library).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        let mut temp = tempfile::NamedTempFile::new_in(self.path.parent().unwrap())
            .map_err(|e| e.to_string())?;
        temp.write_all(&bytes)
            .and_then(|_| temp.as_file().sync_all())
            .map_err(|e| e.to_string())?;
        temp.persist(&self.path).map_err(|e| e.to_string())?;
        self.saved_bytes = bytes;
        self.saved = self.library.clone();
        Ok(())
    }

    fn restore(&mut self, index: usize) {
        self.library.entries[index] = self.saved.entries[index].clone();
    }
}

pub struct GripEditor {
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
    rotation_step: f32,
    confirm_close: bool,
    allow_close: bool,
}

impl GripEditor {
    pub fn new(path: Option<PathBuf>, model: Option<String>, hand: String, view: String) -> Self {
        // Hints are the same authoring inputs used by gameplay and the baker.
        let hints_path = default_library_path().with_file_name("astra-vr-grip-hints.json");
        let hints = std::fs::read(hints_path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()));
        Self {
            document: GripDocument::load(path.unwrap_or_else(default_library_path)),
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
            rotation_step: 5.0,
            confirm_close: false,
            allow_close: false,
        }
    }

    pub fn error(&self) -> Option<&str> {
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
        let Ok(doc) = &mut self.document else {
            return;
        };
        if !self.allow_close && doc.dirty() && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        if self.confirm_close {
            egui::Modal::new(egui::Id::new("unsaved_grips")).show(ctx, |ui| {
                ui.heading("Unsaved grip edits");
                ui.label("Save your drafts before closing?");
                ui.horizontal(|ui| {
                    if ui.button("Keep editing").clicked() {
                        self.confirm_close = false;
                    }
                    if ui
                        .add_enabled(!self.stale, egui::Button::new("Save and close"))
                        .clicked()
                    {
                        match doc.save() {
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
        ui.label("Pickup overrides · shared glove rig");
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
            "Edits affect every item using this model. Left and right hands save independently.",
        );
        ui.label("Authored weapon hands and psi-amp editing arrive in the weapon workstream.");
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        frame: &mut eframe::Frame,
        preview: &mut ModelPreview,
    ) {
        if let (Err(error), _) | (_, Err(error)) = (&self.document, &self.hints) {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
            return;
        }
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
            ui.label("No prepared grip for this hand. Select the other hand above.");
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
                Ok((_, rig, surface)) => Some((surface, rig.fingerprint())),
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
        ui.horizontal(|ui| {
            let label = if doc.dirty() {
                "Save all edits *"
            } else {
                "Saved"
            };
            if ui
                .add_enabled(doc.dirty() && !self.stale, egui::Button::new(label))
                .clicked()
            {
                self.message = match doc.save() {
                    Ok(()) => "Saved. Restart the game runtime to load the resource.".into(),
                    Err(e) => e,
                };
            }
            if ui.button("Revert this hand to saved").clicked() {
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
                if ui
                    .add_enabled(
                        self.hashes.is_some(),
                        egui::Button::new("Replace draft with auto fit"),
                    )
                    .clicked()
                {
                    let mut fit_hints = hints.clone();
                    if self.family.is_some() {
                        fit_hints.pose_family = self.family;
                    }
                    match preview.fit_grip(&key, hand, &fit_hints) {
                        Ok(grip) => {
                            let e = &mut doc.library.entries[index];
                            e.grip = grip;
                            e.authored = self.family.is_some();
                            let (surface, rig) = self.hashes.as_ref().unwrap();
                            e.surface_hash = surface.clone();
                            e.kinematics_hash = rig.clone();
                            e.hints_hash = hints.fingerprint();
                            self.stale = false;
                            self.message = "Automatic fit ready for review; save to apply.".into();
                        }
                        Err(e) => self.message = e,
                    }
                }
            });
        });
        let entry = &mut doc.library.entries[index];
        let before = entry.grip.clone();
        ui.add_enabled_ui(!self.stale, |ui| {
            ui.horizontal(|ui| {
                ui.label("Item position (cm)");
                for (axis, value) in ["X", "Y", "Z"].into_iter().zip([
                    &mut entry.grip.offset.x,
                    &mut entry.grip.offset.y,
                    &mut entry.grip.offset.z,
                ]) {
                    let mut cm = *value * shock2vr::METERS_PER_WORLD_UNIT * 100.0;
                    if ui
                        .add(
                            egui::DragValue::new(&mut cm)
                                .speed(0.1)
                                .prefix(format!("{axis} "))
                                .range(-500.0..=500.0),
                        )
                        .changed()
                    {
                        *value = cm / (shock2vr::METERS_PER_WORLD_UNIT * 100.0);
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Rotate item about hand axis");
                ui.add(
                    egui::DragValue::new(&mut self.rotation_step)
                        .speed(0.1)
                        .range(0.1..=90.0)
                        .suffix("°"),
                );
                for (name, axis) in [
                    ("X", Vector3::unit_x()),
                    ("Y", Vector3::unit_y()),
                    ("Z", Vector3::unit_z()),
                ] {
                    for sign in [-1.0, 1.0] {
                        if ui
                            .button(format!("{name}{}", if sign < 0.0 { "−" } else { "+" }))
                            .clicked()
                        {
                            entry.grip.rotation =
                                (Quaternion::from_axis_angle(axis, Deg(self.rotation_step * sign))
                                    * entry.grip.rotation)
                                    .normalize();
                        }
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Fingers");
                if ui.button("Open").clicked() {
                    entry.grip.curls = [0.0; 5];
                }
                if ui.button("Point").clicked() {
                    entry.grip.curls = [0.85, 0.0, 0.9, 0.9, 0.9];
                }
                if ui.button("Closed").clicked() {
                    entry.grip.curls = [1.0; 5];
                }
                for (name, value) in ["Thumb", "Index", "Middle", "Ring", "Pinky"]
                    .into_iter()
                    .zip(&mut entry.grip.curls)
                {
                    ui.add(
                        egui::DragValue::new(value)
                            .speed(0.01)
                            .range(0.0..=1.0)
                            .prefix(format!("{name} ")),
                    );
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
        ui.small("Drag values to adjust; click a value to type. Unsaved drafts stay when switching models. Orbit to inspect hidden contacts.");
        // Build first, then select the requested camera so the initial model
        // framing cannot overwrite it. show() keeps the same camera on edits.
        let scene = PreviewScene::Grip(hand, entry.grip.clone());
        preview.prepare(&key, &scene);
        if self.camera_pending {
            preview.grip_camera(&self.view, hand);
            self.camera_pending = false;
        }
        preview.show(ui, frame, &key, &scene);
    }
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
}
