//! TS-authored initial and transient lighting for existing GPU capture routes.
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
use vera20k_core::skirmish_launch::{
    LaunchCountry, LaunchHumanRole, LaunchStartPosition, LaunchTeam, PreFillHouseRoster,
    SkirmishHumanSlot, SkirmishLaunchMode, SkirmishLaunchOptions, SkirmishLaunchSession,
    SkirmishLocalSlot,
};

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
    mission::create(&mut base, Kind::House, "Neutral", "Neutral", None)?;
    base.set_ini("Neutral", "Country", Some(b"Neutral".to_vec()))?;
    base.add_record("Structures", [64, 70], "Neutral", "GAPOWR")?;
    base.waypoint(0, [60, 60])?;
    base.commit("GPU lighting base");
    let worker = Worker::new()?;
    let custom = "normal:{ambient:0.61,red:1.2,green:0.8,blue:0.9,ground:0.05,level:0.032},ion:{ambient:0.31,red:0.4,green:0.5,blue:0.6,ground:0.02,level:0.004},dominator:{ambient:1.15,red:0.85,green:0.2,blue:0.3,ground:0.01,level:0.008}";
    let mut cases = Vec::new();
    for (name, lighting, rules, ticks, dominator) in [
        (
            "control",
            "normal:{ambient:1,red:1,green:1,blue:1,ground:0,level:0}".to_string(),
            "",
            vec![0, 60, 260, 500, 300, 1000],
            false,
        ),
        (
            "dim",
            "normal:{ambient:0.4,red:1,green:1,blue:1,ground:0,level:0}".into(),
            "",
            vec![0],
            false,
        ),
        (
            "red",
            "normal:{ambient:1,red:1,green:0,blue:0,ground:0,level:0}".into(),
            "",
            vec![0],
            false,
        ),
        (
            "green",
            "normal:{ambient:1,red:0,green:1,blue:0,ground:0,level:0}".into(),
            "",
            vec![0],
            false,
        ),
        (
            "blue",
            "normal:{ambient:1,red:0,green:0,blue:1,ground:0,level:0}".into(),
            "",
            vec![0],
            false,
        ),
        (
            "authored",
            format!("{custom},dominatorAmbientChangeRate:0.001,nukeAmbientChangeRate:1"),
            "",
            vec![0, 60, 260, 500, 300, 1000],
            false,
        ),
        (
            "dominator_retained_control",
            "normal:{ambient:0.61,red:1.2,green:0.8,blue:0.9,ground:0.01,level:0.032}".into(),
            "",
            vec![500],
            false,
        ),
        (
            "storm",
            format!("{custom},dominatorAmbientChangeRate:0.001,nukeAmbientChangeRate:1"),
            "m.rule('storm',{when:event.sinceScenarioStart(1),once:true,actions:[action.startLightningStorm(120)]});m.rule('stop',{when:event.sinceScenarioStart(12),once:true,actions:[action.stopLightningStorm()]});",
            vec![0, 300, 1000],
            false,
        ),
        (
            "nuke_fast",
            format!("{custom},dominatorAmbientChangeRate:0.001,nukeAmbientChangeRate:1"),
            "m.rule('nuke',{when:event.sinceScenarioStart(1),once:true,actions:[action.nukeAt(at)]});",
            vec![260, 500],
            false,
        ),
        (
            "nuke_slow",
            format!("{custom},dominatorAmbientChangeRate:0.004,nukeAmbientChangeRate:4"),
            "m.rule('nuke',{when:event.sinceScenarioStart(1),once:true,actions:[action.nukeAt(at)]});",
            vec![260],
            false,
        ),
        (
            "dominator_fast",
            format!("{custom},dominatorAmbientChangeRate:0.001,nukeAmbientChangeRate:1"),
            "const sw=m.superWeapon(SuperWeapon.PsychicDominatorSpecial);m.rule('ready',{owner:m.country(Country.Americans),when:event.sinceScenarioStart(1),once:true,actions:[action.grantSuperWeapon(sw),action.setSuperCharge(sw,100)]});",
            vec![60, 500],
            true,
        ),
        (
            "dominator_slow",
            format!("{custom},dominatorAmbientChangeRate:0.004,nukeAmbientChangeRate:4"),
            "const sw=m.superWeapon(SuperWeapon.PsychicDominatorSpecial);m.rule('ready',{owner:m.country(Country.Americans),when:event.sinceScenarioStart(1),once:true,actions:[action.grantSuperWeapon(sw),action.setSuperCharge(sw,100)]});",
            vec![60],
            true,
        ),
    ] {
        let folder = root.join(name);
        std::fs::create_dir(&folder)?;
        let map = folder.join("mission.map");
        let mut doc = base.fork();
        std::fs::write(&map, doc.snapshot().bytes()?)?;
        let scene =
            ra2_map_scene::map::load_scene(&mut install, map.to_str().context("map path")?)?;
        let catalog = mission_script::catalog(&doc, &scene);
        let entry = folder.join("mission.ts");
        std::fs::write(
            &entry,
            format!(
                "import {{defineMission,MissionMode,event,action}} from './sdk';import {{map,Country,SuperWeapon}} from './map.generated';export default defineMission(map,{{id:'gpu_{name}',mode:MissionMode.Multiplayer}},m=>{{m.configureBasic({{ignoreGlobalAiTriggers:true}});m.configureLighting({{{lighting}}});const at=m.createWaypoint(30,90,90);m.rule('fade',{{when:event.sinceScenarioStart(1),once:true,actions:[action.setAmbientStep(0.05),action.setAmbientRate(0.001)]}});{rules}}});"
            ),
        )?;
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
        ensure!(unchanged, "{name} changed on identical rebuild");
        std::fs::write(&map, doc.snapshot().bytes()?)?;
        let launch = SkirmishLaunchSession {
            mode: SkirmishLaunchMode::stock(1).context("Battle mode")?,
            selected_map_file: Some(map.to_string_lossy().into_owned()),
            humans: vec![SkirmishHumanSlot {
                raw_slot_index: Some(0),
                name: "SDK lighting".into(),
                slot: SkirmishLocalSlot {
                    country: LaunchCountry::America,
                    country_random: false,
                    color_index: 0,
                    color_random: false,
                    start_position: LaunchStartPosition::Position(0),
                    team: LaunchTeam::None,
                },
                role: LaunchHumanRole::default(),
            }],
            local_player_index: 0,
            ai_player_count: 0,
            opponents: vec![],
            pre_fill_house_roster: PreFillHouseRoster::from_compact_skirmish(0),
            options: SkirmishLaunchOptions {
                unit_count: 0,
                short_game: false,
                crates: false,
                shroud: false,
                fog_of_war: false,
                ..Default::default()
            },
        };
        // Independently retain the production simulation lighting trajectory;
        // GPU receipts expose cell lighting rather than Scenario lighting.
        let mut scenario = vera20k_core::headless_scenario::load_session(
            &game,
            map.to_str().context("map path")?,
            12345,
            launch.clone(),
        )
        .map_err(anyhow::Error::msg)?;
        let mut observations = Vec::new();
        while scenario.runtime.simulation.session().tick() < 1000 {
            let sim = &scenario.runtime.simulation;
            let frame = sim.session().tick();
            let commands = if dominator && frame == 30 {
                let owner = sim.interner().get("SDK lighting").context("player")?;
                let sw_type_id = sim
                    .interner()
                    .get("PsychicDominatorSpecial")
                    .context("Dominator")?;
                let command = sim
                    .encode_superweapon_record(
                        owner,
                        &vera20k_core::sim::command::Command::LaunchSuperWeapon {
                            sw_type_id,
                            target_rx: 90,
                            target_ry: 90,
                        },
                        &scenario.runtime.resources.rules,
                    )
                    .context("Dominator command admission")?;
                vec![vera20k_core::sim::command::CommandEnvelope::synchronized(
                    command, frame,
                )]
            } else {
                vec![]
            };
            scenario.runtime.advance_frame(
                &commands,
                22,
                vera20k_core::sim::world::TickLane::Ordinary,
                vera20k_core::sim::world::FrameEffects::default(),
            )?;
            let sim = &scenario.runtime.simulation;
            if ticks.contains(&(sim.session().tick() as i32)) || sim.session().tick() == 1000 {
                observations
                    .push(json!({"tick":sim.session().tick(),"lighting":sim.session().lighting()}));
            }
        }
        std::fs::write(
            folder.join("runtime.json"),
            serde_json::to_vec_pretty(&observations)?,
        )?;
        for tick in &ticks {
            let mut profile = json!({"schema_version":"vera20k.map-observation-profile.v2","launch":launch,"seed":12345,"input_delay_ticks":15,"ticks":tick,"width":1024,"height":768,"timeout_seconds":120,"observe_owners":["SDK lighting","Neutral"],"observe_types":["GAPOWR"],"camera_cell":[60,60],"cursor_position":[1000,700],"terrain_cells":[[52,52],[54,52],[52,54],[54,54]],"observe_sidebar_steps":[tick]});
            if dominator {
                profile["schema_version"] = json!("vera20k.map-observation-profile.v3");
                profile["observe_super_weapons"] = json!(true);
                profile["commands"] = json!([{"issue_after_step":30,"owner":"SDK lighting","payload":{"LaunchSuperWeapon":{"sw_type_id":"PsychicDominatorSpecial","target_rx":90,"target_ry":90}}}]);
            }
            std::fs::write(
                folder.join(format!("profile-{tick}.json")),
                serde_json::to_vec_pretty(&profile)?,
            )?;
            cases.push(json!({"name":name,"tick":tick,"profile":format!("{name}/profile-{tick}.json"),"map":format!("{name}/mission.map")}));
        }
    }
    std::fs::write(
        root.join("fixture.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":true,"cases":cases,"scope":"TS build and identical rebuild only; GPU capture is separate."}),
        )?,
    )?;
    println!("{}", root.join("fixture.json").display());
    Ok(())
}
