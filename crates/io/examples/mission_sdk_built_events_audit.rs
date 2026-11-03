//! TS recent-built events from ordinary paid production and delivery.
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
    rules::object_type::ObjectCategory,
    sim::{
        command::{Command, CommandEnvelope, PlaceRecord, ProductionEvent, ProductionRecord},
        production::{ProductionCategory, ProductionFactoryVisit},
        runtime::SimRuntime,
        trigger_runtime::TriggerEffect,
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;
#[path = "support/saved_field.rs"]
mod saved_field;

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    category: ProductionCategory,
    first: &'static str,
    second: &'static str,
    factory: &'static str,
    cancel: bool,
    idle: bool,
}
impl Case {
    fn object_category(self) -> ObjectCategory {
        match self.category {
            ProductionCategory::Building => ObjectCategory::Building,
            ProductionCategory::Infantry => ObjectCategory::Infantry,
            ProductionCategory::Aircraft => ObjectCategory::Aircraft,
            _ => ObjectCategory::Vehicle,
        }
    }
    fn index(self) -> usize {
        match self.object_category() {
            ObjectCategory::Infantry => 0,
            ObjectCategory::Vehicle => 1,
            ObjectCategory::Aircraft => 2,
            ObjectCategory::Building => 3,
        }
    }
    fn event(self) -> &'static str {
        match self.object_category() {
            ObjectCategory::Building => "buildingBuilt",
            ObjectCategory::Infantry => "infantryBuilt",
            ObjectCategory::Aircraft => "aircraftBuilt",
            ObjectCategory::Vehicle => "vehicleBuilt",
        }
    }
}
fn build(install: &mut Install, root: &Path, case: Case) -> Result<Vec<u8>> {
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
    doc.add_record("Structures", [82, 82], "Russians", "NAPOWR")?;
    doc.add_record("Structures", [40, 60], "Americans", case.factory)?;
    for (section, key, value) in [
        ("General", "BuildSpeed", "0.1"),
        ("NAPOWR", "Power", "2000"),
    ] {
        doc.set_ini(section, key, Some(value.as_bytes().to_vec()))?;
    }
    for (i, name) in [case.first, case.second].iter().enumerate() {
        for (key, value) in [
            ("Owner", "Americans"),
            ("Prerequisite", ""),
            ("TechLevel", "1"),
            ("RequiredHouses", ""),
            ("ForbiddenHouses", ""),
            ("Primary", "none"),
            ("Secondary", "none"),
            ("FreeUnit", "none"),
            ("Adjacent", "8"),
        ] {
            doc.set_ini(name, key, Some(value.as_bytes().to_vec()))?;
        }
        doc.set_ini(name, "Cost", Some((400 + i * 200).to_string().into_bytes()))?;
    }
    doc.commit("built event fixture");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(
        install,
        root.join("base.map").to_str().context("map path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let reference = if case.category == ProductionCategory::Building {
        "building"
    } else {
        "techno"
    };
    let family = if case.category == ProductionCategory::Building {
        "Building"
    } else {
        "Techno"
    };
    let source=r#"import {defineMission,event,action} from './sdk';
import {map,House,Country,__FAMILY__} from './map.generated';
export default defineMission(map,{id:'built_events',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),credits:1000,iq:0,playerControl:true,allies:[m.house(House.Americans)]});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),credits:1000,iq:0,allies:[m.house(House.Russians)]});
 const first=event.__EVENT__(m.__REF__(__FAMILY__.__FIRST__));
 const second=event.__EVENT__(m.__REF__(__FAMILY__.__SECOND__));
 for(const [id,when,owner] of [
   ['first',first,Country.Americans],['second',second,Country.Americans],
   ['wrong_owner',first,Country.Russians],
   ['first_latched',event.all(first,event.globalSet(m.globalFlag(0))),Country.Americans],
 ] as const) m.rule(id,{when,owner:m.country(owner),once:id==='first_latched'?false:true,
   actions:[action.message(m.defineText(id,id)),...(id==='first_latched'?[action.disableRule(m.ruleRef(id))]:[])]});
 m.rule('late',{when:event.sinceScenarioStart(80),once:true,actions:[action.setGlobal(m.globalFlag(0)),action.message(m.defineText('late','late'))]});
});"#.replace("__FAMILY__",family).replace("__REF__",reference).replace("__EVENT__",case.event()).replace("__FIRST__",case.first).replace("__SECOND__",case.second);
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
fn state(runtime: &SimRuntime, case: Case) -> Result<Value> {
    let sim = &runtime.simulation;
    let owner = sim.interner().get("Americans").context("owner")?;
    let houses=["Americans","Russians"].into_iter().map(|name|->Result<Value>{
        let h=sim.houses().get(&sim.interner().get(name).unwrap()).context("House")?;
        Ok(json!({"name":name,"money":h.economy().available_money(),"built":h.stats().built(),"units_lost":h.stats().units_lost(),
            "buildings_lost":h.stats().buildings_lost(),"last_built":saved_field::read(h,&["last_built_types"])?}))
    }).collect::<Result<Vec<_>>>()?;
    let factory = sim
        .production()
        .factories()
        .view(owner, case.category)
        .map(|v| {
            json!({"progress":v.progress,"on_hold":v.on_hold,
        "suspended":v.suspended,"object":v.object,"queue":v.queue})
        });
    let actors=sim.entities().iter_sorted().map(|(id,e)|json!({"id":id,"type":sim.interner().resolve(e.type_ref()),
        "owner":sim.interner().resolve(e.owner()),"cell":sim.object_coords_cell(id),"health":e.health.current(),
        "limbo":e.lifecycle.in_limbo(),"alive":e.lifecycle.object_alive()})).collect::<Vec<_>>();
    Ok(json!({"houses":houses,"factory":factory,"actors":actors,"late":sim.global_variables()[0]}))
}
fn audit(install: &Install, bytes: &[u8], root: &Path, case: Case, restore: bool) -> Result<Value> {
    std::fs::create_dir_all(root)?;
    let mut runtime = campaign::load(install, bytes, 1)?;
    let initial = state(&runtime, case)?;
    let owner = runtime
        .simulation
        .interner()
        .get("Americans")
        .context("owner")?;
    let first_index = runtime
        .resources
        .rules
        .type_array_index(case.object_category(), case.first)
        .context("first index")?;
    let second_index = runtime
        .resources
        .rules
        .type_array_index(case.object_category(), case.second)
        .context("second index")?;
    let texts = mission_script::declared_texts(bytes)?;
    let mut messages = Vec::new();
    let mut observations = Vec::new();
    let mut inputs = Vec::new();
    let mut automatic_places = Vec::new();
    let mut held_ids = BTreeMap::<String, u64>::new();
    let mut delivered = BTreeMap::<String, u64>::new();
    let mut second_queued = false;
    let mut cancel_sent = false;
    let mut checkpoint_state = None;
    let mut active_paid_checkpoint = false;
    for frame in 0..1800 {
        let sim = &runtime.simulation;
        let tick = sim.session().tick();
        let mut commands = Vec::new();
        let mut produce = None;
        let mut abandon = false;
        if !case.idle && frame == 60 {
            produce = Some(case.first);
        }
        if !case.idle
            && !case.cancel
            && !second_queued
            // Real buildup assets keep a newly placed building connected to
            // its yard while Construction runs. Wait for both normal owners
            // to release it before starting the next building product.
            && (case.category != ProductionCategory::Building
                || (held_ids.get(case.first).is_some_and(|id| {
                    sim.entities().get(*id).is_some_and(|e| {
                        !e.in_construction_bstate()
                            && e.mission.effective().known()
                                != Some(vera20k_core::sim::mission::MissionType::Construction)
                    })
                }) && sim.entities().iter_sorted().any(|(_, e)| {
                    e.owner() == owner
                        && sim.interner().resolve(e.type_ref()) == case.factory
                        && e.radio_contacts.is_empty()
                })))
            && delivered.get(case.first).is_some_and(|t| {
                tick > *t
                    + if case.category == ProductionCategory::Vehicle {
                        650
                    } else {
                        30
                    }
            })
        {
            produce = Some(case.second);
            second_queued = true;
        }
        if case.cancel
            && !cancel_sent
            && sim
                .production()
                .factories()
                .view(owner, case.category)
                .is_some_and(|v| v.progress >= 30 && v.progress < 54)
        {
            abandon = true;
            cancel_sent = true;
        }
        if let Some(name) = produce.or(abandon.then_some(case.first)) {
            let event = if abandon {
                ProductionEvent::Abandon
            } else {
                ProductionEvent::Produce
            };
            let record = sim
                .encode_production_record(
                    owner,
                    event,
                    sim.interner().get(name).context("product type")?,
                    &runtime.resources.rules,
                )
                .context("production encoding")?;
            let decoded = ProductionRecord::decode(&record).unwrap();
            inputs.push(json!({"tick":tick,"kind":if abandon {"abandon"}else{"produce"},"type":name,"rtti":decoded.rtti,"index":decoded.type_index,"house":decoded.house_id}));
            commands.push(CommandEnvelope::synchronized(record, tick));
        }
        if case.category == ProductionCategory::Building
            && let Some(v) = sim.production().factories().view(owner, case.category)
            && let Some(object) = v.complete_object()
        {
            let type_id = object.type_id;
            let name = sim.interner().resolve(type_id);
            let cell = if name == case.first {
                (44, 60)
            } else {
                (40, 64)
            };
            let record = sim
                .encode_place_building_record(owner, type_id, cell, &runtime.resources.rules)
                .context("building PLACE")?;
            inputs.push(json!({"tick":tick,"kind":"place","type":name,"cell":cell}));
            commands.push(CommandEnvelope::synchronized(record, tick));
        }
        if case.category == ProductionCategory::Vehicle && !case.cancel {
            for (name, target_rx) in [(case.first, 50), (case.second, 55)] {
                if !delivered.get(name).is_some_and(|t| tick == *t + 400) {
                    continue;
                }
                let id = *held_ids.get(name).context("delivered vehicle")?;
                let record = sim
                    .encode_megamission_record(
                        owner,
                        &Command::Move {
                            entity_id: id,
                            target_rx,
                            target_ry: 65,
                        },
                    )
                    .context("clear factory exit Move")?;
                inputs.push(json!({"tick":tick,"kind":"move","id":id,"cell":[target_rx,65]}));
                commands.push(CommandEnvelope::synchronized(record, tick));
            }
        }
        // The headless host has no retained GUI sidebar. Supply its one linked
        // cameo through the public host boundary so the production publisher
        // itself emits ordinary mobile PLACE; never synthesize completion.
        let visits = sim
            .production()
            .factories()
            .view(owner, case.category)
            .and_then(|view| {
                view.object.map(|object| ProductionFactoryVisit {
                    type_id: object.type_id,
                    category: case.category,
                    construction_order: view.construction_order(),
                })
            })
            .into_iter()
            .collect::<Vec<_>>();
        let mut output = runtime.advance_frame(
            &commands,
            22,
            TickLane::Ordinary,
            FrameEffects::default().with_production_strip(&visits, &|_| {}),
        )?;
        let sim = &runtime.simulation;
        let tick = sim.session().tick();
        for command in output.take_admitted_commands() {
            if let Command::SynchronizedRecord { record } = &command.payload
                && let Some(place) = PlaceRecord::decode(record)
                && case.category != ProductionCategory::Building
            {
                automatic_places.push(json!({"tick":tick,"rtti":place.rtti,"index":place.type_index,"house":place.house_id}));
            }
        }
        if let Some(v) = sim.production().factories().view(owner, case.category)
            && let Some(object) = v.object
            && let Some(id) = object.entity_id
        {
            held_ids
                .entry(sim.interner().resolve(object.type_id).to_owned())
                .or_insert(id);
        }
        for (name, id) in &held_ids {
            if sim
                .entities()
                .get(*id)
                .is_some_and(|e| !e.lifecycle.in_limbo())
            {
                delivered.entry(name.clone()).or_insert(tick);
            }
        }
        let current = state(&runtime, case)?;
        for effect in output.trigger_effects {
            if let TriggerEffect::LocalizedMessage { csf_label } = effect {
                messages.push(json!({"tick":tick,"event":texts.get(&csf_label).context("message")?,"state":current}));
            }
        }
        observations.push(json!({"tick":tick,"state":current}));
        if frame == 79 && restore {
            active_paid_checkpoint = current["factory"]["progress"]
                .as_u64()
                .is_some_and(|p| p > 0 && p < 54)
                && current["houses"][0]["money"].as_i64().unwrap()
                    < initial["houses"][0]["money"].as_i64().unwrap();
            checkpoint::restore_checkpoint(
                &mut runtime,
                bytes,
                &root.join("during-production.bin"),
            )?;
            let after = state(&runtime, case)?;
            checkpoint_state =
                Some(json!({"preserved":current==after,"before":current,"after":after}));
        }
    }
    let final_state = state(&runtime, case)?;
    let success = !case.idle && !case.cancel;
    let count = |event: &str| messages.iter().filter(|m| m["event"] == event).count();
    let first = |event: &str| messages.iter().find(|m| m["event"] == event);
    let mut checks = BTreeMap::from([
        (
            "initial_history_not_authored_buildings",
            initial["houses"][0]["last_built"] == json!([-1, -1, -1, -1]),
        ),
        (
            "first_type_event_matches_delivery",
            count("first") == usize::from(success),
        ),
        (
            "second_type_event_matches_delivery",
            count("second") == usize::from(success),
        ),
        ("wrong_owner_never_fires", count("wrong_owner") == 0),
        ("late_gate_fires_once", count("late") == 1),
        (
            "first_record_latches_until_later_gate",
            count("first_latched") == usize::from(success)
                && first("first_latched").is_none_or(|m| {
                    m["state"]["houses"][0]["last_built"][case.index()] == second_index
                }),
        ),
        (
            "first_callback_has_matching_class_index",
            first("first")
                .is_none_or(|m| m["state"]["houses"][0]["last_built"][case.index()] == first_index),
        ),
        (
            "second_callback_has_matching_class_index",
            first("second").is_none_or(|m| {
                m["state"]["houses"][0]["last_built"][case.index()] == second_index
            }),
        ),
        (
            "events_wait_for_actual_delivery",
            ["first", "second"]
                .into_iter()
                .zip([case.first, case.second])
                .all(|(event, name)| {
                    first(event).is_none_or(|m| {
                        delivered
                            .get(name)
                            .is_some_and(|tick| m["tick"].as_u64().unwrap() >= *tick)
                    })
                }),
        ),
        (
            "original_structures_preserved",
            initial["actors"].as_array().unwrap().iter().all(|a| {
                observations
                    .iter()
                    .all(|r| r["state"]["actors"].as_array().unwrap().contains(a))
            }),
        ),
        (
            "other_house_history_and_economy_preserved",
            final_state["houses"][1] == initial["houses"][1],
        ),
        (
            "no_kill_losses",
            final_state["houses"][0]["units_lost"] == 0
                && final_state["houses"][0]["buildings_lost"] == 0,
        ),
        (
            "cancel_or_idle_does_not_record_built",
            success
                || (final_state["houses"][0]["last_built"] == initial["houses"][0]["last_built"]
                    && final_state["houses"][0]["built"] == 0),
        ),
        (
            "paid_complete_products_record_two_builds",
            final_state["houses"][0]["built"] == if success { 2 } else { 0 },
        ),
        (
            "paid_complete_products_cost_exactly_1000",
            final_state["houses"][0]["money"] == if success { 99000 } else { 100000 },
        ),
        (
            "both_original_held_entities_delivered",
            !success
                || [case.first, case.second].iter().all(|name| {
                    held_ids.get(*name).is_some_and(|id| {
                        runtime.simulation.entities().get(*id).is_some_and(|e| {
                            !e.lifecycle.in_limbo() && e.owner() == owner && e.health.current() > 0
                        })
                    })
                }),
        ),
        (
            "normal_mobile_place_records",
            !success
                || case.category == ProductionCategory::Building
                || automatic_places.len() == 2,
        ),
        (
            "cancellation_retires_constructed_limbo_product",
            !case.cancel
                || (cancel_sent
                    && held_ids
                        .get(case.first)
                        .is_some_and(|id| runtime.simulation.entities().get(*id).is_none())
                    && delivered.is_empty()),
        ),
        (
            "normal_moves_deliver_both_vehicles_to_authored_destinations",
            !success
                || case.category != ProductionCategory::Vehicle
                || [(case.first, 50), (case.second, 55)]
                    .iter()
                    .all(|(name, x)| {
                        held_ids.get(*name).is_some_and(|id| {
                            runtime.simulation.object_coords_cell(*id) == Some((*x, 65))
                        })
                    }),
        ),
        (
            "paid_incomplete_first_product_is_limbo_with_no_built_record",
            case.idle
                || observations.iter().any(|r| {
                    r["state"]["factory"]["progress"]
                        .as_u64()
                        .is_some_and(|p| p > 0 && p < 30)
                        && r["state"]["houses"][0]["money"].as_i64().unwrap() < 100000
                        && r["state"]["houses"][0]["last_built"] == json!([-1, -1, -1, -1])
                        && r["state"]["actors"].as_array().unwrap().iter().any(|a| {
                            a["id"] == r["state"]["factory"]["object"]["entity_id"]
                                && a["limbo"] == true
                        })
                }),
        ),
    ])
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect::<BTreeMap<_, _>>();
    if restore {
        checks.insert(
            "checkpoint_preserves_listed_history_and_factory".into(),
            checkpoint_state
                .as_ref()
                .is_some_and(|v| v["preserved"] == true),
        );
        checks.insert(
            "checkpoint_during_paid_incomplete_build".into(),
            active_paid_checkpoint,
        );
    }
    let passed = checks.values().all(|v| *v);
    std::fs::write(
        root.join("observations.json"),
        serde_json::to_vec_pretty(&observations)?,
    )?;
    Ok(
        json!({"passed":passed,"checks":checks,"initial":initial,"final":final_state,"messages":messages,"inputs":inputs,
        "held_ids":held_ids,"delivered":delivered,"automatic_places":automatic_places,"checkpoint":checkpoint_state}),
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
    let vehicle = Case {
        name: "vehicle",
        category: ProductionCategory::Vehicle,
        first: "MTNK",
        second: "HTK",
        factory: "GAWEAP",
        cancel: false,
        idle: false,
    };
    let mut cases = Vec::new();
    for case in [
        vehicle,
        Case {
            name: "infantry",
            category: ProductionCategory::Infantry,
            first: "E1",
            second: "ENGINEER",
            factory: "GAPILE",
            ..vehicle
        },
        Case {
            name: "aircraft",
            category: ProductionCategory::Aircraft,
            first: "ORCA",
            second: "BEAG",
            factory: "GAAIRC",
            ..vehicle
        },
        Case {
            name: "building",
            category: ProductionCategory::Building,
            first: "GAPOWR",
            second: "NAHAND",
            factory: "GACNST",
            ..vehicle
        },
        Case {
            name: "cancel",
            cancel: true,
            ..vehicle
        },
        Case {
            name: "idle",
            idle: true,
            ..vehicle
        },
    ] {
        let folder = root.join(case.name);
        let bytes = build(&mut install, &folder, case)?;
        let continuous = audit(&install, &bytes, &folder.join("continuous"), case, false)?;
        if !case.idle {
            let restored = audit(&install, &bytes, &folder.join("restored"), case, true)?;
            let events = |v: &Value| {
                v["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|m| m["event"].clone())
                    .collect::<Vec<_>>()
            };
            let agreement = events(&continuous) == events(&restored)
                && continuous["final"] == restored["final"]
                && continuous["held_ids"] == restored["held_ids"];
            let ticks_match = continuous["messages"] == restored["messages"]
                && continuous["delivered"] == restored["delivered"];
            cases.push(json!({"case":format!("{}_restored",case.name),"result":restored,"listed_outcomes_match":agreement,"continuous_ticks_and_callback_states_match":ticks_match}));
        }
        cases.push(json!({"case":case.name,"result":continuous}));
    }
    let passed = cases.iter().all(|c| {
        c["result"]["passed"] == true && c.get("listed_outcomes_match").is_none_or(|v| v == true)
    });
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&json!({"passed":passed,"cases":cases,
      "scope":"TS events19-22; paid ordinary synchronized PRODUCE/PLACE/ABANDON and post-exit Move, original held-entity delivery and headless checkpoint. Headless host supplies one Factory link through the public production-strip boundary; mobile PLACE is emitted by the normal production publisher. Controlled map prerequisites/owners/costs/build speed, no progress/credits/history/position/notification injection. Not retained GUI sidebar, GUI saves or network equivalence."}))?,
    )?;
    ensure!(passed, "built events audit failed; inspect report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
