//! Map information, geometry planning, automatic lighting/crate edits and read-only previews.
use crate::i18n::tr;
use crate::{editing::Editor, map::MapScene};
use eframe::egui;
use ra2_map_document::{
    document::{MapDocument, Snapshot},
    lighting::{COMPONENTS, Draft, Profile},
    map_settings::{self, Footprints, Options, Plan},
    new_map::{MAX_MAP_DIMENSION, MAX_MAP_SPAN, MIN_MAP_DIMENSION},
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
    name: Option<String>,
    original_name: Option<String>,
    author: String,
    original_author: String,
    players: u8,
    original_players: u8,
    pub profile: Profile,
    preview: bool,
    due: Option<Instant>,
    plan: Option<Plan>,
    worker: Option<std::sync::mpsc::Receiver<anyhow::Result<Plan>>>,
    message: String,
    completion: crate::form_edit::EditCompletion,
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
            || self.name != self.original_name
            || self.author != self.original_author
            || self.players != self.original_players
    }
    pub fn allow_leave(&mut self) -> bool {
        if self.dirty() {
            self.open = true;
            self.message =
                tr!("请完成或取消地图信息、光照和箱子设置输入；尺寸修改需检查预览并应用，或放弃尺寸修改。").into();
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
        self.name = Some(doc.ini_text("Basic", "Name"));
        self.original_name = self.name.clone();
        self.author = doc.ini_text("Basic", "Author");
        self.original_author = self.author.clone();
        self.players = ra2_map_document::player_starts::from_document(doc)
            .len()
            .clamp(2, 30) as u8;
        self.original_players = self.players;
        self.plan = None;
        self.worker = None;
        self.completion = Default::default();
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
        save: bool,
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
            && (save || ctx.input(|i| i.viewport().close_requested()) || self.completion.ready(ctx))
        {
            let result = (|| -> anyhow::Result<()> {
                let doc = editor
                    .doc
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!(tr!("没有地图")))?;
                anyhow::ensure!(
                    self.identity == Some((doc.id, doc.state_id)),
                    tr!("文档已变化，请取消本次输入后重新编辑")
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
            && (save || ctx.input(|i| i.viewport().close_requested()) || self.completion.ready(ctx))
        {
            let result = (|| -> anyhow::Result<()> {
                let doc = editor
                    .doc
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!(tr!("没有地图")))?;
                anyhow::ensure!(
                    self.identity == Some((doc.id, doc.state_id)),
                    tr!("文档已变化，请取消本次输入后重新编辑")
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
            && (save || ctx.input(|i| i.viewport().close_requested()) || self.completion.ready(ctx))
        {
            let result = (|| -> anyhow::Result<()> {
                let doc = editor
                    .doc
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!(tr!("没有地图")))?;
                anyhow::ensure!(
                    self.identity == Some((doc.id, doc.state_id)),
                    tr!("文档已变化，请取消本次输入后重新编辑")
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
    pub fn show(&mut self, ctx: &egui::Context, editor: &mut Editor, scene: &MapScene) {
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
        self.finish_edits(ctx, editor, scene);
        let doc = editor.doc.as_ref().unwrap();
        let light_before = self.light.clone();
        let crates_before = self.crates.clone();
        let name_before = self.name.clone();
        let author_before = self.author.clone();
        let players_before = self.players;
        let info_before = self.info.clone();
        if let Some(rx) = &self.worker {
            match rx.try_recv() {
                Ok(result) => {
                    self.worker = None;
                    match result {
                        Ok(p) if Some(&p.options) == self.options.as_ref() => {
                            self.plan = Some(p);
                            self.message = tr!("迁移预览完成，未改写地图。").into();
                        }
                        Ok(_) => self.message = tr!("设置已改变，请重新检查迁移。").into(),
                        Err(e) => self.message = format!("{e:#}"),
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.worker = None;
                    self.message = tr!("迁移检查线程意外结束。").into();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        let old_tab = self.tab;
        let mut open = true;
        let mut apply_map = false;
        let mut discard = false;
        let mut changed = false;
        let mut inspect = false;
        let mut undo = false;
        let mut redo = false;
        crate::ui_theme::window(ctx,tr!("地图设置"),"map-properties",egui::vec2(750.,690.)).open(&mut open).show(ctx,|ui|{
            ui.horizontal_wrapped(|ui|{
                ui.add_enabled_ui(!self.dirty(),|ui|{
                    changed|=ui.selectable_value(&mut self.tab,3,tr!("地图信息")).changed();
                    changed|=ui.selectable_value(&mut self.tab,0,tr!("尺寸与可玩区域")).changed();
                    changed|=ui.selectable_value(&mut self.tab,1,tr!("光照")).changed();
                    changed|=ui.selectable_value(&mut self.tab,2,tr!("箱子设置")).changed();
                });
                if self.dirty(){ui.colored_label(egui::Color32::YELLOW,match self.tab{1=>tr!("光照输入待完成"),2=>tr!("箱子设置输入待完成"),3=>tr!("地图信息输入待完成"),_=>tr!("有未应用尺寸设置")});}
            });
            egui::ScrollArea::vertical().max_height((ui.available_height()-100.).max(140.)).show(ui,|ui|{
                if self.tab==0 {
                    let options=self.options.as_mut().unwrap();
                    ui.label(tr!("当前尺寸：{arg0} × {arg1}；宽＋高 ≤ {MAX_MAP_SPAN}。平移以投影平面左/上为单位。", arg0 = doc.base.header.width, arg1 = doc.base.header.height, MAX_MAP_SPAN = MAX_MAP_SPAN));
                    egui::Grid::new("map-dimensions").num_columns(2).show(ui,|ui|{
                        for (name,value) in [(tr!("新宽度"),&mut options.width),(tr!("新高度"),&mut options.height)]{ui.label(name);changed|=crate::form_edit::number(ui,egui::DragValue::new(value).range(MIN_MAP_DIMENSION..=MAX_MAP_DIMENSION),name).changed();ui.end_row();}
                        ui.label(tr!("原图放置偏移（左、上）"));ui.horizontal(|ui|{for (value,name) in options.offset.iter_mut().zip([tr!("左侧偏移"),tr!("顶部偏移")]){changed|=crate::form_edit::number(ui,egui::DragValue::new(value).range(-i32::from(MAX_MAP_SPAN)..=i32::from(MAX_MAP_SPAN)),name).changed();}});ui.end_row();
                        ui.label(tr!("扩展区域清地高度"));changed|=crate::form_edit::number(ui,egui::DragValue::new(&mut options.fill_level).range(0..=127),tr!("扩展区域清地高度")).changed();ui.end_row();
                        ui.label(tr!("可玩区域（左、上、宽、高）")).on_hover_text("LocalSize");ui.horizontal_wrapped(|ui|{for (value,name) in options.local.iter_mut().zip([tr!("可玩区域左侧"),tr!("可玩区域顶部"),tr!("可玩区域宽度"),tr!("可玩区域高度")]){changed|=crate::form_edit::number(ui,egui::DragValue::new(value).range(0..=MAX_MAP_DIMENSION),name).changed();}});ui.end_row();
                    });
                    ui.horizontal_wrapped(|ui|{
                        if ui.button(tr!("可玩区域铺满有效边距")).clicked(){options.local=[2,4,i32::from(options.width)-4,i32::from(options.height)-10];changed=true;}
                        if ui.button(tr!("居中放置原图")).clicked(){options.offset=[(i32::from(options.width)-doc.base.header.width as i32)/2,(i32::from(options.height)-doc.base.header.height as i32)/2];changed=true;}
                    });
                    changed|=ui.checkbox(&mut options.delete_outside,tr!("应用时删除预览中允许删除的越界内容")).changed();
                    ui.small(tr!("裁切可删除越界对象、覆盖物、格子标签、基地节点和整条隧道。越界航点、桥梁、脚本目的格和任务可视矩形须先调整；共享标签保留。建筑按当前美术配置中的占地检查。"));
                    if let Err(e)=options.validate(){ui.colored_label(egui::Color32::LIGHT_RED,e.to_string());}
                    inspect=ui.add_enabled(self.worker.is_none(),egui::Button::new(tr!("检查迁移与越界预览"))).clicked();
                    if self.worker.is_some(){ui.spinner();}
                    if let Some(plan)=&self.plan {
                        let s=&plan.summary;ui.label(tr!("地形新增 {arg0} / 裁去 {arg1}；移动对象 {arg2}、任务引用 {arg3}；越界 {arg4} 项", arg0 = s.terrain_added, arg1 = s.terrain_removed, arg2 = s.records_moved, arg3 = s.references_moved, arg4 = s.affected.len()));
                        draw_preview(ui,doc,&plan.options,&plan.summary);
                        for a in s.affected.iter().take(100){ui.colored_label(if a.blocking{egui::Color32::LIGHT_RED}else{egui::Color32::YELLOW},format!("{} {:?} · {}",a.source,a.cell,if a.blocking{tr!("请先调整")}else{tr!("将删除")}));}
                        if s.affected.len()>100{ui.label(tr!("其余项目见预览统计；裁切前请检查边界内容。"));}
                        apply_map=ui.add_enabled(plan.can_apply(),egui::Button::new(tr!("应用地图设置（整笔撤销）"))).clicked();
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
                }else{
                    crate_fields(ui,self.crates.as_mut().unwrap());
                }
            });
            ui.separator();
            ui.horizontal_wrapped(|ui|{
                let cancel=ui.button(if self.tab==0{tr!("放弃尺寸修改")}else{tr!("取消本次输入")});
                self.completion.cancel_button(&cancel);
                discard=cancel.clicked();
                undo=ui.add_enabled(!self.dirty(),egui::Button::new(tr!("撤销"))).clicked();
                redo=ui.add_enabled(!self.dirty(),egui::Button::new(tr!("重做"))).clicked();
            });
            ui.label(&self.message);
        });
        if changed {
            self.plan = None;
            if self.tab == 1 || old_tab == 1 {
                self.due = Some(Instant::now());
            }
        }
        if inspect {
            let footprints = footprints(scene, doc);
            let mut scratch = doc.fork();
            scratch.id = doc.id;
            scratch.state_id = doc.state_id;
            let options = self.options.clone().unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            self.worker = Some(rx);
            let ctx = ctx.clone();
            self.message = tr!("正在后台检查迁移…").into();
            self.plan = None;
            std::thread::spawn(move || {
                let _ = tx.send(map_settings::plan(&scratch, options, &footprints));
                ctx.request_repaint();
            });
        }
        if apply_map && let Some(plan) = &self.plan {
            match plan.apply(editor.doc.as_mut().unwrap()) {
                Ok(_) => {
                    editor.structural_changed(scene);
                    self.load(editor.doc.as_ref().unwrap());
                    self.message = tr!("已应用地图设置，可整笔撤销。").into();
                }
                Err(e) => self.message = format!("{e:#}"),
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
        self.finish_edits(ctx, editor, scene);
        if discard {
            self.load(editor.doc.as_ref().unwrap());
            editor.refresh = true;
            self.message.clear();
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
        if !open {
            if self.dirty() {
                self.message = tr!("请完成或取消当前输入；尺寸修改需应用或放弃后关闭。").into();
            } else {
                self.open = false;
                self.due = None;
                editor.refresh = true;
            }
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
fn draw_preview(
    ui: &mut egui::Ui,
    doc: &MapDocument,
    options: &Options,
    summary: &map_settings::Summary,
) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().min(600.), 180.),
        egui::Sense::hover(),
    );
    let p = ui.painter_at(rect);
    let scale = ((rect.width() - 20.) / f32::from(options.width))
        .min((rect.height() - 20.) / f32::from(options.height));
    let origin = rect.min + egui::vec2(10., 10.);
    let r = |left: f32, top: f32, w: f32, h: f32| {
        egui::Rect::from_min_size(
            origin + egui::vec2(left, top) * scale,
            egui::vec2(w, h) * scale,
        )
    };
    p.rect_filled(
        r(0., 0., f32::from(options.width), f32::from(options.height)),
        0,
        egui::Color32::from_gray(42),
    );
    p.rect_stroke(
        r(
            options.offset[0] as f32,
            options.offset[1] as f32,
            doc.base.header.width as f32,
            doc.base.header.height as f32,
        ),
        0,
        egui::Stroke::new(1.5, egui::Color32::LIGHT_BLUE),
        egui::StrokeKind::Inside,
    );
    let [l, t, w, h] = options.local;
    p.rect_stroke(
        r(l as f32, t as f32, w as f32, h as f32),
        0,
        egui::Stroke::new(2., egui::Color32::LIGHT_GREEN),
        egui::StrokeKind::Inside,
    );
    for a in &summary.affected {
        let x = (i32::from(a.cell[0]) - i32::from(a.cell[1]) + doc.base.header.width as i32) as f32
            / 2.
            + options.offset[0] as f32;
        let y = (i32::from(a.cell[0]) + i32::from(a.cell[1]) - doc.base.header.width as i32) as f32
            / 2.
            + options.offset[1] as f32;
        p.circle_filled(
            origin + egui::vec2(x, y) * scale,
            2.5,
            if a.blocking {
                egui::Color32::LIGHT_RED
            } else {
                egui::Color32::YELLOW
            },
        );
    }
    ui.small(tr!(
        "灰：新尺寸；蓝：原图放置范围；绿：可玩区域；红/黄：受影响内容（位置按原图平移后绘制）。"
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

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
                |ui, (form, editor)| form.show(ui.ctx(), editor, &scene),
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
            form.show(ui.ctx(), &mut editor, &scene)
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
            form.show(ui.ctx(), &mut editor, &scene)
        });
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            after
        );
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn start_count_input_commits_on_enter_and_save_and_supports_history() {
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
                    form.show(ui.ctx(), editor, &scene);
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
        harness.get_by_label("重做").click();
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
        drop(harness);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn map_name_input_commits_on_enter_and_save_and_supports_history_and_cancel() {
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
                    form.show(ui.ctx(), editor, &scene);
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
        harness.get_by_label("重做").click();
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
        harness.get_by_label("取消本次输入").click();
        harness.run();
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
    fn crate_panel_typing_cancel_reset_and_undo_use_real_widgets() {
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
                |ui, (form, editor)| form.show(ui.ctx(), editor, &scene),
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
        harness.get_by_label("取消本次输入").click();
        harness.run();
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
        harness
            .get_by_role_and_label(egui::accesskit::Role::TextInput, "最少箱子数")
            .focus();
        harness.run();
        harness
            .get_by_role_and_label(egui::accesskit::Role::TextInput, "最少箱子数")
            .type_text("4");
        harness.run();
        harness.get_by_label("取消本次输入").click();
        harness.run();
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
