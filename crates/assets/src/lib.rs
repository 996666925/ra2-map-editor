//! YR installation resources, decoded assets and authoring catalogs.
pub mod assets;
pub mod catalog;
pub mod map_sprites;
pub mod sprites;
pub mod terrain_library;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub mod mission_resources;

pub mod profile;
