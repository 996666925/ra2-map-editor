//! Region interaction and overlays; document transforms and placement rules live below app.
use super::*;
use ra2_map_document::region::{self, Clipboard, RegionLayers};
use ra2_map_scene::placement;
use vera20k_core::map::cell_index::MAX_CELL_SIDE;

#[derive(Clone, Copy, Default, PartialEq)]
enum SelectionOp {
    #[default]
    Replace,
    Add,
    Subtract,
}
#[derive(Default)]
pub(super) struct RegionEdit {
    pub cells: BTreeSet<Cell>,
    pub(super) clipboard: Option<Clipboard>,
    pub layers: RegionLayers,
    custom: bool,
    op: SelectionOp,
    drag: Option<SelectionDrag>,
    input_op: Option<SelectionOp>,
    revision: u64,
    preview: Option<Preview>,
}
struct SelectionDrag {
    start: Cell,
    last: Cell,
    before: BTreeSet<Cell>,
    op: SelectionOp,
    custom: bool,
}

type PreviewGeometry = (
    Vec<(Cell, u8)>,
    Vec<Record>,
    Vec<(Cell, crate::document::TerrainValue)>,
);
struct Preview {
    key: String,
    cells: Vec<(Cell, u8)>,
    records: Vec<Record>,
    ground: Vec<(Cell, crate::document::TerrainValue)>,
    error: Option<String>,
}
impl RegionEdit {
    pub(super) fn invalidate_preview(&mut self) {
        self.preview = None;
        self.revision += 1;
    }
    pub fn cancel(&mut self) {
        self.input_op = None;
        if let Some(drag) = self.drag.take() {
            self.cells = drag.before;
        }
        self.preview = None;
        self.revision += 1;
    }
}
#[derive(Clone, Copy)]
pub(super) enum Action {
    Copy,
    Cut,
    Paste,
    Move,
    Delete,
    Clone,
}
impl Editor {
    pub(crate) fn region_modifiers(&mut self, modifiers: egui::Modifiers) {
        self.region.input_op = if modifiers.alt {
            Some(SelectionOp::Subtract)
        } else if modifiers.shift {
            Some(SelectionOp::Add)
        } else {
            None
        };
    }
    pub(super) fn region_action(&mut self, scene: &MapScene, action: Action) {
        if !self.check_property_draft() {
            return;
        }
        if self.stroking {
            self.status = "请先完成当前操作".into();
            return;
        }
        let result = (|| -> Result<()> {
            if matches!(action, Action::Clone) {
                ensure!(self.selected.is_some(), "请先选择要克隆的对象");
                self.activate_tool(Tool::Clone);
                return Ok(());
            }
            if matches!(action, Action::Paste) {
                ensure!(self.region.clipboard.is_some(), "请先复制区域");
                self.tool = Tool::Paste;
                return Ok(());
            }
            ensure!(!self.region.cells.is_empty(), "请先框选区域");
            if matches!(action, Action::Move) {
                self.tool = Tool::RegionMove;
                return Ok(());
            }
            let doc = self.doc.as_mut().context("没有可编辑地图")?;
            let clip = Clipboard::capture(
                doc,
                &self.region.cells,
                self.region
                    .layers
                    .visible(self.layers)
                    .with_cell_tags_visible(self.task_labels),
            )?;
            if matches!(action, Action::Copy | Action::Cut) {
                self.region.clipboard = Some(clip.clone());
            }
            if matches!(action, Action::Cut | Action::Delete) {
                let clear = scene
                    .palette
                    .tile_ids
                    .and_then(|ids| u16::try_from(ids.clear).ok())
                    .and_then(|id| scene.catalog.raw_tiles.get(&id))
                    .copied()
                    .unwrap_or(0);
                doc.atomic(|doc| {
                    region::clear(doc, &clip, clear)?;
                    if clip.layers.terrain && self.auto_lat {
                        terrain_tools::recalculate(doc, scene, &self.region.cells)?;
                    }
                    if clip.layers.overlays && self.auto_walls {
                        terrain_tools::connect_walls(doc, scene, &self.region.cells)?;
                    }
                    Ok(())
                })?;
                self.commit(
                    scene,
                    if matches!(action, Action::Cut) {
                        "剪切区域"
                    } else {
                        "删除区域"
                    },
                );
            }
            self.status = format!(
                "{} 格、{} 个对象{}",
                clip.cells.len(),
                clip.records.len(),
                if matches!(action, Action::Copy | Action::Cut) {
                    "已复制；Ctrl+V 后点击目标位置"
                } else {
                    "已删除"
                }
            );
            Ok(())
        })();
        if let Err(e) = result {
            self.status = format!("{e:#}");
        }
        self.region.preview = None;
        self.region.revision += 1;
    }
    fn paste_region(&mut self, scene: &MapScene, at: Cell, moving: bool) -> Result<()> {
        let doc = self.doc.as_mut().context("没有可编辑地图")?;
        let mut clip = if moving {
            Clipboard::capture(
                doc,
                &self.region.cells,
                self.region
                    .layers
                    .visible(self.layers)
                    .with_cell_tags_visible(self.task_labels),
            )?
        } else {
            self.region
                .clipboard
                .as_ref()
                .context("请先复制区域")?
                .clone()
        };
        clip.layers.cell_tags &= self.task_labels;
        let plan = clip.plan(doc, at)?;
        let ignored = if moving {
            clip.records.iter().map(|(id, _)| *id).collect()
        } else {
            BTreeSet::new()
        };
        let records: Vec<_> = plan.records.iter().map(|(_, r)| r.clone()).collect();
        let ground = plan
            .cells
            .iter()
            .filter_map(|c| c.terrain.map(|v| (c.at, v)))
            .collect();
        placement::validate(doc, scene, &records, &ignored, self.allow_overlap, &ground)?;
        let clear = scene
            .palette
            .tile_ids
            .and_then(|ids| u16::try_from(ids.clear).ok())
            .and_then(|id| scene.catalog.raw_tiles.get(&id))
            .copied()
            .unwrap_or(0);
        let mut touched: BTreeSet<_> = plan.cells.iter().map(|c| c.at).collect();
        if moving {
            touched.extend(clip.cells.iter().map(|c| c.at));
        }
        doc.atomic(|doc| {
            if moving {
                region::clear(doc, &clip, clear)?;
            }
            plan.apply(doc, !moving)?;
            if clip.layers.terrain && self.auto_lat {
                terrain_tools::recalculate(doc, scene, &touched)?;
            }
            if clip.layers.overlays && self.auto_walls {
                terrain_tools::connect_walls(doc, scene, &touched)?;
            }
            Ok(())
        })?;
        self.feedback = touched;
        if moving {
            self.region.cells = plan.cells.iter().map(|c| c.at).collect();
            self.tool = Tool::Region;
        }
        self.selected = None;
        self.commit(
            scene,
            if moving {
                "移动区域"
            } else {
                "粘贴区域"
            },
        );
        self.region.preview = None;
        self.region.revision += 1;
        self.status = "区域操作完成，可整笔撤销".into();
        Ok(())
    }
    pub(super) fn region_pointer(
        &mut self,
        scene: &MapScene,
        at: Option<Cell>,
        press: bool,
        down: bool,
        release: bool,
    ) -> bool {
        if matches!(self.tool, Tool::Paste | Tool::RegionMove) && self.region.drag.is_none() {
            if press
                && let Some(at) = at
                && let Err(e) = self.paste_region(scene, at, self.tool == Tool::RegionMove)
            {
                self.status = format!("未修改：{e:#}");
            }
            return true;
        }
        if self.tool != Tool::Region && self.region.drag.is_none() {
            return false;
        }
        if press && let Some(at) = at {
            self.select(None);
            let op = self.region.input_op.take().unwrap_or(self.region.op);
            self.region.drag = Some(SelectionDrag {
                start: at,
                last: at,
                before: self.region.cells.clone(),
                op,
                custom: self.region.custom,
            });
            if op == SelectionOp::Replace {
                self.region.cells.clear();
            }
            self.stroking = true;
        }
        if let Some(SelectionDrag {
            start,
            last,
            before,
            op,
            custom,
        }) = self.region.drag.as_mut()
            && let Some(at) = at
        {
            let selected: Vec<_> = if *custom {
                cell_line(*last, at)
            } else {
                (start[0].min(at[0])..=start[0].max(at[0]))
                    .flat_map(|x| (start[1].min(at[1])..=start[1].max(at[1])).map(move |y| [x, y]))
                    .collect()
            };
            if !*custom {
                self.region.cells = if *op == SelectionOp::Replace {
                    BTreeSet::new()
                } else {
                    before.clone()
                };
            }
            for cell in selected {
                if self
                    .doc
                    .as_ref()
                    .is_some_and(|doc| doc.check_cell(cell).is_ok())
                {
                    if *op == SelectionOp::Subtract {
                        self.region.cells.remove(&cell);
                    } else {
                        self.region.cells.insert(cell);
                    }
                }
            }
            *last = at;
            self.region.revision += 1;
        }
        if self.region.drag.is_some() && (release || !down) {
            self.region.drag = None;
            self.stroking = false;
            self.status = format!(
                "已选 {} 格；Ctrl+C 复制，Ctrl+X 剪切，Delete 删除所选层",
                self.region.cells.len()
            );
        }
        true
    }
    pub(super) fn region_options(&mut self, ui: &mut egui::Ui, scene: &MapScene) {
        ui.label(format!("选区：{} 格", self.region.cells.len()));
        ui.horizontal_wrapped(|ui| {
            for (label, action) in [
                ("复制", Action::Copy),
                ("剪切", Action::Cut),
                ("粘贴", Action::Paste),
                ("移动", Action::Move),
                ("删除", Action::Delete),
            ] {
                let enabled = !self.stroking
                    && if matches!(action, Action::Paste) {
                        self.region.clipboard.is_some()
                    } else {
                        !self.region.cells.is_empty()
                    };
                if ui
                    .add_enabled(enabled, egui::Button::new(label))
                    .on_disabled_hover_text(
                        "先框选区域；粘贴需要已复制的区域。拖动中请先松开鼠标。",
                    )
                    .clicked()
                {
                    self.region_action(scene, action);
                    if matches!(action, Action::Copy | Action::Cut) {
                        self.export_region_clipboard(ui.ctx());
                    }
                }
            }
        });
        if self.tool == Tool::Region {
            ui.small("按住 Shift 添加选区，Alt 减选；松开后恢复下方选择方式。");
            ui.checkbox(&mut self.region.custom, "自定义选区（拖动涂选）");
            egui::ComboBox::from_id_salt("selection-op")
                .selected_text(match self.region.op {
                    SelectionOp::Replace => "重新选择",
                    SelectionOp::Add => "添加选区",
                    SelectionOp::Subtract => "减去选区",
                })
                .show_ui(ui, |ui| {
                    for (op, name) in [
                        (SelectionOp::Replace, "重新选择"),
                        (SelectionOp::Add, "添加选区"),
                        (SelectionOp::Subtract, "减去选区"),
                    ] {
                        ui.selectable_value(&mut self.region.op, op, name);
                    }
                });
        }
        egui::CollapsingHeader::new("参与操作的数据层")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.region.layers.terrain, "地形与高度");
                    ui.add_enabled(
                        self.layers[0],
                        egui::Checkbox::new(&mut self.region.layers.overlays, "覆盖物 / 资源"),
                    );
                });
                for (flags, names) in self.region.layers.objects.chunks_mut(2).zip([
                    ["建筑", "车辆"],
                    ["步兵", "飞机"],
                    ["树木", "污迹"],
                ]) {
                    ui.horizontal(|ui| {
                        for (flag, name) in flags.iter_mut().zip(names) {
                            ui.add_enabled(self.layers[1], egui::Checkbox::new(flag, name));
                        }
                    });
                }
                ui.add_enabled(
                    self.task_labels,
                    egui::Checkbox::new(
                        &mut self.region.layers.cell_tags,
                        "格子标签（共享标签引用）",
                    ),
                );
                ui.small("标签层开启时，目的选区的格子标签会被剪贴板内容替换，包括空标签格。");
                ui.add_enabled(
                    self.layers[2],
                    egui::Checkbox::new(
                        &mut self.region.layers.waypoints,
                        "航点（复制时生成新编号）",
                    ),
                );
            });
        ui.small("隐藏图层不参与选区操作。对象按锚点归入选区。");
        ui.checkbox(&mut self.allow_overlap, "允许对象重叠");
        if let Some(clip) = &self.region.clipboard {
            ui.small(format!(
                "剪贴板：{} 格 / {} 个对象",
                clip.cells.len(),
                clip.records.len()
            ));
        }
        if let Some(error) = self.region.preview.as_ref().and_then(|p| p.error.as_ref()) {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
        if matches!(self.tool, Tool::Paste | Tool::RegionMove) {
            ui.small("鼠标对齐选区的最小坐标角，点击放置；Esc 退出。");
        }
    }
    fn make_preview(&self, scene: &MapScene, at: Cell) -> Result<PreviewGeometry> {
        let doc = self.doc.as_ref().context("没有地图")?;
        if matches!(self.tool, Tool::Paste | Tool::RegionMove) {
            let mut clip = if self.tool == Tool::RegionMove {
                Clipboard::capture(
                    doc,
                    &self.region.cells,
                    self.region
                        .layers
                        .visible(self.layers)
                        .with_cell_tags_visible(self.task_labels),
                )?
            } else {
                self.region
                    .clipboard
                    .as_ref()
                    .context("请先复制区域")?
                    .clone()
            };
            clip.layers.cell_tags &= self.task_labels;
            let plan = clip.plan(doc, at)?;
            let records: Vec<_> = plan.records.iter().map(|(_, r)| r.clone()).collect();
            let ground = plan
                .cells
                .iter()
                .filter_map(|c| c.terrain.map(|v| (c.at, v)))
                .collect();
            let ignored = if self.tool == Tool::RegionMove {
                clip.records.iter().map(|(id, _)| *id).collect()
            } else {
                BTreeSet::new()
            };
            placement::validate(doc, scene, &records, &ignored, self.allow_overlap, &ground)?;
            let mut cells: Vec<_> = plan
                .cells
                .iter()
                .map(|c| {
                    (
                        c.at,
                        c.terrain
                            .unwrap_or_else(|| terrain_tools::value(doc, scene, c.at))
                            .level,
                    )
                })
                .collect();
            for r in &records {
                for at in placement::footprint(scene, r)? {
                    cells.push((
                        at,
                        ground
                            .get(&at)
                            .copied()
                            .unwrap_or_else(|| terrain_tools::value(doc, scene, at))
                            .level,
                    ));
                }
            }
            return Ok((cells, records, ground.into_iter().collect()));
        }
        let (record, ignored) = if self.tool == Tool::Place {
            let value = if self.section == "Terrain" {
                self.type_id.clone()
            } else if self.section == "Smudge" {
                format!("{},{},{},0", self.type_id, at[0], at[1])
            } else if self.section == "Infantry" {
                format!(
                    "{},{},256,{},{},{},Guard,0,None,0,-1,0,1,1",
                    self.owner, self.type_id, at[0], at[1], self.infantry_subcell
                )
            } else {
                format!(
                    "{},{},256,{},{},0,Guard,None,0,-1,0,-1,1,1",
                    self.owner, self.type_id, at[0], at[1]
                )
            };
            (
                Record {
                    section: self.section.clone(),
                    key: (u32::from(at[1]) * 1000 + u32::from(at[0])).to_string(),
                    value: value.into_bytes(),
                    coordinate_radix: 1000,
                },
                BTreeSet::new(),
            )
        } else {
            let id = self.selected.context("请先选择对象")?;
            let record = self.records.get(&id).context("对象已不存在")?.moved(at)?;
            (
                record,
                if self.tool == Tool::Move {
                    BTreeSet::from([id])
                } else {
                    BTreeSet::new()
                },
            )
        };
        placement::validate(
            doc,
            scene,
            std::slice::from_ref(&record),
            &ignored,
            self.allow_overlap,
            &BTreeMap::new(),
        )?;
        let cells = placement::footprint(scene, &record)?
            .into_iter()
            .map(|at| (at, terrain_tools::value(doc, scene, at).level))
            .collect();
        Ok((cells, vec![record], Vec::new()))
    }
    pub fn region_preview(
        &mut self,
        painter: &egui::Painter,
        scene: &MapScene,
        rect: egui::Rect,
        camera: &crate::camera::MapCamera,
        at: Option<Cell>,
    ) {
        self.object_brush_preview(painter, rect, camera);
        if self.object_brush.active() {
            return;
        }
        let Some(doc) = self.doc.as_ref() else { return };
        let world = camera.world_rect(rect);
        let outline = |cell: Cell, level: u8, color: Color32| {
            let (x, y) = vera20k_core::map::terrain::iso_to_screen(cell[0], cell[1], level);
            let center = rect.min + (egui::pos2(x + 30., y + 15.) - world.min) * camera.zoom;
            if !rect.expand(50.).contains(center) {
                return;
            }
            let points = [[0., -15.], [30., 0.], [0., 15.], [-30., 0.]]
                .map(|[x, y]| center + egui::vec2(x, y) * camera.zoom);
            painter.add(egui::Shape::convex_polygon(
                points.to_vec(),
                color.gamma_multiply(0.12),
                egui::Stroke::new(1., color),
            ));
        };
        if matches!(self.tool, Tool::Region | Tool::RegionMove) {
            for &cell in &self.region.cells {
                outline(
                    cell,
                    terrain_tools::value(doc, scene, cell).level,
                    Color32::from_rgb(90, 190, 255),
                );
            }
        }
        if !matches!(
            self.tool,
            Tool::Paste | Tool::RegionMove | Tool::Place | Tool::Move | Tool::Clone
        ) {
            return;
        }
        let Some(at) = at else { return };
        let key = format!(
            "{}:{}:{at:?}:{:?}:{}:{}:{}:{}:{}:{}:{:?}:{:?}",
            doc.id,
            doc.state_id,
            self.tool,
            self.allow_overlap,
            self.infantry_subcell,
            self.region.revision,
            self.section,
            self.type_id,
            self.owner,
            self.selected,
            (self.layers, self.region.layers, self.task_labels)
        );
        if self.region.preview.as_ref().is_none_or(|p| p.key != key) {
            self.region.preview = Some(match self.make_preview(scene, at) {
                Ok((cells, records, ground)) => Preview {
                    key,
                    cells,
                    records,
                    ground,
                    error: None,
                },
                Err(e) => {
                    // Keep a red spatial outline even if one destination is invalid.
                    let cells = if matches!(self.tool, Tool::Paste | Tool::RegionMove) {
                        let clip = if self.tool == Tool::RegionMove {
                            Clipboard::capture(
                                doc,
                                &self.region.cells,
                                self.region
                                    .layers
                                    .visible(self.layers)
                                    .with_cell_tags_visible(self.task_labels),
                            )
                            .ok()
                        } else {
                            self.region.clipboard.clone()
                        };
                        clip.map(|clip| {
                            clip.cells
                                .iter()
                                .filter_map(|c| {
                                    let [x, y] = [0, 1].map(|axis| {
                                        i32::from(c.at[axis]) + i32::from(at[axis])
                                            - i32::from(clip.origin[axis])
                                    });
                                    ((0..i32::from(MAX_CELL_SIDE)).contains(&x)
                                        && (0..i32::from(MAX_CELL_SIDE)).contains(&y))
                                    .then_some((
                                        [x as u16, y as u16],
                                        c.terrain.map_or(0, |v| v.level),
                                    ))
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                    } else {
                        let (section, id) = if self.tool == Tool::Place {
                            (self.section.clone(), self.type_id.clone())
                        } else {
                            self.selected
                                .and_then(|id| self.records.get(&id))
                                .map(|r| (r.section.clone(), r.type_id()))
                                .unwrap_or_default()
                        };
                        placement::offsets(scene, &section, &id)
                            .into_iter()
                            .filter_map(|(dx, dy)| {
                                let x = i32::from(at[0]) + i32::from(dx);
                                let y = i32::from(at[1]) + i32::from(dy);
                                ((0..i32::from(MAX_CELL_SIDE)).contains(&x)
                                    && (0..i32::from(MAX_CELL_SIDE)).contains(&y))
                                .then_some((
                                    [x as u16, y as u16],
                                    terrain_tools::value(doc, scene, at).level,
                                ))
                            })
                            .collect()
                    };
                    Preview {
                        key,
                        cells,
                        records: Vec::new(),
                        ground: Vec::new(),
                        error: Some(e.to_string()),
                    }
                }
            });
        }
        let preview = self.region.preview.as_ref().unwrap();
        let color = if preview.error.is_some() {
            Color32::LIGHT_RED
        } else {
            Color32::from_rgb(90, 225, 170)
        };
        if preview.error.is_none() && !preview.ground.is_empty() {
            let raw: HashMap<_, _> = scene
                .catalog
                .raw_tiles
                .iter()
                .map(|(&tile, &raw)| (raw, tile))
                .collect();
            for &(at, v) in &preview.ground {
                let Some(&tile_id) = raw.get(&v.tile) else {
                    continue;
                };
                let key = vera20k_core::map::theater::TileKey {
                    tile_id,
                    sub_tile: v.sub,
                    variant: 0,
                };
                let Some(image) = scene.palette.images.get(&key) else {
                    continue;
                };
                let (x, y) = vera20k_core::map::terrain::iso_to_screen(at[0], at[1], v.level);
                let origin = rect.min
                    + (egui::pos2(
                        x + image.info.offset[0] as f32,
                        y + image.info.offset[1] as f32,
                    ) - world.min)
                        * camera.zoom;
                let target = egui::Rect::from_min_size(
                    origin,
                    egui::vec2(image.info.width as f32, image.info.height as f32) * camera.zoom,
                );
                if !target.intersects(rect) {
                    continue;
                }
                let texture = self
                    .ui_state
                    .textures
                    .entry((tile_id, v.sub, 0))
                    .or_insert_with(|| {
                        painter.ctx().load_texture(
                            format!("paste-terrain-{tile_id}-{}", v.sub),
                            egui::ColorImage::from_rgba_unmultiplied(
                                [image.info.width as usize, image.info.height as usize],
                                &image.rgba,
                            ),
                            egui::TextureOptions::NEAREST,
                        )
                    });
                painter.image(
                    texture.id(),
                    target,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                    Color32::from_white_alpha(170),
                );
            }
        }
        for &(cell, level) in &preview.cells {
            outline(cell, level, color);
        }
        for r in &preview.records {
            let Some(at) = r.cell() else { continue };
            let Some(appearance) = scene.sprites.objects.get(&(r.section.clone(), r.type_id()))
            else {
                continue;
            };
            let facing = r
                .fields()
                .get(if r.section == "Infantry" { 7 } else { 5 })
                .and_then(|f| f.parse().ok())
                .unwrap_or(0);
            let image = appearance.image(facing);
            let ground = preview
                .ground
                .iter()
                .find(|(c, _)| *c == at)
                .map_or_else(|| terrain_tools::value(doc, scene, at), |(_, value)| *value);
            let world_z = terrain_tools::object_world_z(
                scene,
                at,
                ground,
                ra2_map_scene::visual::object_on_bridge(r),
            );
            let point = ra2_map_scene::visual::object_anchor(r, appearance, world_z);
            let position = ra2_map_scene::visual::sprite_position(point, image);
            let target = egui::Rect::from_min_size(
                rect.min + (position - world.min) * camera.zoom,
                egui::vec2(image.info.width as f32, image.info.height as f32) * camera.zoom,
            );
            if target.intersects(rect) {
                let texture = editing_ui::map_sprite_texture(
                    &mut self.ui_state.map_sprites,
                    painter.ctx(),
                    image,
                );
                painter.image(
                    texture,
                    target,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                    Color32::from_white_alpha(160),
                );
            }
        }
    }
}

const CLIPBOARD_PREFIX: &str = "RA2MAP_REGION_V1\n";
impl Editor {
    pub(super) fn export_region_clipboard(&mut self, ctx: &egui::Context) {
        if let Some(clip) = &self.region.clipboard {
            match serde_json::to_string(clip) {
                Ok(text) => ctx.copy_text(format!("{CLIPBOARD_PREFIX}{text}")),
                Err(e) => self.status = e.to_string(),
            }
        }
    }
    pub(super) fn import_region_clipboard(&mut self, text: &str) -> Result<()> {
        let json = text
            .strip_prefix(CLIPBOARD_PREFIX)
            .context("剪贴板不是地图区域，请先复制区域")?;
        let clip: Clipboard = serde_json::from_str(json).context("区域剪贴板格式无效")?;
        ensure!(
            !clip.cells.is_empty() && clip.cells.len() <= usize::from(MAX_CELL_SIDE).pow(2),
            "剪贴板区域大小无效"
        );
        ensure!(
            clip.cells.iter().all(|c| c.at[0] < MAX_CELL_SIDE
                && c.at[1] < MAX_CELL_SIDE
                && c.terrain.is_none_or(|v| v.level <= 14)),
            "剪贴板坐标或高度无效"
        );
        ensure!(
            clip.cells.iter().all(|c| c.cell_tag.as_ref().is_none_or(
                |v| v.len() <= 480 && !v.iter().any(|b| matches!(b, b'\r' | b'\n' | 0 | b';'))
            )),
            "剪贴板格子标签无效"
        );
        ensure!(
            clip.records.iter().all(
                |(_, r)| region::OBJECT_SECTIONS.contains(&r.section.as_str())
                    || r.section == "Waypoints"
            ),
            "剪贴板对象分类无效"
        );
        ensure!(
            self.doc
                .as_ref()
                .is_some_and(|doc| doc.resource_profile == clip.resource_profile),
            "剪贴板来自不同 Mod 或旧版未知来源，请在当前资源中重新复制"
        );
        self.region.clipboard = Some(clip);
        self.region.preview = None;
        self.region.revision += 1;
        Ok(())
    }
}

#[path = "region_smoke.rs"]
mod smoke;
#[cfg(test)]
#[path = "region_edit_tests.rs"]
mod tests;
