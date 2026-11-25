use super::*;
use crate::cliff_tools;
use ra2_map_assets::{catalog::Catalog, test_support::Fixture};
use ra2_map_document::{
    document::{Target, Value},
    new_map::NewMapOptions,
};

fn setup() -> (MapDocument, MapScene) {
    setup_level(4)
}

fn setup_level(level: u8) -> (MapDocument, MapScene) {
    let fixture = Fixture::with_authoring();
    let mut install = fixture.install();
    install.catalog = Catalog::load(&install.manager, &install.theater);
    let scene = crate::map::load_scene_bytes(
        &mut install,
        "height.map",
        "test".into(),
        NewMapOptions {
            width: 60,
            height: 60,
            level,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
        std::time::Instant::now(),
    )
    .unwrap();
    let doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
    (doc, scene)
}

#[test]
fn filling_an_already_equal_height_is_a_noop_even_on_open_or_occupied_ground() {
    let (mut doc, scene) = setup();
    doc.set(Target::Overlay([46, 46]), Some(Value::Overlay(1, 0)))
        .unwrap();
    doc.commit("overlay");
    let before = doc.snapshot().bytes().unwrap();
    let plan = fill(&doc, &scene, [46, 46], 4).unwrap();
    assert!(plan.edits.is_empty());
    plan.apply(&mut doc).unwrap();
    assert!(!doc.commit("no change"));
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn sampling_every_standard_cliff_subtile_finds_the_same_high_ground() {
    let (mut doc, scene) = setup();
    let ring =
        cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([40, 40], [52, 52]), 4).unwrap();
    ring.apply(&mut doc).unwrap();
    for stamp in &ring.cliffs {
        let t = &scene.palette.templates[&(stamp.tile, 0)];
        for (sub, _) in t.cells.iter().enumerate().filter(|(_, s)| s.is_some()) {
            let at = [
                stamp.origin[0] + sub as u16 % t.width,
                stamp.origin[1] + sub as u16 / t.width,
            ];
            assert_eq!(
                sample(&doc, &scene, at).unwrap(),
                stamp.base + 4,
                "tile {}, sub {}",
                stamp.tile,
                sub
            );
        }
    }
}

#[test]
fn raw_flatten_keeps_cliff_offsets_while_flattening_nearby_ground() {
    let (mut doc, scene) = setup();
    let ring =
        cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([40, 40], [52, 52]), 4).unwrap();
    ring.apply(&mut doc).unwrap();
    doc.commit("ring");
    let selected = ring.edits.keys().copied().chain([[35, 40]]).collect();
    terrain_tools::height(&mut doc, &scene, &selected, TerrainMode::Flatten, 9, false).unwrap();
    for stamp in &ring.cliffs {
        let t = &scene.palette.templates[&(stamp.tile, 0)];
        for (sub, _) in t.cells.iter().enumerate().filter(|(_, s)| s.is_some()) {
            let at = [
                stamp.origin[0] + sub as u16 % t.width,
                stamp.origin[1] + sub as u16 / t.width,
            ];
            assert_eq!(value(&doc, &scene, at), ring.edits[&at]);
        }
    }
    assert_eq!(value(&doc, &scene, [35, 40]).level, 9);
}

#[test]
fn malformed_cliff_subtiles_are_not_silently_sampled_repainted_or_skipped_by_repair() {
    let (mut doc, scene) = setup_level(0);
    let brush = scene
        .brushes
        .iter()
        .find(|b| {
            cliff_geometry(
                &scene,
                TerrainValue {
                    tile: b.raw,
                    sub: b.key.sub_tile,
                    level: 0,
                    ice: 0,
                },
            )
            .unwrap()
            .is_some_and(|(h, _)| h == 4)
        })
        .unwrap();
    let at = [46, 46];
    doc.set(
        Target::Terrain(at),
        Some(Value::Terrain(TerrainValue {
            tile: brush.raw,
            sub: brush.key.sub_tile,
            level: 2,
            ice: 0,
        })),
    )
    .unwrap();
    let before = doc.snapshot().bytes().unwrap();
    assert!(terrain_tools::stamp(&mut doc, &scene, brush, at, false).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.set(
        Target::Terrain(at),
        Some(Value::Terrain(TerrainValue {
            tile: brush.raw,
            sub: 255,
            level: 2,
            ice: 0,
        })),
    )
    .unwrap();
    assert!(sample(&doc, &scene, at).is_err());
    assert!(repair(&doc, &scene, &BTreeSet::from([at]), 0).is_err());
}

#[test]
fn cliff_subtile_paint_keeps_relative_height_is_repeatable_and_samples_partial_templates() {
    let (mut doc, scene) = setup_level(0);
    let high = scene
        .brushes
        .iter()
        .find(|b| {
            cliff_geometry(
                &scene,
                TerrainValue {
                    tile: b.raw,
                    sub: b.key.sub_tile,
                    level: 0,
                    ice: 0,
                },
            )
            .unwrap()
            .is_some_and(|(relative, _)| relative == 4)
        })
        .unwrap();
    let at = [46, 46];
    let preview = terrain_tools::subtile_placement(&doc, &scene, high, at).unwrap();
    assert_eq!(preview.level, 4);
    terrain_tools::stamp(&mut doc, &scene, high, at, false).unwrap();
    assert_eq!(value(&doc, &scene, at), preview);
    assert_eq!(
        sample(&doc, &scene, at).unwrap(),
        4,
        "a single hand-placed subtile can be sampled"
    );
    assert!(
        terrain_tools::stamp(&mut doc, &scene, high, at, false)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        value(&doc, &scene, at).level,
        4,
        "repainting does not stack heights"
    );
    doc.cancel();
    let old = value(&doc, &scene, at);
    doc.set(
        Target::Terrain(at),
        Some(Value::Terrain(TerrainValue { level: 127, ..old })),
    )
    .unwrap();
    let before = doc.snapshot().bytes().unwrap();
    assert!(terrain_tools::stamp(&mut doc, &scene, high, at, false).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn legacy_zero_level_cliff_subtiles_can_be_repaired_then_filled_without_replacing_art() {
    let (mut doc, scene) = setup_level(0);
    let ring =
        cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([40, 40], [52, 52]), 0).unwrap();
    ring.apply(&mut doc).unwrap();
    for &at in ring.edits.keys() {
        let old = value(&doc, &scene, at);
        doc.set(
            Target::Terrain(at),
            Some(Value::Terrain(TerrainValue { level: 0, ..old })),
        )
        .unwrap();
    }
    doc.commit("old subtile ring");
    let before = doc.snapshot().bytes().unwrap();
    let selected = cliff_tools::rectangle([35, 35], [60, 60]);
    let p = repair(&doc, &scene, &selected, 0).unwrap();
    assert!(!p.edits.is_empty());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    assert!(p.edits.keys().all(|at| !ring.interior.contains(at)));
    for (&at, v) in &p.edits {
        let old = value(&doc, &scene, at);
        assert_eq!((v.tile, v.sub, v.ice), (old.tile, old.sub, old.ice));
    }
    let blocked = *p.edits.keys().next().unwrap();
    doc.set(Target::Overlay(blocked), Some(Value::Overlay(1, 0)))
        .unwrap();
    assert!(repair(&doc, &scene, &selected, 0).is_err());
    doc.cancel();
    p.apply(&mut doc).unwrap();
    doc.commit("repair heights");
    let repaired = doc.snapshot().bytes().unwrap();
    assert!(repair(&doc, &scene, &selected, 0).unwrap().edits.is_empty());
    for (&at, &v) in &ring.edits {
        if !ring.interior.contains(&at) {
            assert_eq!(value(&doc, &scene, at), v);
        }
    }
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.redo();
    assert_eq!(doc.snapshot().bytes().unwrap(), repaired);
    fill(&doc, &scene, [46, 46], 4)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    for (&at, &v) in &ring.edits {
        assert_eq!(value(&doc, &scene, at), v);
    }
}

fn hollow(doc: &mut MapDocument, scene: &MapScene) -> PlateauPlan {
    let p = cliff_tools::plan(doc, scene, cliff_tools::rectangle([40, 40], [52, 52]), 4).unwrap();
    p.apply(doc).unwrap();
    // Model a map whose cliffs have been placed but whose interior is still low.
    for &at in &p.interior {
        let old = value(doc, scene, at);
        doc.set(
            Target::Terrain(at),
            Some(Value::Terrain(TerrainValue { level: 4, ..old })),
        )
        .unwrap();
    }
    doc.commit("hollow cliffs");
    p
}

#[test]
fn height_fill_preserves_cliffs_and_material_and_roundtrips_through_history_and_save() {
    let (mut doc, scene) = setup();
    let cliffs = hollow(&mut doc, &scene);
    let before = doc.snapshot().bytes().unwrap();
    let p = fill(&doc, &scene, [46, 46], 8).unwrap();
    assert_eq!(p.interior, cliffs.interior);
    assert_eq!(p.edits.len(), cliffs.interior.len());
    assert_eq!(
        doc.snapshot().bytes().unwrap(),
        before,
        "preview must not mutate"
    );
    for (&at, v) in &p.edits {
        let old = value(&doc, &scene, at);
        assert_eq!((v.tile, v.sub, v.ice), (old.tile, old.sub, old.ice));
        assert_eq!(v.level, 8);
    }
    p.apply(&mut doc).unwrap();
    doc.commit("fill");
    let after = doc.snapshot().bytes().unwrap();
    let map = vera20k_core::map::map_file::load_from_bytes(after.clone()).unwrap();
    let baseline = ra2_map_document::document::Baseline::parse(after.clone().into(), &map).unwrap();
    let reopened = MapDocument::new(baseline);
    for (&at, v) in &cliffs.edits {
        if !cliffs.interior.contains(&at) {
            assert_eq!(value(&reopened, &scene, at), *v, "existing cliff unchanged");
        }
    }
    for &at in &p.interior {
        assert_eq!(value(&reopened, &scene, at).level, 8);
    }
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.redo();
    assert_eq!(doc.snapshot().bytes().unwrap(), after);
}

#[test]
fn height_fill_rejects_open_contours_structures_and_overlays_without_partial_changes() {
    let (mut doc, scene) = setup();
    let before = doc.snapshot().bytes().unwrap();
    assert!(
        fill(&doc, &scene, [46, 46], 8)
            .err()
            .unwrap()
            .to_string()
            .contains("地图边缘")
    );
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    let p = hollow(&mut doc, &scene);
    let wall = p.cliffs[0].origin;
    assert!(fill(&doc, &scene, wall, 8).is_err());
    doc.set(Target::Overlay([46, 46]), Some(Value::Overlay(1, 0)))
        .unwrap();
    let before = doc.snapshot().bytes().unwrap();
    assert!(
        fill(&doc, &scene, [45, 45], 8)
            .err()
            .unwrap()
            .to_string()
            .contains("覆盖物")
    );
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.cancel();
    doc.add_record("Structures", [46, 46], "Neutral", "TEST")
        .unwrap();
    let before = doc.snapshot().bytes().unwrap();
    assert!(
        fill(&doc, &scene, [45, 45], 8)
            .err()
            .unwrap()
            .to_string()
            .contains("占地")
    );
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn height_fill_follows_a_concave_outline_without_changing_the_cliff_corner() {
    let (mut doc, scene) = setup();
    let region =
        cliff_tools::enclose(&[[40, 40], [56, 40], [56, 48], [48, 48], [48, 56], [40, 56]])
            .unwrap();
    let cliffs = cliff_tools::plan(&doc, &scene, region, 4).unwrap();
    cliffs.apply(&mut doc).unwrap();
    for &at in &cliffs.interior {
        let old = value(&doc, &scene, at);
        doc.set(
            Target::Terrain(at),
            Some(Value::Terrain(TerrainValue {
                level: 4,
                ice: 17,
                ..old
            })),
        )
        .unwrap();
    }
    doc.commit("concave hollow");
    let plan = fill(&doc, &scene, [44, 44], 8).unwrap();
    assert_eq!(plan.interior, cliffs.interior);
    assert!(
        !plan.interior.contains(&[52, 52]),
        "concave cutout remains outside"
    );
    assert!(plan.edits.values().all(|v| v.ice == 17));
    plan.apply(&mut doc).unwrap();
    for (&at, &v) in &cliffs.edits {
        if !cliffs.interior.contains(&at) {
            assert_eq!(value(&doc, &scene, at), v);
        }
    }
}

#[test]
fn height_fill_stops_at_a_different_height_and_supports_high_targets() {
    let (mut doc, scene) = setup();
    let _ = hollow(&mut doc, &scene);
    let at = [46, 46];
    let old = value(&doc, &scene, at);
    doc.set(
        Target::Terrain(at),
        Some(Value::Terrain(TerrainValue { level: 7, ..old })),
    )
    .unwrap();
    let p = fill(&doc, &scene, [45, 45], 127).unwrap();
    assert!(!p.edits.contains_key(&at));
    assert!(p.edits.values().all(|v| v.level == 127));
    assert!(fill(&doc, &scene, [45, 45], 128).is_err());
}

#[test]
fn cliff_placement_rejects_implied_top_overflow_and_never_places_a_negative_back_face_base() {
    let (mut doc, scene) = setup_level(0);
    let back = scene
        .brushes
        .iter()
        .find(|b| {
            cliff_geometry(
                &scene,
                TerrainValue {
                    tile: b.raw,
                    sub: b.key.sub_tile,
                    level: 0,
                    ice: 0,
                },
            )
            .ok()
            .flatten()
            .is_some()
                && scene.palette.templates[&(b.key.tile_id, 0)]
                    .cells
                    .iter()
                    .flatten()
                    .all(|s| s.height == 4)
        })
        .unwrap();
    let planned = terrain_tools::template_placement(&doc, &scene, back, [46, 46], None).unwrap();
    assert!(planned.iter().all(|(_, v)| v.level == 4));
    terrain_tools::stamp_at_height(&mut doc, &scene, back, [46, 46], true, None).unwrap();
    assert_eq!(sample(&doc, &scene, [46, 46]).unwrap(), 4);
    doc.cancel();
    let low = scene
        .brushes
        .iter()
        .find(|b| {
            cliff_geometry(
                &scene,
                TerrainValue {
                    tile: b.raw,
                    sub: b.key.sub_tile,
                    level: 0,
                    ice: 0,
                },
            )
            .ok()
            .flatten()
            .is_some_and(|(relative, _)| relative == 0)
                && scene.palette.templates[&(b.key.tile_id, 0)]
                    .cells
                    .iter()
                    .flatten()
                    .all(|s| s.height == 0)
        })
        .unwrap();
    assert!(terrain_tools::template_placement(&doc, &scene, low, [46, 46], Some(124)).is_err());
    assert!(
        terrain_tools::subtile_placement_at_height(&doc, &scene, low, [46, 46], Some(124)).is_err()
    );
}

#[test]
fn cliff_click_expands_one_stamp_or_the_connected_group_without_leaking_to_other_islands() {
    let (mut doc, scene) = setup_level(0);
    let p = cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([40, 40], [52, 52]), 0).unwrap();
    p.apply(&mut doc).unwrap();
    let distant =
        cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([62, 62], [68, 68]), 0).unwrap();
    distant.apply(&mut doc).unwrap();
    doc.commit("separate cliff islands");
    let stamp = &p.cliffs[0];
    let t = &scene.palette.templates[&(stamp.tile, 0)];
    let sub = t.cells.iter().position(Option::is_some).unwrap() as u16;
    let at = [
        stamp.origin[0] + sub % t.width,
        stamp.origin[1] + sub / t.width,
    ];
    let single = cliff_selection(&doc, &scene, at, false).unwrap().unwrap();
    assert_eq!(single.len(), t.cells.iter().flatten().count());
    let connected = cliff_selection(&doc, &scene, at, true).unwrap().unwrap();
    let expected: BTreeSet<_> = p
        .edits
        .keys()
        .filter(|at| !p.interior.contains(*at))
        .copied()
        .collect();
    assert_eq!(connected, expected);
    assert!(
        cliff_selection(&doc, &scene, [46, 46], true)
            .unwrap()
            .is_none()
    );
    let before = doc.snapshot().bytes().unwrap();
    terrain_tools::height_with_change(
        &mut doc,
        &scene,
        &connected,
        TerrainMode::Raise,
        0,
        false,
        terrain_tools::HeightChange::Step(2),
    )
    .unwrap();
    for &at in &connected {
        assert_eq!(value(&doc, &scene, at).level, p.edits[&at].level + 2);
    }
    for (&at, &v) in &distant.edits {
        assert_eq!(value(&doc, &scene, at), v);
    }
    doc.commit("raise connected cliffs");
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn raising_a_low_only_cliff_validates_the_implied_top_before_writing() {
    let (mut doc, scene) = setup_level(0);
    let ring =
        cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([40, 40], [52, 52]), 0).unwrap();
    let stamp = ring
        .cliffs
        .iter()
        .find(|s| {
            scene.palette.templates[&(s.tile, 0)]
                .cells
                .iter()
                .flatten()
                .all(|s| s.height == 0)
        })
        .unwrap();
    let t = &scene.palette.templates[&(stamp.tile, 0)];
    let mut cells = BTreeSet::new();
    for (sub, s) in t.cells.iter().enumerate() {
        if s.is_some() {
            let at = [
                stamp.origin[0] + sub as u16 % t.width,
                stamp.origin[1] + sub as u16 / t.width,
            ];
            let old = ring.edits[&at];
            doc.set(
                Target::Terrain(at),
                Some(Value::Terrain(TerrainValue { level: 123, ..old })),
            )
            .unwrap();
            cells.insert(at);
        }
    }
    doc.commit("highest implied cliff top");
    assert_eq!(sample(&doc, &scene, *cells.first().unwrap()).unwrap(), 127);
    let before = doc.snapshot().bytes().unwrap();
    assert!(
        terrain_tools::height_with_change(
            &mut doc,
            &scene,
            &cells,
            TerrainMode::Raise,
            0,
            false,
            terrain_tools::HeightChange::Step(1)
        )
        .is_err()
    );
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn raw_cliff_height_moves_whole_templates_once_and_rejects_damaged_stamps() {
    let (mut doc, scene) = setup();
    let p = cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([40, 40], [52, 52]), 4).unwrap();
    p.apply(&mut doc).unwrap();
    doc.commit("cliffs");
    let stamp = p
        .cliffs
        .iter()
        .find(|s| {
            let t = &scene.palette.templates[&(s.tile, 0)];
            t.cells.iter().flatten().any(|s| s.height == 0)
                && t.cells.iter().flatten().any(|s| s.height == 4)
        })
        .unwrap();
    let t = &scene.palette.templates[&(stamp.tile, 0)];
    let cells: BTreeSet<_> = t
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
    let low = *cells
        .iter()
        .find(|&&at| value(&doc, &scene, at).level == 4)
        .unwrap();
    assert_eq!(sample(&doc, &scene, low).unwrap(), 8);
    let before = doc.snapshot().bytes().unwrap();
    for (change, mode, delta) in [
        (terrain_tools::HeightChange::Step(4), TerrainMode::Raise, 4),
        (terrain_tools::HeightChange::To(12), TerrainMode::Raise, 4),
        (terrain_tools::HeightChange::To(4), TerrainMode::Lower, -4),
        (terrain_tools::HeightChange::To(4), TerrainMode::Raise, 0),
    ] {
        terrain_tools::height_with_change(&mut doc, &scene, &cells, mode, 0, false, change)
            .unwrap();
        for &at in &cells {
            assert_eq!(
                i16::from(value(&doc, &scene, at).level),
                i16::from(p.edits[&at].level) + delta
            );
        }
        doc.cancel();
    }
    for change in [
        terrain_tools::HeightChange::Step(127),
        terrain_tools::HeightChange::To(0),
    ] {
        assert!(
            terrain_tools::height_with_change(
                &mut doc,
                &scene,
                &cells,
                TerrainMode::Lower,
                0,
                false,
                change
            )
            .is_err()
        );
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
    }
    let changed =
        terrain_tools::height(&mut doc, &scene, &cells, TerrainMode::Raise, 0, false).unwrap();
    assert_eq!(changed.len(), cells.len());
    for &at in &cells {
        assert_eq!(value(&doc, &scene, at).level, p.edits[&at].level + 1);
    }
    doc.cancel();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    let old = value(&doc, &scene, low);
    doc.set(
        Target::Terrain(low),
        Some(Value::Terrain(TerrainValue {
            level: old.level + 1,
            ..old
        })),
    )
    .unwrap();
    let before = doc.snapshot().bytes().unwrap();
    assert!(terrain_tools::height(&mut doc, &scene, &cells, TerrainMode::Lower, 0, false).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.cancel();
    for &at in &cells {
        let old = value(&doc, &scene, at);
        doc.set(
            Target::Terrain(at),
            Some(Value::Terrain(TerrainValue {
                level: old.level - 4,
                ..old
            })),
        )
        .unwrap();
    }
    assert!(
        terrain_tools::height(
            &mut doc,
            &scene,
            &BTreeSet::from([low]),
            TerrainMode::Lower,
            0,
            false
        )
        .is_err()
    );
}

#[test]
fn raw_cliff_height_moves_cropped_subtiles_and_checks_the_implied_base() {
    use crate::cliff_authoring::{stamp, stamp_clipped};
    use terrain_tools::HeightChange;
    let (mut doc, scene) = setup();
    let cliff = scene.palette.tile_ids.unwrap().cliffs.cliff_set.unwrap();
    let (corner, upper) = ([69, 10], [73, 14]);
    let cells = stamp_clipped(&doc, &scene, cliff + 4, corner, 4).unwrap();
    let high = stamp_clipped(&doc, &scene, cliff + 17, upper, 4).unwrap();
    assert_eq!(cells.len(), 3);
    assert!(stamp(&doc, &scene, cliff + 4, corner, 4).is_err());
    assert_eq!(high.len(), 1);
    assert_eq!(high[&upper].level, 8);
    for (&at, &v) in cells.iter().chain(&high) {
        doc.set(Target::Terrain(at), Some(Value::Terrain(v)))
            .unwrap();
    }
    doc.commit("edge cliffs");
    let before = doc.snapshot().bytes().unwrap();
    let expected = cells.keys().copied().collect::<BTreeSet<_>>();
    assert_eq!(
        cliff_selection(&doc, &scene, corner, false)
            .unwrap()
            .unwrap(),
        expected
    );
    let changed = terrain_tools::height(
        &mut doc,
        &scene,
        &BTreeSet::from([corner]),
        TerrainMode::Raise,
        0,
        false,
    )
    .unwrap();
    assert_eq!(changed.into_iter().collect::<BTreeSet<_>>(), expected);
    for (&at, &old) in &cells {
        let v = value(&doc, &scene, at);
        assert_eq!(
            v,
            TerrainValue {
                level: old.level + 1,
                ..old
            }
        );
    }
    doc.cancel();
    assert!(
        terrain_tools::height_with_change(
            &mut doc,
            &scene,
            &BTreeSet::from([upper]),
            TerrainMode::Lower,
            0,
            false,
            HeightChange::Step(5),
        )
        .is_err()
    );
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}
