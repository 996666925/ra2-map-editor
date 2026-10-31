//! Deferred rectangular ground painting and an exact preview of the plan.
use super::*;
use ra2_map_scene::{
    cliff_tools::rectangle,
    fill_tools::{self, FillPlan},
};

#[derive(Default)]
pub(super) struct FillEdit {
    gesture: Option<Gesture>,
    preview: Option<Preview>,
}
#[derive(PartialEq)]
struct Key {
    document: u64,
    state: u64,
    start: Cell,
    end: Cell,
    brush: usize,
    lat: bool,
}
struct Preview {
    key: Key,
    result: std::result::Result<Arc<FillPlan>, String>,
}
struct Gesture {
    start: Cell,
    last: Cell,
    brush: usize,
    lat: bool,
}
impl FillEdit {
    pub fn cancel(&mut self) {
        *self = Self::default();
    }
    fn generate(
        &mut self,
        doc: &MapDocument,
        scene: &MapScene,
        at: Cell,
    ) -> std::result::Result<Arc<FillPlan>, String> {
        let g = self.gesture.as_ref().unwrap();
        let key = Key {
            document: doc.id,
            state: doc.state_id,
            start: g.start,
            end: at,
            brush: g.brush,
            lat: g.lat,
        };
        self.plan(doc, scene, key)
    }

    fn plan(
        &mut self,
        doc: &MapDocument,
        scene: &MapScene,
        key: Key,
    ) -> std::result::Result<Arc<FillPlan>, String> {
        if self.preview.as_ref().is_none_or(|p| p.key != key) {
            let result = scene
                .brushes
                .get(key.brush)
                .context("请选择填充地表")
                .and_then(|brush| {
                    fill_tools::plan(doc, scene, brush, rectangle(key.start, key.end), key.lat)
                })
                .map(Arc::new)
                .map_err(|e| format!("{e:#}"));
            self.preview = Some(Preview { key, result });
        }
        self.preview.as_ref().unwrap().result.clone()
    }
}
impl Editor {
    pub(super) fn prepare_fill_material(&mut self, scene: &MapScene) {
        if !self.stroking
            && scene
                .brushes
                .get(self.brush)
                .is_none_or(|b| fill_tools::material(scene, b).is_err())
            && let Some(index) = scene
                .brushes
                .iter()
                .position(|b| fill_tools::material(scene, b).is_ok())
        {
            self.brush = index;
        }
    }

    pub(super) fn fill_pointer(
        &mut self,
        scene: &MapScene,
        at: Option<Cell>,
        press: bool,
        down: bool,
        release: bool,
    ) -> bool {
        if self.fill.gesture.is_none()
            && !(self.tool == Tool::Terrain && self.terrain_mode == TerrainMode::Fill)
        {
            return false;
        }
        if press && let Some(at) = at {
            self.prepare_fill_material(scene);
            self.fill.cancel();
            self.feedback.clear();
            self.fill.gesture = Some(Gesture {
                start: at,
                last: at,
                brush: self.brush,
                lat: self.auto_lat,
            });
            self.stroking = true;
        }
        if let Some(g) = self.fill.gesture.as_mut() {
            if (down || press || release)
                && let Some(at) = at
            {
                g.last = at;
            }
            if release || !down {
                let last = g.last;
                let result = self.fill.generate(self.doc.as_ref().unwrap(), scene, last);
                self.fill.cancel();
                self.stroking = false;
                match result {
                    Ok(plan) => match plan.apply(self.doc.as_mut().unwrap()) {
                        Ok(cells) => {
                            self.feedback = cells.into_iter().collect();
                            self.commit(scene, TerrainMode::Fill.name());
                            self.status = format!(
                                "框选铺地 · {} 格修改 · 保留 {} 格结构地形",
                                self.feedback.len(),
                                plan.skipped.len()
                            );
                        }
                        Err(e) => self.abort_stroke(scene, e),
                    },
                    Err(e) => self.abort_stroke(scene, anyhow::anyhow!(e)),
                }
            }
        }
        true
    }

    pub(super) fn fill_preview(
        &mut self,
        painter: &egui::Painter,
        scene: &MapScene,
        rect: egui::Rect,
        camera: &crate::camera::MapCamera,
        at: Option<Cell>,
    ) {
        let Some(doc) = self.doc.as_ref() else { return };
        let at = self.fill.gesture.as_ref().map(|g| g.last).or(at);
        let Some(at) = at else { return };
        let start = self.fill.gesture.as_ref().map_or(at, |g| g.start);
        let result = if self.fill.gesture.is_some() {
            self.fill.generate(doc, scene, at)
        } else {
            self.fill.plan(
                doc,
                scene,
                Key {
                    document: doc.id,
                    state: doc.state_id,
                    start: at,
                    end: at,
                    brush: self.brush,
                    lat: self.auto_lat,
                },
            )
        };
        let world = camera.world_rect(rect);
        let outline = |cell: Cell, level: u8, color: Color32| {
            let (x, y) = vera20k_core::map::terrain::iso_to_screen(cell[0], cell[1], level);
            let center = rect.min + (egui::pos2(x + 30., y + 15.) - world.min) * camera.zoom;
            if !rect.expand(60. * camera.zoom).contains(center) {
                return;
            }
            let points = [[0., -15.], [30., 0.], [0., 15.], [-30., 0.]]
                .map(|[x, y]| center + egui::vec2(x, y) * camera.zoom);
            painter.add(egui::Shape::convex_polygon(
                points.to_vec(),
                color.gamma_multiply(0.12),
                egui::Stroke::new(1., color),
            ));
        };
        let (message, color) = match result {
            Ok(plan) => {
                let mut edits: Vec<_> = plan.edits.iter().collect();
                edits.sort_by_key(|(at, _)| (u32::from(at[0]) + u32::from(at[1]), at[0]));
                for (&cell, v) in edits {
                    let color = if plan.cells.contains(&cell) {
                        Color32::LIGHT_GREEN
                    } else {
                        Color32::LIGHT_BLUE
                    };
                    let origin = egui::pos2(
                        (f32::from(cell[0]) - f32::from(cell[1])) * 30.,
                        f32::from(cell[0] + cell[1]) * 15. - f32::from(v.level) * 15.,
                    );
                    if let Some(&tile) = scene.palette.raw_ids.get(&v.tile)
                        && let Some(image) = scene.palette.previews.get(&(tile, 0))
                    {
                        let position = origin
                            + egui::vec2(image.info.offset[0] as f32, image.info.offset[1] as f32);
                        let target = egui::Rect::from_min_size(
                            rect.min + (position - world.min) * camera.zoom,
                            egui::vec2(image.info.width as f32, image.info.height as f32)
                                * camera.zoom,
                        );
                        if target.intersects(rect)
                            && let Some(texture) = terrain_ui::template_texture(
                                &mut self.ui_state,
                                painter.ctx(),
                                scene,
                                vera20k_core::map::theater::TileKey {
                                    tile_id: tile,
                                    sub_tile: 0,
                                    variant: 0,
                                },
                            )
                        {
                            painter.image(
                                texture.id(),
                                target,
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                                Color32::from_white_alpha(180),
                            );
                        }
                    }
                    outline(cell, v.level, color);
                }
                for &cell in &plan.skipped {
                    outline(
                        cell,
                        terrain_tools::value(doc, scene, cell).level,
                        Color32::from_rgb(245, 180, 70),
                    );
                }
                let dimensions = format!(
                    "{}×{}",
                    start[0].abs_diff(at[0]) + 1,
                    start[1].abs_diff(at[1]) + 1
                );
                (
                    format!(
                        "{dimensions} · 铺地 {} 格 · 保留 {} 格；松手应用 / Esc 取消",
                        plan.cells.len(),
                        plan.skipped.len()
                    ),
                    if plan.cells.is_empty() {
                        Color32::from_rgb(245, 180, 70)
                    } else {
                        Color32::LIGHT_GREEN
                    },
                )
            }
            Err(e) => {
                outline(
                    at,
                    terrain_tools::value(doc, scene, at).level,
                    Color32::LIGHT_RED,
                );
                (e, Color32::LIGHT_RED)
            }
        };
        painter.text(
            rect.left_top() + egui::vec2(12., 12.),
            egui::Align2::LEFT_TOP,
            message,
            egui::FontId::proportional(13.),
            color,
        );
    }
}

#[cfg(test)]
#[path = "fill_edit_tests.rs"]
mod tests;
