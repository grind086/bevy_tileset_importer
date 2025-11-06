use bevy_image::TextureAccessError;
use thiserror::Error;
use wgpu_types::TextureFormat;

use crate::{TileSourceIndex, layout::LayoutError};

#[derive(Debug, Error)]
pub enum ImportTilesetError {
    #[error("unsupported texture format: {0:?}")]
    UnsupportedFormat(TextureFormat),
    #[error("failed to generate mipmaps: {0}")]
    GenerateMips(TextureAccessError),
    #[error("error validating sources: {0}")]
    ValidateSource(#[source] SourceError),
    #[error("error importing tile {} from source {}: {err}", tile_source.1, tile_source.0)]
    ImportTile {
        tile_source: TileSourceIndex,
        #[source]
        err: SourceError,
    },
    #[error("in group {group:?}: error importing tile {} from source {}: {err}", tile_source.1, tile_source.0)]
    ImportGroup {
        group: String,
        tile_source: TileSourceIndex,
        #[source]
        err: SourceError,
    },
}

impl ImportTilesetError {
    /// Converts a [`ImportTilesetError::ImportTile`] into a [`ImportTilesetError::ImportGroup`].
    pub(crate) fn in_group(self, group: &str) -> Self {
        match self {
            Self::ImportTile { tile_source, err } => Self::ImportGroup {
                group: group.into(),
                tile_source,
                err,
            },
            other => other,
        }
    }
}

#[derive(Debug, Error)]
pub enum SourceError {
    #[error("source id was {source_id}, but the tileset has {source_len} sources")]
    SourceOutOfRange { source_id: usize, source_len: usize },
    #[error(
        "source {source_id} has texture format {source_format:?}, which cannot be converted to {expected:?}"
    )]
    SourceFormat {
        source_id: usize,
        source_format: TextureFormat,
        expected: TextureFormat,
    },
    #[error("source {source_id} encountered a layout error: {err}")]
    SourceLayout {
        source_id: usize,
        #[source]
        err: LayoutError,
    },
}
