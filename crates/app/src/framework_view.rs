//! Shared framework subtiles for the flattened canvas and placement ghosts.
use crate::{
    camera::MapCamera,
    document::{Cell, TerrainValue},
    map::MapScene,
};
use eframe::egui::{self, Color32};
use std::collections::HashMap;
use vera20k_core::map::theater::TileKey;

#[derive(Clone, Copy)]
pub(crate) struct View<'a> {
    pub rect: egui::Rect,
    pub camera: &'a MapCamera,
    pub flat: bool,
}
#[derive(Default)]
pub(crate) struct Textures {
    source: usize,
    images: HashMap<(TileKey, u8), egui::TextureHandle>,
}
impl Textures {
    pub fn draw(
        &mut self,
        painter: &egui::Painter,
        scene: &MapScene,
        view: View<'_>,
        at: Cell,
        value: TerrainValue,
        tint: Color32,
    ) {
        let source = std::sync::Arc::as_ptr(&scene.palette) as usize;
        if source != self.source {
            self.source = source;
            self.images.clear();
        }
        let Some(&tile) = scene.palette.raw_ids.get(&value.tile) else {
            return;
        };
        let key = TileKey {
            tile_id: tile,
            sub_tile: value.sub,
            variant: 0,
        };
        let Some(image) = scene.palette.framework_images.get(&key) else {
            return;
        };
        let (x, y) = vera20k_core::map::terrain::iso_to_screen(
            at[0],
            at[1],
            if view.flat { 0 } else { value.level },
        );
        let origin = egui::pos2(
            x + image.info.offset[0] as f32,
            y + image.info.offset[1] as f32,
        );
        let target = egui::Rect::from_min_size(
            view.rect.min + (origin - view.camera.world_rect(view.rect).min) * view.camera.zoom,
            egui::vec2(image.info.width as f32, image.info.height as f32) * view.camera.zoom,
        );
        if !target.intersects(view.rect) {
            return;
        }
        let cache_key = (
            key,
            if scene.palette.framework_height_tile(key) {
                value.level
            } else {
                0
            },
        );
        if self.images.len() >= 512 && !self.images.contains_key(&cache_key) {
            self.images.clear();
        }
        let texture = self.images.entry(cache_key).or_insert_with(|| {
            painter.ctx().load_texture(
                format!("framework-{cache_key:?}"),
                egui::ColorImage::from_rgba_unmultiplied(
                    [image.info.width as usize, image.info.height as usize],
                    &scene.palette.framework_rgba(key, value.level).unwrap(),
                ),
                egui::TextureOptions::NEAREST,
            )
        });
        painter.image(
            texture.id(),
            target,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
            tint,
        );
    }
}
