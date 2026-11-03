//! TS order.spyWaypoint -> real Agent entry into a spyable power plant.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    new_map::NewMapOptions,
};
use ra2_map_io::mission_script::{self, Sources, Worker};
use std::path::PathBuf;
use vera20k_core::{
    map::map_file,
    sim::{
        spy_infiltration::InfiltrationEffect,
        world::{FrameEffects, SimSoundEvent, TickLane},
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
    doc.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    doc.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    for house in ["Americans", "Russians"] {
        ra2_map_document::mission::create(
            &mut doc,
            ra2_map_document::mission::Kind::House,
            house,
            house,
            None,
        )?;
        doc.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    doc.add_record("Structures", [30, 40], "Russians", "NAPOWR")?;
    doc.add_record("Infantry", [20, 30], "Americans", "SPY")?;
    doc.commit("spy waypoint fixture");
    let base = doc.snapshot().bytes()?;
    std::fs::write(root.join("base.map"), &base)?;
    let mut install = Install::open(&game)?;
    let scene = ra2_map_scene::map::load_scene(
        &mut install,
        root.join("base.map").to_str().context("fixture path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let source = r#"import {defineMission,event,action,order} from './sdk';
import {map} from './map.generated';
export default defineMission(map,{id:'spy_waypoint',player:'Americans'},m=>{
  m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
  m.configureHouse(m.house('Americans'),{country:m.country('Americans'),playerControl:true,allies:[m.house('Americans')]});
  m.configureHouse(m.house('Russians'),{country:m.country('Russians'),allies:[m.house('Russians')]});
  const rendezvous=m.createWaypoint(30,25,25);
  const plant=m.createWaypoint(31,30,40);
  const spy=m.team('agent',{house:m.house('Americans'),waypoint:rendezvous,
    units:[{type:m.unit('SPY'),count:1}],
    orders:[order.spyWaypoint(plant),order.waitForever()],full:true,max:1,reinforce:false});
  m.rule('dispatch_spy',{when:event.sinceEnabled(1),once:true,
    actions:[action.recruit(spy)]});
});"#;
    let entry = root.join("mission.ts");
    std::fs::write(&entry, source)?;
    mission_script::refresh_types(&entry, &catalog)?;
    let worker = Worker::new()?;
    let build = worker
        .build(&doc, catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?;
    build.build.apply(&mut doc)?;
    let map_bytes = doc.snapshot().bytes()?;
    std::fs::write(root.join("spy-waypoint.map"), &map_bytes)?;

    let mut runtime = campaign::load(&install, &map_bytes, 0)?;
    let player = runtime
        .simulation
        .interner()
        .get("Americans")
        .context("player")?;
    let target_house = runtime
        .simulation
        .interner()
        .get("Russians")
        .context("target house")?;
    let spy_id = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find(|(_, entity)| {
            runtime.simulation.interner().resolve(entity.type_ref()) == "SPY"
                && runtime.simulation.interner().resolve(entity.owner()) == "Americans"
        })
        .map(|(id, _)| id)
        .context("authored spy")?;
    let plant_id = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find(|(_, entity)| {
            runtime.simulation.interner().resolve(entity.type_ref()) == "NAPOWR"
                && runtime.simulation.interner().resolve(entity.owner()) == "Russians"
        })
        .map(|(id, _)| id)
        .context("authored target plant")?;
    let plant_health = runtime
        .simulation
        .entities()
        .get(plant_id)
        .unwrap()
        .health
        .current();
    let mut recruited = false;
    let mut ordered_to_capture = false;
    let mut infiltrations = 0;
    for _ in 0..1200 {
        let output = runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        for event in output.sound_events {
            if let SimSoundEvent::BuildingInfiltrated {
                owner,
                infiltrator,
                effect,
                ..
            } = event
                && owner == target_house
                && infiltrator == player
                && effect == InfiltrationEffect::Power
            {
                infiltrations += 1;
            }
        }
        for team in runtime.simulation.team_script_vm().teams_in_order() {
            if team.owner() == player && team.members().any(|id| id == spy_id) {
                recruited = true;
                if let Some(spy) = runtime.simulation.entities().get(spy_id) {
                    ordered_to_capture |= spy.mission.current().known()
                        == Some(vera20k_core::sim::mission::MissionType::Capture);
                }
            }
        }
        if runtime.simulation.entities().get(spy_id).is_none() && infiltrations == 1 {
            break;
        }
    }
    ensure!(
        recruited
            && ordered_to_capture
            && infiltrations == 1
            && runtime.simulation.entities().get(spy_id).is_none(),
        "TS must recruit the map spy, issue Capture and complete exactly one real infiltration (recruited={recruited}, capture={ordered_to_capture}, events={infiltrations})"
    );
    let plant = runtime
        .simulation
        .entities()
        .get(plant_id)
        .context("plant after infiltration")?;
    ensure!(
        plant.health.current() == plant_health
            && runtime.simulation.interner().resolve(plant.owner()) == "Russians",
        "spy infiltration must preserve the target building itself"
    );
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "passed": true,
            "map_spy_recruited_by_ts": recruited,
            "spy_waypoint_issued_capture_mission": ordered_to_capture,
            "actual_infiltration_events": infiltrations,
            "infiltration_effect": "Power",
            "target_house": "Russians",
            "spy_consumed_by_agent_entry": runtime.simulation.entities().get(spy_id).is_none(),
            "plant_preserved": true,
            "scope": "TS build, production campaign bootstrap and ordinary frames with installed resources; no GUI message/radar presentation or EVA playback claim"
        }))?,
    )?;
    println!("{}", root.join("report.json").display());
    Ok(())
}
