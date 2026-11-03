//! TS random clocks and ambient predicates through real logic polls and saves.
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
use vera20k_core::sim::{
    runtime::SimRuntime,
    trigger_runtime::TriggerEffect,
    world::{FrameEffects, TickLane},
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;
#[path = "support/saved_field.rs"]
mod saved_field;

fn state(runtime: &SimRuntime, ids: &BTreeMap<String, String>) -> Result<Value> {
    let sim = &runtime.simulation;
    let saved = saved_field::read(sim, &["trigger_runtime"])?;
    let definitions = saved["definitions"]["trigger_types"]
        .as_array()
        .context("saved trigger definitions")?;
    let live = saved["triggers"]
        .as_array()
        .context("saved live triggers")?;
    let mut triggers = BTreeMap::new();
    for (name, id) in ids {
        let index = definitions
            .iter()
            .position(|d| d["id"] == *id)
            .context("retained trigger definition")?;
        triggers.insert(
            name,
            live.iter()
                .find(|t| t["type_index"].as_u64() == Some(index as u64))
                .cloned()
                .unwrap_or(Value::Null),
        );
    }
    let actors = sim.entities().iter_sorted().map(|(id,e)|json!({
        "id":id,"type":sim.interner().resolve(e.type_ref()),"owner":sim.interner().resolve(e.owner()),
        "health":e.health.current(),"alive":e.lifecycle.object_alive(),"cell":sim.object_coords_cell(id)
    })).collect::<Vec<_>>();
    let light = sim.session().lighting();
    Ok(json!({"tick":sim.session().tick(),"triggers":triggers,
        "ambient":light.current_ambient(),"target":light.target_ambient(),
        "lighting":light,"globals":sim.global_variables().as_slice(),"actors":actors}))
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
    let bytes = NewMapOptions {
        width: 64,
        height: 64,
        ..Default::default()
    }
    .bytes()?;
    let map = vera20k_core::map::map_file::load_from_bytes(bytes.clone())?;
    let mut base = MapDocument::new(Baseline::parse(bytes.into(), &map)?);
    for house in ["Americans", "Russians"] {
        mission::create(&mut base, Kind::House, house, house, None)?;
        base.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    base.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    base.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    base.add_record("Structures", [70, 70], "Americans", "GAPOWR")?;
    base.add_record("Structures", [78, 70], "Russians", "NAPOWR")?;
    base.commit("clock and ambient fixture");
    let mut install = Install::open(&game)?;
    let worker = Worker::new()?;
    let mut cases = Vec::new();
    for name in [
        "random_zero",
        "random_one",
        "random_four",
        "random_reenable",
        "ambient_below",
        "ambient_equal",
        "ambient_above",
        "ambient_fade",
    ] {
        let folder = root.join(name);
        std::fs::create_dir(&folder)?;
        let mut doc = base.fork();
        let path = folder.join("mission.map");
        std::fs::write(&path, doc.snapshot().bytes()?)?;
        let scene =
            ra2_map_scene::map::load_scene(&mut install, path.to_str().context("map path")?)?;
        let catalog = mission_script::catalog(&doc, &scene);
        let random = name.starts_with("random");
        let seconds = match name {
            "random_zero" => 0,
            "random_one" => 1,
            _ => 4,
        };
        let body = if random {
            format!(
                r#"
 const clock=m.rule('clock',{{when:event.randomTime({seconds}),enabled:{enabled},once:false,
  actions:[action.message(m.defineText('fired','fired'))]}});
 {controls}
 m.rule('stop',{{when:event.sinceScenarioStart(20),once:true,
  actions:[action.disableRule(clock),action.message(m.defineText('stopped','stopped'))]}});
"#,
                enabled = name != "random_reenable",
                controls = if name == "random_reenable" {
                    r#"
 m.rule('first_enable',{when:event.sinceScenarioStart(1),once:true,actions:[action.enableRule(clock)]});
 m.rule('disable',{when:event.sinceScenarioStart(2),once:true,actions:[action.disableRule(clock)]});
 m.rule('second_enable',{when:event.sinceScenarioStart(4),once:true,actions:[action.enableRule(clock)]});
"#
                } else {
                    ""
                }
            )
        } else if name == "ambient_fade" {
            r#"
 m.configureLighting({normal:{ambient:1}});
 for(const [id,when] of [
  ['below90',event.ambientAtMost(90)],['below80',event.ambientAtMost(80)],
  ['above110',event.ambientAtLeast(110)],
 ] as const)m.rule(id,{when,once:true,actions:[action.message(m.defineText(id,id))]});
 m.rule('fade_down',{when:event.sinceScenarioStart(1),once:true,
  actions:[action.setAmbientRate(0.01),action.setAmbientStep(0.05),action.setAmbient(80)]});
 m.rule('fade_up',{when:event.sinceScenarioStart(5),once:true,actions:[action.setAmbient(110)]});
"#
            .to_string()
        } else {
            let initial = match name {
                "ambient_below" => 0.94,
                "ambient_above" => 0.96,
                _ => 0.95,
            };
            format!(
                r#"
 m.configureLighting({{normal:{{ambient:{initial}}}}});
 for(const [id,when] of [['below',event.ambientAtMost(95)],['above',event.ambientAtLeast(95)]] as const)
  m.rule(id,{{when,once:true,actions:[action.message(m.defineText(id,id))]}});
"#
            )
        };
        let source = format!(
            r#"import {{defineMission,event,action}} from './sdk';import {{map,House,Country}} from './map.generated';
export default defineMission(map,{{id:'clock_{name}',player:House.Americans}},m=>{{
 m.configureBasic({{ignoreGlobalAiTriggers:true,skipScore:true}});
 m.configureHouse(m.house(House.Americans),{{country:m.country(Country.Americans),playerControl:true,iq:0,allies:[m.house(House.Americans)]}});
 {body}
}});"#
        );
        let entry = folder.join("mission.ts");
        std::fs::write(&entry, source)?;
        mission_script::refresh_types(&entry, &catalog)?;
        worker
            .build(&doc, catalog.clone(), Sources::read(&entry)?)?
            .recv()?
            .map_err(anyhow::Error::msg)?
            .build
            .apply(&mut doc)?;
        let unchanged = !worker
            .build(&doc, catalog, Sources::read(&entry)?)?
            .recv()?
            .map_err(anyhow::Error::msg)?
            .build
            .apply(&mut doc)?;
        let ids = mission::ids(&doc, Kind::Trigger)
            .into_iter()
            .map(|id| (mission::csv(&doc.ini_text("Triggers", &id))[2].clone(), id))
            .collect::<BTreeMap<_, _>>();
        ensure!(
            !ids.is_empty() && ids.keys().all(|name| !name.is_empty()),
            "named triggers"
        );
        let bytes = doc.snapshot().bytes()?;
        std::fs::write(&path, &bytes)?;
        let texts = mission_script::declared_texts(&bytes)?;
        let mut runs = Vec::new();
        let mut reference = None;
        let mut first_restored = None;
        for run in ["continuous", "restored_a", "restored_b"] {
            let mut runtime = campaign::load(&install, &bytes, 1)?;
            let mut observations = vec![state(&runtime, &ids)?];
            let mut messages = Vec::new();
            let mut checkpoint_state = Value::Null;
            for tick in 1..=400 {
                let output =
                    runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
                for effect in output.trigger_effects {
                    if let TriggerEffect::LocalizedMessage { csf_label } = effect {
                        messages.push(json!({"tick":tick,"event":texts.get(&csf_label).context("declared message")?}));
                    }
                }
                let current = state(&runtime, &ids)?;
                if run != "continuous" && tick == 20 {
                    checkpoint::restore_checkpoint(
                        &mut runtime,
                        &bytes,
                        &folder.join(format!("{run}-20.bin")),
                    )?;
                    let after = state(&runtime, &ids)?;
                    checkpoint_state =
                        json!({"preserved":current==after,"before":current,"after":after});
                }
                observations.push(current);
            }
            let mut checks = BTreeMap::from([
                ("unchanged_rebuild", unchanged),
                (
                    "checkpoint_preserves_listed_state",
                    run == "continuous" || checkpoint_state["preserved"] == true,
                ),
                (
                    "witness_objects_unchanged",
                    observations
                        .iter()
                        .all(|r| r["actors"] == observations[0]["actors"]),
                ),
            ]);
            let times = |event: &str| {
                messages
                    .iter()
                    .filter(|m| m["event"] == event)
                    .map(|m| m["tick"].as_u64().unwrap() as usize)
                    .collect::<Vec<_>>()
            };
            if random {
                let fired = times("fired");
                let timers = observations
                    .iter()
                    .filter_map(|r| r["triggers"]["clock"]["timer"]["duration"].as_i64())
                    .collect::<Vec<_>>();
                checks.insert(
                    "all_draws_in_inclusive_range_and_seconds_scale",
                    !timers.is_empty()
                        && timers.iter().all(|d| {
                            *d % 15 == 0
                                && *d >= (seconds / 2) * 15
                                && *d <= (seconds / 2 + seconds) * 15
                        }),
                );
                checks.insert("multiple_rounds_actually_fire", fired.len() >= 3);
                checks.insert(
                    "no_firing_before_prior_timer_expires",
                    fired.iter().all(|&t| {
                        let prior = &observations[t - 1]["triggers"]["clock"];
                        prior["enabled"] == true
                            && prior["timer"]["start_frame"]
                                .as_i64()
                                .zip(prior["timer"]["duration"].as_i64())
                                .is_some_and(|(start, duration)| t as i64 > start + duration)
                    }),
                );
                checks.insert(
                    "each_round_reanchors_at_dispatch",
                    fired.iter().all(|&t| {
                        observations[t]["triggers"]["clock"]["timer"]["start_frame"].as_u64()
                            == Some(t as u64 - 1)
                    }),
                );
                checks.insert(
                    "stopped_at_301_and_silent_after",
                    times("stopped") == vec![301]
                        && fired.iter().all(|&t| t <= 301)
                        && observations[301..]
                            .iter()
                            .all(|r| r["triggers"]["clock"]["enabled"] == false),
                );
                if seconds == 0 {
                    checks.insert(
                        "zero_duration_dispatches_each_logic_frame",
                        fired.iter().copied().eq(1..=300),
                    );
                } else {
                    checks.insert(
                        "multiple_distinct_durations_observed",
                        timers.iter().min() != timers.iter().max(),
                    );
                }
                if name == "random_reenable" {
                    checks.insert(
                        "no_dispatch_during_initial_and_disabled_intervals",
                        fired.iter().all(|&t| t >= 91),
                    );
                    checks.insert(
                        "enable_reanchors_and_disable_keeps_timer",
                        observations[16]["triggers"]["clock"]["timer"]["start_frame"] == 15
                            && observations[31]["triggers"]["clock"]["timer"]
                                == observations[30]["triggers"]["clock"]["timer"]
                            && observations[61]["triggers"]["clock"]["timer"]["start_frame"] == 60,
                    );
                }
            } else if name == "ambient_fade" {
                for (label, bound, below) in [
                    ("below90", 90, true),
                    ("below80", 80, true),
                    ("above110", 110, false),
                ] {
                    let event_times = times(label);
                    let expected = observations
                        .iter()
                        .position(|r| {
                            r["ambient"]
                                .as_i64()
                                .is_some_and(|v| if below { v <= bound } else { v >= bound })
                        })
                        .map(|t| t + 1);
                    checks.insert(
                        label,
                        event_times.len() == 1 && expected == event_times.first().copied(),
                    );
                }
                checks.insert(
                    "target_changes_before_current_reaches_it",
                    observations[16]["target"] == 80
                        && observations[16]["ambient"] != 80
                        && observations[76]["target"] == 110
                        && observations[76]["ambient"] != 110,
                );
                checks.insert(
                    "fade_reaches_both_requested_targets",
                    observations.iter().any(|r| r["ambient"] == 80)
                        && observations[400]["ambient"] == 110,
                );
                checks.insert(
                    "checkpoint_during_pending_fade",
                    observations[20]["ambient"] != observations[20]["target"],
                );
            } else {
                let current = observations[0]["ambient"]
                    .as_i64()
                    .context("initial ambient")?;
                checks.insert(
                    "below_includes_equality",
                    if current <= 95 {
                        let ticks = times("below");
                        ticks.len() == 1 && ticks[0] <= 2
                    } else {
                        times("below").is_empty()
                    },
                );
                checks.insert(
                    "above_includes_equality",
                    if current >= 95 {
                        let ticks = times("above");
                        ticks.len() == 1 && ticks[0] <= 2
                    } else {
                        times("above").is_empty()
                    },
                );
                checks.insert(
                    "no_lighting_mutation_from_predicates",
                    observations
                        .iter()
                        .all(|r| r["lighting"] == observations[0]["lighting"]),
                );
            }
            let listed = json!({"observations":observations,"messages":messages});
            let same_continuous = reference.as_ref().is_none_or(|r| *r == listed);
            if !random || seconds == 0 {
                checks.insert("draw_free_timeline_matches_continuous", same_continuous);
            }
            if run != "continuous" && random && name != "random_reenable" {
                let next = |value: &Value| {
                    value["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|m| {
                            m["event"] == "fired" && m["tick"].as_u64().is_some_and(|t| t > 20)
                        })
                        .cloned()
                };
                checks.insert(
                    "already_drawn_next_expiry_is_preserved",
                    next(&listed) == next(reference.as_ref().unwrap()),
                );
            }
            if run == "continuous" {
                reference = Some(listed.clone());
            } else if run == "restored_a" {
                first_restored = Some(listed.clone());
            } else {
                checks.insert(
                    "independent_restorations_match",
                    first_restored.as_ref() == Some(&listed),
                );
            }
            std::fs::write(
                folder.join(format!("{run}.json")),
                serde_json::to_vec_pretty(&listed)?,
            )?;
            runs.push(json!({"name":run,"passed":checks.values().all(|v|*v),"checks":checks,"continuous_listed_timeline_match":same_continuous,"messages":messages,"checkpoint":checkpoint_state}));
        }
        cases.push(json!({"case":name,"passed":runs.iter().all(|r|r["passed"]==true),"runs":runs}));
        std::fs::write(
            root.join("report.json"),
            serde_json::to_vec_pretty(
                &json!({"passed":cases.iter().all(|c|c["passed"]==true),"cases":cases,"scope":"TS-authored random timer reset/enable/disable and ambient equality/fade through ordinary frames, public checkpoint at20. Serialized fields read only; no RNG, timer, lighting or notification injection. No GUI, networking, native execution or full-state equality claim."}),
            )?,
        )?;
    }
    ensure!(
        cases.iter().all(|c| c["passed"] == true),
        "clock/ambient audit failed; see report.json"
    );
    println!("{}", root.join("report.json").display());
    Ok(())
}
