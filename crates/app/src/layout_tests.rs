//! Exercise the production forms without a native window or game installation.
use super::*;
use crate::{editing::tests::terrain_scene, ui_theme as theme};
use egui_kittest::kittest::Queryable as _;

fn setup(ctx: &egui::Context) {
    theme::install(ctx);
    ctx.add_font_provider(Arc::new(
        egui_system_font_provider::SystemFontProvider::new(),
    ));
}

fn view() -> (MapView, PathBuf) {
    let (editor, ctx, root) = crate::editing::tests::editor();
    let view = MapView {
        workbench: Default::default(),
        p2: Default::default(),
        ui_smoke: Default::default(),
        p2_playtest: false,
        editor,
        pending: None,
        leave_after_save: false,
        allow_close: false,
        loader: MapLoader::new(ctx).unwrap(),
        generation: 0,
        loading: false,
        error: None,
        name: String::new(),
        scene: None,
        gpu: None,
        target: None,
        zoom: 1.,
        center: egui::Pos2::ZERO,
        fit: true,
        selected: None,
        hover: None,
        status: String::new(),
        verified: false,
        screenshot_requested: false,
        started: Instant::now(),
        stage: "",
        timings: Default::default(),
        loading_ticks: 0,
        last_loading_tick: None,
        max_loading_gap_ms: 0.,
        max_poll_ms: 0.,
        loading_ui_frames: 0,
        directory: PathBuf::new(),
        smoke_loads: Vec::new(),
    };
    (view, root)
}

#[test]
fn creation_dialog_layout_stays_compact_and_actions_visible_after_resizing() {
    for random in [false, true] {
        let (mut view, root) = view();
        view.workbench.new_map = !random;
        view.workbench.random_map = random;
        let mut directory = String::from("D:/YR");
        let mut harness = crate::test_support::harness()
            .with_context_setup(setup)
            .with_accessibility_check(false)
            .with_size([1280., 800.])
            .with_pixels_per_point(1.25)
            .build_ui(|ui| {
                view.settings_dialogs(ui.ctx(), &mut directory);
            });
        // Reuse the same egui memory, including a previously larger modal.
        for size in [[1280., 800.], [900., 600.], [2048., 1252.8], [900., 600.]] {
            harness.set_size(size.into());
            harness.run();
            let id = if random { "random-map" } else { "new-map" };
            let rect = harness
                .ctx
                .memory(|m| m.area_rect(egui::Id::unique(id)))
                .unwrap();
            let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, size.into());
            assert!(
                bounds.contains_rect(rect),
                "{id}: {rect:?} outside {bounds:?}"
            );
            let action = harness
                .get_by_label(if random {
                    "生成地图"
                } else {
                    "创建地图"
                })
                .rect();
            assert!(rect.contains_rect(action), "{id}: action outside dialog");
            assert!(
                rect.bottom() - action.bottom() < 20.,
                "{id}: blank footer: {rect:?} / {action:?}"
            );
            if size[1] >= 800. {
                let limit = if random { 850. } else { 550. };
                assert!(rect.height() < limit, "{id}: excessive height: {rect:?}");
            }
            #[cfg(feature = "ui-test-video")]
            {
                let output =
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-layout-audit");
                std::fs::create_dir_all(&output).unwrap();
                harness
                    .render()
                    .unwrap()
                    .save(output.join(format!("{id}-{}.png", size[0])))
                    .unwrap();
            }
        }
        harness.get_by_label("取消").click();
        harness.run();
        drop(harness);
        assert!(!view.workbench.new_map && !view.workbench.random_map);
        drop(view);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn information_rows_do_not_consume_the_panel_height() {
    let end = std::cell::Cell::new(egui::Rect::NOTHING);
    let mut harness = crate::test_support::harness()
        .with_context_setup(setup)
        .with_size([900., 1200.])
        .build_ui(|ui| {
            theme::section(ui, "地图信息", "说明");
            theme::field(ui, "地图尺寸", "80 × 80");
            theme::right_to_left(ui, |ui| {
                let _ = ui.button("取消");
            });
            end.set(ui.label("下一节").rect);
        });
    harness.run();
    assert!(end.get().bottom() < 160., "rows stretched: {:?}", end.get());
}

#[test]
fn map_settings_tabs_stay_inside_the_viewport_without_document_writes() {
    let (mut editor, _ctx, root) = crate::editing::tests::editor();
    let scene = terrain_scene();
    editor.attach(&scene);
    let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    let mut settings = crate::map_settings_ui::MapSettingsUi::default();
    settings.open = true;
    for tab in 0..3 {
        settings.tab = tab;
        let mut harness = crate::test_support::harness()
            .with_context_setup(setup)
            .with_accessibility_check(false)
            .with_size([1280., 800.])
            .build_ui(|ui| settings.show(ui.ctx(), &mut editor, &scene));
        for size in [[1280., 800.], [900., 600.]] {
            harness.set_size(size.into());
            harness.run();
            let rect = harness
                .ctx
                .memory(|m| m.area_rect(egui::Id::unique("map-properties")))
                .unwrap();
            assert!(
                egui::Rect::from_min_size(egui::Pos2::ZERO, size.into()).contains_rect(rect),
                "tab {tab}: {rect:?}"
            );
        }
    }
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        before
    );
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn all_workbench_panel_layouts_fit_desktop_and_compact_viewports() {
    use super::ui_smoke::{Panel, panels};
    use crate::editing::Tool;
    use ra2_map_document::mission::{self, Kind};

    for (name, panel) in panels() {
        let (mut view, root) = view();
        let scene = terrain_scene();
        view.editor.attach(&scene);
        mission::seed(view.editor.doc.as_mut().unwrap()).unwrap();
        match panel {
            Panel::Library(tab) => view.editor.show_library(tab, "", ""),
            Panel::Terrain(mode) => view.editor.activate_terrain_mode(mode),
            Panel::Tool(tool) => view.editor.tool = tool,
            Panel::Object => {
                let id = view
                    .editor
                    .doc
                    .as_mut()
                    .unwrap()
                    .add_record("Structures", [18, 20], "Neutral", "GAPOWR")
                    .unwrap();
                view.editor.commit(&scene, "fixture building");
                view.editor.select(Some(id));
            }
            Panel::Region => view.editor.tool = Tool::Region,
            Panel::Settings => view.workbench.settings = true,
            Panel::New => view.workbench.new_map = true,
            Panel::Random => view.workbench.random_map = true,
            Panel::Unsaved => view.pending = Some(Leave::Close),
            Panel::Help => view.workbench.help = true,
            Panel::Playtest => view.workbench.playtest = true,
            Panel::Campaign => view
                .workbench
                .mission
                .campaign
                .smoke_open(&root.join("test.map")),
            Panel::MapProperties(tab) => {
                view.workbench.map_settings.open = true;
                view.workbench.map_settings.tab = tab;
            }
            Panel::Mission(kind) => {
                let id = mission::ids(view.editor.doc.as_ref().unwrap(), kind)
                    .first()
                    .cloned()
                    .unwrap_or_default();
                view.workbench.mission.navigate(&view.editor, kind, id);
            }
            Panel::BaseNodes => {
                let house = mission::ids(view.editor.doc.as_ref().unwrap(), Kind::House)[0].clone();
                view.workbench
                    .mission
                    .navigate(&view.editor, Kind::House, house.clone());
                view.workbench.mission.base.start(house);
            }
        }
        view.scene = Some(scene);
        let before = view
            .editor
            .doc
            .as_ref()
            .unwrap()
            .snapshot()
            .bytes()
            .unwrap();
        let canvas = std::cell::Cell::new(egui::Rect::NOTHING);
        let mut directory = String::from("D:/YR");
        let mut harness = crate::test_support::harness()
            .with_context_setup(setup)
            .with_accessibility_check(false)
            .with_max_steps(12)
            .with_size([1280., 800.])
            .build_ui(|ui| {
                egui::Panel::top("actions").show(ui, |ui| {
                    ui.horizontal(|ui| view.toolbar(ui, &directory));
                    view.quick_tools(ui);
                });
                egui::Panel::bottom("status").show(ui, |ui| view.status_bar(ui));
                egui::Panel::left("library")
                    .exact_size(248.)
                    .show(ui, |ui| view.toolbox(ui));
                egui::Panel::right("inspector")
                    .exact_size(218.)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| view.inspector(ui));
                    });
                egui::CentralPanel::default().show(ui, |ui| canvas.set(ui.max_rect()));
                view.settings_dialogs(ui.ctx(), &mut directory);
                view.dialogs(ui.ctx());
            });
        for size in [[1280., 800.], [900., 600.]] {
            harness.set_size(size.into());
            harness.run();
            let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, size.into());
            assert!(
                bounds.contains_rect(canvas.get())
                    && canvas.get().width() > 200.
                    && canvas.get().height() > 200.,
                "{name}: canvas squeezed: {:?}",
                canvas.get()
            );
            let id = match panel {
                Panel::Settings => Some("project-settings"),
                Panel::New => Some("new-map"),
                Panel::Random => Some("random-map"),
                Panel::Unsaved => Some("unsaved-map"),
                Panel::Help => Some("interaction-help"),
                Panel::Playtest => Some("playtest-settings"),
                Panel::Mission(_) => Some("mission-authoring"),
                Panel::Campaign => Some("campaign-project"),
                Panel::BaseNodes => Some("base-nodes-canvas"),
                Panel::MapProperties(_) => Some("map-properties"),
                _ => None,
            };
            if let Some(id) = id {
                let rect = harness
                    .ctx
                    .memory(|m| m.area_rect(egui::Id::unique(id)))
                    .unwrap();
                assert!(
                    bounds.expand(1.).contains_rect(rect),
                    "{name}: {rect:?} outside {bounds:?}"
                );
            }
            #[cfg(feature = "ui-test-video")]
            {
                let output =
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-layout-audit");
                std::fs::create_dir_all(&output).unwrap();
                harness
                    .render()
                    .unwrap()
                    .save(output.join(format!("{name}-{}.png", size[0])))
                    .unwrap();
            }
        }
        drop(harness);
        assert_eq!(
            view.editor
                .doc
                .as_ref()
                .unwrap()
                .snapshot()
                .bytes()
                .unwrap(),
            before,
            "{name}: document changed"
        );
        drop(view);
        std::fs::remove_dir_all(root).unwrap();
    }
}
