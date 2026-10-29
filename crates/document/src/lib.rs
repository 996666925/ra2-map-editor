//! Authoritative map documents, raw INI, new-map generation and source parsing.
pub mod ai_triggers;
pub mod base_nodes;
pub mod campaign;
pub mod document;
#[cfg(test)]
mod document_tests;
pub mod ini_edit;
pub mod lighting;
pub mod map_settings;
pub mod mission;
pub mod new_map;
pub mod object_properties;
pub mod raw_ini;
pub mod region;
pub mod source;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub mod preview;
