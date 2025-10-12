use super::asset_cache::AssetCache;
use super::asset_paths::ReadableAndSeekable;

pub type AssetLoader<T, TConfig> = fn(
    asset_name: String,
    reader: &mut Box<dyn ReadableAndSeekable>,
    cache: &mut AssetCache,
    config: &TConfig,
) -> T;

pub type AssetProcessor<TData, TOutput, TConfig> =
    fn(data: TData, asset_cache: &mut AssetCache, config: &TConfig) -> TOutput;

pub struct AssetImporter<TData, TOutput, TConfig> {
    pub loader: AssetLoader<TData, TConfig>,
    pub processor: AssetProcessor<TData, TOutput, TConfig>,
}

impl<TData, TOutput, TConfig> AssetImporter<TData, TOutput, TConfig> {
    pub fn define(
        loader: AssetLoader<TData, TConfig>,
        processor: AssetProcessor<TData, TOutput, TConfig>,
    ) -> AssetImporter<TData, TOutput, TConfig> {
        AssetImporter { loader, processor }
    }
}
