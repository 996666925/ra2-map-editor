//! Exercise the normal MapView canvas, including its actual scene/GPU loader.
use super::*;
use ra2_map_document::{
    document::{Cell, Target, Value},
    new_map::NewMapOptions,
    source::SourceMap,
};

fn doc(width: u16, height: u16, level: u8, theater: &str) -> MapDocument {
    let (source, _) = SourceMap::parse(
        "test.yrm".into(),
        "test".into(),
        NewMapOptions {
            width,
            height,
            level,
            theater: theater.into(),
            ..Default::default()
        }
        .bytes()
        .unwrap(),
    )
    .unwrap();
    MapDocument::new(source.edit_base.unwrap())
}

fn setup(
    state: Option<&egui_wgpu::RenderState>,
) -> (
    MapView,
    egui::Context,
    ra2_map_assets::test_support::Fixture,
    PathBuf,
) {
    let (mut view, root) = super::super::layout_tests::view();
    let fixture = ra2_map_assets::test_support::Fixture::with_shp_turret();
    let mut palette = std::fs::read(fixture.0.join("isotem.pal")).unwrap();
    palette[3..6].copy_from_slice(&[30, 45, 20]);
    std::fs::write(fixture.0.join("isotem.pal"), palette).unwrap();
    for archive in [
        "ra2md.mix",
        "ra2.mix",
        "cachemd.mix",
        "cache.mix",
        "localmd.mix",
        "local.mix",
        "conqmd.mix",
        "conquer.mix",
        "cameomd.mix",
        "cameo.mix",
        "mapsmd03.mix",
        "multimd.mix",
        "movmd03.mix",
    ] {
        std::fs::write(fixture.0.join(archive), [0u8; 10]).unwrap();
    }
    // Native-sized, opaque TMP diamonds make terrain picking and raster checks
    // meaningful at the same camera scale as a real map.
    let mut tmp: Vec<u8> = [1u32, 1, 60, 30, 20]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    tmp.resize(72 + 900, 1);
    tmp[20..72].fill(0);
    std::fs::write(fixture.0.join("clear01.tem"), tmp).unwrap();
    let ctx = egui::Context::default();
    theme::install(&ctx);
    ctx.add_font_provider(Arc::new(
        egui_system_font_provider::SystemFontProvider::new(),
    ));
    view.loader = match state {
        Some(state) => MapLoader::with_gpu(ctx.clone(), state.clone()).unwrap(),
        None => MapLoader::new(ctx.clone()).unwrap(),
    };
    let mut original = doc(40, 40, 0, "TEMPERATE");
    original
        .add_record("Units", [40, 40], "Neutral", "TANK")
        .unwrap();
    original
        .set(Target::Overlay([40, 40]), Some(Value::Overlay(255, 7)))
        .unwrap();
    original.commit("original");
    let mut install = fixture.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    let scene = ra2_map_scene::map::load_scene_bytes(
        &mut install,
        "base.yrm",
        "test".into(),
        original.snapshot().bytes().unwrap(),
        Instant::now(),
    )
    .unwrap();
    view.editor.attach(&scene);
    view.scene = Some(scene);
    view.name = "base.yrm".into();
    view.directory = fixture.0.clone();
    view.editor.directory = fixture.0.clone();
    view.center = egui::pos2(0., 1200.);
    view.zoom = 0.75;
    view.fit = false;
    view.open_map_management(&ctx, false);
    let mut imported = doc(20, 20, 4, "SNOW");
    imported
        .add_record("Structures", [20, 20], "Neutral", "BLOCK")
        .unwrap();
    imported.commit("imported");
    view.workbench
        .management
        .import_document("imported.yrm".into(), &imported)
        .unwrap();
    (view, ctx, fixture, root)
}

fn frame(
    view: &mut MapView,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    state: Option<&egui_wgpu::RenderState>,
) -> egui::FullOutput {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280., 800.),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            view.poll(ui.ctx());
            view.canvas_inner(ui, state);
        },
    );
    if let Some(state) = state {
        let mut renderer = state.renderer.write();
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                renderer.update_texture(&state.device, &state.queue, *id, delta);
            }
        }
        for id in &output.textures_delta.free {
            renderer.free_texture(id);
        }
    }
    output.textures_delta.clear();
    output
}

fn settle(view: &mut MapView, ctx: &egui::Context, state: Option<&egui_wgpu::RenderState>) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let _ = frame(view, ctx, vec![], state);
        let m = &view.workbench.management;
        if !view.loading
            && !view.editor.refresh
            && m.worker.is_none()
            && !m.layer_pending
            && (!m.open || m.layers.len() == m.workspace.as_ref().unwrap().pieces.len())
            && (state.is_none() || m.layers.iter().all(|layer| layer.gpu.is_some()))
            && (!m.dirty || !m.apply_requested)
            && m.ready.is_none()
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "preview stalled: loading={}, dirty={}, message={}, error={:?}",
            view.loading,
            m.dirty,
            m.message,
            view.error
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(view.error.is_none(), "{:?}", view.error);
}

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: Default::default(),
    }
}

fn point(view: &MapView, at: Cell) -> egui::Pos2 {
    let m = &view.workbench.management;
    let piece = &m.workspace.as_ref().unwrap().pieces[m.selected];
    if let Some(layer) = m
        .layers
        .iter()
        .find(|l| Arc::ptr_eq(&l.source, &piece.clip))
    {
        let source = [
            (i32::from(at[0]) - piece.offset[0]) as u16,
            (i32::from(at[1]) - piece.offset[1]) as u16,
        ];
        let c = layer
            .scene
            .cells
            .iter()
            .find(|c| [c.rx, c.ry] == source)
            .unwrap();
        let world = egui::pos2(c.screen_x + 30., c.screen_y + 15.) + translation(piece.offset);
        return view.workbench.canvas_rect.unwrap().center() + (world - view.center) * view.zoom;
    }
    let scene = view.scene.as_ref().unwrap();
    let c = scene.cells.iter().find(|c| [c.rx, c.ry] == at).unwrap();
    let world = egui::pos2(c.screen_x + 30., c.screen_y + 15.)
        - translation(view.workbench.management.published_shift);
    view.workbench.canvas_rect.unwrap().center() + (world - view.center) * view.zoom
}

fn preview_doc(view: &MapView) -> MapDocument {
    let composition = view
        .workbench
        .management
        .workspace
        .as_ref()
        .unwrap()
        .compose(&Default::default())
        .unwrap();
    let (source, _) =
        SourceMap::parse("expected.yrm".into(), "test".into(), composition.bytes).unwrap();
    MapDocument::new(source.edit_base.unwrap())
}

#[test]
fn main_canvas_drags_full_import_and_reveals_original_before_release_without_writing_live_map() {
    let (mut view, ctx, _fixture, root) = setup(None);
    let original = view
        .editor
        .doc
        .as_ref()
        .unwrap()
        .snapshot()
        .bytes()
        .unwrap();
    let center = view.center;
    settle(&mut view, &ctx, None);
    assert!(view.workbench.management.open);
    assert_eq!(preview_doc(&view).terrain([40, 40]).unwrap().level, 4);
    assert!(
        !preview_doc(&view)
            .record_iter()
            .any(|(_, r)| r.section == "Units")
    );
    assert!(view.scene.as_ref().unwrap().visual.draws.len() > 1000);
    assert!(
        ctx.memory(|m| m.area_rect(egui::Id::unique("map-management")))
            .is_none(),
        "dragging must not create a separate window"
    );
    let start = point(&view, [40, 40]);
    let _ = frame(
        &mut view,
        &ctx,
        vec![egui::Event::PointerMoved(start), button(start, true)],
        None,
    );
    assert!(view.workbench.management.drag.is_some());
    let end = start + egui::vec2(630., 315.) * view.zoom;
    let _ = frame(&mut view, &ctx, vec![egui::Event::PointerMoved(end)], None);
    assert_eq!(
        view.workbench.management.workspace.as_ref().unwrap().pieces[1].offset,
        [41, 20]
    );
    settle(&mut view, &ctx, None);
    assert!(
        view.workbench.management.drag.is_some(),
        "full scene must update while the pointer is still held"
    );
    let shift = view
        .workbench
        .management
        .workspace
        .as_ref()
        .unwrap()
        .compose(&Default::default())
        .unwrap()
        .shift;
    let original_at = [(40 + shift[0]) as u16, (40 + shift[1]) as u16];
    let moved_at = [(61 + shift[0]) as u16, (40 + shift[1]) as u16];
    let preview = preview_doc(&view);
    assert_eq!(preview.terrain(original_at).unwrap().level, 0);
    assert_eq!(preview.terrain(moved_at).unwrap().level, 4);
    assert!(
        preview
            .record_iter()
            .any(|(_, r)| r.section == "Units" && r.cell() == Some(original_at))
    );
    assert!(
        preview
            .record_iter()
            .any(|(_, r)| r.section == "Structures" && r.cell() == Some(moved_at))
    );
    assert_eq!(
        view.center, center,
        "expansion must not move the camera during a drag"
    );
    assert_eq!(
        view.editor
            .doc
            .as_ref()
            .unwrap()
            .snapshot()
            .bytes()
            .unwrap(),
        original
    );
    let release = end + egui::vec2(30., 15.) * view.zoom;
    let _ = frame(
        &mut view,
        &ctx,
        vec![egui::Event::PointerMoved(release), button(release, false)],
        None,
    );
    settle(&mut view, &ctx, None);
    assert_eq!(
        view.workbench.management.workspace.as_ref().unwrap().pieces[1].offset,
        [42, 20],
        "release movement must be included"
    );
    assert!(view.workbench.management.drag.is_none());
    drop(view);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn main_canvas_cancel_focus_loss_and_zoom_preserve_the_layer_underlay() {
    let (mut view, ctx, _fixture, root) = setup(None);
    settle(&mut view, &ctx, None);
    for zoom in [0.35, 1.25] {
        view.zoom = zoom;
        let _ = frame(
            &mut view,
            &ctx,
            vec![egui::Event::WindowFocused(true)],
            None,
        );
        let start = point(&view, [40, 40]);
        let _ = frame(
            &mut view,
            &ctx,
            vec![egui::Event::PointerMoved(start), button(start, true)],
            None,
        );
        let end = start + egui::vec2(120., 0.) * zoom;
        let _ = frame(&mut view, &ctx, vec![egui::Event::PointerMoved(end)], None);
        assert_eq!(
            view.workbench.management.workspace.as_ref().unwrap().pieces[1].offset,
            [22, 18]
        );
        let event = if zoom < 1. {
            egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }
        } else {
            egui::Event::WindowFocused(false)
        };
        let _ = frame(&mut view, &ctx, vec![event], None);
        let _ = frame(&mut view, &ctx, vec![button(end, false)], None);
        settle(&mut view, &ctx, None);
        assert_eq!(
            view.workbench.management.workspace.as_ref().unwrap().pieces[1].offset,
            [20, 20]
        );
        assert!(view.workbench.management.open);
        assert_eq!(preview_doc(&view).terrain([40, 40]).unwrap().level, 4);
    }
    drop(view);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn continuous_map_drag_reuses_large_layer_art_without_composition_or_scene_loading() {
    let (mut view, ctx, _fixture, root) = setup(None);
    let large = doc(160, 160, 4, "TEMPERATE");
    view.workbench
        .management
        .import_document("large.yrm".into(), &large)
        .unwrap();
    settle(&mut view, &ctx, None);
    let layers = view.workbench.management.layers.clone();
    let scene = view.scene.as_ref().unwrap().visual.clone();
    let generation = view.generation;
    let jobs = view.workbench.management.composition_jobs;
    let start = point(&view, [40, 40]);
    let _ = frame(
        &mut view,
        &ctx,
        vec![egui::Event::PointerMoved(start), button(start, true)],
        None,
    );
    assert!(view.workbench.management.dragging());
    let mut frames = Vec::new();
    for n in 0..120 {
        let pointer = start + egui::vec2((n % 24) as f32 * 15., (n % 12) as f32 * 7.5);
        let started = Instant::now();
        let _ = frame(
            &mut view,
            &ctx,
            vec![egui::Event::PointerMoved(pointer)],
            None,
        );
        frames.push(started.elapsed().as_secs_f64() * 1000.);
        let m = &view.workbench.management;
        assert_eq!(
            m.composition_jobs, jobs,
            "moving must not schedule whole-map compression"
        );
        assert_eq!(
            view.generation, generation,
            "moving must not reload the map scene"
        );
        assert!(Arc::ptr_eq(&view.scene.as_ref().unwrap().visual, &scene));
        assert!(
            m.layers.iter().zip(&layers).all(|(a, b)| Arc::ptr_eq(a, b)),
            "decoded art stays resident"
        );
    }
    frames.sort_by(f64::total_cmp);
    let report = serde_json::json!({
        "map_size": [160,160], "layers":layers.len(), "frames":frames.len(),
        "ui_p50_ms":frames[60], "ui_p95_ms":frames[114], "ui_max_ms":frames[119],
        "composition_jobs_during_drag":view.workbench.management.composition_jobs-jobs,
        "scene_loads_during_drag":view.generation-generation,
        "decoded_art_reused":true,
    });
    let output =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/map-drag-performance.json");
    std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    assert!(
        frames[114] < 50.,
        "160x160 whole-map UI p95 exceeded 50 ms: {report}"
    );
    view.workbench.management.cancel_drag();
    drop(view);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unchanged_mode_exits_in_one_poll_without_composition_reload_or_document_edits() {
    let (mut view, ctx, _fixture, root) = setup(None);
    // Start with an ordinary map, without applying setup's draft import.
    view.workbench.management = Default::default();
    for saved_layers in [false, true] {
        if saved_layers {
            view.open_map_management(&ctx, false);
            view.workbench
                .management
                .import_document("snow".into(), &doc(20, 20, 4, "SNOW"))
                .unwrap();
            view.workbench.management.workspace.as_mut().unwrap().pieces[1].offset = [-12, 20];
            view.workbench.management.apply_requested = true;
            settle(&mut view, &ctx, None);
            // Exercise restoration from embedded data rather than the session cache.
            view.workbench.management = Default::default();
        }
        for cancelled_drag in [false, true] {
            let before = view
                .editor
                .doc
                .as_ref()
                .unwrap()
                .snapshot()
                .bytes()
                .unwrap();
            let document_state = {
                let doc = view.editor.doc.as_ref().unwrap();
                (
                    doc.state_id,
                    doc.edit_revision(),
                    doc.dirty(),
                    doc.undo_name().map(str::to_owned),
                )
            };
            let camera = view.center;
            view.open_map_management(&ctx, false);
            assert!(view.workbench.management.open);
            if cancelled_drag {
                let m = &mut view.workbench.management;
                let offset = m.workspace.as_ref().unwrap().pieces[0].offset;
                m.drag = Some((egui::Pos2::ZERO, offset));
                m.workspace.as_mut().unwrap().pieces[0].offset[0] += 2;
                m.changed();
                assert!(m.cancel_drag());
            }
            // Matching toolbar request; no settle/wait after clicking Finish.
            let generation = view.generation;
            let jobs = view.workbench.management.composition_jobs;
            view.workbench.management.apply_requested = true;
            view.workbench.management.dirty = true;
            view.poll_map_management(&ctx);
            let m = &view.workbench.management;
            assert!(!m.open);
            assert!(!m.apply_requested && !m.dirty && !m.restore_requested);
            assert!(m.worker.is_none() && m.ready.is_none());
            assert_eq!(m.composition_jobs, jobs);
            assert_eq!(m.published_shift, [0, 0]);
            assert_eq!(view.generation, generation, "must reuse the existing scene");
            assert!(!view.loading && !view.editor.refresh && !view.editor.structural_refresh);
            assert_eq!(view.center, camera);
            let doc = view.editor.doc.as_ref().unwrap();
            assert_eq!(
                (
                    doc.state_id,
                    doc.edit_revision(),
                    doc.dirty(),
                    doc.undo_name().map(str::to_owned)
                ),
                document_state
            );
            assert_eq!(doc.snapshot().bytes().unwrap(), before);
        }
    }
    drop(view);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn saved_map_reopens_with_independent_layers_and_canvas_alignment() {
    let (mut view, ctx, fixture, root) = setup(None);
    view.workbench.management.workspace.as_mut().unwrap().pieces[1].offset = [-12, 20];
    view.workbench.management.changed();
    view.workbench.management.apply_requested = true;
    settle(&mut view, &ctx, None);
    let bytes = view
        .editor
        .doc
        .as_ref()
        .unwrap()
        .snapshot()
        .bytes()
        .unwrap();
    let bytes = ra2_map_io::publication::Publisher::default()
        .map_bytes(bytes, &fixture.0)
        .unwrap();
    let saved_path = fixture.0.join("saved.yrm");
    std::fs::write(&saved_path, bytes).unwrap();
    view.open_now(fixture.0.clone(), saved_path.to_string_lossy().into_owned());
    settle(&mut view, &ctx, None);
    assert!(view.editor.doc.is_some());
    let camera = view.center;
    view.open_map_management(&ctx, false);
    settle(&mut view, &ctx, None);
    let management = &view.workbench.management;
    let w = management.workspace.as_ref().unwrap();
    assert_eq!(w.pieces.len(), 2);
    assert_eq!(w.pieces[1].offset, [-12, 20]);
    assert_eq!(view.center, camera - translation(management.document_shift));
    view.workbench
        .management
        .workspace
        .as_mut()
        .unwrap()
        .pieces
        .remove(1);
    view.workbench.management.changed();
    let original = preview_doc(&view);
    assert_eq!(original.terrain([40, 40]).unwrap().level, 0);
    assert!(
        original
            .record_iter()
            .any(|(_, r)| r.section == "Units" && r.cell() == Some([40, 40]))
    );
    drop(view);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn finishing_main_canvas_mode_commits_once_and_reentering_keeps_original_layers_and_camera() {
    let (mut view, ctx, _fixture, root) = setup(None);
    let original = view
        .editor
        .doc
        .as_ref()
        .unwrap()
        .snapshot()
        .bytes()
        .unwrap();
    view.workbench.management.workspace.as_mut().unwrap().pieces[1].offset = [-12, 20];
    view.workbench.management.changed();
    settle(&mut view, &ctx, None);
    let camera = view.center;
    let shift = view
        .workbench
        .management
        .workspace
        .as_ref()
        .unwrap()
        .compose(&Default::default())
        .unwrap()
        .shift;
    view.workbench.management.apply_requested = true;
    view.workbench.management.dirty = true;
    settle(&mut view, &ctx, None);
    assert!(!view.workbench.management.open);
    assert_eq!(view.center, camera + translation(shift));
    let applied = view
        .editor
        .doc
        .as_ref()
        .unwrap()
        .snapshot()
        .bytes()
        .unwrap();
    assert_ne!(applied, original);
    assert_eq!(
        view.editor.doc.as_ref().unwrap().undo_name(),
        Some("地图拼接")
    );
    view.open_map_management(&ctx, false);
    settle(&mut view, &ctx, None);
    assert_eq!(view.center, camera);
    assert_eq!(
        view.workbench
            .management
            .workspace
            .as_ref()
            .unwrap()
            .pieces
            .len(),
        2
    );
    view.workbench
        .management
        .workspace
        .as_mut()
        .unwrap()
        .pieces
        .remove(1);
    view.workbench.management.selected = 0;
    view.workbench.management.changed();
    settle(&mut view, &ctx, None);
    assert_eq!(preview_doc(&view).terrain([40, 40]).unwrap().level, 0);
    assert!(
        preview_doc(&view)
            .record_iter()
            .any(|(_, r)| r.section == "Units")
    );
    view.workbench.management.apply_requested = true;
    view.workbench.management.dirty = true;
    settle(&mut view, &ctx, None);
    assert!(view.editor.doc.as_mut().unwrap().undo());
    assert_eq!(
        view.editor
            .doc
            .as_ref()
            .unwrap()
            .snapshot()
            .bytes()
            .unwrap(),
        applied
    );
    assert!(view.editor.doc.as_mut().unwrap().undo());
    assert_eq!(
        view.editor
            .doc
            .as_ref()
            .unwrap()
            .snapshot()
            .bytes()
            .unwrap(),
        original
    );
    drop(view);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires a GPU adapter; synthetic assets only; captures the real main canvas"]
fn main_canvas_gpu_preview_uses_full_terrain_and_objects_during_map_drag() {
    struct Wake(std::thread::Thread);
    impl std::task::Wake for Wake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let waker = std::task::Waker::from(Arc::new(Wake(std::thread::current())));
    let mut context = std::task::Context::from_waker(&waker);
    let config = egui_wgpu::WgpuConfiguration::default();
    let mut future = std::pin::pin!(egui_wgpu::RenderState::create(
        &config,
        &instance,
        None,
        Default::default()
    ));
    let mut state = loop {
        match std::future::Future::poll(future.as_mut(), &mut context) {
            std::task::Poll::Ready(result) => break result.unwrap(),
            std::task::Poll::Pending => std::thread::park(),
        }
    };
    state.target_format = wgpu::TextureFormat::Rgba8Unorm;
    *state.renderer.write() =
        egui_wgpu::Renderer::new(&state.device, state.target_format, Default::default());
    let (mut view, ctx, _fixture, root) = setup(Some(&state));
    view.zoom = 1.;
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/whole-map-main-canvas");
    std::fs::create_dir_all(&output).unwrap();
    settle(&mut view, &ctx, Some(&state));
    let capture = |view: &mut MapView, name: &str| {
        // Returning to the normal toolbar can change the canvas height. Resolve
        // its layout before aligning readback to integer world pixel origins.
        let _ = frame(view, &ctx, vec![], Some(&state));
        // At 1:1, compare whole pixels with integer world origins, as the
        // renderer's native readback checks do. Half-texel ties can choose
        // opposite edges in CPU and GPU floating-point interpolation.
        let half = view.workbench.canvas_rect.unwrap().size() * 0.5;
        view.center = (view.center - half).round() + half;
        let output_frame = frame(view, &ctx, vec![], Some(&state));
        let jobs = ctx.tessellate(output_frame.shapes, output_frame.pixels_per_point);
        let mut encoder = state.device.create_command_encoder(&Default::default());
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [1280, 800],
            pixels_per_point: output_frame.pixels_per_point,
        };
        let commands = state.renderer.write().update_buffers(
            &state.device,
            &state.queue,
            &mut encoder,
            &jobs,
            &screen,
        );
        let first = &view.workbench.management.layers[0];
        let framebuffer = first
            .gpu
            .as_ref()
            .unwrap()
            .target(&state.device, [1280, 800]);
        {
            let color = framebuffer.color.create_view(&Default::default());
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("whole-map viewport capture"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &color,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            state.renderer.read().render(&mut pass, &jobs, &screen);
        }
        state
            .queue
            .submit(commands.into_iter().chain([encoder.finish()]));
        let viewport = view.workbench.canvas_rect.unwrap();
        let size = [
            viewport.width().round() as u32,
            viewport.height().round() as u32,
        ];
        let actual = framebuffer
            .read_region(
                &state.device,
                &state.queue,
                [viewport.min.x.round() as u32, viewport.min.y.round() as u32],
                size,
            )
            .unwrap();
        let camera = crate::camera::MapCamera {
            center: view.center - view.workbench.management.presentation_shift(),
            zoom: view.zoom,
        };
        let mut expected = [17, 21, 26, 255].repeat((size[0] * size[1]) as usize);
        for piece in &view.workbench.management.workspace.as_ref().unwrap().pieces {
            let Some(layer) = view
                .workbench
                .management
                .layers
                .iter()
                .find(|l| Arc::ptr_eq(&l.source, &piece.clip))
            else {
                // Pending imports have no art yet; the resident layers must
                // continue rendering at their current positions underneath.
                continue;
            };
            let key = Arc::as_ptr(&piece.clip) as usize;
            let target = &view.workbench.management.layer_targets[&key];
            let world = camera
                .world_rect(viewport)
                .translate(-translation(piece.offset));
            let cpu = layer
                .scene
                .visual
                .raster(world, size, view.editor.view_options())
                .unwrap();
            let gpu = target
                .read_region(&state.device, &state.queue, [0, 0], size)
                .unwrap();
            let mut covered = 0;
            for ((dst, cpu), gpu) in expected
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(cpu.as_chunks::<4>().0)
                .zip(gpu.as_chunks::<4>().0)
            {
                if *cpu == [11, 15, 20, 255] {
                    assert_eq!(
                        gpu[3], 0,
                        "pixels outside a map must reveal the layer beneath"
                    );
                } else {
                    covered += 1;
                    assert!(
                        gpu.iter().zip(cpu).all(|(a, b)| a.abs_diff(*b) <= 2),
                        "{name}: layer pixels must match native full-scene raster: cpu={cpu:?}, gpu={gpu:?}, world={world:?}"
                    );
                    dst.copy_from_slice(cpu);
                }
            }
            assert!(
                covered > 1000,
                "exercise terrain geometry at full resolution"
            );
        }
        let differing = actual
            .as_chunks::<4>()
            .0
            .iter()
            .zip(expected.as_chunks::<4>().0)
            .filter(|(a, b)| a.iter().zip(b.iter()).any(|(a, b)| a.abs_diff(*b) > 2))
            .count();
        // Layer outlines, name labels and the drag hint are deliberately drawn
        // over the scene and occupy less than one percent of this viewport.
        assert!(
            differing < (size[0] * size[1]) as usize / 100,
            "alpha composition must preserve the underlay: {differing} pixels differ"
        );
        image::save_buffer(
            output.join(format!("{name}.png")),
            &actual,
            size[0],
            size[1],
            image::ColorType::Rgba8,
        )
        .unwrap();
    };
    capture(&mut view, "imported");
    let start = point(&view, [40, 40]);
    let _ = frame(
        &mut view,
        &ctx,
        vec![egui::Event::PointerMoved(start), button(start, true)],
        Some(&state),
    );
    let end = start + egui::vec2(630., 315.) * view.zoom;
    let _ = frame(
        &mut view,
        &ctx,
        vec![egui::Event::PointerMoved(end)],
        Some(&state),
    );
    settle(&mut view, &ctx, Some(&state));
    assert!(view.workbench.management.drag.is_some());
    capture(&mut view, "dragging-reveals-original");
    let _ = frame(&mut view, &ctx, vec![button(end, false)], Some(&state));

    // Hold a subsequent import pending deterministically. The old all-or-none
    // rendering path returned to the original composite during this interval,
    // hiding the moved map until the new map's art finished loading.
    let cache = view.workbench.management.layer_cache.take().unwrap();
    let (requests, pending_requests) = mpsc::channel();
    let (results, pending_results) = mpsc::channel();
    view.workbench.management.layer_cache = Some(LayerCache {
        requests,
        results: pending_results,
    });
    let previous_layers = view.workbench.management.layers.clone();
    let previous_targets = view.workbench.management.layer_targets.clone();
    let offsets: Vec<_> = view
        .workbench
        .management
        .workspace
        .as_ref()
        .unwrap()
        .pieces
        .iter()
        .map(|p| p.offset)
        .collect();
    let center = view.center;
    let zoom = view.zoom;
    view.workbench
        .management
        .import_document("next.yrm".into(), &doc(20, 20, 0, "SNOW"))
        .unwrap();
    for _ in 0..3 {
        capture(&mut view, "pending-import-keeps-moved-map");
        let m = &view.workbench.management;
        assert!(m.layer_pending);
        assert_eq!(m.layers.len(), 2);
        assert!(
            m.layers
                .iter()
                .zip(&previous_layers)
                .all(|(a, b)| Arc::ptr_eq(a, b))
        );
        assert!(
            previous_targets
                .iter()
                .all(|(key, target)| Arc::ptr_eq(target, &m.layer_targets[key]))
        );
        assert_eq!(
            m.workspace
                .as_ref()
                .unwrap()
                .pieces
                .iter()
                .take(2)
                .map(|p| p.offset)
                .collect::<Vec<_>>(),
            offsets
        );
        assert_eq!(view.center, center);
        assert_eq!(view.zoom, zoom);
    }
    assert_eq!(pending_requests.try_recv().unwrap().pieces.len(), 3);
    // A failed import must also keep the resident maps visible.
    results
        .send(Err(anyhow::anyhow!("test import failure")))
        .unwrap();
    capture(&mut view, "failed-import-keeps-moved-map");
    assert!(!view.workbench.management.layer_pending);
    assert!(
        view.workbench
            .management
            .message
            .contains("test import failure")
    );

    view.workbench.management.layer_cache = Some(cache);
    view.workbench.management.layer_keys.clear();
    settle(&mut view, &ctx, Some(&state));
    capture(&mut view, "import-ready-keeps-moved-map");
    let m = &view.workbench.management;
    assert_eq!(m.layers.len(), 3);
    assert!(
        m.layers
            .iter()
            .zip(&previous_layers)
            .all(|(a, b)| Arc::ptr_eq(a, b))
    );
    assert!(
        previous_targets
            .iter()
            .all(|(key, target)| Arc::ptr_eq(target, &m.layer_targets[key]))
    );
    assert_eq!(view.center, center);
    assert_eq!(view.zoom, zoom);
    // Delay scene publication deterministically: the final composite must never
    // flash back to the old map while its scene/GPU resources are in flight.
    for piece in &mut view.workbench.management.workspace.as_mut().unwrap().pieces {
        piece.offset[0] -= 12;
        piece.offset[1] -= 7;
    }
    view.center -= translation([12, 7]);
    view.workbench.management.changed();
    capture(&mut view, "before-finish");
    let composition = view
        .workbench
        .management
        .workspace
        .as_ref()
        .unwrap()
        .compose(&Default::default())
        .unwrap();
    let shift = composition.shift;
    assert_ne!(
        shift,
        [0, 0],
        "exercise normalized layer camera coordinates"
    );
    let doc = view.editor.doc.as_ref().unwrap();
    let replacement =
        PreparedReplacement::prepare(doc.snapshot(), doc.edit_revision(), composition.bytes)
            .unwrap();
    view.workbench.management.ready = Some(Prepared {
        revision: view.workbench.management.revision,
        shift,
        replacement,
    });
    let (held_sender, held_results) = mpsc::channel();
    let actual_results = std::mem::replace(&mut view.loader.results, held_results);
    view.workbench.management.apply_requested = true;
    view.poll_map_management(&ctx);
    capture(&mut view, "finish-awaiting-new-scene");
    let camera = view.center;
    for _ in 0..3 {
        capture(&mut view, "finish-awaiting-new-scene");
        assert!(!view.workbench.management.open);
        assert!(view.workbench.management.presenting_layers());
        assert!(view.workbench.management.preserve_view);
        assert!(view.editor.structural_refresh);
        assert!(view.loading);
        assert_eq!(view.center, camera);
    }
    view.loader.results = actual_results;
    drop(held_sender);
    settle(&mut view, &ctx, Some(&state));
    assert!(!view.workbench.management.presenting_layers());
    assert!(!view.editor.structural_refresh);
    assert_eq!(view.workbench.management.published_shift, [0, 0]);
    assert_eq!(view.center, camera);
    drop(view);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn whole_map_mode_uses_existing_workbench_panels_in_both_languages_and_compact_windows() {
    use egui_kittest::kittest::Queryable as _;
    let (mut view, ctx, _fixture, root) = setup(None);
    settle(&mut view, &ctx, None);
    let busy = std::cell::Cell::new(false);
    let canvas = std::cell::Cell::new(egui::Rect::NOTHING);
    for language in [
        crate::i18n::Language::Chinese,
        crate::i18n::Language::English,
    ] {
        let _language = crate::i18n::enter(language);
        let mut harness = crate::test_support::harness()
            .with_context_setup(|ctx| {
                ctx.add_font_provider(Arc::new(
                    egui_system_font_provider::SystemFontProvider::new(),
                ));
            })
            .with_accessibility_check(false)
            .with_size([1280., 800.])
            .build_ui(|ui| {
                view.loading = busy.get();
                view.workbench.management.layer_pending = busy.get();
                egui::Panel::left("tool-library")
                    .default_size(240.)
                    .show(ui, |ui| view.toolbox(ui));
                egui::Panel::right("property-inspector")
                    .default_size(220.)
                    .show(ui, |ui| view.inspector(ui));
                egui::CentralPanel::default().show(ui, |ui| view.canvas_inner(ui, None));
                canvas.set(view.workbench.canvas_rect.unwrap());
            });
        for size in [[1280., 800.], [900., 600.]] {
            harness.set_size(size.into());
            harness.run();
            let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, size.into());
            let finish = harness.get_by_label(tr!("完成并返回编辑")).rect();
            assert!(
                bounds.contains_rect(finish),
                "{language:?}: action outside main viewport: {finish:?}"
            );
            assert!(
                harness
                    .ctx
                    .memory(|m| m.area_rect(egui::Id::unique("map-management")))
                    .is_none()
            );
            harness.get_by_label("imported.yrm");
            let idle_canvas = canvas.get();
            busy.set(true);
            harness.run_steps(4);
            assert_eq!(
                canvas.get(),
                idle_canvas,
                "background loading must not move the main canvas during a drag"
            );
            busy.set(false);
            harness.run();
        }
        drop(harness);
    }
    drop(view);
    std::fs::remove_dir_all(root).unwrap();
}
