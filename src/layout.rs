use bevy_math::{URect, UVec2};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{TileIndex, TileSourceIndex};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub enum TilesetLayout {
    /// The source image contains a single tile.
    #[default]
    Single,
    /// Tiles in the source image are laid out in a grid.
    Grid(GridLayout),
    /// Tiles in the source image are defined by rectangular frames in the source image, and the
    /// frames' offsets within their tiles.
    Frames(FramesLayout),
}

#[derive(Debug, Error)]
pub enum LayoutError {
    /// The source image contained no pixels when using [`TilesetLayout::Single`].
    #[error("the source image is empty")]
    EmptyImage,
    /// The requested tile index wasn't present in the layout.
    #[error("tile index was {idx}, but layout contained {len} tiles")]
    OutOfRange { idx: TileIndex, len: TileIndex },
    /// The layout contained more than [`TileIndex::MAX`] tiles.
    #[error(
        "the layout specified {0} tiles, but the maximum tile index is {max}",
        max=TileIndex::MAX
    )]
    TooManyTiles(usize),
    /// A [`FramesLayout`] contained a frame outside its source image.
    #[error(
        "frame {index} with shape {frame:?} does not fit within its source image sized {image_size}"
    )]
    InvalidFrame {
        index: usize,
        frame: URect,
        image_size: UVec2,
    },
    /// A [`FramesLayout`] contained a frame larger than its tile size.
    #[error(
        "frame {index} with shape {frame:?} and anchor {anchor:?} does not fit within a tile of size {tile_size}"
    )]
    InvalidAnchor {
        index: usize,
        frame: URect,
        anchor: UVec2,
        tile_size: UVec2,
    },
    #[error("todo")]
    Other,
}

pub struct TileInfo {
    pub size: UVec2,
    pub count: u16,
}

impl TilesetLayout {
    pub fn tile_info(&self, image_size: UVec2) -> Result<TileInfo, LayoutError> {
        match self {
            Self::Single => single_info(image_size),
            Self::Grid(layout) => layout.tile_info(image_size),
            Self::Frames(layout) => layout.tile_info(image_size),
        }
    }

    pub fn frame(&self, image_size: UVec2, index: u16) -> Result<TileFrame, LayoutError> {
        match self {
            Self::Single => single_frame(image_size, index),
            Self::Grid(layout) => layout.frame(image_size, index),
            Self::Frames(layout) => layout.frame(index),
        }
    }
}

fn single_info(image_size: UVec2) -> Result<TileInfo, LayoutError> {
    if !image_size.cmpne(UVec2::ZERO).all() {
        return Err(LayoutError::EmptyImage);
    }

    Ok(TileInfo {
        size: image_size,
        count: 1,
    })
}

fn single_frame(image_size: UVec2, index: u16) -> Result<TileFrame, LayoutError> {
    if !image_size.cmpne(UVec2::ZERO).all() {
        return Err(LayoutError::EmptyImage);
    }

    if index != 0 {
        return Err(LayoutError::OutOfRange { idx: index, len: 1 });
    }

    Ok(TileFrame::from_size(image_size))
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GridLayout {
    /// The size of a tile in pixels.
    pub tile_size: UVec2,
    /// The number of pixels between tiles.
    #[serde(default)]
    pub tile_padding: UVec2,
    /// The size of the margin around the image border.
    #[serde(default)]
    pub image_margin: URect,
}

impl GridLayout {
    pub fn from_tile_size(tile_size: UVec2) -> Self {
        Self {
            tile_size,
            tile_padding: UVec2::ZERO,
            image_margin: URect::new(0, 0, 0, 0),
        }
    }

    pub fn margin_size(&self) -> UVec2 {
        self.image_margin.min + self.image_margin.max
    }

    pub fn grid(&self, image_size: UVec2) -> UVec2 {
        (image_size - self.margin_size() + self.tile_padding) / (self.tile_size + self.tile_padding)
    }

    pub fn tile_info(&self, image_size: UVec2) -> Result<TileInfo, LayoutError> {
        let count = self.grid(image_size).element_product();
        Ok(TileInfo {
            size: self.tile_size,
            count: count
                .try_into()
                .map_err(|_| LayoutError::TooManyTiles(count as _))?,
        })
    }

    pub fn frame(&self, image_size: UVec2, index: u16) -> Result<TileFrame, LayoutError> {
        let idx = index as u32;
        let grid = self.grid(image_size);
        let len = grid.element_product();
        (idx < len)
            .then(|| {
                let x = idx % grid.x;
                let y = idx / grid.x;

                let min = (self.tile_size + self.tile_padding) * UVec2::new(x, y);
                let max = min + self.tile_size;

                TileFrame {
                    frame: URect { min, max },
                    anchor: UVec2::ZERO,
                }
            })
            .ok_or(LayoutError::OutOfRange {
                idx: index,
                len: len as _,
            })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FramesLayout {
    pub tile_size: UVec2,
    pub frames: Vec<TileFrame>,
}

impl FramesLayout {
    pub fn tile_info(&self, image_size: UVec2) -> Result<TileInfo, LayoutError> {
        for (index, &TileFrame { frame, anchor }) in self.frames.iter().enumerate() {
            if !frame.max.cmple(image_size).all() {
                return Err(LayoutError::InvalidFrame {
                    index,
                    frame,
                    image_size,
                });
            }

            if !(anchor + frame.size()).cmple(self.tile_size).all() {
                return Err(LayoutError::InvalidAnchor {
                    index,
                    frame,
                    anchor,
                    tile_size: self.tile_size,
                });
            }
        }

        let count = self
            .frames
            .len()
            .try_into()
            .map_err(|_| LayoutError::TooManyTiles(self.frames.len()))?;

        Ok(TileInfo {
            size: self.tile_size,
            count,
        })
    }

    pub fn frame(&self, index: u16) -> Result<TileFrame, LayoutError> {
        self.frames
            .get(index as usize)
            .copied()
            .ok_or(LayoutError::OutOfRange {
                idx: index,
                len: self.frames.len() as _,
            })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TileFrame {
    /// The frame in the tileset image.
    pub frame: URect,
    /// The offset of the frame within its tile.
    pub anchor: UVec2,
}

impl TileFrame {
    pub fn from_size(size: UVec2) -> Self {
        Self {
            frame: URect {
                min: UVec2::ZERO,
                max: size,
            },
            anchor: UVec2::ZERO,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TileFilter<T> {
    /// Include all tiles specified in the layout.
    All,
    /// Only include tiles in named groups.
    None,
    /// Include only the tiles with the given source indices.
    ///
    /// The indices of the tiles in the tileset will be the same as in this list.
    List(Vec<T>),
}

impl TileFilter<TileIndex> {
    pub fn indices(&self, source_index: usize, image_tiles: TileIndex) -> TileFilterIndices {
        match self {
            Self::All => TileFilterIndices::AllSingle(source_index, image_tiles),
            Self::None => TileFilterIndices::None,
            Self::List(list) => {
                TileFilterIndices::List(list.iter().map(|i| (source_index, *i)).collect())
            }
        }
    }
}

impl TileFilter<TileSourceIndex> {
    pub fn indices(&self, source_tiles: &[TileIndex]) -> TileFilterIndices {
        match self {
            Self::All => TileFilterIndices::AllMulti(source_tiles.to_vec()),
            Self::None => TileFilterIndices::None,
            Self::List(list) => TileFilterIndices::List(list.clone()),
        }
    }
}

impl<T> Default for TileFilter<T> {
    fn default() -> Self {
        Self::All
    }
}

pub enum TileFilterIndices {
    None,
    AllSingle(usize, TileIndex),
    AllMulti(Vec<TileIndex>),
    List(Vec<TileSourceIndex>),
}

impl TileFilterIndices {
    pub fn for_each<E>(&self, f: impl FnMut(TileSourceIndex) -> Result<(), E>) -> Result<(), E> {
        match self {
            Self::None => Ok(()),
            Self::List(list) => list.iter().copied().try_for_each(f),
            Self::AllSingle(source_index, image_tiles) => (0..*image_tiles)
                .map(|i| (*source_index, i))
                .try_for_each(f),
            Self::AllMulti(source_tiles) => source_tiles
                .iter()
                .enumerate()
                .flat_map(|(source_index, image_tiles)| {
                    (0..*image_tiles).map(move |i| (source_index, i))
                })
                .try_for_each(f),
        }
    }
}
