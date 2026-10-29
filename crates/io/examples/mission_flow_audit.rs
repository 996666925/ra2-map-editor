//! Actual rules/terrain + ordinary frames. No direct trigger or VM callbacks.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{Baseline, MapDocument},
    mission::{self, Kind},
    new_map::NewMapOptions,
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};
use vera20k_core::{
    map::{
        map_file,
        resolved_terrain::{ResolvedTerrainGrid, SharedCellDummy},
        tile_variant_selector::TileVariantSelectorCache,
    },
    rules::{
        audio_sources::AudioDefinitions, process_owner::NativeScenarioRulesPrefix,
        retail_sources::RetailRulesSources, team_ai_ini::TeamAiIniRegistry,
    },
    sim::{
        campaign_carry_over::CampaignCarryOver,
        runtime::{SimResources, SimRuntime},
        scenario_bootstrap::{FreshScenarioPrefix, ScenarioBootstrapRng},
        scenario_session::ScenarioDescriptor,
        world::{FrameEffects, TickLane},
    },
};
fn put(doc: &mut MapDocument, s: &str, k: &str, v: impl AsRef<str>) -> Result<()> {
    doc.set_ini(s, k, Some(v.as_ref().as_bytes().to_vec()))
}
fn authored(loss: bool) -> Result<Vec<u8>> {
    let bytes = NewMapOptions {
        width: 40,
        height: 40,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(bytes.clone())?;
    let mut doc = MapDocument::new(Baseline::parse(bytes.into(), &map)?);
    let start = mission::seed(&mut doc)?;
    let team = mission::ids(&doc, Kind::Team)[0].clone();
    let script = doc.ini_text(&team, "Script");
    doc.waypoint(20, [52, 40])?;
    doc.waypoint(21, [54, 40])?;
    doc.waypoint(22, [40, 40])?;
    put(
        &mut doc,
        &team,
        "Waypoint",
        mission::schema::waypoint_token(20)?,
    )?;
    for (k, v) in [
        ("0", "5,1"),
        ("1", "3,21"),
        ("2", "5,1"),
        ("3", "39,0"),
        ("4", "49,0"),
    ] {
        put(&mut doc, &script, k, v)?;
    }
    put(&mut doc, "Events", &start, "1,24,0,-1")?;
    put(
        &mut doc,
        "Actions",
        &start,
        mission::encode_list(&[mission::action(28, "0"), mission::action(7, &team)]),
    )?;
    let tag = mission::ids(&doc, Kind::Tag)
        .into_iter()
        .find(|id| mission::csv(&doc.ini_text("Tags", id))[2] == start)
        .context("start tag")?;
    mission::bind_cell(&mut doc, [40, 40], &tag)?;
    let follow = mission::ids(&doc, Kind::Trigger)
        .into_iter()
        .find(|id| id != &start)
        .unwrap();
    put(&mut doc, "Events", &follow, "2,36,0,0,13,0,10")?;
    put(
        &mut doc,
        "Actions",
        &follow,
        mission::encode_list(&[mission::action(28, "1")]),
    )?;
    mission::create(&mut doc, Kind::Trigger, "FLOW_RESULT", "Americans", None)?;
    put(&mut doc, "Events", "FLOW_RESULT", "1,27,0,1")?;
    put(
        &mut doc,
        "Actions",
        "FLOW_RESULT",
        mission::encode_list(&[mission::action(if loss { 68 } else { 67 }, "0")]),
    )?;
    mission::create(&mut doc, Kind::TaskForce, "ENTRY_FORCE", "Russians", None)?;
    put(&mut doc, "ENTRY_FORCE", "0", "1,E2")?;
    mission::create(&mut doc, Kind::Script, "ENTRY_SCRIPT", "Russians", None)?;
    put(&mut doc, "ENTRY_SCRIPT", "0", "3,22")?;
    put(&mut doc, "ENTRY_SCRIPT", "1", "2,0")?;
    mission::create(&mut doc, Kind::Team, "ENTRY_TEAM", "Russians", None)?;
    for (k, v) in [
        ("Script", "ENTRY_SCRIPT"),
        ("TaskForce", "ENTRY_FORCE"),
        ("Waypoint", "W"),
    ] {
        put(&mut doc, "ENTRY_TEAM", k, v)?;
    }
    doc.add_record("Infantry", [36, 40], "Russians", "E2")?;
    mission::create(&mut doc, Kind::Trigger, "ENTRY_TRIGGER", "Russians", None)?;
    put(&mut doc, "Events", "ENTRY_TRIGGER", "1,13,0,1")?;
    put(
        &mut doc,
        "Actions",
        "ENTRY_TRIGGER",
        mission::encode_list(&[mission::action(4, "ENTRY_TEAM")]),
    )?;
    let errors: Vec<_> = mission::diagnostics(&doc, None)
        .into_iter()
        .filter(|d| d.severity == mission::Severity::Error)
        .collect();
    ensure!(errors.is_empty(), "{errors:?}");
    doc.snapshot().bytes()
}
fn run(install: &Install, bytes: &[u8], difficulty: i32, loss: bool) -> Result<serde_json::Value> {
    let mut map = map_file::load_from_bytes(bytes.to_vec())?;
    let audio = AudioDefinitions::select(&install.manager);
    let mut owner = RetailRulesSources::select(&install.manager)
        .map_err(anyhow::Error::msg)?
        .into_owner(Arc::clone(audio.sounds()), Arc::clone(audio.eva()))
        .map_err(anyhow::Error::msg)?;
    let (mut rules, _, art, receipt) = owner
        .load_scenario(NativeScenarioRulesPrefix::Campaign(None), &map.ini)?
        .into_parts();
    rules.install_art_data(vera20k_core::rules::art_data::ArtRegistry::from_ini(&art));
    rules.bind_animation_sequences(
        &vera20k_core::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
    );
    let teams = TeamAiIniRegistry::load_retail(&install.manager, &map.ini, false)
        .map_err(anyhow::Error::msg)?;
    let roster =
        vera20k_core::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let descriptor = ScenarioDescriptor {
        seed: 12345,
        map_name: "mission-flow".into(),
        theater: map.header.theater.clone(),
        map_width: 80,
        map_height: 80,
        ..Default::default()
    };
    let (mut sim, _) = ScenarioBootstrapRng::new(12345).into_fresh_staged_simulation(
        &descriptor,
        FreshScenarioPrefix::Campaign {
            native_rules_receipt: receipt,
            rules: &rules,
            map_data: &map,
            house_roster: &roster,
            difficulty,
            mission_counter: 1,
            game_options: None,
            campaign_index: 0,
            restart_carry_over: CampaignCarryOver::CLEARED,
        },
    )?;
    let dummy = SharedCellDummy::fresh();
    dummy.reconstruct_for_map_resize();
    sim.bind_shared_cell_dummy(dummy.clone());
    sim.install_waypoints(&map.waypoints);
    sim.initialize_trigger_definitions(&map, &teams, Some(audio.eva()), Some(audio.sounds()), None);
    let overlay_registry = rules.overlay_registry().clone();
    let terrain_fill = {
        let (mut fill_rng, main_rng) = sim.terrain_load_draws();
        let mut fill = |low, high| fill_rng.next_range_u32_inclusive(low, high);
        let mut draw = move || main_rng.next_u32();
        let mut variants = TileVariantSelectorCache::default();
        let mut selector = variants.begin_load(&mut draw);
        ResolvedTerrainGrid::build_pending_authored_with_variant_selector_and_shared_dummy(
            &map,
            Some(&install.theater),
            Some(&install.manager),
            Some(&rules.terrain_rules),
            Some(&overlay_registry),
            true,
            rules.general.cliff_back_impassability,
            &mut fill,
            &mut selector,
            dummy,
        )
    };
    let roots =
        vera20k_core::map::resolved_terrain::scheduler_anim_roots(&rules, &overlay_registry, &[]);
    rules
        .bind_scheduler_anim_assets(
            &roots,
            &install.manager,
            install.theater.extension,
            &map.header.theater,
        )
        .map_err(anyhow::Error::msg)?;
    let roots = vera20k_core::rules::effect_asset_catalog::tolerant_anim_class_roots(&rules);
    rules.bind_anim_class_assets(
        &roots,
        &install.manager,
        install.theater.extension,
        &map.header.theater,
    );
    rules.populate_anim_frame_dims(
        &install.manager,
        install.theater.extension,
        &map.header.theater,
    );
    rules.bind_effect_assets(
        &install.manager,
        install.theater.extension,
        &map.header.theater,
    );
    rules.bind_terrain_assets(
        &install.manager,
        install.theater.extension,
        vera20k_core::rules::terrain_asset_catalog::TerrainAssetLoadPhase::Fresh,
    );
    let shp_ids = vera20k_core::rules::effect_asset_catalog::resolved_overlay_shp_ids(
        &overlay_registry,
        &install.manager,
        install.theater.extension,
        &map.header.theater,
    );
    sim.construct_native_map_tubes(&map.ini)?;
    let signed_format = map.basic.new_ini_format.unwrap_or(0);
    let cliff = rules.general.cliff_back_impassability;
    let loaded = vera20k_core::sim::runtime::finalize_and_populate_staged_authored_scenario(
        &mut sim,
        &mut map,
        terrain_fill,
        &install.theater,
        &install.manager,
        &mut rules,
        &mut owner,
        &overlay_registry,
        &shp_ids,
        signed_format,
        true,
        cliff,
        install.theater.extension,
        &descriptor.theater,
        vera20k_core::map::basic::BridgeDestroyabilityMode::CampaignOrEditor,
        &descriptor,
        |_, _| {},
    )?;
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.install_team_ai_registry(&teams, &rules)
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let post = vera20k_core::sim::runtime::finalize_constructed_scenario(
        &mut sim,
        &map,
        &rules,
        &overlay_registry,
        loaded.overlay_grid,
        &roster,
        None,
    );
    ensure!(post.navigation_published, "post-map navigation");
    sim.publish_scenario_started();
    let player = sim.session().current_house().context("campaign player")?;
    let team_id = sim
        .interner()
        .get(
            &teams
                .team_types
                .iter()
                .find(|t| t.id.starts_with("EDTM"))
                .context("reinforcement type")?
                .id,
        )
        .unwrap();
    let mut runtime = SimRuntime {
        simulation: sim,
        resources: SimResources::new(rules),
    };
    let mut cursors = BTreeSet::new();
    let mut members = BTreeSet::new();
    let mut region_frame = None;
    let mut completed_frame = None;
    let mut eof = false;
    let mut succeeded = false;
    let mut result_frame = None;
    for frame in 0..1800 {
        let sim = &runtime.simulation;
        if sim.global_variables()[0] && region_frame.is_none() {
            region_frame = Some(frame);
        }
        let states: Vec<_> = sim
            .team_script_vm()
            .teams_in_order()
            .filter(|t| t.team_type_id() == Some(team_id))
            .collect();
        for state in &states {
            ensure!(
                region_frame.is_some(),
                "reinforcement existed before region ingress"
            );
            ensure!(
                state.members().count() == 2,
                "reinforcement lost members: {}",
                state.members().count()
            );
            cursors.insert(state.cursor());
            members.extend(state.members());
            succeeded |= state.cursor() == 4;
        }
        if !members.is_empty() && states.is_empty() {
            eof = true;
            completed_frame.get_or_insert(frame);
        }
        if sim.global_variables()[1] {
            ensure!(eof, "followup fired before script EOF");
            for id in &members {
                let e = sim.entities().get(*id).context("lost member")?;
                let delta = (i32::from(e.position.rx()) - 54).abs()
                    + (i32::from(e.position.ry()) - 40).abs();
                ensure!(delta <= 4, "member did not arrive: {delta}");
            }
        }
        if let Some(outcome) = sim.ready_outcome_for_owner(player) {
            ensure!(
                format!("{:?}", outcome.kind) == if loss { "Defeat" } else { "Victory" },
                "unexpected result"
            );
            result_frame = Some(frame);
            break;
        }
        runtime.advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())?;
    }
    ensure!(
        region_frame.is_some()
            && eof
            && members.len() == 2
            && cursors.contains(&0)
            && cursors.contains(&1)
            && succeeded
            && result_frame.is_some(),
        "incomplete flow: region={region_frame:?}, eof={eof}, members={members:?}, cursors={cursors:?}, success={succeeded}, outcome={result_frame:?}"
    );
    Ok(
        serde_json::json!({"difficulty":difficulty,"loss":loss,"region_frame":region_frame,"members":members,"cursors":cursors,"success_action_observed":succeeded,"script_eof_frame":completed_frame,"result_frame":result_frame,"passed":true}),
    )
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game dir")?);
    let output = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "target/p4-authoring-flow".into()),
    );
    std::fs::create_dir_all(&output)?;
    let executable = args.next().map(PathBuf::from);
    let install = Install::open(&directory)?;
    let mut reports = Vec::new();
    for loss in [false, true] {
        let bytes = authored(loss)?;
        std::fs::write(
            output.join(if loss { "defeat.map" } else { "victory.map" }),
            &bytes,
        )?;
        for difficulty in 0..3 {
            let mut result = run(&install, &bytes, difficulty, loss)?;
            if let Some(exe) = &executable {
                let options = ra2_map_io::playtest::PlayOptions {
                    mission: true,
                    mission_difficulty: difficulty as u8,
                    ..Default::default()
                };
                let mut process =
                    ra2_map_io::editor_io::launch(&bytes, exe, &directory, &output, &options)?;
                let start = std::time::Instant::now();
                let mut status = None;
                while start.elapsed() < std::time::Duration::from_secs(30) {
                    if let Some(exit) = process.child.try_wait()? {
                        status = Some(exit);
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                if status.is_none() {
                    process.child.kill()?;
                    process.child.wait()?;
                }
                let log = std::fs::read_to_string(process.folder.join("logs/ra2.log"))
                    .unwrap_or_default();
                let expected = if loss {
                    "You have Lost"
                } else {
                    "You are Victorious!"
                };
                ensure!(
                    status.is_none_or(|s| s.success())
                        && log.contains("Transitioned to InGame after scenario installation")
                        && log.lines().any(|s| s
                            .contains("Match end ready for local player 'Americans'")
                            && s.contains(expected)),
                    "real campaign failed: {}",
                    process.folder.display()
                );
                result["real_process"] = serde_json::json!({"passed":true,"exit":status.and_then(|s|s.code()),"stopped_after_result":status.is_none(),"folder":process.folder,"seconds":start.elapsed().as_secs_f32(),"evidence":log.lines().filter(|s|s.contains("Transitioned to InGame")||s.contains("Match end ready for local player")).collect::<Vec<_>>()});
            }
            println!("{result}");
            reports.push(result);
        }
    }
    let report = serde_json::json!({"passed":true,"runs":reports,"scope":"real installation rules and terrain, campaign prefix and ordinary production frames; no GPU or native pixel comparison"});
    std::fs::write(
        Path::new(&output).join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
