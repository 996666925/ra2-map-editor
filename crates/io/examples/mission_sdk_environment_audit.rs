//! TS environmental strikes and native scenario flags through ordinary frames.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument, Target, Value as DocumentValue},
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
        runtime::SimRuntime,
        world::{FrameEffects, TickLane},
    },
};
#[path = "support/campaign.rs"]
mod campaign;
#[path = "support/checkpoint.rs"]
mod checkpoint;

/// An isolated active resource profile, never an edit of the user's game.
/// Meteor sprites borrow EXPLOSML; IsMeteor and landing damage remain live.
fn meteor_fixture(install: &Install, root: &Path) -> Result<Install> {
    std::fs::create_dir(root)?;
    for entry in std::fs::read_dir(install.manager.ra2_dir())? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let extension = entry
            .path()
            .extension()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if name == "artmd.ini"
            || !matches!(
                extension.as_str(),
                "mix"
                    | "ini"
                    | "pal"
                    | "shp"
                    | "tmp"
                    | "tem"
                    | "sno"
                    | "urb"
                    | "ubn"
                    | "des"
                    | "lun"
                    | "vxl"
                    | "hva"
                    | "vpl"
                    | "fnt"
                    | "idx"
                    | "bag"
            )
        {
            continue;
        }
        let target = root.join(&name);
        std::fs::hard_link(entry.path(), &target)
            .or_else(|_| std::fs::copy(entry.path(), &target).map(|_| ()))?;
    }
    let art = install
        .manager
        .get_ref(install.manager.ini_roots().art())
        .context("fixed ART")?
        .to_vec();
    // Fresh INI reads retain the first duplicate section. Remove each old
    // physical meteor section before appending the controlled replacement.
    let raw = ra2_map_document::raw_ini::RawIni::parse(art.into())?;
    let mut art = Vec::new();
    for line in &raw.lines {
        if !matches!(line.section.as_str(), "METSMALL" | "METLARGE") {
            art.extend_from_slice(&raw.bytes[line.bytes.clone()]);
        }
    }
    for name in ["METSMALL", "METLARGE"] {
        art.extend_from_slice(format!("\r\n[{name}]\r\nImage=EXPLOSML\r\nRate=1\r\nRandomRate=0,0\r\nIsMeteor=yes\r\nMinZVel=-50\r\nMaxXYVel=100\r\nElasticity=0\r\nExpireAnim=EXPLOSML\r\nDamage=40\r\nWarhead=SDKMeteorWH\r\nDamageRadius=4096\r\n").as_bytes());
    }
    std::fs::write(root.join("artmd.ini"), &art)?;
    Install::open(root)
}

fn observation(runtime: &SimRuntime, target: u64, witness: u64) -> Value {
    let sim = &runtime.simulation;
    let entity = |id| {
        sim.entities()
            .get(id)
            .map(|e| json!({"health":e.health.current(),"cell":[e.position.rx(),e.position.ry()]}))
    };
    let overlays = sim.overlay_grid().map(|grid| {
        [[50, 60], [51, 60], [82, 82]].map(|[x, y]| {
            let cell = grid.cell(x, y);
            json!({"cell":[x,y],"id":cell.overlay_id,"data":cell.overlay_data})
        })
    });
    let smudges = sim.smudge_grid().map(|grid| {
        [[48, 60], [49, 60]].map(|[x, y]| json!({"cell":[x,y],"state":grid.cell(x,y)}))
    });
    let bullets = sim
        .projectiles()
        .iter()
        .map(|(id, p)| {
            let pos = p.position();
            let origin = p.launch_origin();
            let payload = p.payload();
            json!({"id":id,"position":[pos.x,pos.y,pos.z],"origin":[origin.x,origin.y,origin.z],
            "source":p.source_id(),"house":p.firer_house(),"damage":payload.base_damage,
            "weapon":payload.installed_weapon().map(|id|sim.interner().resolve(id)),
            "target":p.target(),"trajectory":p.trajectory()})
        })
        .collect::<Vec<_>>();
    let animations=sim.anims().map(|(id,anim)|{let coord=anim.world_coord();
        json!({"id":id,"type":sim.interner().resolve(anim.type_id()),"coord":[coord.x,coord.y,coord.z]})
    }).collect::<Vec<_>>();
    json!({"tick":sim.session().tick(),"target":entity(target),"witness":entity(witness),
        "ion":sim.ion_blasts().collect::<Vec<_>>(),"bullets":bullets,"animations":animations,
        "overlays":overlays,"smudges":smudges,"flags":{"vein":sim.session().vein_growth_enabled(),
            "ice":sim.session().ice_growth_enabled(),"train":sim.session().train_crate()}})
}

fn audit(install: &Install, bytes: &[u8], root: &Path, name: &str, restore: bool) -> Result<Value> {
    let mut runtime = campaign::load(install, bytes, 1)?;
    let target = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find(|(_, e)| (e.position.rx(), e.position.ry()) == (40, 60))
        .context("target")?
        .0;
    let witness = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find(|(_, e)| (e.position.rx(), e.position.ry()) == (82, 82))
        .context("witness")?
        .0;
    let initial = observation(&runtime, target, witness);
    let mut observations = Vec::new();
    let mut phases = BTreeMap::new();
    let mut damage_tick = None;
    let mut first_bullet = None;
    let mut first_ion = None;
    let mut last_ion_tick = None;
    let mut animations = std::collections::BTreeSet::new();
    let mut meteor_tracks: BTreeMap<u64, Vec<Value>> = BTreeMap::new();
    let mut flash_requests = Vec::new();
    let mut witness_unchanged = true;
    let mut checkpoint_preserved = None;
    let mut last_health = initial["target"]["health"].clone();
    let mut health_changes = Vec::new();
    for _ in 0..800 {
        let output = runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
        let current = observation(&runtime, target, witness);
        let tick = runtime.simulation.session().tick();
        for request in output.combat_lights {
            if let vera20k_core::sim::combat::CombatLightRequest::Impact {
                damage,
                warhead_ref,
                coord,
                ..
            } = request
            {
                flash_requests.push(json!({"tick":tick,"damage":damage,"warhead":runtime.simulation.interner().resolve(warhead_ref),"coord":[coord.x,coord.y,coord.z]}));
            }
        }
        for phase in 0..3 {
            if runtime.simulation.global_variables()[phase] && !phases.contains_key(&phase) {
                phases.insert(phase, current.clone());
            }
        }
        if current["target"]["health"] != last_health {
            damage_tick.get_or_insert(tick);
            health_changes.push(
                json!({"tick":tick,"before":last_health,"after":current["target"]["health"]}),
            );
            last_health = current["target"]["health"].clone();
        }
        witness_unchanged &= current["witness"] == initial["witness"];
        if !current["bullets"].as_array().unwrap().is_empty() {
            first_bullet.get_or_insert(current.clone());
        }
        if !current["ion"].as_array().unwrap().is_empty() {
            first_ion.get_or_insert(current.clone());
            last_ion_tick = Some(tick);
        }
        for anim in current["animations"].as_array().unwrap() {
            animations.insert(anim["type"].as_str().unwrap().to_owned());
            if matches!(anim["type"].as_str(), Some("METSMALL" | "METLARGE")) {
                meteor_tracks
                    .entry(anim["id"].as_u64().unwrap())
                    .or_default()
                    .push(anim["coord"].clone());
            }
        }
        observations.push(current);
        if restore && tick == 20 {
            let before = observation(&runtime, target, witness);
            checkpoint::restore_checkpoint(&mut runtime, bytes, &root.join("pending-ion.bin"))?;
            let after = observation(&runtime, target, witness);
            checkpoint_preserved = Some(before == after);
            std::fs::write(
                root.join("checkpoint-state.json"),
                serde_json::to_vec_pretty(&json!({"before":before,"after":after}))?,
            )?;
        }
    }
    let final_state = observation(&runtime, target, witness);
    let phase = |n| phases.get(&n).cloned().unwrap_or(Value::Null);
    let mut checks = BTreeMap::from([
        ("authored_start_ran", phases.contains_key(&0)),
        ("remote_witness_unchanged", witness_unchanged),
    ]);
    match name {
        "meteor0" | "meteor3" => {
            let first = phase(0);
            let count = first["animations"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|a| matches!(a["type"].as_str(), Some("METSMALL" | "METLARGE")))
                .count();
            let base = if name == "meteor0" { 1 } else { 15 };
            checks.insert(
                "authored_intensity_selects_native_count_range",
                (base..=base + 2).contains(&count),
            );
            checks.insert("all_created_meteors_observed", meteor_tracks.len() == count);
            checks.insert(
                "meteors_really_travel_above_ground",
                meteor_tracks.values().any(|track| {
                    track.len() > 1
                        && track.first() != track.last()
                        && track.iter().any(|coord| coord[2].as_i64().unwrap() > 0)
                }),
            );
            checks.insert("meteor_landing_causes_real_damage", damage_tick.is_some());
            checks.insert(
                "landing_constructs_expire_animation",
                animations.contains("EXPLOSML"),
            );
            checks.insert(
                "all_meteors_retire",
                !final_state["animations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|a| matches!(a["type"].as_str(), Some("METSMALL" | "METLARGE"))),
            );
        }
        "ion" => {
            checks.insert(
                "blast_first_observed_at_ground_after_same_frame_update",
                first_ion
                    .as_ref()
                    .is_some_and(|s| s["ion"] == json!([[[10368, 15488, 0], 1]])),
            );
            checks.insert(
                "first_ion_update_damages_in_action_frame",
                damage_tick == phase(0)["tick"].as_u64(),
            );
            checks.insert(
                "configured_damage_applied_once",
                health_changes.len() == 1 && final_state["target"]["health"] == 260,
            );
            checks.insert(
                "ring_and_beam_animations_constructed",
                animations.contains("RING1") && animations.contains("IONBEAM"),
            );
            checks.insert(
                "blast_progresses_through_frames_one_to_seventy_nine",
                first_ion
                    .as_ref()
                    .and_then(|s| s["tick"].as_u64())
                    .zip(last_ion_tick)
                    .is_some_and(|(first, last)| last - first == 78),
            );
            checks.insert("blast_retires", final_state["ion"] == json!([]));
            checks.insert("ion_does_not_create_missile", first_bullet.is_none());
            if restore {
                checks.insert(
                    "checkpoint_preserves_pending_blast_and_health",
                    checkpoint_preserved == Some(true),
                );
            }
        }
        "nuke" | "chemical" => {
            let bullet = first_bullet
                .as_ref()
                .and_then(|v| v["bullets"].as_array())
                .and_then(|v| v.first());
            checks.insert("real_missile_created", bullet.is_some());
            checks.insert(
                "missile_has_no_firing_house",
                bullet.is_some_and(|b| b["house"].is_null()),
            );
            checks.insert(
                "missile_targets_authored_cell",
                bullet.is_some_and(|b| b["target"] == json!({"Cell":{"rx":40,"ry":60}})),
            );
            checks.insert(
                "damage_follows_flight",
                damage_tick
                    .zip(phase(0)["tick"].as_u64())
                    .is_some_and(|(hit, start)| hit > start),
            );
            checks.insert("missile_retires", final_state["bullets"] == json!([]));
            if name == "nuke" {
                checks.insert(
                    "nuke_launches_from_twenty_thousand_leptons",
                    bullet.is_some_and(|b| b["origin"] == json!([10368, 15488, 20000])),
                );
                checks.insert(
                    "nuke_installs_payload_weapon",
                    bullet.is_some_and(|b| b["weapon"] == "NukePayload" && b["damage"] == 600),
                );
                checks.insert("nukeball_constructed", animations.contains("NUKEBALL"));
                checks.insert(
                    "nuke_kills_target",
                    final_state["target"].is_null() || final_state["target"]["health"] == 0,
                );
            } else {
                checks.insert(
                    "chemical_launches_at_ground_away_from_target",
                    bullet.is_some_and(|b| {
                        b["origin"][2] == 0 && b["origin"] != json!([10368, 15488, 0])
                    }),
                );
                checks.insert(
                    "chemical_keeps_null_installed_weapon",
                    bullet.is_some_and(|b| b["weapon"].is_null() && b["damage"] == 40),
                );
                checks.insert(
                    "chemical_damages_target_once",
                    health_changes.len() == 1 && final_state["target"]["health"] == 260,
                );
                checks.insert(
                    "chemical_detonation_animation_created",
                    animations.contains("EXPLOSML"),
                );
            }
        }
        "reduce" => {
            checks.insert(
                "ore_loaded_at_all_three_cells",
                initial["overlays"]
                    .as_array()
                    .is_some_and(|a| a.iter().all(|v| !v["id"].is_null())),
            );
            checks.insert(
                "only_waypoint_center_depleted",
                phase(0)["overlays"][0]["id"].is_null()
                    && final_state["overlays"][1] == initial["overlays"][1]
                    && final_state["overlays"][2] == initial["overlays"][2],
            );
        }
        "growth" => {
            let density = |s: &Value| s["overlays"][0]["data"].as_u64().unwrap();
            let off = phase(1)["tick"].as_u64().unwrap_or(u64::MAX);
            let on = phase(2)["tick"].as_u64().unwrap_or(u64::MAX);
            checks.insert("all_three_growth_switches_ran", phases.len() == 3);
            checks.insert(
                "density_stays_fixed_while_disabled",
                observations
                    .iter()
                    .filter(|s| {
                        s["tick"].as_u64().unwrap() >= off && s["tick"].as_u64().unwrap() < on
                    })
                    .all(|s| density(s) == density(&phase(1))),
            );
            checks.insert(
                "density_grows_after_reenabled",
                density(&final_state) > density(&phase(2)),
            );
            checks.insert(
                "no_ore_spreads_to_neighbor",
                final_state["overlays"][1]["id"].is_null(),
            );
        }
        "flags" => {
            checks.insert(
                "first_switch_disables_vein_and_ice",
                phase(0)["flags"]["vein"] == false && phase(0)["flags"]["ice"] == false,
            );
            checks.insert(
                "second_switch_enables_vein_and_ice",
                phase(1)["flags"]["vein"] == true && phase(1)["flags"]["ice"] == true,
            );
            checks.insert(
                "third_switch_disables_flags_again",
                phase(2)["flags"]["vein"] == false && phase(2)["flags"]["ice"] == false,
            );
            checks.insert(
                "train_crate_toggle_flips_and_restores",
                phase(0)["flags"]["train"] != initial["flags"]["train"]
                    && phase(1)["flags"]["train"] == initial["flags"]["train"],
            );
        }
        "smudge" => {
            checks.insert(
                "authored_smudges_loaded",
                initial["smudges"]
                    .as_array()
                    .is_some_and(|a| a.iter().all(|v| !v["state"]["type_id"].is_null())),
            );
            checks.insert(
                "smudge_identities_cleared",
                final_state["smudges"]
                    .as_array()
                    .is_some_and(|a| a.iter().all(|v| v["state"]["type_id"].is_null())),
            );
            checks.insert(
                "smudge_data_preserved",
                (0..2).all(|i| {
                    final_state["smudges"][i]["state"]["frame_offset"]
                        == initial["smudges"][i]["state"]["frame_offset"]
                        && final_state["smudges"][i]["state"]["footprint_origin"]
                            == initial["smudges"][i]["state"]["footprint_origin"]
                }),
            );
        }
        "flash" => {
            checks.insert("three_flash_actions_ran", phases.len() == 3);
            checks.insert(
                "native_c4_requests_have_selected_strengths",
                flash_requests
                    .iter()
                    .map(|v| v["damage"].as_i64().unwrap())
                    .collect::<Vec<_>>()
                    == [50, 100, 300],
            );
            checks.insert(
                "flash_requests_target_waypoint",
                flash_requests
                    .iter()
                    .all(|v| v["coord"] == json!([10368, 15488, 0])),
            );
            checks.insert(
                "flash_does_not_damage_target",
                final_state["target"] == initial["target"],
            );
        }
        _ => unreachable!("audit case"),
    }
    std::fs::write(
        root.join("observations.json"),
        serde_json::to_vec_pretty(&observations)?,
    )?;
    Ok(
        json!({"passed":checks.values().all(|v|*v),"checks":checks,"initial":initial,"final":final_state,
        "phases":phases,"first_ion":first_ion,"last_ion_tick":last_ion_tick,"first_bullet":first_bullet,
        "damage_tick":damage_tick,"health_changes":health_changes,"animation_types":animations,"flash_requests":flash_requests,
        "meteor_tracks":meteor_tracks,"restored":restore,"checkpoint_preserved":checkpoint_preserved}),
    )
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let root = std::path::absolute(args.next().context("new output directory")?)?;
    let with_meteors = match args.next() {
        None => false,
        Some(value) => {
            ensure!(value == "--meteor-fixture", "unknown option");
            true
        }
    };
    ensure!(
        args.next().is_none() && !root.exists(),
        "output must be new"
    );
    std::fs::create_dir_all(&root)?;
    let initial = NewMapOptions {
        width: 64,
        height: 64,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(initial.clone())?;
    let mut doc = MapDocument::new(Baseline::parse(initial.into(), &map)?);
    for house in ["Americans", "Russians"] {
        mission::create(&mut doc, Kind::House, house, house, None)?;
        doc.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
    }
    doc.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    doc.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    doc.add_record("Structures", [28, 52], "Americans", "GAPOWR")?;
    for cell in [[40, 60], [82, 82]] {
        doc.add_record("Units", cell, "Americans", "MTNK")?;
    }
    doc.commit("environment fixture");
    let mut install = Install::open(&game)?;
    let reference = campaign::load(&install, &doc.snapshot().bytes()?, 1)?;
    let rules = &reference.resources.rules;
    let prerequisites = json!({
        "weapons":(["NukePayload","ChemLauncher"].map(|name|json!({"id":name,"definition":rules.weapon(name).map(|w|json!({"damage":w.damage,"warhead":w.warhead,"projectile":w.projectile}))}))),
        "assets":(["METSMALL.SHP","METLARGE.SHP","IONBEAM.SHP","RING1.SHP"].map(|name|json!({"name":name,"exists":install.manager.resolve_ref(name).is_some()}))),
        "meteor_registered":(["METSMALL","METLARGE"].map(|name|rules.anim_type_names.contains(name))),
        "ore_id":rules.overlay_registry().flat_tiberium_variant_ids(&rules.tiberium_types.types()[0]),
    });
    std::fs::write(
        root.join("prerequisites.json"),
        serde_json::to_vec_pretty(&prerequisites)?,
    )?;
    let ore = rules
        .overlay_registry()
        .flat_tiberium_variant_ids(&rules.tiberium_types.types()[0])
        .context("ore variants")?[0];
    let ore_section = rules.tiberium_types.types()[0].section.clone();
    drop(reference);
    if with_meteors {
        install = meteor_fixture(&install, &root.join("meteor-resources"))?;
    }
    let base = doc.snapshot().bytes()?;
    let mut cases = Vec::new();
    for (name, actions) in [
        ("ion", "action.ionBlastAt(at)"),
        ("nuke", "action.nukeAt(at)"),
        ("chemical", "action.chemicalStrikeAt(at)"),
        ("reduce", "action.reduceTiberiumAt(ore)"),
        ("growth", "action.setTiberiumGrowth(true)"),
        (
            "flags",
            "action.setVeinGrowth(false),action.setIceGrowth(false),action.toggleTrainCrate()",
        ),
        ("smudge", "action.clearSmudges()"),
        ("flash", "action.combatFlash(at,CombatFlash.Small)"),
        ("meteor0", "action.meteorShower(at,0)"),
        ("meteor3", "action.meteorShower(at,3)"),
    ] {
        if name.starts_with("meteor") && !with_meteors {
            continue;
        }
        let folder = root.join(name);
        std::fs::create_dir(&folder)?;
        let mut authored = MapDocument::new(Baseline::parse(
            base.clone().into(),
            &map_file::load_from_bytes(base.clone())?,
        )?);
        let mut overrides = vec![("Basic", "TiberiumGrowthEnabled", "no")];
        match name {
            "meteor0" | "meteor3" => overrides.extend([
                ("Animations", "9998", "METSMALL"),
                ("Animations", "9999", "METLARGE"),
                ("Warheads", "9999", "SDKMeteorWH"),
                (
                    "SDKMeteorWH",
                    "Verses",
                    "100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%",
                ),
                ("SDKMeteorWH", "CellSpread", "6"),
                ("SDKMeteorWH", "PercentAtMax", "1"),
            ]),
            "ion" => overrides.extend([
                ("CombatDamage", "IonCannonDamage", "40"),
                ("CombatDamage", "IonCannonWarhead", "SDKIonWH"),
                (
                    "SDKIonWH",
                    "Verses",
                    "100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%",
                ),
                ("SDKIonWH", "CellSpread", "0.5"),
                ("SDKIonWH", "PercentAtMax", "1"),
            ]),
            "chemical" => overrides.extend([
                ("HTK", "Primary", "ChemLauncher"),
                ("ChemLauncher", "Damage", "40"),
                ("ChemLauncher", "Projectile", "SDKChemProjectile"),
                ("ChemLauncher", "Warhead", "SDKChemWH"),
                ("SDKChemProjectile", "ROT", "100"),
                ("SDKChemProjectile", "High", "yes"),
                ("SDKChemProjectile", "Ranged", "yes"),
                ("SDKChemProjectile", "Image", "V3DOWN"),
                (
                    "SDKChemWH",
                    "Verses",
                    "100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%",
                ),
                ("SDKChemWH", "CellSpread", "0.5"),
                ("SDKChemWH", "PercentAtMax", "1"),
                ("SDKChemWH", "AnimList", "EXPLOSML"),
            ]),
            "growth" => overrides.extend([
                ("Basic", "TiberiumGrowthEnabled", "yes"),
                (ore_section.as_str(), "Growth", "100"),
                (ore_section.as_str(), "GrowthPercentage", "1"),
                (ore_section.as_str(), "SpreadPercentage", "0"),
                ("SpecialFlags", "TiberiumGrows", "yes"),
                ("SpecialFlags", "TiberiumSpreads", "no"),
            ]),
            "smudge" => {
                authored.add_record("Smudge", [48, 60], "Neutral", "CR1")?;
                authored.add_record("Smudge", [49, 60], "Neutral", "CR2")?;
            }
            _ => {}
        }
        for (section, key, value) in overrides {
            authored
                .set_ini(section, key, Some(value.as_bytes().to_vec()))
                .with_context(|| format!("fixture {name}: {section}/{key}"))?;
        }
        if name == "reduce" {
            for (cell, data) in [([50, 60], 11), ([51, 60], 7), ([82, 82], 5)] {
                authored.set(
                    Target::Overlay(cell),
                    Some(DocumentValue::Overlay(ore, data)),
                )?;
            }
        } else if name == "growth" {
            authored.set(
                Target::Overlay([50, 60]),
                Some(DocumentValue::Overlay(ore, 3)),
            )?;
        }
        authored.commit("environment rules fixture");
        let map_path = folder.join("source.map");
        std::fs::write(&map_path, authored.snapshot().bytes()?)?;
        let scene = ra2_map_scene::map::load_scene(
            &mut install,
            map_path.to_str().context("source map path")?,
        )?;
        let more = match name {
            "flags" => {
                "m.rule('on',{when:event.sinceScenarioStart(2),once:true,actions:[action.setVeinGrowth(true),action.setIceGrowth(true),action.toggleTrainCrate(),action.setGlobal(m.globalFlag(1))]});m.rule('off',{when:event.sinceScenarioStart(4),once:true,actions:[action.setVeinGrowth(false),action.setIceGrowth(false),action.setGlobal(m.globalFlag(2))]});"
            }
            "growth" => {
                "m.rule('off',{when:event.sinceScenarioStart(2),once:true,actions:[action.setTiberiumGrowth(false),action.setGlobal(m.globalFlag(1))]});m.rule('on',{when:event.sinceScenarioStart(6),once:true,actions:[action.setTiberiumGrowth(true),action.setGlobal(m.globalFlag(2))]});"
            }
            "flash" => {
                "m.rule('medium',{when:event.sinceScenarioStart(2),once:true,actions:[action.combatFlash(at,CombatFlash.Medium),action.setGlobal(m.globalFlag(1))]});m.rule('large',{when:event.sinceScenarioStart(3),once:true,actions:[action.combatFlash(at,CombatFlash.Large),action.setGlobal(m.globalFlag(2))]});"
            }
            _ => "",
        };
        let source=r#"import {defineMission,event,action,CombatFlash} from './sdk';
import {map,House,Country} from './map.generated';
export default defineMission(map,{id:'environment',player:House.Americans},m=>{
 m.configureBasic({ignoreGlobalAiTriggers:true,skipScore:true});
 m.configureHouse(m.house(House.Americans),{country:m.country(Country.Americans),iq:0,playerControl:true});
 m.configureHouse(m.house(House.Russians),{country:m.country(Country.Russians),iq:0});
 const at=m.createWaypoint(30,40,60),ore=m.createWaypoint(31,50,60);
 m.rule('start',{when:event.sinceScenarioStart(1),once:true,actions:[__ACTIONS__,action.setGlobal(m.globalFlag(0))]});
 __MORE__
});"#.replace("__ACTIONS__",actions).replace("__MORE__",more);
        let entry = folder.join("mission.ts");
        std::fs::write(&entry, source)?;
        let catalog = mission_script::catalog(&authored, &scene);
        mission_script::refresh_types(&entry, &catalog)?;
        Worker::new()?
            .build(&authored, catalog, Sources::read(&entry)?)?
            .recv()?
            .map_err(anyhow::Error::msg)?
            .build
            .apply(&mut authored)?;
        let bytes = authored.snapshot().bytes()?;
        std::fs::write(folder.join("environment.map"), &bytes)?;
        cases.push(json!({"case":name,"result":audit(&install,&bytes,&folder,name,false)?}));
        if name == "ion" {
            let restored = folder.join("restored");
            std::fs::create_dir(&restored)?;
            cases.push(
                json!({"case":"ion_restored","result":audit(&install,&bytes,&restored,name,true)?}),
            );
        }
    }
    let passed = cases.iter().all(|v| v["result"]["passed"] == true);
    std::fs::write(
        root.join("report.json"),
        serde_json::to_vec_pretty(&json!({"passed":passed,"cases":cases,
        "prerequisites":prerequisites,
        "meteor_fixture":with_meteors,
        "scope":"TS build, retail resource loading and ordinary frames; controlled map Rules for ion damage and chemical prerequisites. Optional isolated meteor profile aliases EXPLOSML sprites while enabling IsMeteor flight/landing damage; this is not stock meteor artwork. Includes public headless checkpoint, not GUI loading or pixel/audio verification. Vein/ice flags do not prove a growth consumer."}))?,
    )?;
    ensure!(passed, "environment audit failed; see report.json");
    println!("{}", root.join("report.json").display());
    Ok(())
}
