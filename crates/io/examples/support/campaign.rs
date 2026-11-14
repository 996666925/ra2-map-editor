//! Shared production campaign loader for AI authoring audits.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use std::sync::Arc;
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
    },
};
pub fn load(install: &Install, bytes: &[u8], difficulty: i32) -> Result<SimRuntime> {
    load_stage(install, bytes, difficulty, "ai-campaign", 1, None, None)
}
pub fn load_stage(
    install: &Install,
    bytes: &[u8],
    difficulty: i32,
    filename: &str,
    mission_counter: i32,
    incoming: Option<CampaignCarryOver>,
    locals: Option<vera20k_core::sim::trigger_runtime::RetainedLocalVariables>,
) -> Result<SimRuntime> {
    load_stage_with_options(
        install,
        bytes,
        difficulty,
        filename,
        mission_counter,
        incoming,
        locals,
        None,
    )
}
#[allow(dead_code)]
pub fn load_with_options(
    install: &Install,
    bytes: &[u8],
    difficulty: i32,
    options: &vera20k_core::sim::game_options::GameOptions,
) -> Result<SimRuntime> {
    load_stage_with_options(
        install,
        bytes,
        difficulty,
        "ai-campaign",
        1,
        None,
        None,
        Some(options),
    )
}
#[allow(clippy::too_many_arguments)]
fn load_stage_with_options(
    install: &Install,
    bytes: &[u8],
    difficulty: i32,
    filename: &str,
    mission_counter: i32,
    incoming: Option<CampaignCarryOver>,
    locals: Option<vera20k_core::sim::trigger_runtime::RetainedLocalVariables>,
    options: Option<&vera20k_core::sim::game_options::GameOptions>,
) -> Result<SimRuntime> {
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
    let teams = TeamAiIniRegistry::load_retail(&install.manager, &map.ini, false)
        .map_err(anyhow::Error::msg)?;
    let roster =
        vera20k_core::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    // The production loaders use the Resize cell-array extent, SizeW+SizeH,
    // for both session axes. Logical map Size here truncates CellTag line
    // scans and changes map-edge consumers in otherwise valid audit maps.
    let native_bounds =
        vera20k_core::sim::scenario_bootstrap::NativeStartBounds::from_map_header(&map.header)
            .context("native map cell-array bounds")?;
    let scenario_cell_extent = native_bounds.min_rx + native_bounds.width;
    let descriptor = ScenarioDescriptor {
        seed: 12345,
        map_name: filename.into(),
        theater: map.header.theater.clone(),
        map_width: scenario_cell_extent,
        map_height: scenario_cell_extent,
        timer_inherit: map.basic.timer_inherit.unwrap_or(false),
        train_crate: map.basic.train_crate.unwrap_or(false),
        truck_crate: map.basic.truck_crate.unwrap_or(false),
        lighting: vera20k_core::sim::scenario_session::ScenarioLightingState::from_map(
            &vera20k_core::map::lighting::parse_lighting_profiles(&map.ini),
        ),
        // The ordinary client resolves these map-owned campaign flags into
        // ScenarioDescriptor before construction (app/loading/init.rs).
        no_damage: map.special_flags.inert.unwrap_or(false),
        harvester_immune: map.special_flags.harvester_immune.unwrap_or(false),
        tiberium_grows_flag: map.special_flags.tiberium_grows.unwrap_or(false),
        tiberium_spreads_flag: map.special_flags.tiberium_spreads.unwrap_or(true),
        carry_over_money: map.basic.carry_over_money,
        carry_over_cap: map.basic.carry_over_cap,
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
            mission_counter,
            game_options: options,
            campaign_index: 0,
            restart_carry_over: incoming.unwrap_or(CampaignCarryOver::CLEARED),
        },
    )?;
    let dummy = SharedCellDummy::fresh();
    dummy.reconstruct_for_map_resize();
    sim.bind_shared_cell_dummy(dummy.clone());
    sim.install_waypoints(&map.waypoints);
    if let Some(locals) = locals {
        sim.inherit_local_variables_for_fresh_load(locals);
    }
    let themes = vera20k_core::rules::theme_ini::ThemeCatalog::select(&install.manager);
    sim.initialize_trigger_definitions(
        &map,
        &teams,
        Some(audio.eva()),
        Some(audio.sounds()),
        Some(&themes),
    );
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
    // Match the production map loader: authored trigger and team-script
    // animations extend the tolerant effect roots before constructors run.
    let mut authored_roots =
        vera20k_core::map::actions::trigger_anim_class_roots(&rules, &map.actions);
    authored_roots.extend(teams.script_anim_class_roots(&rules));
    rules.bind_anim_class_assets(
        &authored_roots,
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
    rules.bind_building_buildup_assets(&install.manager, &map.header.theater);
    rules.bind_building_rubble_assets(
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
    // The engine logs unresolved AI references and installs the remaining data.
    sim.install_team_ai_registry(&teams, &rules);
    let post = vera20k_core::sim::runtime::finalize_constructed_scenario(
        &mut sim,
        &map,
        &rules,
        &overlay_registry,
        loaded.overlay_grid,
        &roster,
        None,
    )?;
    ensure!(post.navigation_published, "post-map navigation");
    sim.publish_scenario_started();
    if let Some(record) = incoming {
        sim.restore_campaign_carry_over(&record, &rules);
    }
    Ok(SimRuntime {
        simulation: sim,
        resources: Arc::new(SimResources::new(rules)),
    })
}
