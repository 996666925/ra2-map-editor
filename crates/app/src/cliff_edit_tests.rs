use super::super::tests::editor;
use super::*;

fn scene() -> MapScene {
    scene_at_level(4)
}

fn scene_at_level(level: u8) -> MapScene {
    scene_from_fixture(
        &ra2_map_assets::test_support::Fixture::with_authoring(),
        level,
    )
}

fn scene_from_fixture(f: &ra2_map_assets::test_support::Fixture, level: u8) -> MapScene {
    let mut install = f.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    crate::map::load_scene_bytes(
        &mut install,
        "plateau.map",
        "test".into(),
        crate::new_map::NewMapOptions {
            width: 40,
            height: 40,
            level,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
        Instant::now(),
    )
    .unwrap()
}

fn polyline_key(e: &mut Editor, ctx: &egui::Context, scene: &MapScene, key: egui::Key) {
    let input = egui::RawInput {
        events: vec![egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
        ..Default::default()
    };
    let _ = crate::test_support::run_ui(ctx, input, |ui| e.shortcuts(ui.ctx(), scene));
}

#[test]
fn road_line_respects_shift_and_commits_the_rendered_plan_in_one_edit() {
    let (mut editor, _, root) = editor();
    let scene = scene_from_fixture(&ra2_map_assets::test_support::Fixture::with_joining(), 4);
    editor.attach(&scene);
    editor.activate_terrain_mode(TerrainMode::Road);
    let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    let start = [26, 26];
    let end = [46, 42];
    editor.pointer(&scene, Some(start), true, true, false, None);
    editor.brush_modifiers(egui::Modifiers::SHIFT);
    editor.pointer(&scene, Some(end), false, true, false, None);
    let straight = editor.plateau_preview_plan(&scene, end).unwrap();
    assert!(straight.interior.contains(&[46, 26]));
    assert!(straight.interior.iter().all(|at| at[1] == 26));
    editor.brush_modifiers(egui::Modifiers::NONE);
    editor.pointer(&scene, Some(end), false, true, false, None);
    let diagonal = editor.plateau_preview_plan(&scene, end).unwrap();
    assert!(diagonal.interior.contains(&start) && diagonal.interior.contains(&end));
    assert_ne!(straight.edits, diagonal.edits);
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        before
    );
    editor.pointer(&scene, Some(end), false, false, true, None);
    assert!(!editor.stroking, "{}", editor.status);
    for (&at, &v) in &diagonal.edits {
        assert_eq!(editor.doc.as_ref().unwrap().terrain(at), Some(v));
    }
    editor.history(false, &scene);
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        before
    );
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn road_route_start_clicked_in_one_frame_does_not_confirm_a_hover_leg() {
    let (mut editor, ctx, root) = editor();
    let scene = scene_from_fixture(&ra2_map_assets::test_support::Fixture::with_joining(), 4);
    editor.attach(&scene);
    editor.activate_terrain_mode(TerrainMode::RoadPolyline);
    let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    let start = [25, 27];
    let hover = [39, 27];

    // A complete click can arrive between two canvas frames.
    editor.pointer(&scene, Some(start), true, false, true, None);
    editor.pointer(&scene, Some(hover), false, false, false, None);
    assert_eq!(editor.plateau.gesture.as_ref().unwrap().path, [start]);
    assert!(editor.plateau_preview_plan(&scene, hover).is_ok());
    polyline_key(&mut editor, &ctx, &scene, egui::Key::Enter);
    assert!(editor.stroking);
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        before
    );

    editor.pointer(&scene, Some(hover), true, false, true, None);
    assert_eq!(
        editor.plateau.gesture.as_ref().unwrap().path,
        [start, hover]
    );
    polyline_key(&mut editor, &ctx, &scene, egui::Key::Enter);
    assert!(!editor.stroking, "{}", editor.status);
    assert_ne!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        before
    );
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn road_route_hover_keeps_confirmed_diagonal_and_displays_it_behind_an_error() {
    let (mut editor, ctx, root) = editor();
    let scene = scene_from_fixture(&ra2_map_assets::test_support::Fixture::with_joining(), 4);
    editor.attach(&scene);
    editor.activate_terrain_mode(TerrainMode::RoadPolyline);
    let start = [38, 28];
    let first = [48, 38];
    let blocked = [48, 48];
    let doc = editor.doc.as_mut().unwrap();
    doc.set(Target::Overlay(blocked), Some(Value::Overlay(0, 0)))
        .unwrap();
    doc.commit("block hover");
    let before = doc.snapshot().bytes().unwrap();

    editor.pointer(&scene, Some(start), true, true, false, None);
    let first_preview = editor.plateau_preview_plan(&scene, first).unwrap();
    editor.pointer(&scene, Some(first), false, false, true, None);
    let confirmed = editor.plateau_preview_plan(&scene, first).unwrap();
    assert!(Arc::ptr_eq(&first_preview, &confirmed));

    // The second endpoint crosses to the other side of the start. Planning
    // all vertices at once can flip the first staircase's traversal here.
    for end in [[48, 43], [28, 44], [48, 44]] {
        let preview = editor.plateau_preview_plan(&scene, end).unwrap();
        assert!(preview.interior.contains(&end));
        assert!(confirmed.interior.is_subset(&preview.interior));
        assert_eq!(
            confirmed
                .edits
                .iter()
                .filter(|(at, _)| at[1] <= 32)
                .collect::<Vec<_>>(),
            preview
                .edits
                .iter()
                .filter(|(at, _)| at[1] <= 32)
                .collect::<Vec<_>>()
        );
    }
    let result = editor.plateau.generate(
        editor.doc.as_ref().unwrap(),
        &scene,
        blocked,
        CutOptions::default(),
    );
    assert!(result.is_err());
    assert!(Arc::ptr_eq(
        &confirmed,
        &editor.plateau.displayed_plan(result).unwrap()
    ));

    // A blocked click stays editable. Removing it restores the exact draft
    // before it, including the already resolved staircase.
    editor.pointer(&scene, Some(blocked), true, true, false, None);
    editor.pointer(&scene, Some(blocked), false, false, true, None);
    assert!(editor.plateau_preview_plan(&scene, blocked).is_err());
    polyline_key(&mut editor, &ctx, &scene, egui::Key::Backspace);
    assert!(Arc::ptr_eq(
        &confirmed,
        &editor.plateau_preview_plan(&scene, first).unwrap()
    ));
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        before
    );
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn road_route_connects_dragged_and_clicked_legs_and_commits_confirmed_points() {
    let (mut editor, ctx, root) = editor();
    let scene = scene_from_fixture(&ra2_map_assets::test_support::Fixture::with_joining(), 4);
    editor.attach(&scene);
    editor.activate_terrain_mode(TerrainMode::RoadPolyline);
    let before = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    let start = [25, 27];
    let first = [37, 27];
    let second = [37, 39];
    let third = [49, 39];
    editor.pointer(&scene, Some(start), true, true, false, None);
    editor.pointer(&scene, Some(first), false, false, true, None);
    assert!(editor.stroking);
    assert_eq!(
        editor.plateau.gesture.as_ref().unwrap().path,
        [start, first]
    );

    editor.brush_modifiers(egui::Modifiers::SHIFT);
    editor.pointer(&scene, Some([40, 39]), true, true, false, None);
    editor.pointer(&scene, Some([40, 39]), false, false, true, None);
    editor.brush_modifiers(egui::Modifiers::NONE);
    assert_eq!(
        editor.plateau.gesture.as_ref().unwrap().path,
        [start, first, second]
    );

    editor.pointer(&scene, Some(third), true, true, false, None);
    editor.pointer(&scene, Some(third), false, false, true, None);
    polyline_key(&mut editor, &ctx, &scene, egui::Key::Backspace);
    assert_eq!(
        editor.plateau.gesture.as_ref().unwrap().path,
        [start, first, second]
    );
    editor.pointer(&scene, Some(third), true, true, false, None);
    editor.pointer(&scene, Some(third), false, false, true, None);
    let confirmed = editor.plateau_preview_plan(&scene, third).unwrap();
    let hover = [49, 48];
    assert_ne!(
        editor.plateau_preview_plan(&scene, hover).unwrap().edits,
        confirmed.edits
    );
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        before
    );
    polyline_key(&mut editor, &ctx, &scene, egui::Key::Enter);
    assert!(!editor.stroking, "{}", editor.status);
    for (&at, &v) in &confirmed.edits {
        assert_eq!(editor.doc.as_ref().unwrap().terrain(at), Some(v));
    }
    assert_eq!(
        terrain_tools::value(editor.doc.as_ref().unwrap(), &scene, hover).tile,
        scene.catalog.raw_tiles[&(scene.palette.tile_ids.unwrap().clear as u16)]
    );
    let after = editor.doc.as_ref().unwrap().snapshot().bytes().unwrap();
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

    editor.pointer(&scene, Some(third), true, true, false, None);
    editor.pointer(&scene, Some(hover), false, false, true, None);
    polyline_key(&mut editor, &ctx, &scene, egui::Key::Escape);
    assert!(!editor.stroking);
    assert_eq!(
        editor.doc.as_ref().unwrap().snapshot().bytes().unwrap(),
        after
    );
    drop(editor);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn polyline_high_side_dropdown_selects_a_direction_without_terrain_edits() {
    use egui_kittest::kittest::Queryable as _;
    let (mut e, _, root) = editor();
    let scene = scene();
    e.attach(&scene);
    e.activate_terrain_mode(TerrainMode::CliffPolyline);
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    let mut harness = crate::test_support::harness()
        .with_context_setup(|ctx| {
            ctx.add_font_provider(Arc::new(
                egui_system_font_provider::SystemFontProvider::new(),
            ));
        })
        .with_accessibility_check(false)
        .with_size([420., 600.])
        .build_ui(|ui| e.plateau_options(ui, &scene));
    harness.run();
    assert!(harness.query_by_label("绘制方向右侧").is_none());
    harness
        .get_by_role_and_label(egui::accesskit::Role::ComboBox, "高侧方向")
        .click();
    harness.run();
    harness.get_by_label("绘制方向右侧").click();
    harness.run();
    assert!(harness.query_by_label("绘制方向左侧").is_none());
    drop(harness);
    assert!(!e.cliff_high_left);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn polyline_clicks_keep_a_draft_enter_commits_confirmed_vertices_and_history_is_atomic() {
    let (mut e, ctx, root) = editor();
    let scene = scene();
    e.attach(&scene);
    e.activate_terrain_mode(TerrainMode::CliffPolyline);
    e.cliff_high_left = false;
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    for at in [[30, 30], [44, 30], [44, 44]] {
        e.pointer(&scene, Some(at), true, true, false, None);
        e.pointer(&scene, Some(at), false, false, true, None);
        assert!(e.stroking, "releasing a click must keep the polyline draft");
        assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    }
    let p = e.plateau_preview_plan(&scene, [44, 44]).unwrap();
    assert!(p.interior.is_empty());
    assert!(Arc::ptr_eq(
        &p,
        &e.plateau_preview_plan(&scene, [44, 44]).unwrap()
    ));
    // Hovering an extra segment must not confirm it on Enter.
    let _ = e.plateau_preview_plan(&scene, [30, 44]);
    e.cliff_high_left = true;
    polyline_key(&mut e, &ctx, &scene, egui::Key::Enter);
    assert!(!e.stroking, "{}", e.status);
    for (&at, &v) in &p.edits {
        assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
    }
    let after = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.history(true, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn polyline_closes_on_first_vertex_and_can_retract_invalid_or_cancelled_drafts() {
    let (mut e, ctx, root) = editor();
    let scene = scene();
    e.attach(&scene);
    e.activate_terrain_mode(TerrainMode::CliffPolyline);
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.pointer(&scene, Some([30, 30]), true, true, false, None);
    polyline_key(&mut e, &ctx, &scene, egui::Key::Enter);
    assert!(e.stroking, "invalid draft remains editable");
    e.pointer(&scene, Some([44, 30]), true, true, false, None);
    polyline_key(&mut e, &ctx, &scene, egui::Key::Backspace);
    assert_eq!(e.plateau.gesture.as_ref().unwrap().path, vec![[30, 30]]);
    for at in [[44, 30], [44, 44], [30, 44]] {
        e.pointer(&scene, Some(at), true, true, false, None);
        e.pointer(&scene, Some(at), false, false, true, None);
    }
    let p = e.plateau_preview_plan(&scene, [30, 30]).unwrap();
    assert!(!p.interior.is_empty());
    assert_eq!(
        p.edits[&[36, 36]].level,
        0,
        "clockwise drawing with the left high side makes a pit"
    );
    e.pointer(&scene, Some([30, 30]), true, true, false, None);
    e.pointer(&scene, Some([30, 30]), false, false, true, None);
    assert!(!e.stroking);
    for (&at, &v) in &p.edits {
        assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
    }
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.pointer(&scene, Some([30, 30]), true, true, false, None);
    e.pointer(&scene, Some([44, 30]), true, true, false, None);
    polyline_key(&mut e, &ctx, &scene, egui::Key::Escape);
    assert!(!e.stroking);
    e.pointer(&scene, Some([44, 30]), false, false, true, None);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn polyline_zero_ground_pit_prompts_to_raise_ground_and_keeps_an_editable_draft() {
    let (mut e, ctx, root) = editor();
    let scene = scene_at_level(0);
    e.attach(&scene);
    e.activate_terrain_mode(TerrainMode::CliffPolyline);
    e.cliff_high_left = true;
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    for at in [[30, 30], [44, 30], [44, 44], [30, 44], [30, 30]] {
        e.pointer(&scene, Some(at), true, true, false, None);
        e.pointer(&scene, Some(at), false, false, true, None);
    }
    assert!(e.status.contains("请先抬高地面"), "{}", e.status);
    assert!(e.stroking);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    polyline_key(&mut e, &ctx, &scene, egui::Key::Backspace);
    assert_eq!(e.plateau.gesture.as_ref().unwrap().path.len(), 4);
    polyline_key(&mut e, &ctx, &scene, egui::Key::Escape);
    assert!(!e.stroking);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn repair_options_do_not_change_plateau_or_manual_template_height_settings() {
    let (mut e, ctx, root) = editor();
    let scene = scene();
    e.attach(&scene);
    e.activate_terrain_mode(TerrainMode::CliffHeightRepair);
    let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| e.tool_options(ui, &scene));
    assert_eq!(
        e.template_level, None,
        "repair options must not disable automatic plateau height inference"
    );
    e.template_level = Some(9);
    e.cliff_repair_base = 3;
    let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| e.tool_options(ui, &scene));
    assert_eq!(e.template_level, Some(9));
    assert_eq!(e.cliff_repair_base, 3);
    e.activate_terrain_mode(TerrainMode::HeightFill);
    assert!(e.picking_height);
    e.activate_terrain_mode(TerrainMode::HeightFill);
    assert!(
        e.picking_height,
        "clicking the active tool should not turn off its pending height pick"
    );
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn repair_ghost_draws_individual_subtiles_at_their_saved_height() {
    for multi in [false, true] {
        assert_repair_ghost(multi);
    }
}

fn assert_repair_ghost(multi: bool) {
    let (mut e, ctx, root) = editor();
    let scene = scene();
    e.attach(&scene);
    let brush = scene
        .brushes
        .iter()
        .find(|b| {
            let t = &scene.palette.templates[&(b.key.tile_id, 0)];
            scene.palette.groups.get(&b.key.tile_id).is_some_and(|g| {
                g.category == ra2_map_assets::terrain_library::TerrainCategory::Cliff
            }) && (t.width > 1) == multi
                && t.cells[usize::from(b.key.sub_tile)].is_some_and(|s| s.height == 4)
        })
        .unwrap();
    let at = [36, 36];
    e.doc
        .as_mut()
        .unwrap()
        .set(
            Target::Terrain(at),
            Some(Value::Terrain(crate::document::TerrainValue {
                tile: brush.raw,
                sub: brush.key.sub_tile,
                level: 0,
                ice: 0,
            })),
        )
        .unwrap();
    e.doc.as_mut().unwrap().commit("old subtile");
    e.activate_terrain_mode(TerrainMode::CliffHeightRepair);
    e.pointer(&scene, Some(at), true, true, false, None);
    let camera = crate::camera::MapCamera {
        center: scene.bounds.center(),
        zoom: 0.75,
    };
    let mut expected = egui::Rect::NOTHING;
    let output = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
        let image = &scene.palette.images[&brush.key];
        let (x, y) = vera20k_core::map::terrain::iso_to_screen(at[0], at[1], 4);
        let origin = egui::pos2(
            x + image.info.offset[0] as f32,
            y + image.info.offset[1] as f32,
        );
        expected = egui::Rect::from_min_size(
            ui.max_rect().min + (origin - camera.world_rect(ui.max_rect()).min) * camera.zoom,
            egui::vec2(image.info.width as f32, image.info.height as f32) * camera.zoom,
        );
        e.terrain_preview(ui.painter(), &scene, ui.max_rect(), &camera, Some(at));
    });
    let meshes: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Mesh(m)
                if m.vertices
                    .iter()
                    .all(|v| v.color == Color32::from_white_alpha(180)) =>
            {
                Some(m)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        meshes.len(),
        1,
        "a partial multi-cell cliff still needs an individual subtile ghost"
    );
    assert_eq!(meshes[0].calc_bounds(), expected);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn height_preview_invalidates_pending_edits_and_resource_reloads() {
    let (mut e, ctx, root) = editor();
    let scene = scene();
    e.attach(&scene);
    let brush = scene
        .brushes
        .iter()
        .find(|b| {
            scene.palette.groups.get(&b.key.tile_id).is_some_and(|g| {
                g.category == ra2_map_assets::terrain_library::TerrainCategory::Cliff
            }) && scene.palette.templates[&(b.key.tile_id, 0)].cells[usize::from(b.key.sub_tile)]
                .is_some_and(|s| s.height == 4)
        })
        .unwrap();
    let at = [36, 36];
    e.doc
        .as_mut()
        .unwrap()
        .set(
            Target::Terrain(at),
            Some(Value::Terrain(crate::document::TerrainValue {
                tile: brush.raw,
                sub: brush.key.sub_tile,
                level: 0,
                ice: 0,
            })),
        )
        .unwrap();
    e.doc.as_mut().unwrap().commit("old subtile");
    e.activate_terrain_mode(TerrainMode::CliffHeightRepair);
    e.pointer(&scene, Some(at), true, true, false, None);
    let first = e.plateau_preview_plan(&scene, at).unwrap();
    e.doc
        .as_mut()
        .unwrap()
        .set(Target::Overlay(at), Some(Value::Overlay(1, 0)))
        .unwrap();
    assert!(e.plateau_preview_plan(&scene, at).is_err());
    e.doc.as_mut().unwrap().cancel();
    let restored = e.plateau_preview_plan(&scene, at).unwrap();
    assert!(!Arc::ptr_eq(&first, &restored));
    let alternate = self::scene();
    let reloaded = e.plateau_preview_plan(&alternate, at).unwrap();
    assert!(!Arc::ptr_eq(&restored, &reloaded));
    let old = terrain_ui::subtile_texture(&mut e.ui_state, &ctx, &scene, brush.key).unwrap();
    let new = terrain_ui::subtile_texture(&mut e.ui_state, &ctx, &alternate, brush.key).unwrap();
    assert_ne!(
        old.id(),
        new.id(),
        "a resource reload must not reuse the prior palette texture"
    );
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cliff_height_repair_previews_only_heights_and_keeps_a_hand_assembled_partial_stamp() {
    let (mut e, ctx, root) = editor();
    let scene = scene();
    e.attach(&scene);
    let brush = scene
        .brushes
        .iter()
        .find(|b| {
            scene.palette.groups.get(&b.key.tile_id).is_some_and(|g| {
                g.category == ra2_map_assets::terrain_library::TerrainCategory::Cliff
            }) && scene.palette.templates[&(b.key.tile_id, 0)].cells[usize::from(b.key.sub_tile)]
                .is_some_and(|s| s.height == 4)
        })
        .unwrap();
    let at = [36, 36];
    let doc = e.doc.as_mut().unwrap();
    doc.set(
        Target::Terrain(at),
        Some(Value::Terrain(crate::document::TerrainValue {
            tile: brush.raw,
            sub: brush.key.sub_tile,
            level: 0,
            ice: 7,
        })),
    )
    .unwrap();
    doc.commit("old partial stamp");
    let before = doc.snapshot().bytes().unwrap();
    e.activate_terrain_mode(TerrainMode::CliffHeightRepair);
    e.template_level = Some(0);
    e.pointer(&scene, Some([34, 34]), true, true, false, None);
    e.pointer(&scene, Some([38, 38]), false, true, false, None);
    let plan = e.plateau_preview_plan(&scene, [38, 38]).unwrap();
    assert_eq!(plan.edits.len(), 1);
    assert_eq!(plan.edits[&at].level, 4);
    let _ = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
        e.tool_options(ui, &scene);
        e.terrain_preview(
            ui.painter(),
            &scene,
            ui.max_rect(),
            &crate::camera::MapCamera {
                center: scene.bounds.center(),
                zoom: 0.75,
            },
            Some([38, 38]),
        );
    });
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.pointer(&scene, Some([38, 38]), false, false, true, None);
    assert_eq!(
        e.doc.as_ref().unwrap().terrain(at).unwrap(),
        plan.edits[&at]
    );
    assert_eq!(e.doc.as_ref().unwrap().terrain(at).unwrap().ice, 7);
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.history(true, &scene);
    assert_eq!(e.doc.as_ref().unwrap().terrain(at).unwrap().level, 4);
    e.activate_terrain_mode(TerrainMode::HeightFill);
    e.pointer(&scene, Some(at), true, true, false, None);
    assert_eq!(
        e.flatten_level, 4,
        "partial cliff can be picked after repair"
    );
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn height_fill_picks_cliff_top_previews_then_commits_and_cancels_as_one_edit() {
    let (mut e, ctx, root) = editor();
    let scene = scene();
    e.attach(&scene);
    let doc = e.doc.as_mut().unwrap();
    let cliffs =
        cliff_tools::plan(doc, &scene, cliff_tools::rectangle([30, 30], [42, 42]), 4).unwrap();
    cliffs.apply(doc).unwrap();
    for &at in &cliffs.interior {
        let old = terrain_tools::value(doc, &scene, at);
        doc.set(
            Target::Terrain(at),
            Some(Value::Terrain(crate::document::TerrainValue {
                level: 4,
                ..old
            })),
        )
        .unwrap();
    }
    doc.commit("hollow");
    let before = doc.snapshot().bytes().unwrap();
    e.activate_terrain_mode(TerrainMode::HeightFill);
    assert!(e.picking_height);
    let wall = cliffs.cliffs[0].origin;
    e.pointer(&scene, Some(wall), true, true, false, None);
    e.pointer(&scene, Some(wall), false, false, true, None);
    assert_eq!(e.flatten_level, 8);
    assert!(!e.picking_height);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.pointer(&scene, Some([36, 36]), true, true, false, None);
    let plan = e.plateau_preview_plan(&scene, [36, 36]).unwrap();
    let output = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
        e.terrain_preview(
            ui.painter(),
            &scene,
            ui.max_rect(),
            &crate::camera::MapCamera {
                center: scene.bounds.center(),
                zoom: 0.75,
            },
            Some([36, 36]),
        );
    });
    assert!(!output.shapes.is_empty());
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.flatten_level = 12; // An active gesture keeps its picked height.
    e.pointer(&scene, None, false, false, true, None);
    assert!(!e.stroking);
    for (&at, &v) in &plan.edits {
        assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
    }
    let after = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.history(true, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    e.flatten_level = 9;
    e.pointer(&scene, Some([36, 36]), true, true, false, None);
    assert!(e.cancel_active(&scene));
    e.pointer(&scene, Some([36, 36]), false, false, true, None);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    // The same picker is available for the existing flatten brush.
    e.activate_terrain_mode(TerrainMode::Flatten);
    e.picking_height = true;
    e.pointer(&scene, Some([36, 36]), true, true, false, None);
    e.pointer(&scene, Some([36, 36]), false, false, true, None);
    assert_eq!(e.flatten_level, 8);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn manual_cliff_defaults_to_low_ground_and_keeps_explicit_high_anchors() {
    let (mut e, ctx, root) = editor();
    let scene = scene();
    e.attach(&scene);
    let choices = terrain_ui::choices(
        &scene,
        Some(ra2_map_assets::terrain_library::TerrainCategory::Cliff),
        None,
        false,
        false,
        "",
    );
    let index = *choices
        .iter()
        .find(|&&i| {
            let b = &scene.brushes[i];
            let t = &scene.palette.templates[&(b.key.tile_id, b.key.variant)];
            t.cells
                .first()
                .and_then(|s| *s)
                .is_some_and(|s| s.height == 4)
                && t.cells.iter().flatten().any(|s| s.height == 0)
        })
        .unwrap();
    e.select_terrain_material(index, false);
    let brush = &scene.brushes[e.brush];
    let t = &scene.palette.templates[&(brush.key.tile_id, brush.key.variant)];
    assert_eq!(t.cells[usize::from(brush.key.sub_tile)].unwrap().height, 0);
    e.template_level = None;
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    let at = [35, 35];
    let plan = terrain_tools::template_placement(e.doc.as_ref().unwrap(), &scene, brush, at, None)
        .unwrap();
    assert_eq!(
        plan.iter().find(|(cell, _)| *cell == at).unwrap().1.level,
        4
    );
    e.pointer(&scene, Some(at), true, true, false, None);
    e.pointer(&scene, Some(at), false, false, true, None);
    for (cell, v) in plan {
        assert_eq!(e.doc.as_ref().unwrap().terrain(cell), Some(v));
    }
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    let high = scene
        .brushes
        .iter()
        .position(|b| {
            b.key.tile_id == brush.key.tile_id
                && b.key.variant == brush.key.variant
                && b.key.sub_tile == 0
        })
        .unwrap();
    e.brush = high;
    crate::test_support::run_ui(&ctx, Default::default(), |ui| {
        e.terrain_template_options(ui, &scene)
    });
    assert_eq!(
        e.brush, high,
        "redrawing options must preserve a manually chosen high anchor"
    );
    let next = choices.iter().position(|&i| i == index).unwrap() + 1;
    {
        use egui_kittest::kittest::Queryable as _;
        let mut harness = crate::test_support::harness()
            .with_context_setup(|ctx| {
                ctx.add_font_provider(Arc::new(
                    egui_system_font_provider::SystemFontProvider::new(),
                ))
            })
            .with_accessibility_check(false)
            .build_ui(|ui| e.terrain_template_options(ui, &scene));
        harness.run();
        harness.get_by_label("下一块 ▶").click();
        harness.run();
        drop(harness);
        assert_eq!(e.brush, choices[next]);
        let mut harness = crate::test_support::harness()
            .with_context_setup(|ctx| {
                ctx.add_font_provider(Arc::new(
                    egui_system_font_provider::SystemFontProvider::new(),
                ))
            })
            .with_accessibility_check(false)
            .build_ui(|ui| e.terrain_template_options(ui, &scene));
        harness.run();
        harness.get_by_label("◀ 上一块").click();
        harness.run();
        drop(harness);
        assert_eq!(e.brush, index);
    }
    e.select_terrain_material(high, true);
    assert_eq!(e.brush, high);
    assert_eq!(e.terrain_mode, TerrainMode::Subtile);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn plateau_preview_is_cached_and_matches_release_even_after_tool_and_height_change() {
    let (mut e, ctx, root) = editor();
    let scene = scene();
    e.attach(&scene);
    e.tool = Tool::Terrain;
    e.terrain_mode = TerrainMode::Plateau;
    e.template_level = Some(4);
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.pointer(&scene, Some([30, 30]), true, true, false, None);
    e.pointer(&scene, Some([38, 38]), false, true, false, None);
    let plan = e
        .plateau
        .generate(
            e.doc.as_ref().unwrap(),
            &scene,
            [38, 38],
            CutOptions::default(),
        )
        .unwrap();
    let same = e
        .plateau
        .generate(
            e.doc.as_ref().unwrap(),
            &scene,
            [38, 38],
            CutOptions::default(),
        )
        .unwrap();
    assert!(Arc::ptr_eq(&plan, &same));
    let output = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
        e.terrain_preview(
            ui.painter(),
            &scene,
            ui.max_rect(),
            &crate::camera::MapCamera {
                center: scene.bounds.center(),
                zoom: 0.75,
            },
            Some([38, 38]),
        );
    });
    assert!(output.shapes.iter().any(|s| matches!(&s.shape,egui::Shape::Mesh(m) if m.vertices.iter().all(|v|v.color==Color32::from_white_alpha(180)))));
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.template_level = Some(0);
    e.tool = Tool::Erase;
    e.pointer(&scene, None, false, false, true, None);
    for (&at, &v) in &plan.edits {
        assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
    }
    let after = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.history(true, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn outline_release_includes_final_point_and_escape_cancels_without_history() {
    let (mut e, ctx, root) = editor();
    let scene = scene();
    e.attach(&scene);
    e.tool = Tool::Terrain;
    e.terrain_mode = TerrainMode::CliffOutline;
    e.pointer(&scene, Some([28, 28]), true, true, false, None);
    for at in [[46, 28], [46, 34], [35, 34], [35, 46]] {
        e.pointer(&scene, Some(at), false, true, false, None);
    }
    e.pointer(&scene, Some([28, 46]), false, false, true, None);
    assert!(!e.status.starts_with("本笔未修改"), "{}", e.status);
    assert_eq!(e.doc.as_ref().unwrap().terrain([30, 40]).unwrap().level, 8);
    e.history(false, &scene);
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.terrain_mode = TerrainMode::Plateau;
    e.pointer(&scene, Some([30, 30]), true, true, false, None);
    e.pointer(&scene, Some([38, 38]), false, true, false, None);
    let input = egui::RawInput {
        events: vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
        ..Default::default()
    };
    let _ = crate::test_support::run_ui(&ctx, input, |ui| e.shortcuts(ui.ctx(), &scene));
    assert!(!e.stroking);
    assert!(e.plateau.gesture.is_none());
    e.pointer(&scene, Some([38, 38]), false, false, true, None);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn bridge_gesture_freezes_type_previews_and_commits_all_rows_once() {
    let (mut e, ctx, root) = editor();
    let scene = scene();
    e.attach(&scene);
    e.tool = Tool::Terrain;
    e.terrain_mode = TerrainMode::Bridge;
    e.bridge_kind = ra2_map_scene::bridge_tools::BridgeKind::LowWood;
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.pointer(&scene, Some([30, 30]), true, true, false, None);
    let p = e
        .plateau
        .generate(
            e.doc.as_ref().unwrap(),
            &scene,
            [42, 30],
            CutOptions::default(),
        )
        .unwrap();
    assert_eq!(p.overlays.len(), 39);
    let same = e
        .plateau
        .generate(
            e.doc.as_ref().unwrap(),
            &scene,
            [42, 30],
            CutOptions::default(),
        )
        .unwrap();
    assert!(Arc::ptr_eq(&p, &same));
    let output = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
        e.terrain_preview(
            ui.painter(),
            &scene,
            ui.max_rect(),
            &crate::camera::MapCamera {
                center: scene.bounds.center(),
                zoom: 0.3,
            },
            Some([42, 30]),
        )
    });
    assert!(!output.shapes.is_empty());
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.bridge_kind = ra2_map_scene::bridge_tools::BridgeKind::HighConcrete;
    e.pointer(&scene, Some([42, 30]), false, false, true, None);
    for (&at, &v) in &p.overlays {
        assert_eq!(e.doc.as_ref().unwrap().overlay(at), v);
    }
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn joining_gestures_cache_previews_freeze_the_mode_and_cancel_invalid_strokes() {
    let f = ra2_map_assets::test_support::Fixture::with_joining();
    let mut install = f.install();
    install.catalog = ra2_map_assets::catalog::Catalog::load(&install.manager, &install.theater);
    let scene = crate::map::load_scene_bytes(
        &mut install,
        "joining.map",
        "test".into(),
        crate::new_map::NewMapOptions {
            width: 40,
            height: 40,
            level: 4,
            ..Default::default()
        }
        .bytes()
        .unwrap(),
        Instant::now(),
    )
    .unwrap();
    let (mut e, ctx, root) = editor();
    e.attach(&scene);
    e.tool = Tool::Terrain;
    for (mode, path) in [
        (TerrainMode::Water, vec![[26, 26], [49, 49]]),
        (
            TerrainMode::WaterOutline,
            vec![[28, 28], [42, 28], [42, 42], [28, 42]],
        ),
        (
            TerrainMode::WaterOutline,
            vec![[28, 38], [38, 28], [48, 38], [38, 48]],
        ),
        (TerrainMode::Road, vec![[28, 28], [42, 28]]),
    ] {
        e.tool = Tool::Terrain;
        e.terrain_mode = mode;
        let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        for (i, &at) in path.iter().enumerate() {
            e.pointer(&scene, Some(at), i == 0, true, false, None);
        }
        let at = *path.last().unwrap();
        let p = e.plateau_preview_plan(&scene, at).unwrap();
        let same = e.plateau_preview_plan(&scene, at).unwrap();
        assert!(Arc::ptr_eq(&p, &same));
        let rendered = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
            e.terrain_preview(
                ui.painter(),
                &scene,
                ui.max_rect(),
                &crate::camera::MapCamera {
                    center: scene.bounds.center(),
                    zoom: 0.3,
                },
                Some(at),
            )
        });
        assert!(!rendered.shapes.is_empty());
        assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
        e.terrain_mode = TerrainMode::LandFill;
        e.tool = Tool::Erase;
        e.pointer(&scene, None, false, false, true, None);
        assert!(!e.status.starts_with("本笔未修改"), "{}", e.status);
        for (&at, &v) in &p.edits {
            assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
        }
        if mode == TerrainMode::Water {
            let lake = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
            e.activate_terrain_mode(TerrainMode::CliffFront);
            let start = [32, 36];
            let end = [43, 36];
            assert!(ra2_map_scene::shore_tools::wet(
                &scene,
                e.doc.as_ref().unwrap().terrain(start).unwrap()
            ));
            e.pointer(&scene, Some(start), true, true, false, None);
            e.pointer(&scene, Some(end), false, true, false, None);
            let cliffs = e.plateau_preview_plan(&scene, end).unwrap();
            assert!(!cliffs.cliffs.is_empty());
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), lake);
            e.pointer(&scene, Some(end), false, false, true, None);
            for (&at, &v) in &cliffs.edits {
                assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
            }
            e.history(false, &scene);
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), lake);

            e.activate_terrain_mode(TerrainMode::Plateau);
            e.pointer(&scene, Some([32, 32]), true, true, false, None);
            e.pointer(&scene, Some([43, 42]), false, false, true, None);
            let island = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
            assert_ne!(island, lake);

            e.activate_terrain_mode(TerrainMode::RampEntrance);
            let (start, end) = ([36, 42], [38, 42]);
            e.pointer(&scene, Some(start), true, true, false, None);
            e.pointer(&scene, Some(end), false, true, false, None);
            let entrance = e.plateau_preview_plan(&scene, end).unwrap();
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), island);
            e.pointer(&scene, Some(end), false, false, true, None);
            for (&at, &v) in &entrance.edits {
                assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
            }
            e.history(false, &scene);
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), island);
            e.history(false, &scene);
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), lake);
        }
        e.history(false, &scene);
        assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    }
    e.tool = Tool::Terrain;
    e.terrain_mode = TerrainMode::Water;
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.pointer(&scene, Some([28, 28]), true, true, false, None);
    assert!(e.plateau_preview_plan(&scene, [28, 28]).is_err());
    e.pointer(&scene, None, false, false, true, None);
    assert!(e.status.starts_with("本笔未修改"));
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    assert!(!e.stroking);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn water_outline_repaints_ice_and_preserves_the_rest_of_the_lake() {
    use crate::document::{Target, TerrainValue, Value};
    use ra2_map_scene::shore_tools;

    let fixture = ra2_map_assets::test_support::Fixture::with_joining();
    let mut scene = scene_from_fixture(&fixture, 4);
    let palette = Arc::get_mut(&mut scene.palette).unwrap();
    palette.bundled_ice_sets = Some([4096, 4160, 4224]);
    let tiles = [4096, 4104, 4114, 4176, 4180, 4287];
    palette
        .raw_ids
        .extend(tiles.map(|tile| (i32::from(tile), tile)));
    let art = palette.images.values().next().unwrap().clone();
    for tile_id in tiles {
        palette.images.insert(
            vera20k_core::map::theater::TileKey {
                tile_id,
                sub_tile: 0,
                variant: 0,
            },
            art.clone(),
        );
    }
    Arc::make_mut(&mut scene.catalog)
        .raw_tiles
        .extend(tiles.map(|tile| (tile, i32::from(tile))));

    let (mut e, _, root) = editor();
    e.attach(&scene);
    let doc = e.doc.as_mut().unwrap();
    let lake = ra2_map_scene::cliff_tools::rectangle([26, 26], [49, 49]);
    shore_tools::plan(doc, &scene, lake, Some(true))
        .unwrap()
        .apply(doc)
        .unwrap();
    let water = doc.terrain([32, 32]).unwrap();
    let selected = [[32, 33], [33, 33], [34, 33], [35, 33], [34, 34]];
    let kept = [36, 33];
    for (at, tile) in selected
        .into_iter()
        .zip([4096, 4176, 4180, 4104, 4287])
        .chain([(kept, 4104)])
    {
        doc.set(
            Target::Terrain(at),
            Some(Value::Terrain(TerrainValue {
                tile,
                ice: 1,
                ..water
            })),
        )
        .unwrap();
    }
    doc.commit("frozen lake");
    let kept_ice = doc.terrain(kept).unwrap();
    let before = doc.snapshot().bytes().unwrap();

    e.activate_terrain_mode(TerrainMode::WaterOutline);
    let path = [[31, 31], [35, 31], [35, 35], [31, 35]];
    for (i, &at) in path.iter().enumerate() {
        e.pointer(&scene, Some(at), i == 0, true, false, None);
    }
    let preview = e.plateau_preview_plan(&scene, path[3]).unwrap();
    assert!(preview.cliffs.is_empty());
    assert_eq!(preview.edits.len(), selected.len() + 1);
    let new_edge = TerrainValue {
        tile: 4114,
        ..water
    };
    let reshaped = TerrainValue {
        tile: 4096,
        ..kept_ice
    };
    assert_eq!(preview.edits[&[35, 33]], new_edge);
    assert_eq!(preview.edits[&kept], reshaped);
    assert!(
        selected
            .into_iter()
            .filter(|&at| at != [35, 33])
            .all(|at| preview.edits[&at] == water)
    );
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.pointer(&scene, Some(path[3]), false, false, true, None);
    let doc = e.doc.as_ref().unwrap();
    for (&at, &after) in &preview.edits {
        assert_eq!(doc.terrain(at), Some(after));
    }
    let after = doc.snapshot().bytes().unwrap();
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.history(true, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ramp_gesture_widens_and_shrinks_preview_then_commits_once() {
    let (mut e, _, root) = editor();
    let scene = scene();
    e.attach(&scene);
    cliff_tools::plan(
        e.doc.as_ref().unwrap(),
        &scene,
        cliff_tools::rectangle([25, 25], [45, 45]),
        4,
    )
    .unwrap()
    .apply(e.doc.as_mut().unwrap())
    .unwrap();
    e.commit(&scene, "plateau");
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.tool = Tool::Terrain;
    e.terrain_mode = TerrainMode::RampEntrance;
    for (start, axis) in [([35, 45], 0), ([45, 35], 1), ([35, 25], 0), ([25, 35], 1)] {
        for sign in [-1, 1] {
            e.pointer(&scene, Some(start), true, true, false, None);
            let narrow = e.plateau_preview_plan(&scene, start).unwrap();
            let mut end = start;
            end[axis] = (i32::from(end[axis]) + sign * 4) as u16;
            e.pointer(&scene, Some(end), false, true, false, None);
            let wide = e.plateau_preview_plan(&scene, end).unwrap();
            assert_eq!(wide.edits.len(), narrow.edits.len() + 16);
            assert!(Arc::ptr_eq(
                &wide,
                &e.plateau_preview_plan(&scene, end).unwrap()
            ));
            end[axis] = (i32::from(start[axis]) + sign * 2) as u16;
            e.pointer(&scene, Some(end), false, true, false, None);
            let shrunk = e.plateau_preview_plan(&scene, end).unwrap();
            assert_eq!(shrunk.edits.len(), narrow.edits.len() + 8);
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
            e.pointer(&scene, None, false, false, true, None);
            assert!(!e.status.starts_with("本笔未修改"), "{}", e.status);
            for (&at, &v) in &shrunk.edits {
                assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
            }
            let after = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
            e.history(false, &scene);
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
            e.history(true, &scene);
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
            e.history(false, &scene);
        }
    }
    e.pointer(&scene, Some([35, 45]), true, true, false, None);
    e.pointer(&scene, Some([55, 45]), false, false, true, None);
    assert!(e.status.starts_with("本笔未修改"), "{}", e.status);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn corner_ramp_gesture_converts_the_corner_and_release_matches_preview() {
    let (mut e, _, root) = editor();
    let scene = scene();
    e.attach(&scene);
    cliff_tools::plan(
        e.doc.as_ref().unwrap(),
        &scene,
        cliff_tools::rectangle([25, 25], [45, 45]),
        4,
    )
    .unwrap()
    .apply(e.doc.as_mut().unwrap())
    .unwrap();
    e.commit(&scene, "plateau");
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.tool = Tool::Terrain;
    e.terrain_mode = TerrainMode::RampEntrance;
    for (corner, signs) in [
        ([25, 25], [1, 1]),
        ([45, 25], [-1, 1]),
        ([25, 45], [1, -1]),
        ([45, 45], [-1, -1]),
    ] {
        for axis in 0..2 {
            e.pointer(&scene, Some(corner), true, true, false, None);
            assert!(e.plateau_preview_plan(&scene, corner).is_ok());
            let mut end = corner;
            end[axis] = (i32::from(end[axis]) + signs[axis] * 4) as u16;
            e.pointer(&scene, Some(end), false, true, false, None);
            let p = e.plateau_preview_plan(&scene, end).unwrap();
            let id = scene.palette.raw_ids[&p.edits[&corner].tile];
            let cell = scene.palette.templates[&(id, 0)].cells[0].unwrap();
            assert!(
                (5..=8).contains(&cell.slope),
                "the clicked corner itself must slope"
            );
            assert_eq!(p.edits[&corner].level, 7);
            assert!(Arc::ptr_eq(
                &p,
                &e.plateau_preview_plan(&scene, end).unwrap()
            ));
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
            e.pointer(&scene, None, false, false, true, None);
            assert!(!e.status.starts_with("本笔未修改"), "{}", e.status);
            for (&at, &v) in &p.edits {
                assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
            }
            let after = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
            e.history(false, &scene);
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
            e.history(true, &scene);
            assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
            e.history(false, &scene);
        }
    }
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn joining_an_existing_corner_and_straight_ramp_previews_and_commits_as_one_edit() {
    let (mut e, _, root) = editor();
    let scene = scene();
    e.attach(&scene);
    cliff_tools::plan(
        e.doc.as_ref().unwrap(),
        &scene,
        cliff_tools::rectangle([25, 25], [45, 45]),
        4,
    )
    .unwrap()
    .apply(e.doc.as_mut().unwrap())
    .unwrap();
    e.commit(&scene, "plateau");
    e.tool = Tool::Terrain;
    e.terrain_mode = TerrainMode::RampEntrance;
    for corner_first in [true, false] {
        let (a, b, c, d) = if corner_first {
            ([45, 45], [45, 45], [35, 45], [41, 45])
        } else {
            ([35, 45], [41, 45], [45, 45], [45, 45])
        };
        e.pointer(&scene, Some(a), true, true, false, None);
        e.pointer(&scene, Some(b), false, false, true, None);
        assert!(!e.status.starts_with("本笔未修改"), "{}", e.status);
        let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        e.pointer(&scene, Some(c), true, true, false, None);
        e.pointer(&scene, Some(d), false, true, false, None);
        let p = e.plateau_preview_plan(&scene, d).unwrap();
        assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
        e.pointer(&scene, None, false, false, true, None);
        assert!(!e.status.starts_with("本笔未修改"), "{}", e.status);
        for (&at, &v) in &p.edits {
            assert_eq!(e.doc.as_ref().unwrap().terrain(at), Some(v));
        }
        let after = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
        e.history(false, &scene);
        assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
        e.history(true, &scene);
        assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
        e.history(false, &scene);
        e.history(false, &scene);
    }
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn tall_plateau_gesture_automatically_infers_base_above_fourteen_and_undoes_once() {
    let (mut e, _, root) = editor();
    let scene = scene();
    e.attach(&scene);
    for cell in &scene.cells {
        let at = [cell.rx, cell.ry];
        let mut v = terrain_tools::value(e.doc.as_ref().unwrap(), &scene, at);
        v.level = 24;
        e.doc
            .as_mut()
            .unwrap()
            .set(Target::Terrain(at), Some(Value::Terrain(v)))
            .unwrap();
    }
    e.commit(&scene, "high ground");
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.tool = Tool::Terrain;
    e.terrain_mode = TerrainMode::Plateau;
    e.pointer(&scene, Some([30, 30]), true, true, false, None);
    e.pointer(&scene, Some([42, 42]), false, true, false, None);
    let p = e.plateau_preview_plan(&scene, [42, 42]).unwrap();
    assert_eq!(p.base, 24);
    assert_eq!(p.edits[&[35, 35]].level, 28);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.pointer(&scene, None, false, false, true, None);
    assert_eq!(e.doc.as_ref().unwrap().terrain([35, 35]).unwrap().level, 28);
    let after = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.history(true, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn plateau_cut_defaults_to_rectangle_and_can_remove_the_whole_plateau() {
    let (mut e, _, root) = editor();
    let scene = scene();
    e.attach(&scene);
    let doc = e.doc.as_mut().unwrap();
    cliff_authoring::reshape(
        doc,
        &scene,
        cliff_tools::rectangle([26, 26], [50, 50]),
        4,
        false,
    )
    .unwrap()
    .apply(doc)
    .unwrap();
    e.commit(&scene, TerrainMode::Plateau.name());
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.activate_terrain_mode(TerrainMode::PlateauCut);
    assert_eq!(e.plateau_cut_method, PlateauCutMethod::Rectangle);

    let (start, end) = ([24, 24], [52, 52]);
    e.pointer(&scene, Some(start), true, true, false, None);
    e.pointer(&scene, Some(end), false, true, false, None);
    let plan = e.plateau_preview_plan(&scene, end).unwrap();
    assert_eq!(plan.edits[&[38, 38]].level, 4);
    assert!(plan.cliffs.is_empty());
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);

    e.pointer(&scene, Some(end), false, false, true, None);
    let doc = e.doc.as_ref().unwrap();
    assert!(!e.stroking, "{}", e.status);
    assert_eq!(doc.undo_name(), Some(TerrainMode::PlateauCut.name()));
    for (&at, &v) in &plan.edits {
        assert_eq!(doc.terrain(at), Some(v));
    }
    let after = doc.snapshot().bytes().unwrap();
    e.history(false, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.history(true, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn plateau_cut_two_click_draft_updates_settings_and_is_one_history_entry() {
    use egui_kittest::kittest::Queryable as _;
    let (mut e, ctx, root) = editor();
    let scene = scene();
    e.attach(&scene);
    let doc = e.doc.as_mut().unwrap();
    cliff_authoring::reshape(
        doc,
        &scene,
        cliff_tools::rectangle([26, 26], [50, 50]),
        4,
        false,
    )
    .unwrap()
    .apply(doc)
    .unwrap();
    e.commit(&scene, TerrainMode::Plateau.name());
    let before = e.doc.as_ref().unwrap().snapshot().bytes().unwrap();
    e.activate_terrain_mode(TerrainMode::PlateauCut);
    let mut harness = egui_kittest::Harness::<Editor>::builder()
        .with_context_setup(|ctx| {
            ctx.add_font_provider(Arc::new(
                egui_system_font_provider::SystemFontProvider::new(),
            ));
        })
        .with_accessibility_check(false)
        .with_size([420., 600.])
        .build_ui_state(|ui, e| e.plateau_options(ui, &scene), e);
    harness.run();
    assert!(harness.query_by_label("指定低地基准高度").is_some());
    assert!(harness.query_by_label("削切方向").is_none());
    harness.get_by_label("两点切线").click();
    harness.run();
    assert!(harness.query_by_label("指定低地基准高度").is_none());
    assert!(harness.query_by_label("削切方向").is_some());
    harness.get_by_label("矩形拖选").click();
    harness.run();
    assert!(harness.query_by_label("指定低地基准高度").is_some());
    harness.get_by_label("两点切线").click();
    harness.run();
    assert_eq!(
        harness.state().plateau_cut_method,
        PlateauCutMethod::TwoPoint
    );
    let e = harness.state_mut();
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);

    let start = [38, 20];
    let short = [38, 22];
    let end = [38, 49];
    assert_eq!(
        terrain_tools::value(e.doc.as_ref().unwrap(), &scene, start).level,
        4
    );
    e.pointer(&scene, Some(start), true, true, false, None);
    let camera = crate::camera::MapCamera {
        center: scene.bounds.center(),
        zoom: 0.75,
    };
    let first_frame = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
        e.terrain_preview(ui.painter(), &scene, ui.max_rect(), &camera, Some(start));
    });
    assert!(first_frame.shapes.iter().any(|s| matches!(&s.shape,
        egui::Shape::Text(t) if t.galley.text().contains("点击第二点提交")
    )));
    assert!(
        e.plateau.preview.is_none(),
        "a stationary first point needs no plan"
    );
    e.pointer(&scene, Some(end), false, true, false, None);
    e.pointer(&scene, Some(end), false, false, true, None);
    assert!(e.stroking, "drag and release don't place the second point");
    let assert_cut_guide = |e: &mut Editor, side| {
        let frame = crate::test_support::run_ui(&ctx, Default::default(), |ui| {
            e.terrain_preview(ui.painter(), &scene, ui.max_rect(), &camera, Some(end));
        });
        let segments: Vec<_> = frame
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::LineSegment { points, stroke }
                    if stroke.color == Color32::from_rgb(80, 220, 170) =>
                {
                    Some(*points)
                }
                _ => None,
            })
            .collect();
        let [line, ticks @ ..] = &segments[..] else {
            panic!("missing cut line");
        };
        assert_eq!(ticks.len(), 3);
        let along = (line[1] - line[0]).normalized();
        let right = egui::vec2(-along.y, along.x);
        let sign = if side == CutSide::Left { -1. } else { 1. };
        for tick in ticks {
            let delta = tick[1] - tick[0];
            assert!((delta.dot(right) - sign * 6.).abs() < 0.01);
            assert!(delta.dot(along).abs() < 0.01);
        }
    };
    assert_cut_guide(e, CutSide::Left);
    let left = e.plateau_preview_plan(&scene, end).unwrap();
    let same_ray = e.plateau_preview_plan(&scene, [38, 45]).unwrap();
    assert!(Arc::ptr_eq(&left, &same_ray));
    assert!(e.plateau_preview_plan(&scene, short).is_err());
    e.pointer(&scene, Some(short), true, false, true, None);
    assert!(
        e.stroking,
        "a line stopping short of the cliff keeps the anchor active"
    );
    assert!(e.status.contains("切线尚未穿过悬崖"));
    assert!(Arc::ptr_eq(
        &left,
        &e.plateau_preview_plan(&scene, end).unwrap()
    ));
    assert_eq!(left.edits[&[45, 40]].level, 4);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);

    e.pointer(&scene, Some(start), true, false, true, None);
    assert!(e.stroking, "a duplicate endpoint keeps the anchor active");
    assert!(e.status.contains("切线需要两个不同的点"));
    e.plateau_preview_plan(&scene, end).unwrap();
    harness.run();
    harness.get_by_label("矩形拖选").click();
    harness.run();
    harness.get_by_label("右侧").click();
    harness.run();
    harness.get_by_label("指定削切高度").click();
    harness.run();
    assert_eq!(harness.state().cliff_cut.target, Some(4));
    harness
        .get_by_role_and_label(egui::accesskit::Role::Slider, "目标高度")
        .set_value_accesskit(6.);
    harness.run();
    let mut e = harness.into_state();
    assert_eq!(e.plateau_cut_method, PlateauCutMethod::TwoPoint);
    assert_eq!(e.cliff_cut.side, CutSide::Right);
    assert_eq!(e.cliff_cut.target, Some(6));
    assert_cut_guide(&mut e, CutSide::Right);
    let right = e.plateau_preview_plan(&scene, end).unwrap();
    assert_eq!(right.edits[&[32, 40]].level, 6);
    assert!(!right.edits.contains_key(&[45, 40]));
    assert!(Arc::ptr_eq(
        &right,
        &e.plateau_preview_plan(&scene, end).unwrap()
    ));
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);

    let mut draw_order = PreviewDrawOrder::default();
    let cached_order = draw_order.edits(&right);
    assert_eq!(cached_order.len(), right.edits.len());
    assert!(cached_order.windows(2).all(|pair| {
        let key = |[x, y]: Cell| (u32::from(x) + u32::from(y), x);
        key(pair[0].0) < key(pair[1].0)
    }));
    assert!(
        draw_order
            .edits(&left)
            .iter()
            .any(|&(at, _)| at == [45, 40])
    );

    let mut alternate = scene.clone();
    alternate.palette = self::scene().palette;
    let error = e.plateau_preview_plan(&alternate, end).err().unwrap();
    assert!(error.to_string().contains("地图或地形资源已变化"));
    e.pointer(&alternate, Some(end), true, false, true, None);
    assert!(e.stroking);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);

    polyline_key(&mut e, &ctx, &scene, egui::Key::Escape);
    assert!(!e.stroking);
    assert!(e.plateau.gesture.is_none());
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.pointer(&scene, Some(start), true, false, true, None);
    e.pointer(&scene, Some(end), true, false, true, None);
    let doc = e.doc.as_ref().unwrap();
    assert!(!e.stroking);
    assert_eq!(doc.undo_name(), Some(TerrainMode::PlateauCut.name()));
    for (&at, &v) in &right.edits {
        assert_eq!(doc.terrain(at), Some(v));
    }
    let after = doc.snapshot().bytes().unwrap();
    let pending = e
        .cut_terrain_preview(&scene, None)
        .expect("retain committed cut while the scene updates");
    assert_eq!(pending.edits, right.edits);
    e.history(false, &scene);
    assert!(
        e.cut_terrain_preview(&scene, None).is_none(),
        "undo clears the retained cut"
    );
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), before);
    e.pointer(&scene, Some(start), true, false, true, None);
    e.pointer(&scene, Some(end), true, false, true, None);
    assert!(e.cut_terrain_preview(&scene, None).is_some());
    let mut published = scene.clone();
    published.visual = Arc::new((*scene.visual).clone());
    assert!(
        e.cut_terrain_preview(&published, None).is_none(),
        "the published scene takes over rendering"
    );
    e.history(false, &scene);
    e.history(true, &scene);
    assert_eq!(e.doc.as_ref().unwrap().snapshot().bytes().unwrap(), after);
    drop(e);
    std::fs::remove_dir_all(root).unwrap();
}
