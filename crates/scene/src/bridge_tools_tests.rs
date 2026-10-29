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
    let scene = crate::map::load_scene_bytes(
        &mut i,
        "bridge.map",
        "test".into(),
        NewMapOptions {
            width: 80,
            height: 80,
            level: 4,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
        std::time::Instant::now(),
    )
    .unwrap();
    (
        MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone()),
        scene,
    )
}
#[test]
fn high_bridges_both_materials_and_axes_write_heads_and_canonical_overlay_data() {
    for along_x in [true, false] {
        let (mut doc, scene) = setup();
        for (a, b) in if along_x {
            [([60, 60], [79, 74]), ([94, 60], [108, 74])]
        } else {
            [([60, 60], [74, 74]), ([60, 89], [74, 103])]
        } {
            cliff_tools::plan(&doc, &scene, cliff_tools::rectangle(a, b), 4)
                .unwrap()
                .apply(&mut doc)
                .unwrap();
            doc.commit("bank");
        }
        let before = doc.snapshot().bytes().unwrap();
        let (a, b) = if along_x {
            ([80, 67], [94, 67])
        } else {
            ([67, 75], [67, 89])
        };
        for kind in [BridgeKind::HighConcrete, BridgeKind::HighWood] {
            let p = plan(&doc, &scene, a, b, kind).unwrap();
            let reversed = plan(&doc, &scene, b, a, kind).unwrap();
            assert_eq!(p.edits, reversed.edits);
            assert_eq!(p.overlays, reversed.overlays);
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
            assert_eq!(p.cliffs.len(), 2);
            assert_eq!(p.overlays.len(), 12);
            let obstructed = if along_x { [86, 65] } else { [65, 81] };
            let mut blocked = doc.fork();
            blocked
                .set(Target::Overlay(obstructed), Some(Value::Overlay(1, 0)))
                .unwrap();
            assert!(
                plan(&blocked, &scene, a, b, kind).is_err(),
                "the extra structural cross row must be free"
            );
            let expected = match (kind, along_x) {
                (BridgeKind::HighConcrete, true) => (24, 0),
                (BridgeKind::HighConcrete, false) => (25, 9),
                (BridgeKind::HighWood, true) => (237, 0),
                _ => (238, 9),
            };
            assert!(p.overlays.values().all(|&v| v == expected));
            for &(id, _) in p.overlays.values() {
                let (_, direction) =
                    vera20k_core::map::bridge_facts::high_bridge_stamp_for_overlay(id).unwrap();
                let (x, y) = vera20k_core::util::direction::direction_delta(direction).unwrap();
                assert_eq!(
                    if along_x { x } else { y },
                    0,
                    "bridge setter cross row must be perpendicular to the span"
                );
            }
            for &at in p.overlays.keys() {
                assert_eq!(value(&doc, &scene, at).level, 4);
            }
            p.apply(&mut doc).unwrap();
            doc.commit("bridge");
            let mut scratch = doc.fork();
            let expansion = if along_x {
                cliff_tools::rectangle([60, 54], [74, 63])
            } else {
                cliff_tools::rectangle([54, 60], [63, 70])
            };
            crate::cliff_authoring::reshape(&scratch, &scene, expansion, 4, false)
                .unwrap()
                .apply(&mut scratch)
                .unwrap();
            for (&at, &v) in &p.edits {
                assert_eq!(
                    scratch.terrain(at),
                    Some(v),
                    "expansion must preserve bridgeheads"
                );
            }
            for (&at, &v) in &p.overlays {
                assert_eq!(scratch.overlay(at), v);
            }
            let after = doc.snapshot().bytes().unwrap();
            let (source, _) = ra2_map_document::source::SourceMap::parse(
                "saved.map".into(),
                "test".into(),
                after.clone(),
            )
            .unwrap();
            let reopened = MapDocument::new(source.edit_base.as_ref().unwrap().clone());
            for (&at, &v) in &p.overlays {
                assert_eq!(reopened.overlay(at), v);
            }
            for (&at, &v) in &p.edits {
                assert_eq!(reopened.terrain(at), Some(v));
            }
            doc.undo();
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
            doc.redo();
            assert_eq!(doc.snapshot().bytes().unwrap(), after);
            doc.undo();
        }
    }
}
#[test]
fn low_bridges_use_native_cross_rows_endpoints_and_preserve_ground() {
    let (mut doc, scene) = setup();
    let before = doc.snapshot().bytes().unwrap();
    for kind in [BridgeKind::LowWood, BridgeKind::LowConcrete] {
        for (b, along_x) in [([72, 60], true), ([60, 72], false)] {
            let p = plan(&doc, &scene, [60, 60], b, kind).unwrap();
            assert!(p.edits.is_empty());
            assert_eq!(p.overlays.len(), 39);
            let base = if kind == BridgeKind::LowWood { 74 } else { 205 };
            assert_eq!(
                p.overlays[&[60, 60]],
                (base + if along_x { 20 } else { 22 }, 1)
            );
            assert_eq!(p.overlays[&b], (base + if along_x { 18 } else { 24 }, 1));
            p.apply(&mut doc).unwrap();
            doc.commit("low bridge");
            doc.undo();
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
        }
    }
}
#[test]
fn invalid_bridge_geometry_occupied_cells_and_registry_changes_never_write() {
    let (mut doc, mut scene) = setup();
    assert!(plan(&doc, &scene, [60, 60], [62, 60], BridgeKind::LowWood).is_err());
    assert!(plan(&doc, &scene, [60, 60], [72, 60], BridgeKind::HighWood).is_err());
    doc.set(Target::Overlay([65, 59]), Some(Value::Overlay(1, 0)))
        .unwrap();
    doc.commit("block");
    let before = doc.snapshot().bytes().unwrap();
    assert!(plan(&doc, &scene, [60, 60], [72, 60], BridgeKind::LowWood).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.undo();
    std::sync::Arc::make_mut(&mut scene.catalog).overlays[74] = "INVALID".into();
    assert!(plan(&doc, &scene, [60, 60], [72, 60], BridgeKind::LowWood).is_err());
}
