//! Map geometry planning, automatic lighting edits and read-only previews.
use crate::{editing::Editor, map::MapScene};
use eframe::egui;
use ra2_map_document::{
    document::{MapDocument, Snapshot},
    lighting::{COMPONENTS, Draft, Profile},
    map_settings::{self, Footprints, Options, Plan},
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
    pub profile: Profile,
    preview: bool,
    due: Option<Instant>,
    plan: Option<Plan>,
    worker: Option<std::sync::mpsc::Receiver<anyhow::Result<Plan>>>,
    message: String,
    completion: crate::form_edit::EditCompletion,
    light_editing: bool,
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
        self.options != self.original || self.light != self.original_light
    }
    pub fn allow_leave(&mut self) -> bool {
        if self.dirty() {
            self.open = true;
            self.message =
                "请完成或取消光照输入；尺寸修改需检查预览并应用，或放弃尺寸修改。".into();
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
        self.plan = None;
        self.worker = None;
        self.completion = Default::default();
        self.light_editing = false;
    }
    pub(crate) fn finish_lighting(
        &mut self,
        ctx: &egui::Context,
        editor: &mut Editor,
        scene: &MapScene,
    ) {
        self.finish_lighting_inner(ctx, editor, scene, false);
    }
    pub(crate) fn finish_for_save(
        &mut self,
        ctx: &egui::Context,
        editor: &mut Editor,
        scene: &MapScene,
    ) {
        self.finish_lighting_inner(ctx, editor, scene, true);
    }
    fn finish_lighting_inner(
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
            && !editor.stroking
            && (save || ctx.input(|i| i.viewport().close_requested()) || self.completion.ready(ctx))
        {
            let result = (|| -> anyhow::Result<()> {
                let doc = editor
                    .doc
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("没有地图"))?;
                anyhow::ensure!(
                    self.identity == Some((doc.id, doc.state_id)),
                    "文档已变化，请取消本次输入后重新编辑"
                );
                self.apply_lighting(doc)
            })();
            match result {
                Ok(()) => {
                    editor.commit(scene, "地图光照");
                    self.identity = editor.doc.as_ref().map(|doc| (doc.id, doc.state_id));
                    self.original_light = self.light.clone();
                    self.light_editing = false;
                    self.message = "光照已自动生效，可撤销。".into();
                }
                Err(error) => self.message = format!("尚未生效：{error:#}"),
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
        self.finish_lighting(ctx, editor, scene);
        let doc = editor.doc.as_ref().unwrap();
        let light_before = self.light.clone();
        if let Some(rx) = &self.worker {
            match rx.try_recv() {
                Ok(result) => {
                    self.worker = None;
                    match result {
                        Ok(p) if Some(&p.options) == self.options.as_ref() => {
                            self.plan = Some(p);
                            self.message = "迁移预览完成，未改写地图。".into();
                        }
                        Ok(_) => self.message = "设置已改变，请重新检查迁移。".into(),
                        Err(e) => self.message = format!("{e:#}"),
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.worker = None;
                    self.message = "迁移检查线程意外结束。".into();
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
        crate::ui_theme::window(ctx,"地图设置与光照","map-properties",egui::vec2(750.,690.)).open(&mut open).show(ctx,|ui|{
            ui.horizontal_wrapped(|ui|{
                ui.add_enabled_ui(!self.dirty(),|ui|{
                    changed|=ui.selectable_value(&mut self.tab,0,"尺寸与可玩区域").changed();
                    changed|=ui.selectable_value(&mut self.tab,1,"光照").changed();
                });
                if self.dirty(){ui.colored_label(egui::Color32::YELLOW,if self.tab==1{"光照输入待完成"}else{"有未应用尺寸设置"});}
            });
            egui::ScrollArea::vertical().max_height((ui.available_height()-100.).max(140.)).show(ui,|ui|{
                if self.tab==0 {
                    let options=self.options.as_mut().unwrap();
                    ui.label(format!("当前尺寸：{} × {}；宽＋高 ≤ 512。平移以投影平面左/上为单位。",doc.base.header.width,doc.base.header.height));
                    egui::Grid::new("map-dimensions").num_columns(2).show(ui,|ui|{
                        for (name,value) in [("新宽度",&mut options.width),("新高度",&mut options.height)]{ui.label(name);changed|=crate::form_edit::number(ui,egui::DragValue::new(value).range(20..=492),name).changed();ui.end_row();}
                        ui.label("原图放置偏移（左、上）");ui.horizontal(|ui|{for (value,name) in options.offset.iter_mut().zip(["左侧偏移","顶部偏移"]){changed|=crate::form_edit::number(ui,egui::DragValue::new(value).range(-512..=512),name).changed();}});ui.end_row();
                        ui.label("扩展区域清地高度");changed|=crate::form_edit::number(ui,egui::DragValue::new(&mut options.fill_level).range(0..=14),"扩展区域清地高度").changed();ui.end_row();
                        ui.label("可玩区域（左、上、宽、高）").on_hover_text("LocalSize");ui.horizontal_wrapped(|ui|{for (value,name) in options.local.iter_mut().zip(["可玩区域左侧","可玩区域顶部","可玩区域宽度","可玩区域高度"]){changed|=crate::form_edit::number(ui,egui::DragValue::new(value).range(0..=512),name).changed();}});ui.end_row();
                    });
                    ui.horizontal_wrapped(|ui|{
                        if ui.button("可玩区域铺满有效边距").clicked(){options.local=[2,4,i32::from(options.width)-4,i32::from(options.height)-10];changed=true;}
                        if ui.button("居中放置原图").clicked(){options.offset=[(i32::from(options.width)-doc.base.header.width as i32)/2,(i32::from(options.height)-doc.base.header.height as i32)/2];changed=true;}
                    });
                    changed|=ui.checkbox(&mut options.delete_outside,"应用时删除预览中允许删除的越界内容").changed();
                    ui.small("裁切可删除越界对象、覆盖物、格子标签、基地节点和整条隧道。越界航点、桥梁、脚本目的格和任务可视矩形须先调整；共享标签保留。建筑按当前美术配置中的占地检查。");
                    if let Err(e)=options.validate(){ui.colored_label(egui::Color32::LIGHT_RED,e.to_string());}
                    inspect=ui.add_enabled(self.worker.is_none(),egui::Button::new("检查迁移与越界预览")).clicked();
                    if self.worker.is_some(){ui.spinner();}
                    if let Some(plan)=&self.plan {
                        let s=&plan.summary;ui.label(format!("地形新增 {} / 裁去 {}；移动对象 {}、任务引用 {}；越界 {} 项",s.terrain_added,s.terrain_removed,s.records_moved,s.references_moved,s.affected.len()));
                        draw_preview(ui,doc,&plan.options,&plan.summary);
                        for a in s.affected.iter().take(100){ui.colored_label(if a.blocking{egui::Color32::LIGHT_RED}else{egui::Color32::YELLOW},format!("{} {:?} · {}",a.source,a.cell,if a.blocking{"请先调整"}else{"将删除"}));}
                        if s.affected.len()>100{ui.label("其余项目见预览统计；裁切前请检查边界内容。");}
                        apply_map=ui.add_enabled(plan.can_apply(),egui::Button::new("应用地图设置（整笔撤销）")).clicked();
                    }
                }else{
                    ui.horizontal_wrapped(|ui|{
                        for p in Profile::ALL{changed|=ui.selectable_value(&mut self.profile,p,p.label()).changed();}
                        changed|=ui.checkbox(&mut self.preview,"在画布实时预览").changed();
                    });
                    ui.small("光照参数结束输入后自动生效，可撤销。关闭面板恢复普通光照预览，保存与发布使用当前参数。空值使用引擎默认值。");
                    let light=self.light.as_mut().unwrap();
                    egui::Grid::new("lighting-components").num_columns(2).show(ui,|ui|{
                        for component in COMPONENTS{let key=format!("{}{component}",self.profile.prefix());let label=crate::labels::field(&key);ui.label(label).on_hover_text(&key);changed|=ui.text_edit_singleline(light.values.get_mut(&key).unwrap()).accessible_name(label).changed();ui.end_row();}
                        for key in ["DominatorAmbientChangeRate","NukeAmbientChangeRate"]{let label=crate::labels::field(key);ui.label(label).on_hover_text(key);changed|=ui.text_edit_singleline(light.values.get_mut(key).unwrap()).accessible_name(label).changed();ui.end_row();}
                    });
                    ui.small("环境亮度与红绿蓝分量按 1%=1 单位量化；地面与每层高度亮度补偿按 0.001 量化。心灵控制器顶部层高使用原生核爆参数（每层补偿 0.1）。过渡间隔只编辑，不模拟动态淡入淡出。");
                    if let Ok(p)=light.effective(doc,self.profile){ui.label(format!("实际量化：环境亮度 {}%，红绿蓝 {:?}%，地面补偿 {} / 每层补偿 {}（千分单位）",p.units.ambient_percent,[p.units.red_percent,p.units.green_percent,p.units.blue_percent],p.units.ground_units,p.units.level_units));}
                    if let Err(e)=light.validate(){ui.colored_label(egui::Color32::LIGHT_RED,e.to_string());}
                    ui.horizontal_wrapped(|ui|{
                        if ui.button("所选配置使用引擎默认").clicked(){for c in COMPONENTS{light.values.insert(format!("{}{c}",self.profile.prefix()),String::new());}changed=true;}
                    });
                }
            });
            ui.separator();
            ui.horizontal_wrapped(|ui|{
                let cancel=ui.button(if self.tab==1{"取消本次输入"}else{"放弃尺寸修改"});
                self.completion.cancel_button(&cancel);
                discard=cancel.clicked();
                undo=ui.add_enabled(!self.dirty(),egui::Button::new("撤销")).clicked();
                redo=ui.add_enabled(!self.dirty(),egui::Button::new("重做")).clicked();
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
            self.message = "正在后台检查迁移…".into();
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
                    self.message = "已应用地图设置，可整笔撤销。".into();
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
        self.finish_lighting(ctx, editor, scene);
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
                self.message = "请完成或取消当前输入；尺寸修改需应用或放弃后关闭。".into();
            } else {
                self.open = false;
                self.due = None;
                editor.refresh = true;
            }
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
    ui.small(
        "灰：新尺寸；蓝：原图放置范围；绿：可玩区域；红/黄：受影响内容（位置按原图平移后绘制）。",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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
            form.finish_lighting(ui.ctx(), &mut editor, &scene);
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
            form.finish_lighting(ui.ctx(), &mut editor, &scene);
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
            form.finish_lighting(ui.ctx(), &mut editor, &scene);
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
            form.finish_lighting(ui.ctx(), &mut editor, &scene);
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
            form.finish_lighting(ui.ctx(), &mut editor, &scene);
        });
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            before
        );
        form.light_editing = true;
        form.options.as_mut().unwrap().width += 1;
        let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
            form.finish_lighting(ui.ctx(), &mut editor, &scene);
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
