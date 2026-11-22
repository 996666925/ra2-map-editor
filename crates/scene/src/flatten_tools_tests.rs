use super::*;
use ra2_map_assets::{catalog::Catalog, test_support::Fixture};
use ra2_map_document::{
    document::{Target, TerrainValue, Value},
    new_map::NewMapOptions,
};

fn setup() -> (MapDocument, MapScene) {
    let f = Fixture::with_authoring();
    let mut i = f.install();
    i.catalog = Catalog::load(&i.manager, &i.theater);
    let scene = crate::map::load_scene_bytes(
        &mut i,
        "flatten.map",
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
        MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone()),
        scene,
    )
}
fn wall(doc: &mut MapDocument, scene: &MapScene) {
    let b = scene
        .brushes
        .iter()
        .find(|b| {
            scene.palette.groups.get(&b.key.tile_id).is_some_and(|g| {
                g.category == ra2_map_assets::terrain_library::TerrainCategory::Cliff
            })
        })
        .unwrap();
    for y in 40..=52 {
        doc.set(
            Target::Terrain([46, y]),
            Some(Value::Terrain(TerrainValue {
                tile: b.raw,
                sub: b.key.sub_tile,
                level: 4,
                ice: 0,
            })),
        )
        .unwrap();
    }
}
#[test]
fn steep_vertex_and_force_modes_change_the_requested_surface_and_keep_occupants() {
    let (mut doc, scene) = setup();
    let selected = BTreeSet::from([[45, 45]]);
    let normal = plan(&doc, &scene, &selected, 8, true).unwrap();
    let steep = plan_with_steep(&doc, &scene, &selected, 8, true, true).unwrap();
    assert!(steep.edits.len() < normal.edits.len());
    assert!(steep.edits.values().any(|v| {
        let tile = scene.palette.raw_ids[&v.tile];
        scene.palette.templates[&(tile, 0)].cells[v.sub as usize]
            .is_some_and(|s| (13..=16).contains(&s.slope))
    }));
    let vertices = plan_with_options(
        &doc,
        &scene,
        &selected,
        8,
        true,
        Options {
            vertices: true,
            ..Default::default()
        },
    )
    .unwrap();
    vertices.apply(&mut doc).unwrap();
    assert_eq!(
        vertex_height(&doc, &scene, [45, 45], false).unwrap(),
        Some(8)
    );
    assert!(
        value(&doc, &scene, [45, 45]).level < 8,
        "one vertex is raised rather than flattening the whole cell"
    );
    doc.cancel();
    wall(&mut doc, &scene);
    doc.add_record("Units", [45, 46], "Neutral", "GI").unwrap();
    let before = value(&doc, &scene, [46, 46]);
    let seed = BTreeSet::from([[46, 46], [45, 46]]);
    let force = plan_with_options(
        &doc,
        &scene,
        &seed,
        8,
        true,
        Options {
            force: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(force.edits.contains_key(&[46, 46]));
    assert_ne!(force.edits[&[46, 46]].tile, before.tile);
    assert!(
        !force.edits.contains_key(&[45, 46]),
        "force morphing still protects occupants"
    );
    assert!(
        window_with_force(&doc, &scene, [46, 46], 3, 8, true)
            .unwrap()
            .contains(&[46, 46])
    );
    assert!(
        !window(&doc, &scene, [46, 46], 3, 8)
            .unwrap()
            .contains(&[46, 46])
    );
}

#[test]
fn local_window_does_not_use_a_route_around_the_cliff_outside_the_brush() {
    let (mut doc, scene) = setup();
    wall(&mut doc, &scene);
    let left = window(&doc, &scene, [45, 46], 5, 8).unwrap();
    assert_eq!(left.len(), 15);
    assert!(left.iter().all(|at| at[0] <= 45));
    let right = window(&doc, &scene, [47, 46], 5, 8).unwrap();
    assert!(right.iter().all(|at| at[0] >= 47));
    assert!(
        !right.is_empty(),
        "actively moving the cursor to the far side can paint that side"
    );
}
#[test]
fn flatten_window_reaches_different_heights_but_skips_overlays_and_foundations() {
    let (mut doc, mut scene) = setup();
    let old = value(&doc, &scene, [45, 46]);
    doc.set(
        Target::Terrain([45, 46]),
        Some(Value::Terrain(TerrainValue { level: 8, ..old })),
    )
    .unwrap();
    doc.set(Target::Overlay([46, 45]), Some(Value::Overlay(1, 0)))
        .unwrap();
    std::sync::Arc::make_mut(&mut scene.catalog).art =
        std::sync::Arc::new(vera20k_core::rules::art_data::ArtRegistry::from_ini(
            &vera20k_core::rules::ini_parser::IniFile::from_str("[BUILDING]\nFoundation=3x2\n"),
        ));
    doc.add_record("Structures", [47, 46], "Neutral", "BUILDING")
        .unwrap();
    let cells = window(&doc, &scene, [46, 46], 5, 4).unwrap();
    assert!(
        cells.contains(&[45, 46]),
        "flattening does not stop at a height difference"
    );
    assert!(!cells.contains(&[46, 45]));
    assert!(
        !cells.contains(&[48, 47]),
        "foundation cells beyond the building origin are protected"
    );
}

#[test]
fn smooth_flatten_does_not_leak_across_diagonally_touching_cliff_cells() {
    let (mut doc, scene) = setup();
    let b = scene
        .brushes
        .iter()
        .find(|b| {
            scene.palette.groups.get(&b.key.tile_id).is_some_and(|g| {
                g.category == ra2_map_assets::terrain_library::TerrainCategory::Cliff
            })
        })
        .unwrap();
    for x in 35..=55 {
        doc.set(
            Target::Terrain([x, x]),
            Some(Value::Terrain(TerrainValue {
                tile: b.raw,
                sub: b.key.sub_tile,
                level: 4,
                ice: 0,
            })),
        )
        .unwrap();
    }
    let selected = window(&doc, &scene, [45, 46], 3, 8).unwrap();
    let p = plan(&doc, &scene, &selected, 8, true).unwrap();
    assert_eq!(p.edits[&[45, 46]].level, 8);
    assert!(
        p.edits.keys().all(|at| at[0] < at[1]),
        "a closed corner cannot join the two ground surfaces"
    );
    assert!(!p.edits.contains_key(&[46, 45]));
}

#[test]
fn smooth_flatten_protects_the_other_bank_objects_and_ice_during_slope_propagation() {
    let (mut doc, scene) = setup();
    wall(&mut doc, &scene);
    doc.add_record("Units", [44, 45], "Neutral", "GI").unwrap();
    let old = value(&doc, &scene, [44, 46]);
    doc.set(
        Target::Terrain([44, 46]),
        Some(Value::Terrain(TerrainValue { ice: 17, ..old })),
    )
    .unwrap();
    let selected = window(&doc, &scene, [45, 46], 5, 8).unwrap();
    let p = plan(&doc, &scene, &selected, 8, true).unwrap();
    assert_eq!(p.edits[&[44, 46]].level, 8);
    assert_eq!(p.edits[&[44, 46]].ice, 17);
    assert!(!p.edits.contains_key(&[44, 45]));
    assert!(p.edits.keys().all(|at| at[0] <= 45));
    doc.commit("wall and occupants");
    let before = doc.snapshot().bytes().unwrap();
    p.apply(&mut doc).unwrap();
    doc.commit("smooth flatten");
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn smooth_flatten_can_lower_ground_and_raw_flatten_keeps_extended_height_support() {
    let (mut doc, scene) = setup();
    for x in 40..=52 {
        for y in 40..=52 {
            let old = value(&doc, &scene, [x, y]);
            doc.set(
                Target::Terrain([x, y]),
                Some(Value::Terrain(TerrainValue { level: 8, ..old })),
            )
            .unwrap();
        }
    }
    let selected = window(&doc, &scene, [46, 46], 3, 4).unwrap();
    let p = plan(&doc, &scene, &selected, 4, true).unwrap();
    assert_eq!(p.edits[&[46, 46]].level, 4);
    assert!(p.edits.values().any(|v| v.level > 4 && v.level < 8));
    assert!(plan(&doc, &scene, &selected, 127, true).is_err());
    let p = plan(&doc, &scene, &selected, 127, false).unwrap();
    assert!(p.edits.values().all(|v| v.level == 127));
}

#[test]
fn smooth_lowering_beside_high_bridges_uses_the_ordinary_terrain_plan() {
    for (transpose, id, frame) in [(false, 24, 0), (true, 25, 9)] {
        let (mut doc, scene) = setup();
        let orient = |[x, y]: Cell| if transpose { [y, x] } else { [x, y] };
        let mut ordinary = doc.fork();
        for x in 34..=46 {
            let target = Target::Overlay(orient([x, 40]));
            ordinary
                .set(target.clone(), Some(Value::Overlay(1, 0)))
                .unwrap();
            doc.set(target, Some(Value::Overlay(id, frame))).unwrap();
        }
        let selected = (41..=45).map(|y| orient([40, y])).collect();
        let expected = plan(&ordinary, &scene, &selected, 0, true).unwrap();
        let actual = plan(&doc, &scene, &selected, 0, true).unwrap();
        assert_eq!(actual.edits, expected.edits);
        assert_eq!(actual.edits[&orient([40, 41])].level, 0);
        assert!(actual.overlays.is_empty());
    }
}
