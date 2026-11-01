//! Authoring tools and map object appearances with selection overlays.
use crate::{
    document::{Cell, MapDocument, Record, Target, Value, cell_line},
    editor_io::{Done, EditorIo, Expected, Job, PlayProcess, RecoveryEntry, RecoveryMeta, Stamp},
    map::MapScene,
};
use anyhow::{Context, Result, ensure};
use eframe::egui::{self, Color32};
use ra2_map_scene::terrain_tools::{self, TerrainMode};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use vera20k_core::map::cell_index::MAX_CELL_SIDE;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Region,
    Paste,
    RegionMove,
    Clone,
    Terrain,
    Ore,
    Gems,
    Overlay,
    Erase,
    Place,
    Move,
    Waypoint,
    Pick,
}
#[cfg(test)]
#[path = "editing_tests.rs"]
pub(crate) mod tests;
impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Self::Select => "选择",
            Self::Region => "区域选择",
            Self::Paste => "粘贴区域",
            Self::RegionMove => "移动区域",
            Self::Clone => "克隆对象",
            Self::Terrain => "地形",
            Self::Ore => "矿石",
            Self::Gems => "宝石",
            Self::Overlay => "覆盖物",
            Self::Erase => "擦除覆盖物",
            Self::Place => "放置对象",
            Self::Move => "移动选中对象",
            Self::Waypoint => "起点 / 航点",
            Self::Pick => "吸取地形",
        }
    }
    fn brush(self) -> bool {
        matches!(
            self,
            Self::Terrain | Self::Ore | Self::Gems | Self::Overlay | Self::Erase
        )
    }
}
#[derive(Clone, Copy)]
struct Stroke {
    tool: Tool,
    mode: TerrainMode,
    brush: usize,
    radius: u16,
    lat: bool,
    smooth: bool,
    level: u8,
    walls: bool,
    overlay: u8,
    frame: u8,
    template_level: Option<u8>,
}
#[path = "interaction.rs"]
pub(crate) mod interaction;
#[path = "playtest_ui.rs"]
mod playtest_ui;

pub struct Editor {
    pub play_options: ra2_map_io::playtest::PlayOptions,
    play_displays: Vec<playtest_ui::PlayDisplay>,
    pub exporting: bool,
    pub lighting: bool,
    pub task_labels: bool,
    pub base_nodes_visible: bool,
    pub remap: bool,
    pub layers: [bool; 3],
    ui_state: editing_ui::EditorUi,
    document_name: String,
    pub doc: Option<MapDocument>,
    pub tool: Tool,
    pub brush: usize,
    pub radius: u16,
    region: region_edit::RegionEdit,
    plateau: cliff_edit::PlateauEdit,
    fill: fill_edit::FillEdit,
    object_brush: object_brush::ObjectBrush,
    pub allow_overlap: bool,
    pub infantry_subcell: u8,
    property_extra: BTreeMap<usize, String>,
    pub terrain_mode: TerrainMode,
    pub auto_lat: bool,
    pub smooth_heights: bool,
    pub flatten_level: u8,
    pub template_level: Option<u8>,
    pub bridge_kind: ra2_map_scene::bridge_tools::BridgeKind,
    pub auto_walls: bool,
    stroke: Option<Stroke>,
    stroke_cells: BTreeSet<Cell>,
    pub overlay_frame: u8,
    pub overlay_id: u8,
    pub section: String,
    pub type_id: String,
    pub owner: String,
    pub waypoint: u32,
    pub selected: Option<u64>,
    pub(crate) records: BTreeMap<u64, Record>,
    pub(super) marker_cache: object_markers::MarkerCache,
    pub(crate) visual_revision: u64,
    pub(crate) overlays: HashMap<Cell, (u8, u8)>,
    last: Option<Cell>,
    pub stroking: bool,
    pub feedback: BTreeSet<Cell>,
    pub refresh: bool,
    pub(crate) structural_refresh: bool,
    pub status: String,
    pub target: Option<PathBuf>,
    expected: Option<Stamp>,
    pub io: EditorIo,
    pub saving: bool,
    pub saved: bool,
    recovering: bool,
    last_edit: Instant,
    last_recovery: Option<(u64, u64)>,
    pub recovery: Option<PathBuf>,
    pub recoveries: Vec<RecoveryEntry>,
    pub restored: Option<(Arc<crate::document::Baseline>, RecoveryMeta, PathBuf)>,
    pub executable: String,
    pub process: Option<PlayProcess>,
    pub launching: bool,
    pub directory: PathBuf,
    property_owner: String,
    property_startup_owner: Option<u8>,
    property_type: String,
    property_health: u16,
    property_facing: u8,
    property_completion: crate::form_edit::EditCompletion,
    pub(crate) save_requested: Option<bool>,
    property_error: String,
}
impl Editor {
    pub fn view_options(&self) -> ra2_map_scene::visual::ViewOptions {
        ra2_map_scene::visual::ViewOptions {
            overlays: self.layers[0],
            objects: self.layers[1],
            lighting: self.lighting,
            remap: self.remap,
        }
    }
    pub fn new(ctx: egui::Context, root: PathBuf) -> Result<Self> {
        Ok(Self {
            layers: [true; 3],
            play_options: Default::default(),
            play_displays: Vec::new(),
            exporting: false,
            lighting: true,
            task_labels: true,
            base_nodes_visible: true,
            remap: true,
            ui_state: Default::default(),
            document_name: String::new(),
            doc: None,
            tool: Tool::Select,
            brush: 0,
            radius: 1,
            region: Default::default(),
            plateau: Default::default(),
            fill: Default::default(),
            object_brush: Default::default(),
            allow_overlap: false,
            infantry_subcell: 0,
            property_extra: BTreeMap::new(),
            property_completion: Default::default(),
            save_requested: None,
            property_error: String::new(),
            terrain_mode: TerrainMode::Template,
            auto_lat: true,
            smooth_heights: true,
            flatten_level: 0,
            template_level: None,
            bridge_kind: Default::default(),
            auto_walls: true,
            stroke: None,
            stroke_cells: BTreeSet::new(),
            overlay_frame: 11,
            overlay_id: 0,
            section: "Structures".into(),
            type_id: "GAPOWR".into(),
            owner: "Neutral".into(),
            waypoint: 0,
            selected: None,
            records: BTreeMap::new(),
            marker_cache: Default::default(),
            visual_revision: 0,
            overlays: HashMap::new(),
            last: None,
            stroking: false,
            feedback: BTreeSet::new(),
            refresh: false,
            structural_refresh: false,
            status: String::new(),
            target: None,
            expected: None,
            io: EditorIo::new(ctx, root)?,
            saving: false,
            saved: false,
            recovering: false,
            last_edit: Instant::now(),
            last_recovery: None,
            recovery: None,
            recoveries: vec![],
            restored: None,
            executable: String::new(),
            process: None,
            launching: false,
            directory: PathBuf::new(),
            property_owner: String::new(),
            property_startup_owner: None,
            property_type: String::new(),
            property_health: 256,
            property_facing: 0,
        })
    }
    pub fn attach(&mut self, scene: &MapScene) {
        self.plateau.cancel();
        self.fill.cancel();
        self.object_brush.cancel();
        self.region = Default::default();
        self.ui_state.textures.clear();
        self.ui_state.template_textures.clear();
        self.ui_state.terrain_group = None;
        self.template_level = None;
        self.document_name = scene.source.name.clone();
        self.doc = if scene.images.is_empty() {
            None
        } else {
            scene
                .source
                .edit_base
                .as_ref()
                .ok()
                .map(|base| MapDocument::new(base.clone()))
        };
        if let Some(doc) = &mut self.doc {
            doc.resource_profile = Some(scene.revision.id());
        }
        self.target = Path::new(&scene.source.name)
            .is_file()
            .then(|| PathBuf::from(&scene.source.name));
        self.expected = self
            .target
            .as_ref()
            .map(|_| Stamp::bytes(scene.source.bytes()));
        self.selected = None;
        self.tool = Tool::Select;
        self.brush = 0;
        self.last = None;
        self.stroking = false;
        self.feedback.clear();
        self.stroke = None;
        self.stroke_cells.clear();
        self.refresh = false;
        self.status.clear();
        self.last_recovery = None;
        self.recovery = self
            .doc
            .as_ref()
            .map(|d| crate::editor_io::recovery_path(&self.io.root, d.id));
        if self.executable.is_empty() {
            self.executable = self.directory.join("vera20k.exe").display().to_string();
        }
        self.cache(scene);
    }
    pub fn restore(
        &mut self,
        base: Arc<crate::document::Baseline>,
        meta: RecoveryMeta,
        path: PathBuf,
    ) {
        self.plateau.cancel();
        self.fill.cancel();
        self.object_brush.cancel();
        self.region = Default::default();
        self.document_name = meta.name.clone();
        let mut doc = MapDocument::new(base);
        doc.saved_state = None;
        doc.resource_profile = meta.resource_profile.clone();
        self.doc = Some(doc);
        self.directory = meta.directory;
        self.target = meta.target;
        self.expected = meta.expected;
        self.recovery = Some(path.clone());
        self.recoveries.retain(|e| e.path != path);
        self.last_recovery = None;
        self.selected = None;
        self.records.clear();
        self.marker_cache = Default::default();
        self.overlays.clear();
        self.stroking = false;
        self.feedback.clear();
        self.refresh = true;
        self.last_edit = Instant::now();
        self.status = "已恢复快照；请确认后保存。恢复不包含崩溃前的撤销历史。".into();
    }
    pub(crate) fn mark_new(&mut self) {
        if let Some(doc) = &mut self.doc {
            doc.saved_state = None;
        }
        self.target = None;
        self.expected = None;
        self.last_edit = Instant::now();
        self.status = "已新建地图；Ctrl+S 选择保存位置。".into();
    }
    pub fn cache(&mut self, scene: &MapScene) {
        self.region.invalidate_preview();
        self.visual_revision = self.visual_revision.wrapping_add(1);
        if self
            .ui_state
            .sprite_source
            .as_ref()
            .is_none_or(|p| !Arc::ptr_eq(p, &scene.sprites))
        {
            self.ui_state.sprites.clear();
            self.ui_state.map_sprites.clear();
            self.ui_state.sprite_source = Some(scene.sprites.clone());
        }
        self.records = self
            .doc
            .as_ref()
            .map(MapDocument::records)
            .unwrap_or_default();
        if self
            .selected
            .is_some_and(|id| !self.records.contains_key(&id))
        {
            self.select(None);
        }
        self.marker_cache =
            object_markers::MarkerCache::new(scene, self.doc.as_ref(), &self.records);
        self.overlays.clear();
        if let Some(doc) = &self.doc {
            for cell in &scene.cells {
                let at = [cell.rx, cell.ry];
                let value = doc.overlay(at);
                if value.0 != 255 {
                    self.overlays.insert(at, value);
                }
            }
        }
    }
    pub fn dirty(&self) -> bool {
        self.doc.as_ref().is_some_and(MapDocument::dirty)
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(done) = self.io.results.try_recv() {
            match done {
                Done::Export { path, result } => {
                    self.exporting = false;
                    self.status = match result {
                        Ok(size) => format!("已导出 {}×{}：{}", size[0], size[1], path.display()),
                        Err(e) => format!("导出失败：{e:#}"),
                    };
                }
                Done::Scan(result) => match result {
                    Ok(entries) => self.recoveries = entries,
                    Err(e) => self.status = format!("扫描恢复快照失败：{e:#}"),
                },
                Done::Save {
                    document,
                    state,
                    path,
                    result,
                } => {
                    self.saving = false;
                    match result {
                        Ok(saved) => {
                            if let Some(doc) = &mut self.doc
                                && doc.id == document
                            {
                                doc.saved_state = Some(state);
                                self.target = Some(path.clone());
                                self.document_name = path.display().to_string();
                                self.expected = Some(saved.stamp);
                                self.saved = true;
                                self.status = format!(
                                    "已保存：{}{}",
                                    path.display(),
                                    if doc.dirty() {
                                        "（仍有较新的修改）"
                                    } else {
                                        ""
                                    }
                                );
                                if let Some(warning) = saved.warning {
                                    self.status.push_str(&format!("；{warning}"));
                                }
                                self.last_recovery = None;
                                if let Some(path) = &self.recovery {
                                    let _ = self.io.send(Job::Discard(path.clone()));
                                }
                            }
                        }
                        Err(e) => self.status = format!("保存失败：{e:#}"),
                    }
                }
                Done::Recover {
                    document,
                    state,
                    path,
                    result,
                } => {
                    self.recovering = false;
                    match result {
                        Ok(()) => {
                            if self
                                .doc
                                .as_ref()
                                .is_some_and(|doc| doc.id == document && doc.dirty())
                            {
                                self.last_recovery = Some((document, state));
                            } else {
                                let _ = self.io.send(Job::Discard(path));
                            }
                        }
                        Err(e) => {
                            self.status = format!("自动恢复快照失败：{e:#}");
                            self.last_edit = Instant::now();
                        }
                    }
                }
                Done::Restore { path, result } => match result {
                    Ok((base, meta)) => self.restored = Some((base, meta, path)),
                    Err(e) => self.status = format!("恢复失败：{e:#}"),
                },
                Done::Discard(result) => {
                    if let Err(e) = result {
                        self.status = format!("删除恢复快照失败：{e:#}");
                    }
                }
                Done::Play(result) => {
                    self.launching = false;
                    match result {
                        Ok(process) => {
                            self.status = format!("试玩已启动；日志：{}", process.folder.display());
                            self.process = Some(process);
                        }
                        Err(e) => self.status = format!("试玩失败：{e:#}"),
                    }
                }
            }
        }
        if let Some(process) = &mut self.process {
            match process.child.try_wait() {
                Ok(Some(code)) => {
                    self.status = format!("试玩退出 {code}；日志：{}", process.folder.display());
                    self.process = None;
                }
                Err(e) => self.status = format!("读取试玩进程状态失败：{e}"),
                Ok(None) => {}
            }
        }
        if !self.dirty() && self.last_recovery.take().is_some() {
            self.discard_recovery();
        }
        if self.dirty()
            && !self.stroking
            && !self.recovering
            && self.last_edit.elapsed() > Duration::from_secs(3)
            && let Some(doc) = &self.doc
            && self.last_recovery != Some((doc.id, doc.state_id))
            && let Some(path) = &self.recovery
        {
            let meta = RecoveryMeta {
                version: 1,
                name: self.document_name.clone(),
                directory: self.directory.clone(),
                target: self.target.clone(),
                expected: self.expected,
                resource_profile: doc.resource_profile.clone(),
            };
            match self.io.send(Job::Recover {
                snapshot: doc.snapshot(),
                path: path.clone(),
                meta,
            }) {
                Ok(()) => self.recovering = true,
                Err(e) => self.status = e.to_string(),
            }
        }
        if self.dirty()
            || self.saving
            || self.recovering
            || self.exporting
            || self.process.is_some()
            || self.launching
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    pub fn save(&mut self, as_new: bool) {
        if !self.check_property_draft() {
            return;
        }
        if self.saving || self.stroking {
            return;
        }
        let Some(doc) = &self.doc else { return };
        let ask = as_new || self.target.is_none();
        let path = if ask {
            rfd::FileDialog::new()
                .add_filter("YR 地图", &["map", "yrm"])
                .set_file_name(
                    self.target
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .or_else(|| Path::new(&self.document_name).file_name())
                        .unwrap_or_default()
                        .to_string_lossy(),
                )
                .save_file()
        } else {
            self.target.clone()
        };
        if let Some(path) = path {
            let expected = if self.target.as_ref() == Some(&path) {
                Expected::Exact(self.expected)
            } else {
                Expected::Capture
            };
            match self.io.send(Job::Save {
                directory: Some(self.directory.clone()),
                snapshot: doc.snapshot(),
                path,
                expected,
            }) {
                Ok(()) => {
                    self.saving = true;
                    self.saved = false;
                    self.status = "正在后台校验并保存…".into();
                }
                Err(e) => self.status = e.to_string(),
            }
        }
    }
    pub fn play(&mut self) {
        if !self.check_property_draft() {
            return;
        }
        if self.launching || self.process.is_some() || self.stroking {
            return;
        }
        if let Some(doc) = &self.doc {
            match self.io.send(Job::Play {
                options: self.play_options.clone(),
                snapshot: doc.snapshot(),
                exe: PathBuf::from(&self.executable),
                directory: self.directory.clone(),
                root: self.io.root.clone(),
            }) {
                Ok(()) => self.launching = true,
                Err(e) => self.status = e.to_string(),
            }
        }
    }
    pub fn discard_recovery(&mut self) {
        if let Some(path) = &self.recovery {
            let _ = self.io.send(Job::Discard(path.clone()));
        }
    }
    pub fn shortcuts(&mut self, ctx: &egui::Context, scene: &MapScene) {
        if ctx.text_edit_focused() || crate::form_edit::popup_open(ctx) {
            return;
        }
        let (undo, redo, cancel, delete) = ctx.input(|i| {
            (
                i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                i.modifiers.command
                    && (i.key_pressed(egui::Key::Y)
                        || (i.modifiers.shift && i.key_pressed(egui::Key::Z))),
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::Delete),
            )
        });
        if cancel {
            self.cancel_operation(scene);
            return;
        }
        if undo || redo {
            self.history(redo, scene);
        }
        self.tool_shortcuts(ctx);
        if self.stroking {
            if delete {
                self.status = "请先完成或取消当前一笔，再删除选择。".into();
            }
            return;
        }
        let region_action = ctx.input(|i| {
            if i.events.iter().any(|e| matches!(e, egui::Event::Copy))
                || i.modifiers.command && i.key_pressed(egui::Key::C)
            {
                Some(region_edit::Action::Copy)
            } else if i.events.iter().any(|e| matches!(e, egui::Event::Cut))
                || i.modifiers.command && i.key_pressed(egui::Key::X)
            {
                Some(region_edit::Action::Cut)
            } else if i.events.iter().any(|e| matches!(e, egui::Event::Paste(_)))
                || i.modifiers.command && i.key_pressed(egui::Key::V)
            {
                Some(region_edit::Action::Paste)
            } else if i.modifiers.command && i.key_pressed(egui::Key::D) {
                Some(region_edit::Action::Clone)
            } else {
                None
            }
        });
        if let Some(action) = region_action {
            let pasted = ctx.input(|i| {
                i.events.iter().find_map(|e| {
                    if let egui::Event::Paste(text) = e {
                        Some(text.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(text) = pasted
                && let Err(e) = self.import_region_clipboard(&text)
            {
                self.status = e.to_string();
                return;
            }
            self.region_action(scene, action);
            if matches!(action, region_edit::Action::Copy | region_edit::Action::Cut) {
                self.export_region_clipboard(ctx);
            }
        }
        if delete {
            if self.tool == Tool::Region && !self.region.cells.is_empty() {
                self.region_action(scene, region_edit::Action::Delete);
            } else {
                self.delete(scene);
            }
        }
    }
    pub fn history(&mut self, redo: bool, scene: &MapScene) {
        if self.cancel_active(scene) {
            return;
        }
        if self.object_properties_dirty() {
            self.select(self.selected);
            self.status = "已取消当前属性输入；地图历史未改变。".into();
            return;
        }
        self.plateau.cancel();
        self.fill.cancel();
        self.object_brush.cancel();
        self.region.cancel();
        self.stroking = false;
        self.last = None;
        self.feedback.clear();
        self.stroke = None;
        self.stroke_cells.clear();
        if let Some(doc) = &mut self.doc {
            let name = if redo {
                doc.redo_name()
            } else {
                doc.undo_name()
            }
            .map(str::to_owned);
            let baseline = doc.base.clone();
            let changed = if redo { doc.redo() } else { doc.undo() };
            let structural = !Arc::ptr_eq(&baseline, &doc.base);
            if changed {
                if structural {
                    self.structural_changed(scene);
                } else {
                    self.changed(scene);
                }
                self.select(self.selected);
                self.status = format!(
                    "已{}：{}",
                    if redo { "重做" } else { "撤销" },
                    name.unwrap_or_default()
                );
            } else {
                self.status = format!("没有可{}的操作。", if redo { "重做" } else { "撤销" });
            }
        }
    }
    fn changed(&mut self, scene: &MapScene) {
        self.refresh = true;
        self.last_edit = Instant::now();
        self.cache(scene);
    }
    pub(crate) fn structural_changed(&mut self, scene: &MapScene) {
        self.structural_refresh = true;
        self.region = Default::default();
        self.plateau.cancel();
        self.fill.cancel();
        self.object_brush.cancel();
        self.select(None);
        self.changed(scene);
    }
    pub fn commit(&mut self, scene: &MapScene, name: &str) {
        if let Some(doc) = &mut self.doc
            && doc.commit(name)
        {
            self.changed(scene);
        }
    }
    fn delete(&mut self, scene: &MapScene) {
        if !self.check_property_draft() {
            return;
        }
        if let (Some(doc), Some(id)) = (&mut self.doc, self.selected) {
            match doc.set(Target::Record(id), None) {
                Ok(()) => {
                    self.selected = None;
                    self.commit(scene, "删除对象 / 航点");
                }
                Err(e) => self.status = e.to_string(),
            }
        }
    }
    pub fn export_preview_png(&mut self, bytes: &[u8]) {
        if self.exporting {
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG 图像", &["png"])
            .set_file_name("map-preview.png")
            .save_file()
        {
            match self.io.send(Job::ExportPreview {
                bytes: Arc::from(bytes),
                path,
            }) {
                Ok(()) => {
                    self.exporting = true;
                    self.status = "正在后台导出地图预览小图…".into();
                }
                Err(e) => self.status = e.to_string(),
            }
        }
    }
    pub fn export_hd_preview_png(&mut self) {
        self.export_scene_png(4096, Default::default(), "map-preview-hd.png");
    }
    pub fn export_png(&mut self, max_edge: u32) {
        self.export_scene_png(max_edge, self.view_options(), "map.png");
    }
    fn export_scene_png(
        &mut self,
        max_edge: u32,
        options: ra2_map_scene::visual::ViewOptions,
        file_name: &str,
    ) {
        if !self.check_property_draft() {
            return;
        }
        if self.exporting || self.stroking {
            return;
        }
        let Some(doc) = &self.doc else { return };
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG 图像", &["png"])
            .set_file_name(file_name)
            .save_file()
        {
            match self.io.send(Job::Export {
                snapshot: doc.snapshot(),
                directory: self.directory.clone(),
                path,
                max_edge,
                options,
            }) {
                Ok(()) => {
                    self.exporting = true;
                    self.status = "正在后台渲染完整地图…".into();
                }
                Err(e) => self.status = e.to_string(),
            }
        }
    }
    pub fn pointer(
        &mut self,
        scene: &MapScene,
        at: Option<Cell>,
        press: bool,
        down: bool,
        release: bool,
        marker: Option<u64>,
    ) {
        if self.doc.is_none() {
            return;
        }
        if press && self.tool != Tool::Select && !self.check_property_draft() {
            return;
        }
        if self.object_pointer(scene, at, press, down, release) {
            return;
        }
        if self.fill_pointer(scene, at, press, down, release) {
            return;
        }
        if self.plateau_pointer(scene, at, press, down, release) {
            return;
        }
        if self.region_pointer(scene, at, press, down, release) {
            return;
        }
        if press {
            self.last = None;
            self.feedback.clear();
            self.stroke_cells.clear();
            self.stroke = Some(Stroke {
                tool: self.tool,
                mode: self.terrain_mode,
                brush: self.brush,
                radius: if self.tool == Tool::Terrain && self.terrain_mode == TerrainMode::Template
                {
                    1
                } else {
                    self.radius
                },
                lat: self.auto_lat,
                smooth: self.smooth_heights,
                level: self.flatten_level,
                walls: self.auto_walls,
                overlay: self.overlay_id,
                frame: self.overlay_frame,
                template_level: self.template_level,
            });
            if self.tool == Tool::Select {
                self.select_from_canvas(marker);
            } else if self.tool == Tool::Pick {
                if let Some(at) = at
                    && let Some(c) = scene.cells.iter().find(|c| [c.rx, c.ry] == at)
                    && let Some(index) = scene
                        .brushes
                        .iter()
                        .position(|b| b.key.tile_id == c.tile_id && b.key.sub_tile == c.sub_tile)
                {
                    self.select_terrain_material(index, true);
                }
            } else if self.tool.brush() {
                self.stroking = true;
            } else if let Some(at) = at {
                let result = (|| -> Result<()> {
                    let doc = self.doc.as_mut().unwrap();
                    match self.tool {
                        Tool::Waypoint => doc.waypoint(self.waypoint, at)?,
                        Tool::Move => {
                            let id = self.selected.context("请先选中对象或航点")?;
                            let record = self
                                .records
                                .get(&id)
                                .context("选中对象已不存在")?
                                .moved(at)?;
                            ra2_map_scene::placement::validate(
                                doc,
                                scene,
                                std::slice::from_ref(&record),
                                &BTreeSet::from([id]),
                                self.allow_overlap,
                                &BTreeMap::new(),
                            )?;
                            doc.set(Target::Record(id), Some(Value::Record(record)))?;
                        }
                        Tool::Clone => {
                            let id = self.selected.context("请先选择要克隆的对象")?;
                            let record = self
                                .records
                                .get(&id)
                                .context("选中对象已不存在")?
                                .moved(at)?;
                            ensure!(record.section != "Waypoints", "航点请使用区域复制");
                            ra2_map_scene::placement::validate(
                                doc,
                                scene,
                                std::slice::from_ref(&record),
                                &BTreeSet::new(),
                                self.allow_overlap,
                                &BTreeMap::new(),
                            )?;
                            self.selected = Some(doc.insert_record(record)?);
                        }
                        _ => {}
                    }
                    Ok(())
                })();
                match result {
                    Ok(()) => {
                        self.commit(scene, self.tool.name());
                        let id = self.selected;
                        self.select(id);
                    }
                    Err(e) => self.status = format!("{e:#}"),
                }
            }
        }
        // Capture settings on press: releasing over a sidebar must still finish
        // the original tool's transaction. Deduplicate cells for height strokes.
        if self.stroking && (down || press || release) {
            let stroke = self.stroke.expect("active stroke settings");
            if let Some(at) = at {
                let line = cell_line(self.last.unwrap_or(at), at);
                self.last = Some(at);
                for center in line {
                    for dx in 0..stroke.radius {
                        for dy in 0..stroke.radius {
                            let x =
                                i32::from(center[0]) + i32::from(dx) - i32::from(stroke.radius / 2);
                            let y =
                                i32::from(center[1]) + i32::from(dy) - i32::from(stroke.radius / 2);
                            if !(0..i32::from(MAX_CELL_SIDE)).contains(&x)
                                || !(0..i32::from(MAX_CELL_SIDE)).contains(&y)
                            {
                                continue;
                            }
                            let cell = [x as u16, y as u16];
                            let doc = self.doc.as_mut().unwrap();
                            if doc.check_cell(cell).is_err() || self.stroke_cells.contains(&cell) {
                                continue;
                            }
                            if stroke.tool == Tool::Terrain && stroke.mode == TerrainMode::Template
                            {
                                let footprint = scene
                                    .brushes
                                    .get(stroke.brush)
                                    .context("请选择地形")
                                    .and_then(|brush| terrain_tools::footprint(scene, brush, cell));
                                match footprint {
                                    Ok(cells) => {
                                        // A dragged template must not overwrite a previous
                                        // template's interior with an offset copy.
                                        if cells
                                            .iter()
                                            .any(|(at, _, _)| self.stroke_cells.contains(at))
                                        {
                                            continue;
                                        }
                                        self.stroke_cells
                                            .extend(cells.into_iter().map(|(at, _, _)| at));
                                    }
                                    Err(e) => {
                                        self.abort_stroke(scene, e);
                                        return;
                                    }
                                }
                            } else {
                                self.stroke_cells.insert(cell);
                            }
                            let doc = self.doc.as_mut().unwrap();
                            let result = (|| -> Result<Vec<Cell>> {
                                match stroke.tool {
                                    Tool::Terrain if stroke.mode.edits_height() => Ok(vec![cell]),
                                    Tool::Terrain => {
                                        let brush = scene
                                            .brushes
                                            .get(stroke.brush)
                                            .context("请选择地形")?;
                                        terrain_tools::stamp_at_height(
                                            doc,
                                            scene,
                                            brush,
                                            cell,
                                            stroke.mode == TerrainMode::Template,
                                            stroke.template_level,
                                        )
                                    }
                                    Tool::Ore | Tool::Gems | Tool::Overlay | Tool::Erase => {
                                        let id = match stroke.tool {
                                            Tool::Ore => scene
                                                .catalog
                                                .ore
                                                .context("当前规则没有注册矿石覆盖物")?,
                                            Tool::Gems => scene
                                                .catalog
                                                .gems
                                                .context("当前规则没有注册宝石覆盖物")?,
                                            Tool::Erase => 255,
                                            _ => stroke.overlay,
                                        };
                                        doc.set(
                                            Target::Overlay(cell),
                                            Some(Value::Overlay(
                                                id,
                                                if id == 255 { 0 } else { stroke.frame },
                                            )),
                                        )?;
                                        Ok(vec![cell])
                                    }
                                    _ => Ok(Vec::new()),
                                }
                            })();
                            match result {
                                Ok(cells) => {
                                    self.feedback.extend(cells);
                                    if stroke.tool != Tool::Terrain {
                                        let v = doc.overlay(cell);
                                        if v.0 == 255 {
                                            self.overlays.remove(&cell);
                                        } else {
                                            self.overlays.insert(cell, v);
                                        }
                                    }
                                }
                                Err(e) => {
                                    self.abort_stroke(scene, e);
                                    return;
                                }
                            }
                        }
                    }
                }
            } else {
                self.last = None;
            }
        }
        if self.stroking && (release || !down) {
            let stroke = self.stroke.take().expect("active stroke settings");
            self.stroking = false;
            self.last = None;
            let result = (|| -> Result<()> {
                let doc = self.doc.as_mut().unwrap();
                if stroke.tool == Tool::Terrain {
                    if stroke.mode.edits_height() {
                        self.feedback = terrain_tools::height(
                            doc,
                            scene,
                            &self.stroke_cells,
                            stroke.mode,
                            stroke.level,
                            stroke.smooth,
                        )?
                        .into_iter()
                        .collect();
                    }
                    if stroke.lat && !self.feedback.is_empty() {
                        self.feedback.extend(terrain_tools::recalculate(
                            doc,
                            scene,
                            &self.feedback,
                        )?);
                    }
                } else if stroke.walls {
                    self.feedback
                        .extend(terrain_tools::connect_walls(doc, scene, &self.feedback)?);
                }
                Ok(())
            })();
            match result {
                Ok(()) => {
                    let name = if stroke.tool == Tool::Terrain {
                        stroke.mode.name()
                    } else {
                        stroke.tool.name()
                    };
                    self.commit(scene, name);
                    self.status = format!("{} · {} 格", name, self.feedback.len());
                }
                Err(e) => self.abort_stroke(scene, e),
            }
            self.stroke_cells.clear();
        }
    }
    fn abort_stroke(&mut self, scene: &MapScene, error: anyhow::Error) {
        self.plateau.cancel();
        self.fill.cancel();
        self.object_brush.cancel();
        if let Some(doc) = &mut self.doc {
            doc.cancel();
        }
        self.stroking = false;
        self.last = None;
        self.stroke = None;
        self.stroke_cells.clear();
        self.feedback.clear();
        self.cache(scene);
        self.status = format!("本笔未修改：{error:#}");
    }
    /// Outline the complete footprint, including invalid boundary cells, before
    /// writing. Uses geometry cached by the resource worker.
    pub fn terrain_preview(
        &mut self,
        painter: &egui::Painter,
        scene: &MapScene,
        rect: egui::Rect,
        camera: &crate::camera::MapCamera,
        at: Option<Cell>,
    ) {
        if self.tool != Tool::Terrain {
            return;
        }
        if self.terrain_mode == TerrainMode::Fill {
            self.fill_preview(painter, scene, rect, camera, at);
            return;
        }
        let (Some(at), Some(doc)) = (at, self.doc.as_ref()) else {
            return;
        };
        if self.terrain_mode.planned() {
            self.plateau_preview(painter, scene, rect, camera, at);
            return;
        }
        let mut valid = true;
        let cells = if self.terrain_mode == TerrainMode::Template {
            let Some(brush) = scene.brushes.get(self.brush) else {
                return;
            };
            match terrain_tools::template_placement(doc, scene, brush, at, self.template_level) {
                Ok(cells) => {
                    if let Some(texture) = terrain_ui::template_texture(
                        &mut self.ui_state,
                        painter.ctx(),
                        scene,
                        brush.key,
                    ) {
                        let t = &scene.palette.templates[&(brush.key.tile_id, brush.key.variant)];
                        let sub = usize::from(brush.key.sub_tile);
                        let anchor_height = t.cells[sub].unwrap().height;
                        let base =
                            i32::from(cells.iter().find(|(cell, _)| *cell == at).unwrap().1.level)
                                - i32::from(anchor_height);
                        let x = i32::from(at[0]) - sub as i32 % i32::from(t.width);
                        let y = i32::from(at[1]) - sub as i32 / i32::from(t.width);
                        let image =
                            &scene.palette.previews[&(brush.key.tile_id, brush.key.variant)];
                        let origin = egui::pos2(
                            (x - y) as f32 * 30.,
                            (x + y) as f32 * 15. - base as f32 * 15.,
                        ) + egui::vec2(
                            image.info.offset[0] as f32,
                            image.info.offset[1] as f32,
                        );
                        let world = camera.world_rect(rect);
                        let target = egui::Rect::from_min_size(
                            rect.min + (origin - world.min) * camera.zoom,
                            egui::vec2(image.info.width as f32, image.info.height as f32)
                                * camera.zoom,
                        );
                        painter.image(
                            texture.id(),
                            target,
                            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                            Color32::from_white_alpha(170),
                        );
                    }
                    cells
                        .into_iter()
                        .map(|(cell, value)| (cell, value.level))
                        .collect()
                }
                Err(error) => {
                    valid = false;
                    painter.text(
                        rect.left_top() + egui::vec2(12., 12.),
                        egui::Align2::LEFT_TOP,
                        error.to_string(),
                        egui::FontId::proportional(13.),
                        Color32::LIGHT_RED,
                    );
                    terrain_tools::footprint(scene, brush, at)
                        .map(|cells| {
                            cells
                                .into_iter()
                                .map(|(cell, _, _)| {
                                    (cell, terrain_tools::value(doc, scene, cell).level)
                                })
                                .collect()
                        })
                        .unwrap_or_else(|_| vec![(at, terrain_tools::value(doc, scene, at).level)])
                }
            }
        } else {
            let radius = self.radius;
            (0..radius)
                .flat_map(|dx| (0..radius).map(move |dy| (dx, dy)))
                .filter_map(|(dx, dy)| {
                    let x = i32::from(at[0]) + i32::from(dx) - i32::from(radius / 2);
                    let y = i32::from(at[1]) + i32::from(dy) - i32::from(radius / 2);
                    if !(0..i32::from(MAX_CELL_SIDE)).contains(&x)
                        || !(0..i32::from(MAX_CELL_SIDE)).contains(&y)
                    {
                        return None;
                    }
                    let cell = [x as u16, y as u16];
                    doc.check_cell(cell).ok()?;
                    Some((cell, terrain_tools::value(doc, scene, cell).level))
                })
                .collect()
        };
        let color = if valid {
            Color32::from_rgb(80, 220, 170)
        } else {
            Color32::from_rgb(245, 90, 80)
        };
        let world = camera.world_rect(rect);
        for (cell, level) in cells {
            let (x, y) = vera20k_core::map::terrain::iso_to_screen(cell[0], cell[1], level);
            let p = egui::pos2(x + 30., y + 15.);
            let center = rect.min + (p - world.min) * camera.zoom;
            let points = [[0., -15.], [30., 0.], [0., 15.], [-30., 0.]]
                .map(|[x, y]| center + egui::vec2(x, y) * camera.zoom);
            painter.add(egui::Shape::convex_polygon(
                points.to_vec(),
                color.gamma_multiply(0.12),
                egui::Stroke::new(1., color),
            ));
        }
    }
    pub fn markers(
        &mut self,
        painter: &egui::Painter,
        scene: &MapScene,
        rect: egui::Rect,
        camera: &crate::camera::MapCamera,
        pointer: Option<egui::Pos2>,
    ) -> Option<u64> {
        let world = camera.world_rect(rect);
        let zoom = camera.zoom;
        let document = self.doc.as_ref();
        let project = |at: Cell| {
            let level = document
                .and_then(|d| d.terrain(at))
                .map_or(scene.source.header.level as u8, |v| v.level) as i8;
            let p = egui::pos2(
                (f32::from(at[0]) - f32::from(at[1])) * 30.,
                (f32::from(at[0]) + f32::from(at[1])) * 15. + 30. - f32::from(level) * 15.,
            );
            rect.min + (p - world.min) * zoom
        };
        for &at in &self.feedback {
            let p = project(at);
            if rect.expand(50.).contains(p) {
                let points = vec![
                    p + egui::vec2(0., -15.) * zoom,
                    p + egui::vec2(30., 0.) * zoom,
                    p + egui::vec2(0., 15.) * zoom,
                    p + egui::vec2(-30., 0.) * zoom,
                ];
                painter.add(egui::Shape::convex_polygon(
                    points,
                    Color32::from_rgba_unmultiplied(70, 220, 180, 55),
                    egui::Stroke::new(1., Color32::LIGHT_GREEN),
                ));
            }
        }
        let mut hit = None;
        let mut distance = 18. * 18.;
        for marker in &self.marker_cache.markers {
            let waypoint = marker.waypoint;
            if (waypoint && !self.layers[2]) || (!waypoint && !self.layers[1]) {
                continue;
            }
            let id = marker.id;
            let origin = rect.min + (marker.origin - world.min) * zoom;
            let slot = usize::from(self.layers[1]) * marker.slots[0]
                + usize::from(self.layers[2]) * marker.slots[1];
            let p = origin - egui::vec2(0., slot as f32 * 18.);
            let image = marker.image.as_ref();
            let image_rect = marker.image_rect.map(|bounds| {
                egui::Rect::from_min_size(
                    rect.min + (bounds.min - world.min) * zoom,
                    bounds.size() * zoom,
                )
            });
            if !rect.expand(30.).contains(p)
                && image_rect.is_none_or(|image| !image.intersects(rect))
            {
                continue;
            }
            let selected = self.selected == Some(id);
            let color = if selected {
                Color32::WHITE
            } else if waypoint {
                Color32::from_rgb(90, 225, 255)
            } else {
                Color32::from_rgb(255, 160, 70)
            };
            let r = if waypoint { 8. } else { 5. };
            if p != origin {
                painter.line_segment(
                    [origin, p],
                    egui::Stroke::new(0.7, color.gamma_multiply(0.5)),
                );
            }
            if !waypoint
                && let (Some(image), Some(target)) = (image, image_rect)
                && let Some(pointer) = pointer.filter(|p| target.contains(*p))
            {
                let pixel = (pointer - target.min) / zoom;
                let offset =
                    ((pixel.y as usize * image.info.width as usize + pixel.x as usize) * 4) + 3;
                if image.rgba.get(offset).is_some_and(|a| *a != 0) {
                    hit = Some(id);
                    distance = 0.;
                }
            }
            let hovered = hit == Some(id)
                || pointer.is_some_and(|pointer| pointer.distance_sq(p) < 18. * 18.);
            if waypoint || image.is_none() || selected || hovered {
                painter.circle_filled(p, r, Color32::from_black_alpha(210));
                painter.circle_stroke(p, r, egui::Stroke::new(1.5, color));
            }
            if waypoint || selected || hovered || image.is_none() && zoom > 0.30 {
                painter.text(
                    p + egui::vec2(9., -3.),
                    egui::Align2::LEFT_BOTTOM,
                    &marker.label,
                    egui::FontId::proportional(11.),
                    color,
                );
            }
            if let Some(pointer) = pointer {
                let d = pointer.distance_sq(p);
                if d < distance || (d == distance && selected) {
                    distance = d;
                    hit = Some(id);
                }
            }
        }
        if let Some(pointer) = pointer {
            if let Some(id) = scene
                .visual
                .sample(camera.world_point(rect, pointer), self.view_options())
                .1
            {
                hit = self.marker_cache.owners.get(&id).copied();
            } else if distance == 0. {
                hit = None;
            }
        }
        hit
    }
}
#[path = "cliff_edit.rs"]
mod cliff_edit;
#[path = "editing_ui.rs"]
mod editing_ui;
#[path = "fill_edit.rs"]
mod fill_edit;
#[path = "object_brush.rs"]
mod object_brush;
#[path = "terrain_ui.rs"]
mod terrain_ui;
#[path = "tool_palette.rs"]
mod tool_palette;

#[path = "region_edit.rs"]
mod region_edit;

#[path = "object_edit.rs"]
mod object_edit;

#[path = "object_markers.rs"]
mod object_markers;
