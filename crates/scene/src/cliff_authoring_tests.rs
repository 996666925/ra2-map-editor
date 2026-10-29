use super::*;
use crate::cliff_tools;
use ra2_map_assets::{catalog::Catalog, test_support::Fixture};
use ra2_map_document::{
    document::{Target, Value},
    new_map::NewMapOptions,
};

fn setup() -> (MapDocument, MapScene) {
    let f = Fixture::with_authoring();
    let mut i = f.install();
    i.catalog = Catalog::load(&i.manager, &i.theater);
    let s = crate::map::load_scene_bytes(
        &mut i,
        "edit.map",
        "test".into(),
        NewMapOptions {
            width: 60,
            height: 60,
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
