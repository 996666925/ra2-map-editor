mod watch;

use crate::{editing::Editor, i18n::tr};
use anyhow::{Context, Result};
use eframe::egui;
use ra2_map_document::document::{MapDocument, PreparedReplacement};
use ra2_map_io::{
    mission_project::{self, BuiltModule, ProjectKind},
    wasm_mission::{self, ModuleKind},
};
use ra2_map_scene::map::MapScene;
use std::{
    path::{Component, Path, PathBuf},
    sync::{Arc, mpsc},
};

use watch::{Source, SourceWatch};

struct PreparedModule {
    replacement: PreparedReplacement,
    sources: Vec<(ModuleKind, Option<Source>)>,
}

enum ModuleUpdate {
    Import(PathBuf),
    Reload(Source),
    Remove,
}

struct EmbeddedModules {
    revision: (u64, u64),
    modules: [Result<Option<Vec<u8>>>; 2],
}

#[derive(Default)]
pub(super) struct WasmMissionUi {
    pub(super) open: bool,
    message: String,
    creating_project: bool,
    project_name: String,
    project_parent: Option<PathBuf>,
    project_message: String,
    selected_project: Option<PathBuf>,
    selected_project_kind: Option<ProjectKind>,
    build_worker: Option<mpsc::Receiver<Result<Vec<BuiltModule>>>>,
    embed_after_build: Option<u64>,
    embedded: Option<EmbeddedModules>,
    worker: Option<mpsc::Receiver<Result<PreparedModule>>>,
    sources: SourceWatch,
    reimporting: bool,
    reload_error: String,
}

impl WasmMissionUi {
    pub(super) fn show(
        &mut self,
        ctx: &egui::Context,
        editor: &mut Editor,
        scene: Option<&MapScene>,
    ) {
        self.poll(editor, scene);
        self.poll_build(ctx, editor, scene);
        self.sources
            .sync_document(editor.doc.as_ref().filter(|_| scene.is_some()));
        self.sources.poll(ctx);
        if !self.open {
            self.reimport_prompt(ctx, editor, scene);
            return;
        }
        let mut open = self.open;
        crate::ui_theme::window(ctx, tr!("扩展系统"), "wasm-mission", egui::vec2(480., 320.))
            .open(&mut open)
            .vscroll(true)
            .show(ctx, |ui| {
                self.project_controls(ui, ctx, editor, scene);
                ui.separator();
                ui.strong(tr!("地图扩展"));
                let Some(doc) = editor.doc.as_ref().filter(|_| scene.is_some()) else {
                    ui.weak(tr!("未打开地图"));
                    return;
                };
                let revision = (doc.id, doc.edit_revision());
                if self
                    .embedded
                    .as_ref()
                    .is_none_or(|entry| entry.revision != revision)
                {
                    let bytes = doc.snapshot().bytes();
                    let modules = [ModuleKind::Rules, ModuleKind::Ui].map(|kind| match &bytes {
                        Ok(bytes) => wasm_mission::extract_kind(kind, bytes),
                        Err(error) => Err(anyhow::anyhow!("{error:#}")),
                    });
                    self.embedded = Some(EmbeddedModules { revision, modules });
                }
                let mut update = None;
                let can_update = self.worker.is_none() && self.embed_after_build.is_none();
                for (kind, embedded) in [ModuleKind::Rules, ModuleKind::Ui]
                    .into_iter()
                    .zip(&self.embedded.as_ref().unwrap().modules)
                {
                    ui.push_id(kind.as_str(), |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.monospace(kind.file_name());
                            match embedded {
                                Ok(Some(module)) => {
                                    ui.label(tr!("{arg0} 字节", arg0 = module.len()));
                                }
                                Ok(None) => {
                                    ui.weak(tr!("未嵌入扩展"));
                                }
                                Err(_) => {}
                            }
                            ui.add_enabled_ui(can_update, |ui| {
                                if ui.button(tr!("导入…")).clicked()
                                    && let Some(path) = rfd::FileDialog::new()
                                        .add_filter("WebAssembly", &["wasm"])
                                        .pick_file()
                                {
                                    update = Some((kind, Some(path)));
                                }
                                if let Ok(Some(module)) = embedded
                                    && ui.button(tr!("导出…")).clicked()
                                    && let Some(path) = rfd::FileDialog::new()
                                        .add_filter("WebAssembly", &["wasm"])
                                        .set_file_name(kind.file_name())
                                        .save_file()
                                {
                                    self.message = match std::fs::write(path, module) {
                                        Ok(()) => tr!("已导出 {arg0}", arg0 = kind.file_name()),
                                        Err(error) => error.to_string(),
                                    };
                                }
                                if !matches!(embedded, Ok(None)) && ui.button(tr!("移除")).clicked()
                                {
                                    update = Some((kind, None));
                                }
                            });
                        });
                        if let Err(error) = embedded {
                            ui.label(format!("{error:#}"));
                        }
                    });
                }
                if let Some((kind, path)) = update {
                    self.begin(ctx, doc, kind, path);
                }
                if !self.message.is_empty() {
                    ui.label(&self.message);
                }
            });
        self.open = open;
        self.reimport_prompt(ctx, editor, scene);
    }

    fn reimport_prompt(&mut self, ctx: &egui::Context, editor: &Editor, scene: Option<&MapScene>) {
        if self.worker.is_some() || self.embed_after_build.is_some() {
            return;
        }
        let Some(doc) = editor.doc.as_ref().filter(|_| scene.is_some()) else {
            return;
        };
        let Some(source) = self.sources.pending() else {
            return;
        };
        let mut reload = false;
        let mut dismiss = false;
        let modal = crate::ui_theme::modal(ctx, "wasm-source-changed", |ui| {
            ui.set_max_width((ctx.content_rect().width() - 48.).clamp(120., 440.));
            ui.heading(tr!("扩展文件已修改"));
            ui.label(tr!(
                "{arg0} 已修改，是否重新导入？",
                arg0 = source.kind.file_name()
            ));
            ui.add(egui::Label::new(source.path.display().to_string()).wrap());
            if !self.reload_error.is_empty() {
                ui.label(&self.reload_error);
            }
            ui.horizontal(|ui| {
                reload = ui.button(tr!("重新导入")).clicked();
                dismiss = ui.button(tr!("暂不导入")).clicked();
            });
        });
        if reload {
            // Import precisely the stable version the user confirmed. Later writes are detected again.
            self.begin_update(ctx, doc, source.kind, ModuleUpdate::Reload(source));
        } else if dismiss || modal.should_close() {
            self.sources.dismiss(source.kind);
            self.reload_error.clear();
        }
    }

    fn project_controls(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        editor: &Editor,
        scene: Option<&MapScene>,
    ) {
        ui.strong(tr!("项目"));
        let embed_target = editor
            .doc
            .as_ref()
            .filter(|_| scene.is_some())
            .map(|doc| doc.id);
        ui.add_enabled_ui(self.build_worker.is_none(), |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add(egui::Button::new(tr!("新建项目…")).selected(self.creating_project))
                    .clicked()
                {
                    self.creating_project = !self.creating_project;
                }
                if ui.button(tr!("打开项目…")).clicked()
                    && let Some(path) = rfd::FileDialog::new().pick_folder()
                {
                    match mission_project::project_type(&path) {
                        Ok(kind) => {
                            self.selected_project = Some(path);
                            self.selected_project_kind = Some(kind);
                            self.creating_project = false;
                            self.project_message.clear();
                        }
                        Err(error) => {
                            self.project_message =
                                tr!("无法打开项目：{arg0}", arg0 = format!("{error:#}"));
                        }
                    }
                }
                let selected = self.selected_project.clone();
                if ui
                    .add_enabled(selected.is_some(), egui::Button::new(tr!("构建")))
                    .clicked()
                    && let Some(path) = selected.as_ref()
                {
                    self.begin_build(ctx, path.clone(), None);
                }
                if ui
                    .add_enabled(
                        selected.is_some() && embed_target.is_some() && self.worker.is_none(),
                        egui::Button::new(tr!("构建并嵌入")),
                    )
                    .clicked()
                    && let Some(path) = selected
                {
                    self.begin_build(ctx, path, embed_target);
                }
            });
            if self.creating_project {
                ui.group(|ui| self.create_project_controls(ui));
            }
        });
        if let Some(path) = &self.selected_project {
            ui.add(
                egui::Label::new(
                    path.file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy(),
                )
                .truncate(),
            )
            .on_hover_text(path.display().to_string());
            if let Some(kind) = self.selected_project_kind {
                for module in kind.modules() {
                    ui.monospace(module.file_name());
                }
            }
        } else if !self.creating_project {
            ui.weak(tr!("未选择项目"));
        }
        if !self.project_message.is_empty() {
            ui.label(&self.project_message);
        }
    }

    fn create_project_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let name_label = ui.label(tr!("项目名称"));
            ui.add(egui::TextEdit::singleline(&mut self.project_name).desired_width(180.))
                .labelled_by(name_label.id);
        });
        ui.horizontal(|ui| {
            if ui.button(tr!("选择位置…")).clicked() {
                let mut dialog = rfd::FileDialog::new();
                if let Some(parent) = &self.project_parent {
                    dialog = dialog.set_directory(parent);
                }
                if let Some(parent) = dialog.pick_folder() {
                    self.project_parent = Some(parent);
                    self.project_message.clear();
                }
            }
            if let Some(parent) = &self.project_parent {
                ui.add(egui::Label::new(parent.display().to_string()).truncate())
                    .on_hover_text(parent.display().to_string());
            }
        });
        let name = self.project_name.trim();
        let valid_name = valid_project_name(name);
        let target = self
            .project_parent
            .as_ref()
            .filter(|_| valid_name)
            .map(|parent| parent.join(name));
        if !name.is_empty() && !valid_name {
            ui.label(tr!("请输入有效的项目目录名称。"));
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(target.is_some(), egui::Button::new(tr!("创建项目")))
                .clicked()
                && let Some(target) = target
            {
                self.project_message = match mission_project::create_project(&target) {
                    Ok(()) => {
                        self.selected_project = Some(target);
                        self.selected_project_kind = Some(ProjectKind::Extension);
                        self.creating_project = false;
                        tr!("项目已创建，请在项目目录执行 npm install。").to_owned()
                    }
                    Err(error) => tr!("无法创建项目：{arg0}", arg0 = format!("{error:#}")),
                };
            }
            if ui.button(tr!("取消")).clicked() {
                self.creating_project = false;
            }
        });
    }

    fn begin_build(&mut self, ctx: &egui::Context, path: PathBuf, embed_after_build: Option<u64>) {
        let (send, receive) = mpsc::channel();
        self.build_worker = Some(receive);
        self.embed_after_build = embed_after_build;
        self.project_message = tr!("正在构建…").to_owned();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = send.send(mission_project::build_modules(&path));
            ctx.request_repaint();
        });
    }

    fn poll_build(&mut self, ctx: &egui::Context, editor: &Editor, scene: Option<&MapScene>) {
        if self.embed_after_build.is_some() && self.worker.is_some() {
            return;
        }
        let Some(worker) = &self.build_worker else {
            return;
        };
        let result = match worker.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(error) => Err(error.into()),
        };
        self.build_worker = None;
        let embed_after_build = std::mem::take(&mut self.embed_after_build);
        match result {
            Ok(modules) if !modules.is_empty() => {
                self.selected_project_kind = Some(if modules.len() == 2 {
                    ProjectKind::Extension
                } else {
                    modules[0].kind.into()
                });
                self.project_message = tr!("构建完成").to_owned();
                if let Some(document_id) = embed_after_build {
                    if let Some(doc) = editor.doc.as_ref().filter(|doc| doc.id == document_id)
                        && scene.is_some()
                    {
                        self.begin_updates(
                            ctx,
                            doc,
                            modules
                                .into_iter()
                                .map(|module| (module.kind, ModuleUpdate::Import(module.path)))
                                .collect(),
                        );
                    } else {
                        self.project_message = tr!("构建完成，但目标地图已切换或关闭。").to_owned();
                    }
                }
            }
            Ok(_) => {
                self.project_message =
                    tr!("构建失败：{arg0}", arg0 = "No extension modules produced");
            }
            Err(error) => {
                self.project_message = tr!("构建失败：{arg0}", arg0 = format!("{error:#}"));
            }
        }
    }

    fn begin(
        &mut self,
        ctx: &egui::Context,
        doc: &MapDocument,
        kind: ModuleKind,
        path: Option<PathBuf>,
    ) {
        self.begin_update(
            ctx,
            doc,
            kind,
            path.map_or(ModuleUpdate::Remove, ModuleUpdate::Import),
        );
    }

    fn begin_update(
        &mut self,
        ctx: &egui::Context,
        doc: &MapDocument,
        kind: ModuleKind,
        update: ModuleUpdate,
    ) {
        self.begin_updates(ctx, doc, vec![(kind, update)]);
    }

    fn begin_updates(
        &mut self,
        ctx: &egui::Context,
        doc: &MapDocument,
        updates: Vec<(ModuleKind, ModuleUpdate)>,
    ) {
        if self.worker.is_some() || self.embed_after_build.is_some() {
            return;
        }
        let snapshot = doc.snapshot();
        let revision = doc.edit_revision();
        let ctx = ctx.clone();
        let (send, recv) = mpsc::channel();
        self.worker = Some(recv);
        self.reimporting = updates
            .iter()
            .any(|(_, update)| matches!(update, ModuleUpdate::Reload(_)));
        self.reload_error.clear();
        self.message = tr!("正在更新扩展…").to_owned();
        std::thread::spawn(move || {
            let result = (|| {
                let mut sources = Vec::new();
                for (kind, update) in updates {
                    let source = match update {
                        ModuleUpdate::Import(path) => {
                            let bytes: Arc<[u8]> = wasm_mission::read_module(&path)?.into();
                            Some(Source { kind, path, bytes })
                        }
                        ModuleUpdate::Reload(source) => Some(source),
                        ModuleUpdate::Remove => None,
                    };
                    sources.push((kind, source));
                }
                let modules: Vec<_> = sources
                    .iter()
                    .map(|(kind, source)| {
                        (*kind, source.as_ref().map(|source| source.bytes.as_ref()))
                    })
                    .collect();
                let replacement = wasm_mission::prepare_modules(snapshot, revision, &modules)?;
                Ok(PreparedModule {
                    replacement,
                    sources,
                })
            })();
            let _ = send.send(result);
            ctx.request_repaint();
        });
    }

    fn poll(&mut self, editor: &mut Editor, scene: Option<&MapScene>) {
        let Some(worker) = &self.worker else { return };
        // Keep the prepared result queued until property input is resolved. A
        // structural replacement clears the selection and its UI-only draft.
        if scene.is_some() && editor.doc.is_some() && !editor.check_property_draft() {
            self.message.clone_from(&editor.status);
            return;
        }
        let result = match worker.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(error) => Err(error.into()),
        };
        self.worker = None;
        let result = result.and_then(|prepared| {
            let scene = scene.context(tr!("缺少地图"))?;
            let doc = editor.doc.as_mut().context(tr!("缺少地图"))?;
            let changed = doc.replace_prepared(prepared.replacement, "Map extension")?;
            for (kind, source) in prepared.sources {
                self.sources.imported(doc, kind, source);
            }
            if changed {
                editor.structural_changed(scene);
            }
            Ok(changed)
        });
        if std::mem::take(&mut self.reimporting)
            && let Err(error) = &result
        {
            self.reload_error = format!("{error:#}");
        }
        self.message = match result {
            Ok(true) => tr!("扩展已更新").to_owned(),
            Ok(false) => tr!("扩展未变化").to_owned(),
            Err(error) => {
                let message = format!("{error:#}");
                // A committed property correction can make the queued snapshot
                // stale. Report that safe rejection even with the panel closed.
                editor.status.clone_from(&message);
                message
            }
        };
    }
}

fn valid_project_name(name: &str) -> bool {
    // Validate a single portable directory component before joining it to the selected parent.
    if name.ends_with('.')
        || name
            .chars()
            .any(|ch| ch.is_control() || "<>:\"/\\|?*".contains(ch))
    {
        return false;
    }
    let mut parts = Path::new(name).components();
    if !matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    ) {
        return false;
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::{NodeT as _, Queryable as _};

    #[test]
    fn wasm_panel_creates_an_extension_project_without_an_open_map() {
        let _language = crate::i18n::enter(crate::i18n::Language::English);
        let parent = tempfile::tempdir().unwrap();
        let state = std::cell::RefCell::new(WasmMissionUi {
            open: true,
            project_name: "Test Mission".into(),
            project_parent: Some(parent.path().to_owned()),
            ..Default::default()
        });
        let mut editor = Editor::new(egui::Context::default(), parent.path().to_owned()).unwrap();
        let mut harness = crate::test_support::harness()
            .with_size([800., 600.])
            .with_accessibility_check(false)
            .build_ui(|ui| state.borrow_mut().show(ui.ctx(), &mut editor, None));
        harness.run();
        assert!(harness.query_by_label("Extension System").is_some());
        assert!(harness.query_by_label("Project name").is_none());
        assert!(harness.get_by_label("Build").accesskit_node().is_disabled());
        assert!(
            harness
                .get_by_label("Build and embed")
                .accesskit_node()
                .is_disabled()
        );
        assert!(harness.query_by_label("No map open").is_some());
        harness.get_by_label("New project…").click();
        harness.run();
        assert!(harness.query_by_label("Module type").is_none());
        assert!(harness.query_by_label("Rules + UI").is_none());
        harness.get_by_label("Create project").click();
        harness.run();
        assert!(harness.query_by_label("Project name").is_none());
        assert!(!harness.get_by_label("Build").accesskit_node().is_disabled());
        assert!(
            harness
                .get_by_label("Build and embed")
                .accesskit_node()
                .is_disabled()
        );
        assert!(harness.query_by_label("Test Mission").is_some());
        let project = parent.path().join("Test Mission");
        assert!(project.join("rules/src/main.ts").is_file());
        assert!(project.join("ui/src/main.ts").is_file());
        assert_eq!(
            mission_project::project_type(&project).unwrap(),
            ProjectKind::Extension
        );
        let package: serde_json::Value =
            serde_json::from_slice(&std::fs::read(project.join("package.json")).unwrap()).unwrap();
        assert_eq!(
            package["dependencies"]["@ra2-map-editor/mission-sdk"],
            "0.1.3"
        );
        assert_eq!(package["vera"], serde_json::json!({"kind": "extension"}));
        assert!(!project.join("vera-mission.json").exists());
        assert!(!project.join("rules/vera-mission.json").exists());
        assert!(!project.join("ui/vera-mission.json").exists());
        assert!(!project.join("vera.json").exists());
        assert!(!project.join("vendor").exists());
        assert_eq!(package["scripts"]["build"], "ra2-mission-project .");
        assert_eq!(
            state.borrow().selected_project_kind,
            Some(ProjectKind::Extension)
        );
        assert_eq!(
            state.borrow().selected_project.as_deref(),
            Some(project.as_path())
        );
    }

    #[test]
    fn completed_build_keeps_its_original_map_target() {
        let _language = crate::i18n::enter(crate::i18n::Language::English);
        let (mut editor, ctx, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let original_id = editor.doc.as_ref().unwrap().id;
        let (send, receive) = mpsc::channel();
        let mut state = WasmMissionUi {
            build_worker: Some(receive),
            embed_after_build: Some(original_id),
            ..Default::default()
        };
        editor.doc = Some(ra2_map_document::test_support::document());
        let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        send.send(Ok(vec![BuiltModule {
            kind: ModuleKind::Rules,
            path: root.join("rules.wasm"),
        }]))
        .unwrap();
        state.poll_build(&ctx, &editor, Some(&scene));
        assert!(state.worker.is_none());
        assert!(state.build_worker.is_none());
        assert!(state.embed_after_build.is_none());
        assert_eq!(
            state.project_message,
            "Build complete, but the target map was switched or closed."
        );
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            before
        );
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn completed_build_preserves_edits_to_the_same_map() {
        check_completed_build(ModuleKind::Rules, ModuleKind::Ui);
    }

    #[test]
    fn completed_ui_build_uses_its_kind_and_preserves_the_rules_module() {
        check_completed_build(ModuleKind::Ui, ModuleKind::Rules);
    }

    fn check_completed_build(kind: ModuleKind, other: ModuleKind) {
        let (mut editor, ctx, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let module = b"\0asm\x01\0\0\0";
        wasm_mission::apply_kind(other, editor.doc.as_mut().unwrap(), Some(module)).unwrap();
        let output = root.join(kind.file_name());
        std::fs::write(&output, module).unwrap();
        let (send, receive) = mpsc::channel();
        let mut state = WasmMissionUi {
            build_worker: Some(receive),
            embed_after_build: Some(editor.doc.as_ref().unwrap().id),
            ..Default::default()
        };
        let doc = editor.doc.as_mut().unwrap();
        doc.set_ini("Basic", "Name", Some(b"Edited while building".to_vec()))
            .unwrap();
        doc.commit("concurrent edit");
        send.send(Ok(vec![BuiltModule { kind, path: output }]))
            .unwrap();
        state.poll_build(&ctx, &editor, Some(&scene));
        assert!(state.worker.is_some());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while state.worker.is_some() && std::time::Instant::now() < deadline {
            state.poll(&mut editor, Some(&scene));
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(state.worker.is_none());
        let doc = editor.doc.as_mut().unwrap();
        assert_eq!(doc.ini_text("Basic", "Name"), "Edited while building");
        assert_eq!(
            wasm_mission::extract_kind(kind, &doc.snapshot().bytes().unwrap())
                .unwrap()
                .as_deref(),
            Some(module.as_slice())
        );
        assert_eq!(
            wasm_mission::extract_kind(other, &doc.snapshot().bytes().unwrap())
                .unwrap()
                .as_deref(),
            Some(module.as_slice())
        );
        assert!(doc.undo());
        assert!(
            wasm_mission::extract_kind(kind, &doc.snapshot().bytes().unwrap())
                .unwrap()
                .is_none()
        );
        assert_eq!(doc.ini_text("Basic", "Name"), "Edited while building");
        assert_eq!(
            wasm_mission::extract_kind(other, &doc.snapshot().bytes().unwrap())
                .unwrap()
                .as_deref(),
            Some(module.as_slice())
        );
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn wasm_panel_lists_both_modules_and_removes_only_the_selected_one() {
        let _language = crate::i18n::enter(crate::i18n::Language::English);
        let (mut editor, _, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let module = b"\0asm\x01\0\0\0";
        for kind in [ModuleKind::Rules, ModuleKind::Ui] {
            wasm_mission::apply_kind(kind, editor.doc.as_mut().unwrap(), Some(module)).unwrap();
        }
        let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        let state = std::cell::RefCell::new(WasmMissionUi {
            open: true,
            ..Default::default()
        });
        let mut harness = crate::test_support::harness()
            .with_size([800., 600.])
            .with_accessibility_check(false)
            .build_ui(|ui| state.borrow_mut().show(ui.ctx(), &mut editor, Some(&scene)));
        harness.run();
        assert!(harness.query_by_label("rules.wasm").is_some());
        assert!(harness.query_by_label("ui.wasm").is_some());
        assert_eq!(harness.get_all_by_label("Export…").count(), 2);
        let removes: Vec<_> = harness.get_all_by_label("Remove").collect();
        assert_eq!(removes.len(), 2);
        removes[1].click();
        harness.run();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while state.borrow().worker.is_some() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(2));
            harness.run();
        }
        assert!(state.borrow().worker.is_none());
        assert_eq!(harness.get_all_by_label("Export…").count(), 1);
        drop(harness);
        let doc = editor.doc.as_mut().unwrap();
        let after = doc.snapshot().bytes().unwrap();
        assert!(
            wasm_mission::extract_kind(ModuleKind::Ui, &after)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            wasm_mission::extract(&after).unwrap().as_deref(),
            Some(module.as_slice())
        );
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pending_extension_writes_cannot_replace_each_other() {
        let _language = crate::i18n::enter(crate::i18n::Language::English);
        let (mut editor, _, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        wasm_mission::apply(editor.doc.as_mut().unwrap(), Some(b"\0asm\x01\0\0\0")).unwrap();
        let (_build_send, build_receive) = mpsc::channel();
        let (_write_send, write_receive) = mpsc::channel();
        let state = std::cell::RefCell::new(WasmMissionUi {
            open: true,
            selected_project: Some(root.clone()),
            build_worker: Some(build_receive),
            embed_after_build: Some(editor.doc.as_ref().unwrap().id),
            ..Default::default()
        });
        let mut harness = crate::test_support::harness()
            .with_size([800., 600.])
            .with_accessibility_check(false)
            .build_ui(|ui| state.borrow_mut().show(ui.ctx(), &mut editor, Some(&scene)));
        harness.run();
        let imports: Vec<_> = harness.get_all_by_label("Import\u{2026}").collect();
        assert_eq!(imports.len(), 2);
        assert!(
            imports
                .iter()
                .all(|node| node.accesskit_node().is_disabled())
        );
        assert!(
            harness
                .get_by_label("Remove")
                .accesskit_node()
                .is_disabled()
        );
        {
            let mut state = state.borrow_mut();
            state.build_worker = None;
            state.embed_after_build = None;
            state.worker = Some(write_receive);
        }
        harness.run();
        assert!(
            harness
                .get_by_label("Build and embed")
                .accesskit_node()
                .is_disabled()
        );
        assert!(!harness.get_by_label("Build").accesskit_node().is_disabled());
        drop(harness);
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn wasm_panel_defaults_to_a_combined_project_without_an_open_map() {
        let _language = crate::i18n::enter(crate::i18n::Language::English);
        let parent = tempfile::tempdir().unwrap();
        let state = std::cell::RefCell::new(WasmMissionUi {
            open: true,
            project_name: "Combined Extension".into(),
            project_parent: Some(parent.path().to_owned()),
            ..Default::default()
        });
        let mut editor = Editor::new(egui::Context::default(), parent.path().to_owned()).unwrap();
        let mut harness = crate::test_support::harness()
            .with_size([800., 600.])
            .with_accessibility_check(false)
            .build_ui(|ui| state.borrow_mut().show(ui.ctx(), &mut editor, None));
        harness.run();
        harness.get_by_label("New project…").click();
        harness.run();
        harness.get_by_label("Create project").click();
        harness.run();
        let project = parent.path().join("Combined Extension");
        assert_eq!(
            mission_project::project_type(&project).unwrap(),
            ProjectKind::Extension
        );
        assert!(project.join("rules/src/main.ts").is_file());
        assert!(project.join("ui/src/main.ts").is_file());
        assert!(project.join("shared/protocol.ts").is_file());
        assert_eq!(
            state.borrow().selected_project_kind,
            Some(ProjectKind::Extension)
        );
        harness.get_by_label("rules.wasm");
        harness.get_by_label("ui.wasm");
    }

    fn select_unit_for_property_draft(editor: &mut Editor, scene: &MapScene) -> u64 {
        let doc = editor.doc.as_mut().unwrap();
        let id = doc
            .add_record("Units", [20, 20], "Neutral", "HTNK")
            .unwrap();
        doc.commit("unit for property input");
        editor.cache(scene);
        editor.select(Some(id));
        id
    }

    fn wait_for_prepared_extension(state: &mut WasmMissionUi) {
        // Make completion deterministic without applying it: put the real
        // worker's prepared result back in a ready channel for poll to consume.
        let prepared = state
            .worker
            .take()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .unwrap();
        let (send, receive) = mpsc::channel();
        assert!(send.send(Ok(prepared)).is_ok());
        state.worker = Some(receive);
    }

    fn initial_mission_input(
        editor: &mut Editor,
        scene: &MapScene,
        replacement: Option<&str>,
    ) -> String {
        let _language = crate::i18n::enter(crate::i18n::Language::English);
        let mut harness = crate::test_support::harness()
            .with_size([640., 1000.])
            .with_accessibility_check(false)
            .build_ui(|ui| editor.selection_properties(ui, scene));
        harness.run();
        if let Some(value) = replacement {
            harness
                .get_by_role_and_label(egui::accesskit::Role::TextInput, "Initial mission")
                .focus();
            harness.run();
            harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
            harness.run();
            harness
                .get_by_role_and_label(egui::accesskit::Role::TextInput, "Initial mission")
                .type_text(value);
            harness.run();
            harness.key_press(egui::Key::Enter);
            harness.run();
        }
        harness
            .get_by_role_and_label(egui::accesskit::Role::TextInput, "Initial mission")
            .value()
            .unwrap()
    }

    fn assert_prepared_extension_waits_for_invalid_property_draft(
        state: &mut WasmMissionUi,
        editor: &mut Editor,
        scene: &MapScene,
        ctx: &egui::Context,
    ) -> Vec<u8> {
        assert!(!state.open);
        let selected = editor.selected;
        let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        assert_eq!(
            initial_mission_input(editor, scene, Some("INVALID")),
            "INVALID"
        );
        editor.finish_property_for_save(ctx, scene);
        assert!(editor.object_properties_dirty());
        assert!(!editor.status.is_empty());
        let error = editor.status.clone();
        let reimporting = state.reimporting;
        for _ in 0..2 {
            editor.status.clear();
            state.poll(editor, Some(scene));
            assert!(state.worker.is_some(), "the ready result must stay queued");
            assert_eq!(state.reimporting, reimporting);
            assert_eq!(editor.selected, selected);
            assert!(editor.object_properties_dirty());
            assert_eq!(editor.status, error);
            assert_eq!(state.message, error);
            assert!(!editor.structural_refresh);
            assert_eq!(
                editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
                before
            );
        }
        assert_eq!(initial_mission_input(editor, scene, None), "INVALID");
        before
    }

    #[test]
    fn prepared_import_and_reimport_wait_for_invalid_property_drafts() {
        for kind in [ModuleKind::Rules, ModuleKind::Ui] {
            for reload in [false, true] {
                let (mut editor, ctx, root) = crate::editing::tests::editor();
                let scene = crate::editing::tests::terrain_scene();
                editor.attach(&scene);
                let id = select_unit_for_property_draft(&mut editor, &scene);
                let module = b"\0asm\x01\0\0\0";
                let path = root.join(kind.file_name());
                std::fs::write(&path, module).unwrap();
                let update = if reload {
                    ModuleUpdate::Reload(Source {
                        kind,
                        path,
                        bytes: module.as_slice().into(),
                    })
                } else {
                    ModuleUpdate::Import(path)
                };
                let mut state = WasmMissionUi::default();
                state.begin_update(&ctx, editor.doc.as_ref().unwrap(), kind, update);
                wait_for_prepared_extension(&mut state);
                let before = assert_prepared_extension_waits_for_invalid_property_draft(
                    &mut state,
                    &mut editor,
                    &scene,
                    &ctx,
                );
                editor.select(Some(id)); // Cancel the invalid input without editing the map.
                assert!(!editor.object_properties_dirty());
                state.poll(&mut editor, Some(&scene));
                assert!(state.worker.is_none());
                assert!(!state.reimporting);
                let doc = editor.doc.as_mut().unwrap();
                assert_eq!(
                    wasm_mission::extract_kind(kind, &doc.snapshot().bytes().unwrap())
                        .unwrap()
                        .as_deref(),
                    Some(module.as_slice())
                );
                assert!(doc.undo());
                assert_eq!(doc.snapshot().bytes().unwrap(), before);
                drop(state);
                drop(editor);
                std::fs::remove_dir_all(root).unwrap();
            }
        }
    }

    #[test]
    fn completed_build_waits_for_invalid_property_draft_then_embeds_after_cancel() {
        let (mut editor, ctx, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let id = select_unit_for_property_draft(&mut editor, &scene);
        let module = b"\0asm\x01\0\0\0";
        let modules = [ModuleKind::Rules, ModuleKind::Ui]
            .map(|kind| {
                let path = root.join(kind.file_name());
                std::fs::write(&path, module).unwrap();
                BuiltModule { kind, path }
            })
            .into_iter()
            .collect();
        let (send, receive) = mpsc::channel();
        let mut state = WasmMissionUi {
            build_worker: Some(receive),
            embed_after_build: Some(editor.doc.as_ref().unwrap().id),
            ..Default::default()
        };
        send.send(Ok(modules)).unwrap();
        state.poll_build(&ctx, &editor, Some(&scene));
        wait_for_prepared_extension(&mut state);
        let before = assert_prepared_extension_waits_for_invalid_property_draft(
            &mut state,
            &mut editor,
            &scene,
            &ctx,
        );
        editor.select(Some(id));
        state.poll(&mut editor, Some(&scene));
        assert!(state.worker.is_none());
        let doc = editor.doc.as_mut().unwrap();
        for kind in [ModuleKind::Rules, ModuleKind::Ui] {
            assert_eq!(
                wasm_mission::extract_kind(kind, &doc.snapshot().bytes().unwrap())
                    .unwrap()
                    .as_deref(),
                Some(module.as_slice())
            );
        }
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        drop(state);
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deferred_extension_rejects_a_committed_property_correction_and_reports_it() {
        let (mut editor, ctx, root) = crate::editing::tests::editor();
        let scene = crate::editing::tests::terrain_scene();
        editor.attach(&scene);
        let id = select_unit_for_property_draft(&mut editor, &scene);
        let path = root.join("rules.wasm");
        std::fs::write(&path, b"\0asm\x01\0\0\0").unwrap();
        let mut state = WasmMissionUi::default();
        state.begin(
            &ctx,
            editor.doc.as_ref().unwrap(),
            ModuleKind::Rules,
            Some(path),
        );
        wait_for_prepared_extension(&mut state);
        assert_prepared_extension_waits_for_invalid_property_draft(
            &mut state,
            &mut editor,
            &scene,
            &ctx,
        );
        editor.select(Some(id));
        assert_eq!(
            initial_mission_input(&mut editor, &scene, Some("Hunt")),
            "Hunt"
        );
        editor.finish_property_for_save(&ctx, &scene);
        assert!(!editor.object_properties_dirty());
        let corrected = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        editor.status.clear();
        state.poll(&mut editor, Some(&scene));
        assert!(state.worker.is_none());
        assert_eq!(editor.selected, Some(id));
        assert_eq!(editor.records[&id].fields()[6], "Hunt");
        assert!(!editor.structural_refresh);
        assert!(!state.message.is_empty());
        assert_eq!(editor.status, state.message);
        assert_eq!(
            editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
            corrected
        );
        assert!(wasm_mission::extract(&corrected).unwrap().is_none());
        drop(state);
        drop(editor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn combined_build_is_embedded_as_one_undoable_transaction_or_not_at_all() {
        for valid_ui in [true, false] {
            let (mut editor, ctx, root) = crate::editing::tests::editor();
            let scene = crate::editing::tests::terrain_scene();
            editor.attach(&scene);
            let doc = editor.doc.as_mut().unwrap();
            doc.set_ini("Basic", "Name", Some(b"Keep edits".to_vec()))
                .unwrap();
            doc.commit("user edit");
            let before = doc.snapshot().bytes().unwrap();
            let rules = root.join("rules.wasm");
            let ui = root.join("ui.wasm");
            let module = b"\0asm\x01\0\0\0";
            std::fs::write(&rules, module).unwrap();
            std::fs::write(
                &ui,
                if valid_ui {
                    module.as_slice()
                } else {
                    b"invalid"
                },
            )
            .unwrap();
            let (send, receive) = mpsc::channel();
            let mut state = WasmMissionUi {
                build_worker: Some(receive),
                embed_after_build: Some(editor.doc.as_ref().unwrap().id),
                ..Default::default()
            };
            send.send(Ok(vec![
                BuiltModule {
                    kind: ModuleKind::Rules,
                    path: rules,
                },
                BuiltModule {
                    kind: ModuleKind::Ui,
                    path: ui,
                },
            ]))
            .unwrap();
            state.poll_build(&ctx, &editor, Some(&scene));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while state.worker.is_some() && std::time::Instant::now() < deadline {
                state.poll(&mut editor, Some(&scene));
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            assert!(state.worker.is_none());
            let doc = editor.doc.as_mut().unwrap();
            assert_eq!(doc.ini_text("Basic", "Name"), "Keep edits");
            if valid_ui {
                for kind in [ModuleKind::Rules, ModuleKind::Ui] {
                    assert_eq!(
                        wasm_mission::extract_kind(kind, &doc.snapshot().bytes().unwrap())
                            .unwrap()
                            .as_deref(),
                        Some(module.as_slice())
                    );
                }
                assert!(doc.undo());
            }
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
            drop(editor);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
