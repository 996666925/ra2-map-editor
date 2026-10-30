//! Asset-backed random-map construction. Runs on the map loader worker.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::random_map::RandomMapOptions;
use vera20k_core::{
    map::rmg::{
        self, Stage,
        build::{self, GenerationPoint, ResolvedTheaterInputs},
        phases::shore::TileBlocks,
        theater_blocks::TheaterTileBlocks,
    },
    rules::{ini_parser::IniFile, native_processing::RulesLayerStack, terrain_rules::TerrainRules},
};
#[path = "random_archipelago.rs"]
mod archipelago;

pub fn generate(
    install: &mut Install,
    options: &RandomMapOptions,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(&'static str),
) -> Result<Vec<u8>> {
    options.validate()?;
    ensure!(!cancelled(), "随机地图生成已取消");
    install.activate_theater(rmg::emit::theater_name(options.terrain.theater))?;
    progress("准备随机地图地形模板…");
    let rules = TerrainRules::from_ini(&install.catalog.rules);
    let resolved = ResolvedTheaterInputs::from_theater(&install.theater, &rules);
    let blocks = TheaterTileBlocks::build(&install.theater.lookup, |name| {
        install.manager.get_ref(name).map(<[u8]>::to_vec)
    });
    let settings = rmg::RmgSettings::load(&install.manager);
    let tech = if options.tech_buildings {
        let processed = RulesLayerStack::new(install.catalog.rules.clone()).process()?;
        let art = install
            .manager
            .get_ref("artmd.ini")
            .map(IniFile::from_bytes)
            .transpose()?
            .unwrap_or_else(IniFile::empty);
        rmg::tech_catalog::resolve(processed.neutral_tech_buildings(), &art)
    } else {
        vec![]
    };
    ensure!(!cancelled(), "随机地图生成已取消");
    let mut recipe = options.clone();
    let mut accepted = None;
    // Rejection sampling keeps the requested configuration and determinism,
    // while skipping native candidates whose surviving regions lack starts.
    // The odd step visits distinct 16-bit seeds. Never return a partial map.
    for attempt in 0..16 {
        ensure!(!cancelled(), "随机地图生成已取消");
        recipe.terrain.seed = (options.terrain.seed + attempt * 7919) & 0xffff;
        let candidate = generate_candidate(
            &recipe,
            &settings,
            &resolved,
            &blocks,
            &tech,
            &mut |stage| {
                if cancelled() {
                    return;
                }
                progress(match stage {
                    GenerationPoint::Initial => "生成随机地貌与水域…",
                    GenerationPoint::After(Stage::Starts) => "布置玩家起点与资源…",
                    GenerationPoint::After(Stage::Tiberium) => "生成山地与地表过渡…",
                    GenerationPoint::After(Stage::Hills) => "生成树林与地表细节…",
                    _ => return,
                });
            },
        );
        if valid_starts(&candidate, recipe.terrain.num_players) {
            accepted = Some(candidate);
            break;
        }
        progress("重新选择可容纳全部玩家的地形…");
    }
    ensure!(!cancelled(), "随机地图生成已取消");
    let mut generated = accepted.context("无法容纳全部玩家起点，请减少水量、玩家数或更换种子")?;
    normalize_water_levels(&mut generated.map_file, &resolved.ids, &blocks);
    if !options.ore {
        generated
            .map_file
            .overlays
            .retain(|e| !matches!(e.overlay_id, 27..=38 | 102..=113));
        generated
            .map_file
            .terrain_objects
            .retain(|t| !t.name.starts_with("TIBTRE"));
    } else if options.terrain.resources != 3 {
        // Native resource modes can still put a gem field in some map types.
        // The editor's explicit gem toggle consistently means ore only.
        for entry in &mut generated.map_file.overlays {
            if matches!(entry.overlay_id, 27..=38) {
                entry.overlay_id += 75;
            }
        }
    }
    progress("编码随机地图…");
    let bytes = recipe.map_bytes(&generated.map_file, |tile| {
        let tile = if tile < 0 { resolved.ids.clear } else { tile };
        u16::try_from(tile)
            .ok()
            .and_then(|id| install.catalog.raw_tiles.get(&id).copied())
            .with_context(|| format!("随机地形 {tile} 无法映射为可保存的地图索引"))
    })?;
    Ok(vera20k_core::util::ini_writer::set_ini_values(
        &bytes,
        "RandomMap",
        &[("EditorRequestedSeed", &options.terrain.seed.to_string())],
    ))
}

/// The engine finalizer assigns multi-cell water the TMP-relative height,
/// while single-cell water retains the initial map height (4). Anchor each
/// connected water surface to its shoreline, keeping elevated lakes separate.
fn normalize_water_levels(
    map: &mut vera20k_core::map::map_file::MapFile,
    ids: &rmg::TileIds,
    blocks: &dyn TileBlocks,
) {
    use std::collections::{HashMap, HashSet, VecDeque};
    if ids.water_base < 0 {
        return;
    }
    let index: HashMap<_, _> = map
        .cells
        .iter()
        .enumerate()
        .map(|(i, c)| ((i32::from(c.rx), i32::from(c.ry)), i))
        .collect();
    let water = |tile| (ids.water_base..ids.water_base + 14).contains(&tile);
    let relative_height = |tile, sub: u8| {
        blocks
            .block(tile)
            .and_then(|b| b.subtiles.get(sub as usize))
            .and_then(|s| *s)
            .map_or(0, |s| s.height)
    };
    let mut visited = HashSet::new();
    for first in 0..map.cells.len() {
        if !water(map.cells[first].tile_index) || !visited.insert(first) {
            continue;
        }
        let mut queue = VecDeque::from([first]);
        let mut component = vec![];
        let mut shores = [0usize; 15];
        let mut fallback = 0;
        while let Some(i) = queue.pop_front() {
            component.push(i);
            let cell = &map.cells[i];
            fallback = fallback.max(
                cell.z
                    .saturating_sub(relative_height(cell.tile_index, cell.sub_tile)),
            );
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let Some(&n) = index.get(&(i32::from(cell.rx) + dx, i32::from(cell.ry) + dy))
                else {
                    continue;
                };
                let next = &map.cells[n];
                if water(next.tile_index) {
                    if visited.insert(n) {
                        queue.push_back(n);
                    }
                } else if ids.shore >= 0 && (ids.shore..ids.shore + 42).contains(&next.tile_index) {
                    let base = next
                        .z
                        .saturating_sub(relative_height(next.tile_index, next.sub_tile));
                    if let Some(count) = shores.get_mut(base as usize) {
                        *count += 1;
                    }
                }
            }
        }
        let base = shores
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
            .max_by_key(|(level, count)| (**count, *level))
            .map_or(fallback, |(level, _)| level as u8);
        for i in component {
            let cell = &mut map.cells[i];
            cell.z = base
                .saturating_add(relative_height(cell.tile_index, cell.sub_tile))
                .min(14);
        }
    }
}

struct Candidate {
    map_file: vera20k_core::map::map_file::MapFile,
    unfilled_start_slots: usize,
}

/// The retail builder normalizes NumPlayers back to eight. The underlying
/// pipeline uses dynamic start/region lists, so extended maps call it directly
/// with the editor's geometry and player quota, including resources/clearings.
fn generate_candidate(
    recipe: &RandomMapOptions,
    settings: &rmg::RmgSettings,
    resolved: &ResolvedTheaterInputs,
    blocks: &dyn TileBlocks,
    tech: &[rmg::phases::tech_buildings::TechType],
    observe: &mut dyn FnMut(GenerationPoint),
) -> Candidate {
    let options = &recipe.terrain;
    if options.num_players <= 8 {
        let generated =
            build::generate_map_observed(options, settings, resolved, blocks, tech, &mut |view| {
                observe(view.point())
            });
        return Candidate {
            map_file: generated.map_file,
            unfilled_start_slots: generated.unfilled_start_slots,
        };
    }
    let geometry = recipe.geometry();
    let mut rng = rmg::RmgRng::new(options.seed_u16());
    let bridge_enabled = rng.next_unit() < 0.25;
    let mut grid = rmg::RmgGrid::new(geometry.stride, geometry.diamond_min, geometry.diamond_max);
    let mut scratch =
        rmg::RmgScratch::new(geometry.stride, geometry.diamond_min, geometry.diamond_max);
    let mut gaussian = rmg::Gaussian::default();
    let morphable = |tile: i32| {
        usize::try_from(tile)
            .ok()
            .and_then(|id| resolved.morphable.get(id))
            .copied()
            .unwrap_or(false)
    };
    let inputs = rmg::pipeline::PipelineInputs {
        ids: &resolved.ids,
        blocks,
        morphable: &morphable,
        wheel_impassable: resolved.wheel_impassable,
        tech_types: tech,
        trig: resolved.trig,
        map_type: options.map_type,
        accessibility: options.accessibility,
        theater: options.theater,
        num_players: options.num_players,
        bridge_enabled,
        water_percent: options.water_amount,
        resources: options.resources,
        tib_option: options.tiberium,
        min_tib: settings.min_tiberium,
        max_tib: settings.max_tiberium,
        tiberium_layout: options.tiberium_layout,
        ruggedness: options.ruggedness,
        vegetation: options.vegetation,
        width: options.width,
        max_trees: settings.max_trees,
    };
    observe(GenerationPoint::Initial);
    let output = if options.map_type == 0 {
        archipelago::run(
            &mut grid,
            &mut scratch,
            &mut rng,
            &mut gaussian,
            &geometry,
            &inputs,
            observe,
        )
    } else {
        rmg::pipeline::run_pipeline(
            &mut grid,
            &mut scratch,
            &mut rng,
            &mut gaussian,
            &geometry,
            &inputs,
            &mut |stage, _, _, _| observe(GenerationPoint::After(stage)),
        )
    };
    let mut map_file =
        rmg::emit::empty_map_file(options, geometry.gen_w as u32, geometry.gen_h as u32);
    rmg::emit::populate(
        &mut map_file,
        &grid,
        &output.terrain,
        &output.structures,
        &output.waypoints,
    );
    Candidate {
        map_file,
        unfilled_start_slots: output.unfilled_start_slots,
    }
}

fn valid_starts(generated: &Candidate, players: i32) -> bool {
    use std::collections::BTreeSet;
    use vera20k_core::map::playfield::PlayfieldBounds;
    let map = &generated.map_file;
    if generated.unfilled_start_slots != 0 || map.waypoints.len() != players as usize {
        return false;
    }
    let bounds = PlayfieldBounds::from_map_header(&map.header);
    let mut unique = BTreeSet::new();
    (0..players as u32).all(|slot| {
        map.waypoints.get(&slot).is_some_and(|start| {
            unique.insert((start.rx, start.ry))
                && map.cells.iter().any(|cell| {
                    (cell.rx, cell.ry) == (start.rx, start.ry)
                        && bounds.contains_height_aware_packed(
                            cell.rx.into(),
                            cell.ry.into(),
                            cell.z as i8,
                            0,
                        )
                })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vera20k_core::map::{
        map_file::MapCell,
        rmg::phases::shore::{SubTile, TileBlock},
    };

    #[test]
    fn water_surfaces_follow_shores_without_flattening_separate_elevated_lakes() {
        struct Blocks(TileBlock);
        impl TileBlocks for Blocks {
            fn block(&self, _: i32) -> Option<&TileBlock> {
                Some(&self.0)
            }
        }
        let blocks = Blocks(TileBlock {
            width: 1,
            height: 1,
            subtiles: vec![Some(SubTile::default())],
        });
        let mut ids = rmg::TileIds::from_keys(&Default::default());
        ids.water_base = 100;
        ids.shore = 200;
        let mut map = rmg::emit::empty_map_file(&Default::default(), 70, 70);
        let cell = |rx, ry, tile_index, z| MapCell {
            rx,
            ry,
            tile_index,
            z,
            sub_tile: 0,
        };
        map.cells = vec![
            cell(20, 20, 100, 0),
            cell(21, 20, 108, 4),
            cell(22, 20, 200, 4),
            cell(30, 30, 100, 0),
            cell(31, 30, 108, 4),
            cell(32, 30, 200, 8),
            cell(40, 40, 0, 9),
        ];
        normalize_water_levels(&mut map, &ids, &blocks);
        assert_eq!(
            map.cells.iter().map(|c| c.z).collect::<Vec<_>>(),
            [4, 4, 4, 8, 8, 8, 9]
        );
    }
}
