use super::*;
use crate::document::{Baseline, MapDocument};
use schema::scripts::{self, Edit};
fn parse(bytes: Vec<u8>) -> MapDocument {
    let map = vera20k_core::map::map_file::load_from_bytes(bytes.clone()).unwrap();
    MapDocument::new(Baseline::parse(bytes.into(), &map).unwrap())
}
fn fixture() -> MapDocument {
    let mut doc = parse(
        crate::new_map::NewMapOptions {
            width: 40,
            height: 40,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
    );
    seed(&mut doc).unwrap();
    doc.commit("seed");
    doc
}
#[test]
fn schema_matches_production_operand_decoders_and_reinforcement_waypoint_source() {
    let mut doc = fixture();
    let team = ids(&doc, Kind::Team)[0].clone();
    assert_eq!(doc.ini_text(&team, "Waypoint"), "B");
    for index in 0..702 {
        let token = schema::waypoint_token(index).unwrap();
        assert_eq!(schema::waypoint_index(&token, true), Some(index));
    }
    let trigger = ids(&doc, Kind::Trigger)[0].clone();
    let mut normal = action(7, &team);
    normal[7] = "ZZ".into();
    put(
        &mut doc,
        "Actions",
        &trigger,
        encode_list(&[normal.clone()]),
    )
    .unwrap();
    assert!(
        !diagnostics(&doc, None)
            .iter()
            .any(|d| d.field == "Actions" && d.severity == Severity::Error)
    );
    normal[0] = "80".into();
    put(&mut doc, "Actions", &trigger, encode_list(&[normal])).unwrap();
    assert!(
        diagnostics(&doc, None)
            .iter()
            .any(|d| d.field == "Actions" && d.severity == Severity::Error)
    );
    put(&mut doc, &team, "Waypoint", "1").unwrap();
    assert!(
        diagnostics(&doc, None)
            .iter()
            .any(|d| d.field == "Waypoint" && d.severity == Severity::Error)
    );
    for event in [true, false] {
        for spec in if event {
            schema::EVENTS
        } else {
            schema::ACTIONS
        } {
            let rows = vec![schema::row(event, spec.id).unwrap()];
            assert_eq!(decode_list(&encode_list(&rows), event).unwrap(), rows);
        }
    }
}
#[test]
fn script_slot_operations_remap_branches_and_reject_dangling_targets_atomically() {
    let mut fields = std::collections::BTreeMap::from([
        ("Name".into(), "script".into()),
        ("0".into(), "6,3".into()),
        ("1".into(), "999,777".into()),
        ("2".into(), "49,0".into()),
    ]);
    scripts::edit(&mut fields, true, Edit::Insert(1)).unwrap();
    assert_eq!(fields["0"], "6,4");
    scripts::edit(&mut fields, true, Edit::Move(3, 2)).unwrap();
    assert_eq!(fields["0"], "6,3");
    assert_eq!(fields["3"], "999,777");
    let before = fields.clone();
    assert!(scripts::edit(&mut fields, true, Edit::Remove(2)).is_err());
    assert_eq!(fields, before);
    scripts::edit(&mut fields, true, Edit::Copy(0)).unwrap();
    assert_eq!(fields["0"], "6,4");
    assert_eq!(fields["1"], "6,4");
    let mut sparse = std::collections::BTreeMap::from([("3".into(), "2,E1".into())]);
    let before = sparse.clone();
    assert!(scripts::edit(&mut sparse, false, Edit::Insert(0)).is_err());
    assert_eq!(sparse, before);
}
#[test]
fn graph_copy_shares_or_clones_cycles_and_keeps_existing_cell_bindings() {
    let mut doc = fixture();
    let start = ids(&doc, Kind::Trigger)[0].clone();
    let team = ids(&doc, Kind::Team)[0].clone();
    let tag = ids(&doc, Kind::Tag)
        .into_iter()
        .find(|t| csv(&doc.ini_text("Tags", t))[2] == start)
        .unwrap();
    put(&mut doc, &team, "Tag", &tag).unwrap();
    bind_cell(&mut doc, [40, 40], &tag).unwrap();
    doc.commit("cycle");
    let initial = doc.snapshot().bytes().unwrap();
    let shared = graph::copy(&mut doc, Kind::Trigger, &start, "SHARED", false).unwrap();
    assert_eq!(shared.len(), 2, "a trigger always needs its own cloned tag");
    assert!(
        references(&doc)
            .iter()
            .any(|r| r.from == (Kind::Trigger, "SHARED".into())
                && r.to == (Kind::Team, team.clone()))
    );
    doc.commit("copy shared");
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), initial);
    let cloned = graph::copy(&mut doc, Kind::Trigger, &start, "CLONED", true).unwrap();
    assert_eq!(
        cloned.len(),
        5,
        "trigger, tag, team, taskforce, script; cycle visited once"
    );
    let copied_team = cloned.iter().find(|r| r.0 == Kind::Team).unwrap();
    assert_ne!(copied_team.2, team);
    assert!(
        references(&doc)
            .iter()
            .any(|r| r.from == (Kind::Trigger, "CLONED".into())
                && r.to == (Kind::Team, copied_team.2.clone()))
    );
    assert_eq!(
        doc.ini_text("CellTags", &cell_key(&doc, [40, 40]).unwrap()),
        tag
    );
    doc.commit("copy graph");
    let bytes = doc.snapshot().bytes().unwrap();
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), initial);
    doc.redo();
    assert_eq!(doc.snapshot().bytes().unwrap(), bytes);
    assert_eq!(ids(&parse(bytes), Kind::Team).len(), 2);
}
#[test]
fn graph_rename_migrates_csv_and_object_edges_preserving_legacy_bytes_and_history() {
    let mut doc = fixture();
    let team = ids(&doc, Kind::Team)[0].clone();
    let start = ids(&doc, Kind::Trigger)[0].clone();
    let tag = ids(&doc, Kind::Tag)
        .into_iter()
        .find(|t| csv(&doc.ini_text("Tags", t))[2] == start)
        .unwrap();
    put(&mut doc, &team, "Tag", &tag).unwrap();
    bind_cell(&mut doc, [40, 40], &tag).unwrap();
    let record = doc
        .add_record("Infantry", [40, 40], "Russians", "E1")
        .unwrap();
    let mut r = doc.records()[&record]
        .patched(&[(8, tag.to_lowercase())])
        .unwrap();
    r.value.extend_from_slice(b",\xD6\xD0");
    doc.set(
        crate::document::Target::Record(record),
        Some(crate::document::Value::Record(r)),
    )
    .unwrap();
    doc.commit("bindings");
    let before = doc.snapshot().bytes().unwrap();
    graph::rename(&mut doc, Kind::Tag, &tag, "RENAMED_TAG").unwrap();
    doc.commit("rename");
    assert_eq!(doc.ini_text(&team, "Tag"), "RENAMED_TAG");
    assert_eq!(
        doc.ini_text("CellTags", &cell_key(&doc, [40, 40]).unwrap()),
        "RENAMED_TAG"
    );
    assert_eq!(doc.records()[&record].fields()[8], "RENAMED_TAG");
    assert!(doc.records()[&record].value.ends_with(b",\xD6\xD0"));
    graph::rename(&mut doc, Kind::Trigger, &start, "RENAMED_TRIGGER").unwrap();
    doc.commit("rename trigger");
    assert_eq!(
        csv(&doc.ini_text("Tags", "RENAMED_TAG"))[2],
        "RENAMED_TRIGGER"
    );
    let reopened = parse(doc.snapshot().bytes().unwrap());
    assert_eq!(reopened.ini_text("Events", "RENAMED_TRIGGER"), "1,13,0,2");
    doc.undo();
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    let snapshot = doc.snapshot().bytes().unwrap();
    assert!(graph::rename(&mut doc, Kind::Team, &team, &start.to_lowercase()).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), snapshot);
    assert!(graph::copy(&mut doc, Kind::Team, &team, "triggers", true).is_err());
}
#[test]
fn graph_refuses_ordinal_and_malformed_references_without_partial_edits() {
    let mut doc = fixture();
    let start = ids(&doc, Kind::Trigger)[0].clone();
    put(&mut doc, "Actions", &start, "1,7,1,0,0,0,0,0,A").unwrap();
    let before = doc.snapshot().bytes().unwrap();
    assert!(graph::copy(&mut doc, Kind::Trigger, &start, "COPY", true).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}
