//! P0 textured-quad probe. All resources use the eframe host's Device/Queue.
//! The map's shared static depth, remap and lighting live in world_gpu.

use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use anyhow::{Context, Result, ensure};

use serde::Serialize;
use wgpu::util::DeviceExt;

use ra2_map_assets::assets::DecodedImage;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct View {
    scale: [f32; 2],
    translation: [f32; 2],
    srgb_target: u32,
    padding: [u32; 3],
}

pub struct PreviewRenderer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    format: wgpu::TextureFormat,
}

pub struct GpuImage {
    texture: wgpu::Texture,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pub paints: AtomicUsize,
}

impl PreviewRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("P0 preview"),
            source: wgpu::ShaderSource::Wgsl(include_str!("preview.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("P0 preview bindings"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<View>() as u64),
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("P0 preview pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("P0 preview pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("P0 nearest sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
            format,
        }
    }

    pub fn upload(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &DecodedImage,
    ) -> Result<Arc<GpuImage>> {
        let (width, height) = (image.info.width.max(1), image.info.height.max(1));
        let limit = device.limits().max_texture_dimension_2d;
        ensure!(
            width <= limit && height <= limit,
            "图像 {width}×{height} 超过 GPU 纹理上限 {limit}"
        );
        ensure!(
            image.rgba.len() as u64
                == u64::from(image.info.width) * u64::from(image.info.height) * 4,
            "解码图像长度错误"
        );
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&image.info.name),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let empty = vec![0u8; (width * height * 4) as usize];
        queue.write_texture(
            texture.as_image_copy(),
            if image.rgba.is_empty() {
                &empty
            } else {
                &image.rgba
            },
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            texture.size(),
        );
        let uniform = self.uniform(device, [1.0; 2], [0.0; 2]);
        let bind_group = self.bind_group(device, &texture, &uniform);
        Ok(Arc::new(GpuImage {
            texture,
            uniform,
            bind_group,
            paints: AtomicUsize::new(0),
        }))
    }

    fn uniform(
        &self,
        device: &wgpu::Device,
        scale: [f32; 2],
        translation: [f32; 2],
    ) -> wgpu::Buffer {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("P0 image view"),
            contents: bytemuck::bytes_of(&View {
                scale,
                translation,
                srgb_target: u32::from(self.format.is_srgb()),
                padding: [0; 3],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        })
    }

    fn bind_group(
        &self,
        device: &wgpu::Device,
        texture: &wgpu::Texture,
        uniform: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("P0 image"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &texture.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        })
    }

    pub fn callback(
        self: &Arc<Self>,
        queue: &wgpu::Queue,
        image: &Arc<GpuImage>,
        rect: egui::Rect,
        image_points: egui::Vec2,
        pan: egui::Vec2,
    ) -> egui::PaintCallback {
        let view = View {
            scale: [
                image_points.x / rect.width(),
                image_points.y / rect.height(),
            ],
            translation: [2.0 * pan.x / rect.width(), -2.0 * pan.y / rect.height()],
            srgb_target: u32::from(self.format.is_srgb()),
            padding: [0; 3],
        };
        queue.write_buffer(&image.uniform, 0, bytemuck::bytes_of(&view));
        egui_wgpu::Callback::new_paint_callback(
            rect,
            ImageCallback {
                renderer: self.clone(),
                image: image.clone(),
            },
        )
    }

    /// Render with the very same pipeline/device used by the UI, then read back
    /// at 1:1. This tests upload, sampling, orientation, alpha and row padding.
    pub fn verify_and_save(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        gpu: &GpuImage,
        source: &DecodedImage,
        path: &Path,
    ) -> Result<PixelCheck> {
        ensure!(
            matches!(
                self.format,
                wgpu::TextureFormat::Rgba8Unorm
                    | wgpu::TextureFormat::Rgba8UnormSrgb
                    | wgpu::TextureFormat::Bgra8Unorm
                    | wgpu::TextureFormat::Bgra8UnormSrgb
            ),
            "烟测不支持目标格式 {:?}",
            self.format
        );
        let (width, height) = (source.info.width, source.info.height);
        ensure!(width > 0 && height > 0, "烟测图像为空");
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("P0 smoke target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let uniform = self.uniform(device, [1.0; 2], [0.0; 2]);
        let bindings = self.bind_group(device, &gpu.texture, &uniform);
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let attachments = [Some(wgpu::RenderPassColorAttachment {
                view: &target.create_view(&Default::default()),
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("P0 smoke draw"),
                color_attachments: &attachments,
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bindings, &[]);
            pass.draw(0..6, 0..1);
        }
        let padded_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("P0 smoke readback"),
            size: u64::from(padded_row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(height),
                },
            },
            target.size(),
        );
        let submission = queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(Duration::from_secs(15)),
        })?;
        rx.recv_timeout(Duration::from_secs(15))
            .context("GPU 读回超时")??;
        let mapped = readback.slice(..).get_mapped_range();
        let mut rgba = Vec::with_capacity(source.rgba.len());
        for row in mapped.chunks_exact(padded_row as usize) {
            rgba.extend_from_slice(&row[..width as usize * 4]);
        }
        drop(mapped);
        readback.unmap();
        if matches!(
            self.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            for pixel in rgba.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
        }
        let mut check = PixelCheck {
            opaque_pixels: 0,
            transparent_pixels: 0,
            max_rgb_error: 0,
        };
        for (actual, expected) in rgba
            .as_chunks::<4>()
            .0
            .iter()
            .zip(source.rgba.as_chunks::<4>().0)
        {
            ensure!(
                actual[3] == expected[3],
                "GPU alpha 不一致：{actual:?} / {expected:?}"
            );
            if expected[3] == 0 {
                check.transparent_pixels += 1;
            } else {
                check.opaque_pixels += 1;
                for channel in 0..3 {
                    check.max_rgb_error = check
                        .max_rgb_error
                        .max(actual[channel].abs_diff(expected[channel]));
                }
            }
        }
        ensure!(check.opaque_pixels > 0, "烟测图像没有可见像素");
        ensure!(
            check.max_rgb_error <= 1,
            "GPU 色彩/方向不一致，最大误差 {}",
            check.max_rgb_error
        );
        image::save_buffer(path, &rgba, width, height, image::ColorType::Rgba8)?;
        Ok(check)
    }
}

#[derive(Serialize)]
pub struct PixelCheck {
    pub opaque_pixels: usize,
    pub transparent_pixels: usize,
    pub max_rgb_error: u8,
}

struct ImageCallback {
    renderer: Arc<PreviewRenderer>,
    image: Arc<GpuImage>,
}

impl egui_wgpu::CallbackTrait for ImageCallback {
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        _resources: &egui_wgpu::CallbackResources,
    ) {
        pass.set_pipeline(&self.renderer.pipeline);
        pass.set_bind_group(0, &self.image.bind_group, &[]);
        pass.draw(0..6, 0..1);
        self.image.paints.fetch_add(1, Ordering::Relaxed);
    }
}
