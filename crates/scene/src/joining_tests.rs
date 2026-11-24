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
    setup_fixture(&Fixture::with_joining())
}

fn setup_fixture(fixture: &Fixture) -> (MapDocument, MapScene) {
    let mut install = fixture.install();
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
fn water_polyline_keeps_concavities_for_both_directions_and_closure_methods() {
    let (mut d, s) = setup();
    let path = [[60, 60], [78, 60], [78, 66], [69, 66], [69, 78], [60, 78]];
    let open = shore_tools::polyline(&d, &s, &path).unwrap();
    let closed: Vec<_> = path.into_iter().chain([path[0]]).collect();
    assert_eq!(
        open.edits,
        shore_tools::polyline(&d, &s, &closed).unwrap().edits
    );
    let reversed: Vec<_> = path.into_iter().rev().collect();
    assert_eq!(
        open.edits,
        shore_tools::polyline(&d, &s, &reversed).unwrap().edits
    );
    undoable(&mut d, &open);
    complete(&d, &s, &open);
    assert!(shore_tools::wet(&s, value(&d, &s, [64, 72])));
    assert!(shore_tools::wet(&s, value(&d, &s, [75, 63])));
    assert!(
        !shore_tools::wet(&s, value(&d, &s, [72, 71])),
        "preserve the dry notch inside the convex hull"
    );
}

#[test]
fn water_polyline_preserves_a_dry_bay_behind_a_narrow_diagonal_mouth() {
    let (mut d, s) = setup();
    let path = [
        [60, 60],
        [68, 60],
        [74, 66],
        [74, 74],
        [87, 74],
        [87, 66],
        [76, 66],
        [70, 60],
        [94, 60],
        [94, 86],
        [60, 86],
    ];
    let p = shore_tools::polyline(&d, &s, &path).unwrap();
    p.apply(&mut d).unwrap();
    assert!(shore_tools::wet(&s, value(&d, &s, [82, 80])));
    assert!(
        !shore_tools::wet(&s, value(&d, &s, [80, 70])),
        "the bay is outside the polygon"
    );
}

#[test]
fn water_polyline_rejects_ambiguous_closures_without_editing_the_map() {
    let (d, s) = setup();
    let before = d.snapshot().bytes().unwrap();
    let start = [60, 60];
    let east = [78, 60];
    let corner = [78, 78];
    let upper = [60, 78];
    for (path, message) in [
        (vec![start, east], "至少三个"),
        // Drawn edges do not cross. The closing edge crosses x = 66.
        (
            vec![
                start,
                east,
                corner,
                [66, 78],
                [66, 66],
                [72, 66],
                [72, 72],
                [69, 72],
            ],
            "自交",
        ),
        (vec![start, corner, upper, east, start], "自交"),
        (vec![start, east, [72, 60]], "重叠"),
        (vec![start, east, corner, upper, east], "自交"),
    ] {
        let error = shore_tools::polyline(&d, &s, &path).err().unwrap();
        assert!(error.to_string().contains(message), "{path:?}: {error}");
        assert_eq!(d.snapshot().bytes().unwrap(), before);
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
fn road_polyline_plans_all_turns_and_rejects_a_blocker_on_the_last_leg_atomically() {
    let (mut doc, scene) = setup();
    let vertices = [[52, 56], [73, 56], [73, 74], [92, 74]];
    let style = road_tools::RoadStyle::Paved;
    let before = doc.snapshot().bytes().unwrap();
    let plan = road_tools::plan_polyline(&doc, &scene, &vertices, style).unwrap();
    let reverse: Vec<_> = vertices.iter().rev().copied().collect();
    assert_eq!(
        plan.edits,
        road_tools::plan_polyline(&doc, &scene, &reverse, style)
            .unwrap()
            .edits
    );
    let paved = scene.palette.tile_ids.unwrap().paved_roads as u16;
    assert!(plan.cliffs.iter().any(|stamp| stamp.tile == paved + 10));
    assert!(plan.cliffs.iter().any(|stamp| stamp.tile == paved + 9));
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    undoable(&mut doc, &plan);
    complete(&doc, &scene, &plan);
    doc.undo();
    doc.set(Target::Overlay([85, 74]), Some(Value::Overlay(0, 0)))
        .unwrap();
    doc.commit("block final leg");
    let before = doc.snapshot().bytes().unwrap();
    assert!(road_tools::plan_polyline(&doc, &scene, &vertices, style).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
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
fn cliff_authoring_draws_directly_on_water() {
    let (mut doc, scene) = setup();
    let ids = scene.palette.tile_ids.unwrap();
    let raw_water = scene.catalog.raw_tiles[&(ids.water_base as u16)];
    let raw_clear = scene.catalog.raw_tiles[&(ids.clear as u16)];
    // Paint complete 2×2 water templates, including nonzero subtiles.
    for at @ [x, y] in cliff_tools::rectangle([50, 50], [81, 81]) {
        let v = TerrainValue {
            tile: raw_water,
            sub: ((y % 2) * 2 + x % 2) as u8,
            level: 4,
            ice: 0,
        };
        doc.set(Target::Terrain(at), Some(Value::Terrain(v)))
            .unwrap();
    }
    doc.commit("lake");
    let before = doc.snapshot().bytes().unwrap();
    let lake_cell = [51, 51];
    let loop_path = [[60, 60], [60, 72], [72, 72], [72, 60], [60, 60]];
    let selection = cliff_tools::enclose(&loop_path).unwrap();
    let plans = [
        (
            cliff_authoring::line(&doc, &scene, [60, 62], [72, 62], 4, false),
            None,
        ),
        (
            cliff_authoring::line(&doc, &scene, [60, 62], [72, 62], 4, true),
            None,
        ),
        (
            cliff_authoring::polyline(&doc, &scene, &[[60, 62], [72, 62]], 4, false),
            None,
        ),
        (
            cliff_authoring::polyline(&doc, &scene, &loop_path, 4, true),
            Some([66, 66]),
        ),
        (
            cliff_authoring::reshape(&doc, &scene, selection.clone(), 4, false),
            Some([66, 66]),
        ),
        (
            cliff_tools::plan(&doc, &scene, selection.clone(), 4),
            Some([66, 66]),
        ),
    ];
    for (i, (plan, summit)) in plans.into_iter().enumerate() {
        let plan = plan.unwrap();
        if i != 2 {
            assert!(plan.cliffs.iter().any(|s| {
                coastal_cliffs::piece(&scene, s.tile).is_some_and(|(_, water)| water)
            }));
        }
        assert!(!plan.edits.contains_key(&lake_cell));
        let mut drawn = doc.fork();
        undoable(&mut drawn, &plan);
        complete(&drawn, &scene, &plan);
        assert_eq!(
            value(&drawn, &scene, lake_cell),
            value(&doc, &scene, lake_cell)
        );
        if let Some(at) = summit {
            let v = value(&drawn, &scene, at);
            assert_eq!((v.tile, v.level), (raw_clear, 8));
        }
    }
    assert_eq!(doc.snapshot().bytes().unwrap(), before);

    doc.set(Target::Overlay([66, 66]), Some(Value::Overlay(0, 0)))
        .unwrap();
    let before = doc.snapshot().bytes().unwrap();
    let Err(err) = cliff_authoring::reshape(&doc, &scene, selection, 4, false) else {
        panic!("covered water cell must block the cliff stroke");
    };
    assert!(err.to_string().contains("覆盖物占用"), "{err:#}");
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn ramp_entrances_replace_water_under_their_footprint() {
    let (mut doc, scene) = setup();
    let ids = scene.palette.tile_ids.unwrap();
    let raw_water = scene.catalog.raw_tiles[&(ids.water_base as u16)];
    for at @ [x, y] in cliff_tools::rectangle([50, 50], [85, 85]) {
        doc.set(
            Target::Terrain(at),
            Some(Value::Terrain(TerrainValue {
                tile: raw_water,
                sub: ((y % 2) * 2 + x % 2) as u8,
                level: 4,
                ice: 0,
            })),
        )
        .unwrap();
    }
    let island = cliff_authoring::reshape(
        &doc,
        &scene,
        cliff_tools::rectangle([59, 59], [76, 76]),
        4,
        false,
    )
    .unwrap();
    island.apply(&mut doc).unwrap();
    doc.commit("cliff island");
    let before = doc.snapshot().bytes().unwrap();
    let ramp_base = ids.ramp_base as u16;
    for (start, end, corner_slope) in [
        ([68, 76], [72, 76], None),
        ([76, 68], [76, 72], None),
        ([68, 59], [72, 59], None),
        ([59, 68], [59, 72], None),
        ([59, 59], [61, 59], Some(5)),
        ([76, 59], [74, 59], Some(6)),
        ([59, 76], [61, 76], Some(8)),
        ([76, 76], [74, 76], Some(7)),
    ] {
        let plan = cliff_authoring::ramp(&doc, &scene, start, end, 4)
            .unwrap_or_else(|e| panic!("{start:?} -> {end:?}: {e:#}"));
        if let Some(slope) = corner_slope {
            assert_eq!(
                plan.edits[&start].tile,
                scene.catalog.raw_tiles[&(ramp_base + slope - 1)]
            );
        }
        let shores: Vec<_> = plan
            .cliffs
            .iter()
            .filter(|s| shore_tools::shore(&scene, s.tile))
            .collect();
        assert!(!shores.is_empty(), "missing shore at {start:?}");
        assert!(
            shores.iter().any(|s| {
                let t = &scene.palette.templates[&(s.tile, 0)];
                (t.width, t.height) == (2, 2)
            }),
            "shore needs a corner piece at {start:?}"
        );
        let submerged = plan
            .edits
            .iter()
            .find_map(|(&at, next)| {
                (shore_tools::flat_water(&scene, value(&doc, &scene, at))
                    && !shore_tools::wet(&scene, *next))
                .then_some(at)
            })
            .expect("the ramp must cover some water");
        let mut changed = doc.fork();
        undoable(&mut changed, &plan);
        complete(&changed, &scene, &plan);
        assert_eq!(
            value(&changed, &scene, [54, 54]),
            value(&doc, &scene, [54, 54])
        );

        if start == [68, 76] {
            let ramp_ends = ids.slope_set_pieces as u16;
            let cap = plan.cliffs.iter().find(|s| s.tile == ramp_ends).unwrap();
            assert!(
                plan.edits.keys().all(|at| at[1] <= cap.origin[1] + 7),
                "do not extend the front to smooth its shoulders"
            );
            let is_floor = |at, next| {
                shore_tools::flat_water(&scene, value(&doc, &scene, at))
                    && shore_tools::ground(&scene, next)
            };
            assert!(plan.edits.iter().any(|(&at, &v)| is_floor(at, v)));
            assert!(
                shores.iter().all(|s| {
                    let t = &scene.palette.templates[&(s.tile, 0)];
                    t.width <= 2 && t.height <= 2
                }),
                "do not insert a diagonal notch between the side and front"
            );
            for (end, side) in [(ramp_ends, 0), (ramp_ends + 1, 2)] {
                let cap = plan.cliffs.iter().find(|s| s.tile == end).unwrap();
                for dy in [3, 4, 5, 6] {
                    let shoulder = [cap.origin[0] + side, cap.origin[1] + dy];
                    let v = plan.edits[&shoulder];
                    let tile = shore_tools::id(&scene, v).unwrap();
                    assert!(shore_tools::shore(&scene, tile) && !shore_tools::wet(&scene, v));
                }
                assert!(shores.iter().any(|s| {
                    s.origin[1] == cap.origin[1] + 6
                        && s.tile == ids.shore as u16 + if side == 0 { 30 } else { 6 }
                }));
            }
            assert!(
                cliff_authoring::ramp(&changed, &scene, start, start, 4)
                    .unwrap()
                    .edits
                    .is_empty()
            );
            // Simulate a saved ramp authored before the shoreline pass existed.
            let mut legacy = doc.fork();
            for (&at, &v) in &plan.edits {
                if !shore_tools::id(&scene, v).is_some_and(|tile| shore_tools::shore(&scene, tile))
                    && !is_floor(at, v)
                {
                    legacy
                        .set(Target::Terrain(at), Some(Value::Terrain(v)))
                        .unwrap();
                }
            }
            legacy.commit("legacy ramp");
            let repaired = cliff_authoring::ramp(&legacy, &scene, start, start, 4).unwrap();
            assert_eq!(
                repaired
                    .cliffs
                    .iter()
                    .filter(|s| shore_tools::shore(&scene, s.tile))
                    .count(),
                shores.len()
            );
            repaired.apply(&mut legacy).unwrap();
            assert_eq!(
                legacy.snapshot().bytes().unwrap(),
                changed.snapshot().bytes().unwrap()
            );

            let (source, _) = ra2_map_document::source::SourceMap::parse(
                "shore-ramp.map".into(),
                "test".into(),
                changed.snapshot().bytes().unwrap(),
            )
            .unwrap();
            let mut reopened = MapDocument::new(source.edit_base.as_ref().unwrap().clone());
            let widened = cliff_authoring::ramp(&reopened, &scene, [69, 76], [75, 76], 4).unwrap();
            undoable(&mut reopened, &widened);
            complete(&reopened, &scene, &widened);
            assert!(
                cliff_authoring::ramp(&reopened, &scene, start, start, 4)
                    .unwrap()
                    .edits
                    .is_empty()
            );

            // A protected cell just beyond the compact coast only blocks
            // the extension. Removing it makes the compact entrance upgradable.
            let protected = [cap.origin[0], cap.origin[1] + 7];
            assert!(plan.edits.contains_key(&protected));
            let mut tight = doc.fork();
            tight
                .set(Target::Overlay(protected), Some(Value::Overlay(0, 0)))
                .unwrap();
            let compact = cliff_authoring::ramp(&tight, &scene, start, end, 4).unwrap();
            assert!(!compact.edits.contains_key(&protected));
            assert!(compact.edits.iter().all(|(&at, &v)| !is_floor(at, v)));
            compact.apply(&mut tight).unwrap();
            let (id, frame) = doc.overlay(protected);
            tight
                .set(Target::Overlay(protected), Some(Value::Overlay(id, frame)))
                .unwrap();
            let upgrade = cliff_authoring::ramp(&tight, &scene, start, start, 4).unwrap();
            upgrade.apply(&mut tight).unwrap();
            complete(&tight, &scene, &plan);
        }

        let mut blocked = doc.fork();
        blocked
            .set(Target::Overlay(submerged), Some(Value::Overlay(0, 0)))
            .unwrap();
        let blocked_before = blocked.snapshot().bytes().unwrap();
        if let Ok(alternative) = cliff_authoring::ramp(&blocked, &scene, start, end, 4) {
            assert!(!alternative.edits.contains_key(&submerged));
        }
        assert_eq!(blocked.snapshot().bytes().unwrap(), blocked_before);
    }
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn a_short_water_set_does_not_let_cliff_tools_overwrite_roads() {
    let fixture = Fixture::with_joining();
    let ini = fixture.0.join("temperatmd.ini");
    let source = std::fs::read_to_string(&ini)
        .unwrap()
        .replace(
            "FileName=water\nTilesInSet=14",
            "FileName=water\nTilesInSet=9",
        )
        .replace("ShorePieces=6\nPavedRoads=7", "ShorePieces=7\nPavedRoads=6")
        .replace(
            "[TileSet0006]\nFileName=shore",
            "[TileSet0007]\nFileName=shore",
        )
        .replace(
            "[TileSet0007]\nFileName=road",
            "[TileSet0006]\nFileName=road",
        );
    std::fs::write(ini, source).unwrap();
    let (mut doc, scene) = setup_fixture(&fixture);
    let ids = scene.palette.tile_ids.unwrap();
    let road = ids.paved_roads as u16;
    assert_eq!(i32::from(road), ids.water_base + 9);
    for (at, v) in cliff_authoring::stamp(&doc, &scene, road, [64, 64], 4).unwrap() {
        doc.set(Target::Terrain(at), Some(Value::Terrain(v)))
            .unwrap();
    }
    let area = cliff_tools::rectangle([60, 60], [72, 72]);
    let before = doc.snapshot().bytes().unwrap();
    assert!(cliff_authoring::reshape(&doc, &scene, area, 4, false).is_err());
    assert!(!shore_tools::wet(&scene, value(&doc, &scene, [64, 64])));
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn water_cliffs_fall_back_when_coastal_art_is_missing() {
    let (mut doc, mut scene) = setup();
    let water = shore_tools::plan(
        &doc,
        &scene,
        cliff_tools::rectangle([48, 48], [84, 84]),
        Some(true),
    )
    .unwrap();
    water.apply(&mut doc).unwrap();
    doc.commit("lake");
    let base = scene.palette.tile_ids.unwrap().cliffs.water_cliffs.unwrap();
    Arc::get_mut(&mut scene.palette)
        .unwrap()
        .images
        .retain(|key, _| !(base..base + 22).contains(&key.tile_id));
    let plan = cliff_authoring::line(&doc, &scene, [60, 62], [72, 62], 4, false).unwrap();
    assert!(!plan.cliffs.is_empty());
    assert!(
        plan.cliffs
            .iter()
            .all(|s| { coastal_cliffs::piece(&scene, s.tile).is_some_and(|(_, water)| !water) })
    );
    undoable(&mut doc, &plan);
    complete(&doc, &scene, &plan);
}

#[test]
fn drawing_cliffs_cannot_leave_half_a_shore_template() {
    let (mut doc, scene) = setup();
    let area = cliff_tools::rectangle([60, 60], [72, 72]);
    let draft = cliff_authoring::reshape(&doc, &scene, area.clone(), 4, false).unwrap();
    let shore = scene.palette.tile_ids.unwrap().shore as u16;
    // Set a two-cell shoreline across the edge of the cliff footprint,
    // leaving its dry cell outside the set of edits.
    let shore_cells = draft
        .edits
        .iter()
        .find_map(|(&at, &v)| {
            if v.level != 4 {
                return None;
            }
            for (n, dx, dy, reverse) in [
                (3, 0, 1, false),
                (11, 1, 0, false),
                (19, 0, 1, true),
                (27, 1, 0, true),
            ] {
                let origin = if reverse {
                    at
                } else {
                    shore_tools::offset(at, -dx, -dy).unwrap()
                };
                let dry = if reverse {
                    shore_tools::offset(at, dx, dy).unwrap()
                } else {
                    origin
                };
                if !draft.edits.contains_key(&dry) {
                    return Some(
                        cliff_authoring::stamp(&doc, &scene, shore + n, origin, 4).unwrap(),
                    );
                }
            }
            None
        })
        .unwrap();
    for (at, v) in shore_cells {
        doc.set(Target::Terrain(at), Some(Value::Terrain(v)))
            .unwrap();
    }
    let before = doc.snapshot().bytes().unwrap();
    let Err(err) = cliff_authoring::reshape(&doc, &scene, area, 4, false) else {
        panic!("a stroke must not cut an intact shore template");
    };
    assert!(err.to_string().contains("切断水岸模板"), "{err:#}");
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
#[ignore = "requires stock YR assets in RA2_DIR"]
fn retail_water_cliffs_survive_map_reload_extension_and_ramps() {
    let root = std::env::var_os("RA2_DIR").expect("RA2_DIR required");
    let mut install = ra2_map_assets::assets::Install::open(std::path::Path::new(&root)).unwrap();
    for theater in ["TEMPERATE", "SNOW", "URBAN", "DESERT", "NEWURBAN"] {
        let scene = load_scene_bytes(
            &mut install,
            "retail-water-cliffs.map",
            "test".into(),
            NewMapOptions {
                theater: theater.into(),
                ..Default::default()
            }
            .bytes()
            .unwrap(),
            Instant::now(),
        )
        .unwrap();
        let ids = scene.palette.tile_ids.unwrap();
        let mut doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
        let water = shore_tools::plan(
            &doc,
            &scene,
            cliff_tools::rectangle([48, 48], [84, 84]),
            Some(true),
        )
        .unwrap_or_else(|e| panic!("{theater}: could not draw lake: {e:#}"));
        water.apply(&mut doc).unwrap();
        doc.commit("lake");
        let land = cliff_tools::rectangle([59, 59], [70, 70]);
        let plan = cliff_authoring::reshape(&doc, &scene, land, 0, false)
            .unwrap_or_else(|e| panic!("{theater}: could not draw island: {e:#}"));
        assert!(
            plan.cliffs
                .iter()
                .any(|s| { coastal_cliffs::piece(&scene, s.tile).is_some_and(|(_, water)| water) })
                || !(0..22).any(|i| coastal_cliffs::supports(&scene, i)),
            "{theater}"
        );
        undoable(&mut doc, &plan);
        let reloaded = load_scene_bytes(
            &mut install,
            "retail-water-cliffs-edited.map",
            "test".into(),
            doc.snapshot().bytes().unwrap(),
            Instant::now(),
        )
        .unwrap();
        let mut reopened = MapDocument::new(reloaded.source.edit_base.as_ref().unwrap().clone());
        complete(&reopened, &reloaded, &plan);
        assert!(shore_tools::wet(
            &reloaded,
            value(&reopened, &reloaded, [54, 54])
        ));
        let extended = cliff_authoring::reshape(
            &reopened,
            &reloaded,
            cliff_tools::rectangle([68, 59], [75, 70]),
            0,
            false,
        )
        .unwrap_or_else(|e| panic!("{theater}: could not extend reopened island: {e:#}"));
        undoable(&mut reopened, &extended);
        complete(&reopened, &reloaded, &extended);
        for at in [[64, 64], [73, 64]] {
            let v = value(&reopened, &reloaded, at);
            assert_eq!(
                (shore_tools::id(&reloaded, v), v.level),
                (Some(ids.clear as u16), 4)
            );
        }
        for (start, end) in [([64, 70], [66, 70]), ([75, 70], [75, 70])] {
            let entrance = cliff_authoring::ramp(&reopened, &reloaded, start, end, 0)
                .unwrap_or_else(|e| panic!("{theater} {start:?} -> {end:?}: {e:#}"));
            assert!(
                entrance.edits.keys().any(|&at| {
                    shore_tools::flat_water(&reloaded, value(&reopened, &reloaded, at))
                }),
                "{theater} {start:?}"
            );
            assert!(
                entrance
                    .cliffs
                    .iter()
                    .any(|s| shore_tools::shore(&reloaded, s.tile)),
                "{theater} {start:?}"
            );
            let mut edited = reopened.fork();
            undoable(&mut edited, &entrance);
            complete(&edited, &reloaded, &entrance);
            if theater == "TEMPERATE" && start == [64, 70] {
                let saved = edited.snapshot().bytes().unwrap();
                let (source, _) = ra2_map_document::source::SourceMap::parse(
                    "retail-tapered-ramp.map".into(),
                    "test".into(),
                    saved,
                )
                .unwrap();
                let saved = MapDocument::new(source.edit_base.unwrap());
                assert!(
                    entrance.edits.keys().any(|&at| {
                        shore_tools::id(&reloaded, value(&saved, &reloaded, at)).is_some_and(|t| {
                            i32::from(t) != ids.green && ids.is_green_lat(i32::from(t))
                        })
                    }),
                    "save grass-to-sand transitional pieces, not merely the base sand"
                );
                assert!(
                    cliff_authoring::ramp(&saved, &reloaded, start, start, 0)
                        .unwrap()
                        .edits
                        .is_empty()
                );
            }
        }
    }
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
fn shore_repair_completes_a_broken_template_without_changing_water_extent() {
    let (mut doc, scene) = setup();
    let lake = cliff_tools::rectangle([60, 60], [72, 72]);
    let original = shore_tools::plan(&doc, &scene, lake.clone(), Some(true)).unwrap();
    original.apply(&mut doc).unwrap();
    doc.commit("lake");
    let original_bytes = doc.snapshot().bytes().unwrap();

    let broken = [60, 60];
    let old = value(&doc, &scene, broken);
    assert!(shore_tools::wet(&scene, old));
    assert!(shore_tools::shore(
        &scene,
        shore_tools::id(&scene, old).unwrap()
    ));
    doc.set(
        Target::Terrain(broken),
        Some(Value::Terrain(TerrainValue {
            tile: value(&doc, &scene, [66, 66]).tile,
            sub: 0,
            ..old
        })),
    )
    .unwrap();
    doc.commit("replace corner with plain water");

    let open_water = shore_tools::plan(&doc, &scene, BTreeSet::from([[62, 62]]), None).unwrap();
    assert!(
        open_water.edits.is_empty(),
        "the brush did not reach the shore"
    );

    let stroke = cliff_tools::rectangle([58, 58], [62, 62]);
    let before = doc.snapshot().bytes().unwrap();
    let repair = shore_tools::plan(&doc, &scene, stroke.clone(), None).unwrap();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    assert_eq!(repair.edits.get(&broken), Some(&old));
    assert!(repair.edits.iter().all(|(&at, &v)| {
        shore_tools::wet(&scene, v) == lake.contains(&at)
            && v.level == value(&doc, &scene, at).level
    }));

    doc.set(Target::Overlay(broken), Some(Value::Overlay(0, 0)))
        .unwrap();
    assert!(shore_tools::plan(&doc, &scene, stroke, None).is_err());
    doc.cancel();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);

    undoable(&mut doc, &repair);
    complete(&doc, &scene, &repair);
    assert_eq!(doc.snapshot().bytes().unwrap(), original_bytes);
}

#[test]
fn shore_repair_varies_repeated_pieces_and_keeps_complete_stamps() {
    let (mut doc, scene) = setup();
    let base = scene.palette.tile_ids.unwrap().shore as u16;
    let lake = cliff_tools::rectangle([54, 62], [88, 72]);
    let painted = shore_tools::plan(&doc, &scene, lake, Some(true)).unwrap();
    painted.apply(&mut doc).unwrap();
    doc.commit("paint lake");
    let painted_bytes = doc.snapshot().bytes().unwrap();

    // Simulate a saved shore made by the old fitter, with one tile per shape.
    let straight: Vec<_> = painted
        .cliffs
        .iter()
        .filter(|stamp| (base..base + 3).contains(&stamp.tile))
        .collect();
    assert!(straight.len() >= 10);
    for stamp in &straight {
        for (at, v) in cliff_authoring::stamp(&doc, &scene, base, stamp.origin, stamp.base).unwrap()
        {
            doc.set(Target::Terrain(at), Some(Value::Terrain(v)))
                .unwrap();
        }
    }
    doc.commit("repeat a single texture along the shore");

    let stroke = cliff_tools::rectangle([52, 60], [90, 74]);
    let repaired = shore_tools::plan(&doc, &scene, stroke.clone(), None).unwrap();
    let repaired_styles: BTreeSet<_> = repaired
        .cliffs
        .iter()
        .filter_map(|stamp| (base..base + 3).contains(&stamp.tile).then_some(stamp.tile))
        .collect();
    assert!(repaired_styles.len() > 1);
    assert!(!repaired.edits.is_empty());
    for (&at, &next) in &repaired.edits {
        let old = value(&doc, &scene, at);
        assert_eq!(
            next,
            TerrainValue {
                tile: next.tile,
                ..old
            }
        );
        assert_eq!(
            shore_tools::wet(&scene, next),
            shore_tools::wet(&scene, old)
        );
    }
    undoable(&mut doc, &repaired);
    complete(&doc, &scene, &repaired);
    assert_eq!(doc.snapshot().bytes().unwrap(), painted_bytes);
    let second = shore_tools::plan(&doc, &scene, stroke, None).unwrap();
    assert!(second.edits.is_empty());
}

#[test]
fn shore_repair_snaps_a_brushed_staircase_without_moving_water_outside_the_stroke() {
    let (mut doc, scene) = setup();
    let water = scene.palette.tile_ids.unwrap().water_base as u16 + 8;
    for at in cliff_tools::rectangle([48, 48], [96, 96]) {
        if at[0] >= at[1] {
            doc.set(
                Target::Terrain(at),
                Some(Value::Terrain(TerrainValue {
                    tile: scene.catalog.raw_tiles[&water],
                    sub: 0,
                    ..value(&doc, &scene, at)
                })),
            )
            .unwrap();
        }
    }
    doc.commit("raw one-cell staircase");

    let mut stroke = BTreeSet::new();
    for center in 63..=78 {
        stroke.extend(cliff_tools::rectangle([center - 3; 2], [center + 3; 2]));
    }
    let plan = shore_tools::plan(&doc, &scene, stroke.clone(), None).unwrap();
    let adjusted: Vec<_> = plan
        .edits
        .iter()
        .filter(|(at, new)| {
            shore_tools::wet(&scene, **new) != shore_tools::wet(&scene, value(&doc, &scene, **at))
        })
        .collect();
    assert!(
        !adjusted.is_empty(),
        "the one-cell staircase needs to be reshaped"
    );
    assert!(adjusted.iter().all(|(at, _)| stroke.contains(*at)));
    assert!(
        plan.cliffs
            .iter()
            .any(|stamp| shore_tools::shore(&scene, stamp.tile))
    );
    undoable(&mut doc, &plan);
    complete(&doc, &scene, &plan);
}

#[test]
fn shore_repair_retries_snaps_that_would_reach_a_higher_terrace() {
    let (mut doc, scene) = setup();
    let water = scene.palette.tile_ids.unwrap().water_base as u16 + 8;
    for at in cliff_tools::rectangle([48, 48], [96, 96]) {
        if at[0] >= at[1] {
            doc.set(
                Target::Terrain(at),
                Some(Value::Terrain(TerrainValue {
                    tile: scene.catalog.raw_tiles[&water],
                    sub: 0,
                    ..value(&doc, &scene, at)
                })),
            )
            .unwrap();
        }
    }
    for x in 60..=81 {
        let at = [x, x + 2];
        doc.set(
            Target::Terrain(at),
            Some(Value::Terrain(TerrainValue {
                level: 5,
                ..value(&doc, &scene, at)
            })),
        )
        .unwrap();
    }
    doc.commit("one-cell staircase beside higher ground");
    let mut stroke = BTreeSet::new();
    for center in 63..=78 {
        stroke.extend(cliff_tools::rectangle([center - 3; 2], [center + 3; 2]));
    }
    let repair = shore_tools::plan(&doc, &scene, stroke, None).unwrap();
    assert!(
        repair
            .cliffs
            .iter()
            .any(|stamp| shore_tools::shore(&scene, stamp.tile))
    );
    for (&at, &next) in &repair.edits {
        assert_eq!(next.level, value(&doc, &scene, at).level);
    }
    undoable(&mut doc, &repair);
    complete(&doc, &scene, &repair);
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

#[test]
fn wide_coastal_cliff_splits_when_only_one_half_touches_water() {
    let (doc, scene) = setup();
    let ids = scene.palette.tile_ids.unwrap();
    let land = ids.cliffs.cliff_set.unwrap();
    let water = ids.cliffs.water_cliffs.unwrap();
    let raw_water = scene.catalog.raw_tiles[&((ids.water_base + 8) as u16)];
    for initial in [land + 4, water + 4] {
        let mut doc = doc.fork();
        for (at, v) in cliff_authoring::stamp(&doc, &scene, initial, [60, 60], 4).unwrap() {
            doc.set(Target::Terrain(at), Some(Value::Terrain(v)))
                .unwrap();
        }
        let at = [60, 62];
        doc.set(
            Target::Terrain(at),
            Some(Value::Terrain(TerrainValue {
                tile: raw_water,
                ..value(&doc, &scene, at)
            })),
        )
        .unwrap();
        doc.commit("mixed shoreline");
        let p = coastal_cliffs::plan(&doc, &scene, BTreeSet::from([[61, 60]])).unwrap();
        assert_eq!(
            p.cliffs
                .iter()
                .map(|s| (s.origin, s.tile))
                .collect::<Vec<_>>(),
            vec![([60, 60], water + 7), ([61, 60], land + 7)]
        );
        undoable(&mut doc, &p);
        complete(&doc, &scene, &p);
        assert_eq!(value(&doc, &scene, at).tile, raw_water);
    }
}

fn check_paved_road_slopes(
    mut doc: MapDocument,
    scene: &MapScene,
    axis: usize,
    positive: bool,
    slope: Option<u8>,
) -> Option<(Vec<u8>, PlateauPlan)> {
    use crate::flatten_tools::{Options, plan_with_options};
    let cell = |i, j| if axis == 0 { [i, j] } else { [j, i] };
    let road = road_tools::plan(&doc, scene, cell(40, 50), cell(56, 50), false).unwrap();
    road.apply(&mut doc).unwrap();
    doc.commit("paved road");
    let before = doc.snapshot().bytes().unwrap();
    let edit = |a, b| cliff_tools::rectangle(cell(a, 47), cell(b, 53));
    let selection = if positive { edit(49, 53) } else { edit(43, 47) };
    let options = Options {
        paved_roads: true,
        ..Default::default()
    };
    let result = plan_with_options(&doc, scene, &selection, 5, true, options);
    let Some(slope) = slope else {
        assert!(
            result
                .err()
                .expect("unsupported direction must be rejected")
                .to_string()
                .contains("没有匹配的公路坡面")
        );
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        return None;
    };
    let plan = result.unwrap();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    assert_eq!(
        plan.cliffs.len(),
        2,
        "both ends of the raised stretch have a ramp"
    );
    let stamp = plan
        .cliffs
        .iter()
        .find(|s| s.origin == cell(48, 49))
        .unwrap();
    assert_eq!(stamp.base, 4);
    let t = &scene.palette.templates[&(stamp.tile, 0)];
    assert!(t.cells.iter().all(|sub| sub.unwrap().slope == slope));
    assert_eq!(t.cells.len(), 3);
    undoable(&mut doc, &plan);
    complete(&doc, scene, &plan);
    for j in 49..=51 {
        assert_eq!(
            value(&doc, scene, cell(if positive { 49 } else { 47 }, j)).level,
            5
        );
        assert_eq!(
            value(&doc, scene, cell(if positive { 47 } else { 49 }, j)).level,
            4
        );
    }
    let changed =
        crate::terrain_tools::recalculate(&mut doc, scene, &plan.edits.keys().copied().collect())
            .unwrap();
    assert!(
        changed.iter().all(|at| !road.edits.contains_key(at)),
        "LAT must preserve the planned road"
    );
    let center = |x| cell(x, 50);
    let (high_from, high_to, cut_from, cut_to) = if positive {
        (49, 51, 42, 43)
    } else {
        (44, 46, 54, 55)
    };
    let mut protected = BTreeSet::new();
    for stamp in &plan.cliffs {
        let x = stamp.origin[axis];
        protected.extend(edit(x - 1, x + 1));
    }
    for (from, to, cut) in [
        (40, 36, false),
        (56, 60, false),
        (high_from, high_to, false),
        (cut_from, cut_to, true),
    ] {
        let edit = road_tools::plan(&doc, scene, center(from), center(to), cut).unwrap();
        assert!(
            edit.edits.keys().all(|p| !protected.contains(p)),
            "editing the flat network changed a slope or one of its approach tiles"
        );
        if from != high_from {
            assert!(!edit.edits.is_empty(), "flat editing should still succeed");
        }
    }
    for cut in [false, true] {
        assert!(road_tools::plan(&doc, scene, center(48), center(46), cut).is_err());
    }
    Some((doc.snapshot().bytes().unwrap(), plan))
}

#[test]
fn raising_across_paved_roads_places_complete_ramps_and_rejects_partial_strokes() {
    for (axis, positive, slope) in [(0, true, 1), (0, false, 3), (1, true, 2), (1, false, 4)] {
        let (doc, scene) = setup();
        check_paved_road_slopes(doc, &scene, axis, positive, Some(slope));
    }
    let (mut doc, scene) = setup();
    let road = road_tools::plan(&doc, &scene, [40, 50], [56, 50], false).unwrap();
    road.apply(&mut doc).unwrap();
    let before = doc.snapshot().bytes().unwrap();
    assert!(
        crate::flatten_tools::plan_with_options(
            &doc,
            &scene,
            &BTreeSet::from([[48, 50]]),
            5,
            true,
            crate::flatten_tools::Options {
                paved_roads: true,
                ..Default::default()
            },
        )
        .is_err()
    );
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
#[ignore = "requires stock YR assets in RA2_DIR"]
fn retail_paved_road_slopes_survive_map_reload() {
    let root = std::env::var_os("RA2_DIR").expect("RA2_DIR required");
    let mut install = ra2_map_assets::assets::Install::open(std::path::Path::new(&root)).unwrap();
    for theater in ["TEMPERATE", "SNOW", "URBAN", "DESERT", "NEWURBAN"] {
        let scene = load_scene_bytes(
            &mut install,
            "retail-road.map",
            "test".into(),
            NewMapOptions {
                theater: theater.into(),
                level: 4,
                ..Default::default()
            }
            .bytes()
            .unwrap(),
            Instant::now(),
        )
        .unwrap();
        for (axis, positive, slope) in [(0, true, 1), (0, false, 3), (1, true, 2), (1, false, 4)] {
            let doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
            let slope = (theater != "NEWURBAN" || axis == 0).then_some(slope);
            let Some((bytes, plan)) = check_paved_road_slopes(doc, &scene, axis, positive, slope)
            else {
                continue;
            };
            let reloaded = load_scene_bytes(
                &mut install,
                "retail-road-edited.map",
                "test".into(),
                bytes,
                Instant::now(),
            )
            .unwrap();
            for stamp in plan.cliffs {
                let template = &scene.palette.templates[&(stamp.tile, 0)];
                for sub in 0..3u16 {
                    let at = [
                        stamp.origin[0] + sub % template.width,
                        stamp.origin[1] + sub / template.width,
                    ];
                    let cell = reloaded.cells.iter().find(|c| [c.rx, c.ry] == at).unwrap();
                    assert_eq!(
                        (cell.tile_id, cell.sub_tile, cell.z),
                        (stamp.tile, sub as u8, stamp.base),
                        "{theater} {at:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn raw_paved_road_height_edits_keep_stamps_and_existing_seams_intact() {
    use crate::terrain_tools::{TerrainMode, height};

    let (mut doc, scene) = setup();
    let road = road_tools::plan(&doc, &scene, [40, 50], [56, 50], false).unwrap();
    road.apply(&mut doc).unwrap();
    doc.commit("paved road");
    let before = doc.snapshot().bytes().unwrap();
    for mode in [TerrainMode::Raise, TerrainMode::Lower] {
        let mut failed = doc.fork();
        for (brush, error) in [
            (BTreeSet::from([[48, 50], [45, 45]]), "整块公路"),
            (cliff_tools::rectangle([48, 49], [48, 51]), "断开相连公路"),
        ] {
            let result = height(&mut failed, &scene, &brush, mode, 0, false);
            assert!(result.unwrap_err().to_string().contains(error));
            assert_eq!(failed.snapshot().bytes().unwrap(), before);
        }
        let mut complete = doc.fork();
        let all = road.edits.keys().copied().collect();
        let changed = height(&mut complete, &scene, &all, mode, 0, false).unwrap();
        assert_eq!(changed.len(), road.edits.len());
        let level = if mode == TerrainMode::Raise { 5 } else { 3 };
        for (&at, &cell) in &road.edits {
            assert_eq!(value(&complete, &scene, at), TerrainValue { level, ..cell });
        }
    }
}
