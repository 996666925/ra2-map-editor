//! Background save, backup, recovery and isolated VERA20k playtest services.
pub mod editor_io;
#[cfg(test)]
mod editor_io_tests;

pub mod campaign;
pub mod cli;
pub mod playtest;
pub mod provenance;
pub mod publication;
