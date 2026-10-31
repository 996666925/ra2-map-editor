//! Six-theater terrain commands, reload consistency and maximum-map timings.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::MapDocument,
    new_map::{NewMapOptions, THEATERS},
};
use ra2_map_scene::{
    map::load_scene_bytes,
    terrain_tools::{self as tools, TerrainMode},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::Instant,
};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory required")?);
    let output = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "target/p3-terrain-audit".into()),
    );
    std::fs::create_dir_all(&output)?;
    let mut install = Install::open(&directory)?;
    let mut reports = Vec::new();
    for (theater, _) in THEATERS {
        let bytes = NewMapOptions {
            theater: theater.into(),
            width: 40,
            height: 40,
            level: 4,
            ..Default::default()
        }
        .bytes()?;
        let scene = load_scene_bytes(
            &mut install,
            "terrain.map",
            "audit".into(),
            bytes,
            Instant::now(),
        )?;
        let mut doc = MapDocument::new(
            scene
                .source
                .edit_base
                .as_ref()
                .map_err(|e| anyhow::anyhow!(e.clone()))?
                .clone(),
        );
        let ids = scene.palette.tile_ids.context("no theater ids")?;
        let mut changed = BTreeSet::new();
        let smooth = ids.ramp_base >= 0;
        let selected = BTreeSet::from([[40, 40], [41, 40], [40, 41], [41, 41]]);
        changed.extend(tools::height(
            &mut doc,
            &scene,
            &selected,
            TerrainMode::Raise,
            0,
            smooth,
        )?);
        ensure!(
            selected
                .iter()
                .all(|&at| doc.terrain(at).unwrap().level == 5),
            "{theater}: height"
        );
        let ground = scene.brushes.iter().find(|b| {
            i32::from(b.key.tile_id) == ids.rough && b.key.sub_tile == 0 && b.key.variant == 0
        });
        let mut lat = false;
        if let Some(ground) = ground {
            for x in 30..33 {
                for y in 30..33 {
                    changed.extend(tools::stamp(&mut doc, &scene, ground, [x, y], false)?);
                }
            }
            changed.extend(tools::recalculate(&mut doc, &scene, &changed)?);
            lat = true;
            let clear = scene
                .brushes
                .iter()
                .find(|b| {
                    i32::from(b.key.tile_id) == ids.clear
                        && b.key.sub_tile == 0
                        && b.key.variant == 0
                })
                .context("no clear")?;
            let filled = ra2_map_scene::fill_tools::plan(
                &doc,
                &scene,
                clear,
                ra2_map_scene::cliff_tools::rectangle([30, 30], [32, 32]),
                true,
            )?
            .apply(&mut doc)?;
            ensure!(
                filled.len() == 9,
                "{theater}: LAT family fill: {}",
                filled.len()
            );
            doc.cancel(); // Exercise fill without removing the LAT visual fixture.
            // cancel rolls back the whole pending command: rebuild the fixture.
            changed.clear();
            changed.extend(tools::height(
                &mut doc,
                &scene,
                &selected,
                TerrainMode::Raise,
                0,
                smooth,
            )?);
            for x in 30..33 {
                for y in 30..33 {
                    changed.extend(tools::stamp(&mut doc, &scene, ground, [x, y], false)?);
                }
            }
        }
        changed.extend(tools::recalculate(&mut doc, &scene, &changed)?);
        doc.commit("terrain fixture");
        let saved = doc.snapshot().bytes()?;
        std::fs::write(output.join(format!("{theater}.map")), &saved)?;
        let reloaded = load_scene_bytes(
            &mut install,
            "reloaded.map",
            "audit".into(),
            saved.clone(),
            Instant::now(),
        )?;
        let drawn: BTreeMap<_, _> = reloaded
            .cells
            .iter()
            .map(|c| ([c.rx, c.ry], (c.tile_id, c.sub_tile, c.z)))
            .collect();
        for &at in &changed {
            let v = doc.terrain(at).context("missing edited cell")?;
            let tile = scene
                .catalog
                .raw_tiles
                .iter()
                .find_map(|(&tile, &raw)| (raw == v.tile).then_some(tile))
                .context("tile translation")?;
            ensure!(
                drawn.get(&at) == Some(&(tile, v.sub, v.level)),
                "{theater}: reload changed {at:?}: expected {tile}:{}, got {:?}",
                v.sub,
                drawn.get(&at)
            );
        }
        doc.undo();
        ensure!(doc.state_id == 0, "{theater}: undo");
        doc.redo();
        ensure!(doc.snapshot().bytes()? == saved, "{theater}: redo");
        reports.push(serde_json::json!({"theater":theater,"automatic_ramps":smooth,"lat_and_fill":lat,"changed_cells":changed.len(),"engine_reload_matches":true}));
    }
    install.activate_theater("TEMPERATE")?;
    let scene = load_scene_bytes(
        &mut install,
        "max.map",
        "audit".into(),
        NewMapOptions {
            width: 256,
            height: 256,
            ..Default::default()
        }
        .bytes()?,
        Instant::now(),
    )?;
    let mut doc = MapDocument::new(
        scene
            .source
            .edit_base
            .as_ref()
            .map_err(|e| anyhow::anyhow!(e.clone()))?
            .clone(),
    );
    let ids = scene.palette.tile_ids.context("no ids")?;
    let brush = scene
        .brushes
        .iter()
        .find(|b| {
            i32::from(b.key.tile_id) == ids.rough && b.key.variant == 0 && b.key.sub_tile == 0
        })
        .context("no rough")?;
    let started = Instant::now();
    let selected = scene.cells.iter().map(|c| [c.rx, c.ry]).collect();
    let count = ra2_map_scene::fill_tools::plan(&doc, &scene, brush, selected, false)?
        .apply(&mut doc)?
        .len();
    let fill_ms = started.elapsed().as_secs_f64() * 1000.;
    doc.cancel();
    let started = Instant::now();
    tools::height(
        &mut doc,
        &scene,
        &BTreeSet::from([[256, 256]]),
        TerrainMode::Raise,
        0,
        true,
    )?;
    let height_ms = started.elapsed().as_secs_f64() * 1000.;
    let report = serde_json::json!({"passed":true,"theaters":reports,"max_map":{"size":[256,256],"fill_cells":count,"fill_ms":fill_ms,"height_ms":height_ms}});
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
