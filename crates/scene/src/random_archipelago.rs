//! Extended archipelagos share a bounded number of sufficiently large islands.
//! The retail water phase couples island count to players, making 9..30-player
//! islands too small for its later region survival and 6x6 start gates.
use vera20k_core::map::rmg::{
    self, Gaussian, MapGeometry, RmgGrid, RmgRng, RmgScratch, Stage,
    build::GenerationPoint,
    phases::{
        blob, green_spread, hills, lat_fixup, lat_patches, regions, rocks, starts, tiberium, trees,
        water, water_finalize, zones,
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
    water_finalize::run(grid, input.ids, input.blocks, rng);
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
        },
        zones::ZoneKind::for_map_type(0),
    );
    let outcome = starts::run(
        &mut starts::StartsCtx {
            grid,
            scratch,
            ids: input.ids,
            rng,
            regions: &mut regions,
        },
        &starts::StartsArgs {
            map_type: 0,
            start_quota: input.num_players,
            num_players: input.num_players,
            tiberium_layout: input.tiberium_layout,
            gen_w,
            gen_h,
            map_w,
        },
        &zones,
    );
    observe(GenerationPoint::After(Stage::Starts));
    // Native archipelagos skip neutral-tech placement. All player quotas,
    // protected clearings and resource fields use the full extended count.
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
    output.unfilled_start_slots = outcome.unfilled_start_slots;
    output
}
