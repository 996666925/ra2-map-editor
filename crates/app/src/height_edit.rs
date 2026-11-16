//! Raise/lower previews use the same validated plan as release, including LAT.
use super::*;
use crate::i18n::tr;
use ra2_map_scene::cliff_tools::PlateauPlan;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Settings {
    mode: TerrainMode,
    change: terrain_tools::HeightChange,
    smooth: bool,
    lat: bool,
    origin: Option<Cell>,
    connected: bool,
    steep: bool,
    vertices: bool,
    force: bool,
}
impl From<Stroke> for Settings {
    fn from(s: Stroke) -> Self {
        Self {
            mode: s.mode,
            change: s.height_change,
            smooth: s.smooth,
            lat: s.lat,
            origin: s.height_origin,
            connected: s.height_connected,
            steep: s.height_steep,
            vertices: s.height_vertices,
            force: s.height_force,
        }
    }
}
#[derive(PartialEq)]
struct Key {
    document: u64,
    revision: u64,
    palette: usize,
    cells: BTreeSet<Cell>,
    settings: Settings,
}
struct Preview {
    key: Key,
    result: std::result::Result<Arc<PlateauPlan>, String>,
}
#[derive(Default)]
pub(super) struct HeightEdit {
    preview: Option<Preview>,
}
impl HeightEdit {
    pub fn cancel(&mut self) {
        self.preview = None;
    }
    pub fn plan(
        &mut self,
        doc: &MapDocument,
        scene: &MapScene,
        cells: &BTreeSet<Cell>,
        settings: Settings,
    ) -> std::result::Result<Arc<PlateauPlan>, String> {
        let key = Key {
            document: doc.id,
            revision: doc.edit_revision(),
            palette: Arc::as_ptr(&scene.palette) as usize,
            cells: cells.clone(),
            settings,
        };
        if self.preview.as_ref().is_none_or(|p| p.key != key) {
            let result = (|| -> Result<Arc<PlateauPlan>> {
                let mut copy = doc.fork();
                let cliff_cells = settings
                    .origin
                    .filter(|_| !settings.vertices && !settings.force)
                    .map(|at| {
                        ra2_map_scene::height_tools::cliff_selection(
                            doc,
                            scene,
                            at,
                            settings.connected,
                        )
                    })
                    .transpose()?
                    .flatten();
                let mut vertex_cells = cells.clone();
                vertex_cells.extend(settings.origin);
                let vertex_heights = if settings.vertices {
                    ra2_map_scene::flatten_tools::vertex_heights(
                        doc,
                        scene,
                        &vertex_cells,
                        settings.force,
                    )?
                } else {
                    BTreeMap::new()
                };
                let old_height = |at| {
                    if settings.vertices {
                        vertex_heights.get(&at).copied().unwrap_or(0)
                    } else {
                        terrain_tools::value(doc, scene, at).level
                    }
                };
                let source = settings.origin.map(old_height);
                let editable = if settings.smooth && cliff_cells.is_none() {
                    if settings.vertices {
                        cells
                            .intersection(&vertex_heights.keys().copied().collect())
                            .copied()
                            .collect()
                    } else {
                        ra2_map_scene::flatten_tools::editable_cells_with_force(
                            doc,
                            scene,
                            cells,
                            settings.force,
                        )?
                    }
                } else {
                    cells.clone()
                };
                let ground: BTreeSet<_> = editable
                    .iter()
                    .copied()
                    .filter(|&at| match settings.change {
                        terrain_tools::HeightChange::Step(_) => source.is_none_or(|level| {
                            if settings.vertices {
                                if settings.mode == TerrainMode::Raise {
                                    old_height(at) <= level
                                } else {
                                    old_height(at) >= level
                                }
                            } else {
                                old_height(at) == level
                            }
                        }),
                        terrain_tools::HeightChange::To(level) => {
                            let old = old_height(at);
                            if settings.mode == TerrainMode::Raise {
                                old < level
                            } else {
                                old > level
                            }
                        }
                    })
                    .collect();
                let selected = cliff_cells.as_ref().unwrap_or(&ground);
                let smooth_ground = settings.smooth && cliff_cells.is_none();
                let mut changed: BTreeSet<_> = if smooth_ground {
                    let target = match settings.change {
                        terrain_tools::HeightChange::To(level) => level,
                        _ => settings.change.target(settings.mode, source.unwrap_or(0)),
                    };
                    ra2_map_scene::flatten_tools::plan_with_options(
                        doc,
                        scene,
                        selected,
                        target,
                        true,
                        ra2_map_scene::flatten_tools::Options {
                            steep: settings.steep && !settings.force,
                            vertices: settings.vertices,
                            force: settings.force,
                        },
                    )?
                    .apply(&mut copy)?
                } else {
                    terrain_tools::height_with_change(
                        &mut copy,
                        scene,
                        selected,
                        settings.mode,
                        0,
                        smooth_ground,
                        settings.change,
                    )?
                }
                .into_iter()
                .collect();
                if settings.lat && !changed.is_empty() {
                    changed.extend(terrain_tools::recalculate(&mut copy, scene, &changed)?);
                }
                if smooth_ground && matches!(settings.change, terrain_tools::HeightChange::To(_)) {
                    let next_vertices = if settings.vertices {
                        ra2_map_scene::flatten_tools::vertex_heights(
                            &copy,
                            scene,
                            &editable,
                            settings.force,
                        )?
                    } else {
                        BTreeMap::new()
                    };
                    for &at in &editable {
                        let expected = settings.change.target(settings.mode, old_height(at));
                        let actual = if settings.vertices {
                            next_vertices.get(&at).copied().unwrap_or(0)
                        } else {
                            terrain_tools::value(&copy, scene, at).level
                        };
                        anyhow::ensure!(
                            actual == expected,
                            "{},{} 的目标高度与邻接坡面冲突；请缩小范围或关闭自动坡面，未修改地形",
                            at[0],
                            at[1]
                        );
                    }
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
                    interior: cells.clone(),
                    base: 0,
                }))
            })()
            .map_err(|e| format!("{e:#}"));
            self.preview = Some(Preview { key, result });
        }
        self.preview.as_ref().unwrap().result.clone()
    }
}
impl Editor {
    pub(crate) fn nudge_flatten_height(&mut self, scene: &MapScene, at: Cell, raise: bool) {
        if self.stroking
            || self.doc.is_none()
            || self.tool != Tool::Terrain
            || self.terrain_mode != TerrainMode::Flatten
        {
            return;
        }
        let doc = self.doc.as_ref().unwrap();
        let mut cells = BTreeSet::new();
        for dx in 0..self.radius {
            for dy in 0..self.radius {
                let x = i32::from(at[0]) + i32::from(dx) - i32::from(self.radius / 2);
                let y = i32::from(at[1]) + i32::from(dy) - i32::from(self.radius / 2);
                if let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y))
                    && doc.check_cell([x, y]).is_ok()
                {
                    cells.insert([x, y]);
                }
            }
        }
        let mode = if raise {
            TerrainMode::Raise
        } else {
            TerrainMode::Lower
        };
        let settings = Settings {
            mode,
            change: terrain_tools::HeightChange::Step(1),
            smooth: self.smooth_heights,
            lat: self.auto_lat,
            origin: Some(at),
            connected: false,
            steep: self.brush_shift,
            vertices: self.height_vertices && self.smooth_heights,
            force: false,
        };
        match self.height_edit.plan(doc, scene, &cells, settings) {
            Ok(plan) => match plan.apply(self.doc.as_mut().unwrap()) {
                Ok(cells) => {
                    self.feedback = cells.into_iter().collect();
                    self.commit(scene, mode.name());
                    self.status = tr!(
                        "{arg0} · {arg1} 格",
                        arg0 = mode.name(),
                        arg1 = self.feedback.len()
                    );
                }
                Err(error) => self.abort_stroke(scene, error),
            },
            Err(error) => self.abort_stroke(scene, anyhow::Error::msg(error)),
        }
    }

    pub(super) fn raise_lower_preview_active(&self) -> bool {
        self.stroke.filter(|_| self.stroking).is_some_and(|s| {
            s.tool == Tool::Terrain && matches!(s.mode, TerrainMode::Raise | TerrainMode::Lower)
        }) || self.tool == Tool::Terrain
            && matches!(self.terrain_mode, TerrainMode::Raise | TerrainMode::Lower)
    }

    pub(super) fn raise_lower_plan(
        &mut self,
        scene: &MapScene,
        at: Option<Cell>,
    ) -> std::result::Result<Arc<PlateauPlan>, String> {
        let doc = self
            .doc
            .as_ref()
            .ok_or_else(|| tr!("请先打开地图").to_owned())?;
        let (settings, cells) = if let Some(s) = self.stroke.filter(|_| self.stroking) {
            (Settings::from(s), self.stroke_cells.clone())
        } else {
            let mut cells = BTreeSet::new();
            if let Some(center) = at {
                for dx in 0..self.radius {
                    for dy in 0..self.radius {
                        let x = i32::from(center[0]) + i32::from(dx) - i32::from(self.radius / 2);
                        let y = i32::from(center[1]) + i32::from(dy) - i32::from(self.radius / 2);
                        if let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) {
                            let at = [x, y];
                            if doc.check_cell(at).is_ok() {
                                cells.insert(at);
                            }
                        }
                    }
                }
            }
            (
                Settings {
                    mode: self.terrain_mode,
                    change: self.height_change(),
                    smooth: self.smooth_heights,
                    lat: self.auto_lat,
                    origin: at,
                    connected: self.brush_ctrl,
                    steep: self.brush_shift,
                    vertices: self.height_vertices && self.smooth_heights,
                    force: self.brush_ctrl && self.brush_shift && self.smooth_heights,
                },
                cells,
            )
        };
        self.height_edit.plan(doc, scene, &cells, settings)
    }

    pub(super) fn raise_lower_preview(
        &mut self,
        painter: &egui::Painter,
        scene: &MapScene,
        rect: egui::Rect,
        camera: &crate::camera::MapCamera,
        at: Option<Cell>,
    ) {
        if !self.stroking && at.is_none() {
            return;
        }
        let result = self.raise_lower_plan(scene, at);
        let (message, color) = match result {
            Ok(plan) => {
                let world = camera.world_rect(rect);
                for (&at, v) in &plan.edits {
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
                let mode = self
                    .stroke
                    .filter(|_| self.stroking)
                    .map_or(self.terrain_mode, |s| s.mode);
                (
                    tr!(
                        "{arg0} · {arg1} 格",
                        arg0 = mode.name(),
                        arg1 = plan.edits.len()
                    ),
                    Color32::LIGHT_GREEN,
                )
            }
            Err(error) => (error, Color32::LIGHT_RED),
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
#[path = "height_edit_tests.rs"]
mod tests;
