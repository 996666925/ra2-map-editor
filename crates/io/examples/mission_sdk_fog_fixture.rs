//! TS-authored campaign fog controls for the existing opening GPU capture.
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

#[path = "support/campaign.rs"]
mod campaign;

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
    mission::create(&mut base, Kind::House, "Americans", "Americans", None)?;
    base.set_ini("Americans", "Country", Some(b"Americans".to_vec()))?;
    base.add_record("Structures", [57, 60], "Americans", "GAPOWR")?;
    base.waypoint(0, [60, 60])?;
    base.commit("campaign fog base");
    let worker = Worker::new()?;
    let mut cases = Vec::new();
    for fog in [false, true] {
        for reveal in [false, true] {
            let name = format!("fog_{fog}_reveal_{reveal}");
            let folder = root.join(&name);
            std::fs::create_dir(&folder)?;
            let map = folder.join("ALL01UMD.MAP");
            let entry = folder.join("mission.ts");
            let mut doc = base.fork();
            std::fs::write(&map, doc.snapshot().bytes()?)?;
            let scene =
                ra2_map_scene::map::load_scene(&mut install, map.to_str().context("map path")?)?;
            let catalog = mission_script::catalog(&doc, &scene);
            let reveal_rule = if reveal {
                "m.rule('explore',{when:event.sinceScenarioStart(3),once:true,actions:[action.revealAt(explored)]});"
            } else {
                ""
            };
            std::fs::write(
                &entry,
                format!(
                    "import {{defineMission,MissionMode,event,action}} from './sdk';import {{map,House}} from './map.generated';export default defineMission(map,{{id:'{name}',mode:MissionMode.SinglePlayer,player:House.Americans}},m=>{{const at=m.createWaypoint(30,60,60);const explored=m.createWaypoint(31,65,65);m.configureBasic({{homeCell:at,ignoreGlobalAiTriggers:true,skipScore:true}});m.configureSpecialFlags({{fogOfWar:{fog}}});m.rule('lock',{{when:event.sinceScenarioStart(1),once:true,actions:[action.lockInput(),action.jumpCamera(at)]}});m.rule('unlock',{{when:event.sinceScenarioStart(2),once:true,actions:[action.unlockInput()]}});{reveal_rule}}});"
                ),
            )?;
            mission_script::refresh_types(&entry, &catalog)?;
            worker
                .build(&doc, catalog.clone(), Sources::read(&entry)?)?
                .recv()?
                .map_err(anyhow::Error::msg)?
                .build
                .apply(&mut doc)?;
            ensure!(
                !worker
                    .build(&doc, catalog, Sources::read(&entry)?)?
                    .recv()?
                    .map_err(anyhow::Error::msg)?
                    .build
                    .apply(&mut doc)?,
                "identical rebuild changed {name}"
            );
            let bytes = doc.snapshot().bytes()?;
            std::fs::write(&map, &bytes)?;
            let mut runtime = campaign::load(&install, &bytes, 1)?;
            let mut observations = Vec::new();
            loop {
                let sim = &runtime.simulation;
                let tick = sim.session().tick();
                if [0, 30, 46, 120].contains(&tick) {
                    let viewer = sim.session().current_house().context("campaign player")?;
                    let cells = [[60,60], [65,65], [61,52]].map(|[x,y]|json!({"cell":[x,y],"revealed":sim.fog().is_cell_revealed(viewer,x,y),"visible":sim.fog().is_cell_visible(viewer,x,y)}));
                    observations.push(json!({"tick":tick,"mode":sim.session().game_mode(),"viewer":sim.interner().resolve(viewer),"fog":sim.session().game_options().fog_of_war,"cells":cells}));
                }
                if tick == 120 {
                    break;
                }
                runtime.advance_frame(
                    &[],
                    22,
                    vera20k_core::sim::world::TickLane::Ordinary,
                    vera20k_core::sim::world::FrameEffects::default(),
                )?;
            }
            std::fs::write(
                folder.join("runtime.json"),
                serde_json::to_vec_pretty(&observations)?,
            )?;
            let payload = mission_script::playtest_payload(&bytes, &game, &scene.revision.id())?;
            let assets = folder.join("install");
            std::fs::create_dir(&assets)?;
            for source in std::fs::read_dir(&game)? {
                let source = source?;
                let name = source.file_name().to_string_lossy().to_ascii_lowercase();
                if source.file_type()?.is_file() && !payload.contains_key(&name) {
                    let target = assets.join(&name);
                    if name.ends_with(".mix") {
                        std::fs::hard_link(source.path(), target)?;
                    } else if [".ini", ".csf", ".fnt", ".pal"]
                        .iter()
                        .any(|ext| name.ends_with(ext))
                    {
                        std::fs::copy(source.path(), target)?;
                    }
                }
            }
            for (name, content) in payload {
                std::fs::write(assets.join(name), content)?;
            }
            std::fs::write(assets.join("ALL01UMD.MAP"), &bytes)?;
            let config =
                vera20k_core::util::config::set_paths_key("", "ra2_dir", &assets.to_string_lossy());
            let config = vera20k_core::util::config::set_paths_key(
                &config,
                "settings_dir",
                &folder.join("settings").to_string_lossy(),
            );
            std::fs::write(folder.join("config.toml"), config)?;
            std::fs::create_dir(folder.join("settings"))?;
            std::fs::write(
                folder.join("settings/VERA.INI"),
                "[Intro]\nPlay=no\n[VERA]\nSkipStartupLogo=yes\nFullScreen=no\nLockCursor=no\n",
            )?;
            cases.push(json!({"name":name,"fog":fog,"reveal":reveal,"tick":120}));
        }
    }
    std::fs::write(
        root.join("fixture.json"),
        serde_json::to_vec_pretty(
            &json!({"cases":cases,"cells":[[60,60],[65,65],[61,52]],"scope":"TS build, unchanged rebuild and independent campaign runtime; GPU verification is separate."}),
        )?,
    )?;
    println!("{}", root.join("fixture.json").display());
    Ok(())
}
