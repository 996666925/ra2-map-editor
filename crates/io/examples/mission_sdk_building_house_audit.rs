//! TS-authored House, diplomacy and attached-building actions in ordinary frames.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    mission::{self, Kind},
    new_map::NewMapOptions,
};
use ra2_map_io::mission_script::{self, Sources, Worker};
use std::path::PathBuf;
use vera20k_core::{
    map::map_file,
    sim::world::{FrameEffects, TickLane},
};
#[path = "support/campaign.rs"]
mod campaign;

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(PathBuf::from(args.next().context("new output directory")?))?;
    ensure!(!root.exists(), "请选择新的审计目录");
    std::fs::create_dir_all(&root)?;
    let bytes = NewMapOptions {
        width: 48,
        height: 48,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(bytes.clone())?;
    let mut doc = MapDocument::new(Baseline::parse(bytes.into(), &map)?);
    for house in ["Americans", "Russians"] {
        mission::create(&mut doc, Kind::House, house, house, None)?;
        doc.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    doc.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    doc.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    doc.add_record("Structures", [20, 30], "Americans", "NAHAND")?;
    doc.add_record("Structures", [26, 30], "Americans", "NAHAND")?;
    doc.add_record("Structures", [30, 34], "Americans", "NAPOWR")?;
    doc.commit("building and House fixture");
    let base = doc.snapshot().bytes()?;
    std::fs::write(root.join("base.map"), &base)?;
    let mut install = Install::open(&game)?;
    let scene = ra2_map_scene::map::load_scene(
        &mut install,
        root.join("base.map").to_str().context("fixture path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let entry = root.join("mission.ts");
    std::fs::write(
        &entry,
        r#"import {defineMission,event,action,TagRepeat} from './sdk';
import {map} from './map.generated';
export default defineMission(map,{id:'building_house',player:'Americans'},m=>{
  const americans=m.house('Americans'), russians=m.house('Russians');
  m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
  m.configureHouse(americans,{country:m.country('Americans'),playerControl:true,allies:[americans,russians]});
  m.configureHouse(russians,{country:m.country('Russians'),allies:[russians]});
  m.createHouse('builder',{country:m.country('Alliance'),credits:750,iq:0,techLevel:6,allies:[]});
  const offline=m.rule('offline',{when:event.sinceEnabled(1),once:false,linked:m.ruleRef('online'),
    actions:[action.setAttachedBuildingsOnline(false),action.disableRule(m.ruleRef('offline')),action.setGlobal(m.globalFlag(0))]});
  m.rule('online',{when:event.sinceEnabled(2),once:false,
    actions:[action.setAttachedBuildingsOnline(true),action.disableRule(m.ruleRef('online')),action.setGlobal(m.globalFlag(1))]});
  const tag=m.tag('plant',{rule:offline,repeat:TagRepeat.Repeat});
  m.bindObject(m.object('Structures:0'),tag);
  m.bindObject(m.object('Structures:2'),tag);
  m.rule('ally',{when:event.sinceEnabled(3),once:true,owner:m.country('Americans'),
    actions:[action.makeAlly(m.country('Russians')),action.setGlobal(m.globalFlag(2))]});
  m.rule('enemy',{when:event.sinceEnabled(4),once:true,owner:m.country('Americans'),
    actions:[action.makeEnemy(m.country('Russians')),action.setGlobal(m.globalFlag(3))]});
  m.rule('build',{when:event.sinceEnabled(5),once:true,owner:m.country('Alliance'),
    actions:[action.createBuilding(m.building('GAPOWR'),m.createWaypoint(30,36,42)),action.setGlobal(m.globalFlag(4))]});
});"#,
    )?;
    mission_script::refresh_types(&entry, &catalog)?;
    let build = Worker::new()?
        .build(&doc, catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?;
    build.build.apply(&mut doc)?;
    let builder_name = mission::ids(&doc, Kind::House)
        .into_iter()
        .find(|id| doc.ini_text(id, "Name") == "builder")
        .context("created builder House")?;
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(root.join("building-house.map"), &bytes)?;
    let mut runtime = campaign::load(&install, &bytes, 0)?;
    let sim = &runtime.simulation;
    let americans = sim.interner().get("Americans").context("Americans")?;
    let russians = sim.interner().get("Russians").context("Russians")?;
    let builder = sim.interner().get(&builder_name).context("builder")?;
    let state = sim.houses().get(&builder).context("builder state")?;
    let credits = state.economy().credits();
    let initial_house = credits == 75_000
        && state.authored_iq() == 0
        && state.tech_level() == 6
        && sim.interner().resolve(state.house_type_id()) == "Alliance";
    ensure!(
        initial_house,
        "TS-created House: credits={credits}, IQ={}, tech={}, country={:?}",
        state.authored_iq(),
        state.tech_level(),
        sim.interner().resolve(state.house_type_id())
    );
    let initial_one_way = sim
        .house_alliances()
        .is_allied(sim.interner(), americans, russians)
        && !sim
            .house_alliances()
            .is_allied(sim.interner(), russians, americans);
    ensure!(
        initial_one_way,
        "configured Allies must retain their one-way meaning"
    );
    std::fs::write(root.join("initial-entities.json"), serde_json::to_vec_pretty(
        &sim.entities().iter_sorted().map(|(id,e)| serde_json::json!({
            "id":id,"type":sim.interner().resolve(e.type_ref()),"owner":sim.interner().resolve(e.owner()),
            "location":[e.position.rx(),e.position.ry()],"get_coords_cell":sim.object_coords_cell(id)
        })).collect::<Vec<_>>()
    )?)?;
    let tagged = sim
        .entities()
        .iter_sorted()
        .find_map(|(id, e)| {
            (e.owner() == americans && (e.position.rx(), e.position.ry()) == (20, 30)).then_some(id)
        })
        .context("tagged barracks")?;
    let untouched = sim
        .entities()
        .iter_sorted()
        .find_map(|(id, e)| {
            (e.owner() == americans && (e.position.rx(), e.position.ry()) == (26, 30)).then_some(id)
        })
        .context("untagged barracks")?;
    let generator = sim
        .entities()
        .iter_sorted()
        .find_map(|(id, e)| {
            (e.owner() == americans && sim.interner().resolve(e.type_ref()) == "NAPOWR")
                .then_some(id)
        })
        .context("tagged power generator")?;
    ensure!(
        sim.entities()
            .get(tagged)
            .is_some_and(|e| e.building_online())
            && sim
                .entities()
                .get(untouched)
                .is_some_and(|e| e.building_online()),
        "initial online plants"
    );
    let mut offline_seen = false;
    let mut online_seen = false;
    let mut ally_seen = false;
    let mut enemy_seen = false;
    let mut untouched_stayed_online = true;
    let mut generator_stayed_online = true;
    let mut created = None;
    for _ in 0..1200 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let sim = &runtime.simulation;
        let flags = sim.global_variables();
        let plant = sim
            .entities()
            .get(tagged)
            .context("tagged plant survives")?;
        untouched_stayed_online &= sim
            .entities()
            .get(untouched)
            .is_some_and(|e| e.building_online());
        generator_stayed_online &= sim
            .entities()
            .get(generator)
            .is_some_and(|e| e.building_online());
        if flags[0] && !flags[1] {
            offline_seen |= !plant.building_online();
        }
        if flags[1] {
            online_seen |= plant.building_online();
        }
        if flags[2] && !flags[3] {
            ally_seen |= sim
                .house_alliances()
                .is_allied(sim.interner(), americans, russians)
                && sim
                    .house_alliances()
                    .is_allied(sim.interner(), russians, americans);
        }
        if flags[3] {
            enemy_seen |= !sim
                .house_alliances()
                .is_allied(sim.interner(), americans, russians)
                && !sim
                    .house_alliances()
                    .is_allied(sim.interner(), russians, americans);
        }
        if flags[4] {
            created = sim.entities().iter_sorted().find_map(|(id, e)| {
                (e.owner() == builder
                    && sim.interner().resolve(e.type_ref()) == "GAPOWR"
                    && (e.position.rx(), e.position.ry()) == (36, 42))
                    .then_some(id)
            });
            if created.is_some() {
                break;
            }
        }
    }
    let passed = offline_seen
        && online_seen
        && untouched_stayed_online
        && generator_stayed_online
        && ally_seen
        && enemy_seen
        && created.is_some();
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "passed":passed, "created_house_initial_parameters":initial_house,
            "created_house_credits":credits, "authored_allies_are_one_way":initial_one_way,
            "attached_building_offline":offline_seen, "attached_building_online_again":online_seen,
            "untagged_building_stayed_online":untouched_stayed_online,
            "tagged_power_generator_stayed_online":generator_stayed_online,
            "make_ally_both_directions":ally_seen, "make_enemy_both_directions":enemy_seen,
            "created_building":created, "created_building_matches_type_owner_and_waypoint":created.is_some(),
            "scope":"TS build, production campaign bootstrap and ordinary frames; no direct state mutation or GUI claim"
        }))?,
    )?;
    ensure!(
        passed,
        "House/diplomacy/building audit failed; see report.json"
    );
    println!("{}", root.join("report.json").display());
    Ok(())
}
