//! Mission forms are drafts until Apply; commands use the document's shared history.
use crate::{editing::Editor, map::MapScene};
use eframe::egui;
use ra2_map_document::{
    document::{Cell, Value},
    mission::{self, Kind},
};
use std::collections::BTreeMap;
use std::sync::Arc;

#[path = "ai_trigger_forms.rs"]
mod ai_forms;
#[path = "mission_authoring_smoke.rs"]
mod authoring_smoke;
#[path = "mission_forms.rs"]
mod forms;

#[derive(Default)]
struct AuditCache {
    key: Option<(u64, u64)>,
    definitions: Option<Arc<mission::ExternalDefinitions>>,
    report: Arc<AuditReport>,
}
#[derive(Default)]
struct AuditReport {
    references: Vec<mission::Reference>,
    diagnostics: Vec<mission::Diagnostic>,
}
impl AuditCache {
    fn get(
        &mut self,
        doc: &ra2_map_document::document::MapDocument,
        definitions: &Arc<mission::ExternalDefinitions>,
    ) -> Arc<AuditReport> {
        if self.key != Some((doc.id, doc.state_id))
            || self
                .definitions
                .as_ref()
                .is_none_or(|old| !Arc::ptr_eq(old, definitions))
        {
            self.report = Arc::new(AuditReport {
                references: mission::references(doc),
                diagnostics: mission::diagnostics(doc, Some(definitions)),
            });
            self.key = Some((doc.id, doc.state_id));
            self.definitions = Some(definitions.clone());
        }
        self.report.clone()
    }
}

#[derive(Default)]
pub(crate) struct MissionUi {
    pub(crate) campaign: crate::campaign_ui::CampaignUi,
    pub(crate) base: crate::base_nodes_ui::BaseNodesUi,
    pub open: bool,
    kind: Kind,
    selected: String,
    document: u64,
    state: u64,
    original: BTreeMap<String, Vec<u8>>,
    draft: BTreeMap<String, String>,
    new_id: String,
    new_key: String,
    filter: String,
    message: String,
    audit: AuditCache,
    field_target: Option<String>,
    pick: Option<forms::Pick>,
    rename_id: String,
    clone_dependencies: bool,
}
impl MissionUi {
    pub(crate) fn has_draft(&self) -> bool {
        self.dirty()
    }
    pub(crate) fn allow_leave(&mut self) -> bool {
        if !self.campaign.allow_leave() {
            return false;
        }
        if !self.dirty() {
            return true;
        }
        self.open = true;
        self.message =
            "任务草稿尚未应用。请先应用修改，或点击“放弃草稿并重载”，再打开、新建或关闭地图。"
                .into();
        false
    }
    pub(crate) fn navigate(&mut self, editor: &Editor, kind: Kind, id: String) {
        if self.dirty() {
            self.message = "任务草稿尚未应用，请先应用或重新载入。".into();
            self.open = true;
            return;
        }
        self.open = true;
        self.kind = kind;
        self.selected = id;
        self.message.clear();
        self.load(editor);
    }

    pub(crate) fn select_trigger(&mut self, editor: &Editor, id: String) {
        self.open = true;
        self.kind = Kind::Trigger;
        self.selected = id;
        self.load(editor);
    }

    fn load(&mut self, editor: &Editor) {
        let Some(doc) = &editor.doc else { return };
        self.document = doc.id;
        self.state = doc.state_id;
        self.original = mission::fields(doc, self.kind, &self.selected)
            .into_iter()
            .collect();
        self.draft = self
            .original
            .iter()
            .map(|(k, v)| (k.clone(), String::from_utf8_lossy(v).into_owned()))
            .collect();
        self.new_id = mission::next_id(doc, self.kind);
        self.field_target = None;
        self.pick = None;
        self.rename_id = mission::next_id(doc, self.kind);
    }
    fn dirty(&self) -> bool {
        self.original.keys().any(|k| !self.draft.contains_key(k))
            || self.draft.iter().any(|(k, v)| {
                self.original
                    .get(k)
                    .is_none_or(|b| String::from_utf8_lossy(b) != v.as_str())
            })
    }
    pub(crate) fn picking(&self) -> bool {
        self.open && (self.pick.is_some() || self.base.active)
    }
    pub(crate) fn pick_canvas(&mut self, editor: &Editor, at: Cell, marker: Option<u64>) {
        let Some(pick) = self.pick.clone() else {
            return;
        };
        let Some(doc) = &editor.doc else { return };
        if (doc.id, doc.state_id) != (self.document, self.state) {
            self.message = "文档已变化，请重新载入后拾取。".into();
            self.pick = None;
            return;
        }
        let result = (|| -> anyhow::Result<String> {
            match pick.kind {
                mission::schema::ParameterKind::Waypoint { letters } => {
                    let records = doc.records();
                    let candidates: Vec<_> = records
                        .values()
                        .filter(|r| r.section == "Waypoints" && r.cell() == Some(at))
                        .collect();
                    let r = marker
                        .and_then(|id| records.get(&id))
                        .filter(|r| r.section == "Waypoints")
                        .or_else(|| {
                            if candidates.len() == 1 {
                                Some(candidates[0])
                            } else {
                                None
                            }
                        })
                        .ok_or_else(|| anyhow::anyhow!("请点击唯一航点或航点标记"))?;
                    Ok(if letters {
                        mission::schema::waypoint_token(r.key.parse()?)?
                    } else {
                        r.key.clone()
                    })
                }
                mission::schema::ParameterKind::Type(section) => {
                    let records = doc.records();
                    let objects: Vec<_> = records
                        .values()
                        .filter(|r| r.section != "Waypoints" && r.cell() == Some(at))
                        .collect();
                    let r = marker
                        .and_then(|id| records.get(&id))
                        .filter(|r| r.section != "Waypoints")
                        .or_else(|| {
                            if objects.len() == 1 {
                                Some(objects[0])
                            } else {
                                None
                            }
                        })
                        .ok_or_else(|| anyhow::anyhow!("请点击唯一对象或对象标记"))?;
                    anyhow::ensure!(
                        section != "Foot"
                            || matches!(r.section.as_str(), "Infantry" | "Units" | "Aircraft"),
                        "特遣部队成员须为步兵、车辆或飞机"
                    );
                    Ok(r.type_id())
                }
                mission::schema::ParameterKind::Cell128 => {
                    anyhow::ensure!(at[0] < 128, "旧格子参数横坐标须小于 128");
                    Ok((u32::from(at[1]) * 128 + u32::from(at[0])).to_string())
                }
                _ => anyhow::bail!("此参数不支持画布拾取"),
            }
        })();
        match result {
            Ok(token) => {
                if let Some(value) = self.draft.get_mut(&pick.key) {
                    if let Some(i) = pick.row {
                        if let Ok(mut rows) = mission::decode_list(value, pick.key == "Events")
                            && let Some(field) = rows.get_mut(i).and_then(|r| r.get_mut(pick.index))
                        {
                            *field = token;
                            *value = mission::encode_list(&rows);
                        }
                    } else if pick.key == "Waypoint" {
                        *value = token;
                    } else {
                        let mut fields = mission::csv(value);
                        if let Some(f) = fields.get_mut(pick.index) {
                            *f = token;
                            *value = fields.join(",");
                        }
                    }
                }
                self.pick = None;
                self.message = "已更新草稿参数，请应用修改。".into();
            }
            Err(e) => self.message = format!("{e:#}"),
        }
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        editor: &mut Editor,
        scene: &MapScene,
        cell: Option<Cell>,
        directory: &std::path::Path,
    ) -> Option<Cell> {
        self.campaign.show(ctx, editor, directory);
        if !self.open {
            return None;
        }
        let Some(doc) = &editor.doc else {
            self.open = false;
            return None;
        };
        if self.document != doc.id {
            self.base.stop();
            self.kind = Kind::Basic;
            self.selected = "Basic".into();
            self.load(editor);
        }
        if self.state != doc.state_id && !self.dirty() {
            self.load(editor);
        }
        if self.base.active {
            self.base.panel(ctx, editor, scene);
            return None;
        }
        if self.pick.is_some() {
            crate::ui_theme::window(ctx, "任务参数拾取", "mission-pick", egui::vec2(460., 180.))
                .show(ctx, |ui| {
                    ui.label("在地图点击航点标记、对象或目标格；Esc 取消。草稿不会立即写入文档。");
                    if ui.button("取消拾取").clicked()
                        || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        self.pick = None;
                    }
                });
            return None;
        }
        let mut open = true;
        let mut jump = None;
        crate::ui_theme::window(ctx, "任务制作", "mission-authoring", egui::vec2(1000., 690.))
            .open(&mut open).resizable(true)
            .show(ctx,|ui| {
                ui.horizontal_wrapped(|ui| {
                    for kind in Kind::ALL {if ui.add_enabled(!self.dirty(),egui::Button::new(kind.label()).selected(self.kind==kind)).clicked(){self.kind=kind;self.selected=mission::ids(editor.doc.as_ref().unwrap(),kind).first().cloned().unwrap_or_default();self.message.clear();self.load(editor);}}
                });
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(!editor.stroking&&!self.dirty(),egui::Button::new("添加最小任务示例")).on_hover_text("将当前地图设置为单人任务，添加两个所属方、增援小队、局部变量和触发器：2 秒后增援并置位 → 宣告胜利。保留原有内容，可整笔撤销。").clicked(){
                        let result=mission::seed(editor.doc.as_mut().unwrap());
                        match result {Ok(id)=>{editor.commit(scene,"创建最小任务示例");self.kind=Kind::Trigger;self.selected=id;self.message="已添加：经过时间 → 小队增援 / 设置局部变量 → 宣告胜利。可修改事件时间、动作和脚本后试玩。".into();self.load(editor);},Err(e)=>self.message=format!("{e:#}")}
                    }
                    if ui.add_enabled(!self.dirty()&&!editor.stroking,egui::Button::new("撤销")).clicked(){editor.history(false,scene);self.load(editor);}
                    if ui.add_enabled(!self.dirty()&&!editor.stroking,egui::Button::new("重做")).clicked(){editor.history(true,scene);self.load(editor);}
                    if ui.button("任务试玩…").clicked(){editor.play_options.mission=true;self.message="已切换为单人任务试玩；关闭此窗后点击顶栏“试玩”配置难度并启动。".into();}
                    if ui.add_enabled(!self.dirty(),egui::Button::new("战役与发布…")).clicked(){self.campaign.open=true;}
                });
                if !self.message.is_empty(){ui.label(&self.message);}
                ui.separator();
                egui::ScrollArea::vertical().id_salt("mission-body").auto_shrink([false, false]).max_height((ui.available_height()-45.).max(120.)).show(ui,|ui| {
                    if ui.available_width() < 900. {
                        egui::CollapsingHeader::new(format!("记录列表 · {}", self.selected))
                            .id_salt("mission-record-list").default_open(self.selected.is_empty())
                            .show(ui, |ui| self.record_list(ui, editor, scene, cell));
                        ui.separator();
                        self.record_form(ui, editor, scene, cell);
                    } else {
                        let width = (ui.available_width() * 0.25).clamp(190., 250.);
                        ui.horizontal_top(|ui| {
                            ui.allocate_ui_with_layout(egui::vec2(width, 0.), egui::Layout::top_down(egui::Align::Min), |ui| self.record_list(ui, editor, scene, cell));
                            ui.separator();
                            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 0.), egui::Layout::top_down(egui::Align::Min), |ui| self.record_form(ui, editor, scene, cell));
                        });
                    }
                    ui.separator();ui.label("引用导航（点击跳转；对象/格子引用可定位地图）");
                    let report=self.audit.get(editor.doc.as_ref().unwrap(), &scene.mission_definitions);
                    let current=(self.kind,self.selected.clone());
                    for r in report.references.iter().filter(|r|r.from==current||r.to==current) {
                        let target=if r.from==(self.kind,self.selected.clone()){&r.to}else{&r.from};
                        let fixed = scene.mission_definitions.contains(target.0,&target.1) && !mission::ids(editor.doc.as_ref().unwrap(),target.0).iter().any(|id| mission::same_id(target.0,id,&target.1));
                        let source=if fixed {scene.mission_definitions.source.as_str()}else{"地图"};
                        if ui.add_enabled(!self.dirty(),egui::Link::new(format!("{} {} / {} → {} {} · {source}",r.from.0.label(),r.from.1,r.field,r.to.0.label(),r.to.1))).clicked(){if r.cell.is_some(){jump=r.cell;}else if fixed {self.message=format!("{} {} 来自 {source}，固定定义只读，可在地图新建同 ID 覆盖。",target.0.label(),target.1);}else{self.kind=target.0;self.selected=mission::ids(editor.doc.as_ref().unwrap(),target.0).into_iter().find(|id|mission::same_id(target.0,id,&target.1)).unwrap_or_else(||target.1.clone());self.load(editor);}}
                    }
                    ui.separator();egui::CollapsingHeader::new("引用与能力诊断").default_open(true).show(ui,|ui| {
                        let count=|severity|report.diagnostics.iter().filter(|d|d.severity==severity).count();
                        ui.label(format!("{} 项错误 · {} 项警告 · {} 项能力提示", count(mission::Severity::Error),count(mission::Severity::Warning),count(mission::Severity::Capability)));
                        for diagnostic in &report.diagnostics {
                            let issue=&diagnostic.issue;
                            let color=match diagnostic.severity {mission::Severity::Error=>egui::Color32::LIGHT_RED,mission::Severity::Warning=>egui::Color32::YELLOW,mission::Severity::Capability=>egui::Color32::LIGHT_BLUE};
                            if ui.add_enabled(!self.dirty(),egui::Link::new(egui::RichText::new(format!("[{}] {} {} / {}：{}",diagnostic.severity.label(),issue.kind.label(),issue.id,diagnostic.field,issue.message)).color(color))).clicked(){
                                if issue.id.starts_with('@') {jump=issue.cell;} else {self.kind=issue.kind;self.selected=issue.id.clone();self.load(editor);self.field_target=Some(diagnostic.field.clone());if issue.cell.is_some(){jump=issue.cell;}}
                            }
                        }
                    });
                });
                if !scene.mission_resources.errors.is_empty(){ui.collapsing("未读取的任务资源目录",|ui|{for error in &scene.mission_resources.errors{ui.small(error);}});}
                ui.small("能力提示依据固定的 VERA20k 源码；未知参数可保留/编辑原文，未列入清单不代表引擎拒绝。");
            });
        self.open = open;
        jump
    }
    fn record_list(
        &mut self,
        ui: &mut egui::Ui,
        editor: &mut Editor,
        scene: &MapScene,
        cell: Option<Cell>,
    ) {
        let doc = editor.doc.as_ref().unwrap();
        let ids = if self.kind == Kind::AiTrigger {
            scene
                .mission_definitions
                .candidates(doc, self.kind)
                .into_iter()
                .map(|(id, _)| id)
                .collect()
        } else {
            mission::ids(doc, self.kind)
        };
        crate::ui_theme::section(ui, "记录列表", "");
        ui.add(
            egui::TextEdit::singleline(&mut self.filter)
                .desired_width(f32::INFINITY)
                .hint_text("查找 ID…"),
        );
        for id in &ids {
            if !id.to_lowercase().contains(&self.filter.to_lowercase()) {
                continue;
            }
            if ui
                .add_enabled_ui(!self.dirty(), |ui| {
                    ui.add_sized(
                        [ui.available_width(), 28.],
                        egui::Button::new(id)
                            .truncate()
                            .selected(self.selected == *id),
                    )
                })
                .inner
                .on_hover_text(id)
                .clicked()
            {
                self.selected = id.clone();
                self.message.clear();
                self.load(editor);
            }
        }
        if !matches!(self.kind, Kind::Basic | Kind::Global) {
            ui.separator();
            ui.label(if self.kind == Kind::CellTag {
                "新标签使用当前选中格坐标"
            } else {
                "新建 ID"
            });
            ui.add(egui::TextEdit::singleline(&mut self.new_id).desired_width(f32::INFINITY));
            if ui
                .add_enabled(!editor.stroking && !self.dirty(), egui::Button::new("新建"))
                .clicked()
            {
                let doc = editor.doc.as_mut().unwrap();
                let owner = mission::ids(doc, Kind::House)
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "Americans".into());
                let result = mission::create(doc, self.kind, &self.new_id, &owner, cell);
                match result {
                    Ok(()) => {
                        editor.commit(scene, "新建任务记录");
                        self.selected = if self.kind == Kind::CellTag {
                            mission::cell_key(editor.doc.as_ref().unwrap(), cell.unwrap()).unwrap()
                        } else {
                            self.new_id.clone()
                        };
                        self.message.clear();
                        self.load(editor);
                    }
                    Err(e) => self.message = format!("{e:#}"),
                }
            }
        }
    }
    fn record_form(
        &mut self,
        ui: &mut egui::Ui,
        editor: &mut Editor,
        scene: &MapScene,
        cell: Option<Cell>,
    ) {
        ui.style_mut().spacing.text_edit_width = ui.available_width().min(220.);

        ui.heading(format!("{}  {}", self.kind.label(), self.selected));
        if self.selected.is_empty() {
            ui.label("选择或新建一条记录。");
            return;
        }
        if self.kind == Kind::AiTrigger
            && editor
                .doc
                .as_ref()
                .unwrap()
                .ini_value("AITriggerTypes", &self.selected)
                .is_none()
            && let Some((_, raw)) = scene
                .mission_definitions
                .ai_trigger_rows
                .iter()
                .find(|(id, _)| mission::same_id(Kind::AiTrigger, id, &self.selected))
        {
            ui.label(format!("固定定义 · {}", scene.mission_definitions.source));
            ui.collapsing("查看固定电脑触发器", |ui| {
                ui.label(raw);
            });
            if ui
                .add_enabled(
                    !self.dirty() && !editor.stroking,
                    egui::Button::new("复制固定定义为地图覆盖"),
                )
                .clicked()
            {
                let result = editor.doc.as_mut().unwrap().set_ini(
                    "AITriggerTypes",
                    &self.selected,
                    Some(raw.as_bytes().to_vec()),
                );
                match result {
                    Ok(()) => {
                        editor.commit(scene, "覆盖固定 AI 触发器");
                        self.load(editor);
                    }
                    Err(e) => self.message = format!("{e:#}"),
                }
            }
        }
        if self.kind == Kind::House {
            ui.small(
                "初始资金使用原生地图资金单位（每单位 100）；盟友列表为单向关系。ID 不自动改名。",
            );
            if ui
                .add_enabled(
                    !self.dirty() && !editor.stroking,
                    egui::Button::new("图形编辑基地节点…"),
                )
                .clicked()
            {
                self.base.start(self.selected.clone());
                editor.base_nodes_visible = true;
            }
        }
        if self.kind == Kind::Global {
            ui.label("固定编号 0～49；此处只保存编辑注释，不写运行时初值。新战役全部清零；续关继承，重试/重开恢复进入本关时的状态。全局 0 还控制下一关与分支关卡的选择。");
        }
        if self.kind == Kind::TaskForce {
            ui.small("0～5：数量,类型 ID；分组保留原生数值。");
        }
        if self.kind == Kind::Script {
            ui.small("0～49：动作 ID,整数参数。例如 3,0 表示移动到航点 0；未知动作按原值保留。");
        }
        if self.kind == Kind::Tag {
            ui.small("重复方式：0 一次、1 等待全部关联对象、2 重复；标签绑定触发器，格子和对象绑定标签。");
        }
        let stale = editor.doc.as_ref().unwrap().state_id != self.state;
        if stale {
            ui.colored_label(egui::Color32::YELLOW, "文档已变化，请重新载入草稿再应用。");
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !stale && !editor.stroking && self.dirty(),
                    crate::ui_theme::primary("应用修改"),
                )
                .clicked()
            {
                let changes = draft_changes(&self.original, &self.draft);
                match mission::write_fields(
                    editor.doc.as_mut().unwrap(),
                    self.kind,
                    &self.selected,
                    &changes,
                ) {
                    Ok(()) => {
                        editor.commit(scene, "修改任务字段");
                        self.message = "修改已应用，可撤销、保存或试玩。".into();
                        self.load(editor);
                    }
                    Err(e) => self.message = format!("{e:#}"),
                }
            }
            if ui
                .button(if self.dirty() {
                    "放弃草稿并重载"
                } else {
                    "重新载入"
                })
                .clicked()
            {
                self.load(editor);
            }
            if !matches!(self.kind, Kind::Basic | Kind::Global)
                && ui
                    .add_enabled(
                        !editor.stroking && !self.dirty(),
                        egui::Button::new("删除记录"),
                    )
                    .clicked()
            {
                match mission::delete(editor.doc.as_mut().unwrap(), self.kind, &self.selected) {
                    Ok(()) => {
                        editor.commit(scene, "删除任务记录");
                        self.selected = mission::ids(editor.doc.as_ref().unwrap(), self.kind)
                            .first()
                            .cloned()
                            .unwrap_or_default();
                        self.load(editor);
                        self.message.clear();
                    }
                    Err(e) => self.message = format!("{e:#}"),
                }
            }
        });
        if matches!(
            self.kind,
            Kind::Trigger
                | Kind::Tag
                | Kind::Team
                | Kind::Script
                | Kind::TaskForce
                | Kind::AiTrigger
        ) {
            ui.horizontal_wrapped(|ui| {
                ui.label("目标 ID");
                ui.text_edit_singleline(&mut self.rename_id);
                ui.checkbox(&mut self.clone_dependencies, "复制本地图依赖");
                if ui
                    .add_enabled(
                        !self.dirty() && !editor.stroking,
                        egui::Button::new("复制记录"),
                    )
                    .clicked()
                {
                    match mission::graph::copy(
                        editor.doc.as_mut().unwrap(),
                        self.kind,
                        &self.selected,
                        &self.rename_id,
                        self.clone_dependencies,
                    ) {
                        Ok(records) => {
                            editor.commit(scene, "复制任务图");
                            self.selected = self.rename_id.clone();
                            self.message = format!(
                                "已复制 {} 条定义；地图对象与格子仍绑定原任务图。",
                                records.len()
                            );
                            self.load(editor);
                        }
                        Err(e) => self.message = format!("{e:#}"),
                    }
                }
                if ui
                    .add_enabled(
                        !self.dirty() && !editor.stroking,
                        egui::Button::new("重命名并迁移引用"),
                    )
                    .clicked()
                {
                    match mission::graph::rename(
                        editor.doc.as_mut().unwrap(),
                        self.kind,
                        &self.selected,
                        &self.rename_id,
                    ) {
                        Ok(()) => {
                            editor.commit(scene, "重命名任务记录并迁移引用");
                            self.selected = self.rename_id.clone();
                            self.message = "已迁移跨表、格子与对象引用，可整笔撤销。".into();
                            self.load(editor);
                        }
                        Err(e) => self.message = format!("{e:#}"),
                    }
                }
            });
            ui.small("复制和重命名使用上方目标 ID。依赖包含地图内触发器、标签、小队、脚本和特遣部队；所属方、变量和固定任务定义共享。");
        }
        if matches!(self.kind, Kind::Script | Kind::TaskForce) {
            forms::slots(
                ui,
                self.kind,
                &mut self.draft,
                editor.doc.as_ref().unwrap(),
                scene,
                &mut self.pick,
                &mut self.message,
            );
        }
        let mut keys: Vec<_> = self.draft.keys().cloned().collect();
        if self.kind == Kind::Trigger {
            keys.sort_by_key(|key| match key.as_str() {
                "Triggers" => 0,
                "Events" => 1,
                _ => 2,
            });
        }
        for key in keys {
            if matches!(self.kind, Kind::Script | Kind::TaskForce) && key.parse::<usize>().is_ok() {
                continue;
            }
            let locate = self.field_target.as_ref() == Some(&key);
            let field = ui.push_id(&key, |ui| {
                if locate {
                    ui.colored_label(egui::Color32::YELLOW, format!("诊断字段：{key}"));
                }
                let value = self.draft.get_mut(&key).unwrap();
                if self.kind == Kind::Global {
                    ui.label("编辑注释");
                    ui.text_edit_singleline(value);
                } else if self.kind == Kind::Basic
                    && super::campaign_ui::basic_field(ui, &key, value, scene)
                {
                } else if self.kind == Kind::AiTrigger && key == "AITriggerTypesEnable" {
                    ai_forms::enable(ui, value);
                } else if self.kind == Kind::AiTrigger && key == "AITriggerTypes" {
                    ai_forms::form(ui, value, editor.doc.as_ref().unwrap(), scene);
                } else if self.kind == Kind::Trigger && matches!(key.as_str(), "Events" | "Actions")
                {
                    list_editor(
                        ui,
                        &key,
                        value,
                        editor.doc.as_ref().unwrap(),
                        scene,
                        &mut self.pick,
                    );
                } else if self.kind == Kind::Trigger && key == "Triggers" {
                    csv_form(
                        ui,
                        value,
                        &[
                            "House",
                            "链接 Trigger",
                            "名称",
                            "禁用 (0/1)",
                            "简单",
                            "普通",
                            "困难",
                            "占领转移",
                        ],
                        editor.doc.as_ref().unwrap(),
                        &scene.mission_definitions,
                    );
                } else if self.kind == Kind::Tag {
                    csv_form(
                        ui,
                        value,
                        &["重复方式", "名称", "Trigger"],
                        editor.doc.as_ref().unwrap(),
                        &scene.mission_definitions,
                    );
                } else {
                    if self.kind == Kind::Team && key == "Waypoint" {
                        let p = mission::schema::Parameter {
                            index: 0,
                            label: "起始航点",
                            kind: mission::schema::ParameterKind::Waypoint { letters: true },
                        };
                        if forms::parameter(ui, p, value, editor.doc.as_ref().unwrap(), scene) {
                            self.pick = Some(forms::Pick {
                                key: key.clone(),
                                row: None,
                                index: 0,
                                kind: p.kind,
                            });
                        }
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(crate::labels::field(&key)).on_hover_text(&key);
                            reference_input(
                                ui,
                                &key,
                                value,
                                editor.doc.as_ref().unwrap(),
                                &scene.mission_definitions,
                            );
                        });
                    }
                }
            });
            if locate {
                field.response.scroll_to_me(Some(egui::Align::Center));
                self.field_target = None;
            }
        }
        if self.kind.section_definition() {
            ui.horizontal_wrapped(|ui| {
                ui.text_edit_singleline(&mut self.new_key);
                if ui.button("添加字段").clicked() && !self.new_key.is_empty() {
                    self.draft.entry(self.new_key.clone()).or_default();
                    self.new_key.clear();
                }
            });
            ui.small("清空字段后应用可删除该字段；未改动字段和未知字节保持原样。");
        }
        if self.dirty() {
            ui.small("先应用修改或重新载入以放弃草稿，再切换记录。");
        }
        if self.kind == Kind::Tag {
            if let Some(at) = cell
                && ui
                    .add_enabled(
                        !self.dirty() && !editor.stroking,
                        egui::Button::new(format!("绑定当前格 {},{}", at[0], at[1])),
                    )
                    .on_disabled_hover_text("请先应用或放弃任务草稿，并完成当前一笔。")
                    .clicked()
            {
                match mission::bind_cell(editor.doc.as_mut().unwrap(), at, &self.selected) {
                    Ok(()) => {
                        editor.commit(scene, "绑定格子标签");
                        self.load(editor);
                    }
                    Err(e) => self.message = format!("{e:#}"),
                }
            }
            if let Some(id) = editor.selected
                && ui
                    .add_enabled(
                        !self.dirty() && !editor.stroking && !editor.object_properties_dirty(),
                        egui::Button::new("绑定当前选中对象"),
                    )
                    .on_disabled_hover_text(
                        "请先应用或放弃任务草稿与对象属性修改，并完成当前一笔。",
                    )
                    .clicked()
            {
                let doc = editor.doc.as_mut().unwrap();
                let result = (|| -> anyhow::Result<()> {
                    let r = doc
                        .records()
                        .get(&id)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("对象不存在"))?;
                    let col = match r.section.as_str() {
                        "Structures" => 6,
                        "Infantry" => 8,
                        "Units" | "Aircraft" => 7,
                        _ => anyhow::bail!("该对象不支持 Tag"),
                    };
                    doc.set(
                        crate::document::Target::Record(id),
                        Some(Value::Record(r.patched(&[(col, self.selected.clone())])?)),
                    )
                })();
                match result {
                    Ok(()) => {
                        editor.commit(scene, "绑定对象标签");
                        self.load(editor);
                    }
                    Err(e) => self.message = format!("{e:#}"),
                }
            }
        }
    }
}
fn reference_input(
    ui: &mut egui::Ui,
    key: &str,
    value: &mut String,
    doc: &ra2_map_document::document::MapDocument,
    definitions: &mission::ExternalDefinitions,
) {
    let kind = match key {
        "House" | "Player" => Some(Kind::House),
        "Script" => Some(Kind::Script),
        "TaskForce" => Some(Kind::TaskForce),
        "Trigger" | "链接 Trigger" => Some(Kind::Trigger),
        "Tag" => Some(Kind::Tag),
        _ => None,
    };
    let width = (ui.available_width() - if kind.is_some() { 84. } else { 0. }).clamp(70., 220.);
    ui.add(egui::TextEdit::singleline(value).desired_width(width));
    if let Some(kind) = kind {
        egui::ComboBox::from_id_salt(key)
            .selected_text("选择")
            .width(70.)
            .show_ui(ui, |ui| {
                for (id, external) in definitions.candidates(doc, kind) {
                    let label = if external {
                        format!("{id} · {}", definitions.source)
                    } else {
                        format!("{id} · 地图")
                    };
                    ui.selectable_value(value, id, label);
                }
            });
    }
}
fn csv_form(
    ui: &mut egui::Ui,
    value: &mut String,
    labels: &[&str],
    doc: &ra2_map_document::document::MapDocument,
    definitions: &mission::ExternalDefinitions,
) {
    let mut fields = mission::csv(value);
    let mut changed = false;
    for (i, label) in labels.iter().enumerate().take(fields.len()) {
        let before = fields[i].clone();
        ui.push_id(i, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add_sized(
                    [96., 28.],
                    egui::Label::new(crate::labels::field(label)).halign(egui::Align::Min),
                )
                .on_hover_text(*label);
                reference_input(ui, label, &mut fields[i], doc, definitions);
            });
        });
        changed |= before != fields[i];
    }
    if changed {
        *value = fields.join(",");
    }
    egui::CollapsingHeader::new("原始 CSV").show(ui, |ui| {
        ui.text_edit_singleline(value);
    });
}
fn list_editor(
    ui: &mut egui::Ui,
    key: &str,
    value: &mut String,
    doc: &ra2_map_document::document::MapDocument,
    scene: &MapScene,
    pick: &mut Option<forms::Pick>,
) {
    let event = key == "Events";
    ui.strong(if event {
        "事件列表"
    } else {
        "动作列表"
    });
    if let Ok(mut rows) = mission::decode_list(value, event) {
        let before = rows.clone();
        let mut remove = None;
        let mut movement = None;
        let mut copy = None;
        for (i, row) in rows.iter_mut().enumerate() {
            ui.push_id(i, |ui| {
                let specs = if event {
                    mission::schema::EVENTS
                } else {
                    mission::schema::ACTIONS
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("{}", i + 1));
                    egui::ComboBox::from_id_salt("opcode")
                        .selected_text(
                            specs
                                .iter()
                                .find(|s| s.id.to_string() == row[0])
                                .map_or_else(
                                    || format!("原始 ID {}", row[0]),
                                    |s| format!("{} {}", s.id, s.name),
                                ),
                        )
                        .show_ui(ui, |ui| {
                            for spec in specs {
                                if ui
                                    .selectable_label(
                                        row[0] == spec.id.to_string(),
                                        format!("{} {}", spec.id, spec.name),
                                    )
                                    .clicked()
                                {
                                    *row = mission::schema::row(event, spec.id).unwrap();
                                }
                            }
                        });
                    if i > 0 && ui.small_button("↑").clicked() {
                        movement = Some((i, i - 1));
                    }
                    if ui.small_button("↓").clicked() {
                        movement = Some((i, i + 1));
                    }
                    if ui.small_button("复制").clicked() {
                        copy = Some(i);
                    }
                    if ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                });
                if let Some(spec) = mission::schema::opcode(event, row[0].parse().unwrap_or(-1)) {
                    if row[1].parse::<i32>().ok() == Some(spec.parameter_type) {
                        ui.small(spec.hint);
                        for p in spec.parameters {
                            if let Some(v) = row.get_mut(p.index)
                                && forms::parameter(ui, *p, v, doc, scene)
                            {
                                *pick = Some(forms::Pick {
                                    key: key.into(),
                                    row: Some(i),
                                    index: p.index,
                                    kind: p.kind,
                                });
                            }
                        }
                    } else {
                        ui.small(
                            "参数类型与已核对 schema 不同，保留原文；重新选择动作会采用标准类型。",
                        );
                    }
                }
                egui::CollapsingHeader::new("全部原始字段").show(ui, |ui| {
                    for (col, v) in row.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(format!("字段 {col}"));
                            ui.text_edit_singleline(v);
                        });
                    }
                });
            });
        }
        if let Some(i) = remove {
            rows.remove(i);
        }
        if let Some((a, b)) = movement
            && a < rows.len()
            && b < rows.len()
        {
            rows.swap(a, b);
        }
        if let Some(i) = copy
            && rows.len() < 40
        {
            rows.insert(i + 1, rows[i].clone());
        }
        if ui
            .add_enabled(
                rows.len() < 40,
                egui::Button::new(if event { "＋事件" } else { "＋动作" }),
            )
            .clicked()
        {
            rows.push(mission::schema::row(event, if event { 13 } else { 67 }).unwrap());
        }
        if rows != before {
            *value = mission::encode_list(&rows);
        }
    } else {
        ui.small("存在扩展或无法识别的字段，保留原文。");
    }
    egui::CollapsingHeader::new("原文（含数量字段）").show(ui, |ui| {
        ui.text_edit_singleline(value);
    });
}
/// Include removed slots and recover legacy tokens moved between list keys.
fn draft_changes(
    original: &BTreeMap<String, Vec<u8>>,
    draft: &BTreeMap<String, String>,
) -> Vec<(String, Vec<u8>)> {
    let mut changes = Vec::new();
    for (key, bytes) in original {
        if !draft.contains_key(key) {
            changes.push((key.clone(), Vec::new()));
        } else if String::from_utf8_lossy(bytes) == draft[key] {
            continue;
        }
    }
    for (key, value) in draft {
        if original
            .get(key)
            .is_some_and(|b| String::from_utf8_lossy(b) == value.as_str())
        {
            continue;
        }
        let bytes = if key.parse::<usize>().is_ok() {
            let tokens: Vec<_> = original
                .iter()
                .filter(|(k, _)| k.parse::<usize>().is_ok())
                .flat_map(|(_, v)| v.split(|b| *b == b','))
                .collect();
            value
                .split(',')
                .map(|v| {
                    tokens
                        .iter()
                        .find(|b| String::from_utf8_lossy(b).trim() == v.trim())
                        .map_or_else(|| v.as_bytes().to_vec(), |b| b.to_vec())
                })
                .collect::<Vec<_>>()
                .join(&b',')
        } else {
            preserve_csv(original.get(key).map(Vec::as_slice), value)
        };
        changes.push((key.clone(), bytes));
    }
    changes
}
/// Editing a known CSV column must not transcode an untouched legacy-byte tail.
fn preserve_csv(original: Option<&[u8]>, draft: &str) -> Vec<u8> {
    let Some(original) = original else {
        return draft.as_bytes().to_vec();
    };
    let raw: Vec<_> = original.split(|b| *b == b',').collect();
    let edited: Vec<_> = draft.split(',').collect();
    if raw.len() != edited.len() {
        return edited
            .iter()
            .map(|b| {
                raw.iter()
                    .find(|a| String::from_utf8_lossy(a).trim() == b.trim())
                    .map_or_else(|| b.as_bytes().to_vec(), |a| a.to_vec())
            })
            .collect::<Vec<_>>()
            .join(&b',');
    }
    raw.iter()
        .zip(edited)
        .map(|(a, b)| {
            if String::from_utf8_lossy(a).trim() == b.trim() {
                a.to_vec()
            } else {
                raw.iter()
                    .find(|candidate| String::from_utf8_lossy(candidate).trim() == b.trim())
                    .map_or_else(|| b.as_bytes().to_vec(), |candidate| candidate.to_vec())
            }
        })
        .collect::<Vec<_>>()
        .join(&b',')
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn leaving_with_unapplied_mission_draft_preserves_and_reopens_it() {
        let mut ui = MissionUi::default();
        ui.draft.insert("Name".into(), "unfinished mission".into());
        assert!(!ui.allow_leave());
        assert!(ui.open);
        assert_eq!(ui.draft["Name"], "unfinished mission");
        ui.draft.clear();
        assert!(ui.allow_leave());
    }
    #[test]
    fn applying_removed_and_moved_list_slots_deletes_tail_and_preserves_bytes() {
        let original = BTreeMap::from([
            ("0".into(), b"1,\xD6\xD0".to_vec()),
            ("1".into(), b"2,E1".to_vec()),
            ("2".into(), b"3,E2".to_vec()),
        ]);
        let draft = BTreeMap::from([
            ("0".into(), "2,E1".into()),
            (
                "1".into(),
                String::from_utf8_lossy(&original["0"]).into_owned(),
            ),
        ]);
        let changes: BTreeMap<_, _> = draft_changes(&original, &draft).into_iter().collect();
        assert_eq!(changes["2"], Vec::<u8>::new());
        assert_eq!(changes["1"], b"1,\xD6\xD0");
        let form = MissionUi {
            original,
            draft,
            ..Default::default()
        };
        assert!(form.dirty());
    }
    #[test]
    fn reordered_and_duplicated_event_rows_keep_opaque_legacy_tokens() {
        let original = b"2,999,2,0,\xD6\xD0,13,0,30";
        let mut rows = mission::decode_list(&String::from_utf8_lossy(original), true).unwrap();
        rows.swap(0, 1);
        assert_eq!(
            preserve_csv(Some(original), &mission::encode_list(&rows)),
            b"2,13,0,30,999,2,0,\xD6\xD0"
        );
        rows.push(rows[1].clone());
        assert_eq!(
            preserve_csv(Some(original), &mission::encode_list(&rows)),
            b"3,13,0,30,999,2,0,\xD6\xD0,999,2,0,\xD6\xD0"
        );
    }
    #[test]
    fn diagnostics_cache_tracks_documents_history_and_installation_changes() {
        use ra2_map_document::{
            document::{Baseline, MapDocument},
            new_map::NewMapOptions,
        };
        let bytes = NewMapOptions::default().bytes().unwrap();
        let map = vera20k_core::map::map_file::load_from_bytes(bytes.clone()).unwrap();
        let base = Baseline::parse(bytes.into(), &map).unwrap();
        let mut doc = MapDocument::new(base.clone());
        let mut cache = AuditCache::default();
        let definitions = Arc::new(mission::ExternalDefinitions::default());
        let initial = cache.get(&doc, &definitions);
        assert!(Arc::ptr_eq(&initial, &cache.get(&doc, &definitions)));
        doc.set_ini("Basic", "Player", Some(b"MissingHouse".to_vec()))
            .unwrap();
        doc.commit("change mission");
        let changed = cache.get(&doc, &definitions);
        assert!(!Arc::ptr_eq(&initial, &changed));
        assert!(changed.diagnostics.iter().any(|d| d.field == "Player"));
        doc.undo();
        assert!(cache.get(&doc, &definitions).diagnostics.is_empty());
        doc.redo();
        assert!(
            cache
                .get(&doc, &definitions)
                .diagnostics
                .iter()
                .any(|d| d.field == "Player")
        );
        let replacement = Arc::new(mission::ExternalDefinitions {
            source: "aimd.ini".into(),
            error: Some("missing".into()),
            ..Default::default()
        });
        let report = cache.get(&doc, &replacement);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.issue.message.contains("读取失败"))
        );
        let other = MapDocument::new(base);
        assert!(cache.get(&other, &definitions).diagnostics.is_empty());
    }
    #[test]
    fn edited_flag_does_not_transcode_legacy_name_or_extension_tail() {
        let original = b"Americans,<none>,\xD6\xD0,0,1,1,1,0,\xCE\xC4";
        let draft = String::from_utf8_lossy(original).replacen(",0,1,1,1,0,", ",1,1,1,1,0,", 1);
        assert_eq!(
            preserve_csv(Some(original), &draft),
            b"Americans,<none>,\xD6\xD0,1,1,1,1,0,\xCE\xC4"
        );
    }
}
