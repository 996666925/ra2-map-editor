//! Exercise the actual shader at fractional zoom and pan, including atlas/alpha edges.
use super::*;
use ra2_map_scene::visual::ViewOptions;

fn gpu_device() -> (wgpu::Device, wgpu::Queue) {
    struct Wake(std::thread::Thread);
    impl std::task::Wake for Wake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let waker = std::task::Waker::from(Arc::new(Wake(std::thread::current())));
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(async {
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        adapter.request_device(&Default::default()).await.unwrap()
    });
    loop {
        match std::future::Future::poll(future.as_mut(), &mut context) {
            std::task::Poll::Ready(result) => break result,
            std::task::Poll::Pending => std::thread::park(),
        }
    }
}

#[test]
#[ignore = "requires a GPU adapter; uses synthetic assets only"]
fn framework_digit_magnification_matches_cpu_coverage() {
    let (device, queue) = gpu_device();
    let fixture = ra2_map_assets::test_support::Fixture::new();
    // Full-size clear diamond so the real generated 36 label is present.
    let mut tmp: Vec<u8> = [1u32, 1, 60, 30, 20]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    tmp.resize(20 + 52, 0);
    tmp.resize(20 + 52 + 900, 1);
    std::fs::write(fixture.0.join("clear01.tem"), tmp).unwrap();
    let mut install = fixture.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    let scene = ra2_map_scene::map::load_scene_bytes(
        &mut install,
        "framework-digits.map",
        "test".into(),
        ra2_map_document::new_map::NewMapOptions {
            width: 40,
            height: 40,
            level: 36,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
        std::time::Instant::now(),
    )
    .unwrap();
    assert!(
        scene.visual.surfaces.iter().any(|surface| {
            surface.size == [60, 30]
                && surface
                    .framework
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p == &[35, 28, 47, 255])
        }),
        "fixture must include the generated height digits"
    );
    let gpu = TerrainGpu::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        &scene,
        &mut None,
    )
    .unwrap();
    let size = [512, 256];
    let target = gpu.target(&device, size);
    let cell = scene
        .cells
        .iter()
        .find(|c| c.rx == 40 && c.ry == 40)
        .unwrap();
    let center = egui::pos2(cell.screen_x + 30., cell.screen_y + 15.);
    let output =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/framework-digits");
    std::fs::create_dir_all(&output).unwrap();
    // The ordinary view remains nearest sampled; framework also retains native/reduced sampling.
    for framework in [false, true] {
        for zoom in [0.75, 1., 1.25, 2.45, 3.1, 5.] {
            for phase in [0.137, 0.371] {
                let rect = egui::Rect::from_center_size(
                    center + egui::vec2(phase, phase),
                    egui::vec2(size[0] as f32, size[1] as f32) / zoom,
                );
                let options = ViewOptions {
                    framework,
                    ..Default::default()
                };
                let mut encoder = device.create_command_encoder(&Default::default());
                gpu.world
                    .render(&mut encoder, &queue, &target, rect, options);
                queue.submit([encoder.finish()]);
                let actual = target.read_region(&device, &queue, [0, 0], size).unwrap();
                let expected = scene.visual.raster(rect, size, options).unwrap();
                let error = actual
                    .iter()
                    .zip(&expected)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap();
                if error > 2 {
                    image::save_buffer(
                        output.join("actual.png"),
                        &actual,
                        size[0],
                        size[1],
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                    image::save_buffer(
                        output.join("expected.png"),
                        &expected,
                        size[0],
                        size[1],
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                    let pixel = actual
                        .iter()
                        .zip(&expected)
                        .position(|(a, b)| a.abs_diff(*b) == error)
                        .unwrap()
                        / 4;
                    eprintln!(
                        "max error at {}, {}: {:?} vs {:?}",
                        pixel % size[0] as usize,
                        pixel / size[0] as usize,
                        &actual[pixel * 4..pixel * 4 + 4],
                        &expected[pixel * 4..pixel * 4 + 4]
                    );
                }
                assert!(
                    error <= 2,
                    "framework={framework}, zoom={zoom}, phase={phase}, error={error}"
                );
                if framework && zoom == 2.45 && phase == 0.371 {
                    image::save_buffer(
                        output.join("after.png"),
                        &actual,
                        size[0],
                        size[1],
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                    let mut nearest = (*scene.visual).clone();
                    for surface in &mut nearest.surfaces {
                        let surface = Arc::make_mut(surface);
                        if !surface.framework.is_empty() {
                            surface.rgba.clone_from(&surface.framework);
                        }
                    }
                    let before = nearest
                        .raster(
                            rect,
                            size,
                            ViewOptions {
                                lighting: false,
                                ..Default::default()
                            },
                        )
                        .unwrap();
                    image::save_buffer(
                        output.join("before.png"),
                        &before,
                        size[0],
                        size[1],
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                }
            }
        }
    }
}

#[test]
#[ignore = "requires a GPU adapter; uses synthetic assets only"]
fn cut_replaces_old_cliff_pixels_and_cancel_restores_them() {
    use ra2_map_document::{document::MapDocument, new_map::NewMapOptions};
    use ra2_map_scene::{
        cliff_authoring::{self, CliffCut, CutSide},
        cliff_tools,
        map::load_scene_bytes,
    };
    let (device, queue) = gpu_device();
    let fixture = ra2_map_assets::test_support::Fixture::with_authoring();
    let mut install = fixture.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    let mut load = |bytes| {
        load_scene_bytes(
            &mut install,
            "cut.map",
            "test".into(),
            bytes,
            std::time::Instant::now(),
        )
        .unwrap()
    };
    let flat = load(
        NewMapOptions {
            width: 40,
            height: 40,
            level: 4,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
    );
    let mut doc = MapDocument::new(flat.source.edit_base.as_ref().unwrap().clone());
    let plateau = cliff_tools::rectangle([26, 26], [50, 50]);
    cliff_authoring::reshape(&doc, &flat, plateau.clone(), 4, false)
        .unwrap()
        .apply(&mut doc)
        .unwrap();
    doc.commit("plateau");
    let before = doc.snapshot().bytes().unwrap();
    let scene = load(before.clone());
    let mut pipelines = None;
    let gpu = TerrainGpu::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        &scene,
        &mut pipelines,
    )
    .unwrap();
    let size = [1024, 640];
    let world = egui::Rect::from_center_size(
        egui::pos2(0., 1070.),
        egui::vec2(size[0] as f32, size[1] as f32),
    );
    let target = gpu.target(&device, size);
    let render =
        |gpu: &TerrainGpu, target: &Target, options, preview: Option<&TerrainPreviewGpu>| {
            let mut encoder = device.create_command_encoder(&Default::default());
            gpu.world.render_layer(
                &mut encoder,
                &queue,
                target,
                world,
                options,
                crate::world_gpu::TerrainLayer::Opaque(preview),
            );
            queue.submit([encoder.finish()]);
            target.read_region(&device, &queue, [0, 0], size).unwrap()
        };
    let line = [[38, 20], [38, 49]];
    let seed = CliffCut::first_wall(&doc, &scene, line[0], line[1]).unwrap();
    let cut = CliffCut::select(&doc, &scene, seed).unwrap();
    for plan in [
        cut.plan_line(&doc, &scene, line, CutSide::Left, None)
            .unwrap(),
        cliff_authoring::reshape(&doc, &scene, plateau, 4, true).unwrap(),
    ] {
        let projection = scene
            .visual
            .terrain_preview(&scene.palette, &plan.edits)
            .unwrap();
        assert_eq!(projection.replaced.len(), plan.edits.len());
        let patch = gpu
            .prepare_terrain_preview(&device, &queue, &target, projection)
            .unwrap();
        assert_eq!(doc.snapshot().bytes().unwrap(), before);
        let mut result = doc.fork();
        plan.apply(&mut result).unwrap();
        let committed = load(result.snapshot().bytes().unwrap());
        let revised_gpu = TerrainGpu::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            &committed,
            &mut pipelines,
        )
        .unwrap();
        let revised_target = revised_gpu.target(&device, size);
        for framework in [false, true] {
            let options = ViewOptions {
                framework,
                lighting: false,
                ..Default::default()
            };
            let old = render(&gpu, &target, options, None);
            let preview = render(&gpu, &target, options, Some(&patch));
            let expected = render(&revised_gpu, &revised_target, options, None);
            assert!(
                old != expected,
                "fixture must have visible cliff pixels to remove"
            );
            assert!(
                preview == expected,
                "cut preview differs from rebuilt terrain (framework={framework})"
            );
            let hits = target.viewport_stats().cache_hits;
            assert!(render(&gpu, &target, options, Some(&patch)) == expected);
            assert_eq!(target.viewport_stats().cache_hits, hits + 1);
            assert!(render(&gpu, &target, options, None) == old);
        }
    }
}
