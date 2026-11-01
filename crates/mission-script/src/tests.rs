use super::{native::*, typescript::Compiler};
use ra2_map_document::{
    document::MapDocument,
    mission::{self, Kind},
    test_support,
};
use std::collections::BTreeMap;

fn fixture() -> (MapDocument, Catalog) {
    let doc = test_support::document();
    let catalog = Catalog::from_document(&doc, vec!["E1".into()], vec!["MISSION:TEXT".into()]);
    (doc, catalog)
}
fn plan() -> Mission {
    serde_json::from_value(serde_json::json!({
        "version":1, "id":"test", "player":"Neutral", "teams":[], "flags":[],
        "rules":[{"id":"win", "once":true, "when":{"kind":"scenarioTime", "seconds":10}, "actions":[{"kind":"winPlayer"}]}]
    })).unwrap()
}

#[test]
fn native_roundtrip_rebuild_and_history_preserve_manual_content() {
    let (mut doc, catalog) = fixture();
    let original = doc.snapshot().bytes().unwrap();
    let plan = plan();
    let build = prepare(&doc, &plan, &catalog).unwrap();
    assert!(build.apply(&mut doc).unwrap());
    let first = doc.snapshot().bytes().unwrap();
    assert_eq!(mission::ids(&doc, Kind::Trigger).len(), 1);
    let trigger = &mission::ids(&doc, Kind::Trigger)[0];
    assert_eq!(doc.ini_text("Events", trigger), "1,47,0,10");
    assert_eq!(doc.ini_text("Actions", trigger), "1,67,0,0,0,0,0,0,A");
    assert_eq!(doc.ini_text("Unknown", "A"), "first");
    assert!(
        !prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap()
    );
    assert_eq!(doc.snapshot().bytes().unwrap(), first);
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), original);
    assert!(doc.redo());
    assert_eq!(doc.snapshot().bytes().unwrap(), first);
    assert!(
        !prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap()
    );
}

#[test]
fn native_rejects_stale_results_and_modified_generated_records() {
    let (mut doc, catalog) = fixture();
    let plan = plan();
    let stale = prepare(&doc, &plan, &catalog).unwrap();
    doc.set_ini("Basic", "Name", Some(b"changed".to_vec()))
        .unwrap();
    doc.commit("edit");
    assert!(stale.apply(&mut doc).is_err());
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let trigger = mission::ids(&doc, Kind::Trigger)[0].clone();
    doc.set_ini("Events", &trigger, Some(b"1,47,0,100".to_vec()))
        .unwrap();
    doc.commit("manual edit");
    let before = doc.snapshot().bytes().unwrap();
    assert!(prepare(&doc, &plan, &catalog).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn deleting_rules_checks_incoming_manual_references() {
    let (mut doc, catalog) = fixture();
    let mut plan = plan();
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let tag = mission::ids(&doc, Kind::Tag)[0].clone();
    doc.set_ini("CellTags", "2002", Some(tag.into_bytes()))
        .unwrap();
    doc.commit("manual attachment");
    plan.rules.clear();
    assert!(prepare(&doc, &plan, &catalog).is_err());
    doc.set_ini("CellTags", "2002", None).unwrap();
    doc.commit("remove manual attachment");
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert!(mission::ids(&doc, Kind::Trigger).is_empty());
    assert_eq!(doc.ini_text("Unknown", "A"), "first");
}

#[test]
fn native_validation_rejects_forged_references_and_numeric_values() {
    let (doc, catalog) = fixture();
    let mut plan = plan();
    plan.rules[0].actions = vec![Action::Message {
        text: Reference {
            kind: "house".into(),
            id: "MISSION:TEXT".into(),
        },
    }];
    assert!(prepare(&doc, &plan, &catalog).is_err());
    plan.rules[0].actions = vec![Action::WinPlayer];
    plan.rules[0].when = Event::ScenarioTime { seconds: u32::MAX };
    assert!(prepare(&doc, &plan, &catalog).is_err());
    plan.rules[0].when = Event::Destroyed {
        object: Reference {
            kind: "object".into(),
            id: "Infantry:0".into(),
        },
    };
    assert!(
        prepare(&doc, &plan, &catalog).is_err(),
        "ambiguous duplicate objects must be rejected"
    );
}

#[test]
fn flags_keep_slots_and_do_not_clobber_manual_variables() {
    let (mut doc, catalog) = fixture();
    doc.set_ini("VariableNames", "0", Some(b"Manual,0".to_vec()))
        .unwrap();
    doc.commit("manual flag");
    let mut plan = plan();
    plan.flags.push(Flag {
        id: "complete".into(),
        initial: false,
    });
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert_eq!(doc.ini_text("VariableNames", "0"), "Manual,0");
    assert_eq!(doc.ini_text("VariableNames", "1"), "test_complete,0");
    assert!(
        !prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap()
    );
}

#[test]
fn teams_encode_native_waypoints_and_preserve_member_counts() {
    let (mut doc, mut catalog) = fixture();
    catalog.waypoints.push(26);
    doc.waypoint(26, [2, 2]).unwrap();
    doc.commit("waypoint");
    let mut plan = plan();
    plan.teams.push(Team {
        id: "assault".into(),
        house: TeamOwner::House {
            id: "Neutral".into(),
        },
        units: vec![Unit {
            r#type: Reference {
                kind: "unit".into(),
                id: "E1".into(),
            },
            count: 3,
        }],
        orders: vec![
            Order::MoveTo {
                waypoint: Waypoint {
                    kind: "waypoint".into(),
                    id: 26,
                },
            },
            Order::Guard { seconds: 5 },
        ],
        waypoint: Waypoint {
            kind: "waypoint".into(),
            id: 0,
        },
    });
    plan.rules[0].actions = vec![Action::Reinforce {
        team: Reference {
            kind: "team".into(),
            id: "assault".into(),
        },
        at: Waypoint {
            kind: "waypoint".into(),
            id: 26,
        },
    }];
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let team = &mission::ids(&doc, Kind::Team)[0];
    let script = doc.ini_text(team, "Script");
    let task = doc.ini_text(team, "TaskForce");
    assert_eq!(doc.ini_text(&script, "0"), "3,26");
    assert_eq!(doc.ini_text(&task, "0"), "3,E1");
    let trigger = &mission::ids(&doc, Kind::Trigger)[0];
    assert!(doc.ini_text("Actions", trigger).ends_with(",AA"));
    assert!(
        !prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap()
    );
}

fn gift_plan(slot: u32) -> Mission {
    serde_json::from_value(serde_json::json!({
        "version": 1, "id": "gift", "mode": "multiplayer",
        "teams": [{
            "id": "tanks", "house": {"kind": "startPlayer", "id": slot},
            "waypoint": {"kind": "waypoint", "id": 0},
            "units": [{"type": {"kind": "unit", "id": "E1"}, "count": 3}],
            "orders": [{"kind": "guard", "seconds": 60}]
        }], "flags": [],
        "rules": [{"id": "give", "once": true,
            "when": {"kind": "scenarioTime", "seconds": 10},
            "actions": [{"kind": "reinforce", "team": {"kind": "team", "id": "tanks"},
                         "at": {"kind": "waypoint", "id": 20}}]}]
    }))
    .unwrap()
}

#[test]
fn multiplayer_gifts_encode_all_native_start_selectors_and_preserve_history() {
    let (mut doc, _) = fixture();
    for slot in 0..8 {
        doc.waypoint(slot, [2, 2]).unwrap();
    }
    doc.waypoint(20, [2, 2]).unwrap();
    doc.set_ini("Basic", "Player", Some(b"Special".to_vec()))
        .unwrap();
    doc.commit("start positions");
    let catalog = Catalog::from_document(&doc, vec!["E1".into()], vec![]);
    for slot in 0..8 {
        let before = doc.snapshot().bytes().unwrap();
        let plan = gift_plan(slot);
        prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        let after = doc.snapshot().bytes().unwrap();
        let team = &mission::ids(&doc, Kind::Team)[0];
        assert_eq!(
            doc.ini_text(team, "House"),
            format!("<Player @ {}>", char::from(b'A' + slot as u8))
        );
        assert_eq!(doc.ini_text(team, "Waypoint"), "A");
        assert_eq!(doc.ini_text("Basic", "Player"), "Special");
        assert_eq!(doc.ini_text("Basic", "MultiplayerOnly"), "yes");
        let trigger = &mission::ids(&doc, Kind::Trigger)[0];
        assert!(
            doc.ini_text("Triggers", trigger)
                .starts_with("Neutral,<none>,")
        );
        assert!(
            doc.ini_text("Actions", trigger).ends_with(",U"),
            "spawn waypoint is independent of ownership"
        );
        assert!(
            !mission::references(&doc)
                .iter()
                .any(|r| r.from.0 == Kind::Team && r.field == "House")
        );
        assert!(
            !mission::diagnostics(&doc, None)
                .iter()
                .any(|d| d.issue.message.contains("<Player @"))
        );
        let rebuilt = prepare(&doc, &plan, &catalog).unwrap();
        assert_eq!(rebuilt.bytes, after, "rebuild changed slot {slot}");
        assert!(!rebuilt.apply(&mut doc).unwrap());
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        assert!(doc.redo());
        assert_eq!(doc.snapshot().bytes().unwrap(), after);
    }
    // Switching modes restores the previously managed Basic fields first.
    prepare(&doc, &plan(), &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert_eq!(doc.ini_text("Basic", "Player"), "Neutral");
    assert_eq!(doc.ini_text("Basic", "MultiplayerOnly"), "no");
    prepare(&doc, &gift_plan(0), &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert_eq!(doc.ini_text("Basic", "Player"), "Special");
    assert_eq!(doc.ini_text("Basic", "MultiplayerOnly"), "yes");
}

#[test]
fn start_player_validation_cannot_be_bypassed_by_forged_ir() {
    let (doc, mut catalog) = fixture();
    catalog.waypoints.extend([8, 20]);
    for slot in [1, 7, 8, 20, u32::MAX] {
        assert!(
            prepare(&doc, &gift_plan(slot), &catalog).is_err(),
            "invalid/missing start {slot}"
        );
    }
    let mut plan = gift_plan(0);
    plan.mode = MissionMode::SinglePlayer;
    plan.player = "Neutral".into();
    assert!(
        prepare(&doc, &plan, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("仅支持多人任务")
    );
    plan.mode = MissionMode::Multiplayer;
    assert!(
        prepare(&doc, &plan, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("不能指定固定")
    );
    plan.player.clear();
    for action in [Action::WinPlayer, Action::LosePlayer] {
        plan.rules[0].actions = vec![action];
        assert!(
            prepare(&doc, &plan, &catalog)
                .err()
                .unwrap()
                .to_string()
                .contains("仅支持单人任务")
        );
    }
    let mut payload = serde_json::to_value(gift_plan(0)).unwrap();
    for owner in [
        serde_json::json!({"kind":"waypoint", "id":0}),
        serde_json::json!({"kind":"startPlayer", "id":-1}),
        serde_json::json!({"kind":"startPlayer", "id":"0"}),
        serde_json::json!({"kind":"startPlayer", "id":0, "extra":true}),
    ] {
        payload["teams"][0]["house"] = owner;
        assert!(serde_json::from_value::<Mission>(payload.clone()).is_err());
    }
}

#[test]
fn object_tags_roundtrip_and_rebuild_preserves_object_edits() {
    use ra2_map_document::document::{Target, Value};
    let (mut doc, _) = fixture();
    let id = doc.add_record("Units", [2, 2], "Neutral", "HTNK").unwrap();
    let record = doc.records().remove(&id).unwrap();
    let object_key = format!("Units:{}", record.key);
    doc.commit("object");
    let catalog = Catalog::from_document(&doc, vec!["HTNK".into()], vec![]);
    let mut plan = plan();
    plan.rules[0].when = Event::Destroyed {
        object: Reference {
            kind: "object".into(),
            id: object_key,
        },
    };
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let (id, current) = doc
        .records()
        .into_iter()
        .find(|(_, r)| r.section == "Units" && r.key == record.key)
        .unwrap();
    assert!(current.fields()[7].starts_with("TSTG"));
    doc.set(
        Target::Record(id),
        Some(Value::Record(
            current.patched(&[(2, "128".into())]).unwrap(),
        )),
    )
    .unwrap();
    doc.commit("health edit");
    assert!(
        !prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap()
    );
    plan.rules.clear();
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let current = doc
        .records()
        .into_values()
        .find(|r| r.section == "Units" && r.key == record.key)
        .unwrap();
    assert_eq!(current.fields()[7], "None");
    assert_eq!(current.fields()[2], "128");
}

#[test]
fn generated_descriptions_preserve_ids_and_cannot_escape_comments() {
    let (_, mut catalog) = fixture();
    catalog.descriptions.insert(
        "units".into(),
        BTreeMap::from([(
            "E1".into(),
            "步兵：美国大兵 */\nexport const injected = true;\u{2028}@deprecated".into(),
        )]),
    );
    let source = catalog.typescript().unwrap();
    assert!(source.contains("步兵：美国大兵 * / export const injected = true; @deprecated */"));
    assert!(source.contains(
        "/** 步兵：美国大兵 * / export const injected = true; @deprecated */\n  E1 = \"E1\","
    ));
    assert!(source.contains(
        "units: Object.keys(Unit).map(key => Unit[key as keyof typeof Unit]) as readonly Unit[]"
    ));
    assert!(source.contains(
        "strings: Object.keys(Text).map(key => Text[key as keyof typeof Text]) as readonly Text[]"
    ));
    let serialized = serde_json::to_value(&catalog).unwrap();
    assert_eq!(serialized["units"], serde_json::json!(["E1"]));
    assert!(serialized.get("descriptions").is_none());
}

#[test]
fn generated_enum_members_handle_invalid_identifiers_collisions_and_prototype_names() {
    let catalog = Catalog {
        units: ["0", "A-B", "A_B", "A_B_2", "__proto__", "quote\"", "中文"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        ..Default::default()
    };
    let members: std::collections::BTreeSet<_> = catalog
        .units
        .iter()
        .map(|id| catalog.enum_member("units", id).unwrap())
        .collect();
    assert_eq!(members.len(), catalog.units.len());
    assert!(!members.contains("__proto__"));
    assert!(catalog.typescript().unwrap().contains("A_B_2 = \"A_B\""));
    assert!(catalog.enum_member("units", "missing").is_err());
}

#[test]
fn transpiler_invalidates_modules_and_failed_builds() {
    std::thread::Builder::new().stack_size(16 * 1024 * 1024).spawn(|| {
        let (_, catalog) = fixture();
        let mut compiler = Compiler::new().unwrap();
        let source = "import {defineMission, event, action} from './sdk'; import {map, House} from './map.generated'; import {delay} from './lib/waves'; export default defineMission(map, {id:'test', player:House.Neutral}, m => { m.rule('win', {when:event.sinceScenarioStart(delay()), once:true, actions:[action.winPlayer()]}); });";
        let module = "let calls = 9; export function delay():number { return ++calls; }";
        let mut files = BTreeMap::from([
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
            ("/mission.ts".into(), source.into()),
            ("/lib/waves.ts".into(), module.into()),
        ]);
        let seconds = |compiler: &mut Compiler, files: &BTreeMap<String,String>| {
            let plan = compiler.build(files.clone()).unwrap();
            match plan.rules[0].when { Event::ScenarioTime { seconds } => seconds, _ => panic!("wrong event") }
        };
        assert_eq!(seconds(&mut compiler, &files), 10);
        assert_eq!(seconds(&mut compiler, &files), 10); // exact-source cache
        files.get_mut("/mission.ts").unwrap().push_str("\n// edit\n");
        assert_eq!(seconds(&mut compiler, &files), 10); // new execution realm
        files.insert("/lib/waves.ts".into(), module.replace("= 9", "= 19"));
        assert_eq!(seconds(&mut compiler, &files), 20); // same signature, changed body
        files.insert("/lib/waves.ts".into(), "export function delay():string {return 'bad';}".into());
        for _ in 0..2 {
            assert!(compiler.build(files.clone()).unwrap_err().to_string().contains("任务结构无效"));
        }
        files.insert("/lib/waves.ts".into(), module.into());
        assert_eq!(seconds(&mut compiler, &files), 10);
        files.remove("/lib/waves.ts");
        assert!(compiler.build(files.clone()).unwrap_err().to_string().contains("找不到可执行任务模块"));
        files.insert("/lib/waves.ts".into(), module.into());
        assert_eq!(seconds(&mut compiler, &files), 10);
        // Remove a previously emitted module from the import graph, then re-add it.
        files.insert("/mission.ts".into(), source.replace("import {delay} from './lib/waves';", "const delay = () => 30;"));
        assert_eq!(seconds(&mut compiler, &files), 30);
        files.insert("/mission.ts".into(), source.into());
        assert_eq!(seconds(&mut compiler, &files), 10);
        let mut empty_catalog = catalog.clone();
        empty_catalog.houses.clear();
        files.insert("/map.generated.ts".into(), empty_catalog.typescript().unwrap());
        assert!(compiler.build(files.clone()).unwrap_err().to_string().contains("Unknown House"));
        files.insert("/map.generated.ts".into(), catalog.typescript().unwrap());
        assert_eq!(seconds(&mut compiler, &files), 10);
        // Declarations and type-only imports are left to the external IDE.
        files.insert("/lib.custom.d.ts".into(), "export interface Custom { value: MissingType }".into());
        files.insert("/mission.ts".into(), format!("import type {{Custom}} from './lib.custom.d'; const value: Custom = {{value:'bad'}}; {source}"));
        assert_eq!(seconds(&mut compiler, &files), 10);
        files.insert("/lib/unused.ts".into(), "const broken = ;".into());
        assert_eq!(seconds(&mut compiler, &files), 10);
        files.insert("/mission.ts".into(), source.replace("./lib/waves", "./lib/waves.ts"));
        assert_eq!(seconds(&mut compiler, &files), 10);
        files.insert("/mission.ts".into(), source.replace("./lib/waves", "./lib/waves.js"));
        assert_eq!(seconds(&mut compiler, &files), 10);
        // Syntax failure must not reuse the previous successful task.
        files.insert("/lib/waves.ts".into(), "export const delay = ;".into());
        let error = compiler.build(files.clone()).unwrap_err().to_string();
        assert!(error.contains("/lib/waves.ts:1:") && error.contains("语法错误"), "{error}");
        files.insert("/lib/waves.ts".into(), module.into());
        files.insert("/mission.ts".into(), source.into());
        assert_eq!(seconds(&mut compiler, &files), 10);
    }).unwrap().join().unwrap();
}

#[test]
fn transpiler_preserves_reexports_default_imports_and_module_cycles() {
    let (_, catalog) = fixture();
    let mut compiler = Compiler::new().unwrap();
    let source = "import {defineMission, event, action} from './sdk'; import {map, House} from './map.generated'; import value, * as values from './lib/index'; import {delay} from './lib/cycle'; export default defineMission(map, {id:'test', player:House.Neutral}, m => { if (value !== 10 || values.value !== 10 || values.named !== 10 || values.extra !== 20 || delay() !== 10) throw new Error('module exports changed'); m.rule('win', {when:event.sinceScenarioStart(value), once:true, actions:[action.winPlayer()]}); });";
    let mut files = BTreeMap::from([
        ("/map.generated.ts".into(), catalog.typescript().unwrap()),
        ("/mission.ts".into(), source.into()),
        ("/lib/values.ts".into(), "export const value: number = 10; export const extra = 20;".into()),
        ("/lib/index.ts".into(), "export * from './values'; export {value as default, value as named} from './values';".into()),
        ("/lib/cycle.ts".into(), "import {value} from './peer'; export function delay(): number {return value();}".into()),
        ("/lib/peer.ts".into(), "import {delay} from './cycle'; export function value(): number {return typeof delay === 'function' ? 10 : 0;}".into()),
    ]);
    assert_eq!(compiler.build(files.clone()).unwrap().rules.len(), 1);
    // Node's require/process and imports outside the task never reach the host.
    for (statement, expected) in [
        ("import './missing';", "找不到可执行任务模块"),
        ("import '../outside';", "导入超出任务目录"),
        ("import 'node:fs';", "只支持项目内相对导入"),
        ("require('fs');", "Only project-relative imports"),
        ("process.exit();", "执行失败"),
    ] {
        files.insert("/mission.ts".into(), format!("{statement}\n{source}"));
        let error = compiler.build(files.clone()).unwrap_err().to_string();
        assert!(error.contains(expected), "{statement}: {error}");
    }
    files.insert("/mission.ts".into(), source.into());
    assert_eq!(compiler.build(files).unwrap().rules.len(), 1);
}

#[test]
fn transpilation_skips_types_but_keeps_sdk_and_native_validation() {
    std::thread::Builder::new().stack_size(16 * 1024 * 1024).spawn(|| {
        let (doc, mut catalog) = fixture();
        catalog.descriptions.insert("units".into(), BTreeMap::from([
            ("E1".into(), "步兵：美国大兵 */\n特殊资源说明".into())
        ]));
        let mut compiler = Compiler::new().unwrap();
        let base = "import {defineMission, MissionMode, event, action, order} from './sdk'; import {map, House, Unit, Waypoint, MapObject, Text} from './map.generated'; export default defineMission(map, {id:'test', player:House.Neutral}, m => { BODY });";
        let mut files = BTreeMap::from([
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
            ("/mission.ts".into(), base.replace("BODY", "m.unit(Unit.E1); m.house(House.Neutral); m.waypoint(Waypoint.WP0); m.object(MapObject.Infantry_0); m.text(Text.MISSION_TEXT); m.rule('win', {when:event.sinceScenarioStart(10), once:true, actions:[action.winPlayer()]});")),
        ]);
        assert_eq!(compiler.build(files.clone()).unwrap().rules.len(), 1);
        files.insert("/mission.ts".into(), base.replace("BODY", "if (map.waypoints.length !== 1 || map.waypoints[0] !== Waypoint.WP0) throw new Error('numeric enum reverse keys leaked'); if (map.units.length !== 1 || map.units[0] !== Unit.E1) throw new Error('missing unit');"));
        compiler.build(files.clone()).unwrap();
        // No semantic TS checker runs during Apply, including assignments,
        // implicit any, nonexistent type names and string enum arguments.
        files.insert("/mission.ts".into(), base.replace("BODY", "const bad: number = 'wrong'; const noAny = (x) => x; type Custom = MissingType; m.unit('E1');"));
        compiler.build(files.clone()).unwrap();
        for (body, expected) in [
            ("m.house('MissingHouse');", "Unknown House"),
            ("m.waypoint(999);", "Unknown waypoint"),
            ("m.playerAt(Waypoint.WP0);", "仅支持多人任务"),
            ("m.unit(Unit.MissingUnit);", "Unknown unit type"),
            ("m.unit(House.Neutral);", "Unknown unit type"),
            ("defineMission({...map, units: [] as const}, {id:'other', player:House.Neutral}, other => { other.unit(Unit.E1); });", "Unknown unit type"),
            ("m.rule('invalid', {when:event.sinceScenarioStart(1), once:false, actions:[action.winPlayer()]});", "once:true"),
            ("m.rule('invalid', {when:event.sinceScenarioStart(1), once:true, actions:[action.reinforce(m.house(House.Neutral), m.waypoint(Waypoint.WP0))]});", "应为 team"),
            ("m.rule('invalid', {when:event.sinceScenarioStart(2147483647), once:true, actions:[action.winPlayer()]});", "范围"),
        ] {
            files.insert("/mission.ts".into(), base.replace("BODY", body));
            let error = compiler.build(files.clone())
                .and_then(|plan| prepare(&doc, &plan, &catalog))
                .map(|_| ())
                .unwrap_err().to_string();
            assert!(error.contains(expected), "{body}: {error}");
        }
        catalog.waypoints.extend([7, 8, 20]);
        files.insert("/map.generated.ts".into(), catalog.typescript().unwrap());
        let multiplayer = base.replace("player:House.Neutral", "mode:MissionMode.Multiplayer");
        files.insert("/mission.ts".into(), multiplayer.replace("BODY", "const gift = m.team('gift', {house:m.playerAt(Waypoint.WP7), waypoint:m.waypoint(Waypoint.WP20), units:[{type:m.unit(Unit.E1), count:3}], orders:[order.guard(60)]}); m.rule('give', {when:event.sinceScenarioStart(10), once:true, actions:[action.reinforce(gift, m.waypoint(Waypoint.WP20))]});"));
        let built = compiler.build(files.clone()).unwrap();
        assert_eq!(built.mode, MissionMode::Multiplayer);
        assert!(built.player.is_empty());
        assert!(matches!(built.teams[0].house, TeamOwner::StartPlayer { id: 7 }));
        for (source, expected) in [
            (multiplayer.replace("BODY", "m.playerAt(Waypoint.WP8);"), "仅支持 0～7"),
            (multiplayer.replace("BODY", "m.playerAt(Waypoint.WP20);"), "仅支持 0～7"),
            (multiplayer.replace("BODY", "m.playerAt(m.waypoint(Waypoint.WP0));"), "Unknown player start"),
            (multiplayer.replace("BODY", "m.playerAt(House.Neutral);"), "Unknown player start"),
            (multiplayer.replace("BODY", "m.playerAt(6);"), "Unknown player start"),
            (multiplayer.replace("mode:MissionMode.Multiplayer", "mode:MissionMode.Multiplayer, player:House.Neutral").replace("BODY", ""), "多人"),
        ] {
            files.insert("/mission.ts".into(), source.clone());
            let error = compiler.build(files.clone())
                .and_then(|plan| prepare(&doc, &plan, &catalog))
                .map(|_| ())
                .unwrap_err().to_string();
            assert!(error.contains(expected), "{source}: {error}");
        }
        files.insert("/mission.ts".into(), base.replace("BODY", "(m.playerAt as any)(Waypoint.WP0);"));
        assert!(compiler.build(files.clone()).unwrap_err().to_string().contains("仅支持多人任务"));
        files.insert("/mission.ts".into(), base.replace("BODY", "while (true) {}"));
        assert!(compiler.build(files).unwrap_err().to_string().contains("执行失败"));
    }).unwrap().join().unwrap();
}
