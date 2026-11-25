//! Map information, geometry planning, automatic lighting/crate edits and read-only previews.
use crate::i18n::tr;
use crate::{editing::Editor, map::MapScene};
use eframe::egui;
use ra2_map_document::{
    document::{MapDocument, Snapshot},
    lighting::{COMPONENTS, Draft, Profile},
    map_settings::{self, Footprints, Options, Plan},
    new_map::{MAX_MAP_DIMENSION, MAX_MAP_SPAN, MIN_MAP_DIMENSION},
    production_settings::{self, Restriction, Restrictions},
};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct MapSettingsUi {
    pub open: bool,
    pub tab: u8,
    identity: Option<(u64, u64)>,
    options: Option<Options>,
    original: Option<Options>,
    light: Option<Draft>,
    original_light: Option<Draft>,
    crates: Option<ra2_map_document::crate_settings::Draft>,
    original_crates: Option<ra2_map_document::crate_settings::Draft>,
    info: Option<ra2_map_document::map_information::Draft>,
    original_info: Option<ra2_map_document::map_information::Draft>,
    production: Restrictions,
    original_production: Restrictions,
    name: Option<String>,
    original_name: Option<String>,
    author: String,
    original_author: String,
    players: u8,
    original_players: u8,
    pub profile: Profile,
    preview: bool,
    due: Option<Instant>,
    blocked: Vec<map_settings::Affected>,
    worker: Option<std::sync::mpsc::Receiver<anyhow::Result<Plan>>>,
    message: String,
    completion: crate::form_edit::EditCompletion,
    finish_close: Option<egui::Id>,
    light_editing: bool,
    crates_editing: bool,
    info_editing: bool,
}
impl MapSettingsUi {
    pub(crate) fn smoke_preview(&mut self, doc: &MapDocument, profile: Profile) {
        self.load(doc);
        self.open = true;
        self.tab = 1;
        self.profile = profile;
        self.preview = true;
        let light = self.light.as_mut().unwrap();
        light.values.insert("IonAmbient".into(), "0.45".into());
        light
            .values
            .insert("DominatorAmbient".into(), "0.25".into());
        light.values.insert("IonBlue".into(), "0.7".into());
        light.values.insert("DominatorRed".into(), "0.6".into());
    }
    pub(crate) fn apply_lighting(&self, doc: &mut MapDocument) -> anyhow::Result<()> {
        self.light.as_ref().unwrap().apply(doc)
    }
    pub fn dirty(&self) -> bool {
        self.options != self.original
            || self.light != self.original_light
            || self.crates != self.original_crates
            || self.info != self.original_info
            || self.production != self.original_production
            || self.name != self.original_name
            || self.author != self.original_author
            || self.players != self.original_players
    }
    pub fn allow_leave(&mut self) -> bool {
        if self.dirty() {
            self.open = true;
            self.message = tr!("请完成地图设置中的修改，或关闭窗口放弃未生效的修改。").into();
            false
        } else {
            true
        }
    }
    fn load(&mut self, doc: &MapDocument) {
        self.identity = Some((doc.id, doc.state_id));
        self.options = Some(Options::from_document(doc));
        self.original = self.options.clone();
        self.light = Some(Draft::read(doc));
        self.original_light = self.light.clone();
        self.crates = Some(ra2_map_document::crate_settings::Draft::read(doc));
        self.original_crates = self.crates.clone();
        self.info = Some(ra2_map_document::map_information::Draft::read(doc));
        self.original_info = self.info.clone();
        self.production = production_settings::read(doc);
        self.original_production = self.production.clone();
        self.name = Some(doc.ini_text("Basic", "Name"));
        self.original_name = self.name.clone();
        self.author = doc.ini_text("Basic", "Author");
        self.original_author = self.author.clone();
        self.players = ra2_map_document::player_starts::from_document(doc)
            .len()
            .clamp(2, 30) as u8;
        self.original_players = self.players;
        self.blocked.clear();
        self.worker = None;
        self.completion = Default::default();
        self.finish_close = None;
        self.light_editing = false;
        self.crates_editing = false;
        self.info_editing = false;
    }
    pub(crate) fn finish_edits(
        &mut self,
        ctx: &egui::Context,
        editor: &mut Editor,
        scene: &MapScene,
    ) {
        self.finish_edits_inner(ctx, editor, scene, false);
    }
    pub(crate) fn finish_for_save(
        &mut self,
        ctx: &egui::Context,
        editor: &mut Editor,
        scene: &MapScene,
    ) {
        self.finish_edits_inner(ctx, editor, scene, true);
    }
    fn finish_edits_inner(
        &mut self,
        ctx: &egui::Context,
        editor: &mut Editor,
        scene: &MapScene,
        force: bool,
    ) {
        if self.light_editing
            && self.tab == 1
            && self.light != self.original_light
            && self.options == self.original
            && self.crates == self.original_crates
            && self.info == self.original_info
            && self.author == self.original_author
            && self.name == self.original_name
            && self.players == self.original_players
            && !editor.stroking
            && (force
                || ctx.input(|i| i.viewport().close_requested())
                || self.completion.ready(ctx))
        {
            let result = (|| -> anyhow::Result<()> {
                let doc = editor
                    .doc
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!(tr!("没有地图")))?;
                anyhow::ensure!(
                    self.identity == Some((doc.id, doc.state_id)),
                    tr!("文档已变化，请关闭地图设置后重新编辑")
                );
                self.apply_lighting(doc)
            })();
            match result {
                Ok(()) => {
                    editor.commit(scene, "地图光照");
                    self.identity = editor.doc.as_ref().map(|doc| (doc.id, doc.state_id));
                    self.original_light = self.light.clone();
                    self.light_editing = false;
                    self.message = tr!("光照已自动生效，可撤销。").into();
                }
                Err(error) => {
                    self.message = tr!("尚未生效：{error}", error = format!("{:#}", error))
                }
            }
        }
        if self.crates_editing
            && self.tab == 2
            && self.crates != self.original_crates
            && self.options == self.original
            && self.light == self.original_light
            && self.info == self.original_info
            && self.author == self.original_author
            && self.name == self.original_name
            && self.players == self.original_players
            && !editor.stroking
            && (force
                || ctx.input(|i| i.viewport().close_requested())
                || self.completion.ready(ctx))
        {
            let result = (|| -> anyhow::Result<()> {
                let doc = editor
                    .doc
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!(tr!("没有地图")))?;
                anyhow::ensure!(
                    self.identity == Some((doc.id, doc.state_id)),
                    tr!("文档已变化，请关闭地图设置后重新编辑")
                );
                self.crates.as_ref().unwrap().apply(doc)
            })();
            match result {
                Ok(()) => {
                    editor.commit(scene, "箱子设置");
                    self.identity = editor.doc.as_ref().map(|doc| (doc.id, doc.state_id));
                    // Read back normalized values and removed overrides.
                    self.crates = editor
                        .doc
                        .as_ref()
                        .map(ra2_map_document::crate_settings::Draft::read);
                    self.original_crates = self.crates.clone();
                    self.crates_editing = false;
                    self.message = tr!("箱子设置已自动生效，可撤销。").into();
                }
                Err(error) => {
                    self.message = tr!("尚未生效：{error}", error = format!("{:#}", error))
                }
            }
        }
        if self.info_editing
            && self.tab == 3
            && (self.name != self.original_name
                || self.players != self.original_players
                || self.author != self.original_author
                || self.info != self.original_info)
            && self.options == self.original
            && self.light == self.original_light
            && self.crates == self.original_crates
            && !editor.stroking
            && (force
                || ctx.input(|i| i.viewport().close_requested())
                || self.completion.ready(ctx))
        {
            let result = (|| -> anyhow::Result<()> {
                let doc = editor
                    .doc
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!(tr!("没有地图")))?;
                anyhow::ensure!(
                    self.identity == Some((doc.id, doc.state_id)),
                    tr!("文档已变化，请关闭地图设置后重新编辑")
                );
                doc.atomic(|doc| {
                    if self.name != self.original_name {
                        map_settings::rename(doc, self.name.as_deref().unwrap())?;
                    }
                    if self.author != self.original_author {
                        map_settings::set_author(doc, &self.author)?;
                    }
                    if self.players != self.original_players {
                        ra2_map_document::player_starts::set_count(doc, self.players)?;
                    }
                    self.info.as_ref().unwrap().apply(doc)?;
                    Ok(())
                })
            })();
            match result {
                Ok(()) => {
                    editor.commit(scene, "修改地图信息");
                    let doc = editor.doc.as_ref().unwrap();
                    self.identity = Some((doc.id, doc.state_id));
                    self.name = Some(doc.ini_text("Basic", "Name"));
                    self.original_name = self.name.clone();
                    self.author = doc.ini_text("Basic", "Author");
                    self.original_author = self.author.clone();
                    self.original_players = self.players;
                    self.info = Some(ra2_map_document::map_information::Draft::read(doc));
                    self.original_info = self.info.clone();
                    self.info_editing = false;
                    self.message = tr!("地图信息已自动生效，可撤销。").into();
                }
                Err(error) => {
                    self.message = tr!("尚未生效：{error}", error = format!("{:#}", error))
                }
            }
        }
        self.completion.observe(ctx);
    }
    pub fn preview_snapshot(&self, doc: &MapDocument) -> anyhow::Result<(Snapshot, Profile)> {
        if self.open
            && self.tab == 1
            && self.preview
            && let Some(light) = &self.light
        {
            Ok((light.snapshot(doc)?, self.profile))
        } else {
            Ok((doc.snapshot(), Profile::Normal))
        }
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        editor: &mut Editor,
        scene: &MapScene,
        scene_pending: bool,
    ) {
        if !self.open {
            return;
        }
        let Some(doc) = editor.doc.as_ref() else {
            return;
        };
        if self.identity.is_none_or(|(id, _)| id != doc.id)
            || (self.identity != Some((doc.id, doc.state_id)) && !self.dirty())
        {
            self.load(doc);
        }
        let state_before = doc.state_id;
        let finish_close = self.finish_close.take();
        if let Some(field) = finish_close {
            ctx.memory_mut(|memory| memory.surrender_focus(field));
        } else {
            self.finish_edits(ctx, editor, scene);
        }
        let doc = editor.doc.as_ref().unwrap();
        let scene_pending = scene_pending || doc.state_id != state_before;
        let light_before = self.light.clone();
        let crates_before = self.crates.clone();
        let name_before = self.name.clone();
        let author_before = self.author.clone();
        let players_before = self.players;
        let info_before = self.info.clone();
        let mut requested_tab = self.tab;
        let mut open = true;
        let mut apply_map = false;
        let mut apply_production = false;
        let mut changed = false;
        let mut undo = false;
        let mut redo = false;
        let window = crate::ui_theme::window(ctx,tr!("地图设置"),"map-properties",egui::vec2(750.,690.)).open(&mut open).show(ctx,|ui|{
            ui.horizontal_wrapped(|ui|{
                ui.add_enabled_ui(!self.dirty() && finish_close.is_none(), |ui| {
                    ui.selectable_value(&mut requested_tab, 3, tr!("地图信息"));
                    ui.selectable_value(&mut requested_tab, 0, tr!("尺寸与可玩区域"));
                    ui.selectable_value(&mut requested_tab, 1, tr!("光照"));
                    ui.selectable_value(&mut requested_tab, 2, tr!("箱子设置"));
                    ui.selectable_value(&mut requested_tab, 4, tr!("生产限制"));
                });
                if self.dirty(){ui.colored_label(egui::Color32::YELLOW,match self.tab{1=>tr!("光照输入待完成"),2=>tr!("箱子设置输入待完成"),3=>tr!("地图信息输入待完成"),4=>tr!("有未应用的生产限制"),_=>tr!("有未应用尺寸设置")});}
            });
            // Finish the focused field while its page is still visible.
            if requested_tab != self.tab
                && ctx.text_edit_focused()
                && let Some(field) = ctx.memory(|memory| memory.focused())
                && ctx.read_response(field).is_some_and(|response| response.layer_id == ui.layer_id())
            {
                ctx.memory_mut(|memory| memory.surrender_focus(field));
            }
            egui::ScrollArea::vertical().max_height((ui.available_height()-100.).max(140.)).show(ui,|ui|{
                if self.tab==0 {
                    let options=self.options.as_mut().unwrap();
                    ui.label(tr!("当前尺寸：{arg0} × {arg1}；宽＋高 ≤ {MAX_MAP_SPAN}。", arg0 = doc.base.header.width, arg1 = doc.base.header.height, MAX_MAP_SPAN = MAX_MAP_SPAN));
                    ui.add_enabled_ui(self.worker.is_none(), |ui| {
                        egui::Grid::new("map-dimensions").num_columns(2).show(ui,|ui|{
                            let previous_size = [options.width, options.height];
                            let mut margins = local_margins(previous_size, options.local);
                            for (name,value) in [(tr!("新宽度"),&mut options.width),(tr!("新高度"),&mut options.height)]{ui.label(name);changed|=crate::form_edit::number(ui,egui::DragValue::new(value).range(MIN_MAP_DIMENSION..=MAX_MAP_DIMENSION),name).changed();ui.end_row();}
                            ui.label(tr!("原图移动（向右、向下）"))
                                .on_hover_text(tr!("按投影地图格数移动。负数向左或向上。"));
                            ui.horizontal(|ui| {
                                let limit = i32::from(MAX_MAP_SPAN);
                                let names = [tr!("原图向右移动"), tr!("原图向下移动")];
                                for (value, name) in options.offset.iter_mut().zip(names) {
                                    changed |= crate::form_edit::number(
                                        ui,
                                        egui::DragValue::new(value).range(-limit..=limit),
                                        name,
                                    )
                                    .changed();
                                }
                                ui.add(egui::Label::new(
                                    egui::RichText::new(tr!("单位为格，负数向左或向上。如 (5, -3) 表示右移 5 格、上移 3 格。"))
                                        .small()
                                        .weak(),
                                ).wrap());
                            });
                            ui.end_row();
                            ui.label(tr!("扩展区域清地高度"));changed|=crate::form_edit::number(ui,egui::DragValue::new(&mut options.fill_level).range(0..=127),tr!("扩展区域清地高度")).changed();ui.end_row();
                            let map_size = [options.width, options.height];
                            ui.label(tr!("可玩区域（左、上、右、下）"))
                                .on_hover_text(tr!("可玩区域距地图四边的距离"));
                            let mut local_changed = false;
                            ui.horizontal_wrapped(|ui| {
                                let names = [
                                    tr!("可玩区域左侧"),
                                    tr!("可玩区域顶部"),
                                    tr!("可玩区域右侧"),
                                    tr!("可玩区域底部"),
                                ];
                                for (value, name) in margins.iter_mut().zip(names) {
                                    local_changed |= crate::form_edit::number(
                                        ui,
                                        egui::DragValue::new(value)
                                            .range(0..=MAX_MAP_DIMENSION)
                                            .clamp_existing_to_range(false),
                                        name,
                                    )
                                    .changed();
                                }
                            });
                            if local_changed || map_size != previous_size {
                                options.local = local_from_margins(map_size, margins);
                                changed = true;
                            }
                            ui.end_row();
                        });
                        ui.horizontal_wrapped(|ui|{
                            if ui.button(tr!("可玩区域铺满有效边距")).clicked(){options.local=[2,4,i32::from(options.width)-4,i32::from(options.height)-10];changed=true;}
                            if ui.button(tr!("居中放置原图")).clicked(){options.offset=[(i32::from(options.width)-doc.base.header.width as i32)/2,(i32::from(options.height)-doc.base.header.height as i32)/2];changed=true;}
                        });
                        changed|=ui.checkbox(&mut options.delete_outside,tr!("应用时删除可移除的越界内容")).changed();
                    });
                    ui.small(tr!("裁切可删除越界对象、覆盖物、格子标签、基地节点和整条隧道。越界航点、桥梁、脚本目的格和任务可视矩形须先调整；共享标签保留。建筑按当前美术配置中的占地检查。"));
                    if let Err(error) = options.validate() {
                        ui.colored_label(egui::Color32::LIGHT_RED, error.to_string());
                    }
                    ui.horizontal(|ui| {
                        apply_map = ui.add_enabled(self.worker.is_none(), egui::Button::new(tr!("应用地图设置（整笔撤销）"))).clicked();
                        if self.worker.is_some() {
                            ui.spinner();
                        }
                    });
                    if !changed && !self.blocked.is_empty() {
                        ui.label(tr!("有 {count} 项越界内容阻止应用。", count = self.blocked.len()));
                        for affected in self.blocked.iter().take(100) {
                            let (color, hint) = if affected.blocking {
                                (egui::Color32::LIGHT_RED, tr!("请先调整"))
                            } else {
                                (egui::Color32::YELLOW, tr!("可勾选删除"))
                            };
                            ui.colored_label(color, format!("{} {:?} · {}", affected.source, affected.cell, hint));
                        }
                    }
                }else if self.tab==1{
                    ui.horizontal_wrapped(|ui|{
                        for p in Profile::ALL{changed|=ui.selectable_value(&mut self.profile,p,crate::i18n::text(p.label())).changed();}
                        changed|=ui.checkbox(&mut self.preview,tr!("在画布实时预览")).changed();
                    });
                    ui.small(tr!("光照参数结束输入后自动生效，可撤销。关闭面板恢复普通光照预览，保存与发布使用当前参数。空值使用引擎默认值。"));
                    let light=self.light.as_mut().unwrap();
                    egui::Grid::new("lighting-components").num_columns(2).show(ui,|ui|{
                        for component in COMPONENTS{let key=format!("{}{component}",self.profile.prefix());let label=crate::labels::field(&key);ui.label(label).on_hover_text(&key);changed|=ui.text_edit_singleline(light.values.get_mut(&key).unwrap()).accessible_name(label).changed();ui.end_row();}
                        for key in ["DominatorAmbientChangeRate","NukeAmbientChangeRate"]{let label=crate::labels::field(key);ui.label(label).on_hover_text(key);changed|=ui.text_edit_singleline(light.values.get_mut(key).unwrap()).accessible_name(label).changed();ui.end_row();}
                    });
                    ui.small(tr!("环境亮度与红绿蓝分量按 1%=1 单位量化；地面与每层高度亮度补偿按 0.001 量化。心灵控制器顶部层高使用原生核爆参数（每层补偿 0.1）。过渡间隔只编辑，不模拟动态淡入淡出。"));
                    if let Ok(p)=light.effective(doc,self.profile){ui.label(tr!("实际量化：环境亮度 {arg0}%，红绿蓝 {arg1}%，地面补偿 {arg2} / 每层补偿 {arg3}（千分单位）", arg0 = p.units.ambient_percent, arg1 = format!("{:?}", [p.units.red_percent,p.units.green_percent,p.units.blue_percent]), arg2 = p.units.ground_units, arg3 = p.units.level_units));}
                    if let Err(e)=light.validate(){ui.colored_label(egui::Color32::LIGHT_RED,e.to_string());}
                    ui.horizontal_wrapped(|ui|{
                        if ui.button(tr!("所选配置使用引擎默认")).clicked(){for c in COMPONENTS{light.values.insert(format!("{}{c}",self.profile.prefix()),String::new());}changed=true;}
                    });
                }else if self.tab==3{
                    crate::map_information_ui::show(ui,self.info.as_mut().unwrap(),self.original_info.as_ref().unwrap(),doc,scene,
                        crate::map_information_ui::IdentityFields { name:self.name.as_mut().unwrap(),author:&mut self.author,players:&mut self.players });
                }else if self.tab==4 {
                    apply_production = production_fields(
                        ui, &mut self.production, &self.original_production, doc,
                        scene.catalog.production_inventory.as_deref(), editor.stroking || scene_pending,
                    );
                }else{
                    crate_fields(ui,self.crates.as_mut().unwrap());
                }
            });
            ui.separator();
            ui.horizontal_wrapped(|ui|{
                undo=ui.add_enabled(!self.dirty(),egui::Button::new(tr!("撤销"))).clicked();
                redo=ui.add_enabled(!self.dirty(),egui::Button::new(tr!("取消撤销"))).clicked();
            });
            ui.label(&self.message);
        });
        if changed {
            if self.tab == 0 && self.worker.is_none() {
                self.blocked.clear();
                self.message.clear();
            }
            if self.tab == 1 {
                self.due = Some(Instant::now());
            }
        }
        if undo || redo {
            editor.history(redo, scene);
            self.load(editor.doc.as_ref().unwrap());
        }
        if self.light != light_before {
            self.light_editing = true;
        }
        if self.crates != crates_before {
            self.crates_editing = true;
        }
        if self.name != name_before
            || self.players != players_before
            || self.author != author_before
            || self.info != info_before
        {
            self.info_editing = true;
        }
        if apply_production {
            self.apply_production(editor, scene);
        }
        // An accessibility click can close the window without blurring the field.
        // Draw the numeric editor once more after surrendering focus so it can publish its value.
        if !open
            && finish_close.is_none()
            && self.tab == 3
            && ctx.text_edit_focused()
            && let Some(field) = ctx.memory(|memory| memory.focused())
            && let Some(window) = window
            && ctx
                .read_response(field)
                .is_some_and(|response| response.layer_id == window.response.layer_id)
        {
            self.finish_close = Some(field);
            ctx.request_repaint();
            return;
        }
        let closing = !open || finish_close.is_some();
        let switching_tabs = requested_tab != self.tab;
        self.finish_edits_inner(ctx, editor, scene, closing || switching_tabs);
        if closing {
            self.load(editor.doc.as_ref().unwrap());
            self.message.clear();
            self.open = false;
            self.due = None;
            editor.refresh = true;
            return;
        }
        if apply_map && self.options != self.original {
            self.start_apply(ctx, editor.doc.as_ref().unwrap(), scene);
        } else {
            self.poll_apply(ctx, editor, scene);
        }
        if switching_tabs && !self.dirty() {
            if self.tab == 1 || requested_tab == 1 {
                self.due = Some(Instant::now());
            }
            self.tab = requested_tab;
            if self.tab == 0 && self.worker.is_none() {
                self.blocked.clear();
                self.message.clear();
            }
            ctx.request_repaint();
        }
        if let Some(start) = self.due {
            if start.elapsed() >= Duration::from_millis(250) {
                if self.preview_snapshot(editor.doc.as_ref().unwrap()).is_ok() {
                    editor.refresh = true;
                }
                self.due = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
    }

    fn apply_production(&mut self, editor: &mut Editor, scene: &MapScene) {
        let doc = editor.doc.as_mut().unwrap();
        if self.identity != Some((doc.id, doc.state_id)) {
            self.message = tr!("文档已变化，请关闭地图设置后重新编辑").into();
            return;
        }
        match production_settings::apply(
            doc,
            &self.production,
            scene.catalog.production_inventory.as_deref(),
        ) {
            Ok(count) => {
                editor.commit(scene, "生产限制");
                self.load(editor.doc.as_ref().unwrap());
                self.message = tr!("已应用生产限制，涉及 {count} 种类型。", count = count);
            }
            Err(error) => {
                self.message = tr!("尚未生效：{error}", error = format!("{error:#}"));
            }
        }
    }

    fn start_apply(&mut self, ctx: &egui::Context, doc: &MapDocument, scene: &MapScene) {
        let footprints = footprints(scene, doc);
        let mut scratch = doc.fork();
        scratch.id = doc.id;
        scratch.state_id = doc.state_id;
        let options = self.options.clone().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        self.worker = Some(rx);
        self.blocked.clear();
        self.message = tr!("正在后台检查迁移…").into();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(map_settings::plan(&scratch, options, &footprints));
            ctx.request_repaint();
        });
    }

    fn poll_apply(&mut self, ctx: &egui::Context, editor: &mut Editor, scene: &MapScene) {
        let Some(worker) = &self.worker else {
            return;
        };
        let result = match worker.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.worker = None;
                self.message = tr!("迁移检查线程意外结束。").into();
                ctx.request_repaint();
                return;
            }
        };
        self.worker = None;
        ctx.request_repaint();
        match result {
            Ok(plan) if Some(&plan.options) != self.options.as_ref() => {
                self.message = tr!("设置已改变，请重新应用。").into();
            }
            Ok(plan) if !plan.can_apply() => {
                self.blocked = plan
                    .summary
                    .affected
                    .into_iter()
                    .filter(|item| item.blocking || !plan.options.delete_outside)
                    .collect();
                self.message.clear();
            }
            Ok(plan) => match plan.apply(editor.doc.as_mut().unwrap()) {
                Ok(did_change) => {
                    if did_change {
                        editor.structural_changed(scene);
                    }
                    self.load(editor.doc.as_ref().unwrap());
                    self.message = if did_change {
                        tr!("已应用地图设置，可整笔撤销。").into()
                    } else {
                        String::new()
                    };
                }
                Err(error) => self.message = format!("{error:#}"),
            },
            Err(error) => self.message = format!("{error:#}"),
        }
    }
}
fn crate_fields(ui: &mut egui::Ui, draft: &mut ra2_map_document::crate_settings::Draft) {
    ui.heading(tr!("随机箱子"));
    ui.label(tr!(
        "控制本张地图的箱子数量与刷新。结束输入后自动生效，可撤销；保存地图后保留设置。"
    ));
    ui.small(tr!(
        "留空表示继承游戏或 Mod 规则，字段提示中的原版参考值仅供参考。"
    ));
    ui.add_space(8.);
    egui::Grid::new("crate-settings")
        .num_columns(2)
        .spacing([18., 12.])
        .show(ui, |ui| {
            for field in &ra2_map_document::crate_settings::FIELDS {
                ui.label(crate::i18n::text(field.label))
                    .on_hover_text(format!(
                        "[CrateRules] {}\n{}",
                        field.key,
                        crate::i18n::text(field.hint)
                    ));
                ui.add(
                    egui::TextEdit::singleline(draft.values.get_mut(field.key).unwrap())
                        .desired_width(160.)
                        .hint_text(tr!("继承游戏 / Mod")),
                )
                .accessible_name(crate::i18n::text(field.label))
                .on_hover_text(crate::i18n::text(field.hint));
                ui.end_row();
            }
        });
    ui.add_space(10.);
    ui.label(tr!(
        "固定开局数量：将最少和最多箱子数设为相同值；最多设为 0 可禁止开局随机生成。"
    ));
    ui.small(tr!("初始数量按人类玩家人数限制在上下限之间，实际生成还取决于可放置位置。刷新间隔越小，未拾取箱子换位置越频繁；实际间隔在约 0.5～2 倍之间随机，拾取后会尝试立即补充。"));
    ui.small(tr!(
        "遭遇战或试玩需同时开启“随机箱子”。单人任务中预放的箱子不使用随机刷新机制。"
    ));
    if let Err(error) = draft.validate() {
        ui.colored_label(egui::Color32::LIGHT_RED, error.to_string());
    }
    if ui.button(tr!("全部继承游戏 / Mod 规则")).clicked() {
        for value in draft.values.values_mut() {
            value.clear();
        }
    }
}

fn production_fields(
    ui: &mut egui::Ui,
    selected: &mut Restrictions,
    original: &Restrictions,
    doc: &ra2_map_document::document::MapDocument,
    inventory: Option<&production_settings::Inventory>,
    blocked: bool,
) -> bool {
    ui.small(tr!("空军包括飞机、飞行步兵和飞行载具。海军按 Naval 标记识别。预置和任务直接生成的对象仍可使用。"));
    ui.add_space(8.);
    let available = inventory.is_some_and(|types| !types.is_empty());
    for restriction in Restriction::ALL {
        let mut enabled = selected.contains(&restriction);
        let mut row_changed = false;
        ui.horizontal_wrapped(|ui| {
            row_changed = ui
                .add_enabled(
                    available || enabled,
                    egui::Checkbox::new(&mut enabled, crate::i18n::text(restriction.label())),
                )
                .changed();
            if let Some(types) = inventory {
                let count = types
                    .values()
                    .filter(|groups| groups.contains(&restriction))
                    .count();
                ui.weak(tr!("{count} 种类型", count = count));
            }
        });
        if row_changed {
            if enabled {
                selected.insert(restriction);
            } else {
                selected.remove(&restriction);
            }
        }
    }
    if !available {
        ui.small(tr!("当前游戏资源的生产类型不可用"));
    }
    let outdated = !blocked
        && selected == original
        && inventory.is_some_and(|types| production_settings::needs_apply(doc, selected, types));
    if outdated {
        ui.small(tr!("生产规则与当前类型清单不一致，请重新应用。"));
    }
    ui.add_space(8.);
    ui.add_enabled(
        !blocked && (selected != original || outdated) && (available || selected.is_empty()),
        egui::Button::new(tr!("应用生产限制")),
    )
    .clicked()
}

pub(crate) fn footprints(scene: &MapScene, doc: &MapDocument) -> Footprints {
    doc.records()
        .values()
        .filter(|r| r.section == "Structures")
        .map(|r| {
            (
                r.type_id(),
                ra2_map_scene::placement::offsets(scene, &r.section, &r.type_id()),
            )
        })
        .collect()
}
fn local_margins([map_width, map_height]: [u16; 2], local: [i32; 4]) -> [i64; 4] {
    let [left, top, width, height] = local.map(i64::from);
    [
        left,
        top,
        i64::from(map_width) - left - width,
        i64::from(map_height) - top - height,
    ]
}

fn local_from_margins(
    [map_width, map_height]: [u16; 2],
    [left, top, right, bottom]: [i64; 4],
) -> [i32; 4] {
    [
        left,
        top,
        i64::from(map_width) - left - right,
        i64::from(map_height) - top - bottom,
    ]
    .map(|value| value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_settings_apply_and_undo_through_map_settings() {
        use egui_kittest::kittest::Queryable as _;
        let _language = crate::i18n::enter(crate::i18n::Language::Chinese);
        let (mut editor, _, root) = crate::editing::tests::editor();
        let mut scene = crate::editing::tests::terrain_scene();
        std::sync::Arc::make_mut(&mut scene.catalog).production_inventory =
            Some(std::sync::Arc::new(
                [
                    ("ORCA".into(), [Restriction::Air].into_iter().collect()),
                    ("DEST".into(), [Restriction::Naval].into_iter().collect()),
                ]
                .into_iter()
                .collect(),
            ));
        editor.attach(&scene);
        let form = MapSettingsUi {
            open: true,
            tab: 4,
            ..Default::default()
        };
        let scene = std::cell::RefCell::new(scene);
        let loading = std::cell::Cell::new(false);
        let mut harness = egui_kittest::Harness::<(MapSettingsUi, Editor)>::builder()
            .with_context_setup(|ctx| {
                crate::ui_theme::install(ctx);
                ctx.add_font_provider(std::sync::Arc::new(
                    egui_system_font_provider::SystemFontProvider::new(),
                ));
            })
            .with_size([1000., 850.])
            .build_ui_state(
                |ui, (form, editor)| form.show(ui.ctx(), editor, &scene.borrow(), loading.get()),
                (form, editor),
            );
        harness.get_by_label("禁止生产空军").click();
        harness.run();
        harness.get_by_label("禁止生产海军").click();
        harness.run();
        assert!(harness.state().0.dirty());
        let doc = harness.state().1.doc.as_ref().unwrap();
        assert!(production_settings::read(doc).is_empty());
        harness.get_by_label("应用生产限制").click();
        harness.run();
        assert!(!harness.state().0.dirty());
        let doc = harness.state().1.doc.as_ref().unwrap();
        assert_eq!(doc.ini_text("ORCA", "TechLevel"), "-1");
        assert_eq!(doc.ini_text("DEST", "BuildLimit"), "0");
        let applied_state = doc.state_id;
        harness.get_by_label("禁止生产空军").click();
        harness.run();
        assert!(harness.state().0.dirty());
        assert!(harness.query_by_label("取消本次输入").is_none());
        harness.get_by_label("Close window").click();
        harness.run();
        let (form, editor) = harness.state();
        assert!(!form.open);
        assert!(!form.dirty());
        let doc = editor.doc.as_ref().unwrap();
        assert_eq!(doc.state_id, applied_state);
        assert_eq!(doc.ini_text("ORCA", "TechLevel"), "-1");
        assert!(form.production.contains(&Restriction::Air));
        harness.state_mut().0.open = true;
        harness.run();

        let mut updated = scene.borrow_mut();
        let catalog = std::sync::Arc::make_mut(&mut updated.catalog);
        let types = std::sync::Arc::make_mut(catalog.production_inventory.as_mut().unwrap());
        types.remove("ORCA");
        types.insert("MODEJET".into(), [Restriction::Air].into_iter().collect());
        drop(updated);
        loading.set(true);
        harness.run();
        assert!(
            harness
                .query_by_label("生产规则与当前类型清单不一致，请重新应用。")
                .is_none()
        );
        loading.set(false);
        harness.run();
        assert!(
            harness
                .query_by_label("生产规则与当前类型清单不一致，请重新应用。")
                .is_some()
        );
        assert!(!harness.state().0.dirty());
        harness.get_by_label("应用生产限制").click();
        harness.run();
        let doc = harness.state().1.doc.as_ref().unwrap();
        assert!(doc.ini_value("ORCA", "TechLevel").is_none());
        assert_eq!(doc.ini_text("MODEJET", "BuildLimit"), "0");
        harness.get_by_label("撤销").click();
        harness.run();
        let doc = harness.state().1.doc.as_ref().unwrap();
        assert_eq!(doc.ini_text("ORCA", "TechLevel"), "-1");
        assert!(doc.ini_value("MODEJET", "BuildLimit").is_none());
        harness.get_by_label("撤销").click();
        harness.run();
        let doc = harness.state().1.doc.as_ref().unwrap();
        assert!(production_settings::read(doc).is_empty());
        assert!(doc.ini_value("ORCA", "TechLevel").is_none());
        assert!(doc.ini_value("DEST", "BuildLimit").is_none());
        drop(harness);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn playable_area_margins_round_trip_to_native_local_size() {
        let local = [2, 4, 76, 70];
        assert_eq!(local_margins([80, 80], local), [2, 4, 2, 6]);
        assert_eq!(local_from_margins([80, 80], [2, 4, 2, 6]), local);
        assert_eq!(local_margins([91, 83], local), [2, 4, 13, 9]);
        assert_eq!(local_margins([76, 73], local), [2, 4, -2, -1]);
        let malformed = [i32::MAX, i32::MIN, i32::MAX, i32::MIN];
        assert_eq!(
            local_from_margins([80, 80], local_margins([80, 80], malformed)),
            malformed
        );
    }

    #[test]
    fn movement_hint_fits_beside_offset_inputs() {
        use crate::i18n::{Language, enter, text};
        use egui_kittest::kittest::Queryable as _;
        let (mut editor, _, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        for language in [Language::Chinese, Language::English] {
            let _language = enter(language);
            let form = MapSettingsUi {
                open: true,
                ..Default::default()
            };
            let mut harness = egui_kittest::Harness::builder()
                .with_context_setup(|ctx| {
                    crate::ui_theme::install(ctx);
                    ctx.add_font_provider(std::sync::Arc::new(
                        egui_system_font_provider::SystemFontProvider::new(),
                    ));
                })
                .with_size([1000., 850.])
                .build_ui_state(
                    |ui, form| form.show(ui.ctx(), &mut editor, &scene, false),
                    form,
                );
            for size in [[1000., 850.], [750., 600.], [600., 480.], [1000., 850.]] {
                harness.set_size(size.into());
                harness.run();
                let window = harness
                    .ctx
                    .memory(|m| m.area_rect(egui::Id::unique("map-properties")))
                    .unwrap();
                let right = harness.get_by_label(text("原图向右移动")).rect();
                let down = harness.get_by_label(text("原图向下移动")).rect();
                let hint = harness
                    .get_by_label(text(
                        "单位为格，负数向左或向上。如 (5, -3) 表示右移 5 格、上移 3 格。",
                    ))
                    .rect();
                assert!(
                    [right, down, hint]
                        .into_iter()
                        .all(|widget| window.contains_rect(widget)),
                    "offset row outside the dialog at {size:?} in {language:?}"
                );
                assert!(right.right() <= down.left() && down.right() <= hint.left());
                assert!((right.center().y - down.center().y).abs() <= 1.);
                assert!(hint.y_range().contains(down.center().y));
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn map_size_applies_in_one_click_and_reports_blocked_migrations() {
        use egui_kittest::kittest::Queryable as _;
        let (mut editor, _, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let original_bytes = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        let form = MapSettingsUi {
            open: true,
            ..Default::default()
        };
        let mut harness = egui_kittest::Harness::<(MapSettingsUi, Editor)>::builder()
            .with_context_setup(|ctx| {
                crate::ui_theme::install(ctx);
                ctx.add_font_provider(std::sync::Arc::new(
                    egui_system_font_provider::SystemFontProvider::new(),
                ));
            })
            .with_size([1000., 850.])
            .build_ui_state(
                |ui, (form, editor)| form.show(ui.ctx(), editor, &scene, false),
                (form, editor),
            );
        let role = egui::accesskit::Role::SpinButton;
        for (name, displayed) in [
            ("可玩区域左侧", "2"),
            ("可玩区域顶部", "4"),
            ("可玩区域右侧", "2"),
            ("可玩区域底部", "6"),
        ] {
            let field = harness.get_by_role_and_label(role, name);
            assert_eq!(field.value().as_deref(), Some(displayed));
        }
        harness.get_by_role_and_label(role, "可玩区域底部").focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(role, "可玩区域底部")
            .type_text("8");
        harness.run();
        assert!(!harness.state().0.dirty());
        let apply = |harness: &mut egui_kittest::Harness<'_, (MapSettingsUi, Editor)>| {
            harness.get_by_label("应用地图设置（整笔撤销）").click();
            harness.step();
            let deadline = Instant::now() + Duration::from_secs(5);
            while harness.state().0.worker.is_some() {
                harness.step();
                assert!(Instant::now() < deadline, "map settings worker timeout");
                std::thread::yield_now();
            }
            harness.run();
        };

        apply(&mut harness);
        let (form, editor) = harness.state();
        assert!(!form.dirty(), "{}", form.message);
        let header = &editor.doc.as_ref().unwrap().base.header;
        assert_eq!(header.local_left, 2);
        assert_eq!(header.local_top, 4);
        assert_eq!(header.local_width, 16);
        assert_eq!(header.local_height, 8);

        for (name, size, expected_local) in [
            ("新宽度", "27", [2, 4, 23, 8]),
            ("新高度", "25", [2, 4, 23, 13]),
            ("新宽度", "24", [2, 4, 20, 13]),
        ] {
            harness.get_by_role_and_label(role, name).focus();
            harness.run();
            harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
            harness.get_by_role_and_label(role, name).type_text(size);
            harness.run();
            harness.key_press(egui::Key::Enter);
            harness.run();
            for (margin, expected) in [
                ("可玩区域左侧", "2"),
                ("可玩区域顶部", "4"),
                ("可玩区域右侧", "2"),
                ("可玩区域底部", "8"),
            ] {
                let field = harness.get_by_role_and_label(role, margin);
                assert_eq!(field.value().as_deref(), Some(expected), "{name}={size}");
            }
            assert_eq!(
                harness.state().0.options.as_ref().unwrap().local,
                expected_local
            );
        }
        apply(&mut harness);
        let (form, editor) = harness.state();
        assert!(!form.dirty(), "{}", form.message);
        let header = &editor.doc.as_ref().unwrap().base.header;
        assert_eq!((header.width, header.height), (24, 25));
        assert_eq!((header.local_width, header.local_height), (20, 13));

        for _ in 0..2 {
            harness.get_by_label("撤销").click();
            harness.run();
        }
        let bytes = |harness: &egui_kittest::Harness<'_, (MapSettingsUi, Editor)>| {
            harness
                .state()
                .1
                .doc
                .as_ref()
                .unwrap()
                .snapshot()
                .bytes()
                .unwrap()
        };
        assert_eq!(bytes(&harness), original_bytes);

        harness.get_by_label("单位为格，负数向左或向上。如 (5, -3) 表示右移 5 格、上移 3 格。");
        for name in ["原图向右移动", "原图向下移动"] {
            harness.get_by_role_and_label(role, name).focus();
            harness.run();
            harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
            harness.get_by_role_and_label(role, name).type_text("20");
            harness.run();
            harness.key_press(egui::Key::Enter);
            harness.run();
        }
        let options = harness.state_mut().0.options.as_mut().unwrap();
        assert_eq!(options.offset, [20, 20]);
        options.delete_outside = true;
        harness.run();
        apply(&mut harness);
        let form = &harness.state().0;
        assert!(form.dirty());
        assert!(form.blocked.iter().any(|item| item.blocking));
        assert_eq!(bytes(&harness), original_bytes);

        assert!(harness.query_by_label("放弃尺寸修改").is_none());
        harness.get_by_label("Close window").click();
        harness.run();
        let form = &harness.state().0;
        assert!(!form.open);
        assert!(!form.dirty());
        assert!(form.blocked.is_empty());
        assert_eq!(bytes(&harness), original_bytes);

        harness.state_mut().0.open = true;
        harness.run();
        let (form, editor) = harness.state();
        let expected = Options::from_document(editor.doc.as_ref().unwrap());
        assert_eq!(form.options.as_ref(), Some(&expected));
        drop(harness);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn map_information_mode_input_applies_on_enter_through_the_form() {
        use egui_kittest::kittest::Queryable as _;
        let (mut editor, _, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let form = MapSettingsUi {
            open: true,
            tab: 3,
            ..Default::default()
        };
        let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        let mut harness = egui_kittest::Harness::<(MapSettingsUi, Editor)>::builder()
            .with_context_setup(|ctx| {
                crate::ui_theme::install(ctx);
                ctx.add_font_provider(std::sync::Arc::new(
                    egui_system_font_provider::SystemFontProvider::new(),
                ));
            })
            .with_size([1000., 1000.])
            .build_ui_state(
                |ui, (form, editor)| form.show(ui.ctx(), editor, &scene, false),
                (form, editor),
            );
        harness.get_by_label("自定义模式…").scroll_to_me();
        harness.run();
        harness.get_by_label("自定义模式…").click();
        harness.run();
        harness
            .get_by_role_and_label(egui::accesskit::Role::TextInput, "适用游戏模式")
            .focus();
        harness.run();
        harness
            .get_by_role_and_label(egui::accesskit::Role::TextInput, "适用游戏模式")
            .type_text("standard,teamgame");
        harness.run();
        assert!(harness.state().0.dirty());
        assert_eq!(
            harness
                .state()
                .1
                .doc
                .as_ref()
                .unwrap()
                .snapshot()
                .bytes()
                .unwrap(),
            before
        );
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert!(!harness.state().0.dirty());
        assert_eq!(
            harness
                .state()
                .1
                .doc
                .as_ref()
                .unwrap()
                .ini_text("Basic", "GameMode"),
            "standard,teamgame"
        );
        drop(harness);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn map_information_save_is_atomic_and_reloads_normalized_values_after_history() {
        let (mut editor, ctx, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let mut form = MapSettingsUi {
            open: true,
            tab: 3,
            ..Default::default()
        };
        form.load(editor.doc.as_ref().unwrap());
        let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        form.name = Some("新的地图".into());
        let info = form.info.as_mut().unwrap();
        info.values
            .insert(("Basic", "CarryOverMoney"), "NaN".into());
        info.values.insert(("Basic", "FreeRadar"), "yes".into());
        info.values
            .insert(("MultiplayerDialogSettings", "Bases"), "no".into());
        form.info_editing = true;
        form.finish_for_save(&ctx, &mut editor, &scene);
        assert!(form.dirty());
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            before
        );
        form.info
            .as_mut()
            .unwrap()
            .values
            .insert(("Basic", "CarryOverMoney"), " 50% ".into());
        form.finish_for_save(&ctx, &mut editor, &scene);
        assert!(!form.dirty());
        assert_eq!(
            form.info.as_ref().unwrap().values[&("Basic", "CarryOverMoney")],
            "50%"
        );
        assert_eq!(
            editor
                .doc
                .as_ref()
                .unwrap()
                .ini_text("MultiplayerDialogSettings", "Bases"),
            "no"
        );
        assert_eq!(
            editor.doc.as_ref().unwrap().ini_text("Basic", "FreeRadar"),
            "yes"
        );
        let after = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        editor.history(false, &scene);
        crate::test_support::run_ui(&ctx, Default::default(), |ui| {
            form.show(ui.ctx(), &mut editor, &scene, false)
        });
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            before
        );
        assert_eq!(
            form.info.as_ref().unwrap().values[&("Basic", "FreeRadar")],
            ""
        );
        editor.history(true, &scene);
        crate::test_support::run_ui(&ctx, Default::default(), |ui| {
            form.show(ui.ctx(), &mut editor, &scene, false)
        });
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            after
        );
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn start_count_input_commits_before_save_and_navigation_and_supports_history() {
        use egui_kittest::kittest::Queryable as _;
        let (mut editor, _, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        let form = MapSettingsUi {
            open: true,
            tab: 3,
            ..Default::default()
        };
        let mut harness = egui_kittest::Harness::<(MapSettingsUi, Editor)>::builder()
            .with_context_setup(|ctx| {
                crate::ui_theme::install(ctx);
                ctx.add_font_provider(std::sync::Arc::new(
                    egui_system_font_provider::SystemFontProvider::new(),
                ));
            })
            .with_size([1000., 850.])
            .build_ui_state(
                |ui, (form, editor): &mut (MapSettingsUi, Editor)| {
                    form.show(ui.ctx(), editor, &scene, false);
                    if crate::form_edit::take_save_shortcut(ui.ctx()).is_some() {
                        form.finish_for_save(ui.ctx(), editor, &scene);
                    }
                },
                (form, editor),
            );
        harness
            .get_by_role_and_label(egui::accesskit::Role::SpinButton, "出生点数量")
            .focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(egui::accesskit::Role::SpinButton, "出生点数量")
            .type_text("4");
        harness.run();
        assert_eq!(
            harness
                .state()
                .1
                .doc
                .as_ref()
                .unwrap()
                .snapshot()
                .bytes()
                .unwrap(),
            before
        );
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert_eq!(
            ra2_map_document::player_starts::from_document(harness.state().1.doc.as_ref().unwrap()),
            vec![0, 1, 2, 3]
        );
        assert!(!harness.state().0.dirty());
        harness.get_by_label("撤销").click();
        harness.run();
        assert_eq!(
            harness
                .state()
                .1
                .doc
                .as_ref()
                .unwrap()
                .snapshot()
                .bytes()
                .unwrap(),
            before
        );
        harness.get_by_label("取消撤销").click();
        harness.run();
        assert_eq!(
            harness
                .state()
                .1
                .doc
                .as_ref()
                .unwrap()
                .ini_text("Basic", "MaxPlayers"),
            "4"
        );
        harness
            .get_by_role_and_label(egui::accesskit::Role::SpinButton, "出生点数量")
            .focus();
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(egui::accesskit::Role::SpinButton, "出生点数量")
            .type_text("2");
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert_eq!(
            ra2_map_document::player_starts::from_document(harness.state().1.doc.as_ref().unwrap()),
            vec![0, 1]
        );
        assert!(!harness.state().0.dirty());

        for (via_accesskit, text, expected) in [(false, "5", 5), (true, "6", 6)] {
            let (role, label) = (egui::accesskit::Role::SpinButton, "出生点数量");
            harness.get_by_role_and_label(role, label).focus();
            harness.run();
            harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
            harness.get_by_role_and_label(role, label).type_text(text);
            harness.run();
            let tab = harness.get_by_label("箱子设置");
            if via_accesskit {
                tab.click_accesskit();
            } else {
                tab.click();
            }
            harness.run();
            let (form, editor) = harness.state();
            assert_eq!(form.tab, 2);
            assert!(!form.dirty());
            assert_eq!(
                ra2_map_document::player_starts::from_document(editor.doc.as_ref().unwrap()),
                (0..expected).collect::<Vec<_>>(),
                "switching tabs via AccessKit: {via_accesskit}"
            );
            harness.get_by_label("地图信息").click();
            harness.run();
        }

        for via_accesskit in [false, true] {
            harness.state_mut().0.open = true;
            harness.run();
            let input = || (egui::accesskit::Role::SpinButton, "出生点数量");
            harness.get_by_role_and_label(input().0, input().1).focus();
            harness.run();
            harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
            let (text, expected) = if via_accesskit { ("3", 3) } else { ("4", 4) };
            harness
                .get_by_role_and_label(input().0, input().1)
                .type_text(text);
            harness.run();
            let close = harness.get_by_label("Close window");
            if via_accesskit {
                close.click_accesskit();
            } else {
                close.click();
            }
            harness.run();
            let (form, editor) = harness.state();
            assert!(!form.open);
            assert!(!form.dirty());
            assert_eq!(
                ra2_map_document::player_starts::from_document(editor.doc.as_ref().unwrap()),
                (0..expected).collect::<Vec<_>>()
            );
        }
        drop(harness);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn map_name_input_commits_on_enter_and_save_and_discards_invalid_input_on_close() {
        use egui_kittest::kittest::Queryable as _;
        let (mut editor, _, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        let form = MapSettingsUi {
            open: true,
            tab: 3,
            ..Default::default()
        };
        let mut harness = egui_kittest::Harness::<(MapSettingsUi, Editor)>::builder()
            .with_context_setup(|ctx| {
                crate::ui_theme::install(ctx);
                ctx.add_font_provider(std::sync::Arc::new(
                    egui_system_font_provider::SystemFontProvider::new(),
                ));
            })
            .with_size([1000., 850.])
            .build_ui_state(
                |ui, (form, editor)| {
                    form.show(ui.ctx(), editor, &scene, false);
                    if crate::form_edit::take_save_shortcut(ui.ctx()).is_some() {
                        form.finish_for_save(ui.ctx(), editor, &scene);
                    }
                },
                (form, editor),
            );
        let input = || (egui::accesskit::Role::TextInput, "地图名称");
        harness.get_by_role_and_label(input().0, input().1).focus();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(input().0, input().1)
            .type_text("中文地图");
        harness.run();
        assert!(harness.state().0.dirty());
        assert_eq!(
            harness
                .state()
                .1
                .doc
                .as_ref()
                .unwrap()
                .snapshot()
                .bytes()
                .unwrap(),
            before
        );
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert!(!harness.state().0.dirty());
        assert_eq!(
            harness
                .state()
                .1
                .doc
                .as_ref()
                .unwrap()
                .ini_text("Basic", "Name"),
            "中文地图"
        );
        harness.get_by_label("撤销").click();
        harness.run();
        assert_eq!(
            harness
                .state()
                .1
                .doc
                .as_ref()
                .unwrap()
                .snapshot()
                .bytes()
                .unwrap(),
            before
        );
        harness.get_by_label("取消撤销").click();
        harness.run();
        assert_eq!(harness.state().0.name.as_deref(), Some("中文地图"));
        harness.get_by_role_and_label(input().0, input().1).focus();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness
            .get_by_role_and_label(input().0, input().1)
            .type_text("  保存前修改  ");
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert!(!harness.state().0.dirty());
        let doc = harness.state().1.doc.as_ref().unwrap();
        assert_eq!(doc.ini_text("Basic", "Name"), "保存前修改");
        let (source, _) = ra2_map_document::source::SourceMap::parse(
            "map.map".into(),
            "test".into(),
            doc.snapshot().bytes().unwrap(),
        )
        .unwrap();
        assert_eq!(source.title, "保存前修改");
        harness.get_by_role_and_label(input().0, input().1).focus();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        harness.key_press(egui::Key::Backspace);
        harness.run();
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
        harness.run();
        assert!(harness.state().0.dirty());
        assert_eq!(
            harness
                .state()
                .1
                .doc
                .as_ref()
                .unwrap()
                .ini_text("Basic", "Name"),
            "保存前修改"
        );
        assert!(harness.query_by_label("取消本次输入").is_none());
        harness.get_by_label("Close window").click();
        harness.run();
        assert!(!harness.state().0.open);
        assert!(!harness.state().0.dirty());
        assert_eq!(harness.state().0.name.as_deref(), Some("保存前修改"));
        drop(harness);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn crate_settings_finish_for_save_and_protect_invalid_or_stale_drafts() {
        let (mut editor, ctx, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let mut form = MapSettingsUi {
            open: true,
            tab: 2,
            ..Default::default()
        };
        form.load(editor.doc.as_ref().unwrap());
        let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        form.crates
            .as_mut()
            .unwrap()
            .values
            .insert("CrateRegen".into(), "0.5".into());
        form.crates_editing = true;
        let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
            ui.text_edit_singleline(
                form.crates
                    .as_mut()
                    .unwrap()
                    .values
                    .get_mut("CrateRegen")
                    .unwrap(),
            )
            .request_focus();
            form.completion.observe(ui.ctx());
            form.finish_edits(ui.ctx(), &mut editor, &scene);
            assert!(form.dirty(), "typing stays a draft");
            form.finish_for_save(ui.ctx(), &mut editor, &scene);
        });
        assert!(!form.dirty());
        assert_eq!(
            editor
                .doc
                .as_ref()
                .unwrap()
                .ini_text("CrateRules", "CrateRegen"),
            "0.5"
        );
        let after = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        editor.history(false, &scene);
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            before
        );
        editor.history(true, &scene);
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            after
        );

        form.load(editor.doc.as_ref().unwrap());
        form.crates
            .as_mut()
            .unwrap()
            .values
            .insert("CrateRegen".into(), "NaN".into());
        form.crates_editing = true;
        form.finish_for_save(&ctx, &mut editor, &scene);
        assert!(!form.allow_leave());
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            after
        );
        assert_eq!(form.crates.as_ref().unwrap().values["CrateRegen"], "NaN");
        form.crates
            .as_mut()
            .unwrap()
            .values
            .insert("CrateRegen".into(), "1".into());
        editor
            .doc
            .as_mut()
            .unwrap()
            .set_ini("CrateRules", "CrateRegen", Some(b"2".to_vec()))
            .unwrap();
        editor.commit(&scene, "external edit");
        form.finish_for_save(&ctx, &mut editor, &scene);
        assert!(form.message.contains("文档已变化"));
        assert_eq!(
            editor
                .doc
                .as_ref()
                .unwrap()
                .ini_text("CrateRules", "CrateRegen"),
            "2"
        );
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn crate_panel_typing_close_reset_and_undo_use_real_widgets() {
        use egui_kittest::kittest::Queryable as _;
        let (mut editor, _ctx, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let form = MapSettingsUi {
            open: true,
            tab: 2,
            ..Default::default()
        };
        let mut harness = egui_kittest::Harness::<(MapSettingsUi, Editor)>::builder()
            .with_context_setup(|ctx| {
                crate::ui_theme::install(ctx);
                ctx.add_font_provider(std::sync::Arc::new(
                    egui_system_font_provider::SystemFontProvider::new(),
                ));
            })
            .with_size([1000., 850.])
            .build_ui_state(
                |ui, (form, editor)| form.show(ui.ctx(), editor, &scene, false),
                (form, editor),
            );
        harness
            .get_by_role_and_label(egui::accesskit::Role::TextInput, "刷新间隔（游戏分钟）")
            .focus();
        harness.run();
        harness
            .get_by_role_and_label(egui::accesskit::Role::TextInput, "刷新间隔（游戏分钟）")
            .type_text("0.5");
        harness.run();
        assert!(harness.state().0.dirty());
        assert!(
            harness
                .state()
                .1
                .doc
                .as_ref()
                .unwrap()
                .ini_text("CrateRules", "CrateRegen")
                .is_empty()
        );
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert!(!harness.state().0.dirty());
        assert_eq!(
            harness
                .state()
                .1
                .doc
                .as_ref()
                .unwrap()
                .ini_text("CrateRules", "CrateRegen"),
            "0.5"
        );
        harness
            .get_by_role_and_label(egui::accesskit::Role::TextInput, "最少箱子数")
            .focus();
        harness.run();
        harness
            .get_by_role_and_label(egui::accesskit::Role::TextInput, "最少箱子数")
            .type_text("-1");
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert!(harness.state().0.dirty());
        assert!(harness.query_by_label("取消本次输入").is_none());
        harness.get_by_label("Close window").click();
        harness.run();
        assert!(!harness.state().0.open);
        assert!(!harness.state().0.dirty());
        assert!(
            harness
                .state()
                .1
                .doc
                .as_ref()
                .unwrap()
                .ini_text("CrateRules", "CrateMinimum")
                .is_empty()
        );
        harness.state_mut().0.open = true;
        harness.run();
        harness
            .get_by_role_and_label(egui::accesskit::Role::TextInput, "最少箱子数")
            .focus();
        harness.run();
        harness
            .get_by_role_and_label(egui::accesskit::Role::TextInput, "最少箱子数")
            .type_text("4");
        harness.run();
        assert!(harness.state().0.dirty());
        harness.get_by_label("Close window").click();
        harness.run();
        assert!(!harness.state().0.open);
        assert!(!harness.state().0.dirty());
        assert_eq!(
            harness
                .state()
                .1
                .doc
                .as_ref()
                .unwrap()
                .ini_text("CrateRules", "CrateMinimum"),
            "4"
        );
        harness.state_mut().0.open = true;
        harness.run();
        harness.get_by_label("全部继承游戏 / Mod 规则").click();
        harness.run();
        assert!(!harness.state().0.dirty());
        assert!(
            harness
                .state()
                .1
                .doc
                .as_ref()
                .unwrap()
                .ini_text("CrateRules", "CrateRegen")
                .is_empty()
        );
        harness.get_by_label("撤销").click();
        harness.run();
        assert_eq!(
            harness
                .state()
                .1
                .doc
                .as_ref()
                .unwrap()
                .ini_text("CrateRules", "CrateRegen"),
            "0.5"
        );
        drop(harness);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn save_finishes_focused_lighting_but_preserves_invalid_input_and_resize_plans() {
        let (mut editor, ctx, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let mut form = MapSettingsUi {
            open: true,
            tab: 1,
            ..Default::default()
        };
        form.load(editor.doc.as_ref().unwrap());
        form.light_editing = true;
        form.light
            .as_mut()
            .unwrap()
            .values
            .insert("Ambient".into(), "0.65".into());
        let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
            ui.text_edit_singleline(
                form.light
                    .as_mut()
                    .unwrap()
                    .values
                    .get_mut("Ambient")
                    .unwrap(),
            )
            .request_focus();
            form.completion.observe(ui.ctx());
            form.finish_edits(ui.ctx(), &mut editor, &scene);
            assert!(form.dirty());
            form.finish_for_save(ui.ctx(), &mut editor, &scene);
        });
        assert!(!form.dirty());
        assert_eq!(
            editor.doc.as_ref().unwrap().ini_text("Lighting", "Ambient"),
            "0.65"
        );
        let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        form.light_editing = true;
        form.light
            .as_mut()
            .unwrap()
            .values
            .insert("Ambient".into(), "NaN".into());
        form.finish_for_save(&ctx, &mut editor, &scene);
        assert!(!form.allow_leave());
        assert_eq!(form.light.as_ref().unwrap().values["Ambient"], "NaN");
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            before
        );
        form.light
            .as_mut()
            .unwrap()
            .values
            .insert("Ambient".into(), "0.7".into());
        form.options.as_mut().unwrap().width += 1;
        form.finish_for_save(&ctx, &mut editor, &scene);
        assert!(!form.allow_leave());
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            before
        );
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn completed_lighting_is_undoable_and_invalid_or_stale_input_cannot_write() {
        let (mut editor, ctx, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let mut form = MapSettingsUi {
            open: true,
            tab: 1,
            ..Default::default()
        };
        form.load(editor.doc.as_ref().unwrap());
        let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        form.light
            .as_mut()
            .unwrap()
            .values
            .insert("Ambient".into(), "0.65".into());
        form.light_editing = true;
        let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
            form.finish_edits(ui.ctx(), &mut editor, &scene);
        });
        assert!(!form.dirty());
        assert_eq!(
            editor.doc.as_ref().unwrap().ini_text("Lighting", "Ambient"),
            "0.65"
        );
        let after = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        editor.history(false, &scene);
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            before
        );
        editor.history(true, &scene);
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            after
        );

        form.load(editor.doc.as_ref().unwrap());
        form.light
            .as_mut()
            .unwrap()
            .values
            .insert("Ambient".into(), "NaN".into());
        form.light_editing = true;
        let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
            form.finish_edits(ui.ctx(), &mut editor, &scene);
        });
        assert!(form.dirty());
        assert!(form.message.starts_with("尚未生效"));
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            after
        );

        form.light
            .as_mut()
            .unwrap()
            .values
            .insert("Ambient".into(), "0.7".into());
        editor
            .doc
            .as_mut()
            .unwrap()
            .set_ini("Lighting", "Ambient", Some(b"0.8".to_vec()))
            .unwrap();
        editor.commit(&scene, "external edit");
        let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
            form.finish_edits(ui.ctx(), &mut editor, &scene);
        });
        assert!(form.message.contains("文档已变化"));
        assert_eq!(
            editor.doc.as_ref().unwrap().ini_text("Lighting", "Ambient"),
            "0.8"
        );
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lighting_preview_and_pending_resize_do_not_auto_write() {
        let (mut editor, ctx, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let mut form = MapSettingsUi::default();
        form.smoke_preview(editor.doc.as_ref().unwrap(), Profile::Ion);
        let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
            form.finish_edits(ui.ctx(), &mut editor, &scene);
        });
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            before
        );
        form.light_editing = true;
        form.options.as_mut().unwrap().width += 1;
        let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
            form.finish_edits(ui.ctx(), &mut editor, &scene);
        });
        assert!(form.dirty());
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            before
        );
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }
}
