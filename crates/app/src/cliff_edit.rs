//! Deferred plateau gestures and cached, side-effect-free generation previews.
use super::*;
use ra2_map_scene::cliff_authoring;
use ra2_map_scene::cliff_tools::{self, PlateauPlan};

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
    bridge: ra2_map_scene::bridge_tools::BridgeKind,
}
#[derive(PartialEq)]
struct Key {
    document: u64,
    state: u64,
    mode: TerrainMode,
    path: Vec<Cell>,
    base: u8,
    bridge: ra2_map_scene::bridge_tools::BridgeKind,
}
struct Preview {
    key: Key,
    result: std::result::Result<Arc<PlateauPlan>, String>,
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
        let g = self.gesture.as_ref().ok_or("按下并拖动绘制高台")?;
        let mut path = g.path.clone();
        if path.last() != Some(&at) {
            path.push(at);
        }
        if !matches!(
            g.mode,
            TerrainMode::CliffOutline | TerrainMode::WaterOutline
        ) {
            path = vec![g.start, at];
        }
        let key = Key {
            document: doc.id,
            state: doc.state_id,
            mode: g.mode,
            path,
            base: g.base,
            bridge: g.bridge,
        };
        if self.preview.as_ref().is_none_or(|p| p.key != key) {
            let result = (|| -> Result<Arc<PlateauPlan>> {
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
                    let cells = if key.mode == TerrainMode::WaterOutline {
                        cliff_tools::enclose(&key.path)?
                    } else {
                        cliff_tools::rectangle(g.start, at)
                    };
                    if key.mode == TerrainMode::CoastalCliff {
                        return Ok(Arc::new(ra2_map_scene::coastal_cliffs::plan(
                            doc, scene, cells,
                        )?));
                    }
                    let paint = match key.mode {
                        TerrainMode::Water | TerrainMode::WaterOutline => Some(true),
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
            self.preview = Some(Preview { key, result });
        }
        self.preview.as_ref().unwrap().result.clone()
    }
}
impl Editor {
    pub(crate) fn plateau_preview_plan(
        &mut self,
        scene: &MapScene,
        at: Cell,
    ) -> Result<Arc<PlateauPlan>> {
        self.plateau
            .generate(self.doc.as_ref().context("没有地图文档")?, scene, at)
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
                bridge: self.bridge_kind,
                base: self.template_level.unwrap_or_else(|| {
                    if self.terrain_mode.joining() {
                        terrain_tools::value(doc, scene, at).level
                    } else if self.terrain_mode == TerrainMode::Bridge {
                        terrain_tools::value(doc, scene, at)
                            .level
                            .saturating_sub(if self.bridge_kind.high() { 4 } else { 0 })
                    } else {
                        cliff_authoring::infer_base(doc, scene, at)
                    }
                }),
            });
        }
        if let Some(g) = self.plateau.gesture.as_mut() {
            if (down || press || release)
                && let Some(at) = at
                && g.path.last() != Some(&at)
            {
                g.path.push(at);
            }
            if release || !down {
                let last = *g.path.last().unwrap();
                let mode = g.mode;
                let result = self
                    .plateau
                    .generate(self.doc.as_ref().unwrap(), scene, last);
                self.plateau.cancel();
                self.stroking = false;
                match result {
                    Ok(plan) => match plan.apply(self.doc.as_mut().unwrap()) {
                        Ok(cells) => {
                            self.feedback = cells.into_iter().collect();
                            self.commit(scene, mode.name());
                            self.status = format!(
                                "{} · {} 格 · {} 个地形模板 · {} 个桥面格",
                                mode.name(),
                                self.feedback.len(),
                                plan.cliffs.len(),
                                plan.overlays.len()
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
                self.terrain_mode.name(),
                egui::FontId::proportional(13.),
                Color32::LIGHT_GREEN,
            );
            return;
        }
        let world = camera.world_rect(rect);
        let result = self.plateau.generate(doc, scene, at);
        let color = if result.is_ok() {
            Color32::from_rgb(80, 220, 170)
        } else {
            Color32::LIGHT_RED
        };
        let outline = |painter: &egui::Painter, at: Cell, level: u8| {
            let (x, y) = vera20k_core::map::terrain::iso_to_screen(at[0], at[1], level);
            let center = rect.min + (egui::pos2(x + 30., y + 15.) - world.min) * camera.zoom;
            if !rect.expand(60. * camera.zoom).contains(center) {
                return;
            }
            let points = [[0., -15.], [30., 0.], [0., 15.], [-30., 0.]]
                .map(|[x, y]| center + egui::vec2(x, y) * camera.zoom);
            painter.add(egui::Shape::convex_polygon(
                points.to_vec(),
                color.gamma_multiply(0.10),
                egui::Stroke::new(1., color),
            ));
        };
        match result {
            Ok(plan) => {
                // Filled water and restored ground are part of the ghost too.
                let stamped: BTreeSet<_> = plan.cliffs.iter().map(|s| (s.tile, s.origin)).collect();
                for (&at, v) in &plan.edits {
                    let Some(&tile) = scene.palette.raw_ids.get(&v.tile) else {
                        continue;
                    };
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
                    let level = plan
                        .edits
                        .get(&at)
                        .copied()
                        .unwrap_or_else(|| terrain_tools::value(doc, scene, at))
                        .level;
                    outline(painter, at, level + if high { 4 } else { 0 });
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
                                Color32::from_white_alpha(180),
                            );
                        }
                    }
                }
                let g = self.plateau.gesture.as_ref().unwrap();
                let heights = if g.mode.joining() {
                    "按原高度连接地形".into()
                } else if g.mode == TerrainMode::Bridge && !g.bridge.high() {
                    format!("桥面与地面 {} 层", plan.base)
                } else {
                    format!("低地 {} / 高地 {} 层", plan.base, plan.base + 4)
                };
                painter.text(
                    rect.left_top() + egui::vec2(12., 12.),
                    egui::Align2::LEFT_TOP,
                    format!(
                        "{} 格修改 · {} 个地形模板 · {}；松开放置，Esc 取消",
                        plan.edits.len() + plan.overlays.len(),
                        plan.cliffs.len(),
                        heights
                    ),
                    egui::FontId::proportional(13.),
                    Color32::LIGHT_GREEN,
                );
            }
            Err(error) => {
                let g = self.plateau.gesture.as_ref().unwrap();
                if matches!(
                    g.mode,
                    TerrainMode::Plateau
                        | TerrainMode::PlateauCut
                        | TerrainMode::Water
                        | TerrainMode::LandFill
                        | TerrainMode::ShoreRepair
                        | TerrainMode::CoastalCliff
                ) {
                    for cell in cliff_tools::rectangle(g.start, at) {
                        outline(painter, cell, g.base);
                    }
                } else {
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
                    error,
                    egui::FontId::proportional(13.),
                    Color32::LIGHT_RED,
                );
            }
        }
    }

    pub(super) fn plateau_options(&mut self, ui: &mut egui::Ui, scene: &MapScene) {
        if self.terrain_mode.joining() {
            let text = match self.terrain_mode {
                TerrainMode::Water => {
                    "拖动矩形绘制水域；自动生成直岸、内角和外角，与已有水域合并。"
                }
                TerrainMode::WaterOutline => "按住描绘水域轮廓，松开闭合并填水；自动衔接已有岸线。",
                TerrainMode::LandFill => "拖动矩形回填为平地；自动移除旧岸线，修补湖岸和岛屿。",
                TerrainMode::ShoreRepair => {
                    "拖动矩形修复水陆边界，保持现有水域范围；包含邻近的完整岸线模板。"
                }
                TerrainMode::CoastalCliff => {
                    "框选已有正面悬崖；按低地是否临水，切换水悬崖或陆地悬崖。背面方向保持原样。"
                }
                TerrainMode::Road => {
                    "沿地图轴向拖出三格宽道路。从已有路端继续画可生成弯道；接入道路中部生成 T 字或十字路口。"
                }
                _ => "沿道路中心线拖动切除，修复剩余路段的端点和路口。",
            };
            ui.small(text);
            if matches!(self.terrain_mode, TerrainMode::Road | TerrainMode::RoadCut) {
                ui.small(
                    "中心线吸附附近道路；路口与转角之间请保留至少 4 格直道。切除后恢复为平地。",
                );
            } else if self.terrain_mode != TerrainMode::CoastalCliff {
                ui.small(
                    "水道和陆地至少宽 2 格；狭窄急转角请适当加宽。水域保持原高度，岸线两侧需等高。",
                );
            }
            ui.small("自动选择当前环境资源；保护建筑、对象、桥面和特殊地形。绿色可放置，红色说明原因，Esc 取消。");
            return;
        }
        let supported = scene
            .palette
            .tile_ids
            .is_some_and(|ids| ids.cliffs.cliff_set.is_some());
        if !supported {
            ui.colored_label(
                Color32::LIGHT_RED,
                "当前环境没有标准陆地悬崖，无法自动生成高台。",
            );
        }
        let mut fixed = self.template_level.is_some();
        if ui.checkbox(&mut fixed, "指定低地基准高度").changed() {
            self.template_level = fixed.then_some(0);
        }
        if let Some(level) = self.template_level.as_mut() {
            ui.add(egui::Slider::new(level, 0..=10).text("低地高度"));
        } else {
            ui.small("自动识别所选高台的低地高度；悬崖高差为 4 层。");
        }
        match self.terrain_mode {
            TerrainMode::Plateau => {
                ui.small("拖到对角格：新建、扩建或合并同层高台，重建悬崖边界。");
            }
            TerrainMode::PlateauCut => {
                ui.small("拖到对角格：切除所选高台，修补剩余边界；可整块删除。");
            }
            TerrainMode::CliffFront | TerrainMode::CliffBack => {
                ui.small("拖出悬崖线段，自动吸附八方向。正面高地朝屏幕上方，背面相反。");
                ui.small("高侧生成窄高台，末端和后侧自动封口。接续时合并高地并移除内部旧崖壁；可用高台工具继续扩建。");
            }
            TerrainMode::RampEntrance => {
                ui.small("从高台向低地拖动，选择坡道方向；自动替换完整悬崖并衔接两侧。");
            }
            _ => {
                ui.small("按住左键描绘高台轮廓：松开闭合，与触及的同层高台合并。");
            }
        }
        ui.small("使用标准陆地悬崖，保留未改变的对象；修改范围内的覆盖物和对象会阻止放置。");
        ui.small("绿色为可生成预览；红色说明原因。松开整笔提交，Esc 取消。");
    }
}
#[cfg(test)]
#[path = "cliff_edit_tests.rs"]
mod tests;
