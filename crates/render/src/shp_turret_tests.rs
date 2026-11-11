//! Actual GPU depth, painter order and viewport culling for independent turrets.
use super::*;
use ra2_map_document::{
    document::{MapDocument, Target as DocumentTarget, Value},
    new_map::NewMapOptions,
    source::SourceMap,
};
use ra2_map_scene::visual::ViewOptions;

#[test]
#[ignore = "requires a GPU adapter; uses synthetic assets only"]
fn shp_turret_depth_and_y_sort_match_gpu_picking_and_export() {
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
    let output =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/shp-turret-depth-gpu");
    std::fs::create_dir_all(&output).unwrap();
    let options = ViewOptions {
        lighting: false,
        remap: false,
        ..Default::default()
    };
    let mut checks = Vec::new();
    let mut history = Vec::new();
    for level in [0, 4, 36] {
        for (name, neighbor, y_sort, z_adjust, expected) in [
            ("before-body", None, -1, -30, [60, 80, 100, 255]),
            ("after-body", None, 1, -30, [200, 0, 0, 255]),
            (
                "before-unit",
                Some(("Units", "TANK")),
                255,
                -30,
                [0, 200, 0, 255],
            ),
            (
                "after-unit",
                Some(("Units", "TANK")),
                257,
                -30,
                [200, 0, 0, 255],
            ),
            (
                "behind-building",
                Some(("Structures", "BLOCK")),
                512,
                30,
                [0, 0, 200, 255],
            ),
            (
                "front-building",
                Some(("Structures", "BLOCK")),
                512,
                -30,
                [200, 0, 0, 255],
            ),
            ("material25", None, 512, -30, [0; 4]),
            ("material50", None, 512, -30, [0; 4]),
            ("material75", None, 512, -30, [0; 4]),
            ("stacked50", None, 512, -30, [0; 4]),
            ("shadow", None, 512, -30, [200, 0, 0, 255]),
            ("start", None, 512, -30, [0, 200, 0, 255]),
            ("voxel-front", None, 512, -60, [0; 4]),
            ("voxel-back", None, 512, 60, [0; 4]),
        ] {
            let fixture = if name.starts_with("voxel") {
                ra2_map_assets::test_support::Fixture::with_voxel_turret()
            } else {
                ra2_map_assets::test_support::Fixture::with_shp_turret()
            };
            let material = match name {
                "material25" => Some((25, 2)),
                "material50" | "stacked50" => Some((50, 4)),
                "material75" => Some((75, 6)),
                _ => None,
            };
            let expected = if level == 36 && name == "before-unit" {
                // Engine high-terrain compensation applies to buildings and
                // slot anims; the Foot-class unit stays two rows below.
                [200, 0, 0, 255]
            } else if let Some((value, selector)) = material {
                let path = fixture.0.join("artmd.ini");
                std::fs::write(&path,std::fs::read_to_string(&path).unwrap().replace("LoopEnd=32",
                    &format!("LoopEnd=32\nTranslucency={value}\nShouldUseCellDrawer=no\nUseNormalLight=yes"))).unwrap();
                let mut palette = vec![0; 768];
                palette[9..12].copy_from_slice(&[0, 0, 50]);
                std::fs::write(fixture.0.join("anim.pal"), palette).unwrap();
                let mut rgb = ra2_map_voxel::packed_blend([60, 80, 100], [0, 0, 200], selector);
                if name == "stacked50" {
                    rgb = ra2_map_voxel::packed_blend(rgb, [0, 0, 200], selector);
                }
                [rgb[0], rgb[1], rgb[2], 255]
            } else {
                expected
            };
            if name == "start" {
                let path = fixture.0.join("artmd.ini");
                std::fs::write(
                    &path,
                    std::fs::read_to_string(&path)
                        .unwrap()
                        .replace("LoopEnd=32", "LoopEnd=32\nStart=32"),
                )
                .unwrap();
                let path = fixture.0.join("guntur.shp");
                let mut bytes = std::fs::read(&path).unwrap();
                for frame in 32..64 {
                    bytes[8 + 64 * 24 + frame * 2] = 4;
                }
                std::fs::write(path, bytes).unwrap();
            }
            if name == "shadow" {
                let path = fixture.0.join("artmd.ini");
                std::fs::write(
                    &path,
                    std::fs::read_to_string(&path)
                        .unwrap()
                        .replace("LoopEnd=32", "LoopEnd=32\nShadow=yes"),
                )
                .unwrap();
                let path = fixture.0.join("guntur.shp");
                let mut bytes = std::fs::read(&path).unwrap();
                for frame in 32..64 {
                    bytes[8 + 24 * frame..10 + 24 * frame].copy_from_slice(&2u16.to_le_bytes());
                    bytes[8 + 64 * 24 + frame * 2] = 3;
                }
                std::fs::write(path, bytes).unwrap();
            }
            let mut install = fixture.install();
            install.catalog =
                ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
            let (source, _) = SourceMap::parse(
                "turret.map".into(),
                "test".into(),
                NewMapOptions {
                    width: if level > 20 { 40 } else { 20 },
                    height: if level > 20 { 40 } else { 20 },
                    level,
                    ..Default::default()
                }
                .bytes()
                .unwrap(),
            )
            .unwrap();
            let mut doc = MapDocument::new(source.edit_base.as_ref().unwrap().clone());
            let at = if level > 20 { [40, 40] } else { [20, 20] };
            doc.add_record("Structures", at, "Neutral", "GUN").unwrap();
            if name == "stacked50" {
                doc.add_record("Structures", at, "Neutral", "GUN").unwrap();
            }
            if let Some((section, kind)) = neighbor {
                doc.add_record(section, at, "Neutral", kind).unwrap();
            }
            for (key, value) in [("TurretAnimYSort", y_sort), ("TurretAnimZAdjust", z_adjust)] {
                doc.set_ini("GUN", key, Some(value.to_string().into_bytes()))
                    .unwrap();
            }
            doc.commit("turret settings");
            let scene = ra2_map_scene::map::load_scene_bytes(
                &mut install,
                "turret.map",
                "test".into(),
                doc.snapshot().bytes().unwrap(),
                std::time::Instant::now(),
            )
            .unwrap();
            let y = if level > 20 { 1215. } else { 615. } - f32::from(level) * 15.;
            if !name.starts_with("voxel") {
                assert_eq!(
                    scene.visual.sample(egui::pos2(-0.5, y + 0.5), options).0,
                    expected,
                    "{name}, level={level}"
                );
            }
            let gpu = TerrainGpu::new(
                &device,
                &queue,
                wgpu::TextureFormat::Rgba8Unorm,
                &scene,
                &mut None,
            )
            .unwrap();
            let size = [64, 64];
            let target = gpu.target(&device, size);
            for zoom in [0.75, 1., 2.25] {
                let rect = egui::Rect::from_center_size(
                    egui::pos2(0.137, y + 0.371),
                    egui::vec2(64., 64.) / zoom,
                );
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
                image::save_buffer(
                    output.join(format!("{name}-h{level}-z{zoom}.png")),
                    &actual,
                    size[0],
                    size[1],
                    image::ColorType::Rgba8,
                )
                .unwrap();
                assert!(
                    error <= 2,
                    "{name}, level={level}, zoom={zoom}, error={error}"
                );
                checks.push(serde_json::json!({"case":name,"level":level,"zoom":zoom,"max_rgba_error":error}));
            }
            if name == "after-unit" && level == 4 {
                let mut doc = MapDocument::new(scene.source.edit_base.as_ref().unwrap().clone());
                let gun_id = doc
                    .records()
                    .into_iter()
                    .find(|(_, record)| record.type_id() == "GUN")
                    .unwrap()
                    .0;
                let mut current = scene;
                let mut current_gpu = gpu;
                for step in 0..3 {
                    let before = doc.snapshot();
                    match step {
                        0 => {
                            let moved = doc.records()[&gun_id].moved([21, 20]).unwrap();
                            doc.set(DocumentTarget::Record(gun_id), Some(Value::Record(moved)))
                                .unwrap();
                            doc.commit("move turret building");
                        }
                        1 => {
                            assert!(doc.undo());
                        }
                        _ => {
                            assert!(doc.redo());
                        }
                    }
                    let updated = ra2_map_scene::work::update_records(
                        &current,
                        &before,
                        &doc.snapshot(),
                        &install,
                        ra2_map_document::lighting::Profile::Normal,
                        &|| false,
                    )
                    .unwrap()
                    .unwrap();
                    let next_gpu = current_gpu
                        .updated_records(&device, &queue, &updated)
                        .expect("moving a turret building retains the body/turret atlas surfaces");
                    let path = output.join(format!("history-{step}"));
                    std::fs::create_dir_all(&path).unwrap();
                    let report = next_gpu
                        .verify_scene(&device, &queue, &updated, &path)
                        .unwrap();
                    history.push(serde_json::json!({"step":step,"report":report}));
                    current = updated;
                    current_gpu = next_gpu;
                }
            }
        }
    }
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"passed":true,"checks":checks,"history":history}),
        )
        .unwrap(),
    )
    .unwrap();
}
