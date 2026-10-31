use super::super::tests::editor;
use super::*;

fn scene() -> MapScene {
    let f = ra2_map_assets::test_support::Fixture::with_authoring();
    let mut install = f.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    crate::map::load_scene_bytes(
        &mut install,
        "plateau.map",
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
    .unwrap()
}

#[test]
fn plateau_preview_is_cached_and_matches_release_even_after_tool_and_height_change() {
    let (mut e, ctx, root) = editor();
    let scene = scene();
    e.attach(&scene);
    e.tool = Tool::Terrain;
    e.terrain_mode = TerrainMode::Plateau;
    e.template_level = Some(4);
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.pointer(&scene, Some([30, 30]), true, true, false, None);
    e.pointer(&scene, Some([38, 38]), false, true, false, None);
    let plan = e
        .plateau
        .generate(e.doc.as_ref().unwrap(), &scene, [38, 38])
        .unwrap();
    let same = e
        .plateau
        .generate(e.doc.as_ref().unwrap(), &scene, [38, 38])
        .unwrap();
    assert!(Arc::ptr_eq(&plan, &same));
    let output = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
        e.terrain_preview(
            ui.painter(),
            &scene,
            ui.max_rect(),
            &crate::camera::MapCamera {
                center: scene.bounds.center(),
                zoom: 0.75,
            },
            Some([38, 38]),
        );
    });
    assert!(output.shapes.iter().any(|s| matches!(&s.shape,egui::Shape::Mesh(m) if m.vertices.iter().all(|v|v.color==Color32::from_white_alpha(180)))));
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.template_level = Some(0);
    e.tool = Tool::Erase;
    e.pointer(&scene, None, false, false, true, None);
    for (&at, &v) in &plan.edits {
        assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
    }
    let after = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.history(true, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn outline_release_includes_final_point_and_escape_cancels_without_history() {
    let (mut e, ctx, root) = editor();
    let scene = scene();
    e.attach(&scene);
    e.tool = Tool::Terrain;
    e.terrain_mode = TerrainMode::CliffOutline;
    e.pointer(&scene, Some([28, 28]), true, true, false, None);
    for at in [[46, 28], [46, 34], [35, 34], [35, 46]] {
        e.pointer(&scene, Some(at), false, true, false, None);
    }
    e.pointer(&scene, Some([28, 46]), false, false, true, None);
    assert!(!e.status.starts_with("本笔未修改"), "{}", e.status);
    assert_eq!(e.doc.as_ref().unwrap().terrain([30, 40]).unwrap().level, 8);
    e.history(false, &scene);
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.terrain_mode = TerrainMode::Plateau;
    e.pointer(&scene, Some([30, 30]), true, true, false, None);
    e.pointer(&scene, Some([38, 38]), false, true, false, None);
    let input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
        ..Default::default()
    };
    let _ = crate::test_support::run_ui(&ctx, input, |ui| e.shortcuts(ui.ctx(), &scene));
    assert!(!e.stroking);
    assert!(e.plateau.gesture.is_none());
    e.pointer(&scene, Some([38, 38]), false, false, true, None);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn bridge_gesture_freezes_type_previews_and_commits_all_rows_once() {
    let (mut e, ctx, root) = editor();
    let scene = scene();
    e.attach(&scene);
    e.tool = Tool::Terrain;
    e.terrain_mode = TerrainMode::Bridge;
    e.bridge_kind = ra2_map_scene::bridge_tools::BridgeKind::LowWood;
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.pointer(&scene, Some([30, 30]), true, true, false, None);
    let p = e
        .plateau
        .generate(e.doc.as_ref().unwrap(), &scene, [42, 30])
        .unwrap();
    assert_eq!(p.overlays.len(), 39);
    let same = e
        .plateau
        .generate(e.doc.as_ref().unwrap(), &scene, [42, 30])
        .unwrap();
    assert!(Arc::ptr_eq(&p, &same));
    let output = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
        e.terrain_preview(
            ui.painter(),
            &scene,
            ui.max_rect(),
            &crate::camera::MapCamera {
                center: scene.bounds.center(),
                zoom: 0.3,
            },
            Some([42, 30]),
        )
    });
    assert!(!output.shapes.is_empty());
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.bridge_kind = ra2_map_scene::bridge_tools::BridgeKind::HighConcrete;
    e.pointer(&scene, Some([42, 30]), false, false, true, None);
    for (&at, &v) in &p.overlays {
        assert_eq!(e.doc.as_ref().unwrap().overlay(at), v);
    }
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn joining_gestures_cache_previews_freeze_the_mode_and_cancel_invalid_strokes() {
    let f = ra2_map_assets::test_support::Fixture::with_joining();
    let mut install = f.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    let scene = crate::map::load_scene_bytes(
        &mut install,
        "joining.map",
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
    let (mut e, ctx, root) = editor();
    e.attach(&scene);
    e.tool = Tool::Terrain;
    for (mode, path) in [
        (TerrainMode::Water, vec![[28, 28], [38, 38]]),
        (
            TerrainMode::WaterOutline,
            vec![[28, 28], [42, 28], [42, 42], [28, 42]],
        ),
        (TerrainMode::Road, vec![[28, 28], [42, 28]]),
    ] {
        e.tool = Tool::Terrain;
        e.terrain_mode = mode;
        let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        for (i, &at) in path.iter().enumerate() {
            e.pointer(&scene, Some(at), i == 0, true, false, None);
        }
        let at = *path.last().unwrap();
        let p = e.plateau_preview_plan(&scene, at).unwrap();
        let same = e.plateau_preview_plan(&scene, at).unwrap();
        assert!(Arc::ptr_eq(&p, &same));
        let rendered = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
            e.terrain_preview(
                ui.painter(),
                &scene,
                ui.max_rect(),
                &crate::camera::MapCamera {
                    center: scene.bounds.center(),
                    zoom: 0.3,
                },
                Some(at),
            )
        });
        assert!(!rendered.shapes.is_empty());
        assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
        e.terrain_mode = TerrainMode::LandFill;
        e.tool = Tool::Erase;
        e.pointer(&scene, None, false, false, true, None);
        assert!(!e.status.starts_with("本笔未修改"), "{}", e.status);
        for (&at, &v) in &p.edits {
            assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
        }
        e.history(false, &scene);
        assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    }
    e.tool = Tool::Terrain;
    e.terrain_mode = TerrainMode::Water;
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.pointer(&scene, Some([28, 28]), true, true, false, None);
    assert!(e.plateau_preview_plan(&scene, [28, 28]).is_err());
    e.pointer(&scene, None, false, false, true, None);
    assert!(e.status.starts_with("本笔未修改"));
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    assert!(!e.stroking);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}
