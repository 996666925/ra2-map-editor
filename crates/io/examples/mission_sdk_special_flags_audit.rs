//! TS-owned startup flags, ordinary damage/ore/targeting and actual starting forces.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument, Target, Value as DocumentValue},
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
    headless_scenario::{battle_session, load_with_launch_and_ini},
    map::map_file,
    sim::{
        runtime::SimRuntime,
        scenario_bootstrap::MatchLaunchDescriptor,
        scenario_session::ScenarioGameMode,
        world::{FrameEffects, TickLane},
    },
    skirmish_launch::LaunchStartPosition,
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;
#[path = "support/saved_field.rs"]
mod saved_field;

fn campaign_state(runtime: &SimRuntime) -> Value {
    let sim = &runtime.simulation;
    let objects=sim.entities().iter_sorted().map(|(id,e)|json!({"id":id,"type":sim.interner().resolve(e.type_ref()),"owner":sim.interner().resolve(e.owner()),"health":e.health.current(),"cell":[e.position.rx(),e.position.ry()],"target":e.attack_target,"rank":e.veterancy.rank()})).collect::<Vec<_>>();
    let ore = sim.overlay_grid().map(|grid| {
        grid.iter_occupied()
            .map(|(x, y, c)| json!({"cell":[x,y],"id":c.overlay_id,"data":c.overlay_data}))
            .collect::<Vec<_>>()
    });
    json!({"tick":sim.session().tick(),"inert":sim.session().no_damage(),"harvester":sim.session().harvester_immune(),"fog":sim.session().game_options().fog_of_war,"grows":sim.session().tiberium_grows_flag(),"spreads":sim.session().tiberium_spreads_flag(),"bridges":sim.bridge_state().map(|b|b.is_destroyable()),"objects":objects,"ore":ore,"fired":sim.global_variables()[0]})
}
fn build(
    doc: &mut MapDocument,
    install: &mut Install,
    folder: &Path,
    source: &str,
) -> Result<(Vec<u8>, bool)> {
    std::fs::create_dir(folder)?;
    let map = folder.join("mission.map");
    std::fs::write(&map, doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(install, map.to_str().context("map path")?)?;
    let catalog = mission_script::catalog(doc, &scene);
    let entry = folder.join("mission.ts");
    std::fs::write(&entry, source)?;
    mission_script::refresh_types(&entry, &catalog)?;
    let worker = Worker::new()?;
    worker
        .build(doc, catalog.clone(), Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(doc)?;
    let unchanged = !worker
        .build(doc, catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(doc)?;
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(&map, &bytes)?;
    Ok((bytes, unchanged))
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
    let mut install = Install::open(&game)?;
    let bytes = NewMapOptions {
        width: 64,
        height: 64,
        ..Default::default()
    }
    .bytes()?;
    let mut base = MapDocument::new(Baseline::parse(
        bytes.clone().into(),
        &map_file::load_from_bytes(bytes)?,
    )?);
    for house in ["Americans", "Russians"] {
        mission::create(&mut base, Kind::House, house, house, None)?;
        base.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    base.add_record("Structures", [28, 52], "Americans", "GAPOWR")?;
    for (kind, owner, cell, asleep) in [
        ("MTNK", "Americans", [40, 70], true),
        ("HARV", "Americans", [40, 60], true),
        ("MTNK", "Russians", [44, 60], false),
    ] {
        let id = base.add_record("Units", cell, owner, kind)?;
        if asleep {
            let record = base.records()[&id].patched(&[(6, "Sleep".into())])?;
            base.set(Target::Record(id), Some(DocumentValue::Record(record)))?;
        }
    }
    for (s, k, v) in [
        ("Basic", "Player", "Americans"),
        ("HTK", "Primary", "SDKBlast"),
        ("SDKBlast", "Damage", "40"),
        ("SDKBlast", "Warhead", "SDKWH"),
        (
            "SDKWH",
            "Verses",
            "100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%",
        ),
        ("SDKWH", "CellSpread", "0.5"),
        ("SDKWH", "PercentAtMax", "1"),
        ("SDKWH", "AnimList", "EXPLOSML"),
        ("HARV", "Primary", "none"),
        ("HARV", "Secondary", "none"),
    ] {
        base.set_ini(s, k, Some(v.as_bytes().to_vec()))?;
    }
    let ref_runtime = campaign::load(&install, &base.snapshot().bytes()?, 1)?;
    let rules = &ref_runtime.resources.rules;
    let ore = rules
        .overlay_registry()
        .tiberium_placement_overlay_id(&rules.tiberium_types.types()[0], 0)
        .context("ore")?;
    ensure!(
        rules.tiberium_types.types()[0].growth == 2200,
        "audit expects stock Riparius Growth=2200"
    );
    drop(ref_runtime);
    base.set(
        Target::Overlay([50, 60]),
        Some(DocumentValue::Overlay(ore, 3)),
    )?;
    base.set(
        Target::Overlay([53, 60]),
        Some(DocumentValue::Overlay(ore, 11)),
    )?;
    base.commit("flag audit base");
    let mut checks = BTreeMap::<String, bool>::new();
    let mut cases = Vec::new();
    for (name, inert, harvester, grows, spreads) in [
        ("control", false, false, false, false),
        ("inert", true, false, false, false),
        ("harvester", false, true, false, false),
        ("growth", false, false, true, false),
        ("spread", false, false, false, true),
    ] {
        let folder = root.join(name);
        let mut doc = base.fork();
        let source = format!(
            "import {{defineMission,event,action}} from './sdk';import {{map,House,Country,Weapon}} from './map.generated';\nexport default defineMission(map,{{id:'special_{name}',player:House.Americans}},m=>{{\n m.configureBasic({{ignoreGlobalAiTriggers:true,tiberiumGrowthEnabled:true}});\n m.configureHouse(m.house(House.Americans),{{country:m.country(Country.Americans),playerControl:true,iq:0}});\n m.configureHouse(m.house(House.Russians),{{country:m.country(Country.Russians),iq:0}});\n m.configureSpecialFlags({{inert:{inert},harvesterImmune:{harvester},fogOfWar:{inert},destroyableBridges:{},tiberiumGrows:{grows},tiberiumSpreads:{spreads}}});\n const at=m.createWaypoint(30,40,70);m.rule('blast',{{when:event.sinceScenarioStart(1),once:true,actions:[action.explosionAt(m.weapon(Weapon.SDKBlast),at),action.setGlobal(m.globalFlag(0))]}});\n}});",
            !inert
        );
        let (bytes, unchanged) = build(&mut doc, &mut install, &folder, &source)?;
        checks.insert(format!("{name}_unchanged_rebuild"), unchanged);
        let mut finals = Vec::new();
        for restore in [false, true] {
            let tag = format!("{name}_{}", if restore { "restored" } else { "continuous" });
            let mut runtime = campaign::load(&install, &bytes, 1)?;
            let initial = campaign_state(&runtime);
            let mut observations = vec![initial.clone()];
            let mut preserved = true;
            let mut acquired = false;
            let mut growth_timer = Value::Null;
            let victim = initial["objects"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["cell"] == json!([40, 70]))
                .context("victim")?["id"]
                .as_u64()
                .unwrap();
            let miner = initial["objects"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["type"] == "HARV")
                .context("miner")?["id"]
                .as_u64()
                .unwrap();
            checks.insert(
                format!("{tag}_startup_flags"),
                initial["inert"] == inert
                    && initial["harvester"] == harvester
                    && initial["fog"] == inert
                    && initial["grows"] == grows
                    && initial["spreads"] == spreads
                    && initial["bridges"] == !inert,
            );
            let original_health = runtime
                .simulation
                .entities()
                .get(victim)
                .unwrap()
                .health
                .current();
            for tick in 1..=240 {
                runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
                if tick == 1 {
                    growth_timer = saved_field::read(
                        &runtime.simulation,
                        &["ore_growth_state", "native_tiberium"],
                    )?["classes"][0]["growth_timer"]
                        .clone();
                }
                for (_, e) in runtime.simulation.entities().iter_sorted() {
                    if runtime.simulation.interner().resolve(e.owner()) == "Russians" {
                        acquired |= e.attack_target.target()
                            == Some(vera20k_core::sim::combat::TargetKind::Entity(miner));
                    }
                }
                if restore && tick == 10 {
                    let before = campaign_state(&runtime);
                    checkpoint::restore_checkpoint(
                        &mut runtime,
                        &bytes,
                        &folder.join("checkpoint.bin"),
                    )?;
                    preserved = before == campaign_state(&runtime);
                }
                observations.push(campaign_state(&runtime));
            }
            let final_state = campaign_state(&runtime);
            let final_health = runtime
                .simulation
                .entities()
                .get(victim)
                .context("victim survived")?
                .health
                .current();
            checks.insert(
                format!("{tag}_authored_blast_ran"),
                final_state["fired"] == true,
            );
            checks.insert(
                format!("{tag}_damage_policy"),
                final_health == original_health - if inert { 0 } else { 40 },
            );
            checks.insert(
                format!("{tag}_harvester_acquisition_policy"),
                acquired != harvester,
            );
            let density = |state: &Value| {
                state["ore"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|c| c["cell"] == json!([50, 60]))
                    .unwrap()["data"]
                    .as_u64()
                    .unwrap()
            };
            let count = |state: &Value| {
                state["ore"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|c| c["id"].is_number())
                    .count()
            };
            checks.insert(
                format!("{tag}_ore_density_grows_at_both_speeds"),
                density(&final_state) > density(&initial),
            );
            checks.insert(
                format!("{tag}_growth_timer_speed"),
                growth_timer["duration"] == if grows { 659 } else { 2200 },
            );
            checks.insert(
                format!("{tag}_ore_spread_policy"),
                if spreads {
                    count(&final_state) > count(&initial)
                } else {
                    count(&final_state) == count(&initial)
                },
            );
            checks.insert(format!("{tag}_checkpoint_observed_state"), preserved);
            std::fs::write(
                folder.join(format!("observations-{restore}.json")),
                serde_json::to_vec_pretty(&observations)?,
            )?;
            cases.push(
                json!({"name":tag,"initial":initial,"final":final_state,"miner_acquired":acquired,"first_growth_timer":growth_timer}),
            );
            finals.push(final_state);
        }
        // Snapshot loading resets Scenario RNG; only flags and victim health
        // are compared across runs, not random ore placement or whole state.
        checks.insert(
            format!("{name}_restored_flags"),
            ["inert", "harvester", "fog", "grows", "spreads", "bridges"]
                .iter()
                .all(|k| finals[0][*k] == finals[1][*k]),
        );
    }
    for veteran in [false, true] {
        let folder = root.join(format!("multiplayer_{veteran}"));
        let mut doc = base.fork();
        doc.waypoint(0, [35, 45])?;
        for (id, record) in doc.records() {
            if record.section == "Units" {
                doc.set(
                    Target::Record(id),
                    Some(DocumentValue::Record(
                        record.patched(&[(0, "Neutral".into())])?,
                    )),
                )?;
            }
        }
        doc.commit("starting-force waypoint");
        let source = format!(
            "import {{defineMission,MissionMode}} from './sdk';import {{map}} from './map.generated';export default defineMission(map,{{id:'initial_veteran',mode:MissionMode.Multiplayer}},m=>{{m.configureBasic({{ignoreGlobalAiTriggers:true}});m.configureSpecialFlags({{initialVeteran:{veteran}}});}});"
        );
        let (_, unchanged) = build(&mut doc, &mut install, &folder, &source)?;
        checks.insert(
            format!("multiplayer_{veteran}_unchanged_rebuild"),
            unchanged,
        );
        for mode in [
            ScenarioGameMode::Skirmish,
            ScenarioGameMode::Lan,
            ScenarioGameMode::Wol,
        ] {
            let map_path = folder.join("mission.map");
            let filename = map_path.to_str().context("multiplayer path")?;
            let mut launch = battle_session(filename, &[]);
            launch.humans[0].slot.start_position = LaunchStartPosition::Position(0);
            launch.options.unit_count = 4;
            launch.options.short_game = false;
            launch.options.crates = false;
            let launch = MatchLaunchDescriptor::from_resolved(launch)
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            let scenario = load_with_launch_and_ini(
                &game,
                filename,
                12345,
                launch,
                &Default::default(),
                mode,
                None,
                None,
            )
            .map_err(anyhow::Error::msg)?;
            let sim = scenario.sim();
            let player = sim.session().start_slot_houses()[&0];
            let objects=sim.entities().iter_sorted().map(|(id,e)|json!({"id":id,"type":sim.interner().resolve(e.type_ref()),"owner":sim.interner().resolve(e.owner()),"rank":e.veterancy.rank(),"active":e.is_active()})).collect::<Vec<_>>();
            let starting = sim
                .entities()
                .iter_sorted()
                .filter(|(_, e)| {
                    e.owner() == player && sim.interner().resolve(e.type_ref()) != "AMCV"
                })
                .collect::<Vec<_>>();
            let tag = format!("{}_veteran_{veteran}", mode.as_str());
            checks.insert(
                format!("{tag}_actual_mode"),
                sim.session().game_mode() == mode,
            );
            checks.insert(
                format!("{tag}_nonempty_starting_force"),
                !starting.is_empty(),
            );
            checks.insert(
                format!("{tag}_starting_rank"),
                starting
                    .iter()
                    .all(|(_, e)| e.veterancy.rank() == if veteran { 200 } else { 0 }),
            );
            checks.insert(
                format!("{tag}_mcv_rookie"),
                sim.entities()
                    .iter_sorted()
                    .filter(|(_, e)| {
                        e.owner() == player && sim.interner().resolve(e.type_ref()) == "AMCV"
                    })
                    .map(|(_, e)| e.veterancy.rank())
                    .collect::<Vec<_>>()
                    == [0],
            );
            checks.insert(
                format!("{tag}_map_objects_unchanged_rank"),
                sim.entities()
                    .iter_sorted()
                    .filter(|(_, e)| sim.interner().resolve(e.owner()) == "Neutral")
                    .map(|(_, e)| e.veterancy.rank())
                    .collect::<Vec<_>>()
                    == [0, 0, 0],
            );
            cases.push(json!({"name":tag,"objects":objects,"mode":sim.session().game_mode()}));
        }
    }
    let passed = checks.values().all(|v| *v);
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":passed,"checks":checks,"cases":cases,"scope":"TS build and stable rebuild, campaign startup flags/ordinary damage, harvester automatic acquisition and ore gates; public checkpoint observed-state restoration; actual Skirmish/LAN/WOL starting-force ranks. No GPU, network transport, whole-state replay or original SAV compatibility claim."}),
        )?,
    )?;
    ensure!(passed, "special flags audit failed; see report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
