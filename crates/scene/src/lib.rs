//! CPU scene projection built from documents and decoded game assets.
pub mod bridge_tools;
pub mod cliff_authoring;
pub mod cliff_tools;
pub mod coastal_cliffs;
pub mod map;
pub mod placement;
pub mod road_tools;
pub mod shore_tools;
pub mod terrain_tools;

pub mod visual;

#[cfg(test)]
mod joining_tests;
mod mission_catalog;

pub mod work;
