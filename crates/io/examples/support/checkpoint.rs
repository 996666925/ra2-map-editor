use anyhow::{Context, Result, ensure};
use vera20k_core::sim::{runtime::SimRuntime, snapshot::GameSnapshot};

/// Headless checkpoint: resume the existing resource owner through the public
/// hierarchy/navigation validators. This does not exercise the GUI save loader.
pub fn restore_checkpoint(
    runtime: &mut SimRuntime,
    map: &[u8],
    path: &std::path::Path,
) -> Result<usize> {
    let map_hash = ra2_map_assets::profile::hash(map);
    let rules_hash = runtime.resources.rules.simulation_config_hash();
    let bytes = GameSnapshot::save_validated_with_presentation(
        &runtime.simulation,
        map_hash,
        rules_hash,
        "SDK checkpoint",
        0,
        &[],
        &[],
        None,
        None,
        None,
        None,
    );
    let header = GameSnapshot::read_header(&bytes)?;
    ensure!(
        header.map_hash == map_hash && header.rules_hash == rules_hash,
        "checkpoint resource identity"
    );
    let snapshot: GameSnapshot = vera20k_core::util::save_codec::deserialize(&bytes)?;
    let mut restored = snapshot.sim;
    restored.retain_process_state_for_load(vera20k_core::sim::world::SnapshotProcessState::Live(
        &runtime.simulation,
    ));
    restored.bind_shared_cell_dummy(vera20k_core::map::resolved_terrain::SharedCellDummy::fresh());
    restored.reconstruct_cellclass_dummy_for_map_resize();
    restored.restore_after_snapshot_load()?;
    restored.rebuild_caches_after_load(
        runtime
            .simulation
            .resolved_terrain()
            .context("checkpoint terrain")?
            .clone(),
        runtime.simulation.terrain_speed_config().clone(),
        &runtime.resources.rules,
    );
    restored.restore_map_authority_after_snapshot_load(
        &runtime.resources.rules,
        &runtime.resources.overlay_registry,
        None,
    )?;
    restored.intern_rule_type_ids(&runtime.resources.rules);
    restored.resolve_type_handles(&runtime.resources.rules);
    restored.rebuild_lighting_sources_after_load(&runtime.resources.rules);
    ensure!(
        restored.global_variables() == runtime.simulation.global_variables(),
        "checkpoint globals"
    );
    let dummy = runtime.simulation.effective_shared_cell_dummy();
    dummy.adopt_prepared_load_state(&restored.effective_shared_cell_dummy());
    restored.bind_shared_cell_dummy(dummy);
    restored.install_fresh_process_main(runtime.simulation.main_rng());
    restored.publish_scenario_started();
    std::fs::write(path, &bytes)?;
    runtime.simulation = restored;
    Ok(bytes.len())
}
