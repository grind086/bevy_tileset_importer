use bevy_asset::{AssetLoader, AssetPath, LoadContext, LoadDirectError, io::Reader};
use bevy_image::{Image, ImageLoaderSettings, ImageSampler};
use bevy_math::UVec2;
use bevy_platform::collections::HashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use wgpu_types::TextureFormat;

use crate::{
    TileSourceIndex,
    helpers::SetOrExpected,
    import::{TilesetImportData, TilesetSource},
    layout::{LayoutError, TileFilter, TileInfo, TilesetLayout},
};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DataTileset {
    /// Explicitly sets the global tile size. If not set, it will be detected from the first
    /// source.
    #[serde(default)]
    pub tile_size: Option<UVec2>,
    pub sources: Vec<DataTilesetSource>,
    #[serde(default)]
    pub filter: TileFilter<TileSourceIndex>,
    #[serde(default)]
    pub groups: HashMap<String, Vec<TileSourceIndex>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DataTilesetSource {
    pub path: AssetPath<'static>,
    #[serde(default)]
    pub layout: TilesetLayout,
    #[serde(default)]
    pub image_settings: Option<ImageLoaderSettings>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DataTilesetSettings {
    /// The [`ImageSampler`] to use for the loaded tileset texture.
    pub sampler: ImageSampler,
    /// Explicitly set the desired [`TextureFormat`] of the loaded tileset. If not set, it will
    /// be detected from the first source.
    pub texture_format: Option<TextureFormat>,
    /// If `true`, mipmaps will be generated during the import process.
    pub generate_mips: bool,
}

#[derive(Default)]
pub struct DataTilesetLoader;

impl AssetLoader for DataTilesetLoader {
    type Asset = TilesetImportData;
    type Settings = DataTilesetSettings;
    type Error = DataTilesetError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        &DataTilesetSettings {
            ref sampler,
            texture_format,
            generate_mips,
        }: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;

        let DataTileset {
            tile_size,
            sources,
            filter,
            groups,
        } = ron::de::from_bytes(&bytes)?;

        // N.B., This check is required to prevent panics by `.expect()`s below
        if sources.is_empty() {
            return Err(DataTilesetError::NoSources);
        }

        // If these are not already set, they will be detected from the first loaded source
        let mut tile_size = tile_size;
        let mut texture_format = texture_format;

        let mut source_tiles = Vec::with_capacity(sources.len());
        let mut loaded_sources = Vec::with_capacity(sources.len());

        for (id, source) in sources.into_iter().enumerate() {
            let (tile_info, source) =
                load_source(load_context, &mut tile_size, &mut texture_format, &source)
                    .await
                    .map_err(|err| DataTilesetError::source_err(id, &source.path, err))?;

            source_tiles.push(tile_info.count);
            loaded_sources.push(source);
        }

        Ok(TilesetImportData {
            tile_size: tile_size.expect("will be set if at least one source exists"),
            tile_indices: filter.indices(&source_tiles),
            tile_groups: groups,
            sources: loaded_sources,
            texture_format: texture_format.expect("will be set if at least one source exists"),
            sampler: sampler.clone(),
            generate_mips,
        })
    }

    fn extensions(&self) -> &[&str] {
        &[crate::tileset_ext_literal!("ron")]
    }
}

#[derive(Debug, Error)]
pub enum DataTilesetError {
    #[error("failed to load tileset data: {0}")]
    Io(#[from] std::io::Error),
    #[error("a tileset must have at least one source image")]
    NoSources,
    #[error("failed to deserialize ron tileset data: {0}")]
    Ron(#[from] ron::de::SpannedError),
    #[error("failed to load source {id} from \"{path}\": {err}")]
    LoadSource {
        id: usize,
        path: AssetPath<'static>,
        #[source]
        err: SourceError,
    },
}

impl DataTilesetError {
    pub fn source_err(id: usize, path: &AssetPath<'static>, err: impl Into<SourceError>) -> Self {
        Self::LoadSource {
            id,
            path: path.clone(),
            err: err.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum SourceError {
    #[error(transparent)]
    Load(#[from] LoadDirectError),
    #[error("invalid `TilesetLayout`: {0}")]
    Layout(#[from] LayoutError),
    #[error("expected tiles to be {expected:?}, but this source's tiles are {got}")]
    TileSize { expected: UVec2, got: UVec2 },
    #[error(
        "expected the texture format to be {expected:?}, but it was {got:?} and conversion failed"
    )]
    Format {
        expected: TextureFormat,
        got: TextureFormat,
    },
}

async fn load_source(
    load_context: &mut LoadContext<'_>,
    tile_size: &mut Option<UVec2>,
    texture_format: &mut Option<TextureFormat>,
    source: &DataTilesetSource,
) -> Result<(TileInfo, TilesetSource), SourceError> {
    let layout = source.layout.clone();
    let settings = source.image_settings.clone().unwrap_or_default();
    let mut texture: Image = load_context
        .loader()
        .immediate()
        .with_settings(move |s: &mut ImageLoaderSettings| *s = settings.clone())
        .load(&source.path)
        .await?
        .take();

    let tile_info = layout.tile_info(texture.size())?;

    // Detect or check the validity of this source's tile size
    if let Err(&expected) = tile_size.set_or_expected(tile_info.size) {
        return Err(SourceError::TileSize {
            expected,
            got: tile_info.size,
        });
    }

    // Detect or check the validity of this source's texture format
    if let Err(&expected) = texture_format.set_or_expected(texture.texture_descriptor.format) {
        // Attempt to convert to the expected format.
        // TODO: This is very limited, and a more complete conversion via a `DynamicImage` is
        // probably possible.
        if let Some(converted_image) = texture.convert(expected) {
            texture = converted_image;
        } else {
            return Err(SourceError::Format {
                expected,
                got: texture.texture_descriptor.format,
            });
        }
    }

    Ok((
        tile_info,
        TilesetSource {
            source: texture,
            layout,
        },
    ))
}
