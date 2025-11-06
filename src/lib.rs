use std::ops::{Deref, Range};

use bevy_app::{App, Plugin};
use bevy_asset::{Asset, AssetApp, Handle};
use bevy_image::Image;
use bevy_platform::collections::HashMap;
use bevy_reflect::TypePath;

pub use crate::importer::TilesetImportSettings;

pub type TileIndex = u16;
pub type TileSourceIndex = (usize, TileIndex);

pub mod format;
pub mod importer;
pub mod layout;
pub mod loader;
pub mod process;

#[derive(Default)]
pub struct TilesetImporterPlugin;

impl Plugin for TilesetImporterPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<Tileset>()
            .init_asset_loader::<loader::TilesetLoader>()
            .init_asset_loader::<process::ImageTilesetLoader>()
            .init_asset_loader::<process::DataTilesetLoader>()
            .register_asset_processor(process::ImageProcess::default())
            .register_asset_processor(process::DataProcess::default());

        for ext in process::DATA_EXTS {
            app.set_default_asset_processor::<process::DataProcess>(ext);
        }
    }
}

#[derive(Asset, Clone, TypePath)]
pub struct Tileset {
    #[dependency]
    pub texture: Handle<Image>,
    pub count: TileIndex,
    pub groups: TileGroups,
}

impl Deref for Tileset {
    type Target = TileGroups;
    fn deref(&self) -> &Self::Target {
        &self.groups
    }
}

#[derive(Debug, Default, Clone)]
pub struct TileGroups {
    ranges: HashMap<String, Range<usize>>,
    indices: Vec<TileIndex>,
}

impl TileGroups {
    pub fn group(&self, name: &str) -> &[TileIndex] {
        self.get_group(name).unwrap_or(&[])
    }

    pub fn get_group(&self, name: &str) -> Option<&[TileIndex]> {
        self.ranges.get(name).map(|r| &self.indices[r.clone()])
    }
}
