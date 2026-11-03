//! Shared TS Tags: notification, detachment and headless checkpoint lifecycle.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    new_map::NewMapOptions,
};
use ra2_map_io::mission_script::{self, Sources, Worker};
use std::path::{Path, PathBuf};
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

fn build(install: &mut Install, root: &Path, repeat: &str) -> Result<Vec<u8>> {
    let bytes = NewMapOptions {
        width: 48,
        height: 48,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(bytes.clone())?;
    let mut doc = MapDocument::new(Baseline::parse(bytes.into(), &map)?);
    doc.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    doc.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    for house in ["Americans", "Russians"] {
        ra2_map_document::mission::create(
            &mut doc,
            ra2_map_document::mission::Kind::House,
            house,
            house,
            None,
        )?;
        doc.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    doc.add_record("Infantry", [20, 30], "Americans", "ENGINEER")?;
    doc.add_record("Infantry", [22, 30], "Americans", "SENGINEER")?;
    doc.commit("all attachments fixture");
    let base = doc.snapshot().bytes()?;
    std::fs::write(root.join("base.map"), &base)?;
    let scene = ra2_map_scene::map::load_scene(
        install,
        root.join("base.map").to_str().context("fixture path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let source = r#"import {defineMission,event,action,order,TagRepeat} from './sdk';
import {map} from './map.generated';
export default defineMission(map,{id:'all_attachments',player:'Americans'},m=>{
  m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
  m.configureHouse(m.house('Americans'),{country:m.country('Americans'),playerControl:true,allies:[m.house('Americans')]});
  m.configureHouse(m.house('Russians'),{country:m.country('Russians'),allies:[m.house('Russians')]});
  const first=m.team('first',{house:m.house('Americans'),waypoint:m.createWaypoint(30,20,30),
    units:[{type:m.unit('ENGINEER'),count:1}],orders:[order.killMembers()],full:true,max:1,reinforce:false});
  const second=m.team('second',{house:m.house('Americans'),waypoint:m.createWaypoint(31,22,30),
    units:[{type:m.unit('SENGINEER'),count:1}],orders:[order.killMembers()],full:true,max:1,reinforce:false});
  const all=m.tag('all_engineers',{rule:m.ruleRef('last_object_gone'),repeat:TagRepeat.REPEAT_MODE});
  m.bindObject(m.object('Infantry:0'),all);
  m.bindObject(m.object('Infantry:1'),all);
  m.rule('last_object_gone',{when:event.destroyedAny(m.object('Infantry:0')),once:ONCE,
    actions:[action.setGlobal(m.globalFlag(0))]});
  m.rule('dispatch_first',{when:event.sinceEnabled(1),once:true,actions:[action.recruit(first)]});
  m.rule('clear_observation',{when:event.sinceEnabled(6),once:true,
    actions:[action.clearGlobal(m.globalFlag(0)),action.setGlobal(m.globalFlag(2))]});
  m.rule('dispatch_second',{when:event.sinceEnabled(10),once:true,
    actions:[action.setGlobal(m.globalFlag(1)),action.recruit(second)]});
});"#
        .replace("REPEAT_MODE", repeat)
        .replace("ONCE", if repeat == "Repeat" { "false" } else { "true" });
    let entry = root.join("mission.ts");
    std::fs::write(&entry, source)?;
    mission_script::refresh_types(&entry, &catalog)?;
    let worker = Worker::new()?;
    let build = worker
        .build(&doc, catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?;
    build.build.apply(&mut doc)?;
    let map_bytes = doc.snapshot().bytes()?;
    std::fs::write(root.join("all-attachments.map"), &map_bytes)?;
    Ok(map_bytes)
}

fn engineers(runtime: &SimRuntime) -> Result<(u64, u64)> {
    let americans = runtime
        .simulation
        .interner()
        .get("Americans")
        .context("Americans")?;
    let engineers: Vec<_> = runtime
        .simulation
        .entities()
        .iter_sorted()
        .filter_map(|(id, entity)| {
            (matches!(
                runtime.simulation.interner().resolve(entity.type_ref()),
                "ENGINEER" | "SENGINEER"
            ) && entity.owner() == americans)
                .then_some((id, runtime.simulation.object_coords_cell(id)))
        })
        .collect();
    ensure!(
        engineers.len() == 2,
        "expected two authored Engineers: {engineers:?}"
    );
    let first = engineers
        .iter()
        .find(|(_, cell)| *cell == Some((20, 30)))
        .map(|(id, _)| *id)
        .context("first Engineer")?;
    let second = engineers
        .iter()
        .find(|(_, cell)| *cell == Some((22, 30)))
        .map(|(id, _)| *id)
        .context("second Engineer")?;
    Ok((first, second))
}

type Observation = (u64, bool, bool, [bool; 3]);

struct Run {
    report: serde_json::Value,
    observations: Vec<Observation>,
}

fn run(install: &Install, bytes: &[u8], repeat: &str, save: Option<&Path>) -> Result<Run> {
    let mut runtime = campaign::load(install, bytes, 0)?;
    let (first, second) = engineers(&runtime)?;
    let mut early_action = false;
    let mut waiting_frames = 0;
    let mut second_dispatch_frame = None;
    let mut action_frames = Vec::new();
    let mut clear_frame = None;
    let mut checkpoint_bytes = None;
    let mut observations = Vec::new();
    let mut previous_action = false;
    let mut premature_second_action = false;
    for frame in 1..=240 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let globals = runtime.simulation.global_variables();
        let first_present = runtime.simulation.entities().get(first).is_some();
        let second_present = runtime.simulation.entities().get(second).is_some();
        observations.push((
            runtime.simulation.session().tick(),
            first_present,
            second_present,
            [globals[0], globals[1], globals[2]],
        ));
        if globals[1] {
            second_dispatch_frame.get_or_insert(frame);
        } else {
            // Check every frame until the independently scheduled second
            // recruitment. Entity removal and Tag detachment are separate
            // lifecycle steps, so don't infer either from a single frame.
            early_action |= globals[0];
            premature_second_action |= globals[2] && globals[0];
        }
        if globals[0] && !previous_action {
            action_frames.push(frame);
        }
        previous_action = globals[0];
        if globals[2] {
            clear_frame.get_or_insert(frame);
        }
        if !globals[1] && !first_present && second_present {
            waiting_frames += 1;
        }
        if frame == 75 {
            ensure!(
                !first_present && second_present && !globals[1] && !globals[2],
                "checkpoint must lie between the two object lifecycles and before TS clears the observation"
            );
            if let Some(path) = save {
                checkpoint_bytes = Some(checkpoint::restore_checkpoint(&mut runtime, bytes, path)?);
            }
        }
    }
    let first_removed = runtime.simulation.entities().get(first).is_none();
    let second_removed = runtime.simulation.entities().get(second).is_none();
    let action_after_last_detach = runtime.simulation.global_variables()[0];
    // Observe successful callbacks, then have a separate TS timer clear the
    // marker. This distinguishes a second callback from the first one's latch.
    let first_callback_correct = early_action == (repeat != "AllAttachments");
    let second_callback_correct = action_after_last_detach == (repeat != "Once");
    let marker_transitions_correct = action_frames.len() == if repeat == "Repeat" { 2 } else { 1 };
    let callbacks_in_order = action_frames.iter().all(|&action| {
        if repeat == "AllAttachments" || clear_frame.is_some_and(|clear| action > clear) {
            second_dispatch_frame.is_some_and(|dispatch| action >= dispatch)
        } else {
            clear_frame.is_some_and(|clear| action < clear)
        }
    });
    let passed = waiting_frames >= 90
        && first_callback_correct
        && second_callback_correct
        && marker_transitions_correct
        && callbacks_in_order
        && !premature_second_action
        && clear_frame.is_some()
        && first_removed
        && second_removed;
    Ok(Run {
        observations,
        report: serde_json::json!({
            "passed": passed,
            "repeat": repeat,
            "first_callback_correct": first_callback_correct,
            "second_callback_correct": second_callback_correct,
            "marker_transitions_correct": marker_transitions_correct,
            "callbacks_in_order": callbacks_in_order,
            "no_callback_between_ts_clear_and_second_dispatch": !premature_second_action,
            "first_and_second_objects_removed": first_removed && second_removed,
            "frames_waiting_after_first_removal_before_second_dispatch": waiting_frames,
            "second_dispatch_frame": second_dispatch_frame,
            "clear_observation_frame": clear_frame,
            "action_frames": action_frames,
            "checkpoint_frame": save.map(|_|75),
            "checkpoint_bytes": checkpoint_bytes,
            "observed_frames":240,
        }),
    })
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(PathBuf::from(args.next().context("new output directory")?))?;
    ensure!(!root.exists(), "请选择新的审计目录");
    std::fs::create_dir_all(&root)?;
    let mut install = Install::open(&game)?;
    let mut cases = Vec::new();
    for repeat in ["Once", "AllAttachments", "Repeat"] {
        let directory = root.join(repeat);
        std::fs::create_dir_all(&directory)?;
        let bytes = build(&mut install, &directory, repeat)?;
        let continuous = run(&install, &bytes, repeat, None)?;
        let resumed = run(
            &install,
            &bytes,
            repeat,
            Some(&directory.join("checkpoint.bin")),
        )?;
        let same_timeline = continuous.observations == resumed.observations;
        std::fs::write(
            directory.join("observations.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "columns":["tick","first_present","second_present","flags_action_dispatch_clear"],
                "continuous": continuous.observations, "resumed":resumed.observations,
            }))?,
        )?;
        cases.push(serde_json::json!({
            "repeat": repeat,
            "passed": continuous.report["passed"]==true && resumed.report["passed"]==true && same_timeline,
            "continuous":continuous.report,"resumed":resumed.report,
            "all_240_observed_frames_match":same_timeline,
        }));
    }
    let passed = cases.iter().all(|case| case["passed"] == true);
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "passed":passed,"cases":cases,
            "scope":"TS build, production campaign bootstrap and ordinary frames; checkpoint uses public hierarchy/navigation validators and the existing resource owner. No GUI save loading or full-state equivalence claim.",
        }))?,
    )?;
    println!("{}", root.join("report.json").display());
    ensure!(passed, "shared Tag lifecycle audit failed; see report.json");
    Ok(())
}
