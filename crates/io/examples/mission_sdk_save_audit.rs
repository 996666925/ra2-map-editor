//! Inspect a production client save independently of the capture input adapter.
//! This checks envelope admission, not the GUI's resource/GPU restore commit.
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::path::PathBuf;
use vera20k_core::sim::snapshot::GameSnapshot;
#[path = "support/saved_field.rs"]
mod saved_field;

fn main() -> Result<()> {
    // The production client also reserves a larger startup stack for the
    // deeply nested saved hierarchy. Keep this standalone reader independent
    // of the shell's linker settings and the host thread's default stack.
    std::thread::Builder::new()
        .name("sdk-save-audit".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(audit)?
        .join()
        .map_err(|_| anyhow::anyhow!("save audit worker panicked"))?
}

fn audit() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let save = PathBuf::from(args.next().context("client save path")?);
    let output = PathBuf::from(args.next().context("new report path")?);
    ensure!(!output.exists(), "report must be new");
    let bytes = std::fs::read(&save)?;
    let mut snapshot = GameSnapshot::load_with_resources(&bytes)?;
    let resources = snapshot
        .take_scenario_resources()
        .context("missing saved resources")?;
    let local = snapshot.take_local_view().context("missing local view")?;
    local.validate_structure()?;
    ensure!(
        snapshot.tactical_view().is_some(),
        "missing saved tactical view"
    );
    ensure!(
        resources.map().map_hash() == snapshot.map_hash,
        "resource/map identity mismatch"
    );
    let timer = snapshot.sim.session().scenario_timer_view();
    let sim = &snapshot.sim;
    let objects = sim
        .entities()
        .iter_sorted()
        .map(|(id, e)| {
            json!({
                "id":id,"type":sim.interner().resolve(e.type_ref()),
                "owner":sim.interner().resolve(e.owner()),"health":e.health.current(),
                "alive":e.lifecycle.object_alive(),"active":e.is_active(),
                "cell":sim.object_coords_cell(id),
            })
        })
        .collect::<Vec<_>>();
    let teams = sim.team_script_vm().teams_in_order().map(|team|json!({
        "id":team.id(),"type":team.team_type_id().map(|id|sim.interner().resolve(id)),
        "owner":sim.interner().resolve(team.owner()),"script":sim.interner().resolve(team.script_id()),
        "state":team,
    })).collect::<Vec<_>>();
    let report = json!({
        "passed":true,"full_gui_restore":false,"save":save,"bytes":bytes.len(),
        "tick":snapshot.tick,"map":snapshot.map_name,"description":snapshot.description,
        "map_hash":snapshot.map_hash,"rules_hash":snapshot.rules_hash,
        "saved_resources":true,"local_view_structure":true,"tactical_view":true,
        "local_player":resources.map().local_player_name(),
        "countdown":timer.map(|(frames,title)|json!({"frames":frames,"title":title})),
        "objects":objects,"teams":teams,
        "triggers":saved_field::read(sim,&["trigger_runtime"] )?,
        "set_globals":snapshot.sim.global_variables().iter().enumerate()
            .filter_map(|(index,set)|set.then_some(index)).collect::<Vec<_>>(),
        "limits":"Production envelope admission and local-view structure only; does not commit resources, simulation hierarchy or GPU presentation through the GUI loader."
    });
    std::fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("Admitted {} at tick {}", save.display(), snapshot.tick);
    Ok(())
}
