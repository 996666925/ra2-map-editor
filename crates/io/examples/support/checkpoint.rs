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
    )?;
    let header = GameSnapshot::read_header(&bytes)?;
    ensure!(
        header.map_hash == map_hash && header.rules_hash == rules_hash,
        "checkpoint resource identity"
    );
    let snapshot: GameSnapshot = vera20k_core::util::save_codec::deserialize(&bytes)?;
    let mut restored = snapshot.restore_checkpoint_map_for_test(
        &runtime.resources.rules,
        runtime.resources.rules.overlay_registry(),
        runtime
            .simulation
            .resolved_terrain()
            .context("checkpoint terrain")?
            .clone(),
        *runtime.simulation.terrain_speed_config(),
    )?;
    restored.intern_rule_type_ids(&runtime.resources.rules);
    restored.resolve_type_handles(&runtime.resources.rules);
    ensure!(
        restored.global_variables() == runtime.simulation.global_variables(),
        "checkpoint globals"
    );
    let dummy = runtime.simulation.effective_shared_cell_dummy();
    dummy.adopt_prepared_load_state(&restored.effective_shared_cell_dummy());
    restored.bind_shared_cell_dummy(dummy);
    restored.publish_scenario_started();
    std::fs::write(path, &bytes)?;
    runtime.simulation = restored;
    Ok(bytes.len())
}
