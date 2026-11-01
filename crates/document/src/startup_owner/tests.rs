use super::*;
use crate::{document::Baseline, test_support::document};
use std::sync::Arc;
use vera20k_core::map::map_file;

#[test]
fn startup_owners_share_graph_roundtrip_change_clear_and_undo() {
    let mut doc = document();
    doc.waypoint(7, [2, 2]).unwrap();
    let mut ids = Vec::new();
    for (section, kind) in [
        ("Structures", "GAPOWR"),
        ("Units", "HTNK"),
        ("Infantry", "E1"),
        ("Aircraft", "ORCA"),
    ] {
        ids.push(doc.add_record(section, [2, 2], "Americans", kind).unwrap());
    }
    doc.commit("objects");
    let before = doc.snapshot().bytes().unwrap();
    for &id in &ids {
        set(&mut doc, id, Some(7)).unwrap();
    }
    doc.commit("startup owner");
    assert_eq!(mission::ids(&doc, Kind::Trigger).len(), 1);
    let tag = doc.records()[&ids[0]].fields()[6].clone();
    assert_eq!(doc.ini_text("Events", &tag), "1,13,0,0");
    assert_eq!(doc.ini_text("Actions", &tag), "1,14,0,4482,0,0,0,0,A");
    let bytes = doc.snapshot().bytes().unwrap();
    let map = map_file::load_from_bytes(bytes.clone()).unwrap();
    let reopened = MapDocument::new(Baseline::parse(Arc::from(bytes), &map).unwrap());
    assert_eq!(map.events[&tag].conditions()[0].kind, 13);
    assert_eq!(map.events[&tag].conditions()[0].value, 0);
    for &id in &ids {
        let original = &doc.records()[&id];
        let records = reopened.records();
        let r = records
            .values()
            .find(|r| r.section == original.section && r.key == original.key)
            .unwrap();
        assert_eq!(get(&reopened, r), Some(7));
        assert_eq!(r.fields()[0], "Neutral");
    }
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.redo();
    set(&mut doc, ids[0], Some(0)).unwrap();
    assert_eq!(get(&doc, &doc.records()[&ids[0]]), Some(0));
    assert_eq!(mission::ids(&doc, Kind::Trigger).len(), 2);
    set(&mut doc, ids[0], None).unwrap();
    assert_eq!(mission::ids(&doc, Kind::Trigger).len(), 1);
    for &id in &ids[1..] {
        set(&mut doc, id, None).unwrap();
    }
    assert!(mission::ids(&doc, Kind::Trigger).is_empty());
    assert!(mission::ids(&doc, Kind::Tag).is_empty());
}

#[test]
fn startup_owners_reject_missing_starts_custom_tags_and_modified_graphs_atomically() {
    let mut doc = document();
    let id = doc.add_record("Units", [2, 2], "Neutral", "HTNK").unwrap();
    let before = doc.snapshot().bytes().unwrap();
    for slot in [1, 8, 29] {
        assert!(set(&mut doc, id, Some(slot)).is_err());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
    }
    // Allocate around pre-existing, differently cased IDs, including orphan actions.
    doc.set_ini("Actions", "edstart0001", Some(b"custom".to_vec()))
        .unwrap();
    set(&mut doc, id, Some(0)).unwrap();
    let tag = doc.records()[&id].fields()[7].clone();
    assert_eq!(tag, "EDSTART0002");
    doc.set_ini("Events", &tag, Some(b"1,13,0,60".to_vec()))
        .unwrap();
    let before = doc.snapshot().bytes().unwrap();
    assert!(!available(&doc, &doc.records()[&id]));
    assert!(set(&mut doc, id, None).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}
