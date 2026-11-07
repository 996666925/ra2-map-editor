//! Extended archipelagos share a bounded number of sufficiently large islands.
//! The retail water phase couples island count to players, making 9..30-player
//! islands too small for its later region survival and 6x6 start gates.
use vera20k_core::map::rmg::{
    self, Gaussian, MapGeometry, RmgGrid, RmgRng, RmgScratch, Stage,
    build::GenerationPoint,
    phases::{
        blob, green_spread, hills, lat_fixup, lat_patches, regions, rocks, starts, tech_buildings,
        tiberium, trees, water, water_finalize, zones,
    },
    pipeline::{PipelineInputs, PipelineOutput},
};

pub(super) fn run(
    grid: &mut RmgGrid,
    scratch: &mut RmgScratch,
    rng: &mut RmgRng,
    gauss: &mut Gaussian,
    geometry: &MapGeometry,
    input: &PipelineInputs<'_>,
    observe: &mut dyn FnMut(GenerationPoint),
) -> PipelineOutput {
    let MapGeometry {
        map_w,
        map_h,
        gen_w,
        gen_h,
        stride,
        ..
    } = *geometry;
    let local_rect = [2, 5, gen_w, gen_h];
    water::run(
        &mut blob::BlobCtx {
            grid,
            scratch,
            ids: input.ids,
            blocks: input.blocks,
            rng,
            gauss,
            trig: input.trig,
            map_w,
            map_h,
            rollback_level: 4,
        },
        &water::WaterArgs {
            map_type: 0,
            water_percent: input.water_percent,
            // Native's two-player layout grows three or four large islands;
            // the later start phase distributes the actual player count.
            num_players: 2,
            bridge_enabled: input.bridge_enabled,
            playable: water::PlayableRect {
                x: 2,
                y: 5,
                w: gen_w,
                h: gen_h,
            },
        },
    );
    observe(GenerationPoint::After(Stage::Water));
    water_finalize::run(grid, scratch, input.ids, input.blocks, rng);
    observe(GenerationPoint::After(Stage::WaterFinalize));
    let mut regions = regions::run(&mut regions::RegionCtx {
        grid,
        scratch,
        ids: input.ids,
        rng,
        map_type: 0,
        default_level: 4,
    });
    green_spread::run(grid, input.ids, rng);
    lat_fixup::run(grid, input.ids);
    let zones = zones::compute(
        grid,
        input.blocks,
        &zones::ZoneParams {
            map_w,
            local_rect,
            wheel_impassable: input.wheel_impassable,
            cliff_back_impassability: input.cliff_back_impassability,
            overlays: input.overlay_zones,
        },
        zones::ZoneKind::for_map_type(0),
    );
    let args = starts::StartsArgs {
        start_quota: input.num_players,
        num_players: input.num_players,
        tiberium_layout: input.tiberium_layout,
        gen_w,
        gen_h,
        map_w,
    };
    let outcome = {
        let mut ctx = starts::StartsCtx {
            grid,
            scratch,
            ids: input.ids,
            rng,
            regions: &mut regions,
        };
        starts::run(&mut ctx, &args, &zones)
    };
    // CreateStartingPoints @ 0x00594B50 places tech on start-free islands
    // before MarkStartAreas @ 0x005A1FB0. Reuse the engine's placement and RNG
    // ordering, including the coin flips when tech buildings are disabled.
    let placements = tech_buildings::run_start_flips_traced(
        &mut tech_buildings::TechCtx {
            grid,
            scratch,
            ids: input.ids,
            blocks: input.blocks,
            rng,
            regions: &regions,
            types: input.tech_types,
        },
        &tech_buildings::TechArgs {
            map_type: 0,
            map_w,
            local_rect,
            stride: stride as i32,
            cliff_back_impassability: input.cliff_back_impassability,
        },
        &outcome.region_order,
        &mut rmg::RmgConstructionTrace::default(),
        0,
    );
    starts::mark_start_areas(
        &mut starts::StartsCtx {
            grid,
            scratch,
            ids: input.ids,
            rng,
            regions: &mut regions,
        },
        &args,
        &outcome,
    );
    observe(GenerationPoint::After(Stage::Starts));
    // All player quotas, protected clearings and resource fields use the
    // full extended count.
    let resources = tiberium::run(
        &mut tiberium::TiberiumCtx {
            grid,
            scratch,
            ids: input.ids,
            rng,
            gauss,
            regions: &regions,
            waypoints: &outcome.waypoints,
        },
        &tiberium::TiberiumArgs {
            map_type: 0,
            resources: input.resources,
            tib_option: input.tib_option,
            min_tib: input.min_tib,
            max_tib: input.max_tib,
            map_w,
            local_rect,
        },
    );
    observe(GenerationPoint::After(Stage::Tiberium));
    scratch.reset_region_ids();
    lat_fixup::run(grid, input.ids);
    // RecalcAttributes @ 0x0047D2B0 before CreateHills supplies the stored
    // land type that PlaceTreeRegion @ 0x005A45E0 reads. Later hills and LAT
    // changes must not replace this snapshot (same boundary as run_pipeline).
    let recalc_rock =
        trees::RecalcRock::snapshot(grid, input.blocks, input.cliff_back_impassability);
    hills::run(
        &mut hills::HillsCtx {
            grid,
            scratch,
            ids: input.ids,
            gauss,
            rng,
        },
        &hills::HillsArgs {
            ruggedness: input.ruggedness,
            map_w,
            map_h,
        },
        input.morphable,
    );
    observe(GenerationPoint::After(Stage::Hills));
    lat_patches::run(
        &mut lat_patches::PatchCtx {
            grid,
            scratch,
            ids: input.ids,
            rng,
            gauss,
        },
        input.theater,
        input.vegetation,
    );
    lat_fixup::run(grid, input.ids);
    let mut terrain = resources.trees;
    terrain.extend(trees::run(
        &mut trees::TreeCtx {
            grid,
            scratch,
            ids: input.ids,
            rng,
            gauss,
            recalc_rock: &recalc_rock,
        },
        &trees::TreeArgs {
            width: input.width,
            vegetation: input.vegetation,
            max_trees: input.max_trees,
            stride: stride as i32,
        },
    ));
    if input.theater == 0 {
        rocks::run(
            &mut rocks::RockCtx {
                grid,
                scratch,
                ids: input.ids,
                rng,
            },
            &rocks::RockArgs {
                map_w,
                map_h,
                stride: stride as i32,
            },
        );
    }
    lat_fixup::run(grid, input.ids);
    let mut output = rmg::pipeline::PipelineOutput::default();
    output.waypoints = outcome
        .waypoints
        .iter()
        .enumerate()
        .filter_map(|(slot, at)| at.map(|(x, y)| (slot as u8, x as u16, y as u16)))
        .collect();
    output.terrain = terrain;
    output.structures = placements
        .into_iter()
        .map(|placement| (placement.name, placement.x, placement.y))
        .collect();
    output.unfilled_start_slots = outcome.unfilled_start_slots;
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmg::phases::shore::{SubTile, TileBlock, TileBlocks};

    struct ClearBlocks(TileBlock);

    impl TileBlocks for ClearBlocks {
        fn block(&self, _: i32) -> Option<&TileBlock> {
            Some(&self.0)
        }
    }

    #[test]
    fn start_tech_honors_start_quotas_disabled_types_and_cliff_back_rules() {
        let seed = (0..32)
            .find(|seed| RmgRng::new(*seed).next_unit() < 0.5)
            .unwrap();
        let ids = rmg::TileIds::from_keys(&Default::default());
        let mut regions = regions::Regions::default();
        for (id, quota, at) in [(0, 9, (50, 50)), (1, 0, (60, 60))] {
            let mut region = regions::RmgRegion::rebuilt(id, 0, false, 1);
            region.start_quota = quota;
            region.cells.push(at);
            regions.list.push(region);
        }
        let types = [tech_buildings::TechType {
            name: "CATECH".into(),
            footprint: vec![(0, 0), (1, 0)],
        }];
        let blocks = ClearBlocks(TileBlock {
            width: 1,
            height: 1,
            subtiles: vec![Some(SubTile::default())],
        });
        for (enabled, cliff_back, raised, placed) in [
            (true, 2, false, true),
            (false, 2, false, false),
            (true, 2, true, false),
            (true, 1, true, true),
        ] {
            let mut grid = RmgGrid::new(133, 64, 200);
            let mut scratch = RmgScratch::new(133, 64, 200);
            for at in [(50, 50), (51, 50), (60, 60), (61, 60)] {
                grid.get_mut(at.0, at.1).unwrap().tile = 0;
            }
            if raised {
                // RecalcAttributes' cliff-back test sees the northern cell
                // four levels above this footprint (0x0047D2B0).
                grid.get_mut(60, 59).unwrap().level = 8;
            }
            let mut rng = RmgRng::new(seed);
            let mut trace = rmg::RmgConstructionTrace::default();
            let placements = tech_buildings::run_start_flips_traced(
                &mut tech_buildings::TechCtx {
                    grid: &mut grid,
                    scratch: &mut scratch,
                    ids: &ids,
                    blocks: &blocks,
                    rng: &mut rng,
                    regions: &regions,
                    types: if enabled { &types } else { &[] },
                },
                &tech_buildings::TechArgs {
                    map_type: 0,
                    map_w: 64,
                    local_rect: [2, 5, 60, 56],
                    stride: 133,
                    cliff_back_impassability: cliff_back,
                },
                &[0, 1],
                &mut trace,
                0,
            );
            assert!(!grid.get(50, 50).unwrap().occupied);
            assert_eq!(placements.len(), usize::from(placed));
            assert_eq!(trace.events.len(), usize::from(enabled));
            for at in [(60, 60), (61, 60)] {
                assert_eq!(grid.get(at.0, at.1).unwrap().occupied, placed);
            }
            if placed {
                assert_eq!((placements[0].x, placements[0].y), (60, 60));
            }
            if !enabled {
                let mut expected = RmgRng::new(seed);
                expected.next_u32();
                assert_eq!(rng.next_u32(), expected.next_u32());
            }
        }
    }
}
