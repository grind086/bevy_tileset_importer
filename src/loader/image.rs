use std::sync::LazyLock;

use bevy_asset::{AssetLoader, LoadContext, io::Reader};
use bevy_image::{CompressedImageFormats, ImageLoader, ImageLoaderError, ImageLoaderSettings};
use bevy_platform::collections::HashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    TileIndex,
    import::{TilesetImportData, TilesetSource},
    layout::{LayoutError, TileFilter, TilesetLayout},
    loader::TILESET_EXT,
};

pub struct ImageTilesetLoader {
    image_loader: ImageLoader,
    default_settings: ImageLoaderSettings,
}

impl Default for ImageTilesetLoader {
    fn default() -> Self {
        Self {
            image_loader: ImageLoader::new(CompressedImageFormats::NONE),
            default_settings: ImageLoaderSettings::default(),
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageTilesetSettings {
    pub layout: TilesetLayout,
    pub filter: TileFilter<TileIndex>,
    pub groups: HashMap<String, Vec<TileIndex>>,
    pub image_settings: Option<ImageLoaderSettings>,
    pub generate_mips: bool,
}

impl AssetLoader for ImageTilesetLoader {
    type Asset = TilesetImportData;
    type Settings = ImageTilesetSettings;
    type Error = ImageTilesetError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        ImageTilesetSettings {
            layout,
            filter,
            groups,
            image_settings,
            generate_mips,
        }: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let image_settings = image_settings.as_ref().unwrap_or(&self.default_settings);

        let source = self
            .image_loader
            .load(reader, image_settings, load_context)
            .await?;

        let tile_info = layout.tile_info(source.size())?;

        Ok(TilesetImportData {
            tile_size: tile_info.size,
            tile_indices: filter.indices(0, tile_info.count),
            tile_groups: groups
                .iter()
                .map(|(name, tiles)| (name.clone(), tiles.iter().map(|i| (0, *i)).collect()))
                .collect(),
            texture_format: source.texture_descriptor.format,
            sampler: source.sampler.clone(),
            sources: vec![TilesetSource {
                source,
                layout: layout.clone(),
            }],
            generate_mips: *generate_mips,
        })
    }

    fn extensions(&self) -> &[&str] {
        &IMAGE_EXTS
    }
}

pub static IMAGE_EXTS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    ImageLoader::SUPPORTED_FILE_EXTENSIONS
        .iter()
        .map(|ext| format!("{TILESET_EXT}.{ext}"))
        .map(|ext| &*String::leak(ext))
        .collect()
});

#[derive(Debug, Error)]
pub enum ImageTilesetError {
    #[error(transparent)]
    Image(#[from] ImageLoaderError),
    #[error("`TilesetLayout` is incompatible with its source image: {0}")]
    Layout(#[from] LayoutError),
}
