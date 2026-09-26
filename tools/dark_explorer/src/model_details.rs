//! Inspect the selected file independently of the renderer's high-detail gate.
//! A Nightdive LGMD is a replacement mesh; only LGMM carries a PMNM alternative.

use std::io::{Cursor, Seek};

use dark::{ss2_bin_ai_loader, ss2_bin_header, ss2_bin_obj_loader, ss2_bin_pmnm};

/// The high-detail geometry a skinned mesh carries alongside its classic one.
pub enum HighDetail {
    /// An LGMD object: an upgrade replaces the whole file, so there is no
    /// second mesh to choose between.
    NotApplicable,
    Absent,
    /// A PMNM chunk is there but did not parse - the renderer falls back.
    Invalid,
    Available {
        faces: usize,
        vertices: usize,
    },
}

pub struct ModelDetails {
    /// "LGMD" (static object) or "LGMM" (skinned mesh).
    pub format: &'static str,
    pub version: u32,
    /// Polygons for an LGMD object, triangles for an LGMM mesh.
    pub faces: usize,
    pub vertices: usize,
    pub high_detail: HighDetail,
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
                        format: "LGMD",
                        version: header.version,
                        faces: mesh.polygons.len(),
                        vertices: mesh.vertices.len(),
                        high_detail: HighDetail::NotApplicable,
                    }
                }
                ss2_bin_header::BinFileType::Mesh => {
                    let mesh = ss2_bin_ai_loader::read(&mut reader, &header);
                    // Scan for PMNM from the end of the LGMM data, as the
                    // renderer does - from 0 the vertex bytes can spoof it.
                    let end = reader.stream_position().unwrap() as usize;
                    let high_detail = match ss2_bin_pmnm::find_chunk(bytes, end) {
                        Some(base) => match ss2_bin_pmnm::read(bytes, base) {
                            Some(mesh) => HighDetail::Available {
                                faces: mesh.triangle_count(),
                                vertices: mesh.vertices.len(),
                            },
                            None => HighDetail::Invalid,
                        },
                        None => HighDetail::Absent,
                    };
                    Self {
                        format: "LGMM",
                        version: header.version,
                        faces: mesh.triangles.len(),
                        vertices: mesh.vertices.len(),
                        high_detail,
                    }
                }
            }
        })
    }

    fn face_noun(&self) -> &'static str {
        if self.format == "LGMD" {
            "polygons"
        } else {
            "triangles"
        }
    }

    /// One line for listing a shadowed copy beside the selected file.
    pub fn summary(&self) -> String {
        let mut line = format!(
            "{} v{} - {} {}, {} vertices",
            self.format,
            self.version,
            self.faces,
            self.face_noun(),
            self.vertices
        );
        if let HighDetail::Available { faces, vertices } = self.high_detail {
            line += &format!(" (+ PMNM: {faces} triangles, {vertices} vertices)");
        }
        line
    }

    pub fn show(&self, ui: &mut eframe::egui::Ui) {
        eframe::egui::Grid::new("model_details")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Mesh format");
                ui.label(format!("{} v{}", self.format, self.version));
                ui.end_row();
                ui.label("Base geometry");
                ui.label(format!(
                    "{} {}, {} vertices{}",
                    self.faces,
                    self.face_noun(),
                    self.vertices,
                    match self.high_detail {
                        HighDetail::NotApplicable => "",
                        _ => " (classic mesh)",
                    }
                ));
                ui.end_row();
                ui.label("High-detail PMNM");
                ui.label(match self.high_detail {
                    HighDetail::NotApplicable => {
                        "No embedded alternative; object upgrades replace the file".to_string()
                    }
                    HighDetail::Absent => "Not available in this file".to_string(),
                    HighDetail::Invalid => {
                        "Invalid PMNM chunk; renderer uses the classic mesh".to_string()
                    }
                    HighDetail::Available { faces, vertices } => {
                        format!("Available: {faces} triangles, {vertices} vertices")
                    }
                });
                ui.end_row();
            });
    }
}

#[cfg(test)]
mod tests {
    use super::{HighDetail, ModelDetails};

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
        for (name, old_faces, new_faces) in [("atek_h", 79, 1739), ("amp_h", 114, 1173)] {
            for (archive, entry, expected) in [
                (
                    &base,
                    format!("data/res/obj/{}.BIN", name.to_uppercase()),
                    old_faces,
                ),
                (&remaster, format!("obj/{name}.bin"), new_faces),
            ] {
                let bytes = crate::archives::read_entry(archive, &entry).unwrap();
                let details = ModelDetails::inspect(&bytes).unwrap();
                assert_eq!(details.format, "LGMD");
                assert!(matches!(details.high_detail, HighDetail::NotApplicable));
                assert_eq!(details.faces, expected);
            }
        }
        let mut skinned = 0;
        for entry in crate::archives::list_entries(&remaster).unwrap() {
            if !entry.starts_with("mesh/") || !entry.ends_with(".bin") {
                continue;
            }
            let bytes = crate::archives::read_entry(&remaster, &entry).unwrap();
            let details = ModelDetails::inspect(&bytes).unwrap();
            assert_eq!(details.format, "LGMM", "{entry}");
            assert!(
                matches!(details.high_detail, HighDetail::Available { .. }),
                "{entry}"
            );
            skinned += 1;
        }
        assert_eq!(skinned, 66);
    }
}
