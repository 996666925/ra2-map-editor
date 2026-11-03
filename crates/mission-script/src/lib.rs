//! Embedded TypeScript transpilation and native mission validation. No external runtime.
pub mod campaign;
pub mod capabilities;
pub mod catalog;
pub mod native;
pub mod typescript;

pub const SDK: &str = include_str!("../sdk/mission.ts");

#[cfg(test)]
mod tests;
