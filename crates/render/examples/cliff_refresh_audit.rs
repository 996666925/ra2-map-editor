//! Compare cliff refresh latency and pixels with the complete reload path.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{document::MapDocument, lighting::Profile, new_map::NewMapOptions};
use ra2_map_render::terrain_gpu::TerrainGpu;
use ra2_map_scene::{cliff_authoring, cliff_tools, map::load_scene_bytes, work::update_records};
use std::{path::PathBuf, sync::Arc, time::Instant};

fn main() -> Result<()> {
    struct Wake(std::thread::Thread);
    impl std::task::Wake for Wake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(Arc::new(Wake(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(run());
    loop {
        match future.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(result) => return result,
            std::task::Poll::Pending => std::thread::park(),
        }
    }
}

async fn run() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory required")?);
    let output = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "target/cliff-refresh-audit".into()),
    );
    std::fs::create_dir_all(&output)?;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = instance.request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    let mut install = Install::open(&directory)?;
    let mut pipelines = None;
    let mut reports = Vec::new();
    for side in [80, 256] {
        let bytes = NewMapOptions {
            width: side,
            height: side,
            ..Default::default()
        }
        .bytes()?;
        let initial = load_scene_bytes(
            &mut install,
            "refresh.map",
            "audit".into(),
            bytes,
            Instant::now(),
        )?;
        let mut doc = MapDocument::new(
            initial
                .source
                .edit_base
                .as_ref()
                .map_err(|e| anyhow::anyhow!(e.clone()))?
                .clone(),
        );
        let started = Instant::now();
        let plan = cliff_authoring::reshape(
            &doc,
            &initial,
            cliff_tools::rectangle([side - 5, side - 5], [side + 5, side + 5]),
            0,
            false,
        )?;
        let plan_ms = started.elapsed().as_secs_f64() * 1000.;
        ensure!(!plan.cliffs.is_empty(), "missing cliff stamps");
        // Objects on the plateau exercise elevation and lighting invalidation.
        doc.add_record("Units", [side, side], "Neutral", "MTNK")?;
        doc.commit("object fixture");
        let initial = load_scene_bytes(
            &mut install,
            "refresh.map",
            "audit".into(),
            doc.snapshot().bytes()?,
            Instant::now(),
        )?;
        let gpu = TerrainGpu::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            &initial,
            &mut pipelines,
        )?;
        let before = doc.snapshot();
        plan.apply(&mut doc)?;
        doc.commit("plateau");
        let after = doc.snapshot();
        let mut scene = initial.clone();
        let mut previous = before;
        let mut current_gpu = gpu;
        for step in 0..3 {
            if step == 1 {
                ensure!(doc.undo(), "undo");
            }
            if step == 2 {
                ensure!(doc.redo(), "redo");
            }
            let next = doc.snapshot();
            let started = Instant::now();
            let updated =
                update_records(&scene, &previous, &next, &install, Profile::Normal, &|| {
                    false
                })?
                .context("incremental scene")?;
            let scene_ms = started.elapsed().as_secs_f64() * 1000.;
            let started = Instant::now();
            let replacement = TerrainGpu::new_cached(
                &device,
                &queue,
                wgpu::TextureFormat::Rgba8Unorm,
                &updated,
                &mut pipelines,
                Some(&current_gpu),
            )?;
            queue.submit([]);
            ensure!(
                replacement.update_stats()["incremental"] == true,
                "GPU atlas was rebuilt"
            );
            let gpu_ms = started.elapsed().as_secs_f64() * 1000.;
            let started = Instant::now();
            let full = load_scene_bytes(
                &mut install,
                "refresh.map",
                "full".into(),
                next.bytes()?,
                Instant::now(),
            )?;
            let full_scene_ms = started.elapsed().as_secs_f64() * 1000.;
            let started = Instant::now();
            let _full_gpu = TerrainGpu::new(
                &device,
                &queue,
                wgpu::TextureFormat::Rgba8Unorm,
                &full,
                &mut pipelines,
            )?;
            queue.submit([]);
            let full_gpu_ms = started.elapsed().as_secs_f64() * 1000.;
            let rect =
                egui::Rect::from_center_size(full.bounds.center().floor(), egui::vec2(800., 600.));
            for options in [
                Default::default(),
                ra2_map_scene::visual::ViewOptions {
                    framework: true,
                    ..Default::default()
                },
            ] {
                ensure!(
                    updated.visual.raster(rect, [800, 600], options)?
                        == full.visual.raster(rect, [800, 600], options)?,
                    "CPU pixels changed"
                );
            }
            let pixels = if side == 80 {
                std::fs::create_dir_all(output.join(format!("{side}-{step}")))?;
                Some(replacement.verify_scene(
                    &device,
                    &queue,
                    &updated,
                    &output.join(format!("{side}-{step}")),
                )?)
            } else {
                None
            };
            // The old GPU must remain usable after appending atlas materials.
            if side == 80 && step == 0 {
                std::fs::create_dir_all(output.join("previous-scene"))?;
                current_gpu.verify_scene(
                    &device,
                    &queue,
                    &scene,
                    &output.join("previous-scene"),
                )?;
            }
            let report = serde_json::json!({"side":side,"step":step,"plan_ms":plan_ms,"scene_ms":scene_ms,"gpu_ms":gpu_ms,"total_ms":scene_ms+gpu_ms,"full_scene_ms":full_scene_ms,"full_gpu_ms":full_gpu_ms,"full_total_ms":full_scene_ms+full_gpu_ms,"gpu_update":replacement.update_stats(),"cpu_pixels_equal":true,"gpu_pixels":pixels});
            println!(
                "{side}x{side} step {step}: cached {:.1} ms, full {:.1} ms",
                scene_ms + gpu_ms,
                full_scene_ms + full_gpu_ms
            );
            reports.push(report);
            previous = next;
            scene = updated;
            current_gpu = replacement;
        }
        ensure!(after.bytes()? == doc.snapshot().bytes()?, "redo bytes");
    }
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"passed":true,"gpu":adapter.get_info().name,"samples":reports}),
        )?,
    )?;
    Ok(())
}
