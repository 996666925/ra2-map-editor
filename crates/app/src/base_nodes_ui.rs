//! Base-plan overlay and a canvas gesture; a drag commits once on release.
use crate::{camera::MapCamera, document::Cell, editing::Editor, map::MapScene};
use eframe::egui::{self, Color32};
use ra2_map_document::{
    base_nodes::{self, Node},
    mission::{self, Kind},
};

pub(crate) struct BaseNodesUi {
    pub active: bool,
    house: String,
    building: String,
    selected: Option<usize>,
    drag: Option<Drag>,
    message: String,
    key: Option<(u64, u64)>,
    marks: Vec<(String, usize, Node)>,
}
struct Drag {
    key: (u64, u64),
    index: Option<usize>,
    origin: Cell,
    start: Cell,
    at: Option<Cell>,
}
impl Default for BaseNodesUi {
    fn default() -> Self {
        Self {
            active: false,
            house: String::new(),
            building: "GAPOWR".into(),
            selected: None,
            drag: None,
            message: String::new(),
            key: None,
            marks: Vec::new(),
        }
    }
}
impl BaseNodesUi {
    pub fn start(&mut self, house: String) {
        self.active = true;
        self.house = house;
        self.selected = None;
        self.drag = None;
        self.message.clear();
    }
    pub fn stop(&mut self) {
        self.active = false;
        self.drag = None;
    }
    pub fn cancel_drag(&mut self) -> bool {
        let cancelled = self.drag.take().is_some();
        if cancelled {
            self.message = "已取消节点拖动。".into();
        }
        cancelled
    }
    fn finish(
        &mut self,
        result: anyhow::Result<()>,
        editor: &mut Editor,
        scene: &MapScene,
        name: &str,
    ) {
        match result {
            Ok(()) => {
                editor.commit(scene, name);
                self.message = format!("{name}，可撤销。");
            }
            Err(e) => self.message = format!("{e:#}"),
        }
    }
    pub fn panel(&mut self, ctx: &egui::Context, editor: &mut Editor, scene: &MapScene) {
        crate::ui_theme::window(
            ctx,
            "基地节点图形编辑",
            "base-nodes-canvas",
            egui::vec2(450., 520.),
        )
        .default_pos(ctx.content_rect().min + egui::vec2(16., 80.))
        .show(ctx, |ui| {
            let Some(doc) = editor.doc.as_ref() else {
                self.stop();
                return;
            };
            if !mission::ids(doc, Kind::House).contains(&self.house) {
                self.stop();
                return;
            }
            ui.label(format!("所属方：{}", self.house));
            ui.label("点击空格创建节点；拖动节点移动。Esc 取消本次拖动，再按退出。中键/右键平移。");
            egui::ComboBox::from_id_salt("base-building")
                .selected_text(scene.catalog.object_label(&self.building))
                .show_ui(ui, |ui| {
                    if let Some(types) = scene.catalog.object_types.get("Structures") {
                        for id in types {
                            ui.selectable_value(&mut self.building, id.clone(), scene.catalog.object_label(id));
                        }
                    }
                });
            let nodes = match base_nodes::read(doc, &self.house) {
                Ok(nodes) => nodes,
                Err(e) => {
                    ui.colored_label(Color32::YELLOW, format!("{e:#}"));
                    if ui.button("返回任务面板").clicked() {
                        self.stop();
                    }
                    return;
                }
            };
            if self.selected.is_some_and(|i| i >= nodes.len()) {
                self.selected = None;
            }
            ui.add_enabled_ui(self.drag.is_none(), |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("撤销").clicked() {
                        editor.history(false, scene);
                        self.selected = None;
                    }
                    if ui.button("重做").clicked() {
                        editor.history(true, scene);
                        self.selected = None;
                    }
                    if ui.button("从本方建筑补齐节点").clicked() {
                        let result = base_nodes::import_structures(
                            editor.doc.as_mut().unwrap(),
                            &self.house,
                        )
                        .map(|_| ());
                        self.finish(result, editor, scene, "补齐基地节点");
                    }
                });
                let mut percent = editor
                    .doc
                    .as_ref()
                    .unwrap()
                    .ini_text(&self.house, "PercentBuilt")
                    .parse::<i32>()
                    .unwrap_or(0);
                ui.horizontal(|ui| {
                    ui.label(crate::labels::field("PercentBuilt")).on_hover_text("PercentBuilt");
                    if crate::form_edit::number(ui,
                            egui::DragValue::new(&mut percent)
                                .range(0..=100)
                                .suffix(" %"),
                            "初始建成比例")
                        .changed()
                    {
                        let result = editor.doc.as_mut().unwrap().set_ini(
                            &self.house,
                            "PercentBuilt",
                            Some(percent.to_string().into_bytes()),
                        );
                        self.finish(result, editor, scene, "修改基地初始建成比例");
                    }
                });
                ui.small(
                    "节点只规划 AI 建造位置，不直接放置建筑；建造仍需施工场、资金和 AI 生产启用。",
                );
                egui::ScrollArea::vertical()
                    .id_salt("base-plan-list")
                    .max_height(190.)
                    .show(ui, |ui| {
                        for (i, node) in nodes.iter().enumerate() {
                            if ui
                                .selectable_label(
                                    self.selected == Some(i),
                                    format!(
                                        "{i:03} · {} · {},{}",
                                        scene.catalog.object_label(&node.building), node.cell[0], node.cell[1]
                                    ),
                                )
                                .clicked()
                            {
                                self.selected = Some(i);
                            }
                            if !node.control()
                                && !scene.catalog.object_types.get("Structures").is_some_and(
                                    |ids| {
                                        ids.iter().any(|id| id.eq_ignore_ascii_case(&node.building))
                                    },
                                )
                            {
                                ui.colored_label(
                                    Color32::LIGHT_RED,
                                    "此建筑类型未在当前规则中注册",
                                );
                            }
                        }
                    });
                if let Some(i) = self.selected.filter(|&i| i < nodes.len()) {
                    ui.horizontal(|ui| {
                        for (label, to, enabled) in [
                            ("前移", i.saturating_sub(1), i > 0),
                            ("后移", i + 1, i + 1 < nodes.len()),
                        ] {
                            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                                let result = base_nodes::reorder(
                                    editor.doc.as_mut().unwrap(),
                                    &self.house,
                                    i,
                                    to,
                                );
                                if result.is_ok() {
                                    self.selected = Some(to);
                                }
                                self.finish(result, editor, scene, "调整基地节点顺序");
                            }
                        }
                        if ui.button("删除节点").clicked() {
                            let result =
                                base_nodes::remove(editor.doc.as_mut().unwrap(), &self.house, i);
                            if result.is_ok() {
                                self.selected = None;
                            }
                            self.finish(result, editor, scene, "删除基地节点");
                        }
                    });
                    let doc = editor.doc.as_ref().unwrap();
                    let ids = base_nodes::matching_structures(doc, &self.house, &nodes[i]);
                    if ids.len() == 1 {
                        let mut rebuild = doc.records()[&ids[0]]
                            .fields()
                            .get(8)
                            .is_some_and(|v| v == "1");
                        if ui.checkbox(&mut rebuild, "对应现有建筑：AI 重建")
                            .on_hover_text("保存建筑记录中的标志；实际重建还取决于基地计划和所属方的生产条件。")
                            .changed() {
                            let result = base_nodes::set_rebuild(
                                editor.doc.as_mut().unwrap(),
                                &self.house,
                                i,
                                rebuild,
                            );
                            self.finish(result, editor, scene, "修改建筑 AI 重建");
                        }
                    } else {
                        ui.small("此节点没有唯一匹配的现有建筑；移动/删除节点不会移动/删除建筑。");
                    }
                }
            });
            if !self.message.is_empty() {
                ui.label(&self.message);
            }
            if ui.button("返回任务面板").clicked() {
                self.stop();
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn canvas(
        &mut self,
        painter: &egui::Painter,
        editor: &mut Editor,
        scene: &MapScene,
        rect: egui::Rect,
        camera: &MapCamera,
        pointer: Option<egui::Pos2>,
        cell: Option<Cell>,
        press: bool,
        down: bool,
        release: bool,
        enabled: bool,
    ) {
        let Some(doc) = editor.doc.as_ref() else {
            self.stop();
            return;
        };
        let key = (doc.id, doc.state_id);
        if self.key != Some(key) {
            if self.key.is_some_and(|old| old.0 != key.0) {
                self.stop();
            }
            self.marks.clear();
            for house in mission::ids(doc, Kind::House) {
                if let Ok(nodes) = base_nodes::read(doc, &house) {
                    self.marks.extend(
                        nodes
                            .into_iter()
                            .enumerate()
                            .map(|(i, n)| (house.clone(), i, n)),
                    );
                }
            }
            self.key = Some(key);
        }
        if !editor.base_nodes_visible && !self.active {
            return;
        }
        let world = camera.world_rect(rect);
        let project = |at: Cell| {
            let level = doc.terrain(at).map_or(0, |t| t.level);
            let (x, y) = vera20k_core::map::terrain::iso_to_screen(at[0], at[1], level);
            rect.min + (egui::pos2(x + 30., y + 15.) - world.min) * camera.zoom
        };
        let mut hit = None;
        let mut distance = f32::INFINITY;
        for (house, i, node) in &self.marks {
            if self.active && *house != self.house || node.cell == [0, 0] {
                continue;
            }
            let pos = project(node.cell);
            if !rect.expand(30.).contains(pos) {
                continue;
            }
            let chosen = self.active && self.selected == Some(*i);
            let color = if chosen {
                Color32::WHITE
            } else {
                Color32::from_rgb(255, 170, 60)
            };
            let r = egui::Rect::from_center_size(pos, egui::vec2(15., 15.));
            painter.rect_stroke(
                r,
                0.,
                egui::Stroke::new(2., color),
                egui::StrokeKind::Outside,
            );
            if camera.zoom >= 0.35 || chosen {
                painter.text(
                    pos + egui::vec2(12., 8.),
                    egui::Align2::LEFT_TOP,
                    format!("{house} {i:03} {}", node.building),
                    egui::FontId::proportional(11.),
                    color,
                );
            }
            if let Some(p) = pointer {
                let d = p.distance(pos);
                if d < 14. && d < distance {
                    hit = Some((*i, node.cell));
                    distance = d;
                }
            }
        }
        if !self.active {
            return;
        }
        if painter.ctx().input(|i| i.key_pressed(egui::Key::Escape))
            && !crate::form_edit::popup_open(painter.ctx())
            && !painter.ctx().text_edit_focused()
        {
            if !self.cancel_drag() {
                self.stop();
            }
            return;
        }
        if self.drag.as_ref().is_some_and(|d| d.key != key) {
            self.drag = None;
            self.message = "文档已变化，已取消节点拖动。".into();
            return;
        }
        if enabled
            && press
            && let Some(at) = cell
        {
            self.selected = hit.map(|(i, _)| i);
            self.drag = Some(Drag {
                key,
                index: self.selected,
                origin: hit.map_or(at, |(_, c)| c),
                start: at,
                at: Some(hit.map_or(at, |(_, c)| c)),
            });
        }
        if let Some(drag) = &mut self.drag {
            if enabled && (down || release) {
                drag.at = cell.and_then(|at| {
                    let x = i32::from(drag.origin[0]) + i32::from(at[0]) - i32::from(drag.start[0]);
                    let y = i32::from(drag.origin[1]) + i32::from(at[1]) - i32::from(drag.start[1]);
                    let moved = [u16::try_from(x).ok()?, u16::try_from(y).ok()?];
                    doc.terrain(moved).map(|_| moved)
                });
            }
            if let Some(at) = drag.at {
                painter.circle_stroke(project(at), 12., egui::Stroke::new(2., Color32::YELLOW));
            }
        }
        if (release || !down)
            && let Some(drag) = self.drag.take()
        {
            if !enabled || drag.at.is_none() {
                self.message = "未在有效地图格释放，节点未修改。".into();
                return;
            }
            let at = drag.at.unwrap();
            let result = if let Some(i) = drag.index {
                base_nodes::move_node(editor.doc.as_mut().unwrap(), &self.house, i, at)
            } else if scene
                .catalog
                .object_types
                .get("Structures")
                .is_some_and(|ids| ids.contains(&self.building))
            {
                base_nodes::append(
                    editor.doc.as_mut().unwrap(),
                    &self.house,
                    &self.building,
                    at,
                )
                .map(|i| self.selected = Some(i))
            } else {
                Err(anyhow::anyhow!("请选择已注册的建筑类型"))
            };
            self.finish(result, editor, scene, "编辑基地节点");
        }
    }
}
