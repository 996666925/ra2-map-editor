use super::super::tests::{editor, terrain_scene};
use super::*;

#[test]
fn flatten_nudges_keep_the_tool_and_fixed_target_and_each_have_their_own_undo() {
    let (mut e, _, root) = editor();
    let scene = terrain_scene();
    e.attach(&scene);
    e.activate_terrain_mode(TerrainMode::Flatten);
    e.smooth_heights = false;
    e.auto_lat = false;
    e.flatten_fixed = true;
    e.flatten_level = 9;
    e.height_step = 7;
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.nudge_flatten_height(&scene, [20, 20], true);
    assert_eq!(e.doc.as_ref().unwrap().terrain([20, 20]).unwrap().level, 1);
    e.nudge_flatten_height(&scene, [20, 20], true);
    assert_eq!(e.doc.as_ref().unwrap().terrain([20, 20]).unwrap().level, 2);
    assert_eq!(e.terrain_mode, TerrainMode::Flatten);
    assert_eq!(e.flatten_level, 9);
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().terrain([20, 20]).unwrap().level, 1);
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.pointer(&scene, Some([20, 20]), true, true, false, None);
    e.nudge_flatten_height(&scene, [20, 20], true);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.cancel_active(&scene);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn height_preview_freezes_all_settings_and_commits_its_exact_plan() {
    let (mut e, ctx, root) = editor();
    let scene = terrain_scene();
    e.attach(&scene);
    e.activate_terrain_mode(TerrainMode::Raise);
    e.height_step = 4;
    e.smooth_heights = false;
    e.auto_lat = false;
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.pointer(&scene, Some([20, 20]), true, true, false, None);
    e.pointer(&scene, Some([21, 20]), false, true, false, None);
    let plan = e.raise_lower_plan(&scene, None).unwrap();
    assert_eq!(plan.edits.len(), 2);
    assert!(plan.edits.values().all(|v| v.level == 4));
    e.radius = 7;
    e.height_absolute = true;
    e.height_target = 12;
    e.smooth_heights = true;
    e.auto_lat = true;
    e.terrain_mode = TerrainMode::Lower;
    assert!(Arc::ptr_eq(
        &plan,
        &e.raise_lower_plan(&scene, None).unwrap()
    ));
    let output = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
        e.terrain_preview(
            ui.painter(),
            &scene,
            ui.max_rect(),
            &crate::camera::MapCamera {
                center: scene.bounds.center(),
                zoom: 1.,
            },
            None,
        )
    });
    assert!(!output.shapes.is_empty());
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.pointer(&scene, None, false, false, true, None);
    for (&at, &v) in &plan.edits {
        assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
    }
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn height_preview_clamps_raw_ground_and_rejects_a_partial_cliff_before_release() {
    let (mut e, _, root) = editor();
    let scene = terrain_scene();
    e.attach(&scene);
    e.activate_terrain_mode(TerrainMode::Raise);
    e.smooth_heights = false;
    e.auto_lat = false;
    let old = terrain_tools::value(e.doc.as_ref().unwrap(), &scene, [20, 20]);
    e.doc
        .as_mut()
        .unwrap()
        .set(
            Target::Terrain([20, 20]),
            Some(Value::Terrain(crate::document::TerrainValue {
                level: 126,
                ..old
            })),
        )
        .unwrap();
    e.height_step = 4;
    assert_eq!(
        e.raise_lower_plan(&scene, Some([20, 20])).unwrap().edits[&[20, 20]].level,
        127
    );
    e.doc.as_mut().unwrap().cancel();
    // Use the authoring fixture, which contains a complete multi-cell cliff.
    let f = ra2_map_assets::test_support::Fixture::with_authoring();
    let mut install = f.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    let scene = crate::map::load_scene_bytes(
        &mut install,
        "height.map",
        "test".into(),
        crate::new_map::NewMapOptions {
            width: 60,
            height: 60,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
        Instant::now(),
    )
    .unwrap();
    e.attach(&scene);
    let ring = ra2_map_scene::cliff_tools::plan(
        e.doc.as_ref().unwrap(),
        &scene,
        ra2_map_scene::cliff_tools::rectangle([40, 40], [52, 52]),
        0,
    )
    .unwrap();
    ring.apply(e.doc.as_mut().unwrap()).unwrap();
    e.doc.as_mut().unwrap().commit("cliffs");
    let stamp = ring
        .cliffs
        .iter()
        .find(|s| {
            scene.palette.templates[&(s.tile, 0)]
                .cells
                .iter()
                .flatten()
                .count()
                > 1
        })
        .unwrap();
    let t = &scene.palette.templates[&(stamp.tile, 0)];
    let cells: Vec<_> = t
        .cells
        .iter()
        .enumerate()
        .filter(|(_, s)| s.is_some())
        .map(|(sub, _)| {
            [
                stamp.origin[0] + sub as u16 % t.width,
                stamp.origin[1] + sub as u16 / t.width,
            ]
        })
        .collect();
    let at = cells[0];
    e.activate_terrain_mode(TerrainMode::Raise);
    e.height_absolute = true;
    e.height_target = 8;
    let plan = e.raise_lower_plan(&scene, Some(at)).unwrap();
    assert_eq!(plan.edits.len(), cells.len());
    e.smooth_heights = true;
    e.radius = 5;
    assert_eq!(
        e.raise_lower_plan(&scene, Some(at)).unwrap().edits,
        plan.edits,
        "normal cliff click automatically moves the full template despite smooth mode and brush size"
    );
    e.brush_modifiers(egui::Modifiers::CTRL);
    let connected = e.raise_lower_plan(&scene, Some(at)).unwrap();
    assert!(connected.edits.len() > plan.edits.len());
    e.pointer(&scene, Some(at), true, true, false, None);
    e.brush_modifiers(egui::Modifiers::NONE);
    assert_eq!(
        e.raise_lower_plan(&scene, None).unwrap().edits,
        connected.edits,
        "Ctrl is captured on press"
    );
    e.cancel_active(&scene);
    e.smooth_heights = false;
    e.radius = 1;
    let missing = cells[1];
    e.doc
        .as_mut()
        .unwrap()
        .set(Target::Terrain(missing), Some(Value::Terrain(old)))
        .unwrap();
    assert!(
        e.raise_lower_plan(&scene, Some(at))
            .err()
            .unwrap()
            .contains("不完整")
    );
    e.doc.as_mut().unwrap().commit("partial cliff");
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.pointer(&scene, Some(at), true, true, false, None);
    assert!(e.raise_lower_plan(&scene, None).is_err());
    e.pointer(&scene, None, false, false, true, None);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn raise_preview_draws_paved_slope_textures_and_commits_the_same_tiles() {
    let (mut e, ctx, root) = editor();
    let fixture = ra2_map_assets::test_support::Fixture::with_joining();
    let mut install = fixture.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    let scene = crate::map::load_scene_bytes(
        &mut install,
        "road-height.map",
        "test".into(),
        crate::new_map::NewMapOptions {
            level: 4,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
        Instant::now(),
    )
    .unwrap();
    e.attach(&scene);
    let doc = e.doc.as_mut().unwrap();
    let road = ra2_map_scene::road_tools::plan(doc, &scene, [40, 50], [56, 50], false).unwrap();
    road.apply(doc).unwrap();
    doc.commit("paved road");
    let before = doc.snapshot().bytes().unwrap();
    e.activate_terrain_mode(TerrainMode::Raise);
    e.height_step = 1;
    e.radius = 7;
    e.smooth_heights = true;
    e.auto_lat = true;
    let at = [50, 50];
    e.pointer(&scene, Some(at), true, true, false, None);
    let plan = e.raise_lower_plan(&scene, None).unwrap();
    assert_eq!(plan.cliffs.len(), 2);
    let (x, y) = vera20k_core::map::terrain::iso_to_screen(at[0], at[1], 4);
    let output = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
        e.terrain_preview(
            ui.painter(),
            &scene,
            ui.max_rect(),
            &crate::camera::MapCamera {
                center: egui::pos2(x, y),
                zoom: 1.,
            },
            None,
        )
    });
    for stamp in &plan.cliffs {
        for sub in 0..3 {
            let texture = e.ui_state.textures[&(stamp.tile, sub, 0)].id();
            assert!(output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == texture)
            }));
        }
    }
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.pointer(&scene, None, false, false, true, None);
    for (&at, &v) in &plan.edits {
        assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
    }
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);

    e.smooth_heights = false;
    e.radius = 1;
    e.pointer(&scene, Some(at), true, true, false, None);
    let error = e.raise_lower_plan(&scene, None).err().unwrap();
    assert!(error.contains("整块公路"));
    e.pointer(&scene, None, false, false, true, None);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}
