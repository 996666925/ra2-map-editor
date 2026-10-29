use crate::{
    cliff_authoring,
    cliff_tools::{self, PlateauPlan},
    coastal_cliffs,
    map::{MapScene, load_scene_bytes},
    road_tools, shore_tools,
    terrain_tools::value,
};
use ra2_map_assets::test_support::Fixture;
use ra2_map_document::{
    document::{MapDocument, Target, TerrainValue, Value},
    new_map::NewMapOptions,
};
use std::{collections::BTreeSet, sync::Arc, time::Instant};

fn setup() -> (MapDocument, MapScene) {
    let f = Fixture::with_joining();
    let mut install = f.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    let s = load_scene_bytes(
        &mut install,
        "joining.map",
        "test".into(),
        NewMapOptions {
            level: 4,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
        Instant::now(),
    )
    .unwrap();
    (
        MapDocument::new(s.source.edit_base.as_ref().unwrap().clone()),
        s,
    )
}
fn complete(doc: &MapDocument, scene: &MapScene, p: &PlateauPlan) {
    for s in &p.cliffs {
        for (at, v) in cliff_authoring::stamp(doc, scene, s.tile, s.origin, s.base).unwrap() {
            assert_eq!(value(doc, scene, at), v, "partial template at {at:?}");
        }
    }
}
fn undoable(doc: &mut MapDocument, p: &PlateauPlan) {
    let before = doc.snapshot().bytes().unwrap();
    p.apply(doc).unwrap();
    doc.commit("joining");
    let after = doc.snapshot().bytes().unwrap();
    if before != after {
        doc.undo();
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        doc.redo();
        assert_eq!(doc.snapshot().bytes().unwrap(), after);
    }
}
#[test]
fn shores_merge_cut_form_islands_and_remove_obsolete_templates_atomically() {
    let (mut d, s) = setup();
    let initial = d.snapshot().bytes().unwrap();
    for (a, b, paint) in [
        ([60, 60], [72, 72], Some(true)),
        ([68, 65], [80, 78], Some(true)),
        ([70, 70], [78, 76], Some(false)),
        ([64, 64], [66, 66], Some(false)),
    ] {
        let p = shore_tools::plan(&d, &s, cliff_tools::rectangle(a, b), paint).unwrap();
        undoable(&mut d, &p);
        complete(&d, &s, &p);
        assert!(
            shore_tools::plan(&d, &s, cliff_tools::rectangle([55, 55], [85, 85]), None)
                .unwrap()
                .edits
                .is_empty()
        );
    }
    assert!(!shore_tools::wet(&s, value(&d, &s, [65, 65])));
    assert!(shore_tools::wet(&s, value(&d, &s, [62, 65])));
    let p = shore_tools::plan(
        &d,
        &s,
        cliff_tools::rectangle([55, 55], [85, 85]),
        Some(false),
    )
    .unwrap();
    undoable(&mut d, &p);
    assert_eq!(
        d.snapshot().bytes().unwrap(),
        initial,
        "full refill must remove every old shore"
    );
}
#[test]
fn shores_handle_two_cell_lakes_and_reject_thin_water_edges_and_missing_assets() {
    let (mut d, mut s) = setup();
    let p = shore_tools::plan(
        &d,
        &s,
        cliff_tools::rectangle([60, 60], [61, 61]),
        Some(true),
    )
    .unwrap();
    undoable(&mut d, &p);
    complete(&d, &s, &p);
    let before = d.snapshot().bytes().unwrap();
    assert!(shore_tools::plan(&d, &s, BTreeSet::from([[75, 75]]), Some(true)).is_err());
    assert!(
        shore_tools::plan(&d, &s, cliff_tools::rectangle([80, 1], [85, 5]), Some(true)).is_err()
    );
    let base = s.palette.tile_ids.unwrap().shore;
    Arc::get_mut(&mut s.palette)
        .unwrap()
        .images
        .retain(|k, _| !(base..base + 40).contains(&i32::from(k.tile_id)));
    assert!(
        shore_tools::plan(
            &d,
            &s,
            cliff_tools::rectangle([70, 70], [78, 78]),
            Some(true)
        )
        .is_err()
    );
    assert_eq!(d.snapshot().bytes().unwrap(), before);
}
#[test]
fn automatic_terrain_protects_entire_foundations_and_overlays() {
    let (mut d, mut s) = setup();
    Arc::make_mut(&mut s.catalog).art =
        Arc::new(vera20k_core::rules::art_data::ArtRegistry::from_ini(
            &vera20k_core::rules::ini_parser::IniFile::from_str("[B]\nFoundation=3x2\n"),
        ));
    d.add_record("Structures", [60, 60], "Neutral", "B")
        .unwrap();
    d.commit("building");
    let before = d.snapshot().bytes().unwrap();
    assert!(
        shore_tools::plan(
            &d,
            &s,
            cliff_tools::rectangle([62, 61], [66, 66]),
            Some(true)
        )
        .is_err()
    );
    assert!(road_tools::plan(&d, &s, [62, 61], [75, 61], false).is_err());
    assert_eq!(d.snapshot().bytes().unwrap(), before);
    d.set(Target::Overlay([75, 75]), Some(Value::Overlay(0, 0)))
        .unwrap();
    d.commit("overlay");
    assert!(
        shore_tools::plan(
            &d,
            &s,
            cliff_tools::rectangle([74, 74], [80, 80]),
            Some(true)
        )
        .is_err()
    );
    assert!(road_tools::plan(&d, &s, [70, 75], [80, 75], false).is_err());
}
#[test]
fn roads_recover_all_curves_and_t_junctions_and_repair_crossing_cuts() {
    for (a, b, c, corner) in [
        ([60, 60], [80, 60], [80, 80], 10),
        ([80, 60], [60, 60], [60, 80], 13),
        ([60, 80], [80, 80], [80, 60], 12),
        ([80, 80], [60, 80], [60, 60], 9),
    ] {
        let (mut d, s) = setup();
        let base = s.palette.tile_ids.unwrap().paved_roads as u16;
        let p = road_tools::plan(&d, &s, a, b, false).unwrap();
        undoable(&mut d, &p);
        let p = road_tools::plan(&d, &s, b, c, false).unwrap();
        assert!(p.cliffs.iter().any(|c| c.tile == base + corner));
        undoable(&mut d, &p);
        complete(&d, &s, &p);
        assert!(
            road_tools::plan(&d, &s, b, c, false)
                .unwrap()
                .edits
                .is_empty(),
            "repeated curve must be idempotent"
        );
    }
    for (end, piece) in [([70, 50], 7), ([70, 90], 5)] {
        let (mut d, s) = setup();
        road_tools::plan(&d, &s, [55, 70], [85, 70], false)
            .unwrap()
            .apply(&mut d)
            .unwrap();
        d.commit("road");
        let p = road_tools::plan(&d, &s, [70, 70], end, false).unwrap();
        assert!(
            p.cliffs
                .iter()
                .any(|c| i32::from(c.tile) == s.palette.tile_ids.unwrap().paved_roads + piece)
        );
        undoable(&mut d, &p);
    }
    let (mut d, s) = setup();
    for (a, b, cut) in [
        ([55, 70], [85, 70], false),
        ([70, 50], [70, 90], false),
        ([66, 70], [74, 70], true),
    ] {
        let p = road_tools::plan(&d, &s, a, b, cut).unwrap();
        undoable(&mut d, &p);
        complete(&d, &s, &p);
    }
    assert!(shore_tools::ground(&s, value(&d, &s, [70, 70])));
}
#[test]
fn roads_reject_broken_templates_dense_bends_and_resource_loss_without_writes() {
    let (mut d, mut s) = setup();
    road_tools::plan(&d, &s, [60, 60], [80, 60], false)
        .unwrap()
        .apply(&mut d)
        .unwrap();
    d.commit("road");
    let before = d.snapshot().bytes().unwrap();
    let p = road_tools::plan(&d, &s, [80, 60], [80, 75], false).unwrap();
    undoable(&mut d, &p);
    assert!(road_tools::plan(&d, &s, [80, 62], [90, 62], false).is_err());
    d.undo();
    assert_eq!(d.snapshot().bytes().unwrap(), before);
    let clear = cliff_authoring::clear(&s, value(&d, &s, [65, 60]), 4).unwrap();
    d.set(Target::Terrain([65, 60]), Some(Value::Terrain(clear)))
        .unwrap();
    d.commit("manual partial overwrite");
    assert!(road_tools::plan(&d, &s, [60, 60], [80, 60], false).is_err());
    d.undo();
    let tile = s.palette.tile_ids.unwrap().paved_roads as u16 + 2;
    Arc::get_mut(&mut s.palette)
        .unwrap()
        .images
        .retain(|k, _| k.tile_id != tile);
    assert!(road_tools::plan(&d, &s, [70, 50], [70, 75], false).is_err());
    assert_eq!(d.snapshot().bytes().unwrap(), before);
}
#[test]
fn coastal_cliffs_switch_both_ways_and_are_recognized_by_height_tools() {
    let (mut d, s) = setup();
    let ids = s.palette.tile_ids.unwrap();
    let tile = ids.cliffs.cliff_set.unwrap() + 7;
    for (at, v) in cliff_authoring::stamp(&d, &s, tile, [60, 60], 4).unwrap() {
        d.set(Target::Terrain(at), Some(Value::Terrain(v))).unwrap();
    }
    let water = ids.water_base + 8;
    d.set(
        Target::Terrain([60, 62]),
        Some(Value::Terrain(TerrainValue {
            tile: s.catalog.raw_tiles[&(water as u16)],
            sub: 0,
            ..value(&d, &s, [60, 62])
        })),
    )
    .unwrap();
    d.commit("coast fixture");
    let selected = BTreeSet::from([[60, 60]]);
    let p = coastal_cliffs::plan(&d, &s, selected.clone()).unwrap();
    assert!(
        p.cliffs
            .iter()
            .all(|c| c.tile == ids.cliffs.water_cliffs.unwrap() + 7)
    );
    undoable(&mut d, &p);
    complete(&d, &s, &p);
    assert_eq!(cliff_authoring::infer_base(&d, &s, [60, 60]), 4);
    let clear = cliff_authoring::clear(&s, value(&d, &s, [60, 62]), 4).unwrap();
    d.set(Target::Terrain([60, 62]), Some(Value::Terrain(clear)))
        .unwrap();
    d.commit("fill around cliff");
    let p = coastal_cliffs::plan(&d, &s, selected).unwrap();
    assert!(p.cliffs.iter().all(|c| c.tile == tile));
    undoable(&mut d, &p);
}

#[test]
fn repair_joins_raw_water_and_water_strokes_update_adjacent_cliff_templates() {
    let (mut d, s) = setup();
    let ids = s.palette.tile_ids.unwrap();
    let water = ids.water_base + 8;
    for at in cliff_tools::rectangle([60, 60], [72, 72]) {
        d.set(
            Target::Terrain(at),
            Some(Value::Terrain(TerrainValue {
                tile: s.catalog.raw_tiles[&(water as u16)],
                sub: 0,
                ..value(&d, &s, at)
            })),
        )
        .unwrap();
    }
    d.commit("raw water");
    let p = shore_tools::plan(&d, &s, cliff_tools::rectangle([58, 58], [74, 74]), None).unwrap();
    assert!(!p.edits.is_empty());
    undoable(&mut d, &p);
    complete(&d, &s, &p);
    let (mut d, s) = setup();
    let ids = s.palette.tile_ids.unwrap();
    for x in 55..=85 {
        for (at, v) in
            cliff_authoring::stamp(&d, &s, ids.cliffs.cliff_set.unwrap() + 7, [x, 60], 4).unwrap()
        {
            d.set(Target::Terrain(at), Some(Value::Terrain(v))).unwrap();
        }
    }
    d.commit("cliff line");
    let p = shore_tools::plan(
        &d,
        &s,
        cliff_tools::rectangle([60, 62], [80, 76]),
        Some(true),
    )
    .unwrap();
    assert!(
        p.cliffs
            .iter()
            .any(|c| c.tile == ids.cliffs.water_cliffs.unwrap() + 7)
    );
    undoable(&mut d, &p);
    complete(&d, &s, &p);
    let p = shore_tools::plan(
        &d,
        &s,
        cliff_tools::rectangle([58, 62], [82, 78]),
        Some(false),
    )
    .unwrap();
    assert!(
        p.cliffs
            .iter()
            .any(|c| c.tile == ids.cliffs.cliff_set.unwrap() + 7)
    );
    undoable(&mut d, &p);
}

#[test]
fn raised_bridges_replace_complete_water_cliffs_at_coastal_landings() {
    let (mut d, s) = setup();
    let ids = s.palette.tile_ids.unwrap();
    let mut banks = Vec::new();
    for (a, b) in [([60, 60], [79, 74]), ([94, 60], [108, 74])] {
        let p = cliff_tools::plan(&d, &s, cliff_tools::rectangle(a, b), 4).unwrap();
        banks.extend(
            p.cliffs
                .iter()
                .filter(|c| c.tile < ids.cliffs.cliff_set.unwrap() + 22)
                .map(|c| c.origin),
        );
        p.apply(&mut d).unwrap();
        d.commit("bank");
    }
    let water = s.catalog.raw_tiles[&((ids.water_base + 8) as u16)];
    for at in cliff_tools::rectangle([80, 60], [93, 74]) {
        let old = value(&d, &s, at);
        if old.level == 4 && shore_tools::ground(&s, old) {
            d.set(
                Target::Terrain(at),
                Some(Value::Terrain(TerrainValue {
                    tile: water,
                    sub: 0,
                    ..old
                })),
            )
            .unwrap();
        }
    }
    d.commit("channel");
    let coast = coastal_cliffs::plan(&d, &s, banks.into_iter().collect()).unwrap();
    assert!(!coast.edits.is_empty());
    coast.apply(&mut d).unwrap();
    d.commit("water cliffs");
    let p = crate::bridge_tools::plan(
        &d,
        &s,
        [80, 67],
        [94, 67],
        crate::bridge_tools::BridgeKind::HighConcrete,
    )
    .unwrap();
    undoable(&mut d, &p);
    complete(&d, &s, &p);
}
