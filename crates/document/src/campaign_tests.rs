use crate::{
    campaign::*,
    document::{Baseline, MapDocument},
    mission::{self, Kind},
    new_map::NewMapOptions,
};
use vera20k_core::{map::map_file, rules::ini_parser::IniFile};
#[test]
fn globals_are_implicit_annotation_history_and_references_do_not_initialize_runtime() {
    let bytes = NewMapOptions::default().bytes().unwrap();
    let map = map_file::load_from_bytes(bytes.clone()).unwrap();
    let mut d = MapDocument::new(Baseline::parse(bytes.into(), &map).unwrap());
    mission::seed(&mut d).unwrap();
    d.commit("seed");
    let before = d.snapshot().bytes().unwrap();
    assert_eq!(mission::ids(&d, Kind::Global).len(), 50);
    mission::write_fields(
        &mut d,
        Kind::Global,
        "49",
        &[("EditorGlobalNames".into(), "最后标志".as_bytes().to_vec())],
    )
    .unwrap();
    d.commit("label");
    assert_eq!(d.ini_text("EditorGlobalNames", "49"), "最后标志");
    assert!(mission::write_fields(&mut d, Kind::Global, "50", &[]).is_err());
    assert!(mission::delete(&mut d, Kind::Global, "49").is_err());
    assert!(mission::create(&mut d, Kind::Global, "50", "", None).is_err());
    assert!(d.undo());
    assert_eq!(d.snapshot().bytes().unwrap(), before);
    assert!(d.redo());
    let id = mission::ids(&d, Kind::Trigger)[0].clone();
    d.set_ini(
        "Actions",
        &id,
        Some(mission::encode_list(&[mission::action(28, "49")]).into_bytes()),
    )
    .unwrap();
    d.set_ini("Events", &id, Some(b"1,27,0,49".to_vec()))
        .unwrap();
    let script = mission::ids(&d, Kind::Script)[0].clone();
    d.set_ini(&script, "0", Some(b"12,49".to_vec())).unwrap();
    assert_eq!(
        mission::references(&d)
            .iter()
            .filter(|r| r.to == (Kind::Global, "49".into()))
            .count(),
        3
    );
    assert!(
        !mission::diagnostics(&d, None)
            .iter()
            .any(|d| d.issue.message.contains("全局变量须"))
    );
    assert_eq!(
        GLOBAL_COUNT,
        vera20k_core::sim::trigger_runtime::SCENARIO_GLOBAL_COUNT
    );
}
#[test]
fn branch_and_receiving_policy_use_engine_fields() {
    let b = vera20k_core::map::basic::parse_basic_section(&IniFile::from_str(
        "[Basic]\nNextScenario=A.MAP\nAltNextScenario=B.MAP\nSkipMapSelect=yes\nTimerInherit=yes\nCarryOverMoney=25%\nCarryOverCap=2000\nBrief=INTRO\n",
    ));
    assert_eq!(destination(&b, false), Some("A.MAP"));
    assert_eq!(destination(&b, true), Some("B.MAP"));
    assert_eq!(b.timer_inherit, Some(true));
    assert_eq!(b.carry_over_cap, Some(2000));
    assert_eq!(b.briefing.as_deref(), Some("INTRO"));
    let mut end = b;
    end.end_of_game = Some(true);
    assert_eq!(destination(&end, false), None);
}
