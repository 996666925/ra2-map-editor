use crate::{
    map::MapScene,
    map_loader::{LoadTimings, MapCreation, MapLoader},
    terrain_gpu::{Target, TerrainGpu},
};
use anyhow::{Context, Result, ensure};
use eframe::{egui, egui_wgpu};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
#[path = "p2_smoke.rs"]
mod p2_smoke;
#[path = "p3_smoke.rs"]
mod p3_smoke;
#[path = "random_map_ui.rs"]
mod random_map_ui;
#[path = "ui_smoke.rs"]
mod ui_smoke;
#[path = "workbench.rs"]
mod workbench;

pub struct MapView {
    workbench: workbench::Workbench,
    p2: p2_smoke::P2Smoke,
    ui_smoke: ui_smoke::UiSmoke,
    pub p2_playtest: bool,
    pub editor: crate::editing::Editor,
    pending: Option<Leave>,
    leave_after_save: bool,
    allow_close: bool,
    loader: MapLoader,
    generation: u64,
    pub loading: bool,
    pub error: Option<String>,
    pub name: String,
    pub scene: Option<MapScene>,
    gpu: Option<Arc<TerrainGpu>>,
    target: Option<Arc<Target>>,
    zoom: f32,
    center: egui::Pos2,
    fit: bool,
    selected: Option<usize>,
    hover: Option<usize>,
    status: String,
    verified: bool,
    screenshot_requested: bool,
    started: Instant,
    stage: &'static str,
    timings: LoadTimings,
    loading_ticks: usize,
    last_loading_tick: Option<Instant>,
    max_loading_gap_ms: f64,
    max_poll_ms: f64,
    loading_ui_frames: usize,
    directory: PathBuf,
    smoke_loads: Vec<serde_json::Value>,
}
enum Leave {
    Open(PathBuf, String),
    New(PathBuf, MapCreation),
    Close,
    Restore(PathBuf),
}
impl MapView {
    pub fn close_smoke(&mut self, ctx: &egui::Context) {
        self.allow_close = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    pub fn new(
        ctx: egui::Context,
        state: egui_wgpu::RenderState,
        data_root: PathBuf,
    ) -> Result<Self> {
        Ok(Self {
            workbench: Default::default(),
            p2: Default::default(),
            ui_smoke: Default::default(),
            p2_playtest: false,
            editor: crate::editing::Editor::new(ctx.clone(), data_root)?,
            pending: None,
            leave_after_save: false,
            allow_close: false,
            loader: MapLoader::with_gpu(ctx, state)?,
            generation: 0,
            loading: false,
            error: None,
            name: String::new(),
            scene: None,
            gpu: None,
            target: None,
            zoom: 1.,
            center: egui::Pos2::ZERO,
            fit: true,
            selected: None,
            hover: None,
            status: String::new(),
            verified: false,
            screenshot_requested: false,
            started: Instant::now(),
            stage: "",
            timings: Default::default(),
            loading_ticks: 0,
            last_loading_tick: None,
            max_loading_gap_ms: 0.,
            max_poll_ms: 0.,
            loading_ui_frames: 0,
            directory: PathBuf::new(),
            smoke_loads: Vec::new(),
        })
    }
    pub fn open(&mut self, directory: PathBuf, name: String) {
        if !self.workbench.map_settings.allow_leave()
            || !self.workbench.mission.allow_leave()
            || !self.editor.check_property_draft()
        {
            return;
        }
        if self.editor.stroking
            && let Some(scene) = &self.scene
        {
            self.editor.pointer(scene, None, false, false, true, None);
        }
        if self.editor.dirty() || self.editor.saving {
            self.pending = Some(Leave::Open(directory, name));
            self.leave_after_save = false;
            return;
        }
        self.open_now(directory, name);
    }
    pub(crate) fn refresh_resources(&mut self, directory: PathBuf) {
        if self.loading
            || self.editor.stroking
            || !self.workbench.map_settings.allow_leave()
            || !self.workbench.mission.allow_leave()
            || !self.editor.check_property_draft()
        {
            return;
        }
        if self.editor.doc.is_none() {
            self.status = "请先打开或新建地图。".into();
            return;
        }
        self.loader.invalidate_resources();
        self.directory = directory.clone();
        self.editor.directory = directory;
        self.editor.structural_refresh = true;
        self.editor.refresh = true;
        self.status = "正在刷新资源；地图修改与历史保留。".into();
    }
    pub fn create(&mut self, directory: PathBuf, options: crate::new_map::NewMapOptions) {
        self.create_map(directory, MapCreation::Blank(options));
    }
    fn create_map(&mut self, directory: PathBuf, options: MapCreation) {
        if !self.workbench.map_settings.allow_leave()
            || !self.workbench.mission.allow_leave()
            || !self.editor.check_property_draft()
        {
            return;
        }
        if let Err(e) = options.validate() {
            self.status = e.to_string();
            return;
        }
        if self.editor.stroking
            && let Some(scene) = &self.scene
        {
            self.editor.pointer(scene, None, false, false, true, None);
        }
        if self.editor.dirty() || self.editor.saving {
            self.pending = Some(Leave::New(directory, options));
            self.leave_after_save = false;
            return;
        }
        self.load_now(directory, "Untitled.map".into(), Some(options));
    }
    fn open_now(&mut self, directory: PathBuf, name: String) {
        self.load_now(directory, name, None);
    }
    fn load_now(&mut self, directory: PathBuf, name: String, new_map: Option<MapCreation>) {
        self.workbench.canvas_pan = false;
        self.workbench.cancel_secondary = false;
        self.editor.doc = None;
        self.editor.refresh = false;
        self.editor.directory = directory.clone();
        self.generation += 1;
        self.loading = true;
        self.error = None;
        self.scene = None;
        self.gpu = None;
        self.target = None;
        self.selected = None;
        self.hover = None;
        self.status.clear();
        self.name = name;
        self.verified = false;
        self.screenshot_requested = false;
        self.started = Instant::now();
        self.stage = "等待读取地图…";
        self.timings = Default::default();
        self.loading_ticks = 0;
        self.last_loading_tick = None;
        self.max_loading_gap_ms = 0.;
        self.max_poll_ms = 0.;
        self.loading_ui_frames = 0;
        self.directory = directory.clone();
        self.smoke_loads.clear();
        let request = if let Some(options) = new_map {
            self.loader.create(self.generation, directory, options)
        } else {
            self.loader
                .request(self.generation, directory, self.name.clone())
        };
        if let Err(e) = request {
            self.loading = false;
            self.error = Some(format!("{e:#}"));
        }
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        if self.loading
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.generation += 1;
            self.loader.cancel(self.generation);
            self.loading = false;
            self.stage = "已取消加载";
            self.status = "已取消；编辑数据仍保留，可刷新资源重新加载预览。".into();
        }
        self.editor.poll(ctx);
        if let Some(scene) = &self.scene {
            self.workbench
                .map_settings
                .finish_lighting(ctx, &mut self.editor, scene);
            self.editor.finish_property_edit(ctx, scene);
            self.workbench
                .mission
                .finish_edit(ctx, &mut self.editor, scene);
        }
        if ctx.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::WindowFocused(false)))
        }) {
            if let Some(scene) = &self.scene {
                self.editor.cancel_active(scene);
            }
            self.workbench.canvas_pan = false;
            self.workbench.mission.base.cancel_drag();
        }
        if let Some((base, meta, path)) = self.editor.restored.take() {
            self.directory = meta.directory.clone();
            self.name = meta.name.clone();
            self.editor.restore(base, meta, path);
            self.scene = None;
            self.gpu = None;
            self.target = None;
            self.selected = None;
            self.hover = None;
            self.fit = true;
            self.error = None;
        }
        let mission_can_leave = !ctx.input(|i| i.viewport().close_requested())
            || self.allow_close
            || self.workbench.map_settings.allow_leave()
                && self.workbench.mission.allow_leave()
                && self.editor.check_property_draft();
        if !mission_can_leave {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && mission_can_leave
            && (self.editor.dirty() || self.editor.saving)
        {
            if self.editor.stroking
                && let Some(scene) = &self.scene
            {
                self.editor.pointer(scene, None, false, false, true, None);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.pending.is_none() {
                self.pending = Some(Leave::Close);
                self.leave_after_save = false;
            }
        }
        if self.editor.saved {
            self.editor.saved = false;
            if let Some(path) = &self.editor.target {
                self.name = path.display().to_string();
            }
            if self.leave_after_save && !self.editor.dirty() {
                self.leave_after_save = false;
                if let Some(action) = self.pending.take() {
                    self.leave(action, ctx);
                }
            }
        }
        if self.editor.refresh
            && !self.editor.stroking
            && let Some(doc) = &self.editor.doc
        {
            self.editor.refresh = false;
            self.generation += 1;
            self.loading = true;
            self.stage = "更新编辑预览…";
            self.error = None;
            // Keep drawing the published scene while the worker prepares its
            // replacement. The worker never mutates callback-bound buffers.
            let preview = self.workbench.map_settings.preview_snapshot(doc);
            let request = preview.and_then(|(snapshot, profile)| {
                self.loader.preview_profile(
                    self.generation,
                    self.directory.clone(),
                    self.name.clone(),
                    snapshot,
                    profile,
                )
            });
            if let Err(e) = request {
                self.loading = false;
                self.error = Some(format!("{e:#}"));
            }
        }
        let started = Instant::now();
        if self.loading {
            self.loading_ticks += 1;
            if let Some(last) = self.last_loading_tick.replace(started) {
                self.max_loading_gap_ms = self
                    .max_loading_gap_ms
                    .max(last.elapsed().as_secs_f64() * 1000.);
            }
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        for progress in self.loader.progress.try_iter() {
            if progress.generation == self.generation {
                self.stage = progress.stage;
            }
        }
        while let Ok(message) = self.loader.results.try_recv() {
            if message.generation != self.generation {
                continue;
            }
            self.loading = false;
            if let Some(identity) = message.preview
                && self
                    .editor
                    .doc
                    .as_ref()
                    .is_none_or(|doc| (doc.id, doc.state_id) != identity)
            {
                self.editor.refresh = true;
                continue;
            }
            self.timings = message.timings;
            match message.result {
                Ok(scene) => {
                    if self.scene.as_ref().is_some_and(|old| {
                        old.source.header.width != scene.source.header.width
                            || old.source.header.height != scene.source.header.height
                    }) {
                        self.selected = None;
                        self.hover = None;
                        self.center = scene.bounds.center();
                        self.fit = true;
                    }
                    self.reset_overview();
                    match message.gpu {
                        Some(Ok(gpu)) => {
                            // Targets bind the scene's atlases and instance buffers,
                            // not just its viewport size. Rebind after every load,
                            // edit, undo or redo, even when the canvas stays the same size.
                            self.target = None;
                            self.gpu = Some(gpu);
                        }
                        Some(Err(e)) => {
                            self.error = Some(format!("地形上传失败；原文仍可导出：{e:#}"));
                            continue;
                        }
                        None => {
                            self.target = None;
                            self.gpu = None;
                        }
                    }
                    if message.preview.is_none() {
                        self.editor.attach(&scene);
                        if message.created {
                            self.editor.mark_new();
                            if let Ok(ini) = vera20k_core::rules::ini_parser::IniFile::from_bytes(
                                scene.source.bytes(),
                            ) && let Some(random) = ini.section("RandomMap")
                            {
                                self.editor.status = format!(
                                    "已生成随机地图，种子 {}；Ctrl+S 选择保存位置。",
                                    random.read_int("Seed", 0)
                                );
                            }
                        }
                        self.center = scene.bounds.center();
                        self.fit = true;
                    } else {
                        if !self.editor.stroking {
                            self.editor.feedback.clear();
                        }
                        if let Some(doc) = &mut self.editor.doc {
                            doc.resource_profile = Some(scene.revision.id());
                        }
                        self.editor.cache(&scene);
                    }
                    self.editor.structural_refresh = false;
                    self.scene = Some(scene);
                }
                Err(e) => self.error = Some(format!("{e:#}")),
            }
        }
        self.max_poll_ms = self
            .max_poll_ms
            .max(started.elapsed().as_secs_f64() * 1000.);
    }
    pub fn canvas(&mut self, ui: &mut egui::Ui, state: &egui_wgpu::RenderState) {
        use crate::ui_theme as theme;
        if self.loading {
            self.loading_ui_frames += 1;
        }
        let Some(scene) = &self.scene else {
            let rect = ui.available_rect_before_wrap();
            let p = ui.painter();
            for x in (0..rect.width() as usize).step_by(28) {
                for y in (0..rect.height() as usize).step_by(28) {
                    p.circle_filled(
                        rect.min + egui::vec2(x as f32, y as f32),
                        0.65,
                        theme::BORDER.gamma_multiply(0.6),
                    );
                }
            }
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(egui::Rect::from_center_size(
                        rect.center(),
                        egui::vec2(320., 250.),
                    ))
                    .layout(egui::Layout::top_down(egui::Align::Center)),
                |ui| {
                    ui.add_space(20.);
                    let (icon, _) =
                        ui.allocate_exact_size(egui::vec2(52., 52.), egui::Sense::hover());
                    ui.painter().rect_filled(icon, 12, theme::SURFACE);
                    theme::icon(ui.painter(), icon, theme::Icon::Terrain, theme::ACCENT);
                    ui.add_space(14.);
                    ui.label(
                        egui::RichText::new(if self.loading {
                            "正在准备地图"
                        } else {
                            "开始编辑你的战场"
                        })
                        .size(21.)
                        .strong(),
                    );
                    ui.label(
                        egui::RichText::new(if self.loading {
                            self.stage
                        } else {
                            "新建空白地图，或打开现有地图继续编辑。"
                        })
                        .color(theme::MUTED),
                    );
                    ui.add_space(12.);
                    if self.loading {
                        ui.spinner();
                    } else {
                        if ui.button("新建地图…").clicked() {
                            self.show_new_map();
                        }
                        if ui.button("新建随机地图…").clicked() {
                            self.show_random_map();
                        }
                        if ui.button("选择资源与地图…").clicked() {
                            self.show_project_settings();
                        }
                    }
                    ui.small("也可以将 .map / .yrm 文件拖到窗口中");
                },
            );
            return;
        };
        let editing = !self.editing_blocked();
        if editing {
            if self.workbench.mission.has_draft()
                && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::S))
            {
                self.workbench.mission.allow_leave();
            } else {
                self.editor.shortcuts(ui.ctx(), scene);
            }
        }
        if scene.images.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("当前地图暂无可预览的地形，请查看左侧诊断；可继续查看地图头并导出原文。");
            });
            return;
        }
        egui::Frame::new()
            .fill(theme::BACKGROUND)
            .inner_margin(egui::Margin::symmetric(12, 7))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    theme::badge(ui, &scene.source.header.theater, theme::BLUE);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(if scene.source.title.is_empty() {
                                "地图视图"
                            } else {
                                &scene.source.title
                            })
                            .size(12.)
                            .strong(),
                        )
                        .truncate(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("适应地图").clicked() {
                            self.fit = true;
                        }
                        if ui.button("1:1").clicked() {
                            self.zoom = 1. / ui.ctx().pixels_per_point();
                            self.fit = false;
                        }
                        ui.label(
                            egui::RichText::new(format!(
                                "{:.0}%",
                                self.zoom * ui.ctx().pixels_per_point() * 100.
                            ))
                            .monospace()
                            .size(11.)
                            .color(theme::MUTED),
                        );
                        if self.loading {
                            ui.spinner();
                            ui.label(self.stage);
                        }
                    });
                });
            });
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(egui::vec2(1., 1.)),
            egui::Sense::click_and_drag(),
        );
        self.workbench.canvas_extent = Some(rect.size());
        self.workbench.canvas_rect = Some(rect);
        let keyboard =
            editing && !ui.ctx().text_edit_focused() && !egui::Popup::is_any_open(ui.ctx());
        if keyboard && (response.hovered() || response.has_focus()) {
            ui.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Home) {
                    self.fit = true;
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Num1) {
                    self.zoom = 1. / ui.ctx().pixels_per_point();
                    self.fit = false;
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::F) {
                    let at = self
                        .editor
                        .selected
                        .and_then(|id| self.editor.records.get(&id))
                        .and_then(|r| r.cell());
                    let cell = at
                        .and_then(|at| scene.cells.iter().find(|c| [c.rx, c.ry] == at))
                        .or_else(|| self.selected.and_then(|n| scene.cells.get(n)));
                    if let Some(c) = cell {
                        self.center = egui::pos2(c.screen_x + 30., c.screen_y + 15.);
                        self.fit = false;
                    }
                }
            });
        }
        let (
            primary_press,
            primary_down,
            primary_release,
            secondary_press,
            secondary_release,
            space,
        ) = ui.input(|i| {
            (
                i.pointer.button_pressed(egui::PointerButton::Primary),
                i.pointer.button_down(egui::PointerButton::Primary),
                i.pointer.button_released(egui::PointerButton::Primary),
                i.pointer.button_pressed(egui::PointerButton::Secondary),
                i.pointer.button_released(egui::PointerButton::Secondary),
                i.key_down(egui::Key::Space),
            )
        });
        if primary_press && response.contains_pointer() {
            response.request_focus();
            if keyboard && space && !self.editor.stroking {
                self.workbench.canvas_pan = true;
            }
        }
        let panning = self.workbench.canvas_pan;
        if secondary_press
            && response.contains_pointer()
            && editing
            && (self.editor.stroking
                || matches!(
                    self.editor.tool,
                    crate::editing::Tool::Paste
                        | crate::editing::Tool::RegionMove
                        | crate::editing::Tool::Clone
                        | crate::editing::Tool::Move
                        | crate::editing::Tool::Place
                        | crate::editing::Tool::Pick
                ))
        {
            self.editor.cancel_operation(scene);
            self.workbench.cancel_secondary = true;
        }
        if self.fit
            || (response.double_clicked()
                && self.editor.tool == crate::editing::Tool::Select
                && !self.workbench.mission.picking())
        {
            self.center = scene.bounds.center();
            self.zoom = ((rect.width() - 30.) / scene.bounds.width())
                .min((rect.height() - 30.) / scene.bounds.height())
                .clamp(0.01, 8.);
            self.fit = false;
        }
        if response.dragged_by(egui::PointerButton::Middle)
            || response.dragged_by(egui::PointerButton::Secondary)
            || panning && response.dragged_by(egui::PointerButton::Primary)
        {
            self.center -= response.drag_delta() / self.zoom;
        }
        if response.hovered() {
            let delta = ui.input(|i| i.smooth_scroll_delta.y);
            let next = (self.zoom * (delta * 0.003).exp()).clamp(0.01, 8.);
            if let Some(pointer) = response.hover_pos() {
                let mut camera = crate::camera::MapCamera {
                    center: self.center,
                    zoom: self.zoom,
                };
                camera.zoom_at(rect, pointer, next);
                self.center = camera.center;
            }
            self.zoom = next;
        }
        let camera = crate::camera::MapCamera {
            center: self.center,
            zoom: self.zoom,
        };
        let world = camera.world_rect(rect);
        let size = [
            (rect.width() * ui.ctx().pixels_per_point()).round().max(1.) as u32,
            (rect.height() * ui.ctx().pixels_per_point())
                .round()
                .max(1.) as u32,
        ];
        if let Some(gpu) = &self.gpu {
            if self.target.as_ref().is_none_or(|t| t.size != size) {
                self.target = Some(gpu.target(&state.device, size));
            }
            ui.painter().add(gpu.callback(
                rect,
                self.target.as_ref().unwrap().clone(),
                world,
                self.editor.view_options(),
            ));
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(if panning {
                egui::CursorIcon::Grabbing
            } else if space && keyboard {
                egui::CursorIcon::Grab
            } else if self.editor.tool == crate::editing::Tool::Select {
                egui::CursorIcon::Default
            } else {
                egui::CursorIcon::Crosshair
            });
        }
        self.hover = response
            .hover_pos()
            .and_then(|p| scene.pick(camera.world_point(rect, p)));
        if response.clicked() && !panning {
            self.selected = self.hover;
        }
        let painter = ui.painter().with_clip_rect(rect);
        let at = self.hover.map(|i| [scene.cells[i].rx, scene.cells[i].ry]);
        self.editor
            .terrain_preview(&painter, scene, rect, &camera, at);
        let marker = self
            .editor
            .markers(&painter, scene, rect, &camera, response.hover_pos());
        let task_hit = if self.editor.task_labels {
            self.editor.doc.as_ref().and_then(|doc| {
                self.workbench.task_overlay.draw(
                    &painter,
                    doc,
                    rect,
                    &camera,
                    response.hover_pos(),
                    self.editor.layers[1],
                )
            })
        } else {
            None
        };
        if task_hit.is_some() {
            response
                .clone()
                .on_hover_text("Ctrl+单击任务标记，打开对应标签或格子触发点。");
        }
        let task_navigation = !self.workbench.mission.picking()
            && editing
            && !self.editor.stroking
            && task_hit.is_some()
            && response.contains_pointer()
            && ui.input(|i| i.modifiers.ctrl);
        if task_navigation
            && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary))
            && let Some((kind, id)) = task_hit
        {
            self.workbench.mission.navigate(&self.editor, kind, id);
        }
        let (press, down, release) = (primary_press, primary_down, primary_release);
        let parameter_pick = self.workbench.mission.picking();
        self.workbench.mission.base.canvas(
            &painter,
            &mut self.editor,
            scene,
            rect,
            &camera,
            response.hover_pos(),
            at,
            press && response.contains_pointer(),
            down,
            release,
            self.workbench.mission.open && !panning && response.contains_pointer(),
        );
        if parameter_pick
            && !self.workbench.mission.base.active
            && press
            && response.contains_pointer()
            && let Some(at) = at
        {
            self.workbench.mission.pick_canvas(&self.editor, at, marker);
        }
        if editing
            && !task_navigation
            && !parameter_pick
            && !panning
            && !self.workbench.cancel_secondary
        {
            if press && response.contains_pointer() {
                self.editor.region_modifiers(ui.input(|i| i.modifiers));
            }
            self.editor.pointer(
                scene,
                at,
                press && response.contains_pointer(),
                down,
                release,
                marker,
            );
        }
        if editing && !panning && !self.workbench.cancel_secondary && !self.editor.stroking {
            if response.secondary_clicked() && marker.is_some() {
                self.editor.select_from_canvas(marker);
            }
            response.context_menu(|ui| self.editor.context_actions(ui, scene));
        }
        if primary_release || !primary_down {
            self.workbench.canvas_pan = false;
        }
        if secondary_release {
            self.workbench.cancel_secondary = false;
        }
        self.editor
            .region_preview(&painter, scene, rect, &camera, at);
        for (index, color) in [
            (self.hover, egui::Color32::YELLOW),
            (self.selected, egui::Color32::from_rgb(70, 230, 250)),
        ] {
            if let Some(c) = index.and_then(|i| scene.cells.get(i)) {
                let origin = egui::pos2(c.screen_x, c.screen_y);
                let points = [[30., 0.], [60., 15.], [30., 30.], [0., 15.], [30., 0.]]
                    .map(|p| rect.min + (origin + egui::vec2(p[0], p[1]) - world.min) * self.zoom);
                painter.add(egui::Shape::line(
                    points.to_vec(),
                    egui::Stroke::new(1.5, color),
                ));
            }
        }
    }
    fn leave(&mut self, action: Leave, ctx: &egui::Context) {
        match action {
            Leave::Open(directory, name) => self.open_now(directory, name),
            Leave::New(directory, options) => {
                self.load_now(directory, "Untitled.map".into(), Some(options))
            }
            Leave::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Leave::Restore(path) => {
                let _ = self.editor.io.send(crate::editor_io::Job::Restore(path));
            }
        }
    }
    pub fn dialogs(&mut self, ctx: &egui::Context) {
        if self.pending.is_none() {
            return;
        }
        let mut choice = 0;
        let modal = egui::Modal::new(egui::Id::unique("unsaved-map")).show(ctx, |ui| {
            ui.set_width(420.0_f32.min(ctx.content_rect().width() - 64.));
            ui.heading("地图有未保存的修改");
            ui.label("保存当前地图后继续，或丢弃本次修改。");
            if !self.editor.status.is_empty() {
                ui.add(egui::Label::new(&self.editor.status).wrap());
            }
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(!self.editor.saving, crate::ui_theme::primary("保存并继续"))
                    .clicked()
                {
                    choice = 1;
                }
                if ui
                    .add_enabled(!self.editor.saving, egui::Button::new("丢弃修改"))
                    .clicked()
                {
                    choice = 2;
                }
                if ui.button("取消").clicked() {
                    choice = 3;
                }
            });
        });
        if modal.should_close() {
            choice = 3;
        }
        match choice {
            1 => {
                self.editor.save(false);
                self.leave_after_save = self.editor.saving;
            }
            2 => {
                self.editor.discard_recovery();
                if let Some(action) = self.pending.take() {
                    self.leave(action, ctx);
                }
            }
            3 => {
                self.pending = None;
                self.leave_after_save = false;
            }
            _ => {}
        }
    }
    pub fn smoke(
        &mut self,
        ctx: &egui::Context,
        state: &egui_wgpu::RenderState,
        output: &Path,
    ) -> Option<Result<()>> {
        if self.started.elapsed() > Duration::from_secs(90) {
            return Some(Err(anyhow::anyhow!("P1 烟测超时")));
        }
        if let Some(error) = &self.error {
            return Some(Err(anyhow::anyhow!(error.clone())));
        }
        if !self.loading
            && self
                .scene
                .as_ref()
                .is_some_and(|scene| scene.images.is_empty())
        {
            return Some(Err(anyhow::anyhow!(
                "没有可显示的地形：P1 烟测需要地图环境对应的完整资源"
            )));
        }
        if !self.loading
            && self
                .gpu
                .as_ref()
                .is_some_and(|gpu| gpu.paints.load(Ordering::Relaxed) >= 2)
            && self.smoke_loads.is_empty()
        {
            // Exercise a real reload in the same process: this must reuse the
            // pipelines and continue servicing UI frames during preparation.
            let sample = self.load_sample();
            self.open(self.directory.clone(), self.name.clone());
            self.smoke_loads.push(sample);
            return None;
        }
        let (Some(scene), Some(gpu)) = (&self.scene, &self.gpu) else {
            ctx.request_repaint_after(Duration::from_millis(16));
            return None;
        };
        if !self.verified {
            if self.smoke_loads.is_empty() {
                ctx.request_repaint_after(Duration::from_millis(16));
                return None;
            }
            self.smoke_loads.push(self.load_sample());
            let result = (|| -> Result<()> {
                std::fs::create_dir_all(output)?;
                let checks = gpu.verify(&state.device, &state.queue, scene, output)?;
                let shared_scene = gpu.verify_scene(&state.device, &state.queue, scene, output)?;
                let copy = output.join(format!(
                    "roundtrip-{}-{}.map",
                    std::process::id(),
                    self.generation
                ));
                scene.source.export_new(&copy)?;
                let exported = std::fs::read(&copy)?;
                ensure!(exported == scene.source.bytes(), "原文往返不一致");
                let (reopened, _) = crate::map::SourceMap::parse(
                    "roundtrip.map".into(),
                    copy.display().to_string(),
                    exported,
                )?;
                ensure!(
                    reopened.header.width == scene.source.header.width
                        && reopened.header.height == scene.source.header.height,
                    "重开地图头不一致"
                );
                let info = state.adapter.get_info();
                ensure!(self.timings.pipelines_reused, "重复加载未复用 GPU 管线");
                let report = serde_json::json!({"phase":"P1","passed":false,"map":scene.source.name,"source":scene.source.origin,"source_bytes":scene.source.bytes().len(),"roundtrip_file":copy,"roundtrip_bytes_equal":true,"reopen":true,"cells":scene.cells.len(),"tile_images":scene.images.len(),"load_ms":scene.elapsed_ms,"diagnostics":scene.diagnostics,"terrain":checks,"shared_scene":shared_scene,"gpu":{"name":info.name,"backend":format!("{:?}",info.backend)},"device_owner":"eframe","loading":self.smoke_loads,"gpu_preparation_thread":"map-loader","terrain_texture_writes_per_load":2,"scene_atlas_texture_writes_per_page":3});
                std::fs::write(
                    output.join("report.json"),
                    serde_json::to_vec_pretty(&report)?,
                )?;
                Ok(())
            })();
            if let Err(error) = result {
                return Some(Err(error));
            }
            self.verified = true;
        }
        if gpu.paints.load(Ordering::Relaxed) >= 2 && !self.screenshot_requested {
            self.screenshot_requested = true;
            crate::screen_capture::request(ctx);
        }
        let capture = crate::screen_capture::take(ctx);
        if let Some(image) = capture {
            return Some((|| -> Result<()> {
                let bytes: Vec<_> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                image::save_buffer(
                    output.join("window.png"),
                    &bytes,
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                )?;
                let path = output.join("report.json");
                let mut report: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&path)?).context("读取烟测报告")?;
                report["passed"] = true.into();
                report["ui_callback_draws"] = gpu.paints.load(Ordering::Relaxed).into();
                report["window_capture"] = serde_json::json!(image.size);
                std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
                println!("P1 smoke passed: {}", output.display());
                Ok(())
            })());
        }
        ctx.request_repaint_after(Duration::from_millis(16));
        None
    }
    fn load_sample(&self) -> serde_json::Value {
        serde_json::json!({"generation":self.generation,"timings":self.timings,"ui_logic_ticks_while_loading":self.loading_ticks,"ui_frames_while_loading":self.loading_ui_frames,"max_ui_tick_gap_ms":self.max_loading_gap_ms,"max_ui_poll_ms":self.max_poll_ms})
    }
}
