//! TS-owned House display names/entry edges, CSF publication and actual paradrops.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    mission::{self, Kind},
    new_map::NewMapOptions,
};
use ra2_map_io::{
    campaign as publish,
    mission_script::{self, Sources, Worker},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use vera20k_core::{
    map::map_file,
    sim::{
        runtime::SimRuntime,
        world::{FrameEffects, TickLane},
    },
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

fn house_state(runtime: &SimRuntime, owner: &str) -> Result<Value> {
    let sim = &runtime.simulation;
    let id = sim.interner().get(owner).context("house owner")?;
    let h = sim.houses().get(&id).context("house state")?;
    Ok(
        json!({"ui_name":h.ui_name(),"authored_edge":h.authored_edge(),"waypoint_edge":h.waypoint_edge()}),
    )
}
struct HouseExpectation<'a> {
    owner: &'a str,
    label: &'a str,
    edge: i32,
    origin: bool,
}
fn audit(
    install: &Install,
    bytes: &[u8],
    folder: &Path,
    expected: HouseExpectation<'_>,
    restore: bool,
) -> Result<Value> {
    let HouseExpectation {
        owner,
        label,
        edge,
        origin,
    } = expected;
    let mut runtime = campaign::load(install, bytes, 1)?;
    let initial = house_state(&runtime, owner)?;
    let mut checks = BTreeMap::from([
        (
            "initial_house_csf_name",
            initial["ui_name"] == json!({"Csf":label}),
        ),
        ("initial_authored_edge", initial["authored_edge"] == edge),
    ]);
    let mut first_carrier = None;
    let mut restored_ok = false;
    let mut original_passengers = Vec::new();
    let mut observed_landing = false;
    let mut cargo_preserved = true;
    for tick in 1..=1600 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let sim = &runtime.simulation;
        if first_carrier.is_none()
            && let Some((id, e)) = sim
                .entities()
                .iter_sorted()
                .find(|(_, e)| sim.interner().resolve(e.type_ref()) == "PDPLANE")
        {
            original_passengers = e
                .passenger_role
                .cargo()
                .context("carrier cargo")?
                .passengers()
                .to_vec();
            let p = vera20k_core::sim::movement::ground_pose::object_location(
                e,
                sim.resolved_terrain(),
            );
            first_carrier = Some(
                json!({"tick":tick,"id":id,"owner":sim.interner().resolve(e.owner()),"cell":[e.position.rx(),e.position.ry()],"xyz":[p.x,p.y,p.z],"cargo":original_passengers}),
            );
        }
        observed_landing |= original_passengers.len() == 2
            && original_passengers.iter().all(|id| {
                sim.entities().get(*id).is_some_and(|e| {
                    !e.in_limbo()
                        && e.passenger_role.inside_transport().is_none()
                        && vera20k_core::sim::movement::ground_pose::object_location(
                            e,
                            sim.resolved_terrain(),
                        )
                        .z == 0
                })
            });
        if tick == 35 && restore {
            let before = house_state(&runtime, owner)?;
            let carrier = first_carrier
                .as_ref()
                .context("carrier before checkpoint")?["id"]
                .as_u64()
                .unwrap();
            let cargo = runtime
                .simulation
                .entities()
                .get(carrier)
                .context("carrier")?
                .passenger_role
                .cargo()
                .context("cargo")?
                .passengers()
                .to_vec();
            checkpoint::restore_checkpoint(&mut runtime, bytes, &folder.join("in-flight.bin"))?;
            restored_ok = before == house_state(&runtime, owner)?;
            cargo_preserved = runtime
                .simulation
                .entities()
                .get(carrier)
                .and_then(|e| e.passenger_role.cargo())
                .is_some_and(|c| c.passengers() == cargo);
        }
    }
    let carrier = first_carrier.as_ref().context("no actual carrier")?;
    let x = carrier["cell"][0].as_i64().unwrap() as i32;
    let y = carrier["cell"][1].as_i64().unwrap() as i32;
    let bounds = runtime.simulation.playfield_bounds().context("playfield")?;
    let u = ((x - y + (bounds.base & 1)) >> 1) + bounds.base / 2 - bounds.off_fc;
    let v = y - bounds.base + x - bounds.off_100;
    let expected = if (0..=3).contains(&edge) {
        edge
    } else {
        initial["waypoint_edge"].as_i64().unwrap() as i32
    };
    // The ordinary carrier may advance within its first cell. Check the full
    // isometric edge coordinate, independently of random along-edge position.
    let on_edge = if origin {
        x == 20 && y == 45
    } else {
        match expected {
            0 => v <= 1,
            1 => u >= bounds.off_104 - 1,
            2 => v >= bounds.off_108 * 2 - 1,
            3 => u <= 1,
            _ => false,
        }
    };
    checks.insert("actual_carrier_at_requested_origin", on_edge);
    checks.insert("actual_carrier_owner", carrier["owner"] == owner);
    checks.insert(
        "original_passenger_owners",
        original_passengers.len() == 2
            && original_passengers.iter().all(|id| {
                runtime
                    .simulation
                    .entities()
                    .get(*id)
                    .is_some_and(|e| runtime.simulation.interner().resolve(e.owner()) == owner)
            }),
    );
    checks.insert("two_original_passengers", original_passengers.len() == 2);
    checks.insert("original_passengers_really_land", observed_landing);
    checks.insert(
        "original_passengers_alive",
        original_passengers.len() == 2
            && original_passengers
                .iter()
                .all(|id| runtime.simulation.entities().contains(*id)),
    );
    checks.insert(
        "team_script_completes",
        runtime.simulation.global_variables()[40],
    );
    checks.insert(
        "house_name_and_edge_preserved",
        initial == house_state(&runtime, owner)?,
    );
    if restore {
        checks.insert("checkpoint_house_state_preserved", restored_ok);
        checks.insert("checkpoint_cargo_preserved", cargo_preserved);
    }
    Ok(
        json!({"passed":checks.values().all(|v|*v),"checks":checks,"restore":restore,"initial_house":initial,"first_carrier":first_carrier,"local_cell":[u,v],"expected_edge":expected}),
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
    let bytes = NewMapOptions {
        width: 48,
        height: 48,
        ..Default::default()
    }
    .bytes()?;
    let mut base = MapDocument::new(Baseline::parse(
        bytes.clone().into(),
        &map_file::load_from_bytes(bytes)?,
    )?);
    for house in ["Americans", "Russians"] {
        mission::create(&mut base, Kind::House, house, house, None)?;
        base.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    base.add_record("Structures", [20, 35], "Americans", "GAPOWR")?;
    base.add_record("Structures", [60, 42], "Russians", "NAPOWR")?;
    base.commit("House names/edges audit base");
    let mut checks = BTreeMap::new();
    let mut cases = Vec::new();
    for (name, edge, index, origin, create) in [
        ("north", "North", 0, false, false),
        ("east", "East", 1, false, false),
        ("south", "South", 2, false, false),
        ("west", "West", 3, false, false),
        ("automatic", "Automatic", -1, false, false),
        ("air", "Air", 4, false, false),
        ("explicit_origin", "East", 1, true, false),
        ("created_house", "West", 3, false, true),
    ] {
        let folder = root.join(name);
        let mut doc = base.fork();
        let house = if create {
            "const owner=m.createHouse('support',{country:m.country(Country.Alliance),uiName:title,edge:HouseEdge.West,iq:0});".into()
        } else {
            format!(
                "const owner=m.house(House.Americans);m.configureHouse(owner,{{country:m.country(Country.Americans),playerControl:true,iq:0,uiName:title,edge:HouseEdge.{edge}}});"
            )
        };
        let source = format!(
            "import {{defineMission,event,action,order,HouseEdge}} from './sdk';import {{map,House,Country,Unit}} from './map.generated';export default defineMission(map,{{id:'house_{name}',player:House.Americans}},m=>{{m.configureBasic({{ignoreGlobalAiTriggers:true,skipScore:true}});const title=m.defineText('display','友军😀指挥部');{house}m.configureHouse(m.house(House.Russians),{{country:m.country(Country.Russians),iq:0}});const target=m.createWaypoint(30,40,40),origin=m.createWaypoint(31,20,45);const team=m.team('drop',{{house:owner,waypoint:target,units:[{{type:m.unit(Unit.E1),count:2}}],orders:[order.setGlobal(m.globalFlag(40)),order.waitForever()],droppod:true,full:false,reinforce:false,useTransportOrigin:{origin},transportWaypoint:origin}});m.rule('send',{{when:event.sinceScenarioStart(1),once:true,actions:[action.reinforceDefault(team)]}});}});"
        );
        let (bytes, unchanged) = build(&mut doc, &mut install, &folder, &source)?;
        checks.insert(format!("{name}_unchanged_rebuild"), unchanged);
        let owner = if create {
            mission::ids(&doc, Kind::House)
                .into_iter()
                .find(|id| doc.ini_text(id, "Name") == "support")
                .context("created house")?
        } else {
            "Americans".into()
        };
        let label = doc.ini_text(&owner, "UIName");
        let mut payload = BTreeMap::from([
            ("mission.map".into(), bytes.clone()),
            (
                "ra2md.csf".into(),
                mission_script::text_payload(&bytes, &install)?.context("generated CSF")?,
            ),
        ]);
        let project = ra2_map_document::campaign::Project {
            version: 1,
            entry: "mission.map".into(),
            missions: vec![ra2_map_document::campaign::Mission {
                file: "mission.map".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let parsed = map_file::load_from_bytes(bytes.clone())?;
        let report = publish::audit_resources(&project, &[parsed], &payload, &install)?;
        checks.insert(
            format!("{name}_name_csf_dependency_resolved"),
            report.passed
                && report
                    .dependencies
                    .iter()
                    .any(|d| d.kind == "Csf" && d.name == label && d.resolved),
        );
        let csf = vera20k_core::assets::csf_file::CsfFile::from_bytes(&payload["ra2md.csf"])?;
        checks.insert(
            format!("{name}_unicode_csf_exact"),
            csf.get(&label) == Some("友军😀指挥部"),
        );
        payload.remove("ra2md.csf");
        let parsed = map_file::load_from_bytes(bytes.clone())?;
        let missing = publish::audit_resources(&project, &[parsed], &payload, &install)?;
        checks.insert(
            format!("{name}_missing_csf_blocks_publication"),
            !missing.passed
                && missing
                    .dependencies
                    .iter()
                    .any(|d| d.kind == "Csf" && d.name == label && !d.resolved),
        );
        std::fs::write(
            folder.join("dependency-audit.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        for restore in [false, true] {
            let mut result = audit(
                &install,
                &bytes,
                &folder,
                HouseExpectation {
                    owner: &owner,
                    label: &label,
                    edge: index,
                    origin,
                },
                restore,
            )?;
            result["name"] = json!(name);
            std::fs::write(
                folder.join(if restore {
                    "checkpoint-result.json"
                } else {
                    "continuous-result.json"
                }),
                serde_json::to_vec_pretty(&result)?,
            )?;
            cases.push(result);
        }
    }
    let passed = checks.values().all(|v| *v) && cases.iter().all(|c| c["passed"] == true);
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":passed,"checks":checks,"cases":cases,"scope":"Real TS build/rebuild, map House UIName/Edge initialization, actual paradrop carrier origin and landings, CSF dependency checks, public checkpoint. No GPU UI text or transport verification."}),
        )?,
    )?;
    ensure!(passed, "House names/edges audit failed; see report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
