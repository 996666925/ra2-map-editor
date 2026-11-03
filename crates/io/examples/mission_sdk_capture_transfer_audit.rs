//! TS transferOnCapture -> real engineer capture -> transferred object Tag callback.
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
        command::{Command, CommandEnvelope},
        runtime::SimRuntime,
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;

fn capture(
    runtime: &mut SimRuntime,
    house: vera20k_core::sim::intern::InternedId,
    actor: u64,
    plant: u64,
) -> Result<()> {
    let tick = runtime.simulation.session().tick();
    runtime.advance_frame(
        &[CommandEnvelope::new(
            house,
            tick,
            Command::CaptureBuilding {
                engineer_id: actor,
                target_building_id: plant,
            },
        )],
        22,
        TickLane::Ordinary,
        FrameEffects::default(),
    )?;
    Ok(())
}

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
    doc.add_record("Infantry", [20, 30], "Americans", "ENGINEER")?;
    doc.add_record("Infantry", [22, 30], "Americans", "ENGINEER")?;
    doc.commit("capture transfer fixture");
    let base = doc.snapshot().bytes()?;
    std::fs::write(root.join("base.map"), &base)?;
    let mut install = Install::open(&game)?;
    let scene = ra2_map_scene::map::load_scene(
        &mut install,
        root.join("base.map").to_str().context("fixture path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let source = r#"import {defineMission,event,action,TagRepeat} from './sdk';
import {map} from './map.generated';
export default defineMission(map,{id:'capture_transfer',player:'Americans'},m=>{
  m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
  m.configureHouse(m.house('Americans'),{country:m.country('Americans'),playerControl:true,allies:[m.house('Americans')]});
  m.configureHouse(m.house('Russians'),{country:m.country('Russians'),allies:[m.house('Russians')]});
  const seen=m.globalFlag(0), idle=m.globalFlag(1);
  m.rule('idle_timer',{enabled:false,when:event.sinceEnabled(30),once:true,actions:[action.setGlobal(idle)]});
  const arrived=m.rule('tag_followed_capture',{when:event.entered(m.country('Americans')),once:false,transferOnCapture:true,
    actions:[action.setGlobal(seen),action.enableRule(m.ruleRef('idle_timer'))]});
  m.bindObject(m.object('Infantry:0'),m.tag('capturing_engineer',{rule:arrived,repeat:TagRepeat.Repeat}));
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
    std::fs::write(root.join("capture-transfer.map"), &map_bytes)?;
    let mut runtime = campaign::load(&install, &map_bytes, 0)?;
    let americans = runtime
        .simulation
        .interner()
        .get("Americans")
        .context("Americans")?;
    let russians = runtime
        .simulation
        .interner()
        .get("Russians")
        .context("Russians")?;
    let engineers: Vec<_> = runtime
        .simulation
        .entities()
        .iter_sorted()
        .filter_map(|(id, entity)| {
            (runtime.simulation.interner().resolve(entity.type_ref()) == "ENGINEER"
                && entity.owner() == americans)
                .then_some((id, runtime.simulation.object_coords_cell(id)))
        })
        .collect();
    ensure!(
        engineers.len() == 2,
        "expected two authored Engineers: {engineers:?}"
    );
    let first = engineers
        .iter()
        .find(|(_, cell)| *cell == Some((20, 30)))
        .map(|(id, _)| *id)
        .context("tagged Engineer at (20,30)")?;
    let second = engineers
        .iter()
        .find(|(_, cell)| *cell == Some((22, 30)))
        .map(|(id, _)| *id)
        .context("visitor Engineer at (22,30)")?;
    let plant = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find(|(_, entity)| {
            runtime.simulation.interner().resolve(entity.type_ref()) == "NAPOWR"
                && entity.owner() == russians
        })
        .map(|(id, _)| id)
        .context("Russian power plant")?;
    capture(&mut runtime, americans, first, plant)?;
    for _ in 0..1200 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let captured = runtime
            .simulation
            .entities()
            .get(plant)
            .is_some_and(|building| building.owner() == americans);
        if captured && runtime.simulation.entities().get(first).is_none() {
            break;
        }
    }
    ensure!(
        runtime
            .simulation
            .entities()
            .get(plant)
            .is_some_and(|building| building.owner() == americans)
            && runtime.simulation.entities().get(first).is_none(),
        "first Engineer must capture the plant and be consumed"
    );
    capture(&mut runtime, americans, second, plant)?;
    for _ in 0..1200 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        if runtime.simulation.global_variables()[0] {
            break;
        }
    }
    let first_callback = runtime.simulation.global_variables()[0];
    ensure!(
        first_callback,
        "transferred Tag did not receive the first building-entry event"
    );
    ensure!(
        runtime.simulation.entities().get(second).is_some(),
        "visitor Engineer must remain available for repeat entry"
    );
    for _ in 0..300 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
    }
    ensure!(
        !runtime.simulation.global_variables()[1],
        "the 30-second rule timer must not expire before the repeat entry"
    );
    capture(&mut runtime, americans, second, plant)?;
    for _ in 0..180 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
    }
    let repeat_reset_timer = !runtime.simulation.global_variables()[1];
    let tick = runtime.simulation.session().tick();
    runtime.advance_frame(
        &[CommandEnvelope::new(
            americans,
            tick,
            Command::Move {
                entity_id: second,
                target_rx: 20,
                target_ry: 30,
            },
        )],
        22,
        TickLane::Ordinary,
        FrameEffects::default(),
    )?;
    for _ in 0..900 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        if runtime.simulation.global_variables()[1] {
            break;
        }
    }
    let timer_expired_after_last_entry = runtime.simulation.global_variables()[1];
    let final_plant_owner = runtime
        .simulation
        .interner()
        .resolve(
            runtime
                .simulation
                .entities()
                .get(plant)
                .context("plant after capture")?
                .owner(),
        )
        .to_owned();
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "passed": first_callback && repeat_reset_timer && timer_expired_after_last_entry,
            "transfer_on_capture": true,
            "first_engineer_consumed_by_capture": runtime.simulation.entities().get(first).is_none(),
            "plant_owner_after_capture": final_plant_owner,
            "transferred_tag_fired_on_building_entry": first_callback,
            "repeat_entry_reset_30_second_rule_timer": repeat_reset_timer,
            "timer_expired_after_30_seconds_without_another_entry": timer_expired_after_last_entry,
            "scope": "TS build, production campaign bootstrap, native capture commands and ordinary frames; no GUI presentation claim"
        }))?,
    )?;
    ensure!(
        repeat_reset_timer && timer_expired_after_last_entry,
        "repeat Tag did not reset and then allow the timer to expire"
    );
    println!("{}", root.join("report.json").display());
    Ok(())
}
