//! Maximum geometry and 20,000 objects: actual CPU/GPU work and cancellation.
use anyhow::{Context, Result, ensure};
use ra2_map_assets::assets::Install;
use ra2_map_document::{
    document::{MapDocument, Target, Value},
    lighting::Profile,
    new_map::NewMapOptions,
};
use ra2_map_render::terrain_gpu::TerrainGpu;
use ra2_map_scene::{map, work};
use std::{path::PathBuf, sync::Arc, time::Instant};
fn compare_scene(actual: &map::MapScene, full: &map::MapScene) -> Result<()> {
    ensure!(
        actual.visual.draws.len() == full.visual.draws.len(),
        "incremental draw count"
    );
    ensure!(
        actual.visual.owners == full.visual.owners,
        "incremental owners"
    );
    let mut checked = std::collections::BTreeSet::new();
    for (a, b) in actual.visual.draws.iter().zip(&full.visual.draws) {
        let mut draw = a.clone();
        draw.surface = b.surface;
        ensure!(draw == *b, "incremental draw command differs");
        if checked.insert((a.surface, b.surface)) {
            let a = &actual.visual.surfaces[a.surface];
            let b = &full.visual.surfaces[b.surface];
            ensure!(
                a.size == b.size
                    && a.rgba == b.rgba
                    && a.original == b.original
                    && a.depth == b.depth,
                "incremental material differs"
            );
        }
    }
    Ok(())
}
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
            std::task::Poll::Ready(r) => return r,
            std::task::Poll::Pending => std::thread::park(),
        }
    }
}
#[cfg(windows)]
fn memory() -> serde_json::Value {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        faults: u32,
        peak: usize,
        working: usize,
        paged_peak: usize,
        paged: usize,
        nonpaged_peak: usize,
        nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
        private: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
    }
    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut Counters,
            size: u32,
        ) -> i32;
    }
    let mut c = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        ..Default::default()
    };
    let ok = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut c,
            std::mem::size_of::<Counters>() as u32,
        )
    };
    if ok == 0 {
        serde_json::Value::Null
    } else {
        serde_json::json!({"working_set":c.working,"peak_working_set":c.peak,"private_bytes":c.private})
    }
}
#[cfg(not(windows))]
fn memory() -> serde_json::Value {
    serde_json::Value::Null
}
async fn run() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let directory = PathBuf::from(args.next().context("game directory required")?);
    let output = PathBuf::from(args.next().unwrap_or_else(|| "target/p5-scale".into()));
    std::fs::create_dir_all(&output)?;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = instance.request_adapter(&Default::default()).await?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    let start = Instant::now();
    let mut install = Install::open(&directory)?;
    let install_ms = start.elapsed().as_secs_f64() * 1000.;
    let mut pipelines = None;
    let mut reports = Vec::new();
    for (width, height, objects) in [(80, 80, 0), (492, 20, 0), (256, 256, 20000)] {
        let mut bytes = NewMapOptions {
            width,
            height,
            ..Default::default()
        }
        .bytes()?;
        if objects > 0 {
            let parsed = vera20k_core::map::map_file::load_from_bytes(bytes.clone())?;
            let cells: Vec<_> = parsed
                .cells
                .iter()
                .filter(|c| c.rx > 8 && c.ry > 8)
                .collect();
            let mut rows = String::from("\n[Units]\n");
            for n in 0..objects {
                let c = cells[n * 5 % cells.len()];
                rows.push_str(&format!(
                    "{n}=Neutral,MTNK,256,{},{},0,Guard,None,0,-1,0,-1,1,0\n",
                    c.rx, c.ry
                ));
            }
            bytes.extend_from_slice(rows.as_bytes());
        }
        let path = output.join(format!("scale-{width}x{height}-{objects}.map"));
        std::fs::write(&path, &bytes)?;
        let start = Instant::now();
        let scene = map::load_scene_bytes(
            &mut install,
            "scale.map",
            "scale".into(),
            bytes,
            Instant::now(),
        )?;
        let open_ms = start.elapsed().as_secs_f64() * 1000.;
        let mut doc = MapDocument::new(
            scene
                .source
                .edit_base
                .as_ref()
                .map_err(|e| anyhow::anyhow!(e.clone()))?
                .clone(),
        );
        let count = doc
            .records()
            .values()
            .filter(|r| r.section == "Units")
            .count();
        ensure!(count == objects, "object count");
        let start = Instant::now();
        let mut gpu = TerrainGpu::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            &scene,
            &mut pipelines,
        )?;
        queue.submit([]);
        let upload_ms = start.elapsed().as_secs_f64() * 1000.;
        let viewport = gpu.verify_viewport(&device, &queue, &scene)?;
        let mut incremental = serde_json::Value::Null;
        if objects > 0 {
            let before = doc.snapshot();
            // One object changes facing; second edit returns to an already cached
            // material so the GPU path can share atlases and copy clean chunks.
            let (id, r) = doc
                .records()
                .into_iter()
                .find(|(_, r)| r.section == "Units")
                .unwrap();
            doc.set(
                Target::Record(id),
                Some(Value::Record(r.patched(&[(5, "64".into())])?)),
            )?;
            doc.commit("rotate");
            let start = Instant::now();
            let changed = work::update_records(
                &scene,
                &before,
                &doc.snapshot(),
                &install,
                Profile::Normal,
                &|| false,
            )?
            .context("incremental scene rejected")?;
            let scene_ms = start.elapsed().as_secs_f64() * 1000.;
            let full = map::load_scene_bytes(
                &mut install,
                "scale.map",
                "full reference".into(),
                doc.snapshot().bytes()?,
                Instant::now(),
            )?;
            compare_scene(&changed, &full)?;
            let probe_center = changed
                .visual
                .draws
                .iter()
                .find(|d| {
                    d.owner
                        .and_then(|id| changed.visual.owners.get(&id))
                        .is_some_and(|(section, key)| section == &r.section && key == &r.key)
                })
                .context("edited object not drawn")?
                .position;
            let probe = egui::Rect::from_center_size(
                egui::Pos2::from(probe_center).floor(),
                egui::vec2(256., 256.),
            );
            ensure!(
                changed
                    .visual
                    .raster(probe, [256, 256], Default::default())?
                    == full.visual.raster(probe, [256, 256], Default::default())?,
                "incremental CPU mismatch"
            );
            drop(gpu);
            gpu = TerrainGpu::new(
                &device,
                &queue,
                wgpu::TextureFormat::Rgba8Unorm,
                &changed,
                &mut pipelines,
            )?;
            let before = doc.snapshot();
            doc.undo();
            let start = Instant::now();
            let restored = work::update_records(
                &changed,
                &before,
                &doc.snapshot(),
                &install,
                Profile::Normal,
                &|| false,
            )?
            .unwrap();
            let restore_ms = start.elapsed().as_secs_f64() * 1000.;
            compare_scene(&restored, &scene)?;
            let start = Instant::now();
            let replacement = gpu
                .updated_records(&device, &queue, &restored)
                .context("GPU incremental rejected warm materials")?;
            queue.submit([]);
            let gpu_ms = start.elapsed().as_secs_f64() * 1000.;
            // The published renderer must remain valid even after its successor
            // has been uploaded, including when it needs to redraw after a pan.
            let previous_viewport = gpu.verify_viewport(&device, &queue, &changed)?;
            gpu = replacement;
            ensure!(
                gpu.update_stats()["uploaded_instance_bytes"]
                    .as_u64()
                    .unwrap()
                    < restored.visual.draws.len() as u64 * 48,
                "full buffer uploaded"
            );
            let restored_viewport = gpu.verify_viewport(&device, &queue, &restored)?;
            let undo_gpu_stats = gpu.update_stats();
            // Reordering, removal and insertion must agree with an independent
            // native rebuild too, including shadows and remapped source IDs.
            let dirty_chunks = restored.visual.dirty_chunks;
            let reused_surfaces = restored.visual.reused_surfaces;
            let mut current = restored;
            let mut topology_checks = Vec::new();
            for operation in ["move", "delete", "add", "repeat"] {
                let before = doc.snapshot();
                match operation {
                    "move" => {
                        doc.set(
                            Target::Record(id),
                            Some(Value::Record(
                                r.patched(&[(3, "256".into()), (4, "256".into())])?,
                            )),
                        )?;
                    }
                    "delete" => {
                        doc.set(Target::Record(id), None)?;
                    }
                    _ => {
                        doc.add_record("Structures", [256, 256], "Neutral", "GAPOWR")?;
                    }
                }
                doc.commit(operation);
                let updated = work::update_records(
                    &current,
                    &before,
                    &doc.snapshot(),
                    &install,
                    Profile::Normal,
                    &|| false,
                )?
                .context("incremental topology rejected")?;
                let full = map::load_scene_bytes(
                    &mut install,
                    "scale.map",
                    "reference".into(),
                    doc.snapshot().bytes()?,
                    Instant::now(),
                )?;
                compare_scene(&updated, &full)?;
                ensure!(
                    updated
                        .visual
                        .raster(probe, [256, 256], Default::default())?
                        == full.visual.raster(probe, [256, 256], Default::default())?,
                    "incremental topology raster"
                );
                let replacement =
                    if let Some(updated_gpu) = gpu.updated_records(&device, &queue, &updated) {
                        updated_gpu
                    } else {
                        TerrainGpu::new(
                            &device,
                            &queue,
                            wgpu::TextureFormat::Rgba8Unorm,
                            &updated,
                            &mut pipelines,
                        )?
                    };
                let viewport = replacement.verify_viewport(&device, &queue, &updated)?;
                topology_checks.push(serde_json::json!({"operation":operation,"gpu":replacement.update_stats(),"viewport":viewport}));
                gpu = replacement;
                current = updated;
            }
            incremental = serde_json::json!({"record_edit_scene_ms":scene_ms,"undo_scene_ms":restore_ms,"gpu_ms":gpu_ms,"gpu":undo_gpu_stats,"dirty_chunks":dirty_chunks,"reused_surfaces":reused_surfaces,"cpu_matches_full_rebuild":true,"all_draws_and_materials_match":true,"topology_checks":topology_checks,"previous_viewport_after_update":previous_viewport,"viewport_after_undo":restored_viewport});
        }
        let checks = std::cell::Cell::new(0);
        let start = Instant::now();
        let cancelled =
            scene
                .visual
                .raster_controlled(scene.bounds, [2048, 2048], Default::default(), &|| {
                    checks.set(checks.get() + 1);
                    checks.get() > 3
                });
        let cancel_ms = start.elapsed().as_secs_f64() * 1000.;
        ensure!(
            cancelled.is_err() && checks.get() > 3,
            "mid-raster cancellation"
        );
        reports.push(serde_json::json!({"size":[width,height],"map":path,"objects":count,"terrain_cells":scene.cells.len(),"draws":scene.visual.draws.len(),"open_ms":open_ms,"gpu_prepare_ms":upload_ms,"memory":memory(),"viewport":viewport,"incremental":incremental,"cancellation":{"mid_raster":true,"checkpoints":checks.get(),"response_ms":cancel_ms}}));
    }
    let report = serde_json::json!({"passed":true,"gpu":adapter.get_info().name,"resources_ms":install_ms,"profile":install.revision,"build":"dev opt-level=1","maps":reports,"scope":"record-only incremental material/index/GPU chunks; terrain and global rules use native full resolution"});
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", output.join("report.json").display());
    Ok(())
}
