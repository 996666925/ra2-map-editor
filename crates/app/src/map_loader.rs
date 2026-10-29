//! Application worker: compose resource decode, scene construction and GPU preparation.
use anyhow::{Context, Result};
use eframe::{egui, egui_wgpu};
use ra2_map_assets::assets::Install;
use ra2_map_render::terrain_gpu::{TerrainGpu, TerrainPipelines};
use ra2_map_scene::map::{MapScene, load_scene_bytes_controlled, load_scene_controlled};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Instant,
};

pub struct MapResult {
    pub generation: u64,
    pub result: Result<MapScene>,
    pub gpu: Option<Result<Arc<TerrainGpu>>>,
    pub timings: LoadTimings,
    pub preview: Option<(u64, u64)>,
    pub created: bool,
}
#[derive(Default, Clone, Serialize)]
pub struct LoadTimings {
    pub resources_ms: f64,
    pub decode_ms: f64,
    pub gpu_prepare_ms: f64,
    pub total_ms: f64,
    pub pipelines_reused: bool,
    pub incremental_scene: bool,
    pub incremental_gpu: bool,
    pub uploaded_instance_bytes: usize,
}
pub struct LoadProgress {
    pub generation: u64,
    pub stage: &'static str,
}
pub struct MapLoader {
    refresh: Arc<std::sync::atomic::AtomicBool>,
    requests: mpsc::Sender<MapRequest>,
    pub results: mpsc::Receiver<MapResult>,
    pub progress: mpsc::Receiver<LoadProgress>,
    latest: Arc<AtomicU64>,
}
struct MapRequest {
    generation: u64,
    directory: PathBuf,
    name: String,
    snapshot: Option<crate::document::Snapshot>,
    new_map: Option<crate::new_map::NewMapOptions>,
    profile: ra2_map_document::lighting::Profile,
}
impl MapLoader {
    #[cfg(test)]
    pub fn new(ctx: egui::Context) -> Result<Self> {
        Self::start(ctx, None)
    }
    pub fn with_gpu(ctx: egui::Context, gpu: egui_wgpu::RenderState) -> Result<Self> {
        Self::start(ctx, Some(gpu))
    }
    fn start(ctx: egui::Context, gpu: Option<egui_wgpu::RenderState>) -> Result<Self> {
        let (requests, rx) = mpsc::channel::<MapRequest>();
        let (tx, results) = mpsc::channel();
        let (progress_tx, progress) = mpsc::channel();
        let latest = Arc::new(AtomicU64::new(0));
        let current = latest.clone();
        let refresh = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let refresh_worker = refresh.clone();
        std::thread::Builder::new()
            .name("map-loader".into())
            .spawn(move || {
                let mut cached: Option<(PathBuf, Install)> = None;
                let mut pipelines: Option<Arc<TerrainPipelines>> = None;
                let mut previous: Option<(MapScene,crate::document::Snapshot,ra2_map_document::lighting::Profile)> = None;
                let mut previous_gpu: Option<Arc<TerrainGpu>> = None;
                while let Ok(mut request) = rx.recv() {
                    for newer in rx.try_iter() {
                        request = newer;
                    }
                    let MapRequest {generation, directory, name, snapshot, new_map,profile} = request;
                    let created = new_map.is_some();
                    let saved_snapshot = snapshot.clone();
                    let cancelled=||current.load(Ordering::Acquire)!=generation;
                    let preview=snapshot.as_ref().map(|snapshot|(snapshot.document_id,snapshot.state_id));
                    if current.load(Ordering::Acquire) != generation {continue;}
                    let started = Instant::now();
                    let mut timings = LoadTimings::default();
                    let stage = |stage| {
                        let _ = progress_tx.send(LoadProgress {generation,stage});
                        ctx.request_repaint();
                    };
                    stage("读取游戏资源…");
                    let result = (|| {
                        if refresh_worker.swap(false, Ordering::AcqRel) { cached = None; previous=None; previous_gpu=None; }
                        if cached.as_ref().is_none_or(|(path, _)| path != &directory) {
                            cached = None; previous=None; previous_gpu=None;
                            cached = Some((directory.clone(), Install::open_controlled(&directory,&cancelled)?));
                        }
                        timings.resources_ms = started.elapsed().as_secs_f64()*1000.;
                        stage("解析地图与地形图像…");
                        let decoding = Instant::now();
                        let install=&mut cached.as_mut().unwrap().1;
                        let scene = if let Some(options) = new_map {
                            stage("生成空白地图…");
                            load_scene_bytes_controlled(install, &name, "新建地图".into(), options.bytes()?, Instant::now(),profile,&cancelled)?
                        } else if let Some(snapshot)=snapshot {
                            let incremental = if let Some((old,before,old_profile))=&previous {
                                if *old_profile==profile {ra2_map_scene::work::update_records(old,before,&snapshot,install,profile,&cancelled)?} else {None}
                            } else {None};
                            if let Some(scene)=incremental {timings.incremental_scene=true;scene} else {
                                ra2_map_scene::map::load_scene_bytes_controlled(install,&name,"编辑快照".into(),snapshot.bytes()?,Instant::now(),profile,&cancelled)?
                            }
                        }else{load_scene_controlled(install, &name,&cancelled)?};
                        timings.decode_ms = decoding.elapsed().as_secs_f64()*1000.;
                        Ok(scene)
                    })();
                    // A newer request cancels publication and expensive GPU work
                    // at this seam; an in-progress core decode is allowed to finish.
                    if current.load(Ordering::Acquire) != generation {continue;}
                    let prepared = result.as_ref().ok().filter(|scene|!scene.images.is_empty()).and_then(|scene| {
                        let state = gpu.as_ref()?;
                        stage("准备地形图集与 GPU 管线…");
                        let preparing = Instant::now();
                        timings.pipelines_reused = pipelines.is_some();
                        let updated = previous_gpu.as_ref().filter(|_|timings.incremental_scene).and_then(|gpu|gpu.updated_records(&state.device,&state.queue,scene));
                        let prepared = if let Some(gpu)=updated {
                            timings.incremental_gpu=true; Ok(gpu)
                        } else { TerrainGpu::new(&state.device,&state.queue,state.target_format,scene,&mut pipelines) };
                        if let Ok(gpu)=&prepared {timings.uploaded_instance_bytes=gpu.update_stats()["uploaded_instance_bytes"].as_u64().unwrap_or(0) as usize;}
                        // Flush staging on this worker instead of deferring all
                        // allocations/copies to the first UI render submission.
                        state.queue.submit([]);
                        timings.gpu_prepare_ms = preparing.elapsed().as_secs_f64()*1000.;
                        Some(prepared)
                    });
                    timings.total_ms = started.elapsed().as_secs_f64()*1000.;
                    if current.load(Ordering::Acquire) != generation {continue;}
                    log::info!("Map {name}: resources {:.1} ms, decode {:.1} ms, GPU preparation {:.1} ms (worker, pipelines reused={})",timings.resources_ms,timings.decode_ms,timings.gpu_prepare_ms,timings.pipelines_reused);
                    previous_gpu=prepared.as_ref().and_then(|r|r.as_ref().ok()).cloned();
                    if let Ok(scene)=&result {
                        let snapshot=saved_snapshot.or_else(||scene.source.edit_base.as_ref().ok().map(|base|crate::document::MapDocument::new(base.clone()).snapshot()));
                        previous=snapshot.map(|snapshot|(scene.clone(),snapshot,profile));
                    }
                    if tx.send(MapResult { generation, result, gpu: prepared, timings, preview, created }).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })?;
        Ok(Self {
            refresh,
            requests,
            results,
            progress,
            latest,
        })
    }
    pub fn invalidate_resources(&self) {
        self.refresh.store(true, Ordering::Release);
    }
    pub fn cancel(&self, generation: u64) {
        self.latest.store(generation, Ordering::Release);
    }
    pub fn request(&self, generation: u64, directory: PathBuf, name: String) -> Result<()> {
        self.latest.store(generation, Ordering::Release);
        self.requests
            .send(MapRequest {
                generation,
                directory,
                name,
                snapshot: None,
                new_map: None,
                profile: Default::default(),
            })
            .context("地图线程已退出")
    }
    pub fn preview_profile(
        &self,
        generation: u64,
        directory: PathBuf,
        name: String,
        snapshot: crate::document::Snapshot,
        profile: ra2_map_document::lighting::Profile,
    ) -> Result<()> {
        self.latest.store(generation, Ordering::Release);
        self.requests
            .send(MapRequest {
                generation,
                directory,
                name,
                snapshot: Some(snapshot),
                new_map: None,
                profile,
            })
            .context("地图线程已退出")
    }
    pub fn create(
        &self,
        generation: u64,
        directory: PathBuf,
        options: crate::new_map::NewMapOptions,
    ) -> Result<()> {
        options.validate()?;
        self.latest.store(generation, Ordering::Release);
        self.requests
            .send(MapRequest {
                generation,
                directory,
                name: "Untitled.map".into(),
                snapshot: None,
                new_map: Some(options),
                profile: Default::default(),
            })
            .context("地图线程已退出")
    }
}

#[cfg(test)]
#[path = "map_loader_tests.rs"]
mod tests;
