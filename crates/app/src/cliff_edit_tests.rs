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
        (
            TerrainMode::WaterOutline,
            vec![[28, 38], [38, 28], [48, 38], [38, 48]],
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

#[test]
fn ramp_gesture_widens_and_shrinks_preview_then_commits_once() {
    let (mut e, _, root) = editor();
    let scene = scene();
    e.attach(&scene);
    cliff_tools::plan(
        e.doc.as_ref().unwrap(),
        &scene,
        cliff_tools::rectangle([25, 25], [45, 45]),
        4,
    )
    .unwrap()
    .apply(e.doc.as_mut().unwrap())
    .unwrap();
    e.commit(&scene, "plateau");
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.tool = Tool::Terrain;
    e.terrain_mode = TerrainMode::RampEntrance;
    for (start, axis) in [([35, 45], 0), ([45, 35], 1), ([35, 25], 0), ([25, 35], 1)] {
        for sign in [-1, 1] {
            e.pointer(&scene, Some(start), true, true, false, None);
            let narrow = e.plateau_preview_plan(&scene, start).unwrap();
            let mut end = start;
            end[axis] = (i32::from(end[axis]) + sign * 4) as u16;
            e.pointer(&scene, Some(end), false, true, false, None);
            let wide = e.plateau_preview_plan(&scene, end).unwrap();
            assert_eq!(wide.edits.len(), narrow.edits.len() + 16);
            assert!(Arc::ptr_eq(
                &wide,
                &e.plateau_preview_plan(&scene, end).unwrap()
            ));
            end[axis] = (i32::from(start[axis]) + sign * 2) as u16;
            e.pointer(&scene, Some(end), false, true, false, None);
            let shrunk = e.plateau_preview_plan(&scene, end).unwrap();
            assert_eq!(shrunk.edits.len(), narrow.edits.len() + 8);
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
            e.pointer(&scene, None, false, false, true, None);
            assert!(!e.status.starts_with("本笔未修改"), "{}", e.status);
            for (&at, &v) in &shrunk.edits {
                assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
            }
            let after = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
            e.history(false, &scene);
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
            e.history(true, &scene);
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
            e.history(false, &scene);
        }
    }
    e.pointer(&scene, Some([35, 45]), true, true, false, None);
    e.pointer(&scene, Some([55, 45]), false, false, true, None);
    assert!(e.status.starts_with("本笔未修改"), "{}", e.status);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn corner_ramp_gesture_converts_the_corner_and_release_matches_preview() {
    let (mut e, _, root) = editor();
    let scene = scene();
    e.attach(&scene);
    cliff_tools::plan(
        e.doc.as_ref().unwrap(),
        &scene,
        cliff_tools::rectangle([25, 25], [45, 45]),
        4,
    )
    .unwrap()
    .apply(e.doc.as_mut().unwrap())
    .unwrap();
    e.commit(&scene, "plateau");
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.tool = Tool::Terrain;
    e.terrain_mode = TerrainMode::RampEntrance;
    for (corner, signs) in [
        ([25, 25], [1, 1]),
        ([45, 25], [-1, 1]),
        ([25, 45], [1, -1]),
        ([45, 45], [-1, -1]),
    ] {
        for axis in 0..2 {
            e.pointer(&scene, Some(corner), true, true, false, None);
            assert!(e.plateau_preview_plan(&scene, corner).is_ok());
            let mut end = corner;
            end[axis] = (i32::from(end[axis]) + signs[axis] * 4) as u16;
            e.pointer(&scene, Some(end), false, true, false, None);
            let p = e.plateau_preview_plan(&scene, end).unwrap();
            let id = scene.palette.raw_ids[&p.edits[&corner].tile];
            let cell = scene.palette.templates[&(id, 0)].cells[0].unwrap();
            assert!(
                (5..=8).contains(&cell.slope),
                "the clicked corner itself must slope"
            );
            assert_eq!(p.edits[&corner].level, 7);
            assert!(Arc::ptr_eq(
                &p,
                &e.plateau_preview_plan(&scene, end).unwrap()
            ));
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
            e.pointer(&scene, None, false, false, true, None);
            assert!(!e.status.starts_with("本笔未修改"), "{}", e.status);
            for (&at, &v) in &p.edits {
                assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
            }
            let after = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
            e.history(false, &scene);
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
            e.history(true, &scene);
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
            e.history(false, &scene);
        }
    }
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn joining_an_existing_corner_and_straight_ramp_previews_and_commits_as_one_edit() {
    let (mut e, _, root) = editor();
    let scene = scene();
    e.attach(&scene);
    cliff_tools::plan(
        e.doc.as_ref().unwrap(),
        &scene,
        cliff_tools::rectangle([25, 25], [45, 45]),
        4,
    )
    .unwrap()
    .apply(e.doc.as_mut().unwrap())
    .unwrap();
    e.commit(&scene, "plateau");
    e.tool = Tool::Terrain;
    e.terrain_mode = TerrainMode::RampEntrance;
    for corner_first in [true, false] {
        let (a, b, c, d) = if corner_first {
            ([45, 45], [45, 45], [35, 45], [41, 45])
        } else {
            ([35, 45], [41, 45], [45, 45], [45, 45])
        };
        e.pointer(&scene, Some(a), true, true, false, None);
        e.pointer(&scene, Some(b), false, false, true, None);
        assert!(!e.status.starts_with("本笔未修改"), "{}", e.status);
        let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        e.pointer(&scene, Some(c), true, true, false, None);
        e.pointer(&scene, Some(d), false, true, false, None);
        let p = e.plateau_preview_plan(&scene, d).unwrap();
        assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
        e.pointer(&scene, None, false, false, true, None);
        assert!(!e.status.starts_with("本笔未修改"), "{}", e.status);
        for (&at, &v) in &p.edits {
            assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
        }
        let after = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        e.history(false, &scene);
        assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
        e.history(true, &scene);
        assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
        e.history(false, &scene);
        e.history(false, &scene);
    }
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn tall_plateau_gesture_automatically_infers_base_above_fourteen_and_undoes_once() {
    let (mut e, _, root) = editor();
    let scene = scene();
    e.attach(&scene);
    for cell in &scene.cells {
        let at = [cell.rx, cell.ry];
        let mut v = terrain_tools::value(e.doc.as_ref().unwrap(), &scene, at);
        v.level = 24;
        e.doc
            .as_mut()
            .unwrap()
            .set(Target::Terrain(at), Some(Value::Terrain(v)))
            .unwrap();
    }
    e.commit(&scene, "high ground");
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.tool = Tool::Terrain;
    e.terrain_mode = TerrainMode::Plateau;
    e.pointer(&scene, Some([30, 30]), true, true, false, None);
    e.pointer(&scene, Some([42, 42]), false, true, false, None);
    let p = e.plateau_preview_plan(&scene, [42, 42]).unwrap();
    assert_eq!(p.base, 24);
    assert_eq!(p.edits[&[35, 35]].level, 28);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.pointer(&scene, None, false, false, true, None);
    assert_eq!(e.doc.as_ref().unwrap().terrain([35, 35]).unwrap().level, 28);
    let after = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.history(true, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}
