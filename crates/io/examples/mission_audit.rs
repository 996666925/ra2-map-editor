//! Publish an authored mission and optionally smoke the real campaign entry.
use anyhow::{Context, Result, ensure};
use ra2_map_document::{document::MapDocument, mission, new_map::NewMapOptions};
use ra2_map_io::{editor_io, playtest::PlayOptions, publication::Publisher};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use vera20k_core::{map::map_file, rules::team_ai_ini::TeamAiIniRegistry};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory required")?);
    let output = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "target/p4-mission-audit".into()),
    );
    let exe = args.next().map(PathBuf::from);
    std::fs::create_dir_all(&output)?;
    let mut publisher = Publisher::default();
    let initial = NewMapOptions {
        width: 40,
        height: 40,
        ..Default::default()
    }
    .bytes()?;
    let scene = publisher.scene(initial.clone(), &directory)?;
    let mut doc = MapDocument::new(scene.source.edit_base.map_err(anyhow::Error::msg)?);
    let trigger = mission::seed(&mut doc)?;
    doc.commit("minimum mission");
    ensure!(
        mission::issues(&doc).is_empty(),
        "mission diagnostics: {:?}",
        mission::issues(&doc)
    );
    let bytes = doc.snapshot().bytes()?;
    ensure!(
        doc.undo() && doc.snapshot().bytes()? == initial,
        "undo differs"
    );
    ensure!(
        doc.redo() && doc.snapshot().bytes()? == bytes,
        "redo differs"
    );
    let bytes = publisher.map_bytes(bytes, &directory)?;
    let map = map_file::load_from_bytes(bytes.clone())?;
    ensure!(
        map.triggers.len() == 2 && map.tags.len() == 2,
        "lost mission records"
    );
    let teams = TeamAiIniRegistry::from_sources(
        &vera20k_core::rules::ini_parser::IniFile::empty(),
        &map.ini,
        false,
    );
    ensure!(
        teams.diagnostics.is_empty(),
        "team diagnostics: {:?}",
        teams.diagnostics
    );
    ensure!(
        teams.team_types.len() == 1 && teams.task_forces.len() == 1 && teams.scripts.len() == 1,
        "lost team graph"
    );
    ensure!(
        map.events[&trigger].conditions()[0].kind == 13,
        "lost event"
    );
    ensure!(
        map.actions[&trigger]
            .entries()
            .iter()
            .map(|a| a.kind)
            .collect::<Vec<_>>()
            == [7, 56],
        "lost actions"
    );
    let map_path = output.join("minimum-mission.map");
    editor_io::replace_map(&map_path, &bytes, editor_io::Stamp::file(&map_path)?)?;
    publisher.png(
        bytes.clone(),
        &directory,
        &output.join("minimum-mission.png"),
        2048,
        Default::default(),
    )?;
    let options = PlayOptions {
        mission: true,
        ..Default::default()
    };
    std::fs::write(output.join("spawn.ini"), options.spawn_ini(&map)?)?;
    let mut report = serde_json::json!({"passed":true,"engine_readers":true,"publication":true,"undo_redo":true,"triggers":map.triggers.len(),"teams":teams.team_types.len(),"scope":"authored graph and readers; optional real campaign log assertions"});
    if let Some(exe) = exe {
        let mut process = editor_io::launch(&bytes, &exe, &directory, &output, &options)?;
        println!(
            "mission process {}: {}",
            process.child.id(),
            process.folder.display()
        );
        std::fs::write(
            output.join("playtest-folder.txt"),
            process.folder.to_string_lossy().as_bytes(),
        )?;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(20) {
            if let Some(status) = process.child.try_wait()? {
                ensure!(
                    status.success(),
                    "mission process failed: {status}; logs {}",
                    process.folder.display()
                );
                report["mission_process_exit"] = serde_json::json!(status.code());
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if process.child.try_wait()?.is_none() {
            process.child.kill()?;
            process.child.wait()?;
            report["mission_process_alive_20_seconds"] = true.into();
        }
        report["process_observation_seconds"] = serde_json::json!(start.elapsed().as_secs_f32());
        let log = std::fs::read_to_string(process.folder.join("logs/ra2.log")).unwrap_or_default();
        let evidence: Vec<_> = log
            .lines()
            .filter(|line| {
                line.contains("CnCNet campaign:")
                    || line.contains("Transitioned to InGame")
                    || line.contains("Match end ready for local player")
            })
            .collect();
        report["campaign_loaded"] = log
            .contains("Transitioned to InGame after scenario installation")
            .into();
        report["victory_logged"] = log
            .lines()
            .any(|line| {
                line.contains("Match end ready for local player 'Americans': You are Victorious!")
            })
            .into();
        ensure!(
            report["campaign_loaded"] == true && report["victory_logged"] == true,
            "missing campaign/victory evidence; run with RUST_LOG=info and inspect {}",
            process.folder.display()
        );
        report["runtime_evidence"] = serde_json::json!(evidence);
        report["playtest_folder"] = serde_json::json!(process.folder);
    }
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
