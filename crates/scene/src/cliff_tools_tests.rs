use super::*;
use ra2_map_assets::{catalog::Catalog, test_support::Fixture};
use ra2_map_document::new_map::NewMapOptions;
use std::sync::Arc;

fn setup() -> (MapDocument, MapScene) {
    let fixture = Fixture::with_cliffs();
    let mut install = fixture.install();
    install.catalog = Catalog::load(&install.manager, &install.theater);
    let scene = crate::map::load_scene_bytes(
        &mut install,
        "cliffs.map",
        "test".into(),
        NewMapOptions {
            width: 40,
            height: 40,
            level: 4,
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
fn rectangles_choose_closed_edges_and_corners_keep_complete_templates_and_undo_once() {
    let (mut doc, scene) = setup();
    let before = doc.snapshot().bytes().unwrap();
    for width in 1..9 {
        for height in 1..9 {
            let region = rectangle([30, 30], [30 + width, 30 + height]);
            let p = plan(&doc, &scene, region.clone(), 4).unwrap();
            assert_eq!(
                doc.snapshot().bytes().unwrap(),
                before,
                "planning must not write"
            );
            p.apply(&mut doc).unwrap();
            for at in region {
                assert_eq!(doc.terrain(at).unwrap().level, 8);
            }
            for stamp in &p.cliffs {
                let t = &scene.palette.templates[&(stamp.tile, 0)];
                for (sub, s) in t.cells.iter().enumerate() {
                    if let Some(s) = s {
                        let at = [
                            stamp.origin[0] + sub as u16 % t.width,
                            stamp.origin[1] + sub as u16 / t.width,
                        ];
                        let v = doc.terrain(at).unwrap();
                        assert_eq!(v.tile, scene.catalog.raw_tiles[&stamp.tile]);
                        assert_eq!(v.sub, sub as u8);
                        assert_eq!(v.level, stamp.base + s.height);
                    }
                }
            }
            doc.cancel();
        }
    }
    let p = plan(&doc, &scene, rectangle([30, 30], [38, 38]), 4).unwrap();
    p.apply(&mut doc).unwrap();
    doc.commit("plateau");
    let after = doc.snapshot().bytes().unwrap();
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.redo();
    assert_eq!(doc.snapshot().bytes().unwrap(), after);
}
#[test]
fn open_l_outline_closes_fills_the_inside_and_selects_a_complete_concave_corner() {
    let (doc, scene) = setup();
    let path = [[28, 28], [46, 28], [46, 34], [35, 34], [35, 46], [28, 46]];
    let region = enclose(&path).unwrap();
    assert!(region.contains(&[30, 40]));
    assert!(!region.contains(&[42, 42]));
    let mut reversed = path;
    reversed.reverse();
    assert_eq!(region, enclose(&reversed).unwrap());
    let p = plan(&doc, &scene, region, 4).unwrap();
    let base = scene.palette.tile_ids.unwrap().cliffs.cliff_set.unwrap();
    assert!(
        p.cliffs
            .iter()
            .any(|s| (base + 8..=base + 10).contains(&s.tile)),
        "concave corner is a complete 3-cell piece"
    );
}
#[test]
fn protection_and_resource_errors_leave_the_entire_document_unchanged() {
    let (mut doc, mut scene) = setup();
    let region = rectangle([30, 30], [38, 38]);
    doc.set(Target::Overlay([34, 34]), Some(Value::Overlay(1, 0)))
        .unwrap();
    doc.commit("overlay");
    let before = doc.snapshot().bytes().unwrap();
    assert!(plan(&doc, &scene, region.clone(), 4).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.undo();
    doc.add_record("Units", [34, 34], "Neutral", "GI").unwrap();
    doc.commit("unit");
    assert!(plan(&doc, &scene, region.clone(), 4).is_err());
    doc.undo();
    assert!(plan(&doc, &scene, region.clone(), MAX_CLIFF_BASE + 1).is_err());
    assert!(plan(&doc, &scene, rectangle([1, 39], [8, 46]), 4).is_err());
    let base = scene.palette.tile_ids.unwrap().cliffs.cliff_set.unwrap();
    Arc::get_mut(&mut scene.palette)
        .unwrap()
        .templates
        .remove(&(base + 28, 0));
    assert!(plan(&doc, &scene, region, 4).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), scene.source.bytes());
}
#[test]
fn diagonal_and_rotated_concave_outlines_preserve_interior_without_overlapping_templates() {
    let (doc, scene) = setup();
    let mut paths = Vec::new();
    for r in 2..=10 {
        paths.push(vec![[38 - r, 38], [38, 38 - r], [38 + r, 38], [38, 38 + r]]);
    }
    let l = [[28, 28], [46, 28], [46, 34], [35, 34], [35, 46], [28, 46]];
    for rotation in 0..4 {
        paths.push(
            l.map(|mut p| {
                for _ in 0..rotation {
                    p = [76 - p[1], p[0]];
                }
                p
            })
            .to_vec(),
        );
    }
    paths.push(vec![
        [28, 32],
        [35, 28],
        [45, 32],
        [48, 43],
        [36, 48],
        [28, 43],
    ]);
    for path in paths {
        let region = enclose(&path).unwrap();
        let p = plan(&doc, &scene, region.clone(), 4).unwrap_or_else(|e| panic!("{path:?}: {e:#}"));
        for at in region {
            assert_eq!(p.edits[&at].level, 8);
        }
        let mut occupied = BTreeSet::new();
        for stamp in p.cliffs {
            let t = &scene.palette.templates[&(stamp.tile, 0)];
            for (sub, s) in t.cells.iter().enumerate() {
                if s.is_some() {
                    assert!(occupied.insert([
                        stamp.origin[0] + sub as u16 % t.width,
                        stamp.origin[1] + sub as u16 / t.width
                    ]));
                }
            }
        }
    }
}
#[test]
fn unsupported_environment_and_flat_or_empty_outlines_are_rejected() {
    let (doc, mut scene) = setup();
    assert!(enclose(&[[30, 30], [35, 30], [39, 30]]).is_err());
    assert!(enclose(&[]).is_err());
    Arc::get_mut(&mut scene.palette)
        .unwrap()
        .tile_ids
        .as_mut()
        .unwrap()
        .cliffs
        .cliff_set = None;
    assert!(plan(&doc, &scene, rectangle([30, 30], [38, 38]), 4).is_err());
}

#[test]
fn long_cliff_edges_mix_compatible_siblings_without_changing_the_elevation_plan() {
    let (doc, mut scene) = setup();
    let region = rectangle([29, 29], [49, 49]);
    let original = geometry(&doc, &scene, region.clone(), 4).unwrap();
    let styled = plan(&doc, &scene, region.clone(), 4).unwrap();
    assert_eq!(
        styled.edits,
        plan(&doc, &scene, region.clone(), 4).unwrap().edits
    );
    assert_eq!(styled.edits.len(), original.edits.len());
    for (&at, &v) in &original.edits {
        assert_eq!(
            (styled.edits[&at].level, styled.edits[&at].ice),
            (v.level, v.ice)
        );
    }
    let first = scene.palette.tile_ids.unwrap().cliffs.cliff_set.unwrap();
    for (low, axis) in [(5, 0), (15, 1), (23, 0), (35, 1)] {
        let mut wide: Vec<_> = styled
            .cliffs
            .iter()
            .filter(|s| (first + low - 1..=first + low + 1).contains(&s.tile))
            .collect();
        assert!(wide.len() >= 7, "too few wide pieces for family {low}");
        assert!(wide.iter().map(|s| s.tile).collect::<BTreeSet<_>>().len() >= 2);
        wide.sort_by_key(|s| s.origin[axis]);
        for pair in wide.windows(2) {
            assert_eq!(pair[0].origin[axis] + 2, pair[1].origin[axis]);
            assert_ne!(
                pair[0].tile, pair[1].tile,
                "adjacent siblings in family {low}"
            );
        }
    }
    // Leave the first family with a missing template, missing image, and a
    // geometrically incompatible replacement. All must fall back to #8.
    let palette = Arc::get_mut(&mut scene.palette).unwrap();
    palette.templates.remove(&(first + 4, 0));
    palette.images.remove(&TileKey {
        tile_id: first + 5,
        sub_tile: 0,
        variant: 0,
    });
    palette.templates.get_mut(&(first + 6, 0)).unwrap().cells[0]
        .as_mut()
        .unwrap()
        .terrain ^= 1;
    let fallback = plan(&doc, &scene, region, 4).unwrap();
    assert!(
        fallback
            .cliffs
            .iter()
            .all(|s| !(first + 4..=first + 6).contains(&s.tile))
    );
    for s in original.cliffs.iter().filter(|s| s.tile == first + 7) {
        assert!(
            fallback
                .cliffs
                .iter()
                .any(|t| t.tile == s.tile && t.origin == s.origin)
        );
    }
}

#[test]
fn cliff_variant_pairs_keep_their_style_when_the_start_of_a_straight_run_changes() {
    let (doc, scene) = setup();
    let base = scene.palette.tile_ids.unwrap().cliffs.cliff_set.unwrap();
    let run = |first, y| {
        let mut p = PlateauPlan {
            edits: BTreeMap::new(),
            overlays: BTreeMap::new(),
            cliffs: Vec::new(),
            interior: BTreeSet::new(),
            base: 4,
        };
        for x in first..49 {
            let origin = [x, y];
            p.edits
                .extend(crate::cliff_authoring::stamp(&doc, &scene, base + 7, origin, 4).unwrap());
            p.cliffs.push(CliffStamp {
                tile: base + 7,
                origin,
                base: 4,
            });
        }
        crate::cliff_variants::apply(&doc, &scene, &mut p);
        p.cliffs
            .iter()
            .map(|s| (s.origin, s.tile))
            .collect::<BTreeMap<_, _>>()
    };
    for y in 36..=42 {
        let long = run(30, y);
        let short = run(34, y);
        for (&at, &tile) in short.iter().filter(|(at, _)| at[0] > 34) {
            assert_eq!(
                long[&at], tile,
                "trimming a straight run changed the style at {at:?}"
            );
        }
    }
}

#[test]
fn cliff_variants_skip_an_equal_shape_in_another_tileset() {
    let (doc, mut scene) = setup();
    let region = rectangle([29, 29], [49, 49]);
    let base = scene.palette.tile_ids.unwrap().cliffs.cliff_set.unwrap();
    let tile = plan(&doc, &scene, region.clone(), 4)
        .unwrap()
        .cliffs
        .into_iter()
        .find(|s| (base + 4..=base + 6).contains(&s.tile))
        .unwrap()
        .tile;
    // A shortened mod CliffSet can make the numeric range straddle two sets.
    Arc::get_mut(&mut scene.palette)
        .unwrap()
        .groups
        .get_mut(&tile)
        .unwrap()
        .id += 1;
    let styled = plan(&doc, &scene, region.clone(), 4).unwrap();
    assert!(styled.cliffs.iter().all(|s| s.tile != tile));

    // A required narrow piece outside the set must fail the whole plan.
    Arc::get_mut(&mut scene.palette)
        .unwrap()
        .groups
        .get_mut(&(base + 37))
        .unwrap()
        .id += 1;
    assert!(plan(&doc, &scene, region, 4).is_err());
}
