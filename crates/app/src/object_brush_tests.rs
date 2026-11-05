use super::super::tests::{editor, terrain_scene};
use super::*;
use ra2_map_assets::{catalog::Catalog, test_support::Fixture};
use ra2_map_document::{new_map::NewMapOptions, source::SourceMap};
use vera20k_core::rules::{art_data::ArtRegistry, ini_parser::IniFile};

fn scene() -> MapScene {
    let mut scene = terrain_scene();
    let catalog = Arc::make_mut(&mut scene.catalog);
    for (section, types) in [
        ("Units", vec!["HTNK"]),
        ("Infantry", vec!["E1"]),
        ("Structures", vec!["BUILDING"]),
        ("Terrain", vec!["TREE01", "TREE02"]),
        ("Smudge", vec!["CRATER"]),
        ("Aircraft", vec!["ORCA"]),
    ] {
        catalog.object_types.insert(
            section.into(),
            types.into_iter().map(str::to_owned).collect(),
        );
    }
    catalog.art = Arc::new(ArtRegistry::from_ini(&IniFile::from_str(
        "[BUILDING]\nFoundation=3x2\n",
    )));
    scene
}
fn start(e: &mut Editor, section: &str, type_id: &str) {
    e.tool = Tool::Place;
    e.section = section.into();
    e.type_id = type_id.into();
    e.owner = "Neutral".into();
}

#[test]
fn shift_object_brush_locks_both_axes_and_preserves_undo_and_cancel() {
    let (mut e, _, root) = editor();
    let scene = scene();
    e.attach(&scene);
    start(&mut e, "Units", "HTNK");
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.brush_modifiers(egui::Modifiers::SHIFT);
    e.pointer(&scene, Some([20, 20]), true, true, false, None);
    e.pointer(&scene, Some([24, 21]), false, true, false, None);
    e.pointer(&scene, Some([21, 27]), false, true, false, None);
    assert_eq!(e.feedback, BTreeSet::from([[20, 20], [21, 20]]));
    assert_eq!(
        e.records.values().filter(|r| r.section == "Units").count(),
        2
    );
    e.pointer(&scene, Some([26, 25]), false, false, true, None);
    let cells: BTreeSet<_> = e
        .records
        .values()
        .filter(|r| r.section == "Units")
        .filter_map(Record::cell)
        .collect();
    assert_eq!(cells, (20..=26).map(|x| [x, 20]).collect());
    let after = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.history(true, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);

    e.pointer(&scene, Some([20, 22]), true, true, false, None);
    e.pointer(&scene, Some([21, 26]), false, true, false, None);
    e.pointer(&scene, Some([26, 28]), false, true, false, None);
    assert_eq!(e.feedback, (22..=28).map(|y| [20, y]).collect());
    e.pointer(&scene, Some([26, 24]), false, true, false, None);
    assert_eq!(e.feedback, (22..=24).map(|y| [20, y]).collect());
    e.cancel_operation(&scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    // Cancellation must not retain the preceding stroke's lock.
    e.pointer(&scene, Some([22, 22]), true, true, false, None);
    e.pointer(&scene, Some([26, 23]), false, true, false, None);
    e.pointer(&scene, Some([24, 23]), false, false, true, None);
    assert_eq!(e.feedback, (22..=24).map(|x| [x, 22]).collect());
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn shift_building_line_shrinks_without_losing_prefix_or_existing_objects() {
    let (mut e, _, root) = editor();
    let scene = scene();
    e.attach(&scene);
    let existing = e
        .doc
        .as_mut()
        .unwrap()
        .add_record("Units", [26, 20], "Neutral", "HTNK")
        .unwrap();
    e.commit(&scene, "existing occupant");
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    start(&mut e, "Structures", "BUILDING");
    // Begin freely, then keep the prefix when the constrained part retracts.
    e.pointer(&scene, Some([20, 20]), true, true, false, None);
    e.pointer(&scene, Some([23, 20]), false, true, false, None);
    e.brush_modifiers(egui::Modifiers::SHIFT);
    e.pointer(&scene, Some([32, 21]), false, true, false, None);
    e.pointer(&scene, Some([24, 22]), false, true, false, None);
    let anchors = |e: &Editor| -> BTreeSet<_> {
        e.records
            .values()
            .filter(|r| r.section == "Structures")
            .filter_map(Record::cell)
            .collect()
    };
    assert_eq!(anchors(&e), BTreeSet::from([[20, 20], [23, 20]]));
    assert!(e.records.contains_key(&existing));
    assert_eq!(e.object_brush.gesture.as_ref().unwrap().skipped, 0);
    // Releasing Shift fixes this shortened segment before choosing a new one,
    // including when there is no pointer sample between modifier changes.
    e.brush_modifiers(egui::Modifiers::NONE);
    e.brush_modifiers(egui::Modifiers::SHIFT);
    e.pointer(&scene, Some([25, 26]), false, true, false, None);
    assert_eq!(
        anchors(&e),
        BTreeSet::from([[20, 20], [23, 20], [24, 22], [24, 24], [24, 26]])
    );
    e.pointer(&scene, Some([25, 22]), false, false, true, None);
    assert_eq!(anchors(&e), BTreeSet::from([[20, 20], [23, 20], [24, 22]]));
    let after = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.history(true, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    start(&mut e, "Units", "HTNK");
    let records = e.records.clone();
    let marker_ids: BTreeSet<_> = e.marker_cache.markers.iter().map(|m| m.id).collect();
    let state = e.doc.as_ref().unwrap().state_id;
    e.pointer(&scene, Some([26, 20]), true, true, false, None);
    e.pointer(&scene, Some([30, 21]), false, true, false, None);
    assert!(e.records.len() > records.len());
    // Retracting all the way to an occupied origin must remove every ghost
    // even if release has no remaining edits and therefore adds no undo step.
    e.pointer(&scene, Some([26, 21]), false, false, true, None);
    assert_eq!(e.records, records);
    assert_eq!(
        e.marker_cache
            .markers
            .iter()
            .map(|m| m.id)
            .collect::<BTreeSet<_>>(),
        marker_ids
    );
    assert_eq!(e.doc.as_ref().unwrap().state_id, state);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn object_drag_interpolates_deduplicates_freezes_settings_and_undoes_once() {
    let (mut e, _, root) = editor();
    let scene = scene();
    e.attach(&scene);
    start(&mut e, "Units", "HTNK");
    e.radius = 5; // Terrain brush size never multiplies objects.
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.pointer(&scene, Some([20, 20]), true, true, false, None);
    e.pointer(&scene, Some([24, 20]), false, true, false, None);
    e.pointer(&scene, Some([20, 20]), false, true, false, None);
    assert_eq!(
        e.records.values().filter(|r| r.section == "Units").count(),
        5
    );
    assert!(e.stroking);
    assert!(e.doc.as_ref().unwrap().undo_name().is_none());
    e.tool = Tool::Erase;
    e.section = "Infantry".into();
    e.type_id = "E1".into();
    e.owner = "GDI".into();
    e.pointer(&scene, Some([26, 20]), false, false, true, None);
    assert!(!e.stroking);
    let records: Vec<_> = e
        .records
        .values()
        .filter(|r| r.section == "Units")
        .collect();
    assert_eq!(records.len(), 7);
    for x in 20..=26 {
        assert!(records.iter().any(|r| r.cell() == Some([x, 20])));
    }
    assert!(
        records
            .iter()
            .all(|r| r.type_id() == "HTNK" && r.fields()[0] == "Neutral")
    );
    let after = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    let (source, _) = SourceMap::parse("objects.map".into(), "test".into(), after.clone()).unwrap();
    assert_eq!(
        MapDocument::new(source.edit_base.unwrap())
            .records()
            .values()
            .filter(|r| r.section == "Units")
            .count(),
        7
    );
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.history(true, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn object_drag_skips_blocked_and_uneven_foundations_without_losing_valid_placements() {
    let (mut e, _, root) = editor();
    let scene = scene();
    e.attach(&scene);
    let doc = e.doc.as_mut().unwrap();
    doc.add_record("Units", [26, 20], "Neutral", "HTNK")
        .unwrap();
    let old = doc.terrain([28, 21]).unwrap();
    doc.set(
        Target::Terrain([28, 21]),
        Some(Value::Terrain(crate::document::TerrainValue {
            level: 1,
            ..old
        })),
    )
    .unwrap();
    e.commit(&scene, "obstacles");
    start(&mut e, "Structures", "BUILDING");
    e.pointer(&scene, Some([20, 20]), true, true, false, None);
    e.pointer(&scene, Some([29, 20]), false, false, true, None);
    let anchors: BTreeSet<_> = e
        .records
        .values()
        .filter(|r| r.section == "Structures")
        .filter_map(Record::cell)
        .collect();
    assert_eq!(anchors, BTreeSet::from([[20, 20], [23, 20], [29, 20]]));
    assert!(e.status.contains("跳过"));
    assert_eq!(
        e.records.values().filter(|r| r.section == "Units").count(),
        1
    );
    // Explicit overlap permits pre-existing occupants, but a drag still never
    // stacks its own building foundations on top of one another.
    e.allow_overlap = true;
    e.pointer(&scene, Some([20, 24]), true, true, false, None);
    e.pointer(&scene, Some([24, 24]), false, false, true, None);
    let anchors: BTreeSet<_> = e
        .records
        .values()
        .filter(|r| r.section == "Structures" && r.cell().unwrap()[1] == 24)
        .filter_map(Record::cell)
        .collect();
    assert_eq!(anchors, BTreeSet::from([[20, 24], [23, 24]]));
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn object_drag_cancel_preserves_previous_history_and_reentry_does_not_bridge_the_gap() {
    let (mut e, ctx, root) = editor();
    let scene = scene();
    e.attach(&scene);
    e.doc
        .as_mut()
        .unwrap()
        .add_record("Units", [18, 20], "Neutral", "HTNK")
        .unwrap();
    e.commit(&scene, "previous");
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    start(&mut e, "Infantry", "E1");
    e.infantry_subcell = 3;
    e.pointer(&scene, Some([20, 20]), true, true, false, None);
    e.pointer(&scene, Some([22, 20]), false, true, false, None);
    let input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
        ..Default::default()
    };
    let _ = crate::test_support::run_ui(&ctx, input, |ui| e.shortcuts(ui.ctx(), &scene));
    e.pointer(&scene, None, false, false, true, None);
    assert!(!e.stroking);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    assert_eq!(e.doc.as_ref().unwrap().undo_name(), Some("previous"));
    e.pointer(&scene, Some([20, 20]), true, true, false, None);
    e.pointer(&scene, None, false, true, false, None);
    e.infantry_subcell = 1;
    e.pointer(&scene, Some([25, 20]), false, true, false, None);
    e.pointer(&scene, None, false, false, true, None);
    let infantry: Vec<_> = e
        .records
        .values()
        .filter(|r| r.section == "Infantry")
        .collect();
    assert_eq!(infantry.len(), 2);
    assert!(infantry.iter().all(|r| r.fields()[5] == "3"));
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn terrain_objects_are_never_replaced_and_single_click_and_noop_keep_history_correct() {
    let (mut e, _, root) = editor();
    let scene = scene();
    e.attach(&scene);
    let tree = e
        .doc
        .as_mut()
        .unwrap()
        .add_record("Terrain", [20, 20], "Neutral", "TREE01")
        .unwrap();
    e.commit(&scene, "existing tree");
    start(&mut e, "Terrain", "TREE02");
    e.allow_overlap = true;
    let state = e.doc.as_ref().unwrap().state_id;
    e.pointer(&scene, Some([20, 20]), true, false, true, None);
    assert_eq!(e.doc.as_ref().unwrap().state_id, state);
    e.pointer(&scene, Some([20, 20]), true, true, false, None);
    e.pointer(&scene, Some([22, 20]), false, false, true, None);
    assert_eq!(e.records[&tree].type_id(), "TREE01");
    assert_eq!(
        e.records
            .values()
            .filter(|r| r.section == "Terrain")
            .count(),
        3
    );
    start(&mut e, "Smudge", "CRATER");
    e.pointer(&scene, Some([25, 20]), true, false, true, None);
    assert_eq!(
        e.records.values().filter(|r| r.section == "Smudge").count(),
        1
    );
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn pending_object_sprites_are_drawn_during_the_drag_and_removed_on_cancel() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.0.join("rulesmd.ini"),
        b"[VehicleTypes]\n0=GI\n[GI]\nImage=GI\n",
    )
    .unwrap();
    let mut install = fixture.install();
    install.catalog = Catalog::load(&install.manager, &install.theater);
    let scene = crate::map::load_scene_bytes(
        &mut install,
        "brush.map",
        "test".into(),
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
    let (mut e, ctx, root) = editor();
    e.attach(&scene);
    start(&mut e, "Units", "GI");
    e.pointer(&scene, Some([20, 20]), true, true, false, None);
    e.pointer(&scene, Some([22, 20]), false, true, false, None);
    let draw = |e: &mut Editor, at: Option<Cell>| {
        crate::test_support::run_ui(&ctx, Default::default(), |ui| {
            e.region_preview(
                ui.painter(),
                &scene,
                ui.max_rect(),
                &crate::camera::MapCamera {
                    center: scene.bounds.center(),
                    zoom: 1.,
                },
                at,
            );
        })
    };
    let output = draw(&mut e, Some([22, 20]));
    let count = output.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Mesh(m) if m.vertices.iter().all(|v| v.color == Color32::from_white_alpha(160)))).count();
    assert_eq!(
        count, 3,
        "all pending sprites, without an occupied-cell cursor ghost"
    );
    e.cancel_operation(&scene);
    let output = draw(&mut e, None);
    assert!(!output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Mesh(m) if m.vertices.iter().all(|v| v.color == Color32::from_white_alpha(160)))));
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}
