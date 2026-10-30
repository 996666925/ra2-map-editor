//! Cached symbols shared with the map's embedded radar preview.
use crate::{editing::Editor, map::MapScene};
use eframe::egui::{self, Color32};

#[derive(PartialEq)]
struct Key {
    revision: u64,
    flags: [bool; 4],
    layers: [bool; 3],
    size: [usize; 2],
    bounds: egui::Rect,
}
#[derive(Default)]
pub(crate) struct OverviewOverlay {
    cached: Option<(Key, egui::TextureHandle)>,
}
impl OverviewOverlay {
    pub fn draw(
        &mut self,
        painter: &egui::Painter,
        scene: &MapScene,
        editor: &Editor,
        rect: egui::Rect,
        flags: [bool; 4],
        bounds: egui::Rect,
    ) {
        let dpi = painter.ctx().pixels_per_point();
        let size = [
            (rect.width() * dpi).ceil().max(1.) as usize,
            (rect.height() * dpi).ceil().max(1.) as usize,
        ];
        let key = Key {
            revision: editor.visual_revision,
            flags,
            layers: editor.layers,
            size,
            bounds,
        };
        if self.cached.as_ref().is_none_or(|(old, _)| old != &key) {
            let symbols = editor
                .doc
                .as_ref()
                .map(|doc| ra2_map_scene::radar::symbols(scene, doc))
                .unwrap_or_default();
            let flags = [
                flags[0] && editor.layers[2],
                flags[1] && editor.layers[0],
                flags[2] && editor.layers[1],
                flags[3] && editor.layers[1],
            ];
            let image = ra2_map_scene::radar::overlay(
                size,
                bounds,
                &symbols,
                flags,
                size[0].max(size[1]) as f32 / 192.,
            );
            self.cached = Some((
                key,
                painter.ctx().load_texture(
                    "navigator-symbols",
                    image,
                    egui::TextureOptions::LINEAR,
                ),
            ));
        }
        painter.image(
            self.cached.as_ref().unwrap().1.id(),
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
            Color32::WHITE,
        );
    }
}
