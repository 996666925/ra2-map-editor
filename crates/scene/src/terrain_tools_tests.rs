use super::*;
use ra2_map_assets::{catalog::TerrainSubtile, test_support::Fixture};
use ra2_map_document::{new_map::NewMapOptions, source::SourceMap};
use std::{sync::Arc, time::Instant};
use vera20k_core::{map::theater::TileKey, rules::ini_parser::IniFile};

fn setup() -> (MapDocument, MapScene) {
    let fixture = Fixture::new();
    let mut install = fixture.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    let scene = crate::map::load_scene_bytes(
        &mut install,
        "tools.map",
        "fixture".into(),
        NewMapOptions {
            width: 20,
            height: 20,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
        Instant::now(),
    )
    .unwrap();
    let doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
    (doc, scene)
}

fn brush(scene: &mut MapScene, id: u16, width: u16, heights: &[Option<u8>]) -> TerrainBrush {
    let image = scene.palette.images.values().next().unwrap().clone();
    let palette = Arc::get_mut(&mut scene.palette).unwrap();
    palette.templates.insert(
        (id, 0),
        TerrainTemplate {
            width,
            height: heights.len() as u16 / width,
            cells: heights
                .iter()
                .map(|h| {
                    h.map(|height| TerrainSubtile {
                        height,
                        slope: 0,
                        terrain: 0,
                    })
                })
                .collect(),
        },
    );
    for (i, h) in heights.iter().enumerate() {
        if h.is_some() {
            palette.images.insert(
                TileKey {
                    tile_id: id,
                    sub_tile: i as u8,
                    variant: 0,
                },
                image.clone(),
            );
        }
    }
    Arc::make_mut(&mut scene.catalog)
        .raw_tiles
        .insert(id, 1000 + i32::from(id));
    TerrainBrush {
        key: TileKey {
            tile_id: id,
            sub_tile: 0,
            variant: 0,
        },
        raw: 1000 + i32::from(id),
        label: "fixture".into(),
    }
}

fn put(doc: &mut MapDocument, scene: &MapScene, at: Cell, tile: i32, level: u8) {
    doc.set(
        Target::Terrain(at),
        Some(Value::Terrain(TerrainValue {
            tile,
            level,
            ..value(doc, scene, at)
        })),
    )
    .unwrap();
}

#[test]
fn large_map_templates_and_neighbor_tools_cross_the_retail_grid_boundary() {
    let (_, mut scene) = setup();
    let bytes = String::from_utf8(NewMapOptions::default().bytes().unwrap())
        .unwrap()
        .replace("Size=0,0,80,80", "Size=0,0,500,500")
        .into_bytes();
    let (source, _) = SourceMap::parse("large.map".into(), "fixture".into(), bytes).unwrap();
    let mut doc = MapDocument::new(source.edit_base.unwrap());
    let b = brush(&mut scene, 2, 2, &[Some(0), Some(1), Some(2), Some(3)]);
    stamp_at_height(&mut doc, &scene, &b, [511, 600], true, Some(3)).unwrap();
    assert_eq!(doc.terrain([512, 601]).unwrap().level, 6);
    assert!(neighbors([511, 600]).any(|c| c == [512, 600]));
    assert_eq!(
        crate::shore_tools::offset([511, 600], 1, 0),
        Some([512, 600])
    );
    doc.commit("extended template");
    let (_, map) = SourceMap::parse(
        "saved.map".into(),
        "fixture".into(),
        doc.snapshot().bytes().unwrap(),
    )
    .unwrap();
    assert!(
        map.cells
            .iter()
            .any(|c| [c.rx, c.ry] == [512, 601] && c.z == 6)
    );
    assert!(doc.undo());
    assert!(doc.terrain([512, 601]).is_none());
}

#[test]
fn cliff_base_height_preflight_matches_stamp_and_rejects_overflow_without_partial_writes() {
    let (mut doc, mut scene) = setup();
    let mut b = brush(&mut scene, 2, 2, &[Some(0), Some(4), None, Some(2)]);
    b.key.sub_tile = 1; // High anchor on existing level zero used to prevent placement.
    let before = doc.snapshot().bytes().unwrap();
    assert!(template_placement(&doc, &scene, &b, [20, 20], None).is_err());
    let planned = template_placement(&doc, &scene, &b, [20, 20], Some(3)).unwrap();
    stamp_at_height(&mut doc, &scene, &b, [20, 20], true, Some(3)).unwrap();
    for (at, value) in planned {
        assert_eq!(doc.terrain(at), Some(value));
    }
    assert_eq!(doc.terrain([20, 20]).unwrap().level, 7);
    assert_eq!(doc.terrain([19, 20]).unwrap().level, 3);
    doc.commit("cliff");
    let after = doc.snapshot().bytes().unwrap();
    assert!(stamp_at_height(&mut doc, &scene, &b, [25, 25], true, Some(11)).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), after);
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.redo();
    assert_eq!(doc.snapshot().bytes().unwrap(), after);
}

#[test]
fn visual_variants_with_different_geometry_cannot_be_authored_as_the_pristine_template() {
    let (mut doc, mut scene) = setup();
    let mut b = brush(&mut scene, 2, 2, &[Some(0), Some(4)]);
    let palette = Arc::get_mut(&mut scene.palette).unwrap();
    palette.templates.insert(
        (2, 1),
        TerrainTemplate {
            width: 2,
            height: 1,
            cells: vec![
                Some(TerrainSubtile {
                    height: 0,
                    slope: 0,
                    terrain: 0,
                }),
                Some(TerrainSubtile {
                    height: 3,
                    slope: 0,
                    terrain: 0,
                }),
            ],
        },
    );
    for sub_tile in 0..2 {
        let image = palette.images[&TileKey {
            tile_id: 2,
            sub_tile,
            variant: 0,
        }]
            .clone();
        palette.images.insert(
            TileKey {
                tile_id: 2,
                sub_tile,
                variant: 1,
            },
            image,
        );
    }
    b.key.variant = 1;
    let before = doc.snapshot().bytes().unwrap();
    assert!(stamp_at_height(&mut doc, &scene, &b, [20, 20], true, Some(2)).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    Arc::get_mut(&mut scene.palette)
        .unwrap()
        .templates
        .get_mut(&(2, 1))
        .unwrap()
        .cells[1]
        .as_mut()
        .unwrap()
        .height = 4;
    stamp_at_height(&mut doc, &scene, &b, [20, 20], true, Some(2)).unwrap();
    assert_eq!(doc.terrain([21, 20]).unwrap().level, 6);
}

#[test]
fn sparse_template_anchors_selected_subtile_preserves_height_and_ice_and_is_atomic() {
    let (mut doc, mut scene) = setup();
    let mut b = brush(
        &mut scene,
        2,
        3,
        &[Some(0), None, Some(2), Some(1), None, Some(2)],
    );
    b.key.sub_tile = 5;
    put(&mut doc, &scene, [20, 20], 0, 5);
    let old = value(&doc, &scene, [18, 19]);
    doc.set(
        Target::Terrain([18, 19]),
        Some(Value::Terrain(TerrainValue { ice: 7, ..old })),
    )
    .unwrap();
    doc.commit("fixture");
    let before = doc.snapshot().bytes().unwrap();
    let changed = stamp(&mut doc, &scene, &b, [20, 20], true).unwrap();
    assert_eq!(changed.len(), 4);
    assert_eq!(
        doc.terrain([18, 19]).unwrap(),
        TerrainValue {
            tile: 1002,
            sub: 0,
            level: 3,
            ice: 7
        }
    );
    assert_eq!(doc.terrain([20, 20]).unwrap().sub, 5);
    assert_eq!(doc.terrain([20, 19]).unwrap().level, 5);
    assert_eq!(doc.terrain([19, 19]).unwrap().tile, 0, "hole untouched");
    doc.commit("template");
    let after = doc.snapshot().bytes().unwrap();
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.redo();
    assert_eq!(doc.snapshot().bytes().unwrap(), after);
    assert!(stamp(&mut doc, &scene, &b, [1, 20], true).is_err());
    assert_eq!(
        doc.snapshot().bytes().unwrap(),
        after,
        "no partial template at boundary"
    );
    assert!(
        stamp(&mut doc, &scene, &b, [22, 22], true).is_err(),
        "negative template base"
    );
    assert_eq!(doc.snapshot().bytes().unwrap(), after);
}

#[test]
fn fill_is_contiguous_height_bounded_and_accepts_only_single_cell_templates() {
    let (mut doc, mut scene) = setup();
    let b = brush(&mut scene, 2, 1, &[Some(0)]);
    for at in [[20, 20], [21, 20], [21, 21], [24, 24]] {
        put(&mut doc, &scene, at, 9, 0);
    }
    put(&mut doc, &scene, [20, 21], 9, 1);
    doc.commit("fixture");
    let before = doc.snapshot().bytes().unwrap();
    assert_eq!(fill(&mut doc, &scene, &b, [20, 20]).unwrap().len(), 3);
    assert_eq!(doc.terrain([24, 24]).unwrap().tile, 9);
    assert_eq!(doc.terrain([20, 21]).unwrap().level, 1);
    doc.commit("fill");
    let after = doc.snapshot().bytes().unwrap();
    let (source, _) =
        SourceMap::parse("reopen.map".into(), "fixture".into(), after.clone()).unwrap();
    let reopened = MapDocument::new(source.edit_base.unwrap());
    assert_eq!(reopened.terrain([21, 21]).unwrap().tile, b.raw);
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.redo();
    assert_eq!(doc.snapshot().bytes().unwrap(), after);
    let multi = brush(&mut scene, 3, 2, &[Some(0), Some(0)]);
    assert!(fill(&mut doc, &scene, &multi, [20, 20]).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), after);
}

#[test]
fn incomplete_template_resources_are_rejected_before_writing_any_subtile() {
    let (mut doc, mut scene) = setup();
    let b = brush(&mut scene, 3, 2, &[Some(0), Some(0)]);
    Arc::get_mut(&mut scene.palette)
        .unwrap()
        .images
        .remove(&TileKey {
            sub_tile: 1,
            ..b.key
        });
    let before = doc.snapshot().bytes().unwrap();
    assert!(stamp(&mut doc, &scene, &b, [20, 20], true).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

fn ramps(scene: &mut MapScene) {
    for id in 200..218 {
        brush(scene, id, 1, &[Some(0)]);
        Arc::get_mut(&mut scene.palette)
            .unwrap()
            .templates
            .get_mut(&(id, 0))
            .unwrap()
            .cells[0]
            .as_mut()
            .unwrap()
            .slope = (id - 199) as u8;
    }
    let palette = Arc::get_mut(&mut scene.palette).unwrap();
    palette.tile_ids.as_mut().unwrap().ramp_base = 200;
    palette.morphable.insert(0);
}

#[test]
fn height_clamps_raw_levels_and_constructs_smooth_plateau_with_transactional_undo() {
    let (mut doc, mut scene) = setup();
    ramps(&mut scene);
    let before = doc.snapshot().bytes().unwrap();
    let selected = BTreeSet::from([[20, 20], [21, 20], [20, 21], [21, 21]]);
    let changed = height(&mut doc, &scene, &selected, TerrainMode::Raise, 0, true).unwrap();
    assert!(changed.len() > selected.len(), "neighbor ramps included");
    for at in &selected {
        assert_eq!(doc.terrain(*at).unwrap().level, 1);
    }
    assert!(
        changed
            .iter()
            .any(|&at| doc.terrain(at).unwrap().tile >= 1200)
    );
    doc.commit("raise");
    let raised = doc.snapshot().bytes().unwrap();
    height(&mut doc, &scene, &selected, TerrainMode::Flatten, 0, true).unwrap();
    for at in &selected {
        assert_eq!(doc.terrain(*at).unwrap().level, 0);
        assert_eq!(doc.terrain(*at).unwrap().tile, 0);
    }
    doc.cancel();
    assert_eq!(doc.snapshot().bytes().unwrap(), raised);
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    height(&mut doc, &scene, &selected, TerrainMode::Flatten, 14, false).unwrap();
    assert!(
        height(&mut doc, &scene, &selected, TerrainMode::Raise, 0, false)
            .unwrap()
            .is_empty()
    );
    for at in &selected {
        assert_eq!(doc.terrain(*at).unwrap().level, 14);
    }
}

#[test]
fn smooth_height_rejects_occupied_cells_without_partial_changes() {
    let (mut doc, mut scene) = setup();
    ramps(&mut scene);
    doc.set(Target::Overlay([21, 21]), Some(Value::Overlay(1, 0)))
        .unwrap();
    doc.commit("fixture");
    let before = doc.snapshot().bytes().unwrap();
    assert!(
        height(
            &mut doc,
            &scene,
            &BTreeSet::from([[20, 20], [21, 21]]),
            TerrainMode::Raise,
            0,
            true
        )
        .is_err()
    );
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn flatten_removes_a_slope_even_when_its_base_already_equals_the_target() {
    let (mut doc, mut scene) = setup();
    ramps(&mut scene);
    let selected = BTreeSet::from([[20, 20]]);
    let changed = height(&mut doc, &scene, &selected, TerrainMode::Raise, 0, true).unwrap();
    let slope = changed
        .into_iter()
        .find(|&at| doc.terrain(at).unwrap().level == 0 && doc.terrain(at).unwrap().tile >= 1200)
        .unwrap();
    height(
        &mut doc,
        &scene,
        &BTreeSet::from([slope]),
        TerrainMode::Flatten,
        0,
        true,
    )
    .unwrap();
    assert_eq!(doc.terrain(slope).unwrap().tile, 0);
    assert_eq!(doc.terrain(slope).unwrap().level, 0);
}

#[test]
fn smooth_height_protects_building_foundation_cells_beyond_the_origin() {
    let (mut doc, mut scene) = setup();
    ramps(&mut scene);
    Arc::make_mut(&mut scene.catalog).art =
        Arc::new(vera20k_core::rules::art_data::ArtRegistry::from_ini(
            &IniFile::from_str("[BUILDING]\nFoundation=3x2\n"),
        ));
    doc.add_record("Structures", [20, 20], "Neutral", "BUILDING")
        .unwrap();
    doc.commit("fixture");
    let before = doc.snapshot().bytes().unwrap();
    assert!(
        height(
            &mut doc,
            &scene,
            &BTreeSet::from([[22, 21]]),
            TerrainMode::Raise,
            0,
            true
        )
        .is_err()
    );
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn local_lat_uses_translated_ids_repairs_neighbors_and_leaves_distant_cells() {
    let (mut doc, mut scene) = setup();
    brush(&mut scene, 1, 1, &[Some(0)]);
    for id in 100..116 {
        brush(&mut scene, id, 1, &[Some(0)]);
    }
    let ids = Arc::get_mut(&mut scene.palette)
        .unwrap()
        .tile_ids
        .as_mut()
        .unwrap();
    ids.rough = 1;
    ids.rough_lat = 100;
    for at in [[20, 20], [21, 20], [29, 29]] {
        put(&mut doc, &scene, at, 1001, 0);
    }
    doc.commit("fixture");
    let before = doc.snapshot().bytes().unwrap();
    recalculate(&mut doc, &scene, &BTreeSet::from([[20, 20]])).unwrap();
    assert_eq!(doc.terrain([20, 20]).unwrap().tile, 1113, "N/S/W edges");
    assert_eq!(doc.terrain([21, 20]).unwrap().tile, 1107, "N/E/S edges");
    assert_eq!(
        doc.terrain([29, 29]).unwrap().tile,
        1001,
        "distant cell preserved"
    );
    doc.commit("lat");
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn walls_connect_same_types_preserve_damage_and_repair_after_delete_in_one_undo() {
    let (mut doc, mut scene) = setup();
    let catalog = Arc::make_mut(&mut scene.catalog);
    catalog.overlays = vec!["WALL".into(), "OTHER".into()];
    catalog.rules = IniFile::from_str("[WALL]\nWall=yes\n[OTHER]\nWall=yes\n");
    for at in [[20, 20], [20, 19], [21, 20], [20, 21]] {
        doc.set(Target::Overlay(at), Some(Value::Overlay(0, 0x20)))
            .unwrap();
    }
    doc.set(Target::Overlay([19, 20]), Some(Value::Overlay(1, 0)))
        .unwrap();
    connect_walls(&mut doc, &scene, &BTreeSet::from([[20, 20]])).unwrap();
    assert_eq!(doc.overlay([20, 20]), (0, 0x27));
    assert_eq!(doc.overlay([20, 19]), (0, 0x24));
    assert_eq!(doc.overlay([19, 20]), (1, 0));
    doc.commit("walls");
    let before = doc.snapshot().bytes().unwrap();
    doc.set(Target::Overlay([20, 20]), Some(Value::Overlay(255, 0)))
        .unwrap();
    connect_walls(&mut doc, &scene, &BTreeSet::from([[20, 20]])).unwrap();
    assert_eq!(doc.overlay([20, 19]), (0, 0x20));
    assert_eq!(doc.overlay([21, 20]), (0, 0x20));
    doc.commit("delete");
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}
