//! TS idle/attached animations and deferred House emergency strategy.
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

fn attached(name: &str) -> bool {
    name.starts_with("attached_")
}
fn strategy(name: &str) -> bool {
    name.starts_with("sale_")
}
fn ai_sale(name: &str) -> bool {
    matches!(name, "sale_script_ai" | "sale_action_ai")
}
fn owner(name: &str) -> &'static str {
    if strategy(name) && name != "sale_script_human" {
        "Russians"
    } else {
        "Americans"
    }
}
fn loops(name: &str) -> u32 {
    name.rsplit('_')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1)
}

fn animation(name: &str) -> &'static str {
    if name.contains("chrono") {
        "CHRONOSK"
    } else {
        "EXPLOSML"
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
    for house in ["Americans", "Russians"] {
        mission::create(&mut doc, Kind::House, house, house, None)?;
        doc.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    doc.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    doc.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    doc.add_record("Structures", [20, 46], "Americans", "GAPOWR")?;
    doc.add_record("Structures", [80, 80], "Russians", "GAPOWR")?;
    if strategy(name) {
        doc.add_record("Structures", [60, 70], owner(name), "NAHAND")?;
        doc.add_record("Structures", [64, 74], owner(name), "NAPOWR")?;
    }
    let tank = attached(name) || strategy(name) || name == "idle_tank";
    let kind = if tank { "MTNK" } else { "E2" };
    doc.add_record(
        if tank { "Units" } else { "Infantry" },
        [40, 60],
        owner(name),
        kind,
    )?;
    if attached(name) {
        doc.add_record("Units", [42, 60], owner(name), kind)?;
    }
    for ty in ["MTNK", "E2"] {
        for key in ["Primary", "Secondary"] {
            doc.set_ini(ty, key, Some(b"none".to_vec()))?;
        }
    }
    for ty in ["NAHAND", "NAPOWR", "GAPOWR"] {
        doc.set_ini(ty, "Crewed", Some(b"no".to_vec()))?;
    }
    doc.commit("Team animation and strategy fixture");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(
        install,
        root.join("base.map").to_str().context("fixture path")?,
    )?;
    let command = if attached(name) {
        format!(
            "order.playAnimation(m.animation(Animation.{}),{})",
            animation(name),
            loops(name)
        )
    } else if strategy(name) {
        if name == "sale_script_ai" || name == "sale_script_human" {
            "order.fireSale()".into()
        } else {
            "order.next()".into()
        }
    } else {
        format!("order.idleAnimation({})", u8::from(name == "idle_two"))
    };
    let source=r#"import {defineMission,event,action,order} from './sdk';
import {map,House,Country,Unit,Animation} from './map.generated';
export default defineMission(map,{id:'team_animation_strategy',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),credits:100,iq:0,playerControl:true,allies:[m.house(House.Americans)]});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),credits:100,iq:0,allies:[m.house(House.Russians)]});
 const start=m.createWaypoint(30,40,60),end=m.createWaypoint(31,48,64);
 const team=m.team('main',{house:m.house(House.__OWNER__),waypoint:start,units:[{type:m.unit(Unit.__KIND__),count:__COUNT__}],full:true,max:1,reinforce:false,
  orders:[order.guard(4),__COMMAND__,order.setGlobal(m.globalFlag(0)),__MOVE__order.guard(12),order.setGlobal(m.globalFlag(1)),order.waitForever()]});
 m.rule('dispatch',{when:event.sinceScenarioStart(1),once:true,actions:[action.recruit(team)]});
 __ACTION__
});"#.replace("__OWNER__",owner(name)).replace("__KIND__",kind)
        .replace("__COUNT__",if attached(name) {"2"} else {"1"}).replace("__COMMAND__",&command)
        .replace("__MOVE__",if attached(name) {"order.moveTo(end),"}else{""})
        .replace("__ACTION__",if name=="sale_action_ai" {"m.rule('sell',{when:event.sinceScenarioStart(6),once:true,actions:[action.fireSale(m.country(Country.Russians))]});"}else{""});
    let entry = root.join("mission.ts");
    std::fs::write(&entry, source)?;
    let catalog = mission_script::catalog(&doc, &scene);
    mission_script::refresh_types(&entry, &catalog)?;
    Worker::new()?
        .build(&doc, catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(&mut doc)?;
    let team = mission::ids(&doc, Kind::Team)
        .into_iter()
        .find(|id| doc.ini_text(id, "Name") == "main")
        .context("main team type")?;
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(root.join("compiled.map"), &bytes)?;
    Ok((bytes, team))
}

fn state(runtime: &SimRuntime, team: &str) -> Result<Value> {
    let sim = &runtime.simulation;
    let actors=sim.entities().iter_sorted().map(|(id,e)|json!({"id":id,"type":sim.interner().resolve(e.type_ref()),"owner":sim.interner().resolve(e.owner()),
        "cell":sim.object_coords_cell(id),"health":e.health.current(),"alive":e.lifecycle.object_alive(),"limbo":e.lifecycle.in_limbo(),
        "mission":e.mission.effective().raw(),"queued":e.mission.queued().raw(),"nav":e.navigation.nav_com(),"selling":e.building_down(),
        "doing":e.mission_leaf.as_infantry().map(|i|i.doing()),"stage":e.native_stage(),"world_xy":vera20k_core::sim::movement::ground_pose::position_world_xy(&e.position)})).collect::<Vec<_>>();
    let teams = sim
        .team_script_vm()
        .teams_in_order()
        .filter(|t| {
            t.team_type_id()
                .is_some_and(|id| sim.interner().resolve(id) == team)
        })
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
    let animations=sim.anims().filter(|(_,a)|["EXPLOSML","CHRONOSK"].contains(&sim.interner().resolve(a.type_id()))).map(|(id,a)|json!({"id":id,"owner":a.owner_entity(),"absolute":sim.anim_absolute_coord(*id),"anim":a})).collect::<Vec<_>>();
    let houses=sim.houses().order().iter().map(|id|{let h=&sim.houses()[id];json!({"id":sim.interner().resolve(*id),"human":h.is_controlled_by_human(sim.session().game_mode_nonzero()),
        "emergency":h.strategy_emergency(),"strategy_timer":h.strategy_timer(),"money":h.economy().available_money(),"unit_losses":h.stats().units_lost(),"building_losses":h.stats().buildings_lost(),"outcome":h.outcome()})}).collect::<Vec<_>>();
    Ok(
        json!({"actors":actors,"teams":teams,"animations":animations,"houses":houses,"flags":sim.global_variables()[..2]}),
    )
}
fn house<'a>(s: &'a Value, name: &str) -> &'a Value {
    s["houses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["id"] == name)
        .unwrap()
}
fn actors(s: &Value) -> &[Value] {
    s["actors"].as_array().unwrap()
}
fn member(a: &Value) -> bool {
    a["type"] == "MTNK" || a["type"] == "E2"
}

// Raw clocks remain in final/observations and their separate full comparison.
// The existing checkpoint resets Scenario RNG: future strategy deadlines and
// the retained timer of a completed idle action need not replay identically.
fn outcomes(s: &Value) -> Value {
    let mut result = s.clone();
    for h in result["houses"].as_array_mut().unwrap() {
        h.as_object_mut().unwrap().remove("strategy_timer");
    }
    for a in result["actors"].as_array_mut().unwrap() {
        a["stage"].as_object_mut().unwrap().remove("timer");
    }
    result
}

fn audit(
    install: &Install,
    bytes: &[u8],
    root: &Path,
    name: &str,
    team: &str,
    restore: bool,
) -> Result<Value> {
    std::fs::create_dir_all(root)?;
    let mut runtime = campaign::load(install, bytes, 1)?;
    let inventory=(0..).map_while(|i|runtime.resources.rules.anim_type_name_by_index(i)).filter_map(|name|runtime.resources.rules.art().anim_runtime_config(name).map(|c|json!({"name":name,"loops":c.loop_count,"rate":c.rate_logic_frames,"start":c.start,"loop_start":c.loop_start,"loop_end":c.loop_end,"end":c.end,"frames":c.raw_shp_frame_count}))).collect::<Vec<_>>();
    std::fs::write(
        root.join("animation-configs.json"),
        serde_json::to_vec_pretty(&inventory)?,
    )?;
    let initial = state(&runtime, team)?;
    let members = actors(&initial)
        .iter()
        .filter(|a| member(a))
        .cloned()
        .collect::<Vec<_>>();
    let expected_refund = runtime
        .simulation
        .entities()
        .iter_sorted()
        .filter(|(_, e)| {
            runtime.simulation.interner().resolve(e.owner()) == owner(name)
                && !matches!(
                    runtime.simulation.interner().resolve(e.type_ref()),
                    "MTNK" | "E2"
                )
        })
        .filter_map(|(id, _)| {
            vera20k_core::sim::production::techno_refund(
                &runtime.simulation,
                &runtime.resources.rules,
                id,
            )
        })
        .sum::<i32>();
    let raw_loops = runtime
        .resources
        .rules
        .art()
        .anim_runtime_config(animation(name))
        .context("stock animation config")?
        .loop_count;
    let expected_loops = (raw_loops as u8)
        .wrapping_mul(if loops(name) > 1 {
            loops(name) as u8
        } else {
            1
        })
        .max(1);
    let mut command = None;
    let mut following = None;
    let mut finished = None;
    let mut sale = None;
    let mut retired = None;
    let mut checkpoint_state = None;
    let mut trace = Vec::new();
    let mut created = false;
    let mut identities = true;
    let mut witnesses = true;
    let mut bindings = true;
    let mut follows_move = false;
    let mut unique_animations = std::collections::BTreeSet::new();
    let mut idle_completed = false;
    let mut stage_changed = false;
    let mut timer_unchanged = true;
    let mut deferred = true;
    let mut sold_events = 0;
    let mut timer_due = false;
    let mut animation_end = None;
    for frame in 0..700 {
        let before = state(&runtime, team)?;
        let h = runtime
            .simulation
            .interner()
            .get(owner(name))
            .context("owner House")?;
        let due = runtime.simulation.houses()[&h]
            .strategy_timer()
            .expired(runtime.simulation.session().binary_frame() as i32);
        let outcome =
            runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        sold_events += outcome
            .sound_events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    vera20k_core::sim::world::SimSoundEvent::StructureSold { .. }
                )
            })
            .count();
        let sim = &runtime.simulation;
        let tick = sim.session().tick();
        let current = state(&runtime, team)?;
        identities &= members.iter().all(|a| {
            actors(&current).iter().any(|b| {
                a["id"] == b["id"]
                    && a["type"] == b["type"]
                    && a["owner"] == b["owner"]
                    && a["health"] == b["health"]
                    && b["alive"] == true
            })
        });
        witnesses &= actors(&initial)
            .iter()
            .filter(|a| !member(a) && (!ai_sale(name) || a["owner"] != owner(name)))
            .all(|a| {
                actors(&current).iter().any(|b| {
                    ["id", "type", "owner", "cell", "health", "alive"]
                        .iter()
                        .all(|k| a[k] == b[k])
                })
            });
        let live = current["teams"].as_array().unwrap().first();
        if let Some(t) = live {
            created = true;
            if t["cursor"] == 1 && command.is_none() {
                timer_unchanged = house(&current, owner(name))["strategy_timer"]
                    == house(&before, owner(name))["strategy_timer"];
                deferred = actors(&current)
                    .iter()
                    .filter(|a| !member(a))
                    .all(|a| a["selling"] == false);
                command = Some(json!({"tick":tick,"state":current}));
            }
        }
        if current["flags"][0] == true && following.is_none() {
            following = Some(json!({"tick":tick,"state":current}));
        }
        if current["flags"][1] == true && finished.is_none() {
            finished = Some(json!({"tick":tick,"state":current}));
        }
        if house(&current, owner(name))["emergency"]["all_to_hunt_bias"] == true && sale.is_none() {
            timer_due = due;
            sale = Some(json!({"tick":tick,"state":current}));
        }
        if created && live.is_none() && retired.is_none() {
            retired = Some(json!({"tick":tick,"state":current}));
        }
        for a in current["animations"].as_array().unwrap() {
            unique_animations.insert(a["id"].to_string());
            let parent = actors(&current).iter().find(|p| p["id"] == a["owner"]);
            bindings &= parent.is_some_and(member) && a["anim"]["draw_flags"] == 0x600;
            if let Some(p) = parent {
                bindings &= a["absolute"]["x"] == p["world_xy"][0]
                    && a["absolute"]["y"] == p["world_xy"][1];
                follows_move |= members
                    .iter()
                    .any(|m| m["id"] == p["id"] && m["world_xy"] != p["world_xy"]);
            }
        }
        if !unique_animations.is_empty()
            && current["animations"] == json!([])
            && animation_end.is_none()
        {
            animation_end = Some(tick);
        }
        if let Some(c) = &command
            && !strategy(name)
            && !attached(name)
            && name != "idle_tank"
            && tick > c["tick"].as_u64().unwrap()
        {
            idle_completed |= actors(&current)
                .iter()
                .filter(|a| member(a))
                .all(|a| a["doing"] == 0 || a["doing"] == 1);
            stage_changed |= actors(&current).iter().filter(|a| member(a)).any(|a| {
                actors(&c["state"])
                    .iter()
                    .any(|old| old["id"] == a["id"] && old["stage"] != a["stage"])
            });
        }
        if frame % 10 == 0 {
            trace.push(json!({"tick":tick,"state":current}));
        }
        let checkpoint_due = command.is_some()
            && (name != "sale_action_ai" || house(&current, owner(name))["emergency"]["mode"] == 4);
        if restore && checkpoint_state.is_none() && checkpoint_due {
            checkpoint::restore_checkpoint(&mut runtime, bytes, &root.join("checkpoint.bin"))?;
            let after = state(&runtime, team)?;
            checkpoint_state = Some(
                json!({"tick":tick,"preserved":current==after,"before":current,"after":after}),
            );
        }
    }
    let final_state = state(&runtime, team)?;
    let at_command = command.as_ref().map(|c| &c["state"]);
    let expected_sale = ai_sale(name);
    let checks = BTreeMap::from([
        ("normal_recruitment_reaches_command", command.is_some()),
        (
            "sale_occurs_only_on_due_house_strategy",
            !ai_sale(name) || timer_due,
        ),
        (
            "ai_sale_removes_original_buildings_and_refunds_exactly",
            !ai_sale(name)
                || (actors(&initial)
                    .iter()
                    .filter(|a| !member(a) && a["owner"] == owner(name))
                    .all(|a| !actors(&final_state).iter().any(|b| b["id"] == a["id"]))
                    && house(&final_state, owner(name))["money"].as_i64()
                        == Some(
                            house(&initial, owner(name))["money"].as_i64().unwrap()
                                + i64::from(expected_refund),
                        )),
        ),
        (
            "original_members_preserve_identity_health_owner",
            identities,
        ),
        ("other_house_and_unsold_witnesses_preserved", witnesses),
        ("following_script_marker_runs", following.is_some()),
        (
            "idle_request_matches_variant_or_empty_tank_receiver",
            strategy(name)
                || attached(name)
                || at_command.is_some_and(|s| {
                    actors(s).iter().filter(|a| member(a)).all(|a| {
                        if name == "idle_tank" {
                            a["doing"].is_null()
                        } else {
                            a["doing"] == if name == "idle_two" { 10 } else { 9 }
                        }
                    })
                }),
        ),
        (
            "idle_stage_advances_then_action_completes",
            strategy(name)
                || attached(name)
                || name == "idle_tank"
                || (stage_changed && idle_completed),
        ),
        (
            "attached_animation_per_original_member_once",
            !attached(name) || unique_animations.len() == members.len(),
        ),
        (
            "animation_attached_to_members_with_native_draw_flags",
            bindings,
        ),
        (
            "constructor_loop_byte_matches_argument",
            !attached(name)
                || at_command.is_some_and(|s| {
                    s["animations"].as_array().is_some_and(|a| {
                        a.len() == members.len()
                            && a.iter()
                                .all(|a| a["anim"]["runtime"]["loop_remaining"] == expected_loops)
                    })
                }),
        ),
        (
            "attached_animation_survives_into_actual_movement",
            !name.contains("chrono") || follows_move,
        ),
        (
            "finite_attached_animations_finish",
            !attached(name) || final_state["animations"] == json!([]),
        ),
        (
            "script_does_not_rearm_strategy_timer",
            !name.starts_with("sale_script_") || timer_unchanged,
        ),
        (
            "script_does_not_sell_synchronously",
            !name.starts_with("sale_script_") || deferred,
        ),
        (
            "emergency_state_four_written_only_when_requested",
            !strategy(name)
                || if name == "sale_control_ai" {
                    house(&final_state, owner(name))["emergency"]["mode"] != 4
                } else {
                    house(&final_state, owner(name))["emergency"]["mode"] == 4
                },
        ),
        (
            "sale_and_hunt_requires_ai_strategy",
            sale.is_some() == expected_sale,
        ),
        (
            "actual_sale_events_only_for_ai",
            if expected_sale {
                sold_events == 3
            } else {
                sold_events == 0
            },
        ),
        (
            "ai_sale_releases_original_team",
            retired.is_some() == expected_sale,
        ),
        (
            "normal_later_guard_finishes_without_ai_sale",
            expected_sale || finished.is_some(),
        ),
        (
            "no_player_outcome_or_combat_losses",
            final_state["houses"].as_array().unwrap().iter().all(|h| {
                h["outcome"].is_null() && h["unit_losses"] == 0 && h["building_losses"] == 0
            }),
        ),
        (
            "checkpoint_preserves_listed_state",
            !restore
                || checkpoint_state
                    .as_ref()
                    .is_some_and(|s| s["preserved"] == true),
        ),
    ]);
    std::fs::write(
        root.join("observations.json"),
        serde_json::to_vec_pretty(&trace)?,
    )?;
    Ok(
        json!({"passed":checks.values().all(|b|*b),"checks":checks,"initial":initial,"final":final_state,"command":command,"following":following,"finished":finished,"sale":sale,"retired":retired,"sold_events":sold_events,"unique_animations":unique_animations,"animation_end":animation_end,"expected_refund":expected_refund,"art_loop_count":raw_loops,"checkpoint":checkpoint_state}),
    )
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(args.next().context("new output directory")?)?;
    let filter = args.next().and_then(|s| s.into_string().ok());
    ensure!(
        args.next().is_none() && !root.exists(),
        "output directory must be new"
    );
    std::fs::create_dir_all(&root)?;
    let mut install = Install::open(&game)?;
    let mut cases = Vec::new();
    for name in [
        "idle_one",
        "idle_two",
        "idle_tank",
        "attached_0",
        "attached_1",
        "attached_3",
        "attached_256",
        "attached_chrono_1",
        "attached_chrono_3",
        "attached_chrono_256",
        "sale_script_ai",
        "sale_script_human",
        "sale_action_ai",
        "sale_control_ai",
    ] {
        if filter.as_ref().is_some_and(|f| name != f) {
            continue;
        }
        let folder = root.join(name);
        let (bytes, team) = build(&mut install, &folder, name)?;
        let continuous = audit(
            &install,
            &bytes,
            &folder.join("continuous"),
            name,
            &team,
            false,
        )?;
        let restored = audit(
            &install,
            &bytes,
            &folder.join("restored"),
            name,
            &team,
            true,
        )?;
        let stages = ["command", "following", "finished", "sale", "retired"]
            .iter()
            .all(|k| continuous[k] == restored[k]);
        let ticks = ["command", "following", "finished", "sale", "retired"]
            .iter()
            .all(|k| continuous[k].get("tick") == restored[k].get("tick"))
            && continuous["animation_end"] == restored["animation_end"];
        cases.push(json!({"case":name,"continuous":continuous,"restored":restored,"final_outcomes_match":outcomes(&continuous["final"])==outcomes(&restored["final"]),"full_listed_final_states_match":continuous["final"]==restored["final"],"continuous_event_ticks_match":ticks,"continuous_ticks_and_stage_states_match":stages}));
    }
    ensure!(!cases.is_empty(), "unknown fixture name");
    let mut lifecycle_checks = BTreeMap::new();
    if filter.is_none() {
        let end = |name: &str, mode: &str| {
            cases
                .iter()
                .find(|c| c["case"] == name)
                .and_then(|c| c[mode]["animation_end"].as_u64())
                .context("animation end")
        };
        for mode in ["continuous", "restored"] {
            lifecycle_checks.insert(
                format!("{mode}_art_zero_ignores_constructor_multiplier"),
                ["attached_0", "attached_1", "attached_3", "attached_256"]
                    .iter()
                    .all(|n| end(n, mode).ok() == end("attached_1", mode).ok()),
            );
            lifecycle_checks.insert(
                format!("{mode}_chrono_repeated_animation_lasts_longer"),
                end("attached_chrono_3", mode)? > end("attached_chrono_1", mode)?,
            );
            lifecycle_checks.insert(
                format!("{mode}_constructor_256_wraps_to_same_duration_as_one"),
                end("attached_chrono_256", mode)? == end("attached_chrono_1", mode)?,
            );
        }
    }
    let passed = lifecycle_checks.values().all(|b| *b)
        && cases.iter().all(|c| {
            c["continuous"]["passed"] == true
                && c["restored"]["passed"] == true
                && c["final_outcomes_match"] == true
        });
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":passed,"cases":cases,"lifecycle_checks":lifecycle_checks,"scope":"TS recruit and ordinary frames: idle Do_Action/stage, attached animation owners/loop byte/lifecycle, script/action emergency state and deferred AI sell/hunt. Public queries and existing Serialize only; no state or event injection. Checkpoint immediately compares all listed fields. final_outcomes_match excludes only House strategy_timer and actor stage.timer; their raw values and full_listed_final_states_match remain reported. Existing checkpoint resets Scenario RNG. Not GUI, network, GPU pixels or all-state equivalence."}),
        )?,
    )?;
    ensure!(
        passed,
        "team animation/strategy audit failed; inspect report.json"
    );
    println!("{}", root.join("report.json").display());
    Ok(())
}
