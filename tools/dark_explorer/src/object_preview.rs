//! LGMD inspection lives in the normal model preview: visibility is geometry
//! selection, independent of the optional pivot overlay and of clip playback.
use std::collections::BTreeMap;

use cgmath::{
    Deg, InnerSpace, Matrix4, Point3, Quaternion, Rotation3, SquareMatrix, Transform, Vector3, vec3,
};
use dark::{
    model::Model,
    motion::AnimationPlayer,
    ss2_bin_obj_loader::{self as obj, SystemShock2ObjectMesh},
    weapon_animation::{self, WeaponAnimation},
};
use dark_viewer::scenes::build_model_scene_with_debug_skeletons;
use eframe::egui;
use engine::{
    assets::{asset_cache::AssetCache, text_importer::TEXT_IMPORTER},
    scene::Scene,
};

/// Optional initial state for repeatable captures. Every control is also in UI.
#[derive(clap::Args, Clone, Default, PartialEq)]
pub struct ObjectPreviewOptions {
    /// Select a Nightdive weapon clip (e.g. reload); controls also appear in the UI
    #[arg(long)]
    pub weapon_clip: Option<String>,
    /// Show only this zero-based LGMD sub-object index
    #[arg(long)]
    pub isolate_joint: Option<usize>,
    /// Show only this material (case-insensitive authored name)
    #[arg(long)]
    pub isolate_material: Option<String>,
    /// Set an initial scalar parameter, e.g. --joint 2=-2 (one-based Nightdive joint)
    #[arg(long, value_parser = parse_joint)]
    pub joint: Vec<(i32, f32)>,
}

fn parse_joint(value: &str) -> Result<(i32, f32), String> {
    let (id, value) = value.split_once('=').ok_or("expected joint=value")?;
    let id = id.parse::<i32>().map_err(|_| "invalid joint number")?;
    let value = value.parse::<f32>().map_err(|_| "invalid joint value")?;
    if id <= 0 || !value.is_finite() {
        return Err("joint must be positive and value finite".into());
    }
    Ok((id - 1, value))
}

pub struct ObjectPreview {
    mesh: SystemShock2ObjectMesh,
    model: Model,
    visible_joints: Vec<bool>,
    materials: BTreeMap<String, bool>,
    parameters: BTreeMap<i32, f32>,
    clips: BTreeMap<String, WeaponAnimation>,
    category: Option<&'static str>,
    clip: Option<String>,
    frame: f32,
    playing: bool,
    looped: bool,
    root_motion: bool,
    dirty_geometry: bool,
    pub frame_selection: bool,
}

impl ObjectPreview {
    pub fn load(
        key: &str,
        cache: &mut AssetCache,
        options: &ObjectPreviewOptions,
    ) -> Result<Option<Self>, String> {
        let Some(reader) = cache.get_raw_reader(key) else {
            return Ok(None);
        };
        let mut reader = reader.into_inner();
        let header = dark::ss2_bin_header::read(&mut reader);
        if !matches!(header.bin_type, dark::ss2_bin_header::BinFileType::Obj) {
            return Ok(None);
        }
        let mesh = obj::read(&mut reader, &header);
        let model = Model::from_obj_bin(mesh.clone(), cache);
        let category = category(key);
        let mut clips = BTreeMap::new();
        if let Some(category) = category {
            if let Some(source) = cache.get_opt(&TEXT_IMPORTER, "sq_scripts/animations_weapons.nut")
            {
                let data = weapon_animation::parse(&source);
                // Include the same default fallbacks that the runtime resolves.
                for name in ["default", category] {
                    if let Some(authored) = data.by_category.get(name) {
                        clips.extend(
                            authored
                                .iter()
                                .map(|(name, clip)| (name.clone(), clip.clone())),
                        );
                    }
                }
            }
        }
        let visible_joints = vec![true; mesh.sub_objects.len()];
        let materials = mesh
            .materials
            .iter()
            .map(|m| (m.name.clone(), true))
            .collect();
        let parameters = mesh
            .sub_objects
            .iter()
            .filter(|s| s.parameter >= 0 && s.motion_type != 0)
            .map(|s| (s.parameter, 0.0))
            .collect();
        let mut preview = Self {
            mesh,
            model,
            visible_joints,
            materials,
            parameters,
            clips,
            category,
            clip: None,
            frame: 0.0,
            playing: false,
            looped: true,
            root_motion: true,
            dirty_geometry: false,
            frame_selection: false,
        };
        if let Some(index) = options.isolate_joint {
            if index >= preview.visible_joints.len() {
                return Err(format!("No sub-object {index} in {key}"));
            }
            preview.visible_joints.fill(false);
            preview.visible_joints[index] = true;
            preview.dirty_geometry = true;
        }
        if let Some(name) = &options.isolate_material {
            if !preview
                .materials
                .keys()
                .any(|m| m.eq_ignore_ascii_case(name))
            {
                return Err(format!("No material {name} in {key}"));
            }
            for (material, visible) in &mut preview.materials {
                *visible = material.eq_ignore_ascii_case(name);
            }
            preview.dirty_geometry = true;
        }
        if let Some(name) = &options.weapon_clip {
            if !preview.clips.contains_key(name) {
                return Err(format!("No Nightdive clip {name} for {key}"));
            }
            preview.clip = Some(name.clone());
            preview.playing = true;
        }
        if !options.joint.is_empty() && options.weapon_clip.is_some() {
            return Err("Choose a clip or manual joint values".into());
        }
        for (id, value) in &options.joint {
            *preview
                .parameters
                .get_mut(id)
                .ok_or_else(|| format!("No joint{} in {key}", id + 1))? = *value;
        }
        preview.frame_selection = preview.dirty_geometry;
        preview.rebuild(cache);
        Ok(Some(preview))
    }

    pub fn is_playing(&self) -> bool {
        self.playing && self.clip.is_some()
    }

    pub fn advance(&mut self, seconds: f32) {
        let Some(clip) = self.clip.as_ref().and_then(|name| self.clips.get(name)) else {
            return;
        };
        self.frame += seconds * clip.fps;
        if self.frame > clip.length {
            if self.looped && clip.length > 0.0 {
                self.frame %= clip.length;
            } else {
                self.frame = clip.length;
                self.playing = false;
            }
        }
        self.sample();
    }

    fn sample(&mut self) {
        let Some(clip) = self.clip.as_ref().and_then(|name| self.clips.get(name)) else {
            return;
        };
        for (id, value) in &mut self.parameters {
            *value = clip
                .sample(&format!("joint{}", id + 1), self.frame)
                .map(|(pos, _)| pos.x)
                .unwrap_or(0.0);
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, cache: &mut AssetCache) -> bool {
        let mut changed = false;
        if !self.clips.is_empty() {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "Nightdive · {}",
                    self.category.unwrap_or("default")
                ));
                let before = self.clip.clone();
                egui::ComboBox::from_id_salt("weapon_clip")
                    .selected_text(self.clip.as_deref().unwrap_or("Manual / rest pose"))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.clip, None, "Manual / rest pose");
                        for name in self.clips.keys() {
                            ui.selectable_value(&mut self.clip, Some(name.clone()), name);
                        }
                    });
                if before != self.clip {
                    self.frame = 0.0;
                    self.parameters.values_mut().for_each(|v| *v = 0.0);
                    self.playing = self.clip.is_some();
                    changed = true;
                }
                if self.clip.is_some() {
                    if ui
                        .button(if self.playing { "Pause" } else { "Play" })
                        .clicked()
                    {
                        if !self.playing
                            && self.frame >= self.clips[self.clip.as_ref().unwrap()].length
                        {
                            self.frame = 0.0;
                        }
                        self.playing = !self.playing;
                        changed = true;
                    }
                    ui.checkbox(&mut self.looped, "Loop");
                }
            });
            if let Some(clip) = self.clip.as_ref().and_then(|name| self.clips.get(name)) {
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Slider::new(&mut self.frame, 0.0..=clip.length).text("Frame"))
                        .changed()
                    {
                        self.playing = false;
                        changed = true;
                    }
                    ui.label(format!(
                        "{:.2}s / {:.2}s",
                        self.frame / clip.fps,
                        clip.duration_seconds()
                    ));
                    changed |= ui
                        .checkbox(&mut self.root_motion, "Whole-weapon motion")
                        .changed();
                });
            }
        } else if self.category.is_some() {
            ui.label("Nightdive animation data unavailable; manual joints still work.");
        }
        self.sample();
        egui::CollapsingHeader::new("Parts and joints")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Show all").clicked() {
                        self.visible_joints.fill(true);
                        self.materials.values_mut().for_each(|v| *v = true);
                        self.dirty_geometry = true;
                    }
                    if ui.button("Rest pose").clicked() {
                        self.clip = None;
                        self.playing = false;
                        self.frame = 0.0;
                        self.parameters.values_mut().for_each(|v| *v = 0.0);
                        changed = true;
                    }
                    if ui.button("Frame visible").clicked() {
                        self.frame_selection = true;
                    }
                });
                egui::ScrollArea::vertical()
                    .id_salt("object_parts")
                    .max_height(165.0)
                    .show(ui, |ui| {
                        for (index, joint) in self.mesh.sub_objects.iter().enumerate() {
                            ui.push_id(index, |ui| {
                                ui.horizontal(|ui| {
                                    self.dirty_geometry |= ui
                                        .checkbox(
                                            &mut self.visible_joints[index],
                                            format!("{index}: {}", joint.name),
                                        )
                                        .changed();
                                    if ui.small_button("Solo").clicked() {
                                        self.visible_joints.fill(false);
                                        self.visible_joints[index] = true;
                                        self.materials.values_mut().for_each(|v| *v = true);
                                        self.dirty_geometry = true;
                                        self.frame_selection = true;
                                    }
                                    if let Some(value) = self.parameters.get_mut(&joint.parameter) {
                                        let range = if joint.motion_type == 1 {
                                            -180.0..=180.0
                                        } else {
                                            -5.0..=5.0
                                        };
                                        let unit = if joint.motion_type == 1 {
                                            "°"
                                        } else {
                                            " Dark units"
                                        };
                                        if ui
                                            .add(
                                                egui::Slider::new(value, range)
                                                    .clamping(egui::SliderClamping::Never)
                                                    .text(format!("joint{}", joint.parameter + 1))
                                                    .suffix(unit),
                                            )
                                            .changed()
                                        {
                                            self.clip = None;
                                            self.playing = false;
                                            changed = true;
                                        }
                                    }
                                })
                            });
                        }
                        ui.separator();
                        ui.label("Materials (separate a hand from a magazine on the same joint)");
                        let mut solo = None;
                        for (name, visible) in &mut self.materials {
                            ui.horizontal(|ui| {
                                self.dirty_geometry |= ui.checkbox(visible, name).changed();
                                if ui.small_button("Solo").clicked() {
                                    solo = Some(name.clone());
                                }
                            });
                        }
                        if let Some(solo) = solo {
                            self.visible_joints.fill(true);
                            for (name, visible) in &mut self.materials {
                                *visible = *name == solo;
                            }
                            self.dirty_geometry = true;
                            self.frame_selection = true;
                        }
                    });
            });
        changed |= self.dirty_geometry;
        self.rebuild(cache);
        changed
    }

    fn filtered_mesh(&self) -> SystemShock2ObjectMesh {
        let mesh = obj::retain_sub_objects(self.mesh.clone(), |index| {
            self.visible_joints[index as usize]
        });
        obj::retain_materials(mesh, |name| {
            self.materials.get(name).copied().unwrap_or(false)
        })
    }

    fn rebuild(&mut self, cache: &mut AssetCache) {
        if self.dirty_geometry {
            self.model = Model::from_obj_bin(self.filtered_mesh(), cache);
            self.dirty_geometry = false;
        }
    }

    fn player(&self) -> AnimationPlayer {
        let mut player = AnimationPlayer::empty();
        if let Some(rig) = self.model.object_articulation() {
            let parameters = self
                .parameters
                .iter()
                .map(|(id, value)| (*id, *value))
                .collect::<Vec<_>>();
            for (joint, transform) in rig.joint_transforms(&parameters) {
                player = AnimationPlayer::set_additional_joint_transform(&player, joint, transform);
            }
        }
        player
    }

    fn root(&self) -> Matrix4<f32> {
        if self.root_motion {
            if let Some((pos, rot)) = self
                .clip
                .as_ref()
                .and_then(|name| self.clips.get(name))
                .and_then(|clip| clip.sample("gunPoint", self.frame))
            {
                // Same PointRig heading/pitch/bank basis as the flat runtime.
                let rotation = Quaternion::from_angle_y(Deg(rot.x))
                    * Quaternion::from_angle_z(Deg(rot.y))
                    * Quaternion::from_angle_x(Deg(-rot.z));
                return Matrix4::from_translation(vec3(-pos.x, pos.z, pos.y) / dark::SCALE_FACTOR)
                    * Matrix4::from(rotation);
            }
        }
        Matrix4::identity()
    }

    pub fn visible_bounds(&self) -> Option<(Vector3<f32>, f32)> {
        let pose = if self.model.object_articulation().is_some() {
            self.model.get_joint_transforms(&self.player())
        } else {
            obj::obj_skeleton(&self.mesh).get_transforms()
        };
        let root = self.root();
        let mut min = vec3(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let mut max = -min;
        let mut any = false;
        for vertices in obj::to_vertices(&self.filtered_mesh()).values() {
            for v in vertices {
                let p = (root * pose[v.bone_indices[0] as usize]).transform_point(Point3::new(
                    v.position.x,
                    v.position.y,
                    v.position.z,
                ));
                min = vec3(min.x.min(p.x), min.y.min(p.y), min.z.min(p.z));
                max = vec3(max.x.max(p.x), max.y.max(p.y), max.z.max(p.z));
                any = true;
            }
        }
        any.then(|| ((min + max) * 0.5, ((max - min).magnitude() * 0.5).max(0.05)))
    }

    pub fn render(&self, skeletons: bool, hitboxes: bool, articulation: bool) -> Scene {
        let player = self.player();
        let mut scene = build_model_scene_with_debug_skeletons(
            &self.model,
            Some(&player),
            self.model.to_animated_scene_objects(&player),
            Vec::new(),
            skeletons,
            hitboxes,
            articulation,
        );
        let root = self.root();
        for object in &mut scene.objects {
            object.set_transform(root * object.transform);
        }
        scene
    }
}

fn category(key: &str) -> Option<&'static str> {
    match key.to_ascii_lowercase().trim_end_matches(".bin") {
        "atek_h" => Some("Pistol"),
        "sg_h" => Some("Shotgun"),
        "ar15_h" => Some("Assault Rifle"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_joint_arguments_are_one_based_and_finite() {
        assert_eq!(parse_joint("2=-2"), Ok((1, -2.0)));
        for invalid in ["0=1", "-1=2", "1=NaN", "1=inf", "1=", "2"] {
            assert!(parse_joint(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    #[ignore = "requires installed 25th Anniversary archives"]
    fn installed_magazines_are_separable_and_reload_drives_their_parameters() {
        let archive = shock2vr::paths::data_root().join("mods/sshock2ee.kpf");
        let clips =
            crate::archives::read_entry(&archive, "sq_scripts/animations_weapons.nut").unwrap();
        let clips = weapon_animation::parse(std::str::from_utf8(&clips).unwrap());
        for (name, index, material, parameter, faces, frame, displacement) in [
            ("atek_h", 1, Some("ND-ammo1.psd"), 1, 79, 20.0, -2.0),
            ("ar15_h", 2, None, 2, 27, 15.0, 5.0),
        ] {
            let bytes = crate::archives::read_entry(&archive, &format!("obj/{name}.bin")).unwrap();
            let mut reader = std::io::Cursor::new(bytes);
            let header = dark::ss2_bin_header::read(&mut reader);
            let mesh = obj::read(&mut reader, &header);
            assert_eq!(mesh.sub_objects[index].parameter, parameter);
            let mesh = obj::retain_sub_objects(mesh, |i| i as usize == index);
            let mesh = obj::retain_materials(mesh, |m| material.is_none_or(|wanted| m == wanted));
            assert_eq!(mesh.polygons.len(), faces, "{name}");
            assert!(!obj::to_vertices(&mesh).is_empty());
            let clip = clips.get(category(name).unwrap(), "reload").unwrap();
            let joint = format!("joint{}", parameter + 1);
            assert_eq!(clip.sample(&joint, 0.0).unwrap().0.x, 0.0);
            assert_eq!(clip.sample(&joint, frame).unwrap().0.x, displacement);
            assert_eq!(clip.sample(&joint, clip.length).unwrap().0.x, 0.0);
            if name == "ar15_h" {
                assert_eq!(
                    clip.track("gunPoint").unwrap().keys.last().unwrap().frame,
                    95.0
                );
                assert_eq!(clip.sample(&joint, 32.0).unwrap().0.x, 0.0);
            }
        }
    }
}
