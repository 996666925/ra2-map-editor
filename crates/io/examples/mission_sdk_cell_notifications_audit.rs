//! TS line, spy identity and airborne CellTag notifications from normal movement.
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
    map::map_file,
    sim::{
        command::{Command, CommandEnvelope},
        runtime::SimRuntime,
        trigger_runtime::TriggerEffect,
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;

type Route = ([u16; 2], [u16; 2]);
fn routes(name: &str) -> Vec<Route> {
    match name {
        "horizontal" => vec![([30, 45], [30, 55])],
        "vertical" => vec![([45, 70], [55, 70])],
        "combined" => vec![([30, 60], [30, 70]), ([55, 80], [65, 80])],
        "airborne" => vec![([30, 50], [40, 50])],
        _ => vec![([30, 50], [42, 50])],
    }
}
fn tag_cell(name: &str) -> [u16; 2] {
    match name {
        "horizontal" => [60, 50],
        "vertical" => [50, 90],
        "combined" => [60, 65],
        _ => [40, 50],
    }
}
fn build(install: &mut Install, root: &Path, name: &str) -> Result<Vec<u8>> {
    std::fs::create_dir_all(root)?;
    let initial = NewMapOptions {
        width: 64,
        height: 64,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(initial.clone())?;
    let mut doc = MapDocument::new(Baseline::parse(initial.into(), &map)?);
    for house in ["Russians", "Americans"] {
        mission::create(&mut doc, Kind::House, house, house, None)?;
        doc.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    doc.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    doc.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    doc.add_record("Structures", [28, 52], "Americans", "GAPOWR")?;
    doc.add_record("Structures", [82, 82], "Russians", "NAPOWR")?;
    let actor = if name.starts_with("spy_") {
        "SPY"
    } else if name == "airborne" {
        "JUMPJET"
    } else {
        "ENGINEER"
    };
    for (start, _) in routes(name) {
        doc.add_record("Infantry", start, "Americans", actor)?;
    }
    if name.starts_with("spy_") || name == "non_agent" {
        doc.add_record("Infantry", [32, 48], "Russians", "E2")?;
    }
    for (section, key, value) in [
        ("E2", "Primary", "none"),
        ("E2", "Secondary", "none"),
        ("E2", "Speed", "0"),
        ("JUMPJET", "Primary", "none"),
    ] {
        doc.set_ini(section, key, Some(value.as_bytes().to_vec()))?;
    }
    if name == "spy_default" {
        doc.set_ini("SPY", "Primary", Some(b"none".to_vec()))?;
    }
    doc.commit("cell notification fixture");
    std::fs::write(root.join("base.map"), doc.snapshot().bytes()?)?;
    let scene = ra2_map_scene::map::load_scene(
        install,
        root.join("base.map").to_str().context("map path")?,
    )?;
    let catalog = mission_script::catalog(&doc, &scene);
    let conditions = match name {
        "horizontal" => {
            "[['horizontal',event.crossedHorizontal(american)],['wrong_country',event.crossedHorizontal(russian)]]"
        }
        "vertical" => {
            "[['vertical',event.crossedVertical(american)],['wrong_country',event.crossedVertical(russian)]]"
        }
        "combined" => {
            "[['horizontal',event.crossedHorizontal(american)],['vertical',event.crossedVertical(american)],['wrong_country',event.crossedHorizontal(russian)]]"
        }
        _ => {
            "[['entered',event.entered(american)],['overflown',event.enteredOrOverflown(american)],['american_identity',event.spyDisguisedAsCountry(american)],['russian_identity',event.spyDisguisedAsCountry(russian)],['e1_identity',event.spyDisguisedAsInfantry(m.techno(Techno.E1))],['e2_identity',event.spyDisguisedAsInfantry(m.techno(Techno.E2))],['wrong_type',event.spyDisguisedAsInfantry(m.techno(Techno.INIT))]]"
        }
    };
    let [x, y] = tag_cell(name);
    let source=r#"import {defineMission,event,action,TagRepeat} from './sdk';
import {map,House,Country,Techno} from './map.generated';
export default defineMission(map,{id:'cell_notifications',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),iq:0,playerControl:true,allies:[m.house(House.Americans)]});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),iq:0,allies:[m.house(House.Russians)]});
 const american=m.country(Country.Americans),russian=m.country(Country.Russians);
 const specs=__CONDITIONS__ as const;
 for(let i=0;i<specs.length;i++) {
   const [id,when]=specs[i]!;
   m.rule(id,{when,once:false,...(i+1<specs.length?{linked:m.ruleRef(specs[i+1]![0])}:{}),
     actions:[action.message(m.defineText(id,id)),action.setGlobal(m.globalFlag(i)),action.disableRule(m.ruleRef(id))]});
 }
 m.bindCell(__X__,__Y__,m.tag('cell',{rule:m.ruleRef(specs[0][0]),repeat:TagRepeat.Repeat}));
});"#.replace("__CONDITIONS__",conditions).replace("__X__",&x.to_string()).replace("__Y__",&y.to_string());
    let entry = root.join("mission.ts");
    std::fs::write(&entry, source)?;
    mission_script::refresh_types(&entry, &catalog)?;
    Worker::new()?
        .build(&doc, catalog, Sources::read(&entry)?)?
        .recv()?
        .map_err(anyhow::Error::msg)?
        .build
        .apply(&mut doc)?;
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(root.join("compiled.map"), &bytes)?;
    Ok(bytes)
}

fn actor_state(runtime: &SimRuntime, actor: u64) -> Result<Value> {
    let sim = &runtime.simulation;
    let entity = sim.entities().get(actor).context("actor alive")?;
    let coord = vera20k_core::sim::movement::ground_pose::position_world_coord(&entity.position);
    Ok(
        json!({"id":actor,"cell":sim.object_coords_cell(actor),"coord":[coord.x,coord.y,coord.z],
        "health":entity.health.current(),"owner":sim.interner().resolve(entity.owner()),
        "type":sim.interner().resolve(entity.type_ref()),
        "disguise":entity.disguise.as_ref().map(|d|json!({"active":d.is_disguised(),
            "type":d.type_id().map(|id|sim.interner().resolve(id)),"house":d.house().map(|id|sim.interner().resolve(id))}))}),
    )
}

fn audit(install: &Install, bytes: &[u8], root: &Path, name: &str, restore: bool) -> Result<Value> {
    std::fs::create_dir_all(root)?;
    let mut runtime = campaign::load(install, bytes, 1)?;
    let ids = routes(name)
        .iter()
        .map(|(start, _)| {
            runtime
                .simulation
                .entities()
                .iter_sorted()
                .find(|(_, e)| [e.position.rx(), e.position.ry()] == *start)
                .map(|(id, _)| id)
                .context("moving actor")
        })
        .collect::<Result<Vec<_>>>()?;
    let owner = runtime
        .simulation
        .interner()
        .get("Americans")
        .context("owner")?;
    let texts = mission_script::declared_texts(bytes)?;
    let initial = ids
        .iter()
        .map(|id| actor_state(&runtime, *id))
        .collect::<Result<Vec<_>>>()?;
    let witnesses = runtime
        .simulation
        .entities()
        .iter_sorted()
        .filter(|(id, _)| !ids.contains(id))
        .map(|(id, e)| {
            (
                id,
                e.health.current(),
                runtime.simulation.object_coords_cell(id),
            )
        })
        .collect::<Vec<_>>();
    let mut commands = Vec::new();
    if name == "spy_copied" {
        let target = runtime
            .simulation
            .entities()
            .iter_sorted()
            .find(|(_, e)| runtime.simulation.interner().resolve(e.type_ref()) == "E2")
            .context("disguise model")?
            .0;
        commands.push(
            runtime
                .simulation
                .encode_megamission_record(
                    owner,
                    &Command::Attack {
                        attacker_id: ids[0],
                        target_id: target,
                    },
                )
                .context("ordinary MakeupKit attack")?,
        );
    }
    let mut messages = Vec::new();
    let mut observations = Vec::new();
    let mut witnesses_preserved = true;
    let mut checkpoint_state = None;
    let mut before_move = None;
    for frame in 0..700 {
        if frame == 60 {
            before_move = Some(actor_state(&runtime, ids[0])?);
            for (id, (_, destination)) in ids.iter().zip(routes(name)) {
                commands.push(
                    runtime
                        .simulation
                        .encode_megamission_record(
                            owner,
                            &Command::Move {
                                entity_id: *id,
                                target_rx: destination[0],
                                target_ry: destination[1],
                            },
                        )
                        .context("ordinary move command")?,
                );
            }
        }
        let tick = runtime.simulation.session().tick();
        let visit_binary_frame = runtime.simulation.session().binary_frame();
        let envelopes = std::mem::take(&mut commands)
            .into_iter()
            .map(|c| CommandEnvelope::synchronized(c, tick))
            .collect::<Vec<_>>();
        let output =
            runtime.advance_frame(&envelopes, 22, TickLane::Ordinary, FrameEffects::default())?;
        let sim = &runtime.simulation;
        let tick = sim.session().tick();
        let actors = ids
            .iter()
            .map(|id| actor_state(&runtime, *id))
            .collect::<Result<Vec<_>>>()?;
        let binary_frame = sim.session().binary_frame();
        let row = json!({"tick":tick,"binary_frame":binary_frame,"visit_binary_frame":visit_binary_frame,"actors":actors,"flags":sim.global_variables()[..7]});
        for effect in output.trigger_effects {
            if let TriggerEffect::LocalizedMessage { csf_label } = effect {
                messages.push(json!({"tick":tick,"binary_frame":binary_frame,"visit_binary_frame":visit_binary_frame,"event":texts.get(&csf_label).context("message label")?,"actors":actors}));
            }
        }
        witnesses_preserved &= witnesses.iter().all(|(id, health, cell)| {
            sim.entities().get(*id).is_some_and(|e| {
                e.health.current() == *health && sim.object_coords_cell(*id) == *cell
            })
        });
        observations.push(row);
        if restore && checkpoint_state.is_none() && frame > 60 {
            let cell = sim.object_coords_cell(ids[0]).context("actor cell")?;
            let middle = match name {
                "horizontal" => cell == (30, 48),
                _ => cell == (34, 50),
            };
            if middle {
                let before = json!({"actor":actors[0],"flags":sim.global_variables().as_slice()});
                checkpoint::restore_checkpoint(&mut runtime, bytes, &root.join("in-transit.bin"))?;
                let after = json!({"actor":actor_state(&runtime,ids[0])?,"flags":runtime.simulation.global_variables().as_slice()});
                checkpoint_state = Some(
                    json!({"tick":tick,"preserved":before==after,"before":before,"after":after}),
                );
            }
        }
    }
    let count = |event: &str| messages.iter().filter(|m| m["event"] == event).count();
    let expected: &[&str] = match name {
        "horizontal" | "combined" => &["horizontal"],
        "vertical" => &["vertical"],
        "spy_default" => &["entered", "overflown", "american_identity", "e1_identity"],
        "spy_copied" => &["entered", "overflown", "russian_identity", "e2_identity"],
        "non_agent" => &["entered", "overflown"],
        "airborne" => &["overflown"],
        _ => unreachable!(),
    };
    let map_header = map_file::load_from_bytes(bytes.to_vec())?.header;
    let expected_extent = u16::try_from(map_header.width + map_header.height)?;
    let map_cell_extent = [
        runtime.simulation.session().map_width(),
        runtime.simulation.session().map_height(),
    ];
    let mut checks = BTreeMap::from([
        (
            "session_cell_array_matches_native_map_size",
            map_cell_extent == [expected_extent; 2],
        ),
        (
            "only_expected_notifications_once",
            messages.len() == expected.len() && expected.iter().all(|e| count(e) == 1),
        ),
        (
            "all_destinations_reached",
            ids.iter().zip(routes(name)).all(|(id, (_, destination))| {
                runtime.simulation.object_coords_cell(*id) == Some((destination[0], destination[1]))
            }),
        ),
        (
            "original_actor_types_owners_and_health_preserved",
            ids.iter().enumerate().all(|(i, id)| {
                actor_state(&runtime, *id).is_ok_and(|a| {
                    ["type", "owner", "health"]
                        .iter()
                        .all(|key| a[key] == initial[i][key])
                })
            }),
        ),
        ("witnesses_unchanged", witnesses_preserved),
        (
            "no_notifications_before_movement",
            messages.iter().all(|m| m["tick"].as_u64().unwrap() > 60),
        ),
    ]);
    if name.starts_with("spy_") {
        let (kind, house) = if name == "spy_copied" {
            ("E2", "Russians")
        } else {
            ("E1", "Americans")
        };
        checks.insert(
            "spy_stores_expected_identity_before_moving",
            before_move.as_ref().is_some_and(|v| {
                v["disguise"]["active"] == true
                    && v["disguise"]["type"] == kind
                    && v["disguise"]["house"] == house
            }),
        );
        checks.insert(
            "spy_callback_identity_and_actual_owner_differ_when_copied",
            messages.iter().all(|m| {
                m["actors"][0]["owner"] == "Americans"
                    && m["actors"][0]["disguise"]["type"] == kind
                    && m["actors"][0]["disguise"]["house"] == house
            }),
        );
    }
    if matches!(name, "horizontal" | "vertical" | "combined") {
        let cell = tag_cell(name);
        checks.insert(
            "line_triggered_away_from_tagged_cell",
            messages
                .iter()
                .all(|m| m["actors"][0]["cell"] != json!(cell)),
        );
        checks.insert(
            "line_callback_at_crossed_row_or_column",
            messages.iter().all(|m| {
                if name == "vertical" {
                    m["actors"][0]["cell"][0] == 50
                } else {
                    m["actors"][0]["cell"][1] == if name == "horizontal" { 50 } else { 65 }
                }
            }),
        );
    }
    if name == "airborne" {
        checks.insert(
            "airborne_notification_uses_native_sixteen_frame_visit",
            messages.iter().all(|m| {
                m["visit_binary_frame"].as_u64().unwrap() % 16 == 0
                    && m["actors"][0]["coord"][2].as_i64().unwrap() > 0
            }),
        );
    }
    if restore {
        checks.insert(
            "in_transit_checkpoint_preserves_actor_and_flags",
            checkpoint_state
                .as_ref()
                .is_some_and(|c| c["preserved"] == true),
        );
    }
    let passed = checks.values().all(|v| *v);
    std::fs::write(
        root.join("observations.json"),
        serde_json::to_vec_pretty(&observations)?,
    )?;
    if let Some(state) = checkpoint_state {
        std::fs::write(
            root.join("checkpoint-state.json"),
            serde_json::to_vec_pretty(&state)?,
        )?;
    }
    Ok(
        json!({"passed":passed,"checks":checks,"map_cell_extent":map_cell_extent,"initial":initial,"before_move":before_move,"messages":messages}),
    )
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(args.next().context("new output directory")?)?;
    ensure!(
        args.next().is_none() && !root.exists(),
        "output must be new"
    );
    std::fs::create_dir_all(&root)?;
    let mut install = Install::open(&game)?;
    let mut cases = Vec::new();
    for name in [
        "horizontal",
        "vertical",
        "combined",
        "spy_default",
        "spy_copied",
        "non_agent",
        "airborne",
    ] {
        let folder = root.join(name);
        let bytes = build(&mut install, &folder, name)?;
        let continuous = audit(&install, &bytes, &folder.join("continuous"), name, false)?;
        if matches!(name, "horizontal" | "spy_copied" | "airborne") {
            let restored = audit(&install, &bytes, &folder.join("restored"), name, true)?;
            let agreement = continuous["messages"] == restored["messages"];
            cases.push(json!({"case":format!("{name}_restored"),"result":restored,"continuous_notifications_match":agreement}));
        }
        cases.push(json!({"case":name,"result":continuous}));
    }
    let passed = cases.iter().all(|c| {
        c["result"]["passed"] == true
            && c.get("continuous_notifications_match")
                .is_none_or(|v| v == true)
    });
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&json!({"passed":passed,"cases":cases,
        "scope":"TS-generated CellTags, retail loading, encoded ordinary Attack/Move commands and normal ground/air updates. Spy copied identity uses the real MakeupKit receiver. Headless checkpoints inspect actor/flags and notification observations; not native pixel parity, GUI save loading, network synchronization or arbitrary geometry."}))?,
    )?;
    ensure!(
        passed,
        "cell notification audit failed; inspect report.json"
    );
    println!("{}", root.join("report.json").display());
    Ok(())
}
