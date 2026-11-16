use super::*;
use crate::cliff_tools;
use ra2_map_assets::{catalog::Catalog, test_support::Fixture};
use ra2_map_document::{
    document::{Target, Value},
    new_map::NewMapOptions,
};

fn setup() -> (MapDocument, MapScene) {
    setup_size(60)
}

fn setup_size(size: u16) -> (MapDocument, MapScene) {
    let f = Fixture::with_authoring();
    let mut i = f.install();
    i.catalog = Catalog::load(&i.manager, &i.theater);
    let s = crate::map::load_scene_bytes(
        &mut i,
        "edit.map",
        "test".into(),
        NewMapOptions {
            width: size,
            height: size,
            level: 4,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
        std::time::Instant::now(),
    )
    .unwrap();
    (
        MapDocument::new(s.source.edit_base.as_ref().unwrap().clone()),
        s,
    )
}
fn complete(doc: &MapDocument, scene: &MapScene, p: &PlateauPlan) {
    for s in &p.cliffs {
        let t = &scene.palette.templates[&(s.tile, 0)];
        for (i, c) in t.cells.iter().enumerate() {
            if let Some(c) = c {
                let at = offset(
                    s.origin,
                    i as i32 % i32::from(t.width),
                    i as i32 / i32::from(t.width),
                )
                .unwrap();
                let v = value(doc, scene, at);
                assert_eq!(
                    (v.tile, v.sub, v.level),
                    (scene.catalog.raw_tiles[&s.tile], i as u8, s.base + c.height)
                );
            }
        }
    }
}

#[test]
fn polyline_internal_vertex_keeps_native_corners_for_both_sides_and_all_rotations() {
    let (doc, scene) = setup();
    for rotation in 0..4 {
        let rotate = |at: Cell| {
            let (mut x, mut y) = (i32::from(at[0]) - 60, i32::from(at[1]) - 60);
            for _ in 0..rotation {
                (x, y) = (-y, x);
            }
            [(60 + x) as u16, (60 + y) as u16]
        };
        let path = [[40, 60], [60, 60], [60, 40]].map(rotate);
        let closed = [path[0], path[1], path[2], rotate([40, 40]), path[0]];
        for high_left in [true, false] {
            let reference = polyline(&doc, &scene, &closed, 4, high_left).unwrap();
            let p = polyline(&doc, &scene, &path, 4, high_left).unwrap();
            let reversed: Vec<_> = path.into_iter().rev().collect();
            assert_eq!(
                p.edits,
                polyline(&doc, &scene, &reversed, 4, !high_left)
                    .unwrap()
                    .edits
            );
            let corners: Vec<_> = reference
                .cliffs
                .iter()
                .filter(|s| {
                    let dx = i32::from(s.origin[0]) - 60;
                    let dy = i32::from(s.origin[1]) - 60;
                    dx * dx + dy * dy <= 4
                })
                .collect();
            assert!(!corners.is_empty());
            for corner in corners {
                assert!(
                    p.cliffs
                        .iter()
                        .any(|s| s.origin == corner.origin && s.tile == corner.tile),
                    "joint must preserve the whole native corner: rotation={rotation}, left={high_left}, origin={:?}",
                    corner.origin
                );
                for (at, v) in stamp(&doc, &scene, corner.tile, corner.origin, 4).unwrap() {
                    assert_eq!(p.edits.get(&at), Some(&v));
                }
            }
        }
    }
}

#[test]
fn polyline_oblique_joint_matches_the_same_corner_when_closed() {
    let (doc, scene) = setup_size(80);
    for rotation in 0..4 {
        let rotate = |at: Cell| {
            let (mut x, mut y) = (i32::from(at[0]) - 80, i32::from(at[1]) - 80);
            for _ in 0..rotation {
                (x, y) = (-y, x);
            }
            [(80 + x) as u16, (80 + y) as u16]
        };
        let path = [[60, 58], [80, 80], [84, 60]].map(rotate);
        let closed = [path[0], path[1], path[2], rotate([64, 38]), path[0]];
        for high_left in [true, false] {
            let reference = polyline(&doc, &scene, &closed, 4, high_left).unwrap();
            let p = polyline(&doc, &scene, &path, 4, high_left).unwrap();
            assert!(p.interior.is_empty());
            let reversed: Vec<_> = path.into_iter().rev().collect();
            assert_eq!(
                p.edits,
                polyline(&doc, &scene, &reversed, 4, !high_left)
                    .unwrap()
                    .edits
            );
            let corners: Vec<_> = reference
                .cliffs
                .iter()
                .filter(|s| {
                    let dx = i32::from(s.origin[0]) - 80;
                    let dy = i32::from(s.origin[1]) - 80;
                    dx * dx + dy * dy <= 16
                })
                .collect();
            assert!(!corners.is_empty());
            for corner in corners {
                assert!(
                    p.cliffs
                        .iter()
                        .any(|s| s.origin == corner.origin && s.tile == corner.tile),
                    "oblique joint must match the closed corner: rotation={rotation}, left={high_left}, missing={corner:?}, actual={:?}",
                    p.cliffs
                        .iter()
                        .filter(|s| s.origin[0].abs_diff(80) <= 4 && s.origin[1].abs_diff(80) <= 4)
                        .collect::<Vec<_>>()
                );
                for (at, v) in stamp(&doc, &scene, corner.tile, corner.origin, 4).unwrap() {
                    assert_eq!(p.edits.get(&at), Some(&v));
                }
            }
        }
    }
}

#[test]
fn polyline_closed_direction_chooses_plateau_or_pit_without_changing_the_exterior() {
    let (mut doc, scene) = setup();
    let clockwise = [[40, 40], [64, 40], [64, 64], [40, 64], [40, 40]];
    let mut counterclockwise = clockwise;
    counterclockwise.reverse();
    let before = doc.snapshot().bytes().unwrap();
    for (path, high_left, target) in [
        (&clockwise, false, 8),
        (&clockwise, true, 0),
        (&counterclockwise, true, 8),
        (&counterclockwise, false, 0),
    ] {
        let p = polyline(&doc, &scene, path, 4, high_left).unwrap();
        assert_eq!(p.edits[&[50, 50]].level, target);
        assert!(
            !p.edits.contains_key(&[30, 30]),
            "the exterior must retain its original height"
        );
        p.apply(&mut doc).unwrap();
        complete(&doc, &scene, &p);
        assert_eq!(value(&doc, &scene, [30, 30]).level, 4);
        doc.commit("directed closed cliff");
        doc.undo();
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
    }
}

#[test]
fn polyline_pits_require_raised_matching_exterior_and_support_the_height_limit() {
    let (mut doc, scene) = setup();
    let path = [[40, 40], [64, 40], [64, 64], [40, 64], [40, 40]];
    for ground in [0, 127] {
        for c in &scene.cells {
            let at = [c.rx, c.ry];
            if doc.check_cell(at).is_ok() {
                doc.set(
                    Target::Terrain(at),
                    Some(Value::Terrain(TerrainValue {
                        level: ground,
                        ..value(&doc, &scene, at)
                    })),
                )
                .unwrap();
            }
        }
        doc.commit("reference ground");
        let before = doc.snapshot().bytes().unwrap();
        if ground == 0 {
            let error = polyline(&doc, &scene, &path, 0, true).err().unwrap();
            assert!(error.to_string().contains("请先抬高地面"));
            let error = polyline(&doc, &scene, &path, 4, true).err().unwrap();
            assert!(error.to_string().contains("轮廓外侧地面须统一"));
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
            continue;
        }
        let p = polyline(&doc, &scene, &path, ground, true).unwrap();
        assert_eq!(p.base, 123);
        assert!(p.interior.iter().all(|at| p.edits[at].level == 123));
        assert!(p.edits.values().all(|v| (123..=127).contains(&v.level)));
        p.apply(&mut doc).unwrap();
        complete(&doc, &scene, &p);
        for c in &scene.cells {
            let at = [c.rx, c.ry];
            if doc.check_cell(at).is_ok() && !p.edits.contains_key(&at) {
                assert_eq!(value(&doc, &scene, at).level, ground);
            }
        }
        doc.commit("maximum height pit");
        doc.undo();
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
    }
}

#[test]
fn polyline_open_faces_all_directions_keep_only_complete_edge_templates() {
    let (mut doc, scene) = setup();
    let before = doc.snapshot().bytes().unwrap();
    for high_left in [false, true] {
        for (dx, dy) in [
            (16, 0),
            (0, 16),
            (-16, 0),
            (0, -16),
            (16, 16),
            (-16, 16),
            (16, -16),
            (-16, -16),
        ] {
            let a = [60, 60];
            let b = offset(a, dx, dy).unwrap();
            let p = polyline(&doc, &scene, &[a, b], 4, high_left).unwrap();
            assert!(!p.cliffs.is_empty());
            assert!(
                p.interior.is_empty(),
                "open edges must never fill a strip or polygon"
            );
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
            let reversed = polyline(&doc, &scene, &[b, a], 4, !high_left).unwrap();
            assert_eq!(
                p.edits, reversed.edits,
                "reversing the edge and high side preserves geometry"
            );
            let mut covered = BTreeSet::new();
            let mut main_faces = 0;
            for s in &p.cliffs {
                let x = i32::from(s.origin[0]) - i32::from(a[0]);
                let y = i32::from(s.origin[1]) - i32::from(a[1]);
                let t = dx * x + dy * y;
                if crate::coastal_cliffs::piece(&scene, s.tile).is_some()
                    && (dx * y - dy * x).abs() <= 2 * (dx.abs() + dy.abs())
                    && (0..=dx * dx + dy * dy).contains(&t)
                {
                    main_faces += 1;
                }
                let template = &scene.palette.templates[&(s.tile, 0)];
                for (i, c) in template.cells.iter().enumerate() {
                    if c.is_some() {
                        covered.insert(
                            offset(
                                s.origin,
                                i as i32 % i32::from(template.width),
                                i as i32 / i32::from(template.width),
                            )
                            .unwrap(),
                        );
                    }
                }
            }
            assert!(main_faces > 0, "the requested cliff face must be present");
            assert_eq!(
                covered,
                p.edits.keys().copied().collect(),
                "open paths must write cliff template cells only"
            );
            for s in &p.cliffs {
                assert!(
                    crate::coastal_cliffs::piece(&scene, s.tile).is_some(),
                    "open paths must never add ramp templates"
                );
                let x = i32::from(s.origin[0]) - i32::from(a[0]);
                let y = i32::from(s.origin[1]) - i32::from(a[1]);
                assert!((dx * y - dy * x).abs() <= 2 * (dx.abs() + dy.abs()));
                assert!(
                    (0..=dx * dx + dy * dy).contains(&(dx * x + dy * y)),
                    "no end caps or return faces"
                );
            }
            p.apply(&mut doc).unwrap();
            complete(&doc, &scene, &p);
            doc.commit("open edge");
            let saved = doc.snapshot().bytes().unwrap();
            let (loaded, _) = ra2_map_document::source::SourceMap::parse(
                "open.map".into(),
                "test".into(),
                saved.clone(),
            )
            .unwrap();
            let reopened = MapDocument::new(loaded.edit_base.as_ref().unwrap().clone());
            for (&at, &v) in &p.edits {
                assert_eq!(reopened.terrain(at), Some(v));
            }
            doc.undo();
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
            doc.redo();
            assert_eq!(doc.snapshot().bytes().unwrap(), saved);
            doc.undo();
        }
    }
}

#[test]
fn polyline_closed_unifies_uneven_interior_independent_of_direction() {
    let (mut doc, scene) = setup();
    doc.set(
        Target::Terrain([50, 50]),
        Some(Value::Terrain(TerrainValue {
            level: 12,
            ..value(&doc, &scene, [50, 50])
        })),
    )
    .unwrap();
    doc.commit("uneven ground");
    let before = doc.snapshot().bytes().unwrap();
    let path = [[40, 40], [64, 40], [64, 64], [40, 64], [40, 40]];
    let p = polyline(&doc, &scene, &path, 4, false).unwrap();
    let mut reversed = path;
    reversed.reverse();
    assert_eq!(
        p.edits,
        polyline(&doc, &scene, &reversed, 4, true).unwrap().edits
    );
    assert!(!p.interior.is_empty());
    assert!(p.interior.iter().all(|at| p.edits[at].level == 8));
    p.apply(&mut doc).unwrap();
    complete(&doc, &scene, &p);
    assert_eq!(doc.terrain([50, 50]).unwrap().level, 8);
    doc.commit("closed cliff");
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn polyline_oblique_edges_do_not_accept_uncovered_height_boundaries() {
    let (doc, scene) = setup();
    let start = [55, 55];
    let mut oblique = 0;
    for dx in [-17, -11, -5, 0, 5, 11, 17] {
        for dy in [-17, -11, -5, 0, 5, 11, 17] {
            if dx == 0 && dy == 0 {
                continue;
            }
            let end = offset(start, dx, dy).unwrap();
            for high_left in [false, true] {
                let Ok(p) = polyline(&doc, &scene, &[start, end], 4, high_left) else {
                    continue;
                };
                if dx != 0 && dy != 0 && dx.abs() != dy.abs() {
                    oblique += 1;
                }
                assert_eq!(
                    p.edits,
                    polyline(&doc, &scene, &[end, start], 4, !high_left)
                        .unwrap()
                        .edits
                );
                let rim: BTreeSet<_> = cell_line(start, end).into_iter().collect();
                let high = |at: Cell| {
                    let x = i32::from(at[0]) - i32::from(start[0]);
                    let y = i32::from(at[1]) - i32::from(start[1]);
                    rim.contains(&at)
                        || if high_left {
                            dx * y - dy * x <= 0
                        } else {
                            dx * y - dy * x >= 0
                        }
                };
                let project = |at: Cell| {
                    let x = i32::from(at[0]) - i32::from(start[0]);
                    let y = i32::from(at[1]) - i32::from(start[1]);
                    (x * dx + y * dy) as f64 / f64::from(dx * dx + dy * dy)
                };
                for at in cliff_tools::rectangle([35, 35], [75, 75]) {
                    for (nx, ny) in [(1, 0), (0, 1)] {
                        let next = offset(at, nx, ny).unwrap();
                        if (0.15..0.85).contains(&project(at))
                            && (0.15..0.85).contains(&project(next))
                            && high(at) != high(next)
                        {
                            assert!(
                                p.edits.contains_key(&at) || p.edits.contains_key(&next),
                                "uncovered edge {start:?}->{end:?}, left={high_left}: {at:?}->{next:?}"
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(
        oblique > 0,
        "the coverage check must exercise accepted oblique lines"
    );
}

#[test]
fn polyline_near_grid_edge_checks_written_templates_not_virtual_padding() {
    let (doc, scene) = setup();
    let path = [[5, 59], [5, 63]];
    for at in path {
        doc.check_cell(at).unwrap();
    }
    for high_left in [true, false] {
        let p = polyline(&doc, &scene, &path, 4, high_left).unwrap();
        assert!(!p.edits.is_empty());
        assert!(p.edits.keys().all(|&at| doc.check_cell(at).is_ok()));
    }
    // Actual template cells crossing the Size diamond are still rejected.
    let path = [[2, 60], [2, 61]];
    for at in path {
        doc.check_cell(at).unwrap();
    }
    assert!(
        [true, false]
            .into_iter()
            .any(|high_left| polyline(&doc, &scene, &path, 4, high_left).is_err())
    );
}

#[test]
fn polyline_open_faces_preserve_surrounding_ground_at_the_maximum_height() {
    let (mut doc, scene) = setup();
    let path = [[50, 55], [70, 55]];
    let p = polyline(&doc, &scene, &path, 4, true).unwrap();
    let stamped: BTreeSet<_> = p
        .cliffs
        .iter()
        .flat_map(|s| {
            stamp(&doc, &scene, s.tile, s.origin, s.base)
                .unwrap()
                .into_keys()
        })
        .collect();
    assert_eq!(stamped, p.edits.keys().copied().collect());
    let at = *p.edits.keys().next().unwrap();
    doc.set(Target::Overlay(at), Some(Value::Overlay(0, 0)))
        .unwrap();
    let before = doc.snapshot().bytes().unwrap();
    assert!(polyline(&doc, &scene, &path, 4, true).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.cancel();
    for c in &scene.cells {
        let at = [c.rx, c.ry];
        if doc.check_cell(at).is_ok() {
            doc.set(
                Target::Terrain(at),
                Some(Value::Terrain(TerrainValue {
                    level: 123,
                    ..value(&doc, &scene, at)
                })),
            )
            .unwrap();
        }
    }
    doc.commit("high ground");
    let before = doc.snapshot().bytes().unwrap();
    let p = polyline(&doc, &scene, &path, 123, true).unwrap();
    assert!(p.edits.values().all(|v| (123..=127).contains(&v.level)));
    assert!(p.edits.values().any(|v| v.level == 127));
    p.apply(&mut doc).unwrap();
    complete(&doc, &scene, &p);
    for c in &scene.cells {
        let at = [c.rx, c.ry];
        if doc.check_cell(at).is_ok() && !p.edits.contains_key(&at) {
            assert_eq!(
                value(&doc, &scene, at).level,
                123,
                "surrounding ground must stay at its original height"
            );
            assert!(
                crate::coastal_cliffs::piece(
                    &scene,
                    scene.palette.raw_ids[&value(&doc, &scene, at).tile]
                )
                .is_none()
            );
        }
    }
    doc.commit("supported high cliff");
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn polyline_turns_and_protected_cells_are_atomic() {
    let (mut doc, scene) = setup();
    for high_left in [false, true] {
        for path in [
            vec![[40, 40], [60, 40], [60, 60]],
            vec![[40, 40], [60, 40], [60, 60], [40, 60]],
            vec![[40, 40], [60, 60], [70, 50]],
        ] {
            let p = polyline(&doc, &scene, &path, 4, high_left).unwrap();
            let middle = [(path[0][0] + path[1][0]) / 2, (path[0][1] + path[1][1]) / 2];
            let at = *p
                .edits
                .iter()
                .find(|(at, v)| {
                    at[0].abs_diff(middle[0]) + at[1].abs_diff(middle[1]) <= 3
                        && crate::coastal_cliffs::piece(&scene, scene.palette.raw_ids[&v.tile])
                            .is_some()
                })
                .expect("the requested face remains at the middle of the first segment")
                .0;
            doc.set(Target::Overlay(at), Some(Value::Overlay(0, 0)))
                .unwrap();
            let before = doc.snapshot().bytes().unwrap();
            assert!(polyline(&doc, &scene, &path, 4, high_left).is_err());
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
            doc.cancel();
        }
    }
    for path in [&[][..], &[[40, 40]][..], &[[40, 40], [40, 40]][..]] {
        assert!(polyline(&doc, &scene, path, 4, true).is_err());
    }
    assert!(polyline(&doc, &scene, &[[40, 40], [60, 40]], 124, true).is_err());
    for path in [
        vec![[40, 40], [60, 60], [40, 60], [60, 40]],
        vec![[40, 40], [60, 40], [50, 40]],
        vec![[40, 40], [60, 40], [60, 60], [50, 40]],
    ] {
        assert!(polyline(&doc, &scene, &path, 4, true).is_err());
    }
}
#[test]
fn cliff_strokes_both_faces_all_directions_are_atomic_and_complete() {
    let (mut doc, scene) = setup();
    let before = doc.snapshot().bytes().unwrap();
    for back in [false, true] {
        for (dx, dy) in [
            (12, 0),
            (0, 12),
            (-12, 0),
            (0, -12),
            (12, 12),
            (-12, 12),
            (12, -12),
            (-12, -12),
        ] {
            let p = line(
                &doc,
                &scene,
                [55, 55],
                offset([55, 55], dx, dy).unwrap(),
                4,
                back,
            )
            .unwrap();
            assert!(!p.cliffs.is_empty());
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
            p.apply(&mut doc).unwrap();
            complete(&doc, &scene, &p);
            for &at in p.edits.keys() {
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let Some(other) = offset(at, dx, dy) else {
                        continue;
                    };
                    if doc.check_cell(other).is_err() {
                        continue;
                    }
                    let a = value(&doc, &scene, at);
                    let b = value(&doc, &scene, other);
                    assert!(
                        a.level == b.level
                            || cliff(&scene, a, at).is_some()
                            || cliff(&scene, b, other).is_some(),
                        "free ends must close with complete cliff faces: {at:?} -> {other:?}"
                    );
                }
            }
            doc.commit("line");
            doc.undo();
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
            doc.redo();
            complete(&doc, &scene, &p);
            doc.undo();
        }
    }
}

#[test]
fn turning_a_cliff_stroke_rebuilds_the_join_as_complete_templates() {
    let (mut doc, scene) = setup();
    for end in [[65, 50], [55, 62], [65, 60]] {
        line(&doc, &scene, [43, 50], [55, 50], 4, false)
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        doc.commit("first");
        let before = doc.snapshot().bytes().unwrap();
        let p = line(&doc, &scene, [55, 50], end, 4, false).unwrap();
        p.apply(&mut doc).unwrap();
        complete(&doc, &scene, &p);
        doc.commit("turn");
        doc.undo();
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        doc.undo();
    }
}
#[test]
fn line_continuation_keeps_the_previous_segment_and_rejects_occupied_changes() {
    let (mut doc, scene) = setup();
    let first = line(&doc, &scene, [45, 50], [55, 50], 4, false).unwrap();
    first.apply(&mut doc).unwrap();
    doc.commit("first");
    let old = value(&doc, &scene, [47, 49]);
    let second = line(&doc, &scene, [55, 50], [65, 50], 4, false).unwrap();
    second.apply(&mut doc).unwrap();
    complete(&doc, &scene, &second);
    assert_eq!(value(&doc, &scene, [47, 49]).level, old.level);
    doc.cancel();
    doc.set(Target::Overlay([60, 49]), Some(Value::Overlay(1, 0)))
        .unwrap();
    doc.commit("block");
    let before = doc.snapshot().bytes().unwrap();
    assert!(line(&doc, &scene, [55, 50], [65, 50], 4, false).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}
#[test]
fn recover_plateau_then_expand_merge_and_cut_without_old_internal_cliffs() {
    let (mut doc, scene) = setup();
    let p = cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([40, 40], [50, 50]), 4).unwrap();
    p.apply(&mut doc).unwrap();
    doc.commit("original");
    let (core, _) = existing(&doc, &scene, &BTreeSet::from([[45, 45]]), 4).unwrap();
    assert_eq!(
        core, p.interior,
        "recover logical interior from disk geometry"
    );
    let before = doc.snapshot().bytes().unwrap();
    let p = reshape(
        &doc,
        &scene,
        cliff_tools::rectangle([48, 40], [60, 50]),
        4,
        false,
    )
    .unwrap();
    p.apply(&mut doc).unwrap();
    complete(&doc, &scene, &p);
    doc.commit("expand");
    assert_eq!(value(&doc, &scene, [57, 45]).level, 8);
    assert!(cliff(&scene, value(&doc, &scene, [50, 45]), [50, 45]).is_none());
    let p = reshape(
        &doc,
        &scene,
        cliff_tools::rectangle([55, 37], [64, 54]),
        4,
        true,
    )
    .unwrap();
    p.apply(&mut doc).unwrap();
    complete(&doc, &scene, &p);
    doc.commit("cut");
    assert_eq!(value(&doc, &scene, [58, 45]).level, 4);
    doc.undo();
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn ramps_in_all_four_directions_replace_complete_cliffs_and_undo_as_one() {
    let (mut doc, scene) = setup();
    cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([40, 40], [58, 58]), 4)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    doc.commit("plateau");
    let before = doc.snapshot().bytes().unwrap();
    for (start, end) in [
        ([49, 58], [49, 63]),
        ([58, 49], [63, 49]),
        ([49, 40], [49, 35]),
        ([40, 49], [35, 49]),
    ] {
        let p = ramp(&doc, &scene, start, end, 4).unwrap();
        p.apply(&mut doc).unwrap();
        complete(&doc, &scene, &p);
        assert!(p.edits.values().any(|v| (5..8).contains(&v.level)));
        doc.commit("ramp");
        doc.undo();
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
    }
    assert!(
        ramp(&doc, &scene, [49, 49], [49, 54], 4).is_err(),
        "flat centre has no cliff to replace"
    );
    doc.add_record("Units", [49, 58], "Neutral", "GI").unwrap();
    doc.commit("block");
    assert!(ramp(&doc, &scene, [49, 58], [49, 63], 4).is_err());
}

#[test]
fn merge_two_plateaus_then_remove_everything_and_recover_after_reload() {
    let (mut doc, scene) = setup();
    for (a, b) in [([35, 40], [44, 52]), ([54, 40], [65, 52])] {
        cliff_tools::plan(&doc, &scene, cliff_tools::rectangle(a, b), 4)
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        doc.commit("bank");
    }
    let p = reshape(
        &doc,
        &scene,
        cliff_tools::rectangle([42, 40], [57, 52]),
        4,
        false,
    )
    .unwrap();
    p.apply(&mut doc).unwrap();
    doc.commit("merge");
    for x in 36..64 {
        assert_eq!(value(&doc, &scene, [x, 45]).level, 8);
    }
    let bytes = doc.snapshot().bytes().unwrap();
    let (source, _) =
        ra2_map_document::source::SourceMap::parse("saved.map".into(), "test".into(), bytes)
            .unwrap();
    let mut reopened = MapDocument::new(source.edit_base.as_ref().unwrap().clone());
    let p = reshape(
        &reopened,
        &scene,
        cliff_tools::rectangle([30, 35], [70, 57]),
        4,
        true,
    )
    .unwrap();
    assert!(p.cliffs.is_empty());
    p.apply(&mut reopened).unwrap();
    for x in 34..67 {
        for y in 39..54 {
            assert_eq!(value(&reopened, &scene, [x, y]).level, 4);
        }
    }
}

#[test]
fn reshaping_another_edge_preserves_existing_ramp_templates() {
    let (mut doc, scene) = setup();
    cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([40, 40], [58, 58]), 4)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    doc.commit("plateau");
    let entrance = ramp(&doc, &scene, [49, 58], [49, 63], 4).unwrap();
    entrance.apply(&mut doc).unwrap();
    doc.commit("ramp");
    let p = reshape(
        &doc,
        &scene,
        cliff_tools::rectangle([55, 40], [65, 53]),
        4,
        false,
    )
    .unwrap();
    p.apply(&mut doc).unwrap();
    for (&at, &v) in &entrance.edits {
        assert_eq!(value(&doc, &scene, at), v);
    }
}

#[test]
fn ramp_width_drag_fills_continuous_slopes_in_both_directions_on_every_edge() {
    let (mut doc, scene) = setup();
    cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([40, 40], [58, 58]), 4)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    doc.commit("plateau");
    let before = doc.snapshot().bytes().unwrap();
    for (start, axis, slope) in [
        ([49, 58], 0, 4),
        ([58, 49], 1, 3),
        ([49, 40], 0, 2),
        ([40, 49], 1, 1),
    ] {
        let narrow = ramp(&doc, &scene, start, start, 4).unwrap();
        for delta in [-4, -2, 2, 4] {
            let mut end = start;
            end[axis] = (i32::from(end[axis]) + delta) as u16;
            let p = ramp(&doc, &scene, start, end, 4)
                .unwrap_or_else(|e| panic!("{start:?} -> {end:?}: {e:#}"));
            assert_eq!(
                p.edits.len(),
                narrow.edits.len() + delta.unsigned_abs() as usize * 4
            );
            let id = (scene.palette.tile_ids.unwrap().ramp_base + slope - 1) as u16;
            let strips: Vec<_> = p.cliffs.iter().filter(|s| s.tile == id).collect();
            assert_eq!(strips.len(), delta.unsigned_abs() as usize * 4);
            // Each inserted lane has the same four consecutive slope cells,
            // with one level per step, as the inner edges of the two end caps.
            let lanes: BTreeSet<_> = strips.iter().map(|s| s.origin[axis]).collect();
            for lane in lanes {
                let mut steps: Vec<_> = strips
                    .iter()
                    .filter(|s| s.origin[axis] == lane)
                    .copied()
                    .collect();
                steps.sort_by_key(|s| s.origin[1 - axis]);
                assert_eq!(steps.len(), 4);
                assert_eq!(
                    steps.iter().map(|s| s.base).collect::<Vec<_>>(),
                    if slope >= 3 {
                        vec![7, 6, 5, 4]
                    } else {
                        vec![4, 5, 6, 7]
                    }
                );
                assert_eq!(steps[3].origin[1 - axis] - steps[0].origin[1 - axis], 3);
            }
            assert_eq!(
                doc.snapshot().bytes().unwrap(),
                before,
                "preview must be read-only"
            );
            p.apply(&mut doc).unwrap();
            complete(&doc, &scene, &p);
            doc.commit("wide ramp");
            let after = doc.snapshot().bytes().unwrap();
            let (source, _) = ra2_map_document::source::SourceMap::parse(
                "wide.map".into(),
                "test".into(),
                after.clone(),
            )
            .unwrap();
            let reopened = MapDocument::new(source.edit_base.as_ref().unwrap().clone());
            complete(&reopened, &scene, &p);
            doc.undo();
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
            doc.redo();
            assert_eq!(doc.snapshot().bytes().unwrap(), after);
            doc.undo();
        }
    }
}

#[test]
fn wide_ramp_rejects_obstructions_and_corners_without_partial_changes() {
    let (mut doc, scene) = setup();
    cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([40, 40], [58, 58]), 4)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    doc.commit("plateau");
    let start = [49, 58];
    let end = [53, 58];
    let narrow = ramp(&doc, &scene, start, start, 4).unwrap();
    let wide = ramp(&doc, &scene, start, end, 4).unwrap();
    let at = *wide
        .edits
        .keys()
        .find(|at| !narrow.edits.contains_key(*at))
        .unwrap();
    for unit in [false, true] {
        if unit {
            doc.add_record("Units", at, "Neutral", "GI").unwrap();
        } else {
            doc.set(Target::Overlay(at), Some(Value::Overlay(1, 0)))
                .unwrap();
        }
        doc.commit("obstruction");
        let before = doc.snapshot().bytes().unwrap();
        assert!(ramp(&doc, &scene, start, start, 4).is_ok());
        assert!(ramp(&doc, &scene, start, end, 4).is_err());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        doc.undo();
    }
    let before = doc.snapshot().bytes().unwrap();
    for end in [[65, 58], [30, 58]] {
        assert!(ramp(&doc, &scene, start, end, 4).is_err());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
    }
}

#[test]
fn rectangular_corner_slopes_cover_the_corner_and_join_both_edges() {
    let (mut doc, scene) = setup();
    cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([40, 40], [58, 58]), 4)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    doc.commit("plateau");
    let before = doc.snapshot().bytes().unwrap();
    let ramp_base = scene.palette.tile_ids.unwrap().ramp_base as u16;
    for (corner, outward, slope) in [
        ([40, 40], [-1, -1], 5),
        ([58, 40], [1, -1], 6),
        ([40, 58], [-1, 1], 8),
        ([58, 58], [1, 1], 7),
    ] {
        for (dx, dy) in [(0, 0), (2, 0), (0, 2), (4, 0), (0, 4), (2, 2), (-2, -2)] {
            let end = offset(corner, -outward[0] * dx, -outward[1] * dy).unwrap();
            let p = ramp(&doc, &scene, corner, end, 4)
                .unwrap_or_else(|e| panic!("{corner:?} -> {end:?}: {e:#}"));
            for k in 0..4 {
                let at = offset(corner, outward[0] * k, outward[1] * k).unwrap();
                assert_eq!(
                    p.edits[&at].tile,
                    scene.catalog.raw_tiles[&(ramp_base + slope - 1)]
                );
                assert_eq!(p.edits[&at].level, 7 - k as u8);
            }
            // Shared vertices must have identical heights throughout the
            // straight and diagonal body, including its unchanged flat rim.
            let patterns = [
                [0, 0, 0, 0],
                [0, 1, 1, 0],
                [0, 0, 1, 1],
                [1, 0, 0, 1],
                [1, 1, 0, 0],
                [0, 0, 1, 0],
                [0, 0, 0, 1],
                [1, 0, 0, 0],
                [0, 1, 0, 0],
            ];
            let mut vertices = BTreeMap::new();
            for (&at, &v) in &p.edits {
                let id = scene.palette.raw_ids[&v.tile];
                if !(ramp_base..ramp_base + 8).contains(&id) {
                    continue;
                }
                let shape = usize::from(id - ramp_base + 1);
                for ((x, y), h) in [(0, 0), (1, 0), (1, 1), (0, 1)]
                    .into_iter()
                    .zip(patterns[shape])
                {
                    let vertex = (at[0] + x, at[1] + y);
                    let height = v.level + h;
                    if let Some(old) = vertices.insert(vertex, height) {
                        assert_eq!(old, height, "surface seam at {vertex:?}");
                    }
                }
                for (dx, dy, own) in [
                    (1, 0, [1, 2]),
                    (-1, 0, [0, 3]),
                    (0, 1, [3, 2]),
                    (0, -1, [0, 1]),
                ] {
                    let n = offset(at, dx, dy).unwrap();
                    if p.edits.contains_key(&n) {
                        continue;
                    }
                    let old = value(&doc, &scene, n);
                    let old_id = scene.palette.raw_ids[&old.tile];
                    if old_id != scene.palette.tile_ids.unwrap().clear as u16 {
                        continue;
                    }
                    for a in own {
                        assert_eq!(
                            v.level + patterns[shape][a],
                            old.level,
                            "slope must meet flat ground at {at:?} -> {n:?}"
                        );
                    }
                }
            }
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
            p.apply(&mut doc).unwrap();
            complete(&doc, &scene, &p);
            doc.commit("corner ramp");
            let after = doc.snapshot().bytes().unwrap();
            let (source, _) = ra2_map_document::source::SourceMap::parse(
                "corner.map".into(),
                "test".into(),
                after.clone(),
            )
            .unwrap();
            let reopened = MapDocument::new(source.edit_base.as_ref().unwrap().clone());
            complete(&reopened, &scene, &p);
            doc.undo();
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
            doc.redo();
            assert_eq!(doc.snapshot().bytes().unwrap(), after);
            doc.undo();
        }
    }
}

#[test]
fn corner_slope_obstructions_and_excess_width_are_atomic() {
    let (mut doc, scene) = setup();
    cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([40, 40], [58, 58]), 4)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    doc.commit("plateau");
    for start in [[40, 40], [58, 40], [40, 58], [58, 58]] {
        for unit in [false, true] {
            if unit {
                doc.add_record("Units", start, "Neutral", "GI").unwrap();
            } else {
                doc.set(Target::Overlay(start), Some(Value::Overlay(1, 0)))
                    .unwrap();
            }
            doc.commit("obstruction");
            let before = doc.snapshot().bytes().unwrap();
            assert!(
                ramp(&doc, &scene, start, start, 4).is_err(),
                "corner must actually be edited"
            );
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
            doc.undo();
        }
    }
    let before = doc.snapshot().bytes().unwrap();
    assert!(ramp(&doc, &scene, [58, 58], [28, 58], 4).is_err());
    assert!(ramp(&doc, &scene, [49, 49], [51, 49], 4).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn straight_and_corner_entrances_join_in_either_order_on_all_eight_edges() {
    let (mut doc, scene) = setup();
    cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([35, 35], [65, 65]), 4)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    doc.commit("plateau");
    for (corner, inward) in [
        ([35, 35], [1, 1]),
        ([65, 35], [-1, 1]),
        ([35, 65], [1, -1]),
        ([65, 65], [-1, -1]),
    ] {
        for axis in 0..2 {
            for corner_first in [true, false] {
                let mut start = corner;
                start[axis] = (i32::from(corner[axis]) + inward[axis] * 10) as u16;
                let mut end = start;
                end[axis] = (i32::from(start[axis]) - inward[axis] * 6) as u16;
                let (first_start, first_end, second_start, second_end) = if corner_first {
                    (corner, corner, start, end)
                } else {
                    (start, end, corner, corner)
                };
                let first = ramp(&doc, &scene, first_start, first_end, 4).unwrap();
                first.apply(&mut doc).unwrap();
                doc.commit("first entrance");
                let before = doc.snapshot().bytes().unwrap();
                let joined = ramp(&doc, &scene, second_start, second_end, 4).unwrap_or_else(|e| {
                    panic!("corner={corner:?} axis={axis} corner_first={corner_first}: {e:#}")
                });
                let ends = joined
                    .cliffs
                    .iter()
                    .filter(|s| {
                        let ids = scene.palette.tile_ids.unwrap();
                        (ids.slope_set_pieces..ids.slope_set_pieces + 10)
                            .contains(&i32::from(s.tile))
                    })
                    .count();
                assert_eq!(
                    ends,
                    if corner == [35, 35] { 4 } else { 2 },
                    "only the two outer ends may remain after joining"
                );
                assert!(
                    first
                        .edits
                        .iter()
                        .any(|(at, v)| joined.edits.get(at).is_some_and(|n| n.tile != v.tile)),
                    "the facing end cap must be removed"
                );
                assert_eq!(doc.snapshot().bytes().unwrap(), before);
                joined.apply(&mut doc).unwrap();
                complete(&doc, &scene, &joined);
                let v = value(&doc, &scene, corner);
                let id = scene.palette.raw_ids[&v.tile];
                assert!(
                    (5..=8).contains(
                        &scene.palette.templates[&(id, 0)].cells[usize::from(v.sub)]
                            .unwrap()
                            .slope
                    )
                );
                // Every old body cell survives; the facing end caps become
                // ordinary continuous slopes in the rebuilt entrance.
                for (&at, &v) in &first.edits {
                    let id = scene.palette.raw_ids[&v.tile];
                    let base = scene.palette.tile_ids.unwrap().ramp_base as u16;
                    if (base..base + 8).contains(&id) {
                        assert_eq!(value(&doc, &scene, at), v);
                    }
                }
                doc.commit("join");
                let after = doc.snapshot().bytes().unwrap();
                let (source, _) = ra2_map_document::source::SourceMap::parse(
                    "joined.map".into(),
                    "test".into(),
                    after.clone(),
                )
                .unwrap();
                let reopened = MapDocument::new(source.edit_base.as_ref().unwrap().clone());
                complete(&reopened, &scene, &joined);
                doc.undo();
                assert_eq!(doc.snapshot().bytes().unwrap(), before);
                doc.redo();
                assert_eq!(doc.snapshot().bytes().unwrap(), after);
                doc.undo();
                doc.undo();
            }
        }
    }
}

#[test]
fn continuing_from_saved_ramp_caps_and_corner_bodies_keeps_the_existing_surface() {
    let (mut doc, scene) = setup();
    cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([35, 35], [65, 65]), 4)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    doc.commit("plateau");
    for corner in [[35, 35], [65, 35], [35, 65], [65, 65]] {
        let p = ramp(&doc, &scene, corner, corner, 4).unwrap();
        p.apply(&mut doc).unwrap();
        doc.commit("corner");
        let bytes = doc.snapshot().bytes().unwrap();
        let (source, _) =
            ra2_map_document::source::SourceMap::parse("saved.map".into(), "test".into(), bytes)
                .unwrap();
        let reopened = MapDocument::new(source.edit_base.as_ref().unwrap().clone());
        assert_eq!(infer_base(&reopened, &scene, corner), 4);
        for axis in 0..2 {
            let mut end = corner;
            end[axis] = if corner[axis] == 35 {
                corner[axis] + 4
            } else {
                corner[axis] - 4
            };
            let wider = ramp(&reopened, &scene, corner, end, 4)
                .unwrap_or_else(|e| panic!("continue corner {corner:?} -> {end:?}: {e:#}"));
            assert!(wider.edits.contains_key(&corner));
        }
        for stamp in p.cliffs.iter().filter(|s| {
            let ids = scene.palette.tile_ids.unwrap();
            (ids.slope_set_pieces..ids.slope_set_pieces + 10).contains(&i32::from(s.tile))
        }) {
            let t = &scene.palette.templates[&(stamp.tile, 0)];
            let sub = t.cells.iter().position(Option::is_some).unwrap();
            let at = offset(
                stamp.origin,
                sub as i32 % i32::from(t.width),
                sub as i32 / i32::from(t.width),
            )
            .unwrap();
            assert_eq!(infer_base(&reopened, &scene, at), 4);
            let extended = ramp(&reopened, &scene, at, at, 4)
                .unwrap_or_else(|e| panic!("saved cap {at:?}: {e:#}"));
            assert!(extended.edits.contains_key(&corner));
        }
        doc.undo();
    }
}

#[test]
fn joining_ramps_protects_objects_overlays_and_incomplete_end_caps() {
    let (mut doc, scene) = setup();
    cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([35, 35], [65, 65]), 4)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    doc.commit("plateau");
    let first = ramp(&doc, &scene, [65, 65], [65, 65], 4).unwrap();
    first.apply(&mut doc).unwrap();
    doc.commit("corner");
    let joined = ramp(&doc, &scene, [55, 65], [61, 65], 4).unwrap();
    let at = *first
        .edits
        .keys()
        .find(|&&at| {
            joined
                .edits
                .get(&at)
                .is_some_and(|&v| v != value(&doc, &scene, at))
        })
        .unwrap();
    for unit in [false, true] {
        if unit {
            doc.add_record("Units", at, "Neutral", "GI").unwrap();
        } else {
            doc.set(Target::Overlay(at), Some(Value::Overlay(1, 0)))
                .unwrap();
        }
        doc.commit("block join");
        let before = doc.snapshot().bytes().unwrap();
        assert!(ramp(&doc, &scene, [55, 65], [61, 65], 4).is_err());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        doc.undo();
    }
    let broken = clear(&scene, value(&doc, &scene, at), 4).unwrap();
    doc.set(Target::Terrain(at), Some(Value::Terrain(broken)))
        .unwrap();
    doc.commit("incomplete cap");
    let before = doc.snapshot().bytes().unwrap();
    assert!(ramp(&doc, &scene, [55, 65], [61, 65], 4).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn probe_continuation_from_every_ramp_subtile() {
    let (mut doc, scene) = setup();
    cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([35, 35], [65, 65]), 4)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    doc.commit("plateau");
    let mut failed = Vec::new();
    for (start, end) in [
        ([35, 35], [35, 35]),
        ([65, 35], [65, 35]),
        ([35, 65], [35, 65]),
        ([65, 65], [65, 65]),
        ([50, 35], [50, 35]),
        ([50, 65], [50, 65]),
        ([35, 50], [35, 50]),
        ([65, 50], [65, 50]),
    ] {
        let p = ramp(&doc, &scene, start, end, 4).unwrap();
        p.apply(&mut doc).unwrap();
        doc.commit("entrance");
        for &at in p.edits.keys() {
            let v = value(&doc, &scene, at);
            let id = scene.palette.raw_ids[&v.tile];
            let shape = scene.palette.templates[&(id, 0)].cells[usize::from(v.sub)].unwrap();
            if shape.slope == 0 {
                continue;
            }
            let base = infer_base(&doc, &scene, at);
            if ramp(&doc, &scene, at, at, base).is_err() {
                failed.push((start, at, base, id, v.sub));
            }
        }
        doc.undo();
    }
    assert!(failed.is_empty(), "unhandled slope cells: {failed:?}");
}

#[test]
fn probe_drag_from_every_end_cap_subtile() {
    let (mut doc, scene) = setup();
    cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([35, 35], [65, 65]), 4)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    doc.commit("plateau");
    let ids = scene.palette.tile_ids.unwrap();
    let mut failed = Vec::new();
    let mut passed = 0;
    for start in [[35, 35], [65, 35], [35, 65], [65, 65]] {
        let p = ramp(&doc, &scene, start, start, 4).unwrap();
        p.apply(&mut doc).unwrap();
        doc.commit("entrance");
        for (&at, &v) in &p.edits {
            let id = scene.palette.raw_ids[&v.tile];
            let index = i32::from(id) - ids.slope_set_pieces;
            if !(0..10).contains(&index) {
                continue;
            }
            let axis = if index <= 1 || (4..=6).contains(&index) {
                0
            } else {
                1
            };
            for sign in [-1, 1] {
                let end = offset(
                    at,
                    if axis == 0 { sign * 4 } else { 0 },
                    if axis == 1 { sign * 4 } else { 0 },
                )
                .unwrap();
                match ramp(&doc, &scene, at, end, infer_base(&doc, &scene, at)) {
                    Ok(_) => passed += 1,
                    Err(e) => failed.push(format!(
                        "corner={start:?}, at={at:?}, end={end:?}, cap={index}, sub={}: {e:#}",
                        v.sub
                    )),
                }
            }
        }
        doc.undo();
    }
    eprintln!("passed {passed}, failed {}", failed.len());
    for f in &failed {
        eprintln!("{f}");
    }
    assert!(failed.is_empty());
}

#[test]
fn probe_every_cliff_corner_subtile() {
    let (mut doc, scene) = setup();
    let plateau =
        cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([35, 35], [65, 65]), 4).unwrap();
    plateau.apply(&mut doc).unwrap();
    doc.commit("plateau");
    let base = scene.palette.tile_ids.unwrap().cliffs.cliff_set.unwrap();
    let mut failed = Vec::new();
    for stamp in &plateau.cliffs {
        if ![1, 12, 21, 29].contains(&(stamp.tile - base + 1)) {
            continue;
        }
        let t = &scene.palette.templates[&(stamp.tile, 0)];
        for (sub, c) in t.cells.iter().enumerate() {
            if c.is_none() {
                continue;
            }
            let at = offset(
                stamp.origin,
                sub as i32 % i32::from(t.width),
                sub as i32 / i32::from(t.width),
            )
            .unwrap();
            if let Err(e) = ramp(&doc, &scene, at, at, infer_base(&doc, &scene, at)) {
                failed.push(format!(
                    "at={at:?}, piece={} sub={sub}: {e:#}",
                    stamp.tile - base + 1
                ));
            }
        }
    }
    assert!(failed.is_empty(), "{failed:?}");
}

#[test]
fn probe_wide_candidate_can_bridge_gap_when_narrow_start_cannot_fit() {
    let (mut doc, scene) = setup();
    cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([35, 35], [65, 65]), 4)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    doc.commit("plateau");
    let mut failures = Vec::new();
    for (corner, inward) in [
        ([35, 35], [1, 1]),
        ([65, 35], [-1, 1]),
        ([35, 65], [1, -1]),
        ([65, 65], [-1, -1]),
    ] {
        ramp(&doc, &scene, corner, corner, 4)
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        doc.commit("corner");
        for axis in 0..2 {
            for distance in 1..=12 {
                let mut start = corner;
                start[axis] = (i32::from(start[axis]) + inward[axis] * distance) as u16;
                let mut end = start;
                end[axis] = (i32::from(start[axis]) + inward[axis] * 6) as u16;
                if ramp(&doc, &scene, start, end, 4).is_ok() {
                    continue;
                }
                let direction = if axis == 0 {
                    if inward[1] > 0 { 2 } else { 0 }
                } else if inward[0] > 0 {
                    3
                } else {
                    1
                };
                let mut valid = None;
                for x in -6..=1 {
                    for y in -6..=1 {
                        let Some(mut origin) = offset(start, x, y) else {
                            continue;
                        };
                        if inward[axis] < 0 {
                            origin[axis] -= 6;
                        }
                        if ramp_candidate(&doc, &scene, start, origin, 4, direction, 6).is_ok() {
                            valid = Some(origin);
                        }
                    }
                }
                if let Some(origin) = valid {
                    failures.push((corner, start, end, origin));
                }
            }
        }
        doc.undo();
    }
    assert!(
        failures.is_empty(),
        "valid wider plans incorrectly rejected: {failures:?}"
    );
}

#[test]
fn adjacent_corner_entrances_can_connect_along_the_shared_edge() {
    let (mut doc, scene) = setup();
    cliff_tools::plan(&doc, &scene, cliff_tools::rectangle([40, 40], [58, 58]), 4)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    doc.commit("plateau");
    ramp(&doc, &scene, [40, 58], [44, 58], 4)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    doc.commit("first corner");
    ramp(&doc, &scene, [58, 58], [54, 58], 4)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    doc.commit("second corner");
    let p = ramp(&doc, &scene, [49, 58], [51, 58], 4).unwrap();
    p.apply(&mut doc).unwrap();
    complete(&doc, &scene, &p);
}

#[test]
fn tall_cliffs_support_reshaping_ramps_save_reload_and_atomic_height_limits() {
    for base in [11, 24, 100, cliff_tools::MAX_CLIFF_BASE] {
        let (mut doc, scene) = setup();
        for cell in &scene.cells {
            let at = [cell.rx, cell.ry];
            let v = clear(&scene, value(&doc, &scene, at), base).unwrap();
            doc.set(Target::Terrain(at), Some(Value::Terrain(v)))
                .unwrap();
        }
        doc.commit("elevated ground");
        let ground = doc.snapshot().bytes().unwrap();
        let p = cliff_tools::plan(
            &doc,
            &scene,
            cliff_tools::rectangle([40, 40], [58, 58]),
            base,
        )
        .unwrap();
        p.apply(&mut doc).unwrap();
        complete(&doc, &scene, &p);
        doc.commit("tall plateau");
        let plateau = doc.snapshot().bytes().unwrap();
        assert_eq!(value(&doc, &scene, [49, 49]).level, base + 4);
        for (start, end) in [
            ([49, 58], [51, 58]),
            ([58, 49], [58, 51]),
            ([49, 40], [51, 40]),
            ([40, 49], [40, 51]),
            ([58, 58], [54, 58]),
            ([40, 40], [44, 40]),
            ([58, 40], [54, 40]),
            ([40, 58], [44, 58]),
        ] {
            assert_eq!(infer_base(&doc, &scene, start), base);
            let p = ramp(&doc, &scene, start, end, base).unwrap();
            p.apply(&mut doc).unwrap();
            complete(&doc, &scene, &p);
            doc.commit("tall ramp");
            let after = doc.snapshot().bytes().unwrap();
            let (source, _) = ra2_map_document::source::SourceMap::parse(
                "tall.map".into(),
                "test".into(),
                after.clone(),
            )
            .unwrap();
            let reopened = MapDocument::new(source.edit_base.as_ref().unwrap().clone());
            complete(&reopened, &scene, &p);
            doc.undo();
            assert_eq!(doc.snapshot().bytes().unwrap(), plateau);
            doc.redo();
            assert_eq!(doc.snapshot().bytes().unwrap(), after);
            doc.undo();
        }
        for cut in [false, true] {
            let p = reshape(
                &doc,
                &scene,
                cliff_tools::rectangle([52, 40], [64, 54]),
                base,
                cut,
            )
            .unwrap();
            p.apply(&mut doc).unwrap();
            complete(&doc, &scene, &p);
            doc.commit("tall reshape");
            doc.undo();
            assert_eq!(doc.snapshot().bytes().unwrap(), plateau);
        }
        doc.undo();
        assert_eq!(doc.snapshot().bytes().unwrap(), ground);
        let p = line(&doc, &scene, [43, 50], [59, 50], base, false).unwrap();
        p.apply(&mut doc).unwrap();
        complete(&doc, &scene, &p);
        doc.commit("tall line");
        doc.undo();
        assert_eq!(doc.snapshot().bytes().unwrap(), ground);
        for invalid in [cliff_tools::MAX_CLIFF_BASE + 1, u8::MAX] {
            assert!(
                cliff_tools::plan(
                    &doc,
                    &scene,
                    cliff_tools::rectangle([40, 40], [58, 58]),
                    invalid
                )
                .is_err()
            );
            assert!(
                reshape(
                    &doc,
                    &scene,
                    cliff_tools::rectangle([40, 40], [58, 58]),
                    invalid,
                    false
                )
                .is_err()
            );
            assert!(line(&doc, &scene, [43, 50], [59, 50], invalid, false).is_err());
            assert!(ramp(&doc, &scene, [49, 58], [51, 58], invalid).is_err());
            assert_eq!(doc.snapshot().bytes().unwrap(), ground);
        }
    }
}

#[test]
fn stacking_plateaus_can_cross_the_previous_fourteen_level_limit() {
    let (mut doc, scene) = setup();
    for (base, low, high) in [(4, 35, 65), (8, 39, 61), (12, 43, 57), (16, 47, 53)] {
        let p = reshape(
            &doc,
            &scene,
            cliff_tools::rectangle([low, low], [high, high]),
            base,
            false,
        )
        .unwrap();
        p.apply(&mut doc).unwrap();
        complete(&doc, &scene, &p);
        doc.commit("stacked cliff");
        assert_eq!(value(&doc, &scene, [50, 50]).level, base + 4);
    }
    assert_eq!(infer_base(&doc, &scene, [50, 50]), 16);
}
