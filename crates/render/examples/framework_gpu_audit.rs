//! Verify framework GPU materials against the CPU compositor on an authored map.
use anyhow::{Context, Result};
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
    let map = PathBuf::from(args.next().context("authored map required")?);
    let output = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "target/framework-gpu-audit".into()),
    );
    std::fs::create_dir_all(&output)?;
    let mut install = ra2_map_assets::assets::Install::open(&directory)?;
    let scene = ra2_map_scene::map::load_scene_bytes(
        &mut install,
        "framework.map",
        "GPU audit".into(),
        std::fs::read(map)?,
        Instant::now(),
    )?;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = instance.request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    let gpu = ra2_map_render::terrain_gpu::TerrainGpu::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        &scene,
        &mut None,
    )?;
    let report = gpu.verify_scene(&device, &queue, &scene, &output)?;
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("Framework CPU/GPU comparison passed: {}", output.display());
    Ok(())
}
