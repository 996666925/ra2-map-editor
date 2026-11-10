use super::*;
use crate::i18n::tr;
use crate::new_map::{MAX_MAP_DIMENSION, MAX_MAP_SPAN, MAX_PLAYERS, MIN_MAP_DIMENSION};
use crate::ui_theme::{self as theme, Icon};
use egui::RichText;
#[derive(Default)]
pub(super) struct Workbench {
    pub(super) flat_view: crate::flat_view::FlatView,
    pub(super) size_guide: crate::map_boundary::SizeGuide,
    pub(super) task_overlay: crate::mission_overlay::MissionOverlay,
    pub(super) map_settings: crate::map_settings_ui::MapSettingsUi,
    pub(super) settings: bool,
    pub(super) playtest: bool,
    pub(super) mission: crate::mission_ui::MissionUi,
    pub(super) mission_script: crate::mission_script_ui::ScriptUi,
    pub(super) new_map: bool,
    pub(super) random_map: bool,
    pub(super) random_options: ra2_map_document::random_map::RandomMapOptions,
    pub(super) help: bool,
    pub(super) canvas_pan: bool,
    pub(super) cancel_secondary: bool,
    new_options: crate::new_map::NewMapOptions,
    overview: Option<egui::TextureHandle>,
    pub(super) overview_bounds: Option<egui::Rect>,
    overview_revision: Option<u64>,
    pub(super) canvas_extent: Option<egui::Vec2>,
    pub(super) canvas_rect: Option<egui::Rect>,
    pub(super) overview_rect: Option<egui::Rect>,
}

fn navigator_point(bounds: egui::Rect, rect: egui::Rect, pointer: egui::Pos2) -> egui::Pos2 {
    let t = (pointer - rect.min) / rect.size();
    bounds.min + bounds.size() * t.clamp(egui::Vec2::ZERO, egui::Vec2::splat(1.))
}

impl MapView {
    /// Run after all widgets so this frame's last text or number update is
    /// included in the snapshot. Dialogs with independent files keep ownership.
    pub(crate) fn finish_save(&mut self, ctx: &egui::Context) {
        let requested = self.editor.save_requested.take();
        if self.pending.is_some()
            || self.loading
            || self.editor.structural_refresh
            || self.workbench.new_map
            || self.workbench.random_map
            || self.workbench.settings
            || self.workbench.help
            || self.workbench.playtest
            || self.workbench.mission.campaign.open
        {
            return;
        }
        let Some(as_new) = requested.or_else(|| crate::form_edit::take_save_shortcut(ctx)) else {
            return;
        };
        let Some(scene) = &self.scene else { return };
        self.workbench
            .map_settings
            .finish_for_save(ctx, &mut self.editor, scene);
        self.editor.finish_property_for_save(ctx, scene);
        self.workbench
            .mission
            .finish_for_save(ctx, &mut self.editor, scene);
        if self.workbench.map_settings.allow_leave()
            && self.workbench.mission.allow_leave()
            && self.editor.check_property_draft()
        {
            self.editor.save(as_new);
        }
    }
    pub(super) fn editing_blocked(&self) -> bool {
        self.pending.is_some()
            || self.workbench.new_map
            || self.workbench.random_map
            || self.workbench.settings
            || self.workbench.playtest
            || self.workbench.help
            || self.workbench.mission.open
            || self.workbench.mission_script.open
            || self.workbench.mission.campaign.open
            || self.workbench.map_settings.open
            || self.editor.structural_refresh
    }
    pub(super) fn overview_texture_id(&self) -> Option<egui::TextureId> {
        self.workbench.overview.as_ref().map(|texture| texture.id())
    }
    pub(super) fn reset_overview(&mut self) {
        self.workbench.overview = None;
        self.workbench.overview_bounds = None;
        self.workbench.overview_revision = None;
    }
    pub fn show_project_settings(&mut self) {
        self.workbench.settings = true;
    }
    pub fn show_new_map(&mut self) {
        if self.pending.is_none() {
            self.workbench.random_map = false;
            self.workbench.new_map = true;
        }
    }
    pub fn toolbar(&mut self, ui: &mut egui::Ui, directory: &str) {
        if self.pending.is_none()
            && !self.workbench.new_map
            && !self.workbench.random_map
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F1))
        {
            self.workbench.help = !self.workbench.help;
        }
        if !ui.ctx().text_edit_focused()
            && !self.editing_blocked()
            && !crate::form_edit::popup_open(ui.ctx())
        {
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::N)) {
                self.show_new_map();
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O)) {
                self.pick_map(directory);
            }
            if self.editor.doc.is_some()
                && !self.editor.stroking
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F5))
            {
                self.workbench.playtest = true;
            }
        }
        ui.menu_button(tr!("文件"), |ui| {
            if ui.button(tr!("新建地图…   Ctrl+N")).clicked() {
                self.show_new_map();
                ui.close();
            }
            if ui.button(tr!("新建随机地图…")).clicked() {
                self.show_random_map();
                ui.close();
            }
            if ui.button(tr!("打开地图…   Ctrl+O")).clicked() {
                self.pick_map(directory);
                ui.close();
            }
            if ui
                .add_enabled(
                    self.editor.doc.is_some()
                        && !self.editor.saving
                        && !self.editor.stroking
                        && !self.workbench.mission.has_draft()
                        && !self.workbench.map_settings.open,
                    egui::Button::new(tr!("另存为…   Ctrl+Shift+S")),
                )
                .clicked()
            {
                self.editor.save_requested = Some(true);
                ui.close();
            }
            if ui
                .add_enabled(
                    self.editor.target.is_some()
                        || self.scene.is_some()
                            && self
                                .editor
                                .doc
                                .as_ref()
                                .is_none_or(|doc| doc.saved_state.is_some()),
                    egui::Button::new(tr!("重新加载地图")),
                )
                .clicked()
            {
                let name = self
                    .editor
                    .target
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| self.name.clone());
                self.open(PathBuf::from(directory.trim()), name);
                ui.close();
            }
            ui.separator();
            if ui
                .add_enabled(
                    self.editor.doc.is_some()
                        && !self.loading
                        && !self.editor.exporting
                        && !self.editor.stroking
                        && !self.workbench.mission.has_draft()
                        && !self.workbench.map_settings.open,
                    egui::Button::new(tr!("导出高清地图预览图 PNG…")),
                )
                .on_hover_text(
                    tr!("使用试玩设置中的 VERA20k 引擎生成高清地图预览，最长边 4096 像素。包含未保存的修改，使用游戏实际渲染器。"),
                )
                .clicked()
            {
                self.editor.export_hd_preview_png();
                ui.close();
            }
            if ui
                .add_enabled(
                    self.scene.is_some()
                        && !self.editor.exporting
                        && !self.loading,
                    egui::Button::new(tr!("导出内嵌预览原图 PNG…")),
                )
                .on_hover_text(
                    tr!("导出当前加载地图内嵌的 PreviewPack 小图，保留原始尺寸和像素。未保存的修改不会更新此预览。"),
                )
                .clicked()
            {
                if let Some(scene) = &self.scene {
                    self.editor.export_preview_png(scene.source.bytes());
                }
                ui.close();
            }
            if ui
                .add_enabled(
                    self.scene.is_some(),
                    egui::Button::new(tr!("导出加载时的原文副本…")),
                )
                .on_hover_text(tr!("不包含当前未保存的编辑"))
                .clicked()
            {
                self.export_original();
                ui.close();
            }
        });
        ui.menu_button(tr!("设置"), |ui| {
            crate::i18n::language_menu(ui);
            ui.separator();
            if ui
                .add_enabled(
                    self.editor.doc.is_some()
                        && !self.loading
                        && !self.editor.stroking
                        && !self.workbench.mission.has_draft()
                        && !self.workbench.map_settings.open,
                    egui::Button::new(tr!("地图信息…")),
                )
                .clicked()
            {
                if self.editor.check_property_draft() {
                    self.workbench.mission.open = false;
                    self.workbench.mission_script.open = false;
                    self.workbench.mission.campaign.open = false;
                    self.workbench.map_settings.tab = 3;
                    self.workbench.map_settings.open = true;
                }
                ui.close();
            }
            if ui
                .add_enabled(
                    self.editor.doc.is_some()
                        && !self.loading
                        && !self.editor.stroking
                        && !self.workbench.mission.has_draft()
                        && !self.workbench.map_settings.open,
                    egui::Button::new(tr!("地图尺寸与光照…")),
                )
                .clicked()
            {
                if self.editor.check_property_draft() {
                    self.workbench.mission.open = false;
                    self.workbench.mission_script.open = false;
                    self.workbench.mission.campaign.open = false;
                    self.workbench.map_settings.tab = 0;
                    self.workbench.map_settings.open = true;
                }
                ui.close();
            }
            if ui
                .add_enabled(
                    self.editor.doc.is_some()
                        && !self.loading
                        && !self.editor.stroking
                        && !self.workbench.mission.has_draft()
                        && !self.workbench.map_settings.open,
                    egui::Button::new(tr!("箱子设置…")),
                )
                .clicked()
            {
                if self.editor.check_property_draft() {
                    self.workbench.mission.open = false;
                    self.workbench.mission_script.open = false;
                    self.workbench.mission.campaign.open = false;
                    self.workbench.map_settings.tab = 2;
                    self.workbench.map_settings.open = true;
                }
                ui.close();
            }
            if ui.button(tr!("游戏资源与地图来源…")).clicked() {
                self.workbench.settings = true;
                ui.close();
            }
            if ui.button(tr!("试玩配置…")).clicked() {
                self.workbench.playtest = true;
                ui.close();
            }
        });
        if ui
            .add_enabled(
                self.editor.doc.is_some()
                    && !self.editor.stroking
                    && !self.workbench.map_settings.open,
                egui::Button::new(tr!("任务制作")),
            )
            .clicked()
        {
            self.workbench.mission.open = true;
            self.workbench.mission_script.open = false;
        }
        if ui
            .add_enabled(
                self.editor.doc.is_some()
                    && !self.loading
                    && !self.editor.stroking
                    && !self.workbench.mission.has_draft()
                    && !self.workbench.map_settings.open,
                egui::Button::new(tr!("TypeScript 任务")),
            )
            .clicked()
            && self.editor.check_property_draft()
        {
            self.workbench.mission.open = false;
            self.workbench.mission.campaign.open = false;
            self.workbench.mission_script.open = true;
        }
        if !self.editor.recoveries.is_empty()
            && ui
                .small_button(tr!("恢复 {arg0}", arg0 = self.editor.recoveries.len()))
                .on_hover_text(tr!("发现可恢复的地图快照"))
                .clicked()
        {
            self.workbench.settings = true;
        }
        ui.separator();
        if theme::button(ui, Icon::Folder, tr!("打开"), 73., false)
            .on_hover_text(tr!("打开地图文件  Ctrl+O"))
            .clicked()
        {
            self.pick_map(directory);
        }
        if let Some(scene) = &self.scene {
            ui.add_enabled_ui(
                !self.workbench.mission.has_draft() && !self.workbench.map_settings.open,
                |ui| self.editor.document_actions(ui, scene),
            )
            .response
            .on_disabled_hover_text(tr!("请先完成或取消当前任务输入。"));
        } else {
            ui.add_enabled_ui(false, |ui| {
                theme::button(ui, Icon::Save, tr!("保存"), 73., false);
            });
        }
        if ui
            .add_sized([56., 32.], egui::Button::new(tr!("帮助")))
            .on_hover_text(tr!("操作说明与快捷键  F1"))
            .clicked()
        {
            self.workbench.help = true;
        }
        ui.separator();
        let target_name = self.editor.target.as_ref().map(|p| p.display().to_string());
        let name = target_name
            .as_deref()
            .or_else(|| self.scene.as_ref().map(|scene| scene.source.name.as_str()))
            .unwrap_or(tr!("未打开地图"));
        let filename = Path::new(name)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let (dot, _) = ui.allocate_exact_size(egui::vec2(6., 12.), egui::Sense::hover());
        ui.painter().circle_filled(
            dot.center(),
            2.5,
            if self.editor.dirty() {
                theme::ACCENT
            } else {
                theme::GREEN
            },
        );
        ui.allocate_ui_with_layout(
            egui::vec2((ui.available_width() - 92.).max(24.), 28.),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.add(egui::Label::new(RichText::new(filename).size(12.)).truncate())
                    .on_hover_text(name);
            },
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .scope_id(egui::Id::unique("workbench-playtest-action"))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
            |ui| {
                if ui
                    .add_enabled_ui(self.editor.doc.is_some() && !self.editor.stroking, |ui| {
                        theme::button(ui, Icon::Play, tr!("试玩"), 76., false)
                    })
                    .inner
                    .on_hover_text(tr!("设置电脑、游戏速度，并启动当前地图快照  F5"))
                    .on_disabled_hover_text(tr!("先打开地图，并完成当前一笔。"))
                    .clicked()
                {
                    self.workbench.playtest = true;
                }
            },
        );
    }
    pub fn quick_tools(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(!self.editing_blocked(), |ui| self.editor.quick_tools(ui));
    }
    pub fn toolbox(&mut self, ui: &mut egui::Ui) {
        if self.editing_blocked() {
            ui.disable();
        }
        egui::ScrollArea::vertical()
            .id_salt((
                "tool-workspace-scroll",
                self.editor.tool.name(),
                self.editor.terrain_mode.name(),
            ))
            .show(ui, |ui| self.editor.toolbox(ui, self.scene.as_ref()));
    }
    pub fn inspector(&mut self, ui: &mut egui::Ui) {
        if self.workbench.map_settings.open || self.editor.structural_refresh {
            ui.disable();
        }
        self.overview(ui);
        let Some(scene) = &self.scene else {
            theme::section(ui, tr!("属性检查"), "");
            ui.add_space(16.);
            ui.label(RichText::new(tr!("尚未选择内容")).color(theme::MUTED));
            ui.small(tr!("打开地图后，可在这里导航、查看格子和编辑对象。"));
            return;
        };
        theme::section(ui, tr!("当前选择"), "");
        if self.editor.selected.is_some() {
            self.editor.selection_properties(ui, scene);
        } else if let Some(cell) = self.selected.and_then(|i| scene.cells.get(i)) {
            let value = self
                .editor
                .doc
                .as_ref()
                .map(|doc| ra2_map_scene::terrain_tools::value(doc, scene, [cell.rx, cell.ry]));
            theme::field(ui, tr!("格子坐标"), format!("{}, {}", cell.rx, cell.ry));
            theme::field(
                ui,
                tr!("地形子块"),
                value.map_or_else(
                    || format!("{} : {}", cell.tile_id, cell.sub_tile),
                    |v| {
                        format!(
                            "{} : {}",
                            scene
                                .palette
                                .raw_ids
                                .get(&v.tile)
                                .copied()
                                .map_or(v.tile, i32::from),
                            v.sub
                        )
                    },
                ),
            );
            theme::field(ui, tr!("高度"), value.map_or(cell.z, |v| v.level) as i8);
            theme::badge(
                ui,
                if scene.is_playable(cell) {
                    tr!("可玩区域内")
                } else {
                    tr!("边界填充区域")
                },
                theme::BLUE,
            );
        } else {
            ui.label(
                RichText::new(tr!("在画布中选择格子或对象"))
                    .color(theme::MUTED)
                    .size(12.),
            );
        }
        ui.add_space(8.);
        ui.separator();
        ui.add_space(6.);
        theme::section(ui, tr!("显示图层"), "");
        ui.checkbox(&mut self.editor.map_boundaries, tr!("地图边界线"))
            .on_hover_text(tr!(
                "红线：完整地图范围（Size）；蓝线：可玩区域（LocalSize）。"
            ));
        if self.editor.framework {
            ui.small(tr!("框架与 2D 地形互斥；Ctrl+F 切换框架，F2 切换 2D。"));
        }
        if self.editor.flat_view {
            ui.checkbox(&mut self.workbench.flat_view.heights, tr!("2D 高度数字"));
            ui.checkbox(
                &mut self.workbench.flat_view.templates,
                tr!("2D 完整模板边界 / 编号"),
            );
            ui.small(tr!(
                "金线：高度变化；蓝线：模板外沿；红叉 / 红点：对象占地 / 覆盖物。"
            ));
        } else {
            for (index, label) in [
                (0, tr!("矿石与覆盖物")),
                (1, tr!("对象预览与标记")),
                (2, tr!("航点与玩家起点")),
            ] {
                ui.checkbox(&mut self.editor.layers[index], label);
            }
            ui.checkbox(&mut self.editor.task_labels, tr!("任务标签与格子触发点"));
            ui.checkbox(
                &mut self.editor.base_nodes_visible,
                tr!("基地节点与建造顺序"),
            );
            ui.checkbox(&mut self.editor.remap, tr!("所属方换色"));
            ui.checkbox(&mut self.editor.lighting, tr!("地图基础光照"));
        }
        ui.checkbox(&mut self.editor.tunnels_visible, tr!("隧道路径"));
        ui.add_space(8.);
        ui.separator();
        egui::CollapsingHeader::new(tr!("地图信息"))
            .default_open(true)
            .show(ui, |ui| {
                let header = &scene.source.header;
                let theater = crate::new_map::THEATERS
                    .iter()
                    .find(|(id, _)| *id == header.theater)
                    .map_or(header.theater.as_str(), |(_, label)| *label);
                theme::field(ui, tr!("环境"), crate::i18n::text(theater));
                theme::field(
                    ui,
                    tr!("地图尺寸"),
                    format!("{} × {}", header.width, header.height),
                );
                theme::field(
                    ui,
                    tr!("可玩尺寸"),
                    format!("{} × {}", header.local_width, header.local_height),
                );
                theme::field(ui, tr!("地形格数"), scene.cells.len());
                ui.add(
                    egui::Label::new(
                        RichText::new(&scene.source.title)
                            .size(11.)
                            .color(theme::MUTED),
                    )
                    .wrap(),
                );
            });
        let issues: Vec<_> = scene
            .diagnostics
            .iter()
            .filter(|d| d.code != "EDITOR_PREVIEW")
            .collect();
        egui::CollapsingHeader::new(tr!("地图诊断  ·  {arg0}", arg0 = issues.len())).show(
            ui,
            |ui| {
                if issues.is_empty() {
                    ui.label(
                        RichText::new(tr!("未发现资源或地图格式错误"))
                            .size(11.)
                            .color(theme::GREEN),
                    );
                }
                for diagnostic in issues {
                    if ui
                        .small_button(format!(
                            "{}{}",
                            diagnostic.code,
                            diagnostic
                                .cell
                                .map(|c| format!("  {}, {}", c[0], c[1]))
                                .unwrap_or_default()
                        ))
                        .clicked()
                        && let Some(at) = diagnostic.cell
                        && let Some((index, c)) = scene
                            .cells
                            .iter()
                            .enumerate()
                            .find(|(_, c)| [c.rx, c.ry] == at)
                    {
                        self.selected = Some(index);
                        self.center = egui::pos2(c.screen_x + 30., c.screen_y + 15.);
                        self.zoom = 1. / ui.ctx().pixels_per_point();
                        self.fit = false;
                    }
                    ui.small(&diagnostic.message);
                }
                ui.add_space(6.);
                ui.small(tr!(
                    "静态场景使用共享像素深度；完整桥梁、动态效果与局部灯光仍需继续完善。"
                ));
            },
        );
    }
    fn overview(&mut self, ui: &mut egui::Ui) {
        let Some(scene) = &self.scene else { return };
        theme::section(ui, tr!("地图导航"), "");
        if self.workbench.overview.is_none()
            || self.workbench.overview_revision != Some(self.editor.visual_revision)
        {
            let radar = match ra2_map_scene::radar::render(scene, self.editor.doc.as_ref()) {
                Ok(radar) => radar,
                Err(error) => {
                    ui.small(error.to_string());
                    return;
                }
            };
            self.workbench.overview_revision = Some(self.editor.visual_revision);
            self.workbench.overview_bounds = Some(radar.bounds);
            self.workbench.overview = Some(ui.ctx().load_texture(
                "map-navigator",
                radar.image,
                egui::TextureOptions::NEAREST,
            ));
        }
        let bounds = self.workbench.overview_bounds.unwrap_or(scene.bounds);
        let width = ui.available_width();
        let (frame, _) = ui.allocate_exact_size(
            egui::vec2(
                width,
                (width * 112. / 192.).min(if ui.ctx().content_rect().height() < 700. {
                    105.
                } else {
                    145.
                }),
            ),
            egui::Sense::hover(),
        );
        // Letterbox rectangular maps instead of stretching their geography.
        let scale = (frame.width() / bounds.width()).min(frame.height() / bounds.height());
        let rect = egui::Rect::from_center_size(frame.center(), bounds.size() * scale);
        self.workbench.overview_rect = Some(rect);
        let response = ui.interact(
            rect,
            ui.scope_id().with("map-navigator"),
            egui::Sense::click_and_drag(),
        );
        ui.painter().rect_filled(frame, 0, theme::BACKGROUND);
        if (response.clicked() || response.dragged())
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let position = navigator_point(bounds, rect, pointer);
            self.center = if self.editor.flat_view {
                position
            } else {
                ra2_map_scene::radar::canvas_position(scene, self.editor.doc.as_ref(), position)
            };
            self.fit = false;
        }
        let p = ui.painter().with_clip_rect(rect);
        p.image(
            self.workbench.overview.as_ref().unwrap().id(),
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
            egui::Color32::WHITE,
        );
        let project = |pos: egui::Pos2| {
            rect.min
                + egui::vec2(
                    (pos.x - bounds.left()) / bounds.width() * rect.width(),
                    (pos.y - bounds.top()) / bounds.height() * rect.height(),
                )
        };
        let screen = ui.ctx().content_rect();
        // The precise canvas extent is retained from its last layout pass.
        let extent = self.workbench.canvas_extent.unwrap_or(screen.size());
        let radar_center = if self.editor.flat_view {
            self.center
        } else {
            ra2_map_scene::radar::radar_position(scene, self.center)
        };
        let world = egui::Rect::from_center_size(radar_center, extent / self.zoom);
        let visible = egui::Rect::from_min_max(project(world.min), project(world.max))
            .intersect(rect.shrink(1.));
        if visible.is_positive() {
            p.rect_filled(visible, 0, theme::ACCENT.gamma_multiply(0.10));
            p.rect_stroke(
                visible,
                0,
                egui::Stroke::new(1., theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        response.on_hover_text(tr!("点击或拖动，快速移动地图视角"));
        ui.add_space(5.);
        ui.separator();
        ui.add_space(4.);
    }
    pub fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (color, text) = if self.loading {
                (theme::ACCENT, crate::i18n::text(self.stage))
            } else if self.editor.saving {
                (theme::ACCENT, tr!("正在保存…"))
            } else if self.error.is_some() {
                (egui::Color32::LIGHT_RED, tr!("加载失败"))
            } else if self.workbench.mission.has_draft() {
                (theme::ACCENT, tr!("任务输入待完成"))
            } else if self.workbench.map_settings.dirty() {
                (theme::ACCENT, tr!("地图设置草稿未应用"))
            } else if self.editor.object_properties_dirty() {
                (theme::ACCENT, tr!("属性输入待完成"))
            } else if self.editor.dirty() {
                (theme::ACCENT, tr!("有未保存修改"))
            } else if self.scene.is_some() {
                (theme::GREEN, tr!("已保存"))
            } else {
                (theme::MUTED, tr!("就绪"))
            };
            let (dot, _) = ui.allocate_exact_size(egui::vec2(8., 14.), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 3., color);
            ui.label(RichText::new(text).size(11.).color(color));
            if let Some(cell) = self.hover.and_then(|i| self.scene.as_ref()?.cells.get(i)) {
                ui.separator();
                ui.monospace(format!(
                    "X {:03}   Y {:03}   Z {}",
                    cell.rx, cell.ry, cell.z as i8
                ));
            }
            ui.separator();
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    self.editor.tool.name(),
                    self.editor.tool.shortcut()
                ))
                .size(11.)
                .color(theme::BLUE),
            )
            .on_hover_text(self.editor.tool.hint());
            if ui
                .small_button("F1")
                .on_hover_text(tr!("操作帮助"))
                .clicked()
            {
                self.workbench.help = true;
            }
            let status = self
                .error
                .as_deref()
                .unwrap_or(if !self.editor.status.is_empty() {
                    &self.editor.status
                } else {
                    &self.status
                });
            if status.is_empty() {
                ui.label(
                    RichText::new(self.editor.tool.hint())
                        .size(10.)
                        .color(theme::MUTED),
                );
            } else {
                ui.add(
                    egui::Label::new(RichText::new(status).size(10.).color(theme::MUTED))
                        .truncate(),
                )
                .on_hover_text(status);
            }
        });
    }
    fn pick_map(&mut self, directory: &str) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(tr!("YR 地图"), &["map", "mpr", "yrm", "mmx", "yro"])
            .pick_file()
        {
            self.open(PathBuf::from(directory.trim()), path.display().to_string());
        }
    }
    fn export_original(&mut self) {
        if let Some(scene) = &self.scene
            && let Some(path) = rfd::FileDialog::new()
                .set_file_name("map-original.map")
                .save_file()
        {
            self.status = match scene.source.export_new(&path) {
                Ok(()) => tr!("已导出原文：{arg0}", arg0 = path.display()),
                Err(e) => tr!("导出失败：{e}", e = format!("{:#}", e)),
            };
        }
    }
    pub fn settings_windows(
        &mut self,
        ctx: &egui::Context,
        directory: &mut String,
        frame: &eframe::Frame,
    ) {
        if self.workbench.playtest {
            self.editor.refresh_play_displays(frame);
        }
        self.settings_dialogs(ctx, directory);
    }
    pub(super) fn settings_dialogs(&mut self, ctx: &egui::Context, directory: &mut String) {
        self.random_map_window(ctx, directory);
        if let Some(scene) = &self.scene {
            self.editor.finish_property_edit(ctx, scene);
            self.workbench
                .map_settings
                .show(ctx, &mut self.editor, scene);
        }
        if self.pending.is_none()
            && !self.workbench.new_map
            && !self.workbench.random_map
            && !crate::form_edit::popup_open(ctx)
            && (self.workbench.help
                || self.workbench.playtest
                || self.workbench.settings
                || self.workbench.mission_script.open
                || self.workbench.mission.open && !self.workbench.mission.picking())
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            if self.workbench.help {
                self.workbench.help = false;
            } else if self.workbench.playtest {
                self.workbench.playtest = false;
            } else if self.workbench.settings {
                self.workbench.settings = false;
            } else if self.workbench.mission_script.open {
                self.workbench.mission_script.open = false;
            } else {
                self.workbench.mission.open = false;
            }
        }
        if self.workbench.help {
            theme::window(
                ctx,
                tr!("操作帮助与快捷键"),
                "interaction-help",
                egui::vec2(570., 540.),
            )
            .open(&mut self.workbench.help)
            .vscroll(true)
            .show(ctx, |ui| {
                ui.heading(tr!("地图导航"));
                for text in [
                    tr!("中键 / 右键拖动，或空格 + 左键拖动：平移地图"),
                    tr!("滚轮：围绕鼠标缩放；Home：适应地图；1：原始像素比例"),
                    tr!("F2：切换 2D 地形视图，铺平地形并显示高度与模板边界；对象与区域工具自动返回立体视图"),
                    tr!("Ctrl+F：框架模式，同步简化画布、地形素材栏和放置预览；与 2D 地形互斥"),
                    tr!("F：定位当前对象或格子；小地图单击 / 拖动：快速定位"),
                ] {
                    ui.label(text);
                }
                ui.separator();
                ui.heading(tr!("工具切换"));
                for &(_, tool, key) in &crate::editing::interaction::TOOL_KEYS {
                    ui.label(format!("{key}  —  {}", tool.name()))
                        .on_hover_text(tool.hint());
                }
                ui.label(tr!("B  —  地形画笔；G  —  框选铺地；C  —  围合悬崖；H  —  矩形高台"));
                ui.label(tr!("[ / ]  —  缩小 / 放大子块、矿石和覆盖物画笔"));
                ui.separator();
                ui.heading(tr!("选择与编辑"));
                for text in [
                    tr!("Shift + 框选：添加；Alt + 框选：减去；按下鼠标时确定本笔方式"),
                    tr!("Shift + 地形 / 对象 / 资源画笔：锁定地图轴向；松开恢复自由绘制"),
                    tr!("Ctrl+C / X / V：复制 / 剪切 / 粘贴区域；Ctrl+D：克隆选中对象"),
                    tr!("Delete：删除选中对象或当前区域所选数据层"),
                    tr!("Esc / 右键：取消当前绘制或放置；空闲时 Esc 返回选择工具"),
                    tr!("Ctrl+Z：撤销；绘制中先取消本笔，保留上一笔；Ctrl+Y / Ctrl+Shift+Z：重做"),
                    tr!("右键单击：常用操作菜单；右键拖动：平移地图"),
                ] {
                    ui.label(text);
                }
                ui.separator();
                ui.heading(tr!("文件与试玩"));
                for text in [
                    tr!("Ctrl+N / O：新建 / 打开地图"),
                    tr!("Ctrl+S / Ctrl+Shift+S：保存 / 另存为；F5：打开试玩设置"),
                    tr!("对象属性、任务参数和光照结束输入后自动生效；撤销可恢复修改"),
                    tr!("编辑文字、下拉菜单和浮动设置窗口时，地图工具快捷键暂停"),
                    tr!("窗口失去焦点时取消未完成的一笔，已完成的修改保留"),
                ] {
                    ui.label(text);
                }
            });
        }
        if self.workbench.new_map {
            let mut create = false;
            let mut cancel = false;
            let modal = theme::modal(ctx, "new-map", |ui| {
                ui.set_width(440.0_f32.min(ctx.content_rect().width() - 64.));
                ui.set_max_height((ctx.content_rect().height() - 64.).max(120.));
                ui.heading(tr!("新建地图"));
                ui.label(
                    RichText::new(tr!("从平坦地面开始，创建遭遇战地图。")).color(theme::MUTED),
                );
                ui.separator();
                let mut valid = false;
                egui::ScrollArea::vertical().id_salt("new-map-form")
                    .max_height((ctx.content_rect().height() - 185.).max(120.))
                    .show(ui, |ui| {
                        ui.label(tr!("游戏资源目录"));
                        if theme::path_entry(ui, directory, tr!("包含 ra2.mix 与 ra2md.mix 的目录"), tr!("浏览…"))
                            && let Some(path) = rfd::FileDialog::new().pick_folder()
                        { *directory = path.display().to_string(); }
                        ui.label(tr!("地图名称"));
                        ui.add(egui::TextEdit::singleline(&mut self.workbench.new_options.title).desired_width(f32::INFINITY)).accessible_name(tr!("地图名称"));
                        let options = &mut self.workbench.new_options;
                        ui.label(tr!("作者署名（可留空）"));
                        ui.add(egui::TextEdit::singleline(&mut options.author).desired_width(f32::INFINITY)).accessible_name(tr!("作者署名"));
                        egui::Grid::new("new-map-fields").num_columns(2).spacing([20., 8.]).show(ui, |ui| {
                            ui.label(tr!("地图环境"));
                            egui::ComboBox::from_id_salt("new-map-theater")
                                .width(210.)
                                .selected_text(crate::new_map::THEATERS.iter().find(|(id,_)| *id == options.theater).map_or(options.theater.clone(), |(id,label)| format!("{} / {id}", crate::i18n::text(label))))
                                .show_ui(ui, |ui| {
                                    for (id, label) in crate::new_map::THEATERS {
                                        ui.selectable_value(&mut options.theater, id.into(), format!("{} / {id}", crate::i18n::text(label)));
                                    }
                                });
                            ui.end_row();
                            ui.label(tr!("地图宽度"));
                            crate::form_edit::number(ui, egui::DragValue::new(&mut options.width).range(MIN_MAP_DIMENSION..=MAX_MAP_DIMENSION).suffix(tr!(" 格")), tr!("地图宽度"));
                            ui.end_row();
                            ui.label(tr!("地图高度"));
                            crate::form_edit::number(ui, egui::DragValue::new(&mut options.height).range(MIN_MAP_DIMENSION..=MAX_MAP_DIMENSION).suffix(tr!(" 格")), tr!("地图高度"));
                            ui.end_row();
                            ui.label(tr!("初始地面高度"));
                            crate::form_edit::number(ui, egui::DragValue::new(&mut options.level).range(0..=127).suffix(tr!(" 层")), tr!("初始地面高度"));
                            ui.end_row();
                            ui.label(tr!("玩家起点"));
                            crate::form_edit::number(ui, egui::DragValue::new(&mut options.players).range(2..=MAX_PLAYERS).suffix(tr!(" 个")), tr!("玩家起点"));
                            ui.end_row();
                        });
                        ui.small(tr!("宽＋高 ≤ {MAX_MAP_SPAN}，例如 {arg0} × {arg1} 或 {MAX_MAP_DIMENSION} × {MIN_MAP_DIMENSION}。", MAX_MAP_SPAN = MAX_MAP_SPAN, arg0 = MAX_MAP_SPAN / 2, arg1 = MAX_MAP_SPAN / 2, MAX_MAP_DIMENSION = MAX_MAP_DIMENSION, MIN_MAP_DIMENSION = MIN_MAP_DIMENSION));
                        theme::field(ui, tr!("可玩区域"), tr!("{arg0} × {arg1} 格", arg0 = options.width.saturating_sub(4), arg1 = options.height.saturating_sub(10)));
                        ui.small(tr!("尺寸包含边界；初始不放置建筑、单位或矿石，起点可继续编辑。"));
                        let result = options.validate();
                        valid = result.is_ok() && !directory.trim().is_empty();
                        if let Err(e) = result { ui.colored_label(theme::ACCENT, e.to_string()); }
                        if directory.trim().is_empty() { ui.colored_label(theme::ACCENT, tr!("请先选择游戏资源目录。")); }
                    });
                ui.separator();
                theme::right_to_left(ui, |ui| {
                    create = ui
                        .add_enabled(valid, theme::primary(tr!("创建地图")))
                        .clicked();
                    cancel = ui.button(tr!("取消")).clicked();
                });
            });
            if create {
                self.workbench.new_map = false;
                self.create(
                    PathBuf::from(directory.trim()),
                    self.workbench.new_options.clone(),
                );
            } else if cancel || modal.should_close() {
                self.workbench.new_map = false;
            }
        }
        if self.workbench.settings {
            let mut open = true;
            theme::window(
                ctx,
                tr!("资源与地图来源"),
                "project-settings",
                egui::vec2(490., 420.),
            )
            .open(&mut open)
            .vscroll(true)
            .show(ctx, |ui| {
                theme::section(ui, tr!("游戏资源目录"), "");
                if let Some(scene)=&self.scene { ui.label(tr!("数据结构版本：{arg0}", arg0 = scene.revision.schema));ui.label(tr!("资源版本：{arg0} · {arg1} 文件", arg0 = scene.revision.resources, arg1 = scene.revision.files)); }
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(!self.loading && self.editor.doc.is_some(),egui::Button::new(tr!("刷新当前资源（保留地图）"))).clicked() { self.refresh_resources(PathBuf::from(directory.trim())); }
                    if ui.button(tr!("载入模组配置…")).clicked() && let Some(path)=rfd::FileDialog::new().add_filter(tr!("模组配置"), &["json"]).pick_file() {
                        match ra2_map_assets::profile::Profile::read(&path) {
                            Ok(p)=>{*directory=p.game_dir.display().to_string();if let Some(exe)=p.playtest_exe{self.editor.executable=exe.display().to_string();}self.refresh_resources(p.game_dir);},
                            Err(e)=>self.status=format!("{e:#}"),
                        }
                    }
                });
                ui.small(tr!("刷新重读归档、类型与任务资源；同环境跨模组剪贴板会按版本检查。保存和试玩记录资源与数据结构版本。"));

                if theme::path_entry(ui, directory, tr!("包含 ra2.mix 与 ra2md.mix 的目录"), tr!("浏览…"))
                    && let Some(path) = rfd::FileDialog::new().pick_folder()
                {
                    *directory = path.display().to_string();
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.name.trim().is_empty() && !directory.trim().is_empty(),
                            egui::Button::new(tr!("应用并重载")),
                        )
                        .clicked()
                    {
                        self.open(PathBuf::from(directory.trim()), self.name.clone());
                    }
                });
                ui.separator();
                theme::section(ui, tr!("地图路径或归档名称"), "");
                ui.add(
                    egui::TextEdit::singleline(&mut self.name)
                        .desired_width(f32::INFINITY)
                        .hint_text(tr!("例如 XMP03T4.MAP，或本地文件路径")),
                );
                if ui
                    .add_enabled(
                        !self.name.trim().is_empty() && !directory.trim().is_empty(),
                        theme::primary(tr!("加载地图")),
                    )
                    .clicked()
                {
                    self.open(PathBuf::from(directory.trim()), self.name.clone());
                }
                if !self.editor.recoveries.is_empty() {
                    ui.separator();
                    theme::section(ui, tr!("恢复未保存地图"), "");
                    let mut restore = None;
                    let mut discard = None;
                    for (index, entry) in self.editor.recoveries.iter().enumerate() {
                        ui.add(egui::Label::new(&entry.meta.name).truncate())
                            .on_hover_text(&entry.meta.name);
                        ui.horizontal(|ui| {
                            if ui.button(tr!("恢复")).clicked() {
                                restore = Some(entry.path.clone());
                            }
                            if ui.button(tr!("丢弃快照")).clicked() {
                                discard = Some((index, entry.path.clone()));
                            }
                        });
                    }
                    if let Some(path) = restore.filter(|_| {
                        self.workbench.map_settings.allow_leave()
                            && self.workbench.mission.allow_leave()
                            && self.editor.check_property_draft()
                    }) {
                        if self.editor.dirty() || self.editor.saving {
                            self.pending = Some(Leave::Restore(path));
                            self.leave_after_save = false;
                        } else {
                            let _ = self.editor.io.send(crate::editor_io::Job::Restore(path));
                        }
                    }
                    if let Some((index, path)) = discard {
                        let _ = self.editor.io.send(crate::editor_io::Job::Discard(path));
                        self.editor.recoveries.remove(index);
                    }
                }
            });
            self.workbench.settings = open;
        }
        if self.workbench.playtest {
            theme::window(
                ctx,
                tr!("VERA20k 试玩设置"),
                "playtest-settings",
                egui::vec2(520., 600.),
            )
            .open(&mut self.workbench.playtest)
            .resizable(true)
            .show(ctx, |ui| {
                let draft =
                    self.workbench.mission.has_draft() || self.workbench.map_settings.dirty();
                if draft {
                    ui.colored_label(
                        theme::ACCENT,
                        tr!("请先完成或取消任务与光照输入；尺寸修改需应用或放弃。"),
                    );
                }
                ui.add_enabled_ui(!draft, |ui| self.editor.playtest_controls(ui));
            });
        }
        if let Some(scene) = &self.scene {
            self.workbench
                .mission_script
                .show(ctx, &mut self.editor, scene);
            let cell = self
                .selected
                .and_then(|i| scene.cells.get(i))
                .map(|c| [c.rx, c.ry]);
            if let Some(at) =
                self.workbench
                    .mission
                    .show(ctx, &mut self.editor, scene, cell, &self.directory)
            {
                let level = self
                    .editor
                    .doc
                    .as_ref()
                    .and_then(|d| d.terrain(at))
                    .map_or(0, |v| v.level);
                let (x, y) = vera20k_core::map::terrain::iso_to_screen(at[0], at[1], level);
                self.center = egui::pos2(x + 30., y + 15.);
                self.fit = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::navigator_point;
    use eframe::egui;

    #[test]
    fn navigator_positions_use_the_cropped_world_bounds_and_clamp_dragging() {
        let bounds = egui::Rect::from_min_max(egui::pos2(-530., 900.), egui::pos2(590., 1460.));
        let rect = egui::Rect::from_min_size(egui::pos2(40., 25.), egui::vec2(200., 100.));
        let t = egui::vec2(0.27, 0.38);
        assert!(
            navigator_point(bounds, rect, rect.min + rect.size() * t)
                .distance(bounds.min + bounds.size() * t)
                < 0.001
        );
        assert_eq!(
            navigator_point(bounds, rect, rect.max + egui::vec2(12., 8.)),
            bounds.max
        );
        assert_eq!(
            navigator_point(bounds, rect, rect.min - egui::vec2(12., 8.)),
            bounds.min
        );
    }
}
