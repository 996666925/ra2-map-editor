//! Generate, reopen and render random maps against a real installation.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{document::MapDocument, random_map::RandomMapOptions, source::SourceMap};
use ra2_map_scene::{map::load_scene_bytes, random_map, visual::ViewOptions};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::Instant,
};
use vera20k_core::map::entities::EntityCategory;

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory required")?);
    let output = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "target/random-map-audit".into()),
    );
    let filter = args
        .next()
        .map(|value| value.to_string_lossy().into_owned());
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
    cases.push((
        "ore-without-gems".into(),
        RandomMapOptions {
            gems: false,
            ..Default::default()
        },
    ));
    for resources in [0, 2] {
        let mut options = RandomMapOptions::default();
        options.terrain.resources = resources;
        cases.push((format!("engine-resource-level-{resources}"), options));
    }
    let mut derelicts = RandomMapOptions {
        tech_buildings: false,
        ..Default::default()
    };
    derelicts.terrain.derelict_amount = 100;
    derelicts.terrain.damage_amount = 100;
    cases.push(("engine-derelicts-without-towns".into(), derelicts));
    let mut extras = RandomMapOptions::default();
    extras
        .terrain
        .extras
        .set(vera20k_core::map::rmg::extras::Extra::Landmark, 1);
    extras
        .terrain
        .extras
        .set(vera20k_core::map::rmg::extras::Extra::Mood, 2);
    cases.push(("engine-landmark-and-dusk-without-towns".into(), extras));
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
    for players in [2, 30] {
        for landscape in [3, 4] {
            for region_size in [0, 100] {
                let mut options = RandomMapOptions::default();
                options.terrain.num_players = players;
                options.terrain.map_type = landscape;
                options.terrain.region_size = region_size;
                cases.push((
                    format!("region-size-{players}-{landscape}-{region_size}"),
                    options,
                ));
            }
        }
    }
    for players in [2, 9, 30] {
        for enabled in [true, false] {
            let mut options = RandomMapOptions::default();
            options.terrain.num_players = players;
            options.terrain.map_type = 0;
            options.terrain.water_amount = 50;
            options.tech_buildings = enabled;
            cases.push((format!("archipelago-tech-{players}-{enabled}"), options));
        }
    }
    for (enabled, presence) in [(false, 100), (true, 0), (true, 1), (true, 100)] {
        let mut options = RandomMapOptions::default();
        options.terrain.urban_enabled = enabled;
        options.terrain.urban_presence = presence;
        cases.push((format!("towns-{enabled}-{presence}"), options));
    }
    for theater in 1..5 {
        let mut options = RandomMapOptions::default();
        options.terrain.theater = theater;
        options.terrain.urban_enabled = true;
        options.terrain.urban_presence = 100;
        cases.push((format!("urban-theater-{theater}"), options));
    }
    type TownBaseline = (
        Vec<(i32, u8, u8)>,
        BTreeSet<(u32, u16, u16)>,
        Vec<(String, u16, u16)>,
    );
    let mut town_baseline: Option<TownBaseline> = None;
    let mut fingerprints = BTreeSet::new();
    let mut region_shapes = BTreeMap::new();
    if let Some(filter) = &filter {
        cases.retain(|(name, _)| name.contains(filter));
        ensure!(!cases.is_empty(), "no audit cases match {filter}");
    }
    for (name, options) in cases {
        eprintln!("Generating {name}");
        let result = (|| -> Result<serde_json::Value> {
            let bytes = random_map::generate(&mut install, &options, &|| false, &|_| {})?;
            let repeated = random_map::generate(&mut install, &options, &|| false, &|_| {})?;
            ensure!(bytes == repeated, "same seed is not deterministic");
            let (source, map) =
                SourceMap::parse(format!("{name}.map"), "audit".into(), bytes.clone())?;
            if name.starts_with("towns-") {
                let shape: Vec<_> = map
                    .cells
                    .iter()
                    .map(|c| (c.tile_index, c.sub_tile, c.z))
                    .collect();
                let starts: BTreeSet<_> = map
                    .waypoints
                    .iter()
                    .map(|(id, w)| (*id, w.rx, w.ry))
                    .collect();
                let objects = map
                    .entities
                    .iter()
                    .filter(|e| e.category == EntityCategory::Structure)
                    .map(|e| (e.type_id.clone(), e.cell_x, e.cell_y))
                    .collect::<Vec<_>>();
                let stored = map.ini.section_or_empty("RandomMap");
                ensure!(
                    stored.read_bool("VeraUrban", false) == options.terrain.urban_enabled,
                    "town switch lost"
                );
                ensure!(
                    stored.read_int("UrbanPresence", -1) == options.terrain.urban_presence,
                    "town presence lost"
                );
                if let Some((base_shape, base_starts, base_objects)) = &town_baseline {
                    ensure!(&starts == base_starts, "towns changed starts");
                    if options.terrain.urban_presence == 0 {
                        ensure!(&shape == base_shape, "zero presence changed terrain");
                        ensure!(&objects == base_objects, "zero presence added towns");
                    } else {
                        ensure!(&shape != base_shape, "urban districts did not pave terrain");
                        ensure!(
                            objects.len() > base_objects.len(),
                            "towns were not generated"
                        );
                        ensure!(
                            base_objects.iter().all(|object| objects.contains(object)),
                            "towns changed existing structures"
                        );
                    }
                } else {
                    town_baseline = Some((shape, starts, objects));
                }
            }
            let reopened =
                RandomMapOptions::from_source(&source).context("recipe could not be restored")?;
            let mut expected_terrain = options.terrain.clone();
            expected_terrain.seed = map.ini.section_or_empty("RandomMap").read_int("Seed", -1);
            ensure!(
                reopened.terrain == expected_terrain,
                "terrain recipe changed"
            );
            ensure!(
                reopened.ore == options.ore
                    && reopened.gems == options.gems
                    && reopened.tech_buildings == options.tech_buildings,
                "editor switches changed"
            );
            if options.terrain.derelict_amount > 0 {
                use vera20k_core::map::rmg::{RmgSettings, extras::derelict_types_from_lists};
                let mut owner =
                    ra2_map_assets::mission_resources::rules_process_owner(&install.manager, None)
                        .map_err(anyhow::Error::msg)?;
                owner.prepare_random_map_preview(None, map.header.theater.as_str(), true)?;
                let rules = vera20k_core::rules::ruleset::RuleSet::from_processed_rules(
                    owner
                        .prepared_random_map_rules()
                        .context("preview rules missing")?,
                )?;
                let settings = RmgSettings::load(&install.manager);
                let types = if settings.extras.derelicts.is_empty() {
                    derelict_types_from_lists(
                        rules.build_const_types(),
                        &rules.build_barracks_types,
                        &rules.build_tech_types,
                        &rules.build_weapons_types,
                    )
                } else {
                    settings.extras.derelicts
                };
                ensure!(
                    !map.entities.is_empty(),
                    "no derelicts with towns and tech buildings disabled"
                );
                for entity in &map.entities {
                    ensure!(
                        entity.category == EntityCategory::Structure
                            && entity.owner == "Neutral"
                            && !entity.structure_online
                            && entity.health == 34
                            && types
                                .iter()
                                .any(|id| id.eq_ignore_ascii_case(&entity.type_id)),
                        "derelict {} lost its Rules type, owner, damage or power state",
                        entity.type_id
                    );
                }
            }
            if name == "engine-landmark-and-dusk-without-towns" {
                let settings = vera20k_core::map::rmg::RmgSettings::load(&install.manager);
                ensure!(
                    map.entities.iter().any(|entity| settings
                        .extras
                        .landmarks
                        .iter()
                        .any(|id| id.eq_ignore_ascii_case(&entity.type_id))),
                    "no landmark generated without towns"
                );
                let (ambient, [red, green, blue]) = settings.extras.moods[1];
                let light = vera20k_core::map::lighting::parse_lighting_profiles(&map.ini).normal;
                ensure!(
                    [
                        light.ambient_percent,
                        light.red_percent,
                        light.green_percent,
                        light.blue_percent
                    ] == [ambient, red, green, blue],
                    "generated mood lost in map export"
                );
            }
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
            ensure!(
                map.ini
                    .section_or_empty("RandomMap")
                    .read_int("RegionSize", -1)
                    == options.terrain.region_size,
                "recipe lost region size"
            );
            if name.starts_with("region-size-") {
                let key = (options.terrain.num_players, options.terrain.map_type);
                let shape: Vec<_> = map
                    .cells
                    .iter()
                    .map(|c| (c.tile_index, c.sub_tile, c.z))
                    .collect();
                if options.terrain.region_size == 0 {
                    region_shapes.insert(key, shape);
                } else {
                    ensure!(
                        region_shapes.get(&key) != Some(&shape),
                        "region size did not affect terrain"
                    );
                }
            }
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
                if !options.gems {
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
            if !options.tech_buildings && options.terrain.derelict_amount == 0 {
                ensure!(
                    map.entities.iter().all(|e| e.type_id == "CABHUT"),
                    "tech buildings disabled but remain"
                );
            }
            let tech: Vec<_> = map
                .entities
                .iter()
                .filter(|e| e.category == EntityCategory::Structure && e.type_id != "CABHUT")
                .collect();
            ensure!(
                tech.iter().all(|e| e.owner == "Neutral"),
                "incorrect tech owner"
            );
            if name == "archipelago-tech-2-true" {
                ensure!(!tech.is_empty(), "start-free islands lost tech buildings");
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
                serde_json::json!({"case":name,"passed":true,"cells":map.cells.len(),"starts":map.waypoints.len(),"ore_cells":ore,"terrain_objects":map.terrain_objects.len(),"structures":map.entities.iter().filter(|e| e.category == EntityCategory::Structure).count(),"infantry":map.entities.iter().filter(|e| e.category == EntityCategory::Infantry).count(),"vehicles":map.entities.iter().filter(|e| e.category == EntityCategory::Unit).count(),"tech_buildings":tech.len(),"region_size":options.terrain.region_size,"diagnostics":scene.diagnostics,"seed":map.ini.section_or_empty("RandomMap").read_int("Seed", -1)}),
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
        filter.is_some() || fingerprints.len() > 5,
        "landscapes and seed produced identical terrain"
    );
    println!("{} random-map cases passed", report.len());
    Ok(())
}
