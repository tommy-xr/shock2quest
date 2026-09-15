//! Inspect the selected file independently of the renderer's high-detail gate.
//! A Nightdive LGMD is a replacement mesh; only LGMM carries a PMNM alternative.

use std::io::{Cursor, Seek};

use dark::{ss2_bin_ai_loader, ss2_bin_header, ss2_bin_obj_loader, ss2_bin_pmnm};

pub struct ModelDetails {
    pub format: String,
    pub geometry: String,
    pub high_detail: String,
    pub is_object: bool,
}

impl ModelDetails {
    pub fn inspect(bytes: &[u8]) -> Result<Self, String> {
        crate::ui::quiet_catch(|| {
            let mut reader = Cursor::new(bytes);
            let header = ss2_bin_header::read(&mut reader);
            match header.bin_type {
                ss2_bin_header::BinFileType::Obj => {
                    let mesh = ss2_bin_obj_loader::read(&mut reader, &header);
                    Self {
                        format: format!("LGMD v{}", header.version),
                        geometry: format!(
                            "{} polygons, {} vertices",
                            mesh.polygons.len(),
                            mesh.vertices.len()
                        ),
                        high_detail: "No embedded alternative; object upgrades replace the file"
                            .into(),
                        is_object: true,
                    }
                }
                ss2_bin_header::BinFileType::Mesh => {
                    let mesh = ss2_bin_ai_loader::read(&mut reader, &header);
                    let end = reader.stream_position().unwrap() as usize;
                    let high_detail = match ss2_bin_pmnm::find_chunk(bytes, end) {
                        Some(base) => match ss2_bin_pmnm::read(bytes, base) {
                            Some(mesh) => format!(
                                "Available: {} triangles, {} vertices",
                                mesh.triangle_count(),
                                mesh.vertices.len()
                            ),
                            None => "Invalid PMNM chunk; renderer uses the classic mesh".into(),
                        },
                        None => "Not available in this file".into(),
                    };
                    Self {
                        format: format!("LGMM v{}", header.version),
                        geometry: format!(
                            "{} triangles, {} vertices (classic mesh)",
                            mesh.triangles.len(),
                            mesh.vertices.len()
                        ),
                        high_detail,
                        is_object: false,
                    }
                }
            }
        })
    }

    pub fn show(&self, ui: &mut eframe::egui::Ui, source: &str) {
        eframe::egui::Grid::new("model_details")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Mesh format");
                ui.label(&self.format);
                ui.end_row();
                if self.is_object {
                    ui.label("Selected mesh");
                    ui.label(
                        if source.replace('\\', "/").ends_with("/mods/sshock2ee.kpf")
                            || source == "mods/sshock2ee.kpf"
                        {
                            "Remastered replacement (independent of the PMNM setting)"
                        } else {
                            "Object mesh from the selected archive"
                        },
                    );
                    ui.end_row();
                }
                ui.label("Base geometry");
                ui.label(&self.geometry);
                ui.end_row();
                ui.label("High-detail PMNM");
                ui.label(&self.high_detail);
                ui.end_row();
            });
    }
}

#[cfg(test)]
mod tests {
    use super::ModelDetails;

    #[test]
    fn malformed_models_report_errors() {
        for bytes in [
            b"".as_slice(),
            b"not a model",
            b"LGMD",
            b"LGMM\x01\x00\x00\x00",
        ] {
            assert!(ModelDetails::inspect(bytes).is_err());
        }
    }

    #[test]
    #[ignore = "requires the installed 25th Anniversary game archives"]
    fn installed_remaster_details() {
        let root = shock2vr::paths::data_root();
        let remaster = root.join("mods/sshock2ee.kpf");
        let base = root.join("sshock2.kpf");
        for (name, old_count, new_count) in [("atek_h", 79, 1739), ("amp_h", 114, 1173)] {
            for (archive, entry, expected) in [
                (
                    &base,
                    format!("data/res/obj/{}.BIN", name.to_uppercase()),
                    old_count,
                ),
                (&remaster, format!("obj/{name}.bin"), new_count),
            ] {
                let bytes = crate::archives::read_entry(archive, &entry).unwrap();
                let details = ModelDetails::inspect(&bytes).unwrap();
                assert!(details.is_object);
                assert!(
                    details
                        .geometry
                        .starts_with(&format!("{expected} polygons,"))
                );
            }
        }
        let mut skinned = 0;
        for entry in crate::archives::list_entries(&remaster).unwrap() {
            if !entry.starts_with("mesh/") || !entry.ends_with(".bin") {
                continue;
            }
            let bytes = crate::archives::read_entry(&remaster, &entry).unwrap();
            let details = ModelDetails::inspect(&bytes).unwrap();
            assert!(!details.is_object, "{entry}");
            assert!(details.high_detail.starts_with("Available:"), "{entry}");
            skinned += 1;
        }
        assert_eq!(skinned, 66);
    }
}
