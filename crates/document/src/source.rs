//! Original map bytes and metadata, independent of rendering and game resources.
use anyhow::{Context, Result, ensure};
use std::{path::Path, sync::Arc};
use vera20k_core::map::map_file::{self, MapFile, MapHeader};

pub const MAX_SOURCE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone)]
pub struct SourceMap {
    bytes: Vec<u8>,
    pub name: String,
    pub origin: String,
    pub header: MapHeader,
    pub title: String,
    pub edit_base: Result<Arc<crate::document::Baseline>, String>,
}
impl SourceMap {
    pub fn parse(name: String, origin: String, bytes: Vec<u8>) -> Result<(Self, MapFile)> {
        ensure!(bytes.len() <= MAX_SOURCE_BYTES, "地图原文超过 32 MiB 上限");
        let map = map_file::load_from_bytes(bytes.clone()).context("地图解析失败")?;
        let edit_base = crate::document::Baseline::parse(Arc::from(bytes.clone()), &map)
            .map_err(|error| format!("{error:#}"));
        // New editor-authored names are UTF-8; keep legacy byte names on the
        // core's existing path when the original value is not valid UTF-8.
        let title = edit_base
            .as_ref()
            .ok()
            .and_then(|base| {
                base.raw
                    .lines
                    .iter()
                    .enumerate()
                    .find(|(_, line)| {
                        line.section == "Basic" && line.key.as_deref() == Some("Name")
                    })
                    .and_then(|(i, _)| std::str::from_utf8(base.raw.value(i)).ok())
            })
            .map(|name| name.split(';').next().unwrap_or_default().trim().to_owned())
            .unwrap_or_else(|| map.basic.name.clone().unwrap_or_default());
        let source = Self {
            header: map.header.clone(),
            title,
            bytes,
            name,
            origin,
            edit_base,
        };
        Ok((source, map))
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// P1 has no mutations; preserve encoding, comments, unknown sections and wrappers.
    /// create_new prevents accidental overwrite; the original is never truncated.
    pub fn export_new(&self, path: &Path) -> Result<()> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| format!("无法创建副本（目标必须不存在）：{}", path.display()))?;
        file.write_all(&self.bytes)?;
        file.sync_all()?;
        Ok(())
    }
}
