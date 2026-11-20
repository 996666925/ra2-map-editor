use super::*;
use crate::cliff_tools;
use ra2_map_assets::{catalog::Catalog, test_support::Fixture};
use ra2_map_document::{
    document::{Target, Value},
    new_map::NewMapOptions,
};

fn setup() -> (MapDocument, MapScene) {
    setup_fixture(Fixture::with_joining())
}

fn setup_fixture(f: Fixture) -> (MapDocument, MapScene) {
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
fn reordered_wooden_heads_match_both_banks_and_survive_later_cliff_edits() {
    for axis in 0..2 {
        let f = Fixture::with_joining();
        // NEWURBAN: positive-X is piece 2 and negative-Y is piece 3;
        // pieces 4/5 are deck sections rather than negative-Y heads.
        for (from, to) in [(3, 2), (4, 3), (7, 4), (7, 5)] {
            std::fs::copy(
                f.0.join(format!("wood{from:02}.tem")),
                f.0.join(format!("wood{to:02}.tem")),
            )
            .unwrap();
        }
        let (mut doc, scene) = setup_fixture(f);
        let rotate = |[x, y]: Cell| if axis == 0 { [x, y] } else { [y, x] };
        for (a, b) in [([60, 60], [79, 74]), ([94, 60], [108, 74])] {
            cliff_tools::plan(
                &doc,
                &scene,
                cliff_tools::rectangle(rotate(a), rotate(b)),
                4,
            )
            .unwrap()
            .apply(&mut doc)
            .unwrap();
            doc.commit("bank");
        }
        let before = doc.snapshot().bytes().unwrap();
        let p = plan(
            &doc,
            &scene,
            rotate([77, 67]),
            rotate([98, 67]),
            BridgeKind::HighWood,
        )
        .unwrap();
        let family = scene
            .palette
            .tile_ids
            .unwrap()
            .cliffs
            .wood_bridge_set
            .unwrap();
        let expected = if axis == 0 { [0, 1] } else { [2, 5] };
        for (head, offset) in p.cliffs.iter().zip(expected) {
            assert_eq!(head.tile, family + offset);
        }
        p.apply(&mut doc).unwrap();
        doc.commit("bridge");
        let saved = doc.snapshot().bytes().unwrap();
        let (source, _) = ra2_map_document::source::SourceMap::parse(
            "saved.map".into(),
            "test".into(),
            saved.clone(),
        )
        .unwrap();
        let mut reopened = MapDocument::new(source.edit_base.as_ref().unwrap().clone());
        crate::cliff_authoring::reshape(
            &reopened,
            &scene,
            cliff_tools::rectangle(rotate([60, 54]), rotate([74, 63])),
            4,
            false,
        )
        .unwrap()
        .apply(&mut reopened)
        .unwrap();
        for (&at, &v) in &p.edits {
            if bridge_head(&scene, scene.palette.raw_ids[&v.tile]) {
                assert_eq!(reopened.terrain(at), Some(v));
            }
        }
        for (&at, &v) in &p.overlays {
            assert_eq!(reopened.overlay(at), v);
        }
        doc.undo();
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        doc.redo();
        assert_eq!(doc.snapshot().bytes().unwrap(), saved);
    }
}
#[test]
fn low_bridge_draft_tracks_open_water_endpoints_without_relaxing_placement() {
    for axis in 0..2 {
        let rotate = |[x, y]: Cell| if axis == 0 { [x, y] } else { [y, x] };
        let (mut doc, scene) = setup();
        crate::shore_tools::plan(
            &doc,
            &scene,
            cliff_tools::rectangle(rotate([70, 55]), rotate([104, 80])),
            Some(true),
        )
        .unwrap()
        .apply(&mut doc)
        .unwrap();
        doc.commit("wide river");
        let before = doc.snapshot().bytes().unwrap();
        for kind in [BridgeKind::LowWood, BridgeKind::LowConcrete] {
            for end in [76, 90] {
                let a = rotate([69, 67]);
                let b = rotate([end, 67]);
                let draft = preview_low(&doc, &scene, a, b, kind).unwrap();
                assert_eq!(draft.overlays.len(), usize::from(end - 69 + 1) * 3);
                assert!(draft.edits.is_empty());
                assert_eq!(
                    draft.overlays,
                    preview_low(&doc, &scene, b, a, kind).unwrap().overlays
                );
                assert!(plan(&doc, &scene, a, b, kind).is_err());
            }
            let a = rotate([69, 67]);
            let b = rotate([105, 67]);
            assert_eq!(
                preview_low(&doc, &scene, a, b, kind).unwrap().overlays,
                plan(&doc, &scene, a, b, kind).unwrap().overlays
            );
        }
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
    }
}

#[test]
fn low_bridge_accepts_non_morphable_dry_lats_but_rejects_impassable_banks() {
    use ra2_map_assets::catalog::{TerrainSubtile, TerrainTemplate};
    for axis in 0..2 {
        let rotate = |[x, y]: Cell| if axis == 0 { [x, y] } else { [y, x] };
        let (mut doc, mut scene) = setup();
        crate::shore_tools::plan(
            &doc,
            &scene,
            cliff_tools::rectangle(rotate([70, 55]), rotate([82, 80])),
            Some(true),
        )
        .unwrap()
        .apply(&mut doc)
        .unwrap();
        // A separate, non-morphable LAT outside the hardcoded terrain families.
        let id = 5000;
        std::sync::Arc::get_mut(&mut scene.palette)
            .unwrap()
            .raw_ids
            .insert(5000, id);
        std::sync::Arc::make_mut(&mut scene.catalog)
            .raw_tiles
            .insert(id, 5000);
        for at in cliff_tools::rectangle(rotate([63, 66]), rotate([90, 68])) {
            let old = value(&doc, &scene, at);
            if !crate::shore_tools::wet(&scene, old) {
                doc.set(
                    Target::Terrain(at),
                    Some(Value::Terrain(ra2_map_document::document::TerrainValue {
                        tile: 5000,
                        sub: 0,
                        ..old
                    })),
                )
                .unwrap();
            }
        }
        doc.commit("LAT river banks");
        let before = doc.snapshot().bytes().unwrap();
        for (terrain, slope, accepted) in [
            (14, 0, true),  // Rough: retail LAT Icy and Grey/Clear.
            (13, 0, true),  // Clear.
            (11, 0, true),  // Pavement.
            (1, 0, true),   // Ice.
            (7, 0, false),  // Rock.
            (9, 0, false),  // Water.
            (10, 0, false), // Wet shore.
            (14, 1, false), // Ramp.
        ] {
            std::sync::Arc::get_mut(&mut scene.palette)
                .unwrap()
                .templates
                .insert(
                    (id, 0),
                    TerrainTemplate {
                        width: 1,
                        height: 1,
                        cells: vec![Some(TerrainSubtile {
                            height: 0,
                            slope,
                            terrain,
                        })],
                    },
                );
            for kind in [BridgeKind::LowWood, BridgeKind::LowConcrete] {
                let a = rotate([69, 67]);
                let b = rotate([83, 67]);
                let result = plan(&doc, &scene, a, b, kind);
                assert_eq!(
                    result.is_ok(),
                    accepted,
                    "terrain={terrain}, slope={slope}: {:?}",
                    result.as_ref().err()
                );
                if let Ok(p) = result {
                    assert!(p.edits.is_empty());
                    assert_eq!(p.overlays, plan(&doc, &scene, b, a, kind).unwrap().overlays);
                    p.apply(&mut doc).unwrap();
                    doc.commit("bridge on LAT banks");
                    doc.undo();
                }
            }
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
        }
    }
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
            let heads: Vec<_> = p
                .cliffs
                .iter()
                .filter(|s| bridge_head(&scene, s.tile))
                .collect();
            assert_eq!(heads.len(), 2);
            assert!(
                p.cliffs.len() > heads.len(),
                "this crossing splits a wide cliff"
            );
            let axis = usize::from(!along_x);
            // The road overhangs the ravine, but the two outer shoulder rows
            // must keep the existing cliff profile. Aligning the road's raised
            // centre instead shifts both cliff joins inward by one cell (#6).
            for head in &heads {
                let t = &scene.palette.templates[&(head.tile, 0)];
                let across = if along_x { t.height } else { t.width };
                for (sub, s) in t.cells.iter().enumerate() {
                    if s.is_none() {
                        continue;
                    }
                    let at = [
                        head.origin[0] + sub as u16 % t.width,
                        head.origin[1] + sub as u16 / t.width,
                    ];
                    let side = at[1 - axis] - head.origin[1 - axis];
                    if side == 0 || side == across - 1 {
                        assert_eq!(
                            p.edits[&at].level,
                            value(&doc, &scene, at).level,
                            "{kind:?}: bridge shoulder must follow the cliff at {at:?}"
                        );
                    }
                }
            }
            let distance = heads[1].origin[axis] - heads[0].origin[axis];
            assert_eq!(p.overlays.len(), usize::from(distance - 2));
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
            for s in p.cliffs.iter().filter(|s| !bridge_head(&scene, s.tile)) {
                for (at, v) in stamp(&doc, &scene, s.tile, s.origin, s.base).unwrap() {
                    assert_eq!(p.edits[&at], v);
                    assert_eq!(v.level, value(&doc, &scene, at).level);
                }
            }
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
                if bridge_head(&scene, scene.palette.raw_ids[&v.tile]) {
                    assert_eq!(
                        scratch.terrain(at),
                        Some(v),
                        "expansion must preserve bridgeheads"
                    );
                }
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
    assert!(plan(&doc, &scene, [60, 60], [61, 60], BridgeKind::LowWood).is_err());
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

#[test]
fn high_bridge_endpoints_snap_from_bank_interiors_and_cliff_feet_on_both_axes() {
    for axis in 0..2 {
        let (mut doc, scene) = setup();
        let rotate = |[x, y]: Cell| if axis == 0 { [x, y] } else { [y, x] };
        for (a, b) in [([60, 60], [79, 74]), ([94, 60], [108, 74])] {
            cliff_tools::plan(
                &doc,
                &scene,
                cliff_tools::rectangle(rotate(a), rotate(b)),
                4,
            )
            .unwrap()
            .apply(&mut doc)
            .unwrap();
            doc.commit("bank");
        }
        let before = doc.snapshot().bytes().unwrap();
        for kind in [BridgeKind::HighConcrete, BridgeKind::HighWood] {
            let expected = plan(&doc, &scene, rotate([80, 67]), rotate([94, 67]), kind).unwrap();
            for (a, b) in [([77, 67], [98, 67]), ([81, 67], [93, 67])] {
                let p = plan(&doc, &scene, rotate(a), rotate(b), kind).unwrap();
                assert_eq!(p.edits, expected.edits);
                assert_eq!(p.overlays, expected.overlays);
                let reverse = plan(&doc, &scene, rotate(b), rotate(a), kind).unwrap();
                assert_eq!(p.edits, reverse.edits);
                assert_eq!(p.overlays, reverse.overlays);
            }
        }
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
    }
}

#[test]
fn low_bridge_accepts_real_shore_subcells_and_snaps_ends_without_modifying_water() {
    for axis in 0..2 {
        let (mut doc, scene) = setup();
        let rotate = |[x, y]: Cell| if axis == 0 { [x, y] } else { [y, x] };
        crate::shore_tools::plan(
            &doc,
            &scene,
            cliff_tools::rectangle(rotate([70, 55]), rotate([82, 80])),
            Some(true),
        )
        .unwrap()
        .apply(&mut doc)
        .unwrap();
        doc.commit("river");
        let before = doc.snapshot().bytes().unwrap();
        for kind in [BridgeKind::LowWood, BridgeKind::LowConcrete] {
            let p = plan(&doc, &scene, rotate([70, 67]), rotate([82, 67]), kind).unwrap();
            assert!(p.edits.is_empty());
            let reverse = plan(&doc, &scene, rotate([82, 67]), rotate([70, 67]), kind).unwrap();
            assert_eq!(p.overlays, reverse.overlays);
            p.apply(&mut doc).unwrap();
            doc.commit("bridge");
            doc.undo();
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
        }
        assert!(
            plan(
                &doc,
                &scene,
                rotate([76, 67]),
                rotate([82, 67]),
                BridgeKind::LowWood
            )
            .is_err(),
            "an endpoint deep in open water must still refuse"
        );
    }
}

#[test]
fn low_deck_variation_uses_absolute_coordinates_and_short_bridges_are_supported() {
    let (doc, scene) = setup();
    for kind in [BridgeKind::LowWood, BridgeKind::LowConcrete] {
        let first = if kind == BridgeKind::LowWood { 74 } else { 205 };
        for (a, b, axis) in [([61, 60], [69, 60], 0), ([60, 61], [60, 69], 1)] {
            let p = plan(&doc, &scene, a, b, kind).unwrap();
            for (&at, &(id, _)) in &p.overlays {
                if at[axis] > a[axis] && at[axis] < b[axis] {
                    assert_eq!(
                        id,
                        first + if axis == 0 { 0 } else { 9 } + (at[axis] % 4) as u8
                    );
                }
            }
        }
        assert_eq!(
            plan(&doc, &scene, [60, 60], [62, 60], kind)
                .unwrap()
                .overlays
                .len(),
            9
        );
    }
}

#[test]
fn snapping_does_not_bypass_blockers_bad_heights_or_missing_bridge_resources() {
    let (mut doc, mut scene) = setup();
    for (a, b) in [([60, 60], [79, 74]), ([94, 60], [108, 74])] {
        cliff_tools::plan(&doc, &scene, cliff_tools::rectangle(a, b), 4)
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        doc.commit("bank");
    }
    doc.set(Target::Overlay([86, 67]), Some(Value::Overlay(1, 0)))
        .unwrap();
    doc.commit("block");
    let before = doc.snapshot().bytes().unwrap();
    assert!(plan(&doc, &scene, [77, 67], [98, 67], BridgeKind::HighConcrete).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.undo();
    let old = value(&doc, &scene, [86, 67]);
    doc.set(
        Target::Terrain([86, 67]),
        Some(Value::Terrain(ra2_map_document::document::TerrainValue {
            level: 5,
            ..old
        })),
    )
    .unwrap();
    doc.commit("bump");
    assert!(plan(&doc, &scene, [77, 67], [98, 67], BridgeKind::HighConcrete).is_err());
    doc.undo();
    std::sync::Arc::make_mut(&mut scene.catalog).overlays[24] = "INVALID".into();
    assert!(plan(&doc, &scene, [77, 67], [98, 67], BridgeKind::HighConcrete).is_err());
}

#[test]
fn high_bridges_fit_short_ravines_only_when_aligned_heads_have_room() {
    for axis in 0..2 {
        let rotate = |[x, y]: Cell| if axis == 0 { [x, y] } else { [y, x] };
        for span in 3..8 {
            let (mut doc, scene) = setup();
            for (a, b) in [([60, 60], [79, 74]), ([80 + span, 60], [98 + span, 74])] {
                cliff_tools::plan(
                    &doc,
                    &scene,
                    cliff_tools::rectangle(rotate(a), rotate(b)),
                    4,
                )
                .unwrap()
                .apply(&mut doc)
                .unwrap();
                doc.commit("bank");
            }
            let before = doc.snapshot().bytes().unwrap();
            for kind in [BridgeKind::HighConcrete, BridgeKind::HighWood] {
                let result = plan(
                    &doc,
                    &scene,
                    rotate([77, 67]),
                    rotate([84 + span, 67]),
                    kind,
                );
                assert_eq!(doc.snapshot().bytes().unwrap(), before);
                if span < 5 {
                    assert!(result.err().unwrap().to_string().contains("桥梁跨度过短"));
                    continue;
                }
                let p = result.unwrap();
                assert_eq!(
                    p.cliffs
                        .iter()
                        .filter(|s| bridge_head(&scene, s.tile))
                        .count(),
                    2
                );
                assert!(!p.overlays.is_empty());
            }
        }
    }
}

#[test]
fn paved_approaches_are_valid_and_remain_intact() {
    let (mut doc, scene) = setup();
    let ids = scene.palette.tile_ids.unwrap();
    // A complete straight road at the landing, perpendicular to its cross row.
    let road = stamp(&doc, &scene, ids.paved_roads as u16, [59, 59], 4).unwrap();
    for (&at, &v) in &road {
        doc.set(Target::Terrain(at), Some(Value::Terrain(v)))
            .unwrap();
    }
    doc.commit("road approach");
    let p = plan(&doc, &scene, [60, 60], [72, 60], BridgeKind::LowConcrete).unwrap();
    p.apply(&mut doc).unwrap();
    for (at, v) in road {
        assert_eq!(doc.terrain(at), Some(v));
    }
}

fn crossing_setup(axis: usize) -> (MapDocument, MapScene) {
    let (mut doc, scene) = setup();
    let rotate = |[x, y]: Cell| if axis == 0 { [x, y] } else { [y, x] };
    crate::shore_tools::plan(
        &doc,
        &scene,
        cliff_tools::rectangle(rotate([85, 57]), rotate([89, 77])),
        Some(true),
    )
    .unwrap()
    .apply(&mut doc)
    .unwrap();
    doc.commit("water under crossing");
    for (a, b) in [([60, 60], [79, 74]), ([94, 60], [108, 74])] {
        cliff_tools::plan(
            &doc,
            &scene,
            cliff_tools::rectangle(rotate(a), rotate(b)),
            4,
        )
        .unwrap()
        .apply(&mut doc)
        .unwrap();
        doc.commit("bank");
    }
    (doc, scene)
}

#[test]
fn double_bridges_replace_only_the_low_crossing_in_either_order_and_survive_history() {
    for axis in 0..2 {
        let (doc, scene) = crossing_setup(axis);
        let rotate = |[x, y]: Cell| if axis == 0 { [x, y] } else { [y, x] };
        let high_ends = (rotate([77, 67]), rotate([98, 67]));
        let low_ends = (rotate([86, 55]), rotate([86, 79]));
        let crossing = cliff_tools::rectangle(rotate([85, 67]), rotate([87, 67]));
        let clear = scene.catalog.raw_tiles[&(scene.palette.tile_ids.unwrap().clear as u16)];
        for high in [BridgeKind::HighConcrete, BridgeKind::HighWood] {
            let hp = plan(&doc, &scene, high_ends.0, high_ends.1, high).unwrap();
            for low in [BridgeKind::LowWood, BridgeKind::LowConcrete] {
                let lp = plan(&doc, &scene, low_ends.0, low_ends.1, low).unwrap();
                let mut results = Vec::new();
                for high_first in [false, true] {
                    let mut edited = doc.fork();
                    (if high_first { &hp } else { &lp })
                        .apply(&mut edited)
                        .unwrap();
                    edited.commit("first bridge");
                    let before = edited.snapshot().bytes().unwrap();
                    let (a, b, kind) = if high_first {
                        (low_ends.0, low_ends.1, low)
                    } else {
                        (high_ends.0, high_ends.1, high)
                    };
                    let p = plan(&edited, &scene, a, b, kind).unwrap();
                    let reverse = plan(&edited, &scene, b, a, kind).unwrap();
                    assert_eq!(p.edits, reverse.edits);
                    assert_eq!(p.overlays, reverse.overlays);
                    assert_eq!(edited.snapshot().bytes().unwrap(), before);
                    let changed: BTreeSet<_> = p
                        .edits
                        .keys()
                        .filter(|at| !hp.edits.contains_key(*at))
                        .copied()
                        .collect();
                    assert_eq!(changed, crossing);
                    p.apply(&mut edited).unwrap();
                    edited.commit("crossing bridge");
                    for (&at, &v) in &hp.edits {
                        assert_eq!(edited.terrain(at), Some(v), "keep complete heads");
                    }
                    for (&at, &v) in &hp.overlays {
                        assert_eq!(edited.overlay(at), v, "keep high anchors");
                    }
                    for (&at, &v) in &lp.overlays {
                        if crossing.contains(&at) {
                            assert_eq!(
                                edited.terrain(at),
                                Some(TerrainValue {
                                    tile: clear,
                                    sub: 0,
                                    level: 4,
                                    ice: 0
                                })
                            );
                            assert_eq!(
                                edited.overlay(at),
                                hp.overlays.get(&at).copied().unwrap_or((255, 0))
                            );
                        } else {
                            assert_eq!(edited.overlay(at), v, "keep low bridge outside crossing");
                            assert_eq!(edited.terrain(at), doc.terrain(at));
                        }
                    }
                    let after = edited.snapshot().bytes().unwrap();
                    // Repainting either layer must keep the narrow crossing,
                    // including the existing high anchor in the low span.
                    for (start, end, repaint) in [
                        (high_ends.0, high_ends.1, high),
                        (low_ends.0, low_ends.1, low),
                    ] {
                        plan(&edited, &scene, start, end, repaint)
                            .unwrap()
                            .apply(&mut edited)
                            .unwrap();
                        assert_eq!(edited.snapshot().bytes().unwrap(), after);
                    }
                    let (source, _) = ra2_map_document::source::SourceMap::parse(
                        "crossing.map".into(),
                        "test".into(),
                        after.clone(),
                    )
                    .unwrap();
                    let reopened = MapDocument::new(source.edit_base.as_ref().unwrap().clone());
                    for &at in &crossing {
                        assert_eq!(reopened.terrain(at), edited.terrain(at));
                        assert_eq!(reopened.overlay(at), edited.overlay(at));
                    }
                    edited.undo();
                    assert_eq!(edited.snapshot().bytes().unwrap(), before);
                    edited.redo();
                    assert_eq!(edited.snapshot().bytes().unwrap(), after);
                    results.push(after);
                }
                assert_eq!(
                    results[0], results[1],
                    "placement order must not change the result"
                );
            }
        }
    }
}

#[test]
fn double_bridge_crossings_still_reject_blockers_and_modified_overlay_ids() {
    let (mut doc, mut scene) = crossing_setup(0);
    plan(&doc, &scene, [86, 55], [86, 79], BridgeKind::LowWood)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    doc.commit("low bridge");
    assert!(plan(&doc, &scene, [86, 55], [86, 79], BridgeKind::LowConcrete).is_ok());
    let id = doc.overlay([86, 65]).0;
    let name = scene.catalog.overlays[usize::from(id)].clone();
    std::sync::Arc::make_mut(&mut scene.catalog).overlays[usize::from(id)] = "MOD_OVERLAY".into();
    assert!(plan(&doc, &scene, [77, 67], [98, 67], BridgeKind::HighWood).is_err());
    std::sync::Arc::make_mut(&mut scene.catalog).overlays[usize::from(id)] = name;
    doc.set(Target::Overlay([86, 65]), Some(Value::Overlay(1, 0)))
        .unwrap();
    let before = doc.snapshot().bytes().unwrap();
    assert!(plan(&doc, &scene, [77, 67], [98, 67], BridgeKind::HighWood).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.cancel();
    doc.undo();
    plan(&doc, &scene, [77, 67], [98, 67], BridgeKind::HighWood)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    doc.commit("high bridge");
    assert!(plan(&doc, &scene, [77, 67], [98, 67], BridgeKind::HighConcrete).is_ok());
    // This cell has no high anchor, but is still in its structural row.
    doc.set(Target::Overlay([86, 65]), Some(Value::Overlay(1, 0)))
        .unwrap();
    let before = doc.snapshot().bytes().unwrap();
    assert!(plan(&doc, &scene, [86, 55], [86, 79], BridgeKind::LowWood).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn existing_bridges_can_be_repainted_with_either_material_on_both_axes() {
    for axis in 0..2 {
        let (doc, scene) = crossing_setup(axis);
        let rotate = |[x, y]: Cell| if axis == 0 { [x, y] } else { [y, x] };
        for (a, b, kinds) in [
            (
                [77, 67],
                [98, 67],
                [BridgeKind::HighWood, BridgeKind::HighConcrete],
            ),
            (
                [86, 55],
                [86, 79],
                [BridgeKind::LowWood, BridgeKind::LowConcrete],
            ),
        ] {
            let (a, b) = (rotate(a), rotate(b));
            for old in kinds {
                for new in kinds {
                    let mut edited = doc.fork();
                    plan(&edited, &scene, a, b, old)
                        .unwrap()
                        .apply(&mut edited)
                        .unwrap();
                    edited.commit("old bridge");
                    let before = edited.snapshot().bytes().unwrap();
                    let expected = plan(&doc, &scene, a, b, new).unwrap();
                    let mut endpoints = vec![(a, b), (b, a)];
                    if new.high() {
                        // Pick both the road centre and the low foot of each
                        // saved head, rather than only untouched bank cells.
                        for offset in [[1, 2], [0, 0]] {
                            let offset = if axis == 0 {
                                offset
                            } else {
                                [offset[1], offset[0]]
                            };
                            let heads: Vec<_> = expected
                                .cliffs
                                .iter()
                                .filter(|head| bridge_head(&scene, head.tile))
                                .map(|head| cell(head.origin, offset[0], offset[1]).unwrap())
                                .collect();
                            endpoints.extend([(heads[0], heads[1]), (heads[1], heads[0])]);
                        }
                    }
                    for (start, end) in endpoints {
                        let p = plan(&edited, &scene, start, end, new).unwrap();
                        let expected_changes: BTreeMap<_, _> = expected
                            .edits
                            .iter()
                            .filter(|&(&at, &v)| value(&edited, &scene, at) != v)
                            .map(|(&at, &v)| (at, v))
                            .collect();
                        let actual_changes: BTreeMap<_, _> = p
                            .edits
                            .iter()
                            .filter(|&(&at, &v)| value(&edited, &scene, at) != v)
                            .map(|(&at, &v)| (at, v))
                            .collect();
                        assert_eq!(actual_changes, expected_changes);
                        assert_eq!(p.overlays, expected.overlays);
                    }
                    let p = plan(&edited, &scene, a, b, new).unwrap();
                    p.apply(&mut edited).unwrap();
                    edited.commit("replace bridge");
                    for (&at, &v) in &expected.edits {
                        assert_eq!(edited.terrain(at), Some(v));
                    }
                    for (&at, &v) in &expected.overlays {
                        assert_eq!(edited.overlay(at), v);
                    }
                    if old != new {
                        let after = edited.snapshot().bytes().unwrap();
                        edited.undo();
                        assert_eq!(edited.snapshot().bytes().unwrap(), before);
                        edited.redo();
                        assert_eq!(edited.snapshot().bytes().unwrap(), after);
                    }
                }
            }
        }
    }
}

#[test]
fn partial_low_bridge_overwrite_keeps_old_cells_outside_the_new_span() {
    for axis in 0..2 {
        let (mut doc, scene) = setup();
        let rotate = |[x, y]: Cell| if axis == 0 { [x, y] } else { [y, x] };
        let old = plan(
            &doc,
            &scene,
            rotate([60, 60]),
            rotate([72, 60]),
            BridgeKind::LowWood,
        )
        .unwrap();
        old.apply(&mut doc).unwrap();
        doc.commit("old bridge");
        let before = doc.snapshot().bytes().unwrap();
        let p = plan(
            &doc,
            &scene,
            rotate([65, 60]),
            rotate([78, 60]),
            BridgeKind::LowConcrete,
        )
        .unwrap();
        assert!(p.edits.is_empty());
        p.apply(&mut doc).unwrap();
        doc.commit("partial overwrite");
        for (&at, &v) in &old.overlays {
            assert_eq!(doc.overlay(at), p.overlays.get(&at).copied().unwrap_or(v));
        }
        for (&at, &v) in &p.overlays {
            assert_eq!(doc.overlay(at), v);
        }
        let after = doc.snapshot().bytes().unwrap();
        doc.undo();
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        doc.redo();
        assert_eq!(doc.snapshot().bytes().unwrap(), after);
    }
}
