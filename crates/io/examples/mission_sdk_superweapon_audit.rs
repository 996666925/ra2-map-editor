//! TS superweapon state, two-click Chronosphere/save and weather lifecycles.
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
#[path = "support/checkpoint.rs"]
mod checkpoint;

fn advance(runtime: &mut SimRuntime) -> Result<()> {
    runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
    Ok(())
}

fn fire(runtime: &mut SimRuntime, name: &str, cell: [u16; 2]) -> Result<bool> {
    let sim = &runtime.simulation;
    let owner = sim.interner().get("Americans").context("player")?;
    let weapon = sim.interner().get(name).context("superweapon")?;
    let command = sim
        .encode_superweapon_record(
            owner,
            &Command::LaunchSuperWeapon {
                sw_type_id: weapon,
                target_rx: cell[0],
                target_ry: cell[1],
            },
            &runtime.resources.rules,
        )
        .context("ordinary superweapon command admission")?;
    let frame = sim.session().tick();
    let result = runtime.advance_frame(
        &[CommandEnvelope::synchronized(command, frame)],
        22,
        TickLane::Ordinary,
        FrameEffects::default(),
    )?;
    Ok(result.sound_events.iter().any(|event| {
        matches!(event,
        vera20k_core::sim::world::SimSoundEvent::SuperWeaponLaunched { owner: by, sw_type, .. }
            if *by == owner && *sw_type == weapon)
    }))
}

fn state(runtime: &SimRuntime, weapon: &str) -> Result<Value> {
    let sim = &runtime.simulation;
    let player = sim.interner().get("Americans").context("player")?;
    let weapon = sim.interner().get(weapon).context("weapon")?;
    Ok(serde_json::to_value(
        &sim.super_weapons()[&player][&weapon],
    )?)
}

fn audit_state(install: &Install, bytes: &[u8], root: &std::path::Path) -> Result<Value> {
    let mut runtime = campaign::load(install, bytes, 0)?;
    let player = runtime
        .simulation
        .interner()
        .get("Americans")
        .context("player")?;
    let mut phases = BTreeMap::new();
    let mut ready_tick = None;
    for _ in 0..120 {
        let action_frame = runtime.simulation.session().binary_frame();
        advance(&mut runtime)?;
        let sim = &runtime.simulation;
        for phase in 0..6 {
            if sim.global_variables()[phase] && !phases.contains_key(&phase) {
                let house = serde_json::to_value(&sim.houses()[&player])?;
                phases.insert(
                    phase,
                    json!({"tick":sim.session().tick(),"action_frame":action_frame,
                    "super":state(&runtime,"IronCurtainSpecial")?,
                    "targets":house["super_weapon_targets"]}),
                );
            }
        }
        if sim.global_variables()[3]
            && !sim.global_variables()[4]
            && state(&runtime, "IronCurtainSpecial")?["is_ready"] == true
        {
            ready_tick.get_or_insert(sim.session().tick());
        }
    }
    let phase = |n| &phases[&n];
    let weapon = |n| &phase(n)["super"];
    let default_recharge = runtime
        .resources
        .rules
        .super_weapon("IronCurtainSpecial")
        .context("curtain type")?
        .recharge_time_frames;
    let checks = json!({
        "all_six_authored_phases_ran":phases.len()==6,
        "repeat_grant_is_active_and_cannot_be_lost":weapon(0)["is_active"]==true
            &&weapon(0)["one_time"]==false&&weapon(0)["can_lose"]==false,
        "override_does_not_restart_existing_timer":weapon(0)["custom_recharge_time"]==240
            &&weapon(0)["charge_duration"]==default_recharge,
        "quarter_charge_uses_override":weapon(1)["custom_recharge_time"]==240
            &&weapon(1)["charge_duration"]==181&&weapon(1)["is_ready"]==false,
        "reset_override_preserves_current_timer":weapon(2)["custom_recharge_time"]== -1
            &&weapon(2)["charge_start_tick"]==weapon(1)["charge_start_tick"]
            &&weapon(2)["charge_duration"]==weapon(1)["charge_duration"],
        "full_percentage_keeps_one_native_rounding_frame":weapon(3)["is_ready"]==false
            &&weapon(3)["charge_duration"]==1,
        "full_percentage_becomes_ready_on_next_super_ai":ready_tick.is_some_and(|tick|
            Some(tick)==phase(3)["tick"].as_u64().map(|tick|tick+1)),
        "reset_weapon_keeps_grant_and_restarts_default_recharge":weapon(4)["is_active"]==true
            &&weapon(4)["is_ready"]==false&&weapon(4)["charge_duration"]==default_recharge
            &&weapon(4)["charge_start_tick"]==phase(4)["action_frame"],
        "preferred_target_is_authored_cell":phase(0)["targets"]["preferred_target"]==json!([66,66]),
        "defense_target_and_frame_are_authored":phase(0)["targets"]["defense_cell"]==json!([40,60])
            &&phase(0)["targets"]["defense_frame"]==phase(0)["action_frame"],
        "clear_targets_restores_native_sentinels":phase(5)["targets"]["preferred_target"]==json!([0,0])
            &&phase(5)["targets"]["defense_cell"]==json!([0,0])
            &&phase(5)["targets"]["defense_frame"]== -100,
    });
    let passed = checks.as_object().unwrap().values().all(|v| v == true);
    let report = json!({"passed":passed,"checks":checks,"phases":phases,"default_recharge":default_recharge,
        "full_percentage_ready_tick":ready_tick,
        "rounding":"Existing native binary32 0.01 and integer truncation leave 181/240 frames at 25 percent and one frame at 100 percent; readiness is observed on the next Super AI, not forced."});
    std::fs::write(
        root.join("state-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

fn audit_chrono(
    install: &Install,
    bytes: &[u8],
    root: &std::path::Path,
    restore: bool,
) -> Result<Value> {
    let mut runtime = campaign::load(install, bytes, 0)?;
    let sim = &runtime.simulation;
    let mut actors = BTreeMap::new();
    for (id, entity) in sim.entities().iter_sorted() {
        if matches!(
            (entity.position.rx(), entity.position.ry()),
            (40, 60) | (41, 60) | (40, 61) | (82, 82)
        ) {
            actors.insert(sim.interner().resolve(entity.type_ref()).to_string(), id);
        }
    }
    ensure!(actors.len() == 4, "all four actors have distinct types");
    while runtime.simulation.session().tick() < 30 {
        advance(&mut runtime)?;
    }
    let initial = actors.iter().map(|(name,id)| {
        let e=runtime.simulation.entities().get(*id).unwrap();
        (name.clone(),json!({"owner":e.owner(),"health":e.health.current(),"cell":[e.position.rx(),e.position.ry()],
            "locomotor_kind":e.locomotor.as_ref().map(|l|l.active_kind())}))
    }).collect::<BTreeMap<_,_>>();
    let source_launched = fire(&mut runtime, "ChronoSphereSpecial", [40, 60])?;
    let source = state(&runtime, "ChronoSphereSpecial")?;
    let placement_before = runtime.simulation.super_placement_anims();
    let mut checkpoint_preserved = true;
    if restore {
        checkpoint::restore_checkpoint(&mut runtime, bytes, &root.join("between-clicks.bin"))?;
        checkpoint_preserved = source == state(&runtime, "ChronoSphereSpecial")?
            && placement_before == runtime.simulation.super_placement_anims();
    }
    while runtime.simulation.session().tick() < 40 {
        advance(&mut runtime)?;
    }
    let destination_launched = fire(&mut runtime, "ChronoWarpSpecial", [66, 66])?;
    let destination = state(&runtime, "ChronoSphereSpecial")?;
    let placements_released = runtime.simulation.super_placement_anims().is_empty();
    let gi = actors["E1"];
    let organic_killed = runtime
        .simulation
        .entities()
        .get(gi)
        .is_none_or(|e| e.health.current() <= 0);
    let mut warped = false;
    let mut landed = false;
    let mut completed = false;
    let mut locomotors_restored = false;
    let mut witness_untouched = true;
    let mut survivors_preserved = true;
    let mut observations = Vec::new();
    for _ in 0..160 {
        let sim = &runtime.simulation;
        let mut rows = Vec::new();
        let mut both_landed = true;
        let mut both_done = true;
        let mut both_warped = true;
        let mut both_original_locomotors = true;
        for kind in ["MTNK", "CLEG"] {
            let entity = sim
                .entities()
                .get(actors[kind])
                .context("warped survivor")?;
            let target = if kind == "MTNK" { [66, 66] } else { [66, 67] };
            // The saved locomotor is already serializable; observe its active
            // Teleport payload without widening engine-private debug APIs.
            let movement = serde_json::to_value(&entity.locomotor)?;
            let chrono = &movement["runtime_payload"]["Teleport"]["chrono"];
            both_original_locomotors &= movement["kind"] == initial[kind]["locomotor_kind"]
                && movement["piggyback"].is_null();
            both_landed &= [entity.position.rx(), entity.position.ry()] == target;
            both_done &= chrono.is_null() && !entity.is_warping_in() && !entity.is_warped_out();
            both_warped &= chrono["latched"] == true;
            survivors_preserved &= json!(entity.owner()) == initial[kind]["owner"]
                && json!(entity.health.current()) == initial[kind]["health"];
            rows.push(json!({"kind":kind,"id":actors[kind],"cell":[entity.position.rx(),entity.position.ry()],
                "warp_latch":chrono["latched"],"warping_in":entity.is_warping_in(),
                "warped_out":entity.is_warped_out(),"locomotor":movement}));
        }
        warped |= both_warped;
        landed |= both_landed;
        completed |= both_landed && both_done;
        locomotors_restored |= both_landed && both_done && both_original_locomotors;
        let witness = sim.entities().get(actors["HTK"]).context("witness")?;
        witness_untouched &= json!(witness.owner()) == initial["HTK"]["owner"]
            && json!(witness.health.current()) == initial["HTK"]["health"]
            && json!([witness.position.rx(), witness.position.ry()]) == initial["HTK"]["cell"];
        observations.push(json!({"tick":sim.session().tick(),"actors":rows}));
        advance(&mut runtime)?;
    }
    let checks = json!({"source_launch":source_launched,"source_cell_stored":source["chrono_cell"]==json!([40,60]),
        "source_placement_animation_created":placement_before.len()==1,
        "checkpoint_preserves_source_and_placement":checkpoint_preserved,
        "destination_launch":destination_launched,"placement_released_after_destination":placements_released,
        "paired_source_restarts_recharge":destination["is_active"]==true&&destination["is_ready"]==false
            &&destination["charge_duration"]==runtime.resources.rules.super_weapon("ChronoSphereSpecial").unwrap().recharge_time_frames,
        "ordinary_organic_member_killed":organic_killed,"both_survivors_warped":warped,
        "both_survivors_land_at_authored_offsets":landed,"both_survivors_finish_warp":completed,
        "both_original_locomotors_restored":locomotors_restored,
        "survivor_identity_health_and_owner_preserved":survivors_preserved,
        "unrelated_transport_untouched":witness_untouched});
    let passed = checks.as_object().unwrap().values().all(|v| v == true);
    std::fs::write(
        root.join("observations.json"),
        serde_json::to_vec_pretty(&observations)?,
    )?;
    Ok(
        json!({"passed":passed,"checks":checks,"source":source,"destination":destination,
        "initial":initial,"restored_between_clicks":restore}),
    )
}

fn audit_one_shot(install: &Install, bytes: &[u8]) -> Result<Value> {
    let mut runtime = campaign::load(install, bytes, 0)?;
    let tank = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find_map(|(id, e)| {
            (runtime.simulation.interner().resolve(e.type_ref()) == "MTNK").then_some(id)
        })
        .context("tank")?;
    let witness = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find_map(|(id, e)| {
            (runtime.simulation.interner().resolve(e.type_ref()) == "HTK").then_some(id)
        })
        .context("witness")?;
    while !runtime.simulation.global_variables()[0] && runtime.simulation.session().tick() < 30 {
        advance(&mut runtime)?;
    }
    let grant_tick = runtime.simulation.session().tick();
    let granted = state(&runtime, "IronCurtainSpecial")?;
    while runtime.simulation.session().tick() < 35 {
        advance(&mut runtime)?;
    }
    let after_repeat_grant = state(&runtime, "IronCurtainSpecial")?;
    let launched = fire(&mut runtime, "IronCurtainSpecial", [40, 60])?;
    let consumed = state(&runtime, "IronCurtainSpecial")?;
    let tank_protected = runtime
        .simulation
        .entities()
        .get(tank)
        .and_then(|e| e.invulnerability.as_ref())
        .is_some_and(|state| {
            state.kind() == vera20k_core::sim::superweapon::invulnerability::InvulnKind::IronCurtain
        });
    let second_launch = fire(&mut runtime, "IronCurtainSpecial", [82, 82])?;
    let witness_unprotected = runtime
        .simulation
        .entities()
        .get(witness)
        .is_some_and(|e| e.invulnerability.is_none());
    while !runtime.simulation.global_variables()[1] && runtime.simulation.session().tick() < 75 {
        advance(&mut runtime)?;
    }
    let regrant_tick = runtime.simulation.session().tick();
    let regranted = state(&runtime, "IronCurtainSpecial")?;
    let regranted_launch = fire(&mut runtime, "IronCurtainSpecial", [82, 82])?;
    let witness_protected = runtime
        .simulation
        .entities()
        .get(witness)
        .and_then(|e| e.invulnerability.as_ref())
        .is_some_and(|state| {
            state.kind() == vera20k_core::sim::superweapon::invulnerability::InvulnKind::IronCurtain
        });
    let checks = json!({
        "one_shot_grant_is_immediately_ready":grant_tick==16&&granted["is_active"]==true&&granted["one_time"]==true&&granted["is_ready"]==true,
        "repeat_grant_of_active_one_shot_keeps_one_time_flag":after_repeat_grant["is_active"]==true
            &&after_repeat_grant["one_time"]==true&&after_repeat_grant["can_lose"]==false,
        "first_launch":launched,"tank_received_iron_curtain":tank_protected,
        "one_shot_consumed":consumed["is_active"]==false&&consumed["one_time"]==false&&consumed["is_ready"]==false,
        "second_command_cannot_launch_or_protect_remote_witness":!second_launch&&witness_unprotected,
        "later_ts_regrant_reuses_retained_instance":regrant_tick==61&&regranted["native_unique_id"]==granted["native_unique_id"]
            &&regranted["is_active"]==true&&regranted["one_time"]==true&&regranted["is_ready"]==true,
        "regranted_weapon_launches_and_protects_witness":regranted_launch&&witness_protected,
    });
    Ok(
        json!({"passed":checks.as_object().unwrap().values().all(|v|v==true),
        "checks":checks,"grant_tick":grant_tick,"regrant_tick":regrant_tick,
        "granted":granted,"after_repeat_grant":after_repeat_grant,
        "consumed":consumed,"regranted":regranted}),
    )
}

fn audit_weather(
    install: &Install,
    bytes: &[u8],
    root: &std::path::Path,
    mode: &str,
) -> Result<Value> {
    let mut runtime = campaign::load(install, bytes, 0)?;
    let sim = &runtime.simulation;
    let player = sim.interner().get("Americans").context("player")?;
    let (target, target_health) = sim
        .entities()
        .iter_sorted()
        .find_map(|(id, e)| {
            (sim.interner().resolve(e.owner()) == "Russians"
                && sim.interner().resolve(e.type_ref()) == "NAPOWR")
                .then_some((id, e.health.current()))
        })
        .context("weather target")?;
    let deferment = runtime.resources.rules.general.lightning_deferment;
    let duration = runtime.resources.rules.general.lightning_storm_duration;
    ensure!(
        (30..10_000).contains(&deferment) && (1..10_000).contains(&duration),
        "fixture storm timing out of bounds: deferment={deferment}, duration={duration}"
    );
    let mut launched = true;
    let mut consumed = true;
    if mode == "weather_player" {
        while runtime.simulation.session().tick() < 30 {
            advance(&mut runtime)?;
        }
        launched = fire(&mut runtime, "LightningStormSpecial", [78, 76])?;
        let weapon = state(&runtime, "LightningStormSpecial")?;
        consumed = weapon["is_active"] == false && weapon["one_time"] == false;
    }
    let mut first_deferred = None;
    let mut first_active = None;
    let mut first_damage = None;
    let mut ended = None;
    let mut saw_cloud = false;
    let mut saw_bolt = false;
    let mut early_stop = None;
    let mut active_stop = None;
    let mut observations = Vec::new();
    let final_tick = if mode == "weather_cloud" {
        200
    } else {
        duration + deferment + 500
    };
    while runtime.simulation.session().tick() < (final_tick as u64) {
        advance(&mut runtime)?;
        let sim = &runtime.simulation;
        let tick = sim.session().tick();
        let storm = serde_json::to_value(sim.lightning_storm())?;
        if storm["deferment"].as_i64().is_some_and(|n| n > 0) && first_deferred.is_none() {
            first_deferred = Some(json!({"tick":tick,"storm":storm}));
        }
        if storm["active"] == true && first_active.is_none() {
            first_active = Some(json!({"tick":tick,"storm":storm}));
        }
        if first_active.is_some() && storm["active"] == false && ended.is_none() {
            ended = Some(tick);
        }
        if sim
            .entities()
            .get(target)
            .is_none_or(|e| e.health.current() < target_health)
        {
            first_damage.get_or_insert(tick);
        }
        let anims = sim
            .anims()
            .map(|(_, a)| sim.interner().resolve(a.type_id()).to_string())
            .collect::<Vec<_>>();
        saw_cloud |= anims.iter().any(|name| {
            runtime
                .resources
                .rules
                .general
                .weather_con_clouds
                .contains(name)
        });
        saw_bolt |= anims.iter().any(|name| {
            runtime
                .resources
                .rules
                .general
                .weather_con_bolts
                .contains(name)
        });
        if mode == "weather_global" {
            if sim.global_variables()[0] && early_stop.is_none() {
                early_stop = Some(json!({"tick":tick,"storm":storm}));
            }
            if sim.global_variables()[1] && active_stop.is_none() {
                active_stop = Some(json!({"tick":tick,"storm":storm}));
            }
        }
        observations.push(json!({"tick":tick,"storm":storm,"anims":anims,
            "target_health":sim.entities().get(target).map(|e|e.health.current())}));
    }
    let final_storm = serde_json::to_value(runtime.simulation.lightning_storm())?;
    let mut checks = BTreeMap::from([
        ("cloud_animation_created", saw_cloud),
        ("bolt_animation_created", saw_bolt),
        (
            "all_clouds_retire",
            final_storm["clouds_present"] == json!([])
                && final_storm["clouds_manifesting"] == json!([]),
        ),
        ("storm_inactive_at_end", final_storm["active"] == false),
    ]);
    if mode == "weather_cloud" {
        checks.insert(
            "single_cloud_does_not_start_global_storm",
            first_active.is_none() && first_deferred.is_none(),
        );
        checks.insert("single_cloud_really_damages_target", first_damage.is_some());
    } else {
        checks.insert("configured_deferment_observed", first_deferred.is_some());
        checks.insert(
            "storm_becomes_active_and_ends",
            first_active.is_some() && ended.is_some(),
        );
        checks.insert(
            "first_active_frame_follows_deferment",
            first_active
                .as_ref()
                .and_then(|v| v["tick"].as_u64())
                .zip(
                    first_deferred.as_ref().and_then(|v| {
                        Some((v["tick"].as_u64()?, v["storm"]["deferment"].as_u64()?))
                    }),
                )
                .is_some_and(|(active, (first, left))| active == first + left),
        );
        if mode == "weather_global" {
            checks.insert(
                "global_storm_has_no_house",
                first_deferred
                    .as_ref()
                    .is_some_and(|v| v["storm"]["owner"].is_null()),
            );
            checks.insert(
                "global_storm_uses_authored_duration",
                first_deferred
                    .as_ref()
                    .is_some_and(|v| v["storm"]["duration"] == 600),
            );
            checks.insert(
                "stop_during_deferment_does_not_cancel",
                early_stop.as_ref().is_some_and(|v| {
                    v["storm"]["active"] == false
                        && v["storm"]["deferment"].as_i64().is_some_and(|n| n > 0)
                        && v["storm"]["time_to_end"] == false
                }),
            );
            checks.insert(
                "stop_active_requests_end_without_deleting_clouds",
                active_stop.as_ref().is_some_and(|v| {
                    v["storm"]["active"] == true
                        && v["storm"]["time_to_end"] == true
                        && v["storm"]["clouds_present"]
                            .as_array()
                            .is_some_and(|a| !a.is_empty())
                }),
            );
            checks.insert(
                "requested_stop_ends_before_duration",
                ended
                    .zip(first_active.as_ref().and_then(|v| v["tick"].as_u64()))
                    .is_some_and(|(end, start)| end < start + 600),
            );
        } else {
            checks.insert(
                "owned_storm_targets_authored_cell",
                first_deferred.as_ref().is_some_and(|v| {
                    v["storm"]["owner"] == json!(player) && v["storm"]["cell"] == json!([78, 76])
                }),
            );
            checks.insert("owned_storm_damages_enemy_building", first_damage.is_some());
            checks.insert(
                "owned_storm_lasts_configured_duration",
                ended
                    .zip(first_active.as_ref().and_then(|v| v["tick"].as_u64()))
                    .is_some_and(|(end, start)| end > start + duration as u64),
            );
            if mode == "weather_player" {
                checks.insert("ordinary_weather_superweapon_launch", launched);
                checks.insert("weather_one_shot_consumed", consumed);
            }
        }
    }
    std::fs::write(
        root.join("observations.json"),
        serde_json::to_vec_pretty(&observations)?,
    )?;
    Ok(json!({"passed":checks.values().all(|v|*v),"checks":checks,
        "configured_deferment":deferment,"configured_duration":duration,"first_deferred":first_deferred,
        "first_active":first_active,"first_damage_tick":first_damage,"end_tick":ended,
        "early_stop":early_stop,"active_stop":active_stop,"final_storm":final_storm}))
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
    doc.add_record("Structures", [78, 76], "Russians", "NAPOWR")?;
    for (section, kind, at) in [
        ("Units", "MTNK", [40, 60]),
        ("Infantry", "E1", [41, 60]),
        ("Infantry", "CLEG", [40, 61]),
        ("Units", "HTK", [82, 82]),
    ] {
        doc.add_record(section, at, "Americans", kind)?;
    }
    doc.commit("superweapon fixture");
    let base = doc.snapshot().bytes()?;
    std::fs::write(root.join("base.map"), &base)?;
    let mut install = Install::open(&game)?;
    let scene =
        ra2_map_scene::map::load_scene(&mut install, root.join("base.map").to_str().unwrap())?;
    let stop_seconds = {
        let reference = campaign::load(&install, &base, 0)?;
        // Stop the global 600-frame storm about 200 frames into its active
        // phase; use the install's actual deferment, not a default guess.
        let deferment = u32::try_from(reference.resources.rules.general.lightning_deferment)?;
        1 + (deferment + 200).div_ceil(15)
    };
    let mut cases = Vec::new();
    for name in [
        "state",
        "chrono",
        "one_shot",
        "weather_player",
        "weather_owned",
        "weather_global",
        "weather_cloud",
    ] {
        let folder = root.join(name);
        std::fs::create_dir(&folder)?;
        let mut authored = MapDocument::new(Baseline::parse(
            base.clone().into(),
            &map_file::load_from_bytes(base.clone())?,
        )?);
        let source=r#"import {defineMission,event,action} from './sdk';
import {map,House,Country,SuperWeapon} from './map.generated';
export default defineMission(map,{id:'superweapon',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 for(const country of [Country.Americans,Country.Russians]) m.configureHouse(m.house(country===Country.Americans?House.Americans:House.Russians),{country:m.country(country),iq:0,playerControl:country===Country.Americans});
 const target=m.createWaypoint(30,66,66),origin=m.createWaypoint(31,40,60);
 const curtain=m.superWeapon(SuperWeapon.IronCurtainSpecial),sphere=m.superWeapon(SuperWeapon.ChronoSphereSpecial),weather=m.superWeapon(SuperWeapon.LightningStormSpecial);
 const storm=m.createWaypoint(32,78,76);
 __RULES__
});"#.replace("__RULES__",if name=="state"{r#"
 m.rule('grant',{when:event.sinceScenarioStart(1),once:true,actions:[action.grantSuperWeapon(curtain,false),action.setSuperRecharge(curtain,240),action.setSuperTarget(target),action.setSuperDefense(origin),action.setGlobal(m.globalFlag(0))]});
 m.rule('quarter',{when:event.sinceScenarioStart(2),once:true,actions:[action.setSuperCharge(curtain,25),action.setGlobal(m.globalFlag(1))]});
 m.rule('inherit',{when:event.sinceScenarioStart(3),once:true,actions:[action.resetSuperRecharge(curtain),action.setGlobal(m.globalFlag(2))]});
 m.rule('ready',{when:event.sinceScenarioStart(4),once:true,actions:[action.setSuperCharge(curtain,100),action.setGlobal(m.globalFlag(3))]});
 m.rule('reset',{when:event.sinceScenarioStart(5),once:true,actions:[action.resetSuperWeapon(curtain),action.setGlobal(m.globalFlag(4))]});
 m.rule('clear',{when:event.sinceScenarioStart(6),once:true,actions:[action.clearSuperTarget(),action.clearSuperDefense(),action.setGlobal(m.globalFlag(5))]});
"#}else if name=="chrono"{r#"
 m.rule('ready',{when:event.sinceScenarioStart(1),once:true,actions:[action.grantSuperWeapon(sphere,false),action.setSuperCharge(sphere,100)]});
"#}else if name=="one_shot"{r#"
 m.rule('grant',{when:event.sinceScenarioStart(1),once:true,actions:[action.grantSuperWeapon(curtain),action.setGlobal(m.globalFlag(0))]});
 m.rule('repeat_existing',{when:event.sinceScenarioStart(2),once:true,actions:[action.grantSuperWeapon(curtain,false)]});
 m.rule('regrant',{when:event.sinceScenarioStart(4),once:true,actions:[action.grantSuperWeapon(curtain),action.setGlobal(m.globalFlag(1))]});
"#}else if name=="weather_player"{r#"
 m.rule('grant',{when:event.sinceScenarioStart(1),once:true,actions:[action.grantSuperWeapon(weather)]});
"#}else if name=="weather_owned"{r#"
 m.rule('storm',{when:event.sinceScenarioStart(1),once:true,actions:[action.lightningStormAt(storm)]});
"#}else if name=="weather_global"{r#"
 m.rule('storm',{when:event.sinceScenarioStart(1),once:true,actions:[action.startLightningStorm(600)]});
 m.rule('early_stop',{when:event.sinceScenarioStart(2),once:true,actions:[action.stopLightningStorm(),action.setGlobal(m.globalFlag(0))]});
 m.rule('stop',{when:event.sinceScenarioStart(__STOP_SECONDS__),once:true,actions:[action.stopLightningStorm(),action.setGlobal(m.globalFlag(1))]});
"#}else{r#"
 m.rule('cloud',{when:event.sinceScenarioStart(1),once:true,actions:[action.lightningAt(storm)]});
"#}).replace("__STOP_SECONDS__",&stop_seconds.to_string());
        let entry = folder.join("mission.ts");
        std::fs::write(&entry, source)?;
        let catalog = mission_script::catalog(&authored, &scene);
        mission_script::refresh_types(&entry, &catalog)?;
        Worker::new()?
            .build(&authored, catalog, Sources::read(&entry)?)?
            .recv()?
            .map_err(anyhow::Error::msg)?
            .build
            .apply(&mut authored)?;
        let bytes = authored.snapshot().bytes()?;
        std::fs::write(folder.join("superweapon.map"), &bytes)?;
        if name == "state" {
            cases.push(json!({"case":name,"result":audit_state(&install,&bytes,&folder)?}));
        } else if name == "chrono" {
            let continuous = folder.join("continuous");
            let restored = folder.join("restored");
            std::fs::create_dir(&continuous)?;
            std::fs::create_dir(&restored)?;
            cases.push(json!({"case":"chrono_continuous","result":audit_chrono(&install,&bytes,&continuous,false)?}));
            cases.push(json!({"case":"chrono_restored","result":audit_chrono(&install,&bytes,&restored,true)?}));
        } else if name == "one_shot" {
            cases.push(json!({"case":name,"result":audit_one_shot(&install,&bytes)?}));
        } else {
            cases.push(json!({"case":name,"result":audit_weather(&install,&bytes,&folder,name)?}));
        }
    }
    let passed = cases.iter().all(|case| case["result"]["passed"] == true);
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&json!({"passed":passed,"cases":cases,
        "scope":"TS build, real campaign resources, ordinary player commands and frames; readonly existing serialization for private state. Between-click checkpoint uses public headless restore, not GUI load. No engine changes; no pixel/audio or whole-state equivalence claim."}))?,
    )?;
    ensure!(passed, "superweapon audit failed; see report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
