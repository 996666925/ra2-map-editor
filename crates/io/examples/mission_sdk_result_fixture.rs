//! Publish TS outcome cases for ordinary campaign GUI/audio/transition checks.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    new_map::NewMapOptions,
};
use ra2_map_io::{campaign, mission_script};
use serde_json::json;
use std::path::{Path, PathBuf};
use vera20k_core::map::map_file;

fn tone(root: &Path, name: &str, hz: f64) -> Result<()> {
    let count = if hz == 0.0 { 80000 } else { 8000 };
    let pcm: Vec<u8> = (0..count)
        .flat_map(|i| {
            let sample = (0.2
                * f64::from(i)
                    .mul_add(hz / 8000.0, 0.0)
                    .mul_add(std::f64::consts::TAU, 0.0)
                    .sin()
                * f64::from(i16::MAX)) as i16;
            sample.to_le_bytes()
        })
        .collect();
    let mut wav = Vec::new();
    wav.extend(b"RIFF");
    wav.extend((36 + pcm.len() as u32).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(8000u32.to_le_bytes());
    wav.extend(16000u32.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend((pcm.len() as u32).to_le_bytes());
    wav.extend(pcm);
    std::fs::write(root.join(name), wav)?;
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = std::path::absolute(PathBuf::from(args.next().context("game directory")?))?;
    let root = std::path::absolute(PathBuf::from(args.next().context("new output directory")?))?;
    let mode = args.next();
    ensure!(
        !root.exists()
            && mode
                .as_ref()
                .is_none_or(|v| v == "--font-only" || v == "--ranking-only")
            && args.next().is_none(),
        "new output directory required; optional --font-only or --ranking-only"
    );
    std::fs::create_dir_all(&root)?;
    let mut install = Install::open(&game)?;
    std::fs::write(
        root.join("game.fnt"),
        install
            .manager
            .get_ref("game.fnt")
            .context("production bitmap font")?,
    )?;
    if mode.as_ref().is_some_and(|v| v == "--font-only") {
        return Ok(());
    }
    let ranking_only = mode.as_ref().is_some_and(|v| v == "--ranking-only");
    let bytes = NewMapOptions::default().bytes()?;
    let map = map_file::load_from_bytes(bytes.clone())?;
    let mut doc = MapDocument::new(Baseline::parse(bytes.into(), &map)?);
    for name in ["Americans", "Russians"] {
        ra2_map_document::mission::create(
            &mut doc,
            ra2_map_document::mission::Kind::House,
            name,
            name,
            None,
        )?;
        doc.set_ini(name, "Country", Some(name.as_bytes().to_vec()))?;
    }
    doc.add_record("Structures", [40, 100], "Americans", "GAPOWR")?;
    doc.add_record("Structures", [52, 100], "Russians", "NAPOWR")?;
    doc.add_record("Infantry", [48, 100], "Russians", "E2")?;
    doc.commit("result GUI fixture");
    let base = doc.snapshot().bytes()?;
    if ranking_only {
        let mut missing = doc.fork();
        missing.set_ini(
            "Ranking",
            "UnderParTitle",
            Some(b"SDK:MISSING_RANK".to_vec()),
        )?;
        let audit = campaign::audit_map(&missing.snapshot().bytes()?, "first.map", &install)?;
        ensure!(
            !audit.passed
                && audit
                    .dependencies
                    .iter()
                    .any(|d| d.kind == "Csf" && d.name == "SDK:MISSING_RANK" && !d.resolved),
            "missing ranking CSF must block publishing"
        );
        let fallback = campaign::audit_map(&base, "first.map", &install)?;
        ensure!(
            !fallback.dependencies.iter().any(
                |d| d.name.starts_with("GUI:NoUnderPar") || d.name.starts_with("GUI:NoOverPar")
            ),
            "implicit fallback labels must not become new dependencies"
        );
        std::fs::write(
            root.join("ranking-audit-controls.json"),
            serde_json::to_vec_pretty(
                &json!({"passed":true,"missing_explicit_label":audit,"no_implicit_fallback_dependencies":true}),
            )?,
        )?;
    }
    std::fs::write(root.join("base.map"), &base)?;
    let scene = ra2_map_scene::map::load_scene(
        &mut install,
        root.join("base.map").to_str().context("path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let mut cases = vec![
        (
            "country_win_match",
            "action.winCountry(m.country(Country.Americans))",
            "Victory",
            false,
        ),
        (
            "country_win_other",
            "action.winCountry(m.country(Country.Russians))",
            "Defeat",
            false,
        ),
        (
            "country_lose_match",
            "action.loseCountry(m.country(Country.Americans))",
            "Defeat",
            false,
        ),
        (
            "country_lose_other",
            "action.loseCountry(m.country(Country.Russians))",
            "Victory",
            false,
        ),
        ("announce_win", "action.winPlayer()", "Victory", false),
        ("announce_lose", "action.losePlayer()", "Defeat", false),
        (
            "end_without_result",
            "action.endScenario()",
            "Victory",
            false,
        ),
        ("team_win", "order.winPlayer()", "Victory", true),
        ("team_lose", "order.losePlayer()", "Defeat", true),
        (
            "control",
            "action.setGlobal(m.globalFlag(8))",
            "None",
            false,
        ),
    ];
    if ranking_only {
        cases = vec![
            ("ranking_under", "action.winPlayer()", "Victory", false),
            ("ranking_over", "action.winPlayer()", "Victory", false),
        ];
    }
    let mut summaries = Vec::new();
    for (name, command, expected, team) in cases {
        let folder = root.join(name);
        std::fs::create_dir(&folder)?;
        for (file, hz) in [
            ("sdkwin.wav", 660.0),
            ("sdklose.wav", 880.0),
            ("sdkquiet.wav", 0.0),
        ] {
            tone(&folder, file, hz)?;
        }
        std::fs::write(
            folder.join("evamd.ini"),
            "[DialogList]\n0=EVA_MissionAccomplished\n1=EVA_MissionFailed\n[EVA_MissionAccomplished]\nAllied=sdkwin\nRussian=sdkwin\nYuri=sdkwin\n[EVA_MissionFailed]\nAllied=sdklose\nRussian=sdklose\nYuri=sdklose\n",
        )?;
        std::fs::write(
            folder.join("thememd.ini"),
            "[Themes]\n0=SdkQuiet\n[SdkQuiet]\nSound=sdkquiet\nNormal=yes\nRepeat=yes\n",
        )?;
        std::fs::write(folder.join("first.map"), &base)?;
        std::fs::write(folder.join("second.map"), &base)?;
        let declarations = if team {
            format!(
                "const caller=m.team('caller',{{house:m.house(House.Russians),waypoint:at,units:[{{type:m.unit(Unit.E2),count:1}}],full:true,reinforce:false,orders:[order.guard(1),{command}]}});\n m.rule('finish',{{owner:m.country(Country.Russians),when:event.sinceScenarioStart(4),once:true,actions:[action.recruit(caller)]}});"
            )
        } else {
            format!(
                "m.rule('finish',{{owner:m.country(Country.Russians),when:event.sinceScenarioStart(4),once:true,actions:[{command}]}});"
            )
        };
        let setup = r#"m.asset('evamd.ini','evamd.ini');m.asset('sdkwin.wav','sdkwin.wav');m.asset('sdklose.wav','sdklose.wav');
 m.asset('thememd.ini','thememd.ini');m.asset('sdkquiet.wav','sdkquiet.wav');
 const at=m.createWaypoint(30,48,100);
 m.configureBasic({homeCell:at,theme:m.assetTheme('SdkQuiet'),ignoreGlobalAiTriggers:true,skipScore:false,__INHERIT__});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),playerControl:true,iq:0,allies:[m.house(House.Russians)]});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),iq:0,allies:[m.house(House.Americans)]});"#;
        let ranking = ranking_only.then(|| {
            let seconds = if name == "ranking_under" {60} else {0};
            format!("m.configureRanking({{parTime:{{easy:{seconds},normal:{seconds},hard:{seconds}}},underParTitle:m.defineText('under_title','SDK on time'),underParMessage:m.defineText('under_message','SDK swift commander'),overParTitle:m.defineText('over_title','SDK over time'),overParMessage:m.defineText('over_message','SDK try faster')}});")
        });
        let first = format!(
            "import {{defineMission,event,action,order}} from './sdk';\nimport {{map,House,Country,Unit}} from './map.generated';\nexport default defineMission(map,{{id:'{name}',player:House.Americans}},m=>{{\n{}\n m.rule('begin',{{when:event.sinceScenarioStart(0),once:true,actions:[action.revealMap(),action.setGlobal(m.globalFlag(7)),action.setCountdown(40),action.message(m.defineText('begin','SDK stage one {name}'))]}});\n {declarations}\n}});",
            setup.replace("__INHERIT__", "timerInherit:false") + ranking.as_deref().unwrap_or("")
        );
        let second = format!(
            "import {{defineMission,event,action}} from './sdk';\nimport {{map,House,Country}} from './map.generated';\nexport default defineMission(map,{{id:'result_next',player:House.Americans}},m=>{{\n{}\n m.rule('begin',{{when:event.sinceScenarioStart(0),once:true,actions:[action.revealMap(),action.message(m.defineText('begin','SDK stage two reached'))]}});\n m.rule('carry',{{when:event.globalSet(m.globalFlag(7)),once:true,actions:[action.message(m.defineText('carry','SDK inherited flag seven'))]}});\n}});",
            setup.replace("__INHERIT__", "timerInherit:true")
        );
        std::fs::write(folder.join("first.ts"), first)?;
        std::fs::write(folder.join("second.ts"), second)?;
        std::fs::write(
            folder.join("campaign.ts"),
            "import {defineCampaign} from './sdk';\nexport default defineCampaign({id:'SDK_RESULT',entry:'first.map',missions:[{file:'first.map',source:'first.map',script:'first.ts',next:'second.map',title:'SDK stage one'},{file:'second.map',source:'second.map',script:'second.ts',endOfGame:true,title:'SDK stage two'}]});\n",
        )?;
        mission_script::refresh_types(&folder.join("first.ts"), &catalog)?;
        let prepared = mission_script::prepare_campaign(&folder.join("campaign.ts"), &game)?;
        if ranking_only {
            for field in [
                "UnderParTitle",
                "UnderParMessage",
                "OverParTitle",
                "OverParMessage",
            ] {
                let map = map_file::load_from_bytes(prepared.payload["first.map"].clone())?;
                let label = map
                    .ini
                    .section_or_empty("Ranking")
                    .read_string(field, "", 0x1f);
                ensure!(
                    prepared
                        .report
                        .dependencies
                        .iter()
                        .any(|d| d.kind == "Csf" && d.name == label && d.resolved),
                    "ranking CSF dependency not audited: {field}"
                );
            }
        }
        let report = campaign::export(prepared, &folder.join("package"))?;
        ensure!(report.passed, "dependency audit failed: {name}");
        campaign::install_clean(&folder.join("package"), &game, &folder.join("install"))?;
        std::fs::create_dir(folder.join("settings"))?;
        std::fs::write(
            folder.join("settings/VERA.INI"),
            "[Intro]\nPlay=no\n[VERA]\nSkipStartupLogo=yes\nFullScreen=no\nLockCursor=no\nCustomEndGameMovie=no\nCustomEndGameBanner=no\n",
        )?;
        let config = vera20k_core::util::config::set_paths_key(
            "",
            "ra2_dir",
            &folder.join("install").to_string_lossy(),
        );
        let config = vera20k_core::util::config::set_paths_key(
            &config,
            "settings_dir",
            &folder.join("settings").to_string_lossy(),
        );
        std::fs::write(folder.join("config.toml"), config)?;
        let mut summary = json!({"name":name,"command":command,"expected":expected,"team":team,"publication":report});
        if ranking_only {
            summary["ranking"] = if name == "ranking_under" {
                json!({"title":"SDK on time","message":"SDK swift commander","par":"00:01:00","opposite_title":"SDK over time","opposite_message":"SDK try faster"})
            } else {
                json!({"title":"SDK over time","message":"SDK try faster","par":"00:00:00","opposite_title":"SDK on time","opposite_message":"SDK swift commander"})
            };
        }
        summaries.push(summary);
    }
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":true,"cases":summaries,"scope":"TS campaign build, complete dependency audit, publication and clean install only; ordinary visible client result/audio/transition checks pending."}),
        )?,
    )?;
    println!("{}", root.display());
    Ok(())
}
