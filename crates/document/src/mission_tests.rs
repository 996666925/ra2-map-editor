use super::*;
use crate::document::{Baseline, Target, Value};
use std::sync::Arc;
use vera20k_core::{
    map::map_file,
    rules::{ini_parser::IniFile, team_ai_ini::TeamAiIniRegistry},
};
fn parse(bytes: Vec<u8>) -> MapDocument {
    let map = map_file::load_from_bytes(bytes.clone()).unwrap();
    MapDocument::new(Baseline::parse(Arc::from(bytes), &map).unwrap())
}
#[test]
fn physical_ini_edits_preserve_duplicates_legacy_bytes_and_share_spatial_history() {
    let mut bytes = crate::test_support::fixture();
    bytes.extend_from_slice(b"\r\n[Triggers]\r\nT=Neutral,<none>,\xD6\xD0,0,1,1,1,0 ; untouched\r\nT=shadow\r\n[Custom]\r\n  X = old ; keep\r\nX=shadow\r\n");
    let mut doc = parse(bytes.clone());
    assert!(
        doc.set_ini("Custom", "X", Some(b"ambiguous".to_vec()))
            .is_err()
    );
    let rows = doc.ini_entries("Custom");
    doc.set(rows[1].0.clone(), Some(Value::Ini(b"physical".to_vec())))
        .unwrap();
    doc.set_ini("Basic", "Author", Some(b"Author".to_vec()))
        .unwrap();
    doc.waypoint(30, [2, 2]).unwrap();
    doc.commit("mixed");
    let edited = doc.snapshot().bytes().unwrap();
    assert!(
        edited
            .windows(b"T=Neutral,<none>,\xD6\xD0,0,1,1,1,0 ; untouched".len())
            .any(|w| w == b"T=Neutral,<none>,\xD6\xD0,0,1,1,1,0 ; untouched")
    );
    assert!(
        edited
            .windows(b"  X = old ; keep\r\nX=physical".len())
            .any(|w| w == b"  X = old ; keep\r\nX=physical")
    );
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), bytes);
    assert!(doc.redo());
    assert_eq!(doc.snapshot().bytes().unwrap(), edited);
    doc.set_ini("Custom", "X", None).unwrap();
    doc.commit("delete shadows");
    let reopened = parse(doc.snapshot().bytes().unwrap());
    assert!(reopened.ini_value("Custom", "X").is_none());
    assert!(
        doc.set(
            Target::IniNew("Map".into(), "Size".into()),
            Some(Value::Ini(b"bad".to_vec()))
        )
        .is_err()
    );
    assert!(
        doc.set_ini("Triggers", "BAD", Some(b"line\n[Other]".to_vec()))
            .is_err()
    );
}
#[test]
fn minimum_mission_roundtrips_through_engine_readers_and_undoes_as_one_command() {
    let bytes = crate::new_map::NewMapOptions::default().bytes().unwrap();
    let mut doc = parse(bytes.clone());
    let start = seed(&mut doc).unwrap();
    doc.commit("mission");
    assert!(issues(&doc).is_empty(), "{:?}", issues(&doc));
    let saved = doc.snapshot().bytes().unwrap();
    let map = map_file::load_from_bytes(saved.clone()).unwrap();
    assert_eq!(map.triggers.len(), 2);
    assert_eq!(map.tags.len(), 2);
    assert_eq!(map.events[&start].conditions()[0].kind, 13);
    assert_eq!(
        map.actions[&start]
            .entries()
            .iter()
            .map(|a| a.kind)
            .collect::<Vec<_>>(),
        [7, 56]
    );
    let teams = TeamAiIniRegistry::from_sources(&IniFile::empty(), &map.ini, false);
    assert!(teams.diagnostics.is_empty(), "{:?}", teams.diagnostics);
    assert_eq!(teams.team_types.len(), 1);
    assert_eq!(teams.scripts.len(), 1);
    assert_eq!(teams.task_forces[0].entries[0].member_type, "E1");
    assert_eq!(
        parse(saved.clone()).ini_text("Basic", "Player"),
        "Americans"
    );
    doc.undo();
    assert_eq!(doc.snapshot().bytes().unwrap(), bytes);
    doc.redo();
    assert_eq!(doc.snapshot().bytes().unwrap(), saved);
    assert!(delete(&mut doc, Kind::Trigger, &start).is_err());
    assert!(delete(&mut doc, Kind::House, "Americans").is_err());
    let tag = ids(&doc, Kind::Tag)
        .into_iter()
        .find(|id| doc.ini_text("Tags", id).ends_with(&start))
        .unwrap();
    delete(&mut doc, Kind::Tag, &tag).unwrap();
    delete(&mut doc, Kind::Trigger, &start).unwrap();
    assert!(doc.ini_value("Events", &start).is_none());
}
#[test]
fn failed_mission_command_rolls_back_and_unknown_event_fields_remain_opaque() {
    let mut doc = crate::test_support::document();
    let before = doc.snapshot().bytes().unwrap();
    assert!(create(&mut doc, Kind::Team, "TEAM", "Neutral", None).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    assert!(decode_list("1,999,2,0,ExtensionType,extra", true).is_err());
    let rows = decode_list("1,60,2,0,HTNK", true).unwrap();
    assert_eq!(encode_list(&rows), "1,60,2,0,HTNK");
}

#[test]
fn sample_allocates_independent_variables_and_house_deletion_checks_object_owners() {
    let mut doc = parse(crate::new_map::NewMapOptions::default().bytes().unwrap());
    create(&mut doc, Kind::Variable, "0", "Americans", None).unwrap();
    seed(&mut doc).unwrap();
    assert_eq!(doc.ini_text("VariableNames", "0"), "Flag0,0");
    assert!(ids(&doc, Kind::Variable).contains(&"1".into()));
    let start = ids(&doc, Kind::Trigger)[0].clone();
    assert_eq!(
        decode_list(&doc.ini_text("Actions", &start), false).unwrap()[1][2],
        "1"
    );
    create(&mut doc, Kind::House, "ObjectOwner", "Americans", None).unwrap();
    doc.add_record("Units", [80, 80], "ObjectOwner", "HTNK")
        .unwrap();
    assert!(delete(&mut doc, Kind::House, "ObjectOwner").is_err());
}
#[test]
fn cell_tags_follow_map_format_reject_collisions_and_undo_together() {
    for format in [3, 4] {
        let bytes = crate::new_map::NewMapOptions::default().bytes().unwrap();
        let bytes = String::from_utf8(bytes)
            .unwrap()
            .replace("NewINIFormat=4", &format!("NewINIFormat={format}"));
        let mut doc = parse(bytes.into_bytes());
        seed(&mut doc).unwrap();
        doc.commit("seed");
        let before = doc.snapshot().bytes().unwrap();
        let key = cell_key(&doc, [80, 80]).unwrap();
        assert_eq!(
            key,
            (80 * if format < 4 { 128 } else { 1000 } + 80).to_string()
        );
        create(
            &mut doc,
            Kind::CellTag,
            "ignored",
            "Americans",
            Some([80, 80]),
        )
        .unwrap();
        assert!(
            create(
                &mut doc,
                Kind::CellTag,
                "another",
                "Americans",
                Some([80, 80])
            )
            .is_err()
        );
        doc.commit("cell tag");
        let reopened = parse(doc.snapshot().bytes().unwrap());
        assert_eq!(cell_at(&reopened, &key), Some([80, 80]));
        assert!(
            references(&reopened)
                .iter()
                .any(|r| r.from == (Kind::CellTag, key.clone()) && r.cell == Some([80, 80]))
        );
        assert!(cell_key(&doc, [511, 511]).is_err());
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
    }
}

#[test]
fn fixed_references_use_map_precedence_and_distinguish_missing_from_unavailable() {
    let mut doc = parse(crate::new_map::NewMapOptions::default().bytes().unwrap());
    let trigger = seed(&mut doc).unwrap();
    put(
        &mut doc,
        "Actions",
        &trigger,
        encode_list(&[action(4, "FIXEDTEAM")]),
    )
    .unwrap();
    let mut external = ExternalDefinitions {
        source: "aimd.ini".into(),
        ..Default::default()
    };
    external.teams.insert("FIXEDTEAM".into());
    let unresolved = diagnostics(&doc, None);
    assert!(
        unresolved
            .iter()
            .any(|d| d.field == "Actions" && d.severity == Severity::Warning)
    );
    assert!(diagnostics(&doc, Some(&external)).is_empty());
    external.teams.clear();
    assert!(
        diagnostics(&doc, Some(&external))
            .iter()
            .any(|d| d.field == "Actions" && d.severity == Severity::Error)
    );
    external.error = Some("resource unavailable".into());
    let unavailable = diagnostics(&doc, Some(&external));
    assert!(unavailable.iter().all(|d| d.severity == Severity::Warning));
    assert_eq!(unavailable.len(), 2); // One source warning and the unresolved reference.
    external.error = None;
    external.teams.insert("FIXEDTEAM".into());
    let local = ids(&doc, Kind::Team)[0].clone();
    external.teams.insert(local.clone());
    assert_eq!(
        external.candidates(&doc, Kind::Team),
        vec![(local, false), ("FIXEDTEAM".into(), true)]
    );
    assert!(external.contains(Kind::Team, "fixedteam"));
}

#[test]
fn native_case_insensitive_definitions_and_nonzero_flags_are_not_rejected() {
    let mut doc = parse(crate::new_map::NewMapOptions::default().bytes().unwrap());
    let trigger = seed(&mut doc).unwrap();
    let team = ids(&doc, Kind::Team)[0].clone();
    put(
        &mut doc,
        "Actions",
        &trigger,
        encode_list(&[action(4, &team.to_lowercase())]),
    )
    .unwrap();
    put(
        &mut doc,
        "Triggers",
        &trigger,
        "Americans,<none>,Short Header",
    )
    .unwrap();
    put(&mut doc, "VariableNames", "0", "Flag,2").unwrap();
    let result = diagnostics(&doc, None);
    assert!(
        result.iter().all(|d| d.severity != Severity::Error),
        "{result:?}"
    );
    assert!(
        result
            .iter()
            .any(|d| d.field == "Triggers" && d.severity == Severity::Warning)
    );
    assert!(
        delete(&mut doc, Kind::Team, &team).is_err(),
        "lowercase references must protect their definition"
    );
    let mut external = ExternalDefinitions::default();
    external.teams.insert(team.to_lowercase());
    assert_eq!(external.candidates(&doc, Kind::Team), vec![(team, false)]);
}

#[test]
fn diagnostics_locate_typed_errors_without_modifying_unknown_mission_bytes() {
    let mut doc = parse(crate::new_map::NewMapOptions::default().bytes().unwrap());
    let trigger = seed(&mut doc).unwrap();
    put(
        &mut doc,
        "Events",
        &trigger,
        "2,36,0,100,999,2,0,CustomType",
    )
    .unwrap();
    let team = ids(&doc, Kind::Team)[0].clone();
    let mut reinforcement = schema::row(false, 80).unwrap();
    reinforcement[2] = team;
    reinforcement[7] = "ZZ".into();
    put(&mut doc, "Actions", &trigger, encode_list(&[reinforcement])).unwrap();
    put(&mut doc, "Americans", "PlayerControl", "perhaps").unwrap();
    put(&mut doc, "Americans", "Credits", "2147483647").unwrap();
    // No hard-coded stock IQ/tech ceiling: Mods and runtime comparisons admit other values.
    put(&mut doc, "Americans", "IQ", "-2").unwrap();
    put(&mut doc, "Americans", "TechLevel", "100").unwrap();
    doc.commit("diagnostic fixture");
    let bytes = doc.snapshot().bytes().unwrap();
    let result = diagnostics(&doc, None);
    for field in ["Events", "Actions"] {
        assert!(
            result
                .iter()
                .any(|d| d.field == field && d.severity == Severity::Error),
            "{result:?}"
        );
    }
    assert!(
        result
            .iter()
            .any(|d| d.field == "Events" && d.severity == Severity::Capability)
    );
    assert!(
        result
            .iter()
            .any(|d| d.field == "PlayerControl" && d.severity == Severity::Warning)
    );
    assert!(
        result
            .iter()
            .any(|d| d.field == "Credits" && d.severity == Severity::Warning)
    );
    assert!(
        !result
            .iter()
            .any(|d| matches!(d.field.as_str(), "IQ" | "TechLevel"))
    );
    assert_eq!(doc.snapshot().bytes().unwrap(), bytes);
    let reopened = parse(bytes);
    assert_eq!(
        reopened.ini_text("Events", &trigger),
        "2,36,0,100,999,2,0,CustomType"
    );
}
