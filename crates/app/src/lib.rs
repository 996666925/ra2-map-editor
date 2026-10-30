//! Editor UI and application orchestration. Lower layers never depend on this crate.
use ra2_map_assets::assets;
use ra2_map_document::{document, new_map};
use ra2_map_io::editor_io;
use ra2_map_render::{camera, gpu, terrain_gpu};
use ra2_map_scene::map;

pub mod app;
mod base_nodes_ui;
mod campaign_ui;
mod editing;
mod labels;
mod map_loader;
mod map_settings_ui;
mod map_view;
mod mission_overlay;
mod mission_ui;
mod overview;
mod ui_theme;

#[cfg(test)]
mod viewport_perf;
