//! TS Droppod reinforcement, carrier origin and in-flight snapshot recovery.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    mission::{self, Kind},
    new_map::NewMapOptions,
};
use ra2_map_io::mission_script::{self, Sources, Worker};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::PathBuf};
use vera20k_core::{
    map::map_file,
    sim::{
        movement::ground_pose::object_location,
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;

fn run(install: &Install, bytes: &[u8], root: &std::path::Path, restore: bool) -> Result<Value> {
    let mut runtime = campaign::load(install, bytes, 0)?;
    let mut observations = Vec::new();
    let mut carrier = None;
    let mut initial_carrier = None;
    let mut passengers = BTreeSet::new();
    let mut falling = BTreeSet::new();
    let mut landed = BTreeSet::new();
    let mut departed = None;
    let mut first_descent = None;
    let mut all_landed = None;
    let mut checkpoint_cargo = None;
    let mut checkpoint_preserved = true;
    let mut ownership_preserved = true;
    for frame in 1..=1400 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let sim = &runtime.simulation;
        if carrier.is_none() {
            for (id, entity) in sim.entities().iter_sorted() {
                if sim.interner().resolve(entity.type_ref()) != "PDPLANE" {
                    continue;
                }
                let cargo = entity.passenger_role.cargo().context("carrier cargo")?;
                passengers.extend(cargo.passengers().iter().copied());
                carrier = Some(id);
                initial_carrier = Some(json!({"frame":frame,"id":id,
                    "cell":[entity.position.rx(),entity.position.ry()],
                    "cargo":cargo.passengers(),"mission":entity.mission.current().raw(),
                    "cargo_matches_team_members":sim.team_script_vm().teams_in_order().any(|t|t.members().collect::<BTreeSet<_>>()==passengers),
                    "in_team":sim.team_script_vm().teams_in_order().any(|t|t.members().any(|member|member==id))}));
                break;
            }
        }
        let mut entities = Vec::new();
        for id in passengers.iter().copied().chain(carrier) {
            let Some(entity) = sim.entities().get(id) else {
                if carrier == Some(id) {
                    departed.get_or_insert(frame);
                }
                entities.push(json!({"id":id,"present":false}));
                continue;
            };
            let location = object_location(entity, sim.resolved_terrain());
            let owner = sim.interner().resolve(entity.owner());
            ownership_preserved &= owner == "Americans";
            let cargo = entity.passenger_role.cargo().map(|c| c.passengers());
            if passengers.contains(&id) && !entity.in_limbo() {
                if location.z > 0 {
                    falling.insert(id);
                    first_descent.get_or_insert(frame);
                } else if entity.passenger_role.inside_transport().is_none() {
                    landed.insert(id);
                }
            }
            entities.push(
                json!({"id":id,"present":true,"type":sim.interner().resolve(entity.type_ref()),
                "owner":owner,"xyz":[location.x,location.y,location.z],"limbo":entity.in_limbo(),
                "inside":entity.passenger_role.inside_transport(),"cargo":cargo,
                "mission":entity.mission.current().raw()}),
            );
        }
        if landed.len() == 4 {
            all_landed.get_or_insert(frame);
        }
        let teams: Vec<_> = sim.team_script_vm().teams_in_order().map(|team|
            json!({"id":team.id(),"cursor":team.cursor(),"members":team.members().collect::<Vec<_>>()})).collect();
        observations.push(
            json!({"frame":frame,"tick":sim.session().tick(),"entities":entities,
            "teams":teams,"script_completed":sim.global_variables()[40]}),
        );
        if frame == 40 {
            checkpoint_cargo = carrier
                .and_then(|id| sim.entities().get(id))
                .and_then(|e| e.passenger_role.cargo())
                .map(|c| c.passengers().to_vec());
            if restore {
                let location = carrier
                    .and_then(|id| sim.entities().get(id))
                    .map(|entity| object_location(entity, sim.resolved_terrain()));
                checkpoint::restore_checkpoint(&mut runtime, bytes, &root.join("in-flight.bin"))?;
                let loaded = &runtime.simulation;
                let loaded_carrier = carrier.and_then(|id| loaded.entities().get(id));
                checkpoint_preserved &= loaded_carrier
                    .map(|entity| object_location(entity, loaded.resolved_terrain()))
                    == location;
                checkpoint_preserved &= loaded_carrier
                    .and_then(|entity| entity.passenger_role.cargo())
                    .map(|cargo| cargo.passengers().to_vec())
                    == checkpoint_cargo;
                checkpoint_preserved &= passengers.iter().all(|id| {
                    loaded.entities().get(*id).is_some_and(|entity| {
                        entity.in_limbo()
                            && entity
                                .passenger_role
                                .inside_transport()
                                .map(|(transport, _)| transport)
                                == carrier
                    })
                });
            }
        }
    }
    let sim = &runtime.simulation;
    let all_survived = passengers.iter().all(|id| sim.entities().contains(*id));
    let checks = json!({
        "carrier_created_with_four_original_members":passengers.len()==4,
        "carrier_cargo_matches_constructed_team":initial_carrier.as_ref().is_some_and(|v|v["cargo_matches_team_members"]==true),
        "carrier_is_not_a_team_member":initial_carrier.as_ref().is_some_and(|v|v["in_team"]==false),
        "checkpoint_precedes_drop":checkpoint_cargo.as_ref().is_some_and(|cargo|cargo.len()==4),
        "checkpoint_carrier_position_and_cargo_preserved":checkpoint_preserved,
        "all_four_members_descended":falling==passengers&&passengers.len()==4,
        "all_four_members_landed_alive":landed==passengers&&passengers.len()==4&&all_survived,
        "original_owners_preserved":ownership_preserved,
        "team_script_ran":sim.global_variables()[40],
        "carrier_left_map":departed.is_some()
    });
    let passed = checks
        .as_object()
        .unwrap()
        .values()
        .all(|value| value == true);
    Ok(
        json!({"passed":passed,"checks":checks,"initial_carrier":initial_carrier,
        "checkpoint_cargo":checkpoint_cargo,"passengers":passengers,"falling":falling,
        "landed":landed,"first_descent_frame":first_descent,"all_landed_frame":all_landed,
        "carrier_departed_frame":departed,"observations":observations}),
    )
}

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
    let mut base = MapDocument::new(Baseline::parse(bytes.into(), &map)?);
    for house in ["Americans", "Russians"] {
        mission::create(&mut base, Kind::House, house, house, None)?;
        base.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    base.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    base.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    base.add_record("Structures", [20, 35], "Americans", "GAPOWR")?;
    base.add_record("Structures", [60, 42], "Russians", "NAPOWR")?;
    base.commit("paradrop fixture");
    let base_bytes = base.snapshot().bytes()?;
    let base_map = map_file::load_from_bytes(base_bytes.clone())?;
    std::fs::write(root.join("base.map"), &base_bytes)?;
    let mut install = Install::open(&game)?;
    let scene = ra2_map_scene::map::load_scene(
        &mut install,
        root.join("base.map").to_str().context("fixture path")?,
    )?;
    let mut cases = Vec::new();
    for authored_origin in [false, true] {
        let name = if authored_origin {
            "transport_waypoint"
        } else {
            "house_edge"
        };
        let case_root = root.join(name);
        std::fs::create_dir(&case_root)?;
        let mut doc = MapDocument::new(Baseline::parse(base_bytes.clone().into(), &base_map)?);
        let entry = case_root.join("mission.ts");
        let source = r#"import {defineMission,event,action,order} from './sdk';
import {map} from './map.generated';
export default defineMission(map,{id:'paradrop',player:'Americans'},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house('Americans'),{country:m.country('Americans'),playerControl:true,iq:0,allies:[m.house('Americans')]});
 m.configureHouse(m.house('Russians'),{country:m.country('Russians'),iq:0,allies:[m.house('Russians')]});
 const target=m.createWaypoint(30,40,40), origin=m.createWaypoint(31,20,45);
 const team=m.team('paratroopers',{house:m.house('Americans'),waypoint:target,
  units:[{type:m.unit('E1'),count:4}],orders:[order.setGlobal(m.globalFlag(40)),order.waitForever()],
  droppod:true,full:false,reinforce:false,useTransportOrigin:__ORIGIN__,transportWaypoint:origin});
 m.rule('dispatch',{when:event.sinceScenarioStart(1),once:true,actions:[action.reinforceDefault(team)]});
});"#.replace("__ORIGIN__", if authored_origin { "true" } else { "false" });
        std::fs::write(&entry, source)?;
        let catalog = mission_script::catalog(&doc, &scene);
        mission_script::refresh_types(&entry, &catalog)?;
        Worker::new()?
            .build(&doc, catalog, Sources::read(&entry)?)?
            .recv()?
            .map_err(anyhow::Error::msg)?
            .build
            .apply(&mut doc)?;
        let bytes = doc.snapshot().bytes()?;
        std::fs::write(case_root.join("paradrop.map"), &bytes)?;
        let mut continuous = run(&install, &bytes, &case_root, false)?;
        let mut restored = run(&install, &bytes, &case_root, true)?;
        let repeat_root = case_root.join("repeat");
        std::fs::create_dir(&repeat_root)?;
        let mut repeated = run(&install, &bytes, &repeat_root, true)?;
        let first_difference = continuous["observations"]
            .as_array()
            .unwrap()
            .iter()
            .zip(restored["observations"].as_array().unwrap())
            .find(|(left, right)| left != right)
            .map(|(left, _)| left["frame"].clone());
        // The snapshot reader intentionally reseeds Scenario RNG. Compare two
        // independent loads for reproducibility; retain the uninterrupted
        // comparison as a separate measurement, without claiming equality.
        let comparison = json!({
            "continuous_and_restored_observations_match":continuous["observations"]==restored["observations"],
            "restored_runs_all_1400_frames_match":restored["observations"]==repeated["observations"],
            "continuous_first_differing_frame":first_difference,
            "load_semantics":"Existing snapshot load resets Scenario RNG to seed 0; subsequent random retreat choices and later frame observations may differ from uninterrupted play. Both restored runs must match, and all three runs must finish the authored payload/script lifecycle."});
        let origin_matches =
            !authored_origin || continuous["initial_carrier"]["cell"] == json!([20, 45]);
        for (kind, result) in [
            ("continuous", &mut continuous),
            ("restored", &mut restored),
            ("repeated", &mut repeated),
        ] {
            let observations = result
                .as_object_mut()
                .unwrap()
                .remove("observations")
                .unwrap();
            std::fs::write(
                case_root.join(format!("{kind}-observations.json")),
                serde_json::to_vec_pretty(&observations)?,
            )?;
        }
        let passed = continuous["passed"] == true
            && restored["passed"] == true
            && repeated["passed"] == true
            && comparison["restored_runs_all_1400_frames_match"] == true
            && origin_matches;
        cases.push(
            json!({"case":name,"passed":passed,"continuous":continuous,"restored":restored,"repeated":repeated,
            "snapshot_comparison":comparison,"authored_carrier_origin_matches":origin_matches}),
        );
    }
    let passed = cases.iter().all(|case| case["passed"] == true);
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&json!({"passed":passed,"cases":cases,
        "scope":"TS build, production campaign bootstrap and ordinary frames. Snapshot compares listed entity/team/script observations, not all simulation state or GUI save/load. No engine changes or event injection."}))?,
    )?;
    ensure!(passed, "paradrop audit failed; see report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
