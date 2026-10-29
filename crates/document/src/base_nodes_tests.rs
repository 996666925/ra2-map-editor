use super::*;
use crate::{document::Baseline, new_map::NewMapOptions};
use vera20k_core::{
    map::{houses::parse_house_roster, map_file},
    rules::{ini_parser::IniFile, ruleset::RuleSet},
};

fn document() -> MapDocument {
    let bytes = NewMapOptions::default().bytes().unwrap();
    let map = map_file::load_from_bytes(bytes.clone()).unwrap();
    let mut doc = MapDocument::new(Baseline::parse(bytes.into(), &map).unwrap());
    mission::create(&mut doc, Kind::House, "AIHOUSE", "Americans", None).unwrap();
    doc.commit("house");
    doc
}
#[test]
fn ordered_plan_roundtrips_in_engine_and_history_preserves_controls_and_legacy_tail() {
    let mut doc = document();
    let before = doc.snapshot().bytes().unwrap();
    doc.atomic(|doc| {
        append(doc, "AIHOUSE", "GAPOWR", [80, 80])?;
        let mut nodes = read(doc, "AIHOUSE")?;
        nodes.push(Node::parse(b"-1,0,0,\xD6\xD0".to_vec())?);
        write(doc, "AIHOUSE", &nodes)?;
        append(doc, "AIHOUSE", "GACNST", [83, 80])?;
        reorder(doc, "AIHOUSE", 2, 0)?;
        move_node(doc, "AIHOUSE", 1, [81, 80])
    })
    .unwrap();
    doc.commit("ordered plan");
    let saved = doc.snapshot().bytes().unwrap();
    let map = map_file::load_from_bytes(saved.clone()).unwrap();
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=GAPOWR\n1=GACNST\n[GAPOWR]\nStrength=750\n[GACNST]\nStrength=1000\n",
    ))
    .unwrap();
    let roster = parse_house_roster(&map.ini, &[], Some(&rules));
    let house = roster.houses.iter().find(|h| h.name == "AIHOUSE").unwrap();
    assert_eq!(
        house
            .base_plan
            .nodes
            .iter()
            .map(|n| n.type_or_control)
            .collect::<Vec<_>>(),
        vec![1, 0, -1]
    );
    assert_eq!(house.base_plan.nodes[1].packed_cell, 81 | (80 << 16));
    assert_eq!(read(&doc, "AIHOUSE").unwrap()[2].raw, b"-1,0,0,\xD6\xD0");
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    assert!(doc.redo());
    assert_eq!(doc.snapshot().bytes().unwrap(), saved);
    remove(&mut doc, "AIHOUSE", 0).unwrap();
    doc.commit("delete");
    assert!(doc.ini_value("AIHOUSE", "002").is_none());
    assert_eq!(read(&doc, "AIHOUSE").unwrap().len(), 2);
}
#[test]
fn structure_import_and_rebuild_are_independent_reversible_and_unambiguous() {
    let mut doc = document();
    let building = doc
        .add_record("Structures", [80, 80], "AIHOUSE", "GAPOWR")
        .unwrap();
    doc.commit("building");
    let before = doc.snapshot().bytes().unwrap();
    doc.atomic(|doc| {
        assert_eq!(import_structures(doc, "AIHOUSE")?, 1);
        assert_eq!(import_structures(doc, "AIHOUSE")?, 0);
        set_rebuild(doc, "AIHOUSE", 0, true)
    })
    .unwrap();
    doc.commit("import and rebuild");
    assert_eq!(doc.records()[&building].fields()[8], "1");
    assert_eq!(doc.records()[&building].cell(), Some([80, 80]));
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.redo();
    move_node(&mut doc, "AIHOUSE", 0, [82, 80]).unwrap();
    assert!(set_rebuild(&mut doc, "AIHOUSE", 0, false).is_err());
}
#[test]
fn malformed_duplicate_orphan_and_failed_moves_never_partially_write() {
    let mut doc = document();
    append(&mut doc, "AIHOUSE", "GAPOWR", [80, 80]).unwrap();
    doc.set_ini("AIHOUSE", "001", Some(b"GACNST,81,80".to_vec()))
        .unwrap();
    doc.commit("orphan");
    let before = doc.snapshot().bytes().unwrap();
    assert!(append(&mut doc, "AIHOUSE", "GAPOWR", [82, 80]).is_err());
    assert!(move_node(&mut doc, "AIHOUSE", 0, [65535, 65535]).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    doc.set_ini("AIHOUSE", "NodeCount", Some(b"2".to_vec()))
        .unwrap();
    doc.set_ini("AIHOUSE", "001", Some(b"extension".to_vec()))
        .unwrap();
    doc.commit("unknown");
    let before = doc.snapshot().bytes().unwrap();
    assert!(remove(&mut doc, "AIHOUSE", 0).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    let mut bytes = document().snapshot().bytes().unwrap();
    bytes.extend_from_slice(
        b"\r\n[AIHOUSE]\r\nNodeCount=1\r\n000=GAPOWR,80,80\r\n000=GACNST,81,80\r\n",
    );
    let map = map_file::load_from_bytes(bytes.clone()).unwrap();
    let mut duplicate = MapDocument::new(Baseline::parse(bytes.into(), &map).unwrap());
    let before = duplicate.snapshot().bytes().unwrap();
    assert!(append(&mut duplicate, "AIHOUSE", "GAPOWR", [82, 80]).is_err());
    assert_eq!(duplicate.snapshot().bytes().unwrap(), before);
}
