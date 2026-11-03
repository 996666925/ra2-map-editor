//! TS House conditions through actual pickup, building lifecycle and Team scripts.
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
        command::{Command, CommandEnvelope},
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
    // Country indices must not accidentally stand in for House slots.
    for house in ["Russians", "Americans"] {
        mission::create(&mut doc, Kind::House, house, house, None)?;
        doc.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    doc.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    doc.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    doc.add_record("Structures", [28, 52], "Americans", "GAPOWR")?;
    doc.add_record("Structures", [34, 52], "Americans", "NAHAND")?;
    doc.add_record("Structures", [82, 82], "Russians", "NAPOWR")?;
    doc.add_record("Infantry", [30, 50], "Americans", "ENGINEER")?;
    doc.add_record("Units", [44, 60], "Americans", "MTNK")?;
    for (section, key, value) in [
        ("GAPOWR", "Power", "100"),
        ("NAHAND", "Power", "-150"),
        ("NAHAND", "Crewed", "no"),
        ("MTNK", "Primary", "none"),
        ("MTNK", "Secondary", "none"),
        ("CrateRules", "SoloCrateMoney", "4321"),
        ("CrateRules", "WoodCrate", "Money"),
    ] {
        doc.set_ini(section, key, Some(value.as_bytes().to_vec()))?;
    }
    doc.commit("House conditions fixture");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(
        install,
        root.join("base.map").to_str().context("map path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let source=r#"import {defineMission,event,action,order,Crate,TagRepeat,type Event} from './sdk';
import {map,House,Country,Building,Techno,MapObject,Unit} from './map.generated';
export default defineMission(map,{id:'house_conditions',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),credits:10,iq:0,playerControl:true,allies:[m.house(House.Americans)]});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),credits:70,iq:0,allies:[m.house(House.Russians)]});
 const stage=m.globalFlag(0), rebuilt=m.globalFlag(1);
 const observations:readonly (readonly [string,Event,Country?])[]=[
  ['funds_ge_equal',event.creditsAtLeast(1000)],['funds_le_equal',event.creditsAtMost(1000)],
  ['funds_ge_above',event.creditsAtLeast(1001)],['funds_le_below',event.creditsAtMost(999)],
  ['funds_reward_equal',event.creditsAtLeast(5321)],['funds_reward_above',event.creditsAtLeast(5322)],
  ['wrong_owner_funds',event.creditsAtMost(1000),Country.Russians],
  ['build_loss',event.buildingsLost(1)],['build_loss_two',event.buildingsLost(2)],
  ['unit_loss',event.unitsLost(1)],['unit_loss_two',event.unitsLost(2)],
  ['wrong_owner_build_loss',event.buildingsLost(1),Country.Russians],
  ['wrong_owner_unit_loss',event.unitsLost(1),Country.Russians],
  ['no_factories',event.noFactories()],
  ['low_power',event.lowPower(m.country(Country.Americans)),Country.Russians],
  ['full_after',event.all(event.fullPower(m.country(Country.Americans)),event.globalSet(stage)),Country.Russians],
  ['full_before_destruction',event.all(event.fullPower(m.country(Country.Americans)),event.sinceScenarioStart(5)),Country.Russians],
  ['wrong_country_low',event.lowPower(m.country(Country.Russians))],
  ['wrong_country_full',event.fullPower(m.country(Country.Russians))],
  ['null_country_low',event.lowPower(m.country(Country.Alliance))],
  ['null_country_full',event.fullPower(m.country(Country.Alliance))],
  ['factory_present',event.buildingExists(m.building(Building.NAHAND))],
  ['factory_absent',event.buildingAbsent(m.building(Building.NAHAND))],
  ['wrong_owner_present',event.buildingExists(m.building(Building.NAHAND)),Country.Russians],
  ['runtime_building_record',event.buildingBuilt(m.building(Building.GAPOWR))],
  ['late_no_factories',event.all(event.noFactories(),event.globalSet(rebuilt))],
 ];
 for(const [id,when,owner] of observations) m.rule(id,{when,owner:m.country(owner??Country.Americans),once:true,
   actions:[action.message(m.defineText(id,id))]});
 const nulls:readonly Event[]=[event.creditsAtLeast(999999),event.creditsAtMost(0),event.buildingsLost(100),event.unitsLost(100),
   event.noFactories(),event.buildingExists(m.building(Building.NAHAND)),event.buildingAbsent(m.building(Building.NAHAND)),
   event.buildingBuilt(m.building(Building.GAPOWR)),event.vehicleBuilt(m.techno(Techno.MTNK)),
   event.infantryBuilt(m.techno(Techno.E1)),event.aircraftBuilt(m.techno(Techno.PDPLANE))];
 for(let i=0;i<nulls.length;i++) { const id='null_owner_'+i; m.rule(id,{when:nulls[i]!,owner:m.country(Country.Alliance),once:true,
   actions:[action.message(m.defineText(id,id))]});
   const polled='null_owner_polled_'+i; m.rule(polled,{when:event.all(nulls[i]!,event.sinceScenarioStart(1)),
     owner:m.country(Country.Alliance),once:true,actions:[action.message(m.defineText(polled,polled))]}); }
 for(const [id,when] of [
  ['present_live',event.all(event.buildingExists(m.building(Building.NAHAND)),event.globalSet(stage))],
  ['absent_live',event.all(event.buildingAbsent(m.building(Building.NAHAND)),event.globalSet(rebuilt))],
  ['record_latched',event.all(event.buildingBuilt(m.building(Building.GAPOWR)),event.globalSet(rebuilt))],
 ] as const) m.rule(id,{when,owner:m.country(Country.Americans),once:false,
   actions:[action.message(m.defineText(id,id)),action.disableRule(m.ruleRef(id))]});
 const tank=m.team('tank',{house:m.house(House.Americans),waypoint:m.createWaypoint(31,44,60),
   units:[{type:m.unit(Unit.MTNK),count:1}],max:1,full:true,reinforce:false,
   orders:[order.guard(8),__TAIL__]});
 m.rule('dispatch',{when:event.sinceScenarioStart(1),once:true,actions:[action.recruit(tank)]});
 __CHANGES__
});"#.replace("__TAIL__",match name { "kill"=>"order.killMembers()", "remove"=>"order.removeMembers()",_=>"order.waitForever()" })
    .replace("__CHANGES__",if name=="idle" {""}else{r#"
 m.rule('crate',{when:event.sinceScenarioStart(1),once:true,
   actions:[action.createCrate(Crate.Money,m.createWaypoint(30,40,50))]});
 m.rule('power',{when:event.sinceScenarioStart(4),once:true,owner:m.country(Country.Americans),
   actions:[action.createBuilding(m.building(Building.GAPOWR),m.createWaypoint(32,36,58)),action.message(m.defineText('power_added','power_added'))]});
 const destroy=m.rule('destroy',{when:event.sinceScenarioStart(8),once:true,
   actions:[action.destroyAttached(),action.setGlobal(stage),action.message(m.defineText('factory_destroyed','factory_destroyed'))]});
 m.bindObject(m.object(MapObject.Structures_1),m.tag('factory',{rule:destroy,repeat:TagRepeat.Once}));
 m.rule('rebuild',{when:event.sinceScenarioStart(12),once:true,owner:m.country(Country.Americans),
   actions:[action.createBuilding(m.building(Building.NAHAND),m.createWaypoint(33,34,52)),action.setGlobal(rebuilt),
     action.message(m.defineText('factory_recreated','factory_recreated'))]});
"#});
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
    let houses=["Americans","Russians"].into_iter().map(|name| -> Result<Value> {
        let id=sim.interner().get(name).context("House ID")?;
        let house=sim.houses().get(&id).context("House")?;
        let power=sim.power_states().get(&id);
        let counts=saved_field::read(house.tracking(),&["active_building_types"])?;
        let active=|name:&str|sim.interner().get(name).and_then(|id|counts.get(id.index().to_string())).and_then(Value::as_i64).unwrap_or(0);
        Ok(json!({"name":name,"money":house.economy().available_money(),"buildings_lost":house.stats().buildings_lost(),
            "units_lost":house.stats().units_lost(),"active_factory":active("NAHAND"),"active_power":active("GAPOWR"),
            "scored_built":house.stats().built(),"last_built":saved_field::read(house,&["last_built_types"])?,
            "power":power.map(|p|json!({"output":p.total_output(),"drain":p.total_drain(),"low":p.low_power()}))}))
    }).collect::<Result<Vec<_>>>()?;
    let actors=sim.entities().iter_sorted().map(|(id,e)|json!({"id":id,"type":sim.interner().resolve(e.type_ref()),
        "owner":sim.interner().resolve(e.owner()),"cell":sim.object_coords_cell(id),"health":e.health.current(),
        "limbo":e.lifecycle.in_limbo(),"alive":e.lifecycle.object_alive()})).collect::<Vec<_>>();
    Ok(json!({"houses":houses,"actors":actors,"flags":sim.global_variables()[..2]}))
}
fn audit(install: &Install, bytes: &[u8], root: &Path, name: &str, restore: bool) -> Result<Value> {
    std::fs::create_dir_all(root)?;
    let mut runtime = campaign::load(install, bytes, 1)?;
    let sim = &runtime.simulation;
    let owner = sim.interner().get("Americans").context("owner")?;
    let actor_id = |name: &str| {
        sim.entities()
            .iter_sorted()
            .find(|(_, e)| sim.interner().resolve(e.type_ref()) == name)
            .map(|(id, _)| id)
            .context("authored actor")
    };
    let collector = actor_id("ENGINEER")?;
    let tank = actor_id("MTNK")?;
    let factory = actor_id("NAHAND")?;
    let initial = state(&runtime)?;
    let texts = mission_script::declared_texts(bytes)?;
    let mut observations = Vec::new();
    let mut messages = Vec::new();
    let mut checkpoint_state = None;
    for frame in 0..700 {
        let tick = runtime.simulation.session().tick();
        let commands = if frame == 60 && name != "idle" {
            vec![CommandEnvelope::synchronized(
                runtime
                    .simulation
                    .encode_megamission_record(
                        owner,
                        &Command::Move {
                            entity_id: collector,
                            target_rx: 42,
                            target_ry: 50,
                        },
                    )
                    .context("Move encoding")?,
                tick,
            )]
        } else {
            Vec::new()
        };
        let output =
            runtime.advance_frame(&commands, 22, TickLane::Ordinary, FrameEffects::default())?;
        let tick = runtime.simulation.session().tick();
        let current = state(&runtime)?;
        for effect in output.trigger_effects {
            if let TriggerEffect::LocalizedMessage { csf_label } = effect {
                messages.push(json!({"tick":tick,"event":texts.get(&csf_label).context("message label")?,"state":current}));
            }
        }
        observations.push(json!({"tick":tick,"state":current}));
        if restore && frame == 39 {
            checkpoint::restore_checkpoint(&mut runtime, bytes, &root.join("before-changes.bin"))?;
            let after = state(&runtime)?;
            checkpoint_state =
                Some(json!({"preserved":after==current,"before":current,"after":after}));
        }
    }
    let final_state = state(&runtime)?;
    let changed = name != "idle";
    let count = |event: &str| messages.iter().filter(|m| m["event"] == event).count();
    let first = |event: &str| messages.iter().find(|m| m["event"] == event);
    let americans = &final_state["houses"][0];
    let original = |id| {
        final_state["actors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == id)
    };
    let mut checks = BTreeMap::from([
        (
            "initial_literal_funds_not_map_units",
            initial["houses"][0]["money"] == 1000 && initial["houses"][1]["money"] == 7000,
        ),
        (
            "inclusive_initial_funds_thresholds",
            count("funds_ge_equal") == 1 && count("funds_le_equal") == 1,
        ),
        (
            "funds_one_above_requires_reward",
            count("funds_ge_above") == usize::from(changed),
        ),
        (
            "funds_below_initial_never_met",
            count("funds_le_below") == 0,
        ),
        (
            "reward_threshold_is_inclusive",
            count("funds_reward_equal") == usize::from(changed) && count("funds_reward_above") == 0,
        ),
        (
            "reward_changes_real_available_money",
            americans["money"] == if changed { 5321 } else { 1000 },
        ),
        (
            "rule_owner_selects_funds_and_losses",
            count("wrong_owner_funds") == 0
                && count("wrong_owner_build_loss") == 0
                && count("wrong_owner_unit_loss") == 0,
        ),
        (
            "one_building_loss",
            americans["buildings_lost"] == u64::from(changed)
                && count("build_loss") == usize::from(changed)
                && count("build_loss_two") == 0,
        ),
        (
            "kill_but_not_remove_counts_unit_loss",
            americans["units_lost"] == u64::from(name == "kill")
                && count("unit_loss") == usize::from(name == "kill")
                && count("unit_loss_two") == 0,
        ),
        (
            "initial_existing_factory",
            count("factory_present") == 1 && initial["houses"][0]["active_factory"] == 1,
        ),
        (
            "absent_factory_only_after_destruction",
            count("factory_absent") == usize::from(changed)
                && first("factory_absent")
                    .is_none_or(|m| m["state"]["houses"][0]["active_factory"] == 0),
        ),
        (
            "no_factories_reads_living_buildings",
            count("no_factories") == usize::from(changed)
                && first("no_factories")
                    .is_none_or(|m| m["state"]["houses"][0]["active_factory"] == 0),
        ),
        (
            "no_factories_does_not_latch",
            count("late_no_factories") == 0,
        ),
        (
            "factory_recreated_without_preserving_original",
            americans["active_factory"] == 1 && (original(factory).is_none() == changed),
        ),
        (
            "country_selects_power_independently_of_owner",
            count("low_power") == 1
                && count("wrong_country_low") == 0
                && count("wrong_country_full") == 1,
        ),
        (
            "power_recovers_through_real_construction",
            count("full_after") == usize::from(changed)
                && americans["active_power"] == if changed { 2 } else { 1 },
        ),
        (
            "additional_plant_resolves_deficit_before_factory_destruction",
            count("full_before_destruction") == usize::from(changed)
                && first("full_before_destruction").is_none_or(|m| {
                    m["state"]["houses"][0]["active_factory"] == 1
                        && m["state"]["houses"][0]["power"]
                            == json!({"output":200,"drain":150,"low":false})
                }),
        ),
        (
            "funds_callback_reads_exact_reward_balance",
            first("funds_reward_equal").is_none_or(|m| m["state"]["houses"][0]["money"] == 5321),
        ),
        (
            "null_country_power_tail",
            count("null_country_low") == 1 && count("null_country_full") == 1,
        ),
        (
            "existing_owner_filters_building_type",
            count("wrong_owner_present") == 0,
        ),
        (
            "runtime_discovery_records_building_without_scoring_production",
            count("runtime_building_record") == usize::from(changed)
                && americans["scored_built"] == 0,
        ),
        (
            "present_condition_waits_for_current_count_after_destruction",
            count("present_live") == usize::from(changed)
                && first("present_live").is_none_or(|m| {
                    m["state"]["houses"][0]["active_factory"] == 1 && m["state"]["flags"][1] == true
                }),
        ),
        (
            "absent_condition_does_not_latch_before_recreation",
            count("absent_live") == 0,
        ),
        (
            "building_record_latches_after_type_changes",
            count("record_latched") == usize::from(changed)
                && first("record_latched").is_none_or(|m| {
                    m["state"]["houses"][0]["last_built"][3]
                        != json!(
                            runtime
                                .resources
                                .rules
                                .type_array_index(
                                    vera20k_core::rules::object_type::ObjectCategory::Building,
                                    "GAPOWR"
                                )
                                .unwrap()
                        )
                }),
        ),
        (
            "original_tank_lifecycle",
            original(tank).is_none() == changed,
        ),
        (
            "collector_survives_and_reaches_destination",
            original(collector).is_some_and(|a| {
                a["owner"] == "Americans"
                    && a["health"]
                        == initial["actors"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|a| a["id"] == collector)
                            .unwrap()["health"]
                    && a["cell"]
                        == if changed {
                            json!([42, 50])
                        } else {
                            json!([30, 50])
                        }
            }),
        ),
        (
            "other_house_economy_counters_and_witness_unchanged",
            [
                "money",
                "buildings_lost",
                "units_lost",
                "active_factory",
                "active_power",
                "scored_built",
                "last_built",
            ]
            .iter()
            .all(|key| final_state["houses"][1][key] == initial["houses"][1][key])
                && initial["actors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|a| a["type"] == "GAPOWR" || a["type"] == "NAPOWR")
                    .all(|a| {
                        observations
                            .iter()
                            .all(|r| r["state"]["actors"].as_array().unwrap().contains(a))
                    }),
        ),
        (
            "stable_power_totals_show_real_deficit_and_recovery",
            observations.iter().any(|r| {
                r["state"]["houses"][0]["power"] == json!({"output":100,"drain":150,"low":true})
            }) && americans["power"]
                == if changed {
                    json!({"output":200,"drain":150,"low":false})
                } else {
                    json!({"output":100,"drain":150,"low":true})
                },
        ),
    ])
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect::<BTreeMap<_, _>>();
    for i in 0..11 {
        checks.insert(
            format!("null_owner_has_no_house_poll_{i}"),
            count(&format!("null_owner_{i}")) == 0,
        );
        checks.insert(
            format!("null_owner_true_tail_when_logic_polled_{i}"),
            count(&format!("null_owner_polled_{i}")) == 1,
        );
    }
    if restore {
        checks.insert(
            "checkpoint_preserves_listed_state".into(),
            checkpoint_state
                .as_ref()
                .is_some_and(|c| c["preserved"] == true),
        );
    }
    let passed = checks.values().all(|b| *b);
    std::fs::write(
        root.join("observations.json"),
        serde_json::to_vec_pretty(&observations)?,
    )?;
    // Original identities are stable; newly created IDs may shift after load
    // because the current scenario RNG stream and transient allocations reset.
    let mut outcome = final_state.clone();
    for actor in outcome["actors"].as_array_mut().unwrap() {
        if !initial["actors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == actor["id"])
        {
            actor.as_object_mut().unwrap().remove("id");
        }
    }
    Ok(
        json!({"passed":passed,"checks":checks,"messages":messages,"initial":initial,"final":final_state,"listed_outcome":outcome,"checkpoint":checkpoint_state}),
    )
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(args.next().context("new output directory")?)?;
    ensure!(
        args.next().is_none() && !root.exists(),
        "output must be new"
    );
    std::fs::create_dir_all(&root)?;
    let mut install = Install::open(&game)?;
    let mut cases = Vec::new();
    for name in ["kill", "remove", "idle"] {
        let folder = root.join(name);
        let bytes = build(&mut install, &folder, name)?;
        let continuous = audit(&install, &bytes, &folder.join("continuous"), name, false)?;
        if name != "idle" {
            let restored = audit(&install, &bytes, &folder.join("restored"), name, true)?;
            let event_names = |result: &Value| {
                result["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|m| m["event"].clone())
                    .collect::<Vec<_>>()
            };
            let agreement = event_names(&continuous) == event_names(&restored)
                && continuous["listed_outcome"] == restored["listed_outcome"];
            let ticks_match = continuous["messages"] == restored["messages"];
            cases.push(json!({"case":format!("{name}_restored"),"result":restored,"listed_observations_match":agreement,"continuous_ticks_and_callback_states_match":ticks_match}));
        }
        cases.push(json!({"case":name,"result":continuous}));
    }
    let passed = cases.iter().all(|c| {
        c["result"]["passed"] == true
            && c.get("listed_observations_match").is_none_or(|v| v == true)
    });
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&json!({"passed":passed,"cases":cases,
      "scope":"TS events, public ordinary Move and TS building/Team actions. No money, power, loss, tracking or notification injection. Selected headless observations only; not GUI, network, empty-silo parity or all-state equivalence."}))?,
    )?;
    ensure!(passed, "House conditions audit failed; inspect report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
