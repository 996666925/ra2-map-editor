//! Readonly SDK observations of an actual LAN recording, through strict replay.
//! Reuses production reconstruction and playback; never relaxes recorded hashes.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use vera20k_core::{
    headless_scenario::{HeadlessScenario, load_with_launch_and_ini},
    sim::{
        replay::{ReplayLog, ReplayPlayback},
        scenario_bootstrap::MatchLaunchDescriptor,
        world::FrameEffects,
    },
};

fn state(scenario: &HeadlessScenario) -> Value {
    let sim = scenario.sim();
    let objects: Vec<_> = sim
        .entities()
        .iter_sorted()
        .map(|(id, e)| {
            json!({
                "id":id,"type":sim.interner().resolve(e.type_ref()),
                "owner":sim.interner().resolve(e.owner()),"health":e.health.current(),
                "active":e.is_active(),"cell":sim.object_coords_cell(id),
            })
        })
        .collect();
    json!({"tick":sim.session().tick(),"mode":sim.session().game_mode(),
        "starts":sim.session().start_slot_houses().iter().map(|(slot,house)|
            (slot.to_string(),json!(sim.interner().resolve(*house)))).collect::<BTreeMap<_,_>>(),
        "objects":objects,"teams":sim.team_script_vm().teams_in_order().collect::<Vec<_>>(),
        "globals":sim.global_variables().as_slice(),"timer":sim.session().scenario_timer_view()})
}

fn audit(game: PathBuf, path: PathBuf) -> Result<Value> {
    let replay = ReplayLog::load(&path)?;
    let startup = replay.standalone_startup()?;
    let launch = MatchLaunchDescriptor::from_resolved(startup.launch_session().clone())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut loaded = load_with_launch_and_ini(
        &game,
        &replay.header.map_name,
        startup.seed(),
        launch,
        &Default::default(),
        startup.game_mode(),
        Some(startup.native_id_prefix()?),
        None,
    )
    .map_err(anyhow::Error::msg)?;
    loaded.runtime.simulation.batch_path_searches();
    loaded.runtime.simulation.batch_target_scans();
    let mut playback = ReplayPlayback::prepare(
        &mut loaded.runtime,
        Arc::new(replay.clone()),
        &loaded.source,
        loaded.map.ini.content_hash(),
    )?;
    let initial = state(&loaded);
    let mut checks = BTreeMap::new();
    checks.insert(
        "lan_two_occupied_starts",
        initial["mode"] == "Lan"
            && initial["starts"]
                .as_object()
                .context("start mapping")?
                .len()
                == 2
            && !initial["starts"]["0"].is_null()
            && !initial["starts"]["1"].is_null(),
    );
    let original: Vec<_> = initial["objects"]
        .as_array()
        .context("objects")?
        .iter()
        .filter(|e| e["owner"] == "Neutral" && e["type"] == "MTNK")
        .cloned()
        .collect();
    checks.insert("two_original_neutral_tanks", original.len() == 2);
    checks.insert(
        "no_initial_engineers",
        !initial["objects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "ENGINEER"),
    );
    let mut stages = Vec::new();
    let mut births = Vec::new();
    let mut previous = 0;
    let mut at_300 = Value::Null;
    while playback.can_advance() {
        playback.advance(&mut loaded.runtime, FrameEffects::empty())?;
        let tick = loaded.sim().session().tick();
        if tick > 300 {
            continue;
        }
        let current = state(&loaded);
        let engineers: Vec<_> = current["objects"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["type"] == "ENGINEER")
            .cloned()
            .collect();
        if engineers.len() != previous {
            births.push(json!({"tick":tick,"count":engineers.len(),"objects":engineers}));
            previous = engineers.len();
        }
        if [
            1, 2, 45, 46, 50, 61, 75, 76, 90, 91, 105, 106, 120, 140, 300,
        ]
        .contains(&tick)
        {
            stages.push(current.clone());
        }
        if tick == 300 {
            at_300 = current;
        }
    }
    let verification = playback.verification(&loaded.runtime)?;
    ensure!(
        !at_300.is_null(),
        "recording must reach SDK observation tick 300"
    );
    let engineers: Vec<_> = at_300["objects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "ENGINEER")
        .collect();
    checks.insert(
        "three_pairs_and_no_empty_start_team",
        engineers.len() == 6
            && ["0", "1"].iter().all(|slot| {
                engineers
                    .iter()
                    .filter(|e| e["owner"] == initial["starts"][slot])
                    .count()
                    == 3
            }),
    );
    checks.insert(
        "all_gifts_alive_at_300",
        engineers
            .iter()
            .all(|e| e["active"] == true && e["health"].as_i64().is_some_and(|n| n > 0)),
    );
    checks.insert(
        "repeat_31_61_91_then_disabled_through_300",
        births
            .iter()
            .map(|b| (b["tick"].as_u64().unwrap(), b["count"].as_u64().unwrap()))
            .collect::<Vec<_>>()
            == [(31, 2), (61, 4), (91, 6)],
    );
    checks.insert(
        "both_script_successors_and_no_empty_start_flag",
        at_300["globals"][0] == true
            && at_300["globals"][1] == true
            && at_300["globals"][2] == false,
    );
    let timers = [
        (1, Some(449)),
        (45, Some(405)),
        (46, None),
        (50, None),
        (61, None),
        (75, None),
        (76, Some(404)),
        (90, Some(390)),
        (91, Some(419)),
        (105, Some(405)),
        (106, Some(389)),
        (120, Some(375)),
        (140, Some(355)),
        (300, Some(195)),
    ];
    checks.insert(
        "timer_set_stop_resume_extend_shorten",
        timers.iter().all(|(tick, left)| {
            stages
                .iter()
                .find(|s| s["tick"] == *tick)
                .is_some_and(|s| match left {
                    Some(left) => s["timer"] == json!([left, ""]),
                    None => s["timer"].is_null(),
                })
        }),
    );
    // Authored neutral objects retain their IDs. Derive their intended start
    // from the fixture cell rather than the randomized player assignment.
    checks.insert(
        "original_tanks_transferred_and_preserved_at_300",
        original.len() == 2
            && original.iter().all(|before| {
                let slot = match before["cell"] {
                    ref cell if *cell == json!([35, 50]) => "0",
                    ref cell if *cell == json!([60, 65]) => "1",
                    _ => return false,
                };
                at_300["objects"].as_array().unwrap().iter().any(|after| {
                    after["id"] == before["id"]
                        && after["type"] == before["type"]
                        && after["cell"] == before["cell"]
                        && after["health"] == before["health"]
                        && after["active"] == true
                        && after["owner"] == initial["starts"][slot]
                })
            }),
    );
    let passed = checks.values().all(|v| *v);
    Ok(json!({"passed":passed,"checks":checks,"replay":path,
        "seed":startup.seed(),"map_sha256":startup.map_sha256(),
        "verification":{"ticks":verification.ticks,"checked_hashes":verification.checked_hashes,
            "final_tick":verification.final_tick,"final_state_hash":verification.final_state_hash},
        "initial":initial,"at_300":at_300,"stages":stages,"births":births,
        "scope":"Readonly SDK observations during strict playback of an actual client recording; link final hash to live LAN receipt separately."}))
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let replay = PathBuf::from(args.next().context("recording JSON")?);
    let output = PathBuf::from(args.next().context("new report JSON")?);
    ensure!(!output.exists(), "report must be new");
    let report = match audit(game, replay) {
        Ok(report) => report,
        Err(e) => json!({"passed":false,"error":format!("{e:#}")}),
    };
    std::fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("{}", output.display());
    ensure!(
        report["passed"] == true,
        "replay SDK audit failed; see report"
    );
    Ok(())
}
