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
        p.cliffs.iter().any(|s| s.tile == base + 8),
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
    assert!(plan(&doc, &scene, region.clone(), 11).is_err());
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
