//! TS grinder, tank bunker and Bio Reactor orders through ordinary frames.
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
    sim::world::{FrameEffects, TickLane},
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
    doc.add_record("Structures", [20, 30], "Americans", "YAGRND")?;
    doc.add_record("Structures", [35, 25], "Americans", "NATBNK")?;
    doc.add_record("Structures", [42, 40], "Americans", "YAPOWR")?;
    doc.add_record("Structures", [20, 35], "Americans", "GAPOWR")?;
    doc.add_record("Structures", [42, 30], "Russians", "NAPOWR")?;
    doc.add_record("Infantry", [24, 25], "Americans", "E1")?;
    doc.add_record("Units", [29, 25], "Americans", "YTNK")?;
    doc.add_record("Infantry", [40, 32], "Americans", "INIT")?;
    doc.commit("special building entry fixture");
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
        r#"import {defineMission,event,action,order} from './sdk';
import {map} from './map.generated';
export default defineMission(map,{id:'special_building_entry',player:'Americans'},m=>{
  m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
  m.configureHouse(m.house('Americans'),{country:m.country('Americans'),playerControl:true,iq:0,allies:[m.house('Americans')]});
  m.configureHouse(m.house('Russians'),{country:m.country('Russians'),iq:0,allies:[m.house('Russians')]});
  const grinder=m.team('grinder_team',{house:m.house('Americans'),waypoint:m.createWaypoint(30,24,25),
    units:[{type:m.unit('E1'),count:1}],orders:[order.enterGrinder(),order.waitForever()],full:true,max:1,reinforce:false});
  const tank=m.team('tank_bunker_team',{house:m.house('Americans'),waypoint:m.createWaypoint(31,29,25),
    units:[{type:m.unit('YTNK'),count:1}],orders:[order.enterTankBunker(),order.waitForever()],full:true,max:1,reinforce:false});
  const reactor=m.team('reactor_team',{house:m.house('Americans'),waypoint:m.createWaypoint(32,40,32),
    units:[{type:m.unit('INIT'),count:1}],orders:[order.enterBioReactor(),order.waitForever()],full:true,max:1,reinforce:false});
  m.rule('dispatch',{when:event.sinceScenarioStart(1),once:true,
    actions:[action.recruit(grinder),action.recruit(tank),action.recruit(reactor)]});
});"#,
    )?;
    mission_script::refresh_types(&entry, &catalog)?;
    let build = Worker::new()?
        .build(&doc, catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?;
    build.build.apply(&mut doc)?;
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(root.join("special-building-entry.map"), &bytes)?;
    let mut runtime = campaign::load(&install, &bytes, 0)?;
    let (grinder, bunker, reactor, engineer, tank, recruit) = {
        let sim = &runtime.simulation;
        let find = |name: &str, cell: Option<[u16; 2]>| {
            sim.entities()
                .iter_sorted()
                .find_map(|(id, e)| {
                    (sim.interner().resolve(e.type_ref()) == name
                        && cell.is_none_or(|c| [e.position.rx(), e.position.ry()] == c))
                    .then_some(id)
                })
                .with_context(|| format!("{name} {cell:?}"))
        };
        let engineer = find("E1", Some([24, 25]))?;
        let tank = find("YTNK", Some([29, 25]))?;
        let recruit = find("INIT", Some([40, 32]))?;
        (
            find("YAGRND", Some([20, 30]))?,
            find("NATBNK", Some([35, 25]))?,
            find("YAPOWR", Some([42, 40]))?,
            engineer,
            tank,
            recruit,
        )
    };
    let initial_total = runtime
        .simulation
        .houses()
        .values()
        .map(|house| house.economy().credits())
        .sum::<i32>();
    let mut grinder_order_completed = false;
    let mut grinder_removed = false;
    let mut grinder_untouched = true;
    let mut bunker_order_completed = false;
    let mut bunker_installed = false;
    let mut tank_kept_on_map = true;
    let mut reactor_order_completed = false;
    let mut reactor_absorbed = false;
    let mut reactor_member_limbo = true;
    let mut reactor_passenger_link = false;
    let mut other_buildings_preserved = true;
    let mut team_entry_timeline = Vec::new();
    let mut credited_refund = 0;
    let mut grinder_consumed_frame = None;
    let mut bunker_installed_frame = None;
    let mut reactor_absorbed_frame = None;
    for _ in 0..1400 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let sim = &runtime.simulation;
        let grinder_team = sim
            .team_script_vm()
            .teams_in_order()
            .find(|t| t.members().any(|id| id == engineer));
        grinder_removed |= sim.entities().get(engineer).is_none();
        grinder_order_completed |= grinder_removed;
        if grinder_removed {
            grinder_consumed_frame.get_or_insert(sim.session().binary_frame());
        }
        grinder_untouched &= sim
            .entities()
            .get(grinder)
            .is_some_and(|e| sim.interner().resolve(e.type_ref()) == "YAGRND");
        let tank_team = sim
            .team_script_vm()
            .teams_in_order()
            .find(|t| t.members().any(|id| id == tank));
        let linked = sim.entities().get(bunker).unwrap().bunker.occupant() == Some(tank)
            && sim
                .entities()
                .get(tank)
                .unwrap()
                .bunker
                .link()
                .installed_in()
                == Some(bunker);
        bunker_installed |= linked;
        bunker_order_completed |= linked;
        if linked {
            bunker_installed_frame.get_or_insert(sim.session().binary_frame());
        }
        tank_kept_on_map &= sim.entities().get(tank).is_some_and(|e| !e.in_limbo());
        let reactor_team = sim
            .team_script_vm()
            .teams_in_order()
            .find(|t| t.members().any(|id| id == recruit));
        let cargo = sim.entities().get(reactor).unwrap().passenger_role.cargo();
        let passenger_link = sim
            .entities()
            .get(recruit)
            .and_then(|e| e.passenger_role.inside_transport())
            .is_some_and(|(id, _)| id == reactor);
        reactor_absorbed |= cargo.is_some_and(|c| c.passengers().contains(&recruit));
        reactor_order_completed |= reactor_absorbed;
        if reactor_absorbed {
            reactor_absorbed_frame.get_or_insert(sim.session().binary_frame());
        }
        reactor_member_limbo &= if reactor_absorbed {
            sim.entities().get(recruit).is_some_and(|e| e.in_limbo())
        } else {
            true
        };
        reactor_passenger_link |= passenger_link;
        other_buildings_preserved &= [bunker, reactor, grinder]
            .iter()
            .all(|id| sim.entities().get(*id).is_some());
        let total = sim
            .houses()
            .values()
            .map(|house| house.economy().credits())
            .sum::<i32>();
        credited_refund = total - initial_total;
        let team_states=[
            grinder_team.map(|t|json!({"id":t.id(),"cursor":t.cursor(),"members":t.members().collect::<Vec<_>>()})),
            tank_team.map(|t|json!({"id":t.id(),"cursor":t.cursor(),"members":t.members().collect::<Vec<_>>()})),
            reactor_team.map(|t|json!({"id":t.id(),"cursor":t.cursor(),"members":t.members().collect::<Vec<_>>()})),
        ];
        if !team_states.iter().all(Option::is_none) && team_entry_timeline.len() < 12 {
            team_entry_timeline.push(
                json!({"frame":sim.session().binary_frame(),"teams":team_states,
                "engineer_alive":sim.entities().contains(engineer),"tank_linked":linked,
                "reactor_contains_member":cargo.is_some_and(|c|c.passengers().contains(&recruit))}),
            );
        }
        if grinder_removed && bunker_installed && reactor_absorbed {
            break;
        }
    }
    let checks = json!({
        "grinder_order_consumed_engineer":grinder_order_completed&&grinder_removed,
        "grinder_building_survived":grinder_untouched,
        "tank_bunker_order_installed_reciprocal_link":bunker_order_completed&&bunker_installed,
        "installed_tank_stays_live_on_map":tank_kept_on_map,
        "bio_reactor_order_loaded_recruit":reactor_order_completed&&reactor_absorbed,
        "absorbed_recruit_is_limbo":reactor_member_limbo,
        "recruit_references_reactor":reactor_passenger_link,
        "all_target_buildings_survived":other_buildings_preserved,
        "grinder_refund_nonnegative":credited_refund>=0
    });
    let passed = checks.as_object().unwrap().values().all(|v| v == true);
    let report = json!({"passed":passed,"checks":checks,"initial_total_credits":initial_total,
        "final_total_credits":initial_total+credited_refund,"observed_credit_delta":credited_refund,
        "grinder_refund":credited_refund,"team_entry_timeline":team_entry_timeline,
        "grinder_consumed_frame":grinder_consumed_frame,"tank_bunker_installed_frame":bunker_installed_frame,
        "reactor_absorbed_frame":reactor_absorbed_frame,
        "scope":"TS build, production campaign bootstrap and ordinary frames; no direct state mutation, event injection, engine changes or GUI claim"});
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    ensure!(
        passed,
        "special building entry audit failed; see report.json"
    );
    println!("{}", root.join("report.json").display());
    Ok(())
}
