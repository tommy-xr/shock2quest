//! Author each item's pose inside the same shell rendered in gameplay.
use crate::{
    grip_editor::{default_library_path, persist_json, tweak_slider},
    model_preview::{ModelPreview, PreviewScene},
};
use eframe::egui;
use shock2vr::vr_holster::{
    HolsterEntry, HolsterLibrary, HolsterPose, RESOURCE, is_bin_model_name, model_key,
};
use std::path::PathBuf;

struct Document {
    library: HolsterLibrary,
    saved: HolsterLibrary,
    bytes: Vec<u8>,
    path: PathBuf,
}

impl Document {
    fn load(path: PathBuf) -> Result<Self, String> {
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let library =
            HolsterLibrary::parse(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)?;
        Ok(Self {
            saved: library.clone(),
            library,
            bytes,
            path,
        })
    }

    fn dirty(&self) -> bool {
        self.library != self.saved
    }

    fn save(&mut self) -> Result<(), String> {
        let mut bytes = serde_json::to_vec_pretty(&self.library).map_err(|e| e.to_string())?;
        HolsterLibrary::parse(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)?;
        if std::fs::read(&self.path).map_err(|e| e.to_string())? != self.bytes {
            return Err(
                "Holster resource changed on disk. Reopen the editor before saving.".into(),
            );
        }
        bytes.push(b'\n');
        persist_json(&self.path, &bytes, false)?;
        self.bytes = bytes;
        self.saved = self.library.clone();
        Ok(())
    }
}

pub struct HolsterEditor {
    document: Result<Document, String>,
    model: String,
    new_model: String,
    message: String,
    camera_pending: bool,
    view: String,
}

impl HolsterEditor {
    pub fn new(path: Option<PathBuf>, model: Option<String>) -> Self {
        let mut editor = Self {
            document: Document::load(
                path.unwrap_or_else(|| default_library_path().with_file_name(RESOURCE)),
            ),
            model: "atek_w".into(),
            new_model: String::new(),
            message: String::new(),
            camera_pending: true,
            view: "oblique".into(),
        };
        if let Some(model) = model {
            editor.open_model(&model);
        }
        editor
    }

    pub fn open_model(&mut self, model: &str) {
        self.model = self
            .document
            .as_ref()
            .ok()
            .and_then(|doc| doc.library.get(model))
            .map(|entry| entry.model.clone())
            .unwrap_or_else(|| model_key(model));
        self.new_model = self.model.clone();
        self.camera_pending = true;
    }

    pub fn error(&self) -> Option<&str> {
        self.document.as_ref().err().map(String::as_str)
    }

    pub fn dirty(&self) -> bool {
        self.document.as_ref().is_ok_and(|doc| doc.dirty())
    }

    pub fn save(&mut self) -> Result<(), String> {
        if !self.dirty() {
            return Ok(());
        }
        self.document.as_mut().map_err(|e| e.clone())?.save()
    }

    pub fn show_list(&mut self, ui: &mut egui::Ui) {
        ui.label("Items with holster poses");
        let Ok(doc) = &mut self.document else {
            return;
        };
        for entry in &doc.library.entries {
            if ui
                .selectable_label(self.model == entry.model, &entry.label)
                .clicked()
            {
                self.model = entry.model.clone();
                self.camera_pending = true;
            }
        }
        ui.separator();
        ui.label("Add BIN world model (e.g. atek_w)");
        ui.text_edit_singleline(&mut self.new_model);
        let key = model_key(&self.new_model);
        if ui
            .add_enabled(
                is_bin_model_name(&self.new_model),
                egui::Button::new("Add / select"),
            )
            .clicked()
        {
            self.model = if let Some(entry) = doc.library.get(&key) {
                entry.model.clone()
            } else {
                doc.library.entries.push(HolsterEntry {
                    model: key.clone(),
                    label: key.clone(),
                    held_model: None,
                    pose: HolsterPose::default(),
                });
                key
            };
            self.camera_pending = true;
        }
        ui.small("Only items with a saved pose can be holstered.");
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        frame: &mut eframe::Frame,
        preview: &mut ModelPreview,
    ) {
        ui.heading("Holster — item placement");
        ui.label("Position the item inside the shell. Saving its pose enables holstering; removing the pose disables it.");
        let doc = match &mut self.document {
            Ok(doc) => doc,
            Err(error) => {
                ui.colored_label(egui::Color32::LIGHT_RED, error.as_str());
                return;
            }
        };
        ui.horizontal(|ui| {
            if ui
                .add_enabled(doc.dirty(), egui::Button::new("Save holster poses *"))
                .clicked()
            {
                self.message = match doc.save() {
                    Ok(()) => "Saved. Restart the game to use these poses.".into(),
                    Err(error) => error,
                };
            }
            if ui
                .add_enabled(doc.dirty(), egui::Button::new("Revert"))
                .clicked()
            {
                doc.library = doc.saved.clone();
            }
            if ui.button("Remove pose").clicked() {
                doc.library
                    .entries
                    .retain(|entry| entry.model != self.model);
            }
        });
        ui.small(format!("Resource: {}", doc.path.display()));
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        let Some(entry) = doc
            .library
            .entries
            .iter_mut()
            .find(|entry| entry.model == self.model)
        else {
            ui.label(
                "No holster pose for this model. Add its world model in the list to author one.",
            );
            return;
        };
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut entry.label);
        });
        ui.columns(2, |columns| {
            columns[0].label("Position inside holster (cm)");
            for (i, label) in ["Right +X", "Up +Y", "Back +Z"].into_iter().enumerate() {
                let mut cm = entry.pose.position_m[i] * 100.0;
                if tweak_slider(&mut columns[0], label, &mut cm, -100.0..=100.0, 0.1, 1) {
                    entry.pose.position_m[i] = cm / 100.0;
                }
            }
            let mut length = entry.pose.length_m * 100.0;
            if tweak_slider(
                &mut columns[0],
                "Item length (cm)",
                &mut length,
                2.0..=120.0,
                0.1,
                1,
            ) {
                entry.pose.length_m = length / 100.0;
            }
            columns[1].label("Rotation (degrees)");
            for (i, label) in ["X", "Y", "Z"].into_iter().enumerate() {
                tweak_slider(
                    &mut columns[1],
                    label,
                    &mut entry.pose.rotation_degrees[i],
                    -180.0..=180.0,
                    1.0,
                    1,
                );
            }
        });
        ui.label("Holster shell scale (1.0 = default)");
        ui.small("Resizes the shell for this item without changing the item's size or placement.");
        for (i, label) in ["Width X", "Height Y", "Depth Z"].into_iter().enumerate() {
            tweak_slider(
                ui,
                label,
                &mut entry.pose.shell_scale[i],
                0.1..=3.0,
                0.01,
                2,
            );
        }
        ui.horizontal(|ui| {
            for view in ["front", "back", "oblique", "top"] {
                if ui.selectable_label(self.view == view, view).clicked() {
                    self.view = view.into();
                    self.camera_pending = true;
                }
            }
            ui.label("Drag to orbit · scroll to zoom");
        });
        let key = format!("{}.bin", model_key(&entry.model));
        let scene = PreviewScene::Holster(entry.pose.clone());
        preview.prepare(&key, &scene);
        if self.camera_pending {
            preview.holster_camera(&self.view);
            self.camera_pending = false;
        }
        preview.show(ui, frame, &key, &scene);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_item_pose_reaches_runtime_and_external_edits_are_protected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(RESOURCE);
        std::fs::write(&path, include_str!("../../../assets/vr-holsters.json")).unwrap();
        let mut doc = Document::load(path.clone()).unwrap();
        let original = doc.library.get("laser").unwrap().clone();
        doc.library.entries[0].pose.position_m = [0.01, 0.06, -0.02];
        doc.library.entries[0].pose.rotation_degrees[1] = 25.0;
        doc.library.entries[0].pose.shell_scale = [0.7, 1.3, 1.8];
        doc.save().unwrap();
        let loaded = HolsterLibrary::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            loaded.get("atek_h").unwrap().pose,
            doc.library.entries[0].pose
        );
        assert_eq!(loaded.get("laser").unwrap(), &original);
        std::fs::write(&path, "external edit").unwrap();
        assert!(doc.save().is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "external edit");
    }
}
