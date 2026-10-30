use super::*;
use ra2_map_document::test_support::{document, fixture};

#[test]
fn cached_object_geometry_and_navigator_follow_move_history_and_layer_changes() {
    let (mut editor, ctx, _root) = editor();
    let scene = terrain_scene();
    editor.attach(&scene);
    let id = editor
        .doc
        .as_mut()
        .unwrap()
        .add_record("Units", [18, 20], "Neutral", "MISSING")
        .unwrap();
    editor.commit(&scene, "place");
    let before_origin = editor
        .marker_cache
        .markers
        .iter()
        .find(|m| m.id == id)
        .unwrap()
        .origin;
    let mut overview = crate::overview::OverviewOverlay::default();
    let rect = egui::Rect::from_min_size(egui::pos2(30., 40.), egui::vec2(256., 128.));
    let mut draw = |editor: &Editor| {
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            overview.draw(
                ui.painter(),
                &scene,
                editor,
                rect,
                [false, false, false, true],
            );
        });
        output
            .textures_delta
            .set
            .into_iter()
            .find_map(|(_, delta)| {
                if delta.image.size() != [256, 128] {
                    return None;
                }
                let egui::ImageData::Color(image) = delta.image;
                Some(image.pixels.clone())
            })
    };
    let before = draw(&editor).unwrap();
    assert!(before.iter().any(|p| p.a() != 0));
    assert!(
        draw(&editor).is_none(),
        "unchanged navigator must reuse its texture"
    );
    let moved = editor.records[&id].moved([25, 20]).unwrap();
    editor
        .doc
        .as_mut()
        .unwrap()
        .set(Target::Record(id), Some(Value::Record(moved)))
        .unwrap();
    editor.commit(&scene, "move");
    assert_ne!(
        before_origin,
        editor
            .marker_cache
            .markers
            .iter()
            .find(|m| m.id == id)
            .unwrap()
            .origin
    );
    assert_ne!(before, draw(&editor).unwrap());
    editor.history(false, &scene);
    assert_eq!(
        before_origin,
        editor
            .marker_cache
            .markers
            .iter()
            .find(|m| m.id == id)
            .unwrap()
            .origin
    );
    assert_eq!(before, draw(&editor).unwrap());
    editor.layers[1] = false;
    assert!(draw(&editor).unwrap().iter().all(|p| p.a() == 0));
    editor.layers[1] = true;
    assert_eq!(before, draw(&editor).unwrap());
    editor.history(true, &scene);
    assert_ne!(before, draw(&editor).unwrap());
}

pub(crate) fn terrain_scene() -> MapScene {
    let fixture = ra2_map_assets::test_support::Fixture::new();
    let mut install = fixture.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    ra2_map_scene::map::load_scene_bytes(
        &mut install,
        "stroke.map",
        "fixture".into(),
        crate::new_map::NewMapOptions {
            width: 20,
            height: 20,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
        Instant::now(),
    )
    .unwrap()
}

#[test]
fn height_drag_deduplicates_revisited_cells_samples_release_and_undoes_once() {
    let (mut editor, _, root) = editor();
    let scene = terrain_scene();
    editor.attach(&scene);
    let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    editor.tool = Tool::Terrain;
    editor.terrain_mode = TerrainMode::Raise;
    editor.smooth_heights = false;
    editor.auto_lat = false;
    editor.pointer(&scene, Some([20, 20]), true, true, false, None);
    editor.pointer(&scene, Some([21, 20]), false, true, false, None);
    editor.pointer(&scene, Some([20, 20]), false, true, false, None);
    assert_eq!(
        editor
            .doc
            .as_ref()
            .unwrap()
            .terrain([20, 20])
            .unwrap()
            .level,
        0,
        "height stroke is previewed until release"
    );
    // A sidebar may change the current tool in the release frame.
    editor.tool = Tool::Erase;
    editor.pointer(&scene, Some([22, 20]), false, false, true, None);
    let doc = editor.doc.as_ref().unwrap();
    for x in 20..=22 {
        assert_eq!(doc.terrain([x, 20]).unwrap().level, 1);
    }
    let after = doc.snapshot().bytes().unwrap();
    editor.history(false, &scene);
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        before
    );
    editor.history(true, &scene);
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        after
    );
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_fill_cancels_the_stroke_and_keeps_document_and_history_unchanged() {
    let (mut editor, _, root) = editor();
    let mut scene = terrain_scene();
    let b = &scene.brushes[0];
    Arc::get_mut(&mut scene.palette)
        .unwrap()
        .templates
        .get_mut(&(b.key.tile_id, b.key.variant))
        .unwrap()
        .width = 2;
    editor.attach(&scene);
    let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    editor.tool = Tool::Terrain;
    editor.terrain_mode = TerrainMode::Fill;
    editor.pointer(&scene, Some([20, 20]), true, false, true, None);
    assert!(!editor.stroking);
    assert!(editor.status.contains("1×1"));
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        before
    );
    assert!(editor.doc.as_ref().unwrap().undo_name().is_none());
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn terrain_library_groups_templates_and_preserves_subtiles_and_variants_on_request() {
    use ra2_map_assets::terrain_library::{TerrainCategory, TerrainGroup};
    let mut scene = terrain_scene();
    let original = scene.brushes[0].clone();
    let palette = Arc::get_mut(&mut scene.palette).unwrap();
    palette.groups.insert(
        original.key.tile_id,
        TerrainGroup {
            id: 7,
            name: "Custom Cliff".into(),
            category: TerrainCategory::Cliff,
        },
    );
    let mut sub = original.clone();
    sub.key.sub_tile = 1;
    let mut variant = original.clone();
    variant.key.variant = 1;
    let mut other = original.clone();
    other.key.tile_id = 99;
    scene.brushes = vec![original, sub, variant, other];
    assert_eq!(
        terrain_ui::choices(
            &scene,
            Some(TerrainCategory::Cliff),
            Some(7),
            false,
            false,
            ""
        ),
        vec![0]
    );
    assert_eq!(
        terrain_ui::choices(&scene, None, Some(7), true, true, ""),
        vec![0, 1, 2]
    );
    assert_eq!(
        terrain_ui::choices(&scene, None, None, false, false, ""),
        vec![0, 3]
    );
    assert_eq!(
        terrain_ui::choices(&scene, None, None, false, true, "悬崖"),
        vec![0, 2]
    );
    scene.brushes[0].label = "Water Cliffs · cliff01.tem · 0:0 · v0".into();
    let before = scene.brushes[0].clone();
    for filter in ["水面悬崖", "water cliffs", "cliff01.tem"] {
        let matching = terrain_ui::choices(&scene, None, Some(7), false, false, filter);
        assert_eq!(
            matching,
            vec![0],
            "translated names and original IDs remain searchable"
        );
        assert_eq!(scene.brushes[matching[0]].key, before.key);
        assert_eq!(scene.brushes[matching[0]].raw, before.raw);
    }
    assert_eq!(scene.brushes[0].label, before.label);
    assert_eq!(crate::labels::terrain("Custom Cliff"), "Custom Cliff");
}

#[test]
fn template_height_is_frozen_on_press_and_invalid_preview_does_not_write() {
    let (mut editor, ctx, root) = editor();
    let scene = terrain_scene();
    editor.attach(&scene);
    editor.tool = Tool::Terrain;
    editor.terrain_mode = TerrainMode::Template;
    editor.auto_lat = false;
    editor.template_level = Some(3);
    editor.pointer(&scene, Some([20, 20]), true, true, false, None);
    editor.template_level = Some(8);
    editor.pointer(&scene, Some([21, 20]), false, false, true, None);
    assert_eq!(
        editor
            .doc
            .as_ref()
            .unwrap()
            .terrain([20, 20])
            .unwrap()
            .level,
        3
    );
    assert_eq!(
        editor
            .doc
            .as_ref()
            .unwrap()
            .terrain([21, 20])
            .unwrap()
            .level,
        3
    );
    let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    editor.template_level = Some(15);
    let _ = ctx.run_ui(Default::default(), |ui| {
        let rect = ui.max_rect();
        editor.terrain_preview(
            ui.painter(),
            &scene,
            rect,
            &crate::camera::MapCamera {
                center: scene.bounds.center(),
                zoom: 1.,
            },
            Some([20, 20]),
        );
    });
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        before
    );
    editor.pointer(&scene, Some([22, 20]), true, false, true, None);
    assert!(editor.status.starts_with("本笔未修改"));
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        before
    );
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn terrain_template_ghost_matches_saved_tile_position_at_multiple_heights_and_zooms() {
    let fixture = ra2_map_assets::test_support::Fixture::new();
    let mut install = fixture.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    let scene = ra2_map_scene::map::load_scene_bytes(
        &mut install,
        "ghost.map",
        "test".into(),
        crate::new_map::NewMapOptions {
            width: 20,
            height: 20,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
        Instant::now(),
    )
    .unwrap();
    let (mut editor, ctx, root) = editor();
    editor.attach(&scene);
    editor.tool = Tool::Terrain;
    editor.terrain_mode = TerrainMode::Template;
    editor.auto_lat = false;
    for level in [0, 4, 10] {
        editor.template_level = Some(level);
        let at = [20, 20];
        let mut ghost = None;
        for zoom in [0.5, 1., 1.75] {
            let rect = egui::Rect::from_min_size(egui::pos2(71., 37.), egui::vec2(500., 400.));
            let camera = crate::camera::MapCamera {
                center: scene.bounds.center(),
                zoom,
            };
            let output = ctx.run_ui(Default::default(), |ui| {
                editor.terrain_preview(ui.painter(), &scene, rect, &camera, Some(at));
            });
            let mesh = output
                .shapes
                .iter()
                .find_map(|shape| {
                    if let egui::Shape::Mesh(mesh) = &shape.shape
                        && mesh
                            .vertices
                            .iter()
                            .all(|v| v.color == Color32::from_white_alpha(170))
                    {
                        Some(mesh)
                    } else {
                        None
                    }
                })
                .expect("complete template ghost");
            let min = mesh
                .vertices
                .iter()
                .fold(egui::pos2(f32::INFINITY, f32::INFINITY), |p, v| {
                    p.min(v.pos)
                });
            ghost = Some(camera.world_point(rect, min));
            let (x, y) = vera20k_core::map::terrain::iso_to_screen(at[0], at[1], level);
            assert!((ghost.unwrap() - egui::pos2(x, y)).length() < 0.001);
        }
        editor.pointer(&scene, Some(at), true, false, true, None);
        let bytes = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        let placed = ra2_map_scene::map::load_scene_bytes(
            &mut install,
            "placed.map",
            "test".into(),
            bytes,
            Instant::now(),
        )
        .unwrap();
        let cell = placed.cells.iter().find(|c| [c.rx, c.ry] == at).unwrap();
        assert_eq!(cell.z, level);
        let image = placed.image(cell).unwrap();
        assert!(
            (ghost.unwrap()
                - egui::pos2(
                    cell.screen_x + image.offset_x as f32,
                    cell.screen_y + image.offset_y as f32
                ))
            .length()
                < 0.001
        );
    }
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}
pub(crate) fn editor() -> (Editor, egui::Context, PathBuf) {
    static NEXT_ROOT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "yr-edit-worker-{}-{}-{}",
        std::process::id(),
        NEXT_ROOT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let ctx = egui::Context::default();
    let mut editor = Editor::new(ctx.clone(), root.clone()).unwrap();
    editor.doc = Some(document());
    editor.recovery = Some(root.join("recovery/current.yrrec"));
    (editor, ctx, root)
}
fn poll_until(editor: &mut Editor, ctx: &egui::Context, condition: impl Fn(&Editor) -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        editor.poll(ctx);
        if condition(editor) {
            break;
        }
        assert!(Instant::now() < until, "worker timeout: {}", editor.status);
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn late_save_receipt_does_not_clear_new_edits_and_undo_reaches_saved_state() {
    let (mut editor, ctx, root) = editor();
    let path = root.join("edited.map");
    let doc = editor.doc.as_mut().unwrap();
    doc.waypoint(1, [3, 2]).unwrap();
    doc.commit("saved start");
    let snapshot = doc.snapshot();
    let saved_bytes = snapshot.bytes().unwrap();
    editor
        .io
        .send(Job::Save {
            directory: None,
            snapshot,
            path: path.clone(),
            expected: Expected::Exact(None),
        })
        .unwrap();
    editor.saving = true;
    let doc = editor.doc.as_mut().unwrap();
    doc.waypoint(2, [2, 3]).unwrap();
    doc.commit("newer start");
    poll_until(&mut editor, &ctx, |e| !e.saving);
    assert!(editor.dirty());
    assert_eq!(std::fs::read(path).unwrap(), saved_bytes);
    editor.doc.as_mut().unwrap().undo();
    assert!(!editor.dirty());
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn late_recovery_is_removed_after_undo_to_clean_document() {
    let (mut editor, ctx, root) = editor();
    let doc = editor.doc.as_mut().unwrap();
    doc.waypoint(1, [3, 2]).unwrap();
    doc.commit("start");
    let snapshot = doc.snapshot();
    let path = editor.recovery.clone().unwrap();
    let meta = RecoveryMeta {
        version: 1,
        name: "test.map".into(),
        directory: root.clone(),
        target: None,
        expected: None,
        resource_profile: None,
    };
    editor
        .io
        .send(Job::Recover {
            snapshot,
            path: path.clone(),
            meta,
        })
        .unwrap();
    editor.recovering = true;
    editor.doc.as_mut().unwrap().undo();
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        fixture()
    );
    poll_until(&mut editor, &ctx, |e| !e.recovering);
    // Fence the discard behind a real read on the same worker queue.
    editor.io.send(Job::Restore(path.clone())).unwrap();
    poll_until(&mut editor, &ctx, |e| e.status.starts_with("恢复失败"));
    assert!(!path.exists());
    assert!(!editor.dirty());
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn untouched_new_map_recovers_without_a_target_and_first_save_clears_dirty() {
    let (mut editor, ctx, root) = editor();
    let bytes = crate::new_map::NewMapOptions {
        title: "新建测试".into(),
        ..Default::default()
    }
    .bytes()
    .unwrap();
    let (source, _) =
        crate::map::SourceMap::parse("Untitled.map".into(), "新建地图".into(), bytes.clone())
            .unwrap();
    assert_eq!(source.title, "新建测试");
    editor.doc = Some(MapDocument::new(source.edit_base.unwrap()));
    editor.mark_new();
    assert!(editor.dirty());
    assert!(editor.target.is_none());
    assert!(editor.expected.is_none());
    // Cancelling or undoing back to the initial blank state still needs a save.
    let doc = editor.doc.as_mut().unwrap();
    doc.waypoint(2, [80, 80]).unwrap();
    doc.commit("add start");
    doc.undo();
    assert!(editor.dirty());
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        bytes
    );
    editor.last_edit = Instant::now() - Duration::from_secs(4);
    poll_until(&mut editor, &ctx, |e| e.last_recovery.is_some());
    let (meta, recovered) =
        crate::editor_io::read_recovery(editor.recovery.as_ref().unwrap()).unwrap();
    assert_eq!(meta.target, None);
    assert_eq!(recovered, bytes);
    let path = root.join("新地图.map");
    editor
        .io
        .send(Job::Save {
            directory: None,
            snapshot: editor.doc.as_ref().unwrap().snapshot(),
            path: path.clone(),
            expected: Expected::Exact(None),
        })
        .unwrap();
    editor.saving = true;
    poll_until(&mut editor, &ctx, |e| !e.saving);
    assert!(!editor.dirty());
    assert_eq!(editor.target, Some(path.clone()));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}
