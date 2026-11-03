//! TS boarding, an authored water crossing, landing and a mid-water checkpoint.
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
    rules::terrain_rules::LandType,
    sim::{
        runtime::SimRuntime,
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;

fn build(install: &mut Install, root: &Path, carrier: &str) -> Result<(Vec<u8>, String)> {
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
    doc.add_record("Units", [50, 64], "Americans", carrier)?;
    for x in 46..=49 {
        for y in 61..=63 {
            doc.add_record("Infantry", [x, y], "Americans", "ENGINEER")?;
        }
    }
    doc.commit("Water transport fixture");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(
        install,
        root.join("base.map").to_str().context("fixture path")?,
    )?;
    ra2_map_scene::shore_tools::plan(
        &doc,
        &scene,
        ra2_map_scene::cliff_tools::rectangle([58, 50], [74, 82]),
        Some(true),
    )?
    .apply(&mut doc)?;
    doc.commit("Author a complete water and shore rectangle");
    let catalog = mission_script::catalog(&doc, &scene);
    let source = r#"import {defineMission,event,action,order,UnloadMode} from './sdk';
import {map,House,Country,Unit} from './map.generated';
export default defineMission(map,{id:'water_transport',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),iq:0,playerControl:true,allies:[m.house(House.Americans)]});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),iq:0,allies:[m.house(House.Russians)]});
 const convoy=m.team('convoy',{house:m.house(House.Americans),waypoint:m.createWaypoint(30,50,64),
  units:[{type:m.unit(Unit.__CARRIER__),count:1},{type:m.unit(Unit.ENGINEER),count:12}],
  max:1,full:true,reinforce:false,
  orders:[order.load(),order.waitLoaded(),order.setGlobal(m.globalFlag(0)),order.guard(1),
   order.moveTo(m.createWaypoint(31,66,64)),order.setGlobal(m.globalFlag(1)),order.guard(2),
   order.moveTo(m.createWaypoint(32,82,64)),order.unload(UnloadMode.KeepAll),
   order.setGlobal(m.globalFlag(2)),order.waitForever()]});
 m.rule('dispatch',{when:event.sinceScenarioStart(1),once:true,actions:[action.recruit(convoy)]});
});"#.replace("__CARRIER__", carrier);
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
    worker
        .build(&doc, catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(&mut doc)?;
    ensure!(bytes == doc.snapshot().bytes()?, "unchanged rebuild");
    let team = mission::ids(&doc, Kind::Team)
        .into_iter()
        .find(|id| doc.ini_text(id, "Name") == "convoy")
        .context("convoy")?;
    std::fs::write(root.join("compiled.map"), &bytes)?;
    Ok((bytes, team))
}

fn state(runtime: &SimRuntime, team_type: &str) -> Value {
    let sim = &runtime.simulation;
    let actors = sim.entities().iter_sorted().map(|(id,e)| {
        let cell = sim.object_coords_cell(id);
        let land = cell.and_then(|(x,y)| sim.resolved_terrain()?.cell(x,y)).map(|c| c.land_type);
        let cargo = e.passenger_role.cargo().map(|c|c.passengers().to_vec()).unwrap_or_default();
        json!({"id":id,"type":sim.interner().resolve(e.type_ref()),"owner":sim.interner().resolve(e.owner()),
            "health":e.health.current(),"alive":e.lifecycle.object_alive(),"limbo":e.lifecycle.in_limbo(),
            "cell":cell,"land":land,"cargo":cargo})
    }).collect::<Vec<_>>();
    let teams=sim.team_script_vm().teams_in_order().filter(|t|t.team_type_id().is_some_and(|id|sim.interner().resolve(id)==team_type))
        .map(|t|json!({"id":t.id(),"owner":sim.interner().resolve(t.owner()),"cursor":t.cursor(),"members":t.members().collect::<Vec<_>>()})).collect::<Vec<_>>();
    let house = sim
        .interner()
        .get("Americans")
        .and_then(|id| sim.houses().get(&id));
    json!({"tick":sim.session().tick(),"actors":actors,"teams":teams,"flags":sim.global_variables()[..3],"losses":house.map(|h|h.stats().units_lost())})
}

fn audit(
    install: &Install,
    bytes: &[u8],
    root: &Path,
    carrier_type: &str,
    team: &str,
    restore: bool,
) -> Result<Value> {
    std::fs::create_dir_all(root)?;
    let mut runtime = campaign::load(install, bytes, 1)?;
    let initial = state(&runtime, team);
    let original = initial["actors"].as_array().context("initial actors")?;
    let carrier = original
        .iter()
        .find(|a| a["type"] == carrier_type)
        .context("carrier")?["id"]
        .as_u64()
        .context("carrier id")?;
    let passengers = original
        .iter()
        .filter(|a| a["type"] == "ENGINEER")
        .map(|a| a["id"].as_u64().unwrap())
        .collect::<BTreeSet<_>>();
    let all_members = passengers
        .iter()
        .copied()
        .chain([carrier])
        .collect::<BTreeSet<_>>();
    let water = LandType::Water.as_index();
    let terrain = runtime
        .simulation
        .resolved_terrain()
        .context("authored terrain")?;
    let route = [[50, 64], [60, 64], [66, 64], [72, 64], [82, 64]]
        .map(|[x, y]| json!({"cell":[x,y],"land":terrain.cell(x,y).map(|c|c.land_type)}));
    let mut trace = vec![initial.clone()];
    let mut loaded = None;
    let mut middle = None;
    let mut unloaded = None;
    let mut completed = None;
    let mut checkpoint_state = None;
    let mut water_cells = BTreeSet::new();
    let mut recruited = false;
    let mut cargo_hidden = false;
    let mut identities_preserved = true;
    let mut witnesses_preserved = true;
    let mut cargo_crossing_intact = true;
    for _ in 0..2400 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let current = state(&runtime, team);
        let actors = current["actors"].as_array().unwrap();
        let ship = actors
            .iter()
            .find(|a| a["id"] == carrier)
            .context("surviving carrier")?;
        let cargo = ship["cargo"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect::<BTreeSet<_>>();
        identities_preserved &= actors.len() == original.len()
            && original.iter().all(|a| {
                actors.iter().any(|b| {
                    a["id"] == b["id"]
                        && a["type"] == b["type"]
                        && a["owner"] == b["owner"]
                        && a["health"] == b["health"]
                        && b["alive"] == true
                })
            });
        witnesses_preserved &= original
            .iter()
            .filter(|a| a["type"] == "NAPOWR" || a["type"] == "GAPOWR")
            .all(|a| actors.contains(a));
        if let Some(t) = current["teams"].as_array().unwrap().first() {
            let members = t["members"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap())
                .collect::<BTreeSet<_>>();
            recruited |= members == all_members;
            if cargo == passengers {
                cargo_hidden |= members == BTreeSet::from([carrier])
                    && actors
                        .iter()
                        .filter(|a| a["type"] == "ENGINEER")
                        .all(|a| a["limbo"] == true);
            }
        }
        if cargo == passengers && current["flags"][0] == true && loaded.is_none() {
            loaded = Some(current.clone());
        }
        if loaded.is_some() && ship["land"] == water {
            cargo_crossing_intact &= cargo == passengers
                && actors
                    .iter()
                    .filter(|a| a["type"] == "ENGINEER")
                    .all(|a| a["limbo"] == true);
        }
        if cargo == passengers && ship["land"] == water {
            let cell: [u16; 2] = serde_json::from_value(ship["cell"].clone())?;
            water_cells.insert(cell);
            if current["flags"][1] == true && middle.is_none() {
                middle = Some(current.clone());
            }
            if checkpoint_state.is_none() && cell[0] >= 64 {
                let mut after = current.clone();
                if restore {
                    checkpoint::restore_checkpoint(
                        &mut runtime,
                        bytes,
                        &root.join("mid-water.bin"),
                    )?;
                    after = state(&runtime, team);
                }
                checkpoint_state =
                    Some(json!({"preserved":current==after,"before":current,"after":after}));
            }
        }
        if loaded.is_some() && cargo.is_empty() && unloaded.is_none() {
            unloaded = Some(current.clone());
        }
        if current["flags"][2] == true && completed.is_none() {
            completed = Some(current.clone());
        }
        trace.push(current);
    }
    let final_state = state(&runtime, team);
    let actors = final_state["actors"].as_array().unwrap();
    let ship = actors.iter().find(|a| a["id"] == carrier).unwrap();
    let final_team = final_state["teams"].as_array().unwrap().first();
    let final_members = final_team
        .map(|t| {
            t["members"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let checks = BTreeMap::from([
        (
            "stock_capacity_twelve_without_map_override",
            runtime
                .resources
                .rules
                .object(carrier_type)
                .is_some_and(|t| t.passengers == 12),
        ),
        (
            "authored_land_water_land_route",
            route[0]["land"] != water
                && route[1]["land"] == water
                && route[2]["land"] == water
                && route[3]["land"] == water
                && route[4]["land"] != water,
        ),
        (
            "original_carrier_twelve_infantry_two_buildings",
            passengers.len() == 12 && original.len() == 15,
        ),
        ("recruited_original_members", recruited),
        (
            "ordinary_boarding_hides_passengers_and_removes_from_team",
            cargo_hidden,
        ),
        (
            "boarding_finishes_on_starting_land",
            loaded.as_ref().is_some_and(|s| {
                s["actors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|a| a["id"] == carrier)
                    .is_some_and(|a| {
                        a["cell"][0]
                            .as_i64()
                            .zip(a["cell"][1].as_i64())
                            .is_some_and(|(x, y)| (x - 50).pow(2) + (y - 64).pow(2) <= 4)
                            && a["land"] != water
                    })
            }),
        ),
        (
            "loaded_transport_crosses_at_least_ten_distinct_water_cells",
            water_cells.len() >= 10,
        ),
        (
            "all_original_cargo_remains_aboard_through_water_crossing",
            cargo_crossing_intact,
        ),
        (
            "middle_waypoint_flag_executes_with_full_cargo_on_water",
            middle.is_some(),
        ),
        (
            "checkpoint_is_reached_in_water_with_full_original_cargo",
            checkpoint_state.is_some(),
        ),
        (
            "checkpoint_preserves_listed_state",
            checkpoint_state
                .as_ref()
                .is_some_and(|c| c["preserved"] == true),
        ),
        (
            "actual_unload_and_following_flag",
            unloaded.is_some() && completed.is_some(),
        ),
        (
            "carrier_finishes_on_opposite_land_without_cargo",
            ship["cell"] == json!([82, 64]) && ship["land"] != water && ship["cargo"] == json!([]),
        ),
        (
            "all_original_passengers_disembark_on_opposite_bank",
            actors.iter().filter(|a| a["type"] == "ENGINEER").all(|a| {
                a["limbo"] == false
                    && a["land"] != water
                    && a["cell"][0].as_u64().is_some_and(|x| x >= 78)
            }),
        ),
        (
            "keep_all_rejoins_exact_original_members",
            final_members == all_members,
        ),
        (
            "following_wait_forever_reached",
            final_team.is_some_and(|t| t["cursor"] == 10 && t["owner"] == "Americans"),
        ),
        (
            "all_objects_survive_identity_owner_health",
            identities_preserved,
        ),
        ("witness_buildings_unchanged", witnesses_preserved),
        ("no_unit_loss", final_state["losses"] == 0),
    ]);
    std::fs::write(
        root.join("observations.json"),
        serde_json::to_vec_pretty(&trace)?,
    )?;
    Ok(
        json!({"passed":checks.values().all(|b|*b),"checks":checks,"route":route,"water_cells":water_cells,"initial":initial,"final":final_state,"loaded":loaded,"middle":middle,"unloaded":unloaded,"completed":completed,"checkpoint":checkpoint_state}),
    )
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(args.next().context("new output directory")?)?;
    ensure!(
        !root.exists() && args.next().is_none(),
        "output must be new"
    );
    std::fs::create_dir_all(&root)?;
    let mut install = Install::open(&game)?;
    let mut cases = Vec::new();
    for carrier in ["LCRF", "SAPC", "YHVR"] {
        let folder = root.join(carrier);
        let (bytes, team) = build(&mut install, &folder, carrier)?;
        let continuous = audit(
            &install,
            &bytes,
            &folder.join("continuous"),
            carrier,
            &team,
            false,
        )?;
        let restored = audit(
            &install,
            &bytes,
            &folder.join("restored"),
            carrier,
            &team,
            true,
        )?;
        let restored_again = audit(
            &install,
            &bytes,
            &folder.join("restored-again"),
            carrier,
            &team,
            true,
        )?;
        let timeline_matches = std::fs::read(folder.join("continuous/observations.json"))?
            == std::fs::read(folder.join("restored/observations.json"))?;
        let restored_runs_match = restored == restored_again
            && std::fs::read(folder.join("restored/observations.json"))?
                == std::fs::read(folder.join("restored-again/observations.json"))?;
        let final_matches = continuous["final"] == restored["final"];
        let stages_match = ["loaded", "middle", "unloaded", "completed"]
            .iter()
            .all(|k| continuous[k] == restored[k]);
        let final_actor_differences = continuous["final"]["actors"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|before| {
                let after = restored["final"]["actors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|a| a["id"] == before["id"]);
                (after != Some(before)).then(|| json!({"continuous":before,"restored":after}))
            })
            .collect::<Vec<_>>();
        cases.push(json!({"carrier":carrier,"continuous":continuous,"restored":restored,
            "restored_again":restored_again,"independent_restored_runs_match":restored_runs_match,
            "listed_timeline_matches":timeline_matches,"listed_final_state_matches":final_matches,
            "stage_ticks_and_states_match":stages_match,"final_actor_differences":final_actor_differences}));
    }
    let passed = cases.iter().all(|c| {
        c["continuous"]["passed"] == true
            && c["restored"]["passed"] == true
            && c["restored_again"]["passed"] == true
            && c["independent_restored_runs_match"] == true
    });
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&json!({"passed":passed,"cases":cases,
        "scope":"TS recruit/load/waitLoaded/moveTo/unload KeepAll on editor-authored retail water/coasts. Stock twelve-seat LCRF/SAPC/YHVR with twelve original infantry. Production campaign loader, ordinary frames and two independent public headless mid-water checkpoint runs. No terrain, cargo, position or membership injection; not GUI save, network, original SAV or full internal-state equivalence. Successful arrival and repeatability are required. Continuous-vs-restored exact timeline/stage/final equality and actual actor differences are reported separately and not required."}))?,
    )?;
    ensure!(passed, "water transport audit failed; inspect report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
