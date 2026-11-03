//! Country destruction conditions through TS actions and ordinary transport loading.
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
        combat::TargetKind,
        command::{Command, CommandEnvelope},
        mission::MissionType,
        runtime::SimRuntime,
        trigger_runtime::TriggerEffect,
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;

fn build(install: &mut Install, root: &Path, name: &str) -> Result<Vec<u8>> {
    std::fs::create_dir_all(root)?;
    let initial = NewMapOptions {
        width: 64,
        height: 64,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(initial.clone())?;
    let mut doc = MapDocument::new(Baseline::parse(initial.into(), &map)?);
    for house in ["Russians", "Americans"] {
        mission::create(&mut doc, Kind::House, house, house, None)?;
        doc.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    doc.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    doc.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    doc.add_record("Structures", [28, 52], "Americans", "NAPOWR")?;
    doc.add_record("Units", [30, 52], "Americans", "MTNK")?;
    match name {
        "mixed" => {
            doc.add_record("Structures", [62, 60], "Russians", "GAPOWR")?;
            doc.add_record("Units", [56, 60], "Russians", "HTK")?;
            doc.add_record("Infantry", [58, 60], "Russians", "ENGINEER")?;
            doc.add_record("Aircraft", [60, 60], "Russians", "ORCA")?;
        }
        "aircraft" => {
            doc.add_record("Aircraft", [60, 60], "Russians", "ORCA")?;
        }
        "landing_transport" | "naval_counter" => {
            // Retail amphibious naval transport: valid on clear land. Only the
            // counter-control variant removes its declared passenger capacity.
            doc.add_record("Units", [60, 60], "Russians", "LCRF")?;
            if name == "naval_counter" {
                doc.set_ini("LCRF", "Passengers", Some(b"0".to_vec()))?;
            }
        }
        _ => {
            doc.add_record("Units", [60, 60], "Americans", "HTK")?;
            doc.add_record("Infantry", [58, 60], "Russians", "ENGINEER")?;
            doc.add_record("Infantry", [58, 61], "Russians", "ENGINEER")?;
        }
    }
    for ty in ["MTNK", "HTK", "ENGINEER", "ORCA", "LCRF"] {
        for (key, value) in [("Primary", "none"), ("Secondary", "none")] {
            doc.set_ini(ty, key, Some(value.as_bytes().to_vec()))?;
        }
    }
    doc.set_ini("GAPOWR", "Crewed", Some(b"no".to_vec()))?;
    doc.commit("Country destroyed condition fixture");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(
        install,
        root.join("base.map").to_str().context("map path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let changes = match name {
        "mixed" => {
            "m.rule('destroy_land',{when:event.sinceScenarioStart(8),once:true,actions:[action.destroyHouseObjects(target,HouseObjectSubset.Land)]});\n m.rule('destroy_buildings',{when:event.sinceScenarioStart(16),once:true,actions:[action.destroyHouseObjects(target,HouseObjectSubset.Buildings)]});"
        }
        "landing_transport" | "naval_counter" => {
            "m.rule('destroy_naval',{when:event.sinceScenarioStart(8),once:true,actions:[action.destroyHouseObjects(target,HouseObjectSubset.Naval)]});"
        }
        _ => "",
    };
    let source=r#"import {defineMission,event,action,HouseObjectSubset,type Event} from './sdk';
import {map,House,Country} from './map.generated';
export default defineMission(map,{id:'destroyed_conditions',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),credits:10,iq:0,playerControl:true,allies:[m.house(House.Americans),m.house(House.Russians)]});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),credits:10,iq:0,playerControl:__CONTROL__,allies:[m.house(House.Russians),m.house(House.Americans)]});
 const target=m.country(Country.Russians), wrong=m.country(Country.Americans);
 const conditions:readonly (readonly [string,(country:typeof target)=>Event])[]=[
  ['units',event.countryUnitsDestroyed],['buildings',event.countryBuildingsDestroyed],
  ['all',event.countryAllDestroyed],['navy',event.countryNavyDestroyed],['land',event.countryLandDestroyed]];
 for(const [id,condition] of conditions) {
  // Owner deliberately differs from the parameter Country; it supplies polling.
  m.rule(id,{when:condition(target),owner:wrong,once:true,actions:[action.message(m.defineText(id,id))]});
  const negative='other_'+id;
  m.rule(negative,{when:condition(wrong),owner:target,once:true,actions:[action.message(m.defineText(negative,negative))]});
  const late='late_'+id;
  m.rule(late,{when:event.all(condition(target),event.sinceScenarioStart(140)),owner:wrong,once:false,
    actions:[action.message(m.defineText(late,late)),action.disableRule(m.ruleRef(late))]});
 }
 __CHANGES__
});"#.replace("__CHANGES__",changes).replace("__CONTROL__",if name.starts_with("allied_transport") {"true"} else {"false"});
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
fn state(runtime: &SimRuntime) -> Result<Value> {
    let sim = &runtime.simulation;
    let houses=["Russians","Americans"].into_iter().map(|name|->Result<Value>{
        let house=sim.houses().get(&sim.interner().get(name).context("House id")?).context("House")?;
        let tracking=serde_json::to_value(house.tracking())?;
        let mut counts=BTreeMap::new();
        for field in ["buildings","vehicles","navy","infantry"] { counts.insert(field,tracking[field].clone()); }
        for field in ["active_unit_types","active_infantry_types","active_aircraft_types","active_building_types"] {
            counts.insert(field,json!(tracking[field].as_object().context("type counts")?.values().map(|v|v.as_i64().unwrap()).sum::<i64>()));
        }
        Ok(json!({"name":name,"counts":counts,"unit_losses":house.stats().units_lost(),"building_losses":house.stats().buildings_lost(),"money":house.economy().available_money()}))
    }).collect::<Result<Vec<_>>>()?;
    let actors=sim.entities().iter_sorted().map(|(id,e)|json!({"id":id,"type":sim.interner().resolve(e.type_ref()),"owner":sim.interner().resolve(e.owner()),"cell":sim.object_coords_cell(id),"health":e.health.current(),"alive":e.lifecycle.object_alive(),"limbo":e.lifecycle.in_limbo(),"passengers":e.passenger_role.passenger_count()})).collect::<Vec<_>>();
    Ok(json!({"houses":houses,"actors":actors,"flags":sim.global_variables()[..2]}))
}
fn audit(install: &Install, bytes: &[u8], root: &Path, name: &str, restore: bool) -> Result<Value> {
    std::fs::create_dir_all(root)?;
    let mut runtime = campaign::load(install, bytes, 1)?;
    let initial = state(&runtime)?;
    let texts = mission_script::declared_texts(bytes)?;
    let mut observations = Vec::new();
    let mut messages = Vec::new();
    let mut checkpoint_state = None;
    let mut fully_loaded = None;
    let carrier = initial["actors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["type"] == "HTK" && a["owner"] == "Americans")
        .and_then(|a| a["id"].as_u64());
    let passengers = initial["actors"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["type"] == "ENGINEER" && a["owner"] == "Russians")
        .map(|a| a["id"].as_u64().unwrap())
        .collect::<Vec<_>>();
    if name.starts_with("allied_transport") {
        ensure!(
            carrier.is_some() && passengers.len() == 2,
            "actual allied transport and both authored passengers must spawn"
        );
    }
    let mut admitted_commands = 0;
    let mut command_observations = Vec::new();
    for frame in 0..2400 {
        let sim = &runtime.simulation;
        let commands = if name == "allied_transport" {
            let orders = match frame {
                30 => passengers
                    .iter()
                    .map(|id| {
                        (
                            "Russians",
                            Command::clicked_mission(
                                *id,
                                MissionType::Enter,
                                TargetKind::Entity(carrier.unwrap()),
                            ),
                        )
                    })
                    .collect::<Vec<_>>(),
                1400 => vec![(
                    "Americans",
                    Command::UnloadPassengers {
                        transport_id: carrier.unwrap(),
                    },
                )],
                _ => Vec::new(),
            };
            orders
                .into_iter()
                .map(|(owner, command)| -> Result<_> {
                    Ok(CommandEnvelope::synchronized(
                        sim.encode_megamission_record(
                            sim.interner().get(owner).context("command House")?,
                            &command,
                        )
                        .context("transport command encoding")?,
                        sim.session().tick(),
                    ))
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        let mut output =
            runtime.advance_frame(&commands, 22, TickLane::Ordinary, FrameEffects::default())?;
        admitted_commands += output.take_admitted_commands().len();
        let tick = runtime.simulation.session().tick();
        let current = state(&runtime)?;
        if name == "allied_transport" && [30, 31, 40, 60, 100, 300].contains(&frame) {
            let sim = &runtime.simulation;
            command_observations.push(json!({"tick":tick,"allied":sim.house_alliances().is_allied(sim.interner(),sim.interner().get("Americans").unwrap(),sim.interner().get("Russians").unwrap()),"actors":sim.entities().iter_sorted().map(|(id,e)|json!({"id":id,"mission":format!("{:?}",e.mission.effective()),"nav":format!("{:?}",e.navigation.nav_com()),"radio":format!("{:?}",e.radio_contacts)})).collect::<Vec<_>>()}));
        }
        if current["actors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["passengers"] == 2)
            && fully_loaded.is_none()
        {
            fully_loaded = Some(json!({"tick":tick,"state":current}));
        }
        let mut received = false;
        for effect in output.trigger_effects {
            if let TriggerEffect::LocalizedMessage { csf_label } = effect {
                messages.push(json!({"tick":tick,"event":texts.get(&csf_label).context("message label")?,"state":current}));
                received = true;
            }
        }
        if frame % 10 == 0 || received {
            observations.push(json!({"tick":tick,"state":current}));
        }
        let checkpoint_due = if name == "allied_transport" {
            fully_loaded.is_some()
        } else {
            frame == 39
        };
        if restore && checkpoint_state.is_none() && checkpoint_due {
            checkpoint::restore_checkpoint(&mut runtime, bytes, &root.join("checkpoint.bin"))?;
            let after = state(&runtime)?;
            checkpoint_state = Some(
                json!({"tick":tick,"preserved":after==current,"before":current,"after":after}),
            );
        }
    }
    let final_state = state(&runtime)?;
    let count = |event: &str| messages.iter().filter(|m| m["event"] == event).count();
    let first = |event: &str| messages.iter().find(|m| m["event"] == event);
    let original = |ty: &str| {
        initial["actors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["type"] == ty && a["owner"] == "Russians")
    };
    let initial_counts = &initial["houses"][0]["counts"];
    let final_counts = &final_state["houses"][0]["counts"];
    let mut checks = BTreeMap::<String, bool>::new();
    let mut check = |key: &str, value: bool| {
        checks.insert(key.into(), value);
    };
    check(
        "real_house_order_differs_from_country",
        runtime
            .simulation
            .houses()
            .order()
            .first()
            .is_some_and(|id| runtime.simulation.interner().resolve(*id) == "Russians"),
    );
    check(
        "parameter_country_not_rule_owner",
        count("other_units") == 0
            && count("other_buildings") == 0
            && count("other_all") == 0
            && count("other_land") == 0
            && count("other_navy") == 1,
    );
    check(
        "other_house_unchanged",
        initial["houses"][1] == final_state["houses"][1]
            && initial["actors"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|a| a["owner"] == "Americans")
                .all(|a| final_state["actors"].as_array().unwrap().contains(a)),
    );
    for kind in ["units", "buildings", "all", "navy", "land"] {
        let expected = match (name, kind) {
            ("allied_transport" | "allied_transport_idle", "land") => 0,
            ("allied_transport_idle", "units" | "all") => 0,
            _ => 1,
        };
        check(
            &format!("single_{kind}_notification"),
            count(kind) == expected,
        );
        let late_expected = if name == "allied_transport" && (kind == "units" || kind == "all") {
            0
        } else {
            expected
        };
        check(
            &format!("current_{kind}_condition_at_late_stage"),
            count(&format!("late_{kind}")) == late_expected,
        );
    }
    match name {
        "mixed" => {
            check(
                "four_original_target_categories",
                initial_counts["buildings"] == 1
                    && initial_counts["vehicles"] == 1
                    && initial_counts["infantry"] == 1
                    && initial_counts["active_aircraft_types"] == 1,
            );
            check(
                "land_destroy_waits_for_action",
                first("units").is_some_and(|m| m["tick"].as_u64().unwrap() >= 121)
                    && first("land").is_some_and(|m| m["tick"].as_u64().unwrap() >= 121),
            );
            check(
                "building_prevents_all_after_land_destroy",
                first("units").is_some_and(|m| m["state"]["houses"][0]["counts"]["buildings"] == 1)
                    && first("all").is_some_and(|m| m["tick"].as_u64().unwrap() >= 241),
            );
            check(
                "land_action_also_destroys_non_naval_aircraft",
                final_counts["active_aircraft_types"] == 0
                    && final_state["actors"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|a| a["owner"] != "Russians"),
            );
            check(
                "real_destruction_losses",
                final_state["houses"][0]["unit_losses"] == 3
                    && final_state["houses"][0]["building_losses"] == 1,
            );
        }
        "aircraft" => {
            check(
                "aircraft_survives_all_destroyed",
                original("ORCA")
                    .is_some_and(|a| final_state["actors"].as_array().unwrap().contains(a))
                    && final_counts["active_aircraft_types"] == 1,
            );
            check(
                "aircraft_does_not_delay_initial_conditions",
                ["units", "all", "land"]
                    .iter()
                    .all(|e| first(e).is_some_and(|m| m["tick"].as_u64().unwrap() < 40)),
            );
        }
        "landing_transport" | "naval_counter" => {
            let combatant = name == "naval_counter";
            check(
                "naval_type_is_amphibious_with_declared_capacity",
                runtime.resources.rules.object("LCRF").is_some_and(|ty| {
                    ty.naval
                        && ty.movement_zone
                            == vera20k_core::rules::locomotor_type::MovementZone::Amphibious
                        && if combatant {
                            ty.passengers == 0
                        } else {
                            ty.passengers > 0
                        }
                }),
            );
            check(
                "original_amphibious_transport_is_on_map",
                original("LCRF").is_some_and(|a| a["alive"] == true && a["limbo"] == false),
            );
            check(
                "passenger_capacity_controls_navy_counter",
                initial_counts["vehicles"] == 1 && initial_counts["navy"] == i64::from(combatant),
            );
            check(
                "transport_counts_toward_units_until_destroyed",
                first("units").is_some_and(|m| m["tick"].as_u64().unwrap() >= 121),
            );
            let early = if combatant { "land" } else { "navy" };
            let delayed = if combatant { "navy" } else { "land" };
            check(
                "transport_and_naval_counter_have_different_categories",
                first(early).is_some_and(|m| m["tick"].as_u64().unwrap() < 40)
                    && first(delayed).is_some_and(|m| m["tick"].as_u64().unwrap() >= 121),
            );
            check(
                "naval_action_destroys_transport_regardless_of_capacity",
                final_counts["vehicles"] == 0 && final_state["houses"][0]["unit_losses"] == 1,
            );
        }
        _ => {
            let loading = name == "allied_transport";
            check(
                "real_loading_only_in_dispatched_case",
                fully_loaded.is_some() == loading,
            );
            check(
                "tracked_infantry_survives_transport_limbo",
                final_counts["infantry"] == 2 && final_state["houses"][0]["unit_losses"] == 0,
            );
            check(
                "original_transport_and_passengers_survive",
                initial["actors"].as_array().unwrap().iter().all(|a| {
                    final_state["actors"].as_array().unwrap().iter().any(|b| {
                        a["id"] == b["id"]
                            && a["health"] == b["health"]
                            && a["owner"] == b["owner"]
                            && a["type"] == b["type"]
                    })
                }),
            );
            if loading {
                check(
                    "boarding_house_dispatches_player_commands",
                    runtime
                        .simulation
                        .houses()
                        .get(&runtime.simulation.interner().get("Russians").unwrap())
                        .is_some_and(|house| house.event_dispatch_eligible()),
                );
                check(
                    "both_boarding_orders_and_unload_admitted",
                    admitted_commands == 3,
                );
                check(
                    "loaded_infantry_leaves_active_but_not_tracked_counts",
                    fully_loaded.as_ref().is_some_and(|m| {
                        m["state"]["houses"][0]["counts"]["active_infantry_types"] == 0
                            && m["state"]["houses"][0]["counts"]["infantry"] == 2
                    }),
                );
                check(
                    "units_and_all_fire_with_living_loaded_passengers",
                    ["units", "all"].iter().all(|e| {
                        first(e).is_some_and(|m| {
                            m["state"]["actors"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .filter(|a| a["type"] == "ENGINEER")
                                .all(|a| a["limbo"] == true && a["alive"] == true)
                        })
                    }),
                );
                check(
                    "ordinary_unload_restores_active_infantry",
                    final_counts["active_infantry_types"] == 2
                        && final_state["actors"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .all(|a| a["limbo"] == false && a["passengers"] == 0),
                );
            }
        }
    }
    if restore {
        check(
            "checkpoint_preserves_listed_state",
            checkpoint_state
                .as_ref()
                .is_some_and(|c| c["preserved"] == true),
        );
    }
    std::fs::write(
        root.join("observations.json"),
        serde_json::to_vec_pretty(&observations)?,
    )?;
    Ok(
        json!({"passed":checks.values().all(|b|*b),"checks":checks,"initial":initial,"final":final_state,"messages":messages,"fully_loaded":fully_loaded,"checkpoint":checkpoint_state,"command_observations":command_observations}),
    )
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(args.next().context("new output directory")?)?;
    let selected = args.next().map(|arg| arg.to_string_lossy().into_owned());
    ensure!(
        args.next().is_none() && !root.exists(),
        "output must be new"
    );
    std::fs::create_dir_all(&root)?;
    let mut install = Install::open(&game)?;
    let mut cases = Vec::new();
    for name in [
        "mixed",
        "aircraft",
        "landing_transport",
        "naval_counter",
        "allied_transport",
        "allied_transport_idle",
    ] {
        if selected.as_ref().is_some_and(|selected| selected != name) {
            continue;
        }
        let folder = root.join(name);
        let bytes = build(&mut install, &folder, name)?;
        let continuous = audit(&install, &bytes, &folder.join("continuous"), name, false)?;
        if name != "allied_transport_idle" {
            let restored = audit(&install, &bytes, &folder.join("restored"), name, true)?;
            let names = |r: &Value| {
                r["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|m| m["event"].clone())
                    .collect::<Vec<_>>()
            };
            let outcome_match =
                continuous["final"] == restored["final"] && names(&continuous) == names(&restored);
            let exact = continuous["messages"] == restored["messages"]
                && continuous["final"] == restored["final"];
            cases.push(json!({"case":format!("{name}_restored"),"result":restored,"listed_outcome_and_event_order_match":outcome_match,"continuous_ticks_and_object_states_match":exact}));
        }
        cases.push(json!({"case":name,"result":continuous}));
    }
    ensure!(!cases.is_empty(), "unknown selected fixture");
    let passed = cases.iter().all(|c| {
        c["result"]["passed"] == true
            && c.get("listed_outcome_and_event_order_match")
                .is_none_or(|v| v == true)
    });
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":passed,"cases":cases,"scope":"Existing House conditions, TS damage actions and normal allied transport Enter/Unload commands. LCRF is amphibious: passenger-capacity zero is a controlled map rule variant, not retail combat-ship acceptance. No counter, position, lifecycle or event injection; headless checkpoint, not GUI or network acceptance."}),
        )?,
    )?;
    ensure!(
        passed,
        "destroyed conditions audit failed; inspect report.json"
    );
    println!("{}", root.join("report.json").display());
    Ok(())
}
