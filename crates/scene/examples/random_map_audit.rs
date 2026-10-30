//! Generate, reopen and render random maps against a real installation.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{document::MapDocument, random_map::RandomMapOptions, source::SourceMap};
use ra2_map_scene::{map::load_scene_bytes, random_map, visual::ViewOptions};
use std::{collections::BTreeSet, path::PathBuf, time::Instant};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory required")?);
    let output = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "target/random-map-audit".into()),
    );
    std::fs::create_dir_all(&output)?;
    let mut install = Install::open(&directory)?;
    let mut report = vec![];
    let mut cases = vec![];
    for landscape in 0..5 {
        let mut options = RandomMapOptions::default();
        options.terrain.map_type = landscape;
        options.terrain.water_amount = if landscape < 3 { 50 } else { 20 };
        cases.push((format!("landscape-{landscape}"), options));
    }
    for theater in 1..5 {
        let mut options = RandomMapOptions::default();
        options.terrain.theater = theater;
        cases.push((format!("theater-{theater}"), options));
    }
    let mut bare = RandomMapOptions {
        ore: false,
        tech_buildings: false,
        ..Default::default()
    };
    bare.terrain.water_amount = 0;
    bare.terrain.vegetation = 0;
    bare.terrain.ruggedness = 0;
    bare.terrain.num_players = 8;
    cases.push(("bare-eight-players".into(), bare));
    let mut ore_only = RandomMapOptions::default();
    ore_only.terrain.resources = 1;
    cases.push(("ore-without-gems".into(), ore_only));
    for size in [0, 3] {
        let mut options = RandomMapOptions::default();
        options.terrain.num_players = 8;
        options.terrain.width = size;
        options.terrain.height = 3 - size;
        cases.push((format!("eight-players-size-{size}"), options));
    }
    let mut other_seed = RandomMapOptions::default();
    other_seed.terrain.seed = 45678;
    cases.push(("other-seed".into(), other_seed));
    for players in [9, 16, 30] {
        for landscape in 0..5 {
            let mut options = RandomMapOptions::default();
            options.terrain.num_players = players;
            options.terrain.map_type = landscape;
            options.terrain.water_amount = if landscape < 3 { 50 } else { 20 };
            cases.push((format!("players-{players}-landscape-{landscape}"), options));
        }
    }
    for size in [0, 3] {
        let mut options = RandomMapOptions::default();
        options.terrain.num_players = 30;
        options.terrain.width = size;
        options.terrain.height = size;
        cases.push((format!("thirty-players-size-{size}"), options));
    }
    let mut fingerprints = BTreeSet::new();
    for (name, options) in cases {
        eprintln!("Generating {name}");
        let result = (|| -> Result<serde_json::Value> {
            let bytes = random_map::generate(&mut install, &options, &|| false, &|_| {})?;
            let repeated = random_map::generate(&mut install, &options, &|| false, &|_| {})?;
            ensure!(bytes == repeated, "same seed is not deterministic");
            let (source, map) =
                SourceMap::parse(format!("{name}.map"), "audit".into(), bytes.clone())?;
            let document = MapDocument::new(source.edit_base.map_err(anyhow::Error::msg)?);
            ensure!(
                (map.header.width as u16, map.header.height as u16) == options.dimensions(),
                "generated dimensions differ from dialog"
            );
            ensure!(
                map.ini.section_or_empty("Basic").read_int("MaxPlayers", 0)
                    == options.terrain.num_players,
                "incorrect player capacity"
            );
            ensure!(
                map.ini
                    .section_or_empty("RandomMap")
                    .read_int("NumPlayers", 0)
                    == options.terrain.num_players,
                "recipe lost extended player count"
            );
            ensure!(
                document.snapshot().bytes()? == bytes,
                "document roundtrip changed output"
            );
            let scene = load_scene_bytes(
                &mut install,
                &name,
                "audit".into(),
                bytes.clone(),
                Instant::now(),
            )?;
            ensure!(
                !scene
                    .diagnostics
                    .iter()
                    .any(|d| matches!(d.code, "INVALID_TILE_ID" | "MISSING_TILE")),
                "invalid or missing tile"
            );
            let starts: BTreeSet<_> = map.waypoints.values().map(|s| [s.rx, s.ry]).collect();
            ensure!(
                map.cells.len() == map.header.height as usize * (2 * map.header.width as usize - 1),
                "incomplete Size diamond"
            );
            ensure!(
                map.cells.iter().all(|c| c.z <= 14),
                "invalid terrain height"
            );
            if let Some(water) = install.theater.rmg_tiles.water_set {
                let levels: std::collections::BTreeMap<_, _> = scene
                    .cells
                    .iter()
                    .filter(|c| (water..water + 14).contains(&c.tile_id))
                    .map(|c| ((c.rx, c.ry), c.z))
                    .collect();
                for (&(x, y), &z) in &levels {
                    for at in [(x + 1, y), (x, y + 1)] {
                        ensure!(
                            levels.get(&at).is_none_or(|other| *other == z),
                            "discontinuous water surface"
                        );
                    }
                }
            }
            ensure!(
                starts.len() == options.terrain.num_players as usize,
                "missing or duplicate starts"
            );
            for at in starts {
                let cell = scene
                    .cells
                    .iter()
                    .find(|c| [c.rx, c.ry] == at)
                    .context("start cell absent")?;
                ensure!(
                    scene.playfield.contains_height_aware_packed(
                        at[0].into(),
                        at[1].into(),
                        cell.z as i8,
                        0
                    ),
                    "start outside playable area"
                );
            }
            let ore = map
                .overlays
                .iter()
                .filter(|o| matches!(o.overlay_id, 27..=38 | 102..=113))
                .count();
            if options.ore {
                ensure!(ore > 0, "no resources");
                if options.terrain.resources == 1 {
                    ensure!(
                        !map.overlays.iter().any(|o| matches!(o.overlay_id, 27..=38)),
                        "gems disabled but remain"
                    );
                }
            } else {
                ensure!(
                    ore == 0
                        && !map
                            .terrain_objects
                            .iter()
                            .any(|t| t.name.starts_with("TIBTRE")),
                    "ore disabled but remains"
                );
            }
            if !options.tech_buildings {
                ensure!(
                    map.entities.iter().all(|e| e.type_id == "CABHUT"),
                    "tech buildings disabled but remain"
                );
            }
            fingerprints.insert(
                map.cells
                    .iter()
                    .map(|c| (c.tile_index, c.sub_tile, c.z))
                    .collect::<Vec<_>>(),
            );
            std::fs::write(output.join(format!("{name}.map")), bytes)?;
            let bounds = scene
                .visual
                .bounds
                .context("generated map has no visual bounds")?;
            let scale = 1200. / bounds.width().max(bounds.height());
            let size = [
                (bounds.width() * scale).ceil() as u32,
                (bounds.height() * scale).ceil() as u32,
            ];
            let rgba = scene.visual.raster(bounds, size, ViewOptions::default())?;
            image::save_buffer(
                output.join(format!("{name}.png")),
                &rgba,
                size[0],
                size[1],
                image::ColorType::Rgba8,
            )?;
            Ok(
                serde_json::json!({"case":name,"passed":true,"cells":map.cells.len(),"starts":map.waypoints.len(),"ore_cells":ore,"terrain_objects":map.terrain_objects.len(),"structures":map.entities.len(),"diagnostics":scene.diagnostics,"seed":options.terrain.seed}),
            )
        })();
        report.push(match result {
            Ok(report) => report,
            Err(error) => {
                eprintln!("{name}: {error:#}");
                serde_json::json!({"case":name,"passed":false,"error":format!("{error:#}")})
            }
        });
    }
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    ensure!(
        report.iter().all(|r| r["passed"] == true),
        "random-map audit failed; see report.json"
    );
    ensure!(
        fingerprints.len() > 5,
        "landscapes and seed produced identical terrain"
    );
    println!("{} random-map cases passed", report.len());
    Ok(())
}
