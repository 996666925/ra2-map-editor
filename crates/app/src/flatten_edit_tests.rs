use super::super::tests::editor;
use super::*;

fn scene() -> MapScene {
    let f = ra2_map_assets::test_support::Fixture::with_authoring();
    let mut i = f.install();
    i.catalog = ra2_map_assets::catalog::Catalog::load(&i.manager, &i.theater);
    crate::map::load_scene_bytes(
        &mut i,
        "flatten.map",
        "test".into(),
        crate::new_map::NewMapOptions {
            width: 60,
            height: 60,
            level: 4,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
        Instant::now(),
    )
    .unwrap()
}
fn put(e: &mut Editor, scene: &MapScene, at: Cell, level: u8) {
    let doc = e.doc.as_mut().unwrap();
    let old = terrain_tools::value(doc, scene, at);
    doc.set(
        Target::Terrain(at),
        Some(Value::Terrain(crate::document::TerrainValue {
            level,
            ..old
        })),
    )
    .unwrap();
}
#[test]
fn default_flatten_samples_on_press_and_keeps_that_height_until_the_whole_stroke_commits() {
    let (mut e, ctx, root) = editor();
    let scene = scene();
    e.attach(&scene);
    put(&mut e, &scene, [44, 46], 8);
    e.doc.as_mut().unwrap().commit("source");
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.activate_terrain_mode(TerrainMode::Flatten);
    e.smooth_heights = false;
    e.auto_lat = false;
    assert!(!e.flatten_fixed);
    e.pointer(&scene, Some([44, 46]), true, true, false, None);
    assert_eq!(e.flatten_level, 8);
    e.pointer(&scene, Some([45, 46]), false, true, false, None);
    e.pointer(&scene, Some([46, 46]), false, true, false, None);
    let plan = e.flatten.plan(e.doc.as_ref().unwrap(), &scene).unwrap();
    assert_eq!(plan.edits[&[45, 46]].level, 8);
    assert_eq!(plan.edits[&[46, 46]].level, 8);
    let v = plan.edits[&[45, 46]];
    let key = vera20k_core::map::theater::TileKey {
        tile_id: scene.palette.raw_ids[&v.tile],
        sub_tile: v.sub,
        variant: 0,
    };
    let im = &scene.palette.images[&key];
    let pixel = im
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .position(|p| p[3] != 0)
        .unwrap();
    let (x, y) = vera20k_core::map::terrain::iso_to_screen(45, 46, v.level);
    let world = egui::pos2(
        x + im.info.offset[0] as f32 + (pixel % im.info.width as usize) as f32 + 0.5,
        y + im.info.offset[1] as f32 + (pixel / im.info.width as usize) as f32 + 0.5,
    );
    assert_eq!(
        e.flatten_hover(&scene, world),
        Some([45, 46]),
        "drag picking follows the previewed height"
    );
    let output = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
        e.terrain_preview(
            ui.painter(),
            &scene,
            ui.max_rect(),
            &crate::camera::MapCamera {
                center: scene.bounds.center(),
                zoom: 0.75,
            },
            Some([46, 46]),
        )
    });
    assert!(!output.shapes.is_empty());
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.flatten_level = 1;
    e.flatten_fixed = true; // Active settings remain frozen.
    e.tool = Tool::Erase;
    e.pointer(&scene, None, false, false, true, None);
    assert_eq!(e.doc.as_ref().unwrap().terrain([46, 46]).unwrap().level, 8);
    let after = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.history(true, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    e.activate_terrain_mode(TerrainMode::Flatten);
    e.flatten_fixed = false;
    e.pointer(&scene, Some([48, 46]), true, true, false, None);
    assert_eq!(e.flatten_level, 4, "the next stroke samples again");
    assert!(e.cancel_active(&scene));
    e.pointer(&scene, None, false, false, true, None);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn wide_flatten_brush_preserves_the_far_side_and_cliff_until_the_cursor_crosses() {
    let (mut e, _, root) = editor();
    let scene = scene();
    e.attach(&scene);
    let b = scene
        .brushes
        .iter()
        .find(|b| {
            scene.palette.groups.get(&b.key.tile_id).is_some_and(|g| {
                g.category == ra2_map_assets::terrain_library::TerrainCategory::Cliff
            })
        })
        .unwrap();
    for y in 40..=52 {
        e.doc
            .as_mut()
            .unwrap()
            .set(
                Target::Terrain([46, y]),
                Some(Value::Terrain(crate::document::TerrainValue {
                    tile: b.raw,
                    sub: b.key.sub_tile,
                    level: 4,
                    ice: 0,
                })),
            )
            .unwrap();
    }
    put(&mut e, &scene, [45, 46], 8);
    e.doc.as_mut().unwrap().commit("wall");
    e.activate_terrain_mode(TerrainMode::Flatten);
    e.radius = 5;
    e.smooth_heights = false;
    e.auto_lat = false;
    e.pointer(&scene, Some([45, 46]), true, true, false, None);
    let plan = e.flatten.plan(e.doc.as_ref().unwrap(), &scene).unwrap();
    assert!(plan.edits.keys().all(|at| at[0] <= 45));
    e.pointer(&scene, None, false, false, true, None);
    assert_eq!(e.doc.as_ref().unwrap().terrain([47, 46]).unwrap().level, 4);
    assert_eq!(e.doc.as_ref().unwrap().terrain([46, 46]).unwrap().level, 4);
    e.flatten_fixed = true;
    e.flatten_level = 8;
    e.pointer(&scene, Some([47, 46]), true, true, false, None);
    e.pointer(&scene, None, false, false, true, None);
    assert_eq!(e.doc.as_ref().unwrap().terrain([47, 46]).unwrap().level, 8);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn default_smooth_brush_can_sample_a_cliff_and_drag_into_its_unraised_interior() {
    let (mut e, _, root) = editor();
    let scene = scene();
    e.attach(&scene);
    let doc = e.doc.as_mut().unwrap();
    let ring = ra2_map_scene::cliff_tools::plan(
        doc,
        &scene,
        ra2_map_scene::cliff_tools::rectangle([40, 40], [52, 52]),
        4,
    )
    .unwrap();
    ring.apply(doc).unwrap();
    for &at in &ring.interior {
        let old = terrain_tools::value(doc, &scene, at);
        doc.set(
            Target::Terrain(at),
            Some(Value::Terrain(crate::document::TerrainValue {
                level: 4,
                ..old
            })),
        )
        .unwrap();
    }
    doc.commit("hollow");
    let source = *ring
        .edits
        .iter()
        .find(|(at, v)| !ring.interior.contains(*at) && v.level == 8)
        .unwrap()
        .0;
    e.activate_terrain_mode(TerrainMode::Flatten);
    e.radius = 3;
    e.pointer(&scene, Some(source), true, true, false, None);
    assert_eq!(e.flatten_level, 8);
    e.pointer(&scene, Some([46, 46]), false, true, false, None);
    let plan = e.flatten.plan(e.doc.as_ref().unwrap(), &scene).unwrap();
    assert_eq!(plan.edits[&[46, 46]].level, 8);
    e.pointer(&scene, None, false, false, true, None);
    assert_eq!(e.doc.as_ref().unwrap().terrain([46, 46]).unwrap().level, 8);
    for (&at, &v) in &ring.edits {
        if !ring.interior.contains(&at) {
            assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
        }
    }
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn fixed_height_and_shift_retraction_keep_the_preview_and_commit_in_sync() {
    let (mut e, _, root) = editor();
    let scene = scene();
    e.attach(&scene);
    e.activate_terrain_mode(TerrainMode::Flatten);
    e.flatten_fixed = true;
    e.flatten_level = 7;
    e.smooth_heights = false;
    e.auto_lat = false;
    e.brush_modifiers(egui::Modifiers::SHIFT);
    e.pointer(&scene, Some([42, 46]), true, true, false, None);
    e.pointer(&scene, Some([48, 47]), false, true, false, None);
    e.pointer(&scene, Some([44, 47]), false, true, false, None);
    let plan = e.flatten.plan(e.doc.as_ref().unwrap(), &scene).unwrap();
    assert!(!plan.edits.contains_key(&[48, 46]));
    assert!(plan.edits.contains_key(&[44, 46]));
    e.pointer(&scene, None, false, false, true, None);
    for (&at, &v) in &plan.edits {
        assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
    }
    assert_eq!(e.doc.as_ref().unwrap().terrain([48, 46]).unwrap().level, 4);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}
