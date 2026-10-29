//! Navigator symbols are baked once per document revision/options/size. Moving
//! the camera only redraws its viewport box and the few player-start labels.
use crate::{editing::Editor, map::MapScene};
use eframe::egui::{self, Color32};

#[derive(PartialEq)]
struct Key {
    revision: u64,
    flags: [bool; 4],
    layers: [bool; 3],
    size: [usize; 2],
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
    ) {
        let project = |pos: egui::Pos2| {
            rect.min + (pos - scene.bounds.min) / scene.bounds.size() * rect.size()
        };
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
        };
        if self.cached.as_ref().is_none_or(|(old, _)| old != &key) {
            let mut pixels = vec![Color32::TRANSPARENT; size[0] * size[1]];
            let mut circle = |pos: egui::Pos2, radius: f32, color: Color32| {
                let center = (project(pos) - rect.min) * dpi;
                let radius = radius * dpi;
                let left = (center.x - radius - 1.).floor().max(0.) as usize;
                let top = (center.y - radius - 1.).floor().max(0.) as usize;
                let right = ((center.x + radius + 1.).ceil().max(0.) as usize).min(size[0]);
                let bottom = ((center.y + radius + 1.).ceil().max(0.) as usize).min(size[1]);
                for y in top..bottom {
                    for x in left..right {
                        let a = (radius + 0.5
                            - (egui::vec2(x as f32 + 0.5, y as f32 + 0.5) - center).length())
                        .clamp(0., 1.);
                        if a > 0. {
                            let color = Color32::from_rgba_unmultiplied(
                                color.r(),
                                color.g(),
                                color.b(),
                                (a * 255.) as u8,
                            );
                            pixels[y * size[0] + x] = pixels[y * size[0] + x].blend(color);
                        }
                    }
                }
            };
            if flags[1]
                && editor.layers[0]
                && let Some(doc) = &editor.doc
            {
                for (&at, &(id, _)) in &editor.overlays {
                    let gem = scene.catalog.gem_family(id);
                    if scene.catalog.ore_family(id) || gem {
                        let level = doc
                            .terrain(at)
                            .map_or(scene.source.header.level as u8, |v| v.level);
                        let (x, y) = vera20k_core::map::terrain::iso_to_screen(at[0], at[1], level);
                        circle(
                            egui::pos2(x + 30., y + 15.),
                            1.,
                            if gem {
                                Color32::from_rgb(230, 80, 220)
                            } else {
                                Color32::from_rgb(255, 205, 50)
                            },
                        );
                    }
                }
            }
            if editor.layers[1] {
                for marker in &editor.marker_cache.markers {
                    let show = if marker.radar_radius > 1.5 {
                        flags[2]
                    } else {
                        flags[3]
                    };
                    if show && marker.radar_radius > 0. {
                        circle(
                            marker.radar_origin,
                            marker.radar_radius,
                            Color32::from_rgb(255, 135, 65),
                        );
                    }
                }
            }
            self.cached = Some((
                key,
                painter.ctx().load_texture(
                    "navigator-symbols",
                    egui::ColorImage::new(size, pixels),
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
        if flags[0] && editor.layers[2] {
            for marker in &editor.marker_cache.markers {
                if let Some(label) = &marker.waypoint_label {
                    let point = project(marker.radar_origin);
                    painter.circle_filled(point, 3., Color32::BLACK);
                    painter.text(
                        point,
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::proportional(9.),
                        Color32::WHITE,
                    );
                }
            }
        }
    }
}
