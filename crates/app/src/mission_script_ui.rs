use crate::i18n::tr;
use crate::{editing::Editor, map::MapScene};
use anyhow::{Context, Result, ensure};
use eframe::egui;
use ra2_map_io::mission_script::{self, Built, Sources, Worker};
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
};
type FreshnessResult = (u64, u64, Result<(), String>);

#[derive(Default)]
pub(crate) struct ScriptUi {
    pub open: bool,
    document: u64,
    input_profile: String,
    path: String,
    message: String,
    worker: Option<Worker>,
    waiting: Option<mpsc::Receiver<Result<Built, String>>>,
    pending: Option<Built>,
    freshness: Option<mpsc::Receiver<FreshnessResult>>,
    freshness_at: Option<std::time::Instant>,
    freshness_message: String,
}

impl ScriptUi {
    fn script_directory(&self, editor: &Editor) -> Result<PathBuf> {
        let path = Path::new(self.path.trim());
        if !path.as_os_str().is_empty() {
            if path.is_dir() {
                return Ok(path.to_path_buf());
            }
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                return Ok(parent.to_path_buf());
            }
        } else if let Some(parent) = editor
            .target
            .as_deref()
            .and_then(Path::parent)
            .filter(|p| !p.as_os_str().is_empty())
        {
            return Ok(parent.to_path_buf());
        }
        std::env::current_dir().context(tr!("无法读取当前目录"))
    }

    fn script_files(directory: &Path) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(directory).context(tr!("无法读取脚本目录"))? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if entry.file_type()?.is_file()
                && path.extension().is_some_and(|s| s == "ts")
                && !name.ends_with(".d.ts")
                && !matches!(name.as_str(), "sdk.ts" | "map.generated.ts")
            {
                files.push(path);
            }
        }
        files.sort();
        Ok(files)
    }

    fn script_path(&self) -> Result<PathBuf> {
        ensure!(
            !self.path.trim().is_empty(),
            tr!("请先新建脚本或填写已有脚本路径")
        );
        let path = std::fs::canonicalize(self.path.trim()).context(tr!("无法读取任务脚本路径"))?;
        ensure!(
            path.is_file() && path.extension().is_some_and(|s| s == "ts"),
            tr!("任务入口须为已有的 .ts 文件")
        );
        Ok(path)
    }

    fn refresh_types(&mut self, editor: &Editor, scene: &MapScene) -> Result<()> {
        let path = self.script_path()?;
        let doc = editor.doc.as_ref().context(tr!("缺少地图"))?;
        mission_script::refresh_document_types(doc, &path, &mission_script::catalog(doc, scene))?;
        self.message =
            tr!("已刷新 sdk.ts 和 map.generated.ts，外部编辑器可使用最新类型提示。").into();
        Ok(())
    }

    fn start(&mut self, editor: &Editor, scene: &MapScene) -> Result<()> {
        let path = self.script_path()?;
        let doc = editor.doc.as_ref().context(tr!("缺少地图"))?;
        let catalog = mission_script::catalog(doc, scene);
        mission_script::refresh_document_types(doc, &path, &catalog)?;
        let sources = Sources::read_for_document(doc, &path)?;
        if self.worker.is_none() {
            self.worker = Some(Worker::new()?);
        }
        self.waiting = Some(self.worker.as_ref().unwrap().build(doc, catalog, sources)?);
        self.input_profile = scene.revision.id();
        self.message = tr!("正在读取已保存的脚本并检查、构建，通过后自动应用到地图…").into();
        Ok(())
    }

    fn apply(&mut self, built: Built, editor: &mut Editor, scene: &MapScene) -> Result<()> {
        ensure!(
            self.input_profile == scene.revision.id(),
            tr!("资源配置已变化，请重新点击“应用到地图”")
        );
        built.sources.unchanged()?;
        let changed = built
            .build
            .apply(editor.doc.as_mut().context(tr!("缺少地图"))?)?;
        if changed {
            editor.structural_changed(scene);
            editor.status = tr!("已应用 TypeScript 任务，可撤销、保存或试玩。").into();
            self.message = tr!(
                "已应用到地图：{arg0} 条规则、{arg1} 支小队。可整笔撤销，保存地图后即可试玩。",
                arg0 = built.build.rule_count,
                arg1 = built.build.team_count
            );
        } else {
            self.message = tr!("任务未变化，无需重复应用。").into();
        }
        Ok(())
    }

    pub fn show(&mut self, ctx: &egui::Context, editor: &mut Editor, scene: &MapScene) {
        let Some(doc) = editor.doc.as_ref() else {
            return;
        };
        if self.document != doc.id {
            self.document = doc.id;
            self.path.clear();
            if let Ok(Some(path)) = mission_script::saved_entry(doc) {
                self.path = path;
            }
            self.message.clear();
            self.freshness_message.clear();
            self.freshness_at = None;
            self.pending = None;
            self.waiting = None;
        }
        if let Some(receiver) = &self.waiting {
            match receiver.try_recv() {
                Ok(Ok(built)) => {
                    self.pending = Some(built);
                    self.waiting = None;
                }
                Ok(Err(error)) => {
                    self.message = error;
                    self.waiting = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.message = tr!("任务编译线程意外退出").into();
                    self.waiting = None;
                    self.worker = None;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }
        // Wait until the current brush stroke finishes before applying a structural map edit.
        if !editor.stroking
            && let Some(built) = self.pending.take()
            && let Err(error) = self.apply(built, editor, scene)
        {
            self.message = tr!("应用失败：{error}", error = format!("{:#}", error));
        }
        if !self.open {
            return;
        }
        let Some(doc) = editor.doc.as_ref() else {
            return;
        };
        if let Some(receiver) = &self.freshness {
            match receiver.try_recv() {
                Ok((id, state, result)) => {
                    if (id, state) == (doc.id, doc.state_id) {
                        self.freshness_message = result.err().map_or_else(
                            || tr!("已应用脚本与当前源码一致。").into(),
                            |e| tr!("待重新构建：{e}", e = e),
                        );
                    }
                    self.freshness = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.freshness = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.freshness.is_none() && self.freshness_at.is_none_or(|t| t.elapsed().as_secs() >= 3)
        {
            self.freshness_at = Some(std::time::Instant::now());
            let snapshot = doc.snapshot();
            let id = doc.id;
            let state = doc.state_id;
            let profile = scene.revision.id();
            let (send, receive) = mpsc::channel();
            if std::thread::Builder::new()
                .name("mission-freshness".into())
                .spawn(move || {
                    let result = (|| -> Result<()> {
                        mission_script::resource_payload(&snapshot.bytes()?, &profile)?;
                        Ok(())
                    })();
                    let _ = send.send((id, state, result.map_err(|e| format!("{e:#}"))));
                })
                .is_ok()
            {
                self.freshness = Some(receive);
            }
        }
        if self.worker.is_none() {
            match Worker::new() {
                Ok(worker) => self.worker = Some(worker),
                Err(error) => {
                    self.message = tr!(
                        "无法启动任务编译线程：{error}",
                        error = format!("{:#}", error)
                    )
                }
            }
        }
        let mut open = true;
        let busy = self.waiting.is_some() || self.pending.is_some();
        let mut result: Result<()> = Ok(());
        crate::ui_theme::window(
            ctx,
            tr!("TypeScript 任务"),
            "mission-typescript",
            egui::vec2(700., 300.),
        )
        .open(&mut open)
        .resizable(true)
        .show(ctx, |ui| {
            ui.label(tr!(
                "在外部编辑器中编写、检查类型并保存脚本，然后应用到地图。"
            ));
            if !self.freshness_message.is_empty() {
                ui.label(&self.freshness_message);
            }
            ui.horizontal(|ui| {
                ui.label(tr!("脚本路径"));
                ui.add_enabled(
                    !busy,
                    egui::TextEdit::singleline(&mut self.path)
                        .hint_text(tr!("从下拉列表选择，或填写 .ts 文件路径"))
                        .desired_width(f32::INFINITY),
                );
            });
            ui.horizontal(|ui| {
                ui.label(tr!("选择脚本"));
                ui.add_enabled_ui(!busy, |ui| {
                    let selected = Path::new(self.path.trim())
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| tr!("选择当前目录的 .ts 文件").into());
                    egui::ComboBox::from_id_salt("mission-script-file")
                        .selected_text(selected)
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            match self.script_directory(editor).and_then(|directory| {
                                let files = Self::script_files(&directory)?;
                                Ok((directory, files))
                            }) {
                                Ok((directory, files)) => {
                                    ui.label(directory.display().to_string());
                                    ui.separator();
                                    if files.is_empty() {
                                        ui.label(tr!("当前目录没有可用的 .ts 任务脚本"));
                                    }
                                    for path in files {
                                        let name = path.file_name().unwrap().to_string_lossy();
                                        if ui
                                            .selectable_value(
                                                &mut self.path,
                                                path.to_string_lossy().into_owned(),
                                                name.as_ref(),
                                            )
                                            .changed()
                                        {
                                            self.message.clear();
                                        }
                                    }
                                }
                                Err(error) => {
                                    ui.label(format!("{error:#}"));
                                }
                            }
                        });
                });
            });
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new(tr!("新建…")))
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_file_name("mission.ts")
                        .add_filter("TypeScript", &["ts"])
                        .save_file()
                {
                    let doc = editor.doc.as_ref().unwrap();
                    let catalog = mission_script::catalog(doc, scene);
                    match mission_script::create(&path, &catalog, &doc.ini_text("Basic", "Player"))
                    {
                        Ok(_) => {
                            self.path = path.to_string_lossy().into_owned();
                            self.message =
                                tr!("已创建任务脚本及类型文件，请在外部编辑器中打开脚本。").into();
                        }
                        Err(error) => result = Err(error),
                    }
                }
                let has_path = !self.path.trim().is_empty();
                if ui
                    .add_enabled(!busy && has_path, egui::Button::new(tr!("刷新类型")))
                    .clicked()
                {
                    result = self.refresh_types(editor, scene);
                }
                if ui
                    .add_enabled(
                        !busy && has_path && !editor.stroking,
                        egui::Button::new(tr!("应用到地图")),
                    )
                    .clicked()
                {
                    result = self.start(editor, scene);
                }
                if busy {
                    ui.spinner();
                }
            });
            ui.label(tr!(
                "从当前目录下拉选择脚本，也可填写其他路径。修改地图后，刷新类型即可同步提示。"
            ));
            ui.label(tr!(
                "应用时自动刷新类型并转译磁盘源码，不做 TS 类型检查；成功后可撤销、保存或试玩。"
            ));
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("typescript-diagnostics")
                .max_height(150.)
                .show(ui, |ui| {
                    ui.add(egui::Label::new(&self.message).wrap());
                });
        });
        if let Err(error) = result {
            self.message = format!("{error:#}");
        }
        self.open = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra2_map_mission_script::native::{self, Catalog, Mission};
    use std::path::Path;

    fn completed_request(editor: &Editor, scene: &MapScene, entry: &Path) -> ScriptUi {
        let doc = editor.doc.as_ref().unwrap();
        let catalog = Catalog::from_document(doc, vec![], vec![]);
        let plan: Mission = serde_json::from_value(serde_json::json!({
            "version": 1, "id": "test", "player": "Neutral", "teams": [], "flags": [],
            "rules": [{"id": "win", "once": true,
                "when": {"kind": "scenarioTime", "seconds": 10},
                "actions": [{"kind": "winPlayer"}]}]
        }))
        .unwrap();
        let built = Built {
            build: native::prepare(doc, &plan, &catalog).unwrap(),
            sources: Sources::read(entry).unwrap(),
        };
        let (sender, receiver) = mpsc::channel();
        sender.send(Ok(built)).unwrap();
        ScriptUi {
            document: doc.id,
            input_profile: scene.revision.id(),
            path: entry.to_string_lossy().into_owned(),
            waiting: Some(receiver),
            ..Default::default()
        }
    }

    #[test]
    fn completed_apply_waits_for_brush_and_is_one_undo_step() {
        let (mut editor, ctx, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let entry = root.join("mission.ts");
        std::fs::write(&entry, "export default {};").unwrap();
        let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        let mut form = completed_request(&editor, &scene, &entry);
        editor.stroking = true;
        form.show(&ctx, &mut editor, &scene);
        assert!(form.pending.is_some());
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            before
        );
        editor.stroking = false;
        form.show(&ctx, &mut editor, &scene);
        assert!(form.pending.is_none());
        assert!(form.message.starts_with("已应用到地图"), "{}", form.message);
        assert_ne!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            before
        );
        editor.history(false, &scene);
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            before
        );
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn auto_apply_rejects_changed_sources_map_profile_and_different_documents() {
        for change in ["source", "map", "profile", "document"] {
            let (mut editor, ctx, root) = crate::editing::tests::editor();
            let scene = crate::editing::tests::terrain_scene();
            editor.attach(&scene);
            let entry = root.join("mission.ts");
            std::fs::write(&entry, "export default {};").unwrap();
            let mut form = completed_request(&editor, &scene, &entry);
            match change {
                "source" => std::fs::write(&entry, "export default { changed: true };").unwrap(),
                "map" => {
                    editor
                        .doc
                        .as_mut()
                        .unwrap()
                        .set_ini("Basic", "Name", Some(b"newer".to_vec()))
                        .unwrap();
                    editor.commit(&scene, "edit during build");
                }
                "profile" => form.input_profile = "old profile".into(),
                "document" => editor.doc = Some(editor.doc.as_ref().unwrap().fork()),
                _ => unreachable!(),
            }
            let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
            form.show(&ctx, &mut editor, &scene);
            assert_eq!(
                editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
                before,
                "{change}"
            );
            assert!(form.pending.is_none());
            assert!(form.waiting.is_none());
            if change == "document" {
                assert!(form.path.is_empty());
            } else {
                assert!(
                    form.message.starts_with("应用失败"),
                    "{change}: {}",
                    form.message
                );
            }
            drop(editor);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
