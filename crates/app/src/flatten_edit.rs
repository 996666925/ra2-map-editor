//! Sample-on-press flatten gestures with local brush reachability and a cached
//! preview. The document changes only when the whole gesture is committed.
use super::*;
use crate::i18n::tr;
use ra2_map_scene::{cliff_tools::PlateauPlan, flatten_tools};

#[derive(Default)]
pub(super) struct FlattenEdit {
    gesture: Option<Gesture>,
    preview: Option<Preview>,
}
struct Gesture {
    last: Option<Cell>,
    cells: BTreeSet<Cell>,
    segment: Option<BTreeSet<Cell>>,
    size: u16,
    level: u8,
    smooth: bool,
    lat: bool,
    options: flatten_tools::Options,
}
#[derive(PartialEq)]
struct Key {
    document: u64,
    revision: u64,
    palette: usize,
    cells: BTreeSet<Cell>,
    level: u8,
    smooth: bool,
    lat: bool,
    options: flatten_tools::Options,
}
struct Preview {
    key: Key,
    result: std::result::Result<Arc<PlateauPlan>, String>,
}
impl FlattenEdit {
    pub fn cancel(&mut self) {
        *self = Self::default();
    }
    pub fn active(&self) -> bool {
        self.gesture.is_some()
    }
    pub fn finish_segment(&mut self) {
        if let Some(g) = &mut self.gesture {
            g.segment = None;
        }
    }
    fn plan(
        &mut self,
        doc: &MapDocument,
        scene: &MapScene,
    ) -> std::result::Result<Arc<PlateauPlan>, String> {
        let g = self.gesture.as_ref().ok_or(tr!("按下并拖动刷平地表"))?;
        let key = Key {
            document: doc.id,
            revision: doc.edit_revision(),
            palette: Arc::as_ptr(&scene.palette) as usize,
            cells: g.cells.clone(),
            level: g.level,
            smooth: g.smooth,
            lat: g.lat,
            options: g.options,
        };
        if self.preview.as_ref().is_none_or(|p| p.key != key) {
            let result = (|| -> Result<Arc<PlateauPlan>> {
                let mut copy = doc.fork();
                let initial = flatten_tools::plan_with_options(
                    doc,
                    scene,
                    &key.cells,
                    key.level,
                    key.smooth,
                    key.options,
                )?;
                let mut changed: BTreeSet<_> = initial.apply(&mut copy)?.into_iter().collect();
                if key.lat && !changed.is_empty() {
                    changed.extend(terrain_tools::recalculate(&mut copy, scene, &changed)?);
                }
                let edits = changed
                    .into_iter()
                    .filter_map(|at| {
                        let v = terrain_tools::value(&copy, scene, at);
                        (v != terrain_tools::value(doc, scene, at)).then_some((at, v))
                    })
                    .collect();
                Ok(Arc::new(PlateauPlan {
                    edits,
                    overlays: BTreeMap::new(),
                    cliffs: vec![],
                    interior: key.cells.clone(),
                    base: key.level,
                }))
            })()
            .map_err(|e| format!("{e:#}"));
            self.preview = Some(Preview { key, result });
        }
        self.preview.as_ref().unwrap().result.clone()
    }
}
impl Editor {
    /// The brush follows the ground image under the cursor while a raised or
    /// lowered preview is being drawn over the committed terrain.
    pub(crate) fn flatten_hover(&self, scene: &MapScene, world: egui::Pos2) -> Option<Cell> {
        if !self.flatten.active() {
            return None;
        }
        let plan = self.flatten.preview.as_ref()?.result.as_ref().ok()?;
        let mut cells: Vec<_> = plan.edits.iter().collect();
        cells.sort_by_key(|(at, _)| (u32::from(at[0]) + u32::from(at[1]), at[0]));
        for (&at, v) in cells.into_iter().rev() {
            let tile = *scene.palette.raw_ids.get(&v.tile)?;
            let key = vera20k_core::map::theater::TileKey {
                tile_id: tile,
                sub_tile: v.sub,
                variant: 0,
            };
            let images = if self.framework {
                &scene.palette.framework_images
            } else {
                &scene.palette.images
            };
            let Some(im) = images.get(&key) else { continue };
            let (x, y) = vera20k_core::map::terrain::iso_to_screen(at[0], at[1], v.level);
            let px = (world.x - x - im.info.offset[0] as f32).floor() as i32;
            let py = (world.y - y - im.info.offset[1] as f32).floor() as i32;
            if px >= 0
                && py >= 0
                && px < im.info.width as i32
                && py < im.info.height as i32
                && im.rgba[(py as usize * im.info.width as usize + px as usize) * 4 + 3] != 0
            {
                return Some(at);
            }
        }
        None
    }

    pub(super) fn flatten_pointer(
        &mut self,
        scene: &MapScene,
        at: Option<Cell>,
        press: bool,
        down: bool,
        release: bool,
    ) -> bool {
        if !self.flatten.active()
            && !(self.tool == Tool::Terrain && self.terrain_mode == TerrainMode::Flatten)
        {
            return false;
        }
        if press && let Some(at) = at {
            let doc = self.doc.as_ref().unwrap();
            let force = self.smooth_heights && self.brush_ctrl && self.brush_shift;
            let options = flatten_tools::Options {
                steep: self.brush_shift && !force,
                vertices: self.height_vertices && self.smooth_heights,
                force,
                ..Default::default()
            };
            let level = if self.flatten_fixed {
                self.flatten_level
            } else if options.vertices {
                flatten_tools::vertex_height(doc, scene, at, force)
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| terrain_tools::value(doc, scene, at).level)
            } else {
                terrain_tools::value(doc, scene, at).level
            };
            self.flatten_level = level;
            self.stroke = None;
            self.brush_segment = None;
            self.flatten.cancel();
            self.flatten.gesture = Some(Gesture {
                last: None,
                cells: BTreeSet::new(),
                segment: None,
                size: self.radius,
                level,
                smooth: self.smooth_heights,
                lat: self.auto_lat,
                options,
            });
            self.feedback.clear();
            self.stroking = true;
        }
        if let Some(g) = &mut self.flatten.gesture {
            if let Some(origin) = self.brush_direction.line_origin() {
                let saved = g.segment.get_or_insert_with(|| g.cells.clone());
                if at.is_some() && g.last != at {
                    g.cells = saved.clone();
                    g.last = Some(origin);
                }
            } else {
                g.segment = None;
            }
            if (press || down || release)
                && let Some(at) = at
            {
                let doc = self.doc.as_ref().unwrap();
                for center in cell_line(g.last.unwrap_or(at), at) {
                    match flatten_tools::window_with_force(
                        doc,
                        scene,
                        center,
                        g.size,
                        g.level,
                        g.options.force,
                    ) {
                        Ok(cells) => g.cells.extend(cells),
                        Err(e) => {
                            self.abort_stroke(scene, e);
                            return true;
                        }
                    }
                }
                g.last = Some(at);
            } else if down {
                g.last = None;
            }
            let result = self.flatten.plan(self.doc.as_ref().unwrap(), scene);
            if let Ok(plan) = &result {
                self.feedback = plan.edits.keys().copied().collect();
            }
            if release || !down {
                self.flatten.cancel();
                self.stroking = false;
                match result {
                    Ok(plan) => match plan.apply(self.doc.as_mut().unwrap()) {
                        Ok(cells) => {
                            self.feedback = cells.into_iter().collect();
                            self.commit(scene, TerrainMode::Flatten.name());
                            self.status = tr!(
                                "平整至 {arg0} 层 · {arg1} 格",
                                arg0 = plan.base,
                                arg1 = self.feedback.len()
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

    pub(super) fn flatten_preview(
        &mut self,
        painter: &egui::Painter,
        scene: &MapScene,
        rect: egui::Rect,
        camera: &crate::camera::MapCamera,
    ) {
        let result = self.flatten.plan(self.doc.as_ref().unwrap(), scene);
        match result {
            Ok(plan) => {
                let world = camera.world_rect(rect);
                let mut cells: Vec<_> = plan.edits.iter().collect();
                cells.sort_by_key(|(at, _)| (u32::from(at[0]) + u32::from(at[1]), at[0]));
                for (&at, &v) in cells {
                    if self.framework {
                        self.ui_state.framework_textures.draw(
                            painter,
                            scene,
                            crate::framework_view::View {
                                rect,
                                camera,
                                flat: self.flat_view,
                            },
                            at,
                            v,
                            Color32::from_white_alpha(210),
                        );
                    } else if !self.flat_view
                        && let Some(&tile) = scene.palette.raw_ids.get(&v.tile)
                    {
                        let key = vera20k_core::map::theater::TileKey {
                            tile_id: tile,
                            sub_tile: v.sub,
                            variant: 0,
                        };
                        if let Some(im) = scene.palette.images.get(&key) {
                            let (x, y) =
                                vera20k_core::map::terrain::iso_to_screen(at[0], at[1], v.level);
                            let origin = egui::pos2(
                                x + im.info.offset[0] as f32,
                                y + im.info.offset[1] as f32,
                            );
                            let target = egui::Rect::from_min_size(
                                rect.min + (origin - world.min) * camera.zoom,
                                egui::vec2(im.info.width as f32, im.info.height as f32)
                                    * camera.zoom,
                            );
                            if target.intersects(rect)
                                && let Some(texture) = terrain_ui::subtile_texture(
                                    &mut self.ui_state,
                                    painter.ctx(),
                                    scene,
                                    key,
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
                    }
                    let (x, y) = vera20k_core::map::terrain::iso_to_screen(
                        at[0],
                        at[1],
                        if self.flat_view { 0 } else { v.level },
                    );
                    let center =
                        rect.min + (egui::pos2(x + 30., y + 15.) - world.min) * camera.zoom;
                    if !rect.expand(60. * camera.zoom).contains(center) {
                        continue;
                    }
                    let points = [[0., -15.], [30., 0.], [0., 15.], [-30., 0.]]
                        .map(|[x, y]| center + egui::vec2(x, y) * camera.zoom);
                    painter.add(egui::Shape::convex_polygon(
                        points.to_vec(),
                        Color32::from_rgba_unmultiplied(80, 220, 170, 25),
                        egui::Stroke::new(1., Color32::LIGHT_GREEN),
                    ));
                    if self.flat_view && camera.zoom >= 0.65 {
                        painter.text(
                            center,
                            egui::Align2::CENTER_CENTER,
                            v.level.to_string(),
                            egui::FontId::monospace(11.),
                            Color32::LIGHT_GREEN,
                        );
                    }
                }
                painter.text(
                    rect.left_top() + egui::vec2(12., 12.),
                    egui::Align2::LEFT_TOP,
                    tr!(
                        "刷平至 {arg0} 层 · {arg1} 格；松手提交，Esc 取消",
                        arg0 = plan.base,
                        arg1 = plan.edits.len()
                    ),
                    egui::FontId::proportional(13.),
                    Color32::LIGHT_GREEN,
                );
            }
            Err(e) => {
                painter.text(
                    rect.left_top() + egui::vec2(12., 12.),
                    egui::Align2::LEFT_TOP,
                    e,
                    egui::FontId::proportional(13.),
                    Color32::LIGHT_RED,
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "flatten_edit_tests.rs"]
mod tests;
