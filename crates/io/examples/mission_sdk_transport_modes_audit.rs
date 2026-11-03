//! TS unload modes, surviving Team membership and transport return paths.
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
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
use vera20k_core::{
    map::map_file,
    sim::{
        runtime::SimRuntime,
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;

fn mode(name: &str) -> &'static str {
    if name.ends_with("release_all") {
        return "ReleaseAll";
    }
    match name {
        "release_passengers" => "ReleasePassengers",
        "release_transports" => "ReleaseTransports",
        "release_all" | "return_release_all" => "ReleaseAll",
        _ => "KeepAll",
    }
}
fn build(install: &mut Install, root: &Path, name: &str) -> Result<(Vec<u8>, String)> {
    std::fs::create_dir_all(root)?;
    let initial = NewMapOptions {
        width: 64,
        height: 64,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(initial.clone())?;
    let mut doc = MapDocument::new(Baseline::parse(initial.into(), &map)?);
    for house in ["Americans", "Russians"] {
        mission::create(&mut doc, Kind::House, house, house, None)?;
        doc.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    doc.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    doc.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    doc.add_record("Structures", [28, 52], "Americans", "NAPOWR")?;
    doc.add_record("Structures", [80, 80], "Russians", "GAPOWR")?;
    doc.add_record("Units", [40, 56], "Americans", "HTK")?;
    doc.add_record("Infantry", [38, 55], "Americans", "ENGINEER")?;
    doc.add_record("Infantry", [38, 56], "Americans", "ENGINEER")?;
    doc.set_ini(
        "HTK",
        "Passengers",
        Some(if name.contains("partial") {
            b"3".to_vec()
        } else {
            b"2".to_vec()
        }),
    )?;
    for ty in ["HTK", "ENGINEER"] {
        for key in ["Primary", "Secondary"] {
            doc.set_ini(ty, key, Some(b"none".to_vec()))?;
        }
    }
    doc.commit("Transport unload mode fixture");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(
        install,
        root.join("base.map").to_str().context("fixture path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let source=r#"import {defineMission,event,action,order,UnloadMode} from './sdk';
import {map,House,Country,Unit} from './map.generated';
export default defineMission(map,{id:'transport_modes',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),iq:0,playerControl:true,allies:[m.house(House.Americans)]});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),iq:0,allies:[m.house(House.Russians)]});
 const convoy=m.team('convoy',{house:m.house(House.Americans),waypoint:m.createWaypoint(30,40,56),
  units:[{type:m.unit(Unit.HTK),count:1},{type:m.unit(Unit.ENGINEER),count:2}],
  max:1,full:true,reinforce:false,transportsReturnOnUnload:__RETURNS__,
  orders:[order.load(),__WAIT__order.setGlobal(m.globalFlag(0)),order.guard(10),
   order.moveTo(m.createWaypoint(31,52,64)),order.unload(UnloadMode.__MODE__),
   order.setGlobal(m.globalFlag(1)),order.waitForever()]});
 m.rule('dispatch',{when:event.sinceScenarioStart(1),once:true,actions:[action.recruit(convoy)]});
});"#.replace("__RETURNS__",if name.starts_with("return_") {"true"} else {"false"}).replace("__MODE__",mode(name)).replace("__WAIT__",if name.contains("partial") {""}else{"order.waitLoaded(),"});
    let entry = root.join("mission.ts");
    std::fs::write(&entry, source)?;
    mission_script::refresh_types(&entry, &catalog)?;
    Worker::new()?
        .build(&doc, catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(&mut doc)?;
    let team_type = mission::ids(&doc, Kind::Team)
        .into_iter()
        .find(|id| doc.ini_text(id, "Name") == "convoy")
        .context("generated convoy type")?;
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(root.join("compiled.map"), &bytes)?;
    Ok((bytes, team_type))
}
fn state(runtime: &SimRuntime, team_type: &str) -> Result<Value> {
    let sim = &runtime.simulation;
    let actors=sim.entities().iter_sorted().map(|(id,e)|json!({"id":id,"type":sim.interner().resolve(e.type_ref()),"owner":sim.interner().resolve(e.owner()),"health":e.health.current(),"alive":e.lifecycle.object_alive(),"limbo":e.lifecycle.in_limbo(),"cell":sim.object_coords_cell(id),"passengers":e.passenger_role.passenger_count(),"archive":e.archive_target()})).collect::<Vec<_>>();
    let teams=sim.team_script_vm().teams_in_order().filter(|t|t.team_type_id().is_some_and(|id|sim.interner().resolve(id)==team_type)).map(|t|json!({"id":t.id(),"owner":sim.interner().resolve(t.owner()),"cursor":t.cursor(),"members":t.members().collect::<Vec<_>>()})).collect::<Vec<_>>();
    let house = sim
        .houses()
        .get(&sim.interner().get("Americans").context("player id")?)
        .context("House")?;
    Ok(
        json!({"actors":actors,"teams":teams,"flags":sim.global_variables()[..2],"unit_losses":house.stats().units_lost()}),
    )
}
fn audit(
    install: &Install,
    bytes: &[u8],
    root: &Path,
    name: &str,
    team_type: &str,
    restore: bool,
) -> Result<Value> {
    std::fs::create_dir_all(root)?;
    let mut runtime = campaign::load(install, bytes, 1)?;
    let initial = state(&runtime, team_type)?;
    let actors = initial["actors"].as_array().unwrap();
    let carrier = actors
        .iter()
        .find(|a| a["type"] == "HTK")
        .and_then(|a| a["id"].as_u64())
        .context("authored HTK")?;
    let passengers = actors
        .iter()
        .filter(|a| a["type"] == "ENGINEER")
        .map(|a| a["id"].as_u64().unwrap())
        .collect::<BTreeSet<_>>();
    ensure!(
        passengers.len() == 2 && actors.len() == 5,
        "original transport, two passengers and two witness buildings"
    );
    let mut original_members = passengers.clone();
    original_members.insert(carrier);
    let mut all_recruited = false;
    let mut members_hidden_in_cargo = false;
    let mut loaded = None;
    let mut unloaded = None;
    let mut following_line = None;
    let mut checkpoint_state = None;
    let mut trace = Vec::new();
    let mut last_team = None;
    let mut actors_preserved = true;
    let mut witnesses_preserved = true;
    let mut max_distance = 0_i64;
    for frame in 0..3600 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let sim = &runtime.simulation;
        let tick = sim.session().tick();
        let current = state(&runtime, team_type)?;
        let current_actors = current["actors"].as_array().unwrap();
        let transport = current_actors
            .iter()
            .find(|a| a["id"] == carrier)
            .context("carrier survives")?;
        actors_preserved &= actors.iter().all(|a| {
            current_actors.iter().any(|b| {
                a["id"] == b["id"]
                    && a["type"] == b["type"]
                    && a["owner"] == b["owner"]
                    && a["health"] == b["health"]
                    && b["alive"] == true
            })
        });
        witnesses_preserved &= actors
            .iter()
            .filter(|a| a["type"] == "NAPOWR" || a["type"] == "GAPOWR")
            .all(|a| current_actors.contains(a));
        if let Some(cell) = transport["cell"]
            .as_array()
            .filter(|_| transport["passengers"] == 2)
        {
            let dx = cell[0].as_i64().unwrap() - 40;
            let dy = cell[1].as_i64().unwrap() - 56;
            max_distance = max_distance.max(dx * dx + dy * dy);
        }
        let team = current["teams"].as_array().unwrap().first();
        if let Some(team) = team {
            let members = team["members"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| id.as_u64().unwrap())
                .collect::<BTreeSet<_>>();
            all_recruited |= members == original_members;
            if transport["passengers"] == 2 {
                members_hidden_in_cargo |= members == BTreeSet::from([carrier])
                    && current_actors
                        .iter()
                        .filter(|a| a["type"] == "ENGINEER")
                        .all(|a| a["limbo"] == true);
            }
        }
        if transport["passengers"] == 2 && current["flags"][0] == true && loaded.is_none() {
            loaded = Some(json!({"tick":tick,"state":current}));
        }
        if loaded.is_some() && transport["passengers"] == 0 && unloaded.is_none() {
            unloaded = Some(json!({"tick":tick,"state":current}));
        }
        if current["flags"][1] == true && following_line.is_none() {
            following_line = Some(json!({"tick":tick,"state":current}));
        }
        if last_team.as_ref() != Some(&current["teams"]) || frame % 30 == 0 {
            trace.push(json!({"tick":tick,"state":current}));
            last_team = Some(current["teams"].clone());
        }
        if restore && loaded.is_some() && checkpoint_state.is_none() {
            checkpoint::restore_checkpoint(&mut runtime, bytes, &root.join("loaded.bin"))?;
            let after = state(&runtime, team_type)?;
            checkpoint_state = Some(
                json!({"tick":tick,"preserved":current==after,"before":current,"after":after}),
            );
        }
    }
    let final_state = state(&runtime, team_type)?;
    let returns = name.starts_with("return_");
    let partial = name.contains("partial");
    let archived_return = returns && partial;
    let release_all = mode(name) == "ReleaseAll";
    let expected_members = if release_all {
        BTreeSet::new()
    } else if returns || mode(name) == "ReleaseTransports" {
        passengers.clone()
    } else if mode(name) == "ReleasePassengers" {
        BTreeSet::from([carrier])
    } else {
        original_members
    };
    let team = final_state["teams"].as_array().unwrap().first();
    let members = team
        .map(|t| {
            t["members"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let transport = final_state["actors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == carrier)
        .unwrap();
    let cell = transport["cell"].as_array().context("carrier on map")?;
    let near = |x: i64, y: i64| {
        (cell[0].as_i64().unwrap() - x).pow(2) + (cell[1].as_i64().unwrap() - y).pow(2) < 25
    };
    let checks = BTreeMap::from([
        (
            "declared_capacity_matches_full_or_partial_fixture",
            runtime
                .resources
                .rules
                .object("HTK")
                .is_some_and(|ty| ty.passengers == if partial { 3 } else { 2 }),
        ),
        (
            "recruited_original_transport_and_two_passengers",
            all_recruited,
        ),
        (
            "loaded_original_infantry_in_limbo_and_out_of_team",
            members_hidden_in_cargo,
        ),
        (
            "loading_script_finished_before_movement",
            loaded.as_ref().is_some_and(|r| {
                r["state"]["actors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|a| a["id"] == carrier)
                    .is_some_and(|a| a["cell"] == json!([40, 56]))
            }),
        ),
        ("transport_really_moves_with_cargo", max_distance >= 100),
        (
            "real_unload_completes",
            unloaded.as_ref().is_some_and(|r| {
                r["state"]["actors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|a| a["id"] == carrier)
                    .is_some_and(|a| a["cell"] == json!([52, 64]))
            }) && transport["passengers"] == 0
                && final_state["actors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|a| a["limbo"] == false),
        ),
        (
            "all_original_objects_survive_with_owner_and_health",
            actors_preserved,
        ),
        ("witness_buildings_unchanged", witnesses_preserved),
        ("no_unit_loss", final_state["unit_losses"] == 0),
        (
            "mode_selects_exact_remaining_members",
            members == expected_members,
        ),
        ("empty_team_is_removed", team.is_none() == release_all),
        (
            "following_line_runs_only_with_remaining_members",
            following_line.is_some() != release_all,
        ),
        (
            "nonempty_team_waits_on_following_script_line",
            team.is_none_or(|t| {
                t["cursor"] == if partial { 6 } else { 7 } && t["owner"] == "Americans"
            }),
        ),
        (
            "carrier_returns_or_stays_at_landing",
            if archived_return {
                near(40, 56)
            } else {
                near(52, 64)
            },
        ),
        (
            "return_archive_recorded_during_load",
            loaded.as_ref().is_some_and(|r| {
                r["state"]["actors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|a| a["id"] == carrier)
                    .is_some_and(|a| {
                        a["archive"]
                            == if archived_return {
                                json!({"Cell":[40,56]})
                            } else {
                                Value::Null
                            }
                    })
            }),
        ),
        (
            "return_archive_cleared_after_unload",
            !returns || transport["archive"].is_null(),
        ),
        (
            "checkpoint_preserves_listed_state",
            !restore
                || checkpoint_state
                    .as_ref()
                    .is_some_and(|r| r["preserved"] == true),
        ),
    ]);
    std::fs::write(
        root.join("observations.json"),
        serde_json::to_vec_pretty(&trace)?,
    )?;
    Ok(
        json!({"passed":checks.values().all(|b|*b),"checks":checks,"initial":initial,"final":final_state,"loaded":loaded,"unloaded":unloaded,"following_line":following_line,"checkpoint":checkpoint_state}),
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
        "keep_all",
        "release_passengers",
        "release_transports",
        "release_all",
        "return_keep",
        "return_release_all",
        "return_partial_keep",
        "return_partial_release_all",
    ] {
        let folder = root.join(name);
        let (bytes, team_type) = build(&mut install, &folder, name)?;
        let continuous = audit(
            &install,
            &bytes,
            &folder.join("continuous"),
            name,
            &team_type,
            false,
        )?;
        let restored = audit(
            &install,
            &bytes,
            &folder.join("restored"),
            name,
            &team_type,
            true,
        )?;
        let final_match = continuous["final"] == restored["final"];
        let callbacks_match = ["loaded", "unloaded", "following_line"]
            .iter()
            .all(|key| continuous[key] == restored[key]);
        cases.push(json!({"case":name,"continuous":continuous,"restored":restored,"listed_final_states_match":final_match,"continuous_ticks_and_stage_states_match":callbacks_match}));
    }
    let passed = cases.iter().all(|c| {
        c["continuous"]["passed"] == true
            && c["restored"]["passed"] == true
            && c["listed_final_states_match"] == true
    });
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":passed,"cases":cases,"scope":"TS recruit, load, waitLoaded, move, unload and following script lines with live retail HTK and map-declared two/three seats. Underfilled return variants omit waitLoaded. Public ordinary frames and loaded headless checkpoint only. No passenger, position, membership, lifecycle or event injection; not GUI, aircraft transport, naval transport or all-state equality."}),
        )?,
    )?;
    ensure!(passed, "transport modes audit failed; inspect report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
