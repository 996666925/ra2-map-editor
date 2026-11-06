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
fn crate_lottery_uses_isolated_triggers_and_restores_legacy_crate_fields() {
    let (mut doc, _) = fixture();
    doc.set_ini("TSBOX", "Strength", Some(b"900".to_vec()))
        .unwrap();
    doc.set_ini("Powerups", "Money", Some(b"50,MONEYANIM,yes,2000".to_vec()))
        .unwrap();
    mission::create(&mut doc, Kind::House, "Neutral", "Neutral", None).unwrap();
    doc.set_ini("Neutral", "Country", Some(b"Neutral".to_vec()))
        .unwrap();
    doc.commit("manual settings");
    let mut catalog = Catalog::from_document(&doc, vec!["FV".into(), "HTK".into()], vec![]);
    catalog
        .registry_order
        .insert("vehicles".into(), vec!["FV".into(), "HTK".into()]);
    catalog.register(crate::catalog::ResourceKind::Building, vec!["TSBOX".into()]);
    catalog
        .registry_order
        .insert("buildings".into(), vec!["TSBOX".into()]);
    catalog.register(
        crate::catalog::ResourceKind::Country,
        vec!["Neutral".into()],
    );
    let source = r#"import {defineMission} from './sdk';import {map,House,Unit,Building,Waypoint} from './map.generated';
export default defineMission(map,{id:'draw',player:House.Neutral},m=>{
 m.crateLottery('draw',{ticket:m.building(Building.TSBOX),rewards:[m.unit(Unit.FV)],rewardAt:m.waypoint(Waypoint.WP0),cost:200,prerequisites:[]});
});"#;
    let plan = Compiler::new()
        .unwrap()
        .build(BTreeMap::from([
            ("/mission.ts".into(), source.into()),
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
        ]))
        .unwrap();
    assert!(plan.teams.is_empty() && plan.rules.is_empty());
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    for (key, value) in [
        ("CrateBeneath", "no"),
        ("BuildLimit", "1"),
        ("CrateBeneathIsMoney", "no"),
        ("LeaveRubble", "no"),
        ("Unsellable", "yes"),
        ("Strength", "20"),
    ] {
        assert_eq!(doc.ini_text("TSBOX", key), value);
    }
    assert!(doc.ini_value("TSBOX", "PlaceAsCrate").is_none());
    assert!(doc.ini_value("Basic", "TruckCrate").is_none());
    assert_eq!(doc.ini_text("Powerups", "Money"), "50,MONEYANIM,yes,2000");
    assert!(doc.ini_value("Powerups", "Unit").is_none());
    assert!(doc.ini_value("FV", "CrateGoodie").is_none());
    assert!(doc.ini_value("HTK", "CrateGoodie").is_none());
    // Emulate the previous SDK's managed global crate edits, then rebuild.
    let meta = "EditorMissionScript";
    let chunks: usize = doc.ini_text(meta, "Chunks").parse().unwrap();
    let hex: String = (0..chunks)
        .map(|i| doc.ini_text(meta, &format!("Data{i}")))
        .collect();
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    let mut manifest: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    for (section, key, value, before) in [
        (
            "Powerups",
            "Money",
            "0,MONEYANIM,yes,2000",
            Some("50,MONEYANIM,yes,2000"),
        ),
        ("Powerups", "Unit", "1", None),
        ("CrateRules", "UnitCrateType", "<none>", None),
        ("FV", "CrateGoodie", "yes", None),
    ] {
        doc.set_ini(section, key, Some(value.as_bytes().to_vec()))
            .unwrap();
        manifest["fields"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"section":section,"key":key,"value":value,"before":before}));
    }
    let hex: String = serde_json::to_vec(&manifest)
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let chunks: Vec<_> = hex.as_bytes().chunks(400).collect();
    doc.set_ini(meta, "Chunks", Some(chunks.len().to_string().into_bytes()))
        .unwrap();
    for (i, chunk) in chunks.iter().enumerate() {
        doc.set_ini(meta, &format!("Data{i}"), Some(chunk.to_vec()))
            .unwrap();
    }
    doc.commit("legacy global pool");
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert_eq!(doc.ini_text("Powerups", "Money"), "50,MONEYANIM,yes,2000");
    assert!(doc.ini_value("Powerups", "Unit").is_none());
    assert!(doc.ini_value("CrateRules", "UnitCrateType").is_none());
    assert!(doc.ini_value("FV", "CrateGoodie").is_none());
    let built = doc.snapshot().bytes().unwrap();
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert_eq!(doc.snapshot().bytes().unwrap(), built);
    let mut conflict = doc.fork();
    conflict
        .set_ini("TSBOX", "Strength", Some(b"99".to_vec()))
        .unwrap();
    assert!(prepare(&conflict, &plan, &catalog).is_err());
    let empty:Mission=serde_json::from_value(serde_json::json!({"version":2,"id":"draw","player":"Neutral","teams":[],"flags":[],"rules":[]})).unwrap();
    prepare(&doc, &empty, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert_eq!(doc.ini_text("TSBOX", "Strength"), "900");
    assert_eq!(doc.ini_text("Powerups", "Money"), "50,MONEYANIM,yes,2000");
    assert!(doc.ini_value("TSBOX", "CrateBeneath").is_none());
    assert!(mission::ids(&doc, Kind::Trigger).is_empty());
    assert!(mission::ids(&doc, Kind::Team).is_empty());
}

#[test]
fn crate_lottery_rejects_invalid_rewards_and_health() {
    let (mut doc, _) = fixture();
    mission::create(&mut doc, Kind::House, "Neutral", "Neutral", None).unwrap();
    doc.set_ini("Neutral", "Country", Some(b"Neutral".to_vec()))
        .unwrap();
    doc.commit("player");
    let before = doc.snapshot().bytes().unwrap();
    let mut catalog = Catalog::from_document(
        &doc,
        vec!["FV".into(), "E1".into()],
        vec!["MISSION:TEXT".into()],
    );
    catalog
        .registry_order
        .insert("vehicles".into(), vec!["FV".into()]);
    catalog.register(crate::catalog::ResourceKind::Building, vec!["BOX".into()]);
    catalog
        .registry_order
        .insert("buildings".into(), vec!["BOX".into()]);
    catalog.register(
        crate::catalog::ResourceKind::Country,
        vec!["Neutral".into()],
    );
    let mut base = serde_json::to_value(plan()).unwrap();
    base["crateLottery"] = serde_json::json!({"ticket":{"kind":"building","id":"BOX"},"rewards":[{"kind":"unit","id":"FV"}],"uiName":{"kind":"text","id":"MISSION:TEXT"},"rewardAt":{"kind":"waypoint","id":0}});
    prepare(
        &doc,
        &serde_json::from_value(base.clone()).unwrap(),
        &catalog,
    )
    .unwrap();
    for (field, value) in [
        ("rewards", serde_json::json!([])),
        ("rewards", serde_json::json!([{"kind":"unit","id":"E1"}])),
        (
            "rewards",
            serde_json::json!([{"kind":"unit","id":"FV"},{"kind":"unit","id":"FV"}]),
        ),
        ("cost", serde_json::json!(0)),
        ("strength", serde_json::json!(0)),
    ] {
        let mut invalid = base.clone();
        invalid["crateLottery"][field] = value;
        assert!(prepare(&doc, &serde_json::from_value(invalid).unwrap(), &catalog).is_err());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
    }
}

#[test]
fn crate_lottery_rejects_unsupported_ownership_and_preplaced_tickets() {
    let (doc, catalog, mission) = reward_unit_fixture();
    let value = serde_json::to_value(&mission).unwrap();
    let mut multi = value.clone();
    multi["mode"] = serde_json::json!("multiplayer");
    multi["player"] = serde_json::json!("");
    assert!(
        prepare(&doc, &serde_json::from_value(multi).unwrap(), &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("单人")
    );
    let mut shared = doc.fork();
    mission::create(&mut shared, Kind::House, "Other", "Other", None).unwrap();
    shared
        .set_ini("Other", "Country", Some(b"Neutral".to_vec()))
        .unwrap();
    shared.commit("shared country");
    assert!(
        prepare(&shared, &mission, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("共享 Country")
    );
    let mut placed = doc.fork();
    placed
        .add_record("Structures", [2, 2], "Neutral", "BOX")
        .unwrap();
    placed.commit("preplaced ticket");
    assert!(
        prepare(&placed, &mission, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("预放")
    );
    let mut invalid = value;
    invalid["crateLottery"]["rewardAt"]["id"] = serde_json::json!(99);
    assert!(prepare(&doc, &serde_json::from_value(invalid).unwrap(), &catalog).is_err());
}

fn reward_unit_fixture() -> (MapDocument, Catalog, Mission) {
    let (mut doc, _) = fixture();
    doc.set_ini("FV", "Speed", Some(b"6".to_vec())).unwrap();
    doc.set_ini("VehicleTypes", "0", Some(b"FV".to_vec()))
        .unwrap();
    mission::create(&mut doc, Kind::House, "Neutral", "Neutral", None).unwrap();
    doc.set_ini("Neutral", "Country", Some(b"Neutral".to_vec()))
        .unwrap();
    doc.commit("base vehicle");
    let mut catalog = Catalog::from_document(&doc, vec!["FV".into()], vec![]);
    catalog
        .registry_order
        .insert("vehicles".into(), vec!["FV".into()]);
    catalog.register(crate::catalog::ResourceKind::Building, vec!["BOX".into()]);
    catalog
        .registry_order
        .insert("buildings".into(), vec!["BOX".into()]);
    catalog.register(
        crate::catalog::ResourceKind::Country,
        vec!["Neutral".into()],
    );
    catalog.register(
        crate::catalog::ResourceKind::Weapon,
        vec!["HoverMissile".into()],
    );
    catalog.vehicle_templates.insert(
        "FV".into(),
        BTreeMap::from([
            ("Strength".into(), "200".into()),
            ("Speed".into(), "10".into()),
            ("Image".into(), "FV".into()),
            ("Primary".into(), "HoverMissile".into()),
            (
                "Locomotor".into(),
                "{4A582741-9839-11d1-B709-00A024DDAFD1}".into(),
            ),
        ]),
    );
    catalog.fixed_scripts = Some(vec![]);
    let source = r#"import {defineMission,order} from './sdk';import {map,House,Unit,Building,Waypoint} from './map.generated';
export default defineMission(map,{id:'prizes',player:House.Neutral},m=>{
 m.script('start',[order.changeScript(m.scriptRef('next'))]);
 m.script('next',[order.waitForever()]);
 const prize=m.rewardUnit('super_fv',{base:m.unit(Unit.FV),name:'奖励车',rules:{Strength:1500,SelfHealing:true}});
 m.crateLottery('draw',{ticket:m.building(Building.BOX),rewards:[prize],rewardAt:m.waypoint(Waypoint.WP0),prerequisites:[]});
});"#;
    let mission = Compiler::new()
        .unwrap()
        .build(BTreeMap::from([
            ("/mission.ts".into(), source.into()),
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
        ]))
        .unwrap();
    (doc, catalog, mission)
}

#[test]
fn reward_unit_isolated_cloning_rebuild_reload_cleanup_and_conflicts() {
    let (mut doc, catalog, mission) = reward_unit_fixture();
    let before = doc.snapshot().bytes().unwrap();
    prepare(&doc, &mission, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let id = doc
        .ini_entries("VehicleTypes")
        .into_iter()
        .find(|(_, key, _)| key.starts_with("TSRU"))
        .unwrap()
        .1;
    assert_eq!(doc.ini_text(&id, "Strength"), "1500");
    assert_eq!(
        doc.ini_text(&id, "Speed"),
        "6",
        "current map overrides inherited snapshot"
    );
    assert_eq!(doc.ini_text(&id, "Image"), "FV");
    assert_eq!(doc.ini_text(&id, "Primary"), "HoverMissile");
    assert_eq!(doc.ini_text(&id, "SelfHealing"), "yes");
    assert_eq!(doc.ini_text(&id, "TechLevel"), "-1");
    assert_eq!(doc.ini_text(&id, "AllowedToStartInMultiplayer"), "no");
    assert_eq!(doc.ini_text(&id, "CrateGoodie"), "no");
    assert!(doc.ini_text(&id, "UIName").starts_with("TS"));
    let ini =
        vera20k_core::rules::ini_parser::IniFile::from_bytes(&doc.snapshot().bytes().unwrap())
            .unwrap();
    let registry = vera20k_core::rules::team_ai_ini::TeamAiIniRegistry::from_sources(
        &vera20k_core::rules::ini_parser::IniFile::empty(),
        &ini,
        false,
    );
    let mut order = Vec::<&str>::new();
    for id in registry
        .team_types
        .iter()
        .filter_map(|team| team.section().read_name("Script", 0x80))
        .chain(registry.scripts.iter().map(|script| script.id.as_str()))
    {
        if !order.iter().any(|known| known.eq_ignore_ascii_case(id)) {
            order.push(id);
        }
    }
    let next = order
        .iter()
        .position(|id| doc.ini_text(id, "Name") == "next")
        .unwrap();
    let start = registry
        .scripts
        .iter()
        .find(|script| doc.ini_text(&script.id, "Name") == "start")
        .unwrap();
    assert_eq!(
        start.actions[0].argument, next as i32,
        "lottery script allocation must not shift Script17 targets"
    );
    assert!(doc.ini_value("FV", "Strength").is_none());
    assert_eq!(doc.ini_text("FV", "Speed"), "6");
    let built = doc.snapshot().bytes().unwrap();
    prepare(&doc, &mission, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert_eq!(doc.snapshot().bytes().unwrap(), built);
    let mut reloaded = catalog.clone();
    reloaded.units.push(id.clone());
    reloaded.technos.push(id.clone());
    reloaded
        .registry_order
        .get_mut("vehicles")
        .unwrap()
        .push(id.clone());
    reloaded.type_projection = registry_fingerprints(&built).unwrap();
    reloaded
        .type_projection
        .retain(|section, _| !matches!(section.as_str(), "ScriptTypes" | "Houses" | "Waypoints"));
    prepare(&doc, &mission, &reloaded)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert_eq!(doc.snapshot().bytes().unwrap(), built);
    let mut conflict = doc.fork();
    conflict
        .set_ini(&id, "Strength", Some(b"999".to_vec()))
        .unwrap();
    assert!(prepare(&conflict, &mission, &reloaded).is_err());
    let empty:Mission=serde_json::from_value(serde_json::json!({"version":2,"id":"prizes","player":"Neutral","teams":[],"flags":[],"rules":[]})).unwrap();
    prepare(&doc, &empty, &reloaded)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert!(doc.ini_entries(&id).is_empty());
    assert_eq!(doc.ini_entries("VehicleTypes").len(), 1);
    assert!(doc.ini_value("FV", "CrateGoodie").is_none());
    assert_eq!(doc.ini_text("FV", "Speed"), "6");
    assert_ne!(before, built);
}

#[test]
fn reward_unit_respects_cleared_base_fields_and_separate_variants() {
    let (mut doc, catalog, mission) = reward_unit_fixture();
    doc.set_ini("FV", "Primary", Some(Vec::new())).unwrap();
    doc.commit("clear base weapon");
    let mut value = serde_json::to_value(mission).unwrap();
    let mut second = value["rewardUnits"][0].clone();
    second["id"] = serde_json::json!("second_fv");
    second["uiName"] = serde_json::Value::Null;
    second["rules"] = serde_json::json!({"Strength": 600});
    value["rewardUnits"].as_array_mut().unwrap().push(second);
    value["crateLottery"]["rewards"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"kind":"rewardUnit","id":"second_fv"}));
    prepare(&doc, &serde_json::from_value(value).unwrap(), &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let variants: Vec<_> = doc
        .ini_entries("VehicleTypes")
        .into_iter()
        .filter(|(_, key, _)| key.starts_with("TSRU"))
        .map(|(_, key, _)| key)
        .collect();
    assert_eq!(variants.len(), 2);
    let strengths: std::collections::BTreeSet<_> = variants
        .iter()
        .map(|id| doc.ini_text(id, "Strength"))
        .collect();
    assert_eq!(
        strengths,
        std::collections::BTreeSet::from(["600".into(), "1500".into()])
    );
    assert!(
        variants
            .iter()
            .all(|id| doc.ini_value(id, "Primary").is_none())
    );
    assert!(doc.ini_value("FV", "Strength").is_none());
}

#[test]
fn reward_unit_rejects_invalid_rules_without_map_mutation() {
    let (doc, catalog, mission) = reward_unit_fixture();
    let before = doc.snapshot().bytes().unwrap();
    let base = serde_json::to_value(mission).unwrap();
    for rules in [
        serde_json::json!({"TechLevel":1}),
        serde_json::json!({"UIName":"unmanaged"}),
        serde_json::json!({"CrateGoodie":false}),
        serde_json::json!({"AllowedToStartInMultiplayer":true}),
        serde_json::json!({"Speed":[8]}),
        serde_json::json!({"Speed":null}),
        serde_json::json!({"Primary":"MissingWeapon"}),
        serde_json::json!({"EliteWeapon1":"MissingWeapon"}),
        serde_json::json!({"Name":"break\n[General]"}),
        serde_json::json!({"Speed=":8}),
        serde_json::json!({"speed":8}),
    ] {
        let mut invalid = base.clone();
        invalid["rewardUnits"][0]["rules"] = rules;
        assert!(
            prepare(
                &doc,
                &serde_json::from_value(invalid.clone()).unwrap(),
                &catalog
            )
            .is_err(),
            "{invalid}"
        );
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
    }
    let mut invalid = base;
    invalid["rewardUnits"][0]["base"]["kind"] = serde_json::json!("building");
    assert!(prepare(&doc, &serde_json::from_value(invalid).unwrap(), &catalog).is_err());
}

#[test]
fn house_ui_names_and_edges_preserve_native_reading_ownership_and_history() {
    let (mut doc, _) = fixture();
    mission::create(&mut doc, Kind::House, "Neutral", "Neutral", None).unwrap();
    doc.set_ini("Neutral", "UIName", Some(b"MANUAL".to_vec()))
        .unwrap();
    doc.set_ini("Neutral", "Edge", Some(b"South".to_vec()))
        .unwrap();
    doc.set_ini("Neutral", "RatioTeamAircraft", Some(b"37".to_vec()))
        .unwrap();
    doc.commit("manual House");
    let before = doc.snapshot().bytes().unwrap();
    let mut catalog = Catalog::from_document(&doc, vec!["E1".into()], vec![]);
    catalog.register(
        crate::catalog::ResourceKind::Country,
        vec!["Americans".into()],
    );
    catalog.house_colors = vec!["Gold".into()];
    let mut compiler = Compiler::new().unwrap();
    for (edge, index, native) in [
        ("automatic", -1, "<none>"),
        ("north", 0, "North"),
        ("east", 1, "East"),
        ("south", 2, "South"),
        ("west", 3, "West"),
        ("air", 4, "Air"),
    ] {
        let source = format!(
            "import {{defineMission,HouseEdge}} from './sdk';import {{map,House,Country}} from './map.generated';export default defineMission(map,{{id:'house_names',player:House.Neutral}},m=>{{const name=m.defineText('display','友军😀指挥部');m.configureHouse(m.house(House.Neutral),{{uiName:name,edge:'{edge}' as HouseEdge}});m.createHouse('friend',{{country:m.country(Country.Americans),uiName:name,edge:HouseEdge.West}});}});"
        );
        let mut plan = compiler
            .build(BTreeMap::from([
                ("/mission.ts".into(), source),
                ("/map.generated.ts".into(), catalog.typescript().unwrap()),
            ]))
            .unwrap();
        let mut doc = doc.fork();
        prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        let generated = doc.snapshot().bytes().unwrap();
        let parsed = vera20k_core::map::map_file::load_from_bytes(generated.clone()).unwrap();
        let roster = vera20k_core::map::houses::parse_house_roster(&parsed.ini, &[], None);
        let label = plan.house_definitions[0]
            .ui_name
            .as_ref()
            .unwrap()
            .id
            .clone();
        assert_eq!(
            roster
                .houses
                .iter()
                .find(|h| h.name == "Neutral")
                .unwrap()
                .ui_name
                .as_deref(),
            Some(label.as_str())
        );
        assert_eq!(
            parsed.ini.section("Neutral").unwrap().read_edge("Edge", -1),
            index
        );
        assert_eq!(doc.ini_text("Neutral", "Edge"), native);
        let created = mission::ids(&doc, Kind::House)
            .into_iter()
            .find(|h| doc.ini_text(h, "Name") == "friend")
            .unwrap();
        assert_eq!(doc.ini_text(&created, "UIName"), label);
        assert_eq!(doc.ini_text(&created, "Edge"), "West");
        assert_eq!(doc.ini_text("Neutral", "RatioTeamAircraft"), "37");
        assert!(
            !prepare(&doc, &plan, &catalog)
                .unwrap()
                .apply(&mut doc)
                .unwrap()
        );
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        assert!(doc.redo());
        assert_eq!(doc.snapshot().bytes().unwrap(), generated);
        for key in ["UIName", "Edge"] {
            let mut conflict = doc.fork();
            conflict
                .set_ini("Neutral", key, Some(b"manual_edit".to_vec()))
                .unwrap();
            assert!(prepare(&conflict, &plan, &catalog).is_err());
        }
        plan.house_definitions.clear();
        plan.texts.clear();
        prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        assert_eq!(doc.ini_text("Neutral", "UIName"), "MANUAL");
        assert_eq!(doc.ini_text("Neutral", "Edge"), "South");
        assert!(!mission::ids(&doc, Kind::House).contains(&created));
        assert!(doc.ini_fields(&created).is_empty());
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), generated);
    }
}

#[test]
fn house_ui_names_and_edges_reject_forged_references_and_reader_truncation() {
    let (mut doc, _) = fixture();
    mission::create(&mut doc, Kind::House, "Neutral", "Neutral", None).unwrap();
    doc.commit("house");
    let mut catalog = Catalog::from_document(&doc, vec!["E1".into()], vec![]);
    let before = doc.snapshot().bytes().unwrap();
    for (label, valid) in [
        ("A".repeat(31), true),
        ("A".repeat(32), false),
        ("中文".into(), false),
        ("BAD;KEY".into(), false),
        ("".into(), false),
        (" A".into(), false),
    ] {
        catalog.strings.push(label.clone());
        let mut value = serde_json::to_value(plan()).unwrap();
        value["houseDefinitions"] =
            serde_json::json!([{"id":"Neutral","uiName":{"kind":"text","id":label}}]);
        assert_eq!(
            prepare(&doc, &serde_json::from_value(value).unwrap(), &catalog).is_ok(),
            valid
        );
    }
    for fields in [
        serde_json::json!({"edge":"northeast"}),
        serde_json::json!({"edge":1}),
        serde_json::json!({"uiName":{"kind":"sound","id":"A"}}),
        serde_json::json!({"uiName":{"kind":"text","id":"MISSING"}}),
        serde_json::json!({"ratioTeamAircraft":50}),
    ] {
        let mut value = serde_json::to_value(plan()).unwrap();
        let mut fields = fields;
        fields["id"] = serde_json::json!("Neutral");
        value["houseDefinitions"] = serde_json::json!([fields]);
        if let Ok(mission) = serde_json::from_value::<Mission>(value) {
            assert!(prepare(&doc, &mission, &catalog).is_err());
        }
    }
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn lighting_profiles_use_native_quantization_and_restore_only_owned_fields() {
    let (mut doc, catalog) = fixture();
    doc.set_ini("Lighting", "Ambient", Some(b"0.9".to_vec()))
        .unwrap();
    doc.set_ini("Lighting", "Ground", Some(b"0.07".to_vec()))
        .unwrap();
    doc.set_ini("Lighting", "Keep", Some(b"manual".to_vec()))
        .unwrap();
    doc.commit("manual lighting");
    let before = doc.snapshot().bytes().unwrap();
    let source = r#"import {defineMission} from './sdk';import {map,House} from './map.generated';
export default defineMission(map,{id:'lighting',player:House.Neutral},m=>{
 m.configureLighting({normal:{ambient:0.61,red:1.2,green:0,blue:-0.25,level:0.032},
 ion:{ambient:0.3,red:0.4,green:0.5,blue:0.6,ground:0.02,level:0.004},
 dominator:{ambient:1.5,red:0.85,green:0.2,blue:0.3,ground:0.01,level:0.008},
 dominatorAmbientChangeRate:0.003,nukeAmbientChangeRate:4});
});"#;
    let mut compiler = Compiler::new().unwrap();
    for mode in [MissionMode::SinglePlayer, MissionMode::Multiplayer] {
        let input = if mode == MissionMode::SinglePlayer {
            source.to_string()
        } else {
            source
                .replace("{defineMission}", "{defineMission,MissionMode}")
                .replace("player:House.Neutral", "mode:MissionMode.Multiplayer")
        };
        let mut mission = compiler
            .build(BTreeMap::from([
                ("/mission.ts".into(), input),
                ("/map.generated.ts".into(), catalog.typescript().unwrap()),
            ]))
            .unwrap();
        let mut doc = doc.fork();
        prepare(&doc, &mission, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        let generated = doc.snapshot().bytes().unwrap();
        let map = vera20k_core::map::map_file::load_from_bytes(generated.clone()).unwrap();
        let p = vera20k_core::map::lighting::parse_lighting_profiles(&map.ini);
        let values = |p: vera20k_core::map::lighting::LightingProfileUnits| {
            [
                p.ambient_percent,
                p.red_percent,
                p.green_percent,
                p.blue_percent,
                p.ground_units,
                p.level_units,
            ]
        };
        assert_eq!(values(p.normal), [61, 120, 0, -24, 70, 32]);
        assert_eq!(values(p.ion), [30, 40, 50, 60, 20, 4]);
        assert_eq!(values(p.dominator), [150, 85, 20, 30, 10, 8]);
        assert_eq!((p.dominator_change_rate, p.nuke_change_rate), (3, 4));
        assert_eq!(doc.ini_text("Lighting", "Keep"), "manual");
        assert!(
            !prepare(&doc, &mission, &catalog)
                .unwrap()
                .apply(&mut doc)
                .unwrap()
        );
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        assert!(doc.redo());
        assert_eq!(doc.snapshot().bytes().unwrap(), generated);
        let mut conflict = doc.fork();
        conflict
            .set_ini("Lighting", "Ambient", Some(b"1".to_vec()))
            .unwrap();
        assert!(prepare(&conflict, &mission, &catalog).is_err());
        mission.lighting = None;
        prepare(&doc, &mission, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        assert_eq!(doc.ini_text("Lighting", "Ambient"), "0.9");
        assert_eq!(doc.ini_text("Lighting", "Ground"), "0.07");
        assert_eq!(doc.ini_text("Lighting", "Keep"), "manual");
        assert!(doc.ini_value("Lighting", "IonRed").is_none());
        assert!(doc.ini_value("Lighting", "NukeAmbientChangeRate").is_none());
        assert!(doc.undo());
        assert_eq!(doc.snapshot().bytes().unwrap(), generated);
    }
}

#[test]
fn lighting_rejects_nonfinite_overflow_unknown_fields_and_subframe_intervals() {
    let (doc, catalog) = fixture();
    let before = doc.snapshot().bytes().unwrap();
    for fields in [
        serde_json::json!({"normal":{"ambient":1e40}}),
        serde_json::json!({"normal":{"ambient":21474838}}),
        serde_json::json!({"normal":{"ground":2147483.648}}),
        serde_json::json!({"ion":{"level":-2147484.0}}),
        serde_json::json!({"normal":{"ambient":"0.5"}}),
        serde_json::json!({"normal":{"purple":1}}),
        serde_json::json!({"nuke":{"ambient":1}}),
        serde_json::json!({"dominatorAmbientChangeRate":0.0005}),
        serde_json::json!({"dominatorAmbientChangeRate":0}),
        serde_json::json!({"nukeAmbientChangeRate":0}),
        serde_json::json!({"nukeAmbientChangeRate":-1}),
        serde_json::json!({"nukeAmbientChangeRate":1.5}),
        serde_json::json!({"nukeAmbientChangeRate":2147483647}),
    ] {
        let mut json = serde_json::to_value(plan()).unwrap();
        json["lighting"] = fields.clone();
        if let Ok(mission) = serde_json::from_value::<Mission>(json) {
            assert!(prepare(&doc, &mission, &catalog).is_err(), "{fields}");
        }
    }
    // JSON cannot represent these, but Rust callers and TS arithmetic can.
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut mission = plan();
        mission.lighting = Some(serde_json::from_value(serde_json::json!({"normal":{}})).unwrap());
        mission
            .lighting
            .as_mut()
            .unwrap()
            .normal
            .as_mut()
            .unwrap()
            .ambient = Some(value);
        assert!(prepare(&doc, &mission, &catalog).is_err());
    }
    let mut compiler = Compiler::new().unwrap();
    for code in [
        "m.configureLighting({});m.configureLighting({});",
        "m.configureLighting({normal:{ambient:0/0}});",
        "m.configureLighting({ion:{red:1/0}});",
        "m.configureLighting({dominatorAmbientChangeRate:1/0});",
        "m.configureLighting({nukeAmbientChangeRate:0/0});",
    ] {
        let input = format!(
            "import {{defineMission}} from './sdk';import {{map,House}} from './map.generated';export default defineMission(map,{{id:'lighting_bad',player:House.Neutral}},m=>{{{code}}});"
        );
        assert!(
            compiler
                .build(BTreeMap::from([
                    ("/mission.ts".into(), input),
                    ("/map.generated.ts".into(), catalog.typescript().unwrap()),
                ]))
                .is_err()
        );
    }
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn special_flags_follow_mode_ownership_and_restore_existing_map_values() {
    let (mut doc, catalog) = fixture();
    doc.set_ini("SpecialFlags", "Inert", Some(b"no".to_vec()))
        .unwrap();
    doc.set_ini("SpecialFlags", "FogOfWar", Some(b"no".to_vec()))
        .unwrap();
    doc.set_ini("SpecialFlags", "MCVDeploy", Some(b"yes".to_vec()))
        .unwrap();
    doc.commit("manual flags");
    let before = doc.snapshot().bytes().unwrap();
    let source = r#"import {defineMission} from './sdk';import {map,House} from './map.generated';
export default defineMission(map,{id:'special_flags',player:House.Neutral},m=>{
 m.configureSpecialFlags({inert:true,fogOfWar:true,harvesterImmune:true,destroyableBridges:false,tiberiumGrows:true,tiberiumSpreads:false});
});"#;
    let mut compiler = Compiler::new().unwrap();
    let mission = compiler
        .build(BTreeMap::from([
            ("/mission.ts".into(), source.into()),
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
        ]))
        .unwrap();
    prepare(&doc, &mission, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let generated = doc.snapshot().bytes().unwrap();
    let map = vera20k_core::map::map_file::load_from_bytes(generated.clone()).unwrap();
    let flags = &map.special_flags;
    assert_eq!(
        [
            flags.inert,
            flags.fog_of_war,
            flags.harvester_immune,
            flags.destroyable_bridges,
            flags.tiberium_grows,
            flags.tiberium_spreads
        ],
        [
            Some(true),
            Some(true),
            Some(true),
            Some(false),
            Some(true),
            Some(false)
        ]
    );
    assert_eq!(flags.mcv_deploy, Some(true));
    assert!(
        !prepare(&doc, &mission, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap()
    );
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    assert!(doc.redo());
    assert_eq!(doc.snapshot().bytes().unwrap(), generated);
    let mut conflict = doc.fork();
    conflict
        .set_ini("SpecialFlags", "Inert", Some(b"no".to_vec()))
        .unwrap();
    assert!(prepare(&conflict, &mission, &catalog).is_err());
    let mut removed = mission;
    removed.special_flags = None;
    prepare(&doc, &removed, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert_eq!(doc.ini_text("SpecialFlags", "Inert"), "no");
    assert_eq!(doc.ini_text("SpecialFlags", "FogOfWar"), "no");
    assert_eq!(doc.ini_text("SpecialFlags", "MCVDeploy"), "yes");
    assert!(doc.ini_value("SpecialFlags", "TiberiumGrows").is_none());
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), generated);
    let (mut multiplayer, catalog) = fixture();
    let source=source.replace("{defineMission}","{defineMission,MissionMode}").replace("player:House.Neutral","mode:MissionMode.Multiplayer").replace("inert:true,fogOfWar:true,harvesterImmune:true,destroyableBridges:false,tiberiumGrows:true,tiberiumSpreads:false","initialVeteran:true");
    let mission = compiler
        .build(BTreeMap::from([
            ("/mission.ts".into(), source),
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
        ]))
        .unwrap();
    prepare(&multiplayer, &mission, &catalog)
        .unwrap()
        .apply(&mut multiplayer)
        .unwrap();
    let map = vera20k_core::map::map_file::load_from_bytes(multiplayer.snapshot().bytes().unwrap())
        .unwrap();
    assert_eq!(map.special_flags.initial_veteran, Some(true));
}

#[test]
fn special_flags_reject_ignored_mode_fields_and_invalid_forged_data() {
    let (doc, catalog) = fixture();
    let before = doc.snapshot().bytes().unwrap();
    let base = serde_json::to_value(plan()).unwrap();
    for (mode, fields) in [
        ("singlePlayer", serde_json::json!({"initialVeteran":true})),
        ("multiplayer", serde_json::json!({"inert":false})),
        ("multiplayer", serde_json::json!({"fogOfWar":false})),
        ("multiplayer", serde_json::json!({"harvesterImmune":true})),
        (
            "multiplayer",
            serde_json::json!({"destroyableBridges":false}),
        ),
        ("multiplayer", serde_json::json!({"tiberiumGrows":false})),
        ("multiplayer", serde_json::json!({"tiberiumSpreads":false})),
        ("singlePlayer", serde_json::json!({"inert":"yes"})),
        ("singlePlayer", serde_json::json!({"mcvDeploy":true})),
    ] {
        let mut json = base.clone();
        json["mode"] = serde_json::json!(mode);
        json["rules"] = serde_json::json!([]);
        if mode == "multiplayer" {
            json["player"] = serde_json::json!("");
        }
        json["specialFlags"] = fields;
        if let Ok(mission) = serde_json::from_value::<Mission>(json) {
            assert!(prepare(&doc, &mission, &catalog).is_err());
        }
    }
    let source = "import {defineMission} from './sdk';import {map,House} from './map.generated';export default defineMission(map,{id:'flags_twice',player:House.Neutral},m=>{m.configureSpecialFlags({});m.configureSpecialFlags({});});";
    assert!(
        Compiler::new()
            .unwrap()
            .build(BTreeMap::from([
                ("/mission.ts".into(), source.into()),
                ("/map.generated.ts".into(), catalog.typescript().unwrap())
            ]))
            .is_err()
    );
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn ranking_uses_native_time_encoding_and_managed_restore_with_unicode_texts() {
    let (mut doc, _) = fixture();
    doc.set_ini("Ranking", "ParTimeMedium", Some(b"00:07:00".to_vec()))
        .unwrap();
    doc.set_ini("Ranking", "UnderParTitle", Some(b"MANUAL".to_vec()))
        .unwrap();
    doc.set_ini("Ranking", "Keep", Some(b"untouched".to_vec()))
        .unwrap();
    doc.commit("manual ranking");
    let before = doc.snapshot().bytes().unwrap();
    let catalog = Catalog::from_document(&doc, vec!["E1".into()], vec!["MISSION:TEXT".into()]);
    let source = r#"import {defineMission} from './sdk';
import {map,House,Text} from './map.generated';
export default defineMission(map,{id:'ranking',player:House.Neutral},m=>{
 m.configureRanking({parTime:{easy:0,normal:150,hard:359999},
  underParTitle:m.defineText('rank','优秀指挥官'), underParMessage:m.defineText('commendation','准时完成😀'),
  overParTitle:m.text(Text.MISSION_TEXT),overParMessage:m.defineText('slow','请加快速度')});
});"#;
    let plan = Compiler::new()
        .unwrap()
        .build(BTreeMap::from([
            ("/mission.ts".into(), source.into()),
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
        ]))
        .unwrap();
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let generated = doc.snapshot().bytes().unwrap();
    let map = vera20k_core::map::map_file::load_from_bytes(generated.clone()).unwrap();
    let ranking = vera20k_core::map::ranking::parse_ranking_section(&map.ini);
    assert_eq!(
        [
            ranking.par_time_easy_ticks,
            ranking.par_time_medium_ticks,
            ranking.par_time_hard_ticks
        ],
        [0, 9000, 21_599_940]
    );
    assert_eq!(
        ranking.under_par_title,
        plan.ranking
            .as_ref()
            .unwrap()
            .under_par_title
            .as_ref()
            .unwrap()
            .id
    );
    assert_eq!(ranking.over_par_title, "MISSION:TEXT");
    assert!(
        declared_texts(&doc)
            .unwrap()
            .values()
            .any(|s| s == "准时完成😀")
    );
    assert!(
        !prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap()
    );
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    assert!(doc.redo());
    assert_eq!(doc.snapshot().bytes().unwrap(), generated);
    let mut conflict = doc.fork();
    conflict
        .set_ini("Ranking", "ParTimeMedium", Some(b"00:01:00".to_vec()))
        .unwrap();
    assert!(prepare(&conflict, &plan, &catalog).is_err());
    let mut removed = plan;
    removed.ranking = None;
    removed.texts.clear();
    prepare(&doc, &removed, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert_eq!(doc.ini_text("Ranking", "ParTimeMedium"), "00:07:00");
    assert_eq!(doc.ini_text("Ranking", "UnderParTitle"), "MANUAL");
    assert_eq!(doc.ini_text("Ranking", "Keep"), "untouched");
    assert!(doc.ini_value("Ranking", "ParTimeEasy").is_none());
    assert!(doc.ini_value("Ranking", "OverParMessage").is_none());
    assert!(declared_texts(&doc).unwrap().is_empty());
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), generated);
    assert!(doc.redo());
    assert_eq!(doc.ini_text("Ranking", "ParTimeMedium"), "00:07:00");
}

#[test]
fn ranking_rejects_forged_times_references_and_multiplayer_without_mutation() {
    let (doc, mut catalog) = fixture();
    let before = doc.snapshot().bytes().unwrap();
    let base = serde_json::to_value(plan()).unwrap();
    for seconds in [
        serde_json::json!(-1),
        serde_json::json!(1.5),
        serde_json::json!(360000),
        serde_json::json!(4294967296u64),
    ] {
        let mut json = base.clone();
        json["ranking"] = serde_json::json!({"parTime":{"normal":seconds}});
        if let Ok(plan) = serde_json::from_value::<Mission>(json) {
            assert!(prepare(&doc, &plan, &catalog).is_err());
        }
    }
    for (label, valid) in [
        ("A".repeat(30), true),
        ("A".repeat(31), false),
        ("中文".into(), false),
        ("BAD;KEY".into(), false),
        ("BAD\0KEY".into(), false),
    ] {
        catalog.strings.push(label.clone());
        let mut json = base.clone();
        json["ranking"] = serde_json::json!({"underParTitle":{"kind":"text","id":label}});
        let plan: Mission = serde_json::from_value(json).unwrap();
        assert_eq!(prepare(&doc, &plan, &catalog).is_ok(), valid);
    }
    for (mode, kind, label) in [
        ("singlePlayer", "text", "MISSING"),
        ("singlePlayer", "sound", "MISSION:TEXT"),
        ("multiplayer", "text", "MISSION:TEXT"),
    ] {
        let mut json = base.clone();
        json["mode"] = serde_json::json!(mode);
        if mode == "multiplayer" {
            json["player"] = serde_json::json!("");
            json["rules"] = serde_json::json!([]);
        }
        json["ranking"] = serde_json::json!({"underParTitle":{"kind":kind,"id":label}});
        assert!(prepare(&doc, &serde_json::from_value(json).unwrap(), &catalog).is_err());
    }
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
}

#[test]
fn ranking_builder_rejects_duplicate_configuration_and_invalid_seconds() {
    let (_, catalog) = fixture();
    let mut compiler = Compiler::new().unwrap();
    for code in [
        "m.configureRanking({});m.configureRanking({});",
        "m.configureRanking({parTime:{easy:-1}});",
        "m.configureRanking({parTime:{hard:1.5}});",
        "m.configureRanking({parTime:{normal:360000}});",
    ] {
        let source = format!(
            "import {{defineMission}} from './sdk';import {{map,House}} from './map.generated';export default defineMission(map,{{id:'rank',player:House.Neutral}},m=>{{{code}}});"
        );
        assert!(
            compiler
                .build(BTreeMap::from([
                    ("/mission.ts".into(), source),
                    ("/map.generated.ts".into(), catalog.typescript().unwrap())
                ]))
                .is_err()
        );
    }
}

#[test]
fn movie_subtitle_asset_is_allowed_without_opening_arbitrary_text_files() {
    let (doc, catalog) = fixture();
    let mut compiler = Compiler::new().unwrap();
    for (name, allowed) in [
        ("subtitlemd.txt", true),
        ("SUBTITLEMD.TXT", true),
        ("notes.txt", false),
        ("install.txt", false),
    ] {
        let source = format!(
            r#"import {{defineMission,event,action}} from './sdk';
import {{map,House}} from './map.generated';
export default defineMission(map,{{id:'subtitles',player:House.Neutral}},m=>{{
 m.asset('{name}','captions.txt');
 m.rule('win',{{when:event.sinceScenarioStart(1),once:true,actions:[action.winPlayer()]}});
}});"#
        );
        let mission = compiler
            .build(BTreeMap::from([
                ("/mission.ts".into(), source),
                ("/map.generated.ts".into(), catalog.typescript().unwrap()),
            ]))
            .unwrap();
        assert_eq!(
            prepare(&doc, &mission, &catalog).is_ok(),
            allowed,
            "asset {name}"
        );
    }
}

#[test]
fn native_references_aliases_and_sources_preserve_manual_ownership() {
    let (mut doc, _) = fixture();
    mission::create(&mut doc, Kind::TaskForce, "MANUAL_FORCE", "Neutral", None).unwrap();
    mission::create(&mut doc, Kind::Script, "MANUAL_SCRIPT", "Neutral", None).unwrap();
    mission::create(&mut doc, Kind::Trigger, "MANUAL_RULE", "Neutral", None).unwrap();
    let object_id = doc.add_record("Units", [2, 2], "Neutral", "E1").unwrap();
    let key = doc.records()[&object_id].key.clone();
    doc.commit("manual definitions");
    let mut catalog = Catalog::from_document(&doc, vec!["E1".into()], vec![]);
    catalog.fixed_scripts = Some(vec![]);
    let source = format!(
        r#"import {{defineMission,event,action,order}} from './sdk';
export default defineMission({schema},{{id:'existing',player:'Neutral'}},m=>{{
 m.source('waves.ts',()=>{{
  const target=m.aliasObject('target',m.object('Units:{key}'));
  const at=m.aliasWaypoint('entry',m.waypoint(0));
  m.team('team',{{house:m.house('Neutral'),waypoint:at,taskForce:m.nativeTaskForce('MANUAL_FORCE'),script:m.nativeScript('MANUAL_SCRIPT')}});
  m.script('switch',[order.changeScript(m.nativeScript('MANUAL_SCRIPT','other_script'))]);
  m.rule('disable',{{when:event.destroyed(target),once:true,actions:[action.disableRule(m.nativeRule('MANUAL_RULE'))]}});
 }});
}});"#,
        schema = serde_json::to_string(&catalog).unwrap()
    );
    let plan = Compiler::new()
        .unwrap()
        .build(BTreeMap::from([("/mission.ts".into(), source)]))
        .unwrap();
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let mappings = source_mappings(&doc).unwrap();
    assert_eq!(mappings.len(), 3);
    check_product(&doc).unwrap();
    assert!(
        mappings
            .iter()
            .all(|m| m.module.as_deref() == Some("waves.ts"))
    );
    assert!(
        !prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap()
    );
    let team = mission::ids(&doc, Kind::Team)
        .into_iter()
        .find(|id| id.starts_with("TSTM"))
        .unwrap();
    assert_eq!(doc.ini_text(&team, "TaskForce"), "MANUAL_FORCE");
    let (object_id, old) = doc
        .records()
        .into_iter()
        .find(|(_, r)| r.section == "Units" && r.key == key)
        .unwrap();
    let changed = old.patched(&[(2, "128".into())]).unwrap();
    doc.set(
        ra2_map_document::document::Target::Record(object_id),
        Some(ra2_map_document::document::Value::Record(changed)),
    )
    .unwrap();
    assert!(check_product(&doc).is_err());
    assert!(
        prepare(&doc, &plan, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("重新选择")
    );
    doc.set(
        ra2_map_document::document::Target::Record(object_id),
        Some(ra2_map_document::document::Value::Record(old)),
    )
    .unwrap();
    let mut empty = plan;
    empty.rules.clear();
    empty.teams.clear();
    empty.scripts.clear();
    empty.origins.clear();
    empty.aliases.clear();
    empty.native_definitions.clear();
    doc.commit("restore alias");
    prepare(&doc, &empty, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert!(mission::ids(&doc, Kind::TaskForce).contains(&"MANUAL_FORCE".into()));
    assert!(mission::ids(&doc, Kind::Script).contains(&"MANUAL_SCRIPT".into()));
    assert!(mission::ids(&doc, Kind::Trigger).contains(&"MANUAL_RULE".into()));
}

#[test]
fn campaign_entry_compiles_unicode_metadata_and_validates_direct_branches() {
    let source = r#"import {defineCampaign} from './sdk';
export default defineCampaign({id:'two_stage',entry:'one.map',missions:[
 {file:'one.map',source:'maps/one.map',script:'one/mission.ts',title:'第一关：守住阵地🚀',briefing:'等待增援',next:'two.map'},
 {file:'two.map',source:'maps/two.map',title:'第二关',endOfGame:true}
]});"#;
    let mut compiler = Compiler::new().unwrap();
    let files = BTreeMap::from([("/mission.ts".into(), source.into())]);
    let campaign = compiler.build_campaign(files.clone()).unwrap();
    assert_eq!(campaign.project.strings.len(), 3);
    assert_eq!(campaign.scripts["one.map"], "one/mission.ts");
    assert_eq!(
        campaign.transitions["one.map"].next.as_deref(),
        Some("two.map")
    );
    assert!(compiler.build(files).is_err()); // cached output still decodes to requested category
    for invalid in [
        source.replace("maps/one.map", "../one.map"),
        source.replace("next:'two.map'", "next:'missing.map'"),
        source.replace("endOfGame:true", "endOfGame:true,next:'one.map'"),
    ] {
        assert!(
            compiler
                .build_campaign(BTreeMap::from([("/mission.ts".into(), invalid)]))
                .is_err()
        );
    }
}

#[test]
fn entry_modules_preserve_shared_import_paths_and_cache_identity() {
    let mut files = BTreeMap::from([("/shared.ts".into(), "export const delay=30;".into())]);
    for id in ["first", "second"] {
        files.insert(format!("/{id}/mission.ts"),format!(r#"import {{defineMission,event,action}} from '../sdk';
import {{delay}} from '../shared';
export default defineMission({{houses:['Neutral'],waypoints:[],units:[],objects:[],strings:[]}},{{id:'{id}',player:'Neutral'}},m=>{{
 m.rule('timer',{{when:event.sinceEnabled(delay),once:true,actions:[action.winPlayer()]}});
}});"#));
    }
    let mut compiler = Compiler::new().unwrap();
    for id in ["first", "second", "first"] {
        let plan = compiler
            .build_at(files.clone(), &format!("/{id}/mission.ts"))
            .unwrap();
        assert_eq!(plan.id, id);
        assert!(matches!(
            plan.rules[0].when,
            Event::EnabledTime { seconds: 30 }
        ));
    }
}

#[test]
fn native_float_bits_packed_script_operands_and_house_indices_round_trip() {
    use crate::catalog::ResourceKind;
    let (mut doc, mut catalog) = fixture();
    doc.set_ini("Houses", "7", Some(b"SourceHouse".to_vec()))
        .unwrap();
    doc.set_ini("SourceHouse", "Country", Some(b"Russians".to_vec()))
        .unwrap();
    let record = doc.add_record("Units", [2, 2], "Neutral", "E1").unwrap();
    let key = doc.records()[&record].key.clone();
    doc.commit("source house and target");
    catalog = Catalog::from_document(&doc, catalog.units, vec![]);
    catalog.register(
        ResourceKind::Building,
        vec!["Z_BUILDING".into(), "A_BUILDING".into()],
    );
    catalog.register(
        ResourceKind::Animation,
        vec!["Z_ANIM".into(), "A_ANIM".into()],
    );
    catalog.register(
        ResourceKind::SuperWeapon,
        vec!["Z_SUPER".into(), "A_SUPER".into()],
    );
    let source = format!(
        r#"import {{defineMission,event,action,order,BuildingSelection}} from './sdk';
export default defineMission({schema},{{id:'bits',player:'Neutral'}},m=>{{
 m.script('packed',[order.attackEnemyBuilding(m.building('A_BUILDING'),BuildingSelection.Farthest),order.playAnimation(m.animation('A_ANIM'),65535)]);
 m.rule('effects',{{when:event.damagedByHouse(m.object('Units:{key}'),m.house('SourceHouse')),once:true,
  actions:[action.setAmbientStep(0.25),action.setAmbientRate(1.5),action.setSuperCharge(m.superWeapon('A_SUPER'),25),action.createBuilding(m.building('A_BUILDING'),m.waypoint(0))]}});
}});"#,
        schema = serde_json::to_string(&catalog).unwrap()
    );
    let plan = Compiler::new()
        .unwrap()
        .build(BTreeMap::from([("/mission.ts".into(), source)]))
        .unwrap();
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let map =
        vera20k_core::map::map_file::load_from_bytes(doc.snapshot().bytes().unwrap()).unwrap();
    let trigger = &mission::ids(&doc, Kind::Trigger)[0];
    let a = map.actions[trigger].entries();
    assert_eq!(a[0].literal_value(), Some(0.25f32.to_bits() as i32));
    assert_eq!(a[1].literal_value(), Some(1.5f32.to_bits() as i32));
    assert_eq!(a[2].params[1], "1");
    assert_eq!(a[2].params[6], "25");
    assert_eq!(a[3].params[1], "A_BUILDING");
    assert_eq!(a[3].waypoint_index, Some(0));
    let script = &mission::ids(&doc, Kind::Script)[0];
    assert_eq!(doc.ini_text(script, "0"), format!("46,{}", (3 << 16) | 1));
    assert_eq!(doc.ini_text(script, "1"), "51,-65535");
    assert_eq!(doc.ini_text("Events", trigger), "1,44,0,0");
    let mut multiplayer = plan;
    multiplayer.mode = MissionMode::Multiplayer;
    multiplayer.player.clear();
    assert!(prepare(&doc, &multiplayer, &catalog).is_err());
}

#[test]
fn meteor_shower_rejects_values_outside_the_four_native_intensities() {
    let (doc, catalog) = fixture();
    let mut compiler = Compiler::new().unwrap();
    for intensity in [0, 3, -1, 4] {
        let source = format!(
            r#"import {{defineMission,event,action}} from './sdk';
import {{map,House}} from './map.generated';
export default defineMission(map,{{id:'meteor',player:House.Neutral}},m=>{{
 m.rule('impact',{{when:event.sinceScenarioStart(1),once:true,actions:[action.meteorShower(m.waypoint(0),{intensity} as any)]}});
}});"#
        );
        let mission = compiler
            .build(BTreeMap::from([
                ("/mission.ts".into(), source),
                ("/map.generated.ts".into(), catalog.typescript().unwrap()),
            ]))
            .unwrap();
        let result = prepare(&doc, &mission, &catalog);
        if (0..=3).contains(&intensity) {
            result.unwrap();
        } else {
            assert!(
                result.err().unwrap().to_string().contains("0～3"),
                "unsupported intensity {intensity} must fail before play"
            );
        }
    }
}

#[test]
fn camera_velocity_rejects_values_outside_the_clients_five_native_rates() {
    let (doc, catalog) = fixture();
    for declaration in [
        "m.rule('camera',{when:event.sinceScenarioStart(1),once:true,actions:[action.centerCamera(m.waypoint(0),__VELOCITY__)]});",
        "m.team('camera',{house:m.house('Neutral'),waypoint:m.waypoint(0),units:[{type:m.unit('E1'),count:1}],orders:[order.centerCamera(__VELOCITY__)]});",
    ] {
        for velocity in [4, 5, -1] {
            let source = format!(
                "import {{defineMission,event,action,order}} from './sdk'; export default defineMission({},{{id:'camera',player:'Neutral'}},m=>{{{}}});",
                serde_json::to_string(&catalog).unwrap(),
                declaration.replace("__VELOCITY__", &velocity.to_string()),
            );
            let mission = Compiler::new()
                .unwrap()
                .build(BTreeMap::from([("/mission.ts".into(), source)]))
                .unwrap();
            let result = prepare(&doc, &mission, &catalog);
            if velocity == 4 {
                assert!(result.is_ok(), "{declaration}");
            } else {
                let error = result
                    .err()
                    .expect("the client rejects this camera velocity");
                assert!(error.to_string().contains("0～4"), "{error}");
            }
        }
    }
}

#[test]
fn extended_conditions_and_camera_actions_validate_shared_native_state() {
    let (mut doc, mut catalog) = fixture();
    catalog.register(crate::catalog::ResourceKind::Techno, vec!["E1".into()]);
    let source = r#"
import {defineMission,event,action,RadarEvent,SidebarTab} from './sdk';
import {map,House,Techno} from './map.generated';
export default defineMission(map,{id:'extended',player:House.Neutral},m=>{
  m.rule('clock',{when:event.all(event.randomTime(2),event.creditsAtLeast(0),event.noFactories()),once:false,
    actions:[action.centerCamera(m.waypoint(0),4),action.jumpCamera(m.waypoint(0)),
      action.visibleMapArea(0,0,2,2),action.radarEvent(RadarEvent.BeaconPlaced,m.waypoint(0)),
      action.sidebarTab(SidebarTab.Infantry),action.flashCameo(m.techno(Techno.E1),30),action.homeCamera()]});
});"#;
    let mut mission = Compiler::new()
        .unwrap()
        .build(BTreeMap::from([
            ("/mission.ts".into(), source.into()),
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
        ]))
        .unwrap();
    prepare(&doc, &mission, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let id = &mission::ids(&doc, Kind::Trigger)[0];
    assert_eq!(doc.ini_text("Events", id), "3,51,0,2,12,0,0,17,0,0");
    // Decode through the engine reader: field type 9 has a different scalar slot
    // from camera/radar, and the rectangle has four independent raw fields.
    let map =
        vera20k_core::map::map_file::load_from_bytes(doc.snapshot().bytes().unwrap()).unwrap();
    let actions = map.actions[id].entries();
    assert_eq!(actions[0].literal_value(), Some(4));
    assert_eq!(actions[0].waypoint_index, Some(0));
    assert_eq!(actions[5].literal_value(), Some(30));
    assert_eq!(&actions[2].params[2..6], ["0", "0", "2", "2"]);
    assert!(
        !prepare(&doc, &mission, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap()
    );
    mission.rules[0].when = serde_json::from_value(serde_json::json!({"kind":"all","events":[
        {"kind":"enabledTime","seconds":2},{"kind":"randomTime","seconds":2}
    ]}))
    .unwrap();
    assert!(
        prepare(&doc, &mission, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("一个独立计时器")
    );
    mission.rules[0].when = Event::RandomTime {
        seconds: 95_443_718,
    };
    assert!(prepare(&doc, &mission, &catalog).is_err());
    mission.rules[0].when = serde_json::from_value(serde_json::json!({"kind":"all","events":[
        {"kind":"halfHealth","object":{"kind":"object","id":"Infantry:0"}},
        {"kind":"firstDamaged","object":{"kind":"object","id":"Units:0"}}
    ]}))
    .unwrap();
    assert!(
        prepare(&doc, &mission, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("多个附着对象")
    );
    mission.rules[0].when = Event::CreditsAtLeast { amount: 0 };
    mission.rules[0].actions = vec![Action::VisibleMapArea {
        x: 0,
        y: 0,
        width: 4,
        height: 2,
    }];
    assert!(prepare(&doc, &mission, &catalog).is_err());
}

#[test]
fn rule_lifecycle_globals_and_all_validate_and_preserve_native_content() {
    let (mut doc, mut catalog) = fixture();
    doc.set_ini("VariableNames", "0", Some(b"manual,1".to_vec()))
        .unwrap();
    doc.commit("manual flag");
    catalog.register(
        crate::catalog::ResourceKind::Country,
        vec!["Russians".into()],
    );
    let source = r#"
import {defineMission, event, action, Difficulty} from './sdk';
import {map, House, Country} from './map.generated';
export default defineMission(map, {id:'control', player:House.Neutral}, m => {
    const ready = m.flag('ready');
    const old = m.localFlag(0), shared = m.globalFlag(49);
    m.rule('root', {when:event.all(event.sinceEnabled(2), event.all(event.globalSet(shared), event.flagSet(old))), once:false,
      owner:m.country(Country.Russians), enabled:false, difficulties:[Difficulty.Easy, Difficulty.Hard],
      linked:m.ruleRef('tail'), transferOnCapture:true,
      actions:[action.setGlobal(shared), action.clearGlobal(shared), action.setFlag(old), action.clearFlag(old),
        action.setFlag(ready), action.enableRule(m.ruleRef('tail')), action.disableRule(m.ruleRef('tail')), action.forceRule(m.ruleRef('tail'))]});
    m.rule('tail', {when:event.flagSet(ready), once:false, actions:[action.winPlayer()]});
});"#;
    let plan = Compiler::new()
        .unwrap()
        .build(BTreeMap::from([
            ("/mission.ts".into(), source.into()),
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
        ]))
        .unwrap();
    let original = doc.snapshot().bytes().unwrap();
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let ids = mission::ids(&doc, Kind::Trigger);
    let root = ids
        .iter()
        .find(|id| doc.ini_text("Triggers", id).contains(",root,"))
        .unwrap();
    let tail = ids
        .iter()
        .find(|id| doc.ini_text("Triggers", id).contains(",tail,"))
        .unwrap();
    assert_eq!(
        doc.ini_text("Triggers", root),
        format!("Russians,{tail},root,1,1,0,1,1")
    );
    assert_eq!(doc.ini_text("Events", root), "3,13,0,2,27,0,49,36,0,0");
    assert_eq!(doc.ini_text("VariableNames", "0"), "manual,1");
    assert_eq!(doc.ini_text("VariableNames", "1"), "control_ready,0");
    assert!(doc.ini_entries("GlobalVariables").is_empty());
    assert_eq!(
        mission::ids(&doc, Kind::Tag).len(),
        1,
        "linked triggers share one Tag chain"
    );
    let actions = mission::decode_list(&doc.ini_text("Actions", root), false).unwrap();
    assert_eq!(
        actions
            .iter()
            .map(|row| row[0].as_str())
            .collect::<Vec<_>>(),
        ["28", "29", "56", "57", "56", "53", "54", "22"]
    );
    assert_eq!(actions.last().unwrap()[2], *tail);
    assert!(
        !prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap()
    );
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), original);
    assert!(doc.redo());
    let mut deleted = plan;
    deleted.rules.clear();
    deleted.flags.clear();
    prepare(&doc, &deleted, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert_eq!(doc.ini_text("VariableNames", "0"), "manual,1");
    assert!(doc.ini_text("VariableNames", "1").is_empty());
}

#[test]
fn rule_graph_and_condition_validation_reject_unsafe_or_ambiguous_ir() {
    let (doc, catalog) = fixture();
    let base = serde_json::to_value(plan()).unwrap();
    let cases = [
        (serde_json::json!({"kind":"all", "events":[]}), "空条件"),
        (
            serde_json::json!({"kind":"all", "events":[{"kind":"enabledTime","seconds":1},{"kind":"enabledTime","seconds":2}]}),
            "一个 sinceEnabled",
        ),
        (
            serde_json::json!({"kind":"globalSet", "flag":{"kind":"globalFlag","id":50}}),
            "编号/类别",
        ),
        (
            serde_json::json!({"kind":"localSet", "flag":{"kind":"localFlag","id":99}}),
            "尚无 VariableNames",
        ),
        (
            serde_json::json!({"kind":"globalClear", "flag":{"kind":"localFlag","id":1}}),
            "编号/类别",
        ),
        (
            serde_json::json!({"kind":"all", "events":(0..33).map(|_| serde_json::json!({"kind":"countdownElapsed"})).collect::<Vec<_>>()}),
            "32",
        ),
    ];
    for (when, expected) in cases {
        let mut value = base.clone();
        value["rules"][0]["when"] = when;
        let plan: Mission = serde_json::from_value(value).unwrap();
        let error = prepare(&doc, &plan, &catalog).err().unwrap().to_string();
        assert!(error.contains(expected), "{error}");
    }
    for (patch, expected) in [
        (
            serde_json::json!({"linked":{"kind":"rule","id":"win"}}),
            "循环",
        ),
        (
            serde_json::json!({"actions":[{"kind":"forceRule","rule":{"kind":"rule","id":"win"}}]}),
            "递归",
        ),
        (
            serde_json::json!({"actions":[{"kind":"enableRule","rule":{"kind":"rule","id":"missing"}}]}),
            "未定义",
        ),
        (
            serde_json::json!({"owner":{"kind":"house","id":"Neutral"}}),
            "应为 country",
        ),
        (
            serde_json::json!({"difficulties":["easy","easy"]}),
            "难度重复",
        ),
    ] {
        let mut value = base.clone();
        for (key, item) in patch.as_object().unwrap() {
            value["rules"][0][key] = item.clone();
        }
        let plan: Mission = serde_json::from_value(value).unwrap();
        let error = prepare(&doc, &plan, &catalog).err().unwrap().to_string();
        assert!(error.contains(expected), "{error}");
    }
    // Even cached acyclic subgraphs must respect the expanded action budget.
    let mut value = base;
    let mut rules = Vec::new();
    for i in 0..14 {
        let actions = if i == 13 {
            serde_json::json!([{"kind":"winPlayer"}])
        } else {
            let action = serde_json::json!({"kind":"forceRule","rule":{"kind":"rule","id":format!("r{}",i+1)}});
            serde_json::json!([action, action])
        };
        rules.push(serde_json::json!({"id":format!("r{i}"),"when":{"kind":"countdownElapsed"},"once":false,"actions":actions}));
    }
    value["rules"] = serde_json::Value::Array(rules);
    let plan: Mission = serde_json::from_value(value).unwrap();
    assert!(
        prepare(&doc, &plan, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("预算")
    );
}

#[test]
fn explicit_tags_share_object_cell_and_team_bindings_and_restore_transactionally() {
    let (mut doc, _) = fixture();
    let object = doc.add_record("Units", [2, 2], "Neutral", "HTNK").unwrap();
    let object_key = format!("Units:{}", doc.records()[&object].key);
    doc.commit("object");
    let catalog = Catalog::from_document(&doc, vec!["HTNK".into()], vec![]);
    let source = format!(
        r#"
import {{defineMission, event, action, order, TagRepeat}} from './sdk';
import {{map, House, Unit, Waypoint}} from './map.generated';
export default defineMission(map, {{id:'tags', player:House.Neutral}}, m => {{
    const shared = m.tag('shared', {{rule:m.ruleRef('head'), repeat:TagRepeat.Repeat}});
    m.bindObject(m.object('{object_key}'), shared); m.bindCell(2, 2, m.tagRef('shared'));
    m.team('reinforcements', {{house:m.house(House.Neutral), waypoint:m.waypoint(Waypoint.WP0),
      units:[{{type:m.unit(Unit.HTNK),count:1}}], orders:[order.hunt()], tag:shared}});
    m.rule('head', {{when:event.destroyed(m.object('{object_key}')), once:false, linked:m.ruleRef('tail'), actions:[action.winPlayer()]}});
    m.rule('tail', {{when:event.destroyed(m.object('{object_key}')), once:false, actions:[action.losePlayer()]}});
}});"#
    );
    let plan = Compiler::new()
        .unwrap()
        .build(BTreeMap::from([
            ("/mission.ts".into(), source),
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
        ]))
        .unwrap();
    let original = doc.snapshot().bytes().unwrap();
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let tags = mission::ids(&doc, Kind::Tag);
    assert_eq!(tags.len(), 1);
    let tag = &tags[0];
    let key = mission::cell_key(&doc, [2, 2]).unwrap();
    assert_eq!(doc.ini_text("CellTags", &key), *tag);
    assert_eq!(
        doc.records()
            .into_values()
            .find(|r| format!("{}:{}", r.section, r.key) == object_key)
            .unwrap()
            .fields()[7],
        *tag
    );
    let team = &mission::ids(&doc, Kind::Team)[0];
    assert_eq!(doc.ini_text(team, "Tag"), *tag);
    assert!(
        !prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap()
    );
    let bytes = doc.snapshot().bytes().unwrap();
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), original);
    assert!(doc.redo());
    assert_eq!(doc.snapshot().bytes().unwrap(), bytes);
    // Conflicting manual edits fail before any object/CellTag is touched.
    doc.set_ini("CellTags", &key, Some(b"manual".to_vec()))
        .unwrap();
    doc.commit("manual cell");
    let before = doc.snapshot().bytes().unwrap();
    assert!(prepare(&doc, &plan, &catalog).is_err());
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
    assert!(doc.undo());
    let mut deleted = plan;
    deleted.rules.clear();
    deleted.tags.clear();
    deleted.bindings.clear();
    deleted.teams.clear();
    prepare(&doc, &deleted, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert!(mission::ids(&doc, Kind::Tag).is_empty());
    assert!(doc.ini_text("CellTags", &key).is_empty());
    assert_eq!(
        doc.records()
            .into_values()
            .find(|r| format!("{}:{}", r.section, r.key) == object_key)
            .unwrap()
            .fields()[7],
        "None"
    );
}

#[test]
fn explicit_tags_reject_wrong_roots_repeats_and_existing_bindings() {
    let (mut doc, catalog) = fixture();
    let base = serde_json::to_value(plan()).unwrap();
    for (tags, expected) in [
        (
            serde_json::json!([{"id":"a","rule":{"kind":"rule","id":"win"},"repeat":2}]),
            "once 不一致",
        ),
        (
            serde_json::json!([{"id":"a","rule":{"kind":"rule","id":"win"},"repeat":3}]),
            "重复方式",
        ),
        (
            serde_json::json!([{"id":"a","rule":{"kind":"team","id":"win"}}]),
            "应为 rule",
        ),
        (
            serde_json::json!([{"id":"a","rule":{"kind":"rule","id":"win"}},{"id":"b","rule":{"kind":"rule","id":"win"}}]),
            "已有显式 Tag",
        ),
    ] {
        let mut value = base.clone();
        value["tags"] = tags;
        let plan: Mission = serde_json::from_value(value).unwrap();
        let error = prepare(&doc, &plan, &catalog).err().unwrap().to_string();
        assert!(error.contains(expected), "{error}");
    }
    let mut value = base;
    value["tags"] = serde_json::json!([{"id":"a","rule":{"kind":"rule","id":"win"}}]);
    value["bindings"] =
        serde_json::json!([{"kind":"cell","x":2,"y":2,"tag":{"kind":"tag","id":"a"}}]);
    let plan: Mission = serde_json::from_value(value.clone()).unwrap();
    let key = mission::cell_key(&doc, [2, 2]).unwrap();
    doc.set_ini("CellTags", &key, Some(b"handmade".to_vec()))
        .unwrap();
    doc.commit("manual");
    assert!(
        prepare(&doc, &plan, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("手工 CellTag")
    );
    doc.undo();
    value["bindings"][0]["x"] = serde_json::json!(999);
    let plan: Mission = serde_json::from_value(value).unwrap();
    assert!(prepare(&doc, &plan, &catalog).is_err());
}

#[test]
fn any_helpers_rebuild_remove_and_reject_unsupported_combinations() {
    let (mut doc, catalog) = fixture();
    let mut value = serde_json::to_value(plan()).unwrap();
    value["rules"][0]["when"] = serde_json::json!({"kind":"any","events":[{"kind":"scenarioTime","seconds":1},{"kind":"scenarioTime","seconds":2}]});
    let plan: Mission = serde_json::from_value(value.clone()).unwrap();
    let build = prepare(&doc, &plan, &catalog).unwrap();
    assert_eq!(build.rule_count, 1);
    build.apply(&mut doc).unwrap();
    assert_eq!(mission::ids(&doc, Kind::Trigger).len(), 3);
    assert_eq!(mission::ids(&doc, Kind::Variable).len(), 1);
    assert!(
        !prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap()
    );
    let mut deleted = plan;
    deleted.rules.clear();
    prepare(&doc, &deleted, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert!(mission::ids(&doc, Kind::Trigger).is_empty());
    assert!(mission::ids(&doc, Kind::Variable).is_empty());
    value["rules"][0]["once"] = serde_json::json!(false);
    value["rules"][0]["when"]["events"] = serde_json::json!([{"kind":"tagNotification"}]);
    let plan: Mission = serde_json::from_value(value).unwrap();
    assert!(
        prepare(&doc, &plan, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("重复 any")
    );
}

#[test]
fn expanded_events_media_and_team_actions_use_native_parameter_layouts() {
    use crate::catalog::ResourceKind;
    let (mut doc, mut catalog) = fixture();
    for (kind, name) in [
        (ResourceKind::Sound, "Alarm"),
        (ResourceKind::Theme, "Track"),
        (ResourceKind::Eva, "EVA_Ready"),
        (ResourceKind::Techno, "E1"),
    ] {
        catalog.register(kind, vec![name.into()]);
    }
    let source = r#"
import {defineMission, event, action, order, Crate} from './sdk';
import {map, House, Unit, Waypoint, Sound, Theme, Eva, Techno} from './map.generated';
export default defineMission(map, {id:'expanded', player:House.Neutral}, m => {
    const team = m.team('team', {house:m.house(House.Neutral), waypoint:m.waypoint(Waypoint.WP0), units:[{type:m.unit(Unit.E1),count:1}], orders:[order.hunt()]});
    m.rule('run', {when:event.all(event.technoExists(m.techno(Techno.E1),2),event.technoAbsent(m.techno(Techno.E1)),event.tagNotification(),event.bridgeDestroyed()), once:true,
      actions:[action.playSound(m.sound(Sound.Alarm)), action.playMusic(m.theme(Theme.Track)), action.playEva(m.eva(Eva.EVA_Ready)),
        action.deleteTeam(team), action.reinforceDefault(team), action.chronoReinforce(team,m.waypoint(Waypoint.WP0)), action.flashTeam(team,-15),
        action.revealMap(), action.dropZone(m.waypoint(Waypoint.WP0)), action.createCrate(Crate.Veteran,m.waypoint(Waypoint.WP0))]});
});"#;
    let plan = Compiler::new()
        .unwrap()
        .build(BTreeMap::from([
            ("/mission.ts".into(), source.into()),
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
        ]))
        .unwrap();
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let trigger = &mission::ids(&doc, Kind::Trigger)[0];
    assert_eq!(
        doc.ini_text("Events", trigger),
        "4,60,2,2,E1,61,2,0,E1,8,0,0,31,0,0"
    );
    let actions = mission::decode_list(&doc.ini_text("Actions", trigger), false).unwrap();
    assert_eq!(
        actions.iter().map(|r| r[0].as_str()).collect::<Vec<_>>(),
        ["19", "20", "21", "5", "7", "107", "104", "16", "8", "108"]
    );
    assert_eq!(&actions[0][..3], &["19", "7", "Alarm"]);
    assert_eq!(&actions[1][..3], &["20", "8", "Track"]);
    assert_eq!(&actions[2][..3], &["21", "6", "EVA_Ready"]);
    assert_eq!(actions[6][1], "5");
    assert_eq!(actions[6][7], "-15");
    assert_eq!(actions[9][2], "14");
    let map =
        vera20k_core::map::map_file::load_from_bytes(doc.snapshot().bytes().unwrap()).unwrap();
    let native_actions = map.actions[trigger].entries();
    assert_eq!(native_actions[5].waypoint_index, Some(0));
    assert_eq!(map.events[trigger].conditions().last().unwrap().value, 2);
    assert_eq!(
        map.events[trigger]
            .conditions()
            .last()
            .unwrap()
            .type_name
            .as_deref(),
        Some("E1")
    );
}

#[test]
fn talk_bubble_orders_compile_to_the_pinned_team_script_handler() {
    let (mut doc, catalog) = fixture();
    let source = r#"import {defineMission, order} from './sdk';
import {map, House} from './map.generated';
export default defineMission(map,{id:'bubbles',player:House.Neutral},m=>{
  m.script('talk',[order.talkBubble(1),order.talkBubble(3),order.talkBubble(0)]);
});"#;
    let plan = Compiler::new()
        .unwrap()
        .build(BTreeMap::from([
            ("/mission.ts".into(), source.into()),
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
        ]))
        .unwrap();
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let ini =
        vera20k_core::rules::ini_parser::IniFile::from_bytes(&doc.snapshot().bytes().unwrap())
            .unwrap();
    let registry = vera20k_core::rules::team_ai_ini::TeamAiIniRegistry::from_sources(
        &vera20k_core::rules::ini_parser::IniFile::empty(),
        &ini,
        false,
    );
    let actions = &registry.scripts[0].actions;
    assert_eq!(
        actions
            .iter()
            .map(|a| (a.action_id, a.argument))
            .collect::<Vec<_>>(),
        [(52, 1), (52, 3), (52, 0)]
    );
    let mut invalid = serde_json::to_value(plan).unwrap();
    invalid["scripts"][0]["orders"][0]["bubble"] = serde_json::json!(4);
    let invalid: Mission = serde_json::from_value(invalid).unwrap();
    assert!(prepare(&doc, &invalid, &catalog).is_err());
}

#[test]
fn script_labels_media_ordinals_and_transport_commands_preserve_native_semantics() {
    use crate::catalog::ResourceKind;
    let (mut doc, mut catalog) = fixture();
    for kind in [
        ResourceKind::Sound,
        ResourceKind::Theme,
        ResourceKind::Movie,
    ] {
        catalog.register(kind, vec!["Z_FIRST".into(), "A_SECOND".into()]);
    }
    let source = r#"
import {defineMission, order, Quarry, UnitMission, UnloadMode, Direction} from './sdk';
import {map, House, Unit, Waypoint, Sound, Theme, Movie} from './map.generated';
export default defineMission(map,{id:'orders',player:House.Neutral},m=>{
  const flag = m.flag('ready');
  m.script('shared', [order.load(),order.waitLoaded(),order.moveToCell(2,2),order.unload(UnloadMode.ReleaseTransports),
    order.label('patrol'),order.guard(2),order.face(Direction.West),order.mission(UnitMission.Hunt),order.attack(Quarry.Power),
    order.playSound(m.sound(Sound.A_SECOND)),order.playMusic(m.theme(Theme.A_SECOND)),order.playMovie(m.movie(Movie.A_SECOND)),
    order.setFlag(flag),order.clearFlag(flag),order.setGlobal(m.globalFlag(4)),order.clearGlobal(m.globalFlag(4)),order.flash(-15),order.jump('patrol')]);
  m.team('team',{house:m.house(House.Neutral),waypoint:m.waypoint(Waypoint.WP0),units:[{type:m.unit(Unit.E1),count:1}],script:m.scriptRef('shared')});
});"#;
    let files = BTreeMap::from([
        ("/mission.ts".into(), source.into()),
        ("/map.generated.ts".into(), catalog.typescript().unwrap()),
    ]);
    let mut compiler = Compiler::new().unwrap();
    let plan = compiler.build(files.clone()).unwrap();
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let script = mission::ids(&doc, Kind::Script)[0].clone();
    assert_eq!(doc.ini_text(&script, "2"), "4,258");
    assert_eq!(doc.ini_text(&script, "3"), "8,2");
    assert_eq!(doc.ini_text(&script, "8"), "25,1");
    assert_eq!(doc.ini_text(&script, "9"), "27,1");
    assert_eq!(doc.ini_text(&script, "10"), "26,1");
    assert_eq!(doc.ini_text(&script, "15"), "50,-15");
    assert_eq!(doc.ini_text(&script, "16"), "6,5");
    // Production registry sees no pseudo label and gets an actual one-based destination.
    let ini =
        vera20k_core::rules::ini_parser::IniFile::from_bytes(&doc.snapshot().bytes().unwrap())
            .unwrap();
    let registry = vera20k_core::rules::team_ai_ini::TeamAiIniRegistry::from_sources(
        &vera20k_core::rules::ini_parser::IniFile::empty(),
        &ini,
        false,
    );
    let parsed = registry.scripts.iter().find(|s| s.id == script).unwrap();
    assert_eq!(parsed.actions.len(), 17);
    let mut inserted = files;
    inserted.insert(
        "/mission.ts".into(),
        source.replace(
            "order.label('patrol')",
            "order.scatter(),order.label('patrol')",
        ),
    );
    let plan = compiler.build(inserted).unwrap();
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert_eq!(doc.ini_text(&script, "17"), "6,6");
}

#[test]
fn script_validation_rejects_missing_trailing_labels_and_invalid_native_operands() {
    let (doc, catalog) = fixture();
    for orders in [
        serde_json::json!([{"kind":"jump","label":"missing"}]),
        serde_json::json!([{"kind":"hunt"},{"kind":"label","label":"end"}]),
        serde_json::json!([{"kind":"label","label":"dup"},{"kind":"label","label":"dup"},{"kind":"hunt"}]),
        serde_json::json!([{"kind":"mission","mission":32}]),
        serde_json::json!([{"kind":"face","direction":8}]),
        serde_json::json!([{"kind":"unload","mode":4}]),
        serde_json::json!([{"kind":"attack","quarry":12}]),
        serde_json::json!([{"kind":"moveToCell","x":128,"y":2}]),
    ] {
        let mut value = serde_json::to_value(plan()).unwrap();
        value["scripts"] = serde_json::json!([{"id":"test","orders":orders}]);
        let plan: Mission = serde_json::from_value(value).unwrap();
        assert!(prepare(&doc, &plan, &catalog).is_err());
    }
    let mut value = serde_json::to_value(plan()).unwrap();
    value["mode"] = serde_json::json!("multiplayer");
    value["player"] = serde_json::json!("");
    value["rules"] = serde_json::json!([]);
    value["scripts"] = serde_json::json!([{"id":"result","orders":[{"kind":"winPlayer"}]}]);
    let plan: Mission = serde_json::from_value(value).unwrap();
    assert!(
        prepare(&doc, &plan, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("仅支持单人")
    );
}

#[test]
fn change_script_registers_team_references_before_explicit_script_lists() {
    use vera20k_core::rules::{ini_parser::IniFile, team_ai_ini::TeamAiIniRegistry};
    let (mut doc, mut catalog) = fixture();
    let fixed = IniFile::from_str(
        "[TeamTypes]\n0=FIXED_TEAM\n[FIXED_TEAM]\nScript=IMPLICIT_FIXED\n[ScriptTypes]\n0=FIXED_Z\n1=FIXED_A\n[FIXED_Z]\n0=2,0\n[FIXED_A]\n0=2,0\n",
    );
    catalog.fixed_scripts = Some(vec!["FIXED_Z".into(), "FIXED_A".into()]);
    catalog.fixed_team_scripts = vec!["IMPLICIT_FIXED".into()];
    doc.set_ini("TeamTypes", "manual", Some(b"MANUAL_TEAM".to_vec()))
        .unwrap();
    doc.set_ini("MANUAL_TEAM", "Script", Some(b"IMPLICIT_MAP".to_vec()))
        .unwrap();
    doc.set_ini("ScriptTypes", "manual", Some(b"MANUAL".to_vec()))
        .unwrap();
    doc.set_ini("MANUAL", "0", Some(b"2,0".to_vec())).unwrap();
    doc.commit("manual script");
    let source = r#"
import {defineMission,order} from './sdk';
import {map,House,Unit,Waypoint} from './map.generated';
export default defineMission(map,{id:'change_script',player:House.Neutral},m=>{
  m.script('start',[order.changeScript(m.scriptRef('next'))]);
  m.script('next',[order.waitForever()]);
  m.team('team',{house:m.house(House.Neutral),waypoint:m.waypoint(Waypoint.WP0),units:[{type:m.unit(Unit.E1),count:1}],orders:[order.changeScript(m.scriptRef('next'))]});
});"#;
    let plan = Compiler::new()
        .unwrap()
        .build(BTreeMap::from([
            ("/mission.ts".into(), source.into()),
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
        ]))
        .unwrap();
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let ini = IniFile::from_bytes(&doc.snapshot().bytes().unwrap()).unwrap();
    let registry = TeamAiIniRegistry::from_sources(&fixed, &ini, false);
    let mut native_order = vec![];
    for id in registry
        .team_types
        .iter()
        .filter_map(|team| team.section().read_name("Script", 0x80))
        .chain(registry.scripts.iter().map(|script| script.id.as_str()))
    {
        if !native_order
            .iter()
            .any(|old: &&str| old.eq_ignore_ascii_case(id))
        {
            native_order.push(id);
        }
    }
    assert_eq!(native_order[0], "IMPLICIT_FIXED");
    assert!(
        native_order
            .iter()
            .position(|id| *id == "IMPLICIT_MAP")
            .unwrap()
            < native_order.iter().position(|id| *id == "FIXED_Z").unwrap()
    );
    let target = native_order
        .iter()
        .position(|id| doc.ini_text(id, "Name") == "next")
        .unwrap();
    assert!(target >= 5, "{native_order:?}");
    let target_in_explicit_list = registry
        .scripts
        .iter()
        .position(|script| doc.ini_text(&script.id, "Name") == "next")
        .unwrap();
    assert_ne!(target, target_in_explicit_list);
    let changing: Vec<_> = registry
        .scripts
        .iter()
        .filter(|script| {
            script
                .actions
                .first()
                .is_some_and(|action| action.action_id == 17)
        })
        .collect();
    assert_eq!(changing.len(), 2);
    assert!(
        changing
            .iter()
            .all(|script| script.actions[0].argument == target as i32)
    );
    let rebuilt = prepare(&doc, &plan, &catalog).unwrap();
    assert!(!rebuilt.apply(&mut doc).unwrap());
    let mut without_order = catalog.clone();
    without_order.fixed_scripts = None;
    assert!(
        prepare(&doc, &plan, &without_order)
            .err()
            .unwrap()
            .to_string()
            .contains("缺少固定 AIMD")
    );
}

#[test]
fn country_and_action_house_selectors_encode_their_distinct_native_identities() {
    let (mut doc, mut catalog) = fixture();
    catalog.countries = vec!["Americans".into(), "Russians".into()];
    catalog.fixed_country_indices =
        BTreeMap::from([("Americans".into(), 3), ("Russians".into(), 7)]);
    let source = r#"
import {defineMission,event,action,order} from './sdk';
import {map,House,Country} from './map.generated';
export default defineMission(map,{id:'selectors',player:House.Neutral},m=>{
  const country=m.country(Country.Russians), retained=m.retainedHouse();
  m.rule('selected',{when:event.all(event.entered(country),event.enteredZone(country)),once:true,
    actions:[action.startProduction(country),action.enableAi(retained),action.disableAi(country),action.allToHunt(retained),
      action.changeAttachedOwner(country),action.winCountry(country),action.loseCountry(country)]});
  m.script('change',[order.changeCountry(country)]);
});"#;
    let plan = Compiler::new()
        .unwrap()
        .build(BTreeMap::from([
            ("/mission.ts".into(), source.into()),
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
        ]))
        .unwrap();
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let trigger = &mission::ids(&doc, Kind::Trigger)[0];
    assert_eq!(doc.ini_text("Events", trigger), "2,1,0,7,24,0,7");
    let rows = mission::decode_list(&doc.ini_text("Actions", trigger), false).unwrap();
    assert_eq!(
        rows.iter().map(|r| r[0].as_str()).collect::<Vec<_>>(),
        ["3", "74", "75", "6", "14", "1", "2"]
    );
    assert_eq!(
        rows.iter().map(|r| r[2].as_str()).collect::<Vec<_>>(),
        ["7", "8997", "7", "8997", "7", "7", "7"]
    );
    let script = &mission::ids(&doc, Kind::Script)[0];
    assert_eq!(doc.ini_text(script, "0"), "20,7");
    let mut multiplayer = plan;
    multiplayer.mode = MissionMode::Multiplayer;
    multiplayer.player.clear();
    assert!(
        prepare(&doc, &multiplayer, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("仅支持单人")
    );
    multiplayer.rules[0].actions = vec![Action::StartProduction {
        house: ActionHouse::StartPlayer { id: 29 },
    }];
    catalog.waypoints.push(29);
    prepare(&doc, &multiplayer, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let trigger = &mission::ids(&doc, Kind::Trigger)[0];
    let selector = mission::decode_list(&doc.ini_text("Actions", trigger), false).unwrap()[0][2]
        .parse::<i32>()
        .unwrap();
    assert_eq!(
        vera20k_core::rules::native_processing::house_type_player_at_text(selector).as_deref(),
        Some("<Player @ AD>")
    );
    catalog.fixed_country_indices.clear();
    assert!(
        prepare(&doc, &multiplayer, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("注册顺序")
    );
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
fn countdown_encodes_native_controls_and_validates_time_and_title() {
    let (mut doc, catalog) = fixture();
    let mut plan = plan();
    plan.rules[0].when = Event::CountdownElapsed;
    plan.rules[0].actions = vec![
        Action::SetCountdown { seconds: 30 },
        Action::StopCountdown,
        Action::StartCountdown,
        Action::ExtendCountdown { seconds: 5 },
        Action::ShortenCountdown { seconds: 2 },
        Action::SetCountdownTitle {
            text: Reference {
                kind: "text".into(),
                id: "MISSION:TEXT".into(),
            },
        },
    ];
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let trigger = &mission::ids(&doc, Kind::Trigger)[0];
    assert_eq!(doc.ini_text("Events", trigger), "1,14,0,0");
    assert_eq!(
        doc.ini_text("Actions", trigger),
        "6,27,0,30,0,0,0,0,A,24,0,0,0,0,0,0,A,23,0,0,0,0,0,0,A,25,0,5,0,0,0,0,A,26,0,2,0,0,0,0,A,103,4,MISSION:TEXT,0,0,0,0,A"
    );
    for action in [
        Action::SetCountdown { seconds: u32::MAX },
        Action::ExtendCountdown { seconds: u32::MAX },
        Action::ShortenCountdown { seconds: u32::MAX },
        Action::SetCountdownTitle {
            text: Reference {
                kind: "house".into(),
                id: "Neutral".into(),
            },
        },
        Action::SetCountdownTitle {
            text: Reference {
                kind: "text".into(),
                id: "MISSING:TEXT".into(),
            },
        },
    ] {
        plan.rules[0].actions = vec![action];
        assert!(prepare(&doc, &plan, &catalog).is_err());
    }
    let mut oversized = catalog.clone();
    oversized.strings.push("T".repeat(32));
    plan.rules[0].actions = vec![Action::SetCountdownTitle {
        text: Reference {
            kind: "text".into(),
            id: "T".repeat(32),
        },
    }];
    assert!(prepare(&doc, &plan, &oversized).is_err());
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
        units: Some(vec![Unit {
            r#type: Reference {
                kind: "unit".into(),
                id: "E1".into(),
            },
            count: 3,
        }]),
        orders: Some(vec![
            Order::MoveTo {
                waypoint: Waypoint {
                    kind: "waypoint".into(),
                    id: 26,
                },
            },
            Order::Guard { seconds: 5 },
        ]),
        task_force: None,
        script: None,
        tag: None,
        max: None,
        priority: None,
        veteran_level: None,
        group: None,
        autocreate: None,
        full: None,
        recruiter: None,
        is_base_defense: None,
        suicide: None,
        aggressive: None,
        reinforce: None,
        are_team_members_recruitable: None,
        on_trans_only: None,
        avoid_threats: None,
        annoyance: None,
        guard_slower: None,
        transports_return_on_unload: None,
        mind_control_decision: None,
        droppod: None,
        only_target_house_enemy: None,
        loadable: None,
        whiner: None,
        use_transport_origin: None,
        transport_waypoint: None,
        waypoint: Waypoint {
            kind: "waypoint".into(),
            id: 0,
        },
    });
    let team_options = &mut plan.teams[0];
    team_options.max = Some(4);
    team_options.priority = Some(9);
    team_options.veteran_level = Some(3);
    team_options.group = Some(7);
    team_options.autocreate = Some(true);
    team_options.full = Some(false);
    team_options.reinforce = Some(false);
    team_options.droppod = Some(true);
    team_options.use_transport_origin = Some(true);
    team_options.transport_waypoint = Some(Waypoint {
        kind: "waypoint".into(),
        id: 26,
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
    assert_eq!(doc.ini_text(team, "Max"), "4");
    assert_eq!(doc.ini_text(team, "Priority"), "9");
    assert_eq!(doc.ini_text(team, "VeteranLevel"), "3");
    assert_eq!(doc.ini_text(team, "Group"), "7");
    assert_eq!(doc.ini_text(team, "Autocreate"), "yes");
    assert_eq!(doc.ini_text(team, "Full"), "no");
    assert_eq!(doc.ini_text(team, "Reinforce"), "no");
    assert_eq!(doc.ini_text(team, "Droppod"), "yes");
    assert_eq!(doc.ini_text(team, "UseTransportOrigin"), "yes");
    assert_eq!(doc.ini_text(team, "TransportWaypoint"), "AA");
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

#[test]
fn team_house_uses_effective_country_aliases_and_rejects_later_duplicate_house() {
    let (mut base, _) = fixture();
    for house in ["Neutral", "Other"] {
        mission::create(&mut base, Kind::House, house, house, None).unwrap();
    }
    base.commit("two distinct House identities");
    let mut catalog = Catalog::from_document(&base, vec!["E1".into()], vec![]);
    catalog.register(
        crate::catalog::ResourceKind::Country,
        vec!["Americans".into(), "Russians".into()],
    );
    catalog.fixed_country_indices =
        BTreeMap::from([("Americans".into(), 0), ("Russians".into(), 1)]);
    catalog.country_names = BTreeMap::from([
        ("americans".into(), "Americans".into()),
        ("first".into(), "Americans".into()),
        ("russians".into(), "Russians".into()),
        ("second".into(), "Russians".into()),
    ]);
    let source = r#"import {defineMission,order} from './sdk';
import {map,House,Unit,Waypoint} from './map.generated';
export default defineMission(map,{id:'country_owner',player:House.Neutral},m=>{
m.team('squad',{house:m.house(House.Neutral),waypoint:m.waypoint(Waypoint.WP0),units:[{type:m.unit(Unit.E1),count:1}],orders:[order.waitForever()]});
});"#;
    let compiled = Compiler::new()
        .unwrap()
        .build(BTreeMap::from([
            ("/mission.ts".into(), source.into()),
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
        ]))
        .unwrap();
    for (first, second, configure, owner, expected) in [
        (Some("FiRsT"), "Second", false, "Neutral", Some("FiRsT")),
        (Some("First"), "Second", false, "Other", Some("Second")),
        (Some("First"), "First", true, "Neutral", Some("Russians")),
        (None, "Second", false, "Neutral", Some("Americans")),
        (None, "Americans", false, "Other", None),
        (Some("First"), "Americans", false, "Other", None),
        (Some("Unknown"), "Second", false, "Neutral", None),
    ] {
        let mut doc = base.fork();
        doc.set_ini("Neutral", "Country", first.map(|s| s.as_bytes().to_vec()))
            .unwrap();
        doc.set_ini("Other", "Country", Some(second.as_bytes().to_vec()))
            .unwrap();
        doc.commit("effective Country");
        let before = doc.snapshot().bytes().unwrap();
        let mut value = serde_json::to_value(&compiled).unwrap();
        value["teams"][0]["house"]["id"] = serde_json::json!(owner);
        if configure {
            value["houseDefinitions"] =
                serde_json::json!([{"id":"Neutral","country":{"kind":"country","id":"Russians"}}]);
        }
        let plan: Mission = serde_json::from_value(value).unwrap();
        let prepared = prepare(&doc, &plan, &catalog);
        if let Some(expected) = expected {
            prepared.unwrap().apply(&mut doc).unwrap();
            let team = &mission::ids(&doc, Kind::Team)[0];
            assert_eq!(doc.ini_text(team, "House"), expected);
            assert!(
                !prepare(&doc, &plan, &catalog)
                    .unwrap()
                    .apply(&mut doc)
                    .unwrap()
            );
        } else {
            assert!(
                prepared.is_err(),
                "owner {owner}, Country {first:?}/{second}"
            );
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
        }
    }
}

#[test]
fn house_definitions_create_configure_and_restore_transactionally() {
    let (mut doc, _) = fixture();
    mission::create(&mut doc, Kind::House, "Neutral", "Neutral", None).unwrap();
    doc.add_record("Structures", [2, 2], "Neutral", "GACNST")
        .unwrap();
    doc.commit("house fixture");
    let mut catalog = Catalog::from_document(&doc, vec!["E1".into()], vec![]);
    catalog.register(
        crate::catalog::ResourceKind::Country,
        vec!["Americans".into(), "Russians".into()],
    );
    catalog.register(
        crate::catalog::ResourceKind::Building,
        vec!["GAPOWR".into(), "GACNST".into()],
    );
    catalog.house_colors = vec!["Gold".into(), "DarkRed".into()];
    let source = r#"
import {defineMission, event, action, order} from './sdk';
import {map, House, Country, HouseColor, Unit, Building, Waypoint} from './map.generated';
export default defineMission(map, {id:'house_defs', player:House.Neutral}, m => {
  const raiders = m.createHouse('raiders', {
    country:m.country(Country.Russians), color:HouseColor.DarkRed,
    playerControl:false, credits:750, iq:4, techLevel:6,
    allies:[m.house(House.Neutral)],
    basePlan:{percentBuilt:25, nodes:[
      {kind:'building', building:m.building(Building.GAPOWR), x:2, y:2},
      {kind:'control', control:-2, x:2, y:2},
    ]},
  });
  m.configureHouse(m.house(House.Neutral), {iq:2, allies:[raiders],
    basePlan:{nodes:[{kind:'building', building:m.building(Building.GACNST), x:2, y:2, rebuild:true}]}});
  m.team('raider_team', {house:raiders, waypoint:m.waypoint(Waypoint.WP0),
    units:[{type:m.unit(Unit.E1), count:2}], orders:[order.hunt()]});
  m.rule('win', {when:event.sinceScenarioStart(1), once:true,
    actions:[action.setAlternateBase(m.waypoint(Waypoint.WP0)), action.clearAlternateBase(), action.winPlayer()]});
});"#;
    let plan = Compiler::new()
        .unwrap()
        .build(BTreeMap::from([
            ("/mission.ts".into(), source.into()),
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
        ]))
        .unwrap();
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let raiders = mission::ids(&doc, Kind::House)
        .into_iter()
        .find(|id| id.starts_with("TSHS"))
        .unwrap();
    let map =
        vera20k_core::map::map_file::load_from_bytes(doc.snapshot().bytes().unwrap()).unwrap();
    let roster = vera20k_core::map::houses::parse_house_roster(&map.ini, &[], None);
    assert!(
        roster.houses.iter().any(|house| house.name == raiders),
        "the native Houses reader must retain the generated identity without truncation"
    );
    assert_eq!(doc.ini_text(&raiders, "Name"), "raiders");
    assert_eq!(doc.ini_text(&raiders, "Country"), "Russians");
    assert_eq!(doc.ini_text(&raiders, "Color"), "DarkRed");
    assert_eq!(doc.ini_text(&raiders, "Credits"), "750");
    assert_eq!(doc.ini_text(&raiders, "IQ"), "4");
    assert_eq!(doc.ini_text(&raiders, "TechLevel"), "6");
    assert_eq!(doc.ini_text(&raiders, "Allies"), "Neutral");
    assert_eq!(doc.ini_text(&raiders, "PercentBuilt"), "25");
    assert_eq!(doc.ini_text(&raiders, "NodeCount"), "2");
    assert_eq!(doc.ini_text(&raiders, "000"), "GAPOWR,2,2");
    assert_eq!(doc.ini_text(&raiders, "001"), "-2,2,2");
    assert_eq!(doc.ini_text("Neutral", "IQ"), "2");
    assert_eq!(doc.ini_text("Neutral", "Allies"), raiders);
    assert_eq!(doc.ini_text("Neutral", "NodeCount"), "1");
    assert_eq!(doc.ini_text("Neutral", "000"), "GACNST,2,2");
    let trigger = mission::ids(&doc, Kind::Trigger)[0].clone();
    let actions = mission::decode_list(&doc.ini_text("Actions", &trigger), false).unwrap();
    assert_eq!(
        actions
            .iter()
            .map(|row| row[0].as_str())
            .collect::<Vec<_>>(),
        ["137", "138", "67"]
    );
    assert_eq!(actions[0][7], "A");
    assert_eq!(
        doc.records()
            .values()
            .find(|record| record.section == "Structures" && record.type_id() == "GACNST")
            .unwrap()
            .fields()[8],
        "1"
    );
    let team = &mission::ids(&doc, Kind::Team)[0];
    assert_eq!(doc.ini_text(team, "House"), "Russians");
    let roster_index = doc
        .ini_fields("Houses")
        .into_iter()
        .find(|(_, value)| value == raiders.as_bytes())
        .unwrap()
        .0;
    let original = doc.snapshot().bytes().unwrap();
    assert!(
        !prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap()
    );
    let mut edited = doc.fork();
    edited
        .set_ini(&raiders, "Credits", Some(b"900".to_vec()))
        .unwrap();
    edited.commit("external House edit");
    assert!(prepare(&edited, &plan, &catalog).is_err());
    // Previously generated 20-byte House IDs must still restore, then rebuild
    // with the native-readable identity, including incoming Allies/Team refs.
    let mut legacy = doc.fork();
    let meta = "EditorMissionScript";
    let chunks: usize = legacy.ini_text(meta, "Chunks").parse().unwrap();
    let hex: String = (0..chunks)
        .map(|index| legacy.ini_text(meta, &format!("Data{index}")))
        .collect();
    let manifest: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    let old_id = format!("{raiders}F");
    let manifest: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
    for field in manifest["fields"].as_array().unwrap() {
        let section = field["section"].as_str().unwrap();
        let key = field["key"].as_str().unwrap();
        let value = field["value"].as_str().unwrap();
        let old_section = section.replace(&raiders, &old_id);
        if old_section != section {
            legacy.set_ini(section, key, None).unwrap();
        }
        legacy
            .set_ini(
                &old_section,
                key,
                Some(value.replace(&raiders, &old_id).into_bytes()),
            )
            .unwrap();
    }
    let manifest = serde_json::to_string(&manifest)
        .unwrap()
        .replace(&raiders, &old_id);
    let hex: String = manifest.bytes().map(|byte| format!("{byte:02x}")).collect();
    let chunks: Vec<_> = hex.as_bytes().chunks(400).collect();
    legacy
        .set_ini(meta, "Chunks", Some(chunks.len().to_string().into_bytes()))
        .unwrap();
    for (index, chunk) in chunks.iter().enumerate() {
        legacy
            .set_ini(meta, &format!("Data{index}"), Some(chunk.to_vec()))
            .unwrap();
    }
    legacy.commit("legacy 20-byte House fixture");
    prepare(&legacy, &plan, &catalog)
        .unwrap()
        .apply(&mut legacy)
        .unwrap();
    assert!(!mission::ids(&legacy, Kind::House).contains(&old_id));
    assert!(mission::ids(&legacy, Kind::House).contains(&raiders));
    assert_eq!(legacy.ini_text("Neutral", "Allies"), raiders);
    assert_eq!(legacy.ini_text(team, "House"), "Russians");
    let mut deleted = plan;
    deleted.house_definitions.clear();
    deleted.teams.clear();
    prepare(&doc, &deleted, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert!(!mission::ids(&doc, Kind::House).contains(&raiders));
    assert!(doc.ini_text("Houses", &roster_index).is_empty());
    assert_eq!(
        doc.records()
            .values()
            .find(|record| record.section == "Structures" && record.type_id() == "GACNST")
            .unwrap()
            .fields()[8],
        "0"
    );
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), original);
}

#[test]
fn ai_trigger_definitions_encode_team_condition_weights_and_enable_state() {
    let (mut doc, _) = fixture();
    doc.add_record("Units", [2, 2], "Neutral", "E1").unwrap();
    doc.commit("unique player-owned object");
    let mut catalog = Catalog::from_document(&doc, vec!["E1".into()], vec!["MISSION:TEXT".into()]);
    catalog.register(
        crate::catalog::ResourceKind::Country,
        vec!["Russians".into()],
    );
    catalog.register(crate::catalog::ResourceKind::Techno, vec!["E1".into()]);
    catalog.register(crate::catalog::ResourceKind::Movie, vec!["ALLWIN".into()]);
    let source = r#"
import {defineMission, MissionMode, event, action, order, AiTriggerCondition, AiTriggerComparison, Difficulty} from './sdk';
import {map, House, Unit, Techno, Country, Movie, MapObject, Waypoint} from './map.generated';
export default defineMission(map, {id:'ai_defs', mode:MissionMode.Multiplayer}, m => {
  const authoredWaypoint = m.createWaypoint(30, 2, 2);
  m.assignObjectToPlayer(m.object(MapObject.Units_0), 0);
  m.configureBasic({name:'AI scenario', homeCell:authoredWaypoint,
    win:m.movie(Movie.ALLWIN), endOfGame:true, skipScore:false,
    ignoreGlobalAiTriggers:true, timerInherit:true, carryOverMoneyPercent:25.5,
    carryOverCap:2000, nextScenario:'second.map'});
  const main = m.team('main', {house:m.house(House.Neutral), waypoint:m.waypoint(Waypoint.WP0),
    units:[{type:m.unit(Unit.E1), count:2}], orders:[order.hunt()]});
  const reserve = m.team('reserve', {house:m.house(House.Neutral), waypoint:m.waypoint(Waypoint.WP0),
    units:[{type:m.unit(Unit.E1), count:1}], orders:[order.hunt()]});
  m.aiTrigger('rush', {primary:main, secondary:reserve, owner:m.country(Country.Russians),
    condition:AiTriggerCondition.EnemyCredits, objectType:m.techno(Techno.E1),
    amount:1200, comparison:AiTriggerComparison.GreaterOrEqual,
    weight:80, minWeight:20, maxWeight:100, multiplayer:true, side:2,
    difficulties:[Difficulty.Easy, Difficulty.Hard], enabled:false});
  m.aiTrigger('always', {primary:main, owner:'all', condition:AiTriggerCondition.Always, enabled:false});
  m.rule('noop', {when:event.sinceScenarioStart(1), once:true, actions:[action.setCountdown(1)]});
});"#;
    let plan = Compiler::new()
        .unwrap()
        .build(BTreeMap::from([
            ("/mission.ts".into(), source.into()),
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
        ]))
        .unwrap();
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let id = mission::ids(&doc, Kind::AiTrigger)
        .into_iter()
        .find(|id| doc.ini_text("AITriggerTypes", id).starts_with("rush,"))
        .unwrap();
    let fields =
        ra2_map_document::ai_triggers::decode(&doc.ini_text("AITriggerTypes", &id)).unwrap();
    assert!(fields[1].starts_with("TSTM"));
    assert_eq!(fields[2], "Russians");
    assert_eq!(fields[4], "4");
    assert_eq!(fields[5], "E1");
    assert_eq!(
        ra2_map_document::ai_triggers::compare_words(&fields[6]).unwrap(),
        (1200, 3)
    );
    assert_eq!(&fields[7..10], &["80", "20", "100"]);
    assert_eq!(&fields[10..11], &["1"]);
    assert_eq!(&fields[12..14], &["2", "0"]);
    assert!(fields[14].starts_with("TSTM"));
    assert_eq!(&fields[15..18], &["1", "0", "1"]);
    assert!(doc.ini_value("AITriggerTypesEnable", &id).is_none());
    assert_eq!(doc.ini_text("Basic", "Name"), "AI scenario");
    assert_eq!(doc.ini_text("Basic", "HomeCell"), "30");
    assert_eq!(doc.ini_text("Basic", "Win"), "ALLWIN");
    assert_eq!(doc.ini_text("Basic", "EndOfGame"), "yes");
    assert_eq!(doc.ini_text("Basic", "SkipScore"), "no");
    let map =
        vera20k_core::map::map_file::load_from_bytes(doc.snapshot().bytes().unwrap()).unwrap();
    let basic = vera20k_core::map::basic::parse_basic_section(&map.ini);
    assert_eq!(basic.home_cell, Some(30));
    assert_eq!(basic.win.as_deref(), Some("ALLWIN"));
    assert_eq!(basic.ignore_global_ai_triggers, Some(true));
    assert_eq!(basic.timer_inherit, Some(true));
    assert_eq!(basic.carry_over_cap, Some(2000));
    assert_eq!(basic.next_scenario.as_deref(), Some("second.map"));
    assert_eq!(basic.carry_over_money.unwrap().bits(), 0.255f64.to_bits());
    for multiplayer in [false, true] {
        let registry = vera20k_core::rules::team_ai_ini::TeamAiIniRegistry::from_sources(
            &vera20k_core::rules::ini_parser::IniFile::from_str(""),
            &map.ini,
            multiplayer,
        );
        assert_eq!(registry.ai_triggers.len(), 2);
        assert!(registry.ai_triggers.iter().all(|trigger| !trigger.enabled));
        let always = registry
            .ai_triggers
            .iter()
            .find(|trigger| trigger.display_name == "always")
            .unwrap();
        assert_eq!(
            always.owner,
            vera20k_core::rules::team_ai_ini::AiTriggerOwnerIni::All
        );
    }
    let assigned_object = doc
        .records()
        .into_iter()
        .find(|(_, record)| record.section == "Units" && record.key == "0")
        .unwrap()
        .1;
    assert_eq!(
        ra2_map_document::startup_owner::get(&doc, &assigned_object),
        Some(0)
    );
    assert_eq!(
        doc.records()
            .values()
            .find(|record| record.section == "Waypoints" && record.key == "30")
            .and_then(|record| record.cell()),
        Some([2, 2])
    );
    assert!(
        !prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap(),
        "rebuilding the same generated waypoint must be a no-op"
    );
    let mut externally_moved = doc.fork();
    let waypoint_record = externally_moved
        .records()
        .into_iter()
        .find(|(_, record)| record.section == "Waypoints" && record.key == "30")
        .unwrap()
        .0;
    externally_moved
        .set(
            ra2_map_document::document::Target::Record(waypoint_record),
            None,
        )
        .unwrap();
    externally_moved.commit("delete generated waypoint");
    assert!(prepare(&externally_moved, &plan, &catalog).is_err());
    let mut manual_reference = doc.fork();
    manual_reference
        .set_ini("Basic", "AltHomeCell", Some(b"30".to_vec()))
        .unwrap();
    let mut removed_json = serde_json::to_value(&plan).unwrap();
    removed_json["startOwners"] = serde_json::json!([]);
    removed_json["waypoints"] = serde_json::json!([]);
    removed_json["basic"] = serde_json::Value::Null;
    let removed: Mission = serde_json::from_value(removed_json).unwrap();
    assert!(
        prepare(&manual_reference, &removed, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("AltHomeCell")
    );
    prepare(&doc, &removed, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert!(
        !doc.records()
            .values()
            .any(|record| record.section == "Waypoints" && record.key == "30")
    );
    let restored_object = doc
        .records()
        .into_iter()
        .find(|(_, record)| record.section == "Units" && record.key == "0")
        .unwrap()
        .1;
    assert_eq!(
        ra2_map_document::startup_owner::get(&doc, &restored_object),
        None
    );
    assert!(
        !mission::ids(&doc, Kind::Trigger)
            .iter()
            .any(|id| id.starts_with("EDSTART"))
    );
}

#[test]
fn scenario_definitions_reject_invalid_resources_coordinates_and_native_operands() {
    let (mut doc, _) = fixture();
    mission::create(&mut doc, Kind::House, "Neutral", "Neutral", None).unwrap();
    doc.commit("house");
    let mut catalog = Catalog::from_document(&doc, vec!["E1".into()], vec![]);
    catalog.register(
        crate::catalog::ResourceKind::Building,
        vec!["GAPOWR".into()],
    );
    let base = serde_json::to_value(plan()).unwrap();
    for definition in [
        serde_json::json!({"id":30,"x":500,"y":500}),
        serde_json::json!({"id":0,"x":2,"y":2}),
        serde_json::json!({"id":702,"x":2,"y":2}),
    ] {
        let mut value = base.clone();
        value["waypoints"] = serde_json::json!([definition]);
        let invalid: Mission = serde_json::from_value(value).unwrap();
        assert!(prepare(&doc, &invalid, &catalog).is_err());
    }
    for base_plan in [
        serde_json::json!({"nodes":[],"percentBuilt":101}),
        serde_json::json!({"nodes":[{"kind":"control","control":-4,"x":2,"y":2}]}),
        serde_json::json!({"nodes":[{"kind":"building","building":{"kind":"unit","id":"GAPOWR"},"x":2,"y":2}]}),
        serde_json::json!({"nodes":[{"kind":"building","building":{"kind":"building","id":"GAPOWR"},"x":500,"y":500}]}),
    ] {
        let mut value = base.clone();
        value["houseDefinitions"] = serde_json::json!([{"id":"Neutral","basePlan":base_plan}]);
        let invalid: Mission = serde_json::from_value(value).unwrap();
        assert!(prepare(&doc, &invalid, &catalog).is_err());
    }
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
fn repeating_reinforcements_preserve_templates_rebuilds_and_history() {
    let (mut doc, _) = fixture();
    doc.waypoint(20, [2, 2]).unwrap();
    doc.commit("entry waypoint");
    let catalog = Catalog::from_document(&doc, vec!["E1".into()], vec![]);
    let original = doc.snapshot().bytes().unwrap();
    let mut plan = gift_plan(0);
    plan.rules[0].once = false;
    plan.rules[0].when = Event::EnabledTime { seconds: 30 };
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let repeated = doc.snapshot().bytes().unwrap();
    let trigger = mission::ids(&doc, Kind::Trigger)[0].clone();
    let tag = mission::ids(&doc, Kind::Tag)[0].clone();
    assert_eq!(doc.ini_text("Tags", &tag), format!("2,give,{trigger}"));
    assert_eq!(doc.ini_text("Events", &trigger), "1,13,0,30");
    let team = &mission::ids(&doc, Kind::Team)[0];
    assert_eq!(
        doc.ini_text("Actions", &trigger),
        format!("1,80,1,{team},0,0,0,0,U")
    );
    assert_eq!(mission::ids(&doc, Kind::Team).len(), 1);
    assert!(
        !prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap()
    );
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), original);
    assert!(doc.redo());
    assert_eq!(doc.snapshot().bytes().unwrap(), repeated);

    // Changing repetition keeps record identities and does not duplicate teams.
    plan.rules[0].once = true;
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert_eq!(doc.ini_text("Tags", &tag), format!("0,give,{trigger}"));
    assert_eq!(mission::ids(&doc, Kind::Trigger), vec![trigger]);
    assert_eq!(mission::ids(&doc, Kind::Team).len(), 1);
}

#[test]
fn multiplayer_gifts_encode_all_native_start_selectors_and_preserve_history() {
    let (mut doc, _) = fixture();
    for slot in 0..30 {
        doc.waypoint(slot, [2, 2]).unwrap();
    }
    doc.waypoint(20, [2, 2]).unwrap();
    doc.set_ini("Basic", "Player", Some(b"Special".to_vec()))
        .unwrap();
    doc.commit("start positions");
    let catalog = Catalog::from_document(&doc, vec!["E1".into()], vec![]);
    for slot in 0..30 {
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
            format!(
                "<Player @ {}>",
                mission::schema::waypoint_token(slot).unwrap()
            )
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
    catalog.waypoints.extend([8, 20, 29, 30]);
    for slot in [8, 20, 29] {
        assert!(prepare(&doc, &gift_plan(slot), &catalog).is_ok());
    }
    for slot in [1, 7, 28, 30, u32::MAX] {
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
fn shared_definitions_and_forward_references_roundtrip_and_reject_conflicts() {
    let (mut doc, catalog) = fixture();
    let source = r#"
import {defineMission, event, action, order} from './sdk';
import {map, House, Unit, Waypoint} from './map.generated';
export default defineMission(map, {id:'shared', player:House.Neutral}, m => {
    const start = m.rule('start', {when:event.sinceScenarioStart(0), once:true,
        actions:[action.reinforce(m.teamRef('first'), m.waypoint(Waypoint.WP0))]});
    if (start.kind !== 'rule' || m.ruleRef('start').id !== start.id) throw new Error('rule reference');
    for (const id of ['first', 'second']) {
        m.team(id, {house:m.house(House.Neutral), waypoint:m.waypoint(Waypoint.WP0),
            taskForce:m.taskForceRef('members'), script:m.scriptRef('patrol')});
    }
    m.taskForce('members', {units:[{type:m.unit(Unit.E1), count:3}], group:7});
    m.script('patrol', [order.guard(2), order.hunt()]);
});
"#;
    let mut compiler = Compiler::new().unwrap();
    let mut compile = |source: &str| {
        compiler
            .build(BTreeMap::from([
                ("/mission.ts".into(), source.into()),
                ("/map.generated.ts".into(), catalog.typescript().unwrap()),
            ]))
            .unwrap()
    };
    let plan = compile(source);
    assert_eq!(plan.version, 2);
    let concise = compile(
        "import {defineMission,event,action} from './sdk'; import {map,House} from './map.generated'; export default defineMission(map,{id:'concise',player:House.Neutral},m=>m.rule('win',{when:event.sinceScenarioStart(30),once:true,actions:[action.winPlayer()]}));",
    );
    prepare(&doc, &concise, &catalog).unwrap();
    let original = doc.snapshot().bytes().unwrap();
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let applied = doc.snapshot().bytes().unwrap();
    let teams = mission::ids(&doc, Kind::Team);
    assert_eq!(teams.len(), 2);
    let tf = doc.ini_text(&teams[0], "TaskForce");
    let script = doc.ini_text(&teams[0], "Script");
    assert_eq!(doc.ini_text(&teams[1], "TaskForce"), tf);
    assert_eq!(doc.ini_text(&teams[1], "Script"), script);
    assert_eq!(
        mission::ids(&doc, Kind::TaskForce).as_slice(),
        std::slice::from_ref(&tf)
    );
    assert_eq!(
        mission::ids(&doc, Kind::Script).as_slice(),
        std::slice::from_ref(&script)
    );
    assert_eq!(doc.ini_text(&tf, "0"), "3,E1");
    assert_eq!(doc.ini_text(&tf, "Group"), "7");
    assert_eq!(doc.ini_text(&script, "0"), "5,2");
    assert!(
        !prepare(&doc, &plan, &catalog)
            .unwrap()
            .apply(&mut doc)
            .unwrap()
    );
    assert!(doc.undo());
    assert_eq!(doc.snapshot().bytes().unwrap(), original);
    assert!(doc.redo());
    assert_eq!(doc.snapshot().bytes().unwrap(), applied);
    // Read the emitted dependencies through the production game parser.
    let ini = vera20k_core::rules::ini_parser::IniFile::from_bytes(&applied).unwrap();
    let registry = vera20k_core::rules::team_ai_ini::TeamAiIniRegistry::from_sources(
        &vera20k_core::rules::ini_parser::IniFile::empty(),
        &ini,
        false,
    );
    assert_eq!(registry.team_types.len(), 2);
    assert_eq!(registry.task_forces.len(), 1);
    assert_eq!(registry.scripts.len(), 1);
    let undefined = compile(&source.replace("m.scriptRef('patrol')", "m.scriptRef('missing')"));
    assert!(
        prepare(&doc, &undefined, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("未定义的 script")
    );
    let both = compile(&source.replace(
        "script:m.scriptRef('patrol')",
        "orders:[order.hunt()], script:m.scriptRef('patrol')",
    ));
    assert!(
        prepare(&doc, &both, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("orders 或 script")
    );
    let duplicate = compile(&source.replace(
        "m.script('patrol',",
        "m.script('patrol', [order.hunt()]); m.script('patrol',",
    ));
    assert!(
        prepare(&doc, &duplicate, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("重复行为脚本")
    );
    assert_eq!(doc.snapshot().bytes().unwrap(), applied);
    let mut deleted = plan;
    deleted.teams.clear();
    deleted.scripts.clear();
    deleted.task_forces.clear();
    deleted.rules.clear();
    prepare(&doc, &deleted, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    assert!(mission::ids(&doc, Kind::Script).is_empty());
    assert!(mission::ids(&doc, Kind::TaskForce).is_empty());
}

#[test]
fn generated_definition_capacity_is_checked_before_returning_an_unreadable_manifest() {
    let (doc, catalog) = fixture();
    let mut value = serde_json::json!({
        "version":2, "id":"capacity", "player":"Neutral", "rules":[], "flags":[], "teams":[],
        "taskForces":[{"id":"members", "units":[{"type":{"kind":"unit", "id":"E1"}, "count":1}]}],
        "scripts":[]
    });
    for index in 0..256 {
        value["scripts"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "id":format!("script_{index}"), "orders":vec![serde_json::json!({"kind":"hunt"});50]
            }));
        value["teams"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "id":format!("team_{index}"), "house":{"kind":"house", "id":"Neutral"},
                "taskForce":{"kind":"taskForce", "id":"members"},
                "script":{"kind":"script", "id":"script_0"}, "waypoint":{"kind":"waypoint", "id":0}
            }));
    }
    let before = doc.snapshot().bytes().unwrap();
    let mission = serde_json::from_value(value).unwrap();
    assert!(
        prepare(&doc, &mission, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("归属清单容量")
    );
    assert_eq!(doc.snapshot().bytes().unwrap(), before);
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
fn explosion_refuses_animation_reference_even_when_ts_types_are_bypassed() {
    let (doc, mut catalog) = fixture();
    catalog.register(
        crate::catalog::ResourceKind::Animation,
        vec!["VISUAL".into()],
    );
    // Old SDK emitted this invalid shape; reject it rather than reinterpret it.
    assert!(
        serde_json::from_value::<Action>(serde_json::json!({
            "kind":"explosionAt", "animation":{"kind":"animation","id":"VISUAL"},
            "at":{"kind":"waypoint","id":0}
        }))
        .is_err()
    );
    let mut mission = plan();
    mission.rules[0].actions = vec![
        serde_json::from_value(serde_json::json!({
            "kind":"explosionAt", "weapon":{"kind":"animation","id":"VISUAL"},
            "at":{"kind":"waypoint","id":0}
        }))
        .unwrap(),
    ];
    assert!(
        prepare(&doc, &mission, &catalog).is_err(),
        "Action42 must select a weapon, not an animation"
    );
}

#[test]
fn explosion_uses_weapon_allocation_order_independent_of_animation_and_display_order() {
    use crate::catalog::ResourceKind;
    let (mut doc, mut catalog) = fixture();
    catalog.register(
        ResourceKind::Weapon,
        vec!["Z_DAMAGE".into(), "A_DAMAGE".into()],
    );
    catalog.register(
        ResourceKind::Animation,
        vec!["A_DAMAGE".into(), "VISUAL".into()],
    );
    let source = r#"import {defineMission,event,action} from './sdk';
import {map,House,Weapon,Animation,Waypoint} from './map.generated';
export default defineMission(map,{id:'effects',player:House.Neutral},m=>{
 m.rule('detonate',{when:event.sinceScenarioStart(1),once:true,actions:[
  action.playAnimationAt(m.animation(Animation.VISUAL),m.waypoint(Waypoint._0)),
  action.explosionAt(m.weapon(Weapon.A_DAMAGE),m.waypoint(Waypoint._0))
 ]});
});"#;
    // Use the generated numeric enum name rather than assuming its spelling.
    let source = source.replace(
        "Waypoint._0",
        &format!(
            "Waypoint.{}",
            catalog.enum_member("waypoints", "0").unwrap()
        ),
    );
    let mission = Compiler::new()
        .unwrap()
        .build(BTreeMap::from([
            ("/mission.ts".into(), source),
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
        ]))
        .unwrap();
    prepare(&doc, &mission, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let trigger = &mission::ids(&doc, Kind::Trigger)[0];
    let rows = mission::decode_list(&doc.ini_text("Actions", trigger), false).unwrap();
    assert_eq!((rows[0][0].as_str(), rows[0][2].as_str()), ("41", "1"));
    assert_eq!((rows[1][0].as_str(), rows[1][2].as_str()), ("42", "1"));
    let mut missing = catalog.clone();
    missing.registry_order.remove("weapons");
    assert!(prepare(&doc, &mission, &missing).is_err());
}

#[test]
fn extended_resource_references_build_with_generated_enums_and_rust_rejects_wrong_families() {
    use crate::catalog::ResourceKind;
    let (doc, mut catalog) = fixture();
    for kind in ResourceKind::ALL {
        catalog.register(kind, vec!["Z_RESOURCE".into(), "A_RESOURCE".into()]);
    }
    let checks = [
        ("building", "Building", "building"), ("techno", "Techno", "techno"),
        ("country", "Country", "country"), ("sound", "Sound", "sound"),
        ("eva", "Eva", "eva"), ("theme", "Theme", "theme"), ("movie", "Movie", "movie"),
        ("animation", "Animation", "animation"), ("weapon", "Weapon", "weapon"), ("superWeapon", "SuperWeapon", "superWeapon"),
        ("warhead", "Warhead", "warhead"), ("particleSystem", "ParticleSystem", "particleSystem"),
    ].into_iter().map(|(method, name, kind)| format!(
        "const {method} = m.{method}({name}.Z_RESOURCE); if ({method}.kind !== '{kind}' || {method}.id !== 'Z_RESOURCE') throw new Error('reference identity');"
    )).collect::<Vec<_>>().join("\n");
    let source = format!(
        r#"
import {{defineMission, event, action}} from './sdk';
import {{map, House, Text, Building, Techno, Country, Sound, Eva, Theme, Movie, Animation, Weapon, SuperWeapon, Warhead, ParticleSystem}} from './map.generated';
export default defineMission(map, {{id:'resources', player:House.Neutral}}, m => {{
    {checks}
    m.rule('show', {{when:event.sinceScenarioStart(0), once:true, actions:[action.message(m.text(Text.MISSION_TEXT))]}});
}});
"#
    );
    let mut compiler = Compiler::new().unwrap();
    let files = BTreeMap::from([
        ("/mission.ts".into(), source.clone()),
        ("/map.generated.ts".into(), catalog.typescript().unwrap()),
    ]);
    let plan = compiler.build(files.clone()).unwrap();
    prepare(&doc, &plan, &catalog).unwrap();
    let mut forged = files;
    forged.insert(
        "/mission.ts".into(),
        source.replace(
            "m.text(Text.MISSION_TEXT)",
            "m.sound(Sound.Z_RESOURCE) as any",
        ),
    );
    let plan = compiler.build(forged).unwrap();
    assert!(
        prepare(&doc, &plan, &catalog)
            .err()
            .unwrap()
            .to_string()
            .contains("应为 text")
    );
    let legacy = r#"import {defineMission} from './sdk';
export default defineMission({houses:['Neutral'], units:[], waypoints:[], objects:[], strings:[]}, {id:'legacy', player:'Neutral'}, m => {});"#;
    let legacy = compiler
        .build(BTreeMap::from([("/mission.ts".into(), legacy.into())]))
        .unwrap();
    prepare(&doc, &legacy, &catalog).unwrap();
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
            ("m.rule('invalid', {when:event.sinceEnabled(30), once:false, actions:[]});", "1～64 个动作"),
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
        catalog.waypoints.extend([7, 8, 20, 25, 26, 29, 30]);
        files.insert("/map.generated.ts".into(), catalog.typescript().unwrap());
        let multiplayer = base.replace("player:House.Neutral", "mode:MissionMode.Multiplayer");
        files.insert("/mission.ts".into(), multiplayer.replace("BODY", "const gift = m.team('gift', {house:m.playerAt(Waypoint.WP7), waypoint:m.waypoint(Waypoint.WP20), units:[{type:m.unit(Unit.E1), count:3}], orders:[order.guard(60)]}); m.rule('give', {when:event.sinceScenarioStart(10), once:true, actions:[action.reinforce(gift, m.waypoint(Waypoint.WP20))]});"));
        let built = compiler.build(files.clone()).unwrap();
        assert_eq!(built.mode, MissionMode::Multiplayer);
        assert!(built.player.is_empty());
        assert!(matches!(built.teams[0].house, TeamOwner::StartPlayer { id: 7 }));
        for slot in [8, 20, 25, 26, 29] {
            files.insert("/mission.ts".into(), multiplayer.replace("BODY", &format!("const gift = m.team('gift', {{house:m.playerAt(Waypoint.WP{slot}), waypoint:m.waypoint(Waypoint.WP0), units:[{{type:m.unit(Unit.E1), count:3}}], orders:[order.guard(60)]}});")));
            let built = compiler.build(files.clone()).unwrap();
            assert!(matches!(built.teams[0].house, TeamOwner::StartPlayer { id } if id == slot));
            assert!(prepare(&doc, &built, &catalog).is_ok());
        }
        for (source, expected) in [
            (multiplayer.replace("BODY", "m.playerAt(Waypoint.WP30);"), "仅支持 0～29"),
            (multiplayer.replace("BODY", "m.playerAt(1.5);"), "Unknown player start"),
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

#[test]
fn new_engine_orders_and_voxel_action_compile_through_sdk_and_native_readers() {
    use crate::catalog::ResourceKind;
    let (mut doc, mut catalog) = fixture();
    catalog.register(
        ResourceKind::VoxelAnim,
        vec!["Z_DEBRIS".into(), "A_DEBRIS".into()],
    );
    catalog.register(ResourceKind::Animation, vec!["A_DEBRIS".into()]);
    let wp = catalog.enum_member("waypoints", "0").unwrap();
    let source = r#"import {defineMission,MissionMode,event,action,order} from './sdk';
import {map,VoxelAnim,Waypoint} from './map.generated';
export default defineMission(map,{id:'new_handlers',mode:MissionMode.Multiplayer},m=>{
 m.script('storm',[order.startLightningStorm(-1),order.stopLightningStorm(),order.scoutHouses()]);
 m.rule('debris',{when:event.sinceScenarioStart(1),once:true,actions:[
  action.playVoxelAnimAt(m.voxelAnim(VoxelAnim.A_DEBRIS),m.waypoint(Waypoint.WP))
 ]});
});"#
        .replace("Waypoint.WP", &format!("Waypoint.{wp}"));
    let plan = Compiler::new()
        .unwrap()
        .build(BTreeMap::from([
            ("/mission.ts".into(), source),
            ("/map.generated.ts".into(), catalog.typescript().unwrap()),
        ]))
        .unwrap();
    prepare(&doc, &plan, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let bytes = doc.snapshot().bytes().unwrap();
    let map = vera20k_core::map::map_file::load_from_bytes(bytes.clone()).unwrap();
    let trigger = &mission::ids(&doc, Kind::Trigger)[0];
    let action = &map.actions[trigger].entries()[0];
    assert_eq!(action.kind, 43);
    assert_eq!(action.literal_value(), Some(1));
    assert_eq!(action.waypoint_index, Some(0));
    let ini = vera20k_core::rules::ini_parser::IniFile::from_bytes(&bytes).unwrap();
    let scripts = vera20k_core::rules::team_ai_ini::TeamAiIniRegistry::from_sources(
        &vera20k_core::rules::ini_parser::IniFile::empty(),
        &ini,
        false,
    );
    assert_eq!(
        scripts.scripts[0]
            .actions
            .iter()
            .map(|a| (a.action_id, a.argument))
            .collect::<Vec<_>>(),
        [(32, -1), (33, 0), (48, 0)]
    );
    // Rust revalidates family and native ordinals even when TS branding is bypassed.
    let mut forged = serde_json::to_value(&plan).unwrap();
    forged["rules"][0]["actions"][0]["animation"]["kind"] = serde_json::json!("animation");
    let forged: Mission = serde_json::from_value(forged).unwrap();
    assert!(prepare(&fixture().0, &forged, &catalog).is_err());
    let mut missing = catalog.clone();
    missing.registry_order.remove("voxelAnims");
    assert!(prepare(&fixture().0, &plan, &missing).is_err());
}

#[test]
fn voxel_registry_changes_require_a_fresh_sdk_catalog() {
    use crate::catalog::ResourceKind;
    let (mut doc, mut catalog) = fixture();
    doc.set_ini("VoxelAnims", "0", Some(b"Z_DEBRIS".to_vec()))
        .unwrap();
    doc.set_ini("VoxelAnims", "1", Some(b"A_DEBRIS".to_vec()))
        .unwrap();
    doc.commit("initial voxel registry");
    catalog.register(
        ResourceKind::VoxelAnim,
        vec!["Z_DEBRIS".into(), "A_DEBRIS".into()],
    );
    catalog.type_projection = registry_fingerprints(&doc.snapshot().bytes().unwrap()).unwrap();
    let mut mission = plan();
    mission.rules[0].actions = vec![Action::PlayVoxelAnimAt {
        animation: Reference {
            kind: "voxelAnim".into(),
            id: "A_DEBRIS".into(),
        },
        at: Waypoint {
            kind: "waypoint".into(),
            id: 0,
        },
    }];
    assert!(prepare(&doc, &mission, &catalog).is_ok());
    doc.set_ini("VoxelAnims", "0", Some(b"A_DEBRIS".to_vec()))
        .unwrap();
    doc.set_ini("VoxelAnims", "1", Some(b"Z_DEBRIS".to_vec()))
        .unwrap();
    doc.commit("reorder voxel registry");
    let error = prepare(&doc, &mission, &catalog)
        .err()
        .expect("reject stale ordinals");
    assert!(error.to_string().contains("地图类型注册表已变化"));
    catalog.register(
        ResourceKind::VoxelAnim,
        vec!["A_DEBRIS".into(), "Z_DEBRIS".into()],
    );
    catalog.type_projection = registry_fingerprints(&doc.snapshot().bytes().unwrap()).unwrap();
    prepare(&doc, &mission, &catalog)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    let trigger = &mission::ids(&doc, Kind::Trigger)[0];
    let rows = mission::decode_list(&doc.ini_text("Actions", trigger), false).unwrap();
    assert_eq!(rows[0][2], "0");
}
