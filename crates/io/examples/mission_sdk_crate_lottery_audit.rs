//! Paid PLACE -> player force-fire -> isolated native trigger prize; ordinary crates unchanged.
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
        game_options::GameOptions,
        production::ProductionCategory,
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
#[allow(dead_code)]
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
        ([40, 60], "Americans", "GACNST"),
        ([82, 82], "Russians", "NAPOWR"),
    ] {
        doc.add_record("Structures", cell, house, ty)?;
    }
    doc.add_record("Units", [40, 57], "Americans", "MTNK")?;
    // Deliberately no miner: the isolated lottery must bypass crate compensation.
    for (s, k, v) in [
        ("Basic", "Player", "Americans"),
        ("Basic", "MultiplayerOnly", "no"),
        ("General", "BuildSpeed", "0.1"),
        ("HTK", "Strength", "321"),
        ("FV", "Strength", "345"),
        ("HTK", "Speed", "6"),
        ("FV", "Speed", "6"),
        ("NAPOWR", "Power", "2000"),
        ("CrateRules", "CrateMinimum", "0"),
        ("CrateRules", "CrateMaximum", "0"),
        ("GAPILL", "Prerequisite", ""),
        ("GAPILL", "Owner", "Americans"),
        ("GAPILL", "TechLevel", "1"),
        ("GAPILL", "RequiredHouses", ""),
        ("GAPILL", "ForbiddenHouses", ""),
    ] {
        doc.set_ini(s, k, Some(v.as_bytes().to_vec()))?;
    }
    doc.commit("controlled lottery prerequisites");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let scene =
        ra2_map_scene::map::load_scene(install, root.join("base.map").to_str().context("path")?)?;
    let catalog = mission_script::catalog(&doc, &scene);
    let baseline_crates: Vec<_> = ["Powerups", "CrateRules"]
        .iter()
        .map(|section| (*section, doc.ini_fields(section)))
        .collect();
    let baseline_vehicle_crates: Vec<_> = catalog.registry_order["vehicles"]
        .iter()
        .map(|id| (id.clone(), doc.ini_value(id, "CrateGoodie")))
        .collect();
    let demo_entry = root.join("demo/mission.ts");
    std::fs::create_dir_all(demo_entry.parent().context("demo directory")?)?;
    std::fs::copy("examples/missions/reward-unit/mission.ts", &demo_entry)?;
    mission_script::refresh_types(&demo_entry, &catalog)?;
    let mut demo_doc = doc.fork();
    Worker::new()?
        .build(&demo_doc, catalog.clone(), Sources::read(&demo_entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(&mut demo_doc)?;
    let demo_bytes = demo_doc.snapshot().bytes()?;
    std::fs::write(root.join("demo/compiled.map"), &demo_bytes)?;
    let demo_runtime =
        campaign::load_with_options(install, &demo_bytes, 1, &GameOptions::default())?;
    let demo_rules = &demo_runtime.resources.rules;
    ensure!(
        demo_rules
            .vehicle_ids
            .iter()
            .filter(|id| id.starts_with("TSRU"))
            .count()
            == 2
            && demo_rules
                .vehicle_ids
                .iter()
                .filter(|id| id.starts_with("TSRU"))
                .any(|id| demo_rules
                    .object(id)
                    .is_some_and(|unit| unit.image == "HTNK" && unit.strength == 1500)),
        "standalone demo must load its independent Apocalypse reward"
    );
    let source = r#"import {defineMission} from './sdk';
import {map,House,Country,Unit,Building} from './map.generated';
export default defineMission(map,{id:'crate_lottery',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),credits:100,iq:0,playerControl:true,allies:[m.house(House.Americans)]});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),credits:100,iq:0,allies:[m.house(House.Russians)]});
 const htk=m.rewardUnit('armored_htk',{base:m.unit(Unit.HTK),name:'奖励防空车',rules:{Strength:1500,Speed:8,SelfHealing:true}});
 const fv=m.rewardUnit('armored_fv',{base:m.unit(Unit.FV),name:'奖励多功能车',rules:{Strength:1500,Speed:8,SelfHealing:true}});
 m.crateLottery('draw',{ticket:m.building(Building.GAPILL),rewards:[htk,fv],rewardAt:m.createWaypoint(30,48,60),cost:200,prerequisites:[],name:'补给箱（攻击拆开）'});
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
    for (section, fields) in baseline_crates {
        ensure!(
            doc.ini_fields(section) == fields,
            "ordinary crate section {section} changed"
        );
    }
    for (id, value) in baseline_vehicle_crates {
        ensure!(
            doc.ini_value(&id, "CrateGoodie") == value,
            "ordinary {id} crate eligibility changed"
        );
    }
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(root.join("compiled.map"), &bytes)?;
    // A fresh scene includes the generated registry entries and rule snapshots.
    // Rebuilding from that catalog must still produce exactly the same map.
    let reloaded = ra2_map_scene::map::load_scene(
        install,
        root.join("compiled.map").to_str().context("path")?,
    )?;
    let reloaded_catalog = mission_script::catalog(&doc, &reloaded);
    Worker::new()?
        .build(&doc, reloaded_catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(&mut doc)?;
    ensure!(
        doc.snapshot().bytes()? == bytes,
        "reward registry reload must rebuild byte-identically"
    );
    // Keep the lottery active while exercising an unrelated ordinary Unit crate.
    let probe_source = source.replace("{defineMission}","{defineMission,event,action,Crate}").replace("\n});", "\n m.rule('ordinary_crate',{when:event.sinceScenarioStart(1),once:true,actions:[action.createCrate(Crate.Unit,m.createWaypoint(31,42,57))]});\n});");
    std::fs::write(&entry, &probe_source)?;
    let mut probe_doc = doc.fork();
    let probe_catalog = mission_script::catalog(&doc, &reloaded);
    Worker::new()?
        .build(&probe_doc, probe_catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(&mut probe_doc)?;
    std::fs::write(
        root.join("ordinary-crate.map"),
        probe_doc.snapshot().bytes()?,
    )?;
    std::fs::write(&entry, source)?;
    Ok(bytes)
}

fn audit(install: &Install, bytes: &[u8], root: &Path, mode: &str, restore: bool) -> Result<Value> {
    std::fs::create_dir_all(root)?;
    let options = GameOptions {
        crates: false,
        ..Default::default()
    };
    let mut runtime = campaign::load_with_options(install, bytes, 1, &options)?;
    let owner = runtime
        .simulation
        .interner()
        .get("Americans")
        .context("owner")?;
    let ticket = runtime
        .simulation
        .interner()
        .get("GAPILL")
        .context("ticket")?;
    let initial_money = runtime.simulation.houses()[&owner]
        .economy()
        .available_money();
    let sidebar_entry = vera20k_core::sim::production::build_options_for_owner(
        &runtime.simulation,
        &runtime.resources.rules,
        "Americans",
    )
    .into_iter()
    .find(|entry| entry.type_id == ticket);
    let attacker = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find(|(_, e)| runtime.simulation.interner().resolve(e.type_ref()) == "MTNK")
        .context("attacker")?
        .0;
    let mut rewards = BTreeSet::new();
    let mut placed = BTreeSet::new();
    let mut inputs = Vec::new();
    let mut observations = Vec::new();
    let mut purchases = 0;
    let mut cancelled = false;
    let mut restored = false;
    let mut placed_restored = false;
    let mut reward_restored = false;
    let mut ready_wait_observed = false;
    let mut unopened_wait_observed = false;
    let mut blocked_retry_observed = false;
    let mut rejected_once = false;
    let mut active_since = None;
    let mut attacks = BTreeSet::new();
    let cells = [(44, 60), (44, 62)];
    for frame in 0..3000 {
        let sim = &runtime.simulation;
        let tick = sim.session().tick();
        let factory = sim
            .production()
            .factories()
            .view(owner, ProductionCategory::Building);
        let ready = factory
            .as_ref()
            .is_some_and(|f| f.complete_object().is_some());
        let buy = mode != "idle"
            && (frame == 60
                || (mode == "repeat" && purchases == 1 && rewards.len() == 1 && frame > 1100));
        let cancel = mode == "cancel"
            && !cancelled
            && factory
                .as_ref()
                .is_some_and(|f| f.progress >= 10 && f.progress < 54);
        let mut commands = Vec::new();
        let mut expected_place = None::<(u16, u16)>;
        let live_box = sim.entities().iter_sorted().find(|(_, e)| {
            e.type_ref() == ticket && !e.lifecycle.in_limbo() && e.lifecycle.object_alive()
        });
        let live_box_ready = live_box.is_some_and(|(_, e)| !e.in_construction_bstate());
        if buy || cancel {
            let record = sim
                .encode_production_record(
                    owner,
                    if cancel {
                        ProductionEvent::Abandon
                    } else {
                        ProductionEvent::Produce
                    },
                    ticket,
                    &runtime.resources.rules,
                )
                .context("production")?;
            commands.push(CommandEnvelope::synchronized(record, tick));
            inputs.push(json!({"frame":frame,"event":if cancel {"abandon"}else{"produce"}}));
            if cancel {
                cancelled = true;
            } else {
                purchases += 1;
            }
        }
        if ready && placed.is_empty() && frame < 600 {
            ready_wait_observed = true;
            ensure!(
                sim.crate_authority().slots().iter().all(|s| s.is_empty()),
                "no box before placement"
            );
        }
        if ["repeat", "unopened"].contains(&mode) && ready && frame >= 600 {
            let cell = if !rejected_once {
                (40, 60)
            } else {
                cells[placed.len()]
            };
            let preview = vera20k_core::sim::production::placement_preview_for_owner_with_overlays(
                sim,
                &runtime.resources.rules,
                "Americans",
                "GAPILL",
                cell.0,
                cell.1,
                Some(&runtime.resources.overlay_registry),
            )
            .context("preview")?;
            if !rejected_once {
                ensure!(!preview.valid, "occupied yard must refuse");
                rejected_once = true;
            } else {
                ensure!(
                    preview.valid,
                    "selected cell {cell:?}: {:?}",
                    preview.reason
                );
            }
            let record = sim
                .encode_place_building_record(owner, ticket, cell, &runtime.resources.rules)
                .context("PLACE")?;
            commands.push(CommandEnvelope::synchronized(record, tick));
            inputs.push(json!({"frame":frame,"event":"place","cell":cell,"preview":preview.valid}));
            if preview.valid {
                placed.insert(cell);
                expected_place = Some(cell);
            } else {
                blocked_retry_observed = true;
            }
        }
        if live_box_ready && active_since.is_none() {
            active_since = Some(frame);
        }
        if live_box_ready && attacks.len() < placed.len() {
            if frame < active_since.unwrap() + 120 {
                ensure!(
                    sim.crate_authority().slots().iter().all(|s| s.is_empty()),
                    "unattacked building must not drop a crate"
                );
                unopened_wait_observed = true;
            } else if mode == "repeat" {
                let &(x, y) = placed.iter().next_back().unwrap();
                let command = Command::ForceAttackCell {
                    attacker_id: attacker,
                    target_rx: x,
                    target_ry: y,
                };
                commands.push(CommandEnvelope::synchronized(
                    sim.encode_megamission_record(owner, &command)
                        .context("force attack")?,
                    tick,
                ));
                attacks.insert((x, y));
                inputs.push(json!({"frame":frame,"event":"force_attack","cell":[x,y]}));
            }
        }
        ensure!(
            sim.crate_authority()
                .slots()
                .iter()
                .all(|slot| slot.is_empty()),
            "lottery must not drop or modify a crate"
        );
        if !attacks.is_empty() && live_box.is_none() && active_since.take().is_some() {
            commands.push(CommandEnvelope::synchronized(
                sim.encode_megamission_record(
                    owner,
                    &Command::Move {
                        entity_id: attacker,
                        target_rx: 40,
                        target_ry: 57,
                    },
                )
                .context("stop firing")?,
                tick,
            ));
        }
        for (id, e) in sim.entities().iter_sorted() {
            let ty = sim.interner().resolve(e.type_ref());
            if ty.starts_with("TSRU") {
                rewards.insert((
                    id,
                    ty.to_owned(),
                    sim.interner().resolve(e.owner()).to_owned(),
                ));
            }
        }
        runtime.advance_frame(&commands, 22, TickLane::Ordinary, FrameEffects::default())?;
        if expected_place.is_some() {
            ensure!(
                runtime
                    .simulation
                    .entities()
                    .iter_sorted()
                    .any(|(_, e)| e.type_ref() == ticket && !e.lifecycle.in_limbo()),
                "placement must create a real building"
            );
        }
        if restore && !restored && ready && placed.is_empty() {
            checkpoint::restore_checkpoint(&mut runtime, bytes, &root.join("ready-building.sav"))?;
            restored = true;
        }
        if restore && !placed_restored && live_box_ready && attacks.is_empty() {
            checkpoint::restore_checkpoint(
                &mut runtime,
                bytes,
                &root.join("unopened-building.sav"),
            )?;
            placed_restored = true;
        }
        if restore && !reward_restored && !rewards.is_empty() {
            checkpoint::restore_checkpoint(&mut runtime, bytes, &root.join("awarded-prize.sav"))?;
            reward_restored = true;
        }
        if frame % 300 == 0 {
            observations.push(json!({"frame":frame,"placed":placed,"attacks":attacks,"rewards":rewards,
                "actors":runtime.simulation.entities().iter_sorted().map(|(id,e)|json!({"id":id,"type":runtime.simulation.interner().resolve(e.type_ref()),"cell":runtime.simulation.object_coords_cell(id),"hp":e.health.current(),"limbo":e.lifecycle.in_limbo(),"mission":format!("{:?}",e.mission.effective())})).collect::<Vec<_>>() }));
        }
        if mode == "repeat" && rewards.len() == 2 && frame > 1400 {
            break;
        }
    }
    let sim = &runtime.simulation;
    let expected = if mode == "repeat" { 2 } else { 0 };
    let ticket_rule = runtime
        .resources
        .rules
        .object("GAPILL")
        .context("ticket rule")?;
    let building_count = sim
        .entities()
        .iter_sorted()
        .filter(|(_, e)| {
            e.type_ref() == ticket && !e.lifecycle.in_limbo() && e.lifecycle.object_alive()
        })
        .count();
    let rules = &runtime.resources.rules;
    let prize_ids: Vec<_> = rules
        .vehicle_ids
        .iter()
        .filter(|id| id.starts_with("TSRU"))
        .collect();
    let prize_rules_valid = prize_ids.len() == 2
        && prize_ids.iter().all(|id| {
            let prize = rules.object(id).unwrap();
            let base = rules.object(&prize.image).unwrap();
            prize.strength == 1500
                && prize.self_healing
                && prize.speed != base.speed
                && prize.tech_level == -1
                && !prize.allowed_to_start_in_multiplayer
                && !prize.crate_goodie
                && prize.primary() == base.primary()
        });
    let hidden_prizes =
        vera20k_core::sim::production::build_options_for_owner(sim, rules, "Americans")
            .iter()
            .all(|entry| !sim.interner().resolve(entry.type_id).starts_with("TSRU"));
    let checks = json!({
        "reward_type_properties_inherited_and_overridden":prize_rules_valid,
        "original_vehicle_properties_unchanged":rules.object("HTK").unwrap().strength==321 && rules.object("FV").unwrap().strength==345 && rules.object("HTK").unwrap().speed==rules.object("FV").unwrap().speed,
        "reward_types_hidden_from_production":hidden_prizes,
        "random_crates_disabled":!sim.session().game_options().crates,
        "building_sidebar":sidebar_entry.as_ref().is_some_and(|entry|entry.enabled && entry.queue_category==ProductionCategory::Building),
        "native_no_crate_flags":!ticket_rule.crate_beneath && !ticket_rule.crate_beneath_is_money && !ticket_rule.leave_rubble && ticket_rule.build_limit==1,
        "ready_wait_exercised":!["repeat","ready","unopened"].contains(&mode) || ready_wait_observed,
        "unopened_wait_exercised":!["repeat","unopened"].contains(&mode) || unopened_wait_observed,
        "rejected_place_then_retry":!["repeat","unopened"].contains(&mode) || blocked_retry_observed,
        "one_prize_per_destroyed_building":rewards.len()==attacks.len() && attacks.len()==expected,
        "random_vehicle_rewards":rewards.len()==expected,
        "rewards_owned_by_player":rewards.iter().all(|(_,_,h)|h=="Americans"),
        "initial_tank_survives":sim.entities().get(attacker).is_some_and(|e|e.lifecycle.object_alive() && e.health.current()>0),
        "building_survives_without_attack":building_count==usize::from(mode=="unopened"),
        "normal_payment":sim.houses()[&owner].economy().available_money()==initial_money-if mode=="repeat"{400}else if ["ready","unopened"].contains(&mode){200}else{0},
        "ready_kept_without_place":mode!="ready" || sim.production().factories().view(owner,ProductionCategory::Building).is_some_and(|f|f.complete_object().is_some()),
        "cancel_exercised":mode!="cancel" || cancelled,
        "checkpoint_exercised":!restore || (restored && placed_restored && reward_restored),
        "no_custom_engine_key":!std::str::from_utf8(bytes).unwrap_or("").contains("PlaceAsCrate=yes"),
    });
    let report = json!({"passed":checks.as_object().unwrap().values().all(|v|v==true),"checks":checks,"purchases":purchases,"placed":placed,"attacks":attacks,"rewards":rewards,"inputs":inputs,"observations":observations});
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

fn ordinary_crate_probe(install: &Install, bytes: &[u8]) -> Result<Value> {
    let mut runtime = campaign::load_with_options(
        install,
        bytes,
        1,
        &GameOptions {
            crates: false,
            ..Default::default()
        },
    )?;
    let owner = runtime
        .simulation
        .interner()
        .get("Americans")
        .context("probe owner")?;
    let collector = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find(|(_, e)| runtime.simulation.interner().resolve(e.type_ref()) == "MTNK")
        .context("probe collector")?
        .0;
    let mut saw_crate = false;
    for frame in 0..900 {
        saw_crate |= runtime
            .simulation
            .crate_authority()
            .slots()
            .iter()
            .any(|s| !s.is_empty());
        let mut commands = Vec::new();
        if frame > 60 && frame % 30 == 0 {
            let record = runtime
                .simulation
                .encode_megamission_record(
                    owner,
                    &Command::Move {
                        entity_id: collector,
                        target_rx: 42,
                        target_ry: 57,
                    },
                )
                .context("probe pickup")?;
            commands.push(CommandEnvelope::synchronized(
                record,
                runtime.simulation.session().tick(),
            ));
        }
        runtime.advance_frame(&commands, 22, TickLane::Ordinary, FrameEffects::default())?;
    }
    let mut miners = 0;
    let mut prizes = 0;
    for (_, e) in runtime.simulation.entities().iter_sorted() {
        let ty = runtime.simulation.interner().resolve(e.type_ref());
        if runtime
            .resources
            .rules
            .object(ty)
            .is_some_and(|unit| unit.harvester)
        {
            miners += 1;
        }
        if ty.starts_with("TSRU") {
            prizes += 1;
        }
    }
    let original_cell_has_crate = runtime
        .simulation
        .overlay_grid()
        .context("probe overlays")?
        .cell(42, 57)
        .overlay_id
        .and_then(|id| runtime.resources.overlay_registry.flags(id))
        .is_some_and(|flags| flags.crate_type);
    let checks = json!({"ordinary_crate_appeared":saw_crate,"ordinary_crate_collected":!original_cell_has_crate,
        "ordinary_native_miner_compensation":miners==1,"ordinary_crate_never_grants_lottery_variant":prizes==0});
    Ok(json!({"passed":checks.as_object().unwrap().values().all(|v|v==true),"checks":checks}))
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
        ("ready", false),
        ("unopened", false),
        ("cancel", false),
        ("idle", false),
    ] {
        let name = format!("{mode}_{}", if restore { "restored" } else { "continuous" });
        cases.push(
            json!({"name":name,"result":audit(&install,&bytes,&root.join(&name),mode,restore)?}),
        );
    }
    let ordinary_crate =
        ordinary_crate_probe(&install, &std::fs::read(root.join("ordinary-crate.map"))?)?;
    let report = json!({"passed":cases.iter().all(|v|v["result"]["passed"]==true) && ordinary_crate["passed"]==true,"cases":cases,"ordinary_crate":ordinary_crate,
        "scope":"Unmodified engine; real retail rules and TS map configuration, synchronized paid construction/PLACE, explicit player force-fire, native presence/absence triggers and random-timer prize teams. No miner, no crate pickup; ready/unopened/prize checkpoints. No injected damage, health, funds or rewards. Controlled prerequisites and build speed; no GUI or live-network validation."});
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
