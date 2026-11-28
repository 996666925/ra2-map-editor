use anyhow::{Context, Result, ensure};
use ra2_map_document::document::{Baseline, MapDocument};
use ra2_map_io::wasm_mission;
use std::{fs, path::Path, sync::Arc, time::SystemTime};
use vera20k_core::{
    assets::asset_manager::{AssetManager, MediaArchiveMode},
    headless_scenario,
    map::map_file::MapFile,
    sim::snapshot::GameSnapshot,
    util::ini_writer::set_ini_values,
};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [root, module] = args.as_slice() else {
        anyhow::bail!(
            "usage: wasm_mission_smoke <retail-directory> <compiled engine-check/fixture module>"
        );
    };
    let root = Path::new(root);
    let module = wasm_mission::read_module(Path::new(module))?;
    let assets = AssetManager::new(root, MediaArchiveMode::STOCK_DIGITAL)?;
    let base = assets
        .resolve_ref("XMP03T4.MAP")
        .context("missing XMP03T4.MAP")?;
    let bytes = set_ini_values(
        base.bytes,
        "VariableNames",
        &[
            ("5", "WasmStart,0"),
            ("6", "NativeSet,0"),
            ("7", "WasmTimer,0"),
            ("8", "OneShotTimer,0"),
            ("9", "RuntimeRestored,0"),
        ],
    );
    let bytes = set_ini_values(
        &bytes,
        "Triggers",
        &[("WASMBRIDGE", "Neutral,<none>,Wasm bridge,0,1,1,1,0")],
    );
    let bytes = set_ini_values(&bytes, "Events", &[("WASMBRIDGE", "1,36,0,5")]);
    let bytes = set_ini_values(&bytes, "Actions", &[("WASMBRIDGE", "1,56,0,6,0,0,0,0,A")]);
    let bytes = set_ini_values(
        &bytes,
        "Tags",
        &[("WASMBRIDGETAG", "0,Wasm bridge,WASMBRIDGE")],
    );

    let map = MapFile::from_bytes(&bytes)?;
    let mut doc = MapDocument::new(Baseline::parse(Arc::from(bytes), &map)?);
    ensure!(
        wasm_mission::apply(&mut doc, Some(&module))?,
        "module was not embedded"
    );
    doc.set_ini("Basic", "Name", Some(b"Runtime smoke".to_vec()))?;
    doc.commit("unrelated edit");
    let bytes = doc.snapshot().bytes()?;
    ensure!(
        wasm_mission::extract(&bytes)?.as_deref() == Some(module.as_slice()),
        "map edit lost module bytes"
    );

    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ra2-wasm-{}-{nonce}.map", std::process::id()));
    fs::write(&path, &bytes)?;
    let loaded =
        headless_scenario::load(root, path.to_str().context("non-UTF-8 temp path")?, 0x1269);
    fs::remove_file(&path)?;
    let mut scene = loaded.map_err(anyhow::Error::msg)?;
    ensure!(
        scene.sim().ext_modules().status().len() == 1,
        "expected a loaded map module"
    );
    ensure!(
        scene
            .runtime
            .resources
            .rules
            .object("E1")
            .map(|e1| e1.strength)
            == Some(237),
        "onRules did not change E1 Strength"
    );
    for _ in 0..17 * 20 {
        scene.tick();
    }
    let first_logs = scene.runtime.simulation.ext_modules_mut().take_log();
    ensure!(
        first_logs.iter().any(|line| line.contains("mission ready")),
        "missing start log"
    );
    // Restore the guest's mutable state through the production snapshot path.
    let saved = GameSnapshot::save(scene.sim(), 0, 0, "AssemblyScript smoke", 0);
    let rules = &scene.runtime.resources.rules;
    let terrain = scene
        .sim()
        .resolved_terrain()
        .context("missing terrain")?
        .clone();
    let restored = GameSnapshot::load(&saved)?.restore_checkpoint_map_for_test(
        rules,
        rules.overlay_registry(),
        terrain,
        Default::default(),
    )?;
    scene.runtime.simulation = restored;
    for _ in 0..17 * 20 {
        scene.tick();
    }
    let triggers = scene.sim().trigger_runtime();
    ensure!(
        triggers.local_variable_is_set(5),
        "Wasm start callback did not set Local 5"
    );
    ensure!(
        triggers.local_variable_is_set(6),
        "native trigger did not set Local 6"
    );
    ensure!(
        triggers.local_variable_is_set(7),
        "Wasm frame/timer callback did not set Local 7"
    );
    ensure!(
        triggers.local_variable_is_set(8),
        "one-shot timer did not reach the guest"
    );
    ensure!(
        triggers.local_variable_is_set(9),
        "AssemblyScript frame counter did not continue after snapshot restore"
    );
    let status = scene.sim().ext_modules().status();
    ensure!(
        status.iter().all(|(_, fault)| fault.is_none()),
        "module fault: {status:?}"
    );
    let logs = scene.runtime.simulation.ext_modules_mut().take_log();
    let logs = format!("{logs:?}");
    ensure!(
        logs.contains("mission timer"),
        "missing module logs: {logs}"
    );
    println!(
        "VERA20k {} verified AssemblyScript snapshot restore, rules, frame, start, events, native trigger and map round-trip",
        vera20k_core::util::version::GAME_VERSION
    );
    Ok(())
}
