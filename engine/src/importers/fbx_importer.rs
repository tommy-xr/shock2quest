use std::rc::Rc;

use fbxcel_dom::{
    any::AnyDocument,
    v7400::{
        object::{model::TypedModelHandle, TypedObjectHandle},
        Document,
    },
};
use once_cell::sync::Lazy;

use crate::{
    assets::{self, asset_cache::AssetCache, asset_importer::AssetImporter},
    scene::SceneObject,
};

fn load_fbx(
    _name: String,
    reader: &mut Box<dyn assets::asset_paths::ReadableAndSeekable>,
    _assets: &mut AssetCache,
    _config: &(),
) -> Box<Document> {
    match AnyDocument::from_seekable_reader(reader).expect("Failed to load document") {
        AnyDocument::V7400(ver, doc) => {
            println!("Loaded FBX DOM successfully: FBX version = {:?}", ver);
            for scene in doc.scenes() {
                println!("Scene object: object_id={:?}", scene.object_id());
                let root_id = scene
                    .root_object_id()
                    .expect("Failed to get root object ID");
                println!("\tRoot object ID: {:?}", root_id);
            }

            for obj in doc.objects() {
                println!("name: {:?}", obj.name());
                if let TypedObjectHandle::Model(TypedModelHandle::Mesh(mesh)) = obj.get_typed() {
                    let geometry_obj = mesh.geometry().unwrap();
                    // Get vertices
                    let _vertices = geometry_obj
                        .polygon_vertices()
                        .expect("Failed to get vertices");
                    //println!("Vertices: {:?}", vertices);

                    // // Get polygon vertex indices
                    // let indices = mesh.ind.expect("Failed to get indices");
                    // println!("Indices: {:?}", indices.to_vec());

                    // // Get UVs
                    // if let Some(layer_element_uv) = mesh.layer_elements_uv().next() {
                    //     let uvs = layer_element_uv.uv().expect("Failed to get UVs");
                    //     println!("UVs: {:?}", uvs.to_vec());
                    // }
                }
            }
            panic!("done");
            return doc;
        }
        _ => panic!("FBX version unsupported by this example"),
    }
}

fn process_fbx(
    _fbx_document: Box<Document>,
    _assets: &mut AssetCache,
    _config: &(),
) -> Vec<SceneObject> {
    vec![]
}

pub static FBX_IMPORTER: Lazy<AssetImporter<Box<Document>, Vec<SceneObject>, ()>> =
    Lazy::new(|| AssetImporter::define(load_fbx, process_fbx));
