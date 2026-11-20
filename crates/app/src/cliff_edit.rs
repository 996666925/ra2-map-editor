//! Deferred plateau gestures and cached, side-effect-free generation previews.
use super::*;
use crate::i18n::tr;
use ra2_map_scene::cliff_authoring;
use ra2_map_scene::cliff_tools::{self, PlateauPlan};

fn click_polyline(mode: TerrainMode) -> bool {
    matches!(
        mode,
        TerrainMode::CliffPolyline | TerrainMode::WaterPolyline
    )
}

#[derive(Default)]
pub(super) struct PlateauEdit {
    gesture: Option<Gesture>,
    preview: Option<Preview>,
}
struct Gesture {
    mode: TerrainMode,
    start: Cell,
    path: Vec<Cell>,
    base: u8,
    high_left: bool,
    bridge: ra2_map_scene::bridge_tools::BridgeKind,
}
#[derive(PartialEq)]
struct Key {
    document: u64,
    palette: usize,
    state: u64,
    mode: TerrainMode,
    path: Vec<Cell>,
    base: u8,
    high_left: bool,
    bridge: ra2_map_scene::bridge_tools::BridgeKind,
}
struct Preview {
    key: Key,
    result: std::result::Result<Arc<PlateauPlan>, String>,
    bridge_draft: Option<Arc<PlateauPlan>>,
}
impl PlateauEdit {
    pub fn cancel(&mut self) {
        *self = Self::default();
    }
    fn generate(
        &mut self,
        doc: &MapDocument,
        scene: &MapScene,
        at: Cell,
    ) -> std::result::Result<Arc<PlateauPlan>, String> {
        let g = self.gesture.as_ref().ok_or(tr!("按下并拖动绘制高台"))?;
        let mut path = g.path.clone();
        // Water previews show the exact closure Enter will commit. Hover
        // only draws a guide for the next edge until that point is clicked.
        if g.mode != TerrainMode::WaterPolyline && path.last() != Some(&at) {
            path.push(at);
        }
        if !matches!(
            g.mode,
            TerrainMode::CliffOutline
                | TerrainMode::CliffPolyline
                | TerrainMode::WaterOutline
                | TerrainMode::WaterPolyline
        ) {
            path = vec![
                g.start,
                if g.mode == TerrainMode::HeightFill {
                    g.start
                } else {
                    at
                },
            ];
        }
        let key = Key {
            document: doc.id,
            palette: Arc::as_ptr(&scene.palette) as usize,
            state: doc.edit_revision(),
            mode: g.mode,
            path,
            base: g.base,
            high_left: g.high_left,
            bridge: g.bridge,
        };
        if self.preview.as_ref().is_none_or(|p| p.key != key) {
            let result = (|| -> Result<Arc<PlateauPlan>> {
                if key.mode == TerrainMode::CliffPolyline {
                    return Ok(Arc::new(cliff_authoring::polyline(
                        doc,
                        scene,
                        &key.path,
                        key.base,
                        key.high_left,
                    )?));
                }
                if key.mode == TerrainMode::CliffHeightRepair {
                    return Ok(Arc::new(ra2_map_scene::height_tools::repair(
                        doc,
                        scene,
                        &cliff_tools::rectangle(g.start, at),
                        g.base,
                    )?));
                }
                if key.mode == TerrainMode::HeightFill {
                    return Ok(Arc::new(ra2_map_scene::height_tools::fill(
                        doc, scene, g.start, g.base,
                    )?));
                }
                if matches!(key.mode, TerrainMode::Road | TerrainMode::RoadCut) {
                    return Ok(Arc::new(ra2_map_scene::road_tools::plan(
                        doc,
                        scene,
                        g.start,
                        at,
                        key.mode == TerrainMode::RoadCut,
                    )?));
                }
                if key.mode.joining() {
                    if key.mode == TerrainMode::WaterPolyline {
                        return Ok(Arc::new(ra2_map_scene::shore_tools::polyline(
                            doc, scene, &key.path,
                        )?));
                    }
                    if key.mode == TerrainMode::WaterOutline {
                        return Ok(Arc::new(ra2_map_scene::shore_tools::outline(
                            doc, scene, &key.path,
                        )?));
                    }
                    let cells = cliff_tools::rectangle(g.start, at);
                    if key.mode == TerrainMode::CoastalCliff {
                        return Ok(Arc::new(ra2_map_scene::coastal_cliffs::plan(
                            doc, scene, cells,
                        )?));
                    }
                    let paint = match key.mode {
                        TerrainMode::Water => Some(true),
                        TerrainMode::LandFill => Some(false),
                        _ => None,
                    };
                    return Ok(Arc::new(ra2_map_scene::shore_tools::plan(
                        doc, scene, cells, paint,
                    )?));
                }
                if key.mode == TerrainMode::Bridge {
                    return Ok(Arc::new(ra2_map_scene::bridge_tools::plan(
                        doc, scene, g.start, at, key.bridge,
                    )?));
                }
                if matches!(key.mode, TerrainMode::CliffFront | TerrainMode::CliffBack) {
                    return Ok(Arc::new(cliff_authoring::line(
                        doc,
                        scene,
                        g.start,
                        at,
                        g.base,
                        key.mode == TerrainMode::CliffBack,
                    )?));
                }
                if key.mode == TerrainMode::RampEntrance {
                    return Ok(Arc::new(cliff_authoring::ramp(
                        doc, scene, g.start, at, g.base,
                    )?));
                }
                let cells = if key.mode != TerrainMode::CliffOutline {
                    cliff_tools::rectangle(g.start, at)
                } else {
                    cliff_tools::enclose(&key.path)?
                };
                Ok(Arc::new(cliff_authoring::reshape(
                    doc,
                    scene,
                    cells,
                    g.base,
                    key.mode == TerrainMode::PlateauCut,
                )?))
            })()
            .map_err(|e| format!("{e:#}"));
            let bridge_draft =
                if result.is_err() && key.mode == TerrainMode::Bridge && !key.bridge.high() {
                    ra2_map_scene::bridge_tools::preview_low(doc, scene, g.start, at, key.bridge)
                        .ok()
                        .map(Arc::new)
                } else {
                    None
                };
            self.preview = Some(Preview {
                key,
                result,
                bridge_draft,
            });
        }
        self.preview.as_ref().unwrap().result.clone()
    }
}
impl Editor {
    pub(super) fn polyline_shortcuts(&mut self, ctx: &egui::Context, scene: &MapScene) {
        if !self
            .plateau
            .gesture
            .as_ref()
            .is_some_and(|g| click_polyline(g.mode))
        {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            self.finish_plateau(scene);
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)) {
            let g = self.plateau.gesture.as_mut().unwrap();
            if g.path.len() > 1 {
                g.path.pop();
                self.plateau.preview = None;
            } else {
                self.cancel_active(scene);
            }
        }
    }

    fn finish_plateau(&mut self, scene: &MapScene) {
        let Some(g) = self.plateau.gesture.as_ref() else {
            return;
        };
        let last = *g.path.last().unwrap();
        let mode = g.mode;
        let result = self
            .plateau
            .generate(self.doc.as_ref().unwrap(), scene, last);
        // Keep an invalid polyline editable so its last vertex can be removed.
        if click_polyline(mode)
            && let Err(ref e) = result
        {
            self.status = crate::i18n::text(e).to_owned();
            return;
        }
        self.plateau.cancel();
        self.stroking = false;
        match result {
            Ok(plan) => match plan.apply(self.doc.as_mut().unwrap()) {
                Ok(cells) => {
                    self.feedback = cells.into_iter().collect();
                    self.commit(scene, mode.name());
                    self.status = tr!(
                        "{arg0} · {arg1} 格 · {arg2} 个地形模板 · {arg3} 个桥面格",
                        arg0 = mode.name(),
                        arg1 = self.feedback.len(),
                        arg2 = plan.cliffs.len(),
                        arg3 = plan.overlays.len()
                    );
                }
                Err(e) => self.abort_stroke(scene, e),
            },
            Err(e) => self.abort_stroke(scene, anyhow::anyhow!(e)),
        }
    }

    pub(crate) fn plateau_preview_plan(
        &mut self,
        scene: &MapScene,
        at: Cell,
    ) -> Result<Arc<PlateauPlan>> {
        self.plateau
            .generate(self.doc.as_ref().context(tr!("没有地图文档"))?, scene, at)
            .map_err(anyhow::Error::msg)
    }

    pub(super) fn plateau_pointer(
        &mut self,
        scene: &MapScene,
        at: Option<Cell>,
        press: bool,
        down: bool,
        release: bool,
    ) -> bool {
        if self.plateau.gesture.is_none()
            && !(self.tool == Tool::Terrain && self.terrain_mode.planned())
        {
            return false;
        }
        if self
            .plateau
            .gesture
            .as_ref()
            .is_some_and(|g| click_polyline(g.mode))
        {
            if press && let Some(at) = at {
                let g = self.plateau.gesture.as_mut().unwrap();
                if g.mode == TerrainMode::WaterPolyline && at == g.start {
                    // The planner already closes the path. Keep the stored
                    // vertices editable if the attempted closure is invalid.
                    if g.path.len() > 1 {
                        self.finish_plateau(scene);
                    }
                } else if g.path.last() != Some(&at) {
                    g.path.push(at);
                    if at == g.start && g.path.len() >= 4 {
                        self.finish_plateau(scene);
                    }
                }
            }
            return true;
        }
        if press && let Some(at) = at {
            let Some(doc) = self.doc.as_ref() else {
                return true;
            };
            self.plateau.cancel();
            self.feedback.clear();
            self.stroking = true;
            self.plateau.gesture = Some(Gesture {
                mode: self.terrain_mode,
                start: at,
                path: vec![at],
                high_left: self.cliff_high_left,
                bridge: self.bridge_kind,
                base: if self.terrain_mode == TerrainMode::HeightFill {
                    self.flatten_level
                } else if self.terrain_mode == TerrainMode::CliffHeightRepair {
                    self.cliff_repair_base
                } else {
                    self.template_level.unwrap_or_else(|| {
                        if self.terrain_mode.joining()
                            || self.terrain_mode == TerrainMode::CliffPolyline
                        {
                            terrain_tools::value(doc, scene, at).level
                        } else if self.terrain_mode == TerrainMode::Bridge {
                            terrain_tools::value(doc, scene, at)
                                .level
                                .saturating_sub(if self.bridge_kind.high() { 4 } else { 0 })
                        } else {
                            cliff_authoring::infer_base(doc, scene, at)
                        }
                    })
                },
            });
            if click_polyline(self.terrain_mode) {
                return true;
            }
        }
        if let Some(g) = self.plateau.gesture.as_mut() {
            if (down || press || release)
                && let Some(at) = at
                && g.path.last() != Some(&at)
            {
                g.path.push(at);
            }
            if release || !down {
                self.finish_plateau(scene);
            }
        }
        true
    }

    pub(super) fn plateau_preview(
        &mut self,
        painter: &egui::Painter,
        scene: &MapScene,
        rect: egui::Rect,
        camera: &crate::camera::MapCamera,
        at: Cell,
    ) {
        let Some(doc) = self.doc.as_ref() else {
            return;
        };
        if self.plateau.gesture.is_none() {
            painter.text(
                rect.left_top() + egui::vec2(12., 12.),
                egui::Align2::LEFT_TOP,
                if self.picking_height {
                    tr!("点击崖顶或地面拾取高度；此次点击不会修改地图。")
                } else if self.terrain_mode == TerrainMode::CliffPolyline {
                    tr!("点击添加折点；点击起点闭合并统一内部高度；Enter 生成已确认折线，Backspace 撤回折点，Esc 取消。")
                } else if self.terrain_mode == TerrainMode::WaterPolyline {
                    tr!("点击添加折点，点击起点或按 Enter 闭合填水。Backspace 撤回折点，Esc 取消。")
                } else {
                    crate::i18n::text(self.terrain_mode.name())
                },
                egui::FontId::proportional(13.),
                Color32::LIGHT_GREEN,
            );
            return;
        }
        let world = camera.world_rect(rect);
        let result = self.plateau.generate(doc, scene, at);
        let error = result
            .as_ref()
            .err()
            .map(|e| crate::i18n::text(e).to_owned());
        let plan = result
            .ok()
            .or_else(|| self.plateau.preview.as_ref()?.bridge_draft.clone());
        let color = if error.is_none() {
            Color32::from_rgb(80, 220, 170)
        } else {
            Color32::LIGHT_RED
        };
        let outline = |painter: &egui::Painter, at: Cell, level: u8| {
            let (x, y) = vera20k_core::map::terrain::iso_to_screen(
                at[0],
                at[1],
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
                if self.flat_view && !self.framework {
                    crate::flat_view::height_color(level)
                } else {
                    color.gamma_multiply(0.10)
                },
                egui::Stroke::new(1., color),
            ));
            if self.flat_view && camera.zoom >= 0.65 {
                painter.text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    (level as i8).to_string(),
                    egui::FontId::monospace(11.),
                    color,
                );
            }
        };
        match plan {
            Some(plan) => {
                if self.framework {
                    let mut cells: Vec<_> = plan.edits.iter().collect();
                    cells.sort_by_key(|(at, _)| (u32::from(at[0]) + u32::from(at[1]), at[0]));
                    for (&at, &value) in cells {
                        self.ui_state.framework_textures.draw(
                            painter,
                            scene,
                            crate::framework_view::View {
                                rect,
                                camera,
                                flat: self.flat_view,
                            },
                            at,
                            value,
                            Color32::from_white_alpha(210),
                        );
                    }
                }
                // Filled water and restored ground are part of the ghost too.
                let stamped: BTreeSet<_> = plan.cliffs.iter().map(|s| (s.tile, s.origin)).collect();
                let mut preview_cells: Vec<_> = plan.edits.iter().collect();
                preview_cells.sort_by_key(|(at, _)| (u32::from(at[0]) + u32::from(at[1]), at[0]));
                for (&at, v) in preview_cells {
                    if self.flat_view || self.framework {
                        continue;
                    }
                    let Some(&tile) = scene.palette.raw_ids.get(&v.tile) else {
                        continue;
                    };
                    if matches!(
                        self.plateau.gesture.as_ref().unwrap().mode,
                        TerrainMode::HeightFill | TerrainMode::CliffHeightRepair
                    ) {
                        let key = vera20k_core::map::theater::TileKey {
                            tile_id: tile,
                            sub_tile: v.sub,
                            variant: 0,
                        };
                        let Some(image) = scene.palette.images.get(&key) else {
                            continue;
                        };
                        let (x, y) =
                            vera20k_core::map::terrain::iso_to_screen(at[0], at[1], v.level);
                        let origin = egui::pos2(
                            x + image.info.offset[0] as f32,
                            y + image.info.offset[1] as f32,
                        );
                        let target = egui::Rect::from_min_size(
                            rect.min + (origin - world.min) * camera.zoom,
                            egui::vec2(image.info.width as f32, image.info.height as f32)
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
                        continue;
                    }
                    let Some(t) = scene.palette.templates.get(&(tile, 0)) else {
                        continue;
                    };
                    if t.width != 1 || t.height != 1 || stamped.contains(&(tile, at)) {
                        continue;
                    }
                    let Some(image) = scene.palette.previews.get(&(tile, 0)) else {
                        continue;
                    };
                    let origin =
                        egui::pos2(
                            (f32::from(at[0]) - f32::from(at[1])) * 30.,
                            f32::from(at[0] + at[1]) * 15. - f32::from(v.level) * 15.,
                        ) + egui::vec2(image.info.offset[0] as f32, image.info.offset[1] as f32);
                    let target = egui::Rect::from_min_size(
                        rect.min + (origin - world.min) * camera.zoom,
                        egui::vec2(image.info.width as f32, image.info.height as f32) * camera.zoom,
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
                for (&at, v) in &plan.edits {
                    outline(painter, at, v.level);
                }
                let mut stamps: Vec<_> = plan.cliffs.iter().collect();
                stamps.sort_by_key(|s| {
                    (u32::from(s.origin[0]) + u32::from(s.origin[1]), s.origin[0])
                });
                for stamp in stamps {
                    if self.flat_view || self.framework {
                        continue;
                    }
                    let key = vera20k_core::map::theater::TileKey {
                        tile_id: stamp.tile,
                        sub_tile: 0,
                        variant: 0,
                    };
                    if let Some(texture) =
                        terrain_ui::template_texture(&mut self.ui_state, painter.ctx(), scene, key)
                    {
                        let image = &scene.palette.previews[&(stamp.tile, 0)];
                        let origin = egui::pos2(
                            (f32::from(stamp.origin[0]) - f32::from(stamp.origin[1])) * 30.,
                            f32::from(stamp.origin[0] + stamp.origin[1]) * 15.
                                - f32::from(stamp.base) * 15.,
                        ) + egui::vec2(
                            image.info.offset[0] as f32,
                            image.info.offset[1] as f32,
                        );
                        let target = egui::Rect::from_min_size(
                            rect.min + (origin - world.min) * camera.zoom,
                            egui::vec2(image.info.width as f32, image.info.height as f32)
                                * camera.zoom,
                        );
                        if target.intersects(rect) {
                            painter.image(
                                texture.id(),
                                target,
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                                Color32::from_white_alpha(180),
                            );
                        }
                    }
                }
                let mut overlays: Vec<_> = plan.overlays.iter().collect();
                overlays.sort_by_key(|(at, _)| (u32::from(at[0]) + u32::from(at[1]), at[0]));
                for (&at, &(id, frame)) in overlays {
                    let high = vera20k_core::rules::overlay_types::is_high_bridge_index(id);
                    let level = if error.is_some() {
                        plan.base
                    } else {
                        plan.edits
                            .get(&at)
                            .copied()
                            .unwrap_or_else(|| terrain_tools::value(doc, scene, at))
                            .level
                    };
                    outline(painter, at, level + if high { 4 } else { 0 });
                    if self.flat_view {
                        continue;
                    }
                    if let Some(name) = scene.catalog.overlays.get(usize::from(id))
                        && let Some(im) = scene.sprites.overlay_frames.get(&(name.clone(), frame))
                    {
                        let (x, y) = vera20k_core::map::terrain::iso_to_screen(at[0], at[1], level);
                        let bias = if high {
                            if frame >= 9 { -31. } else { -16. }
                        } else {
                            0.
                        };
                        let position =
                            ra2_map_scene::visual::sprite_position([x + 30., y + 15. + bias], im);
                        let target = egui::Rect::from_min_size(
                            rect.min + (position - world.min) * camera.zoom,
                            egui::vec2(im.info.width as f32, im.info.height as f32) * camera.zoom,
                        );
                        if target.intersects(rect) {
                            let texture = editing_ui::map_sprite_texture(
                                &mut self.ui_state.map_sprites,
                                painter.ctx(),
                                im,
                            );
                            painter.image(
                                texture,
                                target,
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                                if error.is_some() {
                                    Color32::from_rgba_unmultiplied(255, 160, 160, 180)
                                } else {
                                    Color32::from_white_alpha(180)
                                },
                            );
                        }
                    }
                }
                let g = self.plateau.gesture.as_ref().unwrap();
                let heights = if g.mode == TerrainMode::CliffHeightRepair {
                    tr!("低地基准 {arg0} 层；保留原悬崖子块", arg0 = plan.base)
                } else if g.mode == TerrainMode::HeightFill {
                    tr!("目标高度 {arg0} 层；保留原悬崖", arg0 = plan.base)
                } else if g.mode == TerrainMode::CliffPolyline && plan.base < g.base {
                    tr!(
                        "坑：外侧 {arg0} / 坑底 {arg1} 层",
                        arg0 = g.base,
                        arg1 = plan.base
                    )
                } else if g.mode == TerrainMode::CliffPolyline && !plan.interior.is_empty() {
                    tr!(
                        "高台：外侧 {arg0} / 内部 {arg1} 层",
                        arg0 = g.base,
                        arg1 = plan.base + 4
                    )
                } else if g.mode.joining() {
                    tr!("按原高度连接地形").into()
                } else if g.mode == TerrainMode::Bridge && !g.bridge.high() {
                    tr!("桥面与地面 {arg0} 层", arg0 = plan.base)
                } else {
                    tr!(
                        "低地 {arg0} / 高地 {arg1} 层",
                        arg0 = plan.base,
                        arg1 = plan.base + 4
                    )
                };
                painter.text(
                    rect.left_top() + egui::vec2(12., 12.),
                    egui::Align2::LEFT_TOP,
                    error
                        .as_ref()
                        .map(|e| {
                            tr!(
                                "预览 {arg0} 排桥面 · {e}；红色时松开不会放置，Esc 取消",
                                arg0 = plan.overlays.len() / 3,
                                e = e
                            )
                        })
                        .unwrap_or_else(|| {
                            if g.mode == TerrainMode::CliffPolyline {
                                format!("{heights} · {}", tr!("点击添加折点；点击起点闭合并统一内部高度；Enter 生成已确认折线，Backspace 撤回折点，Esc 取消。"))
                            } else if g.mode == TerrainMode::WaterPolyline {
                                tr!("点击添加折点，点击起点或按 Enter 闭合填水。Backspace 撤回折点，Esc 取消。").into()
                            } else { tr!(
                                "{arg0} 格修改 · {arg1} 个地形模板 · {arg2}；松开放置，Esc 取消",
                                arg0 = plan.edits.len() + plan.overlays.len(),
                                arg1 = plan.cliffs.len(),
                                arg2 = heights
                            ) }
                        }),
                    egui::FontId::proportional(13.),
                    if error.is_some() {
                        Color32::LIGHT_RED
                    } else {
                        Color32::LIGHT_GREEN
                    },
                );
            }
            None => {
                let g = self.plateau.gesture.as_ref().unwrap();
                if matches!(
                    g.mode,
                    TerrainMode::Plateau
                        | TerrainMode::CliffHeightRepair
                        | TerrainMode::PlateauCut
                        | TerrainMode::Water
                        | TerrainMode::LandFill
                        | TerrainMode::ShoreRepair
                        | TerrainMode::CoastalCliff
                ) {
                    for cell in cliff_tools::rectangle(g.start, at) {
                        outline(painter, cell, g.base);
                    }
                } else if g.mode != TerrainMode::WaterPolyline {
                    for (a, b) in g
                        .path
                        .iter()
                        .chain(std::iter::once(&at))
                        .zip(g.path.iter().skip(1).chain(std::iter::once(&at)))
                    {
                        for cell in cell_line(*a, *b) {
                            outline(painter, cell, g.base);
                        }
                    }
                }
                painter.text(
                    rect.left_top() + egui::vec2(12., 12.),
                    egui::Align2::LEFT_TOP,
                    error.unwrap_or_else(|| tr!("无法生成预览").into()),
                    egui::FontId::proportional(13.),
                    Color32::LIGHT_RED,
                );
            }
        }
        if let Some(g) = self.plateau.gesture.as_ref()
            && click_polyline(g.mode)
        {
            let water = g.mode == TerrainMode::WaterPolyline;
            let position = |at: Cell| {
                let (x, y) = vera20k_core::map::terrain::iso_to_screen(
                    at[0],
                    at[1],
                    if self.flat_view {
                        0
                    } else if water {
                        terrain_tools::value(doc, scene, at).level
                    } else {
                        g.base
                    },
                );
                rect.min + (egui::pos2(x + 30., y + 15.) - world.min) * camera.zoom
            };
            let mut path = g.path.clone();
            if path.last() != Some(&at) {
                path.push(at);
            }
            if water && g.path.len() >= 3 {
                painter.extend(egui::Shape::dashed_line(
                    &[position(*g.path.last().unwrap()), position(g.start)],
                    egui::Stroke::new(2., color),
                    6.,
                    4.,
                ));
            }
            for (i, ab) in path.windows(2).enumerate() {
                let a = position(ab[0]);
                let b = position(ab[1]);
                if i + 1 < g.path.len() {
                    painter.line_segment([a, b], egui::Stroke::new(2., Color32::YELLOW));
                } else if !water || at != g.start || g.path.len() < 3 {
                    painter.extend(egui::Shape::dashed_line(
                        &[a, b],
                        egui::Stroke::new(2., Color32::YELLOW),
                        6.,
                        4.,
                    ));
                }
                if !water && a.distance(b) > 12. {
                    let d = (b - a).normalized();
                    let normal = egui::vec2(d.y, -d.x) * if g.high_left { 1. } else { -1. };
                    painter.arrow(
                        a + (b - a) * 0.5,
                        normal * 18.,
                        egui::Stroke::new(2., color),
                    );
                }
            }
            for (i, &vertex) in g.path.iter().enumerate() {
                painter.circle_stroke(
                    position(vertex),
                    if i == 0 { 6. } else { 3. },
                    egui::Stroke::new(2., Color32::YELLOW),
                );
            }
        }
    }

    pub(super) fn plateau_options(&mut self, ui: &mut egui::Ui, scene: &MapScene) {
        if self.terrain_mode == TerrainMode::CliffPolyline {
            ui.add_enabled_ui(!self.stroking, |ui| {
                let label = ui.label(tr!("高侧方向"));
                egui::ComboBox::from_id_salt("cliff-polyline-high-side")
                    .selected_text(if self.cliff_high_left {
                        tr!("绘制方向左侧")
                    } else {
                        tr!("绘制方向右侧")
                    })
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.cliff_high_left, true, tr!("绘制方向左侧"));
                        ui.selectable_value(&mut self.cliff_high_left, false, tr!("绘制方向右侧"));
                    })
                    .response
                    .labelled_by(label.id);
            });
            ui.small(tr!(
                "箭头始终指向所选高侧；闭合后按绕行方向生成高台或坑，内外相差 4 层，外侧地面保持原高。"
            ));
        }
        if self.terrain_mode == TerrainMode::CliffHeightRepair {
            ui.add(egui::Slider::new(&mut self.cliff_repair_base, 0..=123).text(tr!("低地高度")));
            ui.small(tr!(
                "拖框选中已有悬崖，按低地基准修正各子块高度；不改变图块、轮廓或内部平地。"
            ));
            ui.small(tr!(
                "零层地面上逐格拼出的悬崖，低地高度设为 0。修正后再用填高地将内部升到 4 层。"
            ));
            return;
        }
        if self.terrain_mode == TerrainMode::HeightFill {
            self.height_pick_options(ui);
            ui.small(tr!(
                "先点击崖顶或地面拾取高度，再点击围合区域内部；按住预览，松开填高。"
            ));
            ui.small(tr!(
                "只填充同高度的连通平地，保留悬崖和材质；区域通到地图边缘或有对象占用时拒绝修改。"
            ));
            return;
        }
        if self.terrain_mode.joining() {
            let text = match self.terrain_mode {
                TerrainMode::Water => {
                    tr!("拖动矩形绘制水域；自动生成直岸、内角和外角，与已有水域合并。")
                }
                TerrainMode::WaterOutline => {
                    tr!(
                        "按住描绘水域轮廓，松开闭合并填水；无法拼接的细小折角会按 2×2 格向外规整，以预览为准。"
                    )
                }
                TerrainMode::WaterPolyline => {
                    tr!("点击添加折点，点击起点或按 Enter 闭合填水。Backspace 撤回折点，Esc 取消。")
                }
                TerrainMode::LandFill => {
                    tr!("拖动矩形回填为平地；自动移除旧岸线，修补湖岸和岛屿。")
                }
                TerrainMode::ShoreRepair => {
                    tr!("拖动矩形修复水陆边界，保持现有水域范围；包含邻近的完整岸线模板。")
                }
                TerrainMode::CoastalCliff => {
                    tr!(
                        "框选已有正面悬崖；按低地是否临水，切换水悬崖或陆地悬崖。背面方向保持原样。"
                    )
                }
                TerrainMode::Road => {
                    tr!(
                        "沿地图轴向拖出三格宽道路。从已有路端继续画可生成弯道；接入道路中部生成 T 字或十字路口。"
                    )
                }
                _ => tr!("沿道路中心线拖动切除，修复剩余路段的端点和路口。"),
            };
            ui.small(text);
            if self.terrain_mode == TerrainMode::WaterPolyline {
                ui.small(tr!(
                    "自动连接末点与起点，保留凹形轮廓。岸线无法拼接时按 2×2 格向外规整。"
                ));
            }
            if matches!(self.terrain_mode, TerrainMode::Road | TerrainMode::RoadCut) {
                ui.small(tr!(
                    "中心线吸附附近道路；路口与转角之间请保留至少 4 格直道。切除后恢复为平地。"
                ));
            } else if self.terrain_mode != TerrainMode::CoastalCliff {
                ui.small(tr!(
                    "水道和陆地至少宽 2 格；狭窄急转角请适当加宽。水域保持原高度，岸线两侧需等高。"
                ));
            }
            ui.small(tr!("自动选择当前环境资源；保护建筑、对象、桥面和特殊地形。绿色可放置，红色说明原因，Esc 取消。"));
            return;
        }
        let supported = scene
            .palette
            .tile_ids
            .is_some_and(|ids| ids.cliffs.cliff_set.is_some());
        if !supported {
            ui.colored_label(
                Color32::LIGHT_RED,
                tr!("当前环境没有标准陆地悬崖，无法自动生成高台。"),
            );
        }
        let mut fixed = self.template_level.is_some();
        let polyline = self.terrain_mode == TerrainMode::CliffPolyline;
        if ui
            .checkbox(
                &mut fixed,
                if polyline {
                    tr!("指定基准地面高度")
                } else {
                    tr!("指定低地基准高度")
                },
            )
            .changed()
        {
            self.template_level = fixed.then_some(0);
        }
        if let Some(level) = self.template_level.as_mut() {
            ui.add(
                egui::Slider::new(
                    level,
                    0..=if polyline {
                        cliff_tools::MAX_CLIFF_LEVEL
                    } else {
                        cliff_tools::MAX_CLIFF_BASE
                    },
                )
                .smart_aim(false)
                .drag_value_speed(1.)
                .text(if polyline {
                    tr!("基准地面高度")
                } else {
                    tr!("低地高度")
                }),
            );
            if !polyline {
                ui.small(tr!("可点击数字直接输入高度；高地比低地高 4 层。"));
            }
        } else {
            if !polyline {
                ui.small(tr!("自动识别所选高台的低地高度；悬崖高差为 4 层。"));
            }
        }
        if polyline {
            ui.small(tr!("闭合时以外侧地面为基准：高台内部加 4 层，坑内减 4 层。地面低于 4 层时请先抬高再挖坑。"));
        }
        match self.terrain_mode {
            TerrainMode::CliffPolyline => {
                ui.small(tr!("点击添加折点；点击起点闭合并统一内部高度；Enter 生成已确认折线，Backspace 撤回折点，Esc 取消。"));
                ui.small(tr!(
                    "开放折线只沿线放置一侧崖壁，周围地面保持原高；不生成坡道、填高区域或末端封口。闭合后才统一内部高度。"
                ));
            }
            TerrainMode::Plateau => {
                ui.small(tr!("拖到对角格：新建、扩建或合并同层高台，重建悬崖边界。"));
            }
            TerrainMode::PlateauCut => {
                ui.small(tr!("拖到对角格：切除所选高台，修补剩余边界；可整块删除。"));
            }
            TerrainMode::CliffFront | TerrainMode::CliffBack => {
                ui.small(tr!(
                    "拖出悬崖线段，自动吸附八方向。正面高地朝屏幕上方，背面相反。"
                ));
                ui.small(tr!("高侧生成窄高台，末端和后侧自动封口。接续时合并高地并移除内部旧崖壁；可用高台工具继续扩建。"));
            }
            TerrainMode::RampEntrance => {
                ui.small(tr!("在悬崖边缘按住左键拖动加宽；点在矩形角落会生成向外下降的坡面；接到同高度、同坡向的已有入口时自动合并，松开生成。"));
            }
            _ => {
                ui.small(tr!(
                    "按住左键描绘高台轮廓：松开闭合，与触及的同层高台合并。"
                ));
            }
        }
        ui.small(tr!(
            "使用标准陆地悬崖，保留未改变的对象；修改范围内的覆盖物和对象会阻止放置。"
        ));
        if self.terrain_mode != TerrainMode::CliffPolyline {
            ui.small(tr!(
                "绿色为可生成预览；红色说明原因。松开整笔提交，Esc 取消。"
            ));
        }
    }
}
#[cfg(test)]
#[path = "cliff_edit_tests.rs"]
mod tests;
