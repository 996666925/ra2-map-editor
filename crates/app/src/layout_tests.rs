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
fn bridge_drag_in_canvas_keeps_tracking_pointer_as_preview_gets_longer() {
    let (mut view, root) = view();
    let fixture = ra2_map_assets::test_support::Fixture::with_authoring();
    let mut install = fixture.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    let scene = ra2_map_scene::map::load_scene_bytes(
        &mut install,
        "drag.map",
        "test".into(),
        crate::new_map::NewMapOptions {
            width: 80,
            height: 80,
            level: 4,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
        Instant::now(),
    )
    .unwrap();
    view.editor.attach(&scene);
    view.editor
        .activate_terrain_mode(ra2_map_scene::terrain_tools::TerrainMode::Bridge);
    view.editor.bridge_kind = ra2_map_scene::bridge_tools::BridgeKind::LowWood;
    view.scene = Some(scene.clone());
    view.center = egui::pos2(0., 1900.);
    view.zoom = 0.6;
    view.fit = false;
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200., 800.));
    let _ = crate::test_support::run_ui(
        &ctx,
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ui| view.canvas_inner(ui, None),
    );
    let mut counts = Vec::new();
    for frame in 0..5 {
        let at = match frame {
            0 | 1 => [60, 60],
            2 => [64, 60],
            _ => [84, 60],
        };
        let cell = scene.cells.iter().find(|c| [c.rx, c.ry] == at).unwrap();
        let rect = view.workbench.canvas_rect.unwrap();
        let camera = crate::camera::MapCamera {
            center: view.center,
            zoom: view.zoom,
        };
        let pos = rect.min
            + (scene.cell_rect(cell).unwrap().min + egui::vec2(1., 0.)
                - camera.world_rect(rect).min)
                * camera.zoom;
        let mut events = vec![egui::Event::PointerMoved(pos)];
        if frame == 1 || frame == 4 {
            events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: frame == 1,
                modifiers: Default::default(),
            });
        }
        let _ = crate::test_support::run_ui(
            &ctx,
            egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ui| view.canvas_inner(ui, None),
        );
        if frame == 2 || frame == 3 {
            let hit = view
                .hover
                .map(|n| [scene.cells[n].rx, scene.cells[n].ry])
                .unwrap();
            assert_eq!(hit, at);
            counts.push(
                view.editor
                    .plateau_preview_plan(&scene, hit)
                    .unwrap()
                    .overlays
                    .len(),
            );
        }
    }
    assert_eq!(counts, [15, 75]);
    assert_eq!(view.editor.doc.as_ref().unwrap().overlay([84, 60]).0, 92);
    drop(view);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn low_bridge_preview_stretches_over_open_water_before_reaching_the_far_bank() {
    for kind in [
        ra2_map_scene::bridge_tools::BridgeKind::LowWood,
        ra2_map_scene::bridge_tools::BridgeKind::LowConcrete,
    ] {
        let (mut view, root) = view();
        let fixture = ra2_map_assets::test_support::Fixture::with_joining();
        let mut install = fixture.install();
        install.catalog =
            ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
        let mut scene = ra2_map_scene::map::load_scene_bytes(
            &mut install,
            "river.map",
            "test".into(),
            crate::new_map::NewMapOptions {
                width: 80,
                height: 80,
                level: 4,
                ..Default::default()
            }
            .bytes()
            .unwrap(),
            Instant::now(),
        )
        .unwrap();
        let mut doc =
            crate::document::MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
        ra2_map_scene::shore_tools::plan(
            &doc,
            &scene,
            ra2_map_scene::cliff_tools::rectangle([70, 55], [104, 80]),
            Some(true),
        )
        .unwrap()
        .apply(&mut doc)
        .unwrap();
        doc.commit("river");
        scene = ra2_map_scene::map::load_scene_bytes(
            &mut install,
            "river.map",
            "test".into(),
            doc.snapshot().bytes().unwrap(),
            Instant::now(),
        )
        .unwrap();
        view.editor.attach(&scene);
        view.editor
            .activate_terrain_mode(ra2_map_scene::terrain_tools::TerrainMode::Bridge);
        view.editor.bridge_kind = kind;
        view.scene = Some(scene.clone());
        view.center = egui::pos2(540., 2200.);
        view.zoom = 0.6;
        view.fit = false;
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200., 800.));
        let _ = crate::test_support::run_ui(
            &ctx,
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| view.canvas_inner(ui, None),
        );
        let before = view
            .editor
            .doc
            .as_ref()
            .unwrap()
            .snapshot()
            .bytes()
            .unwrap();
        let mut ghost_counts = Vec::new();
        for frame in 0..7 {
            let at = match frame {
                0 | 1 | 5 => [69, 67],
                2 => [76, 67],
                3 | 4 => [90, 67],
                _ => [105, 67],
            };
            let cell = scene.cells.iter().find(|c| [c.rx, c.ry] == at).unwrap();
            let rect = view.workbench.canvas_rect.unwrap();
            let camera = crate::camera::MapCamera {
                center: view.center,
                zoom: view.zoom,
            };
            let pos = rect.min
                + (scene.cell_rect(cell).unwrap().min + egui::vec2(1., 0.)
                    - camera.world_rect(rect).min)
                    * camera.zoom;
            let mut events = vec![egui::Event::PointerMoved(pos)];
            if matches!(frame, 1 | 4 | 5 | 6) {
                events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: matches!(frame, 1 | 5),
                    modifiers: Default::default(),
                });
            }
            let output = crate::test_support::run_ui(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| view.canvas_inner(ui, None),
            );
            if frame == 2 || frame == 3 {
                assert!(
                    view.editor.plateau_preview_plan(&scene, at).is_err(),
                    "open water is not a valid final landing"
                );
                assert_eq!(
                    view.editor
                        .doc
                        .as_ref()
                        .unwrap()
                        .snapshot()
                        .bytes()
                        .unwrap(),
                    before
                );
                ghost_counts.push(output.shapes.iter().filter(|s|matches!(&s.shape,egui::Shape::Mesh(m) if m.texture_id!=egui::TextureId::default())).count());
            }
            if frame == 4 {
                assert_eq!(
                    view.editor
                        .doc
                        .as_ref()
                        .unwrap()
                        .snapshot()
                        .bytes()
                        .unwrap(),
                    before,
                    "releasing an unfinished crossing must not place the draft"
                );
                assert!(view.editor.status.starts_with("本笔未修改"));
            }
        }
        assert_eq!(
            ghost_counts,
            [24, 66],
            "both materials must keep drawing all three deck cells per row while dragging over water"
        );
        assert!(
            !view.editor.status.starts_with("本笔未修改"),
            "{}",
            view.editor.status
        );
        assert_ne!(
            view.editor
                .doc
                .as_ref()
                .unwrap()
                .snapshot()
                .bytes()
                .unwrap(),
            before
        );
        view.editor.history(false, &scene);
        assert_eq!(
            view.editor
                .doc
                .as_ref()
                .unwrap()
                .snapshot()
                .bytes()
                .unwrap(),
            before
        );
        drop(view);
        std::fs::remove_dir_all(root).unwrap();
    }
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
    for tab in 0..4 {
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
