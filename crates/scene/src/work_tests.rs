use super::*;
use ra2_map_assets::{catalog::Catalog, test_support::Fixture};
use ra2_map_document::{
    document::{Baseline, MapDocument, Value},
    mission::{self, Kind},
    new_map::NewMapOptions,
};
use std::time::Instant;

fn setup() -> (Fixture, Install, MapScene, MapDocument) {
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join("rulesmd.ini"),
        b"[VehicleTypes]\n0=GI\n[GI]\nName=Test unit\n[Countries]\n0=Americans\n[Americans]\nColor=Gold\n[Colors]\nGold=25,255,255\n").unwrap();
    let mut install = fixture.install();
    install.catalog = Catalog::load(&install.manager, &install.theater);
    let bytes = NewMapOptions {
        width: 20,
        height: 20,
        ..Default::default()
    }
    .bytes()
    .unwrap();
    let map = vera20k_core::map::map_file::load_from_bytes(bytes.clone()).unwrap();
    let mut doc = MapDocument::new(Baseline::parse(bytes.into(), &map).unwrap());
    doc.add_record("Units", [20, 20], "Neutral", "GI").unwrap();
    for (kind, id) in [
        (Kind::TaskForce, "TF"),
        (Kind::Script, "SCRIPT"),
        (Kind::Team, "TEAM"),
        (Kind::Trigger, "TRIGGER"),
    ] {
        mission::create(&mut doc, kind, id, "Neutral", None).unwrap();
    }
    doc.commit("fixture");
    let scene = crate::map::load_scene_bytes(
        &mut install,
        "test.map",
        "test".into(),
        doc.snapshot().bytes().unwrap(),
        Instant::now(),
    )
    .unwrap();
    assert!(!scene.visual.draws.is_empty());
    assert!(
        scene.visual.draws.iter().any(|draw| draw.owner.is_some()),
        "fixture must draw the placed unit"
    );
    let doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
    (fixture, install, scene, doc)
}

#[test]
fn mission_edits_reuse_visuals_and_match_full_projection_through_history() {
    let (_fixture, mut install, mut scene, mut doc) = setup();
    for (section, key, value) in [
        ("Events", "TRIGGER", "1,13,0,2"),
        ("Actions", "TRIGGER", "1,68,0,0,0,0,0,0,A"),
        ("Tags", "TAG", "0,tag,TRIGGER"),
        ("CellTags", "20020", "TAG"),
        ("VariableNames", "0", "flag,1"),
        ("EditorGlobalNames", "0", "global"),
        ("AITriggerTypesEnable", "AI", "yes"),
        ("SCRIPT", "0", "3,1"),
        ("TF", "Group", "1"),
        ("TEAM", "Priority", "8"),
        ("Basic", "Name", "renamed"),
    ] {
        let before = doc.snapshot();
        doc.set_ini(section, key, Some(value.as_bytes().to_vec()))
            .unwrap();
        doc.commit("mission field");
        let after = doc.snapshot();
        let updated = update_records(&scene, &before, &after, &install, Profile::Normal, &|| {
            false
        })
        .unwrap()
        .unwrap();
        assert!(
            Arc::ptr_eq(&updated.visual, &scene.visual),
            "{section}/{key}"
        );
        assert!(Arc::ptr_eq(
            updated.resolved.as_ref().unwrap(),
            scene.resolved.as_ref().unwrap()
        ));
        assert!(Arc::ptr_eq(&updated.images, &scene.images));
        assert!(Arc::ptr_eq(&updated.catalog, &scene.catalog));
        assert!(Arc::ptr_eq(&updated.sprites, &scene.sprites));
        assert_eq!(updated.source.bytes(), after.bytes().unwrap());
        let full = crate::map::load_scene_bytes(
            &mut install,
            "test.map",
            "full".into(),
            after.bytes().unwrap(),
            Instant::now(),
        )
        .unwrap();
        assert_eq!(updated.source.title, full.source.title);
        assert_eq!(
            updated.catalog.mission_type_orders,
            full.catalog.mission_type_orders
        );
        assert_eq!(
            updated
                .visual
                .raster(scene.bounds, [80, 80], Default::default())
                .unwrap(),
            full.visual
                .raster(scene.bounds, [80, 80], Default::default())
                .unwrap(),
            "{section}/{key}"
        );
        assert!(doc.undo());
        let restored = update_records(
            &updated,
            &after,
            &doc.snapshot(),
            &install,
            Profile::Normal,
            &|| false,
        )
        .unwrap()
        .unwrap();
        assert!(Arc::ptr_eq(&restored.visual, &scene.visual));
        assert_eq!(restored.source.bytes(), before.bytes().unwrap());
        assert!(doc.redo());
        scene = update_records(
            &restored,
            &before,
            &doc.snapshot(),
            &install,
            Profile::Normal,
            &|| false,
        )
        .unwrap()
        .unwrap();
        assert!(Arc::ptr_eq(&scene.visual, &updated.visual));
    }
}

#[test]
fn visual_unknown_and_mixed_changes_keep_the_full_refresh_path() {
    let (_fixture, install, scene, mut doc) = setup();
    let before = doc.snapshot();
    for (section, key, value) in [
        ("Lighting", "Ambient", "0.5"),
        ("Americans", "Color", "Blue"),
        ("GI", "Image", "OTHER"),
        ("VehicleTypes", "1", "OTHER"),
        ("Basic", "MultiplayerOnly", "no"),
        ("Basic", "Player", "Neutral"),
        ("Basic", "UnknownExtension", "1"),
        ("SCRIPT", "UnknownExtension", "1"),
        ("Extension", "0", "1"),
    ] {
        doc.set_ini("Events", "TRIGGER", Some(b"1,13,0,2".to_vec()))
            .unwrap();
        doc.set_ini(section, key, Some(value.as_bytes().to_vec()))
            .unwrap();
        assert!(
            update_records(
                &scene,
                &before,
                &doc.snapshot(),
                &install,
                Profile::Normal,
                &|| false
            )
            .unwrap()
            .is_none(),
            "{section}/{key}"
        );
        doc.cancel();
    }
    let terrain = doc.terrain([20, 20]).unwrap();
    doc.set(
        Target::Terrain([20, 20]),
        Some(Value::Terrain(ra2_map_document::document::TerrainValue {
            level: 1,
            ..terrain
        })),
    )
    .unwrap();
    assert!(
        update_records(
            &scene,
            &before,
            &doc.snapshot(),
            &install,
            Profile::Normal,
            &|| false
        )
        .unwrap()
        .is_none()
    );
    doc.cancel();
    doc.set(Target::Overlay([20, 20]), Some(Value::Overlay(1, 0)))
        .unwrap();
    assert!(
        update_records(
            &scene,
            &before,
            &doc.snapshot(),
            &install,
            Profile::Normal,
            &|| false
        )
        .unwrap()
        .is_none()
    );
    doc.cancel();
    assert!(
        update_records(
            &scene,
            &before,
            &doc.snapshot(),
            &install,
            Profile::Ion,
            &|| false
        )
        .unwrap()
        .is_none()
    );
    assert!(
        update_records(
            &scene,
            &before,
            &doc.snapshot(),
            &install,
            Profile::Normal,
            &|| true
        )
        .is_err()
    );
}

#[test]
fn resource_or_baseline_changes_disable_visual_reuse() {
    let (_fixture, mut install, scene, mut doc) = setup();
    let before = doc.snapshot();
    let mut revision = (*install.revision).clone();
    revision.resources.push_str("-changed");
    install.revision = Arc::new(revision);
    assert!(
        update_records(
            &scene,
            &before,
            &doc.snapshot(),
            &install,
            Profile::Normal,
            &|| false
        )
        .unwrap()
        .is_none()
    );
    install.revision = scene.revision.clone();
    doc.replace_bytes(
        NewMapOptions {
            width: 21,
            height: 20,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
        "resize",
    )
    .unwrap();
    assert!(
        update_records(
            &scene,
            &before,
            &doc.snapshot(),
            &install,
            Profile::Normal,
            &|| false
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn mission_and_object_edits_rebuild_objects_without_reusing_the_whole_visual() {
    let (_fixture, mut install, scene, mut doc) = setup();
    let before = doc.snapshot();
    let (id, record) = doc
        .records()
        .into_iter()
        .find(|(_, record)| record.section == "Units")
        .unwrap();
    doc.set(
        Target::Record(id),
        Some(Value::Record(record.moved([21, 20]).unwrap())),
    )
    .unwrap();
    doc.set_ini("Events", "TRIGGER", Some(b"1,13,0,2".to_vec()))
        .unwrap();
    doc.commit("mixed edit");
    let updated = update_records(
        &scene,
        &before,
        &doc.snapshot(),
        &install,
        Profile::Normal,
        &|| false,
    )
    .unwrap()
    .unwrap();
    assert!(!Arc::ptr_eq(&updated.visual, &scene.visual));
    assert!(Arc::ptr_eq(&updated.images, &scene.images));
    let full = crate::map::load_scene_bytes(
        &mut install,
        "test.map",
        "full".into(),
        doc.snapshot().bytes().unwrap(),
        Instant::now(),
    )
    .unwrap();
    assert_eq!(
        updated
            .visual
            .raster(scene.bounds, [80, 80], Default::default())
            .unwrap(),
        full.visual
            .raster(scene.bounds, [80, 80], Default::default())
            .unwrap()
    );
}
