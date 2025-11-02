use core::{
    fmt::Debug,
    ops::{Deref, Index, Range},
};

use bevy_app::{App, Plugin};
use bevy_asset::{Asset, AssetApp, AssetLoader, Handle};
use bevy_image::Image;
use bevy_platform::collections::{HashMap, hash_map::Entry};
use bevy_reflect::TypePath;
use bevy_log::trace;

pub mod import;
pub mod layout;
pub mod loader;

mod helpers;

pub type TileIndex = u16;
pub type TileSourceIndex = (usize, TileIndex);

pub struct TilesetLoaderPlugin;

impl Plugin for TilesetLoaderPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<Tileset>()
            .init_asset_loader::<import::ImportedTilesetLoader>()
            .init_asset_loader::<loader::ImageTilesetLoader>()
            .init_asset_loader::<loader::DataTilesetLoader>()
            .register_asset_processor(import::import_process::<loader::DataTilesetLoader>())
            .register_asset_processor(import::import_process::<loader::ImageTilesetLoader>());

        loader::IMAGE_EXTS.iter().for_each(|ext| {
            trace!("Set asset process for {ext:?} to `TilesetImportProcess<ImageTilesetLoader>`");
            app.set_default_asset_processor::<import::TilesetImportProcess<loader::ImageTilesetLoader>>(ext);
        });

        loader::DataTilesetLoader.extensions().iter().for_each(|ext| {
            trace!("Set asset process for {ext:?} to `TilesetImportProcess<DataTilesetLoader>`");
            app.set_default_asset_processor::<import::TilesetImportProcess<loader::DataTilesetLoader>>(ext);
        });
    }
}

/// An asset containing a tileset texture, and a set of named tile groups.
///
/// ```ignore
/// let tileset = asset_server.load("my_tileset.ts.ron");
/// let texture = asset_server.load("my_tileset.ts.ron#texture");
/// ```
#[derive(Asset, TypePath, Default, Clone)]
pub struct Tileset {
    #[dependency]
    pub texture: Handle<Image>,
    pub groups: TileGroups,
}

impl Deref for Tileset {
    type Target = TileGroups;
    fn deref(&self) -> &Self::Target {
        &self.groups
    }
}

/// Named groups of tiles in a [`Tileset`].
#[derive(Default, Clone)]
pub struct TileGroups {
    groups: HashMap<String, Range<usize>>,
    group_tiles: Vec<TileIndex>,
}

impl TileGroups {
    /// Adds the given tile indices to the group `name` if it doesn't already exist.
    ///
    /// Returns `true` if the group was successfully created, and `false` if it already existed.
    pub fn insert_if_new(&mut self, name: impl Into<String>, tiles: &[TileIndex]) -> bool {
        let tile_range = self.group_tiles.len()..(self.group_tiles.len() + tiles.len());
        if self.groups.try_insert(name.into(), tile_range).is_ok() {
            self.group_tiles.extend_from_slice(tiles);
            true
        } else {
            false
        }
    }

    /// Appends the given tile indices to the group `name`, creating it if it doesn't exist.
    pub fn insert(&mut self, name: impl Into<String>, tiles: &[TileIndex]) {
        let old_tiles_len = self.group_tiles.len();
        let add_tiles_len = tiles.len();
        let new_tiles_len = old_tiles_len + add_tiles_len;

        match self.groups.entry(name.into()) {
            // If this is a new group, we can just add the tiles to the end of the list.
            Entry::Vacant(e) => {
                self.group_tiles.extend_from_slice(tiles);
                e.insert(old_tiles_len..new_tiles_len);
            }
            // If a group with the given name already exists, add the new tiles to that group.
            Entry::Occupied(mut e) => {
                let head = e.get().start;
                let old_tail = e.get().end;

                if old_tail == old_tiles_len {
                    // When the group being extended is at the end of the tiles list, we can just
                    // extend the list and save the new range.
                    self.group_tiles.extend_from_slice(tiles);
                    e.insert(head..new_tiles_len);
                } else {
                    // Otherwise we have to make room at the end of the group's range by moving
                    // existing groups further down the list.
                    let new_tail = old_tail + add_tiles_len;

                    self.group_tiles.resize(new_tiles_len, 0);
                    self.group_tiles
                        .copy_within(old_tail..old_tiles_len, new_tail);
                    self.group_tiles[old_tail..new_tail].copy_from_slice(tiles);

                    e.insert(head..new_tail);

                    // Fixup other ranges
                    for range in self.groups.values_mut() {
                        if range.start >= old_tail {
                            *range = (range.start + add_tiles_len)..(range.end + add_tiles_len);
                        }
                    }
                }
            }
        }
    }

    /// Returns a slice of tile indices for the given group name.
    pub fn group(&self, name: &str) -> &[TileIndex] {
        self.get_group(name).unwrap_or(&[])
    }

    /// Returns a slice of tile indices for the given group name, or `None` if the group was not
    /// explicitly created.
    pub fn get_group(&self, name: &str) -> Option<&[TileIndex]> {
        self.groups.get(name).map(|r| &self.group_tiles[r.clone()])
    }

    /// An iterator visiting all tile groups.
    pub fn iter_groups(&self) -> impl Iterator<Item = (&str, &[TileIndex])> {
        self.groups
            .iter()
            .map(|(name, r)| (name.as_str(), &self.group_tiles[r.clone()]))
    }
}

impl FromIterator<(String, Vec<TileIndex>)> for TileGroups {
    fn from_iter<T: IntoIterator<Item = (String, Vec<TileIndex>)>>(iter: T) -> Self {
        let mut this = Self::default();
        for (name, tiles) in iter {
            this.insert(name, &tiles);
        }
        this
    }
}

impl Index<&'_ str> for TileGroups {
    type Output = [TileIndex];
    fn index(&self, name: &str) -> &Self::Output {
        self.group(name)
    }
}

impl Debug for TileGroups {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        struct DebugMap<'a>(&'a TileGroups);
        impl Debug for DebugMap<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_map().entries(self.0.iter_groups()).finish()
            }
        }

        f.debug_struct("TileGroups")
            .field("groups", &DebugMap(self))
            .finish()
    }
}
