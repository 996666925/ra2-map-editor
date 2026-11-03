//! TS team orders through recruitment, ordinary frames and real member behavior.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    mission::{self, Kind},
    new_map::NewMapOptions,
};
use ra2_map_io::mission_script::{self, Sources, Worker};
use serde_json::json;
use std::path::PathBuf;
use vera20k_core::{
    map::map_file,
    sim::{
        combat::TargetKind,
        mission::MissionType,
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(PathBuf::from(args.next().context("new output directory")?))?;
    ensure!(!root.exists(), "请选择新的审计目录");
    std::fs::create_dir_all(&root)?;
    let bytes = NewMapOptions {
        width: 48,
        height: 48,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(bytes.clone())?;
    let mut doc = MapDocument::new(Baseline::parse(bytes.into(), &map)?);
    for house in ["Americans", "Russians"] {
        mission::create(&mut doc, Kind::House, house, house, None)?;
        doc.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    doc.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    doc.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    doc.add_record("Structures", [20, 30], "Americans", "GAPOWR")?;
    doc.add_record("Infantry", [25, 35], "Americans", "E1")?;
    doc.add_record("Units", [35, 45], "Americans", "AMCV")?;
    doc.add_record("Units", [30, 55], "Americans", "MTNK")?;
    doc.add_record("Infantry", [15, 45], "Americans", "ENGINEER")?;
    doc.add_record("Infantry", [16, 45], "Americans", "ENGINEER")?;
    doc.add_record("Infantry", [17, 48], "Americans", "SENGINEER")?;
    doc.add_record("Infantry", [18, 48], "Americans", "SENGINEER")?;
    doc.add_record("Structures", [43, 55], "Russians", "NAPOWR")?;
    doc.commit("team behavior fixture");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let mut install = Install::open(&game)?;
    let scene = ra2_map_scene::map::load_scene(
        &mut install,
        root.join("base.map").to_str().context("fixture path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let entry = root.join("mission.ts");
    std::fs::write(
        &entry,
        r#"import {defineMission,event,action,order,Direction,Quarry,UnitMission} from './sdk';
import {map} from './map.generated';
export default defineMission(map,{id:'team_behavior',player:'Americans'},m=>{
  m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
  m.configureHouse(m.house('Americans'),{country:m.country('Americans'),playerControl:true,iq:0,allies:[m.house('Americans')]});
  m.configureHouse(m.house('Russians'),{country:m.country('Russians'),iq:0,allies:[m.house('Russians')]});
  const infantry=m.team('infantry',{house:m.house('Americans'),waypoint:m.createWaypoint(30,25,35),
    units:[{type:m.unit('E1'),count:1}],full:true,max:1,reinforce:false,
    orders:[order.face(Direction.East),order.setGlobal(m.globalFlag(0)),order.guard(2),
      order.panic(),order.setGlobal(m.globalFlag(1)),order.guard(2),order.calm(),
      order.setGlobal(m.globalFlag(2)),order.scatter(),order.setGlobal(m.globalFlag(3)),
      order.waitForever(),order.setGlobal(m.globalFlag(4))]});
  const mcv=m.team('mcv',{house:m.house('Americans'),waypoint:m.createWaypoint(31,35,45),
    units:[{type:m.unit('AMCV'),count:1}],full:true,max:1,reinforce:false,
    orders:[order.deploy(),order.waitForever()]});
  const attacker=m.team('attacker',{house:m.house('Americans'),waypoint:m.createWaypoint(32,30,55),
    units:[{type:m.unit('MTNK'),count:1}],full:true,max:1,reinforce:false,
    orders:[order.attack(Quarry.Power),order.setGlobal(m.globalFlag(7)),order.waitForever()]});
  const transfer=m.team('transfer',{house:m.house('Americans'),waypoint:m.createWaypoint(33,15,45),
    units:[{type:m.unit('ENGINEER'),count:2}],full:true,max:1,reinforce:false,
    orders:[order.changeCountry(m.country('Russians')),order.setGlobal(m.globalFlag(8))]});
  const sleepers=m.team('sleepers',{house:m.house('Americans'),waypoint:m.createWaypoint(34,17,48),
    units:[{type:m.unit('SENGINEER'),count:2}],full:true,max:1,reinforce:false,
    orders:[order.mission(UnitMission.Sleep),order.setGlobal(m.globalFlag(9))]});
  m.rule('dispatch',{when:event.sinceScenarioStart(1),once:true,
    actions:[action.recruit(infantry),action.recruit(mcv),action.recruit(attacker),action.recruit(transfer),action.recruit(sleepers),action.setGlobal(m.globalFlag(6))]});
});"#,
    )?;
    mission_script::refresh_types(&entry, &catalog)?;
    let build = Worker::new()?
        .build(&doc, catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?;
    build.build.apply(&mut doc)?;
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(root.join("team-behavior.map"), &bytes)?;
    let mut runtime = campaign::load(&install, &bytes, 0)?;
    let find = |name: &str| {
        runtime
            .simulation
            .entities()
            .iter_sorted()
            .find_map(|(id, e)| {
                (runtime.simulation.interner().resolve(e.type_ref()) == name).then_some(id)
            })
            .with_context(|| format!("authored {name}"))
    };
    let soldier = find("E1")?;
    let mcv = find("AMCV")?;
    let tank = find("MTNK")?;
    let plant = find("NAPOWR")?;
    let witness = find("GAPOWR")?;
    let player = runtime.simulation.entities().get(soldier).unwrap().owner();
    let recipient = runtime.simulation.entities().get(plant).unwrap().owner();
    let transferred_members: Vec<_> = runtime
        .simulation
        .entities()
        .iter_sorted()
        .filter_map(|(id, e)| {
            (runtime.simulation.interner().resolve(e.type_ref()) == "ENGINEER").then_some(id)
        })
        .collect();
    ensure!(transferred_members.len() == 2, "two transfer members");
    let sleeping_members: Vec<_> = runtime
        .simulation
        .entities()
        .iter_sorted()
        .filter_map(|(id, e)| {
            (runtime.simulation.interner().resolve(e.type_ref()) == "SENGINEER").then_some(id)
        })
        .collect();
    ensure!(sleeping_members.len() == 2, "two sleeping members");
    let plant_health = runtime
        .simulation
        .entities()
        .get(plant)
        .unwrap()
        .health
        .current();
    let witness_health = runtime
        .simulation
        .entities()
        .get(witness)
        .unwrap()
        .health
        .current();
    let mut face_east = false;
    let mut panic_fear = false;
    let mut calm_fear = false;
    let mut first_guard = None;
    let mut guard_finished = None;
    let mut scatter_start = None;
    let mut scatter_moved = false;
    let mut wait_frames = 0;
    let mut wait_held = true;
    let mut mcv_recruited = false;
    let mut mcv_unloading = false;
    let mut yard = None;
    let mut quarry_target = false;
    let mut plant_damaged = false;
    let mut no_damage_before_dispatch = true;
    let mut witness_intact = true;
    let mut transfer_team = None;
    let mut transfer_owner_preserved = true;
    let mut sleep_mission = false;
    let mut sleep_frames = 0;
    let mut sleep_held = true;
    let mut both_transferred = false;
    let mut transferred_team_removed = false;
    let mut following_transfer_line_after_owner_change = false;
    let mut transfer_timeline = Vec::new();
    let mut previous_transfer_cursor = None;
    let mut previous_cursor = None;
    let mut timeline = Vec::new();
    for _ in 0..1200 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let sim = &runtime.simulation;
        let frame = sim.session().binary_frame();
        let flags = sim.global_variables();
        for team in sim.team_script_vm().teams_in_order() {
            if team.members().any(|id| transferred_members.contains(&id)) {
                transfer_team.get_or_insert(team.id());
            }
            if team.members().any(|id| sleeping_members.contains(&id)) {
                if team.cursor() == 0 {
                    sleep_mission |= sleeping_members.iter().all(|id| {
                        sim.entities().get(*id).is_some_and(|e| {
                            e.mission.effective().known() == Some(MissionType::Sleep)
                        })
                    });
                    sleep_frames += 1;
                }
                if sleep_frames > 0 {
                    sleep_held &= team.cursor() == 0 && !flags[9];
                }
            }
        }
        if let Some(team_id) = transfer_team {
            if let Some(team) = sim.team_script_vm().team(team_id) {
                transfer_owner_preserved &= team.owner() == player;
                if previous_transfer_cursor != Some(team.cursor()) {
                    transfer_timeline.push(json!({"frame":frame,"cursor":team.cursor(),
                        "team_owner":sim.interner().resolve(team.owner()),
                        "members":team.members().collect::<Vec<_>>()}));
                    previous_transfer_cursor = Some(team.cursor());
                }
            } else if both_transferred && flags[8] {
                transferred_team_removed = true;
            }
        }
        both_transferred |= transferred_members.iter().all(|id| {
            sim.entities()
                .get(*id)
                .is_some_and(|e| e.owner() == recipient)
        });
        following_transfer_line_after_owner_change |= flags[8] && both_transferred;
        let member = sim.entities().get(soldier).context("soldier survives")?;
        // Read the existing save representation; do not expose engine internals
        // or add a diagnostic accessor merely for this audit.
        let saved_member = serde_json::to_value(member)?;
        let fear = saved_member
            .pointer("/infantry/fear_level")
            .and_then(|value| value.as_u64())
            .context("saved infantry fear")?;
        let position = [member.position.rx(), member.position.ry()];
        if let Some(team) = sim
            .team_script_vm()
            .teams_in_order()
            .find(|t| t.members().any(|id| id == soldier))
        {
            let cursor = team.cursor();
            if previous_cursor != Some(cursor) {
                timeline.push(json!({"frame":frame,"cursor":cursor,"fear":fear,
                    "body_facing":member.body_facing_current(frame),"position":position}));
                previous_cursor = Some(cursor);
            }
            if (0..3).contains(&cursor) {
                face_east |= member.body_facing_current(frame) == 0x4000;
            }
            if cursor == 2 {
                first_guard.get_or_insert(frame);
            }
            if cursor == 3 {
                guard_finished.get_or_insert(frame);
                panic_fear |= fear > 0;
            }
            if cursor == 6 {
                calm_fear |= fear == 0;
            }
            if cursor == 8 {
                scatter_start.get_or_insert(position);
            }
            if cursor >= 8 {
                scatter_moved |= scatter_start.is_some_and(|start| start != position);
            }
            if cursor == 10 {
                wait_frames += 1;
            }
            if wait_frames > 0 {
                wait_held &= cursor == 10 && !flags[4];
            }
        } else if wait_frames > 0 {
            wait_held = false;
        }
        if let Some(member) = sim.entities().get(mcv) {
            mcv_recruited |= sim
                .team_script_vm()
                .teams_in_order()
                .any(|t| t.members().any(|id| id == mcv));
            mcv_unloading |= member.mission.effective().known() == Some(MissionType::Unload);
        }
        if yard.is_none() {
            yard = sim.entities().iter_sorted().find_map(|(id, e)| {
                (e.owner() == player && sim.interner().resolve(e.type_ref()) == "GACNST")
                    .then_some((id, [e.position.rx(), e.position.ry()]))
            });
        }
        if let Some(member) = sim.entities().get(tank) {
            let active = sim
                .team_script_vm()
                .teams_in_order()
                .any(|t| t.cursor() == 0 && t.members().any(|id| id == tank));
            quarry_target |=
                active && member.attack_target.target() == Some(TargetKind::Entity(plant));
        }
        let damaged = sim
            .entities()
            .get(plant)
            .is_none_or(|e| e.health.current() < plant_health);
        if flags[6] {
            plant_damaged |= damaged;
        } else {
            no_damage_before_dispatch &= !damaged;
        }
        witness_intact &= sim
            .entities()
            .get(witness)
            .is_some_and(|e| e.owner() == player && e.health.current() == witness_health);
        if wait_frames >= 180
            && yard.is_some()
            && quarry_target
            && plant_damaged
            && transferred_team_removed
            && sleep_frames >= 180
        {
            break;
        }
    }
    let guard_elapsed = first_guard
        .zip(guard_finished)
        .map(|(start, end)| end - start);
    let flags = runtime.simulation.global_variables();
    let checks = json!({
        "infantry_script_markers":flags[..4].iter().all(|set|*set),
        "face_east_reached":face_east,
        "guard_two_seconds_at_least_30_logic_frames":guard_elapsed.is_some_and(|elapsed|elapsed>=30),
        "panic_raised_fear":panic_fear,
        "calm_cleared_fear":calm_fear,
        "scatter_changed_member_cell":scatter_moved,
        "wait_forever_held_180_frames":wait_held&&wait_frames>=180&&!flags[4],
        "mcv_recruited":mcv_recruited,
        "deploy_issued_unload":mcv_unloading,
        "mcv_consumed":runtime.simulation.entities().get(mcv).is_none(),
        "construction_yard_created":yard.is_some_and(|(_,cell)|cell==[34,44]),
        "quarry_power_selected_plant":quarry_target,
        "quarry_attack_damaged_plant":plant_damaged,
        "no_damage_before_ts_dispatch":no_damage_before_dispatch,
        "other_player_building_untouched":witness_intact,
        "sleep_mission_issued_to_both_members":sleep_mission,
        "mission_stays_on_current_line_180_frames":sleep_held&&sleep_frames>=180&&!flags[9],
        "change_country_transferred_both_members":both_transferred,
        "change_country_preserved_team_owner":transfer_team.is_some()&&transfer_owner_preserved,
        "completed_transfer_team_removed":transferred_team_removed,
        "following_transfer_line_after_owner_change":following_transfer_line_after_owner_change
    });
    let passed = checks.as_object().unwrap().values().all(|v| v == true);
    let transfer_state = json!({
        "team":transfer_team.and_then(|id|runtime.simulation.team_script_vm().team(id)),
        "members":transferred_members.iter().map(|id| {
            runtime.simulation.entities().get(*id).map(|e|json!({
                "id":id,"owner":runtime.simulation.interner().resolve(e.owner()),
                "mission":e.mission.effective().raw(),"cell":[e.position.rx(),e.position.ry()]
            }))
        }).collect::<Vec<_>>()
    });
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "passed":passed,"checks":checks,"infantry_timeline":timeline,
            "guard_elapsed_logic_frames":guard_elapsed,"wait_frames":wait_frames,"yard":yard,
            "transfer_state":transfer_state,
            "transfer_timeline":transfer_timeline,"sleep_frames":sleep_frames,
            "scope":"TS build, campaign bootstrap and ordinary frames; no direct state mutation, injected notifications or engine changes; no GUI claim"
        }))?,
    )?;
    ensure!(passed, "team behavior audit failed; see report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
