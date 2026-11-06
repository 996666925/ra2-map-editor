//! Adapter for VERA20k's shared CPU/GPU voxel rasterizer.
//!
//! Use the pinned vendor/vera20k submodule shared with the core dependency. Compile
//! the original renderer sources directly: no copied raster implementation,
//! engine modifications, game client, window, audio or simulation startup.
#![allow(dead_code, unused_imports)]
// --all-targets may build a lib-test target despite test=false. The source's
// private engine test harness belongs to the engine workspace, not this adapter.
#![cfg(not(test))]
pub use vera20k_core::{assets, map, rules, sim, util};

#[rustfmt::skip]
#[allow(clippy::all)]
#[path = "../../../vendor/vera20k/src/render/vxl_normals.rs"]
pub mod vxl_normals;
#[rustfmt::skip]
#[allow(clippy::all)]
#[path = "../../../vendor/vera20k/src/render/vxl_raster.rs"]
pub mod vxl_raster;

#[rustfmt::skip]
#[allow(clippy::all)]
#[path = "../../../vendor/vera20k/src/render/native_z.rs"]
pub mod native_z;

#[rustfmt::skip]
#[path = "../../../vendor/vera20k/src/render/voxel_gpu.rs"]
mod voxel_gpu;
#[rustfmt::skip]
#[path = "../../../vendor/vera20k/src/render/atlas_growth.rs"]
mod atlas_growth;
mod gpu;
pub use gpu::Renderer;

pub mod render {
    pub(crate) use crate::atlas_growth;
    pub use crate::{native_z, vxl_normals, vxl_raster};
    // These two descriptor helpers are the GPU raster's only shroud dependency.
    pub(crate) mod shroud_buffer {
        pub(crate) fn layout_entry(
            binding: u32,
            ty: wgpu::BindingType,
        ) -> wgpu::BindGroupLayoutEntry {
            wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty,
                count: None,
            }
        }
        pub(crate) fn texture_entry(
            binding: u32,
            sample_type: wgpu::TextureSampleType,
        ) -> wgpu::BindGroupLayoutEntry {
            layout_entry(
                binding,
                wgpu::BindingType::Texture {
                    sample_type,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            )
        }
    }
}

#[rustfmt::skip]
#[allow(clippy::all)]
#[path = "../../../vendor/vera20k/src/render/foot_depth.rs"]
mod foot_depth;
/// Static map adapter for the engine's FootClass depth and bridge split selectors.
#[allow(clippy::too_many_arguments)]
pub fn static_foot_depth(
    terrain: &map::resolved_terrain::ResolvedTerrainGrid,
    at: [u16; 2],
    high: bool,
    facing: u8,
    world_z: i32,
    bridge_base: i32,
    coefficients: [i32; 4],
    too_big: bool,
) -> (i32, bool) {
    use foot_depth::*;
    let context = FootDepthContext {
        cell: at.map(|n| n as i16),
        on_bridge: high,
        facing_u16: u16::from(facing) * 256,
        world_z_leptons: world_z,
        bridge_set_base: bridge_base,
        ..Default::default()
    };
    let lookup = |coord: [i16; 2]| {
        if let Some(index) = terrain.native_fixed_cell_index(coord[0], coord[1]) {
            let c = &terrain.cells()[index];
            DepthCell {
                coord: [c.rx as i16, c.ry as i16],
                level: c.level as i8,
                ramp: c.slope_type,
                flags: c.bridge_facts.raw_flags,
                iso_tile_index: c.final_tile_index,
                low_bridge: c.is_low_bridge_tube_cell(),
                overlay_rock: None,
                tmp_height: terrain.native_tmp_draw_height(c.final_tile_index, c.final_sub_tile),
                is_dummy: false,
            }
        } else {
            DepthCell {
                coord,
                is_dummy: true,
                ..Default::default()
            }
        }
    };
    (
        foot_z_adjust(
            context,
            FootDepthCoefficients {
                cliff: coefficients[0],
                column: coefficients[1],
                tunnel: coefficients[2],
                bridge: coefficients[3],
            },
            lookup,
        ) - 2,
        unit_composite_split(context, too_big, false, lookup),
    )
}
