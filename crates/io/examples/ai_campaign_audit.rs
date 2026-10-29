//! Authored House nodes + AITrigger selection through real campaign frames.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    base_nodes,
    document::{Baseline, MapDocument, Target, Value},
    mission::{self, Kind},
    new_map::NewMapOptions,
};
use std::{collections::BTreeSet, path::PathBuf};
use vera20k_core::{
    map::map_file,
    sim::world::{FrameEffects, TickLane},
};
#[path = "support/campaign.rs"]
mod campaign;
fn put(doc: &mut MapDocument, s: &str, k: &str, v: impl AsRef<str>) -> Result<()> {
    doc.set_ini(s, k, Some(v.as_ref().as_bytes().to_vec()))
}
fn authored(enable: bool, amount: i32, easy_only: bool, country: usize) -> Result<Vec<u8>> {
    let bytes = NewMapOptions {
        width: 40,
        height: 40,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(bytes.clone())?;
    let mut doc = MapDocument::new(Baseline::parse(bytes.into(), &map)?);
    for house in ["Americans", "Russians"] {
        mission::create(&mut doc, Kind::House, house, house, None)?;
        for (key, value) in [
            ("Country", house),
            ("IQ", "5"),
            ("Credits", "10000"),
            ("TechLevel", "10"),
            ("Allies", "Americans,Russians"),
        ] {
            put(&mut doc, house, key, value)?;
        }
    }
    put(&mut doc, "Americans", "PlayerControl", "yes")?;
    for (key, value) in [
        ("Player", "Americans"),
        ("MultiplayerOnly", "no"),
        ("IgnoreGlobalAITriggers", "yes"),
        ("EndOfGame", "yes"),
        ("OneTimeOnly", "yes"),
        ("SkipScore", "yes"),
        ("HomeCell", "0"),
    ] {
        put(&mut doc, "Basic", key, value)?;
    }
    put(&mut doc, "General", "TeamDelays", "30,30,30")?;
    doc.add_record("Structures", [26, 40], "Americans", "GAPOWR")?;
    doc.add_record("Structures", [38, 36], "Russians", "NACNST")?;
    doc.add_record("Structures", [44, 36], "Russians", "NAPOWR")?;
    let barracks = doc.add_record("Structures", [38, 44], "Russians", "NAHAND")?;
    base_nodes::import_structures(&mut doc, "Russians")?;
    let plan = base_nodes::read(&doc, "Russians")?;
    let index = plan.iter().position(|n| n.building == "NAHAND").unwrap();
    base_nodes::set_rebuild(&mut doc, "Russians", index, true)?;
    mission::create(&mut doc, Kind::Trigger, "BASE_DESTROY", "Russians", None)?;
    put(&mut doc, "Events", "BASE_DESTROY", "1,13,0,20")?;
    put(
        &mut doc,
        "Actions",
        "BASE_DESTROY",
        mission::encode_list(&[mission::action(32, "0")]),
    )?;
    let tag = mission::ids(&doc, Kind::Tag)
        .into_iter()
        .find(|id| mission::csv(&doc.ini_text("Tags", id))[2] == "BASE_DESTROY")
        .unwrap();
    let record = doc.records()[&barracks].patched(&[(6, tag)])?;
    doc.set(Target::Record(barracks), Some(Value::Record(record)))?;
    for at in [[34, 40], [34, 42]] {
        doc.add_record("Infantry", at, "Russians", "E2")?;
    }
    doc.waypoint(20, [34, 46])?;
    mission::create(&mut doc, Kind::TaskForce, "AI_FORCE", "Russians", None)?;
    put(&mut doc, "AI_FORCE", "0", "2,E2")?;
    mission::create(&mut doc, Kind::Script, "AI_SCRIPT", "Russians", None)?;
    for (k, v) in [("0", "3,20"), ("1", "12,0"), ("2", "49,0")] {
        put(&mut doc, "AI_SCRIPT", k, v)?;
    }
    mission::create(&mut doc, Kind::Team, "AI_TEAM", "Russians", None)?;
    for (k, v) in [
        ("Script", "AI_SCRIPT"),
        ("TaskForce", "AI_FORCE"),
        ("House", "Russians"),
        ("IsBaseDefense", "yes"),
        ("Reinforce", "no"),
        ("Max", "1"),
    ] {
        put(&mut doc, "AI_TEAM", k, v)?;
    }
    mission::create(&mut doc, Kind::AiTrigger, "MAP_AI", "Russians", None)?;
    let mut row = ra2_map_document::ai_triggers::decode(&doc.ini_text("AITriggerTypes", "MAP_AI"))?;
    row[4] = "1".into();
    row[5] = "E2".into();
    row[6] = ra2_map_document::ai_triggers::with_comparison(&row[6], amount, 3)?;
    for i in [7, 8, 9] {
        row[i] = "5000".into();
    }
    if easy_only {
        row[16] = "0".into();
        row[17] = "0".into();
    }
    put(&mut doc, "AITriggerTypes", "MAP_AI", row.join(","))?;
    put(
        &mut doc,
        "AITriggerTypesEnable",
        "MAP_AI",
        if enable { "yes" } else { "no" },
    )?;
    mission::create(&mut doc, Kind::Trigger, "ACTIVATE", "Russians", None)?;
    put(&mut doc, "Events", "ACTIVATE", "1,13,0,1")?;
    put(
        &mut doc,
        "Actions",
        "ACTIVATE",
        mission::encode_list(&[
            mission::action(3, &country.to_string()),
            mission::action(74, &country.to_string()),
        ]),
    )?;
    mission::create(&mut doc, Kind::Trigger, "AI_RESULT", "Americans", None)?;
    put(&mut doc, "Events", "AI_RESULT", "2,27,0,0,13,0,40")?;
    put(
        &mut doc,
        "Actions",
        "AI_RESULT",
        mission::encode_list(&[mission::action(67, "0")]),
    )?;
    doc.snapshot().bytes()
}
fn run(
    install: &Install,
    bytes: &[u8],
    difficulty: i32,
    expect_team: bool,
    check_base: bool,
) -> Result<serde_json::Value> {
    let mut runtime = campaign::load(install, bytes, difficulty)?;
    let owner = runtime
        .simulation
        .interner()
        .get("Russians")
        .context("AI owner")?;
    let team_type = runtime
        .simulation
        .interner()
        .get("AI_TEAM")
        .context("AI team")?;
    let barracks_type = runtime
        .simulation
        .interner()
        .get("NAHAND")
        .context("barracks type")?;
    let initial = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find(|(_, e)| e.type_ref() == barracks_type && e.owner() == owner)
        .context("initial barracks")?
        .0;
    let mut members = BTreeSet::new();
    let mut cursors = BTreeSet::new();
    let mut first_team = None;
    let mut destroyed = None;
    let mut rebuilt = None;
    let mut result = None;
    for frame in 0..6000 {
        let sim = &runtime.simulation;
        for team in sim
            .team_script_vm()
            .teams_in_order()
            .filter(|t| t.team_type_id() == Some(team_type))
        {
            first_team.get_or_insert(frame);
            members.extend(team.members());
            cursors.insert(team.cursor());
        }
        if sim
            .entities()
            .get(initial)
            .is_none_or(|e| e.health.current() <= 0)
        {
            destroyed.get_or_insert(frame);
        }
        if destroyed.is_some()
            && let Some((id, e)) = sim.entities().iter_sorted().find(|(id, e)| {
                *id != initial
                    && e.type_ref() == barracks_type
                    && e.owner() == owner
                    && e.health.current() > 0
                    && !e.lifecycle.in_limbo()
            })
        {
            rebuilt.get_or_insert((frame, id, [e.position.rx(), e.position.ry()]));
        }
        if sim.global_variables()[0] {
            result.get_or_insert(frame);
        }
        if expect_team && result.is_some() && (!check_base || rebuilt.is_some()) {
            break;
        }
        if !expect_team && frame >= 1000 && (!check_base || rebuilt.is_some()) {
            break;
        }
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
    }
    ensure!(
        first_team.is_some() == expect_team,
        "difficulty {difficulty}: expected team={expect_team}, frame={first_team:?}, binary_frame={}, destroyed={destroyed:?}",
        runtime.simulation.session().binary_frame()
    );
    if expect_team {
        ensure!(
            members.len() == 2 && result.is_some() && cursors.contains(&0),
            "members={members:?}, cursors={cursors:?}, script global={result:?}"
        );
    } else {
        ensure!(result.is_none(), "disabled AI unexpectedly ran its script");
    }
    if check_base {
        ensure!(
            destroyed.is_some() && rebuilt.is_some(),
            "base not rebuilt: destroyed={destroyed:?}, rebuilt={rebuilt:?}, house={:?}",
            runtime.view().houses()[&owner].base_plan()
        );
        ensure!(
            rebuilt.unwrap().2 == [38, 44],
            "base was rebuilt at a different plan site"
        );
    }
    Ok(
        serde_json::json!({"passed":true,"difficulty":difficulty,"expect_team":expect_team,"first_team_frame":first_team,"members":members,"cursors":cursors,"script_global_frame":result,"destroyed_frame":destroyed,"rebuilt":rebuilt}),
    )
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game dir")?);
    let output = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "target/p4-ai-campaign".into()),
    );
    let executable = args.next().map(PathBuf::from);
    std::fs::create_dir_all(&output)?;
    let install = Install::open(&directory)?;
    let country = install
        .catalog
        .rules
        .section("Countries")
        .context("countries")?
        .registry_ids()
        .iter()
        .position(|id| *id == "Russians")
        .context("Russian country index")?;
    let mut reports = Vec::new();
    for (name, enable, amount, easy_only, difficulty, expected, base) in [
        ("easy", true, 2, false, 0, true, true),
        ("normal", true, 2, false, 1, true, false),
        ("hard", true, 2, false, 2, true, false),
        ("disabled", false, 2, false, 0, false, false),
        ("condition", true, 3, false, 0, false, false),
        ("easy-only", true, 2, true, 0, true, false),
        ("difficulty-gate", true, 2, true, 2, false, false),
        ("weight-gate", true, 2, false, 0, false, false),
        ("country-gate", true, 2, false, 0, false, false),
        ("side-gate", true, 2, false, 0, false, false),
        ("stored-tech-column", true, 2, false, 0, true, false),
        ("tech-gate", true, 2, false, 0, false, false),
    ] {
        let mut bytes = authored(enable, amount, easy_only, country)?;
        if matches!(
            name,
            "weight-gate" | "country-gate" | "side-gate" | "stored-tech-column" | "tech-gate"
        ) {
            let map = map_file::load_from_bytes(bytes.clone())?;
            let mut doc = MapDocument::new(Baseline::parse(bytes.into(), &map)?);
            let mut row =
                ra2_map_document::ai_triggers::decode(&doc.ini_text("AITriggerTypes", "MAP_AI"))?;
            match name {
                "weight-gate" => {
                    for i in [7, 8, 9] {
                        row[i] = "0".into();
                    }
                }
                "country-gate" => row[2] = "Americans".into(),
                "side-gate" => row[12] = "1".into(),
                "stored-tech-column" => row[3] = "999".into(),
                "tech-gate" => put(&mut doc, "E2", "TechLevel", "11")?,
                _ => unreachable!(),
            }
            put(&mut doc, "AITriggerTypes", "MAP_AI", row.join(","))?;
            bytes = doc.snapshot().bytes()?;
        }
        std::fs::write(output.join(format!("{name}.map")), &bytes)?;
        let mut report = run(&install, &bytes, difficulty, expected, base)?;
        report["case"] = name.into();
        if matches!(name, "easy" | "normal" | "hard")
            && let Some(exe) = &executable
        {
            let options = ra2_map_io::playtest::PlayOptions {
                mission: true,
                mission_difficulty: difficulty as u8,
                speed: 0,
                ..Default::default()
            };
            let mut process =
                ra2_map_io::editor_io::launch(&bytes, exe, &directory, &output, &options)?;
            let started = std::time::Instant::now();
            let mut status = None;
            while started.elapsed() < std::time::Duration::from_secs(35) {
                if let Some(exit) = process.child.try_wait()? {
                    status = Some(exit);
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            if status.is_none() {
                process.child.kill()?;
                process.child.wait()?;
            }
            let log =
                std::fs::read_to_string(process.folder.join("logs/ra2.log")).unwrap_or_default();
            let evidence: Vec<_> = log
                .lines()
                .filter(|s| {
                    s.contains("Transitioned to InGame")
                        || s.contains("Match end ready for local player")
                })
                .collect();
            ensure!(
                status.is_none_or(|s| s.success())
                    && evidence
                        .iter()
                        .any(|s| s.contains("Transitioned to InGame"))
                    && evidence
                        .iter()
                        .any(|s| s.contains("Americans") && s.contains("You are Victorious!")),
                "real AI campaign failed: {}",
                process.folder.display()
            );
            report["real_process"] = serde_json::json!({"passed":true,"exit":status.and_then(|s|s.code()),"stopped_after_result":status.is_none(),"folder":process.folder,"seconds":started.elapsed().as_secs_f32(),"evidence":evidence});
        }
        println!("{report}");
        reports.push(report);
    }
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"passed":true,"runs":reports,"scope":"production campaign loader, installed retail rules/terrain and ordinary frames; AI selection/recruitment/script, attached-building destruction and planned reconstruction"}),
        )?,
    )?;
    Ok(())
}
