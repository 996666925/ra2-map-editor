//! TS start ownership, repeated reinforcement and countdown in actual session modes.
//! Uses production headless construction; no network transport or GUI claim.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    new_map::NewMapOptions,
};
use ra2_map_io::mission_script::{self, Sources, Worker};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};
use vera20k_core::{
    headless_scenario::{HeadlessScenario, battle_session, load_with_launch_and_ini},
    map::map_file,
    sim::{
        scenario_bootstrap::MatchLaunchDescriptor,
        scenario_session::ScenarioGameMode,
        world::{FrameEffects, TickLane},
    },
    skirmish_launch::{LaunchCountry, LaunchStartPosition},
};
#[path = "support/checkpoint.rs"]
mod checkpoint;

fn state(scenario: &HeadlessScenario) -> Value {
    let sim = scenario.sim();
    let objects: Vec<_> = sim
        .entities()
        .iter_sorted()
        .map(|(id, e)| {
            json!({
                "id":id,"type":sim.interner().resolve(e.type_ref()),
                "owner":sim.interner().resolve(e.owner()),"health":e.health.current(),
                "active":e.is_active(),"cell":sim.object_coords_cell(id),
            })
        })
        .collect();
    json!({"tick":sim.session().tick(),"mode":sim.session().game_mode(),
        "starts":sim.session().start_slot_houses().iter().map(|(slot,house)|
            (slot.to_string(),json!(sim.interner().resolve(*house)))).collect::<BTreeMap<_,_>>(),
        "objects":objects,"teams":sim.team_script_vm().teams_in_order().collect::<Vec<_>>(),
        "globals":sim.global_variables().as_slice(),"timer":sim.session().scenario_timer_view()})
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(PathBuf::from(args.next().context("new output directory")?))?;
    ensure!(!root.exists(), "请选择新的审计目录");
    std::fs::create_dir_all(&root)?;
    let initial = NewMapOptions {
        width: 64,
        height: 64,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(initial.clone())?;
    let mut doc = MapDocument::new(Baseline::parse(initial.into(), &map)?);
    for (slot, at) in [(0, [30, 40]), (1, [70, 70]), (2, [55, 55])] {
        doc.waypoint(slot, at)?;
    }
    let mut objects = Vec::new();
    for at in [[35, 50], [60, 65]] {
        let id = doc.add_record("Units", at, "Neutral", "MTNK")?;
        objects.push(format!("Units_{}", doc.records()[&id].key));
    }
    for key in ["Primary", "Secondary"] {
        doc.set_ini("MTNK", key, Some(b"none".to_vec()))?;
    }
    doc.commit("multiplayer fixture");
    let map_path = root.join("multiplayer.map");
    std::fs::write(&map_path, doc.snapshot().bytes()?)?;
    let mut install = Install::open(&game)?;
    let scene =
        ra2_map_scene::map::load_scene(&mut install, map_path.to_str().context("map path")?)?;
    let catalog = mission_script::catalog(&doc, &scene);
    let source = r#"import {defineMission,event,action,order,MissionMode} from './sdk';
import {map,Unit,Waypoint,MapObject} from './map.generated';
export default defineMission(map,{id:'multiplayer_audit',mode:MissionMode.Multiplayer},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.assignObjectToPlayer(m.object(MapObject.__OBJECT0__),0);
 m.assignObjectToPlayer(m.object(MapObject.__OBJECT1__),1);
 const entries=[m.createWaypoint(30,40,50),m.createWaypoint(31,65,65),m.createWaypoint(32,55,55)];
 const players=[m.playerAt(Waypoint.WP0),m.playerAt(Waypoint.WP1),m.playerAt(Waypoint.WP2)];
 for(let i=0;i<3;i++) {
   const t=m.team(`gift_${i}`,{house:players[i]!,waypoint:entries[i]!,
     units:[{type:m.unit(Unit.ENGINEER),count:1}],max:-1,reinforce:false,
     orders:[order.guard(1),order.setGlobal(m.globalFlag(i)),order.waitForever()]});
   m.rule(`repeat_${i}`,{when:event.sinceEnabled(2),once:false,
     actions:[action.reinforce(t,entries[i]!)]});
 }
 m.rule('timer',{when:event.sinceScenarioStart(0),once:true,actions:[action.setCountdown(30)]});
 m.rule('stop',{when:event.sinceScenarioStart(3),once:true,actions:[action.stopCountdown()]});
 m.rule('start',{when:event.sinceScenarioStart(5),once:true,actions:[action.startCountdown()]});
 m.rule('extend',{when:event.sinceScenarioStart(6),once:true,actions:[action.extendCountdown(2)]});
 m.rule('finish',{when:event.sinceScenarioStart(7),once:true,actions:[action.shortenCountdown(1),
   action.disableRule(m.ruleRef('repeat_0')),action.disableRule(m.ruleRef('repeat_1')),action.disableRule(m.ruleRef('repeat_2'))]});
});"#.replace("__OBJECT0__",&objects[0]).replace("__OBJECT1__",&objects[1]);
    let entry = root.join("mission.ts");
    std::fs::write(&entry, source)?;
    mission_script::refresh_types(&entry, &catalog)?;
    let worker = Worker::new()?;
    worker
        .build(&doc, catalog.clone(), Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(&mut doc)?;
    let bytes = doc.snapshot().bytes()?;
    let unchanged = !worker
        .build(&doc, catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(&mut doc)?;
    std::fs::write(&map_path, &bytes)?;
    let filename = map_path.to_str().context("map path")?;
    let mut checks = BTreeMap::from([("unchanged_rebuild".to_owned(), unchanged)]);
    let mut runs = Vec::new();
    for mode in [
        ScenarioGameMode::Skirmish,
        ScenarioGameMode::Lan,
        ScenarioGameMode::Wol,
    ] {
        for swapped in [false, true] {
            let mut finals = Vec::new();
            let mut timelines = Vec::new();
            for restore in [false, true] {
                let tag = format!(
                    "{}-{}-{}",
                    mode.as_str(),
                    u8::from(swapped),
                    u8::from(restore)
                );
                let mut launch = battle_session(filename, &[]);
                launch.humans[0].name = "SdkAlpha".into();
                launch.humans[0].slot.start_position =
                    LaunchStartPosition::Position(u8::from(swapped));
                let mut second = launch.humans[0].clone();
                second.name = "SdkBeta".into();
                second.raw_slot_index = Some(1);
                second.slot.start_position = LaunchStartPosition::Position(u8::from(!swapped));
                second.slot.country = LaunchCountry::Russia;
                second.slot.color_index = 1;
                launch.humans.push(second);
                launch.rebuild_house_roster();
                launch.options.unit_count = 0;
                launch.options.short_game = false;
                launch.options.crates = false;
                let launch = MatchLaunchDescriptor::from_resolved(launch)
                    .map_err(|e| anyhow::anyhow!("{e:?}"))?;
                let mut scenario = load_with_launch_and_ini(
                    &game,
                    filename,
                    12345,
                    launch,
                    &Default::default(),
                    mode,
                    None,
                    None,
                )
                .map_err(anyhow::Error::msg)?;
                // The headless loader constructs mode-dependent Houses, but
                // transport setup normally supplies MaxAhead after loading.
                // Use the public production setter before frames or saves.
                if mode.is_network() {
                    scenario.runtime.simulation.install_network_timing(10);
                }
                let initial = state(&scenario);
                let expected = if swapped {
                    ["SdkBeta", "SdkAlpha"]
                } else {
                    ["SdkAlpha", "SdkBeta"]
                };
                checks.insert(
                    format!("{tag}_mode_and_two_starts"),
                    scenario.sim().session().game_mode() == mode
                        && initial["starts"]["0"] == expected[0]
                        && initial["starts"]["1"] == expected[1]
                        && initial["starts"].as_object().unwrap().len() == 2,
                );
                let tank_owners: Vec<_> = initial["objects"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|e| e["type"] == "MTNK")
                    .map(|e| e["owner"].clone())
                    .collect();
                checks.insert(
                    format!("{tag}_original_objects_wait_for_startup_trigger"),
                    tank_owners == [json!("Neutral"), json!("Neutral")],
                );
                let mut stages = Vec::new();
                let mut births = Vec::new();
                let mut previous = 0;
                for tick in 1..=140 {
                    scenario.runtime.advance_frame(
                        &[],
                        22,
                        TickLane::Ordinary,
                        FrameEffects::default(),
                    )?;
                    let current = state(&scenario);
                    if tick == 2 {
                        let tanks: Vec<_> = current["objects"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter(|e| e["type"] == "MTNK")
                            .collect();
                        checks.insert(
                            format!("{tag}_startup_objects_bound_to_actual_players"),
                            tanks.iter().map(|e| e["owner"].clone()).collect::<Vec<_>>()
                                == expected.map(|s| json!(s)),
                        );
                    }
                    let engineers: Vec<_> = current["objects"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|e| e["type"] == "ENGINEER")
                        .cloned()
                        .collect();
                    if engineers.len() != previous {
                        births.push(json!({"tick":tick,"engineers":engineers}));
                        previous = engineers.len();
                    }
                    if [
                        1, 2, 30, 31, 45, 46, 50, 61, 75, 76, 90, 91, 105, 106, 120, 140,
                    ]
                    .contains(&tick)
                    {
                        stages.push(current);
                    }
                    if restore && tick == 50 {
                        let before = state(&scenario);
                        checkpoint::restore_checkpoint(
                            &mut scenario.runtime,
                            &bytes,
                            &root.join(format!("{tag}.bin")),
                        )?;
                        checks.insert(
                            format!("{tag}_checkpoint_immediate"),
                            before == state(&scenario),
                        );
                    }
                }
                let final_state = state(&scenario);
                let engineers: Vec<_> = final_state["objects"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|e| e["type"] == "ENGINEER")
                    .collect();
                checks.insert(
                    format!("{tag}_three_pairs_spawned_no_empty_slot_gift"),
                    engineers.len() == 6
                        && expected.iter().all(|owner| {
                            engineers.iter().filter(|e| e["owner"] == *owner).count() == 3
                        }),
                );
                checks.insert(
                    format!("{tag}_all_gifts_alive"),
                    engineers
                        .iter()
                        .all(|e| e["active"] == true && e["health"].as_i64().unwrap() > 0),
                );
                checks.insert(
                    format!("{tag}_successor_lines_and_empty_slot"),
                    final_state["globals"][0] == true
                        && final_state["globals"][1] == true
                        && final_state["globals"][2] == false,
                );
                checks.insert(
                    format!("{tag}_repeat_cadence"),
                    births
                        .iter()
                        .map(|r| r["tick"].as_u64().unwrap())
                        .collect::<Vec<_>>()
                        == [31, 61, 91],
                );
                let timers = [
                    (1, Some(449)),
                    (45, Some(405)),
                    (46, None),
                    (50, None),
                    (61, None),
                    (75, None),
                    (76, Some(404)),
                    (90, Some(390)),
                    (91, Some(419)),
                    (105, Some(405)),
                    (106, Some(389)),
                    (120, Some(375)),
                    (140, Some(355)),
                ];
                checks.insert(
                    format!("{tag}_countdown_set_stop_resume_extend_shorten"),
                    timers.into_iter().all(|(tick, left)| {
                        let row = stages.iter().find(|row| row["tick"] == tick).unwrap();
                        match left {
                            Some(left) => row["timer"] == json!([left, ""]),
                            None => row["timer"].is_null(),
                        }
                    }),
                );
                let original_tanks: Vec<_> = initial["objects"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|e| e["type"] == "MTNK")
                    .collect();
                checks.insert(
                    format!("{tag}_original_tanks_preserved_after_transfer"),
                    original_tanks.iter().enumerate().all(|(index, original)| {
                        final_state["objects"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|e| e["id"] == original["id"])
                            .is_some_and(|e| {
                                e["active"] == true
                                    && e["type"] == original["type"]
                                    && e["health"] == original["health"]
                                    && e["cell"] == original["cell"]
                                    && e["owner"] == expected[index]
                            })
                    }),
                );
                finals.push(final_state.clone());
                timelines.push(json!({"stages":stages,"births":births}));
                runs.push(json!({"id":tag,"initial":initial,"final":final_state,"timeline":timelines.last(),
                    "runtime_rules_hash":scenario.runtime.resources.rules.simulation_config_hash()}));
            }
            checks.insert(
                format!("{}-{swapped}_continuous_restore_final", mode.as_str()),
                finals[0] == finals[1],
            );
            checks.insert(
                format!("{}-{swapped}_continuous_restore_timeline", mode.as_str()),
                timelines[0] == timelines[1],
            );
        }
    }
    let passed = checks.values().all(|value| *value);
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&json!({"passed":passed,"checks":checks,"runs":runs,
        "input_hashes":{"map":ra2_map_assets::profile::hash(&bytes),
            "mission":ra2_map_assets::profile::hash(&std::fs::read(&entry)?),
            "sdk":ra2_map_assets::profile::hash(ra2_map_mission_script::SDK.as_bytes()),
            "audit":ra2_map_assets::profile::hash(include_bytes!("mission_sdk_multiplayer_audit.rs"))},
        "scope":"TS build/rebuild and production headless pre-Fill launch with actual Skirmish/LAN/WOL GameMode. Two human houses swap starts; real ownership, repeated reinforcement, script progression, empty start and headless save/restore. No network sockets, distributed determinism, GUI or native SAV compatibility claim."}))?,
    )?;
    ensure!(passed, "multiplayer audit failed; inspect report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
