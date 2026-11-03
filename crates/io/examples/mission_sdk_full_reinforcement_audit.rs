//! TS Full reinforcement cargo selection, miner holds and public checkpoints.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    mission::{self, Kind},
    new_map::NewMapOptions,
};
use ra2_map_io::mission_script::{self, Sources, Worker};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};
use vera20k_core::sim::{
    runtime::SimRuntime,
    world::{FrameEffects, TickLane},
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;

fn state(runtime: &SimRuntime, team_type: &str) -> Value {
    let sim = &runtime.simulation;
    let actors = sim.entities().iter_sorted().map(|(id,e)| {
        let cargo = e.passenger_role.cargo().map(|c|c.passengers().to_vec()).unwrap_or_default();
        let miner = e.miner.as_ref().map(|m|json!({"capacity":m.capacity_bales(),"bales":m.cargo()}));
        json!({"id":id,"type":sim.interner().resolve(e.type_ref()),"owner":sim.interner().resolve(e.owner()),"health":e.health.current(),"alive":e.lifecycle.object_alive(),"limbo":e.lifecycle.in_limbo(),"cell":sim.object_coords_cell(id),"cargo":cargo,"miner":miner})
    }).collect::<Vec<_>>();
    let teams = sim.team_script_vm().teams_in_order().filter(|t|t.team_type_id().is_some_and(|id|sim.interner().resolve(id)==team_type)).map(|t|json!({"id":t.id(),"owner":sim.interner().resolve(t.owner()),"cursor":t.cursor(),"members":t.members().collect::<Vec<_>>()})).collect::<Vec<_>>();
    let house = sim
        .interner()
        .get("Americans")
        .and_then(|id| sim.houses().get(&id));
    json!({"tick":sim.session().tick(),"actors":actors,"teams":teams,"marker":sim.global_variables()[0],"credits":house.map(|h|h.economy().credits()),"losses":house.map(|h|h.stats().units_lost())})
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(PathBuf::from(args.next().context("new output directory")?))?;
    ensure!(
        !root.exists() && args.next().is_none(),
        "output must be new"
    );
    std::fs::create_dir_all(&root)?;
    let bytes = NewMapOptions {
        width: 64,
        height: 64,
        ..Default::default()
    }
    .bytes()?;
    let map = vera20k_core::map::map_file::load_from_bytes(bytes.clone())?;
    let mut base = MapDocument::new(Baseline::parse(bytes.into(), &map)?);
    for house in ["Americans", "Russians"] {
        mission::create(&mut base, Kind::House, house, house, None)?;
        base.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    base.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    base.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    base.add_record("Structures", [70, 70], "Americans", "GAPOWR")?;
    base.add_record("Structures", [78, 70], "Russians", "NAPOWR")?;
    base.commit("full reinforcement fixture");
    let mut install = Install::open(&game)?;
    let worker = Worker::new()?;
    let mut cases = Vec::new();
    for (kind, carrier, mixed) in [
        ("ground", "HTK", false),
        ("naval", "LCRF", false),
        ("air_no_capacity", "PDPLANE", false),
        ("air_declared_capacity", "PDPLANE", false),
        ("jumpjet", "SHAD", false),
        ("mixed_naval", "LCRF", true),
        ("ground_over_capacity", "HTK", false),
        ("multiple_carriers", "HTK", false),
        ("miner", "HARV", false),
    ] {
        for full in [false, true] {
            let name = format!("{kind}_{}", if full { "full" } else { "empty" });
            let folder = root.join(&name);
            std::fs::create_dir(&folder)?;
            let path = folder.join("mission.map");
            let entry = folder.join("mission.ts");
            let mut doc = base.fork();
            if kind == "air_declared_capacity" {
                doc.set_ini("PDPLANE", "Passengers", Some(b"2".to_vec()))?;
                doc.commit("declared aircraft capacity control");
            }
            std::fs::write(&path, doc.snapshot().bytes()?)?;
            let scene =
                ra2_map_scene::map::load_scene(&mut install, path.to_str().context("map path")?)?;
            let catalog = mission_script::catalog(&doc, &scene);
            let extra = if mixed {
                ",{type:m.unit(Unit.HTK),count:1}"
            } else {
                ""
            };
            let payload = if kind == "miner" {
                ""
            } else if kind == "ground_over_capacity" {
                ",{type:m.unit(Unit.ENGINEER),count:6}"
            } else {
                ",{type:m.unit(Unit.ENGINEER),count:2}"
            };
            std::fs::write(
                &entry,
                format!(
                    r#"import {{defineMission,event,action,order}} from './sdk';import {{map,House,Country,Unit}} from './map.generated';
export default defineMission(map,{{id:'full_{name}',player:House.Americans}},m=>{{
 m.configureBasic({{ignoreGlobalAiTriggers:true,skipScore:true}});
 m.configureHouse(m.house(House.Americans),{{country:m.country(Country.Americans),playerControl:true,iq:0,credits:1000,allies:[m.house(House.Americans)]}});
 const at=m.createWaypoint(30,60,60);
 const team=m.team('arrivals',{{house:m.house(House.Americans),waypoint:at,
  units:[{{type:m.unit(Unit.{carrier}),count:{carrier_count}}}{extra}{payload}],full:{full},reinforce:false,
  orders:[order.setGlobal(m.globalFlag(0)),order.guard(60),order.waitForever()]}});
 m.rule('arrive',{{when:event.sinceScenarioStart(1),once:true,actions:[action.reinforce(team,at)]}});
}});"#,
                    carrier_count = if kind == "multiple_carriers" { 2 } else { 1 }
                ),
            )?;
            mission_script::refresh_types(&entry, &catalog)?;
            worker
                .build(&doc, catalog.clone(), Sources::read(&entry)?)?
                .recv()?
                .map_err(anyhow::Error::msg)?
                .build
                .apply(&mut doc)?;
            let unchanged = !worker
                .build(&doc, catalog, Sources::read(&entry)?)?
                .recv()?
                .map_err(anyhow::Error::msg)?
                .build
                .apply(&mut doc)?;
            let team_type = mission::ids(&doc, Kind::Team)
                .into_iter()
                .find(|id| doc.ini_text(id, "Name") == "arrivals")
                .context("generated arrivals team")?;
            let bytes = doc.snapshot().bytes()?;
            std::fs::write(&path, &bytes)?;
            let mut runs = Vec::new();
            let mut continuous = None;
            for restore in [false, true] {
                let mut runtime = campaign::load(&install, &bytes, 1)?;
                let mut observations = Vec::new();
                let mut restored = Value::Null;
                for tick in 0..=180 {
                    if tick > 0 {
                        runtime.advance_frame(
                            &[],
                            22,
                            TickLane::Ordinary,
                            FrameEffects::default(),
                        )?;
                    }
                    if restore && tick == 32 {
                        let before = state(&runtime, &team_type);
                        checkpoint::restore_checkpoint(
                            &mut runtime,
                            &bytes,
                            &folder.join("cargo-32.bin"),
                        )?;
                        let after = state(&runtime, &team_type);
                        restored = json!({"preserved":before==after,"before":before,"after":after});
                    }
                    observations.push(state(&runtime, &team_type));
                }
                let tag = if restore { "restored" } else { "continuous" };
                std::fs::write(
                    folder.join(format!("{tag}.json")),
                    serde_json::to_vec_pretty(&observations)?,
                )?;
                let mut checks = BTreeMap::from([
                    ("unchanged_rebuild", unchanged),
                    (
                        "no_objects_before_trigger",
                        observations[15]["actors"].as_array().unwrap().len() == 2,
                    ),
                    (
                        "one_actual_team",
                        observations[32]["teams"].as_array().unwrap().len() == 1,
                    ),
                    (
                        "following_script_executed",
                        observations[32]["marker"] == true,
                    ),
                    (
                        "checkpoint_observed_state",
                        !restore || restored["preserved"] == true,
                    ),
                ]);
                let first = &observations[16];
                let objects = first["actors"].as_array().unwrap();
                let born = objects
                    .iter()
                    .filter(|e| e["type"] != "GAPOWR" && e["type"] != "NAPOWR")
                    .map(|e| e["id"].clone())
                    .collect::<Vec<_>>();
                let carrier_count = if kind == "multiple_carriers" { 2 } else { 1 };
                let payload_count = if kind == "miner" {
                    0
                } else if kind == "ground_over_capacity" {
                    6
                } else {
                    2
                };
                checks.insert(
                    "all_task_force_members_created",
                    born.len() == carrier_count + usize::from(mixed) + payload_count,
                );
                checks.insert(
                    "all_created_members_remain_in_team_including_cargo",
                    observations[16..].iter().all(|row| {
                        let teams = row["teams"].as_array().unwrap();
                        teams.len() == 1
                            && teams[0]["owner"] == "Americans"
                            && teams[0]["members"].as_array().is_some_and(|members| {
                                members.len() == born.len()
                                    && born.iter().all(|id| members.contains(id))
                            })
                    }),
                );
                let selected = objects
                    .iter()
                    .filter(|e| e["type"] == carrier)
                    .max_by_key(|e| e["id"].as_u64());
                let object_rules = runtime
                    .resources
                    .rules
                    .object(carrier)
                    .context("carrier rules")?;
                let rules_fact = json!({"passengers":object_rules.passengers,"naval":object_rules.naval,"storage":object_rules.storage});
                checks.insert(
                    "carrier_capacity_precondition",
                    object_rules.passengers
                        == match kind {
                            "naval" | "mixed_naval" => 12,
                            "air_no_capacity" | "miner" => 0,
                            "air_declared_capacity" => 2,
                            _ => 5,
                        },
                );
                checks.insert("actual_carrier_created", selected.is_some());
                if let Some(selected) = selected {
                    let cargo = selected["cargo"].as_array().unwrap();
                    let expected_payload = objects
                        .iter()
                        .filter(|e| {
                            born.contains(&e["id"])
                                && e["type"] != carrier
                                && full
                                && kind != "air_no_capacity"
                        })
                        .map(|e| e["id"].clone())
                        .collect::<Vec<_>>();
                    checks.insert(
                        "cargo_contains_exact_original_payload",
                        cargo.len() == expected_payload.len()
                            && expected_payload.iter().all(|id| cargo.contains(id)),
                    );
                    checks.insert(
                        "cargo_matches_full_option",
                        cargo.len()
                            == if full && kind != "miner" && kind != "air_no_capacity" {
                                if mixed {
                                    3
                                } else if kind == "ground_over_capacity" {
                                    6
                                } else {
                                    2
                                }
                            } else {
                                0
                            },
                    );
                    checks.insert(
                        "cargo_members_are_existing_limbo_entities",
                        cargo.iter().all(|id| {
                            objects.iter().any(|e| {
                                e["id"] == *id && e["limbo"] == true && e["owner"] == "Americans"
                            })
                        }),
                    );
                    if full && kind == "multiple_carriers" {
                        checks.insert(
                            "two_carriers_survive",
                            objects.iter().filter(|e| e["type"] == carrier).count() == 2,
                        );
                        checks.insert(
                            "earlier_carrier_has_no_cargo",
                            objects
                                .iter()
                                .filter(|e| e["type"] == carrier && e["id"] != selected["id"])
                                .all(|e| e["cargo"] == json!([])),
                        );
                    }
                    if full && kind == "ground_over_capacity" {
                        checks.insert(
                            "constructor_cargo_exceeds_boarding_capacity",
                            cargo.len() > object_rules.passengers as usize,
                        );
                    }
                    if full && kind == "air_no_capacity" {
                        checks.insert(
                            "retail_plane_has_no_passengers_capacity",
                            object_rules.passengers == 0,
                        );
                        checks.insert(
                            "payload_remains_on_map",
                            objects.iter().filter(|e| e["type"] == "ENGINEER").count() == 2
                                && objects
                                    .iter()
                                    .filter(|e| e["type"] == "ENGINEER")
                                    .all(|e| e["limbo"] == false),
                        );
                    }
                    if kind == "miner" {
                        checks.insert(
                            "miner_hold_matches_full_option",
                            selected["miner"]["bales"]
                                .as_array()
                                .is_some_and(|b| b.len() == if full { 40 } else { 0 }),
                        );
                        checks.insert(
                            "miner_capacity_is_retail_40",
                            selected["miner"]["capacity"] == 40,
                        );
                        checks.insert(
                            "fill_does_not_change_credits",
                            observations[0]["credits"] == observations[32]["credits"],
                        );
                        checks.insert(
                            "miner_hold_uses_slot_zero",
                            !full
                                || selected["miner"]["bales"]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .all(|b| b["tiberium_type"] == 0),
                        );
                    }
                }
                checks.insert(
                    "birth_identity_owner_health_retained",
                    observations[16..].iter().all(|row| {
                        objects.iter().all(|a| {
                            row["actors"].as_array().unwrap().iter().any(|b| {
                                a["id"] == b["id"]
                                    && a["type"] == b["type"]
                                    && a["owner"] == b["owner"]
                                    && a["health"] == b["health"]
                                    && b["alive"] == true
                            })
                        })
                    }),
                );
                checks.insert(
                    "cargo_chains_and_miner_holds_retained",
                    observations[16..].iter().all(|row| {
                        objects.iter().all(|a| {
                            row["actors"].as_array().unwrap().iter().any(|b| {
                                a["id"] == b["id"]
                                    && a["cargo"] == b["cargo"]
                                    && a["miner"] == b["miner"]
                                    && a["limbo"] == b["limbo"]
                            })
                        })
                    }),
                );
                checks.insert(
                    "no_losses_from_cargo_construction",
                    observations.iter().all(|r| r["losses"] == 0),
                );
                if restore {
                    checks.insert(
                        "restored_listed_timeline_equals_continuous",
                        continuous.as_ref() == Some(&observations),
                    );
                } else {
                    continuous = Some(observations.clone());
                }
                let passed = checks.values().all(|v| *v);
                runs.push(json!({"name":tag,"passed":passed,"checks":checks,"carrier_rules":rules_fact,"at_creation":first,"checkpoint":restored,"final":observations.last()}));
            }
            let passed = runs.iter().all(|r| r["passed"] == true);
            cases.push(json!({"case":name,"passed":passed,"runs":runs}));
            let report = json!({"passed":cases.iter().all(|r|r["passed"]==true),"cases":cases,"scope":"TS Full creation and real transport/miner cargo, ordinary frames and public checkpoint at32. No direct cargo/object/state injection, engine changes, GUI, networking or full-state equivalence claim."});
            std::fs::write(
                root.join("report.json"),
                serde_json::to_vec_pretty(&report)?,
            )?;
        }
    }
    ensure!(
        cases.iter().all(|r| r["passed"] == true),
        "Full audit failed; see report.json"
    );
    println!("{}", root.join("report.json").display());
    Ok(())
}
