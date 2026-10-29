use super::*;
use ra2_map_assets::{catalog::Catalog, test_support::Fixture};
use ra2_map_document::new_map::NewMapOptions;
use std::{sync::Arc, time::Instant};
use vera20k_core::rules::{art_data::ArtRegistry, ini_parser::IniFile};

fn setup() -> (MapDocument, MapScene) {
    let fixture = Fixture::new();
    let mut install = fixture.install();
    install.catalog = Catalog::load(&install.manager, &install.theater);
    let mut scene = crate::map::load_scene_bytes(
        &mut install,
        "place.map",
        "fixture".into(),
        NewMapOptions {
            width: 20,
            height: 20,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
        Instant::now(),
    )
    .unwrap();
    Arc::make_mut(&mut scene.catalog).art = Arc::new(ArtRegistry::from_ini(&IniFile::from_str(
        "[BUILDING]\nFoundation=3x2\n",
    )));
    let doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
    (doc, scene)
}
fn building(at: Cell) -> Record {
    Record {
        section: "Structures".into(),
        key: "0".into(),
        value: format!("Neutral,BUILDING,256,{},{},0,None,0,0,1", at[0], at[1]).into_bytes(),
        coordinate_radix: 1000,
    }
}
#[test]
fn foundation_checks_interior_occupants_and_explicit_overlap() {
    let (mut doc, scene) = setup();
    doc.add_record("Units", [22, 21], "Neutral", "HTNK")
        .unwrap();
    let r = building([20, 20]);
    assert_eq!(footprint(&scene, &r).unwrap().len(), 6);
    assert!(
        validate(
            &doc,
            &scene,
            std::slice::from_ref(&r),
            &BTreeSet::new(),
            false,
            &BTreeMap::new()
        )
        .is_err()
    );
    assert!(validate(&doc, &scene, &[r], &BTreeSet::new(), true, &BTreeMap::new()).is_ok());
    assert!(
        validate(
            &doc,
            &scene,
            &[building([20, 39])],
            &BTreeSet::new(),
            true,
            &BTreeMap::new()
        )
        .is_err(),
        "overlap override cannot bypass bounds"
    );
}
#[test]
fn infantry_subcells_coexist_but_same_subcell_is_occupied() {
    let (mut doc, scene) = setup();
    let id = doc
        .add_record("Infantry", [20, 20], "Neutral", "E1")
        .unwrap();
    let r = doc.records()[&id].clone();
    let different = r.patched(&[(5, "1".into())]).unwrap();
    assert!(
        validate(
            &doc,
            &scene,
            &[different],
            &BTreeSet::new(),
            false,
            &BTreeMap::new()
        )
        .is_ok()
    );
    assert!(
        validate(
            &doc,
            &scene,
            std::slice::from_ref(&r),
            &BTreeSet::new(),
            false,
            &BTreeMap::new()
        )
        .is_err()
    );
    assert!(
        validate(
            &doc,
            &scene,
            &[r],
            &BTreeSet::from([id]),
            false,
            &BTreeMap::new()
        )
        .is_ok(),
        "moving excludes original occupant"
    );
}
#[test]
fn pasted_terrain_heights_are_checked_instead_of_stale_destination_levels() {
    let (doc, scene) = setup();
    let r = building([20, 20]);
    let ground = BTreeMap::from([(
        [21, 20],
        TerrainValue {
            tile: 0,
            sub: 0,
            level: 1,
            ice: 0,
        },
    )]);
    assert!(
        validate(
            &doc,
            &scene,
            std::slice::from_ref(&r),
            &BTreeSet::new(),
            false,
            &ground
        )
        .is_err()
    );
    assert!(
        validate(
            &doc,
            &scene,
            &[r],
            &BTreeSet::new(),
            false,
            &BTreeMap::new()
        )
        .is_ok()
    );
}
#[test]
fn collision_checks_include_objects_within_the_same_proposed_batch() {
    let (doc, scene) = setup();
    let a = building([20, 20]);
    let b = building([22, 21]);
    assert!(
        validate(
            &doc,
            &scene,
            &[a, b],
            &BTreeSet::new(),
            false,
            &BTreeMap::new()
        )
        .is_err()
    );
}

#[test]
fn overlap_override_cannot_create_duplicate_terrain_coordinate_keys() {
    let (mut doc, scene) = setup();
    let id = doc
        .add_record("Terrain", [20, 20], "Neutral", "TREE01")
        .unwrap();
    let r = doc.records()[&id].clone();
    assert!(validate(&doc, &scene, &[r], &BTreeSet::new(), true, &BTreeMap::new()).is_err());
}
