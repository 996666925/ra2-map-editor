//! Whole-map interaction on the normal terrain canvas. The sidebar owns layers;
//! Immutable per-map art stays on the GPU; dragging changes only view transforms.
use super::*;
use crate::{document::MapDocument, i18n::tr, ui_theme as theme};
use ra2_map_document::region::Clipboard;
use ra2_map_document::{document::PreparedReplacement, stitch::Workspace};
use std::collections::HashMap;
use std::sync::mpsc;

struct ResidentLayer {
    source: Arc<Clipboard>,
    scene: Arc<MapScene>,
    gpu: Option<Arc<TerrainGpu>>,
}

struct LayerCache {
    requests: mpsc::Sender<Workspace>,
    results: mpsc::Receiver<Result<Vec<Arc<ResidentLayer>>>>,
}
impl LayerCache {
    fn start(
        ctx: egui::Context,
        directory: PathBuf,
        gpu: Option<egui_wgpu::RenderState>,
    ) -> Result<Self> {
        let (requests, rx) = mpsc::channel::<Workspace>();
        let (tx, results) = mpsc::channel();
        std::thread::Builder::new()
            .name("map-layers".into())
            .spawn(move || {
                let mut install = None;
                let mut pipelines = None;
                let mut cached: HashMap<usize, Arc<ResidentLayer>> = HashMap::new();
                while let Ok(mut workspace) = rx.recv() {
                    for newer in rx.try_iter() {
                        workspace = newer;
                    }
                    let result = (|| -> Result<Vec<Arc<ResidentLayer>>> {
                        if install.is_none() {
                            install = Some(ra2_map_assets::assets::Install::open(&directory)?);
                        }
                        let mut layers = Vec::new();
                        for (index, piece) in workspace.pieces.iter().enumerate() {
                            let key = Arc::as_ptr(&piece.clip) as usize;
                            let layer = if let Some(layer) = cached.get(&key) {
                                layer.clone()
                            } else {
                                let bytes = workspace.layer_bytes(index)?;
                                let scene = Arc::new(ra2_map_scene::map::load_scene_bytes(
                                    install.as_mut().unwrap(),
                                    &piece.name,
                                    "map layer".into(),
                                    bytes,
                                    Instant::now(),
                                )?);
                                let prepared = if let Some(gpu) = &gpu {
                                    Some(TerrainGpu::new(
                                        &gpu.device,
                                        &gpu.queue,
                                        gpu.target_format,
                                        &scene,
                                        &mut pipelines,
                                    )?)
                                } else {
                                    None
                                };
                                let layer = Arc::new(ResidentLayer {
                                    source: piece.clip.clone(),
                                    scene,
                                    gpu: prepared,
                                });
                                cached.insert(key, layer.clone());
                                layer
                            };
                            layers.push(layer);
                        }
                        cached.retain(|key, _| {
                            workspace
                                .pieces
                                .iter()
                                .any(|p| Arc::as_ptr(&p.clip) as usize == *key)
                        });
                        if let Some(gpu) = &gpu {
                            gpu.queue.submit([]);
                        }
                        Ok(layers)
                    })();
                    if tx.send(result).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })?;
        Ok(Self { requests, results })
    }
}

struct Prepared {
    revision: u64,
    shift: [i32; 2],
    replacement: PreparedReplacement,
}

#[derive(Default)]
pub(super) struct MapManagement {
    pub open: bool,
    workspace: Option<Workspace>,
    applied_layout: Vec<(Arc<Clipboard>, [i32; 2])>,
    selected: usize,
    drag: Option<(egui::Pos2, [i32; 2])>,
    revision: u64,
    dirty: bool,
    worker: Option<mpsc::Receiver<Result<Prepared>>>,
    ready: Option<Prepared>,
    import_requested: bool,
    apply_requested: bool,
    restore_requested: bool,
    document_shift: [i32; 2],
    pub(super) published_shift: [i32; 2],
    pub(super) preserve_view: bool,
    camera_shift: egui::Vec2,
    message: String,
    gpu_state: Option<egui_wgpu::RenderState>,
    layer_cache: Option<LayerCache>,
    layers: Vec<Arc<ResidentLayer>>,
    layer_targets: HashMap<usize, Arc<Target>>,
    layer_pending: bool,
    layer_keys: Vec<usize>,
    #[cfg(test)]
    composition_jobs: usize,
}

pub(super) fn translation(shift: [i32; 2]) -> egui::Vec2 {
    egui::vec2(
        (shift[0] - shift[1]) as f32 * 30.,
        (shift[0] + shift[1]) as f32 * 15.,
    )
}

impl MapManagement {
    pub(super) fn presenting_layers(&self) -> bool {
        self.open || self.preserve_view
    }

    /// While the composite loads, layers retain their free layout coordinates,
    /// but the editor camera has already moved to normalized map coordinates.
    pub(super) fn presentation_shift(&self) -> egui::Vec2 {
        if self.preserve_view && !self.open {
            translation(self.document_shift)
        } else {
            egui::Vec2::ZERO
        }
    }

    pub(super) fn dragging(&self) -> bool {
        self.drag.is_some()
    }
    pub(super) fn prepare(&mut self, doc: &MapDocument, name: String) -> Result<()> {
        if self.workspace.as_ref().is_none_or(|w| !w.matches(doc)) {
            let (workspace, shift) = match Workspace::restore(doc)? {
                Some(restored) => restored,
                None => (Workspace::new(doc, name)?, [0, 0]),
            };
            let applied_layout = workspace
                .pieces
                .iter()
                .map(|p| (p.clip.clone(), p.offset))
                .collect();
            *self = Self {
                workspace: Some(workspace),
                applied_layout,
                document_shift: shift,
                gpu_state: self.gpu_state.clone(),
                ..Default::default()
            };
        }
        if !self.open {
            self.camera_shift -= translation(self.document_shift);
            self.published_shift = self.document_shift;
        }
        self.open = true;
        self.apply_requested = false;
        Ok(())
    }

    fn layout_unchanged(&self, doc: &MapDocument) -> bool {
        self.workspace.as_ref().is_some_and(|w| {
            w.matches(doc)
                && w.pieces.len() == self.applied_layout.len()
                && w.pieces
                    .iter()
                    .zip(&self.applied_layout)
                    .all(|(piece, (clip, offset))| {
                        Arc::ptr_eq(&piece.clip, clip) && piece.offset == *offset
                    })
        })
    }

    fn changed(&mut self) {
        self.revision += 1;
        self.dirty = true;
    }

    fn import_document(&mut self, name: String, source: &MapDocument) -> Result<()> {
        self.workspace
            .as_mut()
            .context(tr!("没有可编辑地图"))?
            .import(name, source)?;
        self.selected = self.workspace.as_ref().unwrap().pieces.len() - 1;
        self.changed();
        Ok(())
    }

    fn import(&mut self, scene: &MapScene) {
        let Some(paths) = rfd::FileDialog::new()
            .add_filter(tr!("地图文件"), &["map", "mpr", "yrm"])
            .pick_files()
        else {
            return;
        };
        let mut errors = Vec::new();
        for path in paths {
            let result = (|| -> Result<()> {
                use std::io::Read;
                let mut bytes = Vec::new();
                std::fs::File::open(&path)?
                    .take(ra2_map_document::source::MAX_SOURCE_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)?;
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let (source, _) = ra2_map_document::source::SourceMap::parse_with_ares(
                    name.clone(),
                    path.display().to_string(),
                    bytes,
                    scene
                        .source
                        .edit_base
                        .as_ref()
                        .map(|base| base.reads_ares)
                        .unwrap_or(false),
                )?;
                self.import_document(
                    name,
                    &MapDocument::new(source.edit_base.map_err(anyhow::Error::msg)?),
                )
            })();
            if let Err(error) = result {
                errors.push(format!("{}: {error:#}", path.display()));
            }
        }
        self.message = errors.join("\n");
    }

    fn cancel_drag(&mut self) -> bool {
        let Some((_, offset)) = self.drag.take() else {
            return false;
        };
        self.workspace.as_mut().unwrap().pieces[self.selected].offset = offset;
        self.changed();
        true
    }

    fn poll(&mut self, ctx: &egui::Context, editor: &mut crate::editing::Editor, scene: &MapScene) {
        if self.open {
            let lost_focus = ctx.input(|i| {
                i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::WindowFocused(false)))
            });
            let escape = !ctx.text_edit_focused()
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if lost_focus || escape {
                self.cancel_drag();
            }
        }
        if let Some(rx) = &self.worker {
            match rx.try_recv() {
                Ok(Ok(prepared)) => {
                    self.worker = None;
                    self.ready = Some(prepared);
                }
                Ok(Err(error)) => {
                    self.worker = None;
                    self.message = format!("{error:#}");
                    self.apply_requested = false;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.worker = None;
                    self.apply_requested = false;
                    self.message = tr!("拼接处理未完成，请重试。").into();
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(16));
                }
            }
        }
        if !self.open {
            return;
        }
        if self.import_requested {
            self.import_requested = false;
            self.import(scene);
        }
        // Entering the mode does not edit the document. Reuse its published
        // scene when the final layout is unchanged, including cancelled drags.
        if self.apply_requested && self.layout_unchanged(editor.doc.as_ref().unwrap()) {
            self.camera_shift += translation(self.document_shift);
            self.published_shift = [0, 0];
            self.open = false;
            self.dirty = false;
            self.worker = None;
            self.ready = None;
            self.apply_requested = false;
            return;
        }
        if self.apply_requested
            && self
                .ready
                .as_ref()
                .is_some_and(|p| p.revision == self.revision)
        {
            let prepared = self.ready.take().unwrap();
            let shift = prepared.shift;
            let result = self
                .workspace
                .as_mut()
                .unwrap()
                .apply_prepared(editor.doc.as_mut().unwrap(), prepared.replacement);
            match result {
                Ok(changed) => {
                    self.applied_layout = self
                        .workspace
                        .as_ref()
                        .unwrap()
                        .pieces
                        .iter()
                        .map(|p| (p.clip.clone(), p.offset))
                        .collect();
                    if changed {
                        editor.queue_structural_refresh();
                    }
                    self.camera_shift += translation(shift);
                    self.document_shift = shift;
                    self.open = false;
                    self.dirty = false;
                    self.worker = None;
                    self.restore_requested = true;
                    self.preserve_view = true;
                    self.apply_requested = false;
                    self.message = tr!("地图图层已应用，可整笔撤销。").into();
                }
                Err(error) => {
                    self.apply_requested = false;
                    self.message = format!("{error:#}");
                }
            }
        }
        if self.open
            && self.apply_requested
            && self.dirty
            && self.worker.is_none()
            && self.drag.is_none()
        {
            #[cfg(test)]
            {
                self.composition_jobs += 1;
            }
            self.dirty = false;
            let workspace = self.workspace.as_ref().unwrap().fork();
            let revision = self.revision;
            let doc = editor.doc.as_ref().unwrap();
            let snapshot = doc.snapshot();
            let edit_revision = doc.edit_revision();
            let footprints = workspace
                .pieces
                .iter()
                .flat_map(|p| p.clip.records.iter())
                .filter(|(_, r)| r.section == "Structures")
                .map(|(_, r)| {
                    (
                        r.type_id(),
                        ra2_map_scene::placement::offsets(scene, "Structures", &r.type_id()),
                    )
                })
                .collect();
            let (tx, rx) = mpsc::channel();
            self.worker = Some(rx);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result = (|| -> Result<Prepared> {
                    let composition = workspace.compose(&footprints)?;
                    let shift = composition.shift;
                    let replacement =
                        PreparedReplacement::prepare(snapshot, edit_revision, composition.bytes)?;
                    Ok(Prepared {
                        revision,
                        shift,
                        replacement,
                    })
                })();
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        }
    }

    pub(super) fn poll_layers(&mut self, ctx: &egui::Context, directory: &Path) -> Result<()> {
        if !self.presenting_layers() && !self.layer_pending {
            return Ok(());
        }
        if self.layer_cache.is_none() {
            self.layer_cache = Some(LayerCache::start(
                ctx.clone(),
                directory.into(),
                self.gpu_state.clone(),
            )?);
        }
        let cache = self.layer_cache.as_ref().unwrap();
        if self.layer_pending {
            match cache.results.try_recv() {
                Ok(Ok(layers)) => {
                    self.layers = layers;
                    self.layer_targets.retain(|key, _| {
                        self.layers
                            .iter()
                            .any(|layer| Arc::as_ptr(&layer.source) as usize == *key)
                    });
                    self.layer_pending = false;
                }
                Ok(Err(error)) => {
                    self.layer_pending = false;
                    return Err(error);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    anyhow::bail!("地图图层加载线程已退出");
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(16));
                }
            }
        }
        if !self.presenting_layers() {
            return Ok(());
        }
        let keys: Vec<_> = self
            .workspace
            .as_ref()
            .unwrap()
            .pieces
            .iter()
            .map(|p| Arc::as_ptr(&p.clip) as usize)
            .collect();
        if !self.layer_pending && keys != self.layer_keys {
            cache
                .requests
                .send(self.workspace.as_ref().unwrap().fork())?;
            self.layer_keys = keys;
            self.layer_pending = true;
        }
        Ok(())
    }

    pub(super) fn bind_gpu(&mut self, state: &egui_wgpu::RenderState) {
        if self.gpu_state.is_none() {
            self.gpu_state = Some(state.clone());
            self.layer_cache = None;
            self.layers.clear();
            self.layer_keys.clear();
            self.layer_pending = false;
        }
    }

    pub(super) fn draw_layers(
        &mut self,
        painter: &egui::Painter,
        rect: egui::Rect,
        camera: &crate::camera::MapCamera,
        size: [u32; 2],
        options: ra2_map_scene::visual::ViewOptions,
    ) -> bool {
        let camera = crate::camera::MapCamera {
            center: camera.center - self.presentation_shift(),
            zoom: camera.zoom,
        };
        let Some(state) = &self.gpu_state else {
            return false;
        };
        let workspace = self.workspace.as_ref().unwrap();
        // Keep the resident layers on screen while a new import is loading.
        // Falling back to the saved composite would lose their current offsets.
        if !workspace.pieces.iter().any(|p| {
            self.layers
                .iter()
                .any(|l| Arc::ptr_eq(&l.source, &p.clip) && l.gpu.is_some())
        }) {
            return false;
        }
        painter.rect_filled(rect, 0., theme::BACKGROUND);
        for piece in &workspace.pieces {
            let Some(layer) = self
                .layers
                .iter()
                .find(|l| Arc::ptr_eq(&l.source, &piece.clip) && l.gpu.is_some())
            else {
                continue;
            };
            if !layer
                .scene
                .bounds
                .translate(translation(piece.offset))
                .intersects(camera.world_rect(rect))
            {
                continue;
            }
            let key = Arc::as_ptr(&piece.clip) as usize;
            let gpu = layer.gpu.as_ref().unwrap();
            let target = self
                .layer_targets
                .entry(key)
                .or_insert_with(|| gpu.target(&state.device, size));
            if target.size != size {
                *target = gpu.target(&state.device, size);
            }
            // Art stays on the GPU; dragging only changes this view transform.
            let world = camera
                .world_rect(rect)
                .translate(-translation(piece.offset));
            painter.add(gpu.layer_callback(rect, target.clone(), world, options));
        }
        true
    }

    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui) {
        theme::badge(ui, tr!("多地图模式"), theme::ACCENT);
        if ui.button(tr!("导入地图…")).clicked() {
            self.import_requested = true;
        }
        if ui
            .add_enabled(
                !self.apply_requested && self.drag.is_none(),
                theme::primary(tr!("完成并返回编辑")),
            )
            .clicked()
        {
            self.apply_requested = true;
            self.dirty = true;
        }
        // Keep the canvas origin fixed while background previews come and go.
        let (slot, _) = ui.allocate_exact_size(egui::vec2(20., 20.), egui::Sense::hover());
        if self.worker.is_some() || self.layer_pending {
            egui::Spinner::new().paint_at(ui, slot);
        }
    }

    pub(super) fn world_bounds(&self) -> egui::Rect {
        self.workspace
            .as_ref()
            .unwrap()
            .pieces
            .iter()
            .map(|p| {
                let [min, max] = p.projected_bounds();
                let level = p.max_level;
                egui::Rect::from_min_max(
                    egui::pos2(
                        min[0] as f32 * 30. - 30.,
                        min[1] as f32 * 15. + 15. - f32::from(level) * 15.,
                    ),
                    egui::pos2(max[0] as f32 * 30. - 30., max[1] as f32 * 15. + 30.),
                )
            })
            .reduce(|a, b| a.union(b))
            .unwrap()
    }

    pub(super) fn panel(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, tr!("地图图层"), "");
        ui.small(tr!("在右侧大地图上按住左键，整体拖动所选地图。"));
        ui.add_enabled_ui(!self.apply_requested && self.drag.is_none(), |ui| {
            let mut changed = false;
            let workspace = self.workspace.as_mut().unwrap();
            for (index, piece) in workspace.pieces.iter().enumerate() {
                if ui
                    .add(
                        egui::Button::new(&piece.name)
                            .selected(self.selected == index)
                            .wrap_mode(egui::TextWrapMode::Truncate),
                    )
                    .on_hover_text(&piece.name)
                    .clicked()
                {
                    self.selected = index;
                }
                ui.small(format!("{} · {}", piece.theater, piece.clip.cells.len()));
            }
            ui.separator();
            theme::section(ui, tr!("图层位置"), "");
            for (axis, label) in [tr!("地图 X 偏移"), tr!("地图 Y 偏移")]
                .into_iter()
                .enumerate()
            {
                ui.horizontal(|ui| {
                    ui.label(label);
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut workspace.pieces[self.selected].offset[axis])
                                .range(-1024..=1024),
                        )
                        .changed();
                });
            }
            if self.selected > 0 && ui.button(tr!("移除所选地图")).clicked() {
                workspace.pieces.remove(self.selected);
                self.selected = self.selected.min(workspace.pieces.len() - 1);
                changed = true;
            }
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(self.selected > 1, egui::Button::new(tr!("下移一层")))
                    .clicked()
                {
                    workspace.pieces.swap(self.selected, self.selected - 1);
                    self.selected -= 1;
                    changed = true;
                }
                if ui
                    .add_enabled(
                        self.selected > 0 && self.selected + 1 < workspace.pieces.len(),
                        egui::Button::new(tr!("上移一层")),
                    )
                    .clicked()
                {
                    workspace.pieces.swap(self.selected, self.selected + 1);
                    self.selected += 1;
                    changed = true;
                }
            });
            ui.small(tr!(
                "列表靠后的地图覆盖前面的地图；移除图层会露出下方地图。"
            ));
            if changed {
                self.changed();
            }
        });
        if !self.message.is_empty() {
            ui.colored_label(theme::ACCENT, &self.message);
        }
    }

    fn contains(&self, index: usize, at: [i32; 2]) -> bool {
        let p = &self.workspace.as_ref().unwrap().pieces[index];
        let source = [at[0] - p.offset[0], at[1] - p.offset[1]];
        if source.iter().any(|&v| !(0..1024).contains(&v)) {
            return false;
        }
        let at = [source[0] as u16, source[1] as u16];
        p.clip.cells.binary_search_by_key(&at, |c| c.at).is_ok()
    }

    pub(super) fn canvas(
        &mut self,
        ui: &egui::Ui,
        scene: &MapScene,
        rect: egui::Rect,
        response: &egui::Response,
        camera: &crate::camera::MapCamera,
        panning: bool,
    ) {
        let (press, down, release, secondary) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.button_pressed(egui::PointerButton::Secondary),
            )
        });
        if secondary && self.drag.is_some() {
            self.cancel_drag();
        }
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let world = pointer.map(|p| camera.world_point(rect, p));
        if !self.apply_requested
            && !panning
            && press
            && response.contains_pointer()
            && let Some(world) = world
        {
            let hit = |index: usize| {
                let piece = &self.workspace.as_ref().unwrap().pieces[index];
                self.layers
                    .iter()
                    .find(|l| Arc::ptr_eq(&l.source, &piece.clip))
                    .and_then(|layer| {
                        layer
                            .scene
                            .pick(world - translation(piece.offset))
                            .map(|i| {
                                let c = &layer.scene.cells[i];
                                [
                                    i32::from(c.rx) + piece.offset[0],
                                    i32::from(c.ry) + piece.offset[1],
                                ]
                            })
                    })
            };
            let cached = hit(self.selected)
                .filter(|&at| self.contains(self.selected, at))
                .or_else(|| {
                    (0..self.workspace.as_ref().unwrap().pieces.len())
                        .rev()
                        .find_map(hit)
                });
            let sample = world + translation(self.published_shift);
            let at = cached.or_else(|| {
                scene.pick(sample).map(|i| {
                    let c = &scene.cells[i];
                    [
                        i32::from(c.rx) - self.published_shift[0],
                        i32::from(c.ry) - self.published_shift[1],
                    ]
                })
            });
            if let Some(at) = at {
                let selected = self
                    .contains(self.selected, at)
                    .then_some(self.selected)
                    .or_else(|| {
                        (0..self.workspace.as_ref().unwrap().pieces.len())
                            .rev()
                            .find(|&i| self.contains(i, at))
                    });
                if let Some(selected) = selected {
                    self.selected = selected;
                    self.drag = Some((
                        world,
                        self.workspace.as_ref().unwrap().pieces[selected].offset,
                    ));
                }
            }
        }
        if !panning
            && (down || release)
            && let (Some((start, offset)), Some(world)) = (self.drag, world)
        {
            let d = world - start;
            let next = [
                (offset[0] + (d.x / 60. + d.y / 30.).round() as i32).clamp(-1024, 1024),
                (offset[1] + (d.y / 30. - d.x / 60.).round() as i32).clamp(-1024, 1024),
            ];
            if next != self.workspace.as_ref().unwrap().pieces[self.selected].offset {
                self.workspace.as_mut().unwrap().pieces[self.selected].offset = next;
                self.changed();
            }
        }
        if release || !down {
            self.drag = None;
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(if self.drag.is_some() || panning {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            });
        }
    }

    pub(super) fn outline(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        camera: &crate::camera::MapCamera,
    ) {
        let p = &self.workspace.as_ref().unwrap().pieces[self.selected];
        let [min, max] = p.projected_bounds();
        let b = egui::Rect::from_min_max(
            egui::pos2(min[0] as f32 * 30. - 30., min[1] as f32 * 15. + 15.),
            egui::pos2(max[0] as f32 * 30. - 30., max[1] as f32 * 15. + 30.),
        );
        let project = |p: egui::Pos2| rect.center() + (p - camera.center) * camera.zoom;
        let outline = egui::Rect::from_min_max(project(b.min), project(b.max));
        painter.rect_stroke(
            outline,
            0.,
            egui::Stroke::new(2., theme::ACCENT),
            egui::StrokeKind::Inside,
        );
        painter.text(
            outline.left_top() + egui::vec2(5., 5.),
            egui::Align2::LEFT_TOP,
            &p.name,
            egui::FontId::proportional(13.),
            theme::ACCENT,
        );
        painter.text(
            rect.left_bottom() + egui::vec2(12., -10.),
            egui::Align2::LEFT_BOTTOM,
            tr!("拖动整张地图；Esc / 右键取消本次移动，空格或中键平移视图。"),
            egui::FontId::proportional(12.),
            theme::TEXT,
        );
    }
}

impl MapView {
    pub(super) fn open_map_management(&mut self, _ctx: &egui::Context, import: bool) {
        if (!self.workbench.management.open && (self.loading || self.editing_blocked()))
            || self.editor.stroking
            || !self.editor.check_property_draft()
        {
            return;
        }
        let Some(doc) = &self.editor.doc else { return };
        if let Err(error) = self.workbench.management.prepare(doc, self.name.clone()) {
            self.status = format!("{error:#}");
            return;
        }
        if let Some(scene) = &self.scene {
            crate::flat_view::set_enabled(&mut self.editor, scene, &mut self.center, false);
        }
        self.editor.set_framework(false);
        if import {
            self.workbench.management.import_requested = true;
        }
    }

    pub(super) fn poll_map_management(&mut self, ctx: &egui::Context) {
        let Some(scene) = &self.scene else { return };
        self.workbench.management.poll(ctx, &mut self.editor, scene);
        if let Err(error) = self.workbench.management.poll_layers(ctx, &self.directory) {
            self.workbench.management.message = format!("{error:#}");
        }
        let m = &mut self.workbench.management;
        self.center += std::mem::take(&mut m.camera_shift);
        if m.restore_requested {
            m.restore_requested = false;
            self.generation += 1;
            self.loader.cancel(self.generation);
            self.loading = false;
            self.editor.refresh = true;
        }
    }
}

#[cfg(test)]
#[path = "map_management_tests.rs"]
mod tests;
