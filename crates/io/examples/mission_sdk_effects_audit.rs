//! TS weapon detonation, animation and particle lifecycle through retail loading.
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
        runtime::SimRuntime,
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;

fn particle_state(runtime: &SimRuntime, full: bool) -> Result<Value> {
    runtime.simulation.particle_systems().iter()
        .filter(|(_, system)| runtime.resources.rules.particle_system_type(system.type_id()).name == "SDKSmokeSys")
        .map(|(id, system)| Ok(if full { json!({"id":id,"system":serde_json::to_value(system)?}) }
        else { let coord=system.coords(); json!({"id":id,"coords":[coord.x,coord.y,coord.z],
            "alive":system.alive(),"done":system.done_spawning(),"particles":system.particles().len()}) }))
        .collect::<Result<Vec<_>>>().map(Value::Array)
}

fn audit(install: &Install, bytes: &[u8], root: &Path, name: &str, restore: bool) -> Result<Value> {
    let mut runtime = campaign::load(install, bytes, 1)?;
    let target = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find(|(_, e)| (e.position.rx(), e.position.ry()) == (40, 60))
        .context("target")?
        .0;
    let witness = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find(|(_, e)| (e.position.rx(), e.position.ry()) == (82, 82))
        .context("remote witness")?
        .0;
    let initial_health = runtime
        .simulation
        .entities()
        .get(target)
        .unwrap()
        .health
        .current();
    let witness_health = runtime
        .simulation
        .entities()
        .get(witness)
        .unwrap()
        .health
        .current();
    let mut observations = Vec::new();
    let mut first_action = None;
    let mut local_stop = None;
    let mut remote_stop = None;
    let mut checkpoint_preserved = None;
    let mut damage_tick = None;
    let mut retired_tick = None;
    let mut saw_animation = false;
    let mut saw_particles = false;
    let mut no_projectile = true;
    let mut witness_unchanged = true;
    for _ in 0..200 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let sim = &runtime.simulation;
        let tick = sim.session().tick();
        let health = sim
            .entities()
            .get(target)
            .context("target survived")?
            .health
            .current();
        witness_unchanged &= sim.entities().get(witness).is_some_and(|e| {
            e.health.current() == witness_health && (e.position.rx(), e.position.ry()) == (82, 82)
        });
        no_projectile &= sim.projectiles().is_empty();
        if health < initial_health {
            damage_tick.get_or_insert(tick);
        }
        let animations=sim.anims().map(|(id,anim)|{
            let coord=anim.world_coord();
            json!({"id":id,"type":sim.interner().resolve(anim.type_id()),"coords":[coord.x,coord.y,coord.z]})
        }).collect::<Vec<_>>();
        saw_animation |= animations.iter().any(|anim| {
            anim["type"] == "EXPLOSML"
                && anim["coords"][0] == 40 * 256 + 128
                && anim["coords"][1] == 60 * 256 + 128
        });
        let particles = particle_state(&runtime, false)?;
        if sim.global_variables()[2] && particles == json!([]) {
            retired_tick.get_or_insert(tick);
        }
        saw_particles |= particles
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["particles"].as_u64().unwrap() > 0);
        let observation =
            json!({"tick":tick,"health":health,"animations":animations,"systems":particles});
        if sim.global_variables()[0] && first_action.is_none() {
            first_action = Some(observation.clone());
        }
        if sim.global_variables()[1] && local_stop.is_none() {
            local_stop = Some(observation.clone());
        }
        if sim.global_variables()[2] && remote_stop.is_none() {
            remote_stop = Some(observation.clone());
        }
        observations.push(observation);
        if restore && tick == 35 {
            let before = particle_state(&runtime, true)?;
            checkpoint::restore_checkpoint(&mut runtime, bytes, &root.join("draining.bin"))?;
            let after = particle_state(&runtime, true)?;
            checkpoint_preserved = Some(before == after);
            std::fs::write(
                root.join("checkpoint-state.json"),
                serde_json::to_vec_pretty(&json!({"before":before,"after":after}))?,
            )?;
        }
    }
    let final_health = runtime
        .simulation
        .entities()
        .get(target)
        .unwrap()
        .health
        .current();
    let mut checks = BTreeMap::from([
        ("authored_action_ran", first_action.is_some()),
        ("remote_witness_unchanged", witness_unchanged),
        ("no_live_flying_projectiles_observed", no_projectile),
    ]);
    if name == "particles" {
        let systems = |value: &Option<Value>| {
            value
                .as_ref()
                .and_then(|v| v["systems"].as_array())
                .cloned()
                .unwrap_or_default()
        };
        let local = systems(&local_stop);
        let remote = systems(&remote_stop);
        let at = |system: &Value, x: i64, y: i64| {
            system["coords"][0] == x * 256 + 128 && system["coords"][1] == y * 256 + 128
        };
        checks.insert(
            "two_systems_created_at_authored_cells",
            systems(&first_action).len() == 2
                && systems(&first_action).iter().any(|s| at(s, 40, 60))
                && systems(&first_action).iter().any(|s| at(s, 82, 82)),
        );
        checks.insert("real_particles_spawned", saw_particles);
        checks.insert(
            "local_stop_marks_live_system_and_retains_particles",
            local.iter().any(|s| {
                at(s, 40, 60)
                    && s["done"] == true
                    && s["alive"] == true
                    && s["particles"].as_u64().unwrap() > 0
            }),
        );
        checks.insert(
            "local_stop_preserves_remote_emission",
            local.iter().any(|s| at(s, 82, 82) && s["done"] == false),
        );
        checks.insert(
            "remote_stop_marks_remote_system",
            remote.iter().any(|s| at(s, 82, 82) && s["done"] == true),
        );
        checks.insert(
            "both_systems_retire_after_particles_drain",
            particle_state(&runtime, false)? == json!([]),
        );
        checks.insert(
            "particles_do_not_damage_target",
            final_health == initial_health,
        );
        if restore {
            checks.insert(
                "checkpoint_preserves_systems_particles_and_stop_flags",
                checkpoint_preserved == Some(true),
            );
        }
    } else {
        checks.insert("selected_animation_constructed_at_waypoint", saw_animation);
        checks.insert(
            "selected_animation_retires",
            !runtime.simulation.anims().any(|(_, anim)| {
                runtime.simulation.interner().resolve(anim.type_id()) == "EXPLOSML"
            }),
        );
        let expected = match name {
            "weak" => 40,
            "strong" => 80,
            _ => 0,
        };
        checks.insert(
            "selected_weapon_damage_applied",
            initial_health - final_health == expected,
        );
        checks.insert(
            "damage_synchronous_with_action",
            if expected == 0 {
                damage_tick.is_none()
            } else {
                damage_tick == first_action.as_ref().and_then(|v| v["tick"].as_u64())
            },
        );
    }
    std::fs::write(
        root.join("observations.json"),
        serde_json::to_vec_pretty(&observations)?,
    )?;
    Ok(
        json!({"passed":checks.values().all(|v|*v),"checks":checks,"initial_health":initial_health,
        "final_health":final_health,"damage_tick":damage_tick,"first_action":first_action,
        "local_stop":local_stop,"remote_stop":remote_stop,"retired_tick":retired_tick,"restored":restore}),
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
    doc.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    doc.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    doc.add_record("Structures", [28, 52], "Americans", "GAPOWR")?;
    for cell in [[40, 60], [82, 82]] {
        doc.add_record("Units", cell, "Americans", "MTNK")?;
    }
    // Map Rules are read by both production scene projection and runtime owner.
    // New weapon IDs are implicitly allocated by the HTK reader, not a sorted list.
    for (section, key, value) in [
        ("HTK", "Primary", "SDKZWeak"),
        ("HTK", "Secondary", "SDKAStrong"),
        ("SDKZWeak", "Damage", "40"),
        ("SDKAStrong", "Damage", "80"),
        ("SDKZWeak", "Warhead", "SDKBlastWH"),
        ("SDKAStrong", "Warhead", "SDKBlastWH"),
        (
            "SDKBlastWH",
            "Verses",
            "100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%",
        ),
        ("SDKBlastWH", "CellSpread", "0.5"),
        ("SDKBlastWH", "PercentAtMax", "1"),
        ("SDKBlastWH", "AnimList", "EXPLOSML"),
        ("EXPLOSML", "Damage", "0"),
        ("ParticleSystems", "999", "SDKSmokeSys"),
        ("Particles", "999", "SDKSmokeParticle"),
        ("SDKSmokeSys", "BehavesLike", "Smoke"),
        ("SDKSmokeSys", "HoldsWhat", "SDKSmokeParticle"),
        ("SDKSmokeSys", "SpawnFrames", "1"),
        ("SDKSmokeSys", "SpawnCutoff", "10000"),
        ("SDKSmokeSys", "Slowdown", "0"),
        ("SDKSmokeSys", "Lifetime", "-1"),
        ("SDKSmokeParticle", "BehavesLike", "Smoke"),
        ("SDKSmokeParticle", "MaxEC", "30"),
        ("SDKSmokeParticle", "Velocity", "1"),
        ("SDKSmokeParticle", "Image", "SGRYSMK"),
    ] {
        doc.set_ini(section, key, Some(value.as_bytes().to_vec()))?;
    }
    doc.commit("effects fixture");
    let base = doc.snapshot().bytes()?;
    std::fs::write(root.join("base.map"), &base)?;
    let mut install = Install::open(&game)?;
    let scene = ra2_map_scene::map::load_scene(
        &mut install,
        root.join("base.map").to_str().context("map path")?,
    )?;
    let mut cases = Vec::new();
    for (name, actions) in [
        (
            "visual",
            "action.playAnimationAt(m.animation(Animation.EXPLOSML),at)",
        ),
        ("weak", "action.explosionAt(m.weapon(Weapon.SDKZWeak),at)"),
        (
            "strong",
            "action.explosionAt(m.weapon(Weapon.SDKAStrong),at)",
        ),
        (
            "particles",
            "action.createParticleSystem(m.particleSystem(ParticleSystem.SDKSmokeSys),at),action.createParticleSystem(m.particleSystem(ParticleSystem.SDKSmokeSys),remote)",
        ),
    ] {
        let folder = root.join(name);
        std::fs::create_dir(&folder)?;
        let mut authored = MapDocument::new(Baseline::parse(
            base.clone().into(),
            &map_file::load_from_bytes(base.clone())?,
        )?);
        let source=r#"import {defineMission,event,action} from './sdk';
import {map,House,Country,Weapon,Animation,ParticleSystem} from './map.generated';
export default defineMission(map,{id:'effects',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),iq:0,playerControl:true});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),iq:0});
 const at=m.createWaypoint(30,40,60),remote=m.createWaypoint(31,82,82);
 m.rule('start',{when:event.sinceScenarioStart(1),once:true,actions:[__ACTIONS__,action.setGlobal(m.globalFlag(0))]});
 __STOP__
});"#.replace("__ACTIONS__",actions).replace("__STOP__",if name=="particles" {r#"
 m.rule('stop_local',{when:event.sinceScenarioStart(2),once:true,actions:[action.markParticlesDone(at),action.setGlobal(m.globalFlag(1))]});
 m.rule('stop_remote',{when:event.sinceScenarioStart(4),once:true,actions:[action.markParticlesDone(remote),action.setGlobal(m.globalFlag(2))]});
"#}else{""});
        let entry = folder.join("mission.ts");
        std::fs::write(&entry, source)?;
        let catalog = mission_script::catalog(&authored, &scene);
        let order = &catalog.registry_order["weapons"];
        ensure!(
            order.iter().position(|id| id == "SDKZWeak")
                < order.iter().position(|id| id == "SDKAStrong"),
            "fixture requires allocation order distinct from name order"
        );
        mission_script::refresh_types(&entry, &catalog)?;
        Worker::new()?
            .build(&authored, catalog, Sources::read(&entry)?)?
            .recv()?
            .map_err(anyhow::Error::msg)?
            .build
            .apply(&mut authored)?;
        let bytes = authored.snapshot().bytes()?;
        std::fs::write(folder.join("effects.map"), &bytes)?;
        cases.push(json!({"case":name,"result":audit(&install,&bytes,&folder,name,false)?}));
        if name == "particles" {
            let restored = folder.join("restored");
            std::fs::create_dir(&restored)?;
            cases.push(json!({"case":"particles_restored","result":audit(&install,&bytes,&restored,name,true)?}));
        }
    }
    let passed = cases.iter().all(|case| case["result"]["passed"] == true);
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&json!({"passed":passed,"cases":cases,
        "scope":"TS build with retail resources and map Rules overrides, production campaign initialization and ordinary frames; particles restored through public headless checkpoint. Simulation state only, not GUI/pixel/audio or whole-state equivalence.",
        "restore_timing":"Existing deserialize_scenario_rng_reset restarts the random stream used by particle_rng. Immediate system/particle state is preserved; subsequent smoke retirement ticks may differ from uninterrupted execution and are reported separately."}))?,
    )?;
    ensure!(passed, "effects audit failed; see report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
