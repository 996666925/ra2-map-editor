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
fn ctrl_ground_fill_captures_source_and_material_and_commits_once_after_release() {
    let (mut e, _, root) = editor();
    let scene = setup();
    e.attach(&scene);
    let old = terrain_tools::value(e.doc.as_ref().unwrap(), &scene, [20, 20]);
    for (at, level) in [([20, 20], 0), ([21, 20], 4), ([24, 20], 0)] {
        e.doc
            .as_mut()
            .unwrap()
            .set(
                Target::Terrain(at),
                Some(Value::Terrain(crate::document::TerrainValue {
                    tile: 1,
                    level,
                    ..old
                })),
            )
            .unwrap();
    }
    e.doc.as_mut().unwrap().commit("source group");
    e.activate_terrain_mode(TerrainMode::Subtile);
    e.brush = 0;
    e.auto_lat = false;
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.brush_modifiers(egui::Modifiers::CTRL);
    e.pointer(&scene, Some([20, 20]), true, true, false, None);
    e.brush_modifiers(egui::Modifiers::NONE);
    e.brush = 1;
    e.pointer(&scene, Some([24, 20]), false, true, false, None);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.pointer(&scene, None, false, false, true, None);
    assert_eq!(e.doc.as_ref().unwrap().terrain([20, 20]).unwrap().tile, 0);
    assert_eq!(e.doc.as_ref().unwrap().terrain([21, 20]).unwrap().tile, 0);
    assert_eq!(e.doc.as_ref().unwrap().terrain([21, 20]).unwrap().level, 4);
    assert_eq!(e.doc.as_ref().unwrap().terrain([24, 20]).unwrap().tile, 1);
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
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
    let plan = e.fill.generate(e.doc.as_ref().unwrap(), &scene).unwrap();
    let cached = e.fill.generate(e.doc.as_ref().unwrap(), &scene).unwrap();
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

#[test]
fn fill_brush_interpolates_a_bent_stroke_and_commits_the_exact_plan() {
    let (mut e, ctx, root) = editor();
    let scene = setup();
    e.attach(&scene);
    e.activate_terrain_mode(TerrainMode::Fill);
    e.fill_style = FillStyle::Brush;
    e.fill_brush_size = 3;
    e.brush = 1;
    e.auto_lat = false;
    let doc = e.doc.as_ref().unwrap();
    let before = doc.snapshot().bytes().unwrap();
    let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
        e.terrain_preview(
            ui.painter(),
            &scene,
            ui.max_rect(),
            &crate::camera::MapCamera {
                center: scene.bounds.center(),
                zoom: 1.,
            },
            Some([17, 20]),
        );
    });
    assert_eq!(
        e.fill
            .preview
            .as_ref()
            .unwrap()
            .result
            .as_ref()
            .unwrap()
            .cells
            .len(),
        9
    );
    e.pointer(&scene, Some([17, 20]), true, true, false, None);
    e.pointer(&scene, Some([23, 20]), false, true, false, None);
    e.fill_style = FillStyle::Rectangle;
    e.fill_brush_size = 9;
    e.brush = 0;
    assert!(
        e.fill_brushing(),
        "freeze shape, size, and material on press"
    );
    e.pointer(&scene, Some([23, 24]), false, true, false, None);
    let plan = e.fill.generate(e.doc.as_ref().unwrap(), &scene).unwrap();
    assert_eq!(plan.cells.len(), 39);
    assert!(plan.cells.contains(&[20, 21]));
    assert!(plan.cells.contains(&[24, 25]));
    assert!(
        !plan.cells.contains(&[18, 23]),
        "do not paint the enclosing rectangle"
    );
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.pointer(&scene, Some([23, 20]), false, true, false, None);
    let revisited = e.fill.generate(e.doc.as_ref().unwrap(), &scene).unwrap();
    assert!(Arc::ptr_eq(&plan, &revisited));
    e.pointer(&scene, None, false, false, true, None);
    assert!(!e.fill_brushing());
    let doc = e.doc.as_ref().unwrap();
    for (&at, &v) in &plan.edits {
        assert_eq!(doc.terrain(at), Some(v));
        assert_eq!(v.tile, 1);
    }
    let after = doc.snapshot().bytes().unwrap();
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.history(true, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn fill_brush_does_not_bridge_a_gap_and_cancel_keeps_the_map_unchanged() {
    let (mut e, _, root) = editor();
    let scene = setup();
    e.attach(&scene);
    e.activate_terrain_mode(TerrainMode::Fill);
    e.fill_style = FillStyle::Brush;
    e.fill_brush_size = 1;
    e.brush = 1;
    e.auto_lat = false;
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    let (first, second) = ([17, 20], [24, 24]);
    e.pointer(&scene, Some(first), true, true, false, None);
    e.pointer(&scene, None, false, true, false, None);
    e.pointer(&scene, Some(second), false, true, false, None);
    assert_eq!(
        e.fill
            .generate(e.doc.as_ref().unwrap(), &scene)
            .unwrap()
            .cells,
        BTreeSet::from([first, second])
    );
    assert!(e.cancel_active(&scene));
    e.pointer(&scene, Some(second), false, false, true, None);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    assert!(e.doc.as_ref().unwrap().undo_name().is_none());

    e.pointer(&scene, Some(first), true, true, false, None);
    e.pointer(&scene, Some([21, 20]), false, false, true, None);
    assert_eq!(e.feedback, (17..=21).map(|x| [x, 20]).collect());
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

fn ice_scene() -> MapScene {
    let mut scene = setup();
    let palette = Arc::make_mut(&mut scene.catalog);
    let assets = Arc::get_mut(&mut scene.palette).unwrap();
    assets.bundled_ice_sets = Some([4096, 4160, 4224]);
    let ids = assets.tile_ids.as_mut().unwrap();
    ids.water_base = 1;
    ids.shore = 80;
    assets.groups.get_mut(&1).unwrap().category = TerrainCategory::Water;
    assets.templates.get_mut(&(1, 0)).unwrap().cells[0] = Some(TerrainSubtile {
        height: 0,
        slope: 0,
        terrain: 9,
    });
    assets.raw_ids.insert(80, 80);
    assets.templates.insert(
        (80, 0),
        TerrainTemplate {
            width: 1,
            height: 2,
            cells: vec![
                Some(TerrainSubtile {
                    height: 0,
                    slope: 0,
                    terrain: 14,
                }),
                Some(TerrainSubtile {
                    height: 0,
                    slope: 0,
                    terrain: 10,
                }),
            ],
        },
    );
    let image = assets.images[&scene.brushes[1].key].clone();
    assets.previews.insert((4096, 0), image.clone());
    for tile_id in 4096..=4159 {
        let raw = i32::from(tile_id);
        let key = vera20k_core::map::theater::TileKey {
            tile_id,
            sub_tile: 0,
            variant: 0,
        };
        assets.raw_ids.insert(raw, tile_id);
        assets.images.insert(key, image.clone());
        palette.raw_tiles.insert(tile_id, raw);
        assets.groups.insert(
            tile_id,
            TerrainGroup {
                id: 84,
                name: "VERA Ice 1".into(),
                category: TerrainCategory::Other,
            },
        );
        scene.brushes.push(ra2_map_assets::catalog::TerrainBrush {
            key,
            raw,
            label: format!("VERA Ice 1 · #{tile_id}"),
        });
    }
    scene
}

#[test]
fn ice_fill_is_in_water_tools_and_manual_ice_templates_remain() {
    let (mut editor, ctx, root) = editor();
    let scene = ice_scene();
    editor.attach(&scene);
    editor.activate_terrain_mode(TerrainMode::Fill);
    let frame = |editor: &mut Editor, scene: &MapScene, events| {
        crate::test_support::run_ui(
            &ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(330., 600.),
                )),
                events,
                ..Default::default()
            },
            |ui| editor.toolbox(ui, Some(scene)),
        )
    };
    let text_positions = |output: &egui::FullOutput, text: &str| {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(t) if t.galley.text() == text => {
                    Some(t.pos + t.galley.size() / 2.)
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let ice_label = tr!("可破裂冰面");
    let growth_label = tr!("铺冰时标记可再冻结");
    let shore_label = tr!("临岸碎冰过渡");
    let ground_panel = frame(&mut editor, &scene, vec![]);
    assert!(text_positions(&ground_panel, ice_label).is_empty());
    assert!(text_positions(&ground_panel, "#4096").is_empty());
    assert!(!text_positions(&ground_panel, tr!("素材库")).is_empty());
    assert!(!text_positions(&ground_panel, tr!("框选铺地")).is_empty());
    let click = |editor: &mut Editor, pos| {
        for pressed in [true, false] {
            frame(
                editor,
                &scene,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    };
    click(&mut editor, text_positions(&ground_panel, tr!("水域"))[0]);
    let water_panel = frame(&mut editor, &scene, vec![]);
    let positions = text_positions(&water_panel, ice_label);
    assert_eq!(positions.len(), 1);
    assert!(text_positions(&water_panel, growth_label).is_empty());
    assert!(text_positions(&water_panel, shore_label).is_empty());
    assert!(text_positions(&frame(&mut editor, &setup(), vec![]), ice_label).is_empty());
    frame(&mut editor, &scene, vec![]);
    click(&mut editor, positions[0]);
    assert_eq!(editor.terrain_mode, TerrainMode::IceFill);
    let ice_panel = frame(&mut editor, &scene, vec![]);
    assert_eq!(text_positions(&ice_panel, growth_label).len(), 1);
    assert!(editor.ice_soften_shore);
    let shore = text_positions(&ice_panel, shore_label);
    assert_eq!(shore.len(), 1);
    click(&mut editor, shore[0]);
    assert!(!editor.ice_soften_shore);
    assert!(text_positions(&ice_panel, tr!("素材库")).is_empty());
    assert_eq!(text_positions(&ice_panel, tr!("框选铺冰")).len(), 1);
    let brush = text_positions(&ice_panel, tr!("涂抹铺冰"));
    assert_eq!(brush.len(), 1);
    click(&mut editor, brush[0]);
    assert!(editor.fill_brushing());
    let brush_panel = frame(&mut editor, &scene, vec![]);
    assert_eq!(text_positions(&brush_panel, tr!("笔刷宽度（格）")).len(), 1);
    let manual = text_positions(&brush_panel, tr!("手工拼贴地形…"));
    assert_eq!(manual.len(), 1);
    click(&mut editor, manual[0]);
    assert_eq!(editor.terrain_mode, TerrainMode::Template);
    editor.show_library(0, "", "VERA Ice");
    let templates = frame(&mut editor, &scene, vec![]);
    assert_eq!(text_positions(&templates, "#4096").len(), 1);
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ice_fill_captures_the_gesture_and_skips_protected_cells() {
    let (mut editor, _, root) = editor();
    let scene = ice_scene();
    editor.attach(&scene);
    editor.activate_terrain_mode(TerrainMode::IceFill);
    let doc = editor.doc.as_mut().unwrap();
    let water = crate::document::TerrainValue {
        tile: 1,
        sub: 0,
        level: 2,
        ice: 0,
    };
    for x in 19..=22 {
        for y in 19..=21 {
            doc.set(Target::Terrain([x, y]), Some(Value::Terrain(water)))
                .unwrap();
        }
    }
    doc.set(Target::Overlay([21, 21]), Some(Value::Overlay(2, 0)))
        .unwrap();
    doc.commit("water fixture");
    let before = doc.snapshot().bytes().unwrap();
    editor.pointer(&scene, Some([20, 20]), true, true, false, None);
    editor.pointer(&scene, Some([20, 20]), false, false, true, None);
    assert_eq!(
        editor.doc.as_ref().unwrap().terrain([20, 20]).unwrap().tile,
        4096
    );
    let previous_edge = crate::document::TerrainValue {
        tile: 4120,
        ..water
    };

    editor.pointer(&scene, Some([21, 20]), true, true, false, None);
    editor.ice_fill_growth = false;
    editor.brush = 1;
    editor.pointer(&scene, Some([21, 21]), false, true, false, None);
    let plan = editor
        .fill
        .generate(editor.doc.as_ref().unwrap(), &scene)
        .unwrap();
    assert_eq!(plan.cells, BTreeSet::from([[21, 20]]));
    assert_eq!(plan.skipped, BTreeSet::from([[21, 21]]));
    assert_eq!(plan.edits[&[20, 20]].tile, 4110);
    assert_eq!(plan.edits[&[21, 20]].tile, 4104);
    assert_eq!(plan.edits[&[22, 20]].tile, 4120); // straight water edge
    assert_eq!(plan.edits[&[22, 19]].tile, 4147); // diagonal-only water corner
    assert!(!plan.edits.contains_key(&[21, 21]));
    assert_eq!(
        editor.doc.as_ref().unwrap().terrain([21, 20]),
        Some(previous_edge)
    );
    editor.pointer(&scene, None, false, false, true, None);
    let doc = editor.doc.as_ref().unwrap();
    for (&at, &value) in &plan.edits {
        assert_eq!(doc.terrain(at), Some(value));
        assert_eq!(value.level, 2);
        assert_eq!(value.ice, u8::from(matches!(at, [20 | 21, 20])));
    }
    assert_eq!(doc.terrain([21, 21]), Some(water));
    assert_eq!(doc.overlay([21, 21]), (2, 0));
    let after = doc.snapshot().bytes().unwrap();
    let (reopened, _) = ra2_map_document::source::SourceMap::parse(
        "ice-fill.map".into(),
        "fixture".into(),
        after.clone(),
    )
    .unwrap();
    let reopened = MapDocument::new(reopened.edit_base.unwrap());
    assert_eq!(reopened.terrain([21, 20]), Some(plan.edits[&[21, 20]]));
    editor.history(false, &scene);
    assert_eq!(
        editor.doc.as_ref().unwrap().terrain([21, 20]),
        Some(previous_edge)
    );
    editor.history(true, &scene);
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        after
    );
    editor.history(false, &scene);
    editor.history(false, &scene);
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        before
    );
    editor.fill_style = FillStyle::Brush;
    editor.fill_brush_size = 3;
    editor.ice_fill_growth = true;
    editor.pointer(&scene, Some([20, 20]), true, true, false, None);
    let plan = editor
        .fill
        .generate(editor.doc.as_ref().unwrap(), &scene)
        .unwrap();
    assert_eq!(plan.cells.len(), 8);
    assert_eq!(plan.skipped, BTreeSet::from([[21, 21]]));
    assert!(!plan.cells.contains(&[22, 20]));
    assert_eq!(plan.edits[&[22, 20]].tile, 4120);
    editor.pointer(&scene, Some([20, 20]), false, false, true, None);
    let doc = editor.doc.as_ref().unwrap();
    assert_eq!(doc.terrain([21, 21]), Some(water));
    for (&at, &value) in &plan.edits {
        assert_eq!(doc.terrain(at), Some(value));
        assert_eq!(value.level, 2);
        assert_eq!(value.ice, u8::from(plan.cells.contains(&at)));
    }
    let brushed = doc.snapshot().bytes().unwrap();
    editor.history(false, &scene);
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        before
    );
    editor.history(true, &scene);
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        brushed
    );

    // A resource refresh that disables ice cancels the current gesture without
    // reverting the previous stroke or falling through to the ground brush.
    editor.pointer(&scene, Some([22, 20]), true, true, false, None);
    assert!(editor.stroking);
    let refreshed = setup();
    editor.sync_ice_tool(&refreshed);
    assert_eq!(editor.tool, Tool::Select);
    assert_ne!(editor.terrain_mode, TerrainMode::IceFill);
    assert!(!editor.stroking);
    editor.pointer(&refreshed, Some([22, 20]), false, false, true, None);
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        brushed
    );
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ice_water_edge_keeps_existing_growth_and_skips_protected_and_uneven_water() {
    let (mut editor, _, root) = editor();
    let scene = ice_scene();
    editor.attach(&scene);
    let doc = editor.doc.as_mut().unwrap();
    let water = crate::document::TerrainValue {
        tile: 1,
        sub: 0,
        level: 0,
        ice: 0,
    };
    for (at, value) in [
        ([20, 20], water),
        ([20, 19], crate::document::TerrainValue { ice: 1, ..water }),
        (
            [21, 20],
            crate::document::TerrainValue { level: 1, ..water },
        ),
        ([19, 19], water),
    ] {
        doc.set(Target::Terrain(at), Some(Value::Terrain(value)))
            .unwrap();
    }
    doc.set(Target::Overlay([19, 19]), Some(Value::Overlay(2, 0)))
        .unwrap();
    doc.commit("protected water fixture");
    let plan = ice_tools::plan(
        doc,
        &scene,
        BTreeSet::from([[20, 20]]),
        ice_tools::Options {
            mark_growth: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(plan.edits.len(), 2);
    assert_eq!(plan.edits[&[20, 20]].ice, 0);
    assert_eq!(
        plan.edits[&[20, 19]],
        crate::document::TerrainValue {
            tile: 4116,
            ice: 1,
            ..water
        }
    );
    assert!(!plan.edits.contains_key(&[21, 20]));
    assert!(!plan.edits.contains_key(&[19, 19]));
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

const LAKE_WATER: crate::document::TerrainValue = crate::document::TerrainValue {
    tile: 1,
    sub: 0,
    level: 2,
    ice: 1,
};
const STOCK_BANK: crate::document::TerrainValue = crate::document::TerrainValue {
    tile: 80,
    sub: 1,
    ice: 0,
    ..LAKE_WATER
};

#[test]
fn ice_fill_preserves_occupied_water_in_the_stroke_and_in_its_automatic_edges() {
    let (mut editor, _, root) = editor();
    let scene = ice_scene();
    editor.attach(&scene);
    let doc = editor.doc.as_mut().unwrap();
    let free = [20, 20];
    let selected_ship = [21, 20];
    let neighboring_ship = [20, 19];
    for at in [free, selected_ship, neighboring_ship] {
        doc.set(Target::Terrain(at), Some(Value::Terrain(LAKE_WATER)))
            .unwrap();
    }
    for at in [selected_ship, neighboring_ship] {
        doc.add_record("Units", at, "Neutral", "DEST").unwrap();
    }
    doc.waypoint(0, free).unwrap(); // Waypoints are not physical occupants.
    doc.commit("ships in open water");
    for soften_shore in [false, true] {
        let plan = ice_tools::plan(
            doc,
            &scene,
            [free, selected_ship].into(),
            ice_tools::Options {
                soften_shore,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(plan.cells, [free].into());
        assert_eq!(plan.skipped, [selected_ship].into());
        assert_eq!(plan.edits.keys().copied().collect::<Vec<_>>(), [free]);
        assert_eq!(plan.edits[&free].tile, 4100); // Water to the north and east.
    }
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

fn ice_bank_fixture(doc: &mut MapDocument, corner: bool) -> BTreeSet<Cell> {
    let water = rectangle([20, 20], [26, 23]);
    let bank = (20..=26)
        .map(|x| [x, 19])
        .chain((20..=23).filter(|_| corner).map(|y| [19, y]));
    for (at, value) in water
        .iter()
        .copied()
        .map(|at| (at, LAKE_WATER))
        .chain(bank.map(|at| (at, STOCK_BANK)))
    {
        doc.set(Target::Terrain(at), Some(Value::Terrain(value)))
            .unwrap();
    }
    doc.commit("stock bank fixture");
    water
}

#[test]
fn ice_bank_rebrush_rounds_the_shore_and_can_restore_direct_contact() {
    let (mut editor, _, root) = editor();
    let scene = ice_scene();
    editor.attach(&scene);
    let doc = editor.doc.as_mut().unwrap();
    ice_bank_fixture(doc, true);
    let selection = rectangle([19, 19], [26, 23]);
    let direct = ice_tools::Options {
        soften_shore: false,
        ..Default::default()
    };
    let fill = ice_tools::plan(doc, &scene, selection.clone(), direct).unwrap();
    fill.apply(doc).unwrap();
    doc.commit("old ice touching the bank");
    let before = doc.snapshot().bytes().unwrap();
    editor.activate_terrain_mode(TerrainMode::IceFill);
    editor.pointer(&scene, Some([19, 19]), true, true, false, None);
    // A setting change affects only the next stroke, not this stroke or its preview.
    editor.ice_soften_shore = false;
    editor.pointer(&scene, Some([26, 23]), false, true, false, None);
    let preview = editor
        .fill
        .generate(editor.doc.as_ref().unwrap(), &scene)
        .unwrap();
    let corner = [20, 20];
    let straight = [24, 20];
    let inside = [21, 21];
    assert_eq!(preview.edits[&corner].tile, 4146); // ice diagonally inside the corner
    assert_eq!(preview.edits[&straight].tile, 4116);
    assert_eq!(preview.edits[&straight].ice, 0);
    assert!(preview.cells.contains(&inside));
    assert!(!preview.cells.contains(&corner));
    editor.pointer(&scene, Some([26, 23]), false, false, true, None);
    let doc = editor.doc.as_ref().unwrap();
    assert_eq!(doc.terrain([24, 19]), Some(STOCK_BANK));
    for (&at, &v) in &preview.edits {
        assert_eq!(doc.terrain(at), Some(v));
    }
    for stroke in [
        selection.clone(),
        BTreeSet::from([straight]),
        BTreeSet::from([corner]),
    ] {
        assert!(
            ice_tools::plan(doc, &scene, stroke, Default::default())
                .unwrap()
                .edits
                .is_empty()
        );
    }
    editor.history(false, &scene);
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        before
    );
    editor.history(true, &scene);

    let doc = editor.doc.as_mut().unwrap();
    let restore = ice_tools::plan(doc, &scene, selection, direct).unwrap();
    restore.apply(doc).unwrap();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ice_bank_preserves_narrow_approaches_and_protected_cells() {
    let (mut editor, _, root) = editor();
    let scene = ice_scene();
    editor.attach(&scene);
    let doc = editor.doc.as_mut().unwrap();
    let water = ice_bank_fixture(doc, false);
    for width in [1, 2] {
        let lane = rectangle([21, 20], [20 + width, 23]);
        let fill = ice_tools::plan(doc, &scene, lane.clone(), Default::default()).unwrap();
        assert_eq!(fill.cells, lane);
    }

    let high = crate::document::TerrainValue {
        level: 3,
        ..LAKE_WATER
    };
    // An isolated stretch of land at another height is not a same-level bank.
    for x in 24..=26 {
        doc.set(
            Target::Terrain([x, 19]),
            Some(Value::Terrain(crate::document::TerrainValue {
                level: 3,
                ..STOCK_BANK
            })),
        )
        .unwrap();
    }
    doc.set(Target::Terrain([26, 23]), Some(Value::Terrain(high)))
        .unwrap();
    doc.set(Target::Overlay([22, 22]), Some(Value::Overlay(2, 0)))
        .unwrap();
    doc.add_record("Units", [21, 20], "Neutral", "HTNK")
        .unwrap();
    doc.commit("protected bank fixture");
    let fill = ice_tools::plan(doc, &scene, water, Default::default()).unwrap();
    assert!(fill.skipped.contains(&[21, 20]));
    assert!(!fill.edits.contains_key(&[21, 20]));
    assert!(fill.skipped.contains(&[22, 22]));
    assert!(!fill.edits.contains_key(&[22, 22]));
    // The tank's protected cell does not make this two-cell entry wider.
    assert!(fill.cells.contains(&[22, 20]));
    assert_eq!(fill.edits[&[22, 20]].ice, 1);
    assert!(fill.cells.contains(&[25, 20]));
    assert_eq!(fill.edits[&[26, 23]].level, 3);
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ice_bank_rounds_backed_steps_but_leaves_a_narrow_entry() {
    let (mut editor, _, root) = editor();
    let scene = ice_scene();
    editor.attach(&scene);
    let doc = editor.doc.as_mut().unwrap();
    let tip = [24, 21];
    let lane = rectangle(tip, [25, 26]);
    let mut wide = lane.clone();
    wide.extend(rectangle([26, 22], [28, 26]));
    wide.extend(rectangle([23, 23], [23, 26]));
    let bank = [[23, 21], [24, 20], [25, 20], [23, 22]];
    for (at, v) in wide
        .iter()
        .copied()
        .map(|at| (at, LAKE_WATER))
        .chain(bank.map(|at| (at, STOCK_BANK)))
    {
        doc.set(Target::Terrain(at), Some(Value::Terrain(v)))
            .unwrap();
    }
    doc.commit("short snowbank step");

    let narrow = ice_tools::plan(doc, &scene, lane.clone(), Default::default()).unwrap();
    assert_eq!(narrow.cells, lane);
    let fill = ice_tools::plan(doc, &scene, wide.clone(), Default::default()).unwrap();
    assert_eq!(fill.edits[&tip].tile, 4146); // The tip touches solid ice only diagonally.
    for at in [tip, [25, 21], [24, 22]] {
        assert!(!fill.cells.contains(&at));
        assert_eq!(fill.edits[&at].ice, 0);
    }
    assert!(fill.cells.contains(&[25, 22]));
    assert!(bank.iter().all(|at| !fill.edits.contains_key(at)));
    fill.apply(doc).unwrap();
    for stroke in [wide, BTreeSet::from([tip]), BTreeSet::from([[25, 21]])] {
        assert!(
            ice_tools::plan(doc, &scene, stroke, Default::default())
                .unwrap()
                .edits
                .is_empty()
        );
    }

    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ice_bank_rebrush_mixes_matching_textures_without_long_repeats() {
    let (mut editor, _, root) = editor();
    let single_style = ice_scene();
    editor.attach(&single_style);
    let doc = editor.doc.as_mut().unwrap();
    let water = ice_bank_fixture(doc, false);
    let old_plan = ice_tools::plan(doc, &single_style, water.clone(), Default::default()).unwrap();
    old_plan.apply(doc).unwrap();
    doc.commit("single-style ice edge");

    let mut mixed = ice_scene();
    let palette = Arc::get_mut(&mut mixed.palette).unwrap();
    let key = vera20k_core::map::theater::TileKey {
        tile_id: 4116,
        sub_tile: 0,
        variant: 0,
    };
    let second = 4180; // Same south-facing piece in Ice2.
    let art = palette.images[&key].clone();
    palette.images.insert(
        vera20k_core::map::theater::TileKey {
            tile_id: second,
            ..key
        },
        art,
    );
    palette.raw_ids.insert(i32::from(second), second);
    Arc::make_mut(&mut mixed.catalog)
        .raw_tiles
        .insert(second, i32::from(second));

    let plan = ice_tools::plan(doc, &mixed, water.clone(), Default::default()).unwrap();
    assert!(!plan.edits.is_empty());
    for (&at, v) in &plan.edits {
        assert_eq!(v.tile, i32::from(second));
        assert_eq!(doc.terrain(at).unwrap().tile, 4116);
        assert_eq!(v.ice, 0);
    }
    plan.apply(doc).unwrap();
    let run: Vec<_> = (20..=26)
        .map(|x| doc.terrain([x, 20]).unwrap().tile)
        .collect();
    assert!(run.contains(&4116) && run.contains(&i32::from(second)));
    assert!(run.windows(3).all(|w| w[0] != w[1] || w[1] != w[2]));
    for stroke in [water, BTreeSet::from([[23, 20]])] {
        assert!(
            ice_tools::plan(doc, &mixed, stroke, Default::default())
                .unwrap()
                .edits
                .is_empty()
        );
    }
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}
