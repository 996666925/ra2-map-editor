//! Real-resource maximum-map culling audit without opening an editor window.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::new_map::NewMapOptions;
use ra2_map_render::terrain_gpu::TerrainGpu;
use ra2_map_scene::map::load_scene_bytes;
use std::{path::PathBuf, sync::Arc, time::Instant};

fn main() -> Result<()> {
    // Adapter/device requests are the only asynchronous work in this CLI.
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
            .unwrap_or_else(|| "target/viewport-audit".into()),
    );
    std::fs::create_dir_all(&output)?;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = instance.request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    let mut install = Install::open(&directory)?;
    let mut pipelines = None;
    let mut reports = Vec::new();
    for side in [128, 256] {
        let bytes = NewMapOptions {
            width: side,
            height: side,
            level: 14,
            ..Default::default()
        }
        .bytes()?;
        std::fs::write(output.join(format!("{side}x{side}.map")), &bytes)?;
        let scene = load_scene_bytes(
            &mut install,
            "viewport.map",
            "audit".into(),
            bytes,
            Instant::now(),
        )?;
        let gpu = TerrainGpu::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            &scene,
            &mut pipelines,
        )?;
        let report = gpu.verify_viewport(&device, &queue, &scene)?;
        let native = &report["checks"][0]["stats"];
        let total = native["total_instances"]
            .as_u64()
            .context("total instances")?;
        let visible = native["visible_instances"]
            .as_u64()
            .context("visible instances")?;
        ensure!(
            visible > 0 && visible * 20 < total,
            "large-map viewport did not cull enough instances"
        );
        println!(
            "{side}x{side}: {visible}/{total} instances at 1280x800, {} draw calls",
            native["draw_calls"]
        );
        reports.push(serde_json::json!({"side":side,"viewport":report}));
    }
    let report = serde_json::json!({"passed":true,"gpu":adapter.get_info().name,"maps":reports});
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(())
}
