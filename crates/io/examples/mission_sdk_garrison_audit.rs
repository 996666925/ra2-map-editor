//! TS occupiable-building entry and first-attached-building garrison release.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    mission::{self, Kind},
    new_map::NewMapOptions,
};
use ra2_map_io::mission_script::{self, Sources, Worker};
use serde_json::json;
use std::path::PathBuf;
use vera20k_core::{
    map::map_file,
    sim::{
        mission::MissionType,
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(PathBuf::from(args.next().context("new output directory")?))?;
    ensure!(!root.exists(), "请选择新的审计目录");
    std::fs::create_dir_all(&root)?;
    let bytes = NewMapOptions {
        width: 48,
        height: 48,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(bytes.clone())?;
    let mut doc = MapDocument::new(Baseline::parse(bytes.into(), &map)?);
    for house in ["Americans", "Russians"] {
        mission::create(&mut doc, Kind::House, house, house, None)?;
        doc.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    doc.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    doc.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    doc.add_record("Structures", [30, 40], "Americans", "NABNKR")?;
    doc.add_record("Structures", [40, 50], "Americans", "NABNKR")?;
    doc.add_record("Structures", [20, 30], "Americans", "GAPOWR")?;
    doc.add_record("Structures", [43, 55], "Russians", "NAPOWR")?;
    for cell in [[25, 35], [26, 35]] {
        doc.add_record("Infantry", cell, "Americans", "E1")?;
    }
    for cell in [[36, 46], [37, 46]] {
        doc.add_record("Infantry", cell, "Americans", "E2")?;
    }
    doc.commit("garrison fixture");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let mut install = Install::open(&game)?;
    let scene = ra2_map_scene::map::load_scene(
        &mut install,
        root.join("base.map").to_str().context("fixture path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let entry = root.join("mission.ts");
    std::fs::write(
        &entry,
        r#"import {defineMission,event,action,order,TagRepeat} from './sdk';
import {map} from './map.generated';
export default defineMission(map,{id:'garrison',player:'Americans'},m=>{
  m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
  m.configureHouse(m.house('Americans'),{country:m.country('Americans'),playerControl:true,iq:0,allies:[m.house('Americans')]});
  m.configureHouse(m.house('Russians'),{country:m.country('Russians'),iq:0,allies:[m.house('Russians')]});
  const first=m.team('first',{house:m.house('Americans'),waypoint:m.createWaypoint(30,25,35),
    units:[{type:m.unit('E1'),count:2}],orders:[order.enterBattleBunker(),order.waitForever()],full:true,max:1,reinforce:false});
  const second=m.team('second',{house:m.house('Americans'),waypoint:m.createWaypoint(31,36,46),
    units:[{type:m.unit('E2'),count:2}],orders:[order.enterNearestOccupiable(),order.waitForever()],full:true,max:1,reinforce:false});
  m.rule('dispatch',{when:event.sinceScenarioStart(1),once:true,actions:[action.recruit(first),action.recruit(second)]});
  const release=m.rule('release',{when:event.sinceScenarioStart(20),once:false,linked:m.ruleRef('repeat_release'),
    actions:[action.releaseGarrison(),action.setGlobal(m.globalFlag(0)),action.disableRule(m.ruleRef('release'))]});
  m.rule('repeat_release',{when:event.sinceScenarioStart(24),once:false,
    actions:[action.releaseGarrison(),action.setGlobal(m.globalFlag(1)),action.disableRule(m.ruleRef('repeat_release'))]});
  const tag=m.tag('shared_bunkers',{rule:release,repeat:TagRepeat.Repeat});
  m.bindObject(m.object('Structures:0'),tag);m.bindObject(m.object('Structures:1'),tag);
});"#,
    )?;
    mission_script::refresh_types(&entry, &catalog)?;
    let build = Worker::new()?
        .build(&doc, catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?;
    build.build.apply(&mut doc)?;
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(root.join("garrison.map"), &bytes)?;
    let mut runtime = campaign::load(&install, &bytes, 0)?;
    let sim = &runtime.simulation;
    let bunkers: Vec<_> = [[30, 40], [40, 50]]
        .iter()
        .map(|cell| {
            sim.entities()
                .iter_sorted()
                .find_map(|(id, e)| {
                    (sim.interner().resolve(e.type_ref()) == "NABNKR"
                        && [e.position.rx(), e.position.ry()] == *cell)
                        .then_some(id)
                })
                .context("authored bunker")
        })
        .collect::<Result<_>>()?;
    let groups: Vec<Vec<_>> = ["E1", "E2"]
        .iter()
        .map(|name| {
            sim.entities()
                .iter_sorted()
                .filter_map(|(id, e)| (sim.interner().resolve(e.type_ref()) == *name).then_some(id))
                .collect()
        })
        .collect();
    ensure!(groups.iter().all(|g| g.len() == 2), "two members per team");
    let original: Vec<_> = sim
        .entities()
        .iter_sorted()
        .map(|(id, e)| (id, e.owner(), e.health.current()))
        .collect();
    let mut recruited = [false; 2];
    let mut capture_ordered = [false; 2];
    let mut both_garrisoned = false;
    let mut retained_until_release = true;
    let mut first_released = false;
    let mut second_kept_garrison = true;
    let mut repeat_kept_second = true;
    let mut stable_repeat_frames = 0;
    let mut actors_preserved = true;
    let mut detached_from_teams = false;
    let mut detach_frames = [None, None];
    let mut first_occupant_frames = [None, None];
    let mut release_frames = [None, None];
    let mut previous_counts = None;
    let mut timeline = Vec::new();
    for _ in 0..1000 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let sim = &runtime.simulation;
        let flags = sim.global_variables();
        let frame = sim.session().binary_frame();
        let counts: Vec<_> = bunkers
            .iter()
            .map(|id| {
                sim.entities()
                    .get(*id)
                    .map(|e| e.passenger_role.occupants().ids().to_vec())
                    .context("bunker survives")
            })
            .collect::<Result<_>>()?;
        if previous_counts.as_ref() != Some(&counts) {
            timeline.push(json!({"frame":frame,"occupants":counts}));
            previous_counts = Some(counts.clone());
        }
        for (index, group) in groups.iter().enumerate() {
            recruited[index] |= sim
                .team_script_vm()
                .teams_in_order()
                .any(|t| t.members().any(|id| group.contains(&id)));
            capture_ordered[index] |= group.iter().any(|id| {
                sim.entities()
                    .get(*id)
                    .is_some_and(|e| e.mission.effective().known() == Some(MissionType::Capture))
            });
            if recruited[index]
                && !sim
                    .team_script_vm()
                    .teams_in_order()
                    .any(|t| t.members().any(|id| group.contains(&id)))
            {
                detach_frames[index].get_or_insert(frame);
            }
            if !counts[index].is_empty() {
                first_occupant_frames[index].get_or_insert(frame);
            }
        }
        let matching = (0..2).all(|index| {
            counts[index].len() == 2
                && groups[index].iter().all(|id| {
                    counts[index].contains(id)
                        && sim.entities().get(*id).is_some_and(|e| e.in_limbo())
                })
        });
        if !flags[0] {
            if both_garrisoned {
                retained_until_release &= matching;
            }
            both_garrisoned |= matching;
        } else {
            release_frames[0].get_or_insert(frame);
            let first_on_map = counts[0].is_empty()
                && groups[0]
                    .iter()
                    .all(|id| sim.entities().get(*id).is_some_and(|e| !e.in_limbo()));
            first_released |= first_on_map;
            second_kept_garrison &= counts[1].len() == 2
                && groups[1].iter().all(|id| {
                    counts[1].contains(id) && sim.entities().get(*id).is_some_and(|e| e.in_limbo())
                });
            if flags[1] {
                release_frames[1].get_or_insert(frame);
                repeat_kept_second &= first_on_map && counts[1].len() == 2;
                stable_repeat_frames += 1;
            }
        }
        if both_garrisoned {
            detached_from_teams |= groups.iter().flatten().all(|id| {
                !sim.team_script_vm()
                    .teams_in_order()
                    .any(|t| t.members().any(|member| member == *id))
            });
        }
        actors_preserved &= original.iter().all(|(id, owner, health)| {
            sim.entities()
                .get(*id)
                .is_some_and(|e| e.owner() == *owner && e.health.current() == *health)
        });
        if stable_repeat_frames >= 30 {
            break;
        }
    }
    let checks = json!({
        "both_map_teams_recruited":recruited.iter().all(|seen|*seen),
        "both_teams_issued_capture_for_garrison":capture_ordered.iter().all(|seen|*seen),
        "both_bunkers_received_correct_original_members":both_garrisoned,
        "garrisons_retained_until_release":both_garrisoned&&retained_until_release,
        "entered_members_detached_from_teams":detached_from_teams,
        "both_groups_detached_before_arrival":(0..2).all(|index|
            detach_frames[index].zip(first_occupant_frames[index]).is_some_and(|(detached,entered)|detached<entered)),
        "first_attached_bunker_released":first_released,
        "second_attached_bunker_kept_garrison":second_kept_garrison,
        "repeat_release_still_selects_empty_first_bunker":release_frames[1].is_some()&&repeat_kept_second&&stable_repeat_frames>=30,
        "all_initial_entities_keep_identity_owner_and_health":actors_preserved
    });
    let passed = checks.as_object().unwrap().values().all(|v| v == true);
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "passed":passed,"checks":checks,"occupant_timeline":timeline,"release_frames":release_frames,
            "detach_frames":detach_frames,"first_occupant_frames":first_occupant_frames,
            "scope":"TS build, campaign bootstrap and ordinary frames; no direct state mutation, notification injection or engine changes; no GUI claim"
        }))?,
    )?;
    ensure!(passed, "garrison audit failed; see report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
