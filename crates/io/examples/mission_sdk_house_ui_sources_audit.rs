//! Resolve the actual House name sources in campaign and multiplayer loaders.
//! Does not synthesize a UI surface or claim that campaign has a House list.
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
use vera20k_core::{
    headless_scenario::{battle_session, load_with_launch_and_ini},
    sim::{
        house_state::HouseUiName,
        scenario_bootstrap::MatchLaunchDescriptor,
        scenario_session::ScenarioGameMode,
        world::{FrameEffects, Simulation, TickLane},
    },
    skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition},
};
#[path = "support/campaign.rs"]
mod campaign;

fn names(sim: &Simulation) -> Vec<Value> {
    sim.houses()
        .order()
        .iter()
        .map(|owner| {
            let house = sim.houses().get(owner).expect("registered house");
            json!({"owner":sim.interner().resolve(*owner),"source":house.ui_name(),
            "country":house.country().map(|id|sim.interner().resolve(id))})
        })
        .collect()
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
    let mut base = MapDocument::new(Baseline::parse(
        bytes.clone().into(),
        &vera20k_core::map::map_file::load_from_bytes(bytes)?,
    )?);
    for name in ["Americans", "Russians", "Neutral", "Special"] {
        mission::create(&mut base, Kind::House, name, name, None)?;
        base.set_ini(name, "Country", Some(name.as_bytes().to_vec()))?;
    }
    base.add_record("Structures", [40, 50], "Americans", "GAPOWR")?;
    base.waypoint(0, [40, 50])?;
    base.waypoint(1, [70, 70])?;
    base.commit("House UI sources");
    let mut install = Install::open(&game)?;
    let worker = Worker::new()?;
    let mut checks = BTreeMap::new();
    let mut runs = Vec::new();
    for multiplayer in [false, true] {
        let folder = root.join(if multiplayer {
            "multiplayer"
        } else {
            "campaign"
        });
        std::fs::create_dir(&folder)?;
        let map = folder.join("mission.map");
        let entry = folder.join("mission.ts");
        let mut doc = base.fork();
        std::fs::write(&map, doc.snapshot().bytes()?)?;
        let scene =
            ra2_map_scene::map::load_scene(&mut install, map.to_str().context("map path")?)?;
        let catalog = mission_script::catalog(&doc, &scene);
        let options = if multiplayer {
            "mode:MissionMode.Multiplayer"
        } else {
            "mode:MissionMode.SinglePlayer,player:House.Americans"
        };
        std::fs::write(
            &entry,
            format!(
                "import {{defineMission,MissionMode}} from './sdk';import {{map,House}} from './map.generated';export default defineMission(map,{{id:'house_sources',{options}}},m=>{{m.configureBasic({{ignoreGlobalAiTriggers:true,skipScore:true}});for(const id of [House.Americans,House.Russians,House.Neutral,House.Special]){{m.configureHouse(m.house(id),{{uiName:m.defineText('name_'+id,'SDK 地图阵营 '+id)}});}}}});"
            ),
        )?;
        mission_script::refresh_types(&entry, &catalog)?;
        worker
            .build(&doc, catalog.clone(), Sources::read(&entry)?)?
            .recv()?
            .map_err(anyhow::Error::msg)?
            .build
            .apply(&mut doc)?;
        checks.insert(
            format!("{multiplayer}_unchanged_rebuild"),
            !worker
                .build(&doc, catalog, Sources::read(&entry)?)?
                .recv()?
                .map_err(anyhow::Error::msg)?
                .build
                .apply(&mut doc)?,
        );
        let bytes = doc.snapshot().bytes()?;
        std::fs::write(&map, &bytes)?;
        let csf_bytes = mission_script::text_payload(&bytes, &install)?.context("generated CSF")?;
        std::fs::write(folder.join("ra2md.csf"), &csf_bytes)?;
        let csf = vera20k_core::assets::csf_file::CsfFile::from_bytes(&csf_bytes)?;
        checks.insert(
            format!("{multiplayer}_all_four_authored_names_resolve"),
            ["Americans", "Russians", "Neutral", "Special"]
                .iter()
                .all(|name| {
                    csf.get(&doc.ini_text(name, "UIName"))
                        == Some(format!("SDK 地图阵营 {name}").as_str())
                }),
        );
        if !multiplayer {
            let mut runtime = campaign::load(&install, &bytes, 1)?;
            let initial = names(&runtime.simulation);
            checks.insert(
                "campaign_four_map_names".into(),
                initial.len() == 4
                    && initial.iter().all(|row| {
                        row["source"]
                            == json!({"Csf":doc.ini_text(row["owner"].as_str().unwrap(),"UIName")})
                    }),
            );
            for _ in 0..60 {
                runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
            }
            let final_names = names(&runtime.simulation);
            checks.insert(
                "campaign_actual_mode_and_tick".into(),
                runtime.simulation.session().game_mode() == ScenarioGameMode::Campaign
                    && runtime.simulation.session().tick() == 60,
            );
            checks.insert(
                "campaign_names_retained_ordinary_frames".into(),
                initial == final_names,
            );
            runs.push(json!({"mode":"Campaign","initial":initial,"final":final_names,"tick":runtime.simulation.session().tick()}));
            continue;
        }
        for mode in [
            ScenarioGameMode::Skirmish,
            ScenarioGameMode::Lan,
            ScenarioGameMode::Wol,
        ] {
            let filename = map.to_str().context("map path")?;
            let mut launch =
                battle_session(filename, &[(LaunchCountry::Russia, AiDifficulty::Normal)]);
            launch.humans[0].name = "SDK Room Player".into();
            launch.humans[0].slot.start_position = LaunchStartPosition::Position(0);
            launch.opponents[0].start_position = LaunchStartPosition::Position(1);
            launch.options.unit_count = 0;
            launch.options.short_game = false;
            launch.options.crates = false;
            launch.rebuild_house_roster();
            let launch = MatchLaunchDescriptor::from_resolved(launch)
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            let mut scenario = load_with_launch_and_ini(
                &game,
                filename,
                12345,
                launch,
                &Default::default(),
                mode,
                None,
                None,
            )
            .map_err(anyhow::Error::msg)?;
            if mode.is_network() {
                scenario.runtime.simulation.install_network_timing(10);
            }
            let sim = &scenario.runtime.simulation;
            let initial = names(sim);
            let source = |name| {
                sim.interner()
                    .get(name)
                    .and_then(|id| sim.houses().get(&id))
                    .map(|h| h.ui_name().clone())
            };
            checks.insert(
                format!("{}_actual_mode_four_houses", mode.as_str()),
                sim.session().game_mode() == mode && initial.len() == 4,
            );
            checks.insert(
                format!("{}_human_room_handle", mode.as_str()),
                source("SDK Room Player") == Some(HouseUiName::handle("SDK Room Player")),
            );
            checks.insert(
                format!("{}_computer_localized_source", mode.as_str()),
                source("Computer1") == Some(HouseUiName::Computer),
            );
            checks.insert(
                format!("{}_special_houses_type_names", mode.as_str()),
                ["Neutral", "Special"]
                    .iter()
                    .all(|name| source(name) == Some(HouseUiName::TypeName)),
            );
            checks.insert(
                format!("{}_no_map_csf_names", mode.as_str()),
                sim.houses().order().iter().all(|id| {
                    !matches!(sim.houses().get(id).unwrap().ui_name(), HouseUiName::Csf(_))
                }),
            );
            for _ in 0..60 {
                scenario.runtime.advance_frame(
                    &[],
                    22,
                    TickLane::Ordinary,
                    FrameEffects::default(),
                )?;
            }
            let final_names = names(scenario.sim());
            checks.insert(
                format!("{}_actual_final_mode_and_tick", mode.as_str()),
                scenario.sim().session().game_mode() == mode
                    && scenario.sim().session().tick() == 60,
            );
            checks.insert(
                format!("{}_names_retained_ordinary_frames", mode.as_str()),
                initial == final_names,
            );
            runs.push(json!({"mode":mode,"initial":initial,"final":final_names,"tick":scenario.sim().session().tick()}));
        }
    }
    let passed = checks.values().all(|v| *v);
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":passed,"checks":checks,"runs":runs,"scope":"TS build/rebuild and real campaign/Skirmish/LAN/WOL name-source initialization, followed by 60 ordinary frames. Campaign UIName is metadata; ordinary campaign lacks the multiplayer House list. No fabricated UI, pixel, network or retail parity claim."}),
        )?,
    )?;
    ensure!(passed, "House UI source audit failed; see report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
