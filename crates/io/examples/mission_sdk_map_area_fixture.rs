//! TS Action40 geometry, equal writers, restoration and public checkpoints.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    new_map::NewMapOptions,
};
use ra2_map_io::mission_script::{self, Sources, Worker};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};
use vera20k_core::{
    headless_scenario::{battle_session, load_session},
    rules::locomotor_type::MovementZone,
    sim::{
        movement::locomotor::MovementLayer,
        world::{FrameEffects, Simulation, TickLane},
    },
    skirmish_launch::LaunchStartPosition,
};
#[path = "support/checkpoint.rs"]
mod checkpoint;

fn state(sim: &Simulation) -> Value {
    let cells=[[60,62],[38,70],[38,68]].map(|[x,y]| {
        let cell=sim.resolved_terrain().and_then(|g|g.cell(x,y));
        let zone=sim.zone_grid().and_then(|g|g.map_for(MovementZone::Normal)).map(|m|m.zone_at(x,y,MovementLayer::Ground));
        json!({"cell":[x,y],"outside":cell.map(|c|c.outside_playfield),"zone_type":cell.map(|c|c.zone_type),"zone":zone})
    });
    let blocked = (36..=41u16)
        .flat_map(|x| (66..=71u16).map(move |y| (x, y)))
        .filter(|&(x, y)| {
            sim.path_grid()
                .and_then(|g| g.cell(x, y))
                .is_some_and(|c| !c.ground_walkable)
        })
        .map(|(x, y)| [x, y])
        .collect::<Vec<_>>();
    let objects=sim.entities().iter_sorted().map(|(id,e)|json!({"id":id,"type":sim.interner().resolve(e.type_ref()),"owner":sim.interner().resolve(e.owner()),"cell":[e.position.rx(),e.position.ry()],"in_playfield":e.in_playfield(),"health":e.health.current()})).collect::<Vec<_>>();
    json!({"tick":sim.session().tick(),"bounds":sim.playfield_bounds(),"revision":sim.playfield_revision(),"cells":cells,"objects":objects,"blocked":blocked})
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
    base.add_record("Structures", [58, 60], "Neutral", "GAPOWR")?;
    base.add_record("Structures", [38, 68], "Neutral", "GAPOWR")?;
    for at in [[60, 60], [38, 70]] {
        base.add_record("Infantry", at, "Neutral", "E1")?;
    }
    base.add_record("Structures", [54, 60], "Neutral", "GAAIRC")?;
    base.waypoint(0, [60, 60])?;
    base.commit("map area geometry");
    let worker = Worker::new()?;
    let mut checks = BTreeMap::new();
    let mut cases = Vec::new();
    for name in ["control", "shrink", "cycle"] {
        let folder = root.join(name);
        std::fs::create_dir(&folder)?;
        let map = folder.join("mission.map");
        let entry = folder.join("mission.ts");
        let mut doc = base.fork();
        std::fs::write(&map, doc.snapshot().bytes()?)?;
        let scene =
            ra2_map_scene::map::load_scene(&mut install, map.to_str().context("map path")?)?;
        let catalog = mission_script::catalog(&doc, &scene);
        let shrink = if name == "control" {
            ""
        } else {
            "m.rule('shrink',{when:event.sinceScenarioStart(10),once:true,actions:[action.visibleMapArea(20,20,24,24)]});"
        };
        let restore = if name == "cycle" {
            "m.rule('same',{when:event.sinceScenarioStart(12),once:true,actions:[action.visibleMapArea(20,20,24,24)]});m.rule('restore',{when:event.sinceScenarioStart(14),once:true,actions:[action.visibleMapArea(2,4,60,54)]});"
        } else {
            ""
        };
        std::fs::write(
            &entry,
            format!(
                "import {{defineMission,MissionMode,event,action}} from './sdk';import {{map,MapObject}} from './map.generated';export default defineMission(map,{{id:'area_{name}',mode:MissionMode.Multiplayer}},m=>{{m.configureBasic({{freeRadar:true,ignoreGlobalAiTriggers:true}});m.assignObjectToPlayer(m.object(MapObject.Structures_0),0);m.assignObjectToPlayer(m.object(MapObject.Structures_2),0);m.assignObjectToPlayer(m.object(MapObject.Infantry_0),0);m.assignObjectToPlayer(m.object(MapObject.Infantry_1),0);{shrink}{restore}}});"
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
            format!("{name}_unchanged_rebuild"),
            !worker
                .build(&doc, catalog, Sources::read(&entry)?)?
                .recv()?
                .map_err(anyhow::Error::msg)?
                .build
                .apply(&mut doc)?,
        );
        let bytes = doc.snapshot().bytes()?;
        std::fs::write(&map, &bytes)?;
        let filename = map.to_str().context("map path")?;
        let mut launch = battle_session(filename, &[]);
        launch.humans[0].name = "SDK area".into();
        launch.humans[0].slot.start_position = LaunchStartPosition::Position(0);
        launch.options.unit_count = 0;
        launch.options.short_game = false;
        launch.options.crates = false;
        launch.options.shroud = false;
        launch.options.fog_of_war = false;
        launch.rebuild_house_roster();
        let mut timelines = Vec::new();
        for saved_at in [None, Some(165), Some(195)] {
            let mut scenario =
                load_session(&game, filename, 12345, launch.clone()).map_err(anyhow::Error::msg)?;
            let mut observations = Vec::new();
            let mut preserved = true;
            for tick in 0..=240 {
                if tick > 0 {
                    scenario.runtime.advance_frame(
                        &[],
                        22,
                        TickLane::Ordinary,
                        FrameEffects::default(),
                    )?;
                }
                if saved_at == Some(tick) {
                    let before = state(scenario.sim());
                    checkpoint::restore_checkpoint(
                        &mut scenario.runtime,
                        &bytes,
                        &folder.join(format!("checkpoint-{tick}.bin")),
                    )?;
                    preserved &= before == state(scenario.sim());
                }
                observations.push(state(scenario.sim()));
            }
            checks.insert(
                format!(
                    "{name}_{}_building_blockers_retained",
                    saved_at.unwrap_or(0)
                ),
                !observations[0]["blocked"].as_array().unwrap().is_empty()
                    && observations
                        .iter()
                        .all(|row| row["blocked"] == observations[0]["blocked"]),
            );
            let identity = |row: &Value| {
                row["objects"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| json!([e["id"], e["type"], e["cell"], e["health"]]))
                    .collect::<Vec<_>>()
            };
            checks.insert(
                format!(
                    "{name}_{}_original_objects_preserved",
                    saved_at.unwrap_or(0)
                ),
                observations
                    .iter()
                    .all(|row| identity(row) == identity(&observations[0])),
            );
            let suffix =
                saved_at.map_or("continuous".to_string(), |tick| format!("restored-{tick}"));
            checks.insert(
                format!("{name}_{suffix}_checkpoint_observed_state"),
                preserved,
            );
            let at = |tick: usize| &observations[tick];
            checks.insert(
                format!("{name}_{suffix}_initial_geometry"),
                at(0)["revision"] == 0
                    && at(0)["cells"][1]["outside"] == false
                    && at(0)["cells"][1]["zone"].as_u64().is_some_and(|v| v > 0),
            );
            for tick in [150, 151, 180, 181, 210, 211, 240] {
                let cropped = name != "control" && tick >= 151 && (name != "cycle" || tick < 211);
                let expected_revision = if name == "control" {
                    0
                } else if name == "cycle" {
                    u64::from(tick >= 151) + u64::from(tick >= 181) + u64::from(tick >= 211)
                } else {
                    u64::from(tick >= 151)
                };
                let row = at(tick);
                checks.insert(format!("{name}_{suffix}_{tick}_bounds_revision"),row["revision"]==expected_revision && row["bounds"]==json!({"base":64,"off_fc":if cropped {20}else{2},"off_100":if cropped {20}else{4},"off_104":if cropped {24}else{60},"off_108":if cropped {24}else{54}}));
                checks.insert(
                    format!("{name}_{suffix}_{tick}_cell_and_zone"),
                    row["cells"][0]["outside"] == false
                        && row["cells"][0]["zone"].as_u64().is_some_and(|v| v > 0)
                        && row["cells"][1]["outside"] == cropped
                        && (row["cells"][1]["zone"] == 0) == cropped
                        && (row["cells"][2]["zone"] == 0) == cropped,
                );
                let objects = row["objects"].as_array().unwrap();
                checks.insert(
                    format!("{name}_{suffix}_{tick}_original_membership"),
                    objects.len() == 6
                        && objects.iter().all(|e| {
                            e["health"].as_i64().is_some_and(|v| v > 0)
                                && e["in_playfield"] == !(cropped && e["cell"][0] == 38)
                        }),
                );
            }
            std::fs::write(
                folder.join(format!("{suffix}.json")),
                serde_json::to_vec_pretty(&observations)?,
            )?;
            timelines.push(observations);
        }
        checks.insert(
            format!("{name}_restored_timelines_equal"),
            timelines[0] == timelines[1] && timelines[0] == timelines[2],
        );
        for tick in [0, 165, 195, 240] {
            let mut profile = json!({"schema_version":"vera20k.map-observation-profile.v2","launch":launch,"seed":12345,"input_delay_ticks":15,"ticks":tick,"width":1024,"height":768,"timeout_seconds":120,"observe_owners":["SDK area","Neutral"],"observe_types":["E1","GAPOWR"],"camera_cell":[60,60],"cursor_position":[1000,700],"terrain_cells":[[60,62],[38,70]],"observe_sidebar_steps":[tick]});
            if tick > 0 {
                profile["gestures"] = json!([{"issue_after_step":tick-1,"gesture":{"kind":"minimap","position":[940,100]}}]);
            }
            std::fs::write(
                folder.join(format!("profile-{tick}.json")),
                serde_json::to_vec_pretty(&profile)?,
            )?;
            cases.push(json!({"name":name,"tick":tick,"profile":format!("{name}/profile-{tick}.json"),"map":format!("{name}/mission.map")}));
        }
    }
    let passed = checks.values().all(|v| *v);
    std::fs::write(
        root.join("fixture.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":passed,"checks":checks,"cases":cases,"scope":"TS Action40 geometry and zone membership, public checkpoint at 165/195 and all listed observations through tick240. GPU radar/gesture capture is separate. No engine changes or all-state equivalence claim."}),
        )?,
    )?;
    ensure!(passed, "map-area audit failed; see fixture.json");
    println!("{}", root.join("fixture.json").display());
    Ok(())
}
