//! TS scouting uses the current player's shroud; success is counted on teardown.
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
    map::{map_file, resolved_terrain::NativeCellQuery},
    sim::{
        runtime::SimRuntime,
        vision,
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;
#[path = "support/saved_field.rs"]
mod saved_field;

struct Definitions {
    team: String,
    score: String,
    secondary: String,
}
fn scout(name: &str) -> bool {
    name.starts_with("scout_")
}
fn foreign(name: &str) -> bool {
    name.starts_with("scout_foreign_")
}
fn teardown(name: &str) -> bool {
    name != "success_wait"
}
fn success(name: &str) -> bool {
    name != "plain_eof"
}
fn build(install: &mut Install, root: &Path, name: &str) -> Result<(Vec<u8>, Definitions)> {
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
    doc.add_record("Structures", [20, 46], "Americans", "GAPOWR")?;
    doc.add_record("Structures", [80, 80], "Russians", "NAPOWR")?;
    doc.add_record(
        "Infantry",
        [40, 60],
        if foreign(name) {
            "Russians"
        } else {
            "Americans"
        },
        "ENGINEER",
    )?;
    for key in ["Primary", "Secondary"] {
        doc.set_ini("ENGINEER", key, Some(b"none".to_vec()))?;
    }
    doc.commit("Scout success fixture");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(
        install,
        root.join("base.map").to_str().context("fixture path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let source=r#"import {defineMission,event,action,order,AiTriggerCondition} from './sdk';
import {map,House,Country,Unit} from './map.generated';
export default defineMission(map,{id:'scout_success',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),iq:0,playerControl:true,allies:[m.house(House.Americans)]});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),iq:0,allies:[m.house(House.Russians)]});
 const start=m.createWaypoint(30,40,60);
 const team=m.team('main',{house:m.house(House.__OWNER__),waypoint:start,units:[{type:m.unit(Unit.ENGINEER),count:1}],full:true,max:1,reinforce:false,
  orders:[order.guard(4),__SCOUT____SUCCESS__order.guard(8),order.setGlobal(m.globalFlag(0))__WAIT__]});
 const other=m.team('other',{house:m.house(House.__OWNER__),waypoint:start,units:[{type:m.unit(Unit.ENGINEER),count:1}],max:0,reinforce:false,orders:[order.waitForever()]});
 m.aiTrigger('score',{primary:team,owner:m.country(Country.__OWNER__),condition:AiTriggerCondition.Always,weight:50,minWeight:0,maxWeight:100,enabled:false});
 m.aiTrigger('secondary',{primary:other,secondary:team,owner:m.country(Country.__OWNER__),condition:AiTriggerCondition.Always,weight:50,minWeight:0,maxWeight:100,enabled:false});
 m.rule('dispatch',{when:event.sinceScenarioStart(1),once:true,actions:[__REVEAL__action.recruit(team)]});
 __DELETE__
});"#.replace("__OWNER__",if foreign(name) {"Russians"}else{"Americans"})
        .replace("__SCOUT__",if scout(name) {"order.scout(),"}else{""})
        .replace("__SUCCESS__",if name=="success_twice" {"order.succeed(),order.succeed(),"}else if success(name) {"order.succeed(),"}else{"order.next(),"})
        .replace("__WAIT__",if matches!(name,"success_wait"|"success_delete") {",order.waitForever()"}else{""})
        .replace("__REVEAL__",if name=="scout_foreign_revealed" {"action.revealMap(),"}else if name=="scout_foreign_local" {"action.revealAt(start),"}else{""})
        .replace("__DELETE__",if name=="success_delete" {"m.rule('delete',{when:event.sinceScenarioStart(20),once:true,actions:[action.deleteTeam(team),action.setGlobal(m.globalFlag(1))]});"}else{""});
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
    let ai = |label| {
        mission::ids(&doc, Kind::AiTrigger)
            .into_iter()
            .find(|id| doc.ini_text("AITriggerTypes", id).split(',').next() == Some(label))
            .with_context(|| format!("AITrigger {label}"))
    };
    let definitions = Definitions {
        team,
        score: ai("score")?,
        secondary: ai("secondary")?,
    };
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(root.join("compiled.map"), &bytes)?;
    Ok((bytes, definitions))
}
fn shroud(
    runtime: &SimRuntime,
    fog: &vision::FogState,
    owner: &str,
    cell: (u16, u16),
) -> Result<bool> {
    let sim = &runtime.simulation;
    Ok(vision::cell_is_shrouded(
        fog,
        &NativeCellQuery::canonical(sim.resolved_terrain().context("terrain")?),
        sim.interner().get(owner).context("viewer")?,
        cell,
    ))
}
fn state(runtime: &SimRuntime, definitions: &Definitions) -> Result<Value> {
    let sim = &runtime.simulation;
    let actors=sim.entities().iter_sorted().map(|(id,e)|json!({"id":id,"type":sim.interner().resolve(e.type_ref()),"owner":sim.interner().resolve(e.owner()),"health":e.health.current(),"cell":sim.object_coords_cell(id),"alive":e.lifecycle.object_alive(),"nav":e.navigation.nav_com(),"mission":e.mission.effective().raw()})).collect::<Vec<_>>();
    let teams = sim
        .team_script_vm()
        .teams_in_order()
        .filter(|t| {
            t.team_type_id()
                .is_some_and(|id| sim.interner().resolve(id) == definitions.team)
        })
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
    let records = saved_field::read(sim.team_script_vm(), &["ai_trigger_records"])?;
    let record = |id: &str| -> Result<Value> {
        let id = sim.interner().get(id).context("AI trigger id")?;
        let key = serde_json::to_value(id)?.to_string();
        Ok(records
            .get(&key)
            .context("serialized AI trigger record")?
            .clone())
    };
    let mut fog = Vec::new();
    for owner in ["Americans", "Russians"] {
        let mut samples = Vec::new();
        for cell in [(40, 60), (80, 80)] {
            samples.push(json!({"cell":cell,"shrouded":shroud(runtime,sim.fog(),owner,cell)?}));
        }
        if let Some(e) = sim
            .entities()
            .iter_sorted()
            .find(|(_, e)| sim.interner().resolve(e.type_ref()) == "ENGINEER")
        {
            let cell = sim.object_coords_cell(e.0).context("member cell")?;
            samples.push(json!({"cell":cell,"shrouded":shroud(runtime,sim.fog(),owner,cell)?}));
        }
        fog.push(json!({"owner":owner,"samples":samples}));
    }
    let houses=sim.houses().order().iter().map(|id|json!({"id":sim.interner().resolve(*id),"outcome":sim.houses()[id].outcome(),"units_lost":sim.houses()[id].stats().units_lost()})).collect::<Vec<_>>();
    Ok(
        json!({"actors":actors,"teams":teams,"score":record(&definitions.score)?,"secondary":record(&definitions.secondary)?,"fog_samples":fog,"houses":houses,"flags":sim.global_variables()[..2]}),
    )
}
fn audit(
    install: &Install,
    bytes: &[u8],
    root: &Path,
    name: &str,
    definitions: &Definitions,
    restore: bool,
) -> Result<Value> {
    std::fs::create_dir_all(root)?;
    let mut runtime = campaign::load(install, bytes, 1)?;
    let initial = state(&runtime, definitions)?;
    let originals = initial["actors"].as_array().unwrap();
    let member = originals
        .iter()
        .find(|a| a["type"] == "ENGINEER")
        .context("original engineer")?;
    let mut selected = None;
    let mut succeeded = None;
    let mut following = None;
    let mut ended = None;
    let mut checkpoint_state = None;
    let mut trace = Vec::new();
    let mut created = false;
    let mut identities_preserved = true;
    let mut witnesses_preserved = true;
    let mut record_unchanged_while_live = true;
    let mut target_stable = true;
    let mut last_target = None;
    for frame in 0..1000 {
        let prior_fog =
            (scout(name) && selected.is_none()).then(|| runtime.simulation.fog().clone());
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let tick = runtime.simulation.session().tick();
        let current = state(&runtime, definitions)?;
        let actors = current["actors"].as_array().unwrap();
        identities_preserved &= originals.iter().all(|a| {
            actors.iter().any(|b| {
                a["id"] == b["id"]
                    && a["type"] == b["type"]
                    && a["owner"] == b["owner"]
                    && a["health"] == b["health"]
                    && b["alive"] == true
            })
        });
        witnesses_preserved &= originals
            .iter()
            .filter(|a| a["id"] != member["id"])
            .all(|a| actors.contains(a));
        if let Some(team) = current["teams"].as_array().unwrap().first() {
            created = true;
            record_unchanged_while_live &= current["score"] == initial["score"];
            if scout(name) && team["cursor"] == 1 && !team["mission_target"].is_null() {
                let target = &team["mission_target"];
                if let Some(last) = &last_target {
                    target_stable &= last == target;
                }
                last_target = Some(target.clone());
                if selected.is_none() {
                    let cell = (
                        target["Cell"]["x"].as_u64().context("scout target x")? as u16,
                        target["Cell"]["y"].as_u64().context("scout target y")? as u16,
                    );
                    let fog = prior_fog.as_ref().context("prior scout fog")?;
                    selected = Some(
                        json!({"tick":tick,"target":cell,"player_shrouded_before":shroud(&runtime,fog,"Americans",cell)?,"team_shrouded_before":shroud(&runtime,fog,if foreign(name) {"Russians"}else{"Americans"},cell)?,"state":current}),
                    );
                }
            }
            if team["succeeded"] == true && succeeded.is_none() {
                succeeded = Some(json!({"tick":tick,"state":current}));
            }
        } else if created && ended.is_none() {
            ended = Some(json!({"tick":tick,"state":current}));
        }
        if current["flags"][0] == true && following.is_none() {
            following = Some(json!({"tick":tick,"state":current}));
        }
        if frame % 15 == 0 {
            trace.push(json!({"tick":tick,"state":current}));
        }
        let due = if scout(name) {
            selected.is_some()
        } else {
            tick == 100
        };
        if restore && checkpoint_state.is_none() && due {
            checkpoint::restore_checkpoint(&mut runtime, bytes, &root.join("checkpoint.bin"))?;
            let after = state(&runtime, definitions)?;
            checkpoint_state = Some(
                json!({"tick":tick,"preserved":after==current,"before":current,"after":after}),
            );
        }
    }
    let final_state = state(&runtime, definitions)?;
    let final_member = final_state["actors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == member["id"])
        .context("member survives")?;
    let near_target = selected.as_ref().is_none_or(|s| {
        let c = &final_member["cell"];
        let t = &s["target"];
        (c[0].as_i64().unwrap() - t[0].as_i64().unwrap()).pow(2)
            + (c[1].as_i64().unwrap() - t[1].as_i64().unwrap()).pow(2)
            <= 9
    });
    let checks = BTreeMap::from([
        (
            "original_objects_preserve_id_type_owner_health",
            identities_preserved,
        ),
        ("witness_buildings_unchanged", witnesses_preserved),
        (
            "only_authored_main_team_created",
            created
                && final_state["teams"].as_array().unwrap().len() == usize::from(!teardown(name)),
        ),
        ("scout_selects_cell", !scout(name) || selected.is_some()),
        (
            "unrevealed_player_target_is_shrouded_before_selection",
            !scout(name)
                || name == "scout_foreign_revealed"
                || selected
                    .as_ref()
                    .is_some_and(|s| s["player_shrouded_before"] == true),
        ),
        (
            "foreign_hidden_target_is_already_known_to_team",
            name != "scout_foreign_hidden"
                || selected
                    .as_ref()
                    .is_some_and(|s| s["team_shrouded_before"] == false),
        ),
        (
            "revealed_player_falls_back_to_center",
            name != "scout_foreign_revealed"
                || selected.as_ref().is_some_and(|s| {
                    s["target"] == member["cell"] && s["player_shrouded_before"] == false
                }),
        ),
        ("scout_retains_selected_cell", target_stable),
        ("scout_reaches_selected_cell", near_target),
        (
            "scout_moves_when_start_is_known_to_player",
            !matches!(name, "scout_player" | "scout_foreign_local")
                || final_member["cell"] != member["cell"],
        ),
        (
            "success_latch_set_only_by_succeed",
            succeeded.is_some() == success(name),
        ),
        ("following_script_marker_runs", following.is_some()),
        (
            "success_records_unchanged_until_teardown",
            record_unchanged_while_live,
        ),
        (
            "exact_primary_attempt_count",
            final_state["score"]["attempts"] == i32::from(teardown(name)),
        ),
        (
            "exact_primary_success_count",
            final_state["score"]["successes"] == i32::from(teardown(name) && success(name)),
        ),
        (
            "secondary_team_reference_does_not_record_result",
            final_state["secondary"] == initial["secondary"],
        ),
        (
            "team_teardown_matches_eof_or_delete",
            ended.is_some() == teardown(name),
        ),
        (
            "teardown_releases_member_destination",
            !teardown(name) || final_member["nav"].is_null(),
        ),
        (
            "no_player_result_or_unit_losses",
            final_state["houses"]
                .as_array()
                .unwrap()
                .iter()
                .all(|h| h["outcome"].is_null() && h["units_lost"] == 0),
        ),
        (
            "checkpoint_preserves_listed_state",
            !restore
                || checkpoint_state
                    .as_ref()
                    .is_some_and(|s| s["preserved"] == true),
        ),
    ]);
    std::fs::write(
        root.join("observations.json"),
        serde_json::to_vec_pretty(&trace)?,
    )?;
    Ok(
        json!({"passed":checks.values().all(|b|*b),"checks":checks,"initial":initial,"final":final_state,"selected":selected,"succeeded":succeeded,"following":following,"ended":ended,"checkpoint":checkpoint_state}),
    )
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(args.next().context("new output directory")?)?;
    ensure!(
        args.next().is_none() && !root.exists(),
        "output directory must be new"
    );
    std::fs::create_dir_all(&root)?;
    let mut install = Install::open(&game)?;
    let mut cases = Vec::new();
    for name in [
        "scout_player",
        "scout_foreign_hidden",
        "scout_foreign_local",
        "scout_foreign_revealed",
        "success_wait",
        "success_eof",
        "success_twice",
        "plain_eof",
        "success_delete",
    ] {
        let folder = root.join(name);
        let (bytes, definitions) = build(&mut install, &folder, name)?;
        let continuous = audit(
            &install,
            &bytes,
            &folder.join("continuous"),
            name,
            &definitions,
            false,
        )?;
        let restored = audit(
            &install,
            &bytes,
            &folder.join("restored"),
            name,
            &definitions,
            true,
        )?;
        let stages = ["selected", "succeeded", "following", "ended"]
            .iter()
            .all(|key| continuous[key] == restored[key]);
        cases.push(json!({"case":name,"listed_final_states_match":continuous["final"]==restored["final"],"continuous_ticks_and_stage_states_match":stages,"continuous":continuous,"restored":restored}));
    }
    let passed = cases.iter().all(|c| {
        c["continuous"]["passed"] == true
            && c["restored"]["passed"] == true
            && c["listed_final_states_match"] == true
    });
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":passed,"cases":cases,"scope":"TS recruit, scout, local/full player reveal, success/next/guard/EOF or DeleteTeam; disabled primary/secondary AITrigger records observed through existing Serialize only. Ordinary frames and selected-target or frame100 headless checkpoint. No fog, target, RNG, member, position or tracking injection. Not GPU, multiplayer, GUI save, all-state equality or native pixel parity."}),
        )?,
    )?;
    ensure!(passed, "scout/success audit failed; inspect report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
