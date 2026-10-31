use super::*;
use ra2_map_assets::{
    catalog::{TerrainSubtile, TerrainTemplate},
    terrain_library::TerrainGroup,
    test_support::Fixture,
};
use ra2_map_document::{new_map::NewMapOptions, source::SourceMap};
use std::{sync::Arc, time::Instant};
use vera20k_core::map::theater::TileKey;

fn setup() -> (MapDocument, MapScene, TerrainBrush) {
    let fixture = Fixture::with_joining();
    let mut install = fixture.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    let mut scene = crate::map::load_scene_bytes(
        &mut install,
        "fill.map",
        "test".into(),
        NewMapOptions {
            width: 40,
            height: 40,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
        Instant::now(),
    )
    .unwrap();
    drop(install);
    let image = scene.palette.images.values().next().unwrap().clone();
    let p = Arc::get_mut(&mut scene.palette).unwrap();
    for id in std::iter::once(4000).chain(4010..4026) {
        p.templates.insert(
            (id, 0),
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
            id,
            TerrainGroup {
                id: 4000,
                name: "Rough".into(),
                category: TerrainCategory::Ground,
            },
        );
        p.raw_ids.insert(i32::from(id), id);
        p.images.insert(
            TileKey {
                tile_id: id,
                sub_tile: 0,
                variant: 0,
            },
            image.clone(),
        );
        Arc::make_mut(&mut scene.catalog)
            .raw_tiles
            .insert(id, i32::from(id));
    }
    p.tile_ids.as_mut().unwrap().rough = 4000;
    p.tile_ids.as_mut().unwrap().rough_lat = 4010;
    let brush = TerrainBrush {
        key: TileKey {
            tile_id: 4000,
            sub_tile: 0,
            variant: 0,
        },
        raw: 4000,
        label: "Rough".into(),
    };
    (
        MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone()),
        scene,
        brush,
    )
}
fn put(doc: &mut MapDocument, scene: &MapScene, at: Cell, tile: i32, level: u8, ice: u8) {
    doc.set(
        Target::Terrain(at),
        Some(Value::Terrain(TerrainValue {
            tile,
            level,
            ice,
            ..value(doc, scene, at)
        })),
    )
    .unwrap();
}

#[test]
fn rectangle_replaces_mixed_ground_preserves_height_ice_and_structures_and_clips_size() {
    let (mut doc, scene, brush) = setup();
    put(&mut doc, &scene, [40, 40], 4000, 3, 7);
    put(&mut doc, &scene, [41, 40], -1, 4, 8);
    let cliff = scene
        .brushes
        .iter()
        .find(|b| scene.palette.groups[&b.key.tile_id].category == TerrainCategory::Cliff)
        .unwrap();
    put(&mut doc, &scene, [40, 41], cliff.raw, 0, 0);
    let road = scene
        .brushes
        .iter()
        .find(|b| scene.palette.groups[&b.key.tile_id].category == TerrainCategory::Road)
        .unwrap();
    put(&mut doc, &scene, [41, 41], road.raw, 0, 0);
    doc.set(Target::Overlay([42, 40]), Some(Value::Overlay(2, 5)))
        .unwrap();
    doc.commit("fixture");
    let before = doc.snapshot().bytes().unwrap();
    let mut selected = crate::cliff_tools::rectangle([42, 41], [40, 40]);
    selected.insert([0, 0]);
    let plan = plan(&doc, &scene, &brush, selected, false).unwrap();
    assert_eq!(plan.cells.len(), 4);
    assert_eq!(plan.skipped, BTreeSet::from([[40, 41], [41, 41]]));
    assert_eq!(
        doc.snapshot().bytes().unwrap(),
        before,
        "preview is read-only"
    );
    plan.apply(&mut doc).unwrap();
    for &at in &plan.cells {
        assert_eq!(doc.terrain(at).unwrap().tile, 4000);
    }
    assert_eq!(
        (
            doc.terrain([41, 40]).unwrap().level,
            doc.terrain([41, 40]).unwrap().ice
        ),
        (4, 8)
    );
    assert_eq!(doc.terrain([40, 41]).unwrap().tile, cliff.raw);
    assert_eq!(
        doc.terrain([39, 40]).unwrap().tile,
        0,
        "no flood beyond selection"
    );
    assert_eq!(doc.overlay([42, 40]), (2, 5));
    doc.commit("fill");
    let saved = doc.snapshot().bytes().unwrap();
    let (source, _) = SourceMap::parse("saved.map".into(), "test".into(), saved.clone()).unwrap();
    assert_eq!(
        MapDocument::new(source.edit_base.unwrap())
            .terrain([41, 40])
            .unwrap()
            .tile,
        4000
    );
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    assert!(doc.redo());
    assert_eq!(doc.snapshot().bytes().unwrap(), saved);
}

#[test]
fn lat_preview_includes_neighbor_repairs_and_matches_committed_values() {
    let (mut doc, scene, brush) = setup();
    put(&mut doc, &scene, [39, 40], 4000, 0, 0);
    let before = doc.snapshot().bytes().unwrap();
    let p = plan(&doc, &scene, &brush, BTreeSet::from([[40, 40]]), true).unwrap();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    assert!(
        p.edits.contains_key(&[39, 40]),
        "outside LAT repair is part of the preview"
    );
    assert_ne!(
        p.edits[&[40, 40]].tile,
        4000,
        "single-cell boundary uses transition art"
    );
    p.apply(&mut doc).unwrap();
    for (&at, &v) in &p.edits {
        assert_eq!(doc.terrain(at), Some(v));
    }
    assert_eq!(doc.terrain([38, 40]).unwrap().tile, 0);
    let p = plan(&doc, &scene, &brush, BTreeSet::from([[40, 40]]), false).unwrap();
    assert_eq!(p.edits.len(), 1, "LAT off writes strictly within selection");
    assert_eq!(p.edits[&[40, 40]].tile, 4000);
}

#[test]
fn fill_material_rejects_structural_tiles_slopes_lat_edges_and_missing_resources() {
    let (_, mut scene, brush) = setup();
    assert!(material(&scene, &brush).is_ok());
    for b in &scene.brushes {
        if scene.palette.groups[&b.key.tile_id].category != TerrainCategory::Ground {
            assert!(material(&scene, b).is_err());
        }
    }
    let mut lat = brush.clone();
    lat.key.tile_id = 4011;
    lat.raw = 4011;
    assert!(material(&scene, &lat).is_err());
    let p = Arc::get_mut(&mut scene.palette).unwrap();
    p.templates.get_mut(&(4000, 0)).unwrap().cells[0]
        .as_mut()
        .unwrap()
        .slope = 1;
    assert!(material(&scene, &brush).is_err());
    let p = Arc::get_mut(&mut scene.palette).unwrap();
    p.templates.get_mut(&(4000, 0)).unwrap().cells[0]
        .as_mut()
        .unwrap()
        .slope = 0;
    p.images.remove(&brush.key);
    assert!(material(&scene, &brush).is_err());
}
