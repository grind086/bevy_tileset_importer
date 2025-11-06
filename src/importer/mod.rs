use std::marker::PhantomData;

use bevy_asset::{
    Asset, AssetLoader, AsyncWriteExt,
    io::Writer,
    meta::{AssetAction, AssetMeta},
    processor::{Process, ProcessContext, ProcessError},
};
use bevy_image::Image;
use bevy_math::UVec2;
use bevy_platform::collections::{HashMap, hash_map::Entry};
use bevy_reflect::TypePath;
use serde::{Deserialize, Serialize};
use wgpu_types::TextureFormat;

use crate::{
    TileSourceIndex,
    format::TilesetFile,
    layout::{TilesetLayout, TilesetSourceFrames},
    loader::{TilesetLoader, TilesetLoaderSettings},
};

mod error;
mod texture_builder;

pub use error::*;
use texture_builder::TextureBuilder;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TilesetImportSettings {
    /// Sets a desired texture format for all imported tilesets.
    ///
    /// If a source image cannot be converted to this format, the import will fail with an error.
    pub texture_format: Option<TextureFormat>,
    /// If set to `true`, mipmaps will be generated for each tile.
    ///
    /// Mipmap generation is limited to texture formats supported by [`Image::get_color_at`].
    pub generate_mips: bool,
    /// A deflate [compression level][flate2::Compression] to use for the texture, from 0-9.
    /// 0 leaves the data uncompressed, and 9 means "take as long as you want".
    pub compression: u32,
}

impl Default for TilesetImportSettings {
    fn default() -> Self {
        Self {
            texture_format: None,
            generate_mips: false,
            compression: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TilesetImporterSettings<L: AssetLoader<Asset = TilesetImportData>> {
    pub source_settings: L::Settings,
    pub import_settings: TilesetImportSettings,
    pub loader_settings: TilesetLoaderSettings,
}

impl<L: AssetLoader<Asset = TilesetImportData>> Default for TilesetImporterSettings<L> {
    fn default() -> Self {
        Self {
            source_settings: L::Settings::default(),
            import_settings: TilesetImportSettings::default(),
            loader_settings: TilesetLoaderSettings::default(),
        }
    }
}

pub struct TilesetImporter<L> {
    _marker: PhantomData<fn(&L)>,
}

impl<L> Default for TilesetImporter<L> {
    fn default() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<L: AssetLoader<Asset = TilesetImportData>> Process for TilesetImporter<L> {
    type Settings = TilesetImporterSettings<L>;
    type OutputLoader = TilesetLoader;

    async fn process(
        &self,
        context: &mut ProcessContext<'_>,
        meta: AssetMeta<(), Self>,
        writer: &mut Writer,
    ) -> Result<<Self::OutputLoader as AssetLoader>::Settings, ProcessError> {
        let AssetAction::Process { settings, .. } = meta.asset else {
            return Err(ProcessError::WrongMetaType);
        };

        let loader_meta = AssetMeta::<L, ()>::new(AssetAction::Load {
            loader: core::any::type_name::<L>().to_string(),
            settings: settings.source_settings,
        });

        let tileset_data = context
            .load_source_asset(loader_meta)
            .await?
            .take::<L::Asset>()
            .expect("loader type is known");

        let TilesetImportSettings {
            texture_format,
            generate_mips,
            compression,
        } = settings.import_settings;

        let tileset_file = tileset_data
            .import(texture_format, generate_mips)
            .map_err(|err| ProcessError::AssetTransformError(err.into()))?;

        async move {
            let mut bytes = Vec::new();
            tileset_file.write(compression, &mut bytes)?;
            writer.write_all(&bytes).await?;
            Ok(())
        }
        .await
        .map_err(ProcessError::AssetSaveError)?;

        Ok(settings.loader_settings)
    }
}

#[derive(Debug, Asset, TypePath)]
pub struct TilesetImportData {
    pub tile_size: UVec2,
    pub tile_filter: TileFilter,
    pub tile_groups: Vec<(String, Vec<TileSourceIndex>)>,
    pub sources: Vec<TilesetSource>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub enum TileFilter {
    #[default]
    All,
    None,
    List(Vec<TileSourceIndex>),
}

impl TileFilter {
    fn try_for_each<E>(
        &self,
        sources: &[(Image, TilesetSourceFrames)],
        f: impl FnMut(TileSourceIndex) -> Result<(), E>,
    ) -> Result<(), E> {
        match self {
            Self::All => sources
                .iter()
                .enumerate()
                .flat_map(|(source_id, (_, source_frames))| {
                    (0..source_frames.tile_count()).map(move |tile_index| (source_id, tile_index))
                })
                .try_for_each(f),
            Self::None => Ok(()),
            Self::List(list) => list.iter().copied().try_for_each(f),
        }
    }
}

#[derive(Debug)]
pub struct TilesetSource {
    pub texture: Image,
    pub layout: TilesetLayout,
}

impl TilesetImportData {
    fn import(
        self,
        mut texture_format: Option<TextureFormat>,
        generate_mips: bool,
    ) -> Result<TilesetFile, ImportTilesetError> {
        let TilesetImportData {
            tile_size,
            tile_filter,
            tile_groups,
            sources,
        } = self;

        // Validate sources
        let sources = sources
            .into_iter()
            .enumerate()
            .map(|(source_id, mut source)| {
                // Check the source format
                let source_format = source.texture.texture_descriptor.format;
                match texture_format {
                    None => texture_format = Some(source_format),
                    Some(expected) => {
                        // If the source is not in the expected format, try to convert it
                        if expected != source_format {
                            source.texture = source.texture.convert(expected).ok_or(
                                ImportTilesetError::ValidateSource(SourceError::SourceFormat {
                                    source_id,
                                    source_format,
                                    expected,
                                }),
                            )?;
                        }
                    }
                }

                // Get a frame accessor from the layout, texture size, and tile size
                let frames = source
                    .layout
                    .tile_frames(source.texture.size(), tile_size)
                    .map_err(|err| {
                        ImportTilesetError::ValidateSource(SourceError::SourceLayout {
                            source_id,
                            err,
                        })
                    })?;

                Ok((source.texture, frames))
            })
            .collect::<Result<Vec<_>, ImportTilesetError>>()?;

        // N.B., This will be set if at least one source is present. If no sources are present,
        // then the choice of texture format is arbitrary because there can be no output tiles.
        let texture_format = texture_format.unwrap_or(TextureFormat::Rgba8Unorm);

        let mut texture_builder = TextureBuilder::new(tile_size, texture_format, generate_mips)?;
        let mut tile_dedup = HashMap::new();

        tile_filter.try_for_each(&sources, |tile_source| {
            let tile_index = texture_builder.import_tile(&sources, tile_source)?;
            tile_dedup.insert(tile_source, tile_index);
            Ok(())
        })?;

        let tile_groups = tile_groups
            .into_iter()
            .map(|(name, tiles)| {
                Ok((
                    name.clone(),
                    tiles
                        .into_iter()
                        .map(|tile_source| match tile_dedup.entry(tile_source) {
                            Entry::Occupied(e) => Ok(*e.get()),
                            Entry::Vacant(e) => Ok(*e.insert(
                                texture_builder
                                    .import_tile(&sources, tile_source)
                                    .map_err(|err| err.in_group(&name))?,
                            )),
                        })
                        .collect::<Result<_, _>>()?,
                ))
            })
            .collect::<Result<Vec<_>, ImportTilesetError>>()?;

        Ok(TilesetFile {
            tile_size: tile_size.into(),
            tile_count: texture_builder.tile_count(),
            tile_groups,
            texture_format: texture_builder.texture_format(),
            texture_mips: texture_builder.mip_levels(),
            texture_data: texture_builder.into_data(),
        })
    }
}
