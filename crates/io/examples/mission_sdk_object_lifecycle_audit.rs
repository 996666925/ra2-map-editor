//! TS ownership/control and initial-object restoration through ordinary frames.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    mission::{self, Kind},
    new_map::NewMapOptions,
};
use ra2_map_io::mission_script::{self, Sources, Worker};
use std::path::{Path, PathBuf};
use vera20k_core::{
    map::{entities::EntityCategory, map_file},
    sim::{
        runtime::SimRuntime,
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;

fn build(install: &mut Install, root: &Path, name: &str, rules: &str) -> Result<Vec<u8>> {
    let root = root.join(name);
    std::fs::create_dir(&root)?;
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
    doc.add_record("Structures", [20, 30], "Americans", "NAPOWR")?;
    doc.add_record("Structures", [30, 40], "Russians", "NAPOWR")?;
    doc.add_record("Infantry", [25, 35], "Russians", "ENGINEER")?;
    doc.add_record("Units", [26, 36], "Russians", "AMCV")?;
    doc.add_record("Aircraft", [27, 37], "Russians", "ORCA")?;
    doc.commit("object lifecycle fixture");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(
        install,
        root.join("base.map").to_str().context("fixture path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let entry = root.join("mission.ts");
    std::fs::write(
        &entry,
        format!(
            r#"import {{defineMission,event,action,HouseObjectSubset}} from './sdk';
import {{map}} from './map.generated';
export default defineMission(map,{{id:'{name}',player:'Americans'}},m=>{{
  m.configureBasic({{ignoreGlobalAiTriggers:true,skipScore:true}});
  m.configureHouse(m.house('Americans'),{{country:m.country('Americans'),playerControl:true,allies:[m.house('Americans')]}});
  m.configureHouse(m.house('Russians'),{{country:m.country('Russians'),allies:[m.house('Russians')]}});
  const target=m.country('Russians');
  {rules}
}});"#
        ),
    )?;
    mission_script::refresh_types(&entry, &catalog)?;
    let build = Worker::new()?
        .build(&doc, catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?;
    build.build.apply(&mut doc)?;
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(root.join("compiled.map"), &bytes)?;
    Ok(bytes)
}

fn authored(runtime: &SimRuntime) -> Result<(u64, Vec<u64>)> {
    let sim = &runtime.simulation;
    let american = sim.interner().get("Americans").context("Americans")?;
    let russian = sim.interner().get("Russians").context("Russians")?;
    let witness = sim
        .entities()
        .iter_sorted()
        .find_map(|(id, e)| (e.owner() == american).then_some(id))
        .context("American witness plant")?;
    let targets: Vec<_> = sim
        .entities()
        .iter_sorted()
        .filter_map(|(id, e)| (e.owner() == russian).then_some(id))
        .collect();
    ensure!(
        targets.len() == 4,
        "four authored categories must spawn; found {:?}",
        targets
            .iter()
            .map(|id| sim
                .interner()
                .resolve(sim.entities().get(*id).unwrap().type_ref()))
            .collect::<Vec<_>>()
    );
    for (name, category) in [
        ("NAPOWR", EntityCategory::Structure),
        ("ENGINEER", EntityCategory::Infantry),
        ("AMCV", EntityCategory::Unit),
        ("ORCA", EntityCategory::Aircraft),
    ] {
        ensure!(
            targets
                .iter()
                .any(|id| sim
                    .entities()
                    .get(*id)
                    .is_some_and(|e| e.category() == category
                        && sim.interner().resolve(e.type_ref()) == name)),
            "initial {name}"
        );
    }
    Ok((witness, targets))
}

fn control(install: &mut Install, root: &Path) -> Result<serde_json::Value> {
    let bytes = build(
        install,
        root,
        "ownership",
        r#"
  m.rule('take',{when:event.sinceScenarioStart(1),once:true,
    actions:[action.controlHouseBuildings(target),action.setGlobal(m.globalFlag(0))]});
  m.rule('return',{when:event.sinceScenarioStart(4),once:true,
    actions:[action.controlHouseBuildings(target,true),action.setGlobal(m.globalFlag(1))]});
  m.rule('transfer_all',{when:event.sinceScenarioStart(7),once:true,owner:target,
    actions:[action.changeAllHouseObjectsOwner(m.country('Americans')),action.setGlobal(m.globalFlag(2))]});
"#,
    )?;
    let mut runtime = campaign::load(install, &bytes, 0)?;
    let (witness, targets) = authored(&runtime)?;
    let american = runtime.simulation.interner().get("Americans").unwrap();
    let russian = runtime.simulation.interner().get("Russians").unwrap();
    let mut controlled = false;
    let mut returned = false;
    let mut transferred = false;
    let mut witness_intact = true;
    for _ in 0..400 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let sim = &runtime.simulation;
        let flags = sim.global_variables();
        witness_intact &= sim
            .entities()
            .get(witness)
            .is_some_and(|e| e.owner() == american);
        if flags[0] && !flags[1] {
            controlled |= targets.iter().all(|id| {
                sim.entities().get(*id).is_some_and(|e| {
                    e.owner()
                        == if e.category() == EntityCategory::Structure {
                            american
                        } else {
                            russian
                        }
                })
            });
        }
        if flags[1] && !flags[2] {
            returned |= targets.iter().all(|id| {
                sim.entities()
                    .get(*id)
                    .is_some_and(|e| e.owner() == russian)
            });
        }
        if flags[2] {
            transferred = targets.iter().all(|id| {
                sim.entities()
                    .get(*id)
                    .is_some_and(|e| e.owner() == american)
            });
            break;
        }
    }
    Ok(
        serde_json::json!({"passed":controlled&&returned&&transferred&&witness_intact,
        "only_buildings_controlled":controlled,"controlled_buildings_returned":returned,
        "transfer_all_four_categories":transferred,"other_house_objects_untouched":witness_intact}),
    )
}

fn restore(install: &mut Install, root: &Path) -> Result<serde_json::Value> {
    let bytes = build(
        install,
        root,
        "restoration",
        r#"
  m.rule('destroy',{when:event.sinceScenarioStart(1),once:true,
    actions:[action.destroyHouseObjects(target,HouseObjectSubset.All),action.setGlobal(m.globalFlag(0))]});
  m.rule('buildings',{when:event.sinceScenarioStart(8),once:true,
    actions:[action.restoreInitialObjects(target,true),action.setGlobal(m.globalFlag(1))]});
  m.rule('all',{when:event.sinceScenarioStart(16),once:true,
    actions:[action.restoreInitialObjects(target),action.setGlobal(m.globalFlag(2))]});
"#,
    )?;
    let mut runtime = campaign::load(install, &bytes, 0)?;
    let (witness, targets) = authored(&runtime)?;
    let american = runtime.simulation.interner().get("Americans").unwrap();
    let russian = runtime.simulation.interner().get("Russians").unwrap();
    let expected: Vec<_> = targets
        .iter()
        .map(|id| {
            let e = runtime.simulation.entities().get(*id).unwrap();
            (
                runtime
                    .simulation
                    .interner()
                    .resolve(e.type_ref())
                    .to_owned(),
                (e.position.rx(), e.position.ry()),
            )
        })
        .collect();
    let mut removed = false;
    let mut only_buildings = false;
    let mut all_restored = false;
    let mut witness_intact = true;
    let mut restored_types = Vec::new();
    for _ in 0..600 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let sim = &runtime.simulation;
        let flags = sim.global_variables();
        witness_intact &= sim
            .entities()
            .get(witness)
            .is_some_and(|e| e.owner() == american);
        let current: Vec<_> = sim
            .entities()
            .iter_sorted()
            .filter(|(_, e)| e.owner() == russian)
            .collect();
        if flags[0] && !flags[1] {
            removed |= targets.iter().all(|id| sim.entities().get(*id).is_none());
        }
        if flags[1] && !flags[2] {
            only_buildings |= current.len() == 1
                && current.iter().all(|(_, e)| {
                    e.category() == EntityCategory::Structure
                        && sim.interner().resolve(e.type_ref()) == "NAPOWR"
                        && (e.position.rx(), e.position.ry()) == (30, 40)
                });
        }
        if flags[2] {
            all_restored = current.len() == 4
                && expected.iter().all(|(kind, cell)| {
                    current.iter().any(|(_, e)| {
                        sim.interner().resolve(e.type_ref()) == kind
                            && (e.position.rx(), e.position.ry()) == *cell
                    })
                });
            restored_types = current
                .iter()
                .map(|(_, e)| sim.interner().resolve(e.type_ref()).to_owned())
                .collect();
            if all_restored {
                break;
            }
        }
    }
    Ok(
        serde_json::json!({"passed":removed&&only_buildings&&all_restored&&witness_intact,
        "destroy_all_four_categories":removed,"buildings_only_restored":only_buildings,
        "all_initial_types_owners_cells_restored":all_restored,"restored_types":restored_types,
        "other_house_objects_untouched":witness_intact}),
    )
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(PathBuf::from(args.next().context("new output directory")?))?;
    ensure!(!root.exists(), "请选择新的审计目录");
    std::fs::create_dir_all(&root)?;
    let mut install = Install::open(&game)?;
    let ownership = control(&mut install, &root)?;
    let restoration = restore(&mut install, &root)?;
    let passed = ownership["passed"] == true && restoration["passed"] == true;
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "passed":passed,"ownership":ownership,"restoration":restoration,
            "scope":"TS build, production campaign bootstrap and ordinary frames; no state mutation, event injection or GUI claim"
        }))?,
    )?;
    ensure!(passed, "object lifecycle audit failed; see report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
