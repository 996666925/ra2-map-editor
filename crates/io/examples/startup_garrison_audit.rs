//! Run editor-generated startup garrisons using ordinary YR rules and assets.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::{
    assets::Install,
    map_resources::{AresMode, MapResources},
};
use ra2_map_document::{
    document::{Baseline, MapDocument},
    mission::{self, Kind},
    new_map::NewMapOptions,
    startup_garrison, startup_owner,
};
use std::path::{Path, PathBuf};
use vera20k_core::{
    headless_scenario::{battle_session, load_with_launch_and_ini},
    map::map_file,
    sim::{
        scenario_bootstrap::MatchLaunchDescriptor,
        scenario_session::ScenarioGameMode,
        world::{FrameEffects, TickLane},
    },
    skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition},
};

#[path = "support/campaign.rs"]
mod campaign;

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let output = PathBuf::from(args.next().context("output map path")?);
    ensure!(
        args.next().is_none(),
        "expected the game directory and output map path"
    );
    let _dialect = vera20k_core::rules::ares_switch::read_ares_way(false);
    let bytes = NewMapOptions {
        width: 48,
        height: 48,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(bytes.clone())?;
    let mut doc = MapDocument::new(Baseline::parse_with_ares(bytes.into(), &map, false)?);
    for house in ["Americans", "Russians", "Neutral"] {
        mission::create(&mut doc, Kind::House, house, house, None)?;
        doc.set_ini(house, "Country", Some(house.as_bytes().to_vec()))?;
        doc.set_ini(house, "IQ", Some(b"0".to_vec()))?;
    }
    doc.set_ini("Americans", "PlayerControl", Some(b"yes".to_vec()))?;
    doc.set_ini("Basic", "Player", Some(b"Americans".to_vec()))?;
    doc.set_ini("Basic", "MultiplayerOnly", Some(b"no".to_vec()))?;
    let bunker = doc.add_record("Structures", [30, 40], "Americans", "NABNKR")?;
    doc.add_record("Structures", [24, 30], "Americans", "NABNKR")?;
    doc.add_record("Structures", [43, 55], "Russians", "NAPOWR")?;
    let passive = doc.add_record("Structures", [49, 55], "Neutral", "CAGAS01")?;
    let assigned_house = doc.add_record("Structures", [42, 32], "Americans", "CAGAS01")?;
    let install = Install::open_with_resources(
        &game,
        Some(MapResources {
            ares: AresMode::Off,
            ..Default::default()
        }),
        &|| false,
    )?;
    let base = campaign::load(&install, &doc.snapshot().bytes()?, 0)?;
    let capacity = |id| -> Result<u16> {
        let building = base.resources.rules.object(id).context("building type")?;
        ensure!(
            building.can_be_occupied && building.max_number_occupants > 0,
            "{id} has no garrison"
        );
        Ok(u16::try_from(building.max_number_occupants)?)
    };
    let bunker_count = capacity("NABNKR")?;
    let passive_count = capacity("CAGAS01")?;
    startup_garrison::set(&mut doc, bunker, Some(("E1", bunker_count)))?;
    startup_garrison::set(&mut doc, passive, Some(("E2", passive_count)))?;
    startup_garrison::set(&mut doc, assigned_house, Some(("E1", passive_count)))?;
    doc.commit("Map editor startup garrisons");
    let bytes = doc.snapshot().bytes()?;
    std::fs::write(&output, &bytes)?;
    let mut runtime = campaign::load(&install, &bytes, 0)?;
    ensure!(!runtime.resources.rules.ares(), "campaign stock mode");
    let targets = [
        ([30, 40], "E1", bunker_count),
        ([49, 55], "E2", passive_count),
        ([24, 30], "E1", 0),
        ([42, 32], "E1", passive_count),
    ];
    let ids: Vec<_> = targets
        .iter()
        .map(|(at, _, _)| {
            runtime
                .simulation
                .entities()
                .iter_sorted()
                .find_map(|(id, actor)| {
                    ([actor.position.rx(), actor.position.ry()] == *at).then_some(id)
                })
                .context("authored building")
        })
        .collect::<Result<_>>()?;
    for _ in 0..500 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, &mut FrameEffects::default())?;
    }
    let sim = &runtime.simulation;
    for ((at, kind, count), id) in targets.iter().zip(ids) {
        let building = sim.entities().get(id).context("building retained")?;
        let occupants = building.passenger_role().occupants().ids();
        println!("{at:?} {} occupants, expected {count}", occupants.len());
        ensure!(
            occupants.len() == usize::from(*count),
            "{at:?} has the wrong garrison"
        );
        if *at == [42, 32] {
            ensure!(
                sim.interner().resolve(building.owner()) == "Americans",
                "wrong assigned owner"
            );
        }
        for &id in occupants {
            let infantry = sim.entities().get(id).context("garrison member")?;
            ensure!(
                infantry.owner() == building.owner()
                    && infantry.in_limbo()
                    && sim.interner().resolve(infantry.type_ref()) == *kind,
                "wrong garrison member"
            );
        }
    }
    skirmish_slot_garrison(&game, &output.with_extension("skirmish.yrm"), passive_count)
}

fn skirmish_slot_garrison(game: &Path, output: &Path, capacity: u16) -> Result<()> {
    let bytes = NewMapOptions {
        width: 48,
        height: 48,
        ..Default::default()
    }
    .bytes()?;
    let map = map_file::load_from_bytes(bytes.clone())?;
    let mut doc = MapDocument::new(Baseline::parse_with_ares(bytes.into(), &map, false)?);
    let at = [42, 32];
    let id = doc.add_record("Structures", at, "Neutral", "CAGAS01")?;
    startup_owner::set(&mut doc, id, Some(0))?;
    startup_garrison::set(&mut doc, id, Some(("E1", capacity)))?;
    doc.commit("Skirmish civilian garrison");
    std::fs::write(output, doc.snapshot().bytes()?)?;
    let filename = output.to_str().context("map path")?;
    let mut launch = battle_session(filename, &[(LaunchCountry::Russia, AiDifficulty::Normal)]);
    launch.humans[0].slot.start_position = LaunchStartPosition::Position(0);
    launch.opponents[0].start_position = LaunchStartPosition::Position(1);
    launch.options.unit_count = 0;
    launch.options.short_game = false;
    launch.options.crates = false;
    let mut runtime = load_with_launch_and_ini(
        game,
        filename,
        12345,
        MatchLaunchDescriptor::from_resolved(launch)
            .map_err(|error| anyhow::anyhow!("{error:?}"))?,
        &Default::default(),
        ScenarioGameMode::Skirmish,
        None,
        None,
    )
    .map_err(anyhow::Error::msg)?
    .runtime;
    ensure!(!runtime.resources.rules.ares(), "skirmish stock mode");
    for _ in 0..500 {
        runtime.advance_frame(&[], 22, TickLane::Ordinary, &mut FrameEffects::default())?;
    }
    let sim = &runtime.simulation;
    let player = *sim
        .session()
        .start_slot_houses()
        .get(&0)
        .context("slot 0 house")?;
    let building = sim
        .entities()
        .iter_sorted()
        .find_map(|(_, entity)| {
            (sim.interner().resolve(entity.type_ref()) == "CAGAS01"
                && [entity.position.rx(), entity.position.ry()] == at)
                .then_some(entity)
        })
        .context("authored building")?;
    let occupants = building.passenger_role().occupants().ids();
    println!(
        "Skirmish slot 0: {} occupants, expected {capacity}",
        occupants.len()
    );
    ensure!(
        building.owner() == player && occupants.len() == usize::from(capacity),
        "wrong skirmish garrison"
    );
    for &id in occupants {
        let infantry = sim.entities().get(id).context("garrison member")?;
        ensure!(
            infantry.owner() == player && sim.interner().resolve(infantry.type_ref()) == "E1",
            "wrong skirmish garrison member"
        );
    }
    Ok(())
}
