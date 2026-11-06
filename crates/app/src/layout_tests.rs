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
