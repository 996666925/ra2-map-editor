//! Paid repeated production -> TS recruitment -> cargo crate -> movement pickup.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    mission::{self, Kind},
    new_map::NewMapOptions,
};
use ra2_map_io::mission_script::{self, Sources, Worker};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};
use vera20k_core::{
    map::map_file,
    sim::{
        command::{Command, CommandEnvelope, ProductionEvent},
        production::{ProductionCategory, ProductionFactoryVisit},
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;

fn build(install: &mut Install, root: &Path) -> Result<Vec<u8>> {
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
    for (cell, house, ty) in [
        ([28, 52], "Americans", "NAPOWR"),
        ([40, 60], "Americans", "GAWEAP"),
        ([82, 82], "Russians", "NAPOWR"),
    ] {
        doc.add_record("Structures", cell, house, ty)?;
    }
    doc.add_record("Infantry", [32, 58], "Americans", "ENGINEER")?;
    for (s, k, v) in [
        ("Basic", "Player", "Americans"),
        ("Basic", "MultiplayerOnly", "no"),
        ("General", "BuildSpeed", "0.1"),
        ("NAPOWR", "Power", "2000"),
        ("CrateRules", "CrateMinimum", "0"),
        ("CrateRules", "CrateMaximum", "0"),
        ("MTNK", "Prerequisite", ""),
        ("MTNK", "Owner", "Americans"),
        ("MTNK", "TechLevel", "1"),
        ("MTNK", "RequiredHouses", ""),
        ("MTNK", "ForbiddenHouses", ""),
        ("MTNK", "Primary", "none"),
        ("MTNK", "Secondary", "none"),
    ] {
        doc.set_ini(s, k, Some(v.as_bytes().to_vec()))?;
    }
    doc.commit("controlled lottery prerequisites");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let scene =
        ra2_map_scene::map::load_scene(install, root.join("base.map").to_str().context("path")?)?;
    let catalog = mission_script::catalog(&doc, &scene);
    let source = r#"import {defineMission} from './sdk';
import {map,House,Country,Unit} from './map.generated';
export default defineMission(map,{id:'crate_lottery',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),credits:100,iq:0,playerControl:true,allies:[m.house(House.Americans)]});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),credits:100,iq:0,allies:[m.house(House.Russians)]});
 m.crateLottery('draw',{ticket:m.unit(Unit.MTNK),rewards:[m.unit(Unit.HTK),m.unit(Unit.FV)],cost:200,
   buyers:[{house:m.house(House.Americans),waypoint:m.createWaypoint(30,40,60)}]});
});"#;
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

fn audit(install: &Install, bytes: &[u8], root: &Path, mode: &str, restore: bool) -> Result<Value> {
    std::fs::create_dir_all(root)?;
    let mut runtime = campaign::load(install, bytes, 1)?;
    let owner = runtime
        .simulation
        .interner()
        .get("Americans")
        .context("owner")?;
    let ticket = runtime
        .simulation
        .interner()
        .get("MTNK")
        .context("ticket")?;
    let collector = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find(|(_, e)| runtime.simulation.interner().resolve(e.type_ref()) == "ENGINEER")
        .context("collector")?
        .0;
    let initial_money = runtime.simulation.houses()[&owner]
        .economy()
        .available_money();
    let mut produced = BTreeSet::new();
    let mut delivered = BTreeSet::new();
    let mut drops = BTreeSet::new();
    let mut rewards = BTreeSet::new();
    let mut inputs = Vec::new();
    let mut observations = Vec::new();
    let mut purchases = 0;
    let mut cancelled = false;
    let mut restored = false;
    let mut target = None;
    for frame in 0..2400 {
        let sim = &runtime.simulation;
        let tick = sim.session().tick();
        let factory = sim
            .production()
            .factories()
            .view(owner, ProductionCategory::Vehicle);
        let mut commands = Vec::new();
        let buy = mode != "idle"
            && ((frame == 60)
                || (mode == "repeat" && purchases == 1 && rewards.len() == 1 && frame > 500));
        let cancel = mode == "cancel"
            && !cancelled
            && factory
                .as_ref()
                .is_some_and(|f| f.progress >= 10 && f.progress < 54);
        if buy || cancel {
            let event = if cancel {
                ProductionEvent::Abandon
            } else {
                ProductionEvent::Produce
            };
            let record = sim
                .encode_production_record(owner, event, ticket, &runtime.resources.rules)
                .context("PRODUCE/ABANDON")?;
            commands.push(CommandEnvelope::synchronized(record, tick));
            inputs.push(json!({"tick":tick,"event":if cancel {"abandon"} else {"produce"}}));
            if cancel {
                cancelled = true;
            } else {
                purchases += 1;
            }
        }
        // Read live overlay authority. A Move must actually enter the crate cell;
        // no pickup, spawn, position, funds, damage or production injection.
        let crate_cell = sim
            .crate_authority()
            .slots()
            .iter()
            .filter(|slot| !slot.is_empty())
            .map(|slot| (slot.cell_x, slot.cell_y))
            .find(|&(x, y)| {
                sim.overlay_grid()
                    .and_then(|g| g.cell(x as u16, y as u16).overlay_id)
                    .is_some()
            });
        if let Some(cell) = crate_cell {
            drops.insert((produced.len(), cell));
            if target != Some(cell) || frame % 120 == 0 {
                let movement = Command::Move {
                    entity_id: collector,
                    target_rx: cell.0 as u16,
                    target_ry: cell.1 as u16,
                };
                let record = sim
                    .encode_megamission_record(owner, &movement)
                    .context("pickup Move")?;
                commands.push(CommandEnvelope::synchronized(record, tick));
                inputs.push(json!({"tick":tick,"event":"move","cell":cell}));
                target = Some(cell);
            }
        } else {
            target = None;
        }
        if let Some(id) = factory
            .as_ref()
            .and_then(|f| f.object)
            .and_then(|o| o.entity_id)
        {
            produced.insert(id);
        }
        for (id, e) in sim.entities().iter_sorted() {
            let ty = sim.interner().resolve(e.type_ref());
            if e.type_ref() == ticket && !e.lifecycle.in_limbo() {
                delivered.insert(id);
            }
            if ["HTK", "FV"].contains(&ty) {
                rewards.insert((
                    id,
                    ty.to_owned(),
                    sim.interner().resolve(e.owner()).to_owned(),
                ));
            }
        }
        let visits = factory
            .and_then(|f| {
                f.object.map(|o| ProductionFactoryVisit {
                    type_id: o.type_id,
                    category: ProductionCategory::Vehicle,
                    construction_order: f.construction_order(),
                })
            })
            .into_iter()
            .collect::<Vec<_>>();
        runtime.advance_frame(
            &commands,
            22,
            TickLane::Ordinary,
            FrameEffects::default().with_production_strip(&visits, &|_| {}),
        )?;
        if restore && !restored && !produced.is_empty() && drops.is_empty() && mode == "repeat" {
            checkpoint::restore_checkpoint(&mut runtime, bytes, &root.join("paid-product.sav"))?;
            restored = true;
        }
        if frame % 150 == 0 {
            let sim = &runtime.simulation;
            observations.push(json!({"frame":frame,"produced":produced,"delivered":delivered,"drops":drops,"rewards":rewards,
                "actors":sim.entities().iter_sorted().map(|(id,e)|json!({"id":id,"type":sim.interner().resolve(e.type_ref()),"cell":sim.object_coords_cell(id),"limbo":e.lifecycle.in_limbo(),"mission":format!("{:?}",e.mission.effective())})).collect::<Vec<_>>() }));
        }
        if mode == "repeat" && rewards.len() == 2 && frame > 1000 {
            break;
        }
    }
    let sim = &runtime.simulation;
    let expected = if mode == "repeat" { 2 } else { 0 };
    let checks = json!({
        "exactly_one_reward_per_paid_delivery":rewards.len()==expected,
        "ticket_deliveries":delivered.len()==expected,
        "dropped_crates":drops.len()==expected,
        "no_ticket_left":sim.entities().iter_sorted().all(|(_,e)|e.type_ref()!=ticket || !e.lifecycle.object_alive()),
        "rewards_owned_by_collector":rewards.iter().all(|(_,_,h)|h=="Americans"),
        "no_free_or_refunded_rewards":if mode=="repeat" {purchases==2} else {rewards.is_empty()},
        "normal_payment":if mode=="repeat" {sim.houses()[&owner].economy().available_money()==initial_money-400} else {sim.houses()[&owner].economy().available_money()==initial_money},
        "cancel_exercised":mode!="cancel" || cancelled,
        "checkpoint_exercised":!restore || restored,
    });
    let report = json!({"passed":checks.as_object().unwrap().values().all(|v|v==true),"checks":checks,"purchases":purchases,
        "produced":produced,"delivered":delivered,"drops":drops,"rewards":rewards,"inputs":inputs,"observations":observations});
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = args.next().context("game directory")?;
    let root = std::path::absolute(args.next().context("new output directory")?)?;
    ensure!(
        !root.exists() && args.next().is_none(),
        "output must be new"
    );
    let mut install = Install::open(Path::new(&game))?;
    let bytes = build(&mut install, &root)?;
    let mut cases = Vec::new();
    for (mode, restore) in [
        ("repeat", false),
        ("repeat", true),
        ("cancel", false),
        ("idle", false),
    ] {
        let name = format!("{mode}_{}", if restore { "restored" } else { "continuous" });
        cases.push(
            json!({"name":name,"result":audit(&install,&bytes,&root.join(&name),mode,restore)?}),
        );
    }
    let report = json!({"passed":cases.iter().all(|v|v["result"]["passed"]==true),"cases":cases,
        "scope":"Real retail rules and TS build; synchronized paid production, automatic mobile PLACE through the public production-strip host boundary, Team recruitment, fatal receiver cargo drop, movement pickup and headless checkpoint. Controlled prerequisites/price/build speed; no GUI sidebar, GUI saves or network validation. Native crate eligibility and compensation rules remain."});
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    ensure!(
        report["passed"] == true,
        "lottery audit failed: {}",
        root.join("report.json").display()
    );
    println!("{}", root.join("report.json").display());
    Ok(())
}
