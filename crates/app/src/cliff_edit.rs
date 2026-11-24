//! Deferred plateau gestures and cached, side-effect-free generation previews.
use super::*;
use crate::i18n::tr;
use ra2_map_scene::cliff_authoring::{self, CliffCut, CutSide};
use ra2_map_scene::cliff_tools::{self, PlateauPlan};
use ra2_map_scene::road_tools::{self, RoadStyle};
use std::sync::Weak;

fn click_polyline(mode: TerrainMode) -> bool {
    matches!(
        mode,
        TerrainMode::CliffPolyline | TerrainMode::WaterPolyline | TerrainMode::RoadPolyline
    )
}

#[derive(Default)]
pub(super) struct PlateauEdit {
    gesture: Option<Gesture>,
    preview: Option<Preview>,
    draw_order: PreviewDrawOrder,
    road_draft: RoadDraft,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum PlateauCutMethod {
    #[default]
    Rectangle,
    TwoPoint,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct CutOptions {
    side: CutSide,
    target: Option<u8>,
}

struct CutDraft {
    selection: CliffCut,
    document: u64,
    revision: u64,
    palette: Weak<ra2_map_assets::catalog::TerrainPalette>,
}

impl CutDraft {
    fn is_current(&self, doc: &MapDocument, scene: &MapScene) -> bool {
        self.document == doc.id
            && self.revision == doc.edit_revision()
            && self.palette.as_ptr() == Arc::as_ptr(&scene.palette)
    }
}

// A cut is an infinite directed line through the anchor. Endpoints on the
// same ray select the same terrain regardless of their distance.
fn cut_ray(start: Cell, end: Cell) -> [i32; 2] {
    let [dx, dy] = [0, 1].map(|axis| i32::from(end[axis]) - i32::from(start[axis]));
    let (mut a, mut b) = (dx.abs(), dy.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    [dx / a.max(1), dy / a.max(1)]
}

struct Gesture {
    mode: TerrainMode,
    start: Cell,
    path: Vec<Cell>,
    base: u8,
    high_left: bool,
    bridge: ra2_map_scene::bridge_tools::BridgeKind,
    road: RoadStyle,
    road_axis_lock: bool,
    road_leg_pressed: bool,
    shore_brush: Option<ShoreBrush>,
    cut: Option<CutDraft>,
}

struct ShoreBrush {
    width: u16,
    last: Option<Cell>,
    cells: BTreeSet<Cell>,
}

impl ShoreBrush {
    fn footprint(doc: &MapDocument, at: Cell, width: u16) -> impl Iterator<Item = Cell> + '_ {
        let low = at.map(|v| v.saturating_sub(width / 2));
        let high = at.map(|v| v.saturating_add((width - 1) / 2));
        (low[0]..=high[0])
            .flat_map(move |x| (low[1]..=high[1]).map(move |y| [x, y]))
            .filter(|&cell| doc.check_cell(cell).is_ok())
    }

    fn sample(&mut self, doc: &MapDocument, at: Option<Cell>) -> bool {
        let last = std::mem::replace(&mut self.last, at);
        let Some(at) = at.filter(|at| Some(*at) != last) else {
            return false;
        };
        let before = self.cells.len();
        for center in cell_line(last.unwrap_or(at), at) {
            self.cells.extend(Self::footprint(doc, center, self.width));
        }
        self.cells.len() != before
    }
}

impl Gesture {
    fn road_endpoint(&self, at: Cell) -> Cell {
        if self.road_axis_lock {
            road_tools::axis_endpoint(*self.path.last().unwrap(), at)
        } else {
            at
        }
    }
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
    road: RoadStyle,
    road_axis_lock: bool,
    cut: Option<(CutOptions, [i32; 2])>,
}
struct Preview {
    key: Key,
    result: std::result::Result<Arc<PlateauPlan>, String>,
    bridge_draft: Option<Arc<PlateauPlan>>,
}
#[derive(Default)]
struct PreviewDrawOrder {
    plan: Weak<PlateauPlan>,
    edits: Vec<(Cell, crate::document::TerrainValue)>,
}

impl PreviewDrawOrder {
    fn edits(&mut self, plan: &Arc<PlateauPlan>) -> &[(Cell, crate::document::TerrainValue)] {
        if self.plan.as_ptr() != Arc::as_ptr(plan) {
            self.plan = Arc::downgrade(plan);
            self.edits.clear();
            self.edits
                .extend(plan.edits.iter().map(|(&at, &v)| (at, v)));
            self.edits
                .sort_unstable_by_key(|(at, _)| (u32::from(at[0]) + u32::from(at[1]), at[0]));
        }
        &self.edits
    }
}

#[derive(PartialEq, Eq)]
struct RoadDraftKey {
    document: u64,
    palette: usize,
    state: u64,
    style: RoadStyle,
}

struct RoadCandidate {
    route: road_tools::RoadRoute,
    plan: Arc<PlateauPlan>,
}
type RoadResult = std::result::Result<RoadCandidate, String>;

#[derive(Default)]
struct RoadDraft {
    key: Option<RoadDraftKey>,
    // One checkpoint per placed endpoint after the start, including invalid
    // ones. A later segment can finish a pending connection (e.g. off a slope).
    confirmed: Vec<RoadResult>,
    hover: Option<(Cell, RoadResult)>,
}
impl RoadDraft {
    fn last_valid(&self) -> Option<(usize, &RoadCandidate)> {
        self.confirmed
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, result)| result.as_ref().ok().map(|plan| (index + 2, plan)))
    }

    fn truncate(&mut self, vertices: usize) {
        self.confirmed.truncate(vertices.saturating_sub(1));
        self.hover = None;
    }

    fn candidate(
        &self,
        doc: &MapDocument,
        scene: &MapScene,
        vertices: &[Cell],
        end: Cell,
        style: RoadStyle,
    ) -> RoadResult {
        let (resolved, previous) = self.last_valid().unzip();
        let mut ends = vertices[resolved.unwrap_or(0)..].to_vec();
        ends.push(end);
        let (route, plan) = match previous {
            Some(previous) => previous.route.extend(doc, scene, &ends),
            None => road_tools::plan_route(doc, scene, &ends, style),
        }
        .map_err(|e| format!("{e:#}"))?;
        Ok(RoadCandidate {
            route,
            plan: Arc::new(plan),
        })
    }

    fn generate(
        &mut self,
        doc: &MapDocument,
        scene: &MapScene,
        vertices: &[Cell],
        end: Cell,
        style: RoadStyle,
    ) -> std::result::Result<Arc<PlateauPlan>, String> {
        let key = RoadDraftKey {
            document: doc.id,
            palette: Arc::as_ptr(&scene.palette) as usize,
            state: doc.edit_revision(),
            style,
        };
        if self.key.as_ref() != Some(&key) {
            *self = Self {
                key: Some(key),
                ..Self::default()
            };
        }
        while self.confirmed.len() + 1 < vertices.len() {
            let index = self.confirmed.len() + 1;
            let placed = vertices[index];
            let result = self
                .hover
                .take()
                .filter(|(at, _)| *at == placed)
                .map_or_else(
                    || self.candidate(doc, scene, &vertices[..index], placed, style),
                    |(_, result)| result,
                );
            self.confirmed.push(result);
        }
        let result = if vertices.last() == Some(&end) {
            self.confirmed
                .last()
                .ok_or_else(|| tr!("连续道路至少需要两个不同的点").to_owned())?
        } else {
            if self.hover.as_ref().is_none_or(|(at, _)| *at != end) {
                self.hover = Some((end, self.candidate(doc, scene, vertices, end, style)));
            }
            &self.hover.as_ref().unwrap().1
        };
        result
            .as_ref()
            .map(|candidate| Arc::clone(&candidate.plan))
            .map_err(Clone::clone)
    }
}
impl PlateauEdit {
    pub fn cancel(&mut self) {
        *self = Self::default();
    }
    pub(super) fn road_draft_end(&self) -> Option<Cell> {
        let g = self.gesture.as_ref()?;
        (g.mode == TerrainMode::RoadPolyline).then(|| *g.path.last().unwrap())
    }

    fn displayed_plan(
        &self,
        result: std::result::Result<Arc<PlateauPlan>, String>,
    ) -> Option<Arc<PlateauPlan>> {
        result.ok().or_else(|| {
            if self.road_draft_end().is_some() {
                self.road_draft
                    .last_valid()
                    .map(|(_, draft)| Arc::clone(&draft.plan))
            } else {
                self.preview.as_ref()?.bridge_draft.clone()
            }
        })
    }

    fn generate(
        &mut self,
        doc: &MapDocument,
        scene: &MapScene,
        at: Cell,
        cut: CutOptions,
    ) -> std::result::Result<Arc<PlateauPlan>, String> {
        let g = self.gesture.as_ref().ok_or(tr!("按下并拖动绘制高台"))?;
        if g.mode == TerrainMode::RoadPolyline {
            return self
                .road_draft
                .generate(doc, scene, &g.path, g.road_endpoint(at), g.road);
        }
        let palette = Arc::as_ptr(&scene.palette) as usize;
        let cut_key = if let Some(draft) = &g.cut {
            if !draft.is_current(doc, scene) {
                return Err("地图或地形资源已变化，请取消后重新选择削切起点".into());
            }
            doc.check_cell(at).map_err(|error| error.to_string())?;
            Some((
                CutOptions {
                    target: Some(cut.target.unwrap_or(g.base)),
                    ..cut
                },
                cut_ray(g.start, at),
            ))
        } else {
            None
        };
        let path = if cut_key.is_some() {
            Vec::new()
        } else {
            match g.mode {
                TerrainMode::CliffOutline
                | TerrainMode::CliffPolyline
                | TerrainMode::WaterOutline => {
                    let mut path = g.path.clone();
                    if path.last() != Some(&at) {
                        path.push(at);
                    }
                    path
                }
                // Water previews show the exact closure Enter will commit. Hover
                // only draws a guide for the next edge until that point is clicked.
                TerrainMode::WaterPolyline | TerrainMode::ShoreRepair => g.path.clone(),
                TerrainMode::HeightFill => vec![g.start, g.start],
                _ => vec![g.start, at],
            }
        };
        let key = Key {
            document: doc.id,
            palette,
            state: doc.edit_revision(),
            mode: g.mode,
            path,
            base: g.base,
            high_left: g.high_left,
            bridge: g.bridge,
            road: g.road,
            road_axis_lock: g.road_axis_lock,
            cut: cut_key,
        };
        if self.preview.as_ref().is_none_or(|p| p.key != key) {
            let result = (|| -> Result<Arc<PlateauPlan>> {
                if let Some(draft) = &g.cut {
                    return Ok(Arc::new(
                        draft.selection.plan(doc, scene, at, cut.side, cut.target)?,
                    ));
                }
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
                    return Ok(Arc::new(road_tools::plan_segment(
                        doc,
                        scene,
                        g.start,
                        at,
                        key.road,
                        key.mode == TerrainMode::RoadCut,
                        key.road_axis_lock,
                    )?));
                }
                if let Some(brush) = &g.shore_brush {
                    return Ok(Arc::new(ra2_map_scene::shore_tools::plan(
                        doc,
                        scene,
                        brush.cells.clone(),
                        None,
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
                        _ => unreachable!("joining mode handled above"),
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
    fn two_point_cut_selected(&self) -> bool {
        self.terrain_mode == TerrainMode::PlateauCut
            && self.plateau_cut_method == PlateauCutMethod::TwoPoint
    }

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
                self.plateau.road_draft.truncate(g.path.len());
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
        let two_point_cut = g.cut.is_some();
        let result = self
            .plateau
            .generate(self.doc.as_ref().unwrap(), scene, last, self.cliff_cut);
        // Invalid clicks leave the existing draft available for correction.
        if (click_polyline(mode) || two_point_cut)
            && let Err(ref e) = result
        {
            if two_point_cut {
                self.plateau.gesture.as_mut().unwrap().path.truncate(1);
            }
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
            .generate(
                self.doc.as_ref().context(tr!("没有地图文档"))?,
                scene,
                at,
                self.cliff_cut,
            )
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
        if let Some(g) = self.plateau.gesture.as_mut()
            && g.cut.is_some()
        {
            if press && let Some(at) = at {
                g.path.push(at);
                self.finish_plateau(scene);
            }
            return true;
        }
        if let Some(g) = self.plateau.gesture.as_mut()
            && click_polyline(g.mode)
        {
            if g.mode == TerrainMode::RoadPolyline {
                g.road_axis_lock = self.brush_shift;
                if press && at.is_some() {
                    g.road_leg_pressed = true;
                }
                if (release || !down)
                    && std::mem::take(&mut g.road_leg_pressed)
                    && let Some(at) = at.map(|at| g.road_endpoint(at))
                    && g.path.last() != Some(&at)
                {
                    g.path.push(at);
                }
                return true;
            }
            if press && let Some(at) = at {
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
            let two_point_cut = self.two_point_cut_selected();
            let cut = if two_point_cut {
                match CliffCut::select(doc, scene, at) {
                    Ok(cut) => {
                        if let Some(target) = &mut self.cliff_cut.target {
                            *target = (*target).clamp(cut.base(), cut.base() + 3);
                        }
                        self.status =
                            tr!("移动鼠标预览削切，点击第二点提交。Esc 或右键取消。").into();
                        Some(CutDraft {
                            selection: cut,
                            document: doc.id,
                            revision: doc.edit_revision(),
                            palette: Arc::downgrade(&scene.palette),
                        })
                    }
                    Err(error) => {
                        self.status = crate::i18n::text(&error.to_string()).into();
                        return true;
                    }
                }
            } else {
                None
            };
            self.plateau.cancel();
            self.feedback.clear();
            self.stroking = true;
            self.plateau.gesture = Some(Gesture {
                mode: self.terrain_mode,
                base: cut
                    .as_ref()
                    .map(|draft| draft.selection.base())
                    .unwrap_or_else(|| self.plateau_base_for_click(doc, scene, at)),
                cut,
                start: at,
                path: vec![at],
                high_left: self.cliff_high_left,
                bridge: self.bridge_kind,
                road: self.road_style,
                road_axis_lock: self.brush_shift,
                shore_brush: (self.terrain_mode == TerrainMode::ShoreRepair).then(|| ShoreBrush {
                    width: self.shore_brush_width,
                    last: None,
                    cells: BTreeSet::new(),
                }),
                // The first click sets the start. Only a press still held
                // after this frame can also drag out the first leg.
                road_leg_pressed: self.terrain_mode == TerrainMode::RoadPolyline
                    && down
                    && !release,
            });
            if click_polyline(self.terrain_mode) || two_point_cut {
                return true;
            }
        }
        if let Some(g) = self.plateau.gesture.as_mut() {
            if g.mode == TerrainMode::Road {
                g.road_axis_lock = self.brush_shift;
            }
            if let Some(brush) = &mut g.shore_brush {
                if (down || press || release) && brush.sample(self.doc.as_ref().unwrap(), at) {
                    self.plateau.preview = None;
                }
            } else if (down || press || release)
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

    fn plateau_base_for_click(&self, doc: &MapDocument, scene: &MapScene, at: Cell) -> u8 {
        match self.terrain_mode {
            TerrainMode::HeightFill => self.flatten_level,
            TerrainMode::CliffHeightRepair => self.cliff_repair_base,
            mode => self.template_level.unwrap_or_else(|| {
                if mode.joining() || mode == TerrainMode::CliffPolyline {
                    terrain_tools::value(doc, scene, at).level
                } else if mode == TerrainMode::Bridge {
                    terrain_tools::value(doc, scene, at)
                        .level
                        .saturating_sub(if self.bridge_kind.high() { 4 } else { 0 })
                } else {
                    cliff_authoring::infer_base(doc, scene, at)
                }
            }),
        }
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
        let world = camera.world_rect(rect);
        let cell_center = |at: Cell, level: u8| {
            let (x, y) = vera20k_core::map::terrain::iso_to_screen(
                at[0],
                at[1],
                if self.flat_view { 0 } else { level },
            );
            rect.min + (egui::pos2(x + 30., y + 15.) - world.min) * camera.zoom
        };
        let cell_points = |at: Cell, level: u8| {
            let center = cell_center(at, level);
            rect.expand(60. * camera.zoom).contains(center).then(|| {
                let points = [[0., -15.], [30., 0.], [0., 15.], [-30., 0.]]
                    .map(|[x, y]| center + egui::vec2(x, y) * camera.zoom);
                (center, points)
            })
        };
        let brush_guide = |cell, color: Color32| {
            let level = terrain_tools::value(doc, scene, cell).level;
            if let Some((_, points)) = cell_points(cell, level) {
                painter.add(egui::Shape::convex_polygon(
                    points.to_vec(),
                    color.gamma_multiply(0.08),
                    egui::Stroke::new(1., color.gamma_multiply(0.6)),
                ));
            }
        };
        let valid_color = Color32::from_rgb(80, 220, 170);
        if self.plateau.gesture.is_none() {
            if self.terrain_mode == TerrainMode::ShoreRepair {
                for cell in ShoreBrush::footprint(doc, at, self.shore_brush_width) {
                    brush_guide(cell, valid_color);
                }
            }
            painter.text(
                rect.left_top() + egui::vec2(12., 12.),
                egui::Align2::LEFT_TOP,
                if self.picking_height {
                    tr!("点击崖顶或地面拾取高度；此次点击不会修改地图。")
                } else if self.two_point_cut_selected() {
                    tr!("第一点请点在已有悬崖或相连的高台顶面")
                } else if self.terrain_mode == TerrainMode::CliffPolyline {
                    tr!("点击添加折点；点击起点闭合并统一内部高度；Enter 生成已确认折线，Backspace 撤回折点，Esc 取消。")
                } else if self.terrain_mode == TerrainMode::WaterPolyline {
                    tr!("点击添加折点，点击起点或按 Enter 闭合填水。Backspace 撤回折点，Esc 取消。")
                } else if self.terrain_mode == TerrainMode::RoadPolyline {
                    tr!("点击或拖动添加路段。Enter 铺设路线，Backspace 撤回一段，Esc 取消。")
                } else {
                    crate::i18n::text(self.terrain_mode.name())
                },
                egui::FontId::proportional(13.),
                Color32::LIGHT_GREEN,
            );
            return;
        }
        let gesture = self.plateau.gesture.as_ref().unwrap();
        if at == gesture.start
            && gesture
                .cut
                .as_ref()
                .is_some_and(|draft| draft.is_current(doc, scene))
        {
            let surface = cell_center(at, terrain_tools::value(doc, scene, at).level);
            painter.circle_filled(surface, 3., valid_color);
            painter.text(
                rect.left_top() + egui::vec2(12., 12.),
                egui::Align2::LEFT_TOP,
                tr!("移动鼠标预览削切，点击第二点提交。Esc 或右键取消。"),
                egui::FontId::proportional(13.),
                Color32::LIGHT_GREEN,
            );
            return;
        }
        let starting_road = self.plateau.gesture.as_ref().is_some_and(|g| {
            g.mode == TerrainMode::RoadPolyline
                && g.path.len() == 1
                && g.road_endpoint(at) == g.start
        });
        let result = self.plateau.generate(doc, scene, at, self.cliff_cut);
        let error = result
            .as_ref()
            .err()
            .filter(|_| !starting_road)
            .map(|e| crate::i18n::text(e).to_owned());
        let plan = self.plateau.displayed_plan(result);
        let road_draft = self.plateau.road_draft_end().is_some();
        let guide_color = if error.is_none() {
            valid_color
        } else {
            Color32::LIGHT_RED
        };
        let color = if road_draft { valid_color } else { guide_color };
        if let Some(brush) = self
            .plateau
            .gesture
            .as_ref()
            .and_then(|g| g.shore_brush.as_ref())
        {
            for &cell in &brush.cells {
                brush_guide(cell, guide_color);
            }
        }
        let outline = |painter: &egui::Painter, at: Cell, level: u8| {
            let Some((center, points)) = cell_points(at, level) else {
                return;
            };
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
                    for &(at, value) in self.plateau.draw_order.edits(&plan) {
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
                } else if !self.flat_view {
                    // Filled water and restored ground are part of the ghost too.
                    let stamped: BTreeSet<_> =
                        plan.cliffs.iter().map(|s| (s.tile, s.origin)).collect();
                    for &(at, v) in self.plateau.draw_order.edits(&plan) {
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
                        let origin = egui::pos2(
                            (f32::from(at[0]) - f32::from(at[1])) * 30.,
                            f32::from(at[0] + at[1]) * 15. - f32::from(v.level) * 15.,
                        ) + egui::vec2(
                            image.info.offset[0] as f32,
                            image.info.offset[1] as f32,
                        );
                        let target = egui::Rect::from_min_size(
                            rect.min + (origin - world.min) * camera.zoom,
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
                }
                for (&at, v) in &plan.edits {
                    outline(painter, at, v.level);
                }
                if !self.flat_view && !self.framework {
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
                        if let Some(texture) = terrain_ui::template_texture(
                            &mut self.ui_state,
                            painter.ctx(),
                            scene,
                            key,
                        ) {
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
                }
                let mut overlays: Vec<_> = plan.overlays.iter().collect();
                overlays.sort_by_key(|(at, _)| (u32::from(at[0]) + u32::from(at[1]), at[0]));
                for (&at, &(id, frame)) in overlays {
                    let high = vera20k_core::rules::overlay_types::is_high_bridge_index(id);
                    let level = if high || error.is_some() {
                        plan.base
                    } else {
                        plan.edits
                            .get(&at)
                            .copied()
                            .unwrap_or_else(|| terrain_tools::value(doc, scene, at))
                            .level
                    };
                    let lift = vera20k_core::util::lepton::BRIDGE_DECK_HEIGHT_LEVELS as u8;
                    outline(painter, at, level + if high { lift } else { 0 });
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
                } else if g.mode == TerrainMode::ShoreRepair {
                    let adjusted = plan
                        .edits
                        .iter()
                        .filter(|(at, new)| {
                            use ra2_map_scene::shore_tools::wet;
                            wet(scene, **new) != wet(scene, terrain_tools::value(doc, scene, **at))
                        })
                        .count();
                    if adjusted == 0 {
                        tr!("保留水陆边界").into()
                    } else {
                        tr!("微调 {arg0} 格水陆边界", arg0 = adjusted)
                    }
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
                            if road_draft {
                                return e.clone();
                            }
                            tr!(
                                "预览 {arg0} 排桥面 · {e}；红色时松开不会放置，Esc 取消",
                                arg0 = plan.overlays.len() / 3,
                                e = e
                            )
                        })
                        .unwrap_or_else(|| {
                            if g.cut.is_some() {
                                tr!("削至 {arg0} 层 · {arg1} 格修改 · 点击第二点提交，Esc 取消", arg0 = self.cliff_cut.target.unwrap_or(plan.base), arg1 = plan.edits.len())
                            } else if g.mode == TerrainMode::CliffPolyline {
                                format!("{heights} · {}", tr!("点击添加折点；点击起点闭合并统一内部高度；Enter 生成已确认折线，Backspace 撤回折点，Esc 取消。"))
                            } else if g.mode == TerrainMode::WaterPolyline {
                                tr!("点击添加折点，点击起点或按 Enter 闭合填水。Backspace 撤回折点，Esc 取消。").into()
                            } else if g.mode == TerrainMode::RoadPolyline {
                                tr!("点击或拖动添加路段。Enter 铺设路线，Backspace 撤回一段，Esc 取消。").into()
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
                match g.mode {
                    _ if g.cut.is_some() => {}
                    TerrainMode::Plateau
                    | TerrainMode::CliffHeightRepair
                    | TerrainMode::PlateauCut
                    | TerrainMode::Water
                    | TerrainMode::LandFill
                    | TerrainMode::CoastalCliff => {
                        for cell in cliff_tools::rectangle(g.start, at) {
                            outline(painter, cell, g.base);
                        }
                    }
                    TerrainMode::WaterPolyline
                    | TerrainMode::RoadPolyline
                    | TerrainMode::ShoreRepair => {}
                    _ => {
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
                }
                painter.text(
                    rect.left_top() + egui::vec2(12., 12.),
                    egui::Align2::LEFT_TOP,
                    error.unwrap_or_else(|| {
                        if starting_road {
                            tr!(
                                "点击或拖动添加路段。Enter 铺设路线，Backspace 撤回一段，Esc 取消。"
                            )
                            .into()
                        } else {
                            tr!("无法生成预览").into()
                        }
                    }),
                    egui::FontId::proportional(13.),
                    guide_color,
                );
            }
        }
        if let Some(g) = self.plateau.gesture.as_ref()
            && g.mode == TerrainMode::Road
        {
            let point = |at: Cell| {
                let level = if self.flat_view {
                    0
                } else {
                    terrain_tools::value(doc, scene, at).level
                };
                let (x, y) = vera20k_core::map::terrain::iso_to_screen(at[0], at[1], level);
                rect.min + (egui::pos2(x + 30., y + 15.) - world.min) * camera.zoom
            };
            let end = if g.road_axis_lock {
                road_tools::axis_endpoint(g.start, at)
            } else {
                at
            };
            painter.extend(egui::Shape::dashed_line(
                &[point(g.start), point(end)],
                egui::Stroke::new(1.5, color),
                6.,
                4.,
            ));
        }
        if let Some(g) = self.plateau.gesture.as_ref()
            && click_polyline(g.mode)
        {
            let water = g.mode == TerrainMode::WaterPolyline;
            let road = g.mode == TerrainMode::RoadPolyline;
            let at = if road { g.road_endpoint(at) } else { at };
            let position = |at: Cell| {
                let (x, y) = vera20k_core::map::terrain::iso_to_screen(
                    at[0],
                    at[1],
                    if self.flat_view {
                        0
                    } else if water || road {
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
                        egui::Stroke::new(2., if road { guide_color } else { Color32::YELLOW }),
                        6.,
                        4.,
                    ));
                }
                if !water && !road && a.distance(b) > 12. {
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
        if let Some(g) = self.plateau.gesture.as_ref().filter(|g| g.cut.is_some()) {
            let start = cell_center(g.start, g.base + 4);
            let end = cell_center(at, g.base + 4);
            let guide = egui::Stroke::new(2., guide_color);
            for (cell, point) in [(g.start, start), (at, end)] {
                let surface = cell_center(cell, terrain_tools::value(doc, scene, cell).level);
                painter.circle_filled(surface, 3., guide_color);
                if surface != point {
                    painter.extend(egui::Shape::dashed_line(&[surface, point], guide, 4., 4.));
                }
            }
            let direction = end - start;
            if direction.length() > 0. {
                painter.arrow(start, direction, guide);
                let right = egui::vec2(-direction.y, direction.x).normalized();
                let cut_direction = if self.cliff_cut.side == CutSide::Left {
                    -right
                } else {
                    right
                };
                painter.arrow(start.lerp(end, 0.5), cut_direction * 20., guide);
            }
        }
    }

    pub(super) fn plateau_options(&mut self, ui: &mut egui::Ui, scene: &MapScene) {
        if self.terrain_mode == TerrainMode::PlateauCut {
            ui.add_enabled_ui(!self.stroking, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(tr!("切除方式"));
                    ui.selectable_value(
                        &mut self.plateau_cut_method,
                        PlateauCutMethod::Rectangle,
                        tr!("矩形拖选"),
                    );
                    ui.selectable_value(
                        &mut self.plateau_cut_method,
                        PlateauCutMethod::TwoPoint,
                        tr!("两点切线"),
                    );
                });
            });
        }
        if self.two_point_cut_selected() {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr!("削切方向"));
                ui.selectable_value(&mut self.cliff_cut.side, CutSide::Left, tr!("左侧"));
                ui.selectable_value(&mut self.cliff_cut.side, CutSide::Right, tr!("右侧"));
            });
            let base = self
                .plateau
                .gesture
                .as_ref()
                .and_then(|g| g.cut.as_ref())
                .map(|draft| draft.selection.base());
            let mut manual = self.cliff_cut.target.is_some();
            if ui.checkbox(&mut manual, tr!("指定削切高度")).changed() {
                self.cliff_cut.target = manual.then_some(base.unwrap_or(0));
            }
            if let Some(height) = &mut self.cliff_cut.target {
                let bounds = base.map_or(0..=cliff_tools::MAX_CLIFF_LEVEL - 1, |b| b..=b + 3);
                ui.add(
                    egui::Slider::new(height, bounds)
                        .smart_aim(false)
                        .text(tr!("目标高度")),
                );
            } else if let Some(base) = base {
                ui.small(tr!("自动削至崖脚 {arg0} 层", arg0 = base));
            } else {
                ui.small(tr!("默认削至崖脚"));
            }
            ui.small(tr!(
                "先点悬崖或高台顶面，再点第二点确定切线。左右以第一点看向第二点为准。"
            ));
            ui.small(tr!(
                "到崖脚时重建崖壁，中间高度用坡面衔接。Esc 或右键取消。"
            ));
            return;
        }
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
            if matches!(
                self.terrain_mode,
                TerrainMode::Road | TerrainMode::RoadPolyline | TerrainMode::RoadCut
            ) {
                let styles: Vec<_> = road_tools::supported_styles(scene).collect();
                let Some(&first) = styles.first() else {
                    ui.colored_label(Color32::LIGHT_RED, tr!("当前环境没有可自动绘制的道路素材"));
                    return;
                };
                if !styles.contains(&self.road_style) {
                    self.road_style = first;
                }
                let label = |style| match style {
                    RoadStyle::Paved => tr!("铺装公路（3 格宽）"),
                    RoadStyle::Dirt => tr!("土路（2 格宽）"),
                };
                ui.add_enabled_ui(self.plateau.road_draft_end().is_none(), |ui| {
                    egui::ComboBox::from_id_salt("road_style")
                        .selected_text(label(self.road_style))
                        .show_ui(ui, |ui| {
                            for style in styles {
                                ui.selectable_value(&mut self.road_style, style, label(style));
                            }
                        });
                });
            }
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
                    ui.add_enabled(
                        !self.stroking,
                        egui::Slider::new(&mut self.shore_brush_width, 1..=21)
                            .step_by(2.)
                            .text(tr!("画笔宽度（格）")),
                    );
                    tr!(
                        "按住左键沿岸线刷过要修复的区域，松开应用，Esc 取消。自动补齐岸线和沙滩过渡。单格锯齿无法拼接时，可在刷区内按 2×2 格微调水陆边界。"
                    )
                }
                TerrainMode::CoastalCliff => {
                    tr!(
                        "框选已有正面悬崖；按低地是否临水，切换水悬崖或陆地悬崖。背面方向保持原样。"
                    )
                }
                TerrainMode::Road => {
                    tr!("拖出一条线段，松开生成道路。斜线自动拼接弯道，按住 Shift 锁定轴向。")
                }
                TerrainMode::RoadPolyline => {
                    tr!("点击或拖动添加路段。Enter 铺设路线，Backspace 撤回一段，Esc 取消。")
                }
                TerrainMode::RoadCut => tr!("切除所选类型的平路，并修复相连路段的端点和路口。"),
                _ => unreachable!("joining mode without sidebar instructions"),
            };
            ui.small(text);
            if self.terrain_mode == TerrainMode::WaterPolyline {
                ui.small(tr!(
                    "自动连接末点与起点，保留凹形轮廓。岸线无法拼接时按 2×2 格向外规整。"
                ));
            }
            if self.terrain_mode == TerrainMode::RoadPolyline {
                ui.small(tr!("按住 Shift 锁定下一段的轴向。"));
                let ready = self
                    .plateau
                    .gesture
                    .as_ref()
                    .is_some_and(|g| g.path.len() >= 2);
                if ui
                    .add_enabled(ready, egui::Button::new(tr!("铺设路线")))
                    .clicked()
                {
                    self.finish_plateau(scene);
                }
            }
            if matches!(
                self.terrain_mode,
                TerrainMode::Road | TerrainMode::RoadPolyline
            ) {
                ui.small(tr!(
                    "接入同类型道路可生成转角和路口。道路沿匹配的一层直坡升降，坡道两端需接到平地。"
                ));
            } else if !matches!(
                self.terrain_mode,
                TerrainMode::RoadCut | TerrainMode::CoastalCliff
            ) {
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
