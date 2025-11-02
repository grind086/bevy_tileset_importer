use bevy::{
    prelude::*,
    sprite_render::{AlphaMode2d, TileData, TilemapChunk, TilemapChunkTileData},
};
use bevy_log::LogPlugin;
use bevy_tileset_importer::{TileIndex, Tileset, TilesetImporterPlugin};

const TILE_HEIGHT: f32 = 64.;
const TILE_DISPLAY_SIZE: UVec2 = UVec2::splat(TILE_HEIGHT as _);

fn main() -> AppExit {
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(AssetPlugin {
                    mode: AssetMode::Processed,
                    ..default()
                })
                .set(LogPlugin {
                    filter: "info,wgpu=error,naga=warn,bevy_tileset_importer=trace".into(),
                    ..default()
                }),
            TilesetImporterPlugin,
        ))
        .add_systems(Startup, (setup_scene, load_tileset))
        .add_systems(Update, make_tilemaps)
        .run()
}

// Scene setup that is not specifically related to tileset functionality.
fn setup_scene(mut commands: Commands) {
    // Camera
    commands.spawn(Camera2d);

    // Labels
    commands.spawn((
        Text2d::new("Alphabet"),
        TextFont {
            font_size: 0.5 * TILE_HEIGHT,
            ..Default::default()
        },
        Transform::from_xyz(0.0, 3. * TILE_HEIGHT, 0.),
    ));

    commands.spawn((
        Text2d::new("Vowels"),
        TextFont {
            font_size: 0.5 * TILE_HEIGHT,
            ..Default::default()
        },
        Transform::from_xyz(0.0, TILE_HEIGHT, 0.),
    ));

    commands.spawn((
        Text2d::new("Consonants"),
        TextFont {
            font_size: 0.5 * TILE_HEIGHT,
            ..Default::default()
        },
        Transform::from_xyz(0.0, -TILE_HEIGHT, 0.),
    ));
}

#[derive(Resource)]
struct TilesetHandle(Handle<Tileset>);

fn load_tileset(asset_server: Res<AssetServer>, mut commands: Commands) {
    // To access the imported tile groups, we have to wait for the tileset to actually load.
    commands.insert_resource(TilesetHandle(asset_server.load("minimal.ts.ron")));

    // But if we don't need to dynamically get tile indices, we can create a `TilemapChunk`
    // without waiting. It still won't actually render until everything is loaded, but we don't
    // have to manually do anything else.
    commands.spawn((
        Transform::from_xyz(0., 2. * TILE_HEIGHT, 0.),
        tile_strip(
            // Note that we select the `texture` sub-asset to get the tileset `Image` handle.
            asset_server.load("minimal.ts.ron#texture"),
            // A, B, C, D, E, F
            &[0, 1, 2, 3, 4, 5],
        ),
    ));
}

fn make_tilemaps(
    mut run_once: Local<bool>,
    handle: Res<TilesetHandle>,
    assets: Res<Assets<Tileset>>,
    mut commands: Commands,
) {
    // Normally you'd use states or run conditions rather than doing this with a `Local`
    if *run_once {
        return;
    }

    let Some(tileset) = assets.get(&handle.0) else {
        return;
    };

    *run_once = true;

    let vowel_tiles = tileset.group("vowels");
    let consonant_tiles = tileset.group("consonants");

    commands.spawn((
        Transform::from_xyz(0., 0., 0.),
        tile_strip(tileset.texture.clone(), vowel_tiles),
    ));

    commands.spawn((
        Transform::from_xyz(0., -2. * TILE_HEIGHT, 0.),
        tile_strip(tileset.texture.clone(), consonant_tiles),
    ));
}

fn tile_strip(tileset: Handle<Image>, tiles: &[TileIndex]) -> impl Bundle {
    let n = 2 * tiles.len() - 1;

    (
        TilemapChunk {
            chunk_size: UVec2::new(n as _, 1),
            tile_display_size: TILE_DISPLAY_SIZE,
            tileset,
            alpha_mode: AlphaMode2d::Opaque,
        },
        // Some(0), None, Some(1), None, .., Some(4), None, Some(5)
        TilemapChunkTileData(
            (0..n)
                .map(|i| {
                    (i % 2 == 0)
                        .then_some(i / 2)
                        .map(|i| TileData::from_tileset_index(tiles[i]))
                })
                .collect(),
        ),
    )
}
