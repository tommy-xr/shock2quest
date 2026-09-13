//! Author the personal card's resting pose against the actual battle belt.
use crate::{
    grip_editor::{default_library_path, persist_json, tweak_slider},
    model_preview::{ModelPreview, PreviewScene},
};
use eframe::egui;
use shock2vr::vr_belt::BeltCardPose;
use std::path::PathBuf;

struct Document {
    pose: BeltCardPose,
    saved: BeltCardPose,
    bytes: Vec<u8>,
    path: PathBuf,
}
impl Document {
    fn load(path: PathBuf) -> Result<Self, String> {
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let pose = BeltCardPose::parse(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)?;
        Ok(Self {
            saved: pose.clone(),
            pose,
            bytes,
            path,
        })
    }
    fn dirty(&self) -> bool {
        self.pose != self.saved
    }
    fn write(&mut self, path: PathBuf, new_file: bool) -> Result<(), String> {
        if !self.pose.is_valid() {
            return Err("Cannot save invalid card pose".into());
        }
        if !new_file && std::fs::read(&self.path).map_err(|e| e.to_string())? != self.bytes {
            return Err(
                "Card resource changed on disk. Use Save As to preserve your draft.".into(),
            );
        }
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(path)
        };
        let mut bytes = serde_json::to_vec_pretty(&self.pose).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        persist_json(&path, &bytes, new_file)?;
        self.path = path;
        self.bytes = bytes;
        self.saved = self.pose.clone();
        Ok(())
    }
}

pub struct BeltCardEditor {
    document: Result<Document, String>,
    message: String,
    save_as: Option<String>,
    camera: Option<String>,
}
impl BeltCardEditor {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            document: Document::load(
                path.unwrap_or_else(|| default_library_path().with_file_name("vr-belt-card.json")),
            ),
            message: String::new(),
            save_as: None,
            camera: Some("front".into()),
        }
    }
    pub fn error(&self) -> Option<&str> {
        self.document.as_ref().err().map(String::as_str)
    }
    pub fn dirty(&self) -> bool {
        self.document.as_ref().is_ok_and(|d| d.dirty())
    }
    pub fn save(&mut self) -> Result<(), String> {
        if !self.dirty() {
            return Ok(());
        }
        let doc = self.document.as_mut().map_err(|e| e.clone())?;
        doc.write(doc.path.clone(), false)
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        frame: &mut eframe::Frame,
        preview: &mut ModelPreview,
    ) {
        ui.heading("Personal card — resting pose");
        ui.label("Position the card on the buckle. This changes its resting model and grab target; hand grips stay independent.");
        let doc = match &mut self.document {
            Ok(doc) => doc,
            Err(e) => {
                ui.colored_label(egui::Color32::LIGHT_RED, e.as_str());
                return;
            }
        };
        ui.horizontal(|ui| {
            if ui
                .add_enabled(doc.dirty(), egui::Button::new("Save card pose *"))
                .clicked()
            {
                self.message = match doc.write(doc.path.clone(), false) {
                    Ok(()) => "Saved. Restart the game to use this pose.".into(),
                    Err(e) => e,
                };
            }
            if ui.button("Save As…").clicked() {
                self.save_as = Some(
                    doc.path
                        .with_file_name("vr-belt-card.custom.json")
                        .display()
                        .to_string(),
                );
            }
            if ui
                .add_enabled(doc.dirty(), egui::Button::new("Revert"))
                .clicked()
            {
                doc.pose = doc.saved.clone();
            }
            if ui.button("Reset to default").clicked() {
                doc.pose = BeltCardPose::default();
            }
        });
        if let Some(path) = &mut self.save_as {
            let mut save = false;
            let mut cancel = false;
            ui.horizontal(|ui| {
                ui.text_edit_singleline(path);
                save = ui.button("Write new file").clicked();
                cancel = ui.button("Cancel").clicked();
            });
            if save {
                match doc.write(PathBuf::from(path.as_str()), true) {
                    Ok(()) => {
                        self.save_as = None;
                        self.message = "Saved a new card resource. Copy it to assets/vr-belt-card.json to use in game.".into();
                    }
                    Err(e) => self.message = e,
                }
            } else if cancel {
                self.save_as = None;
            }
        }
        ui.columns(2, |columns| {
            columns[0].label("Position (cm, relative to belt origin)");
            for (i, label) in ["Right +X", "Up +Y", "Back +Z"].into_iter().enumerate() {
                let mut cm = doc.pose.position_m[i] * 100.0;
                if tweak_slider(&mut columns[0], label, &mut cm, -100.0..=100.0, 0.1, 1) {
                    doc.pose.position_m[i] = cm / 100.0;
                }
            }
            columns[1].label("Rotation (degrees)");
            for (i, label) in ["X", "Y", "Z"].into_iter().enumerate() {
                tweak_slider(
                    &mut columns[1],
                    label,
                    &mut doc.pose.rotation_degrees[i],
                    -180.0..=180.0,
                    1.0,
                    1,
                );
            }
        });
        ui.small(format!("Resource: {}", doc.path.display()));
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        ui.horizontal(|ui| {
            for view in ["front", "oblique", "top", "side"] {
                if ui.button(view).clicked() {
                    self.camera = Some(view.into());
                }
            }
            ui.label("Drag to orbit · scroll to zoom");
        });
        let scene = PreviewScene::BeltCard(doc.pose.clone());
        preview.prepare("scipass.bin", &scene);
        if let Some(view) = self.camera.take() {
            preview.belt_camera(&view);
        }
        preview.show(ui, frame, "scipass.bin", &scene);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_round_trips_and_protects_external_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("card.json");
        std::fs::write(&path, serde_json::to_vec(&BeltCardPose::default()).unwrap()).unwrap();
        let mut doc = Document::load(path.clone()).unwrap();
        doc.pose.position_m[1] = 0.02;
        doc.pose.rotation_degrees[0] = 85.0;
        doc.write(path.clone(), false).unwrap();
        assert_eq!(Document::load(path.clone()).unwrap().pose, doc.pose);
        std::fs::write(&path, "external edit").unwrap();
        assert!(doc.write(path.clone(), false).is_err());
        assert!(doc.write(path.clone(), true).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "external edit");
    }
}
