//! Publication uses the same immutable static scene as the interactive renderer.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_scene::{
    map::{MapScene, load_scene_bytes},
    visual::ViewOptions,
};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};
#[derive(Default)]
pub struct Publisher {
    cached: Option<(PathBuf, Install)>,
}
impl Publisher {
    pub fn check_profile(
        &mut self,
        directory: &Path,
        expected: Option<&str>,
    ) -> Result<ra2_map_assets::profile::Revision> {
        let revision = ra2_map_assets::profile::Revision::scan(directory)?;
        revision.check(expected)?;
        if self
            .cached
            .as_ref()
            .is_some_and(|(_, install)| *install.revision != revision)
        {
            self.cached = None;
        }
        Ok(revision)
    }
    pub fn scene(&mut self, bytes: Vec<u8>, directory: &Path) -> Result<MapScene> {
        if self
            .cached
            .as_ref()
            .is_none_or(|(path, _)| path != directory)
        {
            self.cached = Some((directory.to_owned(), Install::open(directory)?));
        }
        let scene = load_scene_bytes(
            &mut self.cached.as_mut().unwrap().1,
            "publication.map",
            "发布快照".into(),
            bytes,
            Instant::now(),
        )?;
        ensure!(!scene.images.is_empty(), "缺少可渲染地形，不能生成地图预览");
        Ok(scene)
    }
    pub fn map_bytes(&mut self, bytes: Vec<u8>, directory: &Path) -> Result<Vec<u8>> {
        let scene = self.scene(bytes, directory)?;
        let rect = scene.visual.bounds.unwrap_or(scene.bounds);
        let size = fit(rect, 256);
        let rgba = scene.visual.raster(rect, size, ViewOptions::default())?;
        ra2_map_document::preview::embed(scene.source.bytes(), size[0], size[1], &rgba)
    }
    pub fn png(
        &mut self,
        bytes: Vec<u8>,
        directory: &Path,
        path: &Path,
        max_edge: u32,
        options: ViewOptions,
    ) -> Result<[u32; 2]> {
        let scene = self.scene(bytes, directory)?;
        let rect = scene.visual.bounds.unwrap_or(scene.bounds);
        let size = if max_edge == 0 {
            [rect.width().ceil() as u32, rect.height().ceil() as u32]
        } else {
            fit(rect, max_edge)
        };
        let rgba = scene.visual.raster(rect, size, options)?;
        let img = image::RgbaImage::from_raw(size[0], size[1], rgba).context("导出图像尺寸无效")?;
        let temp = path.with_extension(format!("{}.tmp", std::process::id()));
        img.save_with_format(&temp, image::ImageFormat::Png)
            .context("写入导出 PNG 失败")?;
        // Replace only a successfully encoded temporary output.
        std::fs::rename(temp, path)?;
        Ok(size)
    }
}
pub fn fit(rect: egui::Rect, max_edge: u32) -> [u32; 2] {
    let scale = (max_edge as f32 / rect.width().max(rect.height())).min(1.);
    [
        (rect.width() * scale).round().max(1.) as u32,
        (rect.height() * scale).round().max(1.) as u32,
    ]
}
