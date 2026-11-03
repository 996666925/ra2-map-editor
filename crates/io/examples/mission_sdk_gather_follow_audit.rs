//! TS gather/regroup/follow orders through normal diplomacy and movement.
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
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;

fn has_enemy(name: &str) -> bool {
    !name.starts_with("follow_") && !name.ends_with("no_enemy")
}
fn no_target(name: &str) -> bool {
    matches!(
        name,
        "gather_no_enemy" | "gather_enemy_no_base" | "follow_none" | "follow_outbound"
    )
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
    doc.set_ini("General", "AISafeDistance", Some(b"8".to_vec()))?;
    if name != "gather_no_own_base" && name != "regroup_no_base" {
        doc.add_record("Structures", [28, 52], "Americans", "GAPOWR")?;
    }
    if name != "gather_enemy_no_base" {
        doc.add_record("Structures", [80, 80], "Russians", "NAPOWR")?;
    } else {
        doc.add_record("Infantry", [80, 80], "Russians", "SENGINEER")?;
    }
    let origin = if name == "gather_no_own_base" {
        [90, 60]
    } else {
        [40, 56]
    };
    doc.add_record("Units", origin, "Americans", "MTNK")?;
    match name {
        "follow_mobile" => {
            doc.add_record("Units", [55, 60], "Americans", "HTK")?;
        }
        "follow_infantry" => {
            doc.add_record("Infantry", [50, 58], "Americans", "ENGINEER")?;
        }
        "follow_nearest" => {
            doc.add_record("Units", [65, 70], "Americans", "HTK")?;
            doc.add_record("Infantry", [50, 58], "Americans", "ENGINEER")?;
        }
        "follow_inbound" | "follow_outbound" => {
            doc.add_record("Units", [55, 60], "Russians", "HTK")?;
        }
        _ => {}
    }
    for ty in ["MTNK", "HTK"] {
        for key in ["Primary", "Secondary"] {
            doc.set_ini(ty, key, Some(b"none".to_vec()))?;
        }
    }
    doc.commit("Gather and follow fixture");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(
        install,
        root.join("base.map").to_str().context("fixture path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let command = if name.starts_with("regroup_") {
        "regroupAtBase"
    } else if name.starts_with("gather_") {
        "gatherAtEnemyBase"
    } else {
        "followFriendlies"
    };
    let source=r#"import {defineMission,event,action,order} from './sdk';
import {map,House,Country,Unit} from './map.generated';
export default defineMission(map,{id:'gather_follow',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),iq:0,playerControl:true,allies:[m.house(House.Americans)__OUTBOUND__]});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),iq:0,allies:[m.house(House.Russians)__INBOUND__]});
 __BASES__
 const team=m.team('main',{house:m.house(House.Americans),waypoint:m.createWaypoint(30,__START_X__,__START_Y__),units:[{type:m.unit(Unit.MTNK),count:1}],max:1,full:true,reinforce:false,
  orders:[order.guard(4),order.__COMMAND__(),order.setGlobal(m.globalFlag(0)),order.waitForever()]});
 m.rule('dispatch',{when:event.sinceScenarioStart(1),owner:m.country(Country.Americans),once:true,actions:[__ENEMY__action.recruit(team)]});
});"#.replace("__COMMAND__",command)
        .replace("__START_X__",&origin[0].to_string()).replace("__START_Y__",&origin[1].to_string())
        .replace("__OUTBOUND__",if name=="follow_outbound" {",m.house(House.Russians)"}else{""})
        .replace("__INBOUND__",if name=="follow_inbound" {",m.house(House.Americans)"}else{""})
        .replace("__ENEMY__",if has_enemy(name) {"action.makeEnemy(m.country(Country.Russians)),"}else{""})
        .replace("__BASES__",&format!("{}{}",
            if !name.starts_with("follow_") && name!="gather_no_own_base" && name!="regroup_no_base" {"m.rule('own_base',{when:event.sinceScenarioStart(0),owner:m.country(Country.Americans),once:true,actions:[action.setAlternateBase(m.createWaypoint(31,30,54))]});"}else{""},
            if !name.starts_with("follow_") && name!="gather_enemy_no_base" {"m.rule('enemy_base',{when:event.sinceScenarioStart(0),owner:m.country(Country.Russians),once:true,actions:[action.setAlternateBase(m.createWaypoint(32,82,82))]});"}else{""}));
    let entry = root.join("mission.ts");
    std::fs::write(&entry, source)?;
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
        .context("generated main team")?;
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(root.join("compiled.map"), &bytes)?;
    Ok((bytes, team))
}
fn state(runtime: &SimRuntime, team_type: &str) -> Result<Value> {
    let sim = &runtime.simulation;
    let actors=sim.entities().iter_sorted().map(|(id,e)|json!({"id":id,"type":sim.interner().resolve(e.type_ref()),"owner":sim.interner().resolve(e.owner()),"health":e.health.current(),"cell":sim.object_coords_cell(id),"alive":e.lifecycle.object_alive()})).collect::<Vec<_>>();
    let teams = sim
        .team_script_vm()
        .teams_in_order()
        .filter(|t| {
            t.team_type_id()
                .is_some_and(|id| sim.interner().resolve(id) == team_type)
        })
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
    let houses=sim.houses().order().iter().map(|id| {
        let h=&sim.houses()[id];
        json!({"id":sim.interner().resolve(*id),"enemy":h.enemy_house().map(|enemy|sim.interner().resolve(enemy)),"base":h.base_center(),"alternate":h.alternate_base_center(),"unit_losses":h.stats().units_lost(),"building_losses":h.stats().buildings_lost()})
    }).collect::<Vec<_>>();
    let americans = sim.interner().get("Americans").context("player id")?;
    let russians = sim.interner().get("Russians").context("other id")?;
    let alliances = json!({"outbound":sim.house_alliances().is_allied(sim.interner(),americans,russians),"inbound":sim.house_alliances().is_allied(sim.interner(),russians,americans)});
    Ok(
        json!({"actors":actors,"teams":teams,"houses":houses,"alliances":alliances,"flag":sim.global_variables()[0]}),
    )
}
fn distance(a: &Value, b: &Value) -> i64 {
    (a[0].as_i64().unwrap() - b[0].as_i64().unwrap()).pow(2)
        + (a[1].as_i64().unwrap() - b[1].as_i64().unwrap()).pow(2)
}
fn audit(
    install: &Install,
    bytes: &[u8],
    root: &Path,
    name: &str,
    team_type: &str,
    restore: bool,
) -> Result<Value> {
    std::fs::create_dir_all(root)?;
    let mut runtime = campaign::load(install, bytes, 1)?;
    let initial = state(&runtime, team_type)?;
    let actors = initial["actors"].as_array().unwrap();
    let mover = actors
        .iter()
        .find(|a| a["type"] == "MTNK")
        .context("original tank")?;
    let followed = actors.iter().find(|a| {
        a["type"]
            == if name == "follow_infantry" || name == "follow_nearest" {
                "ENGINEER"
            } else {
                "HTK"
            }
    });
    let mut selected = None;
    let mut following = None;
    let mut checkpoint_state = None;
    let mut trace = Vec::new();
    let mut actors_preserved = true;
    let mut witnesses_preserved = true;
    let mut admitted = 0;
    let mut followed_motion = false;
    for frame in 0..1600 {
        let commands = if name == "follow_mobile" && frame == 100 {
            let sim = &runtime.simulation;
            let command = Command::Move {
                entity_id: followed.context("followed vehicle")?["id"]
                    .as_u64()
                    .unwrap(),
                target_rx: 68,
                target_ry: 72,
            };
            vec![CommandEnvelope::synchronized(
                sim.encode_megamission_record(sim.interner().get("Americans").unwrap(), &command)
                    .context("followed move encoding")?,
                sim.session().tick(),
            )]
        } else {
            Vec::new()
        };
        let mut output =
            runtime.advance_frame(&commands, 22, TickLane::Ordinary, FrameEffects::default())?;
        admitted += output.take_admitted_commands().len();
        let tick = runtime.simulation.session().tick();
        let current = state(&runtime, team_type)?;
        let now = current["actors"].as_array().unwrap();
        actors_preserved &= actors.iter().all(|a| {
            now.iter().any(|b| {
                a["id"] == b["id"]
                    && a["owner"] == b["owner"]
                    && a["type"] == b["type"]
                    && a["health"] == b["health"]
                    && b["alive"] == true
            })
        });
        witnesses_preserved &= actors
            .iter()
            .filter(|a| {
                a["id"] != mover["id"]
                    && (name != "follow_mobile" || followed.is_none_or(|f| a["id"] != f["id"]))
            })
            .all(|a| now.contains(a));
        if let Some(team) = current["teams"].as_array().unwrap().first()
            && team["cursor"] == 1
            && !team[if name.starts_with("follow_") {
                "focus"
            } else {
                "mission_target"
            }]
            .is_null()
            && selected.is_none()
        {
            selected = Some(json!({"tick":tick,"state":current}));
        }
        if current["flag"] == true && following.is_none() {
            following = Some(json!({"tick":tick,"state":current}));
        }
        if name == "follow_mobile" && frame > 100 {
            let tank = now
                .iter()
                .find(|a| a["id"] == mover["id"])
                .context("tank survives")?;
            followed_motion |= tank["cell"][0].as_u64().is_some_and(|x| x > 55)
                && current["teams"][0]["cursor"] == 1;
        }
        if frame % 15 == 0 {
            trace.push(json!({"tick":tick,"state":current}));
        }
        if restore && tick == 120 {
            checkpoint::restore_checkpoint(&mut runtime, bytes, &root.join("checkpoint.bin"))?;
            let after = state(&runtime, team_type)?;
            checkpoint_state =
                Some(json!({"preserved":after==current,"before":current,"after":after}));
        }
    }
    let final_state = state(&runtime, team_type)?;
    let team = &final_state["teams"][0];
    let tank = final_state["actors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == mover["id"])
        .context("final original tank")?;
    let target = selected.as_ref().map(|s| {
        &s["state"]["teams"][0][if name.starts_with("follow_") {
            "focus"
        } else {
            "mission_target"
        }]
    });
    let cell = target
        .and_then(|t| t.get("Cell"))
        .map(|c| json!([c["x"], c["y"]]));
    let mut geometric_pick = false;
    if let (Some(s), Some(cell)) = (&selected, &cell) {
        let houses = s["state"]["houses"].as_array().unwrap();
        let base = |name| {
            houses
                .iter()
                .find(|h| h["id"] == name)
                .map(|h| {
                    if h["alternate"] != json!([0, 0]) {
                        h["alternate"].clone()
                    } else if h["base"].is_null() {
                        json!([0, 0])
                    } else {
                        h["base"].clone()
                    }
                })
                .unwrap()
        };
        let own = base("Americans");
        let enemy = base("Russians");
        let origin = if name.starts_with("gather_") {
            &enemy
        } else {
            &own
        };
        let toward = if name.starts_with("gather_") {
            if own == json!([0, 0]) {
                &mover["cell"]
            } else {
                &own
            }
        } else {
            &enemy
        };
        let dot = (cell[0].as_i64().unwrap() - origin[0].as_i64().unwrap())
            * (toward[0].as_i64().unwrap() - origin[0].as_i64().unwrap())
            + (cell[1].as_i64().unwrap() - origin[1].as_i64().unwrap())
                * (toward[1].as_i64().unwrap() - origin[1].as_i64().unwrap());
        geometric_pick = (25..=144).contains(&distance(cell, origin))
            && (name == "regroup_no_enemy"
                || dot > 0
                    && dot * dot * 10 >= distance(cell, origin) * distance(toward, origin) * 9);
    }
    let blocked = matches!(name, "follow_none" | "follow_outbound" | "regroup_no_base");
    let checks = BTreeMap::from([
        ("original_id_type_owner_health_preserved", actors_preserved),
        (
            "other_original_objects_remain_at_cells",
            witnesses_preserved,
        ),
        (
            "no_target_cases_do_not_select",
            !no_target(name) || selected.is_none(),
        ),
        ("target_cases_select", no_target(name) || selected.is_some()),
        (
            "gather_geometry_uses_actual_bases",
            name.starts_with("follow_")
                || no_target(name)
                || name == "regroup_no_base"
                || geometric_pick,
        ),
        (
            "follow_selects_expected_original_candidate",
            !name.starts_with("follow_")
                || no_target(name)
                || target == followed.map(|f| json!({"Object":f["id"]})).as_ref(),
        ),
        (
            "follow_uses_focus_without_mission_target",
            !name.starts_with("follow_")
                || selected
                    .as_ref()
                    .is_none_or(|s| s["state"]["teams"][0]["mission_target"].is_null()),
        ),
        (
            "following_line_runs_unless_destination_unavailable",
            following.is_some() != blocked,
        ),
        (
            "no_target_does_not_move_original_tank",
            !(no_target(name) || name == "regroup_no_base") || tank["cell"] == mover["cell"],
        ),
        (
            "gather_reaches_selected_cell",
            name == "regroup_no_base"
                || name.starts_with("follow_")
                || no_target(name)
                || cell
                    .as_ref()
                    .is_some_and(|c| distance(&tank["cell"], c) <= 9),
        ),
        (
            "follow_reaches_selected_friend",
            !name.starts_with("follow_")
                || no_target(name)
                || followed.is_some_and(|f| {
                    final_state["actors"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|a| a["id"] == f["id"])
                        .is_some_and(|a| distance(&tank["cell"], &a["cell"]) <= 16)
                }),
        ),
        (
            "mobile_friend_command_admitted_and_followed",
            name != "follow_mobile" || admitted == 1 && followed_motion,
        ),
        (
            "mobile_friend_arrives_at_commanded_cell",
            name != "follow_mobile"
                || followed.is_some_and(|f| {
                    final_state["actors"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|a| a["id"] == f["id"] && a["cell"] == json!([68, 72]))
                }),
        ),
        (
            "regroup_without_base_retains_zero_cell_target",
            name != "regroup_no_base" || target == Some(&json!({"Cell":{"x":0,"y":0}})),
        ),
        (
            "team_keeps_original_member",
            team["members"]
                .as_array()
                .is_some_and(|m| m.len() == 1 && m[0]["id"] == mover["id"]),
        ),
        (
            "checkpoint_preserves_listed_state",
            !restore
                || checkpoint_state
                    .as_ref()
                    .is_some_and(|s| s["preserved"] == true),
        ),
        (
            "checkpoint_covers_active_order_unless_gather_skips",
            !restore
                || matches!(name, "gather_no_enemy" | "gather_enemy_no_base")
                || checkpoint_state
                    .as_ref()
                    .is_some_and(|s| s["before"]["teams"][0]["cursor"] == 1),
        ),
        (
            "designated_enemy_from_ts_diplomacy",
            final_state["houses"]
                .as_array()
                .unwrap()
                .iter()
                .find(|h| h["id"] == "Americans")
                .is_some_and(|h| {
                    if has_enemy(name) {
                        h["enemy"] == "Russians"
                    } else {
                        h["enemy"].is_null()
                    }
                }),
        ),
        (
            "directional_alliance_fixture_retained",
            final_state["alliances"]["outbound"] == (name == "follow_outbound")
                && final_state["alliances"]["inbound"] == (name == "follow_inbound"),
        ),
    ]);
    std::fs::write(
        root.join("observations.json"),
        serde_json::to_vec_pretty(&trace)?,
    )?;
    Ok(
        json!({"passed":checks.values().all(|b|*b),"checks":checks,"initial":initial,"final":final_state,"selected":selected,"following":following,"checkpoint":checkpoint_state,"admitted_commands":admitted}),
    )
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(args.next().context("new output directory")?)?;
    ensure!(
        args.next().is_none() && !root.exists(),
        "output directory must be new"
    );
    std::fs::create_dir_all(&root)?;
    let mut install = Install::open(&game)?;
    let mut cases = Vec::new();
    for name in [
        "regroup_enemy",
        "regroup_no_enemy",
        "regroup_no_base",
        "gather_enemy",
        "gather_no_enemy",
        "gather_no_own_base",
        "gather_enemy_no_base",
        "follow_mobile",
        "follow_infantry",
        "follow_nearest",
        "follow_inbound",
        "follow_outbound",
        "follow_none",
    ] {
        let folder = root.join(name);
        let (bytes, team_type) = build(&mut install, &folder, name)?;
        let continuous = audit(
            &install,
            &bytes,
            &folder.join("continuous"),
            name,
            &team_type,
            false,
        )?;
        let restored = audit(
            &install,
            &bytes,
            &folder.join("restored"),
            name,
            &team_type,
            true,
        )?;
        cases.push(json!({"case":name,"listed_final_states_match":continuous["final"]==restored["final"],"continuous_ticks_and_stage_states_match":continuous["selected"]==restored["selected"] && continuous["following"]==restored["following"],"continuous":continuous,"restored":restored}));
    }
    let passed = cases.iter().all(|c| {
        c["continuous"]["passed"] == true
            && c["restored"]["passed"] == true
            && c["listed_final_states_match"] == true
    });
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":passed,"cases":cases,"scope":"TS recruit, diplomacy MakeEnemy, gather/regroup/follow via ordinary frames and one normal encoded Move for the followed friend. Public headless checkpoint at frame 120. No target, enemy, base, membership, position or navigation injection. Not tube traversal, attack mirroring, GUI save, multiplayer or whole-state equality."}),
        )?,
    )?;
    ensure!(passed, "gather/follow audit failed; inspect report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
