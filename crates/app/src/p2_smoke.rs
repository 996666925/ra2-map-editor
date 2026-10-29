//! Window smoke: exercise the same commands as the interactive tools.
use super::*;
use crate::{
    document::{Cell, Target, Value},
    editing::Tool,
    editor_io::{self, Expected, Job},
};
#[path = "base_nodes_smoke.rs"]
mod base_nodes_smoke;
#[path = "map_settings_smoke.rs"]
mod map_settings_smoke;
#[derive(Default)]
pub(super) struct P2Smoke {
    p5_identity: (u64, u64),
    base_house: String,
    base_index: usize,
    settings_before: Vec<u8>,
    settings_after: Vec<u8>,
    settings_checks: Vec<serde_json::Value>,
    play_started: Option<Instant>,
    compact_started: Option<Instant>,
    new_map: bool,
    new_map_captured: bool,
    starts: Vec<(String, Cell)>,
    stage: u8,
    expected: Vec<u8>,
    document: u64,
    at: Cell,
    terrain: Vec<Cell>,
    ore: Cell,
    building: Cell,
    infantry: Cell,
    environment: Vec<(String, String, Cell)>,
    custom_overlay: Cell,
    vehicles: Vec<(String, Cell, u8)>,
    library_page: usize,
    input_events: Vec<egui::Event>,
    wait_for_input_ui: bool,
    interactions_checked: bool,
    interaction_before: Vec<u8>,
    navigator_expected: egui::Pos2,
    painted_overview: Option<egui::TextureId>,
    interaction_after_click: Vec<u8>,
    interaction_after_drag: Vec<u8>,
    viewport_checks: Vec<serde_json::Value>,
    viewport_size: Option<[u32; 2]>,
    preview_loading_frames: usize,
    export_started: bool,
    export_checked: bool,
    plateau_plan: Option<std::sync::Arc<ra2_map_scene::cliff_tools::PlateauPlan>>,
    plateau_before: Vec<u8>,
}
const LIBRARY_PAGES: [(u8, &str, &str, &str); 5] = [
    (1, "Terrain", "", "library-trees.png"),
    (1, "Smudge", "", "library-smudges.png"),
    (3, "", "", "library-overlays.png"),
    (4, "", "", "library-animations.png"),
    (0, "", "v1", "library-variants.png"),
];
impl MapView {
    pub fn smoke_input(&mut self, input: &mut egui::RawInput) {
        // eframe runs logic before UI. Inspect the interaction result only
        // after the canvas and foreground windows have processed this input.
        self.p2.wait_for_input_ui = self.p2.stage >= 60 && !self.p2.input_events.is_empty();
        input.events.append(&mut self.p2.input_events);
    }

    pub fn smoke_p2(
        &mut self,
        ctx: &egui::Context,
        state: &egui_wgpu::RenderState,
        output: &Path,
    ) -> Option<Result<()>> {
        let result = self.p2_step(ctx, state, output);
        match result {
            Ok(true) => Some(Ok(())),
            Ok(false) => {
                ctx.request_repaint_after(Duration::from_millis(16));
                None
            }
            Err(e) => Some(Err(e)),
        }
    }
    fn p2_step(
        &mut self,
        ctx: &egui::Context,
        state: &egui_wgpu::RenderState,
        output: &Path,
    ) -> Result<bool> {
        if std::mem::take(&mut self.p2.wait_for_input_ui) {
            return Ok(false);
        }
        ensure!(
            self.started.elapsed() < Duration::from_secs(120),
            "P2 烟测超时：stage {}；{}",
            self.p2.stage,
            self.editor.status
        );
        if let Some(error) = &self.error {
            anyhow::bail!("{error}");
        }
        if self.loading {
            if self.scene.is_some() {
                let gpu = self.gpu.as_ref().context("更新预览期间释放了地图 GPU")?;
                let target = self.target.as_ref().context("更新预览期间释放了地图画面")?;
                ensure!(gpu.paints.load(Ordering::Relaxed) > 0, "更新期间未绘制地图");
                ensure!(
                    target.viewport_stats().scene_renders > 0,
                    "更新期间地图画面为空"
                );
                self.p2.preview_loading_frames += 1;
            }
            return Ok(false);
        }
        let (Some(scene), Some(gpu)) = (&self.scene, &self.gpu) else {
            return Ok(false);
        };
        if gpu.paints.load(Ordering::Relaxed) < 2 {
            return Ok(false);
        }
        match self.p2.stage {
            0 => {
                std::fs::create_dir_all(output)?;
                ensure!(self.editor.doc.is_some(), "该地图不可编辑");
                if !self.p2.interactions_checked {
                    self.p2.new_map = scene.source.origin == "新建地图";
                }
                if self.p2.new_map {
                    ensure!(
                        self.editor.dirty() && self.editor.target.is_none(),
                        "新建地图必须未保存且无磁盘目标"
                    );
                    let doc = self.editor.doc.as_ref().unwrap();
                    self.p2.starts = doc
                        .records()
                        .values()
                        .filter_map(|r| {
                            (r.section == "Waypoints")
                                .then(|| r.cell().map(|cell| (r.key.clone(), cell)))
                                .flatten()
                        })
                        .collect();
                    ensure!(
                        doc.records().values().all(|r| r.section == "Waypoints"),
                        "空白地图包含对象"
                    );
                    ensure!(
                        scene.brushes.len() > scene.images.len(),
                        "空白地图的素材库仍局限于已用地形"
                    );
                    ensure!(
                        scene
                            .brushes
                            .iter()
                            .any(|b| b.label.to_ascii_lowercase().contains("road")),
                        "素材库缺少道路地形"
                    );
                    ensure!(
                        scene
                            .cells
                            .iter()
                            .all(|c| doc.terrain([c.rx, c.ry]).is_some()
                                && doc.overlay([c.rx, c.ry]) == (255, 0)),
                        "空白地图地形或覆盖物缺失"
                    );
                    std::fs::write(output.join("blank.map"), scene.source.bytes())?;
                    if !self.p2.new_map_captured {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                            egui::UserData::default(),
                        ));
                        self.p2.stage = 10;
                        return Ok(false);
                    }
                }
                let doc = self.editor.doc.as_ref().unwrap();
                let occupied: std::collections::BTreeSet<_> = doc
                    .records()
                    .values()
                    .filter(|r| r.section != "Waypoints")
                    .flat_map(|r| ra2_map_scene::placement::footprint(scene, r).unwrap_or_default())
                    .collect();
                let foundation = ra2_map_scene::placement::offsets(scene, "Structures", "GAPOWR");
                let c = scene
                    .cells
                    .iter()
                    .filter(|c| scene.is_playable(c))
                    .filter(|c| {
                        // Reserve the entire smoke fixture; mature maps can have
                        // trees at the screen center and placement now rejects them.
                        (0..12).all(|dx| {
                            (0..9).all(|dy| {
                                let at = [c.rx + dx, c.ry + dy];
                                doc.check_cell(at).is_ok() && !occupied.contains(&at)
                            })
                        }) && foundation.iter().all(|&(dx, dy)| {
                            let at = [
                                (i32::from(c.rx) + i32::from(dx)) as u16,
                                (i32::from(c.ry) + 3 + i32::from(dy)) as u16,
                            ];
                            doc.terrain(at).map(|v| v.level)
                                == doc.terrain([c.rx, c.ry + 3]).map(|v| v.level)
                        })
                    })
                    .min_by(|a, b| {
                        let distance = |c: &vera20k_core::map::terrain::TerrainCell| {
                            (egui::pos2(c.screen_x, c.screen_y) - scene.bounds.center()).length_sq()
                        };
                        distance(a).total_cmp(&distance(b))
                    })
                    .context("缺少可容纳烟测对象的空闲等高区域")?;
                let at = [c.rx, c.ry];
                self.p2.at = at;
                self.p2.document = self.editor.doc.as_ref().unwrap().id;
                let old = self.editor.doc.as_ref().unwrap().terrain(at);
                self.editor.brush = scene
                    .brushes
                    .iter()
                    .position(|b| {
                        (if self.p2.new_map {
                            b.label.to_ascii_lowercase().contains("road")
                                && !scene.images.contains_key(&b.key)
                        } else {
                            scene.catalog.defaults.contains(&b.key)
                        }) && old.is_none_or(|v| v.tile != b.raw || v.sub != b.key.sub_tile)
                    })
                    .context("缺少第二种可画地形")?;
                self.editor.tool = Tool::Terrain;
                self.editor.terrain_mode = ra2_map_scene::terrain_tools::TerrainMode::Subtile;
                self.editor.radius = 1;
                if !self.p2.interactions_checked {
                    self.p2.interaction_before =
                        self.editor.doc.as_ref().unwrap().snapshot().bytes()?;
                    self.center = egui::pos2(c.screen_x + 30., c.screen_y + 15.);
                    self.zoom = 1.;
                    self.fit = false;
                    let pointer = self
                        .workbench
                        .canvas_rect
                        .context("未布局地图画布")?
                        .center();
                    self.p2.input_events = pointer_events(pointer, &[true, false]);
                    self.p2.stage = 17;
                    return Ok(false);
                }
                self.editor
                    .pointer(scene, Some(at), true, true, false, None);
                self.editor
                    .pointer(scene, Some([at[0] + 2, at[1]]), false, true, false, None);
                self.editor.pointer(scene, None, false, false, true, None);
                self.p2.terrain = (0..3).map(|dx| [at[0] + dx, at[1]]).collect();
                let terrain = self.editor.doc.as_ref().unwrap().snapshot().bytes()?;
                self.editor.history(false, scene);
                ensure!(
                    self.editor.dirty() == self.p2.new_map
                        && self.editor.doc.as_ref().unwrap().snapshot().bytes()?
                            == scene.source.bytes(),
                    "一笔地形未能整体撤销到原文和原始保存状态"
                );
                self.editor.history(true, scene);
                ensure!(
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()? == terrain,
                    "地形 redo 不确定"
                );
                self.p2.ore = [at[0] + 5, at[1]];
                self.editor.tool = Tool::Ore;
                self.editor.radius = 3;
                self.editor
                    .pointer(scene, Some(self.p2.ore), true, true, false, None);
                self.editor.pointer(scene, None, false, false, true, None);
                self.editor.tool = Tool::Gems;
                self.editor.radius = 1;
                self.editor
                    .pointer(scene, Some([at[0] + 5, at[1] + 2]), true, true, false, None);
                self.editor.pointer(scene, None, false, false, true, None);
                self.editor.tool = Tool::Place;
                self.editor.section = "Structures".into();
                self.editor.type_id = "GAPOWR".into();
                self.editor.owner = "Neutral".into();
                self.p2.building = [at[0], at[1] + 3];
                self.editor
                    .pointer(scene, Some(self.p2.building), true, true, false, None);
                self.editor.section = "Infantry".into();
                self.editor.type_id = "E1".into();
                self.p2.infantry = [at[0] + 3, at[1] + 3];
                self.editor
                    .pointer(scene, Some(self.p2.infantry), true, true, false, None);
                ensure!(
                    self.editor
                        .doc
                        .as_ref()
                        .unwrap()
                        .records()
                        .values()
                        .any(|r| {
                            r.section == "Infantry"
                                && r.type_id() == "E1"
                                && r.cell() == Some(self.p2.infantry)
                        }),
                    "步兵放置未执行：{}",
                    self.editor.status
                );
                for (index, section) in ["Terrain", "Smudge"].into_iter().enumerate() {
                    let type_id = scene.catalog.object_types[section]
                        .iter()
                        .find(|id| {
                            scene
                                .sprites
                                .get(section, id)
                                .is_some_and(|p| p.image.is_some())
                        })
                        .context("缺少环境素材预览")?
                        .clone();
                    let cell = [at[0] + 6 + index as u16, at[1] + 3];
                    self.editor.section = section.into();
                    self.editor.type_id = type_id.clone();
                    self.editor
                        .pointer(scene, Some(cell), true, true, false, None);
                    ensure!(
                        self.editor
                            .doc
                            .as_ref()
                            .unwrap()
                            .records()
                            .values()
                            .any(|r| r.section == section && r.cell() == Some(cell)),
                        "{section} 放置失败"
                    );
                    self.editor.history(false, scene);
                    ensure!(
                        !self
                            .editor
                            .doc
                            .as_ref()
                            .unwrap()
                            .records()
                            .values()
                            .any(|r| r.section == section && r.cell() == Some(cell)),
                        "{section} 撤销失败"
                    );
                    self.editor.history(true, scene);
                    self.p2.environment.push((section.into(), type_id, cell));
                }
                // Exercise native VXL body/gun appearance in four directions on the map.
                for (index, facing) in [0u8, 64, 128, 192].into_iter().enumerate() {
                    let cell = [at[0] + index as u16 * 3, at[1] + 7];
                    let doc = self.editor.doc.as_mut().unwrap();
                    let id = doc.add_record("Units", cell, "Neutral", "HTNK")?;
                    let mut record = doc.records()[&id].clone();
                    let mut fields = record.fields();
                    fields[5] = facing.to_string();
                    record.value = fields.join(",").into_bytes();
                    doc.set(Target::Record(id), Some(Value::Record(record)))?;
                    self.p2.vehicles.push(("HTNK".into(), cell, facing));
                }
                self.editor.commit(scene, "四向体素对象");
                self.p2.custom_overlay = [at[0] + 8, at[1] + 3];
                self.editor.tool = Tool::Overlay;
                self.editor.overlay_id = 0;
                self.editor.overlay_frame = 0;
                self.editor
                    .pointer(scene, Some(self.p2.custom_overlay), true, true, false, None);
                self.editor.pointer(scene, None, false, false, true, None);
                self.editor.tool = Tool::Waypoint;
                self.editor.waypoint = 0;
                self.editor
                    .pointer(scene, Some(at), true, true, false, None);
                let doc = self.editor.doc.as_ref().unwrap();
                ensure!(
                    doc.overlay(self.p2.ore).0 == scene.catalog.ore.context("矿石类型未解析")?,
                    "矿石画笔未执行"
                );
                ensure!(
                    doc.records()
                        .values()
                        .any(|r| r.type_id() == "GAPOWR" && r.cell() == Some(self.p2.building)),
                    "建筑放置未执行：{}",
                    self.editor.status
                );
                self.p2.expected = doc.snapshot().bytes()?;
                self.p2.stage = 1;
            }
            1 => {
                let path = self
                    .editor
                    .recovery
                    .as_ref()
                    .context("没有自动恢复路径")?
                    .clone();
                if !path.is_file() {
                    return Ok(false);
                }
                let (meta, bytes) = editor_io::read_recovery(&path)?;
                ensure!(bytes == self.p2.expected, "自动恢复内容不是当前编辑快照");
                ensure!(meta.directory == self.directory, "恢复资源目录丢失");
                std::fs::copy(&path, output.join("autosave.yrrec"))?;
                self.editor.io.send(Job::Restore(path))?;
                self.p2.stage = 2;
            }
            2 => {
                let doc = self.editor.doc.as_ref().context("恢复文档缺失")?;
                if doc.id == self.p2.document {
                    return Ok(false);
                }
                ensure!(
                    doc.snapshot().bytes()? == self.p2.expected && doc.dirty(),
                    "恢复后内容或 dirty 标记错误"
                );
                let path = output.join("edited.map");
                self.editor.io.send(Job::Save {
                    directory: Some(self.directory.clone()),
                    snapshot: doc.snapshot(),
                    path,
                    expected: Expected::Capture,
                })?;
                self.editor.saving = true;
                self.p2.stage = 3;
            }
            3 => {
                if self.editor.saving {
                    return Ok(false);
                }
                ensure!(!self.editor.dirty(), "后台保存失败：{}", self.editor.status);
                let path = output.join("edited.map");
                let saved = std::fs::read(&path)?;
                let ini = vera20k_core::rules::ini_parser::IniFile::from_bytes(&saved)?;
                let preview = vera20k_core::map::preview::decode_preview_image_from_ini(&ini)?
                    .context("保存后缺少 PreviewPack")?;
                ensure!(
                    ra2_map_document::preview::embed(
                        &self.p2.expected,
                        preview.width,
                        preview.height,
                        &preview.rgba
                    )? == saved,
                    "保存内容不匹配"
                );
                self.p2.expected = saved;
                let directory = self.directory.clone();
                self.p2.stage = 4;
                self.open(directory, path.display().to_string());
            }
            4 => {
                ensure!(
                    scene.source.bytes() == self.p2.expected,
                    "保存后重开字节不匹配"
                );
                let doc = self.editor.doc.as_ref().context("重开缺少可编辑文档")?;
                ensure!(!doc.dirty(), "重开状态不干净");
                if !self.p2.export_started {
                    self.editor.io.send(Job::Export {
                        snapshot: doc.snapshot(),
                        directory: self.directory.clone(),
                        path: output.join("full-map.png"),
                        max_edge: 4096,
                        options: Default::default(),
                    })?;
                    self.editor.exporting = true;
                    self.p2.export_started = true;
                    return Ok(false);
                }
                if self.editor.exporting {
                    return Ok(false);
                }
                if !self.p2.export_checked {
                    let png = image::open(output.join("full-map.png"))?.to_rgba8();
                    let rect = scene.visual.bounds.context("没有完整场景范围")?;
                    let size = ra2_map_io::publication::fit(rect, 4096);
                    ensure!([png.width(), png.height()] == size, "后台 PNG 尺寸错误");
                    ensure!(
                        png.as_raw() == &scene.visual.raster(rect, size, Default::default())?,
                        "后台 PNG 与重开后的场景不一致"
                    );
                    let ini =
                        vera20k_core::rules::ini_parser::IniFile::from_bytes(scene.source.bytes())?;
                    let preview = vera20k_core::map::preview::decode_preview_image_from_ini(&ini)?
                        .context("重开后预览缺失")?;
                    ensure!(
                        preview.rgba
                            == scene.visual.raster(
                                rect,
                                [preview.width, preview.height],
                                Default::default()
                            )?,
                        "保存后的 PreviewPack 与实际场景不一致"
                    );
                    self.p2.export_checked = true;
                }
                for (section, type_id, cell) in &self.p2.environment {
                    ensure!(
                        doc.records().values().any(|r| &r.section == section
                            && &r.type_id() == type_id
                            && r.cell() == Some(*cell)),
                        "重开环境对象丢失：{section}"
                    );
                }
                ensure!(
                    doc.overlay(self.p2.custom_overlay) == (0, 0),
                    "重开通用覆盖物丢失"
                );
                if self.p2.new_map {
                    let records = doc.records();
                    ensure!(
                        records
                            .values()
                            .filter(|r| r.section == "Waypoints")
                            .count()
                            == self.p2.starts.len(),
                        "保存重开后起点数量改变"
                    );
                    for (key, cell) in &self.p2.starts {
                        if key != "0" {
                            ensure!(
                                records.values().any(|r| r.section == "Waypoints"
                                    && &r.key == key
                                    && r.cell() == Some(*cell)),
                                "保存重开后起点 {key} 丢失或移动"
                            );
                        }
                    }
                }
                for at in &self.p2.terrain {
                    ensure!(
                        matches!(doc.value(&Target::Terrain(*at)), Some(Value::Terrain(_))),
                        "新增地形缺失"
                    );
                }
                ensure!(
                    doc.overlay(self.p2.ore).0 == scene.catalog.ore.context("ore")?,
                    "重开矿石丢失"
                );
                for (kind, at) in [("GAPOWR", self.p2.building), ("E1", self.p2.infantry)] {
                    ensure!(
                        doc.records()
                            .values()
                            .any(|r| r.type_id() == kind && r.cell() == Some(at)),
                        "重开 {kind} 丢失"
                    );
                }
                for (type_id, cell, facing) in &self.p2.vehicles {
                    let records = doc.records();
                    let record = records
                        .values()
                        .find(|r| {
                            r.section == "Units"
                                && r.cell() == Some(*cell)
                                && &r.type_id() == type_id
                        })
                        .context("VXL 对象保存后丢失")?;
                    ensure!(record.fields()[5] == facing.to_string(), "VXL 对象朝向丢失");
                    let appearance = scene
                        .sprites
                        .objects
                        .get(&("Units".into(), type_id.clone()))
                        .context("没有地图 VXL 图像")?;
                    let image = appearance.image(*facing);
                    ensure!(
                        image.info.count == 32
                            && image.rgba.as_chunks::<4>().0.iter().any(|p| p[3] != 0),
                        "地图体素图像为空"
                    );
                }
                let shadow = scene
                    .sprites
                    .objects
                    .get(&("Structures".into(), "GAPOWR".into()))
                    .and_then(|object| object.shadow.as_ref())
                    .context("GAPOWR 建筑缺少 SHP 阴影")?;
                let shadow_pixels = shadow
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|p| p[3] != 0)
                    .count();
                ensure!(shadow_pixels > 0, "GAPOWR 阴影为空");
                ensure!(
                    shadow
                        .rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|p| p[..3] == [0, 0, 0] && matches!(p[3], 0 | 128)),
                    "阴影必须使用半透明黑色掩码"
                );
                image::save_buffer(
                    output.join("building-shadow.png"),
                    &shadow.rgba,
                    shadow.info.width,
                    shadow.info.height,
                    image::ColorType::Rgba8,
                )?;
                let gpu_checks = gpu.verify(&state.device, &state.queue, scene, output)?;
                let shared_scene_checks =
                    gpu.verify_scene(&state.device, &state.queue, scene, output)?;
                let at = self.p2.at;
                let level = doc.terrain(at).map_or(0, |v| v.level as i8);
                self.center = egui::pos2(
                    (f32::from(at[0]) - f32::from(at[1])) * 30. + 60.,
                    (f32::from(at[0]) + f32::from(at[1])) * 15. + 70. - f32::from(level) * 15.,
                );
                self.zoom = 1.;
                self.fit = false;
                self.editor.tool = Tool::Select;
                let report = serde_json::json!({"phase":"P2","passed":false,"map":scene.source.name,"edited_bytes":self.p2.expected.len(),"terrain_cells":self.p2.terrain,"ore_center":self.p2.ore,"building":self.p2.building,"infantry":self.p2.infantry,"waypoint_0":self.p2.at,"stroke_undo_redo":true,"autosave_restore":true,"async_save_reopen":true,"gpu":gpu_checks,"shared_scene":shared_scene_checks,"preview_thread":"map-loader","io_thread":"editor-io","building_shadow":{"info":shadow.info,"mask_pixels":shadow_pixels,"image":"building-shadow.png"},"visual_scope":"TMP terrain; overlay, object and waypoint editor markers"});
                std::fs::write(
                    output.join("report.json"),
                    serde_json::to_vec_pretty(&report)?,
                )?;
                self.p2.stage = 5;
            }
            5 => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.p2.stage = 6;
            }
            6 => {
                let capture = ctx.input(|i| {
                    i.events.iter().find_map(|e| {
                        if let egui::Event::Screenshot { image, .. } = e {
                            Some(image.clone())
                        } else {
                            None
                        }
                    })
                });
                if let Some(image) = capture {
                    let bytes: Vec<_> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                    image::save_buffer(
                        output.join("window.png"),
                        &bytes,
                        image.size[0] as u32,
                        image.size[1] as u32,
                        image::ColorType::Rgba8,
                    )?;
                    let selected = self.editor.doc.as_ref().and_then(|doc| {
                        doc.records().iter().find_map(|(&id, record)| {
                            (record.type_id() == "GAPOWR"
                                && record.cell() == Some(self.p2.building))
                            .then_some(id)
                        })
                    });
                    ensure!(selected.is_some(), "缺少属性面板验证对象");
                    self.editor.select(selected);
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(900., 600.)));
                    self.p2.compact_started = Some(Instant::now());
                    self.p2.stage = 8;
                }
            }
            8 => {
                // Let the native resize and the inspector's new layout settle before capture.
                if self.p2.compact_started.unwrap().elapsed() < Duration::from_millis(250) {
                    return Ok(false);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.p2.stage = 9;
            }
            9 => {
                let capture = ctx.input(|i| {
                    i.events.iter().find_map(|e| match e {
                        egui::Event::Screenshot { image, .. } => Some(image.clone()),
                        _ => None,
                    })
                });
                if let Some(image) = capture {
                    let bytes: Vec<_> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                    image::save_buffer(
                        output.join("window-compact.png"),
                        &bytes,
                        image.size[0] as u32,
                        image.size[1] as u32,
                        image::ColorType::Rgba8,
                    )?;
                    let path = output.join("report.json");
                    let mut report: serde_json::Value =
                        serde_json::from_slice(&std::fs::read(&path)?)?;
                    report["passed"] = false.into();
                    report["new_map"] = self.p2.new_map.into();
                    report["new_map_unsaved_guard"] = self.p2.new_map_captured.into();
                    if self.p2.new_map {
                        report["new_map_start_count"] = self.p2.starts.len().into();
                        report["new_map_starts_preserved"] = true.into();
                    }
                    report["compact_inspector"] = serde_json::json!({
                        "logical_size": [900, 600],
                        "selected_object": "GAPOWR",
                        "screenshot": "window-compact.png"
                    });
                    std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
                    self.p2.stage = 13;
                }
            }
            7 => {
                if self.editor.launching {
                    return Ok(false);
                }
                let process = self
                    .editor
                    .process
                    .as_mut()
                    .with_context(|| format!("试玩进程未能保持运行：{}", self.editor.status))?;
                let elapsed = self.p2.play_started.unwrap().elapsed().as_secs_f64();
                if elapsed < 8. {
                    return Ok(false);
                }
                let path = output.join("report.json");
                let mut report: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
                let configured =
                    std::fs::read_to_string(output.join("playtest-settings-spawn.ini"))?;
                ensure!(
                    std::fs::read_to_string(process.folder.join("spawn.ini"))? == configured,
                    "试玩启动配置与面板所选参数不一致"
                );
                report["playtest"] = serde_json::json!({"executable":self.editor.executable,"pid":process.child.id(),"directory":process.folder,"running_after_seconds":elapsed,"stdout":process.folder.join("stdout.log"),"stderr":process.folder.join("stderr.log")});
                report["passed"] = true.into();
                process.child.kill()?;
                process.child.wait()?;
                self.editor.process = None;
                std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
                println!("P2 smoke with playtest passed: {}", output.display());
                return Ok(true);
            }
            10 => {
                if save_capture(ctx, &output.join("blank.png"))? {
                    let document = self.editor.doc.as_ref().context("新建文档缺失")?.id;
                    self.create(
                        self.directory.clone(),
                        crate::new_map::NewMapOptions::default(),
                    );
                    ensure!(
                        matches!(self.pending, Some(Leave::New(_, _)))
                            && self
                                .editor
                                .doc
                                .as_ref()
                                .is_some_and(|doc| doc.id == document),
                        "新建另一地图未保护尚未保存的空白地图"
                    );
                    // Cancel the pending replacement, then capture the real creation form.
                    self.pending = None;
                    self.show_new_map();
                    self.p2.compact_started = Some(Instant::now());
                    self.p2.stage = 11;
                }
            }
            11 => {
                if self.p2.compact_started.unwrap().elapsed() < Duration::from_millis(250) {
                    return Ok(false);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.p2.stage = 12;
            }
            12 => {
                if !save_capture(ctx, &output.join("new-map-dialog.png"))? {
                    return Ok(false);
                }
                self.workbench.new_map = false;
                self.p2.new_map_captured = true;
                self.p2.stage = 0;
            }
            13 => {
                if let Some(&(tab, section, filter, _)) = LIBRARY_PAGES.get(self.p2.library_page) {
                    self.editor.show_library(tab, section, filter);
                    self.p2.compact_started = Some(Instant::now());
                    self.p2.stage = 14;
                } else {
                    self.p2.stage = 16;
                }
            }
            14 => {
                if self.p2.compact_started.unwrap().elapsed() < Duration::from_millis(250) {
                    return Ok(false);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.p2.stage = 15;
            }
            15 => {
                if save_capture(ctx, &output.join(LIBRARY_PAGES[self.p2.library_page].3))? {
                    self.p2.library_page += 1;
                    self.p2.stage = 13;
                }
            }
            16 => {
                ensure!(
                    self.p2.preview_loading_frames > 0,
                    "未覆盖后台更新中的地图画面"
                );
                let path = output.join("report.json");
                let mut report: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
                report["p3_terrain_tools"] = super::p3_smoke::check(ctx, scene, output)?;
                report["passed"] = false.into();
                report["environment_objects"] = serde_json::json!(self.p2.environment);
                report["general_overlay_save_reopen"] = true.into();
                report["library_screenshots"] =
                    serde_json::json!(LIBRARY_PAGES.iter().map(|p| p.3).collect::<Vec<_>>());
                report["visual_scope"] =
                    "TMP terrain; map SHP with canvas offsets, static building layers and companion-frame ground shadows; native VXL/HVA/VPL in 32 directions with guns".into();
                report["voxel_objects"] = serde_json::json!(self.p2.vehicles);
                report["publication"] = serde_json::json!({"background_png":"full-map.png","png_pixels_match_scene":self.p2.export_checked,"preview_pack_matches_reopened_scene":self.p2.export_checked});
                report["pointer_interactions"] = serde_json::json!({
                    "preview_loading_frames_with_map": self.p2.preview_loading_frames,
                    "same_frame_terrain_click": self.p2.interactions_checked,
                    "terrain_drag_with_release_movement": self.p2.interactions_checked,
                    "navigator_click": self.p2.interactions_checked,
                    "navigator_drag_and_edge_clamp": self.p2.interactions_checked,
                    "navigator_refresh_after_paint_and_undo": self.p2.interactions_checked,
                    "canvas_pixels": self.p2.viewport_checks,
                });
                std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
                self.editor.tool = Tool::Terrain;
                self.editor.terrain_mode = ra2_map_scene::terrain_tools::TerrainMode::Raise;
                self.editor.show_library(0, "", "ramp");
                self.editor.select(None);
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1280., 850.)));
                self.p2.compact_started = Some(Instant::now());
                self.p2.stage = 30;
            }
            30 => {
                if self.p2.compact_started.unwrap().elapsed() < Duration::from_millis(250) {
                    return Ok(false);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.p2.stage = 31;
            }
            31 => {
                if !save_capture(ctx, &output.join("terrain-tools-panel.png"))? {
                    return Ok(false);
                }
                let path = output.join("report.json");
                let mut report: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
                report["passed"] = false.into();
                report["p3_terrain_panel"] = "terrain-tools-panel.png".into();
                std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
                self.open_now(
                    self.directory.clone(),
                    output.join("terrain-tools.map").display().to_string(),
                );
                self.p2.stage = 32;
            }
            32 => {
                self.editor.prepare_cliff_smoke_preview(scene)?;
                let (x, y) = vera20k_core::map::terrain::iso_to_screen(80, 80, 4);
                self.center = egui::pos2(x + 30., y + 15.);
                self.zoom = 1.5;
                self.fit = false;
                self.p2.input_events = vec![egui::Event::PointerMoved(
                    self.workbench.canvas_rect.context("没有画布")?.center(),
                )];
                self.p2.compact_started = Some(Instant::now());
                self.p2.stage = 33;
            }
            33 => {
                self.p2.input_events = vec![egui::Event::PointerMoved(
                    self.workbench.canvas_rect.context("没有画布")?.center(),
                )];
                if self.hover.is_none() {
                    return Ok(false);
                }
                if self.p2.compact_started.unwrap().elapsed() < Duration::from_millis(350) {
                    return Ok(false);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.p2.stage = 34;
            }
            34 => {
                if !save_capture(ctx, &output.join("terrain-cliff-library.png"))? {
                    return Ok(false);
                }
                let path = output.join("report.json");
                let mut report: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
                report["terrain_cliff_library"] = "terrain-cliff-library.png".into();
                let cell = &scene.cells[self.hover.context("悬崖预览没有拾取到地形")?];
                let at = [cell.rx, cell.ry];
                let brush = &scene.brushes[self.editor.brush];
                let placement = ra2_map_scene::terrain_tools::template_placement(
                    self.editor.doc.as_ref().unwrap(),
                    scene,
                    brush,
                    at,
                    self.editor.template_level,
                )?;
                report["terrain_cliff_preview"] = serde_json::json!({"hovered_cell":at,"valid_cells":placement.len(),"passed":true});
                std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
                self.open_now(
                    self.directory.clone(),
                    output.join("automatic-plateaus.map").display().to_string(),
                );
                self.p2.stage = 35;
            }
            35 => {
                self.editor.show_library(0, "", "");
                self.editor.tool = Tool::Terrain;
                self.editor.terrain_mode = ra2_map_scene::terrain_tools::TerrainMode::Plateau;
                self.editor.template_level = Some(4);
                self.editor.select(None);
                let (x, y) = vera20k_core::map::terrain::iso_to_screen(89, 90, 4);
                self.center = egui::pos2(x + 30., y + 15.);
                self.zoom = 1.;
                self.fit = false;
                self.p2.plateau_before = self.editor.doc.as_ref().unwrap().snapshot().bytes()?;
                let pos = plateau_pointer(
                    self.workbench.canvas_rect.context("没有画布")?,
                    self.center,
                    self.zoom,
                    [84, 86],
                );
                self.p2.input_events = pointer_events(pos, &[]);
                self.p2.stage = 36;
            }
            36 => {
                let pos = plateau_pointer(
                    self.workbench.canvas_rect.context("没有画布")?,
                    self.center,
                    self.zoom,
                    [84, 86],
                );
                self.p2.input_events = pointer_events(pos, &[true]);
                self.p2.stage = 37;
            }
            37 => {
                let pos = plateau_pointer(
                    self.workbench.canvas_rect.context("没有画布")?,
                    self.center,
                    self.zoom,
                    [94, 94],
                );
                self.p2.input_events = pointer_events(pos, &[]);
                self.p2.compact_started = Some(Instant::now());
                self.p2.stage = 38;
            }
            38 => {
                let pos = plateau_pointer(
                    self.workbench.canvas_rect.context("没有画布")?,
                    self.center,
                    self.zoom,
                    [94, 94],
                );
                self.p2.input_events = pointer_events(pos, &[]);
                ensure!(self.editor.stroking, "高台拖动被提前提交");
                ensure!(
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()? == self.p2.plateau_before,
                    "高台拖动提前写入文档"
                );
                if self.p2.compact_started.unwrap().elapsed() < Duration::from_millis(350) {
                    return Ok(false);
                }
                let c = &scene.cells[self.hover.context("高台预览没有拾取")?];
                ensure!([c.rx, c.ry] == [94, 94], "高台拖动终点拾取错误");
                self.p2.plateau_plan = Some(self.editor.plateau_preview_plan(scene, [94, 94])?);
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.p2.stage = 39;
            }
            39 => {
                let pos = plateau_pointer(
                    self.workbench.canvas_rect.context("没有画布")?,
                    self.center,
                    self.zoom,
                    [94, 94],
                );
                self.p2.input_events = pointer_events(pos, &[]);
                if !save_capture(ctx, &output.join("automatic-plateau-preview.png"))? {
                    return Ok(false);
                }
                self.p2.input_events = pointer_events(pos, &[false]);
                self.p2.compact_started = Some(Instant::now());
                self.p2.stage = 50;
            }
            50 => {
                // App::logic runs before canvas UI consumes this frame's
                // injected release. Let that UI pass complete first.
                if self.p2.compact_started.unwrap().elapsed() < Duration::from_millis(150) {
                    return Ok(false);
                }
                ensure!(!self.editor.stroking, "高台鼠标释放没有提交");
                let plan = self.p2.plateau_plan.as_ref().context("高台预览缺失")?;
                let doc = self.editor.doc.as_ref().unwrap();
                for (&at, &v) in &plan.edits {
                    ensure!(doc.terrain(at) == Some(v), "实际鼠标释放与高台预览不一致");
                }
                if self.p2.compact_started.unwrap().elapsed() < Duration::from_millis(500) {
                    return Ok(false);
                }
                std::fs::write(
                    output.join("automatic-plateaus-pointer.map"),
                    doc.snapshot().bytes()?,
                )?;
                let path = output.join("report.json");
                let mut report: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
                report["automatic_plateau_pointer"] = serde_json::json!({"passed":true,"raw_mouse_drag":true,"preview_no_writes":true,"preview_matches_release":true,"cells":plan.edits.len(),"cliff_stamps":plan.cliffs.len(),"preview":"automatic-plateau-preview.png","committed":"automatic-plateau-committed.png"});
                std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.p2.stage = 51;
            }
            51 => {
                if !save_capture(ctx, &output.join("automatic-plateau-committed.png"))? {
                    return Ok(false);
                }
                self.p2.plateau_plan = None;
                self.open_now(
                    self.directory.clone(),
                    output.join("region-objects.map").display().to_string(),
                );
                self.p2.stage = 40;
            }
            40 => {
                self.editor.prepare_region_smoke_preview(scene);
                let (x, y) = vera20k_core::map::terrain::iso_to_screen(90, 105, 4);
                self.center = egui::pos2(x + 30., y + 15.);
                self.zoom = 1.;
                self.fit = false;
                self.p2.input_events = vec![egui::Event::PointerMoved(
                    self.workbench.canvas_rect.context("没有画布")?.center(),
                )];
                self.p2.stage = 41;
            }
            41 => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.p2.stage = 42;
            }
            42 => {
                if !save_capture(ctx, &output.join("region-paste-preview.png"))? {
                    return Ok(false);
                }
                ensure!(
                    self.editor.region_preview_is_valid(),
                    "真实窗口没有产生完整区域粘贴预览"
                );
                let path = output.join("report.json");
                let mut report: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
                report["passed"] = true.into();
                report["region_paste_preview"] = "region-paste-preview.png".into();
                std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
                let doc = self.editor.doc.as_mut().context("no document")?;
                let before = doc.snapshot().bytes()?;
                let baseline_issues = ra2_map_document::mission::issues(doc).len();
                let trigger = ra2_map_document::mission::seed(doc)?;
                doc.commit("P4 mission smoke");
                let bytes = doc.snapshot().bytes()?;
                ensure!(
                    ra2_map_document::mission::issues(doc).len() <= baseline_issues,
                    "mission introduced invalid references: {:?}",
                    ra2_map_document::mission::issues(doc)
                );
                ensure!(
                    doc.undo() && doc.snapshot().bytes()? == before,
                    "mission undo mismatch"
                );
                ensure!(
                    doc.redo() && doc.snapshot().bytes()? == bytes,
                    "mission redo mismatch"
                );
                std::fs::write(output.join("mission.map"), bytes)?;
                self.workbench.mission.select_trigger(&self.editor, trigger);
                self.p2.compact_started = Some(Instant::now());
                self.p2.stage = 43;
            }
            43 => {
                if self.p2.compact_started.unwrap().elapsed() < Duration::from_millis(400) {
                    return Ok(false);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.p2.stage = 44;
            }
            44 => {
                if !save_capture(ctx, &output.join("mission-panel.png"))? {
                    return Ok(false);
                }
                let path = output.join("report.json");
                let mut report: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
                let definitions = &scene.mission_definitions;
                ensure!(
                    definitions.error.is_none(),
                    "fixed AI definitions: {:?}",
                    definitions.error
                );
                ensure!(
                    !definitions.teams.is_empty()
                        && !definitions.scripts.is_empty()
                        && !definitions.task_forces.is_empty(),
                    "empty fixed AI registry"
                );
                report["p4_mission"] = serde_json::json!({"seed_undo_redo":true,"references_valid":true,"map":"mission.map","panel":"mission-panel.png","fixed_ai":{"source":definitions.source,"teams":definitions.teams.len(),"scripts":definitions.scripts.len(),"task_forces":definitions.task_forces.len()}});
                report["p4_authoring"] = self
                    .workbench
                    .mission
                    .authoring_smoke(&mut self.editor, scene)?;
                std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
                self.workbench.mission.open = false;
                // Region/mission smoke uses its own two-player fixture. Return
                // to the authored extended map to inspect its playtest limits.
                if self.p2.new_map && self.p2.starts.len() > 8 {
                    self.open_now(
                        self.directory.clone(),
                        output.join("blank.map").display().to_string(),
                    );
                }
                self.p2.stage = 49;
            }
            49 => {
                self.editor.play_options.mission = false;
                self.editor.play_options.ai = 1;
                if self.p2.new_map && self.p2.starts.len() > 8 {
                    self.editor.play_options.ai = self.p2.starts.len() as u8 - 1;
                    self.editor.play_options.start = self
                        .p2
                        .starts
                        .iter()
                        .filter_map(|(key, _)| key.parse::<u8>().ok())
                        .max()
                        .unwrap();
                }
                self.editor.play_options.ai_side = 8;
                self.editor.play_options.difficulty = 0;
                self.editor.play_options.speed = 2;
                self.workbench.playtest = true;
                self.p2.compact_started = Some(Instant::now());
                self.p2.stage = 45;
            }
            45 => {
                if self.p2.compact_started.unwrap().elapsed() < Duration::from_millis(400) {
                    return Ok(false);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.p2.stage = 46;
            }
            46 => {
                if !save_capture(ctx, &output.join("playtest-settings.png"))? {
                    return Ok(false);
                }
                let map = vera20k_core::map::map_file::load_from_bytes(
                    self.editor
                        .doc
                        .as_ref()
                        .context("no playtest map")?
                        .snapshot()
                        .bytes()?,
                )?;
                let options = &self.editor.play_options;
                std::fs::write(
                    output.join("playtest-settings-spawn.ini"),
                    options.spawn_ini(&map)?,
                )?;
                let path = output.join("report.json");
                let mut report: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
                report["playtest_settings"] = serde_json::json!({"panel":"playtest-settings.png",
                    "spawn_ini":"playtest-settings-spawn.ini","ai":options.ai,"total_players":options.ai+1,
                    "player_start":options.start,"player_color":options.color,"ai_country":options.ai_side,
                    "ai_handicap":options.difficulty,"game_speed":options.speed});
                report["passed"] = (!self.p2_playtest).into();
                std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
                if self.p2_playtest {
                    self.editor.play();
                    self.p2.play_started = Some(Instant::now());
                    self.p2.stage = 7;
                    return Ok(false);
                }
                self.workbench.playtest = false;
                self.p2.stage = 60;
            }
            60 => {
                self.editor.select(None);
                self.editor.tool = Tool::Erase;
                self.center = scene.bounds.center();
                self.zoom = 1.;
                self.fit = false;
                self.p2.navigator_expected = self.center - egui::vec2(60., 30.);
                self.p2.interaction_before =
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()?;
                let canvas = self.workbench.canvas_rect.context("no canvas")?;
                self.p2.input_events = vec![key_event(egui::Key::Space, true)];
                self.p2
                    .input_events
                    .extend(pointer_events(canvas.center(), &[true]));
                self.p2.stage = 61;
            }
            61 => {
                let canvas = self.workbench.canvas_rect.context("no canvas")?;
                self.p2.input_events = vec![egui::Event::PointerMoved(
                    canvas.center() + egui::vec2(60., 30.),
                )];
                self.p2.stage = 62;
            }
            62 => {
                let canvas = self.workbench.canvas_rect.context("no canvas")?;
                self.p2.input_events =
                    pointer_events(canvas.center() + egui::vec2(60., 30.), &[false]);
                self.p2
                    .input_events
                    .push(key_event(egui::Key::Space, false));
                self.p2.stage = 63;
            }
            63 => {
                ensure!(
                    self.center.distance(self.p2.navigator_expected) < 0.1,
                    "space drag did not pan accurately: {:?} vs {:?}",
                    self.center,
                    self.p2.navigator_expected
                );
                ensure!(
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()?
                        == self.p2.interaction_before,
                    "space pan edited the map"
                );
                self.p2.input_events = vec![
                    key_event(egui::Key::R, true),
                    key_event(egui::Key::R, false),
                ];
                self.p2.stage = 64;
            }
            64 => {
                ensure!(
                    self.editor.tool == Tool::Region,
                    "R did not select region tool"
                );
                self.p2.input_events = vec![
                    key_event(egui::Key::F1, true),
                    key_event(egui::Key::F1, false),
                ];
                self.p2.stage = 65;
            }
            65 => {
                ensure!(self.workbench.help, "F1 did not open help");
                self.p2.compact_started = Some(Instant::now());
                self.p2.input_events = vec![
                    key_event(egui::Key::E, true),
                    key_event(egui::Key::E, false),
                ];
                self.p2.stage = 66;
            }
            66 => {
                if self.p2.compact_started.unwrap().elapsed() < Duration::from_millis(250) {
                    return Ok(false);
                }
                ensure!(
                    self.editor.tool == Tool::Region,
                    "help window allowed background tool shortcut"
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.p2.stage = 67;
            }
            67 => {
                if !save_capture(ctx, &output.join("interaction-help.png"))? {
                    return Ok(false);
                }
                self.p2.input_events = vec![
                    key_event(egui::Key::Escape, true),
                    key_event(egui::Key::Escape, false),
                ];
                self.p2.stage = 68;
            }
            68 => {
                ensure!(
                    !self.workbench.help && self.editor.tool == Tool::Region,
                    "Escape closed help and also affected the map"
                );
                self.editor.tool = Tool::Select;
                let pos = self.workbench.canvas_rect.context("no canvas")?.center();
                self.p2.input_events = vec![egui::Event::PointerMoved(pos)];
                for pressed in [true, false] {
                    self.p2.input_events.push(egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                self.p2.stage = 69;
                self.p2.compact_started = Some(Instant::now());
            }
            69 => {
                if self.p2.compact_started.unwrap().elapsed() < Duration::from_millis(250) {
                    return Ok(false);
                }
                ensure!(
                    egui::Popup::is_any_open(ctx),
                    "right click did not open context menu"
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.p2.stage = 70;
            }
            70 => {
                if !save_capture(ctx, &output.join("interaction-context-menu.png"))? {
                    return Ok(false);
                }
                self.p2.input_events = vec![
                    key_event(egui::Key::Escape, true),
                    key_event(egui::Key::Escape, false),
                ];
                self.p2.stage = 71;
            }
            71 => {
                ensure!(
                    !egui::Popup::is_any_open(ctx),
                    "Escape did not close context menu"
                );
                self.editor.tool = Tool::Terrain;
                self.editor.terrain_mode = ra2_map_scene::terrain_tools::TerrainMode::Raise;
                self.editor.smooth_heights = false;
                self.editor.auto_lat = false;
                let pos = self.workbench.canvas_rect.context("no canvas")?.center();
                self.p2.input_events = pointer_events(pos, &[true]);
                self.p2.stage = 72;
            }
            72 => {
                ensure!(self.editor.stroking, "mouse press did not start stroke");
                let pos = self.workbench.canvas_rect.context("no canvas")?.center();
                self.p2.input_events = vec![egui::Event::WindowFocused(false)];
                self.p2.input_events.extend(pointer_events(pos, &[false]));
                self.p2.stage = 73;
            }
            73 => {
                ensure!(!self.editor.stroking, "focus loss left an active stroke");
                ensure!(
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()?
                        == self.p2.interaction_before,
                    "focus loss committed an unfinished stroke"
                );
                let path = output.join("report.json");
                let mut report: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
                report["interaction"] = serde_json::json!({"space_drag_pan_no_writes":true,"tool_shortcuts":true,"help_blocks_background_keys":true,"escape_closes_foreground_only":true,"right_click_context_menu":true,"focus_loss_cancels_stroke":true,"help":"interaction-help.png","context_menu":"interaction-context-menu.png"});
                report["passed"] = false.into();
                std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
                self.p2.stage = 74;
            }
            74..=79 | 81..=87 => return self.base_nodes_smoke_step(ctx, output),
            90..=101 => return self.map_settings_smoke_step(ctx, state, output),
            80 => return self.ui_smoke_step(ctx, output),
            17 => {
                self.p2.stage = 18;
            }
            18 => {
                let doc = self.editor.doc.as_ref().unwrap();
                let brush = &scene.brushes[self.editor.brush];
                let value = doc.terrain(self.p2.at).context("快速点击未写入地形")?;
                ensure!(
                    value.tile == brush.raw && value.sub == brush.key.sub_tile,
                    "同帧按下/松开漏刷地形"
                );
                ensure!(
                    doc.snapshot().bytes()? != self.p2.interaction_before,
                    "鼠标绘制未改变地图"
                );
                ensure!(
                    scene
                        .cells
                        .iter()
                        .any(|cell| [cell.rx, cell.ry] == self.p2.at
                            && cell.tile_id == brush.key.tile_id
                            && cell.sub_tile == brush.key.sub_tile),
                    "刷地形后场景未刷新"
                );
                ensure!(
                    self.overview_texture_id().is_some(),
                    "编辑后小地图未生成纹理"
                );
                self.p2.viewport_size = Some(self.target.as_ref().context("没有主画布目标")?.size);
                self.p2
                    .viewport_checks
                    .push(check_viewport(self, state, self.p2.at, output, "click")?);
                self.p2.painted_overview = self.overview_texture_id();
                self.p2.interaction_after_click = doc.snapshot().bytes()?;
                let canvas = self.workbench.canvas_rect.context("未布局地图画布")?;
                self.p2.input_events = pointer_events(canvas.center(), &[true]);
                self.p2.stage = 26;
            }
            26 => {
                let canvas = self.workbench.canvas_rect.context("未布局地图画布")?;
                let end = canvas.center() + egui::vec2(60., 30.);
                self.p2.input_events = pointer_events(end, &[false]);
                self.p2.stage = 27;
            }
            27 => {
                self.p2.stage = 28;
            }
            28 => {
                let doc = self.editor.doc.as_ref().unwrap();
                let brush = &scene.brushes[self.editor.brush];
                for dx in 0..3 {
                    let value = doc
                        .terrain([self.p2.at[0] + dx, self.p2.at[1]])
                        .context("鼠标拖动漏格")?;
                    ensure!(
                        value.tile == brush.raw && value.sub == brush.key.sub_tile,
                        "鼠标拖动末帧未刷到目标格"
                    );
                }
                let after_drag = doc.snapshot().bytes()?;
                let at = [self.p2.at[0] + 2, self.p2.at[1]];
                self.p2
                    .viewport_checks
                    .push(check_viewport(self, state, at, output, "drag")?);
                self.p2.interaction_after_drag = after_drag;
                self.editor.history(false, scene);
                self.p2.stage = 47;
            }
            47 => {
                ensure!(
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()?
                        == self.p2.interaction_after_click,
                    "鼠标拖动未合并为一笔撤销"
                );
                let at = [self.p2.at[0] + 2, self.p2.at[1]];
                self.p2
                    .viewport_checks
                    .push(check_viewport(self, state, at, output, "undo")?);
                self.editor.history(true, scene);
                self.p2.stage = 48;
            }
            48 => {
                ensure!(
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()?
                        == self.p2.interaction_after_drag,
                    "鼠标拖动重做不一致"
                );
                let at = [self.p2.at[0] + 2, self.p2.at[1]];
                self.p2
                    .viewport_checks
                    .push(check_viewport(self, state, at, output, "redo")?);
                self.editor.history(false, scene);
                let rect = self.workbench.overview_rect.context("未布局导航预览")?;
                let pointer = rect.min + rect.size() * egui::vec2(0.27, 0.38);
                self.p2.navigator_expected =
                    scene.bounds.min + scene.bounds.size() * egui::vec2(0.27, 0.38);
                self.p2.input_events = pointer_events(pointer, &[true, false]);
                self.p2.stage = 19;
            }
            19 => {
                self.p2.stage = 20;
            }
            20 => {
                ensure!(
                    self.center.distance(self.p2.navigator_expected) < 0.1,
                    "小地图点击定位不准确"
                );
                let rect = self.workbench.overview_rect.context("未布局导航预览")?;
                self.p2.input_events = pointer_events(rect.center(), &[true]);
                self.p2.stage = 21;
            }
            21 => {
                let rect = self.workbench.overview_rect.context("未布局导航预览")?;
                self.p2.input_events =
                    vec![egui::Event::PointerMoved(rect.max + egui::vec2(12., 8.))];
                self.p2.stage = 22;
            }
            22 => {
                let rect = self.workbench.overview_rect.context("未布局导航预览")?;
                self.p2.input_events = pointer_events(rect.max + egui::vec2(12., 8.), &[false]);
                self.p2.stage = 23;
            }
            23 => {
                self.p2.stage = 24;
            }
            24 => {
                ensure!(
                    self.center.distance(scene.bounds.max) < 0.1,
                    "小地图拖出边界没有正确钳制定位"
                );
                self.editor.history(false, scene);
                ensure!(
                    self.editor.doc.as_ref().unwrap().snapshot().bytes()?
                        == self.p2.interaction_before,
                    "鼠标一笔撤销未恢复原文"
                );
                self.p2.interactions_checked = true;
                self.p2.stage = 25;
            }
            25 => {
                ensure!(self.overview_texture_id().is_some(), "撤销后小地图未刷新");
                ensure!(
                    self.overview_texture_id() != self.p2.painted_overview,
                    "撤销后小地图仍使用编辑时的缓存纹理"
                );
                self.p2.stage = 0;
            }
            _ => {}
        }
        Ok(false)
    }
}

/// Inspect the existing canvas attachment after normal callback painting. A
/// fresh offscreen target or a document-only assertion misses stale GPU bindings.
fn check_viewport(
    view: &MapView,
    state: &egui_wgpu::RenderState,
    at: Cell,
    output: &Path,
    label: &str,
) -> Result<serde_json::Value> {
    let scene = view.scene.as_ref().context("没有场景")?;
    let target = view.target.as_ref().context("没有主画布目标")?;
    ensure!(
        Some(target.size) == view.p2.viewport_size,
        "绘制回归期间画布尺寸发生变化"
    );
    let canvas = view.workbench.canvas_rect.context("没有画布区域")?;
    let camera = crate::camera::MapCamera {
        center: view.center,
        zoom: view.zoom,
    };
    let world = camera.world_rect(canvas);
    let cell = scene
        .cells
        .iter()
        .find(|c| [c.rx, c.ry] == at)
        .context("画布采样格不存在")?;
    let point = scene.cell_rect(cell).context("采样格缺少图像")?.center();
    let extent = egui::vec2(target.size[0] as f32, target.size[1] as f32);
    let center = (point - world.min) / world.size() * extent;
    let origin = [center.x, center.y].map(|n| (n - 32.).max(0.) as u32);
    ensure!(
        (0..2).all(|i| origin[i] + 64 <= target.size[i]),
        "采样格不在当前画布内"
    );
    let actual = target.read_region(&state.device, &state.queue, origin, [64, 64])?;
    let mut expected = Vec::with_capacity(actual.len());
    let mut checked = 0;
    let mut error = 0;
    for y in 0..64 {
        for x in 0..64 {
            let uv =
                egui::vec2((origin[0] + x) as f32 + 0.5, (origin[1] + y) as f32 + 0.5) / extent;
            let point = world.min + uv * world.size();
            let pixel = scene.visual.sample(point, view.editor.view_options()).0;
            expected.extend_from_slice(&pixel);
            // Avoid integer texel boundaries where f32 rasterization rounding
            // can legitimately select a neighboring texel at fractional DPI.
            if (point.x - point.x.round()).abs() > 0.02 && (point.y - point.y.round()).abs() > 0.02
            {
                let start = ((y * 64 + x) * 4) as usize;
                error = error.max(
                    actual[start..start + 4]
                        .iter()
                        .zip(pixel)
                        .map(|(a, b)| a.abs_diff(b))
                        .max()
                        .unwrap(),
                );
                checked += 1;
            }
        }
    }
    for (name, bytes) in [("actual", &actual), ("expected", &expected)] {
        image::save_buffer(
            output.join(format!("canvas-{label}-{name}.png")),
            bytes,
            64,
            64,
            image::ColorType::Rgba8,
        )?;
    }
    ensure!(
        checked > 100 && error <= 2,
        "主画布仍显示旧场景或像素不匹配：{label},pixels={checked},error={error}"
    );
    Ok(
        serde_json::json!({"operation":label,"cell":at,"viewport_size":target.size,
        "pixels_checked":checked,"max_rgba_error":error,"actual":format!("canvas-{label}-actual.png")}),
    )
}

fn plateau_pointer(rect: egui::Rect, center: egui::Pos2, zoom: f32, at: Cell) -> egui::Pos2 {
    let (x, y) = vera20k_core::map::terrain::iso_to_screen(at[0], at[1], 4);
    rect.center() + (egui::pos2(x + 30., y + 15.) - center) * zoom
}

fn pointer_events(pos: egui::Pos2, buttons: &[bool]) -> Vec<egui::Event> {
    std::iter::once(egui::Event::PointerMoved(pos))
        .chain(buttons.iter().map(|&pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }))
        .collect()
}

fn key_event(key: egui::Key, pressed: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

fn save_capture(ctx: &egui::Context, path: &Path) -> Result<bool> {
    let capture = ctx.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Screenshot { image, .. } => Some(image.clone()),
            _ => None,
        })
    });
    let Some(image) = capture else {
        return Ok(false);
    };
    let bytes: Vec<_> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    image::save_buffer(
        path,
        &bytes,
        image.size[0] as u32,
        image.size[1] as u32,
        image::ColorType::Rgba8,
    )?;
    Ok(true)
}
