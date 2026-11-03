//! TS discovery and BuildingLight notifications through ordinary fog/light updates.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    mission::{self, Kind},
    new_map::NewMapOptions,
};
use ra2_map_io::mission_script::{self, Sources, Worker};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use vera20k_core::{
    map::map_file,
    sim::{
        command::{Command, CommandEnvelope},
        runtime::SimRuntime,
        trigger_runtime::TriggerEffect,
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;
#[path = "support/saved_field.rs"]
mod saved_field;

fn light_state(runtime: &SimRuntime) -> Result<Value> {
    // Read the existing save representation; do not add diagnostic engine APIs.
    Ok(saved_field::read(
        &runtime.simulation,
        &["substrate", "spotlights"],
    )?)
}
fn build(install: &mut Install, root: &Path, name: &str) -> Result<Vec<u8>> {
    std::fs::create_dir_all(root)?;
    let spot = name.starts_with("spot_");
    let initial = NewMapOptions {
        width: 64,
        height: 64,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(initial.clone())?;
    let mut doc = MapDocument::new(Baseline::parse(initial.into(), &map)?);
    for house in ["Russians", "Americans"] {
        mission::create(&mut doc, Kind::House, house, house, None)?;
        doc.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
        doc.set_ini(house, "IQ", Some(b"0".to_vec()))?;
    }
    doc.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    doc.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    doc.set_ini("Americans", "PlayerControl", Some(b"yes".to_vec()))?;
    doc.add_record(
        "Structures",
        if spot { [40, 60] } else { [28, 52] },
        "Americans",
        "GAPOWR",
    )?;
    doc.add_record("Structures", [82, 82], "Russians", "NAPOWR")?;
    for (section, key, value) in [
        ("MTNK", "Primary", "none"),
        ("MTNK", "Secondary", "none"),
        ("MTNK", "Speed", "0"),
        ("GAPOWR", "HasSpotlight", if spot { "yes" } else { "no" }),
        ("General", "SpotlightLocationRadius", "1000"),
        ("General", "SpotlightMovementRadius", "2000"),
        ("General", "SpotlightSpeed", ".005"),
        ("General", "SpotlightAcceleration", ".0001"),
        ("General", "SpotlightAngle", ".02"),
        ("General", "SpotlightRadius", "175"),
    ] {
        doc.set_ini(section, key, Some(value.as_bytes().to_vec()))?;
    }
    if name == "spot_offline" {
        // GoOffline refuses an ordinary power generator; declare a Powered fixture.
        doc.set_ini("GAPOWR", "Powered", Some(b"yes".to_vec()))?;
    }
    let target_cell = if spot {
        let probe = campaign::load(install, &doc.snapshot().bytes()?, 1)?;
        let lights = light_state(&probe)?;
        let light = lights["objects"]
            .as_object()
            .context("light map")?
            .values()
            .next()
            .context("constructed light")?;
        let coord = light["coord"].as_array().context("light coord")?;
        let cell = [
            u16::try_from(coord[0].as_i64().context("x")? / 256)?,
            u16::try_from(coord[1].as_i64().context("y")? / 256)?,
        ];
        std::fs::write(
            root.join("geometry-probe.json"),
            serde_json::to_vec_pretty(
                &json!({"lights":lights,"target_cell":cell,"rules":probe.resources.rules.general.spotlight}),
            )?,
        )?;
        cell
    } else {
        [60, 50]
    };
    doc.add_record(
        "Units",
        target_cell,
        if name == "spot_ally" {
            "Americans"
        } else {
            "Russians"
        },
        "MTNK",
    )?;
    if !spot {
        doc.add_record("Infantry", [30, 50], "Americans", "ENGINEER")?;
    }
    doc.commit("discovery and spotlight fixture");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(
        install,
        root.join("base.map").to_str().context("map path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let definitions = if spot {
        let mode = match name {
            "spot_off" => 0,
            "spot_rotate" => 2,
            "spot_track" => 3,
            _ => 1,
        };
        format!(r#"
 const parent=m.object(MapObject.Structures_0);
 const setup=m.rule('setup',{{when:event.sinceScenarioStart(4),once:false,linked:m.ruleRef('seen'),
   actions:[action.setSpotlightBehavior({mode}),__OFFLINE__action.enableRule(m.ruleRef('seen')),
     action.message(m.defineText('configured','configured')),action.disableRule(m.ruleRef('setup'))]}});
 m.rule('seen',{{when:event.spotlightDetected(parent),once:false,enabled:false,
   actions:[action.message(m.defineText('seen','seen')),action.setGlobal(m.globalFlag(0)),action.disableRule(m.ruleRef('seen'))]}});
 m.bindObject(parent,m.tag('light',{{rule:setup,repeat:TagRepeat.Repeat}}));
 m.rule('wrong_binding',{{when:event.spotlightDetected(m.object(MapObject.Units_0)),once:true,
   actions:[action.message(m.defineText('wrong','wrong')),action.setGlobal(m.globalFlag(2))]}});
 "#).replace("__OFFLINE__",if name=="spot_offline" {"action.setAttachedBuildingsOnline(false),"}else{""})
    } else {
        let reveal = if name == "discover_reveal" {
            "action.revealAt(at),"
        } else {
            ""
        };
        format!(
            r#"
 const at=m.createWaypoint(30,60,50),target=m.object(MapObject.Units_0);
 m.rule('object_seen',{{when:event.discovered(target),once:false,
   actions:[action.message(m.defineText('object','object')),action.setGlobal(m.globalFlag(0)),action.disableRule(m.ruleRef('object_seen'))]}});
 m.rule('house_seen',{{when:event.countryDiscovered(m.country(Country.Russians)),once:true,
   actions:[action.message(m.defineText('country','country')),action.setGlobal(m.globalFlag(1))]}});
 m.rule('wrong_country',{{when:event.countryDiscovered(m.country(Country.Americans)),once:true,
   actions:[action.message(m.defineText('wrong','wrong')),action.setGlobal(m.globalFlag(2))]}});
 const absent=m.country(Country.Alliance);
 const missing=[event.countryDiscovered(absent),event.countryUnitsDestroyed(absent),event.countryBuildingsDestroyed(absent),event.countryAllDestroyed(absent),event.countryNavyDestroyed(absent),event.countryLandDestroyed(absent)];
 for(let i=0;i<missing.length;i++) m.rule('absent_'+i,{{when:missing[i]!,once:true,
   actions:[action.message(m.defineText('absent_'+i,'absent_'+i)),action.setGlobal(m.globalFlag(3+i))]}});
 for(let i=0;i<2;i++) m.rule('reveal_'+i,{{when:event.sinceScenarioStart(4+i*4),once:true,
   actions:[{reveal}action.message(m.defineText('stage_'+i,'stage_'+i))]}});
 "#
        )
    };
    let source=r#"import {defineMission,event,action,TagRepeat} from './sdk';
import {map,House,Country,MapObject} from './map.generated';
export default defineMission(map,{id:'discovery_spotlight',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),iq:0,playerControl:true,allies:[m.house(House.Americans)]});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),iq:0,allies:[m.house(House.Russians)]});
 __DEFINITIONS__
});"#.replace("__DEFINITIONS__",&definitions);
    let entry = root.join("mission.ts");
    std::fs::write(&entry, source)?;
    mission_script::refresh_types(&entry, &catalog)?;
    Worker::new()?
        .build(&doc, catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(&mut doc)?;
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(root.join("compiled.map"), &bytes)?;
    Ok(bytes)
}
fn objects(runtime: &SimRuntime) -> Value {
    let sim = &runtime.simulation;
    json!(sim.entities().iter_sorted().map(|(id,e)|json!({"id":id,"type":sim.interner().resolve(e.type_ref()),"owner":sim.interner().resolve(e.owner()),
        "health":e.health.current(),"cell":sim.object_coords_cell(id),"discovery":e.discovery,"online":e.building_online()})).collect::<Vec<_>>())
}
fn audit(install: &Install, bytes: &[u8], root: &Path, name: &str, restore: bool) -> Result<Value> {
    std::fs::create_dir_all(root)?;
    let spot = name.starts_with("spot_");
    let mut runtime = campaign::load(install, bytes, 1)?;
    let sim = &runtime.simulation;
    let target = sim
        .entities()
        .iter_sorted()
        .find(|(_, e)| sim.interner().resolve(e.type_ref()) == "MTNK")
        .context("target")?
        .0;
    let parent = sim
        .entities()
        .iter_sorted()
        .find(|(_, e)| sim.interner().resolve(e.type_ref()) == "GAPOWR")
        .context("parent")?
        .0;
    let initial = objects(&runtime);
    let initial_light = light_state(&runtime)?;
    let light_id = initial_light["objects"]
        .as_object()
        .context("light objects")?
        .values()
        .find(|l| l["parent"] == parent)
        .and_then(|l| l["stable_id"].as_u64());
    let texts = mission_script::declared_texts(bytes)?;
    let mut messages = Vec::new();
    let mut observations = Vec::new();
    let mut light_stages = Vec::new();
    let mut checkpoint_state = None;
    let mut integrity = true;
    for frame in 0..1000 {
        let sim = &runtime.simulation;
        let tick = sim.session().tick();
        let commands = if name == "discover_move" && frame == 60 {
            let scout = sim
                .entities()
                .iter_sorted()
                .find(|(_, e)| sim.interner().resolve(e.type_ref()) == "ENGINEER")
                .context("scout")?
                .0;
            vec![CommandEnvelope::synchronized(
                sim.encode_megamission_record(
                    sim.interner().get("Americans").unwrap(),
                    &Command::Move {
                        entity_id: scout,
                        target_rx: 62,
                        target_ry: 50,
                    },
                )
                .context("normal scout movement")?,
                tick,
            )]
        } else {
            Vec::new()
        };
        let output =
            runtime.advance_frame(&commands, 22, TickLane::Ordinary, FrameEffects::default())?;
        let sim = &runtime.simulation;
        let tick = sim.session().tick();
        let current = objects(&runtime);
        integrity &= current.as_array().unwrap().iter().all(|o| {
            initial.as_array().unwrap().iter().any(|i| {
                o["id"] == i["id"] && ["type", "owner", "health"].iter().all(|k| o[k] == i[k])
            })
        });
        let light = light_id
            .and_then(|id| sim.building_spotlight(id, &runtime.resources.rules))
            .map(|l| json!({"coord":l.coord,"origin":l.beam_origin,"edges":l.beam_edges}));
        let row = json!({"tick":tick,"objects":current,"light":light,"flags":sim.global_variables()[..9]});
        for effect in output.trigger_effects {
            if let TriggerEffect::LocalizedMessage { csf_label } = effect {
                messages.push(json!({"tick":tick,"event":texts.get(&csf_label).context("label")?,"state":row}));
            }
        }
        observations.push(row);
        if [60, 61, 70, 120, 300].contains(&tick) {
            light_stages.push(json!({"tick":tick,"lights":light_state(&runtime)?}));
        }
        if restore && frame == 39 {
            let before = json!({"objects":current,"lights":light_state(&runtime)?,"flags":sim.global_variables().as_slice()});
            checkpoint::restore_checkpoint(
                &mut runtime,
                bytes,
                &root.join("before-notification.bin"),
            )?;
            let after = json!({"objects":objects(&runtime),"lights":light_state(&runtime)?,"flags":runtime.simulation.global_variables().as_slice()});
            checkpoint_state =
                Some(json!({"tick":tick,"preserved":before==after,"before":before,"after":after}));
        }
    }
    let sim = &runtime.simulation;
    let count = |event: &str| messages.iter().filter(|m| m["event"] == event).count();
    let expected_discovery = matches!(name, "discover_reveal" | "discover_move");
    let mut checks = BTreeMap::from([
        ("singleplayer_mode", !sim.session().game_mode_nonzero()),
        (
            "all_original_objects_owners_types_health_preserved",
            integrity
                && objects(&runtime).as_array().unwrap().len() == initial.as_array().unwrap().len(),
        ),
        (
            "negative_observers_silent",
            count("wrong") == 0 && !sim.global_variables()[2],
        ),
    ]);
    if spot {
        let expected = name == "spot_scan";
        let mode = match name {
            "spot_off" => 0,
            "spot_rotate" => 2,
            "spot_track" => 3,
            _ => 1,
        };
        let all_lights = |stages: &Value| {
            stages["objects"]
                .as_object()
                .unwrap()
                .values()
                .cloned()
                .collect::<Vec<_>>()
        };
        let settled = light_stages
            .iter()
            .filter(|s| s["tick"].as_u64().unwrap() >= 61)
            .collect::<Vec<_>>();
        checks.insert(
            "one_owned_independent_spotlight",
            light_id.is_some()
                && all_lights(&initial_light).len() == 1
                && all_lights(&initial_light)[0]["parent"] == parent,
        );
        checks.insert(
            "configured_once_at_expected_frame",
            count("configured") == 1
                && messages
                    .iter()
                    .any(|m| m["event"] == "configured" && m["tick"] == 61),
        );
        checks.insert(
            "only_scan_detects_enemy",
            count("seen") == usize::from(expected) && sim.global_variables()[0] == expected,
        );
        checks.insert(
            "detection_after_enable",
            messages
                .iter()
                .filter(|m| m["event"] == "seen")
                .all(|m| m["tick"].as_u64().unwrap() >= 61),
        );
        checks.insert(
            "native_mode_retained",
            settled
                .iter()
                .all(|s| all_lights(&s["lights"])[0]["mode"] == mode),
        );
        checks.insert(
            "draw_inputs_follow_enabled_and_online",
            observations
                .iter()
                .filter(|r| r["tick"].as_u64().unwrap() >= 61)
                .all(|r| r["light"].is_null() == matches!(name, "spot_off" | "spot_offline")),
        );
        if name == "spot_track" {
            checks.insert(
                "nearest_enemy_selected_for_tracking",
                settled
                    .iter()
                    .all(|s| all_lights(&s["lights"])[0]["target"] == target),
            );
        }
        if name == "spot_offline" {
            checks.insert(
                "powered_fixture_actually_offline",
                sim.entities()
                    .get(parent)
                    .is_some_and(|e| !e.building_online()),
            );
        }
        if name == "spot_rotate" {
            checks.insert(
                "mode_two_rotates_instead_of_fixed_pose",
                all_lights(&settled[0]["lights"])[0]["coord"]
                    != all_lights(&settled[2]["lights"])[0]["coord"],
            );
        }
    } else {
        let initial_target = initial
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["id"] == target)
            .unwrap();
        checks.insert(
            "target_initially_undiscovered",
            initial_target["discovery"]["discovered_by_current_house"] == false,
        );
        checks.insert(
            "discovery_observers_match_real_visibility",
            count("object") == usize::from(expected_discovery)
                && count("country") == usize::from(expected_discovery)
                && sim
                    .entities()
                    .get(target)
                    .unwrap()
                    .discovery
                    .discovered_by_current_house()
                    == expected_discovery,
        );
        checks.insert(
            "no_discovery_before_script_or_movement",
            messages
                .iter()
                .filter(|m| matches!(m["event"].as_str(), Some("object" | "country")))
                .all(|m| m["tick"].as_u64().unwrap() >= 61),
        );
        checks.insert(
            "both_timed_stages_executed",
            count("stage_0") == 1 && count("stage_1") == 1,
        );
        if name == "discover_move" {
            checks.insert(
                "normal_scout_move_completed",
                sim.entities().iter_sorted().any(|(id, e)| {
                    sim.interner().resolve(e.type_ref()) == "ENGINEER"
                        && sim.object_coords_cell(id) == Some((62, 50))
                }),
            );
        }
        checks.insert(
            "absent_country_has_no_registered_house",
            !sim.houses().keys().any(|id| {
                sim.interner()
                    .resolve(sim.houses().get(id).unwrap().house_type_id())
                    == "Alliance"
            }),
        );
        checks.insert(
            "six_country_predicates_accept_absent_house",
            (0..6).all(|i| count(&format!("absent_{i}")) == 1 && sim.global_variables()[3 + i]),
        );
    }
    if restore {
        checks.insert(
            "checkpoint_preserves_listed_state",
            checkpoint_state
                .as_ref()
                .is_some_and(|v| v["preserved"] == true),
        );
    }
    let passed = checks.values().all(|b| *b);
    std::fs::write(
        root.join("observations.json"),
        serde_json::to_vec_pretty(&observations)?,
    )?;
    Ok(
        json!({"passed":passed,"checks":checks,"initial":initial,"final":objects(&runtime),"initial_light":initial_light,"light_stages":light_stages,"messages":messages,"checkpoint":checkpoint_state}),
    )
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(args.next().context("new output directory")?)?;
    ensure!(
        args.next().is_none() && !root.exists(),
        "output must be new"
    );
    std::fs::create_dir_all(&root)?;
    let mut install = Install::open(&game)?;
    let mut cases = Vec::new();
    for name in [
        "discover_reveal",
        "discover_move",
        "discover_none",
        "spot_scan",
        "spot_off",
        "spot_rotate",
        "spot_track",
        "spot_ally",
        "spot_offline",
    ] {
        let folder = root.join(name);
        let bytes = build(&mut install, &folder, name)?;
        let continuous = audit(&install, &bytes, &folder.join("continuous"), name, false)?;
        if matches!(
            name,
            "discover_reveal" | "discover_move" | "spot_scan" | "spot_track"
        ) {
            let restored = audit(&install, &bytes, &folder.join("restored"), name, true)?;
            let agreement = continuous["messages"] == restored["messages"]
                && continuous["final"] == restored["final"]
                && continuous["light_stages"] == restored["light_stages"];
            cases.push(json!({"case":format!("{name}_restored"),"result":restored,"listed_outcomes_match":agreement}));
        }
        cases.push(json!({"case":name,"result":continuous}));
    }
    let passed = cases.iter().all(|c| {
        c["result"]["passed"] == true && c.get("listed_outcomes_match").is_none_or(|v| v == true)
    });
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&json!({"passed":passed,"cases":cases,
        "scope":"TS rules through normal campaign loading, reveal actions, encoded scout Move, fog and independent spotlight visits. Map rules declare controlled light motion. Private light state is read through existing save serialization. Headless snapshots compare listed observations only; no GUI save, GPU beam, network or native pixel parity claim."}))?,
    )?;
    ensure!(
        passed,
        "discovery/spotlight audit failed; inspect report.json"
    );
    println!("{}", root.join("report.json").display());
    Ok(())
}
