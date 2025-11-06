use bevy_asset::{AssetLoader, LoadContext, io::Reader};
use bevy_image::{
    CompressedImageFormats, ImageFormatSetting, ImageLoader, ImageLoaderError, ImageLoaderSettings,
};
use bevy_math::{URect, UVec2};
use bevy_platform::collections::HashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use wgpu_types::TextureFormat;

use crate::{
    TileIndex,
    importer::{TileFilter, TilesetImportData, TilesetImporter, TilesetSource},
    layout::{TileFrame, TilesetLayout},
};

pub type ImageProcess = TilesetImporter<ImageTilesetLoader>;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ImageTilesetSettings {
    pub layout: ImageLayoutSetting,
    pub tile_filter: ImageTileFilter,
    pub tile_groups: HashMap<String, Vec<TileIndex>>,
    pub format: ImageFormatSetting,
    pub texture_format: Option<TextureFormat>,
    pub is_srgb: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub enum ImageLayoutSetting {
    #[default]
    Single,
    Grid {
        tile_size: UVec2,
        padding: UVec2,
        margins: URect,
    },
    Frames {
        tile_size: UVec2,
        frames: Vec<TileFrame>,
    },
}

impl ImageLayoutSetting {
    fn to_layout_and_tile_size(&self, image_size: UVec2) -> (TilesetLayout, UVec2) {
        match *self {
            Self::Single => (TilesetLayout::unpadded_grid(), image_size),
            Self::Grid {
                tile_size,
                padding,
                margins,
            } => (TilesetLayout::Grid { padding, margins }, tile_size),
            Self::Frames {
                tile_size,
                ref frames,
            } => (TilesetLayout::Frames(frames.clone()), tile_size),
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub enum ImageTileFilter {
    #[default]
    All,
    None,
    List(Vec<TileIndex>),
}

impl ImageTileFilter {
    fn to_filter(&self, source_id: usize) -> TileFilter {
        match self {
            Self::All => TileFilter::All,
            Self::None => TileFilter::None,
            Self::List(list) => TileFilter::List(
                list.iter()
                    .map(|tile_index| (source_id, *tile_index))
                    .collect(),
            ),
        }
    }
}

pub struct ImageTilesetLoader {
    image_loader: ImageLoader,
}

impl Default for ImageTilesetLoader {
    fn default() -> Self {
        Self {
            image_loader: ImageLoader::new(CompressedImageFormats::NONE),
        }
    }
}

impl AssetLoader for ImageTilesetLoader {
    type Asset = TilesetImportData;
    type Settings = ImageTilesetSettings;
    type Error = ImageTilesetError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        &ImageTilesetSettings {
            ref layout,
            ref format,
            ref tile_groups,
            ref tile_filter,
            texture_format,
            is_srgb,
        }: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let image_settings = ImageLoaderSettings {
            format: format.clone(),
            texture_format,
            is_srgb,
            ..Default::default()
        };

        let texture = self
            .image_loader
            .load(reader, &image_settings, load_context)
            .await?;

        let (layout, tile_size) = layout.to_layout_and_tile_size(texture.size());
        let tile_filter = tile_filter.to_filter(0);
        let tile_groups = tile_groups
            .iter()
            .map(|(name, group)| {
                (
                    name.clone(),
                    group.iter().map(|tile_index| (0, *tile_index)).collect(),
                )
            })
            .collect();

        Ok(TilesetImportData {
            tile_size,
            tile_filter,
            tile_groups,
            sources: vec![TilesetSource { texture, layout }],
        })
    }
}

#[derive(Debug, Error)]
pub enum ImageTilesetError {
    #[error(transparent)]
    LoadImage(#[from] ImageLoaderError),
}
