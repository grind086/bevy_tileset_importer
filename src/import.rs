use std::{fmt::Display, io};

use bevy_asset::{
    Asset, AssetLoader, AsyncWriteExt, LoadContext, LoadedAsset, RenderAssetUsages,
    io::{Reader, Writer},
    processor::{LoadTransformAndSave, ProcessError},
    saver::{AssetSaver, SavedAsset},
    transformer::{AssetTransformer, TransformedAsset},
};
use bevy_color::{Color, LinearRgba};
use bevy_image::{Image, ImageSampler, TextureAccessError, TextureFormatPixelInfo};
use bevy_log::warn;
use bevy_math::UVec2;
use bevy_platform::collections::HashMap;
use bevy_reflect::TypePath;
use bincode::{
    Decode, Encode,
    error::{DecodeError, EncodeError},
};
use flate2::{Compression, read::DeflateDecoder, write::DeflateEncoder};
use thiserror::Error;
use wgpu_types::{Extent3d, TextureDataOrder, TextureDimension, TextureFormat};

use crate::{
    TileGroups, TileIndex, TileSourceIndex, Tileset,
    layout::{LayoutError, TileFilterIndices, TileFrame, TilesetLayout},
};

pub type TilesetImportProcess<L> = LoadTransformAndSave<L, TilesetImporter, ImportedTilesetSaver>;

/// Instantiates a [`TilesetImportProcess<L>`].
pub fn import_process<L: AssetLoader<Asset = TilesetImportData>>() -> TilesetImportProcess<L> {
    LoadTransformAndSave::new(TilesetImporter, ImportedTilesetSaver)
}

/// Loaded tileset data that is ready to be imported via [`TilesetImporter`].
#[derive(Asset, TypePath)]
pub struct TilesetImportData {
    /// The size of all tiles in the tile set.
    pub tile_size: UVec2,
    /// A list of tiles to import. Tiles in this list will have a [`TileIndex`] equal to their
    /// index in the list.
    pub tile_indices: TileFilterIndices,
    /// A set of named tile groups to import.
    pub tile_groups: HashMap<String, Vec<TileSourceIndex>>,
    /// The source image(s) of the tileset.
    pub sources: Vec<TilesetSource>,
    /// The tileset's output [`TextureFormat`].
    pub texture_format: TextureFormat,
    /// The samper used by the tileset's output texture.
    pub sampler: ImageSampler,
    /// If `true`, mipmaps will be generated for tiles.
    pub generate_mips: bool,
}

/// A tileset [`Image`] source.
pub struct TilesetSource {
    pub source: Image,
    pub layout: TilesetLayout,
}

/// An [`AssetTransformer`] that imports [tileset data][TilesetImportData] into a bevy-native format.
#[derive(Default)]
pub struct TilesetImporter;

impl AssetTransformer for TilesetImporter {
    type AssetInput = TilesetImportData;
    type AssetOutput = ImportedTilesetData;
    type Settings = ();
    type Error = ImportTilesetError;

    async fn transform<'a>(
        &'a self,
        asset: TransformedAsset<Self::AssetInput>,
        &(): &'a Self::Settings,
    ) -> Result<TransformedAsset<Self::AssetOutput>, Self::Error> {
        let TilesetImportData {
            tile_size,
            tile_indices,
            tile_groups,
            sources,
            texture_format,
            sampler,
            generate_mips,
        } = asset.get();

        // N.B., This check must happen before `make_texture_buffers`, otherwise `make_texture_buffers`
        // needs to be made fallible.
        let format_bytes = texture_format
            .pixel_size()
            .map_err(|_| ImportTilesetError::UnsupportedTextureFormat(*texture_format))?;

        let mut texture_bufs = make_texture_buffers(*texture_format, *tile_size, *generate_mips);
        let mut texture_data = Vec::new();
        let mut imported_count = 0;
        let mut imported_dedup = HashMap::new();
        let mut imported_groups = Vec::new();

        tile_indices.for_each(|source_index| {
            import_tile(
                sources,
                source_index,
                format_bytes,
                &mut texture_bufs,
                &mut texture_data,
            )
            .map_err(|err| err.with_source(TileSource::List(imported_count.into())))?;

            imported_dedup.insert(source_index, imported_count);
            imported_count += 1;

            Ok(())
        })?;

        for (name, group_list) in tile_groups {
            let mut group_tiles = Vec::with_capacity(group_list.len());

            for &source_index in group_list {
                if let Err(err) = imported_dedup.try_insert(source_index, imported_count) {
                    group_tiles.push(*err.entry.get());
                    continue;
                }

                import_tile(
                    sources,
                    source_index,
                    format_bytes,
                    &mut texture_bufs,
                    &mut texture_data,
                )
                .map_err(|err| {
                    err.with_source(TileSource::Group(name.clone(), group_tiles.len()))
                })?;

                group_tiles.push(imported_count);
                imported_count += 1;
            }

            imported_groups.push((name.clone(), group_tiles));
        }

        let imported = LoadedAsset::new_with_dependencies(ImportedTilesetData {
            tile_groups: imported_groups,
            texture_format: *texture_format,
            texture_size: Extent3d {
                width: tile_size.x,
                height: tile_size.y,
                depth_or_array_layers: imported_count.into(),
            },
            texture_data,
            sampler: sampler.clone(),
        });

        Ok(TransformedAsset::from_loaded(imported.into()).expect("the asset type is known"))
    }
}

/// An [`AssetSaver`] for imported tileset data.
pub struct ImportedTilesetSaver;

impl AssetSaver for ImportedTilesetSaver {
    type Asset = ImportedTilesetData;
    type Settings = ();
    type OutputLoader = ImportedTilesetLoader;
    type Error = SaveImportedTilesetError;

    async fn save(
        &self,
        writer: &mut Writer,
        asset: SavedAsset<'_, Self::Asset>,
        &(): &Self::Settings,
    ) -> Result<<Self::OutputLoader as AssetLoader>::Settings, Self::Error> {
        asset.write(writer).await
    }
}

/// An [`AssetLoader`] for imported tileset data.
#[derive(Default)]
pub struct ImportedTilesetLoader;

impl AssetLoader for ImportedTilesetLoader {
    type Asset = Tileset;
    type Settings = ();
    type Error = LoadImportedTilesetError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        &(): &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let ImportedTilesetData {
            tile_groups,
            texture_format,
            texture_size,
            texture_data,
            sampler,
        } = ImportedTilesetData::read(reader).await?;

        let mut texture = Image::new(
            texture_size,
            TextureDimension::D2,
            texture_data,
            texture_format,
            RenderAssetUsages::RENDER_WORLD,
        );

        texture.data_order = TextureDataOrder::LayerMajor;
        texture.sampler = sampler;

        let tile_count =
            TileIndex::try_from(texture_size.depth_or_array_layers).unwrap_or_else(|_| {
                warn!(
                    "Tileset {} has {} tiles, but only {} can be indexed",
                    load_context.asset_path(),
                    texture_size.depth_or_array_layers,
                    TileIndex::MAX
                );
                TileIndex::MAX
            });

        Ok(Tileset {
            texture: load_context.add_labeled_asset("texture".into(), texture),
            groups: TileGroups::from_iter(tile_groups),
            tile_count,
        })
    }
}

#[derive(Asset, TypePath, Encode, Decode)]
pub struct ImportedTilesetData {
    tile_groups: Vec<(String, Vec<TileIndex>)>,
    #[bincode(with_serde)]
    texture_format: TextureFormat,
    #[bincode(with_serde)]
    texture_size: Extent3d,
    texture_data: Vec<u8>,
    #[bincode(with_serde)]
    sampler: ImageSampler,
}

impl ImportedTilesetData {
    const COMPRESSION: Compression = Compression::fast();

    async fn read(reader: &mut dyn Reader) -> Result<Self, LoadImportedTilesetError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;

        let mut sync_reader = DeflateDecoder::new(bytes.as_slice());
        Ok(bincode::decode_from_std_read(
            &mut sync_reader,
            bincode::config::standard(),
        )?)
    }

    async fn write(&self, writer: &mut Writer) -> Result<(), SaveImportedTilesetError> {
        let mut sync_writer = DeflateEncoder::new(Vec::new(), Self::COMPRESSION);
        bincode::encode_into_std_write(self, &mut sync_writer, bincode::config::standard())?;
        let bytes = sync_writer.finish()?;

        writer.write_all(&bytes).await?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum LoadImportedTilesetError {
    /// An error occured when reading the tilemap data.
    #[error("failed to read imported tileset: {0}")]
    Io(#[from] io::Error),
    /// An error occured when decoding the binary tilemap data.
    #[error("failed to decode imported tileset: {0}")]
    Decode(#[from] DecodeError),
}

#[derive(Debug, Error)]
pub enum SaveImportedTilesetError {
    /// An error occured when writing the tilemap data.
    #[error("failed to write imported tileset: {0}")]
    Io(#[from] io::Error),
    /// An error occured when encoding the binary tilemap data.
    #[error("failed to encode imported tileset: {0}")]
    Encode(#[from] EncodeError),
}

impl From<SaveImportedTilesetError> for ProcessError {
    fn from(value: SaveImportedTilesetError) -> Self {
        ProcessError::AssetSaveError(value.into())
    }
}

#[derive(Debug, Error)]
pub enum ImportTilesetError {
    #[error("unable to get pixel size for format {0:?}")]
    UnsupportedTextureFormat(TextureFormat),
    #[error("error importing {tile}: {err}")]
    Tile {
        tile: TileSource,
        #[source]
        err: ImportTileError,
    },
}

#[derive(Debug)]
pub enum TileSource {
    List(usize),
    Group(String, usize),
    Unknown,
}

impl Display for TileSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::List(i) => write!(f, "tile at list index {i}"),
            Self::Group(name, i) => write!(f, "tile in group {name:?} at index {i}"),
            Self::Unknown => write!(f, "tile"),
        }
    }
}

#[derive(Debug, Error)]
pub enum ImportTileError {
    #[error("invalid layout: {0}")]
    Layout(#[from] LayoutError),
    #[error("texture access error (this is a bug): {0}")]
    TextureAccess(#[from] TextureAccessError),
}

impl ImportTileError {
    pub fn with_source(self, tile: TileSource) -> ImportTilesetError {
        ImportTilesetError::Tile { tile, err: self }
    }
}

/// Imports a single tile, generating mipmaps (if necessary) and appending output to `texture_data`.
fn import_tile(
    sources: &[TilesetSource],
    (source_id, tile_index): TileSourceIndex,
    format_bytes: usize,
    texture_bufs: &mut [Image],
    texture_data: &mut Vec<u8>,
) -> Result<(), ImportTileError> {
    const VALID_BUF: &str = "`texture_bufs` should always contain data";

    // Parameters for indexing into the pixel buffers
    let TilesetSource { source, layout } = &sources[source_id];

    let TileFrame { frame, anchor } = layout.frame(source.size(), tile_index)?;
    let frame_size = frame.size();
    let frame_row_bytes = frame_size.x as usize * format_bytes;

    let src_size = source.size();
    let src_row_bytes = src_size.x as usize * format_bytes;
    let src_data = source.data.as_ref().expect(VALID_BUF);

    let tgt_size = texture_bufs[0].size();
    let tgt_row_bytes = tgt_size.x as usize * format_bytes;
    let tgt_data = texture_bufs[0].data.as_mut().expect(VALID_BUF);

    // Index of the top-left pixel in the source and tile images
    let mut src_i = (frame.min.x + frame.min.y * src_size.x) as usize * format_bytes;
    let mut tgt_i = (anchor.x + anchor.y * tgt_size.x) as usize * format_bytes;

    // Copy the tile into the full-size buffer
    for _ in 0..frame_size.y {
        let src_j = src_i + frame_row_bytes;
        let tgt_j = tgt_i + frame_row_bytes;

        tgt_data[tgt_i..tgt_j].copy_from_slice(&src_data[src_i..src_j]);

        src_i += src_row_bytes;
        tgt_i += tgt_row_bytes;
    }

    texture_data.extend_from_slice(tgt_data);

    // Generate mips
    for m in 1..texture_bufs.len() {
        let (a, b) = texture_bufs.split_at_mut(m);

        let src = &a[m - 1];
        let tgt = &mut b[0];

        downscale_image_half(src, tgt)?;

        texture_data.extend_from_slice(tgt.data.as_ref().expect(VALID_BUF));
    }

    Ok(())
}

/// Creates buffer [`Image`]s to generate mipmaps with.
fn make_texture_buffers(
    format: TextureFormat,
    tile_size: UVec2,
    generate_mips: bool,
) -> Vec<Image> {
    let zero_pixel = vec![
        0u8;
        format
            .pixel_size()
            .expect("should be checked prior to calling this function")
    ];

    let base_extent = Extent3d {
        width: tile_size.x,
        height: tile_size.y,
        depth_or_array_layers: 1,
    };

    let mips = if generate_mips {
        base_extent.max_mips(TextureDimension::D2)
    } else {
        1
    };

    (0..mips)
        .map(|m| {
            Image::new_fill(
                base_extent.mip_level_size(m, TextureDimension::D2),
                TextureDimension::D2,
                &zero_pixel,
                format,
                RenderAssetUsages::empty(),
            )
        })
        .collect()
}

/// Downscales `src` into `tgt` by blending 2x2 blocks of pixels.
fn downscale_image_half(src: &Image, tgt: &mut Image) -> Result<(), TextureAccessError> {
    debug_assert_eq!(src.size(), tgt.size() * 2);

    for ty in 0..tgt.height() {
        for tx in 0..tgt.width() {
            let sx = 2 * tx;
            let sy = 2 * ty;

            let mix_color = alpha_discard_mix(&[
                src.get_color_at(sx, sy)?,
                src.get_color_at(sx + 1, sy)?,
                src.get_color_at(sx, sy + 1)?,
                src.get_color_at(sx + 1, sy + 1)?,
            ]);

            tgt.set_color_at(tx, ty, mix_color)?;
        }
    }

    Ok(())
}

/// Mixes a slice of colors, disregarding transparent elements.
fn alpha_discard_mix(colors: &[Color]) -> Color {
    const CUTOFF: f32 = 1e-4;

    let mut n = 0;
    let mut linear_sum = LinearRgba::NONE;
    for color in colors {
        let linear = color.to_linear();
        if linear.alpha < CUTOFF {
            // continue;
            // TODO: Figure out why tile borders are flickering if we don't toss everything. Likely
            // something related to mips not being laid out *quite* right, and this results in
            // enough padding to hide the jank.
            return LinearRgba::NONE.into();
        }

        n += 1;
        linear_sum += linear;
    }

    if n > 1 {
        linear_sum /= n as f32;
    }

    if linear_sum.alpha < CUTOFF {
        LinearRgba::NONE
    } else {
        linear_sum
    }
    .into()
}
