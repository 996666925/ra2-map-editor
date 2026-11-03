//! Publish a TS campaign for ordinary save-browser input, without engine adapters.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    new_map::NewMapOptions,
};
use ra2_map_io::{campaign, mission_script};
use serde_json::json;
use std::path::PathBuf;
use vera20k_core::map::map_file;

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = std::path::absolute(PathBuf::from(args.next().context("game directory")?))?;
    let root = std::path::absolute(PathBuf::from(args.next().context("new output directory")?))?;
    ensure!(
        !root.exists() && args.next().is_none(),
        "output must be new"
    );
    std::fs::create_dir_all(&root)?;
    let mut install = Install::open(&game)?;
    std::fs::write(
        root.join("game.fnt"),
        install.manager.get_ref("game.fnt").context("bitmap font")?,
    )?;
    let bytes = NewMapOptions::default().bytes()?;
    let parsed = map_file::load_from_bytes(bytes.clone())?;
    let mut doc = MapDocument::new(Baseline::parse(bytes.into(), &parsed)?);
    ra2_map_document::mission::create(
        &mut doc,
        ra2_map_document::mission::Kind::House,
        "Americans",
        "Americans",
        None,
    )?;
    doc.set_ini("Americans", "Country", Some(b"Americans".to_vec()))?;
    doc.add_record("Structures", [40, 100], "Americans", "GAPOWR")?;
    doc.add_record("Infantry", [44, 100], "Americans", "E1")?;
    doc.commit("save browser fixture");
    let path = root.join("first.map");
    std::fs::write(&path, doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(&mut install, path.to_str().context("map path")?)?;
    let catalog = mission_script::catalog(&doc, &scene);
    let entry = root.join("mission.ts");
    std::fs::write(
        &entry,
        r#"import {defineMission,event,action,order} from './sdk';
import {map,House,Country,Unit} from './map.generated';
export default defineMission(map,{id:'save_browser',player:House.Americans},m=>{
 const at=m.createWaypoint(30,44,100);
 m.configureBasic({homeCell:at,ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),playerControl:true,iq:0});
 const title=m.defineText('timer','SDK browser countdown');
 const guard=m.team('saved_guard',{house:m.house(House.Americans),waypoint:at,
   units:[{type:m.unit(Unit.E1),count:1}],full:true,max:1,reinforce:false,
   orders:[order.guard(20),order.setGlobal(m.globalFlag(0)),order.waitForever()]});
 const wave=m.team('wave',{house:m.house(House.Americans),waypoint:at,
   units:[{type:m.unit(Unit.ENGINEER),count:1}],max:-1,reinforce:false,
   orders:[order.waitForever()]});
 m.rule('begin',{when:event.sinceScenarioStart(0),once:true,actions:[
   action.revealMap(),action.setCountdown(600),action.setCountdownTitle(title),
   action.recruit(guard),action.message(m.defineText('begin','SDK browser ready'))]});
 m.rule('repeat',{when:event.sinceEnabled(2),once:false,actions:[action.reinforce(wave,at)]});
 m.rule('guard_done',{when:event.globalSet(m.globalFlag(0)),once:true,
   actions:[action.message(m.defineText('done','SDK saved guard finished'))]});
 m.rule('stop',{when:event.sinceScenarioStart(30),once:true,
   actions:[action.disableRule(m.ruleRef('repeat')),action.setGlobal(m.globalFlag(1))]});
});
"#,
    )?;
    mission_script::refresh_types(&entry, &catalog)?;
    let worker = mission_script::Worker::new()?;
    worker
        .build(
            &doc,
            catalog.clone(),
            mission_script::Sources::read(&entry)?,
        )?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(&mut doc)?;
    let unchanged = !worker
        .build(&doc, catalog, mission_script::Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(&mut doc)?;
    ensure!(unchanged, "unchanged rebuild");
    // Campaign preparation must consume the original base and TS source;
    // publication owns the generated map and all declared text dependencies.
    std::fs::write(
        root.join("campaign.ts"),
        "import {defineCampaign} from './sdk';\nexport default defineCampaign({id:'SDK_BROWSER',entry:'first.map',missions:[{file:'first.map',source:'first.map',script:'mission.ts',endOfGame:true,title:'SDK browser task'}]});\n",
    )?;
    let prepared = mission_script::prepare_campaign(&root.join("campaign.ts"), &game)?;
    let publication = campaign::export(prepared, &root.join("package"))?;
    ensure!(publication.passed, "campaign dependency audit");
    campaign::install_clean(&root.join("package"), &game, &root.join("install"))?;
    // install_clean may hardlink loose inputs. Keep only immutable MIX files
    // shared; the normal client owns this fixture's mutable configuration.
    for item in std::fs::read_dir(root.join("install"))? {
        let item = item?;
        if item.file_type()?.is_file()
            && !item
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("mix"))
        {
            let bytes = std::fs::read(item.path())?;
            std::fs::remove_file(item.path())?;
            std::fs::write(item.path(), bytes)?;
        }
    }
    std::fs::create_dir(root.join("settings"))?;
    std::fs::write(
        root.join("settings/VERA.INI"),
        "[Intro]\nPlay=no\n[VERA]\nSkipStartupLogo=yes\nFullScreen=no\nLockCursor=no\n",
    )?;
    let config = vera20k_core::util::config::set_paths_key(
        "",
        "ra2_dir",
        &root.join("install").to_string_lossy(),
    );
    let config = vera20k_core::util::config::set_paths_key(
        &config,
        "settings_dir",
        &root.join("settings").to_string_lossy(),
    );
    std::fs::write(root.join("config.toml"), config)?;
    std::fs::write(
        root.join("fixture.json"),
        serde_json::to_vec_pretty(&json!({"passed":true,"unchanged_rebuild":unchanged,
            "publication":publication,"scope":"TS build, unchanged rebuild, audited campaign publication and isolated installation only. Ordinary browser input, saved state and GPU observations require separate verification."}))?,
    )?;
    println!("{}", root.display());
    Ok(())
}
