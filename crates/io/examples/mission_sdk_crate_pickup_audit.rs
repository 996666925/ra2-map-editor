//! TS crate placement, ordinary movement, synchronous pickup callbacks and rewards.
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
    doc.add_record("Structures", [28, 52], "Americans", "GAPOWR")?;
    doc.add_record("Structures", [82, 82], "Russians", "NAPOWR")?;
    let vehicle = name == "kill_vehicle";
    doc.add_record(
        if vehicle { "Units" } else { "Infantry" },
        [30, 50],
        "Americans",
        if vehicle { "MTNK" } else { "ENGINEER" },
    )?;
    doc.set_ini("MTNK", "Primary", Some(b"none".to_vec()))?;
    doc.set_ini("MTNK", "Secondary", Some(b"none".to_vec()))?;
    let stock = campaign::load(install, &doc.snapshot().bytes()?, 1)?;
    let image = stock
        .resources
        .rules
        .crate_rules
        .wood_crate_img
        .as_deref()
        .context("land crate image")?;
    let registry = &stock.resources.overlay_registry;
    let id = registry.id_for_name(image).context("land crate overlay")?;
    let flags = registry.flags(id).context("land crate flags")?;
    std::fs::write(
        root.join("stock-prerequisites.json"),
        serde_json::to_vec_pretty(&json!({
            "wood_image":image,"overlay_id":id,"crate_type":flags.crate_type,"crate_trigger":flags.crate_trigger,
            "solo_money":stock.resources.rules.crate_rules.solo_crate_money,
            "wood_selection":stock.resources.rules.crate_rules.wood_crate,
            "fixture_overrides":{"CrateTrigger":name!="no_trigger","SoloCrateMoney":4321,"WoodCrate":"Money"}
        }))?,
    )?;
    doc.set_ini(
        image,
        "CrateTrigger",
        Some(if name == "no_trigger" {
            b"no".to_vec()
        } else {
            b"yes".to_vec()
        }),
    )?;
    doc.set_ini("CrateRules", "SoloCrateMoney", Some(b"4321".to_vec()))?;
    doc.set_ini("CrateRules", "WoodCrate", Some(b"Money".to_vec()))?;
    doc.commit("crate pickup fixture");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(
        install,
        root.join("base.map").to_str().context("map path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let mutation = match name {
        "transfer" => "action.changeAttachedOwner(m.country(Country.Russians)),",
        "kill_infantry" | "kill_vehicle" => "action.destroyAttached(),",
        _ => "",
    };
    let source=r#"import {defineMission,event,action,Crate} from './sdk';
import {map,House,Country,MapObject} from './map.generated';
export default defineMission(map,{id:'crate_pickup',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),iq:0,credits:10000,playerControl:true,allies:[m.house(House.Americans)]});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),iq:0,credits:10000,allies:[m.house(House.Russians)]});
 const at=m.createWaypoint(30,40,50),actor=m.object(MapObject.__ACTOR__);
 m.rule('place',{when:event.sinceScenarioStart(1),once:true,
   actions:[action.createCrate(Crate.Money,at),action.message(m.defineText('placed','placed'))]});
 m.rule('pickup',{when:event.pickedUpCrate(actor),once:true,
   actions:[action.message(m.defineText('object','object')),action.setGlobal(m.globalFlag(0)),__MUTATION__]});
 m.rule('any',{when:event.anyCratePickedUp(),once:true,
   actions:[action.message(m.defineText('global','global')),action.setGlobal(m.globalFlag(1))]});
});"#.replace("__MUTATION__",mutation).replace("__ACTOR__",if vehicle {"Units_0"} else {"Infantry_0"});
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

fn state(runtime: &SimRuntime, actor: u64) -> Value {
    let sim = &runtime.simulation;
    let entity = sim.entities().get(actor);
    let cell = sim.overlay_grid().unwrap().cell(40, 50);
    let slots = sim
        .crate_authority()
        .slots()
        .iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    let balances = ["Americans", "Russians"].map(|name| {
        sim.houses()
            .get(&sim.interner().get(name).unwrap())
            .unwrap()
            .economy()
            .available_money()
    });
    json!({"actor":entity.map(|e|json!({"id":actor,"health":e.health.current(),"alive":e.lifecycle.object_alive(),
        "owner":sim.interner().resolve(e.owner()),"type":sim.interner().resolve(e.type_ref()),"cell":sim.object_coords_cell(actor)})),
        "overlay_id":cell.overlay_id,"selection":cell.overlay_data,"slots":slots,"balances":balances,"flags":sim.global_variables()[..2]})
}

fn audit(install: &Install, bytes: &[u8], root: &Path, name: &str, restore: bool) -> Result<Value> {
    std::fs::create_dir_all(root)?;
    let mut runtime = campaign::load(install, bytes, 1)?;
    let actor = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find(|(_, e)| {
            runtime.simulation.interner().resolve(e.type_ref())
                == if name == "kill_vehicle" {
                    "MTNK"
                } else {
                    "ENGINEER"
                }
        })
        .context("collector")?
        .0;
    let owner = runtime
        .simulation
        .interner()
        .get("Americans")
        .context("owner")?;
    let texts = mission_script::declared_texts(bytes)?;
    let initial = state(&runtime, actor);
    let witnesses = runtime
        .simulation
        .entities()
        .iter_sorted()
        .filter(|(id, _)| *id != actor)
        .map(|(id, e)| {
            (
                id,
                e.health.current(),
                e.owner(),
                runtime.simulation.object_coords_cell(id),
            )
        })
        .collect::<Vec<_>>();
    let mut messages = Vec::new();
    let mut observations = Vec::new();
    let mut placed = None;
    let mut checkpoint_state = None;
    let mut witnesses_preserved = true;
    for frame in 0..600 {
        let tick = runtime.simulation.session().tick();
        let commands = if frame == 60 {
            vec![CommandEnvelope::synchronized(
                runtime
                    .simulation
                    .encode_megamission_record(
                        owner,
                        &Command::Move {
                            entity_id: actor,
                            target_rx: 42,
                            target_ry: 50,
                        },
                    )
                    .context("normal move")?,
                tick,
            )]
        } else {
            Vec::new()
        };
        let output =
            runtime.advance_frame(&commands, 22, TickLane::Ordinary, FrameEffects::default())?;
        let tick = runtime.simulation.session().tick();
        let current = state(&runtime, actor);
        for effect in output.trigger_effects {
            if let TriggerEffect::LocalizedMessage { csf_label } = effect {
                messages.push(json!({"tick":tick,"event":texts.get(&csf_label).context("message label")?,"state":current}));
            }
        }
        witnesses_preserved &= witnesses.iter().all(|(id, hp, house, cell)| {
            runtime.simulation.entities().get(*id).is_some_and(|e| {
                e.health.current() == *hp
                    && e.owner() == *house
                    && runtime.simulation.object_coords_cell(*id) == *cell
            })
        });
        observations.push(json!({"tick":tick,"state":current}));
        if frame == 39 {
            placed = Some(current.clone());
            if restore {
                checkpoint::restore_checkpoint(
                    &mut runtime,
                    bytes,
                    &root.join("before-pickup.bin"),
                )?;
                let after = state(&runtime, actor);
                checkpoint_state = Some(
                    json!({"tick":tick,"preserved":current==after,"before":current,"after":after}),
                );
            }
        }
    }
    let placed = placed.context("placement observation")?;
    let final_state = state(&runtime, actor);
    let expected_events: &[&str] = match name {
        "no_trigger" => &["placed"],
        "kill_vehicle" => &["placed", "object"],
        _ => &["placed", "object", "global"],
    };
    let mut expected_balances = initial["balances"]
        .as_array()
        .context("balances")?
        .iter()
        .map(|v| v.as_i64().unwrap())
        .collect::<Vec<_>>();
    if name != "kill_vehicle" {
        expected_balances[usize::from(name == "transfer")] += 4321;
    }
    let killed = final_state["actor"].is_null()
        || final_state["actor"]["alive"] == false
        || final_state["actor"]["health"]
            .as_i64()
            .is_some_and(|n| n <= 0);
    let mut checks = BTreeMap::from([
        (
            "singleplayer_mode",
            !runtime.simulation.session().game_mode_nonzero(),
        ),
        (
            "placed_real_money_overlay_and_one_slot",
            placed["overlay_id"].is_number()
                && placed["selection"] == 0
                && placed["slots"]
                    .as_array()
                    .is_some_and(|s| s.len() == 1 && s[0]["cell_x"] == 40 && s[0]["cell_y"] == 50),
        ),
        (
            "no_reward_or_callback_before_movement",
            placed["balances"] == initial["balances"] && placed["flags"] == json!([false, false]),
        ),
        (
            "notification_order_and_counts",
            messages
                .iter()
                .map(|m| m["event"].as_str().unwrap())
                .collect::<Vec<_>>()
                == expected_events,
        ),
        (
            "pickup_notifications_after_move",
            messages
                .iter()
                .filter(|m| m["event"] != "placed")
                .all(|m| m["tick"].as_u64().unwrap() > 60),
        ),
        (
            "exact_reward_to_reread_owner",
            final_state["balances"] == json!(expected_balances),
        ),
        (
            "overlay_retained_only_if_callback_retires_actor",
            if name == "kill_vehicle" {
                final_state["overlay_id"] == placed["overlay_id"]
            } else {
                final_state["overlay_id"].is_null()
            },
        ),
        (
            "solo_pickup_preserves_runtime_slot",
            final_state["slots"] == placed["slots"],
        ),
        (
            "flags_match_notification_gates",
            final_state["flags"]
                == json!([
                    name != "no_trigger",
                    !matches!(name, "kill_vehicle" | "no_trigger")
                ]),
        ),
        (
            "actor_lifecycle_and_owner",
            if name.starts_with("kill_") {
                killed
            } else {
                !killed
                    && final_state["actor"]["health"] == initial["actor"]["health"]
                    && final_state["actor"]["owner"]
                        == if name == "transfer" {
                            "Russians"
                        } else {
                            "Americans"
                        }
            },
        ),
        ("witness_buildings_unchanged", witnesses_preserved),
    ]);
    if !name.starts_with("kill_") {
        checks.insert(
            "normal_move_completed",
            final_state["actor"]["cell"] == json!([if name == "transfer" { 40 } else { 42 }, 50]),
        );
    }
    if name != "no_trigger" {
        let callback = messages.iter().find(|m| m["event"] == "object");
        checks.insert(
            "object_callback_precedes_global_dispatch",
            callback.is_some_and(|m| m["state"]["flags"] == json!([true, false])),
        );
        checks.insert(
            "reward_and_overlay_settled_in_pickup_frame",
            callback.is_some_and(|m| {
                m["state"]["balances"] == json!(expected_balances)
                    && m["state"]["overlay_id"] == final_state["overlay_id"]
            }),
        );
        if name.starts_with("kill_") {
            checks.insert(
                "callback_death_uses_actual_alive_lifecycle",
                callback.is_some_and(|m| {
                    if name == "kill_infantry" {
                        m["state"]["actor"]["health"] == 0 && m["state"]["actor"]["alive"] == true
                    } else {
                        m["state"]["actor"].is_null() || m["state"]["actor"]["alive"] == false
                    }
                }),
            );
        }
    }
    if restore {
        checks.insert(
            "checkpoint_preserves_listed_state",
            checkpoint_state
                .as_ref()
                .is_some_and(|s| s["preserved"] == true),
        );
    }
    let passed = checks.values().all(|b| *b);
    std::fs::write(
        root.join("observations.json"),
        serde_json::to_vec_pretty(&observations)?,
    )?;
    Ok(
        json!({"passed":passed,"checks":checks,"initial":initial,"placed":placed,"final":final_state,"messages":messages,"checkpoint":checkpoint_state}),
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
    for name in [
        "normal",
        "no_trigger",
        "transfer",
        "kill_infantry",
        "kill_vehicle",
    ] {
        let folder = root.join(name);
        let bytes = build(&mut install, &folder, name)?;
        let continuous = audit(&install, &bytes, &folder.join("continuous"), name, false)?;
        let restored = audit(&install, &bytes, &folder.join("restored"), name, true)?;
        let agreement = continuous["messages"] == restored["messages"]
            && continuous["final"] == restored["final"];
        cases.push(json!({"case":name,"continuous":continuous,"restored":restored,"listed_outcomes_match":agreement}));
    }
    let passed = cases.iter().all(|c| {
        c["continuous"]["passed"] == true
            && c["restored"]["passed"] == true
            && c["listed_outcomes_match"] == true
    });
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&json!({"passed":passed,"cases":cases,
        "scope":"TS createCrate and object/global events through production campaign loading, encoded Move and ordinary frames; controlled map CrateTrigger and SoloCrateMoney. Checkpoints compare listed state and outcomes only, not full simulation or GUI saves. Transfer payout records current engine behavior, not native cross-House parity."}))?,
    )?;
    ensure!(passed, "crate pickup audit failed; inspect report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
