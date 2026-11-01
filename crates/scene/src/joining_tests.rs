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
fn freehand_water_outlines_fit_diagonal_edges_and_small_pointer_jitter() {
    let (mut d, s) = setup();
    let path = [
        [60, 70],
        [63, 65],
        [68, 61],
        [72, 60],
        [75, 63],
        [79, 65],
        [81, 70],
        [80, 74],
        [76, 78],
        [71, 80],
        [66, 78],
        [62, 75],
    ];
    let raw = cliff_tools::enclose(&path).unwrap();
    assert!(
        shore_tools::plan(&d, &s, raw, Some(true)).is_err(),
        "fixture must reproduce the one-cell shoreline failure"
    );
    let before = d.snapshot().bytes().unwrap();
    let p = shore_tools::outline(&d, &s, &path).unwrap();
    assert_eq!(d.snapshot().bytes().unwrap(), before);
    undoable(&mut d, &p);
    complete(&d, &s, &p);
}

#[test]
fn freehand_water_is_bounded_reversible_and_preserves_valid_contours() {
    let (mut d, s) = setup();
    for path in [
        vec![[60, 70], [70, 60], [80, 70], [70, 80]],
        vec![[60, 60], [78, 60], [78, 66], [69, 66], [69, 78], [60, 78]],
        vec![
            [60, 60],
            [67, 60],
            [68, 59],
            [69, 60],
            [78, 60],
            [78, 68],
            [77, 69],
            [78, 70],
            [78, 78],
            [60, 78],
        ],
    ] {
        for shift in [[0, 0], [1, 0], [0, 1], [1, 1]] {
            let mut path: Vec<_> = path
                .iter()
                .map(|p| [p[0] + shift[0], p[1] + shift[1]])
                .collect();
            let raw = cliff_tools::enclose(&path).unwrap();
            let p = shore_tools::outline(&d, &s, &path).unwrap();
            assert!(raw.is_subset(&p.interior), "removed enclosed water");
            assert!(
                p.interior
                    .iter()
                    .all(|at| (-1..=1).any(|dx| (-1..=1).any(|dy| {
                        shore_tools::offset(*at, dx, dy).is_some_and(|n| raw.contains(&n))
                    }))),
                "expanded by more than one cell"
            );
            if let Ok(exact) = shore_tools::plan(&d, &s, raw.clone(), Some(true)) {
                assert_eq!(p.edits, exact.edits, "changed a valid contour");
            }
            path.reverse();
            assert_eq!(p.edits, shore_tools::outline(&d, &s, &path).unwrap().edits);
            undoable(&mut d, &p);
            complete(&d, &s, &p);
            assert!(
                raw.iter()
                    .all(|&at| shore_tools::wet(&s, value(&d, &s, at)))
            );
            assert!(
                shore_tools::outline(&d, &s, &path)
                    .unwrap()
                    .edits
                    .is_empty(),
                "repaint changed contour"
            );
            let repair = cliff_tools::rectangle([54, 54], [86, 86]);
            assert!(
                shore_tools::plan(&d, &s, repair, None)
                    .unwrap()
                    .edits
                    .is_empty()
            );
            d.undo();
        }
    }
}

#[test]
fn snapped_water_outlines_protect_added_cells_and_reject_missing_shores() {
    let (mut d, mut s) = setup();
    let path = [[60, 70], [70, 60], [80, 70], [70, 80]];
    let raw = cliff_tools::enclose(&path).unwrap();
    let p = shore_tools::outline(&d, &s, &path).unwrap();
    let at = *p.interior.difference(&raw).next().unwrap();
    for object in [false, true] {
        if object {
            d.add_record("Structures", at, "Neutral", "B").unwrap();
        } else {
            d.set(Target::Overlay(at), Some(Value::Overlay(0, 0)))
                .unwrap();
        }
        let before = d.snapshot().bytes().unwrap();
        assert!(shore_tools::outline(&d, &s, &path).is_err());
        assert_eq!(d.snapshot().bytes().unwrap(), before);
        d.cancel();
    }
    // Outline expansion must not hide a height mismatch or cross map borders.
    d.set(
        Target::Terrain(at),
        Some(Value::Terrain(TerrainValue {
            level: 5,
            ..value(&d, &s, at)
        })),
    )
    .unwrap();
    let before = d.snapshot().bytes().unwrap();
    assert!(shore_tools::outline(&d, &s, &path).is_err());
    assert_eq!(d.snapshot().bytes().unwrap(), before);
    d.cancel();
    assert!(shore_tools::outline(&d, &s, &[[80, 1], [86, 1], [86, 7], [80, 7]]).is_err());
    let base = s.palette.tile_ids.unwrap().shore;
    Arc::get_mut(&mut s.palette)
        .unwrap()
        .images
        .retain(|k, _| !(base..base + 40).contains(&i32::from(k.tile_id)));
    let before = d.snapshot().bytes().unwrap();
    assert!(shore_tools::outline(&d, &s, &path).is_err());
    assert_eq!(d.snapshot().bytes().unwrap(), before);
}

fn shore_ground(scene: &mut MapScene) {
    use ra2_map_assets::catalog::{TerrainSubtile, TerrainTemplate};
    use vera20k_core::map::theater::TileKey;
    let image = scene.palette.images.values().next().unwrap().clone();
    let palette = Arc::get_mut(&mut scene.palette).unwrap();
    let ids = palette.tile_ids.as_mut().unwrap();
    ids.green = 1000;
    ids.green_lat = 1001;
    for tile in 1000..1017 {
        palette.templates.insert(
            (tile, 0),
            TerrainTemplate {
                width: 1,
                height: 1,
                cells: vec![Some(TerrainSubtile {
                    height: 0,
                    slope: 0,
                    terrain: 14,
                })],
            },
        );
        palette.images.insert(
            TileKey {
                tile_id: tile,
                sub_tile: 0,
                variant: 0,
            },
            image.clone(),
        );
        // Stock LAT art is flat ground even when Morphable is not enabled.
        // Exercise legacy disk ids separately from the resolved art ids.
        palette.raw_ids.insert(2000 + i32::from(tile), tile);
        Arc::make_mut(&mut scene.catalog)
            .raw_tiles
            .insert(tile, 2000 + i32::from(tile));
    }
}

#[test]
fn lake_ground_transitions_cover_four_sides_and_corners_and_repair_old_lakes() {
    let (mut d, mut s) = setup();
    let selection = cliff_tools::rectangle([60, 60], [72, 72]);
    // Reproduce an existing lake authored before ground transitions were added.
    shore_tools::plan(&d, &s, selection.clone(), Some(true))
        .unwrap()
        .apply(&mut d)
        .unwrap();
    d.commit("old lake");
    shore_ground(&mut s);
    let before = d.snapshot().bytes().unwrap();
    let p = shore_tools::plan(&d, &s, selection.clone(), None).unwrap();
    assert_eq!(
        d.snapshot().bytes().unwrap(),
        before,
        "preview mutated document"
    );
    for (at, mask) in [
        ([58, 66], 8),
        ([74, 66], 2),
        ([66, 58], 1),
        ([66, 74], 4),
        ([58, 58], 9),
        ([74, 58], 3),
        ([74, 74], 6),
        ([58, 74], 12),
    ] {
        let v = p.edits.get(&at).expect("missing outer shore transition");
        assert_eq!(
            s.palette.raw_ids[&v.tile],
            1001 + mask,
            "wrong LAT orientation at {at:?}"
        );
        assert_eq!((v.sub, v.level), (0, 4));
    }
    assert!(!p.edits.contains_key(&[55, 55]), "distant ground changed");
    undoable(&mut d, &p);
    complete(&d, &s, &p);
    assert!(
        shore_tools::plan(&d, &s, selection.clone(), None)
            .unwrap()
            .edits
            .is_empty()
    );
    let mut fresh = setup().0;
    let fresh_plan = shore_tools::plan(&fresh, &s, selection, Some(true)).unwrap();
    fresh_plan.apply(&mut fresh).unwrap();
    assert_eq!(
        fresh.snapshot().bytes().unwrap(),
        d.snapshot().bytes().unwrap()
    );
    // Expansion and refill must accept the generated transition cells as ground.
    let p = shore_tools::plan(
        &d,
        &s,
        cliff_tools::rectangle([68, 65], [80, 78]),
        Some(true),
    )
    .unwrap();
    undoable(&mut d, &p);
    complete(&d, &s, &p);
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
        setup().0.snapshot().bytes().unwrap()
    );
}

#[test]
fn shore_transitions_preserve_ice_and_protect_neighbor_objects_and_missing_art() {
    let (mut d, mut s) = setup();
    shore_ground(&mut s);
    let at = [58, 66];
    let old = TerrainValue {
        ice: 7,
        ..value(&d, &s, at)
    };
    d.set(Target::Terrain(at), Some(Value::Terrain(old)))
        .unwrap();
    d.commit("ice");
    let selection = cliff_tools::rectangle([60, 60], [72, 72]);
    let p = shore_tools::plan(&d, &s, selection.clone(), Some(true)).unwrap();
    assert_eq!(p.edits[&at].ice, 7);
    // All these obstructions lie outside the selected water rectangle.
    for object in [false, true] {
        if object {
            d.add_record("Structures", at, "Neutral", "B").unwrap();
        } else {
            d.set(Target::Overlay(at), Some(Value::Overlay(0, 0)))
                .unwrap();
        }
        let before = d.snapshot().bytes().unwrap();
        assert!(shore_tools::plan(&d, &s, selection.clone(), Some(true)).is_err());
        assert_eq!(d.snapshot().bytes().unwrap(), before);
        d.cancel();
    }
    Arc::get_mut(&mut s.palette)
        .unwrap()
        .images
        .remove(&vera20k_core::map::theater::TileKey {
            tile_id: 1009,
            sub_tile: 0,
            variant: 0,
        });
    let before = d.snapshot().bytes().unwrap();
    assert!(shore_tools::plan(&d, &s, selection, Some(true)).is_err());
    assert_eq!(d.snapshot().bytes().unwrap(), before);
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
fn roads_use_inward_facing_caps_for_both_axes_after_drawing_extending_and_cutting() {
    // Stock PavedRoadEnds order: +X end, -Y end, -X end, +Y end.
    for (axis, low_cap, high_cap) in [(0, 2, 0), (1, 1, 3)] {
        let at = |n| if axis == 0 { [n, 70] } else { [70, n] };
        for reverse in [false, true] {
            let (mut d, s) = setup();
            let base = s.palette.tile_ids.unwrap().paved_road_ends as u16;
            let assert_cap = |d: &MapDocument, n, cap| {
                let v = value(d, &s, at(n));
                assert_eq!(
                    s.palette.raw_ids[&v.tile],
                    base + cap,
                    "wrong cap at {:?}, reverse={reverse}",
                    at(n)
                );
                assert_eq!(v.sub, 1, "endpoint must remain on the centreline");
            };
            for (lo, hi) in [(60, 80), (50, 90)] {
                let (a, b) = if reverse { (hi, lo) } else { (lo, hi) };
                let p = road_tools::plan(&d, &s, at(a), at(b), false).unwrap();
                undoable(&mut d, &p);
                complete(&d, &s, &p);
                assert_cap(&d, lo, low_cap);
                assert_cap(&d, hi, high_cap);
                assert!(
                    road_tools::plan(&d, &s, at(a), at(b), false)
                        .unwrap()
                        .edits
                        .is_empty()
                );
            }
            let p = road_tools::plan(&d, &s, at(66), at(74), true).unwrap();
            undoable(&mut d, &p);
            complete(&d, &s, &p);
            for (n, cap) in [(50, low_cap), (65, high_cap), (75, low_cap), (90, high_cap)] {
                assert_cap(&d, n, cap);
            }
        }
    }
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
