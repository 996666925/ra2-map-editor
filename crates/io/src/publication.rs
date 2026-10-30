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

pub use ra2_map_scene::radar::MAX_EDGE as PREVIEW_MAX_EDGE;

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
        embed_scene_preview(&scene)
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
        write_png(&img, path)?;
        Ok(size)
    }
}

/// Generate the same radar terrain and symbols used by the navigator.
pub fn render_preview(scene: &MapScene) -> Result<image::RgbaImage> {
    let radar = ra2_map_scene::radar::terrain(scene)?;
    let base = scene
        .source
        .edit_base
        .as_ref()
        .map_err(|error| anyhow::anyhow!(error.clone()))?;
    let doc = ra2_map_document::document::MapDocument::new(base.clone());
    let symbols = ra2_map_scene::radar::symbols(scene, &doc);
    let size = radar.image.size;
    let overlay = ra2_map_scene::radar::overlay(
        size,
        radar.bounds,
        &symbols,
        [true; 4],
        size[0].max(size[1]) as f32 / 192.,
    );
    let rgba = radar
        .image
        .pixels
        .iter()
        .zip(&overlay.pixels)
        .flat_map(|(background, symbol)| background.blend(*symbol).to_array())
        .collect();
    image::RgbaImage::from_raw(size[0] as u32, size[1] as u32, rgba).context("地图预览图像尺寸无效")
}

pub fn embed_scene_preview(scene: &MapScene) -> Result<Vec<u8>> {
    let preview = render_preview(scene)?;
    ra2_map_document::preview::embed(
        scene.source.bytes(),
        preview.width(),
        preview.height(),
        preview.as_raw(),
    )
}

/// Export the authored thumbnail verbatim, including custom PreviewPack artwork.
/// This does not render the map or require game resources.
pub fn preview_png(bytes: &[u8], path: &Path) -> Result<[u32; 2]> {
    let ini = vera20k_core::rules::ini_parser::IniFile::from_bytes(bytes)
        .context("地图预览 INI 解析失败")?;
    let preview = vera20k_core::map::preview::decode_preview_image_from_ini(&ini)
        .context("地图内嵌预览解码失败")?
        .context("地图没有内嵌预览（PreviewPack）；请先保存地图生成预览，再重新加载后导出")?;
    let size = [preview.width, preview.height];
    let img = image::RgbaImage::from_raw(size[0], size[1], preview.rgba)
        .context("地图预览图像尺寸无效")?;
    write_png(&img, path)?;
    Ok(size)
}

fn write_png(img: &image::RgbaImage, path: &Path) -> Result<()> {
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    img.save_with_format(&temp, image::ImageFormat::Png)
        .context("写入导出 PNG 失败")?;
    // Replace only a successfully encoded temporary output.
    std::fs::rename(temp, path)?;
    Ok(())
}

pub fn fit(rect: egui::Rect, max_edge: u32) -> [u32; 2] {
    let scale = (max_edge as f32 / rect.width().max(rect.height())).min(1.);
    [
        (rect.width() * scale).round().max(1.) as u32,
        (rect.height() * scale).round().max(1.) as u32,
    ]
}
