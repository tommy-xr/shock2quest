use super::ToolScene;
use dark::{ss2_bin_ai_loader, ss2_bin_header, ss2_cal_loader, ss2_skeleton};
use dark::ss2_skeleton::Skeleton;
use engine::assets::asset_cache::AssetCache;
use engine::scene::Scene;
use dark::model::Model;
use std::fs::File;
use std::io::BufReader;
use std::rc::Rc;
use std::time::Duration;

pub struct BinAiViewerScene {
    mesh_file_path: String,
    skeleton_file_path: String,
    skeleton: Option<Rc<Skeleton>>,
    ai_mesh: Option<ss2_bin_ai_loader::SystemShock2AIMesh>,
    total_time: Duration,
}

impl BinAiViewerScene {
    pub fn from_files(mesh_file_path: String, skeleton_file_path: String, resource_path_fn: fn(&str) -> String) -> Result<Self, Box<dyn std::error::Error>> {
        // Load skeleton
        let skeleton_file = File::open(resource_path_fn(&skeleton_file_path))?;
        let mut skeleton_reader = BufReader::new(skeleton_file);
        let ss2_cal = ss2_cal_loader::read(&mut skeleton_reader);
        let skeleton = Rc::new(ss2_skeleton::create(ss2_cal));

        // Load AI mesh
        let mesh_file = File::open(resource_path_fn(&mesh_file_path))?;
        let mut mesh_reader = BufReader::new(mesh_file);
        let header = ss2_bin_header::read(&mut mesh_reader);
        let ai_mesh = ss2_bin_ai_loader::read(&mut mesh_reader, &header);

        Ok(BinAiViewerScene {
            mesh_file_path,
            skeleton_file_path,
            skeleton: Some(skeleton),
            ai_mesh: Some(ai_mesh),
            total_time: Duration::ZERO,
        })
    }
}

impl ToolScene for BinAiViewerScene {
    fn update(&mut self, delta_time: f32) {
        let elapsed = Duration::from_secs_f32(delta_time);
        self.total_time += elapsed;
    }

    fn render(&self, asset_cache: &mut AssetCache) -> Scene {
        let mut scene = vec![];

        if let (Some(ai_mesh), Some(skeleton)) = (&self.ai_mesh, &self.skeleton) {
            let model = Model::from_ai_bin(ai_mesh.clone(), skeleton.clone(), asset_cache);
            for obj in model.to_scene_objects() {
                scene.push(obj.clone());
            }
        }

        Scene::from_objects(scene)
    }
}