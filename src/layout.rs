use bevy_math::{URect, UVec2};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::TileIndex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileFrame {
    pub frame: URect,
    #[serde(default)]
    pub anchor: UVec2,
}

impl TileFrame {
    pub const fn from_tile_size(tile_size: UVec2) -> Self {
        Self {
            frame: URect {
                min: UVec2::ZERO,
                max: tile_size,
            },
            anchor: UVec2::ZERO,
        }
    }

    pub fn is_valid(&self, image_size: UVec2, tile_size: UVec2) -> bool {
        self.frame.max.cmplt(image_size).all()
            && (self.frame.size() + self.anchor).cmplt(tile_size).all()
    }
}

#[derive(Debug)]
pub enum TilesetLayout {
    Grid { padding: UVec2, margins: URect },
    Frames(Vec<TileFrame>),
}

impl TilesetLayout {
    pub const fn unpadded_grid() -> Self {
        Self::Grid {
            padding: UVec2::ZERO,
            margins: URect {
                min: UVec2::new(0, 0),
                max: UVec2::new(0, 0),
            },
        }
    }
}

#[derive(Debug)]
pub enum TilesetSourceFrames {
    Grid {
        tile_count: TileIndex,
        tile_size: UVec2,
        grid_size: UVec2,
        padding: UVec2,
        margins: URect,
    },
    Frames(Vec<TileFrame>),
}

#[derive(Debug, Error)]
pub enum LayoutError {
    #[error("")]
    InvalidGrid {
        image_size: UVec2,
        tile_size: UVec2,
        padding: UVec2,
        margins: URect,
    },
    #[error(
        "frame {frame:?} is not compatible with source image size {image_size} and tile size {tile_size}"
    )]
    InvalidFrame {
        image_size: UVec2,
        tile_size: UVec2,
        frame: TileFrame,
    },
    #[error(
        "the source layout defines {count} tiles, but the maximum index is {}",
        TileIndex::MAX
    )]
    TooManyTiles { count: usize },
    #[error("tile index was {idx}, but the source contains {max} tiles")]
    OutOfRange { idx: TileIndex, max: TileIndex },
}

impl TilesetLayout {
    pub fn tile_frames(
        self,
        image_size: UVec2,
        tile_size: UVec2,
    ) -> Result<TilesetSourceFrames, LayoutError> {
        match self {
            Self::Grid { padding, margins } => {
                Self::grid_tile_frames(image_size, tile_size, padding, margins)
            }
            Self::Frames(frames) => Self::frames_tile_frames(image_size, tile_size, frames),
        }
    }

    fn grid_tile_frames(
        image_size: UVec2,
        tile_size: UVec2,
        padding: UVec2,
        margins: URect,
    ) -> Result<TilesetSourceFrames, LayoutError> {
        let adjusted_size = image_size - margins.size() + padding;

        if adjusted_size % tile_size != UVec2::ZERO {
            return Err(LayoutError::InvalidGrid {
                image_size,
                tile_size,
                padding,
                margins,
            });
        }

        let grid_size = adjusted_size / tile_size;
        let tile_count =
            grid_size
                .element_product()
                .try_into()
                .map_err(|_| LayoutError::TooManyTiles {
                    count: grid_size.element_product() as _,
                })?;

        Ok(TilesetSourceFrames::Grid {
            tile_count,
            tile_size,
            grid_size,
            padding,
            margins,
        })
    }

    fn frames_tile_frames(
        image_size: UVec2,
        tile_size: UVec2,
        frames: Vec<TileFrame>,
    ) -> Result<TilesetSourceFrames, LayoutError> {
        for frame in &frames {
            if !frame.is_valid(image_size, tile_size) {
                return Err(LayoutError::InvalidFrame {
                    tile_size,
                    image_size,
                    frame: *frame,
                });
            }
        }

        if frames.len() > usize::from(TileIndex::MAX) {
            return Err(LayoutError::TooManyTiles {
                count: frames.len(),
            });
        }

        Ok(TilesetSourceFrames::Frames(frames))
    }
}

impl TilesetSourceFrames {
    pub fn tile_count(&self) -> TileIndex {
        match self {
            Self::Grid { tile_count, .. } => *tile_count,
            Self::Frames(frames) => frames.len() as _,
        }
    }

    pub fn get(&self, tile_index: TileIndex) -> Result<TileFrame, LayoutError> {
        match self {
            Self::Grid {
                tile_count,
                tile_size,
                grid_size,
                padding,
                margins,
            } => (tile_index < *tile_count).then(|| {
                let grid_index = UVec2 {
                    x: u32::from(tile_index) % grid_size.x,
                    y: u32::from(tile_index) / grid_size.x,
                };

                let min = margins.min + grid_index * (tile_size + padding);
                let max = min + *tile_size;

                TileFrame {
                    frame: URect { min, max },
                    anchor: UVec2::ZERO,
                }
            }),
            Self::Frames(frames) => frames.get(usize::from(tile_index)).copied(),
        }
        .ok_or_else(|| LayoutError::OutOfRange {
            idx: tile_index,
            max: self.tile_count(),
        })
    }
}
