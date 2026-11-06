//! Exercise the actual shader at fractional zoom and pan, including atlas/alpha edges.
use super::*;
use ra2_map_scene::visual::ViewOptions;

#[test]
#[ignore = "requires a GPU adapter; uses synthetic assets only"]
fn framework_digit_magnification_matches_cpu_coverage() {
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
    let (device, queue) = loop {
        match std::future::Future::poll(future.as_mut(), &mut context) {
            std::task::Poll::Ready(result) => break result,
            std::task::Poll::Pending => std::thread::park(),
        }
    };
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
