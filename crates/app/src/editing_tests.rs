use super::*;
use ra2_map_document::test_support::{document, fixture};

#[test]
fn ini_commits_and_history_keep_spatial_caches_until_the_scene_changes() {
    let (mut editor, ctx, root) = editor();
    let scene = terrain_scene();
    editor.attach(&scene);
    let revision = editor.visual_revision;
    editor
        .doc
        .as_mut()
        .unwrap()
        .set_ini("Events", "TRIGGER", Some(b"1,13,0,2".to_vec()))
        .unwrap();
    editor.commit(&scene, "event");
    assert!(editor.refresh);
    assert_eq!(editor.visual_revision, revision);
    editor.refresh = false;
    editor.history(false, &scene);
    assert!(editor.refresh);
    assert_eq!(editor.visual_revision, revision);
    assert!(
        editor
            .doc
            .as_ref()
            .unwrap()
            .ini_value("Events", "TRIGGER")
            .is_none()
    );
    editor.history(true, &scene);
    assert_eq!(editor.visual_revision, revision);
    assert_eq!(
        editor.doc.as_ref().unwrap().ini_text("Events", "TRIGGER"),
        "1,13,0,2"
    );
    editor
        .doc
        .as_mut()
        .unwrap()
        .add_record("Units", [20, 20], "Neutral", "HTNK")
        .unwrap();
    editor.commit(&scene, "unit");
    assert!(editor.visual_revision > revision);
    let revision = editor.visual_revision;
    editor.history(false, &scene);
    assert!(editor.visual_revision > revision);
    drop(editor);
    drop(ctx);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn navigator_uses_the_same_complete_engine_image_as_previewpack() {
    let (mut editor, _ctx, root) = editor();
    let scene = terrain_scene();
    editor.attach(&scene);
    let radar = ra2_map_scene::radar::render(&scene, editor.doc.as_ref()).unwrap();
    let preview = ra2_map_io::publication::render_preview(&scene).unwrap();
    assert_eq!(
        radar.image.size,
        [preview.width() as usize, preview.height() as usize]
    );
    let pixels: Vec<_> = radar
        .image
        .pixels
        .iter()
        .flat_map(|p| p.to_array())
        .collect();
    assert_eq!(&pixels, preview.as_raw());
    let doc = editor.doc.as_mut().unwrap();
    doc.waypoint(0, [20, 20]).unwrap();
    editor.commit(&scene, "start");
    let moved = ra2_map_scene::radar::render(&scene, editor.doc.as_ref()).unwrap();
    assert_ne!(radar.image, moved.image);
    assert!(
        moved
            .image
            .pixels
            .contains(&egui::Color32::from_rgb(240, 0, 0))
    );
    editor.history(false, &scene);
    assert_eq!(
        radar.image,
        ra2_map_scene::radar::render(&scene, editor.doc.as_ref())
            .unwrap()
            .image
    );
    editor.history(true, &scene);
    assert_eq!(
        moved.image,
        ra2_map_scene::radar::render(&scene, editor.doc.as_ref())
            .unwrap()
            .image
    );
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
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
fn shift_locks_terrain_brushes_to_one_axis_including_preview_release_and_history() {
    for mode in [
        TerrainMode::Subtile,
        TerrainMode::Template,
        TerrainMode::Raise,
    ] {
        let (mut e, ctx, root) = editor();
        let mut scene = terrain_scene();
        // Give the synthetic brush a distinct saved tile value.
        scene.brushes[0].raw = 1;
        e.attach(&scene);
        e.tool = Tool::Terrain;
        e.terrain_mode = mode;
        e.template_level = Some(2);
        e.smooth_heights = false;
        e.auto_lat = false;
        let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        e.brush_modifiers(egui::Modifiers::SHIFT);
        e.pointer(&scene, Some([20, 20]), true, true, false, None);
        e.pointer(&scene, Some([24, 21]), false, true, false, None);
        assert_eq!(e.feedback, (20..=24).map(|x| [x, 20]).collect());

        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800., 600.));
        let camera = crate::camera::MapCamera {
            center: scene.bounds.center(),
            zoom: 1.,
        };
        let draw = |e: &mut Editor, at| {
            crate::test_support::run_ui(&ctx, Default::default(), |ui| {
                e.terrain_preview(ui.painter(), &scene, rect, &camera, Some(at));
            })
            .shapes
        };
        assert_eq!(
            draw(&mut e, [21, 27]),
            draw(&mut e, [21, 20]),
            "the preview must follow the locked axis even when the cursor crosses the other axis"
        );
        e.pointer(&scene, Some([21, 27]), false, true, false, None);
        assert_eq!(e.feedback, BTreeSet::from([[20, 20], [21, 20]]));
        assert_eq!(e.doc.as_ref().unwrap().terrain([24, 20]).unwrap().tile, 0);
        e.pointer(&scene, Some([18, 27]), false, true, false, None);
        assert_eq!(e.feedback, (18..=20).map(|x| [x, 20]).collect());
        e.pointer(&scene, Some([20, 27]), false, true, false, None);
        assert_eq!(e.feedback, BTreeSet::from([[20, 20]]));
        e.pointer(&scene, Some([26, 25]), false, true, false, None);
        // Shrink again in the release frame: commit only the final line.
        e.pointer(&scene, Some([23, 25]), false, false, true, None);
        assert!(!e.stroking);
        assert_eq!(e.feedback, (20..=23).map(|x| [x, 20]).collect());
        let doc = e.doc.as_ref().unwrap();
        for x in 20..=23 {
            let value = doc.terrain([x, 20]).unwrap();
            if mode == TerrainMode::Raise {
                assert_eq!(value.level, 1);
            } else {
                assert_eq!(value.tile, 1);
            }
        }
        assert_eq!(doc.terrain([24, 21]).unwrap().level, 0);
        assert_eq!(doc.terrain([24, 21]).unwrap().tile, 0);
        assert_eq!(doc.terrain([24, 20]).unwrap().level, 0);
        assert_eq!(doc.terrain([24, 20]).unwrap().tile, 0);
        let after = doc.snapshot().bytes().unwrap();
        assert_ne!(after, before);
        e.history(false, &scene);
        assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
        e.history(true, &scene);
        assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
        drop(e);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn shift_can_toggle_during_a_brush_stroke_and_reentry_starts_a_new_constraint() {
    let (mut e, _, root) = editor();
    let mut scene = terrain_scene();
    Arc::make_mut(&mut scene.catalog).ore = Some(102);
    e.attach(&scene);
    e.tool = Tool::Ore;
    e.pointer(&scene, Some([20, 20]), true, true, false, None);
    e.pointer(&scene, Some([21, 21]), false, true, false, None);
    e.brush_modifiers(egui::Modifiers::SHIFT);
    e.pointer(&scene, Some([22, 24]), false, true, false, None);
    assert!(e.feedback.contains(&[21, 24]));
    assert!(!e.feedback.contains(&[22, 24]));
    e.brush_modifiers(egui::Modifiers::NONE);
    e.pointer(&scene, Some([23, 24]), false, true, false, None);
    assert!(e.feedback.contains(&[23, 24]));
    e.brush_modifiers(egui::Modifiers::SHIFT);
    e.pointer(&scene, Some([26, 25]), false, true, false, None);
    assert!(e.feedback.contains(&[26, 24]));
    assert!(!e.feedback.contains(&[26, 25]));
    e.pointer(&scene, Some([24, 26]), false, true, false, None);
    assert!(e.feedback.contains(&[21, 21]));
    assert!(e.feedback.contains(&[21, 24]));
    assert!(e.feedback.contains(&[24, 24]));
    assert!(!e.feedback.contains(&[26, 24]));
    assert_eq!(e.doc.as_ref().unwrap().overlay([26, 24]).0, 255);
    assert!(!e.overlays.contains_key(&[26, 24]));
    let painted = e.feedback.clone();
    e.pointer(&scene, None, false, true, false, None);
    e.pointer(&scene, Some([28, 20]), false, true, false, None);
    e.pointer(&scene, Some([29, 23]), false, false, true, None);
    assert_eq!(
        e.feedback,
        painted
            .into_iter()
            .chain((20..=23).map(|y| [28, y]))
            .collect()
    );
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().overlay([28, 23]).0, 255);
    // A new stroke with Shift still held must choose its own origin and axis.
    e.pointer(&scene, Some([20, 20]), true, true, false, None);
    e.pointer(&scene, Some([24, 21]), false, false, true, None);
    assert_eq!(e.feedback, (20..=24).map(|x| [x, 20]).collect());
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn shift_erase_retraction_restores_original_overlays_under_a_wide_brush() {
    let (mut e, _, root) = editor();
    let scene = terrain_scene();
    e.attach(&scene);
    for x in 19..=26 {
        for y in 19..=20 {
            e.doc
                .as_mut()
                .unwrap()
                .set(Target::Overlay([x, y]), Some(Value::Overlay(102, 7)))
                .unwrap();
        }
    }
    e.commit(&scene, "original resources");
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.tool = Tool::Erase;
    e.radius = 2;
    e.brush_modifiers(egui::Modifiers::SHIFT);
    e.pointer(&scene, Some([20, 20]), true, true, false, None);
    e.pointer(&scene, Some([26, 21]), false, true, false, None);
    assert!(!e.overlays.contains_key(&[26, 20]));
    e.pointer(&scene, Some([22, 21]), false, false, true, None);
    for x in 19..=26 {
        for y in 19..=20 {
            let expected = if x <= 22 { (255, 0) } else { (102, 7) };
            assert_eq!(e.doc.as_ref().unwrap().overlay([x, y]), expected);
            assert_eq!(e.overlays.contains_key(&[x, y]), x > 22);
        }
    }
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
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
    editor.template_level = Some(128);
    let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
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
            let output = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
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
    // The startup scan creates recovery/. Finish it before a short-lived
    // fixture can drop the editor and remove the root on another thread.
    match editor
        .io
        .results
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
    {
        Done::Scan(result) => assert!(result.unwrap().is_empty()),
        _ => panic!("expected initial recovery scan"),
    }
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
