//! Static terrain/SHP/VXL scene pages; all primitives test the same Z attachment.
use crate::visibility::VisibilityIndex;
use anyhow::{Result, ensure};
use ra2_map_scene::visual::{ViewOptions, VisualScene};
use wgpu::util::DeviceExt;

#[derive(Clone, Copy, PartialEq)]
struct FrameKey {
    rect: egui::Rect,
    flags: [u32; 4],
}
#[derive(Clone, Copy, Default, serde::Serialize)]
pub struct ViewportStats {
    pub total_instances: usize,
    pub visible_instances: usize,
    pub draw_calls: usize,
    pub scene_renders: usize,
    pub cache_hits: usize,
}
#[derive(Default)]
pub(crate) struct WorldCache {
    key: Option<FrameKey>,
    pub stats: ViewportStats,
}
impl WorldCache {
    pub fn invalidate(&mut self) {
        self.key = None;
    }
}
#[derive(Clone, Copy)]
struct Batch {
    page: usize,
    pipeline: usize,
    layer: u32,
}
struct Command {
    batch: Batch,
    range: std::ops::Range<u32>,
}

fn visible_commands(ids: &[u32], batches: &[Batch]) -> Vec<Command> {
    let mut commands: Vec<Command> = Vec::new();
    for (index, &id) in ids.iter().enumerate() {
        let batch = batches[id as usize];
        if let Some(last) = commands.last_mut()
            && last.batch.page == batch.page
            && last.batch.pipeline == batch.pipeline
        {
            last.range.end += 1;
        } else {
            commands.push(Command {
                batch,
                range: index as u32..index as u32 + 1,
            });
        }
    }
    commands
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    position: [f32; 2],
    size: [f32; 2],
    atlas: [u32; 2],
    base: i32,
    layer: u32,
    tint: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct View {
    origin: [f32; 2],
    extent: [f32; 2],
    flags: [u32; 4],
}
#[derive(Clone)]
struct Page {
    color: wgpu::Texture,
    original: wgpu::Texture,
    depth: wgpu::Texture,
}
pub(crate) struct WorldGpu {
    placements: Vec<(usize, [u32; 2])>,
    surfaces: Vec<std::sync::Arc<ra2_map_scene::visual::Surface>>,
    cpu_instances: Vec<Instance>,
    pub uploaded_bytes: usize,
    pub incremental: bool,
    pipelines: std::sync::Arc<[wgpu::RenderPipeline; 3]>,
    pages: Vec<Page>,
    instances: wgpu::Buffer,
    batches: Vec<Batch>,
    visibility: VisibilityIndex,
}
impl WorldGpu {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &VisualScene,
        pipelines: std::sync::Arc<[wgpu::RenderPipeline; 3]>,
    ) -> Result<Self> {
        let limit = device.limits().max_texture_dimension_2d.min(8192);
        let width = 2048.min(limit);
        let mut placements = Vec::new();
        let (mut x, mut y, mut row, mut page) = (0, 0, 0, 0usize);
        let mut heights = Vec::new();
        for s in &scene.surfaces {
            let [w, h] = s.size;
            ensure!(w <= width && h <= limit, "对象图像超过 GPU 图集尺寸");
            if x + w > width {
                x = 0;
                y += row;
                row = 0;
            }
            if y + h > limit {
                heights.push((y + row).max(1));
                page += 1;
                x = 0;
                y = 0;
                row = 0;
            }
            ensure!(page < 32, "静态场景图集超过 32 页预算");
            placements.push((page, [x, y]));
            x += w;
            row = row.max(h);
        }
        heights.push((y + row).max(1));
        let mut pages = Vec::new();
        for (p, &height) in heights.iter().enumerate() {
            let mut rgba = vec![0; (width * height * 4) as usize];
            let mut original = rgba.clone();
            let mut depth = vec![0u16; (width * height) as usize];
            for (s, &(page, pos)) in scene.surfaces.iter().zip(&placements) {
                if page != p {
                    continue;
                }
                let w = s.size[0] as usize;
                for y in 0..s.size[1] as usize {
                    let dst = (pos[1] as usize + y) * width as usize + pos[0] as usize;
                    let src = y * w;
                    rgba[dst * 4..(dst + w) * 4].copy_from_slice(&s.rgba[src * 4..(src + w) * 4]);
                    let raw = if s.original.is_empty() {
                        &s.rgba
                    } else {
                        &s.original
                    };
                    original[dst * 4..(dst + w) * 4].copy_from_slice(&raw[src * 4..(src + w) * 4]);
                    depth[dst..dst + w].copy_from_slice(&s.depth[src..src + w]);
                }
            }
            let texture = |format, bytes: &[u8], bpp| {
                let tex = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Static scene atlas"),
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
                });
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
                tex
            };
            pages.push(Page {
                color: texture(wgpu::TextureFormat::Rgba8Unorm, &rgba, 4),
                original: texture(wgpu::TextureFormat::Rgba8Unorm, &original, 4),
                depth: texture(
                    wgpu::TextureFormat::R16Uint,
                    bytemuck::cast_slice(&depth),
                    2,
                ),
            });
        }
        let mut instances = Vec::new();
        let mut batches = Vec::new();
        for d in &scene.draws {
            let (page, atlas) = placements[d.surface];
            let s = &scene.surfaces[d.surface];
            instances.push(Instance {
                position: d.position,
                size: s.size.map(|n| n as f32),
                atlas,
                base: d.base,
                layer: d.layer,
                tint: [d.tint[0], d.tint[1], d.tint[2], 0.],
            });
            let pipe = if d.layer == 0 {
                0
            } else if d.writes {
                1
            } else {
                2
            };
            batches.push(Batch {
                page,
                pipeline: pipe,
                layer: d.layer,
            });
        }
        let visibility = VisibilityIndex::new(
            instances
                .iter()
                .map(|i| egui::Rect::from_min_size(i.position.into(), i.size.into())),
        );
        let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Static scene instances"),
            contents: if instances.is_empty() {
                &[0; 48]
            } else {
                bytemuck::cast_slice(&instances)
            },
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        });
        Ok(Self {
            uploaded_bytes: scene.draws.len() * 48,
            incremental: false,
            cpu_instances: scene
                .draws
                .iter()
                .map(|d| {
                    let (_, atlas) = placements[d.surface];
                    Instance {
                        position: d.position,
                        size: scene.surfaces[d.surface].size.map(|v| v as f32),
                        atlas,
                        base: d.base,
                        layer: d.layer,
                        tint: [d.tint[0], d.tint[1], d.tint[2], 0.],
                    }
                })
                .collect(),
            placements,
            surfaces: scene.surfaces.clone(),
            pipelines,
            pages,
            instances,
            batches,
            visibility,
        })
    }
    /// Share atlas textures, but give each published scene its own instances.
    /// Unchanged chunks are copied on the GPU; only dirty chunks are uploaded.
    pub fn updated(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &VisualScene,
    ) -> Option<Self> {
        if scene.surfaces.len() != self.surfaces.len()
            || !scene
                .surfaces
                .iter()
                .zip(&self.surfaces)
                .all(|(a, b)| std::sync::Arc::ptr_eq(a, b))
        {
            return None;
        }
        let mut next = Vec::with_capacity(scene.draws.len());
        let mut batches = Vec::with_capacity(scene.draws.len());
        for d in &scene.draws {
            let (page, atlas) = self.placements[d.surface];
            next.push(Instance {
                position: d.position,
                size: scene.surfaces[d.surface].size.map(|v| v as f32),
                atlas,
                base: d.base,
                layer: d.layer,
                tint: [d.tint[0], d.tint[1], d.tint[2], 0.],
            });
            batches.push(Batch {
                page,
                pipeline: if d.layer == 0 {
                    0
                } else if d.writes {
                    1
                } else {
                    2
                },
                layer: d.layer,
            });
        }
        let stride = std::mem::size_of::<Instance>();
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Replacement scene instances"),
            size: (next.len().max(1) * stride) as u64,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut uploaded_bytes = 0;
        for (chunk, data) in next.chunks(256).enumerate() {
            let start = chunk * 256;
            let offset = (start * stride) as u64;
            let bytes = bytemuck::cast_slice::<_, u8>(data);
            if self
                .cpu_instances
                .get(start..start + data.len())
                .is_some_and(|old| bytemuck::cast_slice::<_, u8>(old) == bytes)
            {
                encoder.copy_buffer_to_buffer(
                    &self.instances,
                    offset,
                    &instances,
                    offset,
                    bytes.len() as u64,
                );
            } else {
                queue.write_buffer(&instances, offset, bytes);
                uploaded_bytes += bytes.len();
            }
        }
        // Copies and writes touch disjoint chunks, so both can be submitted
        // together without overwriting a dirty upload or the previous scene.
        queue.submit([encoder.finish()]);
        Some(Self {
            visibility: VisibilityIndex::new(
                next.iter()
                    .map(|i| egui::Rect::from_min_size(i.position.into(), i.size.into())),
            ),
            cpu_instances: next,
            uploaded_bytes,
            incremental: true,
            pipelines: self.pipelines.clone(),
            pages: self.pages.clone(),
            placements: self.placements.clone(),
            surfaces: self.surfaces.clone(),
            instances,
            batches,
        })
    }
    pub fn visible_buffer(&self, device: &wgpu::Device) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Viewport visible instance IDs"),
            size: self.batches.len().max(1) as u64 * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }
    pub fn bindings(
        &self,
        device: &wgpu::Device,
        uniform: &wgpu::Buffer,
        visible: &wgpu::Buffer,
    ) -> Vec<wgpu::BindGroup> {
        self.pages
            .iter()
            .map(|p| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.pipelines[0].get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: uniform.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(
                                &p.color.create_view(&Default::default()),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(
                                &p.original.create_view(&Default::default()),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(
                                &p.depth.create_view(&Default::default()),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: self.instances.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: visible.as_entire_binding(),
                        },
                    ],
                })
            })
            .collect()
    }
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        target: &super::terrain_gpu::Target,
        rect: egui::Rect,
        options: ViewOptions,
    ) {
        let key = FrameKey {
            rect,
            flags: [
                options.overlays as u32,
                options.objects as u32,
                options.lighting as u32,
                options.remap as u32,
            ],
        };
        // Target resources are bound to one immutable scene. A resize or scene
        // replacement creates a fresh target, invalidating this cache as well.
        let mut cache = target.world_cache.lock().unwrap();
        if cache.key == Some(key) {
            cache.stats.cache_hits += 1;
            return;
        }
        let mut visible = self.visibility.visible(rect);
        visible.retain(|&id| match self.batches[id as usize].layer {
            1 => options.overlays,
            2 => options.objects,
            _ => true,
        });
        let commands = visible_commands(&visible, &self.batches);
        if !visible.is_empty() {
            // Only compact IDs change when panning; art and instance data stay
            // resident. Keep painter order for shadows and non-writing bodies.
            queue.write_buffer(&target.world_visible, 0, bytemuck::cast_slice(&visible));
        }
        queue.write_buffer(
            &target.uniform,
            0,
            bytemuck::bytes_of(&View {
                origin: rect.min.into(),
                extent: rect.size().into(),
                flags: key.flags,
            }),
        );
        let color = target.color.create_view(&Default::default());
        let depth = target.depth.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Static scene shared depth pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 11. / 255.,
                        g: 15. / 255.,
                        b: 20. / 255.,
                        a: 1.,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        for command in &commands {
            pass.set_pipeline(&self.pipelines[command.batch.pipeline]);
            pass.set_bind_group(0, &target.world_bind[command.batch.page], &[]);
            pass.draw(0..6, command.range.clone());
        }
        cache.key = Some(key);
        cache.stats.total_instances = self.batches.len();
        cache.stats.visible_instances = visible.len();
        cache.stats.draw_calls = commands.len();
        cache.stats.scene_renders += 1;
    }
}

pub(crate) fn pipelines(device: &wgpu::Device) -> std::sync::Arc<[wgpu::RenderPipeline; 3]> {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Static scene shared Z"),
        source: wgpu::ShaderSource::Wgsl(include_str!("world.wgsl").into()),
    });
    let pipeline = |write, compare, layout: Option<&wgpu::PipelineLayout>| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Static scene native Z"),
            layout,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(write),
                depth_compare: Some(compare),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            primitive: Default::default(),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
    };
    let buffer = |binding, ty, visibility| wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let tex = |binding, sample_type| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    let group = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Static scene layout"),
        entries: &[
            buffer(
                0,
                wgpu::BufferBindingType::Uniform,
                wgpu::ShaderStages::VERTEX_FRAGMENT,
            ),
            tex(1, wgpu::TextureSampleType::Float { filterable: false }),
            tex(2, wgpu::TextureSampleType::Float { filterable: false }),
            tex(3, wgpu::TextureSampleType::Uint),
            buffer(
                4,
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::ShaderStages::VERTEX,
            ),
            buffer(
                5,
                wgpu::BufferBindingType::Storage { read_only: true },
                wgpu::ShaderStages::VERTEX,
            ),
        ],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&group)],
        immediate_size: 0,
    });
    let terrain = pipeline(true, wgpu::CompareFunction::LessEqual, Some(&layout));
    let building = pipeline(true, wgpu::CompareFunction::Less, Some(&layout));
    let body = pipeline(false, wgpu::CompareFunction::Less, Some(&layout));

    std::sync::Arc::new([terrain, building, body])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visible_batches_keep_order_across_pages_and_depth_modes() {
        let batches = [
            Batch {
                page: 0,
                pipeline: 0,
                layer: 0,
            },
            Batch {
                page: 1,
                pipeline: 0,
                layer: 0,
            },
            Batch {
                page: 0,
                pipeline: 0,
                layer: 0,
            },
            Batch {
                page: 0,
                pipeline: 2,
                layer: 1,
            },
            Batch {
                page: 0,
                pipeline: 1,
                layer: 2,
            },
            Batch {
                page: 1,
                pipeline: 2,
                layer: 2,
            },
            Batch {
                page: 0,
                pipeline: 2,
                layer: 2,
            },
        ];
        // An invisible page between two visible ground tiles must not prevent
        // batching, while shadows, depth writers and bodies must stay ordered.
        let commands = visible_commands(&[0, 2, 3, 4, 5, 6], &batches);
        let actual: Vec<_> = commands
            .iter()
            .map(|c| (c.batch.page, c.batch.pipeline, c.range.clone()))
            .collect();
        assert_eq!(
            actual,
            vec![
                (0, 0, 0..2),
                (0, 2, 2..3),
                (0, 1, 3..4),
                (1, 2, 4..5),
                (0, 2, 5..6)
            ]
        );
        assert!(visible_commands(&[], &batches).is_empty());
    }
}
