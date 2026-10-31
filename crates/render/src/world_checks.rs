use super::*;

impl Target {
    /// Read a region of the viewport that was actually drawn, including its
    /// existing scene bindings. Creating a fresh target would hide stale bindings.
    pub fn read_region(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        origin: [u32; 2],
        size: [u32; 2],
    ) -> Result<Vec<u8>> {
        use anyhow::Context;
        ensure!(
            size.into_iter().all(|n| n > 0)
                && (0..2).all(|i| origin[i]
                    .checked_add(size[i])
                    .is_some_and(|n| n <= self.size[i])),
            "视口读回区域越界"
        );
        let row = (size[0] * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Live viewport regression readback"),
            size: u64::from(row) * u64::from(size[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                origin: wgpu::Origin3d {
                    x: origin[0],
                    y: origin[1],
                    z: 0,
                },
                ..self.color.as_image_copy()
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(size[1]),
                },
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
        let submission = queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(std::time::Duration::from_secs(15)),
        })?;
        rx.recv_timeout(std::time::Duration::from_secs(15))
            .context("视口 GPU 读回超时")??;
        let pixels = {
            let mapped = buffer.slice(..).get_mapped_range()?;
            mapped
                .chunks_exact(row as usize)
                .flat_map(|r| &r[..size[0] as usize * 4])
                .copied()
                .collect()
        };
        buffer.unmap();
        Ok(pixels)
    }
}

impl TerrainGpu {
    /// Compare actual shared-depth GPU pixels with the export compositor.
    pub fn verify_scene(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &MapScene,
        output: &std::path::Path,
    ) -> Result<serde_json::Value> {
        use anyhow::Context;
        use ra2_map_scene::visual::ViewOptions;
        let mut points: Vec<_> = scene
            .visual
            .draws
            .iter()
            .filter(|d| d.owner.is_some())
            .take(8)
            .map(|d| {
                let s = &scene.visual.surfaces[d.surface];
                egui::pos2(
                    d.position[0] + s.size[0] as f32 / 2.,
                    d.position[1] + s.size[1] as f32 / 2.,
                )
            })
            .collect();
        points.extend(
            scene
                .cells
                .iter()
                .filter(|c| scene.image(c).is_some_and(|i| i.height > 30))
                .take(2)
                .map(|c| egui::pos2(c.screen_x + 30., c.screen_y)),
        );
        if points.is_empty() {
            points.push(scene.bounds.center());
        }
        let mut checks = Vec::new();
        // Reuse a target across camera and layer changes to also exercise cache
        // invalidation, not just a fresh texture for every probe.
        let target = self.target(device, [128, 128]);
        for (i, point) in points.iter().enumerate() {
            for (mode, options) in [
                ("normal", ViewOptions::default()),
                (
                    "unlit",
                    ViewOptions {
                        lighting: false,
                        remap: false,
                        ..Default::default()
                    },
                ),
                (
                    "terrain",
                    ViewOptions {
                        objects: false,
                        overlays: false,
                        ..Default::default()
                    },
                ),
            ] {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(point.x.floor() - 64., point.y.floor() - 64.),
                    egui::vec2(128., 128.),
                );
                let mut encoder = device.create_command_encoder(&Default::default());
                self.world
                    .render(&mut encoder, queue, &target, rect, options);
                let readback = device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: 128 * 512,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                encoder.copy_texture_to_buffer(
                    target.color.as_image_copy(),
                    wgpu::TexelCopyBufferInfo {
                        buffer: &readback,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(512),
                            rows_per_image: Some(128),
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
                    .context("场景 GPU 读回超时")??;
                let bytes = readback.slice(..).get_mapped_range()?;
                let expected = scene.visual.raster(rect, [128, 128], options)?;
                let error = bytes
                    .iter()
                    .zip(&expected)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap_or(0);
                image::save_buffer(
                    output.join(format!("scene-{i}-{mode}.png")),
                    &bytes,
                    128,
                    128,
                    image::ColorType::Rgba8,
                )?;
                ensure!(
                    error <= 2,
                    "共享场景 GPU/PNG 不一致：probe={i},mode={mode},error={error}"
                );
                let before = target.viewport_stats();
                let mut cached = device.create_command_encoder(&Default::default());
                self.world
                    .render(&mut cached, queue, &target, rect, options);
                queue.submit([cached.finish()]);
                let after = target.viewport_stats();
                ensure!(
                    after.scene_renders == before.scene_renders
                        && after.cache_hits == before.cache_hits + 1,
                    "视角未变化时未复用画面"
                );
                checks.push(serde_json::json!({"probe":i,"mode":mode,"max_rgba_error":error,"viewport":after}));
            }
        }
        Ok(
            serde_json::json!({"passed":true,"checks":checks,"surfaces":scene.visual.surfaces.len(),"draws":scene.visual.draws.len(),"viewport":self.verify_viewport(device, queue, scene)?}),
        )
    }

    /// Measure real submitted instances at several zoom levels, and validate
    /// native-scale pixels after panning to/from a completely empty viewport.
    pub fn verify_viewport(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &MapScene,
    ) -> Result<serde_json::Value> {
        use ra2_map_scene::visual::ViewOptions;
        let target = self.target(device, [1280, 800]);
        let mut checks = Vec::new();
        for (name, center, zoom) in [
            ("native", scene.bounds.center(), 1.),
            ("zoom_out", scene.bounds.center(), 0.5),
            ("zoom_in", scene.bounds.center(), 2.),
            ("empty", scene.bounds.max + egui::vec2(10000., 10000.), 1.),
            ("return", scene.bounds.center(), 1.),
        ] {
            // Align native-scale samples to texel centres, as in verify_scene.
            let rect = egui::Rect::from_center_size(center.floor(), egui::vec2(1280., 800.) / zoom);
            let mut encoder = device.create_command_encoder(&Default::default());
            self.world
                .render(&mut encoder, queue, &target, rect, ViewOptions::default());
            queue.submit([encoder.finish()]);
            let actual = target.read_region(device, queue, [0, 0], [1280, 800])?;
            if zoom == 1. {
                let expected = scene
                    .visual
                    .raster(rect, [1280, 800], ViewOptions::default())?;
                let error = actual
                    .iter()
                    .zip(expected)
                    .map(|(a, b)| a.abs_diff(b))
                    .max()
                    .unwrap_or(0);
                ensure!(
                    error <= 2,
                    "视口裁剪 GPU/CPU 不一致：{name}, error={error}, rect={rect:?}"
                );
            }
            let stats = target.viewport_stats();
            if name == "empty" {
                ensure!(
                    stats.visible_instances == 0 && stats.draw_calls == 0,
                    "空视口仍提交地图实例"
                );
            }
            checks.push(serde_json::json!({"view":name,"zoom":zoom,"stats":stats}));
        }
        Ok(serde_json::json!({"passed":true,"checks":checks}))
    }
}
