//! Support-hand authoring uses the same resource and placement as gameplay.
use crate::grip_editor::{POSE_PRESETS, default_library_path, persist_json, tweak_slider};
use eframe::egui;
use shock2vr::{Handedness, vr_support::SupportProfile};
use std::{collections::BTreeMap, path::PathBuf};

struct SupportDocument {
    profiles: BTreeMap<String, SupportProfile>,
    saved: BTreeMap<String, SupportProfile>,
    path: PathBuf,
    saved_bytes: Vec<u8>,
}
impl SupportDocument {
    fn load(path: PathBuf) -> Result<Self, String> {
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let profiles: BTreeMap<String, SupportProfile> =
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if profiles.values().any(|p| !p.is_valid()) {
            return Err("Invalid support pose resource".into());
        }
        Ok(Self {
            saved: profiles.clone(),
            profiles,
            path,
            saved_bytes: bytes,
        })
    }
    fn dirty(&self) -> bool {
        self.profiles != self.saved
    }
    fn restore(&mut self, model: &str) {
        if let Some(saved) = self.saved.get(model) {
            self.profiles.insert(model.into(), saved.clone());
        } else {
            self.profiles.remove(model);
        }
    }
    fn save(&mut self) -> Result<(), String> {
        if !self.dirty() {
            return Ok(());
        }
        if std::fs::read(&self.path).map_err(|e| e.to_string())? != self.saved_bytes {
            return Err(
                "Support resource changed on disk. Save As a new file to preserve your draft."
                    .into(),
            );
        }
        self.write(self.path.clone(), false)
    }
    fn write(&mut self, path: PathBuf, new_file: bool) -> Result<(), String> {
        if self.profiles.values().any(|p| !p.is_valid()) {
            return Err("Cannot save an invalid support pose".into());
        }
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(path)
        };
        let mut bytes = serde_json::to_vec_pretty(&self.profiles).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        persist_json(&path, &bytes, new_file)?;
        self.path = path;
        self.saved = self.profiles.clone();
        self.saved_bytes = bytes;
        Ok(())
    }
}

pub struct SupportEditor {
    document: Result<SupportDocument, String>,
    save_as: Option<String>,
    message: String,
    rotation_step: f32,
}
impl SupportEditor {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            document: SupportDocument::load(path.unwrap_or_else(|| {
                default_library_path().with_file_name("astra-vr-support-grips.json")
            })),
            save_as: None,
            message: String::new(),
            rotation_step: 5.0,
        }
    }
    pub fn dirty(&self) -> bool {
        self.document.as_ref().is_ok_and(|d| d.dirty())
    }
    pub fn save(&mut self) -> Result<(), String> {
        if !self.dirty() {
            return Ok(());
        }
        self.document.as_mut().map_err(|e| e.clone())?.save()
    }
    pub fn profile(&self, model: &str) -> Option<&SupportProfile> {
        self.document.as_ref().ok()?.profiles.get(model)
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        model: &str,
        primary: Handedness,
        item_scale: f32,
        enabled: bool,
    ) {
        let doc = match &mut self.document {
            Ok(doc) => doc,
            Err(e) => {
                ui.colored_label(egui::Color32::LIGHT_RED, e.as_str());
                return;
            }
        };
        ui.label("Adjust the free hand while the primary grip stays visible. One support pose mirrors for either primary hand.");
        if model != "wrench_h" {
            ui.colored_label(
                egui::Color32::YELLOW,
                "Preview only: this model does not yet support two-hand grabbing in game.",
            );
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    enabled && doc.dirty(),
                    egui::Button::new("Save support edits *"),
                )
                .clicked()
            {
                self.message = match doc.save() {
                    Ok(()) => "Saved support poses to the displayed resource.".into(),
                    Err(e) => e,
                };
            }
            if ui
                .add_enabled(enabled, egui::Button::new("Save As…"))
                .clicked()
            {
                self.message.clear();
                self.save_as = Some(
                    doc.path
                        .with_file_name("astra-vr-support-grips.custom.json")
                        .display()
                        .to_string(),
                );
            }
            if ui
                .add_enabled(
                    enabled && doc.profiles.contains_key(model),
                    egui::Button::new("Revert support pose"),
                )
                .clicked()
            {
                doc.restore(model);
            }
        });
        if !doc.profiles.contains_key(model) {
            if ui
                .add_enabled(enabled, egui::Button::new("Add support pose"))
                .clicked()
            {
                doc.profiles.insert(
                    model.into(),
                    SupportProfile {
                        palm_anchor: [-0.09, 0.35, 0.02],
                        rotation_degrees: [0.0; 3],
                        curls: [0.4; 5],
                        grab_radius: 0.07,
                        release_distance: 0.12,
                        max_swing_degrees: 75.0,
                    },
                );
            }
        }
        if let Some(profile) = doc.profiles.get_mut(model) {
            ui.add_enabled_ui(
                enabled && item_scale.is_finite() && item_scale > 0.0,
                |ui| {
                    ui.columns(3, |columns| {
                        columns[0].strong("Palm position on item (cm)");
                        for (i, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
                            let mirror = if primary == Handedness::Left && i == 0 {
                                -1.0
                            } else {
                                1.0
                            };
                            let factor =
                                item_scale * shock2vr::METERS_PER_WORLD_UNIT * 100.0 * mirror;
                            let mut cm = profile.palm_anchor[i] * factor;
                            if tweak_slider(&mut columns[0], axis, &mut cm, -100.0..=100.0, 0.1, 2)
                            {
                                profile.palm_anchor[i] = cm / factor;
                            }
                        }
                        columns[1].strong("Support wrist rotation (degrees)");
                        for (i, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
                            let mirror = if primary == Handedness::Left && i > 0 {
                                -1.0
                            } else {
                                1.0
                            };
                            let mut angle = profile.rotation_degrees[i] * mirror;
                            if tweak_slider(
                                &mut columns[1],
                                axis,
                                &mut angle,
                                -180.0..=180.0,
                                self.rotation_step,
                                1,
                            ) {
                                profile.rotation_degrees[i] = angle * mirror;
                            }
                        }
                        columns[2].strong("Support finger curls");
                        for (name, value) in ["Thumb", "Index", "Middle", "Ring", "Pinky"]
                            .into_iter()
                            .zip(&mut profile.curls)
                        {
                            tweak_slider(&mut columns[2], name, value, 0.0..=1.0, 0.02, 2);
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Support pose presets");
                        for (name, curls) in POSE_PRESETS {
                            if ui.button(name).clicked() {
                                profile.curls = curls;
                            }
                        }
                        ui.label("Rotation nudge");
                        ui.add(egui::Slider::new(&mut self.rotation_step, 0.1..=15.0).suffix("°"));
                    });
                },
            );
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        ui.small(format!(
            "Support poses save separately: {}",
            doc.path.display()
        ));
        if let Some(path) = &mut self.save_as {
            let mut close = false;
            egui::Modal::new(egui::Id::new("save_support_as")).show(ui.ctx(), |ui| {
                ui.heading("Save support poses as JSON");
                ui.text_edit_singleline(path);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui.button("Save new file").clicked() {
                        self.message = match doc.write(PathBuf::from(path.trim()), true) {
                            Ok(()) => {
                                close = true;
                                "Saved support resource".into()
                            }
                            Err(e) => e,
                        };
                    }
                });
                ui.label(&self.message);
            });
            if close {
                self.save_as = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn support_edits_round_trip_and_preserve_external_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("support.json");
        let original = br#"{"wrench_h":{"palm_anchor":[-0.09,0.354,0.02],"curls":[0.08,0.375,0.4,0.45,0.6],"grab_radius":0.07,"release_distance":0.12,"max_swing_degrees":75.0}}"#;
        std::fs::write(&path, original).unwrap();
        let mut doc = SupportDocument::load(path.clone()).unwrap();
        assert_eq!(doc.profiles["wrench_h"].rotation_degrees, [0.0; 3]);
        doc.profiles
            .insert("nanocan".into(), doc.profiles["wrench_h"].clone());
        assert!(doc.dirty());
        doc.restore("nanocan");
        assert!(
            !doc.dirty(),
            "reverting a new draft removes it without saving it"
        );
        assert!(!doc.profiles.contains_key("nanocan"));
        doc.profiles.get_mut("wrench_h").unwrap().rotation_degrees = [20.0, -15.0, 30.0];
        doc.profiles.get_mut("wrench_h").unwrap().palm_anchor[2] = 0.04;
        doc.save().unwrap();
        assert_eq!(
            SupportDocument::load(path.clone()).unwrap().profiles,
            doc.profiles
        );
        let saved = std::fs::read(&path).unwrap();
        doc.profiles.get_mut("wrench_h").unwrap().curls[0] = 0.7;
        std::fs::write(&path, b"external edit").unwrap();
        assert!(doc.save().is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"external edit");
        let copy = dir.path().join("support-custom.json");
        doc.write(copy.clone(), true).unwrap();
        assert!(doc.write(copy.clone(), true).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"external edit");
        assert_ne!(std::fs::read(&copy).unwrap(), saved);
        let before = std::fs::read(&copy).unwrap();
        doc.profiles.get_mut("wrench_h").unwrap().curls[1] = 2.0;
        assert!(doc.save().is_err());
        assert_eq!(std::fs::read(&copy).unwrap(), before);
    }
}
