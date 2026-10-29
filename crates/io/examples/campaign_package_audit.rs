//! Three-map branch/carry-over fixture installed into an empty data directory.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    campaign::{Asset, Mission, Project, destination},
    document::{Baseline, MapDocument},
    mission::{self, Kind},
    new_map::NewMapOptions,
};
use ra2_map_io::campaign::{self as package, Report};
use std::{collections::BTreeMap, path::PathBuf};
use vera20k_core::{
    map::map_file,
    rules::{
        campaign_loading::{CampaignLoadingMetadata, CampaignScenarioNames},
        campaigns::CampaignRegistry,
        ini_parser::IniFile,
        movies::MovieRegistry,
    },
    sim::world::{FrameEffects, TickLane},
};
#[path = "support/campaign.rs"]
mod campaign;
fn put(d: &mut MapDocument, s: &str, k: &str, v: &str) -> Result<()> {
    d.set_ini(s, k, Some(v.as_bytes().to_vec()))
}
fn authored(first: bool, branch: bool) -> Result<Vec<u8>> {
    let bytes = NewMapOptions {
        width: 40,
        height: 40,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(bytes.clone())?;
    let mut d = MapDocument::new(Baseline::parse(bytes.into(), &map)?);
    for house in ["Americans", "Russians"] {
        mission::create(&mut d, Kind::House, house, house, None)?;
        put(&mut d, house, "Country", house)?;
        put(&mut d, house, "Allies", "Americans,Russians")?;
        put(&mut d, house, "Credits", if first { "100" } else { "70" })?;
    }
    for (k, v) in [
        ("Player", "Americans"),
        ("MultiplayerOnly", "no"),
        ("EndOfGame", if first { "no" } else { "yes" }),
        ("OneTimeOnly", "no"),
        ("SkipScore", "yes"),
        ("SkipMapSelect", "yes"),
        ("NextScenario", "SECOND.MAP"),
        ("AltNextScenario", "ALTERNATE.MAP"),
        ("CarryOverMoney", "25%"),
        ("CarryOverCap", "2000"),
        ("TimerInherit", "yes"),
        ("HomeCell", "0"),
    ] {
        put(&mut d, "Basic", k, v)?;
    }
    put(&mut d, "Americans", "PlayerControl", "yes")?;
    d.add_record("Structures", [26, 40], "Americans", "GAPOWR")?;
    d.add_record("Structures", [44, 36], "Russians", "NAPOWR")?;
    put(&mut d, "VariableNames", "3", "LocalOld,0")?;
    put(&mut d, "VariableNames", "7", "Received,0")?;
    mission::write_fields(
        &mut d,
        Kind::Global,
        "49",
        &[("EditorGlobalNames".into(), b"Carry test".to_vec())],
    )?;
    mission::create(&mut d, Kind::Trigger, "CARRY_FLOW", "Americans", None)?;
    put(
        &mut d,
        "Events",
        "CARRY_FLOW",
        if first { "1,13,0,1" } else { "1,27,0,49" },
    )?;
    let actions = if first {
        let mut a = vec![
            mission::action(28, "49"),
            mission::action(56, "3"),
            mission::action(27, "90"),
            mission::action(23, "0"),
            mission::action(11, "ED:HELLO"),
            mission::action(19, "PackageSound"),
            mission::action(20, "PackageTheme"),
        ];
        if branch {
            a.push(mission::action(28, "0"));
        }
        a
    } else {
        vec![
            mission::action(56, "7"),
            mission::action(29, "49"),
            mission::action(11, "ED:HELLO"),
        ]
    };
    put(
        &mut d,
        "Actions",
        "CARRY_FLOW",
        &mission::encode_list(&actions),
    )?;
    for (id, event, action) in if first {
        vec![("LOCAL_SOURCE", "1,36,0,3", mission::action(28, "48"))]
    } else {
        vec![
            ("LOCAL_LEAK", "1,36,0,3", mission::action(28, "46")),
            ("LOCAL_CONSUMED", "1,36,0,7", mission::action(28, "47")),
        ]
    } {
        mission::create(&mut d, Kind::Trigger, id, "Americans", None)?;
        put(&mut d, "Events", id, event)?;
        put(&mut d, "Actions", id, &mission::encode_list(&[action]))?;
    }
    d.snapshot().bytes()
}
fn advance(rt: &mut vera20k_core::sim::runtime::SimRuntime, frames: usize) -> Result<[bool; 3]> {
    let mut media = [false; 3];
    for _ in 0..frames {
        let output = rt.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        for effect in output.trigger_effects {
            use vera20k_core::sim::trigger_runtime::TriggerEffect;
            match effect {
                TriggerEffect::LocalizedMessage { csf_label } if csf_label == "ED:HELLO" => {
                    media[0] = true
                }
                TriggerEffect::MusicTheme { .. } => media[1] = true,
                _ => {}
            }
        }
        media[2] |= output.sound_events.iter().any(|e| matches!(e, vera20k_core::sim::world::SimSoundEvent::VocCentered { sound_id } if sound_id == "PackageSound"));
    }
    Ok(media)
}
fn pcm() -> Vec<u8> {
    let data = vec![0u8; 22050 * 2 / 10];
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((36 + data.len() as u32).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(22050u32.to_le_bytes());
    b.extend(44100u32.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend((data.len() as u32).to_le_bytes());
    b.extend(data);
    b
}
fn verify_metadata(install: &Install) -> Result<()> {
    let csf =
        vera20k_core::assets::csf_file::CsfFile::load_active(|n| install.manager.get_ref(n))?.0;
    ensure!(
        csf.get("ED:HELLO") == Some("跨关测试：中文与 Unicode 😀"),
        "installed CSF missing"
    );
    let mission = IniFile::from_bytes(
        install
            .manager
            .get_ref("missionmd.ini")
            .context("mission metadata")?,
    )?;
    let mut names = CampaignScenarioNames::default();
    names.apply_ini(&mission, "FIRST.MAP", &csf);
    ensure!(names.save_title() == "测试存档", "save title");
    let mut loading = CampaignLoadingMetadata::default();
    loading.apply_ini(&mission, "FIRST.MAP");
    ensure!(loading.load_message_key() == "ED:HELLO", "loading metadata");
    let mut registry = CampaignRegistry::default();
    registry.apply_ini(
        &IniFile::from_bytes(install.manager.get_ref("battlemd.ini").context("battle")?)?,
        &MovieRegistry::default(),
    );
    let index = registry.find_index("all1").context("shell alias")?;
    ensure!(
        registry.get(index).unwrap().scenario() == "FIRST.MAP",
        "campaign entry"
    );
    let mapsel = IniFile::from_bytes(install.manager.get_ref("mapselmd.ini").context("mapsel")?)?;
    ensure!(
        mapsel.section_or_empty("GDI").read_string("1", "", 64) == "EDSTAGE0",
        "stage list"
    );
    ensure!(
        mapsel.section_or_empty("EDSTAGE0").read_string("0", "", 64) == "EDSTAGE1"
            && mapsel.section_or_empty("EDSTAGE0").read_string("1", "", 64) == "EDSTAGE2",
        "branch choices"
    );
    let wav = vera20k_core::assets::wav_file::WavFile::parse(
        install.manager.get_ref("package.wav").context("wave")?,
    )
    .context("wave decode")?;
    ensure!(
        wav.sample_rate == 22050 && wav.bits_per_sample == 16,
        "wave metadata"
    );
    Ok(())
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory")?);
    let root = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "target/p4-campaign-package".into()),
    );
    std::fs::create_dir_all(&root)?;
    let root = std::fs::canonicalize(root)?;
    let run = root.join(format!(
        "run-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs()
    ));
    std::fs::create_dir(&run)?;
    std::fs::write(run.join("FIRST.MAP"), authored(true, false)?)?;
    std::fs::write(run.join("SECOND.MAP"), authored(false, false)?)?;
    std::fs::write(run.join("ALTERNATE.MAP"), authored(false, false)?)?;
    std::fs::write(run.join("package.wav"), pcm())?;
    let install = Install::open(&directory)?;
    let sample = install
        .manager
        .load_audio_index()?
        .context("stock audio index")?
        .index
        .entries()
        .first()
        .context("stock sound sample")?
        .name
        .clone();
    let sound = package::patch_ini(
        install.manager.get_ref("soundmd.ini").context("sounds")?,
        &[
            ("SoundList".into(), "9999".into(), "PackageSound".into()),
            ("PackageSound".into(), "Sounds".into(), sample),
        ],
    )?;
    let theme = package::patch_ini(
        install.manager.get_ref("thememd.ini").context("themes")?,
        &[
            ("Themes".into(), "9999".into(), "PackageTheme".into()),
            ("PackageTheme".into(), "Sound".into(), "package".into()),
        ],
    )?;
    std::fs::write(run.join("soundmd.ini"), sound)?;
    std::fs::write(run.join("thememd.ini"), theme)?;
    let p = Project {
        version: 1,
        id: "PACKAGE_TEST".into(),
        entry: "FIRST.MAP".into(),
        missions: ["FIRST.MAP", "SECOND.MAP", "ALTERNATE.MAP"]
            .into_iter()
            .map(|file| Mission {
                file: file.into(),
                source: file.into(),
                side: "GDI".into(),
                ui_name: "ED:TITLE".into(),
                briefing: "ED:HELLO".into(),
                load_message: "ED:HELLO".into(),
                ..Default::default()
            })
            .collect(),
        strings: BTreeMap::from([
            ("ED:HELLO".into(), "跨关测试：中文与 Unicode 😀".into()),
            ("ED:TITLE".into(), "测试任务".into()),
            ("ED:TITLESav".into(), "测试存档".into()),
        ]),
        assets: ["package.wav", "soundmd.ini", "thememd.ini"]
            .into_iter()
            .map(|s| Asset {
                source: s.into(),
                name: s.into(),
            })
            .collect(),
    };
    package::save_project(&run.join("campaign.json"), &p)?;
    let p = package::read_project(&run.join("campaign.json"))?;
    let prepared = package::prepare(&p, &run, &directory, None)?;
    ensure!(
        prepared.report.passed,
        "missing: {:?}",
        prepared
            .report
            .dependencies
            .iter()
            .filter(|d| !d.resolved)
            .collect::<Vec<_>>()
    );
    let report: Report = package::export(prepared, &run.join("package"))?;
    package::install_clean(&run.join("package"), &directory, &run.join("clean-install"))?;
    let clean = Install::open(&run.join("clean-install"))?;
    verify_metadata(&clean)?;
    let mut runs = Vec::new();
    for difficulty in 0..3 {
        for branch in [false, true] {
            let first = authored(true, branch)?;
            let mut rt = campaign::load(&clean, &first, difficulty)?;
            ensure!(
                rt.simulation.global_variables().iter().all(|b| !*b),
                "new globals not clear"
            );
            ensure!(
                advance(&mut rt, 100)? == [true; 3],
                "text/theme/sound requests missing"
            );
            ensure!(
                rt.simulation.global_variables()[49] && rt.simulation.global_variables()[48],
                "source did not execute"
            );
            let incoming = rt.simulation.save_campaign_carry_over();
            ensure!(
                incoming.money() == 10000 && incoming.countdown_frames() > 0,
                "source carry data"
            );
            let map = map_file::load_from_bytes(first)?;
            let next = destination(&map.basic, rt.simulation.global_variables()[0])
                .context("branch destination")?;
            ensure!(
                next == if branch {
                    "ALTERNATE.MAP"
                } else {
                    "SECOND.MAP"
                },
                "wrong branch"
            );
            let bytes = std::fs::read(run.join("clean-install").join(next.to_ascii_lowercase()))?;
            let mut second = campaign::load_stage(
                &clean,
                &bytes,
                difficulty,
                next,
                2,
                Some(incoming),
                Some(rt.simulation.retain_local_variables_for_fresh_load()),
            )?;
            let carried = second.simulation.save_campaign_carry_over();
            ensure!(
                carried.money() == 9000
                    && carried.countdown_frames() == incoming.countdown_frames()
                    && carried.globals()[49],
                "receiver carry mismatch: {carried:?}"
            );
            let saved =
                serde_json::to_vec(&second.simulation.session().campaign_restart_carry_over())?;
            let restored: Option<vera20k_core::sim::campaign_carry_over::CampaignCarryOver> =
                serde_json::from_slice(&saved)?;
            ensure!(restored == Some(incoming), "saved incoming state lost");
            advance(&mut second, 50)?;
            ensure!(
                second.simulation.global_variables()[47]
                    && !second.simulation.global_variables()[46]
                    && !second.simulation.global_variables()[49],
                "second did not consume inherited flag"
            );
            let retry = campaign::load_stage(
                &clean,
                &bytes,
                difficulty,
                next,
                2,
                restored,
                Some(second.simulation.retain_local_variables_for_fresh_load()),
            )?;
            ensure!(
                retry.simulation.global_variables()[49] && !retry.simulation.global_variables()[47],
                "retry did not restore incoming state"
            );
            runs.push(serde_json::json!({"passed":true,"difficulty":difficulty,"branch":branch,"next":next,"incoming_money":incoming.money(),"received_money":carried.money(),"timer_frames":carried.countdown_frames(),"globals_and_local_initializers":true,"carry_record_serialization":true,"retry":true}));
        }
    }
    // Missing explicit resources and a missing branch must prevent publication.
    let mut bad = p.clone();
    bad.strings.remove("ED:HELLO");
    let missing = package::prepare(&bad, &run, &directory, None)?;
    ensure!(!missing.report.passed, "missing CSF not detected");
    let mut bad = p.clone();
    bad.assets.retain(|a| a.name != "package.wav");
    ensure!(
        !package::prepare(&bad, &run, &directory, None)?
            .report
            .passed,
        "missing WAV not detected"
    );
    let original = std::fs::read(run.join("FIRST.MAP"))?;
    let mut reordered = p.clone();
    reordered.missions.rotate_left(1);
    let ordered = package::prepare(&reordered, &run, &directory, None)?;
    ensure!(
        IniFile::from_bytes(&ordered.payload["mapselmd.ini"])?
            .section_or_empty("EDSTAGE0")
            .read_string("Scenario", "", 64)
            == "FIRST.MAP",
        "entry not at stage zero"
    );
    let alternate = authored(true, true)?;
    let snapshot = package::prepare(&p, &run, &directory, Some(("FIRST.MAP", &alternate)))?;
    ensure!(
        snapshot.payload["first.map"] == alternate
            && std::fs::read(run.join("FIRST.MAP"))? == original,
        "snapshot export modified source"
    );
    let mut invalid = original.clone();
    let text = String::from_utf8(invalid)?;
    invalid = text
        .replace("19,7,PackageSound", "19,0,PackageSound")
        .into_bytes();
    ensure!(
        !package::prepare(&p, &run, &directory, Some(("FIRST.MAP", &invalid)))?
            .report
            .passed,
        "invalid action parameter type admitted"
    );
    let sounds = vera20k_core::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_bytes(
        &std::fs::read(run.join("soundmd.ini"))?,
    )?);
    let ordinal = sounds.index_of("PackageSound").context("sound ordinal")?;
    let numeric = String::from_utf8(original.clone())?
        .replace("19,7,PackageSound", &format!("19,0,{ordinal}"));
    ensure!(
        package::prepare(
            &p,
            &run,
            &directory,
            Some(("FIRST.MAP", numeric.as_bytes()))
        )?
        .report
        .passed,
        "native numeric sound ordinal rejected"
    );
    let mut scripted = MapDocument::new(Baseline::parse(
        original.clone().into(),
        &map_file::load_from_bytes(original.clone())?,
    )?);
    mission::create(
        &mut scripted,
        Kind::Script,
        "MEDIA_CHECK",
        "Americans",
        None,
    )?;
    put(&mut scripted, "MEDIA_CHECK", "0", &format!("25,{ordinal}"))?;
    let bytes = scripted.snapshot().bytes()?;
    ensure!(
        package::prepare(&p, &run, &directory, Some(("FIRST.MAP", &bytes)))?
            .report
            .passed,
        "script sound ordinal rejected"
    );
    put(&mut scripted, "MEDIA_CHECK", "1", "27,2147483647")?;
    let bytes = scripted.snapshot().bytes()?;
    ensure!(
        !package::prepare(&p, &run, &directory, Some(("FIRST.MAP", &bytes)))?
            .report
            .passed,
        "missing script media admitted"
    );
    ensure!(
        package::install_clean(&run.join("package"), &directory, &run.join("clean-install"))
            .is_err(),
        "existing install overwritten"
    );
    std::fs::write(run.join("package/first.map"), b"tampered")?;
    ensure!(
        package::install_clean(
            &run.join("package"),
            &directory,
            &run.join("tampered-install")
        )
        .is_err(),
        "tampered package admitted"
    );
    std::fs::write(run.join("package/first.map"), &original)?;
    let mut bad = p.clone();
    bad.missions.pop();
    ensure!(
        package::prepare(&bad, &run, &directory, None).is_err(),
        "missing branch not rejected"
    );
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"passed":true,"runs":runs,"directory":run,"package":report,"clean_install":true,"metadata_csf_audio":true,"media_requests":true,"snapshot_source_unchanged":true,"tamper_rejected":true,"invalid_parameter_type_rejected":true,"missing_dependencies_rejected":true,"missing_branch_rejected":true,"scope":"fixed engine production campaign frames; authored branch catalog, native carry-over, carry record serialization and retry; no full GUI transition claim"}),
        )?,
    )?;
    println!("campaign package audit passed: {}", root.display());
    Ok(())
}
