use std::{any::TypeId, collections::HashMap};

use bevy_asset::{AssetLoader, AssetPath, LoadContext, LoadDirectError, io::Reader};
use bevy_image::Image;
use bevy_math::{URect, UVec2};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    TileSourceIndex, Tileset,
    importer::{TileFilter, TilesetImportData, TilesetImporter, TilesetSource},
    layout::{TileFrame, TilesetLayout},
};

pub type DataProcess = TilesetImporter<DataTilesetLoader>;

pub const DATA_EXTS: &[&str] = &["ts.ron"];

#[derive(Debug, Serialize, Deserialize)]
pub struct DataTileset {
    pub tile_size: UVec2,
    #[serde(default)]
    pub tile_filter: TileFilter,
    #[serde(default)]
    pub tile_groups: HashMap<String, Vec<TileSourceIndex>>,
    pub sources: Vec<DataTilesetSource>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DataTilesetSource {
    pub path: AssetPath<'static>,
    #[serde(default)]
    pub layout: DataSourceLayout,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub enum DataSourceLayout {
    #[default]
    Auto,
    Grid {
        #[serde(default)]
        padding: UVec2,
        #[serde(default)]
        margins: URect,
    },
    Frames(Vec<TileFrame>),
}

impl DataSourceLayout {
    pub fn into_layout(self) -> TilesetLayout {
        match self {
            Self::Auto => TilesetLayout::unpadded_grid(),
            Self::Grid { padding, margins } => TilesetLayout::Grid { padding, margins },
            Self::Frames(frames) => TilesetLayout::Frames(frames),
        }
    }
}

#[derive(Debug, Default)]
pub struct DataTilesetLoader;

impl AssetLoader for DataTilesetLoader {
    type Asset = TilesetImportData;
    type Settings = ();
    type Error = DataTilesetError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        &(): &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;

        let DataTileset {
            tile_size,
            tile_filter,
            tile_groups,
            sources,
        } = ron::de::from_bytes(&bytes)?;

        let mut loaded_sources = Vec::new();
        for DataTilesetSource { path, layout } in sources {
            let source_asset = load_context
                .loader()
                .immediate()
                .with_unknown_type()
                .load(&path)
                .await?;

            let asset_type_id = source_asset.asset_type_id();
            let texture = if asset_type_id == TypeId::of::<Image>() {
                source_asset.take::<Image>().unwrap()
            } else if asset_type_id == TypeId::of::<Tileset>() {
                let tileset = source_asset.downcast::<Tileset>().ok().unwrap();
                tileset
                    .get_labeled("texture")
                    .and_then(|erased| erased.get::<Image>())
                    .ok_or(DataTilesetError::InvalidSourceTexture(path))?
                    .clone()
            } else {
                return Err(DataTilesetError::UnknownSourceType(
                    path,
                    source_asset.asset_type_name(),
                ));
            };

            loaded_sources.push(TilesetSource {
                texture,
                layout: layout.into_layout(),
            });
        }

        Ok(TilesetImportData {
            tile_size,
            tile_filter,
            tile_groups: tile_groups.into_iter().collect(),
            sources: loaded_sources,
        })
    }

    fn extensions(&self) -> &[&str] {
        DATA_EXTS
    }
}

#[derive(Debug, Error)]
pub enum DataTilesetError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Deserialize(#[from] ron::de::SpannedError),
    #[error(transparent)]
    LoadSource(#[from] LoadDirectError),
    #[error("tileset source {0:?} was loaded as unknown type `{1}`")]
    UnknownSourceType(AssetPath<'static>, &'static str),
    #[error("unable to get texture from source asset {0:?}")]
    InvalidSourceTexture(AssetPath<'static>),
}
