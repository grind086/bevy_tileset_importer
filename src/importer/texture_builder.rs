use bevy_asset::RenderAssetUsages;
use bevy_color::{Color, LinearRgba};
use bevy_image::{Image, TextureAccessError, TextureFormatPixelInfo};
use bevy_math::{UVec2, VectorSpace};
use wgpu_types::{Extent3d, TextureDimension, TextureFormat};

use crate::{
    TileIndex, TileSourceIndex,
    importer::{ImportTilesetError, SourceError},
    layout::{TileFrame, TilesetSourceFrames},
};

pub(crate) struct TextureBuilder {
    mip_bufs: Vec<Image>,
    texture_data: Vec<u8>,
    tile_count: TileIndex,
    pixel_bytes: usize,
}

impl TextureBuilder {
    pub fn new(
        tile_size: UVec2,
        texture_format: TextureFormat,
        generate_mips: bool,
    ) -> Result<Self, ImportTilesetError> {
        let pixel_bytes = texture_format
            .pixel_size()
            .map_err(|_| ImportTilesetError::UnsupportedFormat(texture_format))?;

        let zero_pixel = vec![0; pixel_bytes];
        let base_extent = Extent3d {
            width: tile_size.x,
            height: tile_size.y,
            depth_or_array_layers: 1,
        };
        let mip_levels = if generate_mips {
            base_extent.max_mips(TextureDimension::D2)
        } else {
            1
        };

        Ok(Self {
            mip_bufs: (0..mip_levels)
                .map(|m| {
                    Image::new_fill(
                        base_extent.mip_level_size(m, TextureDimension::D2),
                        TextureDimension::D2,
                        &zero_pixel,
                        texture_format,
                        RenderAssetUsages::empty(),
                    )
                })
                .collect(),
            texture_data: Vec::new(),
            tile_count: 0,
            pixel_bytes,
        })
    }

    pub fn texture_format(&self) -> TextureFormat {
        self.mip_bufs[0].texture_descriptor.format
    }

    pub fn mip_levels(&self) -> u32 {
        self.mip_bufs.len() as _
    }

    pub fn tile_count(&self) -> TileIndex {
        self.tile_count
    }

    pub fn into_data(self) -> Vec<u8> {
        self.texture_data
    }

    pub fn import_tile(
        &mut self,
        sources: &[(Image, TilesetSourceFrames)],
        tile_source: TileSourceIndex,
    ) -> Result<TileIndex, ImportTilesetError> {
        self.copy_base_image(sources, tile_source)
            .map_err(|err| ImportTilesetError::ImportTile { tile_source, err })?;
        self.generate_mips()
            .map_err(ImportTilesetError::GenerateMips)?;
        self.write_mip_bufs();

        let tile_index = self.tile_count;
        self.tile_count += 1;
        Ok(tile_index)
    }

    fn copy_base_image(
        &mut self,
        sources: &[(Image, TilesetSourceFrames)],
        (source_id, tile_index): TileSourceIndex,
    ) -> Result<(), SourceError> {
        if source_id >= sources.len() {
            return Err(SourceError::SourceOutOfRange {
                source_id,
                source_len: sources.len(),
            });
        }

        // Get the source image and tile frame
        let (source, source_frames) = &sources[source_id];
        let TileFrame { frame, anchor } = source_frames
            .get(tile_index)
            .map_err(|err| SourceError::SourceLayout { source_id, err })?;

        // Parameters for indexing into the pixel buffers
        let frame_size = frame.size();
        let frame_row_bytes = frame_size.x as usize * self.pixel_bytes;

        let src_size = source.size();
        let src_row_bytes = src_size.x as usize * self.pixel_bytes;
        let src_data = source.data.as_ref().expect("images are initialized");

        let tgt_size = self.mip_bufs[0].size();
        let tgt_row_bytes = tgt_size.x as usize * self.pixel_bytes;
        let tgt_data = self.mip_bufs[0]
            .data
            .as_mut()
            .expect("images are initialized");

        // Index of the top-left pixel in the source and tile images
        let mut src_i = (frame.min.x + frame.min.y * src_size.x) as usize * self.pixel_bytes;
        let mut tgt_i = (anchor.x + anchor.y * tgt_size.x) as usize * self.pixel_bytes;

        // Copy the tile into the full-size buffer
        for _ in 0..frame_size.y {
            let src_j = src_i + frame_row_bytes;
            let tgt_j = tgt_i + frame_row_bytes;

            tgt_data[tgt_i..tgt_j].copy_from_slice(&src_data[src_i..src_j]);

            src_i += src_row_bytes;
            tgt_i += tgt_row_bytes;
        }

        Ok(())
    }

    fn generate_mips(&mut self) -> Result<(), TextureAccessError> {
        for m in 1..self.mip_bufs.len() {
            let (a, b) = self.mip_bufs.split_at_mut(m);

            let src = &a[m - 1];
            let tgt = &mut b[0];

            if src.size() == 2 * tgt.size() {
                downscale_image_half(src, tgt)?;
            } else {
                downscale_image_bilinear(src, tgt)?;
            }

            self.texture_data
                .extend_from_slice(tgt.data.as_ref().expect("images are initialized"));
        }
        Ok(())
    }

    fn write_mip_bufs(&mut self) {
        for image in &self.mip_bufs {
            self.texture_data
                .extend_from_slice(image.data.as_ref().expect("images are initialized"));
        }
    }
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

/// Downscales `src` into `tgt` using bilinear interpolation.
fn downscale_image_bilinear(src: &Image, tgt: &mut Image) -> Result<(), TextureAccessError> {
    let scale = src.size().as_vec2() / tgt.size().as_vec2();

    for ty in 0..tgt.height() {
        for tx in 0..tgt.width() {
            let sxy_f = scale * UVec2::new(tx, ty).as_vec2();
            let sxy_i = sxy_f.floor();

            let t = sxy_f - sxy_i;
            let sx = sxy_i.x as u32;
            let sy = sxy_i.y as u32;

            let c0 = alpha_discard_lerp(
                src.get_color_at(sx, sy)?,
                src.get_color_at(sx + 1, sy)?,
                t.x,
            );
            let c1 = alpha_discard_lerp(
                src.get_color_at(sx, sy + 1)?,
                src.get_color_at(sx + 1, sy + 1)?,
                t.x,
            );
            let mix_color = alpha_discard_lerp(c0, c1, t.y);

            tgt.set_color_at(tx, ty, mix_color)?;
        }
    }

    Ok(())
}

/// Returns `true` if `alpha` is below the discard threshold.
fn should_discard(alpha: f32) -> bool {
    const ALPHA_CUTOFF: f32 = 1e-4;
    alpha <= ALPHA_CUTOFF
}

/// Mixes a slice of colors, disregarding transparent elements.
fn alpha_discard_mix(colors: &[Color]) -> Color {
    let mut n = 0;
    let mut linear_sum = LinearRgba::NONE;
    for color in colors {
        let linear = color.to_linear();
        if should_discard(linear.alpha) {
            // continue;
            // TODO: Figure out why tile borders are flickering if we don't toss everything.
            return LinearRgba::NONE.into();
        }

        n += 1;
        linear_sum += linear;
    }

    if n > 1 {
        linear_sum /= n as f32;
    }

    if should_discard(linear_sum.alpha) {
        LinearRgba::NONE
    } else {
        linear_sum
    }
    .into()
}

/// Interpolates between two colors, disregarding transparent elements.
fn alpha_discard_lerp(a: Color, b: Color, t: f32) -> Color {
    let a_lin = a.to_linear();
    if should_discard(a_lin.alpha) {
        // return b;
        // TODO: Figure out why tile borders are flickering if we don't toss everything.
        return LinearRgba::NONE.into();
    }

    let b_lin = b.to_linear();
    if should_discard(b_lin.alpha) {
        // return a;
        // TODO: Figure out why tile borders are flickering if we don't toss everything.
        return LinearRgba::NONE.into();
    }

    a_lin.lerp(b_lin, t).into()
}
