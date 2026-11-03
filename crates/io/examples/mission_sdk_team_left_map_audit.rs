//! TS team event23 through real aircraft retreat and ordinary membership cleanup.
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
        trigger_runtime::TriggerEffect,
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;

fn actor_type(name: &str) -> &str {
    match name {
        "air_retreat" | "air_hold" => "PDPLANE",
        "vehicle_remove" => "MTNK",
        _ => "ENGINEER",
    }
}
fn build(install: &mut Install, root: &Path, name: &str) -> Result<(Vec<u8>, String)> {
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
    let section = match name {
        "air_retreat" | "air_hold" => "Aircraft",
        "vehicle_remove" => "Units",
        _ => "Infantry",
    };
    for cell in [[40, 60], [42, 60]] {
        doc.add_record(section, cell, "Americans", actor_type(name))?;
    }
    for key in ["Primary", "Secondary"] {
        doc.set_ini(actor_type(name), key, Some(b"none".to_vec()))?;
    }
    doc.commit("team left map fixture");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(
        install,
        root.join("base.map").to_str().context("map path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let tail = match name {
        "air_retreat" => "order.mission(UnitMission.Retreat)",
        "infantry_remove" | "vehicle_remove" => "order.removeMembers()",
        _ => "order.waitForever()",
    };
    let source=r#"import {defineMission,event,action,order,UnitMission} from './sdk';
import {map,House,Country,Unit} from './map.generated';
export default defineMission(map,{id:'team_left_map',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),iq:0,playerControl:true,allies:[m.house(House.Americans)]});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),iq:0,allies:[m.house(House.Russians)]});
 m.rule('left',{when:event.teamLeftMap(m.teamRef('main')),owner:m.country(Country.Americans),once:true,
   actions:[action.message(m.defineText('left','left')),action.setGlobal(m.globalFlag(0))]});
 m.rule('wrong',{when:event.teamLeftMap(m.teamRef('wrong')),owner:m.country(Country.Americans),once:true,
   actions:[action.message(m.defineText('wrong','wrong')),action.setGlobal(m.globalFlag(1))]});
 m.rule('null_owner',{when:event.teamLeftMap(m.teamRef('wrong')),owner:m.country(Country.Alliance),once:true,
   actions:[action.message(m.defineText('null_owner','null_owner')),action.setGlobal(m.globalFlag(2))]});
 const origin=m.createWaypoint(30,40,60);
 const main=m.team('main',{house:m.house(House.Americans),waypoint:origin,units:[{type:m.unit(Unit.__TYPE__),count:2}],
   full:true,max:1,reinforce:false,orders:[order.setGlobal(m.globalFlag(3)),order.guard(4),__TAIL__]});
 m.team('wrong',{house:m.house(House.Americans),waypoint:origin,units:[{type:m.unit(Unit.__TYPE__),count:2}],
   full:true,max:1,reinforce:false,orders:[order.waitForever()]});
 m.rule('dispatch',{when:event.sinceScenarioStart(1),once:true,actions:[action.recruit(main),action.message(m.defineText('dispatch','dispatch'))]});
 __DISBAND__
});"#.replace("__TYPE__",actor_type(name)).replace("__TAIL__",tail).replace("__DISBAND__",if name=="infantry_disband" {r#"m.rule('disband',{when:event.sinceScenarioStart(8),once:true,actions:[action.deleteTeam(main),action.message(m.defineText('disband','disband'))]});"#}else{""});
    let entry = root.join("mission.ts");
    std::fs::write(&entry, source)?;
    mission_script::refresh_types(&entry, &catalog)?;
    Worker::new()?
        .build(&doc, catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(&mut doc)?;
    let main = mission::ids(&doc, Kind::Team)
        .into_iter()
        .find(|id| doc.ini_text(id, "Name") == "main")
        .context("generated main TeamType")?;
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(root.join("compiled.map"), &bytes)?;
    Ok((bytes, main))
}
fn state(runtime: &SimRuntime, ids: &[u64]) -> Result<Value> {
    let sim = &runtime.simulation;
    let teams = sim
        .team_script_vm()
        .teams_in_order()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
    let actors=ids.iter().map(|id|sim.entities().get(*id).map(|e|{
        let c=vera20k_core::sim::movement::ground_pose::position_world_coord(&e.position);
        let location=vera20k_core::sim::movement::ground_pose::object_location(e,sim.resolved_terrain());
        let inside=vera20k_core::sim::cell_rect::cell_is_in_playfield_leptons((location.x,location.y,location.z),sim.playfield_bounds(),sim.resolved_terrain());
        json!({"id":id,"type":sim.interner().resolve(e.type_ref()),"owner":sim.interner().resolve(e.owner()),"health":e.health.current(),
            "cell":sim.object_coords_cell(*id),"coord":[c.x,c.y,c.z],"in_playfield":e.in_playfield(),"inside_now":inside,"alive":e.lifecycle.object_alive(),"mission":e.mission.effective().raw()})
    })).collect::<Vec<_>>();
    let house = sim
        .houses()
        .get(&sim.interner().get("Americans").unwrap())
        .context("player House")?;
    Ok(
        json!({"actors":actors,"teams":teams,"flags":sim.global_variables()[..4],"units_lost":house.stats().units_lost()}),
    )
}
fn audit(
    install: &Install,
    bytes: &[u8],
    root: &Path,
    name: &str,
    main: &str,
    restore: bool,
) -> Result<Value> {
    std::fs::create_dir_all(root)?;
    let mut runtime = campaign::load(install, bytes, 1)?;
    let sim = &runtime.simulation;
    let ids = sim
        .entities()
        .iter_sorted()
        .filter(|(_, e)| sim.interner().resolve(e.type_ref()) == actor_type(name))
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    ensure!(ids.len() == 2, "two authored members");
    let main_type = sim.interner().get(main).context("main type interner")?;
    let witnesses = sim
        .entities()
        .iter_sorted()
        .filter(|(id, _)| !ids.contains(id))
        .map(|(id, e)| {
            (
                id,
                e.health.current(),
                e.owner(),
                sim.object_coords_cell(id),
            )
        })
        .collect::<Vec<_>>();
    let initial = state(&runtime, &ids)?;
    let texts = mission_script::declared_texts(bytes)?;
    let mut messages = Vec::new();
    let mut observations = Vec::new();
    let mut checkpoint_state = None;
    let mut all_recruited = false;
    let mut full_team_seen = false;
    let mut leaving_seen = false;
    let mut first_absence = None;
    let mut all_absent = None;
    let mut departed = Vec::new();
    let mut remaining = Vec::new();
    let mut witnesses_preserved = true;
    let mut healthy_until_removed = true;
    for frame in 0..2200 {
        let output = runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let sim = &runtime.simulation;
        let tick = sim.session().tick();
        let current = state(&runtime, &ids)?;
        let team = sim
            .team_script_vm()
            .teams_in_order()
            .find(|t| t.team_type_id() == Some(main_type));
        if let Some(team) = team {
            all_recruited |= team.members().collect::<std::collections::BTreeSet<_>>()
                == ids.iter().copied().collect();
            let saved = serde_json::to_value(team)?;
            full_team_seen |= saved["has_been_full"] == true;
            leaving_seen |= saved["leaving_map"] == true;
        }
        for (i, actor) in current["actors"].as_array().unwrap().iter().enumerate() {
            if actor.is_null() {
                if !departed.iter().any(|v: &Value| v["id"] == ids[i]) {
                    departed.push(json!({"id":ids[i],"tick":tick}));
                }
            } else {
                healthy_until_removed &= actor["health"] == initial["actors"][i]["health"]
                    && actor["owner"] == "Americans"
                    && actor["type"] == actor_type(name);
            }
        }
        let count = current["actors"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| !v.is_null())
            .count();
        if count < 2 {
            first_absence.get_or_insert(tick);
        }
        if count == 0 {
            all_absent.get_or_insert(tick);
        }
        if count == 1 {
            remaining.push(tick);
        }
        let row = json!({"tick":tick,"state":current});
        for effect in output.trigger_effects {
            if let TriggerEffect::LocalizedMessage { csf_label } = effect {
                messages.push(json!({"tick":tick,"event":texts.get(&csf_label).context("message label")?,"state":current}));
            }
        }
        witnesses_preserved &= witnesses.iter().all(|(id, hp, owner, cell)| {
            sim.entities().get(*id).is_some_and(|e| {
                e.health.current() == *hp
                    && e.owner() == *owner
                    && sim.object_coords_cell(*id) == *cell
            })
        });
        observations.push(row);
        if restore && frame == 39 {
            checkpoint::restore_checkpoint(&mut runtime, bytes, &root.join("before-exit.bin"))?;
            let after = state(&runtime, &ids)?;
            checkpoint_state = Some(
                json!({"tick":tick,"preserved":current==after,"before":current,"after":after}),
            );
        }
    }
    let sim = &runtime.simulation;
    let final_state = state(&runtime, &ids)?;
    let count = |event: &str| messages.iter().filter(|m| m["event"] == event).count();
    let expected = matches!(name, "air_retreat" | "infantry_remove");
    let removed = matches!(name, "air_retreat" | "infantry_remove" | "vehicle_remove");
    let active = sim
        .team_script_vm()
        .teams_in_order()
        .any(|t| t.team_type_id() == Some(main_type));
    let expected_active = matches!(name, "infantry_wait" | "air_hold");
    let mut checks = BTreeMap::from([
        ("two_original_members_recruited", all_recruited),
        ("team_reached_full_strength", full_team_seen),
        ("script_started", sim.global_variables()[3]),
        ("dispatch_once", count("dispatch") == 1),
        (
            "existing_owner_filters_wrong_template",
            count("wrong") == 0 && !sim.global_variables()[1],
        ),
        (
            "team_left_notification_matches_latch_lifecycle",
            count("left") == usize::from(expected) && sim.global_variables()[0] == expected,
        ),
        (
            "null_owner_bypasses_template_filter_only_on_broadcast",
            count("null_owner") == usize::from(expected) && sim.global_variables()[2] == expected,
        ),
        (
            "all_original_members_removed_or_preserved_as_requested",
            final_state["actors"]
                .as_array()
                .unwrap()
                .iter()
                .all(|a| a.is_null() == removed),
        ),
        ("final_team_lifecycle", active == expected_active),
        (
            "no_kill_losses",
            final_state["units_lost"] == initial["units_lost"],
        ),
        (
            "members_healthy_with_original_owner_until_removed",
            healthy_until_removed,
        ),
        ("witness_buildings_preserved", witnesses_preserved),
        (
            "no_premature_left_notification",
            messages
                .iter()
                .filter(|m| m["event"] == "left")
                .all(|m| all_absent.is_some_and(|tick| m["tick"].as_u64().unwrap() >= tick)),
        ),
    ]);
    if expected {
        checks.insert("leaving_latch_observed", leaving_seen);
        checks.insert(
            "empty_leaving_team_observed_before_callback",
            messages
                .iter()
                .find(|m| m["event"] == "left")
                .is_some_and(|m| {
                    observations.iter().any(|r| {
                        r["tick"].as_u64().unwrap() < m["tick"].as_u64().unwrap()
                            && r["state"]["teams"].as_array().unwrap().iter().any(|t| {
                                t["team_type_id"] == json!(main_type)
                                    && t["leaving_map"] == true
                                    && t["members"] == json!([])
                            })
                    })
                }),
        );
    }
    if name == "air_retreat" {
        checks.insert(
            "real_aircraft_retreat_seen",
            observations.iter().any(|r| {
                r["state"]["actors"].as_array().unwrap().iter().any(|a| {
                    !a.is_null() && a["mission"] == 4 && a["coord"][2].as_i64().unwrap() > 0
                })
            }),
        );
        checks.insert(
            "both_aircraft_exit_from_healthy_retreat_flight",
            ids.iter().enumerate().all(|(i, id)| {
                let last = observations.iter().rev().find_map(|r| {
                    r["state"]["actors"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|a| a["id"] == *id)
                });
                last.is_some_and(|a| {
                    a["mission"] == 4
                        && a["health"] == initial["actors"][i]["health"]
                        && a["coord"][2].as_i64().unwrap() > 0
                        && a["cell"] != initial["actors"][i]["cell"]
                        && final_state["actors"][i].is_null()
                })
            }),
        );
        checks.insert(
            "one_member_departure_does_not_raise_event",
            !remaining.is_empty()
                && observations
                    .iter()
                    .filter(|r| remaining.contains(&r["tick"].as_u64().unwrap()))
                    .all(|r| r["state"]["flags"][0] == false),
        );
    }
    if name == "infantry_remove" {
        checks.insert(
            "in_map_infantry_also_sets_leaving_latch",
            observations
                .iter()
                .filter(|r| {
                    r["state"]["actors"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|a| !a.is_null())
                })
                .all(|r| {
                    r["state"]["actors"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|a| a["in_playfield"] == true && a["inside_now"] == true)
                }),
        );
    }
    if name == "infantry_disband" {
        checks.insert("disband_action_ran_once", count("disband") == 1);
    }
    if restore {
        checks.insert(
            "checkpoint_preserves_listed_state",
            checkpoint_state
                .as_ref()
                .is_some_and(|c| c["preserved"] == true),
        );
    }
    // Fly motion and out-of-bounds UnInit occur in one object visit. Report
    // surviving post-frame geometry without requiring an impossible outside sample.
    let outside_postframe_seen = ids.iter().map(|id| {
        json!({"id":id,"seen":observations.iter().any(|r| {
            r["state"]["actors"].as_array().unwrap().iter().any(|a| a["id"] == *id && a["inside_now"] == false)
        })})
    }).collect::<Vec<_>>();
    let passed = checks.values().all(|b| *b);
    std::fs::write(
        root.join("observations.json"),
        serde_json::to_vec_pretty(&observations)?,
    )?;
    Ok(
        json!({"passed":passed,"checks":checks,"initial":initial,"final":final_state,"messages":messages,"departed":departed,"first_absence":first_absence,"all_absent":all_absent,"single_remaining_frames":remaining,"outside_postframe_seen":outside_postframe_seen,"checkpoint":checkpoint_state}),
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
        "air_retreat",
        "air_hold",
        "infantry_remove",
        "vehicle_remove",
        "infantry_disband",
        "infantry_wait",
    ] {
        let folder = root.join(name);
        let (bytes, main) = build(&mut install, &folder, name)?;
        let continuous = audit(
            &install,
            &bytes,
            &folder.join("continuous"),
            name,
            &main,
            false,
        )?;
        if matches!(name, "air_retreat" | "infantry_remove" | "infantry_disband") {
            let restored = audit(
                &install,
                &bytes,
                &folder.join("restored"),
                name,
                &main,
                true,
            )?;
            let events = |v: &Value| {
                v["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|m| m["event"].clone())
                    .collect::<Vec<_>>()
            };
            let agreement = events(&continuous) == events(&restored)
                && continuous["final"] == restored["final"];
            let ticks_match = continuous["departed"] == restored["departed"]
                && continuous["messages"] == restored["messages"];
            cases.push(json!({"case":format!("{name}_restored"),"result":restored,"listed_outcomes_match":agreement,"continuous_ticks_match":ticks_match}));
        }
        cases.push(json!({"case":name,"native_team_type":main,"result":continuous}));
    }
    let passed = cases.iter().all(|c| {
        c["result"]["passed"] == true && c.get("listed_outcomes_match").is_none_or(|v| v == true)
    });
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&json!({"passed":passed,"cases":cases,
        "scope":"TS forward TeamType refs, ordinary recruitment, guarded script Retreat/RemoveMembers and DeleteTeam. No notification, membership, position or latch injection. Headless checkpoints compare listed state and outcomes; frame equality is reported separately. Not GUI saves, network or native pixel parity."}))?,
    )?;
    ensure!(passed, "team left map audit failed; inspect report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
