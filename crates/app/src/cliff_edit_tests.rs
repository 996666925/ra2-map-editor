use super::super::tests::editor;
use super::*;

fn scene() -> MapScene {
    scene_at_level(4)
}

fn scene_at_level(level: u8) -> MapScene {
    let f = ra2_map_assets::test_support::Fixture::with_authoring();
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
        .generate(e.doc.as_ref().unwrap(), &scene, [38, 38])
        .unwrap();
    let same = e
        .plateau
        .generate(e.doc.as_ref().unwrap(), &scene, [38, 38])
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
        .generate(e.doc.as_ref().unwrap(), &scene, [42, 30])
        .unwrap();
    assert_eq!(p.overlays.len(), 39);
    let same = e
        .plateau
        .generate(e.doc.as_ref().unwrap(), &scene, [42, 30])
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
        (TerrainMode::Water, vec![[28, 28], [38, 38]]),
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
