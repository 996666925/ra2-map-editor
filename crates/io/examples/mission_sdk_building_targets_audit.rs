//! TS building selection and ordinary movement/attack, without engine hooks.
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
        runtime::SimRuntime,
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;
#[path = "support/saved_field.rs"]
mod saved_field;

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
    doc.add_record("Structures", [82, 82], "Russians", "NAPOWR")?;
    let owner = if name.starts_with("own_") {
        "Americans"
    } else {
        "Russians"
    };
    for cell in [[44, 56], [64, 72]] {
        doc.add_record("Structures", cell, owner, "GAPOWR")?;
    }
    doc.add_record("Units", [38, 56], "Americans", "MTNK")?;
    // Natural hostile spatial-threat publication around the distant candidate.
    for cell in [[66, 75], [67, 76], [66, 77]] {
        doc.add_record("Infantry", cell, "Russians", "E2")?;
    }
    doc.set_ini("GAPOWR", "Crewed", Some(b"no".to_vec()))?;
    for (key, value) in [
        ("Primary", "none"),
        ("Secondary", "none"),
        ("Speed", "0"),
        ("ThreatPosed", "400"),
    ] {
        doc.set_ini("E2", key, Some(value.as_bytes().to_vec()))?;
    }
    if !name.starts_with("attack_") && name != "waypoint_building" {
        for key in ["Primary", "Secondary"] {
            doc.set_ini("MTNK", key, Some(b"none".to_vec()))?;
        }
    }
    doc.commit("Building target fixture");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(
        install,
        root.join("base.map").to_str().context("fixture path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let selection = if name.ends_with("least") {
        "LeastThreat"
    } else if name.ends_with("most") {
        "MostThreat"
    } else if name.ends_with("farthest") {
        "Farthest"
    } else {
        "Nearest"
    };
    let building = if name.ends_with("absent") {
        "NAHAND"
    } else {
        "GAPOWR"
    };
    let command = if name.starts_with("waypoint_") {
        format!(
            "order.attackWaypointBuilding(m.createWaypoint(31,{},{}))",
            if name == "waypoint_empty" { 50 } else { 44 },
            if name == "waypoint_empty" { 60 } else { 56 }
        )
    } else {
        format!(
            "order.{}(m.building(Building.{building}),BuildingSelection.{selection})",
            if name.starts_with("own_") {
                "moveToOwnBuilding"
            } else if name.starts_with("attack_") {
                "attackEnemyBuilding"
            } else {
                "moveToEnemyBuilding"
            }
        )
    };
    let source = r#"import {defineMission,event,action,order,BuildingSelection} from './sdk';
import {map,House,Country,Unit,Building} from './map.generated';
export default defineMission(map,{id:'building_targets',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),iq:0,playerControl:true,allies:[m.house(House.Americans)]});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),iq:0,allies:[m.house(House.Russians)]});
 const team=m.team('main',{house:m.house(House.Americans),waypoint:m.createWaypoint(30,38,56),units:[{type:m.unit(Unit.MTNK),count:1}],max:1,full:true,reinforce:false,
  orders:[order.guard(4),__COMMAND__,order.setGlobal(m.globalFlag(0)),order.waitForever()]});
 m.rule('dispatch',{when:event.sinceScenarioStart(1),once:true,actions:[action.recruit(team)]});
});"#.replace("__COMMAND__",&command);
    let entry = root.join("mission.ts");
    std::fs::write(&entry, source)?;
    mission_script::refresh_types(&entry, &catalog)?;
    Worker::new()?
        .build(&doc, catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(&mut doc)?;
    let team = mission::ids(&doc, Kind::Team)
        .into_iter()
        .find(|id| doc.ini_text(id, "Name") == "main")
        .context("main TeamType")?;
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(root.join("compiled.map"), &bytes)?;
    Ok((bytes, team))
}
fn state(runtime: &SimRuntime, team_type: &str) -> Result<Value> {
    let sim = &runtime.simulation;
    let actors=sim.entities().iter_sorted().map(|(id,e)|json!({"id":id,"type":sim.interner().resolve(e.type_ref()),"owner":sim.interner().resolve(e.owner()),"health":e.health.current(),"cell":sim.object_coords_cell(id),"alive":e.lifecycle.object_alive()})).collect::<Vec<_>>();
    let teams = sim
        .team_script_vm()
        .teams_in_order()
        .filter(|t| {
            t.team_type_id()
                .is_some_and(|id| sim.interner().resolve(id) == team_type)
        })
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"actors":actors,"teams":teams,"flag":sim.global_variables()[0]}))
}
fn candidates(runtime: &SimRuntime) -> Result<Value> {
    let sim = &runtime.simulation;
    let house = sim
        .houses()
        .get(&sim.interner().get("Americans").context("owner id")?)
        .context("owner House")?;
    let grid = saved_field::read(house, &["spatial_threat"])?;
    let values = grid.as_array().context("threat array")?;
    let side = if values.len() == 16900 { 130 } else { 258 };
    let mut result = Vec::new();
    for (id, e) in sim
        .entities()
        .iter_sorted()
        .filter(|(_, e)| sim.interner().resolve(e.type_ref()) == "GAPOWR")
    {
        let c =
            vera20k_core::sim::movement::ground_pose::object_get_coords(e, sim.resolved_terrain());
        let cell = vera20k_core::util::lepton::lepton_to_cell_packed;
        let index = side + 1 + i32::from(cell(c.x)) / 4 + side * (i32::from(cell(c.y)) / 4);
        result.push(json!({"id":id,"cell":sim.object_coords_cell(id),"center_cell":[cell(c.x),cell(c.y)],"threat":values[index as usize],"owner":sim.interner().resolve(e.owner())}));
    }
    Ok(json!(result))
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
    let mover = actors
        .iter()
        .find(|a| a["type"] == "MTNK")
        .context("original tank")?
        .clone();
    let targets = actors
        .iter()
        .filter(|a| a["type"] == "GAPOWR")
        .cloned()
        .collect::<Vec<_>>();
    ensure!(
        targets.len() == 2 && actors.len() == 8,
        "two candidates, tank, three threats and two witnesses"
    );
    let far = name.ends_with("most") || name.ends_with("farthest");
    let expected = targets
        .iter()
        .min_by_key(|a| {
            let x = a["cell"][0].as_i64().unwrap();
            if far { -x } else { x }
        })
        .context("expected target")?;
    let attack = name.starts_with("attack_") || name == "waypoint_building";
    let absent = name.ends_with("absent") || name == "waypoint_empty";
    let mut checkpoint_state = None;
    let mut selected = None;
    let mut following = None;
    let mut scoring = None;
    let mut trace = Vec::new();
    let mut other_preserved = true;
    let mut mover_preserved = true;
    let mut selected_damage = false;
    for frame in 0..2200 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let tick = runtime.simulation.session().tick();
        let current = state(&runtime, team_type)?;
        let now = current["actors"].as_array().unwrap();
        other_preserved &= actors
            .iter()
            .filter(|a| a["id"] != mover["id"] && (!attack || a["id"] != expected["id"]))
            .all(|a| now.contains(a));
        mover_preserved &= now.iter().any(|a| {
            a["id"] == mover["id"]
                && a["type"] == mover["type"]
                && a["owner"] == mover["owner"]
                && a["health"] == mover["health"]
                && a["alive"] == true
        });
        if attack {
            selected_damage |= now
                .iter()
                .find(|a| a["id"] == expected["id"])
                .is_none_or(|a| a["health"].as_i64() < expected["health"].as_i64());
        }
        if let Some(team) = current["teams"].as_array().unwrap().first()
            && !team["mission_target"].is_null()
            && selected.is_none()
        {
            scoring = Some(candidates(&runtime)?);
            selected = Some(json!({"tick":tick,"state":current}));
        }
        if current["flag"] == true && following.is_none() {
            following = Some(json!({"tick":tick,"state":current}));
        }
        if frame % 30 == 0 {
            trace.push(json!({"tick":tick,"state":current}));
        }
        if restore && tick == 100 {
            checkpoint::restore_checkpoint(&mut runtime, bytes, &root.join("selected.bin"))?;
            let after = state(&runtime, team_type)?;
            checkpoint_state =
                Some(json!({"preserved":after==current,"before":current,"after":after}));
        }
    }
    let final_state = state(&runtime, team_type)?;
    let final_mover = final_state["actors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == mover["id"])
        .context("tank survives")?;
    let target = selected
        .as_ref()
        .map(|s| &s["state"]["teams"][0]["mission_target"]);
    let cell_distance = |a: &Value, b: &Value| {
        (a[0].as_i64().unwrap() - b[0].as_i64().unwrap()).pow(2)
            + (a[1].as_i64().unwrap() - b[1].as_i64().unwrap()).pow(2)
    };
    let selected_correct = if absent {
        selected.is_none()
    } else if attack {
        target == Some(&json!({"Object":expected["id"]}))
    } else {
        target
            .and_then(|t| t.get("Cell"))
            .is_some_and(|c| cell_distance(&json!([c["x"], c["y"]]), &expected["cell"]) < 25)
    };
    let threats_distinct = scoring.as_ref().is_none_or(|s| {
        let rows = s.as_array().unwrap();
        let near = rows
            .iter()
            .min_by_key(|a| a["cell"][0].as_i64().unwrap())
            .unwrap();
        let far = rows
            .iter()
            .max_by_key(|a| a["cell"][0].as_i64().unwrap())
            .unwrap();
        near["threat"].as_i64() < far["threat"].as_i64()
    });
    let checks = BTreeMap::from([
        ("original_tank_survives_with_owner_health", mover_preserved),
        ("other_original_objects_unchanged", other_preserved),
        ("correct_building_or_approach_selected", selected_correct),
        (
            "natural_threat_orders_candidates",
            absent || threats_distinct,
        ),
        (
            "attack_really_damages_selected_building",
            !attack || selected_damage,
        ),
        (
            "movement_reaches_selected_building",
            attack || absent || cell_distance(&final_mover["cell"], &expected["cell"]) < 25,
        ),
        (
            "missing_target_does_not_move_tank",
            !absent || final_mover["cell"] == mover["cell"],
        ),
        ("following_script_line_runs", following.is_some()),
        (
            "team_keeps_original_member",
            final_state["teams"][0]["members"]
                .as_array()
                .is_some_and(|m| m.len() == 1 && m[0]["id"] == mover["id"]),
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
        json!({"passed":checks.values().all(|b|*b),"checks":checks,"initial":initial,"final":final_state,"selected":selected,"scoring":scoring,"following":following,"checkpoint":checkpoint_state}),
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
        "own_nearest",
        "own_farthest",
        "own_least",
        "own_most",
        "enemy_nearest",
        "enemy_farthest",
        "enemy_least",
        "enemy_most",
        "own_absent",
        "enemy_absent",
        "attack_nearest",
        "waypoint_building",
        "waypoint_empty",
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
        cases.push(json!({"case":name,"listed_final_states_match":continuous["final"]==restored["final"],"continuous_ticks_and_selected_states_match":continuous["selected"]==restored["selected"] && continuous["following"]==restored["following"],"continuous":continuous,"restored":restored}));
    }
    let passed = cases.iter().all(|c| {
        c["continuous"]["passed"] == true
            && c["restored"]["passed"] == true
            && c["listed_final_states_match"] == true
    });
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":passed,"cases":cases,"scope":"TS recruit and building selection, ordinary movement and real weapon attack; map-declared stationary hostile threats, existing serialized threat field read only, public headless checkpoint at frame 100. Not GUI, multiplayer, designated enemy/allied-passive selection, full-state equality or native pixel parity."}),
        )?,
    )?;
    ensure!(passed, "building targets audit failed; inspect report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
