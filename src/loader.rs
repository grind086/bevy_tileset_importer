use bevy_asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader};
use bevy_image::{ImageSampler, ImageSamplerDescriptor};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    Tileset,
    format::{TilesetFile, TilesetFileError},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TilesetLoaderSettings {
    /// Sets the sampler that will be used for the tileset texture.
    #[serde(default, deserialize_with = "TilesetLoaderSettings::de_sampler")]
    pub sampler: ImageSampler,
    /// Sets the asset usage for the tileset texture. Defaults to `RENDER_WORLD`.
    ///
    /// If you need to manually access the texture data after it is loaded, set this to
    /// `RENDER_WORLD | MAIN_WORLD`.
    #[serde(default = "TilesetLoaderSettings::default_asset_usage")]
    pub asset_usage: RenderAssetUsages,
}

impl TilesetLoaderSettings {
    const fn default_asset_usage() -> RenderAssetUsages {
        RenderAssetUsages::RENDER_WORLD
    }

    fn de_sampler<'de, D>(de: D) -> Result<ImageSampler, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        TilesetSamplerSetting::deserialize(de).map(Into::into)
    }
}

impl Default for TilesetLoaderSettings {
    fn default() -> Self {
        Self {
            sampler: ImageSampler::Default,
            asset_usage: Self::default_asset_usage(),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename = "ImageSampler")]
enum TilesetSamplerSetting {
    #[default]
    Default,
    Linear,
    Nearest,
    Descriptor(ImageSamplerDescriptor),
}

impl From<TilesetSamplerSetting> for ImageSampler {
    fn from(value: TilesetSamplerSetting) -> Self {
        match value {
            TilesetSamplerSetting::Default => Self::Default,
            TilesetSamplerSetting::Linear => Self::linear(),
            TilesetSamplerSetting::Nearest => Self::nearest(),
            TilesetSamplerSetting::Descriptor(desc) => Self::Descriptor(desc),
        }
    }
}

pub struct TilesetLoader {
    /// The file extension to use for auto-detecting this loader, without the leading dot. May be
    /// set to `None` to disable extension-based detection.
    ///
    /// The default is [`TilesetLoader::DEFAULT_EXTENSION`].
    pub file_extension: Option<&'static str>,
}

impl TilesetLoader {
    /// The default file extension: **b**evy **t**ile**s**et.
    pub const DEFAULT_EXTENSION: &str = "bts";

    /// Create a loader using the given file extension.
    ///
    /// See [`TilesetLoader::file_extension`].
    pub const fn with_extension(ext: &'static str) -> Self {
        Self {
            file_extension: Some(ext),
        }
    }

    /// Create a loader with no file extensions.
    ///
    /// See [`TilesetLoader::file_extension`].
    pub const fn without_extension() -> Self {
        Self {
            file_extension: None,
        }
    }
}

impl Default for TilesetLoader {
    fn default() -> Self {
        Self::with_extension(Self::DEFAULT_EXTENSION)
    }
}

impl AssetLoader for TilesetLoader {
    type Asset = Tileset;
    type Settings = TilesetLoaderSettings;
    type Error = TilesetLoaderError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;

        let (count, groups, mut image) =
            TilesetFile::read(bytes.as_slice())?.into_count_groups_image()?;
        image.sampler = settings.sampler.clone();
        image.asset_usage = settings.asset_usage;

        let texture = load_context.add_labeled_asset("texture".into(), image);

        Ok(Tileset {
            texture,
            count,
            groups,
        })
    }

    fn extensions(&self) -> &[&str] {
        self.file_extension.as_slice()
    }
}

#[derive(Debug, Error)]
pub enum TilesetLoaderError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    TilesetFile(#[from] TilesetFileError),
}
