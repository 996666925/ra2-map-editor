//! TS team Chronosphere orders, real paired launches and teleport lifecycle.
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
        world::{FrameEffects, SimSoundEvent, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;

fn launches(name: &str) -> bool {
    matches!(
        name,
        "building_nearest"
            | "building_farthest"
            | "building_near_charge"
            | "building_power_recover"
            | "quarry_power"
            | "quarry_unarmed"
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
    doc.add_record("Structures", [28, 52], "Americans", "GAPOWR")?;
    for at in [[64, 66], [82, 82]] {
        doc.add_record("Structures", at, "Russians", "NAPOWR")?;
    }
    for at in [[40, 60], [41, 60]] {
        doc.add_record("Units", at, "Americans", "MTNK")?;
    }
    doc.add_record("Units", [54, 70], "Russians", "HTK")?;
    let low_power = matches!(name, "building_low_power" | "building_power_recover");
    if low_power {
        doc.add_record("Structures", [30, 54], "Americans", "NAHAND")?;
        doc.set_ini("GAPOWR", "Power", Some(b"100".to_vec()))?;
        doc.set_ini("NAHAND", "Power", Some(b"-150".to_vec()))?;
    }
    for key in ["Primary", "Secondary"] {
        doc.set_ini("HTK", key, Some(b"none".to_vec()))?;
    }
    doc.set_ini("HTK", "Speed", Some(b"0".to_vec()))?;
    if !name.starts_with("quarry_") || name == "quarry_unarmed" {
        for key in ["Primary", "Secondary"] {
            doc.set_ini("MTNK", key, Some(b"none".to_vec()))?;
        }
    } else {
        // Declare the actual weapon rather than assuming the install's gun name.
        for (section, key, value) in [
            ("MTNK", "Primary", "SDKChronoProbe"),
            ("MTNK", "Secondary", "none"),
            ("SDKChronoProbe", "Damage", "0"),
            ("SDKChronoProbe", "Projectile", "InvisibleLow"),
            ("SDKChronoProbe", "Speed", "100"),
            ("SDKChronoProbe", "ROF", "30"),
            ("SDKChronoProbe", "Range", "5"),
            ("SDKChronoProbe", "Warhead", "SDKChronoWH"),
            (
                "SDKChronoWH",
                "Verses",
                "100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%",
            ),
        ] {
            doc.set_ini(section, key, Some(value.as_bytes().to_vec()))?;
        }
    }
    doc.commit("Team Chronosphere fixture");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(
        install,
        root.join("base.map").to_str().context("fixture path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let order = if name.starts_with("quarry_") {
        format!(
            "order.chronoshiftToQuarry(Quarry.{})",
            if name == "quarry_absent" {
                "Factories"
            } else {
                "Power"
            }
        )
    } else {
        format!(
            "order.chronoshiftToBuilding(m.building(Building.{}),BuildingSelection.{})",
            if name == "building_absent" {
                "NAHAND"
            } else {
                "NAPOWR"
            },
            if name == "building_farthest" {
                "Farthest"
            } else {
                "Nearest"
            }
        )
    };
    let grant = if name == "building_no_super" {
        "".to_string()
    } else if low_power {
        "action.grantSuperWeapon(sphere),".to_string()
    } else {
        format!(
            "action.grantSuperWeapon(sphere,false),action.setSuperRecharge(sphere,600),action.setSuperCharge(sphere,{}),",
            if name == "building_far_charge" {
                0
            } else if name == "building_near_charge" {
                80
            } else {
                100
            }
        )
    };
    let source=r#"import {defineMission,event,action,order,BuildingSelection,Quarry} from './sdk';
import {map,House,Country,Unit,Building,SuperWeapon} from './map.generated';
export default defineMission(map,{id:'team_chrono',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),iq:0,playerControl:true,allies:[m.house(House.Americans)]});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),iq:0,allies:[m.house(House.Russians)]});
 const sphere=m.superWeapon(SuperWeapon.ChronoSphereSpecial);
 const team=m.team('main',{house:m.house(House.Americans),waypoint:m.createWaypoint(30,40,60),units:[{type:m.unit(Unit.MTNK),count:2}],full:true,max:1,reinforce:false,
  orders:[order.guard(4),__ORDER__,order.setGlobal(m.globalFlag(0)),order.waitForever()]});
 m.rule('dispatch',{when:event.sinceScenarioStart(1),owner:m.country(Country.Americans),once:true,actions:[action.revealMap(),__GRANT__action.recruit(team)]});
 __RECOVER__
});"#.replace("__ORDER__",&order).replace("__GRANT__",&grant)
        .replace("__RECOVER__",if name=="building_power_recover" {"m.rule('restore_power',{when:event.sinceScenarioStart(12),owner:m.country(Country.Americans),once:true,actions:[action.createBuilding(m.building(Building.GAPOWR),m.createWaypoint(31,34,54)),action.setGlobal(m.globalFlag(1))]});"}else{""});
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
        .context("main TeamType")?;
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(root.join("compiled.map"), &bytes)?;
    Ok((bytes, team))
}
fn state(runtime: &SimRuntime, team_type: &str) -> Result<Value> {
    let sim = &runtime.simulation;
    let actors=sim.entities().iter_sorted().map(|(id,e)|json!({"id":id,"type":sim.interner().resolve(e.type_ref()),"owner":sim.interner().resolve(e.owner()),"health":e.health.current(),"cell":sim.object_coords_cell(id),"alive":e.lifecycle.object_alive(),"warped_out":e.is_warped_out(),"warping_in":e.is_warping_in(),"locomotor":if sim.interner().resolve(e.type_ref())=="MTNK" {serde_json::to_value(&e.locomotor).unwrap()}else{Value::Null}})).collect::<Vec<_>>();
    let teams = sim
        .team_script_vm()
        .teams_in_order()
        .filter(|t| {
            t.team_type_id()
                .is_some_and(|id| sim.interner().resolve(id) == team_type)
        })
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
    let owner = sim.interner().get("Americans").context("player")?;
    let supers = sim
        .super_weapons()
        .get(&owner)
        .map(|weapons| {
            weapons
                .iter()
                .map(|(id, sw)| json!({"type":sim.interner().resolve(*id),"instance":sw}))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let power = sim.power_states().get(&owner).map(
        |p| json!({"output":p.total_output(),"drain":p.total_drain(),"full":p.total_output()>=p.total_drain() || p.total_drain()<=0}),
    );
    Ok(
        json!({"actors":actors,"teams":teams,"supers":supers,"power":power,"placements":sim.super_placement_anims(),"flags":sim.global_variables()[..2]}),
    )
}
fn sphere(s: &Value) -> Option<&Value> {
    s["supers"]
        .as_array()?
        .iter()
        .find(|sw| sw["type"] == "ChronoSphereSpecial")
        .map(|sw| &sw["instance"])
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
    let members = actors
        .iter()
        .filter(|a| a["type"] == "MTNK")
        .collect::<Vec<_>>();
    ensure!(members.len() == 2, "two original tanks");
    let target = actors
        .iter()
        .filter(|a| a["type"] == "NAPOWR")
        .min_by_key(|a| {
            let x = a["cell"][0].as_i64().unwrap();
            if name == "building_farthest" { -x } else { x }
        })
        .context("destination witness")?;
    let mut selected = None;
    let mut following = None;
    let mut checkpoint_state = None;
    let mut launch_events = Vec::new();
    let mut first_launch = None;
    let mut both_warping = None;
    let mut landed = None;
    let mut trace = Vec::new();
    let mut objects_preserved = true;
    let mut witnesses_preserved = true;
    let mut wait_frames = 0;
    let mut ready_low_power_frames = 0;
    for frame in 0..1200 {
        let output = runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let tick = runtime.simulation.session().tick();
        let current = state(&runtime, team_type)?;
        let now = current["actors"].as_array().unwrap();
        objects_preserved &= actors.iter().all(|a| {
            now.iter().any(|b| {
                a["id"] == b["id"]
                    && a["type"] == b["type"]
                    && a["owner"] == b["owner"]
                    && a["health"] == b["health"]
                    && b["alive"] == true
            })
        });
        witnesses_preserved &= actors
            .iter()
            .filter(|a| a["type"] != "MTNK")
            .all(|a| now.contains(a));
        let tanks = now
            .iter()
            .filter(|a| a["type"] == "MTNK")
            .collect::<Vec<_>>();
        let warping = tanks.len() == 2
            && tanks.iter().all(|a| {
                a["locomotor"]["runtime_payload"]["Teleport"]["chrono"]["latched"] == true
            });
        if warping && both_warping.is_none() {
            both_warping = Some(json!({"tick":tick,"state":current}));
        }
        if launches(name)
            && landed.is_none()
            && tanks.iter().all(|a| {
                distance(&a["cell"], &target["cell"]) <= 49
                    && a["locomotor"]["runtime_payload"]["Teleport"]["chrono"].is_null()
                    && a["locomotor"]["piggyback"].is_null()
                    && a["warped_out"] == false
                    && a["warping_in"] == false
            })
        {
            landed = Some(json!({"tick":tick,"state":current}));
        }
        if let Some(team) = current["teams"].as_array().unwrap().first() {
            if !team["mission_target"].is_null() && selected.is_none() {
                selected = Some(json!({"tick":tick,"state":current}));
            }
            if team["cursor"] == 1 && launch_events.is_empty() {
                wait_frames += 1;
                if current["power"]["full"] == false
                    && sphere(&current).is_some_and(|s| s["is_ready"] == true)
                {
                    ready_low_power_frames += 1;
                }
            }
        }
        let mut launched = false;
        for event in output.sound_events {
            if let SimSoundEvent::SuperWeaponLaunched {
                owner,
                sw_type,
                world,
            } = event
            {
                let sim = &runtime.simulation;
                launch_events.push(json!({"tick":tick,"owner":sim.interner().resolve(owner),"type":sim.interner().resolve(sw_type),"cell":[world.x/256,world.y/256]}));
                launched = true;
            }
        }
        if launched && first_launch.is_none() {
            first_launch = Some(json!({"tick":tick,"state":current}));
        }
        if current["flags"][0] == true && following.is_none() {
            following = Some(json!({"tick":tick,"state":current}));
        }
        if frame % 15 == 0 || launched || warping {
            trace.push(json!({"tick":tick,"state":current}));
        }
        let due = if launches(name) { warping } else { tick == 120 };
        if restore && checkpoint_state.is_none() && due {
            checkpoint::restore_checkpoint(&mut runtime, bytes, &root.join("checkpoint.bin"))?;
            let after = state(&runtime, team_type)?;
            checkpoint_state = Some(
                json!({"tick":tick,"preserved":current==after,"before":current,"after":after}),
            );
        }
    }
    let final_state = state(&runtime, team_type)?;
    let blocked = name == "building_low_power";
    let expected_launches = launches(name);
    let checks = BTreeMap::from([
        (
            "all_original_objects_keep_identity_type_owner_health",
            objects_preserved,
        ),
        ("original_witnesses_unchanged", witnesses_preserved),
        (
            "launch_pair_only_when_admitted",
            launch_events.len() == if expected_launches { 2 } else { 0 },
        ),
        (
            "launch_pair_order_and_owner",
            !expected_launches
                || launch_events[0]["type"] == "ChronoSphereSpecial"
                    && launch_events[1]["type"] == "ChronoWarpSpecial"
                    && launch_events.iter().all(|e| e["owner"] == "Americans")
                    && launch_events[0]["tick"] == launch_events[1]["tick"],
        ),
        (
            "source_is_team_original_area",
            !expected_launches || launch_events[0]["cell"] == json!([40, 60]),
        ),
        (
            "destination_is_selected_building_center",
            !expected_launches || launch_events[1]["cell"] == target["cell"],
        ),
        (
            "correct_original_building_target_retained",
            if expected_launches {
                selected.as_ref().is_some_and(|s| {
                    s["state"]["teams"][0]["mission_target"] == json!({"Object":target["id"]})
                })
            } else {
                selected.is_none()
            },
        ),
        (
            "source_charge_consumed_by_pair",
            !expected_launches
                || first_launch
                    .as_ref()
                    .and_then(|s| sphere(&s["state"]))
                    .is_some_and(|sw| sw["is_ready"] == false),
        ),
        (
            "both_original_tanks_enter_teleport",
            !expected_launches || both_warping.is_some(),
        ),
        (
            "both_original_tanks_finish_teleport_at_target",
            !expected_launches || landed.is_some(),
        ),
        (
            "both_original_locomotor_kinds_restored",
            !expected_launches
                || landed.as_ref().is_some_and(|s| {
                    members.iter().all(|a| {
                        s["state"]["actors"].as_array().unwrap().iter().any(|b| {
                            a["id"] == b["id"] && a["locomotor"]["kind"] == b["locomotor"]["kind"]
                        })
                    })
                }),
        ),
        (
            "no_launch_keeps_original_tanks_at_origin",
            expected_launches
                || members.iter().all(|a| {
                    final_state["actors"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|b| a["id"] == b["id"] && a["cell"] == b["cell"])
                }),
        ),
        (
            "following_line_runs_except_permanent_low_power",
            following.is_some() != blocked,
        ),
        (
            "near_charge_waits_before_launch",
            name != "building_near_charge"
                || wait_frames > 10
                    && first_launch
                        .as_ref()
                        .is_some_and(|s| s["tick"].as_u64().unwrap() > 100),
        ),
        (
            "ready_super_waits_for_power",
            !matches!(name, "building_low_power" | "building_power_recover")
                || ready_low_power_frames > 10,
        ),
        (
            "power_recovery_allows_late_launch",
            name != "building_power_recover"
                || first_launch.as_ref().is_some_and(|s| {
                    s["tick"].as_u64().unwrap() > 180 && s["state"]["power"]["full"] == true
                }),
        ),
        (
            "source_placement_released_after_pair",
            !expected_launches
                || first_launch
                    .as_ref()
                    .is_some_and(|s| s["state"]["placements"] == json!([])),
        ),
        (
            "original_team_members_remain",
            final_state["teams"][0]["members"]
                .as_array()
                .is_some_and(|m| {
                    m.len() == 2 && members.iter().all(|a| m.iter().any(|b| a["id"] == b["id"]))
                }),
        ),
        (
            "checkpoint_preserves_listed_state",
            !restore
                || checkpoint_state
                    .as_ref()
                    .is_some_and(|s| s["preserved"] == true),
        ),
        (
            "checkpoint_covers_active_warp",
            !restore
                || !expected_launches
                || checkpoint_state.as_ref().is_some_and(|s| {
                    s["before"]["actors"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|a| a["type"] == "MTNK")
                        .all(|a| {
                            a["locomotor"]["runtime_payload"]["Teleport"]["chrono"]["latched"]
                                == true
                        })
                }),
        ),
    ]);
    std::fs::write(
        root.join("observations.json"),
        serde_json::to_vec_pretty(&trace)?,
    )?;
    Ok(
        json!({"passed":checks.values().all(|b|*b),"checks":checks,"initial":initial,"final":final_state,"selected":selected,"following":following,"launch_events":launch_events,"first_launch":first_launch,"both_warping":both_warping,"landed":landed,"checkpoint":checkpoint_state,"waiting_frames":wait_frames,"ready_low_power_frames":ready_low_power_frames}),
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
        "building_nearest",
        "building_farthest",
        "building_absent",
        "building_no_super",
        "building_far_charge",
        "building_near_charge",
        "building_low_power",
        "building_power_recover",
        "quarry_power",
        "quarry_unarmed",
        "quarry_absent",
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
        let stages_match = [
            "selected",
            "following",
            "launch_events",
            "both_warping",
            "landed",
        ]
        .iter()
        .all(|key| continuous[key] == restored[key]);
        cases.push(json!({"case":name,"listed_final_states_match":continuous["final"]==restored["final"],"continuous_ticks_and_stage_states_match":stages_match,"continuous":continuous,"restored":restored}));
    }
    let passed = cases.iter().all(|c| {
        c["continuous"]["passed"] == true
            && c["restored"]["passed"] == true
            && c["listed_final_states_match"] == true
    });
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":passed,"cases":cases,"scope":"TS grants, charge controls, team Chronosphere scripts and normal power recovery through a created plant; paired launches and real teleport of two original tanks. Map-declared zero-damage weapon for armed quarry selection, no weapon for building selection. Public headless checkpoint during active warp or frame 120 for no-launch cases. No target, readiness, power, position, locomotor or membership injection. Not GUI save, multiplayer, all-state equality or native pixel parity."}),
        )?,
    )?;
    ensure!(
        passed,
        "team Chronosphere audit failed; inspect report.json"
    );
    println!("{}", root.join("report.json").display());
    Ok(())
}
