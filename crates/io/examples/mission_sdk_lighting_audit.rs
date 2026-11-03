//! TS map lighting through retail campaign/session loading and ordinary effects.
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
    headless_scenario::{battle_session, load_with_launch_and_ini},
    map::map_file,
    sim::{
        command::{Command, CommandEnvelope},
        runtime::SimRuntime,
        scenario_bootstrap::MatchLaunchDescriptor,
        scenario_session::{ScenarioGameMode, ScenarioLightingProfile},
        world::{FrameEffects, TickLane},
    },
    skirmish_launch::LaunchStartPosition,
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;

fn build(
    doc: &mut MapDocument,
    install: &mut Install,
    folder: &Path,
    source: &str,
) -> Result<(Vec<u8>, bool)> {
    std::fs::create_dir(folder)?;
    let map = folder.join("mission.map");
    std::fs::write(&map, doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(install, map.to_str().context("map path")?)?;
    let catalog = mission_script::catalog(doc, &scene);
    let entry = folder.join("mission.ts");
    std::fs::write(&entry, source)?;
    mission_script::refresh_types(&entry, &catalog)?;
    let worker = Worker::new()?;
    worker
        .build(doc, catalog.clone(), Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(doc)?;
    let unchanged = !worker
        .build(doc, catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(doc)?;
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(map, &bytes)?;
    Ok((bytes, unchanged))
}

fn launch_dominator(runtime: &mut SimRuntime) -> Result<()> {
    let sim = &runtime.simulation;
    let owner = sim.interner().get("Americans").context("player")?;
    let sw_type_id = sim
        .interner()
        .get("PsychicDominatorSpecial")
        .context("dominator")?;
    let command = sim
        .encode_superweapon_record(
            owner,
            &Command::LaunchSuperWeapon {
                sw_type_id,
                target_rx: 66,
                target_ry: 66,
            },
            &runtime.resources.rules,
        )
        .context("ordinary dominator command admission")?;
    let result = runtime.advance_frame(
        &[CommandEnvelope::synchronized(command, sim.session().tick())],
        22,
        TickLane::Ordinary,
        FrameEffects::default(),
    )?;
    ensure!(result.sound_events.iter().any(|event| matches!(event,
        vera20k_core::sim::world::SimSoundEvent::SuperWeaponLaunched {owner:by,sw_type,..} if *by==owner && *sw_type==sw_type_id)),"dominator launch");
    Ok(())
}

fn profiles(runtime: &SimRuntime, interval: i32) -> bool {
    lighting_profiles(runtime.simulation.session().lighting(), interval)
}

fn lighting_profiles(
    p: &vera20k_core::sim::scenario_session::ScenarioLightingState,
    interval: i32,
) -> bool {
    let values = |p: vera20k_core::sim::scenario_session::ScenarioLightProfileUnits| {
        [
            p.ambient_percent,
            p.red_percent,
            p.green_percent,
            p.blue_percent,
            p.ground_units,
            p.level_units,
        ]
    };
    values(p.normal()) == [61, 120, 80, 90, 50, 32]
        && values(p.ion()) == [31, 40, 50, 60, 20, 4]
        && values(p.dominator()) == [115, 85, 20, 30, 10, 8]
        && p.dominator_change_rate() == interval
        && p.nuke_change_rate() == interval
}

fn audit(
    install: &Install,
    bytes: &[u8],
    folder: &Path,
    name: &str,
    interval: i32,
    restore: bool,
) -> Result<Value> {
    let mut runtime = campaign::load(install, bytes, 1)?;
    let initial = serde_json::to_value(runtime.simulation.session().lighting())?;
    let expected = if name == "storm" {
        ScenarioLightingProfile::Ion
    } else if name.starts_with("dominator") {
        ScenarioLightingProfile::Dominator
    } else if name.starts_with("nuke") {
        ScenarioLightingProfile::Nuke
    } else {
        ScenarioLightingProfile::Normal
    };
    let mut checks = BTreeMap::from([
        (
            "all_twenty_map_fields_installed",
            profiles(&runtime, interval),
        ),
        (
            "initial_normal_ambient",
            runtime.simulation.session().lighting().current_ambient() == 61,
        ),
    ]);
    let mut observations = Vec::new();
    let mut first_selected = None;
    let mut first_returned = None;
    let mut checkpoint_preserved = false;
    let mut transition_interval_seen = false;
    let mut ambient_moved = false;
    let mut launched = false;
    while runtime.simulation.session().tick() < 1000 {
        if name.starts_with("dominator") && !launched && runtime.simulation.session().tick() == 30 {
            launch_dominator(&mut runtime)?;
            launched = true;
        } else {
            runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        }
        let sim = &runtime.simulation;
        let tick = sim.session().tick();
        let p = sim.session().lighting();
        let state = serde_json::to_value(p)?;
        ambient_moved |= p.current_ambient() != 61;
        if p.selected_profile() == expected && first_selected.is_none() {
            first_selected = Some(tick);
        }
        if expected != ScenarioLightingProfile::Normal
            && first_selected.is_some()
            && p.selected_profile() == ScenarioLightingProfile::Normal
            && first_returned.is_none()
        {
            first_returned = Some(tick);
        }
        if expected == ScenarioLightingProfile::Dominator
            || expected == ScenarioLightingProfile::Nuke
        {
            transition_interval_seen |= p.selected_profile() == expected
                && state["transition_timer"]["duration"] == interval;
        }
        observations.push(json!({"tick":tick,"lighting":state}));
        let checkpoint_tick = first_selected.map(|tick| tick + 5);
        if restore && Some(tick) == checkpoint_tick {
            let before = serde_json::to_value(runtime.simulation.session().lighting())?;
            checkpoint::restore_checkpoint(
                &mut runtime,
                bytes,
                &folder.join(format!("{name}.bin")),
            )?;
            checkpoint_preserved =
                before == serde_json::to_value(runtime.simulation.session().lighting())?;
        }
    }
    checks.insert(
        "configured_profiles_preserved",
        profiles(&runtime, interval),
    );
    checks.insert("expected_profile_selected", first_selected.is_some());
    if expected != ScenarioLightingProfile::Normal {
        checks.insert("ambient_really_changes", ambient_moved);
        checks.insert("returns_to_normal_profile", first_returned.is_some());
        checks.insert(
            "returns_to_normal_ambient",
            runtime.simulation.session().lighting().current_ambient() == 61,
        );
    }
    if expected == ScenarioLightingProfile::Dominator || expected == ScenarioLightingProfile::Nuke {
        checks.insert("authored_fade_interval_consumed", transition_interval_seen);
    }
    if restore {
        checks.insert("checkpoint_lighting_preserved", checkpoint_preserved);
    }
    if name.starts_with("dominator") {
        checks.insert("ordinary_command_launched", launched);
    }
    Ok(
        json!({"name":name,"restore":restore,"passed":checks.values().all(|v|*v),"checks":checks,"initial":initial,"first_selected":first_selected,"first_returned":first_returned,"observations":observations}),
    )
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
    let mut install = Install::open(&game)?;
    let initial = NewMapOptions {
        width: 64,
        height: 64,
        ..Default::default()
    }
    .bytes()?;
    let mut base = MapDocument::new(Baseline::parse(
        initial.clone().into(),
        &map_file::load_from_bytes(initial)?,
    )?);
    for house in ["Americans", "Russians"] {
        mission::create(&mut base, Kind::House, house, house, None)?;
        base.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    base.add_record("Structures", [28, 52], "Americans", "GAPOWR")?;
    base.add_record("Units", [66, 66], "Russians", "MTNK")?;
    base.waypoint(0, [35, 45])?;
    base.commit("lighting audit base");
    let settings = r#"normal:{ambient:0.61,red:1.2,green:0.8,blue:0.9,ground:0.05,level:0.032},ion:{ambient:0.31,red:0.4,green:0.5,blue:0.6,ground:0.02,level:0.004},dominator:{ambient:1.15,red:0.85,green:0.2,blue:0.3,ground:0.01,level:0.008},dominatorAmbientChangeRate:__RATE__,nukeAmbientChangeRate:__INTERVAL__"#;
    let mut cases = Vec::new();
    let mut checks = BTreeMap::new();
    for (name, interval, rules) in [
        ("normal", 3, ""),
        (
            "storm",
            3,
            "m.rule('storm',{when:event.sinceScenarioStart(1),once:true,actions:[action.startLightningStorm(120)]});m.rule('stop',{when:event.sinceScenarioStart(12),once:true,actions:[action.stopLightningStorm()]});",
        ),
        (
            "dominator_fast",
            1,
            "const sw=m.superWeapon(SuperWeapon.PsychicDominatorSpecial);m.rule('ready',{when:event.sinceScenarioStart(1),once:true,actions:[action.grantSuperWeapon(sw),action.setSuperCharge(sw,100)]});",
        ),
        (
            "dominator_slow",
            4,
            "const sw=m.superWeapon(SuperWeapon.PsychicDominatorSpecial);m.rule('ready',{when:event.sinceScenarioStart(1),once:true,actions:[action.grantSuperWeapon(sw),action.setSuperCharge(sw,100)]});",
        ),
        (
            "nuke_fast",
            1,
            "m.rule('nuke',{when:event.sinceScenarioStart(1),once:true,actions:[action.nukeAt(at)]});",
        ),
        (
            "nuke_slow",
            4,
            "m.rule('nuke',{when:event.sinceScenarioStart(1),once:true,actions:[action.nukeAt(at)]});",
        ),
    ] {
        let folder = root.join(name);
        let config = settings
            .replace("__RATE__", &(f64::from(interval) / 1000.0).to_string())
            .replace("__INTERVAL__", &interval.to_string());
        let source = format!(
            "import {{defineMission,event,action}} from './sdk';import {{map,House,Country,SuperWeapon}} from './map.generated';export default defineMission(map,{{id:'lighting_{name}',player:House.Americans}},m=>{{m.configureBasic({{ignoreGlobalAiTriggers:true,skipScore:true}});m.configureHouse(m.house(House.Americans),{{country:m.country(Country.Americans),playerControl:true,iq:0}});m.configureHouse(m.house(House.Russians),{{country:m.country(Country.Russians),iq:0}});m.configureLighting({{{config}}});const at=m.createWaypoint(30,66,66);m.rule('fade',{{when:event.sinceScenarioStart(1),once:true,actions:[action.setAmbientStep(0.05),action.setAmbientRate(0.001)]}});{rules}}});"
        );
        let (bytes, unchanged) = build(&mut base.fork(), &mut install, &folder, &source)?;
        checks.insert(format!("{name}_unchanged_rebuild"), unchanged);
        let continuous = audit(&install, &bytes, &folder, name, interval, false)?;
        let restored = audit(&install, &bytes, &folder, name, interval, true)?;
        checks.insert(
            format!("{name}_continuous_and_checkpoint_light_trajectory"),
            continuous["observations"] == restored["observations"],
        );
        cases.extend([continuous, restored]);
    }
    for (fast, slow, tick) in [
        ("dominator_fast", "dominator_slow", 40),
        ("nuke_fast", "nuke_slow", 230),
    ] {
        let ambient = |name: &str| -> Result<i64> {
            cases
                .iter()
                .find(|c| c["name"] == name && c["restore"] == false)
                .context("comparison case")?["observations"]
                .as_array()
                .context("observations")?
                .iter()
                .find(|o| o["tick"] == tick)
                .context("comparison tick")?["lighting"]["current_ambient"]
                .as_i64()
                .context("ambient")
        };
        checks.insert(
            format!("{fast}_actually_fades_faster"),
            ambient(fast)? > ambient(slow)?,
        );
    }
    let source = format!(
        "import {{defineMission,MissionMode}} from './sdk';import {{map}} from './map.generated';export default defineMission(map,{{id:'lighting_mp',mode:MissionMode.Multiplayer}},m=>{{m.configureLighting({{{}}});}});",
        settings
            .replace("__RATE__", "0.003")
            .replace("__INTERVAL__", "3")
    );
    let folder = root.join("multiplayer");
    let (_, unchanged) = build(&mut base.fork(), &mut install, &folder, &source)?;
    checks.insert("multiplayer_unchanged_rebuild".into(), unchanged);
    for mode in [
        ScenarioGameMode::Skirmish,
        ScenarioGameMode::Lan,
        ScenarioGameMode::Wol,
    ] {
        let map = folder.join("mission.map");
        let filename = map.to_str().context("multiplayer path")?;
        let mut launch = battle_session(filename, &[]);
        launch.humans[0].slot.start_position = LaunchStartPosition::Position(0);
        launch.options.short_game = false;
        let launch =
            MatchLaunchDescriptor::from_resolved(launch).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let scenario = load_with_launch_and_ini(
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
        let p = scenario.sim().session().lighting();
        let state = serde_json::to_value(p)?;
        let passed = scenario.sim().session().game_mode() == mode
            && lighting_profiles(p, 3)
            && p.current_ambient() == 61;
        cases.push(json!({"name":mode.as_str(),"passed":passed,"checks":{"actual_session_uses_map_lighting":passed},"lighting":state}));
    }
    let passed = checks.values().all(|v| *v) && cases.iter().all(|c| c["passed"] == true);
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":passed,"checks":checks,"cases":cases,"scope":"Real TS build/rebuild, campaign lighting/effect lifecycles and exact observed lighting restoration; actual Skirmish/LAN/WOL startup. No GPU, network transport or original SAV compatibility claim."}),
        )?,
    )?;
    ensure!(passed, "lighting audit failed; see report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
