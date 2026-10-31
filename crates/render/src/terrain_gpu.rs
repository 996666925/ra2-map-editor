//! Editor terrain adapter: shared Device, TMP RGBA/Z atlas, instanced depth pass.
use anyhow::{Result, ensure};
use ra2_map_scene::map::{MapScene, pixel_depth};

pub use crate::world_gpu::ViewportStats;
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use vera20k_core::map::theater::{TileImage, TileKey};
use wgpu::util::DeviceExt;

fn pack_pixels(
    images: &HashMap<TileKey, TileImage>,
    placements: &HashMap<TileKey, [u32; 2]>,
    width: u32,
    height: u32,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut rgba = vec![0; width as usize * height as usize * 4];
    let mut depth = vec![0; width as usize * height as usize];
    for (key, pos) in placements {
        let image = &images[key];
        ensure!(
            pos[0] + image.width <= width && pos[1] + image.height <= height,
            "TMP 图集坐标越界"
        );
        ensure!(
            image.rgba.len() == image.width as usize * image.height as usize * 4
                && image.depth.len() == image.width as usize * image.height as usize,
            "TMP 图像/深度长度错误"
        );
        for row in 0..image.height as usize {
            let src = row * image.width as usize;
            let dst = (pos[1] as usize + row) * width as usize + pos[0] as usize;
            let n = image.width as usize;
            rgba[dst * 4..(dst + n) * 4].copy_from_slice(&image.rgba[src * 4..(src + n) * 4]);
            depth[dst..dst + n].copy_from_slice(&image.depth[src..src + n]);
        }
    }
    Ok((rgba, depth))
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    position: [f32; 2],
    size: [f32; 2],
    atlas: [u32; 2],
    base: i32,
    padding: u32,
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct View {
    origin: [f32; 2],
    extent: [f32; 2],
}
pub struct TerrainPipelines {
    world: Arc<[wgpu::RenderPipeline; 3]>,
    terrain: wgpu::RenderPipeline,
    quad: wgpu::RenderPipeline,
}
pub struct TerrainGpu {
    world: crate::world_gpu::WorldGpu,
    pipelines: Arc<TerrainPipelines>,
    atlas: wgpu::Texture,
    zatlas: wgpu::Texture,
    instances: wgpu::Buffer,
    count: u32,
    pub paints: AtomicUsize,
}
impl TerrainGpu {
    /// Prepare a replacement without changing resources used by live callbacks.
    /// Record-only scenes can share the immutable terrain and world atlases.
    pub fn updated_records(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &MapScene,
    ) -> Option<Arc<Self>> {
        Some(Arc::new(Self {
            world: self.world.updated(device, queue, &scene.visual)?,
            pipelines: self.pipelines.clone(),
            atlas: self.atlas.clone(),
            zatlas: self.zatlas.clone(),
            instances: self.instances.clone(),
            count: self.count,
            paints: AtomicUsize::new(0),
        }))
    }
    pub fn update_stats(&self) -> serde_json::Value {
        serde_json::json!({"incremental":self.world.incremental,"uploaded_instance_bytes":self.world.uploaded_bytes})
    }
}
pub struct Target {
    pub color: wgpu::Texture,
    pub(crate) depth: wgpu::Texture,
    pub(crate) uniform: wgpu::Buffer,
    pub(crate) world_bind: Vec<wgpu::BindGroup>,
    pub(crate) world_visible: wgpu::Buffer,
    pub(crate) world_cache: std::sync::Mutex<crate::world_gpu::WorldCache>,
    terrain_bind: wgpu::BindGroup,
    quad_bind: wgpu::BindGroup,
    pub size: [u32; 2],
}
impl Target {
    pub fn viewport_stats(&self) -> ViewportStats {
        self.world_cache.lock().unwrap().stats
    }
}
impl TerrainPipelines {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("P1 terrain"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("terrain.wgsl")
                    .replace(
                        "TARGET_SRGB",
                        if format.is_srgb() { "true" } else { "false" },
                    )
                    .into(),
            ),
        });
        let terrain = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("TMP depth terrain"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("terrain_vertex"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("terrain_pixel"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            primitive: Default::default(),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let quad = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Terrain viewport composite"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("quad_vertex"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("quad_pixel"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            depth_stencil: None,
            primitive: Default::default(),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            terrain,
            quad,
            world: crate::world_gpu::pipelines(device),
        }
    }
}
impl TerrainGpu {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        scene: &MapScene,
        pipelines: &mut Option<Arc<TerrainPipelines>>,
    ) -> Result<Arc<Self>> {
        let pipelines = pipelines
            .get_or_insert_with(|| Arc::new(TerrainPipelines::new(device, format)))
            .clone();
        let limit = device.limits().max_texture_dimension_2d.min(8192);
        let width = limit.min(2048);
        let mut x = 0;
        let mut y = 0;
        let mut row = 0;
        let mut placements = std::collections::HashMap::new();
        let mut keys: Vec<_> = scene.images.keys().copied().collect();
        keys.sort_by_key(|k| (k.tile_id, k.sub_tile, k.variant));
        for key in keys {
            let image = &scene.images[&key];
            ensure!(
                image.width <= width && image.height <= limit,
                "TMP 超过图集尺寸"
            );
            if x + image.width > width {
                x = 0;
                y += row;
                row = 0;
            }
            ensure!(
                y + image.height <= limit,
                "地形图集超过 {width}×{limit} 预算"
            );
            placements.insert(key, [x, y]);
            x += image.width;
            row = row.max(image.height);
        }
        let height = (y + row).max(1);
        let texture = |label, format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let atlas = texture("TMP color atlas", wgpu::TextureFormat::Rgba8Unorm);
        let zatlas = texture("TMP depth atlas", wgpu::TextureFormat::R8Uint);
        let (rgba, depth) = pack_pixels(&scene.images, &placements, width, height)?;
        // Two contiguous transfers instead of two driver/staging allocations
        // for each of the thousands of small TMP images.
        for (tex, bytes, bpp) in [(&atlas, &rgba, 4), (&zatlas, &depth, 1)] {
            queue.write_texture(
                tex.as_image_copy(),
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * bpp),
                    rows_per_image: Some(height),
                },
                tex.size(),
            );
        }
        let instances: Vec<_> = scene
            .cells
            .iter()
            .filter_map(|cell| {
                let image = scene.image(cell)?;
                Some(Instance {
                    position: [
                        cell.screen_x + image.offset_x as f32,
                        cell.screen_y + image.offset_y as f32,
                    ],
                    size: [image.width as f32, image.height as f32],
                    atlas: placements[&MapScene::key(cell)],
                    base: i32::from(pixel_depth(cell, 0)),
                    padding: 0,
                })
            })
            .collect();
        let count = instances.len() as u32;
        let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("P1 immutable terrain instances"),
            contents: if instances.is_empty() {
                &[0; 32]
            } else {
                bytemuck::cast_slice(&instances)
            },
            usage: wgpu::BufferUsages::STORAGE,
        });
        Ok(Arc::new(Self {
            world: crate::world_gpu::WorldGpu::new(
                device,
                queue,
                &scene.visual,
                pipelines.world.clone(),
            )?,
            pipelines,
            atlas,
            zatlas,
            instances,
            count,
            paints: AtomicUsize::new(0),
        }))
    }
    pub fn target(&self, device: &wgpu::Device, size: [u32; 2]) -> Arc<Target> {
        let size = size.map(|n| n.clamp(1, device.limits().max_texture_dimension_2d));
        let create = |format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("P1 viewport"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let color = create(
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = create(
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Terrain camera"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let terrain_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.pipelines.terrain.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        &self.atlas.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        &self.zatlas.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.instances.as_entire_binding(),
                },
            ],
        });
        let quad_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.pipelines.quad.get_bind_group_layout(1),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &color.create_view(&Default::default()),
                ),
            }],
        });
        let world_visible = self.world.visible_buffer(device);
        Arc::new(Target {
            world_bind: self.world.bindings(device, &uniform, &world_visible),
            world_visible,
            world_cache: Default::default(),
            color,
            depth,
            uniform,
            terrain_bind,
            quad_bind,
            size,
        })
    }
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        target: &Target,
        world: egui::Rect,
    ) {
        target.world_cache.lock().unwrap().invalidate();
        queue.write_buffer(
            &target.uniform,
            0,
            bytemuck::bytes_of(&View {
                origin: world.min.into(),
                extent: world.size().into(),
            }),
        );
        let colors = [Some(wgpu::RenderPassColorAttachment {
            view: &target.color.create_view(&Default::default()),
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: 0.045,
                    g: 0.06,
                    b: 0.08,
                    a: 1.,
                }),
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("P1 terrain Z pass"),
            color_attachments: &colors,
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target.depth.create_view(&Default::default()),
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&self.pipelines.terrain);
        pass.set_bind_group(0, &target.terrain_bind, &[]);
        pass.draw(0..6, 0..self.count);
    }
    pub fn callback(
        self: &Arc<Self>,
        rect: egui::Rect,
        target: Arc<Target>,
        world: egui::Rect,
        options: ra2_map_scene::visual::ViewOptions,
    ) -> egui::PaintCallback {
        egui_wgpu::Callback::new_paint_callback(
            rect,
            Draw {
                gpu: self.clone(),
                options,
                target,
                world,
            },
        )
    }
    /// Read back native-scale pixels through the actual terrain pipeline and
    /// compare them with independently sampled alpha/Z winners on the CPU.
    pub fn verify(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &MapScene,
        output: &std::path::Path,
    ) -> Result<serde_json::Value> {
        use anyhow::Context;
        let probes = scene.pick_checks()?;
        let mut checks = Vec::new();
        for probe in probes["visible_pixel_probes"].as_array().unwrap() {
            let point = egui::pos2(
                probe["world"][0].as_f64().unwrap() as f32,
                probe["world"][1].as_f64().unwrap() as f32,
            );
            let min = egui::pos2(point.x.floor() - 32., point.y.floor() - 32.);
            let world = egui::Rect::from_min_size(min, egui::vec2(64., 64.));
            let target = self.target(device, [64, 64]);
            let mut encoder = device.create_command_encoder(&Default::default());
            self.render(&mut encoder, queue, &target, world);
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 64 * 256,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                target.color.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(64),
                    },
                },
                target.color.size(),
            );
            let submission = queue.submit([encoder.finish()]);
            let (tx, rx) = std::sync::mpsc::channel();
            readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            device.poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(15)),
            })?;
            rx.recv_timeout(std::time::Duration::from_secs(15))
                .context("地形 GPU 读回超时")??;
            let bytes = readback.slice(..).get_mapped_range()?;
            let mut count = 0;
            let mut max_error = 0;
            for y in 0..64 {
                for x in 0..64 {
                    let world = min + egui::vec2(x as f32 + 0.5, y as f32 + 0.5);
                    if let Some(index) = scene.pick(world) {
                        let cell = &scene.cells[index];
                        let rect = scene.cell_rect(cell).unwrap();
                        let image = scene.image(cell).unwrap();
                        let p = ((world.y - rect.min.y).floor() as usize * image.width as usize
                            + (world.x - rect.min.x).floor() as usize)
                            * 4;
                        for c in 0..4 {
                            max_error = max_error
                                .max(bytes[(y * 64 + x) * 4 + c].abs_diff(image.rgba[p + c]));
                        }
                        count += 1;
                    }
                }
            }
            let label = probe["probe"].as_str().unwrap();
            image::save_buffer(
                output.join(format!("terrain-{label}.png")),
                &bytes,
                64,
                64,
                image::ColorType::Rgba8,
            )?;
            ensure!(
                count > 0 && max_error <= 1,
                "地形 GPU/CPU 不一致：{label}, pixels={count}, error={max_error}"
            );
            checks
                .push(serde_json::json!({"probe":label,"pixels":count,"max_rgba_error":max_error}));
        }
        Ok(serde_json::json!({"picking":probes,"gpu_pixels":checks,"instances":self.count}))
    }
}
struct Draw {
    options: ra2_map_scene::visual::ViewOptions,
    gpu: Arc<TerrainGpu>,
    target: Arc<Target>,
    world: egui::Rect,
}
impl egui_wgpu::CallbackTrait for Draw {
    fn prepare(
        &self,
        _: &wgpu::Device,
        queue: &wgpu::Queue,
        _: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        _: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        self.gpu
            .world
            .render(encoder, queue, &self.target, self.world, self.options);
        vec![]
    }
    fn paint(
        &self,
        _: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        _: &egui_wgpu::CallbackResources,
    ) {
        pass.set_pipeline(&self.gpu.pipelines.quad);
        pass.set_bind_group(1, &self.target.quad_bind, &[]);
        pass.draw(0..6, 0..1);
        self.gpu.paints.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(id: u16) -> TileKey {
        TileKey {
            tile_id: id,
            sub_tile: 0,
            variant: 0,
        }
    }
    #[test]
    fn packed_atlas_keeps_pixel_rows_alpha_depth_and_empty_gaps() {
        let images = HashMap::from([
            (
                key(1),
                TileImage {
                    rgba: vec![1, 2, 3, 255, 4, 5, 6, 0, 7, 8, 9, 255, 10, 11, 12, 255],
                    depth: vec![1, 29, 0, 255],
                    width: 2,
                    height: 2,
                    offset_x: -30,
                    offset_y: -45,
                },
            ),
            (
                key(2),
                TileImage {
                    rgba: vec![50, 60, 70, 255],
                    depth: vec![91],
                    width: 1,
                    height: 1,
                    offset_x: 0,
                    offset_y: 0,
                },
            ),
        ]);
        let placements = HashMap::from([(key(1), [1, 1]), (key(2), [0, 3])]);
        let (rgba, depth) = pack_pixels(&images, &placements, 4, 4).unwrap();
        assert_eq!(&rgba[5 * 4..7 * 4], &[1, 2, 3, 255, 4, 5, 6, 0]);
        assert_eq!(&rgba[9 * 4..11 * 4], &[7, 8, 9, 255, 10, 11, 12, 255]);
        assert_eq!(&rgba[12 * 4..13 * 4], &[50, 60, 70, 255]);
        assert_eq!(
            depth,
            vec![0, 0, 0, 0, 0, 1, 29, 0, 0, 0, 255, 0, 91, 0, 0, 0]
        );
        for i in [0, 1, 2, 3, 4, 7, 8, 11, 13, 14, 15] {
            assert_eq!(&rgba[i * 4..i * 4 + 4], &[0; 4]);
        }
        assert!(pack_pixels(&images, &placements, 2, 4).is_err());
    }
    #[test]
    fn truncated_depth_plane_is_an_error_instead_of_invalid_texture_upload() {
        let images = HashMap::from([(
            key(1),
            TileImage {
                rgba: vec![1, 2, 3, 255],
                depth: vec![],
                width: 1,
                height: 1,
                offset_x: 0,
                offset_y: 0,
            },
        )]);
        assert!(pack_pixels(&images, &HashMap::from([(key(1), [0, 0])]), 1, 1).is_err());
    }
}

#[path = "world_checks.rs"]
mod world_checks;
