use super::super::tests::{editor, terrain_scene};
use super::*;
use ra2_map_assets::{
    catalog::{TerrainSubtile, TerrainTemplate},
    terrain_library::{TerrainCategory, TerrainGroup},
};

fn setup() -> MapScene {
    let mut scene = terrain_scene();
    let original = scene.brushes[0].clone();
    let mut brush = original.clone();
    brush.key.tile_id = 1;
    brush.raw = 1;
    let p = Arc::get_mut(&mut scene.palette).unwrap();
    p.templates.insert(
        (1, 0),
        TerrainTemplate {
            width: 1,
            height: 1,
            cells: vec![Some(TerrainSubtile {
                height: 0,
                slope: 0,
                terrain: 0,
            })],
        },
    );
    p.groups.insert(
        1,
        TerrainGroup {
            id: 1,
            name: "Pave".into(),
            category: TerrainCategory::Ground,
        },
    );
    p.images.insert(brush.key, p.images[&original.key].clone());
    p.previews
        .insert((1, 0), p.previews[&(original.key.tile_id, 0)].clone());
    p.raw_ids.insert(1, 1);
    Arc::make_mut(&mut scene.catalog).raw_tiles.insert(1, 1);
    scene.brushes.push(brush);
    scene
}

#[test]
fn fill_drag_previews_without_writing_and_freezes_material_and_lat_until_sidebar_release() {
    let (mut e, ctx, root) = editor();
    let scene = setup();
    e.attach(&scene);
    e.activate_terrain_mode(TerrainMode::Fill);
    e.brush = scene.brushes.len() - 1;
    e.auto_lat = false;
    e.radius = 5;
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.pointer(&scene, Some([22, 22]), true, true, false, None);
    e.pointer(&scene, Some([20, 20]), false, true, false, None);
    let plan = e
        .fill
        .generate(e.doc.as_ref().unwrap(), &scene, [20, 20])
        .unwrap();
    let cached = e
        .fill
        .generate(e.doc.as_ref().unwrap(), &scene, [20, 20])
        .unwrap();
    assert!(Arc::ptr_eq(&plan, &cached));
    assert_eq!(plan.cells.len(), 9);
    let output = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
        e.terrain_preview(
            ui.painter(),
            &scene,
            ui.max_rect(),
            &crate::camera::MapCamera {
                center: scene.bounds.center(),
                zoom: 0.75,
            },
            None,
        );
    });
    assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Mesh(m) if m.vertices.iter().all(|v| v.color == Color32::from_white_alpha(180)))));
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.brush = 0;
    e.auto_lat = true;
    e.tool = Tool::Erase;
    e.pointer(&scene, None, false, false, true, None);
    for (&at, &v) in &plan.edits {
        assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
    }
    assert_eq!(e.feedback.len(), 9);
    assert_eq!(e.doc.as_ref().unwrap().terrain([19, 20]).unwrap().tile, 0);
    let after = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.history(true, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn fill_cancel_does_not_write_and_release_samples_the_final_endpoint() {
    let (mut e, _, root) = editor();
    let scene = setup();
    e.attach(&scene);
    e.activate_terrain_mode(TerrainMode::Fill);
    e.brush = scene.brushes.len() - 1;
    e.auto_lat = false;
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.pointer(&scene, Some([20, 20]), true, true, false, None);
    e.pointer(&scene, Some([23, 23]), false, true, false, None);
    assert!(e.cancel_active(&scene));
    e.pointer(&scene, Some([23, 23]), false, false, true, None);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    assert!(e.doc.as_ref().unwrap().undo_name().is_none());
    e.pointer(&scene, Some([20, 20]), true, true, false, None);
    e.pointer(&scene, Some([22, 21]), false, false, true, None);
    assert_eq!(e.feedback.len(), 6);
    e.pointer(&scene, Some([25, 25]), true, false, true, None);
    assert_eq!(e.feedback, BTreeSet::from([[25, 25]]));
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}
