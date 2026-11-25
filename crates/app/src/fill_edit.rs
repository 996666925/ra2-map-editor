//! Deferred ground and ice painting, with an exact preview of the plan.
use super::*;
use crate::i18n::tr;
use ra2_map_scene::{
    cliff_tools::rectangle,
    fill_tools::{self, FillPlan},
    ice_tools,
};

#[derive(Default)]
pub(super) struct FillEdit {
    gesture: Option<Gesture>,
    preview: Option<Preview>,
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum FillStyle {
    #[default]
    Rectangle,
    Brush,
}

const MAX_BRUSH_SIZE: u16 = 9;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Rectangle,
    Connected,
    Brush(u16),
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Material {
    Ground { brush: usize, lat: bool },
    Ice(ice_tools::Options),
}
#[derive(PartialEq)]
struct Key {
    document: u64,
    state: u64,
    palette: usize,
    start: Cell,
    end: Cell,
    material: Material,
    shape: Shape,
    brushed_len: Option<usize>,
}
struct Preview {
    key: Key,
    result: std::result::Result<Arc<FillPlan>, String>,
}
struct Gesture {
    start: Cell,
    last: Cell,
    material: Material,
    shape: Shape,
    previous: Option<Cell>,
    brushed: BTreeSet<Cell>,
}

fn stamp(doc: &MapDocument, cells: &mut BTreeSet<Cell>, center: Cell, size: u16) {
    let half = (size / 2) as i16;
    for dx in 0..size as i16 {
        for dy in 0..size as i16 {
            if let Some(x) = center[0].checked_add_signed(dx - half)
                && let Some(y) = center[1].checked_add_signed(dy - half)
                && doc.contains_cell([x, y])
            {
                cells.insert([x, y]);
            }
        }
    }
}

impl Gesture {
    fn sample(&mut self, doc: &MapDocument, at: Option<Cell>) {
        match self.shape {
            Shape::Connected => {}
            Shape::Rectangle => {
                if let Some(at) = at {
                    self.last = at;
                }
            }
            Shape::Brush(size) => {
                let at = at.filter(|&at| doc.contains_cell(at));
                let previous = std::mem::replace(&mut self.previous, at);
                if let Some(at) = at {
                    self.last = at;
                    if previous != Some(at) {
                        for center in cell_line(previous.unwrap_or(at), at) {
                            stamp(doc, &mut self.brushed, center, size);
                        }
                    }
                }
            }
        }
    }
}

impl FillEdit {
    pub fn cancel(&mut self) {
        *self = Self::default();
    }
    fn generate(
        &mut self,
        doc: &MapDocument,
        scene: &MapScene,
    ) -> std::result::Result<Arc<FillPlan>, String> {
        let g = self.gesture.as_ref().unwrap();
        let brushed = matches!(g.shape, Shape::Brush(_)).then_some(&g.brushed);
        let key = Key {
            document: doc.id,
            state: doc.edit_revision(),
            palette: Arc::as_ptr(&scene.palette) as usize,
            start: g.start,
            // The footprint only grows; moving within it need not re-plan the stroke.
            end: if brushed.is_some() { g.start } else { g.last },
            material: g.material,
            shape: g.shape,
            brushed_len: brushed.map(BTreeSet::len),
        };
        Self::plan(&mut self.preview, doc, scene, key, brushed)
    }

    fn plan(
        preview: &mut Option<Preview>,
        doc: &MapDocument,
        scene: &MapScene,
        key: Key,
        brushed: Option<&BTreeSet<Cell>>,
    ) -> std::result::Result<Arc<FillPlan>, String> {
        if preview.as_ref().is_none_or(|p| p.key != key) {
            let result = (|| {
                let cells = match key.shape {
                    Shape::Connected => fill_tools::connected(doc, scene, key.start)?,
                    Shape::Rectangle => rectangle(key.start, key.end),
                    Shape::Brush(size) => brushed.cloned().unwrap_or_else(|| {
                        let mut cells = BTreeSet::new();
                        stamp(doc, &mut cells, key.start, size);
                        cells
                    }),
                };
                match key.material {
                    Material::Ground { brush, lat } => {
                        let brush = scene.brushes.get(brush).context(tr!("请选择填充地表"))?;
                        fill_tools::plan(doc, scene, brush, cells, lat)
                    }
                    Material::Ice(options) => ice_tools::plan(doc, scene, cells, options),
                }
            })()
            .map(Arc::new)
            .map_err(|e| format!("{e:#}"));
            *preview = Some(Preview { key, result });
        }
        preview.as_ref().unwrap().result.clone()
    }
}
impl Editor {
    fn fill_shape(&self) -> Shape {
        if !self.terrain_mode.uses_fill_gesture() {
            Shape::Connected
        } else if self.fill_style == FillStyle::Brush {
            Shape::Brush(self.fill_brush_size.clamp(1, MAX_BRUSH_SIZE))
        } else {
            Shape::Rectangle
        }
    }

    pub(crate) fn fill_brushing(&self) -> bool {
        if let Some(g) = &self.fill.gesture {
            matches!(g.shape, Shape::Brush(_))
        } else {
            self.tool == Tool::Terrain && matches!(self.fill_shape(), Shape::Brush(_))
        }
    }

    pub(super) fn fill_controls(&mut self, ui: &mut egui::Ui) {
        let (rectangle, brush) = if self.terrain_mode == TerrainMode::IceFill {
            (tr!("框选铺冰"), tr!("涂抹铺冰"))
        } else {
            (tr!("框选铺地"), tr!("涂抹铺地"))
        };
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.fill_style, FillStyle::Rectangle, rectangle);
            ui.selectable_value(&mut self.fill_style, FillStyle::Brush, brush);
        });
        if self.fill_style == FillStyle::Brush {
            ui.add(
                egui::Slider::new(&mut self.fill_brush_size, 1..=MAX_BRUSH_SIZE)
                    .text(tr!("笔刷宽度（格）")),
            )
            .on_hover_text(tr!("笔刷宽度和长度相同；按住左键自由涂抹，松手提交。"));
        }
    }

    pub(crate) fn sync_ice_tool(&mut self, scene: &MapScene) {
        if self.terrain_mode != TerrainMode::IceFill || scene.palette.bundled_ice_sets.is_some() {
            return;
        }
        if self.tool == Tool::Terrain {
            self.cancel_active(scene);
            self.activate_tool(Tool::Select);
            self.status = tr!("当前地图未加载 VERA 可破裂冰面素材").into();
        }
        self.terrain_mode = TerrainMode::Template;
        self.ui_state.terrain_subtiles = false;
    }

    fn fill_material(&self) -> Material {
        if self.terrain_mode == TerrainMode::IceFill {
            Material::Ice(ice_tools::Options {
                mark_growth: self.ice_fill_growth,
                soften_shore: self.ice_soften_shore,
            })
        } else {
            Material::Ground {
                brush: self.brush,
                lat: self.auto_lat,
            }
        }
    }

    pub(super) fn fill_active_or_requested(&self) -> bool {
        self.fill.gesture.is_some()
            || self.tool == Tool::Terrain
                && (self.terrain_mode.uses_fill_gesture()
                    || !self.stroking
                        && matches!(
                            self.terrain_mode,
                            TerrainMode::Template | TerrainMode::Subtile
                        )
                        && self.brush_ctrl
                        && !self.brush_shift)
    }
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
        if !self.fill_active_or_requested() {
            return false;
        }
        if self.terrain_mode == TerrainMode::IceFill && scene.palette.bundled_ice_sets.is_none() {
            self.sync_ice_tool(scene);
            return true;
        }
        if press && let Some(at) = at {
            let shape = self.fill_shape();
            if self.terrain_mode == TerrainMode::Fill {
                self.prepare_fill_material(scene);
            }
            self.fill.cancel();
            self.feedback.clear();
            self.fill.gesture = Some(Gesture {
                start: at,
                last: at,
                material: self.fill_material(),
                shape,
                previous: None,
                brushed: BTreeSet::new(),
            });
            self.stroking = true;
        }
        if let Some(g) = self.fill.gesture.as_mut() {
            if down || press || release {
                g.sample(self.doc.as_ref().unwrap(), at);
            }
            if release || !down {
                let ice = matches!(g.material, Material::Ice(_));
                let shape = g.shape;
                let result = self.fill.generate(self.doc.as_ref().unwrap(), scene);
                self.fill.cancel();
                self.stroking = false;
                match result {
                    Ok(plan) => match plan.apply(self.doc.as_mut().unwrap()) {
                        Ok(cells) => {
                            self.feedback = cells.into_iter().collect();
                            self.commit(
                                scene,
                                if ice {
                                    tr!("铺设可破裂冰面")
                                } else if matches!(shape, Shape::Brush(_)) {
                                    tr!("涂抹铺地")
                                } else {
                                    tr!("框选铺地")
                                },
                            );
                            self.status = if ice {
                                tr!(
                                    "冰面铺设 · {arg0} 格修改 · 跳过 {arg1} 格",
                                    arg0 = self.feedback.len(),
                                    arg1 = plan.skipped.len()
                                )
                            } else {
                                tr!(
                                    "地表填充 · {arg0} 格修改 · 保留 {arg1} 格结构地形",
                                    arg0 = self.feedback.len(),
                                    arg1 = plan.skipped.len()
                                )
                            };
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
        if self.terrain_mode == TerrainMode::IceFill && scene.palette.bundled_ice_sets.is_none() {
            return;
        }
        let Some(doc) = self.doc.as_ref() else { return };
        let at = self.fill.gesture.as_ref().map(|g| g.last).or(at);
        let Some(at) = at else { return };
        let start = self.fill.gesture.as_ref().map_or(at, |g| g.start);
        let shape = self
            .fill
            .gesture
            .as_ref()
            .map_or_else(|| self.fill_shape(), |g| g.shape);
        let result = if self.fill.gesture.is_some() {
            self.fill.generate(doc, scene)
        } else {
            let material = self.fill_material();
            FillEdit::plan(
                &mut self.fill.preview,
                doc,
                scene,
                Key {
                    document: doc.id,
                    state: doc.edit_revision(),
                    palette: Arc::as_ptr(&scene.palette) as usize,
                    start: at,
                    end: at,
                    material,
                    shape,
                    brushed_len: None,
                },
                None,
            )
        };
        let world = camera.world_rect(rect);
        let outline = |cell: Cell, level: u8, color: Color32| {
            let (x, y) = vera20k_core::map::terrain::iso_to_screen(
                cell[0],
                cell[1],
                if self.flat_view { 0 } else { level },
            );
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
                    if self.framework {
                        self.ui_state.framework_textures.draw(
                            painter,
                            scene,
                            crate::framework_view::View {
                                rect,
                                camera,
                                flat: self.flat_view,
                            },
                            cell,
                            *v,
                            Color32::from_white_alpha(210),
                        );
                    }
                    let color = if plan.cells.contains(&cell) {
                        Color32::LIGHT_GREEN
                    } else {
                        Color32::LIGHT_BLUE
                    };
                    let origin = egui::pos2(
                        (f32::from(cell[0]) - f32::from(cell[1])) * 30.,
                        f32::from(cell[0] + cell[1]) * 15. - f32::from(v.level) * 15.,
                    );
                    if !self.flat_view
                        && !self.framework
                        && let Some(&tile) = scene.palette.raw_ids.get(&v.tile)
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
                let dimensions = if let Shape::Brush(size) = shape {
                    format!("{size}×{size}")
                } else {
                    format!(
                        "{}×{}",
                        start[0].abs_diff(at[0]) + 1,
                        start[1].abs_diff(at[1]) + 1
                    )
                };
                let message = if self.terrain_mode == TerrainMode::IceFill {
                    tr!(
                        "{dimensions} · 铺冰 {arg0} 格 · 保留 {arg1} 格；松手应用 / Esc 取消",
                        dimensions = dimensions,
                        arg0 = plan.cells.len(),
                        arg1 = plan.skipped.len()
                    )
                } else {
                    tr!(
                        "{dimensions} · 铺地 {arg0} 格 · 保留 {arg1} 格；松手应用 / Esc 取消",
                        dimensions = dimensions,
                        arg0 = plan.cells.len(),
                        arg1 = plan.skipped.len()
                    )
                };
                (
                    message,
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
