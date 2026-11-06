//! Static editor images are read back once on the asset worker, then shared by
//! the palette, picking and world atlas. Reuse the engine shader, including its
//! admission rules; UseBuffer and unsupported models retain the CPU renderer.
use crate::{
    assets::{hva_file::HvaFile, vpl_file::VplFile, vxl_file::VxlFile},
    atlas_growth::{InitialShelfPolicy, SPRITE_PADDING, plan_initial_shelves},
    voxel_gpu::{PagePass, VoxelGpu},
    vxl_raster::{self, VoxelImage, VxlRenderParams, VxlSprite},
};

static DEVICE: std::sync::OnceLock<Option<(wgpu::Device, wgpu::Queue)>> =
    std::sync::OnceLock::new();

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    // A process-owned device also avoids destroying Windows GPU backends in TLS teardown.
    DEVICE
        .get_or_init(|| {
            pollster::block_on(async {
                let instance =
                    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
                let adapter = instance.request_adapter(&Default::default()).await.ok()?;
                match adapter.request_device(&Default::default()).await {
                    Ok(gpu) => Some(gpu),
                    Err(error) => {
                        log::warn!("Voxel GPU unavailable: {error}");
                        None
                    }
                }
            })
        })
        .clone()
}

/// One VPL per renderer, matching the engine GPU raster's lighting texture lifetime.
pub struct Renderer<'a> {
    vpl: Option<&'a VplFile>,
    gpu_draws: usize,
    gpu_submissions: usize,
    // Unrequested, unavailable/disabled, or ready. CPU-only work never opens a device.
    gpu: Option<Option<(wgpu::Device, wgpu::Queue)>>,
    raster: VoxelGpu,
}
impl<'a> Renderer<'a> {
    pub fn new(vpl: Option<&'a VplFile>) -> Self {
        Self {
            vpl,
            gpu_draws: 0,
            gpu_submissions: 0,
            gpu: None,
            raster: VoxelGpu::default(),
        }
    }
    pub fn gpu_draws(&self) -> usize {
        self.gpu_draws
    }
    pub fn gpu_submissions(&self) -> usize {
        self.gpu_submissions
    }

    pub fn render(
        &mut self,
        model: &VxlFile,
        hva: Option<&HvaFile>,
        params: &VxlRenderParams,
        composite_depth: bool,
    ) -> VxlSprite {
        self.render_batch(model, hva, std::slice::from_ref(params), composite_depth)
            .pop()
            .expect("one requested pose")
    }

    /// Bake up to 32 poses together on each GPU page and read that page once.
    /// The upstream GPU image carries no geometric Z. A composite requests the
    /// CPU result once, avoiding GPU work followed by a second full CPU raster.
    pub fn render_batch(
        &mut self,
        model: &VxlFile,
        hva: Option<&HvaFile>,
        params: &[VxlRenderParams],
        composite_depth: bool,
    ) -> Vec<VxlSprite> {
        if composite_depth
            || params.iter().all(|p| p.use_buffer)
            || self.gpu.as_ref().is_some_and(Option::is_none)
        {
            return params
                .iter()
                .map(|p| vxl_raster::render_vxl(model, hva, p, self.vpl))
                .collect();
        }
        let mut result = Vec::with_capacity(params.len());
        for chunk in params.chunks(32) {
            let images: Vec<_> = chunk
                .iter()
                .map(|p| vxl_raster::voxel_image(model, hva, p, self.vpl))
                .collect();
            let pixels = self
                .rasterize(&images)
                .unwrap_or_else(|| (0..images.len()).map(|_| None).collect());
            result.extend(images.into_iter().zip(pixels).map(|(image, pixels)| {
                if let Some(palette_indices) = pixels {
                    let ([width, height], [offset_x, offset_y]) = image.frame();
                    VxlSprite {
                        width,
                        height,
                        offset_x,
                        offset_y,
                        palette_indices,
                        depth: vec![0.; (width * height) as usize],
                    }
                } else {
                    // VoxelImage::Cpu already owns a rasterized fallback. Consuming
                    // it also covers UseBuffer/legacy poses in a mixed batch.
                    image
                        .render_cpu(self.vpl)
                        .expect("paint or CPU voxel image")
                }
            }));
        }
        result
    }

    fn rasterize(&mut self, images: &[VoxelImage<'_>]) -> Option<Vec<Option<Vec<u8>>>> {
        let admitted: Vec<_> = images
            .iter()
            .enumerate()
            .filter_map(|(index, image)| {
                self.raster
                    .admit(image, self.vpl)
                    .map(|gpu_image| (index, gpu_image))
            })
            .collect();
        if admitted.is_empty() {
            return None;
        }
        let (device, queue) = self.gpu.get_or_insert_with(device).as_ref()?;
        let sizes: Vec<_> = admitted
            .iter()
            .map(|(index, _)| images[*index].frame().0)
            .collect();
        let limit = device.limits().max_texture_dimension_2d;
        let plan = plan_initial_shelves(
            &sizes,
            InitialShelfPolicy {
                padding: SPRITE_PADDING,
                max_dim: limit,
                split_pages: false,
            },
        );
        let width = plan.width;
        let height = plan.single_page_height();
        if height > limit || plan.placements.len() != admitted.len() {
            return None;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Editor voxel pose page"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Uint,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let stride =
            width.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Editor voxel page readback"),
            size: u64::from(stride) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let view = texture.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        self.raster.encode(
            device,
            queue,
            &mut encoder,
            [width, height],
            &[PagePass {
                target: &view,
                images: plan
                    .placements
                    .iter()
                    .map(|p| (p.origin, &admitted[p.index].1))
                    .collect(),
            }],
            self.vpl,
        );
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(height),
                },
            },
            texture.size(),
        );
        let submission = queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = send.send(result);
            });
        let polled = device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(std::time::Duration::from_secs(30)),
        });
        if polled.is_err()
            || !matches!(
                receive.recv_timeout(std::time::Duration::from_secs(30)),
                Ok(Ok(()))
            )
        {
            log::warn!("Voxel GPU readback failed; using CPU renderer");
            self.gpu = Some(None);
            return None;
        }
        let mapped = buffer.slice(..).get_mapped_range().ok()?;
        let mut pixels: Vec<_> = (0..images.len()).map(|_| None).collect();
        for placement in &plan.placements {
            let (index, _) = &admitted[placement.index];
            let [w, h] = sizes[placement.index];
            let [x, y] = placement.origin;
            pixels[*index] = Some(
                (y..y + h)
                    .flat_map(|row| {
                        let start = (row * stride + x) as usize;
                        mapped[start..start + w as usize].iter().copied()
                    })
                    .collect(),
            );
        }
        drop(mapped);
        buffer.unmap();
        self.gpu_draws += admitted.len();
        self.gpu_submissions += 1;
        Some(pixels)
    }
}
