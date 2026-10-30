//! Campaign project files and package work run on a dedicated worker.
use crate::{editing::Editor, map::MapScene};
use eframe::egui;
use ra2_map_document::campaign::{Asset, Mission, Project};
use ra2_map_io::campaign::{self, Report};
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
};

enum Done {
    Loaded(Project, PathBuf),
    Saved(PathBuf),
    Report(Report),
    Exported(Report, PathBuf),
}
#[derive(Default)]
pub(crate) struct CampaignUi {
    pub open: bool,
    project: Project,
    saved: String,
    path: Option<PathBuf>,
    selected: usize,
    current_file: String,
    label: String,
    text: String,
    message: String,
    report: Option<Report>,
    worker: Option<mpsc::Receiver<anyhow::Result<Done>>>,
}
impl CampaignUi {
    pub(crate) fn smoke_open(&mut self, path: &Path) {
        let file = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_uppercase();
        self.project = Project {
            version: 1,
            id: "EDITOR_CAMPAIGN".into(),
            entry: file.clone(),
            missions: vec![Mission {
                file: file.clone(),
                source: path.to_string_lossy().into(),
                side: "GDI".into(),
                ui_name: "ED:TITLE".into(),
                briefing: "ED:BRIEF".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        self.saved = serde_json::to_string(&self.project).unwrap();
        self.current_file = file;
        self.open = true;
    }
    fn dirty(&self) -> bool {
        serde_json::to_string(&self.project).unwrap_or_default() != self.saved
            && self.project.version == 1
    }
    pub fn allow_leave(&mut self) -> bool {
        if self.dirty() {
            self.open = true;
            self.message = "战役项目尚未保存，请保存或放弃项目修改后继续。".into();
            false
        } else {
            true
        }
    }
    fn start(
        &mut self,
        ctx: &egui::Context,
        task: impl FnOnce() -> anyhow::Result<Done> + Send + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        self.worker = Some(rx);
        self.message = "正在处理…".into();
        std::thread::spawn(move || {
            let _ = tx.send(task());
            ctx.request_repaint();
        });
    }
    pub fn show(&mut self, ctx: &egui::Context, editor: &Editor, directory: &Path) {
        if let Some(rx) = &self.worker {
            match rx.try_recv() {
                Ok(result) => {
                    self.worker = None;
                    match result {
                        Ok(Done::Loaded(p, path)) => {
                            self.project = p;
                            self.saved = serde_json::to_string(&self.project).unwrap();
                            self.path = Some(path);
                            self.selected = 0;
                            self.current_file.clear();
                            self.report = None;
                            self.message = "已读取战役项目；源文件相对项目位置解析。".into();
                        }
                        Ok(Done::Saved(path)) => {
                            self.path = Some(path);
                            self.saved = serde_json::to_string(&self.project).unwrap();
                            self.message = "战役项目已保存。".into();
                        }
                        Ok(Done::Report(r)) => {
                            self.message = if r.passed {
                                "依赖检查通过。".into()
                            } else {
                                "有缺失依赖，请修正后再发布。".into()
                            };
                            self.report = Some(r);
                        }
                        Ok(Done::Exported(r, path)) => {
                            self.message = format!("发布完成：{}", path.display());
                            self.report = Some(r);
                        }
                        Err(e) => self.message = format!("{e:#}"),
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.worker = None;
                    self.message = "后台任务意外结束。".into();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if !self.open {
            return;
        }
        let mut open = true;
        crate::ui_theme::window(ctx,"战役目录与任务发布","campaign-project",egui::vec2(880.,720.)).open(&mut open).show(ctx,|ui| {
            ui.label("战役目录和资源单独保存为项目 JSON；地图内分支/继承选项在“任务设置”编辑。项目与地图分别保存。");
            ui.add_enabled_ui(self.worker.is_none(),|ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(!self.dirty(),egui::Button::new("从当前地图新建项目")).clicked() {
                        if let Some(path)=editor.target.as_ref() {
                            let name=path.file_name().unwrap_or_default().to_string_lossy().to_ascii_uppercase();
                            self.project=Project {version:1,id:"EDITOR_CAMPAIGN".into(),entry:name.clone(),missions:vec![Mission {file:name.clone(),source:path.to_string_lossy().into(),side:"GDI".into(),..Default::default()}],..Default::default()};
                            self.current_file=name; self.path=None; self.selected=0; self.saved=String::new(); self.report=None;
                        } else { self.message="请先将当前地图保存为 .map 文件。".into(); }
                    }
                    if ui.add_enabled(!self.dirty(),egui::Button::new("打开项目…")).clicked()
                        && let Some(path)=rfd::FileDialog::new().add_filter("战役项目", &["json"]).pick_file() {
                        self.start(ctx,move || Ok(Done::Loaded(campaign::read_project(&path)?,path)));
                    }
                    if ui.button("保存项目…").clicked() && self.project.version == 1
                        && let Some(path)=rfd::FileDialog::new().add_filter("战役项目", &["json"]).set_file_name("campaign.json").save_file() {
                        let mut project=self.project.clone();
                        let root=self.path.as_ref().and_then(|p|p.parent()).unwrap_or(Path::new("."));
                        for m in &mut project.missions { m.source=root.join(&m.source).to_string_lossy().into(); }
                        for a in &mut project.assets { a.source=root.join(&a.source).to_string_lossy().into(); }
                        self.project=project.clone();
                        self.start(ctx,move || {campaign::save_project(&path,&project)?;Ok(Done::Saved(path))});
                    }
                    if ui.add_enabled(self.dirty(),egui::Button::new("放弃项目修改")).clicked() {
                        if self.saved.is_empty() {self.project=Project::default();} else if let Ok(p)=serde_json::from_str(&self.saved) {self.project=p;}
                        self.report=None;
                    }
                });
                if self.project.version!=1 { return; }
                ui.label(format!("{}{}",self.path.as_ref().map_or("未保存项目".into(),|p|p.display().to_string()),if self.dirty(){" · 有未保存修改"}else{""}));
                egui::ScrollArea::vertical().max_height((ui.available_height()-105.).max(130.)).show(ui,|ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("战役 ID");ui.text_edit_singleline(&mut self.project.id);
                        egui::ComboBox::from_id_salt("entry-map").selected_text(format!("起始：{}",self.project.entry)).show_ui(ui,|ui| {for m in &self.project.missions {ui.selectable_value(&mut self.project.entry,m.file.clone(),&m.file);}});
                    });
                    ui.small("发布器生成 BATTLEMD / MISSIONMD / MAPSELMD，支持直接 Next/AltNext 分支；全局 0 为真走 Alt。非终关必须 SkipMapSelect=yes，两个目标都须在目录中。跨 Side 的目录也生成相同阶段序列以保留 stage 索引。");
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("添加关卡…").clicked() && let Some(path)=rfd::FileDialog::new().add_filter("地图", &["map"]).pick_file() {
                            self.project.missions.push(Mission {file:path.file_name().unwrap_or_default().to_string_lossy().to_ascii_uppercase(),source:path.to_string_lossy().into(),side:"GDI".into(),..Default::default()}); self.selected=self.project.missions.len()-1;
                        }
                        egui::ComboBox::from_id_salt("mission-index").selected_text(self.project.missions.get(self.selected).map_or("选择关卡",|m|m.file.as_str())).show_ui(ui,|ui|{for (i,m) in self.project.missions.iter().enumerate(){ui.selectable_value(&mut self.selected,i,&m.file);}});
                        if ui.button("删除关卡").clicked() && self.selected<self.project.missions.len(){self.project.missions.remove(self.selected);self.selected=0;}
                    });
                    if let Some(m)=self.project.missions.get_mut(self.selected) {
                        egui::Grid::new("mission-metadata").num_columns(2).show(ui,|ui| {
                            for (label,value) in [("输出文件名",&mut m.file),("源文件",&mut m.source),("阵营：GDI/Nod/ThirdSide",&mut m.side),("显示名称（CSF）",&mut m.ui_name),("任务简报（CSF）",&mut m.briefing),("载入标题（CSF）",&mut m.load_message),("载入简报（CSF）",&mut m.load_briefing),("640 载入背景文件",&mut m.background_640),("800 载入背景文件",&mut m.background_800),("载入背景调色板",&mut m.background_palette)]{ui.label(label);ui.text_edit_singleline(value);ui.end_row();}
                        });
                    }
                    ui.separator(); ui.label("CSF 文本（合并活动 ra2md.csf，保留基础记录；安装后重启游戏）");
                    ui.horizontal_wrapped(|ui|{ui.label("标签");ui.text_edit_singleline(&mut self.label);});
                    ui.add(egui::TextEdit::multiline(&mut self.text).desired_rows(2).desired_width(f32::INFINITY));
                    if ui.button("写入/替换项目字符串").clicked(){self.project.strings.insert(self.label.clone(),self.text.clone());}
                    let mut remove=None;
                    for (key,value) in &self.project.strings {ui.horizontal_wrapped(|ui|{
                        if ui.small_button("编辑").clicked(){self.label=key.clone();self.text=value.clone();}
                        if ui.small_button("删除").clicked(){remove=Some(key.clone());}
                        ui.label(format!("{key} · {value}"));
                    });}
                    if let Some(key)=remove {self.project.strings.remove(&key);}
                    ui.separator();ui.label("独立资源：文件名必须与固定 INI/媒体引用一致；自定义声音须带 soundmd/evamd/thememd 与样本。基础游戏资源只记依赖。");
                    if ui.button("添加资源文件…").clicked() && let Some(path)=rfd::FileDialog::new().pick_file(){self.project.assets.push(Asset {source:path.to_string_lossy().into(),name:path.file_name().unwrap_or_default().to_string_lossy().into()});}
                    let mut remove=None;
                    for (i,a) in self.project.assets.iter_mut().enumerate(){ui.push_id(i,|ui|{ui.horizontal_wrapped(|ui|{if ui.small_button("移除").clicked(){remove=Some(i);}ui.text_edit_singleline(&mut a.name);ui.label(&a.source);});});}
                    if let Some(i)=remove {self.project.assets.remove(i);}
                    if let Err(e)=self.project.validate(){ui.colored_label(egui::Color32::YELLOW,e.to_string());}
                    if let Some(report)=&self.report {
                        ui.separator(); ui.label(format!("上次快照检查：{} 项依赖，{} 项缺失（修改后须重检）",report.dependencies.len(),report.dependencies.iter().filter(|d|!d.resolved).count()));
                        for d in &report.dependencies {ui.colored_label(if d.resolved{egui::Color32::LIGHT_GREEN}else{egui::Color32::LIGHT_RED},format!("{} · {} {} · {}",d.owner,d.kind,d.name,d.source));}
                        for warning in &report.warnings {ui.colored_label(egui::Color32::YELLOW,warning);}
                    }
                });
                ui.separator();
                egui::ComboBox::from_id_salt("snapshot-map").selected_text(format!("当前快照替换：{}",if self.current_file.is_empty(){"不替换"}else{&self.current_file})).show_ui(ui,|ui|{ui.selectable_value(&mut self.current_file,String::new(),"全部读取源文件");for m in &self.project.missions{ui.selectable_value(&mut self.current_file,m.file.clone(),&m.file);}});
                ui.horizontal_wrapped(|ui| {
                    let check=ui.button("检查依赖").clicked();
                    let export=ui.button("导出发布包…").clicked();
                    if check || export {
                        let output=if export {rfd::FileDialog::new().set_file_name("mission-package").save_file()}else{None};
                        if !export || output.is_some() {
                            let snapshot=if self.current_file.is_empty(){None}else{editor.doc.as_ref().map(|d|d.snapshot())};
                            let current_file=self.current_file.clone();let project=self.project.clone();let directory=directory.to_owned();
                            let root=self.path.as_ref().and_then(|p|p.parent()).unwrap_or(Path::new(".")).to_owned();
                            self.start(ctx,move || {let bytes=snapshot.map(|s|s.bytes()).transpose()?;let prepared=campaign::prepare(&project,&root,&directory,bytes.as_ref().map(|b|(current_file.as_str(),b.as_slice())))?;
                                if let Some(output)=output {if !prepared.report.passed{return Ok(Done::Report(prepared.report));}let r=campaign::export(prepared,&output)?;Ok(Done::Exported(r,output))}else{Ok(Done::Report(prepared.report))}});
                        }
                    }
                });
            });
            ui.label(&self.message);
        });
        if !open && !self.dirty() {
            self.open = false;
        } else if !open {
            self.message = "项目尚未保存；请保存或放弃项目修改后关闭。".into();
        }
    }
}
/// Only known runtime fields get controls; remaining fields use the raw form.
pub(crate) fn basic_field(
    ui: &mut egui::Ui,
    key: &str,
    value: &mut String,
    scene: &MapScene,
) -> bool {
    if matches!(
        key,
        "Intro" | "Brief" | "Action" | "Win" | "Lose" | "PostScore" | "PreMapSelect" | "Theme"
    ) {
        ui.horizontal_wrapped(|ui| {
            ui.label(if key == "Brief" {
                "简报影片（任务简报文本在战役目录中设置）"
            } else {
                crate::labels::field(key)
            })
            .on_hover_text(key);
            ui.text_edit_singleline(value);
            let catalog = if key == "Theme" { "Theme" } else { "Movie" };
            if let Some(items) = scene.mission_resources.choices.get(catalog) {
                egui::ComboBox::from_id_salt(key)
                    .selected_text("选择…")
                    .show_ui(ui, |ui| {
                        ui.selectable_value(value, "<none>".into(), "无");
                        for (id, label) in items {
                            ui.selectable_value(value, id.clone(), label);
                        }
                    });
            }
        });
        return true;
    }
    if matches!(
        key,
        "NextScenario"
            | "AltNextScenario"
            | "CarryOverMoney"
            | "CarryOverCap"
            | "TimerInherit"
            | "SkipMapSelect"
    ) {
        let hint = match key {
            "NextScenario" => "全局 0 为假时的目标 .map；须在发布目录中",
            "AltNextScenario" => "全局 0 为真时的目标 .map",
            "CarryOverMoney" => "接收关继承比例，如 25%；最大 100%，默认 0",
            "CarryOverCap" => "接收关继承资金上限；-1 无上限，默认 0",
            "TimerInherit" => "接收关是否继承倒计时；yes/no",
            _ => "直接分支需 yes，并生成 MAPSELMD 选项",
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(crate::labels::field(key)).on_hover_text(key);
            ui.text_edit_singleline(value);
        });
        ui.small(hint);
        return true;
    }
    false
}
