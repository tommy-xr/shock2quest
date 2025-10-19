use std::{
    any::{Any, TypeId},
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    rc::Rc,
};
use tracing::{self, debug, info};

use super::{asset_importer::AssetImporter, asset_paths::AbstractAssetPath};

type ImporterAssetMap = HashMap<TypeId, HashMap<u64, HashMap<String, Option<Rc<dyn Any>>>>>;

pub struct AssetCache {
    base_path: String,
    importer_to_assets: ImporterAssetMap,
    path: Rc<Box<dyn AbstractAssetPath>>,
}

impl AssetCache {
    pub fn new(base_path: String, path: Box<dyn AbstractAssetPath>) -> AssetCache {
        AssetCache {
            base_path,
            path: Rc::new(path),
            importer_to_assets: HashMap::new(),
        }
    }

    pub fn load_from_cache<TData: 'static, TOutput: 'static, TConfig: 'static + Hash + Default>(
        &mut self,
        importer: &AssetImporter<TData, TOutput, TConfig>,
        asset_name: &str,
        config_hash: u64,
    ) -> Option<Rc<TOutput>> {
        let type_id = importer.type_id();

        let config_map = self.importer_to_assets.get(&type_id)?;
        let inner_map = config_map.get(&config_hash)?;
        let maybe_asset_inner = inner_map.get(asset_name)?;

        if maybe_asset_inner.is_none() {
            return None;
        }

        let asset = maybe_asset_inner.clone().unwrap();
        Some(asset.downcast::<TOutput>().unwrap())
    }

    pub fn get_opt<TData: 'static, TOutput: 'static, TConfig: 'static + Hash + Default>(
        &mut self,
        importer: &AssetImporter<TData, TOutput, TConfig>,
        asset_name: &str,
    ) -> Option<Rc<TOutput>> {
        let config = TConfig::default();
        self.get_ext_opt(importer, asset_name, &config)
    }

    pub fn get_ext_opt<TData: 'static, TOutput: 'static, TConfig: 'static + Hash + Default>(
        &mut self,
        importer: &AssetImporter<TData, TOutput, TConfig>,
        asset_name: &str,
        config: &TConfig,
    ) -> Option<Rc<TOutput>> {
        let type_id = importer.type_id();
        debug!(
            "Loading asset for importer: {:?} name: {}",
            type_id, asset_name
        );

        let asset_name = asset_name.to_ascii_lowercase();

        let mut hasher = DefaultHasher::new();
        config.hash(&mut hasher);
        let config_hash = hasher.finish();

        let try_preload = self.load_from_cache(importer, &asset_name, config_hash);

        if try_preload.is_some() {
            return try_preload;
        }

        // Create a separate asset cache to load sub-requests
        // TODO: Merge any sub-loaded assets into the top-level cache
        let mut temp_cache = AssetCache {
            base_path: self.base_path.clone(),
            path: self.path.clone(),
            importer_to_assets: self.importer_to_assets.clone(),
        };

        let config_to_reader = self
            .importer_to_assets
            .entry(type_id)
            .or_default();

        let name_to_reader = config_to_reader
            .entry(config_hash)
            .or_default();

        let asset = name_to_reader.entry(asset_name.clone()).or_insert_with(|| {
            info!(
                "-- Not cached, loading asset \"{}\" from path...",
                asset_name
            );
            let maybe_reader = self
                .path
                .get_reader(self.base_path.clone(), asset_name.clone());

            if let Some(reader) = maybe_reader {
                // TODO: Cache intermediate step, so the same 'read' asset can be used multiple places
                let read_asset = (importer.loader)(
                    asset_name.clone(),
                    &mut reader.borrow_mut(),
                    &mut temp_cache,
                    config,
                );
                let processed_asset = (importer.processor)(read_asset, &mut temp_cache, config);
                // TODO: Cache any assets produced by the processor
                Some(Rc::new(processed_asset))
            } else {
                // If we weren't able to load the asset, we should store that result, so we
                // don't keep trying to load it repeatedly
                None
            }
        });

        match asset {
            Some(v) => {
                let a = v.clone();
                Some(a.downcast::<TOutput>().unwrap())
            }
            None => None,
        }
    }

    pub fn get_ext<TData: 'static, TOutput: 'static, TConfig: 'static + Hash + Default>(
        &mut self,
        importer: &AssetImporter<TData, TOutput, TConfig>,
        asset_name: &str,
        config: &TConfig,
    ) -> Rc<TOutput> {
        self.get_ext_opt(importer, asset_name, config).unwrap()
    }

    pub fn get<TData: 'static, TOutput: 'static, TConfig: 'static + Hash + Default>(
        &mut self,
        importer: &AssetImporter<TData, TOutput, TConfig>,
        asset_name: &str,
    ) -> Rc<TOutput> {
        self.get_opt(importer, asset_name).unwrap()
    }
}
