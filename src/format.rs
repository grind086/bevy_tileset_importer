use std::io::{self, Read, Write};

use bevy_asset::Asset;
use bevy_image::{Image, TextureFormatPixelInfo, Volume};
use bevy_reflect::TypePath;
use bincode::{Decode, Encode};
use flate2::Compression;
use thiserror::Error;
use wgpu_types::{Extent3d, TextureDataOrder, TextureDimension, TextureFormat};

use crate::{TileGroups, TileIndex};

type TileGroupData = Vec<(String, Vec<TileIndex>)>;

/// A tileset file format that is tightly coupled to a bevy [`Image`] for efficient loading.
///
/// The byte format of a tileset file should be considered only semi-stable between bevy
/// versions (as [`Image`] itself is not guaranteed to be stable), and re-importing tilesets
/// may be a required migration step when upgrading.
#[derive(Asset, TypePath, Debug, Encode, Decode)]
pub struct TilesetFile {
    pub tile_size: [u32; 2],
    pub tile_count: TileIndex,
    pub tile_groups: TileGroupData,
    #[bincode(with_serde)]
    pub texture_format: TextureFormat,
    pub texture_mips: u32,
    pub texture_data: Vec<u8>,
}

/// Errors encountered when working with [`TilesetFile`].
#[derive(Debug, Error)]
pub enum TilesetFileError {
    #[error(transparent)]
    Io(#[from] io::Error),
    /// Returned when attempting to construct a tileset file using an image with uninitialized
    /// data.
    #[error("the tileset texture data must be uninitialized")]
    Uninitialized,
    #[error("the tileset texture data, format, and size are not in agreement")]
    InvalidData,
    /// Returned when attempting to construct or load a tileset file containing more than
    /// [`TileIndex::MAX`] tiles.
    #[error("the tileset texture contains {0} tiles, but the maximum tile index is {max}", max=TileIndex::MAX)]
    TooManyTiles(u32),
    /// Returned when attempting to encode a tileset file into bytes.
    #[error("failed to encode tileset data: {0}")]
    Encode(#[from] bincode::error::EncodeError),
    /// Returned when attempting to decode a tileset file from bytes.
    #[error("failed to decode tileset data: {0}")]
    Decode(#[from] bincode::error::DecodeError),
}

impl TilesetFile {
    pub fn new(tile_groups: TileGroups, texture: Image) -> Result<Self, TilesetFileError> {
        let descriptor = &texture.texture_descriptor;

        let texture_format = descriptor.format;
        let texture_size = descriptor.size;
        let texture_mips = descriptor.mip_level_count;
        let texture_data = texture.data.ok_or(TilesetFileError::Uninitialized)?;

        validate_data_volume(texture_format, texture_size, texture_mips, &texture_data)?;

        Ok(Self {
            tile_size: [texture_size.width, texture_size.height],
            tile_count: texture_size
                .depth_or_array_layers
                .try_into()
                .map_err(|_| TilesetFileError::TooManyTiles(texture_size.depth_or_array_layers))?,
            tile_groups: tile_groups.into_file_data(),
            texture_format,
            texture_mips,
            texture_data,
        })
    }

    pub fn into_count_groups_image(
        self,
    ) -> Result<(TileIndex, TileGroups, Image), TilesetFileError> {
        let TilesetFile {
            tile_size,
            tile_count,
            tile_groups,
            texture_format,
            texture_mips,
            texture_data,
        } = self;

        let texture_size = Extent3d {
            width: tile_size[0],
            height: tile_size[1],
            depth_or_array_layers: tile_count.into(),
        };

        validate_data_volume(texture_format, texture_size, texture_mips, &texture_data)?;

        let mut image = Image::new(
            texture_size,
            TextureDimension::D2,
            texture_data,
            texture_format,
            Default::default(),
        );

        image.data_order = TextureDataOrder::LayerMajor;
        image.texture_descriptor.mip_level_count = texture_mips;

        Ok((tile_count, TileGroups::from_file_data(tile_groups), image))
    }

    pub fn read(mut bytes: impl Read) -> Result<Self, TilesetFileError> {
        let mut flags = [0];
        bytes.read_exact(&mut flags)?;

        let file = if flags[0] == 0 {
            // No compression
            bincode::decode_from_std_read(&mut bytes, bincode::config::standard())?
        } else {
            // Inflate
            bincode::decode_from_std_read(
                &mut flate2::read::DeflateDecoder::new(bytes),
                bincode::config::standard(),
            )?
        };
        Ok(file)
    }

    pub fn write(&self, compression: u32, mut writer: impl Write) -> Result<(), TilesetFileError> {
        if compression == 0 {
            // No compression
            writer.write_all(&[0])?;
            bincode::encode_into_std_write(self, &mut writer, bincode::config::standard())?;
        } else {
            // Deflate
            writer.write_all(&[1])?;
            bincode::encode_into_std_write(
                self,
                &mut flate2::write::DeflateEncoder::new(writer, Compression::new(compression)),
                bincode::config::standard(),
            )?;
        }
        Ok(())
    }
}

impl TileGroups {
    fn from_file_data(data: TileGroupData) -> Self {
        let mut indices = Vec::new();
        let ranges = data
            .into_iter()
            .map(|(name, mut group_indices)| {
                let i = indices.len();
                indices.append(&mut group_indices);
                (name, i..indices.len())
            })
            .collect();
        Self { ranges, indices }
    }

    fn into_file_data(self) -> TileGroupData {
        self.ranges
            .into_iter()
            .map(|(name, range)| (name, self.indices[range].to_vec()))
            .collect()
    }
}

/// Checks that `texture_data` contains the expected number of bytes for a texture with the
/// specified format, size, and mip levels.
fn validate_data_volume(
    texture_format: TextureFormat,
    texture_size: Extent3d,
    texture_mips: u32,
    texture_data: &[u8],
) -> Result<(), TilesetFileError> {
    if let Ok(pixel_size) = texture_format.pixel_size() {
        let n_pixels = (0..texture_mips)
            .map(|m| {
                texture_size
                    .mip_level_size(m, TextureDimension::D2)
                    .volume()
            })
            .sum::<usize>();

        if n_pixels * pixel_size == texture_data.len() {
            return Ok(());
        }
    }

    Err(TilesetFileError::InvalidData)
}
