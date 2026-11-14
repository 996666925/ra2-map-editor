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
#[ignore = "requires a GPU adapter; uses synthetic assets only"]
fn mission_refresh_preserves_gpu_and_viewport_bindings_through_history() {
    struct Wake(std::thread::Thread);
    impl std::task::Wake for Wake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let waker = std::task::Waker::from(Arc::new(Wake(std::thread::current())));
    let mut context = std::task::Context::from_waker(&waker);
    let config = egui_wgpu::WgpuConfiguration::default();
    let mut future = std::pin::pin!(egui_wgpu::RenderState::create(
        &config,
        &instance,
        None,
        Default::default()
    ));
    let state = loop {
        match std::future::Future::poll(future.as_mut(), &mut context) {
            std::task::Poll::Ready(result) => break result.expect("GPU adapter"),
            std::task::Poll::Pending => std::thread::park(),
        }
    };
    let fixture = crate::test_support::map_loader_fixture();
    let path = fixture.0.join("gpu.map");
    std::fs::write(
        &path,
        crate::new_map::NewMapOptions {
            width: 20,
            height: 20,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
    )
    .unwrap();
    let ctx = egui::Context::default();
    let (mut view, root) = view();
    view.loader = MapLoader::with_gpu(ctx.clone(), state.clone()).unwrap();
    view.open_now(fixture.0.clone(), path.to_string_lossy().into_owned());
    let wait = |view: &mut MapView| {
        let deadline = Instant::now() + Duration::from_secs(20);
        while view.loading || view.editor.refresh {
            view.poll(&ctx);
            assert!(Instant::now() < deadline, "preview worker timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(view.error.is_none(), "{:?}", view.error);
    };
    wait(&mut view);
    let gpu = view.gpu.as_ref().unwrap().clone();
    let target = gpu.target(&state.device, [128, 128]);
    view.target = Some(target.clone());
    view.editor
        .doc
        .as_mut()
        .unwrap()
        .set_ini("Events", "TRIGGER", Some(b"1,13,0,2".to_vec()))
        .unwrap();
    view.editor.commit(view.scene.as_ref().unwrap(), "event");
    for action in 0..3 {
        if action > 0 {
            view.editor
                .history(action == 2, view.scene.as_ref().unwrap());
        }
        wait(&mut view);
        assert!(view.timings.visual_reused);
        assert_eq!(view.timings.uploaded_instance_bytes, 0);
        assert_eq!(view.timings.gpu_prepare_ms, 0.);
        assert!(Arc::ptr_eq(view.gpu.as_ref().unwrap(), &gpu));
        assert!(Arc::ptr_eq(view.target.as_ref().unwrap(), &target));
    }
    view.editor
        .doc
        .as_mut()
        .unwrap()
        .set_ini("Lighting", "Ambient", Some(b"0.5".to_vec()))
        .unwrap();
    view.editor.commit(view.scene.as_ref().unwrap(), "lighting");
    wait(&mut view);
    assert!(!view.timings.visual_reused);
    assert!(!Arc::ptr_eq(view.gpu.as_ref().unwrap(), &gpu));
    assert!(view.target.is_none());
    drop(view);
    std::fs::remove_dir_all(root).unwrap();
}

fn flat_authoring_view() -> (MapView, MapScene, PathBuf) {
    let (mut view, root) = view();
    let fixture = ra2_map_assets::test_support::Fixture::with_authoring();
    let mut install = fixture.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    let scene = ra2_map_scene::map::load_scene_bytes(
        &mut install,
        "flat.map",
        "test".into(),
        crate::new_map::NewMapOptions {
            width: 40,
            height: 40,
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
        .activate_terrain_mode(ra2_map_scene::terrain_tools::TerrainMode::Plateau);
    view.editor.flat_view = true;
    view.center = crate::flat_view::position([35, 35]);
    view.zoom = 1.;
    view.fit = false;
    view.scene = Some(scene.clone());
    (view, scene, root)
}

fn flat_frame(
    view: &mut MapView,
    ctx: &egui::Context,
    at: Option<[u16; 2]>,
    button: Option<bool>,
    key: Option<egui::Key>,
) {
    let mut events = Vec::new();
    if let Some(at) = at {
        let rect = view.workbench.canvas_rect.unwrap();
        let camera = crate::camera::MapCamera {
            center: view.center,
            zoom: view.zoom,
        };
        let pos =
            rect.min + (crate::flat_view::position(at) - camera.world_rect(rect).min) * camera.zoom;
        events.push(egui::Event::PointerMoved(pos));
        if let Some(pressed) = button {
            events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
        }
    }
    if let Some(key) = key {
        events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        });
        events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Default::default(),
        });
    }
    crate::test_support::run_ui(
        ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200., 800.),
            )),
            events,
            ..Default::default()
        },
        |ui| view.canvas_inner(ui, None),
    );
}

#[test]
fn waypoint_canvas_click_move_cancel_history_and_reopen_preserve_identity() {
    use crate::editing::Tool;

    fn frame(view: &mut MapView, ctx: &egui::Context, events: Vec<egui::Event>) {
        crate::test_support::run_ui(
            ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200., 800.),
                )),
                events,
                ..Default::default()
            },
            |ui| view.canvas_inner(ui, None),
        );
    }
    fn button(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Default::default(),
        }
    }
    let (mut view, scene, root) = flat_authoring_view();
    view.editor.flat_view = false;
    view.editor.activate_tool(Tool::Select);
    view.editor.waypoint = 7; // Moving the clicked waypoint must ignore this palette index.
    let origin = [35, 35];
    let destination = [40, 37];
    let doc = view.editor.doc.as_mut().unwrap();
    doc.waypoint(42, origin).unwrap();
    doc.set_ini("ScriptTypes", "0", Some(b"route".to_vec()))
        .unwrap();
    doc.set_ini("route", "0", Some(b"3,42".to_vec())).unwrap();
    view.editor.commit(&scene, "fixture waypoint and route");
    let id = view
        .editor
        .records
        .iter()
        .find(|(_, r)| r.section == "Waypoints" && r.key == "42")
        .map(|(&id, _)| id)
        .unwrap();
    let before = view
        .editor
        .doc
        .as_ref()
        .unwrap()
        .snapshot()
        .bytes()
        .unwrap();
    let ctx = egui::Context::default();
    frame(&mut view, &ctx, vec![]);
    let rect = view.workbench.canvas_rect.unwrap();
    let camera = crate::camera::MapCamera {
        center: view.center,
        zoom: view.zoom,
    };
    let marker = view
        .editor
        .marker_cache
        .markers
        .iter()
        .find(|m| m.id == id)
        .unwrap();
    let start = rect.min + (marker.origin - camera.world_rect(rect).min) * camera.zoom;
    let cell = scene
        .cells
        .iter()
        .find(|c| [c.rx, c.ry] == destination)
        .unwrap();
    // The fixture's TMP is only a few pixels wide; use one of its visible texels.
    let target_world = scene.cell_rect(cell).unwrap().min + egui::vec2(1., 0.);
    assert_eq!(
        scene
            .pick(target_world)
            .map(|i| [scene.cells[i].rx, scene.cells[i].ry]),
        Some(destination)
    );
    let target = rect.min + (target_world - camera.world_rect(rect).min) * camera.zoom;

    // Cancel through both production input paths; hovering and release never write.
    for cancel_with_escape in [true, false] {
        frame(
            &mut view,
            &ctx,
            vec![
                egui::Event::PointerMoved(start),
                button(start, egui::PointerButton::Primary, true),
            ],
        );
        assert_eq!(view.editor.selected, Some(id));
        assert_eq!(view.editor.tool, Tool::Move);
        frame(
            &mut view,
            &ctx,
            vec![button(start, egui::PointerButton::Primary, false)],
        );
        frame(&mut view, &ctx, vec![egui::Event::PointerMoved(target)]);
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
        let cancel = if cancel_with_escape {
            vec![
                egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                },
                egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers: Default::default(),
                },
            ]
        } else {
            vec![
                button(target, egui::PointerButton::Secondary, true),
                button(target, egui::PointerButton::Secondary, false),
            ]
        };
        frame(&mut view, &ctx, cancel);
        assert_eq!(view.editor.tool, Tool::Select);
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
    }
    frame(
        &mut view,
        &ctx,
        vec![
            egui::Event::PointerMoved(start),
            button(start, egui::PointerButton::Primary, true),
        ],
    );
    frame(
        &mut view,
        &ctx,
        vec![button(start, egui::PointerButton::Primary, false)],
    );
    frame(&mut view, &ctx, vec![egui::Event::PointerMoved(target)]);
    frame(
        &mut view,
        &ctx,
        vec![button(target, egui::PointerButton::Primary, true)],
    );
    frame(
        &mut view,
        &ctx,
        vec![button(target, egui::PointerButton::Primary, false)],
    );
    assert_eq!(view.editor.tool, Tool::Select);
    assert_eq!(view.editor.selected, Some(id));
    assert_eq!(view.editor.records[&id].cell(), Some(destination));
    assert_eq!(view.editor.records[&id].key, "42");
    assert_eq!(view.editor.waypoint, 7);
    let after = view
        .editor
        .doc
        .as_ref()
        .unwrap()
        .snapshot()
        .bytes()
        .unwrap();
    assert_ne!(after, before);
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
    view.editor.history(true, &scene);
    assert_eq!(
        view.editor
            .doc
            .as_ref()
            .unwrap()
            .snapshot()
            .bytes()
            .unwrap(),
        after
    );
    let path = root.join("moved-waypoint.map");
    std::fs::write(&path, &after).unwrap();
    let (source, _) = crate::map::SourceMap::parse(
        "moved.map".into(),
        "test".into(),
        std::fs::read(path).unwrap(),
    )
    .unwrap();
    let reopened = crate::document::MapDocument::new(source.edit_base.unwrap());
    assert_eq!(reopened.ini_text("route", "0"), "3,42");
    assert_eq!(
        reopened
            .record_iter()
            .find(|(_, r)| r.section == "Waypoints" && r.key == "42")
            .unwrap()
            .1
            .cell(),
        Some(destination)
    );
    drop(view);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn flat_terrain_canvas_drag_preview_commit_and_history_share_exact_cells() {
    let (mut view, scene, root) = flat_authoring_view();
    let ctx = egui::Context::default();
    flat_frame(&mut view, &ctx, None, None, None);
    let before = view
        .editor
        .doc
        .as_ref()
        .unwrap()
        .snapshot()
        .bytes()
        .unwrap();
    flat_frame(&mut view, &ctx, Some([30, 30]), Some(true), None);
    flat_frame(&mut view, &ctx, Some([42, 42]), None, Some(egui::Key::F2));
    assert!(view.editor.flat_view, "a drag must not change projection");
    assert_eq!(
        view.hover.map(|i| [scene.cells[i].rx, scene.cells[i].ry]),
        Some([42, 42])
    );
    let plan = view.editor.plateau_preview_plan(&scene, [42, 42]).unwrap();
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
    flat_frame(&mut view, &ctx, Some([42, 42]), Some(false), None);
    assert!(
        !view.editor.status.starts_with("本笔未修改"),
        "{}",
        view.editor.status
    );
    for (&at, &value) in &plan.edits {
        assert_eq!(view.editor.doc.as_ref().unwrap().terrain(at), Some(value));
    }
    let after = view
        .editor
        .doc
        .as_ref()
        .unwrap()
        .snapshot()
        .bytes()
        .unwrap();
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
    view.editor.history(true, &scene);
    assert_eq!(
        view.editor
            .doc
            .as_ref()
            .unwrap()
            .snapshot()
            .bytes()
            .unwrap(),
        after
    );
    flat_frame(&mut view, &ctx, None, None, Some(egui::Key::F2));
    assert!(!view.editor.flat_view);
    flat_frame(&mut view, &ctx, None, None, Some(egui::Key::F2));
    assert!(view.editor.flat_view);
    assert_eq!(
        view.editor
            .doc
            .as_ref()
            .unwrap()
            .snapshot()
            .bytes()
            .unwrap(),
        after
    );
    flat_frame(&mut view, &ctx, Some([30, 30]), Some(true), None);
    flat_frame(
        &mut view,
        &ctx,
        Some([42, 42]),
        None,
        Some(egui::Key::Escape),
    );
    flat_frame(&mut view, &ctx, Some([42, 42]), Some(false), None);
    assert_eq!(
        view.editor
            .doc
            .as_ref()
            .unwrap()
            .snapshot()
            .bytes()
            .unwrap(),
        after
    );
    view.editor.tool = crate::editing::Tool::Region;
    flat_frame(&mut view, &ctx, None, None, None);
    assert!(
        !view.editor.flat_view,
        "region tools must use their own projection"
    );
    drop(view);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn flat_terrain_manual_cliff_uses_the_selected_subtile_and_base_height() {
    let (mut view, scene, root) = flat_authoring_view();
    view.editor
        .activate_terrain_mode(ra2_map_scene::terrain_tools::TerrainMode::Template);
    view.editor.brush = scene
        .brushes
        .iter()
        .position(|b| {
            b.key.sub_tile != 0
                && scene
                    .palette
                    .templates
                    .get(&(b.key.tile_id, 0))
                    .is_some_and(|t| t.cells.iter().flatten().any(|s| s.height > 0))
        })
        .unwrap();
    view.editor.template_level = Some(6);
    let at = [35, 35];
    let planned = ra2_map_scene::terrain_tools::template_placement(
        view.editor.doc.as_ref().unwrap(),
        &scene,
        &scene.brushes[view.editor.brush],
        at,
        Some(6),
    )
    .unwrap();
    let before = view
        .editor
        .doc
        .as_ref()
        .unwrap()
        .snapshot()
        .bytes()
        .unwrap();
    let ctx = egui::Context::default();
    flat_frame(&mut view, &ctx, None, None, None);
    flat_frame(&mut view, &ctx, Some(at), None, None);
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
    flat_frame(&mut view, &ctx, Some(at), Some(true), None);
    flat_frame(&mut view, &ctx, Some(at), Some(false), None);
    for (at, value) in planned {
        assert_eq!(view.editor.doc.as_ref().unwrap().terrain(at), Some(value));
    }
    let brush = scene.brushes[view.editor.brush].key;
    view.editor.tool = crate::editing::Tool::Pick;
    flat_frame(&mut view, &ctx, Some(at), Some(true), None);
    flat_frame(&mut view, &ctx, Some(at), Some(false), None);
    assert_eq!(scene.brushes[view.editor.brush].key.tile_id, brush.tile_id);
    assert_eq!(
        scene.brushes[view.editor.brush].key.sub_tile,
        brush.sub_tile
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

#[test]
fn flat_terrain_workspace_fits_compact_and_desktop_windows() {
    let (mut view, scene, root) = flat_authoring_view();
    ra2_map_scene::cliff_tools::plan(
        view.editor.doc.as_ref().unwrap(),
        &scene,
        ra2_map_scene::cliff_tools::rectangle([28, 28], [44, 44]),
        4,
    )
    .unwrap()
    .apply(view.editor.doc.as_mut().unwrap())
    .unwrap();
    view.editor.commit(&scene, "fixture plateau");
    view.editor.tool = crate::editing::Tool::Select;
    let before = view
        .editor
        .doc
        .as_ref()
        .unwrap()
        .snapshot()
        .bytes()
        .unwrap();
    let mut harness = crate::test_support::harness()
        .with_context_setup(setup)
        .with_accessibility_check(false)
        .with_size([1280., 800.])
        .build_ui(|ui| {
            egui::Panel::left("flat-tools")
                .default_size(260.)
                .show(ui, |ui| view.toolbox(ui));
            egui::Panel::right("flat-inspector")
                .default_size(235.)
                .show(ui, |ui| view.inspector(ui));
            egui::CentralPanel::default().show(ui, |ui| view.canvas_inner(ui, None));
        });
    for size in [[1280., 800.], [900., 600.]] {
        harness.set_size(size.into());
        harness.run();
        harness.get_by_label("2D 地形");
        harness.get_by_label("2D 高度数字");
        #[cfg(feature = "ui-test-video")]
        {
            let output =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/flat-terrain-audit");
            std::fs::create_dir_all(&output).unwrap();
            harness
                .render()
                .unwrap()
                .save(output.join(format!("flat-terrain-{}.png", size[0])))
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
        before
    );
    drop(view);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn framework_and_flat_modes_are_mutually_exclusive_and_keep_other_tools() {
    let (mut view, _scene, root) = flat_authoring_view();
    let before = view
        .editor
        .doc
        .as_ref()
        .unwrap()
        .snapshot()
        .bytes()
        .unwrap();
    let ctx = egui::Context::default();
    flat_frame(&mut view, &ctx, None, None, None);
    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    for expected in [true, false, true] {
        crate::test_support::run_ui(
            &ctx,
            egui::RawInput {
                events: vec![
                    egui::Event::Key {
                        key: egui::Key::F,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    },
                    egui::Event::Key {
                        key: egui::Key::F,
                        physical_key: None,
                        pressed: false,
                        repeat: false,
                        modifiers,
                    },
                ],
                ..Default::default()
            },
            |ui| view.canvas_inner(ui, None),
        );
        assert_eq!(view.editor.framework, expected);
        assert!(!view.editor.flat_view);
    }
    flat_frame(&mut view, &ctx, None, None, Some(egui::Key::F2));
    assert!(view.editor.flat_view && !view.editor.framework);
    flat_frame(&mut view, &ctx, None, None, Some(egui::Key::F2));
    view.editor.set_framework(true);
    view.editor.tool = crate::editing::Tool::Region;
    flat_frame(&mut view, &ctx, None, None, None);
    assert!(view.editor.framework);
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

#[test]
fn framework_workspace_shows_matching_palette_in_compact_and_desktop_windows() {
    let (mut view, mut scene, root) = flat_authoring_view();
    if let Some(directory) = std::env::var_os("RA2_FRAMEWORK_GAME_DIR") {
        let mut install = ra2_map_assets::assets::Install::open(Path::new(&directory)).unwrap();
        scene = ra2_map_scene::map::load_scene_bytes(
            &mut install,
            "framework.map",
            "UI audit".into(),
            crate::new_map::NewMapOptions {
                width: 40,
                height: 40,
                level: 4,
                ..Default::default()
            }
            .bytes()
            .unwrap(),
            Instant::now(),
        )
        .unwrap();
        view.editor.attach(&scene);
    }
    ra2_map_scene::cliff_tools::plan(
        view.editor.doc.as_ref().unwrap(),
        &scene,
        ra2_map_scene::cliff_tools::rectangle([28, 28], [44, 44]),
        4,
    )
    .unwrap()
    .apply(view.editor.doc.as_mut().unwrap())
    .unwrap();
    view.editor.commit(&scene, "plateau");
    view.editor.prepare_cliff_smoke_preview(&scene).unwrap();
    view.editor.set_framework(true);
    view.editor.flat_view = false;
    view.zoom = 0.75;
    view.scene = Some(scene);
    let before = view
        .editor
        .doc
        .as_ref()
        .unwrap()
        .snapshot()
        .bytes()
        .unwrap();
    let mut harness = crate::test_support::harness()
        .with_context_setup(setup)
        .with_accessibility_check(false)
        .with_size([1280., 800.])
        .build_ui(|ui| {
            egui::Panel::left("framework-tools")
                .default_size(260.)
                .show(ui, |ui| view.toolbox(ui));
            egui::Panel::right("framework-inspector")
                .default_size(235.)
                .show(ui, |ui| view.inspector(ui));
            egui::CentralPanel::default().show(ui, |ui| view.canvas_inner(ui, None));
        });
    for size in [[1280., 800.], [900., 600.]] {
        harness.set_size(size.into());
        harness.run();
        harness.get_by_label("框架");
        harness.get_by_label("2D 地形");
        #[cfg(feature = "ui-test-video")]
        {
            let output =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/framework-ui-audit");
            std::fs::create_dir_all(&output).unwrap();
            harness
                .render()
                .unwrap()
                .save(output.join(format!("framework-{}.png", size[0])))
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
        before
    );
    drop(view);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn map_boundaries_follow_camera_and_toggle_in_every_canvas_mode() {
    let (mut view, root) = view();
    let mut scene = terrain_scene();
    for cell in &mut scene.cells {
        cell.z = 4;
        cell.screen_y -= 60.;
    }
    view.editor.attach(&scene);
    view.scene = Some(scene);
    view.editor.tool = crate::editing::Tool::Terrain;
    assert!(view.editor.map_boundaries);
    let ctx = egui::Context::default();
    let before = view
        .editor
        .doc
        .as_ref()
        .unwrap()
        .snapshot()
        .bytes()
        .unwrap();
    for (flat, framework) in [(false, false), (false, true), (true, false)] {
        view.editor.flat_view = flat;
        view.editor.framework = framework;
        view.center = egui::pos2(-25., 650.);
        view.fit = false;
        for (zoom, pan) in [(0.25, egui::vec2(0., 0.)), (0.5, egui::vec2(13., -29.))] {
            view.zoom = zoom;
            view.center += pan;
            for enabled in [true, false] {
                view.editor.map_boundaries = enabled;
                let output = crate::test_support::run_ui(
                    &ctx,
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::pos2(40., 20.),
                            egui::vec2(1000., 700.),
                        )),
                        ..Default::default()
                    },
                    |ui| view.canvas_inner(ui, None),
                );
                let guides: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|s| {
                        if let egui::Shape::Rect(r) = &s.shape
                            && [
                                egui::Color32::from_rgb(255, 64, 64),
                                egui::Color32::from_rgb(64, 112, 255),
                            ]
                            .contains(&r.stroke.color)
                        {
                            Some((s.clip_rect, r.rect))
                        } else {
                            None
                        }
                    })
                    .collect();
                assert_eq!(guides.len(), if enabled { 2 } else { 0 });
                if enabled {
                    let viewport = view.workbench.canvas_rect.unwrap();
                    let camera = crate::camera::MapCamera {
                        center: view.center,
                        zoom,
                    };
                    let world = camera.world_rect(viewport);
                    // Size encloses the elevated border and final half row.
                    // LocalSize remains the logical plane reference.
                    for ((clip, actual), (origin, size)) in guides.iter().zip([
                        (
                            egui::pos2(-600., if flat { 330. } else { 270. }),
                            egui::vec2(1200., 615.),
                        ),
                        (egui::pos2(-480., 450.), egui::vec2(960., 300.)),
                    ]) {
                        assert_eq!(*clip, viewport);
                        assert!(
                            (actual.min - (viewport.min + (origin - world.min) * zoom)).length()
                                < 0.01
                        );
                        assert!((actual.size() - size * zoom).length() < 0.01);
                    }
                }
            }
        }
    }
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

#[test]
fn canvas_mode_buttons_switch_exclusively_without_moving_the_map() {
    let (mut view, _, root) = flat_authoring_view();
    let before = view
        .editor
        .doc
        .as_ref()
        .unwrap()
        .snapshot()
        .bytes()
        .unwrap();
    let modes = std::cell::Cell::new((false, false));
    let mut harness = crate::test_support::harness()
        .with_context_setup(setup)
        .with_accessibility_check(false)
        .with_size([1200., 800.])
        .build_ui(|ui| {
            view.canvas_inner(ui, None);
            modes.set((view.editor.framework, view.editor.flat_view));
        });
    harness.run();
    assert_eq!(modes.get(), (false, true));
    for (label, expected) in [
        ("框架", (true, false)),
        ("2D 地形", (false, true)),
        ("2D 地形", (false, false)),
        ("框架", (true, false)),
        ("框架", (false, false)),
    ] {
        harness.get_by_label(label).click();
        harness.run();
        assert_eq!(modes.get(), expected);
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
        before
    );
    drop(view);
    std::fs::remove_dir_all(root).unwrap();
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
fn random_map_reopening_and_landscape_changes_preserve_edits_until_recommendations_are_applied() {
    let (mut view, root) = view();
    view.workbench.random_options.terrain.seed = 43210;
    view.workbench.random_options.terrain.water_amount = 37;
    view.workbench.random_options.terrain.ruggedness = 43;
    view.workbench.random_options.terrain.region_size = 62;
    let before = view.workbench.random_options.terrain.clone();
    view.show_random_map();
    let view = std::cell::RefCell::new(view);
    let mut directory = String::from("D:/YR");
    let mut harness = crate::test_support::harness()
        .with_context_setup(setup)
        .with_accessibility_check(false)
        .with_size([1280., 1000.])
        .build_ui(|ui| view.borrow_mut().settings_dialogs(ui.ctx(), &mut directory));
    harness.run();
    harness.get_by_label("取消").click();
    harness.run();
    view.borrow_mut().show_random_map();
    harness.run();
    assert_eq!(view.borrow().workbench.random_options.terrain, before);

    harness.get_by_value("内陆湖泊").click();
    harness.run();
    harness.get_by_label("山地").click();
    harness.run();
    let mut mountain = before.clone();
    mountain.map_type = 4;
    assert_eq!(view.borrow().workbench.random_options.terrain, mountain);

    harness.get_by_label("应用推荐参数").click();
    harness.run();
    mountain.water_amount = 10;
    mountain.ruggedness = 70;
    assert_eq!(view.borrow().workbench.random_options.terrain, mountain);
    drop(harness);
    drop(view);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn random_map_controls_follow_landscape_and_resource_availability_in_both_languages() {
    use egui_kittest::kittest::NodeT as _;

    for info in &crate::i18n::LANGUAGES {
        let _language = crate::i18n::enter(info.language);
        let (mut view, root) = view();
        view.show_random_map();
        let view = std::cell::RefCell::new(view);
        let mut directory = String::from("D:/YR");
        let mut harness = crate::test_support::harness()
            .with_context_setup(setup)
            .with_accessibility_check(false)
            .with_size([1280., 1000.])
            .build_ui(|ui| view.borrow_mut().settings_dialogs(ui.ctx(), &mut directory));
        for landscape in [0, 1, 3, 4] {
            view.borrow_mut().workbench.random_options.terrain.map_type = landscape;
            harness.run();
            for (label, enabled) in [
                ("水域规模", landscape != 0),
                ("区域连通程度", matches!(landscape, 3 | 4)),
                ("区域大小", matches!(landscape, 3 | 4)),
                ("树林数量", true),
                ("矿区分散程度", true),
            ] {
                assert_eq!(
                    harness
                        .get_by_role_and_label(
                            egui::accesskit::Role::Slider,
                            crate::i18n::text(label)
                        )
                        .accesskit_node()
                        .is_disabled(),
                    !enabled,
                    "{label}, landscape {landscape}"
                );
            }
        }
        harness
            .get_by_label(crate::i18n::text("生成矿石资源"))
            .click_accesskit();
        harness.run();
        assert!(!view.borrow().workbench.random_options.ore);
        harness
            .get_by_label(crate::i18n::text("生成中立城镇"))
            .click_accesskit();
        harness.run();
        assert!(view.borrow().workbench.random_options.terrain.urban_enabled);
        assert!(
            !harness
                .get_by_role_and_label(egui::accesskit::Role::Slider, crate::i18n::text("城镇规模"))
                .accesskit_node()
                .is_disabled()
        );
        for (label, role) in [
            ("矿区分散程度", egui::accesskit::Role::Slider),
            ("资源丰度", egui::accesskit::Role::Slider),
            ("矿区包含宝石", egui::accesskit::Role::CheckBox),
        ] {
            assert!(
                harness
                    .get_by_role_and_label(role, crate::i18n::text(label))
                    .accesskit_node()
                    .is_disabled()
            );
        }
        drop(harness);
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
fn language_settings_switch_both_ways_without_changing_map_or_draft() {
    use crate::i18n::{self, Language};
    let (mut view, root) = view();
    let scene = terrain_scene();
    view.editor.attach(&scene);
    view.scene = Some(scene);
    let before = view
        .editor
        .doc
        .as_ref()
        .unwrap()
        .snapshot()
        .bytes()
        .unwrap();
    let state = view.editor.doc.as_ref().unwrap().state_id;
    let mut draft = "Name 地图 {language}".to_owned();
    let mut harness = crate::test_support::harness()
        .with_context_setup(setup)
        .with_size([1280., 800.])
        .build_ui(|ui| {
            let _language = i18n::enter(i18n::selected(ui.ctx()));
            ui.horizontal(|ui| view.toolbar(ui, "D:/YR"));
            ui.add(egui::TextEdit::singleline(&mut draft).id_salt("language-draft"))
                .accessible_name(i18n::tr!("地图名称"));
        });
    harness.get_by_label("文件");
    harness.get_by_label("设置").click();
    harness.run();
    harness.get_by_label("语言 / Language").hover();
    harness.run();
    harness.get_by_label("English").click();
    harness.run();
    assert_eq!(i18n::selected(&harness.ctx), Language::English);
    harness.get_by_label("File");
    assert_eq!(
        harness.get_by_label("Map name").value().as_deref(),
        Some("Name 地图 {language}")
    );
    harness.get_by_label("Settings").click();
    harness.run();
    harness.get_by_label("Language / 语言").hover();
    harness.run();
    harness.get_by_label("简体中文").click();
    harness.run();
    assert_eq!(i18n::selected(&harness.ctx), Language::Chinese);
    harness.get_by_label("文件");
    drop(harness);
    assert_eq!(draft, "Name 地图 {language}");
    assert_eq!(view.editor.doc.as_ref().unwrap().state_id, state);
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

#[test]
fn map_settings_tabs_stay_inside_the_viewport_without_document_writes() {
    let (mut editor, _ctx, root) = crate::editing::tests::editor();
    let scene = terrain_scene();
    editor.attach(&scene);
    let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    let mut settings = crate::map_settings_ui::MapSettingsUi::default();
    settings.open = true;
    for language in [
        crate::i18n::Language::Chinese,
        crate::i18n::Language::English,
    ] {
        let _language = crate::i18n::enter(language);
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
                let initial_width = harness
                    .ctx
                    .memory(|m| m.area_rect(egui::Id::unique("map-properties")))
                    .unwrap()
                    .width();
                if tab == 3 {
                    harness.run_steps(120);
                }
                let rect = harness
                    .ctx
                    .memory(|m| m.area_rect(egui::Id::unique("map-properties")))
                    .unwrap();
                assert!(
                    egui::Rect::from_min_size(egui::Pos2::ZERO, size.into()).contains_rect(rect),
                    "tab {tab}: {rect:?}"
                );
                if tab == 3 {
                    assert!(
                        (rect.width() - initial_width).abs() <= 1.,
                        "map information kept expanding over idle frames: {rect:?}"
                    );
                    #[cfg(feature = "ui-test-video")]
                    {
                        let output = Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("../../target/map-settings-width");
                        std::fs::create_dir_all(&output).unwrap();
                        harness
                            .render()
                            .unwrap()
                            .save(output.join(format!("{language:?}-{}.png", size[0])))
                            .unwrap();
                    }
                }
            }
            if tab == 3 {
                harness.set_size([1280., 800.].into());
                harness.run();
                let initial = harness
                    .ctx
                    .memory(|m| m.area_rect(egui::Id::unique("map-properties")))
                    .unwrap();
                let corner = harness
                    .get_by_label("Resize window bottom-right corner")
                    .rect()
                    .center();
                let target = corner + egui::vec2(160., 0.);
                harness.hover_at(corner);
                harness.run();
                harness.drag_at(corner);
                harness.run();
                harness.hover_at(target);
                harness.run();
                harness.drop_at(target);
                harness.run();
                let resized = harness
                    .ctx
                    .memory(|m| m.area_rect(egui::Id::unique("map-properties")))
                    .unwrap();
                assert!(
                    resized.width() > initial.width() + 100.,
                    "the user must be able to widen settings: {initial:?} -> {resized:?}"
                );
                harness.run_steps(120);
                let settled = harness
                    .ctx
                    .memory(|m| m.area_rect(egui::Id::unique("map-properties")))
                    .unwrap();
                assert!(
                    (settled.width() - resized.width()).abs() <= 1.,
                    "resized settings keep growing: {resized:?} -> {settled:?}"
                );
                harness.get_by_label(crate::i18n::text("基本信息")).click();
                harness.run();
                for group in ra2_map_document::map_information::Group::ALL
                    .into_iter()
                    .skip(1)
                {
                    let name = crate::i18n::text(group.label());
                    harness.get_by_label(name).scroll_to_me();
                    harness.run();
                    harness.get_by_label(name).click();
                    harness.run();
                    let initial_width = harness
                        .ctx
                        .memory(|m| m.area_rect(egui::Id::unique("map-properties")))
                        .unwrap()
                        .width();
                    harness.run_steps(120);
                    let rect = harness
                        .ctx
                        .memory(|m| m.area_rect(egui::Id::unique("map-properties")))
                        .unwrap();
                    assert!(
                        (rect.width() - initial_width).abs() <= 1.,
                        "expanded {name} keeps growing: {rect:?}"
                    );
                    #[cfg(feature = "ui-test-video")]
                    {
                        let output = Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("../../target/map-settings-width");
                        harness
                            .render()
                            .unwrap()
                            .save(output.join(format!("{language:?}-{group:?}.png")))
                            .unwrap();
                    }
                    harness.get_by_label(name).scroll_to_me();
                    harness.run();
                    harness.get_by_label(name).click();
                    harness.run();
                }
            }
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

    for (language, name, panel) in crate::i18n::LANGUAGES.iter().flat_map(|info| {
        panels()
            .into_iter()
            .map(move |(name, panel)| (info.language, name, panel))
    }) {
        let _language = crate::i18n::enter(language);
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
                    .save(output.join(format!("{language:?}-{name}-{}.png", size[0])))
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
